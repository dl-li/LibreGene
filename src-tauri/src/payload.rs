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

/// Resolve the single window's active project.
pub(crate) async fn resolve_project_id(state: &State<'_, AppState>) -> Result<String, String> {
    let pm = state.pm.read().await;
    pm.active_id()
        .map(|s| s.to_string())
        .ok_or_else(|| "No project loaded".to_string())
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

/// Tell agent-locked windows where an MCP mutation landed so they can scroll
/// to and select the changed region. Emitted after broadcast_project; the
/// lock gate lives in the frontend. Coordinates are 0-based inclusive.
pub(crate) fn emit_mutation_focus<R: Runtime>(
    app_handle: &AppHandle<R>,
    project_id: &str,
    start: i64,
    end: i64,
    kind: &str,
) {
    let _ = app_handle.emit(
        "agent-mutation-focus",
        serde_json::json!({
            "projectId": project_id,
            "start": start,
            "end": end,
            "kind": kind,
        }),
    );
}

/// The sidebar project list with `agentLocked` injected from the agent-tabs
/// map. The agent-tabs guard is dropped before taking the pm guard.
pub(crate) async fn sidebar_project_list(
    pm: &Arc<RwLock<ProjectManager>>,
    agent_tabs: &AgentTabs,
) -> (Vec<serde_json::Value>, Option<String>) {
    let tab_locks: HashMap<String, bool> = {
        let at = agent_tabs.read().await;
        at.iter().map(|(id, m)| (id.clone(), m.locked)).collect()
    };
    let pm = pm.read().await;
    let mut projects = pm.list_projects();
    let active_id = pm.active_id().map(|s| s.to_string());
    for p in &mut projects {
        if let Some(id) = p["id"].as_str() {
            p["agentLocked"] = tab_locks
                .get(id)
                .map(|l| serde_json::json!(l))
                .unwrap_or(serde_json::Value::Null);
        }
    }
    (projects, active_id)
}

/// Drop agent-tab entries and workspace fragments pointing at projects no
/// longer loaded (e.g. evicted by ProjectManager's capacity limit).
pub(crate) async fn prune_orphan_bindings(
    pm: &Arc<RwLock<ProjectManager>>,
    agent_tabs: &AgentTabs,
    workspace: &crate::mcp::Workspace,
) {
    let loaded: HashSet<String> = pm.read().await.all_project_ids().into_iter().collect();
    {
        let mut at = agent_tabs.write().await;
        at.retain(|k, _| loaded.contains(k));
    }
    workspace
        .write()
        .await
        .retain(|item| loaded.contains(&item.source_project));
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

pub(crate) fn with_project_meta(
    mut data: serde_json::Value,
    pm: &ProjectManager,
    project_id: &str,
) -> serde_json::Value {
    if let Some(map) = data.as_object_mut() {
        map.insert("projectId".to_string(), serde_json::json!(project_id));
        map.insert(
            "revision".to_string(),
            serde_json::json!(pm.revision(project_id)),
        );
        map.insert(
            "dirty".to_string(),
            serde_json::json!(pm.is_dirty(project_id)),
        );
    }
    data
}

/// Build a lightweight mutation response for feature edits: the calling window
/// only needs the updated feature list and the sidebar project list. Avoids
/// serializing the full project (~1.5MB, mostly the enzyme list) on every Apply.
pub(crate) async fn feature_mutation_response(
    pm: &Arc<RwLock<ProjectManager>>,
    agent_tabs: &AgentTabs,
    project_id: &str,
) -> serde_json::Value {
    let data = {
        let pm = pm.read().await;
        let data = match pm.get_project_by_id(project_id) {
            Some(p) => serde_json::json!({ "features": &p.features }),
            None => serde_json::json!({"error": "Project not found"}),
        };
        with_project_meta(data, &pm, project_id)
    };
    let (projects, active_id) = sidebar_project_list(pm, agent_tabs).await;
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
        "projectKind": &project.project_kind,
        "msaRows": &project.msa_rows,
        "msaAligned": project.msa_aligned,
        "referenceRowId": &project.reference_row_id,
        "features": &project.features,
        "primers": &project.primers,
        "alignments": &project.alignments,
        "methylation_systems": &project.methylation_systems,
        "methylation_overlap": project.methylation_overlap,
        "roi": &project.roi,
        "tracePath": &project.trace_path,
        "textColorsTop": &project.text_colors_top,
        "textColorsBottom": &project.text_colors_bottom,
        "enzymeCount": project.enzymes.len(),
        "enzymeFilter": filter,
        "enzymes": &enzymes,
    })
}

