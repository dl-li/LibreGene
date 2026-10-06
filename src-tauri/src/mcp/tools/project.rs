//! Project lifecycle MCP tools: open/save/close + the save_file region
//! export engine.

use rmcp::{ErrorData, handler::server::wrapper::Json};
use tauri::{Emitter, Runtime};

use libregene_core::digest::cut_notation;
use libregene_core::models::{Enzyme, Feature, Primer, PrimerBindingSite, ProjectData, Segment};

use crate::mcp::LibreGeneMcp;
use super::convert::output_project_name;
use crate::mcp::support::{fail_envelope, from1, insert_seq_hashes, ok_envelope, unit_for};
use crate::mcp::types::{CloseProjectRequest, OpenProjectRequest, RegionSpec, SaveFileRequest};

// ---------------------------------------------------------------------------
// save_file region mode: region resolution + export data building
// ---------------------------------------------------------------------------

/// Linear template pieces for an internal 0-based inclusive region; on
/// circular sequences a wrap (start > end) becomes two pieces.
fn region_pieces(project: &ProjectData, s: i64, e: i64) -> Vec<(i64, i64)> {
    if project.topology == "circular" && s > e {
        vec![(s, project.length - 1), (0, e)]
    } else {
        vec![(s, e)]
    }
}

/// The fragment between two cuts (internal 0-based; a cut at index C severs
/// the DNA between bases C-1 and C). Linear: the span between the smaller and
/// the larger cut ([lo, hi-1]). Circular: the forward arc from cut1 to cut2,
/// wrapping over the origin when cut1 > cut2, the whole molecule when they
/// coincide.
fn fragment_pieces(project: &ProjectData, c1: i64, c2: i64) -> Result<Vec<(i64, i64)>, String> {
    let len = project.length;
    if project.topology == "circular" {
        if c1 < c2 {
            Ok(vec![(c1, c2 - 1)])
        } else if c1 > c2 {
            let mut v = vec![(c1, len - 1)];
            if c2 > 0 {
                v.push((0, c2 - 1));
            }
            Ok(v)
        } else {
            Ok(vec![(0, len - 1)])
        }
    } else {
        let (lo, hi) = (c1.min(c2), c1.max(c2));
        if lo == hi {
            return Err("cut1 and cut2 are equal — the fragment between them is empty".to_string());
        }
        Ok(vec![(lo, hi - 1)])
    }
}

/// The top-strand cut index (internal 0-based) of an enzyme's `ordinal`-th
/// recognition site (sorted by rec_start) from the already-computed engine
/// results. Unknown enzymes error with near-match suggestions, mirroring
/// find_restriction_sites.
pub(crate) fn enzyme_cut_index(project: &ProjectData, name: &str, ordinal: usize) -> Result<i64, String> {
    let mut hits: Vec<&Enzyme> = project
        .enzymes
        .iter()
        .filter(|e| e.name.eq_ignore_ascii_case(name))
        .collect();
    hits.sort_by_key(|e| e.rec_start);
    match hits.get(ordinal) {
        Some(site) => Ok(if site.cut_pairs.is_empty() {
            site.cut_index
        } else {
            site.cut_pairs[0].top_cut_index
        }),
        None => {
            let q = name.to_lowercase();
            let sugg: Vec<&str> = project
                .enzymes
                .iter()
                .map(|e| e.name.as_str())
                .filter(|n| n.to_lowercase().contains(&q))
                .take(5)
                .collect();
            if hits.is_empty() {
                if sugg.is_empty() {
                    Err(format!(
                        "Unknown enzyme '{}': no enzyme with a recognition site in this project has a similar name",
                        name
                    ))
                } else {
                    Err(format!(
                        "Unknown enzyme '{}'; enzymes cutting this sequence with similar names: {}",
                        name,
                        sugg.join(", ")
                    ))
                }
            } else {
                Err(format!(
                    "Enzyme '{}' has only {} recognition site(s) on this sequence; cannot select site number {}",
                    name,
                    hits.len(),
                    ordinal + 1
                ))
            }
        }
    }
}

