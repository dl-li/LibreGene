use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::RwLock;

use libregene_core::enzyme;
use libregene_core::file_io;
use libregene_core::models::{Feature, Primer, ProjectData};
use libregene_core::primer;
use libregene_core::project::ProjectManager;

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

pub struct AppState {
    pub pm: Arc<RwLock<ProjectManager>>,
    /// Maps window labels to project IDs for multi-window support.
    /// Main window ("main") is NOT in this map — it uses the active project.
    /// Project windows ("project-{sanitized_id}") are mapped to their project.
    pub window_projects: Arc<RwLock<HashMap<String, String>>>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ProjectParams {
    enzyme_filter: Option<String>,
    row_start: Option<i64>,
    row_end: Option<i64>,
    cpl: Option<i64>,
}

const DEFAULT_CPL: i64 = 60;

/// Resolve the project_id for a given window:
/// - Project windows look up their mapping in `window_projects`
/// - The main window falls back to `ProjectManager::active_id()`
async fn resolve_project_id(state: &State<'_, AppState>, window_label: &str) -> Option<String> {
    // Check per-window mapping first
    let wp = state.window_projects.read().await;
    if let Some(pid) = wp.get(window_label) {
        return Some(pid.clone());
    }
    drop(wp);
    // Fall back to active project (main window)
    let pm = state.pm.read().await;
    pm.active_id().map(|s| s.to_string())
}

/// Returns the set of project IDs that are currently open in dedicated project windows.
/// These projects should be hidden from the main window's sidebar.
fn excluded_project_ids(wp: &tokio::sync::RwLockReadGuard<HashMap<String, String>>) -> HashSet<String> {
    wp.values().cloned().collect()
}

/// Filter out projects that are open in project windows, and adjust the activeId
/// if it points to an excluded project.
fn filter_main_window_projects(
    projects: Vec<serde_json::Value>,
    excluded: &HashSet<String>,
    active_id: Option<String>,
) -> (Vec<serde_json::Value>, Option<String>) {
    let filtered: Vec<_> = projects
        .into_iter()
        .filter(|p| p["id"].as_str().map_or(true, |id| !excluded.contains(id)))
        .collect();
    let active = active_id.filter(|id| !excluded.contains(id.as_str()))
        .or_else(|| {
            filtered.first()
                .and_then(|p| p["id"].as_str().map(String::from))
        });
    (filtered, active)
}

/// Inject projects list and activeId into a JSON response so the main
/// window sidebar stays in sync after any mutation or switch.
fn with_projects_list(
    mut data: serde_json::Value,
    projects: &[serde_json::Value],
    active_id: Option<&str>,
) -> serde_json::Value {
    if let Some(ref mut map) = data.as_object_mut() {
        map.insert(
            "projects".to_string(),
            serde_json::to_value(projects).unwrap_or_default(),
        );
        map.insert(
            "activeId".to_string(),
            serde_json::to_value(active_id).unwrap_or_default(),
        );
    }
    data
}

/// Filter enzymes according to the same logic as routes.rs::filter_project.
fn filter_project(project: &ProjectData, params: &ProjectParams) -> serde_json::Value {
    let filter = params.enzyme_filter.as_deref().unwrap_or("unique");
    let cpl = params.cpl.unwrap_or(DEFAULT_CPL);
    let row_start = params.row_start.map(|rs| rs.max(0));
    let row_end = params.row_end.map(|re| re.max(0));

    let enzymes: Vec<&libregene_core::models::Enzyme> = if filter == "all" {
        let all: Vec<_> = project.enzymes.iter().collect();
        if let (Some(rs), Some(re)) = (row_start, row_end) {
            let idx_s = rs * cpl;
            let idx_e = (re + 1) * cpl - 1;
            all.into_iter()
                .filter(|e| e.cut_index >= idx_s && e.cut_index <= idx_e)
                .collect()
        } else {
            all
        }
    } else {
        let mut seen: HashMap<&str, Vec<&libregene_core::models::Enzyme>> = HashMap::new();
        for e in &project.enzymes {
            seen.entry(&e.name).or_default().push(e);
        }
        let mut unique: Vec<&libregene_core::models::Enzyme> = seen
            .into_values()
            .filter(|v| v.len() == 1)
            .map(|v| v[0])
            .collect();
        if let (Some(rs), Some(re)) = (row_start, row_end) {
            let idx_s = rs * cpl;
            let idx_e = (re + 1) * cpl - 1;
            unique.retain(|e| e.cut_index >= idx_s && e.cut_index <= idx_e);
        }
        unique.sort_by_key(|e| e.cut_index);
        unique
    };

    // Build JSON directly — avoid serializing full enzyme list just to overwrite it
    serde_json::json!({
        "sequence": &project.sequence,
        "length": project.length,
        "topology": &project.topology,
        "features": &project.features,
        "primers": &project.primers,
        "methylation_systems": &project.methylation_systems,
        "methylation_overlap": project.methylation_overlap,
        "roi": &project.roi,
        "enzymeCount": project.enzymes.len(),
        "enzymeFilter": filter,
        "enzymes": &enzymes,
    })
}

/// Emit the current project state + filtered project list as a Tauri event.
async fn broadcast_project(app_handle: &AppHandle, state: &State<'_, AppState>) {
    let pm = state.pm.read().await;
    let wp = state.window_projects.read().await;
    let excluded = excluded_project_ids(&wp);
    let all_projects = pm.list_projects();
    let raw_active_id = pm.active_id().map(|s| s.to_string());
    let (filtered_projects, active_id) = filter_main_window_projects(all_projects, &excluded, raw_active_id);
    let mut payload = serde_json::json!({
        "projects": filtered_projects,
        "activeId": active_id,
    });
    // Include project data if there's an active project in the main window
    if let Some(ref active) = active_id {
        if let Some(project) = pm.get_project_by_id(active) {
            let params = ProjectParams {
                enzyme_filter: Some("all".to_string()),
                row_start: None,
                row_end: None,
                cpl: None,
            };
            let mut filtered = filter_project(project, &params);
            if let Some(ref mut map) = filtered.as_object_mut() {
                map.insert("dirty".to_string(), serde_json::json!(pm.is_dirty(active)));
            }
            payload["data"] = filtered;
        }
    }
    let _ = app_handle.emit("project-update", payload);
}

// ---------------------------------------------------------------------------
// Tauri commands — project
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_project(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    enzyme_filter: Option<String>,
    row_start: Option<i64>,
    row_end: Option<i64>,
    cpl: Option<i64>,
) -> Result<serde_json::Value, String> {
    let window_label = webview_window.label().to_string();
    let params = ProjectParams { enzyme_filter, row_start, row_end, cpl };

    // Resolve project_id for this window
    let project_id = resolve_project_id(&state, &window_label).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    let pm = state.pm.read().await;
    match pm.get_project_by_id(&project_id) {
        Some(p) => Ok(filter_project(p, &params)),
        None => Ok(serde_json::json!({"error": "Project not found"})),
    }
}

#[tauri::command]
async fn open_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<serde_json::Value, String> {
    let id = path.clone();
    let path_buf = std::path::PathBuf::from(&path);

    let result =
        tokio::task::spawn_blocking(move || -> Result<ProjectData, String> {
            let mut project = file_io::parse_file(&path_buf).map_err(|e| e.to_string())?;
            enzyme::recompute(&mut project);
            primer::recompute(&mut project);
            Ok(project)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

    match result {
        Ok(project) => {
            // Serialize before moving into pm.load() so the frontend can cache it
            let params = ProjectParams {
                enzyme_filter: Some("all".to_string()),
                row_start: None,
                row_end: None,
                cpl: None,
            };
            let return_data = filter_project(&project, &params);

            let (projects, active_id) = {
                let mut pm = state.pm.write().await;
                pm.load(&id, project);
                (pm.list_projects(), pm.active_id().map(|s| s.to_string()))
            };

            Ok(with_projects_list(return_data, &projects, active_id.as_deref()))
        }
        Err(e) => Ok(serde_json::json!({"error": e})),
    }
}

#[tauri::command]
async fn save_file(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    path: String,
) -> Result<serde_json::Value, String> {
    let save_path = std::path::PathBuf::from(&path);
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    match project_id {
        Some(id) => {
            let project;
            {
                let pm = state.pm.read().await;
                project = pm.get_project_by_id(&id).cloned();
            }
            match project {
                Some(ref p) => match file_io::gbk::write_gbk(p, &save_path) {
                    Ok(()) => {
                        // Mark project as clean after successful save
                        let mut pm = state.pm.write().await;
                        pm.mark_clean(&id);
                        Ok(serde_json::json!({"status": "ok"}))
                    }
                    Err(e) => Ok(serde_json::json!({"error": e.to_string()})),
                },
                None => Ok(serde_json::json!({"error": "Project not found"})),
            }
        }
        None => Ok(serde_json::json!({"error": "No project loaded"})),
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — sequence
// ---------------------------------------------------------------------------

#[tauri::command]
async fn update_sequence(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    sequence: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    // Update the sequence and trigger recompute
    {
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.sequence = sequence;
            p.length = p.sequence.len() as i64;
            pm.mark_dirty(&project_id);
        }
    }

    let project_clone = {
        let pm = state.pm.read().await;
        pm.get_project_by_id(&project_id).cloned()
    };

    if let Some(mut p) = project_clone {
        let computed = tokio::task::spawn_blocking(move || {
            enzyme::recompute(&mut p);
            primer::recompute(&mut p);
            p
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        let mut pm = state.pm.write().await;
        pm.open_project(project_id.clone(), computed);
    }

    // Return full project data + projects list
    let (result, projects, active_id) = {
        let pm = state.pm.read().await;
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        let data = pm
            .get_project_by_id(&project_id)
            .map(|p| filter_project(p, &params))
            .unwrap_or(serde_json::json!({"error": "Project not found after update"}));
        (data, pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    Ok(with_projects_list(result, &projects, active_id.as_deref()))
}

// ---------------------------------------------------------------------------
// Tauri commands — ROI
// ---------------------------------------------------------------------------

#[tauri::command]
async fn set_roi(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    start: i64,
    end: i64,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    match project_id {
        Some(id) => {
            let mut pm = state.pm.write().await;
            if let Some(p) = pm.get_project_mut_by_id(&id) {
                p.roi = Some((start, end));
                pm.mark_dirty(&id);
            }
            Ok(serde_json::json!({"status": "ok"}))
        }
        None => Ok(serde_json::json!({"error": "No project loaded"})),
    }
}

#[tauri::command]
async fn clear_roi(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    match project_id {
        Some(id) => {
            let mut pm = state.pm.write().await;
            if let Some(p) = pm.get_project_mut_by_id(&id) {
                p.roi = None;
                pm.mark_dirty(&id);
            }
            Ok(serde_json::json!({"status": "ok"}))
        }
        None => Ok(serde_json::json!({"error": "No project loaded"})),
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — features
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_features(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    match project_id {
        Some(id) => {
            let pm = state.pm.read().await;
            match pm.get_project_by_id(&id) {
                Some(p) => Ok(serde_json::to_value(&p.features).unwrap_or(serde_json::json!([]))),
                None => Ok(serde_json::json!([])),
            }
        }
        None => Ok(serde_json::json!([])),
    }
}

#[tauri::command]
async fn add_feature(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature: Feature,
    location_str: Option<String>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    {
        let mut pm = state.pm.write().await;
        let mut feats: Vec<Feature> = pm
            .get_project_by_id(&project_id)
            .map(|p| p.features.clone())
            .unwrap_or_default();

        // If location_str is provided, parse and validate it
        let mut resolved = feature;
        if let Some(loc_str) = location_str {
            let trimmed = loc_str.trim().to_string();
            if trimmed.is_empty() {
                return Ok(serde_json::json!({"error": "Location cannot be empty".to_string()}));
            }
            let parsed = libregene_core::file_io::gbk::parse_location_string(&trimmed)
                .ok_or_else(|| format!("Invalid location: {}", trimmed))?;
            let (segments, start, end, strand) = parsed;
            resolved.segments = segments;
            resolved.start = start;
            resolved.end = end;
            resolved.strand = strand;
        }

        if let Some(pos) = feats.iter().position(|f| f.id == resolved.id) {
            feats[pos] = resolved;
        } else {
            feats.push(resolved);
        }
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.features = feats;
        }
        pm.mark_dirty(&project_id);
    }

    // Broadcast event so listeners update their state
    broadcast_project(&app_handle, &state).await;

    // Return updated project
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
async fn delete_feature(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    id: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    let (feats, projects, active_id) = {
        let mut pm = state.pm.write().await;
        let feats: Vec<Feature> = pm
            .get_project_by_id(&project_id)
            .map(|p| p.features.iter().filter(|f| f.id != id).cloned().collect())
            .unwrap_or_default();
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.features = feats.clone();
        }
        pm.mark_dirty(&project_id);
        (feats, pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    // Broadcast event so listeners update their state
    broadcast_project(&app_handle, &state).await;

    Ok(with_projects_list(
        serde_json::json!({ "features": feats }),
        &projects,
        active_id.as_deref(),
    ))
}

#[tauri::command]
async fn update_feature_ftype(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    new_ftype: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    {
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            if let Some(f) = p.features.iter_mut().find(|f| f.id == feature_id) {
                f.ftype = new_ftype;
            }
            pm.mark_dirty(&project_id);
        }
    }

    // Broadcast event so listeners update their state
    broadcast_project(&app_handle, &state).await;

    // Return updated project
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

// ---------------------------------------------------------------------------
// Tauri commands — feature color
// ---------------------------------------------------------------------------

#[tauri::command]
async fn update_feature_color(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    new_color: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    {
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            if let Some(f) = p.features.iter_mut().find(|f| f.id == feature_id) {
                f.color = new_color;
            }
            pm.mark_dirty(&project_id);
        }
    }

    // Broadcast event so listeners update their state
    broadcast_project(&app_handle, &state).await;

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

// ---------------------------------------------------------------------------
// Tauri commands — feature name
// ---------------------------------------------------------------------------

#[tauri::command]
async fn update_feature_name(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    new_name: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    {
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            if let Some(f) = p.features.iter_mut().find(|f| f.id == feature_id) {
                f.name = new_name;
            }
            pm.mark_dirty(&project_id);
        }
    }

    // Broadcast event so listeners update their state
    broadcast_project(&app_handle, &state).await;

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

// ---------------------------------------------------------------------------
// Tauri commands — feature strand
// ---------------------------------------------------------------------------

#[tauri::command]
async fn update_feature_strand(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    strand: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    let valid = strand == "." || strand == "+" || strand == "-";
    if !valid {
        return Ok(serde_json::json!({"error": "Invalid strand: must be ., +, or -".to_string()}));
    }

    {
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            if let Some(f) = p.features.iter_mut().find(|f| f.id == feature_id) {
                f.strand = strand;
            }
            pm.mark_dirty(&project_id);
        }
    }

    broadcast_project(&app_handle, &state).await;

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

// ---------------------------------------------------------------------------
// Tauri commands — feature location
// ---------------------------------------------------------------------------

#[tauri::command]
async fn update_feature_location(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    location_str: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    {
        let mut pm = state.pm.write().await;
        let parsed = libregene_core::file_io::gbk::parse_location_string(&location_str)
            .ok_or_else(|| format!("Invalid location: {}", location_str))?;
        let (segments, start, end, strand) = parsed;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            if let Some(f) = p.features.iter_mut().find(|f| f.id == feature_id) {
                f.segments = segments;
                f.start = start;
                f.end = end;
                f.strand = strand;
            }
            pm.mark_dirty(&project_id);
        }
    }

    // Broadcast event so listeners update their state
    broadcast_project(&app_handle, &state).await;

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

// ---------------------------------------------------------------------------
// Tauri commands — primers
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_primers(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    match project_id {
        Some(id) => {
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
        None => Ok(serde_json::json!([])),
    }
}

#[tauri::command]
async fn add_primer(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    primer: Primer,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    {
        let mut pm = state.pm.write().await;
        let existing_primers: Vec<Primer> = pm
            .get_project_by_id(&project_id)
            .map(|p| p.primers.clone())
            .unwrap_or_default();
        // Reject duplicate names (same name, different id)
        let name_conflict = existing_primers.iter().any(|p| {
            p.id != primer.id && p.name == primer.name
        });
        if name_conflict {
            return Ok(serde_json::json!({"error": format!("Primer name '{}' already exists", primer.name)}));
        }
        let mut primers = existing_primers;
        if let Some(pos) = primers.iter().position(|p| p.id == primer.id) {
            primers[pos] = primer;
        } else {
            primers.push(primer);
        }
        if let Some(p) = pm.get_project_by_id(&project_id) {
            let template = p.sequence.clone();
            let topology = p.topology.clone();
            let updated = libregene_core::primer::align::recompute_all_primers(&template, &topology, &primers);
            if let Some(p) = pm.get_project_mut_by_id(&project_id) {
                p.primers = updated;
            }
        } else if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.primers = primers;
        }
        pm.mark_dirty(&project_id);
    }

    // Broadcast event so listeners update their state
    broadcast_project(&app_handle, &state).await;

    // Return updated project
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
async fn delete_primer(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    id: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    let (primers, projects, active_id) = {
        let mut pm = state.pm.write().await;
        let primers: Vec<Primer> = pm
            .get_project_by_id(&project_id)
            .map(|p| p.primers.iter().filter(|pr| pr.id != id).cloned().collect())
            .unwrap_or_default();
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.primers = primers.clone();
        }
        pm.mark_dirty(&project_id);
        (primers, pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    // Broadcast event so listeners update their state
    broadcast_project(&app_handle, &state).await;

    Ok(with_projects_list(
        serde_json::json!({ "primers": primers }),
        &projects,
        active_id.as_deref(),
    ))
}

#[tauri::command]
async fn compute_primer_alignment(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    primer_id: Option<String>,
    seed_length: Option<usize>,
    custom_seq: Option<String>,
    custom_name: Option<String>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await
        .ok_or_else(|| "No project loaded".to_string())?;

    let pm = state.pm.read().await;
    let project = pm.get_project_by_id(&project_id)
        .ok_or_else(|| "Project not found".to_string())?;

    // Resolve primer: if primer_id is set, look up existing primer; otherwise use custom data.
    let primer_name: String;
    let primer_seq: String;
    match &primer_id {
        Some(pid) => {
            let existing = project.primers.iter()
                .find(|p| p.id == *pid)
                .ok_or_else(|| "Primer not found".to_string())?;
            primer_name = existing.name.clone();
            primer_seq = custom_seq.clone().unwrap_or_else(|| existing.primer_seq.clone());
        }
        None => {
            primer_name = custom_name.clone().unwrap_or_else(|| "New Primer".to_string());
            primer_seq = custom_seq.clone().ok_or_else(|| "Sequence required for new primer alignment".to_string())?;
        }
    }

    let template = &project.sequence;
    let tpl_bytes = template.as_bytes();
    let tlen = tpl_bytes.len();
    let is_circular = project.topology == "circular";
    let active_seq = primer_seq;
    let orig_bytes = active_seq.as_bytes();              // original case for display
    let active_upper = active_seq.to_ascii_uppercase();
    let primer_bytes = active_upper.as_bytes();           // uppercase for matching
    let plen = primer_bytes.len();

    let seed_len = seed_length.unwrap_or(10).clamp(6, plen.min(20));
    let expansion: usize = 60;

    if plen < seed_len {
        return Err(format!("Primer too short ({}bp < {}bp seed)", plen, seed_len));
    }

    // Pre-compute reverse primer and RC seed (for R-mode search).
    let seed = &primer_bytes[plen - seed_len..];
    let rev_bytes: Vec<u8> = primer_bytes.iter().rev().copied().collect();
    let orig_rev_bytes: Vec<u8> = orig_bytes.iter().rev().copied().collect(); // for display
    // RC of the 3' seed — this is what we search for in R mode.
    let rc_seed: Vec<u8> = seed.iter()
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

    // Search BOTH exact seed (F mode) and RC seed (R mode).
    for mode in &[SearchMode::Forward, SearchMode::Reverse] {
        let (needle, is_rev) = match mode {
            SearchMode::Forward => (&primer_bytes[plen - seed_len..], false),
            SearchMode::Reverse => (&rc_seed[..], true),
        };

        for i in 0..=search_len.saturating_sub(seed_len) {
            if &extended[i..i + seed_len] != needle {
                continue;
            }

            // For F mode: 3' end is the LAST base of the seed match on template.
            // For R mode: 3' end is the FIRST base of the RC-seed match on template.
            let seed_tstart = if is_circular { i % tlen } else { i };
            let tp_3prime = if is_rev { seed_tstart } else { seed_tstart + seed_len - 1 };

            if candidates.iter().any(|c| {
                let d = if c.tp_3prime > tp_3prime { c.tp_3prime - tp_3prime } else { tp_3prime - c.tp_3prime };
                d <= 3
            }) { continue; }

            // Greedy 5'-ward extension.
            // F: extend LEFT  on template (seed_start-1, seed_start-2, …)
            // R: extend RIGHT on template (seed_start+seed_len, seed_start+seed_len+1, …)
            let mut ext = 0usize;
            let max_ext = plen.saturating_sub(seed_len);
            while ext < max_ext {
                let p_pos = plen - seed_len - ext - 1;
                let t_pos = if is_rev {
                    seed_tstart + seed_len + ext
                } else {
                    seed_tstart.checked_sub(ext + 1).unwrap_or(usize::MAX)
                };
                if t_pos >= tlen { break; }
                let ok = if is_rev {
                    libregene_core::primer::iupac::bases_pair(primer_bytes[p_pos], tpl_bytes[t_pos])
                } else {
                    primer_bytes[p_pos] == tpl_bytes[t_pos]
                };
                if ok { ext += 1; } else { break; }
            }

            let footprint_len = seed_len + ext;

            let footprint_seq: String = primer_bytes[plen - footprint_len..]
                .iter().map(|&b| b.to_ascii_uppercase() as char).collect();
            let est_tm = if footprint_seq.len() >= 2 {
                libregene_core::primer::thermodynamics::compute_tm(&footprint_seq)
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
        let win_end = if is_circular {
            (tp + expansion) % tlen
        } else {
            (tp + expansion).min(tlen)
        };

        let template_region = if is_circular {
            primer::alignment::wrap_template_region(tpl_bytes, win_start, win_end)
        } else {
            tpl_bytes[win_start..win_end].to_vec()
        };

        if idx == 0 {
            // Best site: full SW alignment.
            let sw_ok = if c.is_rev {
                primer::alignment::align_first_base_constrained_rev(
                    &rev_bytes, &template_region,
                ).map(|result| {
                    let text = primer::display::format_alignment_text(
                        &orig_rev_bytes, &template_region, &result,
                        "Template", &primer_name, win_start, true,
                    );
                    let sw_tm = primer::display::compute_tm_from_alignment(&rev_bytes, &result);
                    results.push(serde_json::json!({
                        "tm": (sw_tm * 10.0).round() / 10.0,
                        "strand": -1,
                        "start": (result.template_start + win_start) as i64,
                        "end": (result.template_end + win_start) as i64,
                        "alignment": text,
                    }));
                })
            } else {
                primer::alignment::align_3prime_constrained(
                    primer_bytes, &template_region,
                ).map(|result| {
                    let text = primer::display::format_alignment_text(
                        orig_bytes, &template_region, &result,
                        "Template", &primer_name, win_start, false,
                    );
                    let sw_tm = primer::display::compute_tm_from_alignment(primer_bytes, &result);
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
                results.push(serde_json::json!({
                    "tm": (c.est_tm * 10.0).round() / 10.0,
                    "strand": if c.is_rev { -1 } else { 1 },
                    "start": (tp - c.footprint_len + 1) as i64,
                    "end": tp as i64 + 1,
                    "alignment": null,
                }));
            }
        } else {
            // Alternative: just list position + Tm, no alignment.
            results.push(serde_json::json!({
                "tm": (c.est_tm * 10.0).round() / 10.0,
                "strand": if c.is_rev { -1 } else { 1 },
                "start": (tp - c.footprint_len + 1) as i64,
                "end": tp as i64 + 1,
                "alignment": null,
            }));
        }
    }

    if results.is_empty() {
        return Err("No valid binding sites found".to_string());
    }
    let current = results.remove(0);
    Ok(serde_json::json!({ "current": current, "alternatives": results }))
}

enum SearchMode { Forward, Reverse }

struct BindingSiteCandidate {
    is_rev: bool,
    tp_3prime: usize,
    footprint_len: usize,
    est_tm: f64,
}

// ---------------------------------------------------------------------------
// Tauri commands — methylation
// ---------------------------------------------------------------------------

#[tauri::command]
async fn set_methylation(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    systems: Vec<String>,
    overlap: Option<i64>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    let systems: Vec<String> = systems
        .into_iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let overlap = overlap.unwrap_or(2);

    let project_data = {
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.methylation_systems = systems;
            p.methylation_overlap = overlap;
            Some(p.clone())
        } else {
            None
        }
    };

    if let Some(mut p) = project_data {
        let computed = tokio::task::spawn_blocking(move || {
            enzyme::recompute_methylation_only(&mut p);
            p
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        let mut pm = state.pm.write().await;
        pm.open_project(project_id.clone(), computed);
        pm.mark_dirty(&project_id);
    }

    // Return full project data + projects list
    let (result, projects, active_id) = {
        let pm = state.pm.read().await;
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        let data = pm
            .get_project_by_id(&project_id)
            .map(|p| filter_project(p, &params))
            .unwrap_or(serde_json::json!({"error": "Project not found after methylation"}));
        (data, pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    Ok(with_projects_list(result, &projects, active_id.as_deref()))
}

// ---------------------------------------------------------------------------
// Tauri commands — multi-project management
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_projects(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let pm = state.pm.read().await;
    let wp = state.window_projects.read().await;
    let excluded = excluded_project_ids(&wp);
    let all_projects = pm.list_projects();
    let raw_active = pm.active_id().map(|s| s.to_string());
    let (projects, active_id) = filter_main_window_projects(all_projects, &excluded, raw_active);
    Ok(serde_json::json!({
        "projects": projects,
        "activeId": active_id,
    }))
}

#[tauri::command]
async fn get_project_by_id(
    state: State<'_, AppState>,
    id: String,
    enzyme_filter: Option<String>,
) -> Result<serde_json::Value, String> {
    let pm = state.pm.read().await;
    match pm.get_project_by_id(&id) {
        Some(p) => {
            let filter = enzyme_filter.as_deref().unwrap_or("all");
            let needs_all = ["blunt", "overhang5", "overhang3", "iis", "rec4", "rec5", "rec6", "rec8p"]
                .contains(&filter);
            let params = ProjectParams {
                enzyme_filter: Some(if needs_all || filter == "all" { "all" } else { "unique" }.to_string()),
                row_start: None,
                row_end: None,
                cpl: None,
            };
            Ok(filter_project(p, &params))
        }
        None => Ok(serde_json::json!({"error": "project not found"})),
    }
}

#[tauri::command]
async fn activate_project(
    webview_window: tauri::WebviewWindow,
    _app_handle: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    // Project windows must not change the global active project —
    // they are bound to a single project via window_projects mapping.
    {
        let wp = state.window_projects.read().await;
        if wp.contains_key(webview_window.label()) {
            return Ok(serde_json::json!({"error": "Project windows cannot change the active project"}));
        }
    }

    let activated = {
        let mut pm = state.pm.write().await;
        pm.activate_project(&id)
    };
    if !activated {
        return Ok(serde_json::json!({"error": format!("project not found: {}", id)}));
    }

    // Return full project data; no separate broadcast needed (single-window app)
    let result = {
        let pm = state.pm.read().await;
        match pm.get_project() {
            Some(p) => {
                let projects = pm.list_projects();
                let active_id = pm.active_id().map(|s| s.to_string());
                let params = ProjectParams {
                    enzyme_filter: Some("all".to_string()),
                    row_start: None,
                    row_end: None,
                    cpl: None,
                };
                let mut filtered = filter_project(p, &params);
                if let Some(ref mut map) = filtered.as_object_mut() {
                    map.insert(
                        "projects".to_string(),
                        serde_json::to_value(&projects).unwrap_or_default(),
                    );
                    map.insert(
                        "activeId".to_string(),
                        serde_json::to_value(&active_id).unwrap_or_default(),
                    );
                }
                filtered
            }
            None => serde_json::json!({"error": "project not found"}),
        }
    };

    Ok(result)
}

#[tauri::command]
async fn delete_project(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let closed = {
        let mut pm = state.pm.write().await;
        pm.close_project(&id)
    };
    if closed {
        // Clean up any project window mappings for this project
        {
            let mut wp = state.window_projects.write().await;
            wp.retain(|_, v| v != &id);
        }
        broadcast_project(&app_handle, &state).await;
        Ok(serde_json::json!({"status": "ok"}))
    } else {
        Ok(serde_json::json!({"error": "project not found"}))
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — multi-window
// ---------------------------------------------------------------------------

/// Open the given project in a new OS window.
#[tauri::command]
async fn open_in_new_window(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    // Validate the project exists
    let exists = {
        let pm = state.pm.read().await;
        pm.get_project_by_id(&project_id).is_some()
    };
    if !exists {
        return Ok(serde_json::json!({"error": "Project not found"}));
    }

    // Build a safe label for the new window (append timestamp for uniqueness)
    let safe = project_id.replace(['/', '\\', ':', '.', ' '], "_");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let window_label = format!("project-{safe}-{ts}");

    // Register the window → project mapping
    {
        let mut wp = state.window_projects.write().await;
        wp.insert(window_label.clone(), project_id);
    }

    // Create the new window
    let window = WebviewWindowBuilder::new(
        &app_handle,
        &window_label,
        WebviewUrl::App("index.html".into()),
    )
    .title("LibreGene - Plasmid Editor")
    .inner_size(1400.0, 900.0)
    .build()
    .map_err(|e| format!("failed to create window: {e}"))?;

    // When the project window is destroyed, restore the project to the main window
    let ah = app_handle.clone();
    let lbl = window_label.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            let ah = ah.clone();
            let lbl = lbl.clone();
            tauri::async_runtime::spawn(async move {
                let state = ah.state::<AppState>();
                {
                    let mut wp = state.window_projects.write().await;
                    wp.remove(&lbl);
                }
                broadcast_project(&ah, &state).await;
            });
        }
    });

    // Broadcast so the main window updates its sidebar immediately
    broadcast_project(&app_handle, &state).await;

    Ok(serde_json::json!({"status": "ok", "windowLabel": window_label}))
}

/// Return the project_id bound to the calling window.
/// Returns null for the main window (use active project instead).
#[tauri::command]
async fn get_window_project_id(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let wp = state.window_projects.read().await;
    Ok(wp.get(webview_window.label()).cloned())
}

// ---------------------------------------------------------------------------
// App entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            pm: Arc::new(RwLock::new(ProjectManager::new())),
            window_projects: Arc::new(RwLock::new(HashMap::new())),
        })
        .invoke_handler(tauri::generate_handler![
            get_project,
            get_project_by_id,
            open_file,
            save_file,
            update_sequence,
            set_roi,
            clear_roi,
            get_features,
            add_feature,
            delete_feature,
            update_feature_ftype,
            update_feature_color,
            update_feature_name,
            update_feature_strand,
            update_feature_location,
            get_primers,
            add_primer,
            delete_primer,
            compute_primer_alignment,
            set_methylation,
            get_projects,
            activate_project,
            delete_project,
            open_in_new_window,
            get_window_project_id,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
