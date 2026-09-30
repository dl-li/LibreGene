use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::RwLock;

use libregene_core::models::ProjectData;
use libregene_core::project::ProjectManager;

use crate::state::{AgentTabs, AppState};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct ProjectParams {
    pub(crate) enzyme_filter: Option<String>,
    pub(crate) row_start: Option<i64>,
    pub(crate) row_end: Option<i64>,
    pub(crate) cpl: Option<i64>,
}

pub(crate) const DEFAULT_CPL: i64 = 60;

/// Resolve the project_id for a given window:
/// - Project windows look up their mapping in `window_projects`; a miss means
///   the project was evicted — never fall back to the main window's active
///   project, or the window's mutations would silently hit the wrong file.
/// - The main window falls back to `ProjectManager::active_id()`.
pub(crate) async fn resolve_project_id(
    state: &State<'_, AppState>,
    window_label: &str,
) -> Result<String, String> {
    if window_label != "main" {
        let wp = state.window_projects.read().await;
        return wp.get(window_label).cloned().ok_or_else(|| {
            "Project not found in this window (may have been evicted); reload the file".to_string()
        });
    }
    let pm = state.pm.read().await;
    pm.active_id()
        .map(|s| s.to_string())
        .ok_or_else(|| "No project loaded".to_string())
}

/// Returns the set of project IDs that are currently open in dedicated project windows.
/// These projects should be hidden from the main window's sidebar.
pub(crate) fn excluded_project_ids(wp: &tokio::sync::RwLockReadGuard<HashMap<String, String>>) -> HashSet<String> {
    wp.values().cloned().collect()
}

/// Lock the agent tab bound to `project_id` and notify the main window.
/// Called whenever an MCP tool touches the project, so a tab the user
/// unlocked snaps back to locked as soon as the agent acts again.
/// Emits only on an unlocked → locked transition.
pub(crate) async fn lock_agent_tab_for_project<R: Runtime>(
    app_handle: &AppHandle<R>,
    agent_tabs: &AgentTabs,
    project_id: &str,
) {
    let mut at = agent_tabs.write().await;
    if let Some(meta) = at.get_mut(project_id) {
        if !meta.locked {
            meta.locked = true;
            let _ = app_handle.emit(
                "agent-tab-lock",
                serde_json::json!({
                    "projectId": project_id,
                    "locked": true,
                }),
            );
        }
    }
}

/// Filter out projects that are open in project windows, and adjust the activeId
/// if it points to an excluded project.
pub(crate) fn filter_main_window_projects(
    projects: Vec<serde_json::Value>,
    excluded: &HashSet<String>,
    active_id: Option<String>,
) -> (Vec<serde_json::Value>, Option<String>) {
    let filtered: Vec<_> = projects
        .into_iter()
        .filter(|p| p["id"].as_str().is_none_or(|id| !excluded.contains(id)))
        .collect();
    let active = active_id.filter(|id| !excluded.contains(id.as_str()))
        .or_else(|| {
            filtered.first()
                .and_then(|p| p["id"].as_str().map(String::from))
        });
    (filtered, active)
}

/// The main-window sidebar project list: projects bound to dedicated project
/// windows filtered out, activeId adjusted, and `agentLocked` injected from
/// the agent-tabs map. This is the exact same payload shape
/// `broadcast_project_arcs` emits, so mutation responses and broadcasts never
/// diverge. Lock order: agent_tabs is read (and dropped) BEFORE pm/wp; pm is
/// taken before window_projects.
pub(crate) async fn sidebar_project_list(
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
) -> (Vec<serde_json::Value>, Option<String>) {
    let tab_locks: HashMap<String, bool> = {
        let at = agent_tabs.read().await;
        at.iter().map(|(id, m)| (id.clone(), m.locked)).collect()
    };
    let pm = pm.read().await;
    let wp = wp.read().await;
    let excluded = excluded_project_ids(&wp);
    let all_projects = pm.list_projects();
    let raw_active_id = pm.active_id().map(|s| s.to_string());
    let (mut filtered_projects, active_id) =
        filter_main_window_projects(all_projects, &excluded, raw_active_id);
    for p in &mut filtered_projects {
        if let Some(id) = p["id"].as_str() {
            p["agentLocked"] = tab_locks
                .get(id)
                .map(|l| serde_json::json!(l))
                .unwrap_or(serde_json::Value::Null);
        }
    }
    (filtered_projects, active_id)
}

