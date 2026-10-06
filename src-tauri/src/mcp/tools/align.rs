//! add_alignment MCP tool + alignment JSON helpers.

use rmcp::{ErrorData, handler::server::wrapper::Json};
use tauri::Runtime;

use libregene_core::digest::cut_flanks;
use libregene_core::models::{Alignment, Enzyme};
use std::collections::{HashMap, HashSet};

use crate::mcp::LibreGeneMcp;
use crate::mcp::support::{MAX_FLANK, fail_envelope, insert_seq_hashes, ok_envelope, push_note};
use crate::mcp::types::AddAlignmentRequest;

/// Per-alignment JSON for add_alignment responses, with every template
/// coordinate converted to 1-based inclusive. An insertion at internal
/// 0-based `pos` (extra read bases before base `pos`) is reported as the
/// 1-based base BEFORE the break — the bases sit between `pos` and `pos + 1`
/// (`pos = len` on circular templates means between the last and the first
/// base).
fn alignment_json_1based(
    a: &libregene_core::models::Alignment,
    template: &str,
    tlen: i64,
    circular: bool,
    compact: bool,
) -> serde_json::Value {
    let diff = libregene_core::align::alignment_diff(a, template);
    let mut v = serde_json::json!({
        "alignmentId": a.id,
        "name": a.name,
        "identity": a.identity,
        "strand": a.strand,
        "segmentCount": a.segments.len(),
        "alignedLength": diff.aligned_length,
        "readLength": a.seq.len(),
        "mismatches": diff.mismatches.len(),
        "insertions": diff.insertions.iter().map(|i| i.length).sum::<usize>(),
        "deletions": diff.deletions.iter().map(|d| d.length).sum::<usize>(),
        "mismatchDetails": diff.mismatches.iter().map(|m| serde_json::json!({
            "position": m.pos + 1,
            "templateBase": m.template_base,
            "readBase": m.read_base,
        })).collect::<Vec<_>>(),
        "deletionDetails": diff.deletions.iter().map(|d| serde_json::json!({
            "position": d.pos + 1,
            "length": d.length,
            "bases": d.bases,
        })).collect::<Vec<_>>(),
        "insertionDetails": diff.insertions.iter().map(|i| serde_json::json!({
            "position": cut_flanks(i.pos as i64, tlen, circular).0,
            "bases": i.bases,
            "length": i.length,
        })).collect::<Vec<_>>(),
        "coverage": a.segments.iter().map(|s| serde_json::json!({
            "start": s.start + 1,
            "end": s.end + 1,
        })).collect::<Vec<_>>(),
    });
    if !compact {
        v["orientedSequence"] = serde_json::json!(a.seq);
    }
    v
}

/// Stats-only per-alignment JSON (no orientedSequence, no mismatch/deletion/
/// insertion details) — used for every alignment EXCEPT the one just added,
/// so multi-read responses stay small. Counts only: the diff detail vectors
/// are never serialized.
fn alignment_stats_json_1based(
    a: &libregene_core::models::Alignment,
    template: &str,
    _tlen: i64,
    _circular: bool,
) -> serde_json::Value {
    let diff = libregene_core::align::alignment_diff(a, template);
    serde_json::json!({
        "alignmentId": a.id,
        "name": a.name,
        "identity": a.identity,
        "strand": a.strand,
        "segmentCount": a.segments.len(),
        "alignedLength": diff.aligned_length,
        "readLength": a.seq.len(),
        "mismatches": diff.mismatches.len(),
        "insertions": diff.insertions.iter().map(|i| i.length).sum::<usize>(),
        "deletions": diff.deletions.iter().map(|d| d.length).sum::<usize>(),
        "coverage": a.segments.iter().map(|s| serde_json::json!({
            "start": s.start + 1,
            "end": s.end + 1,
        })).collect::<Vec<_>>(),
    })
}

/// 1-based inclusive window membership, wrap-aware (s > e on circular
/// templates means the window crosses the origin).
fn in_window_1based(p: i64, s: i64, e: i64) -> bool {
    if s <= e {
        p >= s && p <= e
    } else {
        p >= s || p <= e
    }
}