/// Resolve a fwd/rev primer argument — a project primer name (stored binding
/// sites are reused, recomputed when empty; name lookup wins) or a raw
/// sequence (binding sites recomputed with the primer engine) — to its best
/// binding site on the wanted strand. Mirrors check_primer_binding's strand
/// semantics: strand 1 = forward, strand -1 = reverse.
fn resolve_primer_binding_site(
    project: &ProjectData,
    input: &str,
    want_strand: i8,
    role: &str,
) -> Result<PrimerBindingSite, String> {
    let seq: String;
    let sites: Vec<PrimerBindingSite>;
    if let Some(p) = project
        .primers
        .iter()
        .find(|p| p.name == input || p.id == input)
    {
        seq = p.primer_seq.clone();
        sites = if p.binding_sites.is_empty() {
            libregene_core::primer::align::compute_binding_sites(
                &project.sequence,
                &p.primer_seq,
                &p.r#type,
                &p.id,
                &project.topology,
                0.0,
            )
        } else {
            p.binding_sites.clone()
        };
    } else {
        let cleaned: String = input
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_uppercase();
        if cleaned.is_empty() {
            return Err(format!(
                "{} '{}' is neither a primer name in the project nor a sequence",
                role, input
            ));
        }
        seq = cleaned.clone();
        let probe = Primer {
            id: role.to_string(),
            name: role.to_string(),
            r#type: "fwd".to_string(),
            primer_seq: cleaned,
            binding_sites: Vec::new(),
        };
        sites = libregene_core::primer::align::compute_binding_sites(
            &project.sequence,
            &probe.primer_seq,
            &probe.r#type,
            &probe.id,
            &project.topology,
            0.0,
        );
    }
    sites
        .iter()
        .find(|s| s.strand == want_strand)
        .cloned()
        .ok_or_else(|| {
            let strand_name = if want_strand == 1 { "forward" } else { "reverse" };
            format!(
                "{} '{}' ({} bp) does not bind the {} strand of the template: {} binding site(s) found, none on the {} strand",
                role, input, seq.len(), strand_name, sites.len(), strand_name
            )
        })
}

/// Bounding box for the export `text` digest. min/max over all pieces:
/// first/last is wrong for multi-segment minus-strand features, whose pieces
/// come back from `resolve_export_region` in descending order. On circular
/// templates, pieces that straddle the origin ((s, len-1) + (0, e)) collapse
/// to the wrap window (s, e) — tighter than a full-length min/max span and
/// meaningful to `project_digest`.
fn region_bbox(pieces: &[(i64, i64)], len: i64, circular: bool) -> (i64, i64) {
    if circular {
        let wrap_start = pieces.iter().filter(|p| p.1 == len - 1).map(|p| p.0).max();
        let wrap_end = pieces.iter().filter(|p| p.0 == 0).map(|p| p.1).min();
        if let (Some(s), Some(e)) = (wrap_start, wrap_end) {
            if s > e {
                return (s, e);
            }
        }
    }
    (
        pieces.iter().map(|p| p.0).min().unwrap_or(0),
        pieces.iter().map(|p| p.1).max().unwrap_or(0),
    )
}

