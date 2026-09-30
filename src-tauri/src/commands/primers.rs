use tauri::{AppHandle, State};

use libregene_core::models::{Primer, Segment};

use crate::kernels::{
    do_add_primer, do_check_primers_binding, do_delete_primer, do_design_primer_candidates,
    primers_equal,
};
use crate::payload::{broadcast_project, filter_project, resolve_project_id, ProjectParams};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Tauri commands — primers
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn get_primers(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => {
            let pm = state.pm.read().await;
            match pm.get_project_by_id(&id) {
                Some(p) => {
                    let primers: Vec<serde_json::Value> = p
                        .primers
                        .iter()
                        .map(|primer| serde_json::to_value(primer).unwrap_or_default())
                        .collect();
                    Ok(serde_json::json!(primers))
                }
                None => Ok(serde_json::json!([])),
            }
        }
        Err(_) => Ok(serde_json::json!([])),
    }
}

#[tauri::command]
pub(crate) async fn add_primer(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    primer: Primer,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_add_primer(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        primer,
    )
    .await
}

#[tauri::command]
pub(crate) async fn delete_primer(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    id: String,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_delete_primer(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        id,
    )
    .await
}

/// Check which of the given primers can bind to the current project's sequence.
/// Returns a per-primer summary (binds + best `site`, plus the full best-first
/// `sites` array and `bindingSiteCount`), reusing the same binding-site engine
/// as the editor for consistency.
#[tauri::command]
pub(crate) async fn check_primers_binding(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    primers: Vec<Primer>,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_check_primers_binding(&state.pm, &project_id, primers).await
}

