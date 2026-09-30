use tauri::{AppHandle, Manager, Runtime, State, WebviewUrl, WebviewWindowBuilder};

use libregene_core::file_io;

use crate::kernels::{
    do_activate_project, do_create_project, do_delete_project, do_open_file, do_save_file,
    NewFeatureInput,
};
use crate::payload::{
    broadcast_project, broadcast_project_arcs, filter_project, resolve_project_id,
    sidebar_project_list, ProjectParams,
};
use crate::state::{validate_user_path, AppState, SEQ_EXTS, TEXT_EXPORT_EXTS};

// ---------------------------------------------------------------------------
// Tauri commands — project
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn get_project(
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
    let project_id = match resolve_project_id(&state, &window_label).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    let pm = state.pm.read().await;
    match pm.get_project_by_id(&project_id) {
        Some(p) => {
            let mut filtered = filter_project(p, &params);
            if let Some(ref mut map) = filtered.as_object_mut() {
                map.insert("dirty".to_string(), serde_json::json!(pm.is_dirty(&project_id)));
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
    do_open_file(&state.pm, &state.window_projects, &state.agent_tabs, path, record_index).await
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
        &state.window_projects,
        &state.agent_tabs,
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
    path: String,
) -> Result<serde_json::Value, String> {
    match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => do_save_file(&state.pm, id, path).await,
        Err(e) => Ok(serde_json::json!({"error": e})),
    }
}

/// Write raw text to a file (used for My Enzymes export). Small payloads only.
#[tauri::command]
pub(crate) async fn write_text_file(path: String, contents: String) -> Result<serde_json::Value, String> {
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

// ---------------------------------------------------------------------------
// Tauri commands — multi-project management
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn get_projects(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let (projects, active_id) =
        sidebar_project_list(&state.pm, &state.window_projects, &state.agent_tabs).await;
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
            let needs_all = ["blunt", "overhang5", "overhang3", "iis", "rec4", "rec5", "rec6", "rec8p"]
                .contains(&filter);
            let params = ProjectParams {
                enzyme_filter: Some(if needs_all || filter == "all" { "all" } else { "unique" }.to_string()),
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
pub(crate) async fn delete_project(
    webview_window: tauri::WebviewWindow,
    app_handle: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    // The sidebar delete path keeps its existing semantics (the frontend runs
    // its own unsaved-changes confirmation before invoking this command), so
    // it passes force: true; the MCP close_project tool passes the caller's
    // flag through instead.
    do_delete_project(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        id,
        true,
    )
    .await
}

// ---------------------------------------------------------------------------
// Tauri commands — multi-window
// ---------------------------------------------------------------------------

/// Build a project window: same chrome as the main window, with cleanup of
/// the `window_projects` mapping when the window is destroyed. The frontend
/// activates the overlay titlebar and shows it.
pub(crate) fn spawn_project_window<R: Runtime>(
    app_handle: &AppHandle<R>,
    window_label: &str,
) -> Result<(), String> {
    let builder = WebviewWindowBuilder::new(
        app_handle,
        window_label,
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

    // When the window is destroyed, restore the project to the main window
    let ah = app_handle.clone();
    let lbl = window_label.to_string();
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
                broadcast_project_arcs(&ah, &state.pm, &state.window_projects, &state.agent_tabs, None).await;
            });
        }
    });
    Ok(())
}

/// Open the given project in a new OS window.
#[tauri::command]
pub(crate) async fn open_in_new_window(
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

    // A locked agent tab must not escape into a project window: project
    // windows do honor the edit lock, but splitting the project across
    // windows while the agent believes it owns it is confusing — make the
    // user unlock explicitly first.
    {
        let at = state.agent_tabs.read().await;
        if let Some(meta) = at.get(&project_id) {
            if meta.locked {
                return Ok(serde_json::json!({
                    "error": "Project is locked by an agent tab. Unlock it in the sidebar before opening in a new window."
                }));
            }
        }
    }

    // Cap the number of simultaneous project windows (each is a full
    // WebView; runaway opens exhaust resources).
    const MAX_PROJECT_WINDOWS: usize = 8;
    let open_count = app_handle
        .webview_windows()
        .keys()
        .filter(|l| l.starts_with("project-"))
        .count();
    if open_count >= MAX_PROJECT_WINDOWS {
        return Ok(serde_json::json!({
            "error": format!("Too many project windows open ({}). Close one first.", MAX_PROJECT_WINDOWS)
        }));
    }

    // Build a safe label for the new window (append timestamp for uniqueness)
    let safe = crate::mcp::sanitize_window_label(&project_id);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let window_label = format!("project-{safe}-{ts}");

    // Build the window FIRST: registering the mapping before a successful
    // build would leave a dangling entry behind on failure.
    spawn_project_window(&app_handle, &window_label)?;

    // Register the window → project mapping
    {
        let mut wp = state.window_projects.write().await;
        wp.insert(window_label.clone(), project_id);
    }

    // Broadcast so the main window updates its sidebar immediately
    broadcast_project(&app_handle, &state, Some(webview_window.label())).await;

    Ok(serde_json::json!({"status": "ok", "windowLabel": window_label}))
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
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
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

    // Keep the window and agent-tab bindings pointed at the renamed project
    // (each lock taken in its own scope — never held together with pm).
    {
        let mut wp = state.window_projects.write().await;
        for v in wp.values_mut() {
            if *v == old_id {
                *v = new_id.clone();
            }
        }
    }
    {
        let mut at = state.agent_tabs.write().await;
        if let Some(meta) = at.remove(&old_id) {
            at.insert(new_id.clone(), meta);
        }
    }

    broadcast_project(&app_handle, &state, Some(webview_window.label())).await;
    Ok(serde_json::json!({"status": "ok", "newId": new_id}))
}
