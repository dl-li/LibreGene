use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, Runtime, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::RwLock;

use libregene_core::enzyme;
use libregene_core::file_io;
use libregene_core::models::{Feature, Primer, ProjectData, Segment};
use libregene_core::primer;
use libregene_core::project::ProjectManager;

mod mcp;

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

/// Build a lightweight mutation response for feature edits: the calling window
/// only needs the updated feature list and the sidebar project list. Avoids
/// serializing the full project (~1.5MB, mostly the enzyme list) on every Apply.
fn feature_mutation_response(pm: &ProjectManager, project_id: &str) -> serde_json::Value {
    let projects = pm.list_projects();
    let active_id = pm.active_id().map(|s| s.to_string());
    match pm.get_project_by_id(project_id) {
        Some(p) => with_projects_list(
            serde_json::json!({ "features": &p.features }),
            &projects,
            active_id.as_deref(),
        ),
        None => serde_json::json!({"error": "Project not found"}),
    }
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
        "alignments": &project.alignments,
        "methylation_systems": &project.methylation_systems,
        "methylation_overlap": project.methylation_overlap,
        "roi": &project.roi,
        "enzymeCount": project.enzymes.len(),
        "enzymeFilter": filter,
        "enzymes": &enzymes,
    })
}

/// Emit the current project state + filtered project list as a Tauri event.
async fn broadcast_project(app_handle: &AppHandle, state: &State<'_, AppState>, source: Option<&str>) {
    broadcast_project_arcs(
        app_handle,
        &state.pm,
        &state.window_projects,
        source,
    )
    .await;
}

/// Same as `broadcast_project` but takes the shared Arcs directly, so the MCP
/// server can broadcast without a `State` handle.
async fn broadcast_project_arcs<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    source: Option<&str>,
) {
    let pm = pm.read().await;
    let wp = wp.read().await;
    let excluded = excluded_project_ids(&wp);
    let all_projects = pm.list_projects();
    let raw_active_id = pm.active_id().map(|s| s.to_string());
    let (filtered_projects, active_id) = filter_main_window_projects(all_projects, &excluded, raw_active_id);
    let mut payload = serde_json::json!({
        "projects": filtered_projects,
        "activeId": active_id,
        "source": source,
    });

    // The source window skips its own broadcast (it applies the command
    // response instead), so the full project data is only needed when another
    // window has to re-sync. Avoid serializing the whole project (with the
    // enzyme list) on every mutation when there's nobody else to notify.
    let has_other_windows = match source {
        Some(label) => app_handle.webview_windows().keys().any(|l| l != label),
        None => !app_handle.webview_windows().is_empty(),
    };

    if has_other_windows {
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
        // Include all project data for multi-window sync (keyed by project ID)
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        let mut project_data_map = serde_json::Map::new();
        for id in pm.all_project_ids() {
            if let Some(project) = pm.get_project_by_id(&id) {
                project_data_map.insert(id, filter_project(project, &params));
            }
        }
        payload["projectData"] = serde_json::Value::Object(project_data_map);
    }
    let _ = app_handle.emit("project-update", payload);
}

// ---------------------------------------------------------------------------
// Shared mutation/analysis cores — called by both the Tauri commands and the
// MCP server so every path goes through identical logic (recompute, dirty,
// broadcast). Each mirrors the command it was extracted from.
// ---------------------------------------------------------------------------

async fn do_open_file(
    pm: &Arc<RwLock<ProjectManager>>,
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
            let params = ProjectParams {
                enzyme_filter: Some("all".to_string()),
                row_start: None,
                row_end: None,
                cpl: None,
            };
            let return_data = filter_project(&project, &params);

            let (projects, active_id) = {
                let mut pm = pm.write().await;
                pm.load(&id, project);
                (pm.list_projects(), pm.active_id().map(|s| s.to_string()))
            };

            Ok(with_projects_list(return_data, &projects, active_id.as_deref()))
        }
        Err(e) => Ok(serde_json::json!({"error": e})),
    }
}