/// Filter an alignment JSON's diff-detail arrays to entries overlapping the
/// 1-based inclusive focus window (wrap-aware). Insertions sit BETWEEN
/// template bases `position` and `position + 1`, so they are kept when either
/// flanking base is inside the window; a `position + 1` past the last base
/// wraps to 1 on circular templates. Returns the in-window base counts
/// `(mismatches, insertions, deletions)` so the caller can report them next to
/// the whole-read totals.
pub(crate) fn filter_alignment_json_focus(
    v: &mut serde_json::Value,
    s1: i64,
    e1: i64,
    tlen: i64,
    circular: bool,
) -> (i64, i64, i64) {
    let obj = match v.as_object_mut() {
        Some(o) => o,
        None => return (0, 0, 0),
    };
    if let Some(arr) = obj.get_mut("mismatchDetails").and_then(|a| a.as_array_mut()) {
        arr.retain(|m| {
            m.get("position")
                .and_then(|p| p.as_i64())
                .is_some_and(|p| in_window_1based(p, s1, e1))
        });
    }
    // A deletion merged across the circular origin (terminal run + origin
    // run in alignment_diff) can carry position + length past tlen; map those
    // coordinates back into 1..=tlen before comparing with the window.
    let wrap_x = |x: i64| if circular && x > tlen { x - tlen } else { x };
    if let Some(arr) = obj.get_mut("deletionDetails").and_then(|a| a.as_array_mut()) {
        arr.retain(|d| {
            match (
                d.get("position").and_then(|p| p.as_i64()),
                d.get("length").and_then(|l| l.as_i64()),
            ) {
                (Some(p), Some(l)) => (p..p + l.max(1)).any(|x| in_window_1based(wrap_x(x), s1, e1)),
                _ => false,
            }
        });
    }
    if let Some(arr) = obj.get_mut("insertionDetails").and_then(|a| a.as_array_mut()) {
        arr.retain(|i| {
            i.get("position").and_then(|p| p.as_i64()).is_some_and(|p| {
                let next = if circular && p == tlen { 1 } else { p + 1 };
                in_window_1based(p, s1, e1) || in_window_1based(next, s1, e1)
            })
        });
    }
    // In-window base counts: mismatch entries are one column each, insertion
    // entries count fully by their anchor, and deletion entries count only
    // the bases actually inside the window (a partially overlapping deletion
    // is kept but its outside bases are not window content).
    let in_mismatches = obj
        .get("mismatchDetails")
        .and_then(|a| a.as_array())
        .map(|a| a.len() as i64)
        .unwrap_or(0);
    let in_deletions: i64 = obj
        .get("deletionDetails")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .map(|d| {
                    match (
                        d.get("position").and_then(|p| p.as_i64()),
                        d.get("length").and_then(|l| l.as_i64()),
                    ) {
                        (Some(p), Some(l)) => (p..p + l.max(1))
                            .filter(|&x| in_window_1based(wrap_x(x), s1, e1))
                            .count() as i64,
                        _ => 0,
                    }
                })
                .sum()
        })
        .unwrap_or(0);
    let in_insertions: i64 = obj
        .get("insertionDetails")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|d| d.get("length").and_then(|l| l.as_i64()))
                .sum()
        })
        .unwrap_or(0);
    (in_mismatches, in_insertions, in_deletions)
}

/// Total template columns not covered by any segment, summed over the gaps
/// between consecutive segments. 0 for single-segment reads and for
/// origin-spanning circular reads whose segments are adjacent at the wrap.
pub(crate) fn uncovered_between_segments(a: &libregene_core::models::Alignment, tlen: usize, circular: bool) -> usize {
    let mut total = 0usize;
    for i in 0..a.segments.len().saturating_sub(1) {
        let end = a.segments[i].end as i64;
        let start = a.segments[i + 1].start as i64;
        let gap = if circular {
            (start - end - 1).rem_euclid(tlen as i64)
        } else {
            start - end - 1
        };
        if gap > 0 {
            total += gap as usize;
        }
    }
    total
}

/// IUPAC single-code vs concrete base match (`N` matches anything).
fn iupac_code_matches(code: u8, base: u8) -> bool {
    let code = code.to_ascii_uppercase();
    let base = base.to_ascii_uppercase();
    match code {
        b'A' => base == b'A',
        b'C' => base == b'C',
        b'G' => base == b'G',
        b'T' | b'U' => base == b'T',
        b'R' => matches!(base, b'A' | b'G'),
        b'Y' => matches!(base, b'C' | b'T'),
        b'W' => matches!(base, b'A' | b'T'),
        b'S' => matches!(base, b'C' | b'G'),
        b'K' => matches!(base, b'G' | b'T'),
        b'M' => matches!(base, b'A' | b'C'),
        b'B' => matches!(base, b'C' | b'G' | b'T'),
        b'D' => matches!(base, b'A' | b'G' | b'T'),
        b'H' => matches!(base, b'A' | b'C' | b'T'),
        b'V' => matches!(base, b'A' | b'C' | b'G'),
        b'N' => true,
        _ => false,
    }
}