/// Resolve a save_file `region` selector to the template pieces it exports:
/// linear internal 0-based inclusive spans in EXPORT order, `flip` (each
/// piece's sequence is reverse-complemented when exporting a minus-strand
/// feature) and a human-readable description of the selected region (1-based,
/// like every agent-facing string). The selector's region `start`/`end` are
/// already converted to internal 0-based by the caller; `cut1`/`cut2` are
/// still the raw 1-based flanking-base numbers and are converted here.
/// Exactly one selector must be given; mixing selectors is rejected.
pub(crate) fn resolve_export_region(
    project: &ProjectData,
    req: &RegionSpec,
) -> Result<(Vec<(i64, i64)>, bool, String), String> {
    let region_active = req.start.is_some() || req.end.is_some();
    let feature_active = req.feature_id.is_some();
    let fragment_active = req.enzyme1.is_some()
        || req.enzyme2.is_some()
        || req.cut1.is_some()
        || req.cut2.is_some();
    let amplicon_active = req.fwd_primer.is_some() || req.rev_primer.is_some();
    let active = [region_active, feature_active, fragment_active, amplicon_active]
        .into_iter()
        .filter(|a| *a)
        .count();
    if active != 1 {
        return Err(
            "exactly one region selector required: (start+end), (featureId), (enzyme1+enzyme2 | cut1+cut2), or (fwdPrimer+revPrimer)"
                .to_string(),
        );
    }
    if project.length <= 0 || project.sequence.is_empty() {
        return Err("Sequence is empty".to_string());
    }
    let len = project.length;
    let circular = project.topology == "circular";

    if region_active {
        let (s, e) = match (req.start, req.end) {
            (Some(s), Some(e)) => (s, e),
            _ => return Err("start and end are both required (1-based inclusive)".to_string()),
        };
        if s > e && !circular {
            return Err(
                "start > end is only allowed on circular sequences (wraps the origin)".to_string(),
            );
        }
        if s < 0 || e < 0 || s >= len || e >= len {
            return Err(format!(
                "range {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                s + 1,
                e + 1,
                len
            ));
        }
        return Ok((
            region_pieces(project, s, e),
            false,
            format!("region {}..{}", s + 1, e + 1),
        ));
    }

    if feature_active {
        let fid = req.feature_id.as_deref().unwrap_or("");
        let f = project
            .features
            .iter()
            .find(|f| f.id == fid)
            .ok_or_else(|| format!("Feature not found: {}", fid))?;
        let segs: Vec<(i64, i64)> = if f.segments.is_empty() {
            vec![(f.start, f.end)]
        } else {
            f.segments.iter().map(|s| (s.start, s.end)).collect()
        };
        let mut pieces: Vec<(i64, i64)> = Vec::new();
        for &(s, e) in &segs {
            if s <= e {
                pieces.push((s, e));
            } else if circular {
                pieces.push((s, len - 1));
                pieces.push((0, e));
            } else {
                return Err(format!(
                    "feature {} spans the origin but the project is linear",
                    f.id
                ));
            }
        }
        for &(s, e) in &pieces {
            // Same shape as the region-selector check above; feature
            // coordinates come from parsed files, so every bound (including
            // e < 0 from a crafted range and s >= len from a wrap split of an
            // out-of-range start) must be rejected before the pieces are
            // sliced. saturating_add: a rejected piece may hold the i64 seeds.
            if s < 0 || e < 0 || s >= len || e >= len {
                return Err(format!(
                    "feature {} coordinate {}..{} out of range for sequence of length {} (1-based inclusive)",
                    f.id,
                    s.saturating_add(1),
                    e.saturating_add(1),
                    len
                ));
            }
        }
        // Keep join order: for origin-wrapping (or otherwise non-ascending)
        // segments the biological 5'→3' order is the stored join order —
        // reversed for the minus strand — NOT coordinate-ascending order.
        let minus = f.strand == "-";
        if minus && !project.is_dna() {
            // Minus-strand export reverse-complements each piece, which is
            // only meaningful for DNA — on RNA/protein projects (single-
            // strand sequences) it would corrupt the exported sequence.
            return Err(format!(
                "feature '{}' is on the minus strand, but minus-strand export (reverse complement) is only supported for DNA projects; this is a {} project",
                f.id, project.molecule_type
            ));
        }
        if minus {
            pieces.reverse();
        }
        return Ok((pieces, minus, format!("feature '{}' ({})", f.name, f.id)));
    }

    if fragment_active {
        let (c1, c2, desc) = match (&req.enzyme1, &req.enzyme2, req.cut1, req.cut2) {
            (Some(e1), Some(e2), None, None) => {
                let c1 = enzyme_cut_index(project, e1, 0)?;
                let c2 = if e1.eq_ignore_ascii_case(e2) {
                    enzyme_cut_index(project, e2, 1)?
                } else {
                    enzyme_cut_index(project, e2, 0)?
                };
                // Type-IIS enzymes cut outside their recognition site; on a
                // linear molecule a site near an end can place the cut before
                // base 1 or past the last base (circular cuts are normalized
                // into [0, len) by the engine). Slicing there would panic.
                if !circular {
                    for (name, c) in [(e1, c1), (e2, c2)] {
                        if c < 1 || c > len {
                            return Err(format!(
                                "cut of enzyme {} at position {} falls outside the linear molecule (1..={})",
                                name, c, len
                            ));
                        }
                    }
                }
                (
                    c1,
                    c2,
                    format!(
                        "fragment between {} (cut {}) and {} (cut {})",
                        e1,
                        cut_notation(c1, len, circular),
                        e2,
                        cut_notation(c2, len, circular)
                    ),
                )
            }
            (None, None, Some(a), Some(b)) => {
                // 1-based input: a cut at N severs the DNA between the 1-based
                // bases N and N+1. An internal cut index C severs between the
                // 0-based bases C-1 and C, so the numeric value of N carries
                // over unchanged; on circular, N = len is the origin cut (0).
                if a < 1 || b < 1 || a > len || b > len {
                    return Err(format!(
                        "cut positions {} and {} out of range (1..={} for a {} bp {}; a cut at N severs the DNA between 1-based bases N and N+1)",
                        a, b, len, len, project.topology
                    ));
                }
                let (a, b) = if circular { (a % len, b % len) } else { (a, b) };
                (
                    a,
                    b,
                    format!(
                        "fragment between cuts {} and {}",
                        cut_notation(a, len, circular),
                        cut_notation(b, len, circular)
                    ),
                )
            }
            _ => {
                return Err(
                    "fragment mode needs enzyme1+enzyme2 (names) OR cut1+cut2 (positions), not a mix"
                        .to_string(),
                )
            }
        };
        return Ok((fragment_pieces(project, c1, c2)?, false, desc));
    }

    let fwd = req.fwd_primer.as_deref().unwrap_or("");
    let rev = req.rev_primer.as_deref().unwrap_or("");
    if fwd.is_empty() || rev.is_empty() {
        return Err(
            "fwdPrimer and revPrimer are both required (name or sequence)".to_string(),
        );
    }
    let fsite = resolve_primer_binding_site(project, fwd, 1, "fwd primer")?;
    let rsite = resolve_primer_binding_site(project, rev, -1, "rev primer")?;
    let f_start = fsite.template_start;
    // Rev primer's 5' end is the last template base it covers (template_end
    // is exclusive); the amplicon runs from the fwd 5' end to that base.
    let r_end = if rsite.template_end == 0 {
        len - 1
    } else {
        rsite.template_end - 1
    };
    // A rev site wrapping the origin stores template_end = (start +
    // footprint) % len, so template_end < template_start (the == 0 case is
    // already mapped to r_end = len - 1 above). The amplicon then always
    // spans the origin — even when f_start <= r_end numerically, the forward
    // arc from the fwd 5' end reaches the rev 5' end only across the origin.
    let rev_wraps = circular && rsite.template_end != 0 && rsite.template_end < rsite.template_start;
    let pieces = if circular {
        if !rev_wraps && f_start <= r_end {
            vec![(f_start, r_end)]
        } else {
            vec![(f_start, len - 1), (0, r_end)]
        }
    } else {
        if f_start > r_end {
            return Err(format!(
                "fwd primer's 5' end (1-based position {}) is downstream of the rev primer's 5' end (1-based position {}); the pair does not define an amplicon on a linear sequence",
                f_start + 1, r_end + 1
            ));
        }
        vec![(f_start, r_end)]
    };
    Ok((
        pieces,
        false,
        format!("amplicon fwd '{}' → rev '{}'", fwd, rev),
    ))
}

