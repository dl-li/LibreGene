use std::collections::HashMap;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
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

    let mut base = serde_json::to_value(project).unwrap_or_default();
    if let Some(ref mut map) = base.as_object_mut() {
        map.insert(
            "enzymeCount".to_string(),
            serde_json::Value::Number(project.enzymes.len().into()),
        );
        map.insert(
            "enzymeFilter".to_string(),
            serde_json::Value::String(filter.to_string()),
        );
        map.insert(
            "enzymes".to_string(),
            serde_json::to_value(&enzymes).unwrap_or(serde_json::json!([])),
        );
    }
    base
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
    state: State<'_, AppState>,
    enzyme_filter: Option<String>,
    row_start: Option<i64>,
    row_end: Option<i64>,
    cpl: Option<i64>,
) -> Result<serde_json::Value, String> {
    let pm = state.pm.read().await;
    match pm.get_project() {
        Some(p) => {
            let params = ProjectParams {
                enzyme_filter,
                row_start,
                row_end,
                cpl,
            };
            Ok(filter_project(p, &params))
        }
        None => Ok(serde_json::json!({"error": "No project loaded"})),
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

            {
                let mut pm = state.pm.write().await;
                pm.load(&id, project);
            }

            Ok(return_data)
        }
        Err(e) => Ok(serde_json::json!({"error": e})),
    }
}

