use tauri::{AppHandle, State};

use libregene_core::file_io;

use crate::kernels::{
    do_activate_project, do_create_project, do_delete_project, do_open_file, do_save_file,
    NewFeatureInput,
};
use crate::payload::{
    broadcast_project, filter_project, resolve_project_id, sidebar_project_list, ProjectParams,
};
use crate::state::{validate_user_path, AppState, SEQ_EXTS, TEXT_EXPORT_EXTS};

// ---------------------------------------------------------------------------
// Tauri commands — project
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn get_project(
    state: State<'_, AppState>,
    enzyme_filter: Option<String>,
    row_start: Option<i64>,
    row_end: Option<i64>,
    cpl: Option<i64>,
) -> Result<serde_json::Value, String> {
    let params = ProjectParams {
        enzyme_filter,
        row_start,
        row_end,
        cpl,
    };

    let project_id = match resolve_project_id(&state).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    let pm = state.pm.read().await;
    match pm.get_project_by_id(&project_id) {
        Some(p) => {
            let mut filtered = filter_project(p, &params);
            if let Some(ref mut map) = filtered.as_object_mut() {
                map.insert(
                    "dirty".to_string(),
                    serde_json::json!(pm.is_dirty(&project_id)),
                );
            }
            Ok(filtered)
        }
        None => Ok(serde_json::json!({"error": "Project not found"})),
    }
}

#[tauri::command]
pub(crate) async fn open_file(
    state: State<'_, AppState>,
    path: String,
    record_index: Option<usize>,
) -> Result<serde_json::Value, String> {
    do_open_file(
        &state.pm,
        &state.agent_tabs,
        &state.workspace,
        path,
        record_index,
    )
    .await
}