/// Build the exported sequence (template bases of the pieces, uppercase,
/// reverse-complemented per piece when `flip`) and the features overlapping
/// the pieces with coordinates translated to the new linear coordinate
/// system (strand flipped when `flip`). A feature-mode export's own feature
/// naturally lands on the full [0, len-1] span. Primers come along when
/// their primary binding site (binding_sites[0]) overlaps the pieces at all;
/// the site is clipped/translated like a feature span and reopening the
/// exported file recomputes exact sites anyway.
pub(crate) fn build_export_data(
    project: &ProjectData,
    pieces: &[(i64, i64)],
    flip: bool,
) -> (String, Vec<Feature>, Vec<Primer>) {
    let mut sequence = String::new();
    let mut windows: Vec<(i64, i64)> = Vec::with_capacity(pieces.len());
    let mut off: i64 = 0;
    for &(s, e) in pieces {
        let span = &project.sequence[s as usize..=e as usize];
        if flip {
            sequence.push_str(&libregene_core::utils::reverse_complement(span));
        } else {
            sequence.push_str(span);
        }
        windows.push((off, off + (e - s + 1)));
        off += e - s + 1;
    }
    let mut features: Vec<Feature> = Vec::new();
    for f in &project.features {
        let segs: Vec<(i64, i64)> = if f.segments.is_empty() {
            vec![(f.start, f.end)]
        } else {
            f.segments.iter().map(|s| (s.start, s.end)).collect()
        };
        let mut mapped: Vec<(i64, i64)> = Vec::new();
        for (pi, &(ps, pe)) in pieces.iter().enumerate() {
            let (wo, _) = windows[pi];
            for &(s, e) in &segs {
                let os = s.max(ps);
                let oe = e.min(pe);
                if os <= oe {
                    let (ns, ne) = if flip {
                        (wo + (pe - oe), wo + (pe - os))
                    } else {
                        (wo + (os - ps), wo + (oe - ps))
                    };
                    mapped.push((ns, ne));
                }
            }
        }
        if mapped.is_empty() {
            continue;
        }
        let merged = merge_sorted_segments(mapped);
        let nstart = merged[0].0;
        let nend = merged[merged.len() - 1].1;
        let nstrand = if flip {
            match f.strand.as_str() {
                "+" => "-".to_string(),
                "-" => "+".to_string(),
                s => s.to_string(),
            }
        } else {
            f.strand.clone()
        };
        features.push(Feature {
            id: f.id.clone(),
            name: f.name.clone(),
            start: nstart,
            end: nend,
            color: f.color.clone(),
            ftype: f.ftype.clone(),
            segments: merged
                .iter()
                .map(|&(s, e)| Segment {
                    start: s,
                    end: e,
                    color: None,
                })
                .collect(),
            strand: nstrand,
            notes: f.notes.clone(),
            translation: f.translation.clone(),
            qualifiers: f.qualifiers.clone(),
        });
    }
    let mut primers: Vec<Primer> = Vec::new();
    for p in &project.primers {
        let Some(site) = p.binding_sites.first() else {
            continue;
        };
        let ss = site.template_start;
        let se = site.template_end - 1;
        // Circular origin-wrapping sites are stored with template_end =
        // (start + footprint) % len, i.e. se < ss; split the span into its
        // two arcs so the overlap test below sees both (otherwise the
        // inverted span matches nothing and the primer is silently dropped).
        let spans: Vec<(i64, i64)> = if se < ss && project.topology == "circular" {
            vec![(ss, project.length - 1), (0, se)]
        } else {
            vec![(ss, se)]
        };
        let mut mapped: Vec<(i64, i64)> = Vec::new();
        for (pi, &(ps, pe)) in pieces.iter().enumerate() {
            let (wo, _) = windows[pi];
            for &(s, e) in &spans {
                let os = s.max(ps);
                let oe = e.min(pe);
                if os > oe {
                    continue;
                }
                let (ns, ne) = if flip {
                    (wo + (pe - oe), wo + (pe - os))
                } else {
                    (wo + (os - ps), wo + (oe - ps))
                };
                mapped.push((ns, ne));
            }
        }
        let merged = merge_sorted_segments(mapped);
        let (ns, ne) = match merged.len() {
            0 => continue,
            1 => merged[0],
            // Both arcs of an origin-wrapping site were exported but stay
            // disjoint in the linear export (e.g. a full-circle export):
            // keep the wrapped form (template_end < template_start) so every
            // bound base survives — gbk.rs writes it as a join.
            _ => (merged[merged.len() - 1].0, merged[0].1),
        };
        let mut site = site.clone();
        site.template_start = ns;
        site.template_end = ne + 1;
        if flip {
            site.strand = -site.strand;
        }
        let mut np = p.clone();
        np.binding_sites = vec![site];
        primers.push(np);
    }
    (sequence.to_ascii_uppercase(), features, primers)
}

