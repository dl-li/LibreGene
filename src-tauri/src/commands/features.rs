use tauri::{AppHandle, State};

use libregene_core::models::Feature;

use crate::kernels::{do_add_features, do_delete_feature, do_update_feature};
use crate::payload::resolve_project_id;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Tauri commands — features
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn get_features(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => {
            let pm = state.pm.read().await;
            match pm.get_project_by_id(&id) {
                Some(p) => Ok(serde_json::to_value(&p.features).unwrap_or(serde_json::json!([]))),
                None => Ok(serde_json::json!([])),
            }
        }
        Err(_) => Ok(serde_json::json!([])),
    }
}

#[tauri::command]
pub(crate) async fn add_feature(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature: Feature,
    location_str: Option<String>,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    // If location_str is provided, parse and validate it (0-based inclusive,
    // the App-wide convention, e.g. "99..199", "join(0..99,199..299)")
    let mut resolved = feature;
    if let Some(loc_str) = location_str {
        let trimmed = loc_str.trim().to_string();
        if trimmed.is_empty() {
            return Ok(serde_json::json!({"error": "Location cannot be empty".to_string()}));
        }
        let parsed = libregene_core::file_io::gbk::parse_location_string_0based(&trimmed)
            .ok_or_else(|| format!("Invalid location: {}", trimmed))?;
        let (segments, start, end, strand) = parsed;
        resolved.segments = segments;
        resolved.start = start;
        resolved.end = end;
        resolved.strand = strand;
    }

    do_add_features(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        vec![resolved],
    )
    .await
}

#[tauri::command]
pub(crate) async fn delete_feature(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    id: String,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_delete_feature(
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

#[tauri::command]
pub(crate) async fn update_feature_ftype(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    new_ftype: String,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        &feature_id,
        |f| {
            f.ftype = new_ftype;
            Ok(())
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Tauri commands — feature color
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn update_feature_color(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    new_color: String,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        &feature_id,
        |f| {
            f.color = new_color.clone();
            // Recolor every segment too so the whole feature changes at once.
            for seg in f.segments.iter_mut() {
                seg.color = Some(new_color.clone());
            }
            Ok(())
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Tauri commands — feature name
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn update_feature_name(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    new_name: String,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        &feature_id,
        |f| {
            f.name = new_name;
            Ok(())
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Tauri commands — feature strand
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn update_feature_strand(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    strand: String,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    let valid = strand == "." || strand == "+" || strand == "-";
    if !valid {
        return Ok(serde_json::json!({"error": "Invalid strand: must be ., +, or -".to_string()}));
    }

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        &feature_id,
        |f| {
            f.strand = strand;
            Ok(())
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Tauri commands — feature location
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn update_feature_location(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    location_str: String,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        &feature_id,
        |f| {
            // location_str is 0-based inclusive (App-wide convention).
            let parsed = libregene_core::file_io::gbk::parse_location_string_0based(&location_str)
                .ok_or_else(|| format!("Invalid location: {}", location_str))?;
            let (segments, start, end, strand) = parsed;
            f.segments = segments;
            f.start = start;
            f.end = end;
            f.strand = strand;
            Ok(())
        },
    )
    .await
}