/// Emit the current project state + project list as a Tauri event.
pub(crate) async fn broadcast_project<R: Runtime>(
    app_handle: &AppHandle<R>,
    state: &State<'_, AppState>,
    source: Option<&str>,
) {
    broadcast_project_arcs(app_handle, &state.pm, &state.agent_tabs, source).await;
}

pub(crate) async fn broadcast_project_for_id<R: Runtime>(
    app_handle: &AppHandle<R>,
    state: &State<'_, AppState>,
    project_id: &str,
    source: Option<&str>,
) {
    broadcast_project_change_arcs(app_handle, &state.pm, &state.agent_tabs, project_id, source)
        .await;
}

/// Full synchronization for project collection changes and legacy callers.
pub(crate) async fn broadcast_project_arcs<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
) {
    broadcast_project_arcs_impl(app_handle, pm, agent_tabs, None, source).await;
}

/// Mutation synchronization: project summaries stay complete, but only the
/// changed project's sequence and derived data cross the IPC boundary.
pub(crate) async fn broadcast_project_change_arcs<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    agent_tabs: &AgentTabs,
    project_id: &str,
    source: Option<&str>,
) {
    broadcast_project_arcs_impl(app_handle, pm, agent_tabs, Some(project_id), source).await;
}

fn project_data_payload(
    pm: &ProjectManager,
    wanted: &HashSet<String>,
) -> serde_json::Map<String, serde_json::Value> {
    let params = ProjectParams {
        enzyme_filter: Some("all".to_string()),
        row_start: None,
        row_end: None,
        cpl: None,
    };
    let mut result = serde_json::Map::new();
    for id in pm.all_project_ids() {
        if !wanted.contains(&id) {
            continue;
        }
        if let Some(project) = pm.get_project_by_id(&id) {
            let mut data = filter_project(project, &params);
            if let Some(map) = data.as_object_mut() {
                map.insert("dirty".to_string(), serde_json::json!(pm.is_dirty(&id)));
            }
            result.insert(id, data);
        }
    }
    result
}

async fn broadcast_project_arcs_impl<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    agent_tabs: &AgentTabs,
    changed_project_id: Option<&str>,
    source: Option<&str>,
) {
    let (projects, active_id) = sidebar_project_list(pm, agent_tabs).await;
    let pm = pm.read().await;
    let mut payload = serde_json::json!({
        "projects": projects,
        "activeId": active_id,
        "source": source,
        "changedProjectId": changed_project_id,
    });

    if !app_handle.webview_windows().is_empty() {
        if let Some(ref active) = active_id {
            if changed_project_id.is_none_or(|id| id == active) {
                if let Some(project) = pm.get_project_by_id(active) {
                    let params = ProjectParams {
                        enzyme_filter: Some("all".to_string()),
                        row_start: None,
                        row_end: None,
                        cpl: None,
                    };
                    let mut filtered = filter_project(project, &params);
                    if let Some(map) = filtered.as_object_mut() {
                        map.insert("dirty".to_string(), serde_json::json!(pm.is_dirty(active)));
                    }
                    payload["data"] = filtered;
                }
            }
        }
        let wanted: HashSet<String> = if let Some(id) = changed_project_id {
            HashSet::from([id.to_string()])
        } else {
            payload["projects"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|p| p["id"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        payload["projectData"] = serde_json::Value::Object(project_data_payload(&pm, &wanted));
    }

    if let Some(id) = changed_project_id {
        if let Some(project) = pm.get_project_by_id(id) {
            crate::session_log::record_project_change(app_handle, id, project, source).await;
        }
    } else {
        let projects = pm
            .all_project_ids()
            .into_iter()
            .filter_map(|id| {
                pm.get_project_by_id(&id)
                    .map(|project| (id.clone(), (project.clone(), pm.is_dirty(&id))))
            })
            .collect();
        crate::session_log::sync_projects(app_handle, &projects).await;
    }

    let _ = app_handle.emit("project-update", payload);
}

#[cfg(test)]
mod tests {
    use super::*;
    use libregene_core::models::ProjectData;

    #[test]
    fn scoped_payload_contains_only_the_changed_project() {
        let mut pm = ProjectManager::new();
        for (id, sequence) in [("a", "AAAA"), ("b", "CCCC")] {
            pm.load(
                id,
                ProjectData {
                    sequence: sequence.to_string(),
                    length: 4,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let payload = project_data_payload(&pm, &HashSet::from(["a".to_string()]));
        assert_eq!(payload.len(), 1);
        assert_eq!(payload["a"]["sequence"], "AAAA");
        assert!(!payload.contains_key("b"));
    }
}
