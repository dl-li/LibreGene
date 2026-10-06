//! add_alignment MCP tool + alignment JSON helpers.

use rmcp::{ErrorData, handler::server::wrapper::Json};
use tauri::Runtime;

use libregene_core::digest::cut_flanks;

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
        let (summary, alignments, region, coverage_note, in_window) = {
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
                    (
                        alignments.last().cloned(),
                        alignments,
                        region,
                        coverage_note,
                        in_window,
                    )
                })
                .unwrap_or((None, Vec::new(), None, None, None))
        };
        let region_view = if compact {
            None
        } else {
            match region {
                Some((s, e)) => self.digest_region(&id, Some((s, e)), true, true).await,
                None => self.digest_region(&id, None, true, true).await,
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