async fn do_save_file(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: String,
    path: String,
) -> Result<serde_json::Value, String> {
    let save_path = std::path::PathBuf::from(&path);
    let project = {
        let pm = pm.read().await;
        pm.get_project_by_id(&project_id).cloned()
    };
    match project {
        Some(ref p) => match file_io::gbk::write_gbk(p, &save_path) {
            Ok(()) => {
                let mut pm = pm.write().await;
                pm.mark_clean(&project_id);
                Ok(serde_json::json!({"status": "ok"}))
            }
            Err(e) => Ok(serde_json::json!({"error": e.to_string()})),
        },
        None => Ok(serde_json::json!({"error": "Project not found"})),
    }
}

async fn do_update_sequence(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: String,
    sequence: String,
) -> Result<serde_json::Value, String> {
    {
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.sequence = sequence;
            p.length = p.sequence.len() as i64;
            pm.mark_dirty(&project_id);
        }
    }

    let project_clone = {
        let pm = pm.read().await;
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

        let mut pm = pm.write().await;
        pm.open_project(project_id.clone(), computed);
    }

    let (result, projects, active_id) = {
        let pm = pm.read().await;
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

async fn do_activate_project(
    pm: &Arc<RwLock<ProjectManager>>,
    id: String,
) -> Result<serde_json::Value, String> {
    let activated = {
        let mut pm = pm.write().await;
        pm.activate_project(&id)
    };
    if !activated {
        return Ok(serde_json::json!({"error": format!("project not found: {}", id)}));
    }

    let result = {
        let pm = pm.read().await;
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

async fn do_delete_project<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    source: Option<&str>,
    id: String,
) -> Result<serde_json::Value, String> {
    let closed = {
        let mut pm = pm.write().await;
        pm.close_project(&id)
    };
    if closed {
        {
            let mut wp = wp.write().await;
            wp.retain(|_, v| v != &id);
        }
        broadcast_project_arcs(app_handle, pm, wp, source).await;
        Ok(serde_json::json!({"status": "ok"}))
    } else {
        Ok(serde_json::json!({"error": "project not found"}))
    }
}

/// Add or replace features (by id) and broadcast. The location string has
/// already been parsed into the Feature by the caller.
async fn do_add_features<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    source: Option<&str>,
    project_id: &str,
    features: Vec<Feature>,
) -> Result<serde_json::Value, String> {
    let mut feats: Vec<Feature> = pm
        .read()
        .await
        .get_project_by_id(project_id)
        .map(|p| p.features.clone())
        .unwrap_or_default();

    for feature in features {
        if let Some(pos) = feats.iter().position(|f| f.id == feature.id) {
            feats[pos] = feature;
        } else {
            feats.push(feature);
        }
    }

    {
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            p.features = feats;
        }
        pm.mark_dirty(project_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, source).await;

    let pm = pm.read().await;
    Ok(feature_mutation_response(&pm, project_id))
}

async fn do_delete_feature<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    source: Option<&str>,
    project_id: &str,
    id: String,
) -> Result<serde_json::Value, String> {
    let (feats, projects, active_id) = {
        let mut pm = pm.write().await;
        let feats: Vec<Feature> = pm
            .get_project_by_id(project_id)
            .map(|p| p.features.iter().filter(|f| f.id != id).cloned().collect())
            .unwrap_or_default();
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            p.features = feats.clone();
        }
        pm.mark_dirty(project_id);
        (feats, pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    broadcast_project_arcs(app_handle, pm, wp, source).await;

    Ok(with_projects_list(
        serde_json::json!({ "features": feats }),
        &projects,
        active_id.as_deref(),
    ))
}

/// Apply `apply` to the feature `feature_id`, mark dirty and broadcast.
async fn do_update_feature<R: Runtime, F>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    source: Option<&str>,
    project_id: &str,
    feature_id: &str,
    apply: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce(&mut Feature) -> Result<(), String>,
{
    {
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            if let Some(f) = p.features.iter_mut().find(|f| f.id == feature_id) {
                apply(f)?;
                pm.mark_dirty(project_id);
            }
        }
    }

    broadcast_project_arcs(app_handle, pm, wp, source).await;

    let pm = pm.read().await;
    Ok(feature_mutation_response(&pm, project_id))
}