/// Does `target` match the IUPAC `pattern` base-for-base at the same length?
fn span_matches_iupac(target: &[u8], pattern: &str) -> bool {
    target.len() == pattern.len()
        && pattern.bytes().zip(target).all(|(c, b)| iupac_code_matches(c, *b))
}

/// Per-template-position read state for an alignment: `covered[p]` is true when
/// the alignment spans base `p` (a gap counts), `base[p]` is the aligned read
/// base (uppercase, `None` for a deletion/uncovered column).
fn read_allele_arrays(a: &Alignment, tlen: usize) -> (Vec<bool>, Vec<Option<u8>>) {
    let mut covered = vec![false; tlen];
    let mut base: Vec<Option<u8>> = vec![None; tlen];
    for seg in &a.segments {
        for (i, ch) in seg.chars.bytes().enumerate() {
            let pos = (seg.start + i) % tlen;
            covered[pos] = true;
            if ch != b'-' {
                base[pos] = Some(ch.to_ascii_uppercase());
            }
        }
    }
    (covered, base)
}

/// Template positions covered by a recognition site, in 5'->3' order (wraps the
/// origin when `rec_start > rec_end`).
fn site_positions(rec_start: i64, rec_end: i64, tlen: i64) -> Vec<usize> {
    if rec_start <= rec_end {
        (rec_start..=rec_end).map(|p| p as usize).collect()
    } else {
        (rec_start..tlen)
            .chain(0..=rec_end)
            .map(|p| p as usize)
            .collect()
    }
}

