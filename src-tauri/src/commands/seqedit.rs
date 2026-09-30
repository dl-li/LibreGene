use tauri::{AppHandle, State};

use libregene_core::models::{Feature, Primer};

use crate::kernels::{do_set_methylation, do_set_topology, do_update_sequence};
use crate::payload::resolve_project_id;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Tauri commands — sequence
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn update_sequence(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    sequence: String,
    features: Option<Vec<Feature>>,
    primers: Option<Vec<Primer>>,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };
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
    do_update_sequence(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        project_id,
        sequence,
        primers,
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
    start: i64,
    end: i64,
) -> Result<serde_json::Value, String> {
    match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => {
            let mut pm = state.pm.write().await;
            if let Some(p) = pm.get_project_mut_by_id(&id) {
                p.roi = Some((start, end));
                pm.mark_dirty(&id);
            }
            Ok(serde_json::json!({"status": "ok"}))
        }
        Err(e) => Ok(serde_json::json!({"error": e})),
    }
}

#[tauri::command]
pub(crate) async fn clear_roi(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => {
            let mut pm = state.pm.write().await;
            if let Some(p) = pm.get_project_mut_by_id(&id) {
                p.roi = None;
                pm.mark_dirty(&id);
            }
            Ok(serde_json::json!({"status": "ok"}))
        }
        Err(e) => Ok(serde_json::json!({"error": e})),
    }
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
    project_id: Option<String>,
) -> Result<serde_json::Value, String> {
    // An explicit projectId wins: the frontend reads the active project, then
    // awaits this call — a project switch in between must not redirect the
    // mutation. Fall back to the window's project for older callers.
    let project_id = match project_id {
        Some(id) => id,
        None => match resolve_project_id(&state, webview_window.label()).await {
            Ok(id) => id,
            Err(e) => return Ok(serde_json::json!({"error": e})),
        },
    };

    do_set_methylation(
        &app_handle,
        &state.pm,
        &state.window_projects,
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
    project_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let project_id = match project_id {
        Some(id) => id,
        None => match resolve_project_id(&state, webview_window.label()).await {
            Ok(id) => id,
            Err(e) => return Ok(serde_json::json!({"error": e})),
        },
    };

    do_set_topology(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        &topology,
    )
    .await
}
