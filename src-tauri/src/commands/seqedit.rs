use tauri::{AppHandle, State};

use libregene_core::file_io::strand_colors;
use libregene_core::models::{Feature, Primer, TextColorRange};

use crate::kernels::{do_set_methylation, do_set_text_colors, do_set_topology, do_update_sequence};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Tauri commands — sequence
// ---------------------------------------------------------------------------

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn update_sequence(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    project_id: String,
    sequence: String,
    features: Option<Vec<Feature>>,
    primers: Option<Vec<Primer>>,
    text_colors_top: Option<Vec<TextColorRange>>,
    text_colors_bottom: Option<Vec<TextColorRange>>,
) -> Result<serde_json::Value, String> {
    if let Some(features) = features {
        // Feature coordinates arrive from the frontend (paste-merge, adjust)
        // and serde only checks types — clamp to the NEW sequence length so a
        // forged clipboard payload can't seed out-of-range spans that later
        // panic coordinate consumers. Mirrors MCP set_feature's bounds gate.
        let new_len = sequence.len() as i64;
        let mut features = features;
        features.retain(|f| {
            let (lo, hi) = if f.segments.is_empty() {
                (f.start, f.end)
            } else {
                (
                    f.segments.iter().map(|s| s.start).min().unwrap_or(f.start),
                    f.segments.iter().map(|s| s.end).max().unwrap_or(f.end),
                )
            };
            hi >= 0 && lo < new_len
        });
        // clamp(0, -1) panics, so floor the upper bound for empty sequences.
        let max_pos = (new_len - 1).max(0);
        for f in features.iter_mut() {
            f.start = f.start.clamp(0, max_pos);
            f.end = f.end.clamp(0, max_pos);
            for seg in f.segments.iter_mut() {
                seg.start = seg.start.clamp(0, max_pos);
                seg.end = seg.end.clamp(0, max_pos);
            }
        }
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.features = features;
        }
    }
    // Text colors arrive already adjusted by the frontend (same client-side
    // contract as features); canonicalize clamps them to the new length.
    if text_colors_top.is_some() || text_colors_bottom.is_some() {
        let new_len = sequence.len() as i64;
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            if let Some(top) = text_colors_top {
                p.text_colors_top = strand_colors::canonicalize(&top, new_len);
            }
            if let Some(bottom) = text_colors_bottom {
                p.text_colors_bottom = strand_colors::canonicalize(&bottom, new_len);
            }
        }
    }
    do_update_sequence(
        &app_handle,
        &state.pm,
        &state.agent_tabs,
        Some(webview_window.label()),
        project_id,
        sequence,
        primers,
    )
    .await
}

// ---------------------------------------------------------------------------
// Tauri commands — sequence text colors
// ---------------------------------------------------------------------------

/// Color the sequence text of `[start, end]` (0-based inclusive) on one
/// strand ("+" | "-" | "both"); an empty `color` clears that span.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn set_text_colors(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    project_id: String,
    strand: String,
    start: i64,
    end: i64,
    color: String,
) -> Result<serde_json::Value, String> {
    do_set_text_colors(
        &app_handle,
        &state.pm,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        &strand,
        start,
        end,
        &color,
    )
    .await
}

// ---------------------------------------------------------------------------
// Tauri commands — ROI
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn set_roi(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    project_id: String,
    start: i64,
    end: i64,
) -> Result<serde_json::Value, String> {
    {
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.roi = Some((start, end));
            pm.mark_dirty(&project_id);
        }
    }
    crate::payload::broadcast_project_for_id(
        &app_handle,
        &state,
        &project_id,
        Some(webview_window.label()),
    )
    .await;
    Ok(serde_json::json!({"status": "ok"}))
}

#[tauri::command]
pub(crate) async fn clear_roi(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    project_id: String,
) -> Result<serde_json::Value, String> {
    {
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.roi = None;
            pm.mark_dirty(&project_id);
        }
    }
    crate::payload::broadcast_project_for_id(
        &app_handle,
        &state,
        &project_id,
        Some(webview_window.label()),
    )
    .await;
    Ok(serde_json::json!({"status": "ok"}))
}

// ---------------------------------------------------------------------------
// Tauri commands — methylation
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn set_methylation(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    systems: Vec<String>,
    overlap: Option<i64>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    do_set_methylation(
        &app_handle,
        &state.pm,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        systems,
        overlap,
    )
    .await
}

#[tauri::command]
pub(crate) async fn set_topology(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    topology: String,
    project_id: String,
) -> Result<serde_json::Value, String> {
    do_set_topology(
        &app_handle,
        &state.pm,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        &topology,
    )
    .await
}
