use tauri::{AppHandle, Runtime, State};

use crate::payload::{filter_project, sidebar_project_list, ProjectParams};
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
        "canUndo": log.can_undo(&project_id),
        "canRedo": log.can_redo(&project_id),
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
        pm.get_project_mut_by_id(&project_id)
            .unwrap()
            .restore_document(before);
        pm.bump_revision(&project_id);
        pm.bump_template_revision(&project_id);
        if baseline_dirty || has_remaining {
            pm.restore_dirty(&project_id);
        } else {
            pm.mark_clean(&project_id);
        }
        log.entries(&project_id)
    };

    crate::recompute_after_sequence_change(
        &app_handle,
        &state.pm,
        &state.agent_tabs,
        Some(SESSION_ROLLBACK_SOURCE),
        &project_id,
    )
    .await?;

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
    let (projects, active_id) = sidebar_project_list(&state.pm, &state.agent_tabs).await;
    Ok(serde_json::json!({
        "status": "ok",
        "data": data,
        "projects": projects,
        "activeId": active_id,
        "entries": restored,
    }))
}

#[derive(Clone, Copy)]
pub(crate) enum HistoryDirection {
    Undo,
    Redo,
}

#[tauri::command]
pub(crate) async fn undo_project_edit(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    restore_project_history(app_handle, state, project_id, HistoryDirection::Undo).await
}

#[tauri::command]
pub(crate) async fn redo_project_edit(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    restore_project_history(app_handle, state, project_id, HistoryDirection::Redo).await
}