/// Sort spans ascending and merge overlapping/touching ones.
fn merge_sorted_segments(mut segs: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    segs.sort_unstable();
    let mut out: Vec<(i64, i64)> = Vec::new();
    for (s, e) in segs {
        if let Some(last) = out.last_mut() {
            if s <= last.1 + 1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        out.push((s, e));
    }
    out
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn open_project_impl(
        &self,
        request: OpenProjectRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = request.path;
        // Fast path: already loaded → reuse the binding or reject. No load
        // happens on this path, so there is nothing to race with.
        let already_loaded = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id).is_some()
        };
        if already_loaded {
            return self.reuse_agent_tab_or_reject(&id).await;
        }
        crate::validate_user_path(&id, crate::SEQ_EXTS)
            .map_err(|e| ErrorData::invalid_params(format!("invalid path: {}", e), None))?;
        // Parse + recompute off the executor with no locks held (the same
        // pipeline do_open_file runs for the frontend open_file command).
        let path_buf = std::path::PathBuf::from(&id);
        let parsed = tokio::task::spawn_blocking(move || -> Result<ProjectData, String> {
            let mut project =
                libregene_core::file_io::parse_file(&path_buf).map_err(|e| e.to_string())?;
            libregene_core::enzyme::recompute(&mut project);
            libregene_core::primer::recompute(&mut project);
            Ok(project)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
        let project = match parsed {
            Ok(p) => p,
            Err(e) => return Ok(Json(fail_envelope(&id, e))),
        };
        // Atomic check-and-load under the pm write lock: the frontend's
        // open_file loads under the same write lock, so whoever takes it
        // first wins — if the user opened this file while we were parsing,
        // we see "already loaded" here and take the reuse/reject path
        // instead of clobbering their project and binding it as ours.
        let loaded = {
            let mut pm = self.pm.write().await;
            if pm.get_project_by_id(&id).is_some() {
                false
            } else {
                match pm.load(&id, project) {
                    Ok(()) => {
                        // A fresh load reflects the file on disk — clear any
                        // stale dirty marker from a previous incarnation.
                        pm.mark_clean(&id);
                        true
                    }
                    Err(e) => return Ok(Json(fail_envelope(&id, e))),
                }
            }
        };
        if !loaded {
            return self.reuse_agent_tab_or_reject(&id).await;
        }
        // The load may have evicted another project; drop its bindings.
        crate::prune_orphan_bindings(&self.pm, &self.wp, &self.agent_tabs).await;
        // Bind the freshly opened project as this agent's tab (locked).
        {
            let mut at = self.agent_tabs.write().await;
            at.insert(id.clone(), crate::AgentTabMeta { locked: true });
        }
        let _ = self.app_handle.emit(
            "agent-tab-lock",
            serde_json::json!({ "projectId": id, "locked": true }),
        );
        // A concurrent load between our load and the bind above could have
        // evicted this project; never leave a binding to a ghost behind.
        let still_loaded = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id).is_some()
        };
        if !still_loaded {
            self.agent_tabs.write().await.remove(&id);
            return Err(ErrorData::internal_error(
                format!(
                    "Project '{}' was evicted before it could be bound as an agent tab; retry open_project",
                    id
                ),
                None,
            ));
        }
        // The open_file command does not broadcast (frontend applies the
        // response) — the MCP server must notify the UI itself.
        crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, &self.agent_tabs, None).await;
        let summary = self.project_summary(&id).await.unwrap_or_else(|| format!("Opened {}", id));
        let region = self.digest_region(&id, None, true).await;
        let unit = self
            .pm
            .read()
            .await
            .get_project_by_id(&id)
            .map(|p| crate::mcp::support::unit_for(&p.molecule_type))
            .unwrap_or("bp");
        let mut v = ok_envelope(&id, summary, region);
        v["unit"] = serde_json::json!(unit);
        v["locked"] = serde_json::json!(true);
        if let Some(h) = self.project_seq_hashes(&id).await {
            insert_seq_hashes(&mut v, &h);
        }
        Ok(Json(v))
    }

    pub(crate) async fn save_file_impl(
        &self,
        request: SaveFileRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            insert_seq_hashes(&mut v, &seq_hashes);
            Json(v)
        };
        let path = request.path.clone();
        // Overwrite rule: a pre-existing target that is not the project's own
        // source file needs an explicit overwrite flag.
        if path != id
            && !request.overwrite.unwrap_or(false)
            && std::path::Path::new(&path).exists()
        {
            return Ok(fail(
                format!(
                    "{} already exists — pass overwrite: true to replace it, or choose a different path",
                    path
                ),
            ));
        }

        let unit = unit_for(&project.molecule_type);

        let Some(spec) = request.region else {
            // Whole-project save.
            let payload = crate::do_save_file(&self.pm, id.clone(), path.clone())
                .await
                .map_err(|e| ErrorData::internal_error(e, None))?;
            if let Some(err) = Self::payload_error(&payload) {
                return Ok(fail(err));
            }
            let bytes_written = payload.get("bytesWritten").and_then(|v| v.as_u64());
            crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, &self.agent_tabs, None).await;
            let region = self.digest_region(&id, None, true).await;
            let mut env = ok_envelope(&id, format!("Saved {}", path), region);
            insert_seq_hashes(&mut env, &seq_hashes);
            env["unit"] = serde_json::json!(unit);
            env["path"] = serde_json::json!(path);
            env["length"] = serde_json::json!(project.length);
            if let Some(b) = bytes_written {
                env["bytesWritten"] = serde_json::json!(b);
            }
            return Ok(Json(env));
        };

        // Region mode: subsequence export.
        if path == id && !request.overwrite.unwrap_or(false) {
            return Ok(fail(
                format!(
                    "{} is the project's own source file — a region export would replace the full file with just the fragment; pass overwrite: true if that is really intended, or choose a different path",
                    path
                ),
            ));
        }
        let ext = crate::validate_user_path(&path, crate::CODON_OUTPUT_EXTS)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let is_protein = project.molecule_type == "protein";
        let wants_gpt = ext == "gpt";
        if wants_gpt != is_protein {
            return Ok(fail(format!(
                "molecule type '{}' exports as {} (DNA/RNA → .gbk/.gb/.genbank, protein → .gpt)",
                project.molecule_type,
                if is_protein { ".gpt" } else { ".gbk/.gb/.genbank" }
            )));
        }
        let unit = unit_for(&project.molecule_type);

        // Region start/end arrive 1-based inclusive; convert to the internal
        // 0-based model. cut1/cut2 stay raw — resolve_export_region validates
        // and converts them (a cut at 1-based N = internal cut index N).
        let mut spec = spec;
        spec.start = spec.start.map(from1);
        spec.end = spec.end.map(from1);

        let (pieces, flip, desc) = resolve_export_region(&project, &spec)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let bbox = region_bbox(&pieces, project.length, project.topology == "circular");
        let out_name = output_project_name(&path);
        let message_path = path.clone();
        let (out_project, primer_names) = tokio::task::spawn_blocking(move || {
            let (sequence, features, primers) = build_export_data(&project, &pieces, flip);
            let primer_names: Vec<String> = primers.iter().map(|p| p.name.clone()).collect();
            let length = sequence.len() as i64;
            (
                ProjectData {
                    name: out_name,
                    sequence,
                    length,
                    topology: "linear".to_string(),
                    molecule_type: project.molecule_type.clone(),
                    features,
                    primers,
                    ..Default::default()
                },
                primer_names,
            )
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
        let length = out_project.length;

        let write_path = message_path.clone();
        let written = tokio::task::spawn_blocking(move || {
            let res = if wants_gpt {
                libregene_core::file_io::gpt::write_gpt(&out_project, std::path::Path::new(&write_path))
            } else {
                libregene_core::file_io::gbk::write_gbk(&out_project, std::path::Path::new(&write_path))
            };
            res.map_err(|e| format!("failed to write {}: {}", write_path, e))
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
        if let Err(e) = written {
            return Err(ErrorData::invalid_params(e, None));
        }

        let region = self.digest_region(&id, Some(bbox), true).await;
        let mut v = ok_envelope(
            &id,
            format!("Exported {} ({} {}) to {}", desc, length, unit, message_path),
            region,
        );
        v["unit"] = serde_json::json!(unit);
        v["path"] = serde_json::json!(message_path);
        v["length"] = serde_json::json!(length);
        v["primerCount"] = serde_json::json!(primer_names.len());
        if let Some(bytes) = std::fs::metadata(&message_path).ok().map(|m| m.len()) {
            v["bytesWritten"] = serde_json::json!(bytes);
        }
        if !primer_names.is_empty() {
            v["primers"] = serde_json::json!(primer_names);
        }
        insert_seq_hashes(&mut v, &seq_hashes);
        Ok(Json(v))
    }

    pub(crate) async fn close_project_impl(
        &self,
        request: CloseProjectRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = request.project_id.clone();
        self.require_agent_tab(&id).await?;
        let hashes = self.project_seq_hashes(&id).await;
        // The dirty/force check runs inside do_delete_project's pm write
        // critical section, so a concurrent mutation cannot slip in between
        // the check and the removal (TOCTOU).
        let payload = crate::do_delete_project(
            &self.app_handle,
            &self.pm,
            &self.wp,
            &self.agent_tabs,
            None,
            request.project_id,
            request.force.unwrap_or(false),
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            if err == "project not found" {
                return Err(ErrorData::invalid_params(format!("Project not found: {}", id), None));
            }
            let mut v = fail_envelope(&id, err);
            if let Some(h) = &hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        let mut v = ok_envelope(&id, format!("Closed project {}", id), None);
        if let Some(h) = &hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }

}
