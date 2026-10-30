use tauri::{AppHandle, Runtime, State};

use crate::payload::{
    ProjectParams, broadcast_project_for_id, filter_project, sidebar_project_list,
};
use crate::session_log::SESSION_ROLLBACK_SOURCE;
use crate::state::AppState;

#[tauri::command]
pub(crate) async fn get_session_edit_log(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    crate::session_log::register_project(&app_handle, &state.pm, &project_id).await;
    let log = state.session_edit_log.read().await;
    Ok(serde_json::json!({
        "projectId": project_id,
        "entries": log.entries(&project_id),
    }))
}

#[tauri::command]
pub(crate) async fn rollback_session_edit(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    entry_id: u64,
) -> Result<serde_json::Value, String> {
    rollback_session_edit_impl(app_handle, state, project_id, entry_id).await
}

async fn rollback_session_edit_impl<R: Runtime>(
    app_handle: AppHandle<R>,
    state: State<'_, AppState>,
    project_id: String,
    entry_id: u64,
) -> Result<serde_json::Value, String> {
    let restored = {
        let mut pm = state.pm.write().await;
        if pm.get_project_by_id(&project_id).is_none() {
            return Ok(serde_json::json!({"error": "Project not found"}));
        }
        let mut log = state.session_edit_log.write().await;
        let Some((before, baseline_dirty, has_remaining)) = log.rollback(&project_id, entry_id)
        else {
            return Ok(serde_json::json!({"error": "This edit is no longer in the session log"}));
        };
        *pm.get_project_mut_by_id(&project_id).unwrap() = before;
        if baseline_dirty || has_remaining {
            pm.mark_dirty(&project_id);
        } else {
            pm.mark_clean(&project_id);
        }
        log.entries(&project_id)
    };

    broadcast_project_for_id(
        &app_handle,
        &state,
        &project_id,
        Some(SESSION_ROLLBACK_SOURCE),
    )
    .await;

    let data = {
        let pm = state.pm.read().await;
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        let mut data = pm
            .get_project_by_id(&project_id)
            .map(|project| filter_project(project, &params))
            .unwrap_or(serde_json::json!({"error": "Project not found after rollback"}));
        if let Some(object) = data.as_object_mut() {
            object.insert(
                "dirty".to_string(),
                serde_json::json!(pm.is_dirty(&project_id)),
            );
        }
        data
    };
    let (projects, active_id) =
        sidebar_project_list(&state.pm, &state.window_projects, &state.agent_tabs).await;
    Ok(serde_json::json!({
        "status": "ok",
        "data": data,
        "projects": projects,
        "activeId": active_id,
        "entries": restored,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tauri::Manager;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tokio::sync::RwLock;

    use libregene_core::models::ProjectData;
    use libregene_core::project::ProjectManager;

    use super::*;
    use crate::payload::broadcast_project_change_arcs;

    #[tokio::test]
    async fn broadcast_records_origins_and_command_rolls_back_project() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let initial = ProjectData {
            sequence: "AACCGGTT".to_string(),
            length: 8,
            ..Default::default()
        };
        pm.write().await.load("p", initial.clone()).unwrap();
        app.manage(AppState {
            pm: pm.clone(),
            window_projects: Arc::new(RwLock::new(HashMap::new())),
            agent_tabs: Arc::new(RwLock::new(HashMap::new())),
            workspace: Arc::new(RwLock::new(Vec::new())),
            session_edit_log: Arc::new(RwLock::new(Default::default())),
            pending_opens: Arc::new(std::sync::Mutex::new(Vec::new())),
            tray_status: Arc::new(std::sync::Mutex::new(None)),
        });
        let state = app.state::<AppState>();
        crate::session_log::register_project(app.handle(), &pm, "p").await;

        {
            let mut manager = pm.write().await;
            let project = manager.get_project_mut_by_id("p").unwrap();
            project.sequence = "AACCAGGTT".to_string();
            project.length = 9;
            manager.mark_dirty("p");
        }
        broadcast_project_change_arcs(
            app.handle(),
            &pm,
            &state.window_projects,
            &state.agent_tabs,
            "p",
            None,
        )
        .await;
        {
            let mut manager = pm.write().await;
            manager.get_project_mut_by_id("p").unwrap().topology = "linear".to_string();
        }
        broadcast_project_change_arcs(
            app.handle(),
            &pm,
            &state.window_projects,
            &state.agent_tabs,
            "p",
            Some("main"),
        )
        .await;

        let entries = state.session_edit_log.read().await.entries("p");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].origin, crate::session_log::EditOrigin::Agent);
        assert_eq!(entries[1].origin, crate::session_log::EditOrigin::User);
        let first_id = entries[0].id;

        let result =
            rollback_session_edit_impl(app.handle().clone(), state, "p".to_string(), first_id)
                .await
                .unwrap();
        assert_eq!(result["status"], "ok");
        assert_eq!(result["data"]["sequence"], initial.sequence);
        assert_eq!(result["entries"].as_array().unwrap().len(), 0);
        let manager = pm.read().await;
        assert_eq!(
            manager.get_project_by_id("p").unwrap().topology,
            initial.topology
        );
        assert!(!manager.is_dirty("p"));
    }
}