pub(crate) async fn restore_project_history<R: Runtime>(
    app_handle: AppHandle<R>,
    state: State<'_, AppState>,
    project_id: String,
    direction: HistoryDirection,
) -> Result<serde_json::Value, String> {
    {
        let mut pm = state.pm.write().await;
        if pm.get_project_by_id(&project_id).is_none() {
            return Ok(serde_json::json!({"error": "Project not found"}));
        }
        let mut log = state.session_edit_log.write().await;
        let restored = match direction {
            HistoryDirection::Undo => log.undo(&project_id),
            HistoryDirection::Redo => log.redo(&project_id),
        };
        let Some((document, dirty)) = restored else {
            return Ok(serde_json::json!({"error": "No edit to restore"}));
        };
        pm.get_project_mut_by_id(&project_id)
            .unwrap()
            .restore_document(document);
        pm.bump_revision(&project_id);
        pm.bump_template_revision(&project_id);
        if dirty {
            pm.restore_dirty(&project_id);
        } else {
            pm.mark_clean(&project_id);
        }
    }

    crate::recompute_after_sequence_change(
        &app_handle,
        &state.pm,
        &state.agent_tabs,
        Some(SESSION_ROLLBACK_SOURCE),
        &project_id,
    )
    .await?;
    {
        let log = state.session_edit_log.read().await;
        crate::session_log::emit_history(&app_handle, &log, &project_id);
    }

    let result = {
        let pm = state.pm.read().await;
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        let data = pm
            .get_project_by_id(&project_id)
            .map(|project| filter_project(project, &params))
            .unwrap_or(serde_json::json!({"error": "Project not found after history restore"}));
        crate::payload::with_mutation_meta(
            data,
            &pm,
            &project_id,
            &[
                crate::payload::MutationField::Sequence,
                crate::payload::MutationField::Features,
                crate::payload::MutationField::Primers,
                crate::payload::MutationField::Enzymes,
                crate::payload::MutationField::Alignments,
                crate::payload::MutationField::TextColorsTop,
                crate::payload::MutationField::TextColorsBottom,
                crate::payload::MutationField::Topology,
                crate::payload::MutationField::MsaRows,
                crate::payload::MutationField::MsaAligned,
                crate::payload::MutationField::ReferenceRowId,
                crate::payload::MutationField::MoleculeType,
            ],
        )
    };
    let (entries, can_undo, can_redo) = {
        let log = state.session_edit_log.read().await;
        (
            log.entries(&project_id),
            log.can_undo(&project_id),
            log.can_redo(&project_id),
        )
    };
    let (projects, active_id) = sidebar_project_list(&state.pm, &state.agent_tabs).await;
    let mut result = crate::payload::with_projects_list(result, &projects, active_id.as_deref());
    result["entries"] = serde_json::json!(entries);
    result["canUndo"] = serde_json::json!(can_undo);
    result["canRedo"] = serde_json::json!(can_redo);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::Manager;
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
            agent_tabs: Arc::new(RwLock::new(HashMap::new())),
            workspace: Arc::new(RwLock::new(Vec::new())),
            session_edit_log: Arc::new(RwLock::new(Default::default())),
            pending_opens: Arc::new(std::sync::Mutex::new(Vec::new())),
            tray_status: Arc::new(std::sync::Mutex::new(None)),
        });
        let state = app.state::<AppState>();
        crate::session_log::register_project(app.handle(), &pm, "p").await;

        crate::do_set_methylation(
            app.handle(),
            &pm,
            &state.agent_tabs,
            Some("main"),
            "p",
            vec!["dam".to_string()],
            Some(4),
        )
        .await
        .unwrap();
        assert!(!pm.read().await.is_dirty("p"));
        assert!(state.session_edit_log.read().await.entries("p").is_empty());

        {
            let mut manager = pm.write().await;
            let project = manager.get_project_mut_by_id("p").unwrap();
            project.sequence = "AACCAGGTT".to_string();
            project.length = 9;
            manager.mark_dirty("p");
        }
        broadcast_project_change_arcs(app.handle(), &pm, &state.agent_tabs, "p", None).await;
        {
            let mut manager = pm.write().await;
            manager.get_project_mut_by_id("p").unwrap().topology = "linear".to_string();
        }
        broadcast_project_change_arcs(app.handle(), &pm, &state.agent_tabs, "p", Some("main"))
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

    #[tokio::test]
    async fn undo_redo_commands_restore_document_and_refresh_derived_cache() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let initial = ProjectData {
            sequence: "GAATTC".to_string(),
            length: 6,
            ..Default::default()
        };
        pm.write().await.load("p", initial.clone()).unwrap();
        app.manage(AppState {
            pm: pm.clone(),
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
            project.sequence = "AAAATT".to_string();
            manager.bump_template_revision("p");
            manager.mark_dirty("p");
        }
        assert!(!pm.read().await.derived_is_current("p"));
        broadcast_project_change_arcs(app.handle(), &pm, &state.agent_tabs, "p", Some("main"))
            .await;

        let undone = restore_project_history(
            app.handle().clone(),
            state.clone(),
            "p".to_string(),
            HistoryDirection::Undo,
        )
        .await
        .unwrap();
        assert_eq!(undone["sequence"], initial.sequence);
        assert_eq!(undone["canRedo"], true);
        assert!(pm.read().await.derived_is_current("p"));
        assert!(!pm.read().await.is_dirty("p"));

        let redone = restore_project_history(
            app.handle().clone(),
            state.clone(),
            "p".to_string(),
            HistoryDirection::Redo,
        )
        .await
        .unwrap();
        assert_eq!(redone["sequence"], "AAAATT");
        assert_eq!(redone["canUndo"], true);
        assert!(pm.read().await.derived_is_current("p"));
        assert!(pm.read().await.is_dirty("p"));
    }

    #[tokio::test]
    async fn batch_history_option_records_one_undoable_change() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write().await.load("p", ProjectData::default()).unwrap();
        app.manage(AppState {
            pm: pm.clone(),
            agent_tabs: Arc::new(RwLock::new(HashMap::new())),
            workspace: Arc::new(RwLock::new(Vec::new())),
            session_edit_log: Arc::new(RwLock::new(Default::default())),
            pending_opens: Arc::new(std::sync::Mutex::new(Vec::new())),
            tray_status: Arc::new(std::sync::Mutex::new(None)),
        });
        let state = app.state::<AppState>();
        crate::session_log::register_project(app.handle(), &pm, "p").await;
        let feature = |id: &str| libregene_core::models::Feature {
            id: id.to_string(),
            name: id.to_string(),
            start: 0,
            end: 0,
            color: "#60A5FA".to_string(),
            ftype: "misc_feature".to_string(),
            segments: Vec::new(),
            strand: "+".to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        };
        crate::do_add_features_with_history(
            app.handle(),
            &pm,
            &state.agent_tabs,
            Some("main"),
            "p",
            vec![feature("a")],
            false,
        )
        .await
        .unwrap();
        crate::do_add_features_with_history(
            app.handle(),
            &pm,
            &state.agent_tabs,
            Some("main"),
            "p",
            vec![feature("b")],
            true,
        )
        .await
        .unwrap();
        let entries = state.session_edit_log.read().await.entries("p");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].summary, "Added 2 features");

        let restored = restore_project_history(
            app.handle().clone(),
            state,
            "p".to_string(),
            HistoryDirection::Undo,
        )
        .await
        .unwrap();
        assert_eq!(restored["features"].as_array().unwrap().len(), 0);
    }
}
