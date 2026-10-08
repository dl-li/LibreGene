use tauri::{AppHandle, State};

use libregene_core::enzyme;
use libregene_core::file_io;
use libregene_core::models::ProjectData;
use libregene_core::primer;

use crate::kernels::{
    alignment_reject_message, commit_computed_alignment, do_add_alignment_seq, do_remove_alignment,
    parse_align_algorithm,
};
use crate::payload::{
    broadcast_project, filter_project, prune_orphan_bindings, resolve_project_id,
    with_projects_list, ProjectParams,
};
use crate::state::{validate_user_path, AppState, SEQ_EXTS};

// ---------------------------------------------------------------------------
// Tauri commands — alignments
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn add_alignment(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    path: String,
    algorithm: Option<String>,
) -> Result<serde_json::Value, String> {
    // The drag-and-drop importer and the multi-file dialog both funnel here,
    // and the frontend extension filter is not a trust boundary — align with
    // the MCP-side add_alignment, which validates every path.
    let ext = validate_user_path(&path, SEQ_EXTS)?;
    let algorithm = parse_align_algorithm(algorithm.as_deref());
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    let project_clone = {
        let pm = state.pm.read().await;
        pm.get_project_by_id(&project_id).cloned()
    };
    let project_clone = match project_clone {
        Some(p) => p,
        None => return Ok(serde_json::json!({"error": "Project not found"})),
    };
    if !project_clone.is_dna() {
        return Ok(serde_json::json!({"error": format!(
            "Alignments are only supported for DNA projects; project '{}' is a {} project",
            project_id, project_clone.molecule_type
        )}));
    }
    let snapshot_ids: Vec<String> = project_clone.alignments.iter().map(|a| a.id.clone()).collect();

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
        let mut aln = libregene_core::align::align_read_checked_with(
            &p.sequence,
            &read_project.sequence,
            circular,
            algorithm,
        )
        .map_err(alignment_reject_message)?;
        aln.name = name;
        aln.id = libregene_core::align::next_alignment_id(&p.alignments);
        if ext == "ab1" {
            aln.trace_path = Some(path.clone());
        }
        p.alignments.push(aln);
        Ok(p)
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;

    let computed = match computed {
        Ok(p) => p,
        Err(e) if e.starts_with("No significant alignment found") => return Err(e),
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    commit_computed_alignment(&state.pm, &project_id, &snapshot_ids, computed).await;

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
pub(crate) async fn add_alignment_seq(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    name: String,
    seq: String,
    algorithm: Option<String>,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };
    let algorithm = parse_align_algorithm(algorithm.as_deref());

    do_add_alignment_seq(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        name,
        seq,
        None,
        algorithm,
    )
    .await
}

#[tauri::command]
pub(crate) async fn remove_alignment(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    alignment_id: String,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_remove_alignment(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        alignment_id,
    )
    .await
}

/// Load the chromatogram (trace channels + peak positions) of an .ab1 file.
/// Traces are fetched lazily by the frontend and never travel inside the
/// project payload — a single read is ~100 KB of sample data.
/// Chromatogram model/parsing ported from GenePad (https://github.com/GenePad),
/// provided by the GenePad team / https://github.com/Masterchiefm.
#[tauri::command]
pub(crate) async fn get_chromatogram(path: String) -> Result<libregene_core::models::Chromatogram, String> {
    validate_user_path(&path, &["ab1"])?;
    tokio::task::spawn_blocking(move || {
        libregene_core::file_io::ensure_within_size_limit(std::path::Path::new(&path))
            .map_err(|e| e.to_string())?;
        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
        libregene_core::file_io::ab1::extract_chromatogram(&data).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?
}

// ---------------------------------------------------------------------------
// Tauri commands — SnapGene history snapshots
// ---------------------------------------------------------------------------

/// Whether `project_id` refers to a project opened from a `.dna` file (the
/// project id of a file-opened project is its path).
fn is_snapgene_dna_path(project_id: &str) -> bool {
    std::path::Path::new(project_id)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dna"))
}

/// The SnapGene history of `project_id` for the Snapshots dialog: the
/// in-memory subtree a snapshot project carries wins, otherwise the source
/// `.dna` file is re-read on demand — snapshot sequences never travel with
/// get_project/broadcast payloads. `entries` is null when there is none.
#[tauri::command]
pub(crate) async fn get_snapgene_history(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    let file_backed;
    {
        let pm = state.pm.read().await;
        let project = pm
            .get_project_by_id(&project_id)
            .ok_or("project not found")?;
        if let Some(history) = &project.snapgene_history {
            return Ok(serde_json::json!({ "entries": history.entries }));
        }
        file_backed = project.is_dna() && is_snapgene_dna_path(&project_id);
    }
    if !file_backed {
        return Ok(serde_json::json!({ "entries": null }));
    }
    let path = std::path::PathBuf::from(&project_id);
    let result = tokio::task::spawn_blocking(move || {
        libregene_core::file_io::ensure_within_size_limit(&path).map_err(|e| e.to_string())?;
        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
        Ok::<_, String>(
            libregene_core::file_io::snapgene_history::parse_snapgene_history(&data)
                .map(|history| history.entries),
        )
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;
    match result {
        Ok(entries) => Ok(serde_json::json!({ "entries": entries })),
        Err(e) => Ok(serde_json::json!({ "error": e })),
    }
}

/// Open one SnapGene history snapshot as a new in-memory project: sequence +
/// snapshot-time features/primers/topology, plus the node's complete subtree
/// history carried along (root = the snapshot) so nested snapshots stay
/// openable — GenePad's "open snapshot as independent document" semantics.
/// Works both on `.dna`-file projects and on snapshot projects themselves.
/// The virtual id `snapshot-<millis>` has no file extension, so the first
/// save always goes through Save As.
#[tauri::command]
pub(crate) async fn open_snapgene_snapshot(
    state: State<'_, AppState>,
    project_id: String,
    node_id: u32,
) -> Result<serde_json::Value, String> {
    enum Source {
        Memory(libregene_core::models::SnapGeneHistoryData),
        File(std::path::PathBuf),
    }
    let source = {
        let pm = state.pm.read().await;
        let project = pm
            .get_project_by_id(&project_id)
            .ok_or("project not found")?;
        if let Some(history) = &project.snapgene_history {
            Source::Memory(history.clone())
        } else if project.is_dna() && is_snapgene_dna_path(&project_id) {
            Source::File(std::path::PathBuf::from(&project_id))
        } else {
            return Ok(serde_json::json!({
                "error": "this project has no SnapGene history (only .dna files and opened snapshots carry one)"
            }));
        }
    };
    let result = tokio::task::spawn_blocking(move || {
        let history = match source {
            Source::Memory(history) => history,
            Source::File(path) => {
                libregene_core::file_io::ensure_within_size_limit(&path)
                    .map_err(|e| e.to_string())?;
                let data = std::fs::read(&path).map_err(|e| e.to_string())?;
                libregene_core::file_io::snapgene_history::parse_snapgene_history(&data)
                    .ok_or("this file carries no SnapGene history")?
            }
        };
        let mut project =
            libregene_core::file_io::snapgene_history::snapshot_project_from_history(
                &history, node_id,
            )
            .ok_or("snapshot not found (its sequence may be unavailable)")?;
        enzyme::recompute(&mut project);
        primer::recompute(&mut project);
        libregene_core::translate::refresh_feature_translations(&mut project);
        Ok::<_, String>(project)
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;
    let project = match result {
        Ok(p) => p,
        Err(e) => return Ok(serde_json::json!({ "error": e })),
    };

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let id = format!("snapshot-{}", ts);

    let params = ProjectParams {
        enzyme_filter: Some("all".to_string()),
        row_start: None,
        row_end: None,
        cpl: None,
    };
    let mut return_data = filter_project(&project, &params);
    if let Some(ref mut map) = return_data.as_object_mut() {
        map.insert("id".to_string(), serde_json::json!(id));
    }

    let (projects, active_id) = {
        let mut pm = state.pm.write().await;
        if let Err(e) = pm.load(&id, project) {
            return Ok(serde_json::json!({ "error": e }));
        }
        (pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };
    // The load may have evicted another project; drop its bindings.
    prune_orphan_bindings(&state.pm, &state.window_projects, &state.agent_tabs).await;

    Ok(with_projects_list(return_data, &projects, active_id.as_deref()))
}
