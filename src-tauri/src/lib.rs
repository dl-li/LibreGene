use std::collections::HashMap;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::RwLock;

use geneie_core::enzyme;
use geneie_core::file_io;
use geneie_core::models::{Feature, Primer, ProjectData};
use geneie_core::primer;
use geneie_core::project::ProjectManager;

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

    let enzymes: Vec<&geneie_core::models::Enzyme> = if filter == "all" {
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
        let mut seen: HashMap<&str, Vec<&geneie_core::models::Enzyme>> = HashMap::new();
        for e in &project.enzymes {
            seen.entry(&e.name).or_default().push(e);
        }
        let mut unique: Vec<&geneie_core::models::Enzyme> = seen
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

/// Emit the current project state + project list as a Tauri event.
async fn broadcast_project(app_handle: &AppHandle, state: &State<'_, AppState>) {
    let pm = state.pm.read().await;
    if let Some(project) = pm.get_project() {
        let projects = pm.list_projects();
        let active_id = pm.active_id().map(|s| s.to_string());
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        let filtered = filter_project(project, &params);
        let payload = serde_json::json!({
            "type": "project",
            "data": filtered,
            "projects": projects,
            "activeId": active_id,
        });
        let _ = app_handle.emit("project-update", payload);
    }
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
            let pm = state.pm.read().await;
            match pm.get_project_by_id(&id) {
                Some(project) => match file_io::gbk::write_gbk(project, &save_path) {
                    Ok(()) => Ok(serde_json::json!({"status": "ok"})),
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
    feature: Feature,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    let (feats, projects, active_id) = {
        let mut pm = state.pm.write().await;
        let mut feats: Vec<Feature> = pm
            .get_project_by_id(&project_id)
            .map(|p| p.features.clone())
            .unwrap_or_default();
        if let Some(pos) = feats.iter().position(|f| f.id == feature.id) {
            feats[pos] = feature;
        } else {
            feats.push(feature);
        }
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.features = feats.clone();
        }
        (feats, pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    Ok(with_projects_list(
        serde_json::to_value(&feats).unwrap_or(serde_json::json!([])),
        &projects,
        active_id.as_deref(),
    ))
}

#[tauri::command]
async fn delete_feature(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
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
        (feats, pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    Ok(with_projects_list(
        serde_json::to_value(&feats).unwrap_or(serde_json::json!([])),
        &projects,
        active_id.as_deref(),
    ))
}

#[tauri::command]
async fn update_feature_ftype(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
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
        pm.update_feature_ftype(&feature_id, &new_ftype);
    }

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
    primer: Primer,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    let (primers, projects, active_id) = {
        let mut pm = state.pm.write().await;
        let mut primers: Vec<Primer> = pm
            .get_project_by_id(&project_id)
            .map(|p| p.primers.clone())
            .unwrap_or_default();
        if let Some(pos) = primers.iter().position(|p| p.id == primer.id) {
            primers[pos] = primer;
        } else {
            primers.push(primer);
        }
        if let Some(p) = pm.get_project_by_id(&project_id) {
            let template = p.sequence.clone();
            let topology = p.topology.clone();
            let updated = geneie_core::primer::align::recompute_all_primers(&template, &topology, &primers);
            if let Some(p) = pm.get_project_mut_by_id(&project_id) {
                p.primers = updated.clone();
            }
        } else if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.primers = primers.clone();
        }
        (pm.get_project_by_id(&project_id).map(|p| p.primers.clone()).unwrap_or_default(), pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    Ok(with_projects_list(
        serde_json::to_value(&primers).unwrap_or(serde_json::json!([])),
        &projects,
        active_id.as_deref(),
    ))
}

#[tauri::command]
async fn delete_primer(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
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
        (primers, pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    Ok(with_projects_list(
        serde_json::to_value(&primers).unwrap_or(serde_json::json!([])),
        &projects,
        active_id.as_deref(),
    ))
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
    let projects = pm.list_projects();
    let active = pm.active_id().map(|s| s.to_string());
    Ok(serde_json::json!({
        "projects": projects,
        "activeId": active,
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
    _app_handle: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
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
    let _window = WebviewWindowBuilder::new(
        &app_handle,
        &window_label,
        WebviewUrl::App("index.html".into()),
    )
    .title("Geneie - Plasmid Editor")
    .inner_size(1400.0, 900.0)
    .build()
    .map_err(|e| format!("failed to create window: {e}"))?;

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
            get_primers,
            add_primer,
            delete_primer,
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