async fn do_add_primer<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    source: Option<&str>,
    project_id: &str,
    primer: Primer,
) -> Result<serde_json::Value, String> {
    let name_conflict = {
        let pm = pm.read().await;
        pm.get_project_by_id(project_id).map(|p| {
            p.primers
                .iter()
                .any(|p| p.id != primer.id && p.name == primer.name)
        }).unwrap_or(false)
    };
    if name_conflict {
        return Ok(serde_json::json!({"error": format!("Primer name '{}' already exists", primer.name)}));
    }

    {
        let mut pm = pm.write().await;
        let existing_primers: Vec<Primer> = pm
            .get_project_by_id(project_id)
            .map(|p| p.primers.clone())
            .unwrap_or_default();
        let mut primers = existing_primers;
        if let Some(pos) = primers.iter().position(|p| p.id == primer.id) {
            primers[pos] = primer.clone();
        } else {
            primers.push(primer.clone());
        }
        if let Some(p) = pm.get_project_by_id(project_id) {
            let template = p.sequence.clone();
            let topology = p.topology.clone();
            let updated = libregene_core::primer::align::recompute_all_primers(&template, &topology, &primers);
            if let Some(p) = pm.get_project_mut_by_id(project_id) {
                p.primers = updated;
            }
        } else if let Some(p) = pm.get_project_mut_by_id(project_id) {
            p.primers = primers;
        }
        pm.mark_dirty(project_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, source).await;

    let pm = pm.read().await;
    match pm.get_project_by_id(project_id) {
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

async fn do_delete_primer<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    source: Option<&str>,
    project_id: &str,
    id: String,
) -> Result<serde_json::Value, String> {
    let (primers, projects, active_id) = {
        let mut pm = pm.write().await;
        let exists = pm
            .get_project_by_id(project_id)
            .map(|p| p.primers.iter().any(|pr| pr.id == id))
            .unwrap_or(false);
        if !exists {
            return Err(format!("Primer not found: {}", id));
        }
        let primers: Vec<Primer> = pm
            .get_project_by_id(project_id)
            .map(|p| p.primers.iter().filter(|pr| pr.id != id).cloned().collect())
            .unwrap_or_default();
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            p.primers = primers.clone();
        }
        pm.mark_dirty(project_id);
        (primers, pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    broadcast_project_arcs(app_handle, pm, wp, source).await;

    Ok(with_projects_list(
        serde_json::json!({ "primers": primers }),
        &projects,
        active_id.as_deref(),
    ))
}

async fn do_set_methylation(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
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
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
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

        let mut pm = pm.write().await;
        pm.open_project(project_id.to_string(), computed);
        pm.mark_dirty(project_id);
    }

    let (result, projects, active_id) = {
        let pm = pm.read().await;
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        let data = pm
            .get_project_by_id(project_id)
            .map(|p| filter_project(p, &params))
            .unwrap_or(serde_json::json!({"error": "Project not found after methylation"}));
        (data, pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };

    Ok(with_projects_list(result, &projects, active_id.as_deref()))
}

async fn do_add_alignment_seq<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    source: Option<&str>,
    project_id: &str,
    name: String,
    seq: String,
) -> Result<serde_json::Value, String> {
    let project_clone = {
        let pm = pm.read().await;
        pm.get_project_by_id(project_id).cloned()
    };
    let project_clone = match project_clone {
        Some(p) => p,
        None => return Ok(serde_json::json!({"error": "Project not found"})),
    };

    let clean_seq: String = seq
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_uppercase();
    if clean_seq.is_empty() {
        return Ok(serde_json::json!({"error": "Sequence is empty"}));
    }

    let computed = tokio::task::spawn_blocking(move || -> Result<ProjectData, String> {
        let mut p = project_clone;
        let circular = p.topology == "circular";
        let mut aln = libregene_core::align::align_read(&p.sequence, &clean_seq, circular)
            .ok_or_else(|| "No significant alignment found".to_string())?;
        aln.name = if name.trim().is_empty() {
            "alignment".to_string()
        } else {
            name.trim().to_string()
        };
        aln.id = libregene_core::align::next_alignment_id(&p.alignments);
        p.alignments.push(aln);
        Ok(p)
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;

    let computed = match computed {
        Ok(p) => p,
        Err(e) if e == "No significant alignment found" => return Err(e),
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    {
        let mut pm = pm.write().await;
        pm.open_project(project_id.to_string(), computed);
        pm.mark_dirty(project_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, source).await;

    let pm = pm.read().await;
    match pm.get_project_by_id(project_id) {
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

async fn do_remove_alignment<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    source: Option<&str>,
    project_id: &str,
    alignment_id: String,
) -> Result<serde_json::Value, String> {
    {
        let mut pm = pm.write().await;
        pm.remove_alignment(project_id, &alignment_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, source).await;

    let pm = pm.read().await;
    match pm.get_project_by_id(project_id) {
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

async fn do_find_orfs(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
    min_aa: Option<usize>,
) -> Result<Vec<Feature>, String> {
    let (sequence, topology) = {
        let pm = pm.read().await;
        let project = pm
            .get_project_by_id(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        (project.sequence.clone(), project.topology.clone())
    };

    let min_aa = min_aa.unwrap_or(75);
    tokio::task::spawn_blocking(move || {
        libregene_core::orf::find_orfs(&sequence, &topology, min_aa)
    })
    .await
    .map_err(|e| format!("task join error: {}", e))
}

async fn do_search_sequence(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
    query: String,
) -> Result<Vec<libregene_core::search::SeqMatch>, String> {
    let sequence = {
        let pm = pm.read().await;
        let project = pm
            .get_project_by_id(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        project.sequence.clone()
    };

    tokio::task::spawn_blocking(move || {
        libregene_core::search::find_seq_matches(&sequence, &query)
    })
    .await
    .map_err(|e| format!("task join error: {}", e))
}

async fn do_check_primers_binding(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
    primers: Vec<Primer>,
) -> Result<serde_json::Value, String> {
    let (template, topology) = {
        let pm = pm.read().await;
        let project = pm
            .get_project_by_id(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        (project.sequence.clone(), project.topology.clone())
    };

    let results = tokio::task::spawn_blocking(move || {
        let updated =
            libregene_core::primer::align::recompute_all_primers(&template, &topology, &primers);
        updated
            .into_iter()
            .map(|p| {
                let site = p.binding_sites.first();
                serde_json::json!({
                    "id": p.id,
                    "binds": site.is_some(),
                    "site": site.map(|s| serde_json::json!({
                        "strand": s.strand,
                        "templateStart": s.template_start,
                        "templateEnd": s.template_end,
                        "tm": s.tm,
                        "annealLen": libregene_core::primer::align::anneal_len(
                            &template, &topology, &p.primer_seq, s,
                        ),
                    })),
                })
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;

    Ok(serde_json::json!({ "results": results }))
}

#[allow(clippy::too_many_arguments)]
async fn do_design_primer_candidates(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
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
    fwd_tail: Option<String>,
    rev_tail: Option<String>,
    na_conc: Option<f64>,
    mg_conc: Option<f64>,
    dntp_conc: Option<f64>,
    tris_conc: Option<f64>,
    primer_conc: Option<f64>,
) -> Result<Vec<libregene_core::primer::design::PrimerGroup>, String> {
    let (sequence, topology) = {
        let pm = pm.read().await;
        let project = pm
            .get_project_by_id(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        (project.sequence.clone(), project.topology.clone())
    };

    let tm_params = libregene_core::primer::thermodynamics::TmParams {
        na_conc: na_conc.unwrap_or(0.050),
        mg_conc: mg_conc.unwrap_or(0.0015),
        dntp_conc: dntp_conc.unwrap_or(0.0008),
        tris_conc: tris_conc.unwrap_or(0.010),
        primer_conc: primer_conc.unwrap_or(2e-7),
    };

    let seg1 = seg.ok_or_else(|| "Segment required for primer design".to_string())?;
    let overlap_len = overlap_len.unwrap_or(20).max(8);
    let arm_len = arm_len.unwrap_or(20).max(8);
    let mut_seq = mut_seq.unwrap_or_default();

    tokio::task::spawn_blocking(move || -> Result<Vec<libregene_core::primer::design::PrimerGroup>, String> {
        match mode.as_str() {
            "amplify" => {
                let name = name.unwrap_or_else(|| "Amplicon".to_string());
                match (fwd_tail, rev_tail) {
                    (None, None) => Ok(libregene_core::primer::design::build_amplify_groups(
                        &sequence, &seg1, &name, target_tm, &topology, &tm_params,
                    )),
                    (f, r) => Ok(libregene_core::primer::design::build_amplify_groups_tailed(
                        &sequence, &seg1, &name, target_tm, &topology,
                        f.as_deref().unwrap_or(""), r.as_deref().unwrap_or(""), &tm_params,
                    )),
                }
            }
            "oepcr" => {
                let seg2 = seg2.ok_or_else(|| "Second segment required for OE-PCR".to_string())?;
                let name1 = name1.unwrap_or_else(|| "Fragment 1".to_string());
                let name2 = name2.unwrap_or_else(|| "Fragment 2".to_string());
                Ok(libregene_core::primer::design::build_oepcr_groups(
                    &sequence, &seg1, &seg2, &name1, &name2, target_tm, overlap_len, &topology, &tm_params,
                ))
            }
            "mutagenesis" => {
                let site_name = site_name.unwrap_or_else(|| "Mutation".to_string());
                Ok(libregene_core::primer::design::build_mutagenesis_groups(
                    &sequence, &seg1, &site_name, &mut_seq, target_tm, arm_len, &tm_params,
                ))
            }
            other => Err(format!("Unknown primer design mode: {other}")),
        }
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?
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
    do_open_file(&state.pm, path).await
}

#[tauri::command]
async fn save_file(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    path: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    match project_id {
        Some(id) => do_save_file(&state.pm, id, path).await,
        None => Ok(serde_json::json!({"error": "No project loaded"})),
    }
}

/// Write raw text to a file (used for My Enzymes export). Small payloads only.
#[tauri::command]
async fn write_text_file(path: String, contents: String) -> Result<serde_json::Value, String> {
    std::fs::write(&path, contents).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"status": "ok"}))
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
    do_update_sequence(&state.pm, project_id, sequence).await
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

    do_add_features(
        &app_handle,
        &state.pm,
        &state.window_projects,
        Some(webview_window.label()),
        &project_id,
        vec![resolved],
    )
    .await
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

    do_delete_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        Some(webview_window.label()),
        &project_id,
        id,
    )
    .await
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

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
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

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
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

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
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

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
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

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        Some(webview_window.label()),
        &project_id,
        &feature_id,
        |f| {
            let parsed = libregene_core::file_io::gbk::parse_location_string(&location_str)
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

// ---------------------------------------------------------------------------
// Tauri commands — enzymes
// ---------------------------------------------------------------------------

/// Return the full static enzyme database (all records, regardless of whether
/// they cut the current sequence).
#[tauri::command]
async fn get_enzyme_database() -> Result<serde_json::Value, String> {
    let db = libregene_core::enzyme::search::get_db();
    serde_json::to_value(&db.enzymes).map_err(|e| e.to_string())
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

    do_add_primer(
        &app_handle,
        &state.pm,
        &state.window_projects,
        Some(webview_window.label()),
        &project_id,
        primer,
    )
    .await
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

    do_delete_primer(
        &app_handle,
        &state.pm,
        &state.window_projects,
        Some(webview_window.label()),
        &project_id,
        id,
    )
    .await
}

/// Check which of the given primers can bind to the current project's sequence.
/// Returns a lightweight per-primer summary (binds + best site), reusing the
/// same binding-site engine as the editor for consistency.
#[tauri::command]
async fn check_primers_binding(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    primers: Vec<Primer>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await
        .ok_or_else(|| "No project loaded".to_string())?;

    do_check_primers_binding(&state.pm, &project_id, primers).await
}

/// Add a batch of primers to the current project (My Primers → current file).
/// Rejects exact-sequence duplicates and name conflicts; recomputes once.
#[tauri::command]
async fn add_primers(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    primers: Vec<Primer>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    {
        let mut pm = state.pm.write().await;
        let existing: Vec<Primer> = pm
            .get_project_by_id(&project_id)
            .map(|p| p.primers.clone())
            .unwrap_or_default();

        let mut merged = existing;
        for primer in primers {
            let seq_conflict = merged
                .iter()
                .any(|p| p.primer_seq.to_uppercase() == primer.primer_seq.to_uppercase());
            let name_conflict =
                merged.iter().any(|p| p.id != primer.id && p.name == primer.name);
            if seq_conflict || name_conflict {
                continue;
            }
            if let Some(pos) = merged.iter().position(|p| p.id == primer.id) {
                merged[pos] = primer;
            } else {
                merged.push(primer);
            }
        }

        if let Some(p) = pm.get_project_by_id(&project_id) {
            let template = p.sequence.clone();
            let topology = p.topology.clone();
            let updated = libregene_core::primer::align::recompute_all_primers(
                &template,
                &topology,
                &merged,
            );
            if let Some(p) = pm.get_project_mut_by_id(&project_id) {
                p.primers = updated;
            }
        } else if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.primers = merged;
        }
        pm.mark_dirty(&project_id);
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
async fn compute_primer_alignment(
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
    let project_id = resolve_project_id(&state, webview_window.label()).await
        .ok_or_else(|| "No project loaded".to_string())?;

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
        mg_conc: mg_conc.unwrap_or(0.0015),
        dntp_conc: dntp_conc.unwrap_or(0.0008),
        tris_conc: tris_conc.unwrap_or(0.010),
        primer_conc: primer_conc.unwrap_or(2e-7),
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

fn compute_primer_alignment_sync(
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
                let d = if c.tp_3prime > tp_3prime { c.tp_3prime - tp_3prime } else { tp_3prime - c.tp_3prime };
                d <= 3
            }) { continue; }

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
        let win_end = if is_circular {
            tp + expansion
        } else {
            (tp + expansion).min(tlen)
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
                results.push(serde_json::json!({
                    "tm": (c.est_tm * 10.0).round() / 10.0,
                    "strand": if c.is_rev { -1 } else { 1 },
                    "start": (tp - c.footprint_len + 1) as i64,
                    "end": tp as i64 + 1,
                    "alignment": null,
                }));
            }
        } else {
            results.push(serde_json::json!({
                "tm": (c.est_tm * 10.0).round() / 10.0,
                "strand": if c.is_rev { -1 } else { 1 },
                "start": (tp - c.footprint_len + 1) as i64,
                "end": tp as i64 + 1,
                "alignment": null,
            }));
        }
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
// Tauri commands — ORF search / sequence search / primer design
// ---------------------------------------------------------------------------

/// Find open reading frames (ATG→stop, both strands, all three frames) on the
/// active project's sequence. Returns virtual CDS features (display only),
/// shaped exactly like the ORF plugin's old JS output: id `orf-<strand><start>:<end>`,
/// name `ORF <start+1>..<end+1>`, per-strand colors, and a `qualifiers`
/// `[("orf", "true")]` entry the renderer maps back to `orf: true`.
#[tauri::command]
async fn find_orfs(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    min_aa: Option<usize>,
) -> Result<Vec<Feature>, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await
        .ok_or_else(|| "No project loaded".to_string())?;

    do_find_orfs(&state.pm, &project_id, min_aa).await
}

/// Search the active project's sequence for an IUPAC-aware query on both
/// strands, mirroring `findSeqMatches` in `src/searchUtils.js`.
#[tauri::command]
async fn search_sequence(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<libregene_core::search::SeqMatch>, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await
        .ok_or_else(|| "No project loaded".to_string())?;

    do_search_sequence(&state.pm, &project_id, query).await
}

/// Generate primer design candidates for the active project's sequence.
/// `mode` is "amplify" | "oepcr" | "mutagenesis"; segments are { start, end }
/// 0-based inclusive. Tm is computed with the same TmParams defaults as the
/// `compute_tm` command.
#[tauri::command]
async fn design_primer_candidates(
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
    let project_id = resolve_project_id(&state, webview_window.label()).await
        .ok_or_else(|| "No project loaded".to_string())?;

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

// ---------------------------------------------------------------------------
// Tauri commands — alignments
// ---------------------------------------------------------------------------

#[tauri::command]
async fn add_alignment(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    path: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    let project_clone = {
        let pm = state.pm.read().await;
        pm.get_project_by_id(&project_id).cloned()
    };
    let project_clone = match project_clone {
        Some(p) => p,
        None => return Ok(serde_json::json!({"error": "Project not found"})),
    };

    let path_buf = std::path::PathBuf::from(&path);
    let name = path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("alignment")
        .to_string();

    let computed = tokio::task::spawn_blocking(move || -> Result<ProjectData, String> {
        let read_project = file_io::parse_file(&path_buf).map_err(|e| e.to_string())?;
        if read_project.sequence.is_empty() {
            return Err("File contains no sequence".to_string());
        }
        let mut p = project_clone;
        let circular = p.topology == "circular";
        let mut aln = libregene_core::align::align_read(&p.sequence, &read_project.sequence, circular)
            .ok_or_else(|| "No significant alignment found".to_string())?;
        aln.name = name;
        aln.id = libregene_core::align::next_alignment_id(&p.alignments);
        p.alignments.push(aln);
        Ok(p)
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;

    let computed = match computed {
        Ok(p) => p,
        Err(e) if e == "No significant alignment found" => return Err(e),
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    {
        let mut pm = state.pm.write().await;
        pm.open_project(project_id.clone(), computed);
        pm.mark_dirty(&project_id);
    }

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
async fn add_alignment_seq(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    name: String,
    seq: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    do_add_alignment_seq(
        &app_handle,
        &state.pm,
        &state.window_projects,
        Some(webview_window.label()),
        &project_id,
        name,
        seq,
    )
    .await
}

#[tauri::command]
async fn remove_alignment(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    alignment_id: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };

    do_remove_alignment(
        &app_handle,
        &state.pm,
        &state.window_projects,
        Some(webview_window.label()),
        &project_id,
        alignment_id,
    )
    .await
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

    do_set_methylation(&state.pm, &project_id, systems, overlap).await
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

    do_activate_project(&state.pm, id).await
}

#[tauri::command]
async fn delete_project(
    webview_window: tauri::WebviewWindow,
    app_handle: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    do_delete_project(
        &app_handle,
        &state.pm,
        &state.window_projects,
        Some(webview_window.label()),
        id,
    )
    .await
}

// ---------------------------------------------------------------------------
// Tauri commands — multi-window
// ---------------------------------------------------------------------------

/// Open the given project in a new OS window.
#[tauri::command]
async fn open_in_new_window(
    webview_window: tauri::WebviewWindow,
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

    // Create the new window; the frontend activates the overlay titlebar and shows it
    let builder = WebviewWindowBuilder::new(
        &app_handle,
        &window_label,
        WebviewUrl::App("index.html".into()),
    )
    .title("LibreGene - Plasmid Editor")
    .inner_size(1400.0, 900.0)
    .min_inner_size(960.0, 540.0)
    .decorations(true)
    .visible(false);

    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(14.0, 22.0));

    let window = builder
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
                broadcast_project(&ah, &state, None).await;
            });
        }
    });

    // Broadcast so the main window updates its sidebar immediately
    broadcast_project(&app_handle, &state, Some(webview_window.label())).await;

    Ok(serde_json::json!({"status": "ok", "windowLabel": window_label}))
}

/// Compute melting temperature using SantaLucia 2004 nearest-neighbour model.
#[tauri::command]
async fn compute_tm(
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
        mg_conc: mg_conc.unwrap_or(0.0015),
        dntp_conc: dntp_conc.unwrap_or(0.0008),
        tris_conc: tris_conc.unwrap_or(0.010),
        primer_conc: primer_conc.unwrap_or(2e-7),
    };
    let tm = libregene_core::primer::thermodynamics::compute_tm_with_params(&seq, &params);
    Ok((tm * 10.0).round() / 10.0)
}

/// Activate the decoration plugin's overlay titlebar, then show the window.
/// Windows start hidden (visible: false) so native decorations never flash.
#[cfg(target_os = "macos")]
const TL_INSET: (f32, f32) = (14.0, 22.0);

#[tauri::command]
fn activate_custom_titlebar(window: tauri::WebviewWindow) -> Result<(), String> {
    use tauri_plugin_decoration::WebviewWindowExt;
    window
        .create_overlay_titlebar()
        .map_err(|e| e.to_string())?;
    // Store the tuned inset in the plugin registry (the value it replays on
    // resize/focus; same semantics as tao's trafficLightPosition: x = left
    // edge, y = extra container height).
    #[cfg(target_os = "macos")]
    window
        .set_traffic_lights_inset(TL_INSET.0, TL_INSET.1)
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

/// Re-assert the tuned traffic-light inset after native open/save panels.
#[tauri::command]
fn reassert_traffic_lights(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_decoration::WebviewWindowExt;
        let _ = window.set_traffic_lights_inset(TL_INSET.0, TL_INSET.1);
    }
    let _ = window;
    Ok(())
}

/// Fallback: restore native decorations if plugin activation fails.
#[tauri::command]
fn restore_native_titlebar(window: tauri::WebviewWindow) -> Result<(), String> {
    use tauri_plugin_decoration::WebviewWindowExt;
    window
        .restore_native_titlebar()
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    Ok(())
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

/// Rename a project's ID (called after Save As to re-key the project).
#[tauri::command]
async fn rekey_project(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    old_id: String,
    new_id: String,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await;
    let project_id = match project_id {
        Some(id) => id,
        None => return Ok(serde_json::json!({"error": "No project loaded"})),
    };
    if project_id != old_id {
        return Ok(serde_json::json!({"error": "Project ID mismatch"}));
    }

    let ok = {
        let mut pm = state.pm.write().await;
        pm.rename_id(&old_id, &new_id)
    };
    if !ok {
        return Ok(serde_json::json!({"error": "Rename failed (target may already exist)"}));
    }

    broadcast_project(&app_handle, &state, Some(webview_window.label())).await;
    Ok(serde_json::json!({"status": "ok", "newId": new_id}))
}

// ---------------------------------------------------------------------------
// Tauri commands — MCP server settings
// ---------------------------------------------------------------------------

/// Current MCP server runtime config (enabled + loopback port).
#[tauri::command]
async fn get_mcp_config(
    mcp: State<'_, mcp::McpServer<tauri::Wry>>,
) -> Result<serde_json::Value, String> {
    let cfg = mcp.config();
    Ok(serde_json::json!({ "enabled": cfg.enabled, "port": cfg.port }))
}

/// Enable/disable the MCP server or move it to a new loopback port. The server
/// is stopped/restarted in place — no app restart needed.
#[tauri::command]
async fn set_mcp_config(
    mcp: State<'_, mcp::McpServer<tauri::Wry>>,
    enabled: bool,
    port: u16,
) -> Result<serde_json::Value, String> {
    let cfg = mcp.set_config(enabled, port).await?;
    Ok(serde_json::json!({
        "enabled": cfg.enabled,
        "port": cfg.port,
        "status": "ok",
    }))
}

// ---------------------------------------------------------------------------
// App entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_decoration::init())
        .manage(AppState {
            pm: Arc::new(RwLock::new(ProjectManager::new())),
            window_projects: Arc::new(RwLock::new(HashMap::new())),
        })
        .setup(|app| {
            let state = app.state::<AppState>();
            let mcp = mcp::McpServer::new(
                app.handle().clone(),
                state.pm.clone(),
                state.window_projects.clone(),
            );
            app.manage(mcp.clone());
            // Start with the default config (enabled on MCP_PORT); the frontend
            // reconciles with the persisted localStorage config on mount.
            tauri::async_runtime::block_on(async move { mcp.apply().await });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_project,
            get_project_by_id,
            open_file,
            save_file,
            write_text_file,
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
            add_primers,
            delete_primer,
            check_primers_binding,
            compute_primer_alignment,
            design_primer_candidates,
            find_orfs,
            search_sequence,
            get_enzyme_database,
            add_alignment,
            add_alignment_seq,
            remove_alignment,
            set_methylation,
            get_projects,
            activate_project,
            delete_project,
            open_in_new_window,
            get_window_project_id,
            rekey_project,
            compute_tm,
            get_mcp_config,
            set_mcp_config,
            activate_custom_titlebar,
            reassert_traffic_lights,
            restore_native_titlebar,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