/// Variant impact of one alignment on the project's restriction sites:
/// destroyed / (optionally) intact template sites plus newly created sites,
/// all as 1-based inclusive spans. Creation detection scans the full enzyme
/// database around the read's differences and does not model insertions or
/// origin-spanning new sites.
pub(crate) fn affected_sites_json(
    a: &Alignment,
    template: &str,
    tlen: i64,
    enzymes: &[Enzyme],
    include_intact: bool,
) -> Vec<serde_json::Value> {
    if tlen <= 0 || a.segments.is_empty() {
        return Vec::new();
    }
    let t = tlen as usize;
    let (covered, base) = read_allele_arrays(a, t);
    let template_upper = template.to_ascii_uppercase();
    let template_bytes = template_upper.as_bytes();
    let mut out: Vec<serde_json::Value> = Vec::new();

    for enz in enzymes {
        let positions = site_positions(enz.rec_start, enz.rec_end, tlen);
        if positions.is_empty() || !positions.iter().all(|&p| covered[p]) {
            continue;
        }
        let target: Vec<u8> = positions.iter().map(|&p| base[p].unwrap_or(b'-')).collect();
        let intact = span_matches_iupac(&target, &enz.rec_seq_pattern);
        if intact && !include_intact {
            continue;
        }
        let changed: Vec<serde_json::Value> = positions
            .iter()
            .filter_map(|&p| {
                let tb = template_bytes
                    .get(p)
                    .copied()
                    .map(|b| b.to_ascii_uppercase())
                    .unwrap_or(b'?');
                let show = base[p].map(|b| b.to_ascii_uppercase()).unwrap_or(b'-');
                (show != tb).then(|| {
                    serde_json::json!({
                        "position": p as i64 + 1,
                        "templateBase": (tb as char).to_string(),
                        "readBase": (show as char).to_string(),
                    })
                })
            })
            .collect();
        out.push(serde_json::json!({
            "enzyme": enz.name,
            "status": if intact { "intact" } else { "destroyed" },
            "recStart": enz.rec_start + 1,
            "recEnd": enz.rec_end + 1,
            "templateSeq": enz.rec_seq,
            "readSeq": String::from_utf8(target).unwrap_or_default(),
            "recognitionStrand": enz.recognition_strand,
            "isUnique": enz.is_unique,
            "changedBases": changed,
        }));
    }

    // Template sites by enzyme name, used to skip "created" hits that merely
    // restate an existing site.
    let mut existing: HashMap<&str, HashSet<(i64, i64)>> = HashMap::new();
    for enz in enzymes {
        existing
            .entry(enz.name.as_str())
            .or_default()
            .insert((enz.rec_start, enz.rec_end));
    }
    // Created sites: scan windows around the read's differences against the FULL
    // enzyme database, reporting only sites that overlap a difference and do not
    // already exist at the same span in the template.
    let diff_positions: Vec<usize> = (0..t)
        .filter(|&p| {
            covered[p]
                && template_bytes.get(p).copied().map(|b| b.to_ascii_uppercase())
                    != base[p].map(|b| b.to_ascii_uppercase())
        })
        .collect();
    if !diff_positions.is_empty() {
        let db = libregene_core::enzyme::search::get_db();
        let max_len = db.enzymes.iter().map(|e| e.site.len()).max().unwrap_or(0);
        // top-strand recognition string -> (enzyme name, recognition strand)
        let mut pattern_names: HashMap<String, Vec<(&str, &str)>> = HashMap::new();
        for e in &db.enzymes {
            let top = e.site.to_ascii_uppercase();
            pattern_names
                .entry(top.clone())
                .or_default()
                .push((e.name.as_str(), "top"));
            if !e.is_palindromic {
                let bottom = libregene_core::enzyme::search::iupac_complement(&top);
                pattern_names
                    .entry(bottom)
                    .or_default()
                    .push((e.name.as_str(), "bottom"));
            }
        }
        // Merge the difference windows (a new site must contain a difference).
        let mut windows: Vec<(i64, i64)> = Vec::new();
        for &d in &diff_positions {
            let s = (d as i64 - max_len as i64).max(0);
            let e = (d as i64 + max_len as i64).min(tlen - 1);
            match windows.last_mut() {
                Some((_, prev_e)) if s <= *prev_e + 1 => *prev_e = (*prev_e).max(e),
                _ => windows.push((s, e)),
            }
        }
        let diff_set: HashSet<usize> = diff_positions.iter().copied().collect();
        let mut seen_hits: HashSet<(String, i64, i64)> = HashSet::new();
        'windows: for (ws, we) in windows {
            let mut sub = String::new();
            let mut sub_coords: Vec<i64> = Vec::new();
            for p in ws..=we {
                if let Some(b) = base[p as usize] {
                    sub.push(b as char);
                    sub_coords.push(p);
                }
            }
            if sub.is_empty() {
                continue;
            }
            for (pattern, names) in &pattern_names {
                let plen = pattern.len();
                for hit in libregene_core::enzyme::matching::fuzzy_find_all(sub.as_bytes(), pattern) {
                    if hit + plen > sub_coords.len() {
                        continue;
                    }
                    let contiguous = (1..plen).all(|j| {
                        (sub_coords[hit + j] - sub_coords[hit + j - 1]).rem_euclid(tlen) == 1
                    });
                    if !contiguous {
                        continue;
                    }
                    let start = sub_coords[hit];
                    let end = sub_coords[hit + plen - 1];
                    if !(start..=end).any(|p| diff_set.contains(&(p as usize))) {
                        continue;
                    }
                    for (name, strand) in names {
                        if existing
                            .get(*name)
                            .is_some_and(|s| s.contains(&(start, end)))
                        {
                            continue;
                        }
                        if !seen_hits.insert((name.to_string(), start, end)) {
                            continue;
                        }
                        out.push(serde_json::json!({
                            "enzyme": name,
                            "status": "created",
                            "recStart": start + 1,
                            "recEnd": end + 1,
                            "templateSeq": (start..=end)
                                .map(|p| template_bytes.get(p as usize).copied().unwrap_or(b'?') as char)
                                .collect::<String>(),
                            "readSeq": sub[hit..hit + plen].to_ascii_uppercase(),
                            "recognitionStrand": strand,
                            "isUnique": false,
                            "changedBases": [],
                        }));
                        if out.len() >= 200 {
                            break 'windows;
                        }
                    }
                }
            }
        }
    }

    out
}