/// Drop window_projects/agent_tabs entries pointing at projects no longer
/// loaded (e.g. evicted by ProjectManager's capacity limit). Called on load
/// paths so an eviction never leaves a dangling binding behind.
pub(crate) async fn prune_orphan_bindings(
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
) {
    let loaded: HashSet<String> = pm.read().await.all_project_ids().into_iter().collect();
    {
        let mut wp = wp.write().await;
        wp.retain(|_, v| loaded.contains(v));
    }
    {
        let mut at = agent_tabs.write().await;
        at.retain(|k, _| loaded.contains(k));
    }
}

/// Inject projects list and activeId into a JSON response so the main
/// window sidebar stays in sync after any mutation or switch.
pub(crate) fn with_projects_list(
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
pub(crate) async fn feature_mutation_response(
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    project_id: &str,
) -> serde_json::Value {
    let data = {
        let pm = pm.read().await;
        match pm.get_project_by_id(project_id) {
            Some(p) => serde_json::json!({ "features": &p.features }),
            None => serde_json::json!({"error": "Project not found"}),
        }
    };
    let (projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;
    with_projects_list(data, &projects, active_id.as_deref())
}

/// Filter enzymes according to the same logic as routes.rs::filter_project.
pub(crate) fn filter_project(project: &ProjectData, params: &ProjectParams) -> serde_json::Value {
    let filter = params.enzyme_filter.as_deref().unwrap_or("unique");
    let cpl = params.cpl.unwrap_or(DEFAULT_CPL).max(1);
    let row_start = params.row_start.map(|rs| rs.max(0));
    let row_end = params.row_end.map(|re| re.max(0));
    // A row window only applies when both bounds are given; saturating
    // math keeps absurd inputs from overflowing i64 instead of panicking
    // (an inverted window simply matches nothing, as before).
    let row_window = match (row_start, row_end) {
        (Some(rs), Some(re)) => Some((
            rs.saturating_mul(cpl),
            re.saturating_add(1).saturating_mul(cpl).saturating_sub(1),
        )),
        _ => None,
    };

    let enzymes: Vec<&libregene_core::models::Enzyme> = if filter == "all" {
        let all: Vec<_> = project.enzymes.iter().collect();
        if let Some((idx_s, idx_e)) = row_window {
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
        if let Some((idx_s, idx_e)) = row_window {
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
        "moleculeType": &project.molecule_type,
        "features": &project.features,
        "primers": &project.primers,
        "alignments": &project.alignments,
        "methylation_systems": &project.methylation_systems,
        "methylation_overlap": project.methylation_overlap,
        "roi": &project.roi,
        "tracePath": &project.trace_path,
        "enzymeCount": project.enzymes.len(),
        "enzymeFilter": filter,
        "enzymes": &enzymes,
    })
}

/// Emit the current project state + filtered project list as a Tauri event.
pub(crate) async fn broadcast_project(app_handle: &AppHandle, state: &State<'_, AppState>, source: Option<&str>) {
    broadcast_project_arcs(
        app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        source,
    )
    .await;
}

/// Same as `broadcast_project` but takes the shared Arcs directly, so the MCP
/// server can broadcast without a `State` handle.
pub(crate) async fn broadcast_project_arcs<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
) {
    let (filtered_projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;
    let pm = pm.read().await;
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
        // Include project data for multi-window sync (keyed by project ID) —
        // only the projects some window actually shows: window-bound projects
        // plus the ones visible in the main window's sidebar.
        let mut wanted: HashSet<String> = payload["projects"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| p["id"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        {
            let wp = wp.read().await;
            wanted.extend(wp.values().cloned());
        }
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        let mut project_data_map = serde_json::Map::new();
        for id in pm.all_project_ids() {
            if !wanted.contains(&id) {
                continue;
            }
            if let Some(project) = pm.get_project_by_id(&id) {
                project_data_map.insert(id, filter_project(project, &params));
            }
        }
        payload["projectData"] = serde_json::Value::Object(project_data_map);
    }
    let _ = app_handle.emit("project-update", payload);
}