/// Add a batch of primers to the current project (My Primers → current file).
/// Rejects exact-sequence duplicates and name conflicts; recomputes once.
#[tauri::command]
pub(crate) async fn add_primers(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    primers: Vec<Primer>,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    // Snapshot → merge → recompute (off-lock) → write-back. The write-back
    // only lands when the primers list is unchanged since the snapshot;
    // otherwise a concurrent add slipped in and we retry onto the fresh list
    // (binding sites depend only on the template, which does not change).
    loop {
        let snapshot = {
            let pm = state.pm.read().await;
            pm.get_project_by_id(&project_id).map(|p| {
                (p.sequence.clone(), p.topology.clone(), p.primers.clone())
            })
        };
        let Some((template, topology, existing)) = snapshot else {
            return Ok(serde_json::json!({"error": "Project not found"}));
        };

        let mut merged = existing.clone();
        for primer in &primers {
            let seq_conflict = merged
                .iter()
                .any(|p| p.primer_seq.to_uppercase() == primer.primer_seq.to_uppercase());
            let name_conflict =
                merged.iter().any(|p| p.id != primer.id && p.name == primer.name);
            if seq_conflict || name_conflict {
                continue;
            }
            if let Some(pos) = merged.iter().position(|p| p.id == primer.id) {
                merged[pos] = primer.clone();
            } else {
                merged.push(primer.clone());
            }
        }

        let updated = tokio::task::spawn_blocking(move || {
            libregene_core::primer::align::recompute_all_primers(&template, &topology, &merged)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        let wrote = {
            let mut pm = state.pm.write().await;
            match pm.get_project_mut_by_id(&project_id) {
                Some(p) if primers_equal(&p.primers, &existing) => {
                    p.primers = updated;
                    pm.mark_dirty(&project_id);
                    true
                }
                // The list changed under us (another window added a primer):
                // re-merge onto the fresh state instead of overwriting.
                Some(_) => false,
                None => return Ok(serde_json::json!({"error": "Project not found"})),
            }
        };
        if wrote {
            break;
        }
    }

    // Broadcast event so listeners update their state
    broadcast_project(&app_handle, &state, Some(webview_window.label())).await;

    let pm = state.pm.read().await;
    match pm.get_project_by_id(&project_id) {
        Some(p) => {
            let params = ProjectParams {
                enzyme_filter: Some("all".to_string()),
                row_start: None,
                row_end: None,
                cpl: None,
            };
            Ok(filter_project(p, &params))
        }
        None => Ok(serde_json::json!({"error": "Project not found"})),
    }
}

#[tauri::command]
pub(crate) async fn compute_primer_alignment(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    primer_id: Option<String>,
    seed_length: Option<usize>,
    custom_seq: Option<String>,
    custom_name: Option<String>,
    na_conc: Option<f64>,
    mg_conc: Option<f64>,
    dntp_conc: Option<f64>,
    tris_conc: Option<f64>,
    primer_conc: Option<f64>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await?;

    // Clone all needed data while holding the read lock, then drop it before spawn_blocking.
    let (template, topology, existing_primers) = {
        let pm = state.pm.read().await;
        let project = pm.get_project_by_id(&project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        (project.sequence.clone(), project.topology.clone(), project.primers.clone())
    };

    // Resolve primer outside the lock.
    let (primer_name, primer_seq) = match &primer_id {
        Some(pid) => {
            let existing = existing_primers.iter()
                .find(|p| p.id == *pid)
                .ok_or_else(|| "Primer not found".to_string())?;
            let seq = custom_seq.clone().unwrap_or_else(|| existing.primer_seq.clone());
            (existing.name.clone(), seq)
        }
        None => {
            let name = custom_name.clone().unwrap_or_else(|| "New Primer".to_string());
            let seq = custom_seq.clone().ok_or_else(|| "Sequence required for new primer alignment".to_string())?;
            (name, seq)
        }
    };

    let is_circular = topology == "circular";

    let tm_params = libregene_core::primer::thermodynamics::TmParams {
        na_conc: na_conc.unwrap_or(0.050),
        mg_conc: mg_conc.unwrap_or(0.0),
        dntp_conc: dntp_conc.unwrap_or(0.0),
        tris_conc: tris_conc.unwrap_or(0.0),
        primer_conc: primer_conc.unwrap_or(2.5e-7),
    };

    // Move heavy computation to blocking thread pool.
    tokio::task::spawn_blocking(move || {
        compute_primer_alignment_sync(
            &template, is_circular, &primer_name, &primer_seq, seed_length, &tm_params,
        )
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?
}

pub(crate) fn compute_primer_alignment_sync(
    template: &str,
    is_circular: bool,
    primer_name: &str,
    primer_seq: &str,
    seed_length: Option<usize>,
    tm_params: &libregene_core::primer::thermodynamics::TmParams,
) -> Result<serde_json::Value, String> {
    let tpl_bytes = template.as_bytes();
    let tlen = tpl_bytes.len();
    let active_upper = primer_seq.to_ascii_uppercase();
    let primer_bytes = active_upper.as_bytes();
    let orig_bytes = primer_seq.as_bytes();
    let plen = primer_bytes.len();

    if plen < 6 {
        return Err(format!("Primer too short ({}bp < 6bp seed)", plen));
    }
    let seed_len = seed_length.unwrap_or(10).clamp(6, plen.min(20));
    let expansion: usize = 60;

    if plen < seed_len {
        return Err(format!("Primer too short ({}bp < {}bp seed)", plen, seed_len));
    }
    if tlen < seed_len {
        return Err(format!(
            "Template too short ({}bp < {}bp seed)",
            tlen, seed_len
        ));
    }

    let rev_bytes: Vec<u8> = primer_bytes.iter().rev().copied().collect();
    let orig_rev_bytes: Vec<u8> = orig_bytes.iter().rev().copied().collect();
    let rc_seed: Vec<u8> = primer_bytes[plen - seed_len..].iter()
        .rev()
        .map(|&b| libregene_core::utils::complement_char(b as char) as u8)
        .collect();

    let search_len = if is_circular { tlen + seed_len } else { tlen };
    let extended: Vec<u8> = if is_circular {
        [tpl_bytes, tpl_bytes].concat()
    } else {
        tpl_bytes.to_vec()
    };

    let mut candidates: Vec<BindingSiteCandidate> = Vec::new();

    for mode in &[SearchMode::Forward, SearchMode::Reverse] {
        let (needle, is_rev) = match mode {
            SearchMode::Forward => (&primer_bytes[plen - seed_len..], false),
            SearchMode::Reverse => (&rc_seed[..], true),
        };

        for i in 0..=search_len.saturating_sub(seed_len) {
            if &extended[i..i + seed_len] != needle {
                continue;
            }

            let seed_tstart = if is_circular { i % tlen } else { i };
            let tp_3prime = if is_rev { seed_tstart } else { seed_tstart + seed_len - 1 };

            if candidates.iter().any(|c| {
                let d = c.tp_3prime.abs_diff(tp_3prime);
                d <= 3
            }) { continue; }

            let mut ext = 0usize;
            // On circular templates the footprint may wrap the origin; cap the
            // extension so it cannot run into its own seed.
            let max_ext = if is_circular {
                plen.saturating_sub(seed_len).min(tlen.saturating_sub(seed_len))
            } else {
                plen.saturating_sub(seed_len)
            };
            while ext < max_ext {
                let p_pos = plen - seed_len - ext - 1;
                let t_pos = if is_rev {
                    let raw = seed_tstart + seed_len + ext;
                    if is_circular { raw % tlen } else { raw }
                } else if is_circular {
                    (seed_tstart + tlen - 1 - (ext % tlen)) % tlen
                } else {
                    seed_tstart.checked_sub(ext + 1).unwrap_or(usize::MAX)
                };
                if t_pos >= tlen { break; }
                let ok = if is_rev {
                    libregene_core::primer::iupac::bases_pair(primer_bytes[p_pos], tpl_bytes[t_pos])
                } else {
                    libregene_core::primer::iupac::bases_overlap(primer_bytes[p_pos], tpl_bytes[t_pos])
                };
                if ok { ext += 1; } else { break; }
            }

            let footprint_len = seed_len + ext;

            let footprint_seq: String = primer_bytes[plen - footprint_len..]
                .iter().map(|&b| b.to_ascii_uppercase() as char).collect();
            let est_tm = if footprint_seq.len() >= 2 {
                libregene_core::primer::thermodynamics::compute_tm_with_params(&footprint_seq, tm_params)
            } else { 0.0 };

            candidates.push(BindingSiteCandidate { is_rev, tp_3prime, footprint_len, est_tm });
        }
    }

    candidates.sort_by(|a, b| b.est_tm.partial_cmp(&a.est_tm).unwrap_or(std::cmp::Ordering::Equal));
    candidates.dedup_by(|a, b| (a.tp_3prime as i64 - b.tp_3prime as i64).unsigned_abs() <= 3);

    if candidates.is_empty() {
        return Err("No candidate binding sites found".to_string());
    }

    let mut results = Vec::new();

    for (idx, c) in candidates.iter().enumerate() {
        let tp = c.tp_3prime;
        let raw_start = (tp as i64) - (expansion as i64) - (plen as i64) + seed_len as i64;
        let win_start = if is_circular {
            (raw_start.rem_euclid(tlen as i64)) as usize
        } else {
            raw_start.max(0) as usize
        };
        // Reverse primers extend to the right of tp_3prime (up to plen bases),
        // so the window must cover that side too, or the alignment display
        // runs out of template.
        let win_end = if is_circular {
            tp + expansion + plen
        } else {
            (tp + expansion + plen).min(tlen)
        };

        let template_region = if is_circular {
            libregene_core::primer::alignment::wrap_template_region(tpl_bytes, win_start, win_end)
        } else {
            tpl_bytes[win_start..win_end].to_vec()
        };

        if idx == 0 {
            let sw_ok = if c.is_rev {
                libregene_core::primer::alignment::align_first_base_constrained_rev(
                    &rev_bytes, &template_region,
                ).map(|result| {
                    let text = libregene_core::primer::display::format_alignment_text(
                        &orig_rev_bytes, &template_region, &result,
                        "Template", primer_name, win_start, true,
                        tlen, is_circular,
                    );
                    let sw_tm = libregene_core::primer::display::compute_tm_from_alignment_with_params(&rev_bytes, &result, tm_params);
                    results.push(serde_json::json!({
                        "tm": (sw_tm * 10.0).round() / 10.0,
                        "strand": -1,
                        "start": (result.template_start + win_start) as i64,
                        "end": (result.template_end + win_start) as i64,
                        "alignment": text,
                    }));
                })
            } else {
                libregene_core::primer::alignment::align_3prime_constrained(
                    primer_bytes, &template_region,
                ).map(|result| {
                    let text = libregene_core::primer::display::format_alignment_text(
                        orig_bytes, &template_region, &result,
                        "Template", primer_name, win_start, false,
                        tlen, is_circular,
                    );
                    let sw_tm = libregene_core::primer::display::compute_tm_from_alignment_with_params(primer_bytes, &result, tm_params);
                    results.push(serde_json::json!({
                        "tm": (sw_tm * 10.0).round() / 10.0,
                        "strand": 1,
                        "start": (result.template_start + win_start) as i64,
                        "end": (result.template_end + win_start) as i64,
                        "alignment": text,
                    }));
                })
            };
            if sw_ok.is_none() {
                results.push(fallback_site_json(c, tp));
            }
        } else {
            results.push(fallback_site_json(c, tp));
        }
    }

    let current = results.remove(0);
    Ok(serde_json::json!({ "current": current, "alternatives": results }))
}

/// Coordinates for a candidate when no SW alignment is available. Forward
/// footprints end at `tp_3prime`; reverse footprints start there.
fn fallback_site_json(c: &BindingSiteCandidate, tp: usize) -> serde_json::Value {
    let (start, end) = if c.is_rev {
        (tp as i64, (tp + c.footprint_len) as i64)
    } else {
        ((tp + 1 - c.footprint_len) as i64, tp as i64 + 1)
    };
    serde_json::json!({
        "tm": (c.est_tm * 10.0).round() / 10.0,
        "strand": if c.is_rev { -1 } else { 1 },
        "start": start,
        "end": end,
        "alignment": null,
    })
}

enum SearchMode { Forward, Reverse }

struct BindingSiteCandidate {
    is_rev: bool,
    tp_3prime: usize,
    footprint_len: usize,
    est_tm: f64,
}

/// Generate primer design candidates for the active project's sequence.
/// `mode` is "amplify" | "oepcr" | "mutagenesis"; segments are { start, end }
/// 0-based inclusive. Tm is computed with the same TmParams defaults as the
/// `compute_tm` command.
#[tauri::command]
pub(crate) async fn design_primer_candidates(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    mode: String,
    seg: Option<Segment>,
    seg2: Option<Segment>,
    name: Option<String>,
    name1: Option<String>,
    name2: Option<String>,
    site_name: Option<String>,
    target_tm: f64,
    overlap_len: Option<usize>,
    arm_len: Option<usize>,
    mut_seq: Option<String>,
    na_conc: Option<f64>,
    mg_conc: Option<f64>,
    dntp_conc: Option<f64>,
    tris_conc: Option<f64>,
    primer_conc: Option<f64>,
) -> Result<Vec<libregene_core::primer::design::PrimerGroup>, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await?;

    do_design_primer_candidates(
        &state.pm,
        &project_id,
        mode,
        seg,
        seg2,
        name,
        name1,
        name2,
        site_name,
        target_tm,
        overlap_len,
        arm_len,
        mut_seq,
        None,
        None,
        na_conc,
        mg_conc,
        dntp_conc,
        tris_conc,
        primer_conc,
    )
    .await
}

/// Compute melting temperature using SantaLucia 2004 nearest-neighbour model.
#[tauri::command]
pub(crate) async fn compute_tm(
    seq: String,
    na_conc: Option<f64>,
    mg_conc: Option<f64>,
    dntp_conc: Option<f64>,
    tris_conc: Option<f64>,
    primer_conc: Option<f64>,
) -> Result<f64, String> {
    if seq.len() < 2 {
        return Ok(0.0);
    }
    let params = libregene_core::primer::thermodynamics::TmParams {
        na_conc: na_conc.unwrap_or(0.050),
        mg_conc: mg_conc.unwrap_or(0.0),
        dntp_conc: dntp_conc.unwrap_or(0.0),
        tris_conc: tris_conc.unwrap_or(0.0),
        primer_conc: primer_conc.unwrap_or(2.5e-7),
    };
    let tm = libregene_core::primer::thermodynamics::compute_tm_with_params(&seq, &params);
    Ok((tm * 10.0).round() / 10.0)
}