#[tauri::command]
async fn save_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<serde_json::Value, String> {
    let save_path = std::path::PathBuf::from(&path);
    let pm = state.pm.read().await;
    match pm.get_project() {
        Some(project) => match file_io::gbk::write_gbk(project, &save_path) {
            Ok(()) => Ok(serde_json::json!({"status": "ok"})),
            Err(e) => Ok(serde_json::json!({"error": e.to_string()})),
        },
        None => Ok(serde_json::json!({"error": "No project loaded"})),
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — sequence
// ---------------------------------------------------------------------------

#[tauri::command]
async fn update_sequence(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    sequence: String,
) -> Result<serde_json::Value, String> {
    let (project, active_id) = {
        let mut pm = state.pm.write().await;
        pm.update_sequence(sequence);
        (pm.get_project().cloned(), pm.active_id().map(|s| s.to_string()))
    };

    if let Some(mut p) = project {
        let computed =
            tokio::task::spawn_blocking(move || {
                enzyme::recompute(&mut p);
                primer::recompute(&mut p);
                p
            })
            .await
            .map_err(|e| format!("task join error: {}", e))?;

        let mut pm = state.pm.write().await;
        if let Some(ref id) = active_id {
            pm.open_project(id.clone(), computed);
        }
    }

    broadcast_project(&app_handle, &state).await;
    Ok(serde_json::json!({"status": "ok"}))
}

// ---------------------------------------------------------------------------
// Tauri commands — ROI
// ---------------------------------------------------------------------------

#[tauri::command]
async fn set_roi(
    state: State<'_, AppState>,
    start: i64,
    end: i64,
) -> Result<serde_json::Value, String> {
    let mut pm = state.pm.write().await;
    pm.set_roi(start, end);
    Ok(serde_json::json!({"status": "ok"}))
}

#[tauri::command]
async fn clear_roi(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let mut pm = state.pm.write().await;
    pm.clear_roi();
    Ok(serde_json::json!({"status": "ok"}))
}

// ---------------------------------------------------------------------------
// Tauri commands — features
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_features(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let pm = state.pm.read().await;
    match pm.get_project() {
        Some(p) => Ok(serde_json::to_value(&p.features).unwrap_or(serde_json::json!([]))),
        None => Ok(serde_json::json!([])),
    }
}

#[tauri::command]
async fn add_feature(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    feature: Feature,
) -> Result<serde_json::Value, String> {
    let mut pm = state.pm.write().await;
    let mut feats: Vec<Feature> = pm
        .get_project()
        .map(|p| p.features.clone())
        .unwrap_or_default();
    if let Some(pos) = feats.iter().position(|f| f.id == feature.id) {
        feats[pos] = feature;
    } else {
        feats.push(feature);
    }
    pm.update_features(feats);
    drop(pm);
    broadcast_project(&app_handle, &state).await;
    Ok(serde_json::json!({"status": "ok"}))
}

#[tauri::command]
async fn delete_feature(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let mut pm = state.pm.write().await;
    let feats: Vec<Feature> = pm
        .get_project()
        .map(|p| p.features.iter().filter(|f| f.id != id).cloned().collect())
        .unwrap_or_default();
    pm.update_features(feats);
    drop(pm);
    broadcast_project(&app_handle, &state).await;
    Ok(serde_json::json!({"status": "ok"}))
}

// ---------------------------------------------------------------------------
// Tauri commands — primers
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_primers(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let pm = state.pm.read().await;
    match pm.get_project() {
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

#[tauri::command]
async fn add_primer(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    primer: Primer,
) -> Result<serde_json::Value, String> {
    let mut pm = state.pm.write().await;
    let mut primers: Vec<Primer> = pm
        .get_project()
        .map(|p| p.primers.clone())
        .unwrap_or_default();
    if let Some(pos) = primers.iter().position(|p| p.id == primer.id) {
        primers[pos] = primer;
    } else {
        primers.push(primer);
    }
    if let Some(project) = pm.get_project() {
        let template = project.sequence.clone();
        let topology = project.topology.clone();
        let updated = geneie_core::primer::align::recompute_all_primers(&template, &topology, &primers);
        pm.update_primers(updated);
    } else {
        pm.update_primers(primers);
    }
    drop(pm);
    broadcast_project(&app_handle, &state).await;
    Ok(serde_json::json!({"status": "ok"}))
}

#[tauri::command]
async fn delete_primer(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let mut pm = state.pm.write().await;
    let primers: Vec<Primer> = pm
        .get_project()
        .map(|p| p.primers.iter().filter(|pr| pr.id != id).cloned().collect())
        .unwrap_or_default();
    pm.update_primers(primers);
    drop(pm);
    broadcast_project(&app_handle, &state).await;
    Ok(serde_json::json!({"status": "ok"}))
}

// ---------------------------------------------------------------------------
// Tauri commands — methylation
// ---------------------------------------------------------------------------

#[tauri::command]
async fn set_methylation(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    systems: Vec<String>,
    overlap: Option<i64>,
) -> Result<serde_json::Value, String> {
    let systems: Vec<String> = systems
        .into_iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let overlap = overlap.unwrap_or(2);

    let project_data = {
        let mut pm = state.pm.write().await;
        pm.set_methylation_systems(systems);
        if let Some(p) = pm.get_project_mut() {
            p.methylation_overlap = overlap;
        }
        pm.get_project().cloned()
    };

    if let Some(mut p) = project_data {
        let computed =
            tokio::task::spawn_blocking(move || {
                enzyme::recompute(&mut p);
                p
            })
            .await
            .map_err(|e| format!("task join error: {}", e))?;

        let mut pm = state.pm.write().await;
        if let Some(id) = pm.active_id().map(|s| s.to_string()) {
            pm.open_project(id, computed);
        }
    }

    broadcast_project(&app_handle, &state).await;
    Ok(serde_json::json!({"status": "ok"}))
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
    app_handle: AppHandle,
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

    // Return full project data immediately; broadcast async so it doesn't delay the response
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

    // Broadcast asynchronously to avoid delaying the response
    let pm_arc = state.pm.clone();
    let app_clone = app_handle.clone();
    tokio::spawn(async move {
        let pm = pm_arc.read().await;
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
            let _ = app_clone.emit("project-update", payload);
        }
    });

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
        broadcast_project(&app_handle, &state).await;
        Ok(serde_json::json!({"status": "ok"}))
    } else {
        Ok(serde_json::json!({"error": "project not found"}))
    }
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
            get_primers,
            add_primer,
            delete_primer,
            set_methylation,
            get_projects,
            activate_project,
            delete_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