/// Lightweight scan of a FASTA file's records (name + length only) so the
/// frontend can offer to split a multi-record file into separate projects
/// before opening it.
#[tauri::command]
pub(crate) async fn peek_fasta_records(path: String) -> Result<serde_json::Value, String> {
    let ext = validate_user_path(&path, SEQ_EXTS)?;
    if !matches!(
        ext.as_str(),
        "fasta" | "fa" | "fna" | "fas" | "ffn" | "fsa" | "faa" | "frn" | "seq"
    ) {
        return Ok(serde_json::json!({"records": []}));
    }
    let path_buf = std::path::PathBuf::from(&path);
    let molecule_type = if ext == "faa" { "protein" } else { "dna" };
    let result = tokio::task::spawn_blocking(move || {
        file_io::fasta::parse_fasta_all_with_molecule_type(&path_buf, molecule_type)
            .map(|records| {
                records
                    .into_iter()
                    .map(|r| {
                    serde_json::json!({"name": r.name, "length": r.length, "moleculeType": r.molecule_type})
                })
                    .collect::<Vec<_>>()
            })
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;
    match result {
        Ok(records) => Ok(serde_json::json!({"records": records})),
        Err(e) => Ok(serde_json::json!({"error": e})),
    }
}

/// Drain OS-opened file paths queued before the frontend was ready (cold
/// start via Open With / double-click / second-instance forwarding). The
/// frontend opens each through the normal `open_file` command.
#[tauri::command]
pub(crate) fn take_pending_opens(state: State<'_, AppState>) -> Vec<String> {
    state
        .pending_opens
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default()
}

/// Create a new in-memory project from a pasted sequence (Empty-page "New
/// Sequence" dialog). `molecule_type` is dna | rna | protein; `topology`
/// circular | linear (RNA/Protein forced linear); `features` are detected
/// annotation hits converted to 0-based segments by the frontend. Returns the
/// same shape as `open_file` plus the generated project id.
#[tauri::command]
pub(crate) async fn create_project(
    state: State<'_, AppState>,
    name: String,
    sequence: String,
    molecule_type: String,
    topology: String,
    features: Vec<NewFeatureInput>,
) -> Result<serde_json::Value, String> {
    do_create_project(
        &state.pm,
        &state.agent_tabs,
        &state.workspace,
        name,
        sequence,
        molecule_type,
        topology,
        features,
    )
    .await
}

#[tauri::command]
pub(crate) async fn save_file(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    project_id: String,
    path: String,
) -> Result<serde_json::Value, String> {
    save_file_impl(
        app_handle,
        state,
        project_id,
        path,
        Some(webview_window.label()),
    )
    .await
}

async fn save_file_impl<R: tauri::Runtime>(
    app_handle: AppHandle<R>,
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    source: Option<&str>,
) -> Result<serde_json::Value, String> {
    let result = do_save_file(&state.pm, project_id.clone(), path.clone(), true).await?;
    if result.get("error").is_none() {
        let final_id = result
            .get("projectId")
            .and_then(|v| v.as_str())
            .unwrap_or(&path)
            .to_string();
        if result.get("rekeyedFrom").and_then(|v| v.as_str()).is_some() {
            let mut at = state.agent_tabs.write().await;
            if let Some(meta) = at.remove(&project_id) {
                at.insert(final_id.clone(), meta);
            }
            drop(at);
            crate::mcp::rekey_fragments_of(&state.workspace, &project_id, &final_id).await;
            crate::session_log::rekey_project(&app_handle, &project_id, &final_id).await;
            broadcast_project(&app_handle, &state, source).await;
        }
        crate::session_log::mark_project_saved(&app_handle, &state.pm, &final_id).await;
    }
    Ok(result)
}

/// Write raw text to a file (used for My Enzymes export). Small payloads only.
#[tauri::command]
pub(crate) async fn write_text_file(
    path: String,
    contents: String,
) -> Result<serde_json::Value, String> {
    validate_user_path(&path, TEXT_EXPORT_EXTS)?;
    // "Small payloads only" per the original intent — cap explicitly so a
    // runaway caller can't push megabyte strings through IPC into disk.
    const MAX_TEXT_EXPORT_BYTES: usize = 1024 * 1024;
    if contents.len() > MAX_TEXT_EXPORT_BYTES {
        return Ok(serde_json::json!({
            "error": format!(
                "Export too large ({} bytes > {}); this command is for small text payloads",
                contents.len(),
                MAX_TEXT_EXPORT_BYTES
            )
        }));
    }
    std::fs::write(&path, contents).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"status": "ok"}))
}