/// True when the 1-based inclusive site span intersects the 1-based focus
/// window (wrap-aware on circular templates).
fn affected_in_window(rec_start: i64, rec_end: i64, s: i64, e: i64, tlen: i64) -> bool {
    site_positions(rec_start - 1, rec_end - 1, tlen)
        .iter()
        .any(|&p| in_window_1based(p as i64 + 1, s, e))
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn add_alignment_impl(
        &self,
        request: AddAlignmentRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            Json(v)
        };
        let name = request.name.clone();
        let compact = request.compact.unwrap_or(false);
        let include_intact = request.include_intact_sites.unwrap_or(false);
        let tlen = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id).map(|p| p.length).unwrap_or(0)
        };

        // Resolve the optional focus window to internal 0-based inclusive
        // coordinates ((s, e), s > e wraps the origin on circular templates).
        let focus: Option<(i64, i64)> = {
            if request.region.is_some() && request.feature_id.is_some() {
                return Ok(fail(
                    "region and featureId are mutually exclusive".to_string(),
                ));
            }
            let flank = request.flank.unwrap_or(0).clamp(0, MAX_FLANK);
            let pm = self.pm.read().await;
            let p = pm
                .get_project_by_id(&id)
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
            let len = p.length;
            let circular = p.topology == "circular";
            if let Some(r) = &request.region {
                if r.start < 1 || r.start > len || r.end < 1 || r.end > len {
                    return Ok(fail(format!(
                        "region {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                        r.start, r.end, len
                    )));
                }
                if r.start > r.end && !circular {
                    return Ok(fail(
                        "region start > end wraps the origin and is only allowed on circular sequences".to_string(),
                    ));
                }
                let s = r.start.saturating_sub(1).saturating_sub(flank).max(0);
                let e = r.end.saturating_sub(1).saturating_add(flank).min(len - 1);
                Some((s, e))
            } else if let Some(fid) = &request.feature_id {
                let f = match p.features.iter().find(|f| &f.id == fid) {
                    Some(f) => f,
                    None => {
                        return Ok(fail(format!(
                            "feature '{}' not found in project; list feature ids with get_project_overview",
                            fid
                        )));
                    }
                };
                let s = f.start.saturating_sub(flank).max(0);
                let e = f.end.saturating_add(flank).min(len - 1);
                Some((s, e))
            } else {
                None
            }
        };

        let mut trace_path: Option<String> = None;
        let seq = match (request.bases, request.path) {
            (Some(_), Some(_)) => {
                return Ok(fail(
                    "Provide exactly one of `bases` or `path`, not both".to_string(),
                ));
            }
            (None, None) => {
                return Ok(fail(
                    "Provide exactly one of `bases` (sequence string) or `path` (sequence file)".to_string(),
                ));
            }
            (Some(bases), None) => bases,
            (None, Some(path)) => {
                let ext = crate::validate_user_path(&path, crate::SEQ_EXTS).map_err(|e| {
                    ErrorData::invalid_params(format!("invalid path: {}", e), None)
                })?;
                if ext == "ab1" {
                    trace_path = Some(path.clone());
                }
                let parsed = tokio::task::spawn_blocking(move || {
                    libregene_core::file_io::parse_file(std::path::Path::new(&path))
                })
                .await
                .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
                match parsed {
                    Ok(data) => data.sequence,
                    Err(e) => {
                        return Ok(fail(format!(
                            "Failed to read alignment sequence file (supported: .gbk/.gb/.genbank, .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1): {}",
                            e
                        )));
                    }
                }
            }
        };

        let payload = match crate::do_add_alignment_seq(
            &self.app_handle,
            &self.pm,
            &self.wp,
            &self.agent_tabs,
            None,
            &id,
            request.name,
            seq,
            trace_path,
            crate::parse_align_algorithm(request.algorithm.as_deref()),
        )
        .await
        {
            Ok(p) => p,
            Err(e) if e.starts_with("No significant alignment found") => {
                let mut v = serde_json::json!({
                    "ok": false,
                    "message": e,
                    "projectId": id.clone(),
                    "unit": "bp",
                    "significant": false,
                });
                if let Some(h) = &seq_hashes {
                    insert_seq_hashes(&mut v, h);
                }
                return Ok(Json(v));
            }
            Err(e) => return Err(ErrorData::internal_error(e, None)),
        };
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(fail(err));
        }
        let (summary, alignments, region, coverage_note, in_window, mut affected) = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .map(|p| {
                    let circular = p.topology == "circular";
                    let total = p.alignments.len();
                    let mut in_window = None;
                    let alignments: Vec<serde_json::Value> = p
                        .alignments
                        .iter()
                        .enumerate()
                        .map(|(idx, a)| {
                            if idx + 1 == total {
                                // The newly added alignment keeps its diff
                                // details (focused to the window when given);
                                // orientedSequence only in a non-compact,
                                // unfocused response.
                                let mut v = alignment_json_1based(
                                    a,
                                    &p.sequence,
                                    p.length,
                                    circular,
                                    compact || focus.is_some(),
                                );
                                if let Some((s, e)) = focus {
                                    let (m, i, d) = filter_alignment_json_focus(
                                        &mut v,
                                        s + 1,
                                        e + 1,
                                        p.length,
                                        circular,
                                    );
                                    in_window = Some((m, i, d));
                                }
                                v
                            } else {
                                alignment_stats_json_1based(a, &p.sequence, p.length, circular)
                            }
                        })
                        .collect();
                    let last = p.alignments.last();
                    let region = focus.or_else(|| {
                        last.and_then(|a| {
                            a.segments.first().map(|s| (s.start as i64, s.end as i64))
                        })
                    });
                    let coverage_note = last.and_then(|a| {
                        let gap = uncovered_between_segments(a, p.sequence.len(), circular);
                        (gap > 0).then(|| {
                            format!("{} template bp uncovered between the read's coverage segments", gap)
                        })
                    });
                    let affected = last
                        .map(|a| {
                            affected_sites_json(
                                a,
                                &p.sequence,
                                p.length,
                                &p.enzymes,
                                include_intact,
                            )
                        })
                        .unwrap_or_default();
                    (
                        alignments.last().cloned(),
                        alignments,
                        region,
                        coverage_note,
                        in_window,
                        affected,
                    )
                })
                .unwrap_or((None, Vec::new(), None, None, None, Vec::new()))
        };
        let region_view = if compact {
            None
        } else {
            match region {
                Some((s, e)) => self.digest_region(&id, Some((s, e)), true).await,
                None => self.digest_region(&id, None, true).await,
            }
        };
        let mut env = ok_envelope(&id, format!("Aligned {}", name), region_view);
        env["unit"] = serde_json::json!("bp");
        if let Some(s) = summary {
            env["significant"] = serde_json::json!(true);
            for (k, v) in s.as_object().unwrap_or(&serde_json::Map::new()) {
                if compact && k == "orientedSequence" {
                    continue;
                }
                env[k] = v.clone();
            }
        }
        env["alignments"] = serde_json::json!(alignments);
        // Variant impact of the newly added read on the project's enzymes.
        if let Some((s, e)) = focus {
            affected.retain(|v| {
                let rs = v.get("recStart").and_then(|x| x.as_i64()).unwrap_or(0);
                let re = v.get("recEnd").and_then(|x| x.as_i64()).unwrap_or(0);
                affected_in_window(rs, re, s + 1, e + 1, tlen)
            });
        }
        let affected_json = serde_json::json!(affected);
        env["affectedSiteCount"] = serde_json::json!(affected.len());
        env["affectedSites"] = affected_json.clone();
        if let Some(last) = env["alignments"].as_array_mut().and_then(|arr| arr.last_mut()) {
            last["affectedSites"] = affected_json;
        }
        if env["affectedSiteCount"].as_u64().unwrap_or(0) > 0 {
            push_note(
                &mut env,
                "affectedSites: restriction sites this read DESTROYS or CREATES (created detection scans the full enzyme database around the read's differences and ignores insertions/origin-spanning new sites)",
            );
        }
        // Window block: the detail lists above are filtered to it, while the
        // total mismatches/insertions/deletions still describe the whole read.
        if let Some((s, e)) = focus {
            let (m, i, d) = in_window.unwrap_or((0, 0, 0));
            env["window"] = serde_json::json!({
                "start": s + 1,
                "end": e + 1,
                "featureId": request.feature_id,
                "flank": request.flank.unwrap_or(0),
                "mismatches": m,
                "insertions": i,
                "deletions": d,
                "note": "mismatchDetails/deletionDetails/insertionDetails are filtered to this window; total mismatches/insertions/deletions still describe the whole read",
            });
        }
        if let Some(note) = coverage_note {
            push_note(&mut env, note.clone());
            if let Some(last) = env["alignments"].as_array_mut().and_then(|arr| arr.last_mut()) {
                last["coverageNote"] = serde_json::json!(note);
            }
        }
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut env, h);
        }
        Ok(Json(env))
    }

}