/// Read a small UTF-8 text file (used for settings import). Mirrors
/// write_text_file's extension policy and size cap.
#[tauri::command]
pub(crate) async fn read_text_file(path: String) -> Result<serde_json::Value, String> {
    validate_user_path(&path, TEXT_EXPORT_EXTS)?;
    const MAX_TEXT_IMPORT_BYTES: u64 = 1024 * 1024;
    let contents = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        if size > MAX_TEXT_IMPORT_BYTES {
            return Err(format!(
                "File too large ({} bytes > {}); this command is for small text files",
                size, MAX_TEXT_IMPORT_BYTES
            ));
        }
        std::fs::read_to_string(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(serde_json::json!({"contents": contents}))
}

// ---------------------------------------------------------------------------
// Tauri commands — multi-project management
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn get_projects(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let (projects, active_id) = sidebar_project_list(&state.pm, &state.agent_tabs).await;
    Ok(serde_json::json!({
        "projects": projects,
        "activeId": active_id,
    }))
}

#[tauri::command]
pub(crate) async fn get_project_by_id(
    state: State<'_, AppState>,
    id: String,
    enzyme_filter: Option<String>,
) -> Result<serde_json::Value, String> {
    let pm = state.pm.read().await;
    match pm.get_project_by_id(&id) {
        Some(p) => {
            let filter = enzyme_filter.as_deref().unwrap_or("all");
            let needs_all = [
                "blunt",
                "overhang5",
                "overhang3",
                "iis",
                "rec4",
                "rec5",
                "rec6",
                "rec8p",
            ]
            .contains(&filter);
            let params = ProjectParams {
                enzyme_filter: Some(
                    if needs_all || filter == "all" {
                        "all"
                    } else {
                        "unique"
                    }
                    .to_string(),
                ),
                row_start: None,
                row_end: None,
                cpl: None,
            };
            let mut filtered = filter_project(p, &params);
            if let Some(ref mut map) = filtered.as_object_mut() {
                map.insert("dirty".to_string(), serde_json::json!(pm.is_dirty(&id)));
            }
            Ok(filtered)
        }
        None => Ok(serde_json::json!({"error": "project not found"})),
    }
}

#[tauri::command]
pub(crate) async fn activate_project(
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    do_activate_project(&state.pm, id).await
}

#[tauri::command]
pub(crate) async fn delete_project(
    webview_window: tauri::WebviewWindow,
    app_handle: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    // The sidebar delete path keeps its existing semantics (the frontend runs
    // its own unsaved-changes confirmation before invoking this command), so
    // it passes force: true for that case;
    // flag through instead.
    do_delete_project(
        &app_handle,
        &state.pm,
        &state.agent_tabs,
        &state.workspace,
        Some(webview_window.label()),
        id,
        true,
    )
    .await
}

/// Rename a project's ID (called after Save As to re-key the project).
#[tauri::command]
pub(crate) async fn rekey_project(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    old_id: String,
    new_id: String,
) -> Result<serde_json::Value, String> {
    let ok = {
        let mut pm = state.pm.write().await;
        pm.rename_id(&old_id, &new_id)
    };
    if !ok {
        return Ok(serde_json::json!({"error": "Rename failed (target may already exist)"}));
    }

    // Keep the agent-tab binding pointed at the renamed project.
    {
        let mut at = state.agent_tabs.write().await;
        if let Some(meta) = at.remove(&old_id) {
            at.insert(new_id.clone(), meta);
        }
    }
    crate::mcp::rekey_fragments_of(&state.workspace, &old_id, &new_id).await;
    crate::session_log::rekey_project(&app_handle, &old_id, &new_id).await;

    broadcast_project(&app_handle, &state, Some(webview_window.label())).await;
    Ok(serde_json::json!({"status": "ok", "newId": new_id}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use libregene_core::models::ProjectData;
    use libregene_core::project::ProjectManager;
    use tauri::Manager;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn background_project_save_uses_requested_id_not_active_project() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let dir =
            std::env::temp_dir().join(format!("libregene-save-target-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a_path = dir.join("a.gbk");
        let a_id = crate::normalize_project_path(a_path.to_str().unwrap()).unwrap();
        let project = |sequence: &str| ProjectData {
            name: "test".to_string(),
            sequence: sequence.to_string(),
            length: sequence.len() as i64,
            topology: "linear".to_string(),
            ..Default::default()
        };
        pm.write().await.load(&a_id, project("AAAA")).unwrap();
        pm.write().await.mark_dirty(&a_id);
        pm.write().await.load("b", project("CCCC")).unwrap();
        app.manage(AppState {
            pm: pm.clone(),
            agent_tabs: Arc::new(RwLock::new(HashMap::new())),
            workspace: Arc::new(RwLock::new(Vec::new())),
            session_edit_log: Arc::new(RwLock::new(Default::default())),
            pending_opens: Arc::new(std::sync::Mutex::new(Vec::new())),
            tray_status: Arc::new(std::sync::Mutex::new(None)),
        });

        let out = save_file_impl(
            app.handle().clone(),
            app.state::<AppState>(),
            a_id,
            a_path.to_string_lossy().into_owned(),
            None,
        )
        .await
        .unwrap();
        assert!(out.get("error").is_none(), "{out}");
        let saved = file_io::parse_file(&a_path).unwrap();
        assert_eq!(saved.sequence, "AAAA");
        assert_eq!(
            pm.read().await.get_project_by_id("b").unwrap().sequence,
            "CCCC"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
