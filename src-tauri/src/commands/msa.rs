use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use libregene_core::file_io;
use libregene_core::models::{MsaRow, ProjectData};
use libregene_core::project::ProjectManager;
use tauri::{AppHandle, State};
use tokio::sync::RwLock;

use crate::payload::{
    broadcast_project_for_id, filter_project, prune_orphan_bindings, sidebar_project_list,
    with_project_meta, with_projects_list, ProjectParams,
};
use crate::state::{normalize_project_path, validate_user_path, AgentTabs, AppState};

fn response(pm: &ProjectManager, project_id: &str) -> Result<serde_json::Value, serde_json::Value> {
    let Some(project) = pm.get_project_by_id(project_id) else {
        return Err(serde_json::json!({"error": "Project not found"}));
    };
    let params = ProjectParams {
        enzyme_filter: Some("all".to_string()),
        row_start: None,
        row_end: None,
        cpl: None,
    };
    Ok(with_project_meta(
        filter_project(project, &params),
        pm,
        project_id,
    ))
}

async fn response_with_projects(
    pm: &Arc<RwLock<ProjectManager>>,
    agent_tabs: &AgentTabs,
    project_id: &str,
) -> serde_json::Value {
    let data = {
        let pm = pm.read().await;
        match response(&pm, project_id) {
            Ok(data) => data,
            Err(error) => return error,
        }
    };
    let (projects, active_id) = sidebar_project_list(pm, agent_tabs).await;
    with_projects_list(data, &projects, active_id.as_deref())
}

fn normalize_rows(mut rows: Vec<MsaRow>) -> Result<Vec<MsaRow>, String> {
    if rows.is_empty() {
        return Err("MSA must contain at least one sequence".to_string());
    }
    let mut ids = HashSet::new();
    for (index, row) in rows.iter_mut().enumerate() {
        row.id = row.id.trim().to_string();
        if row.id.is_empty() {
            row.id = format!("row-{}", index + 1);
        }
        if !ids.insert(row.id.clone()) {
            return Err(format!("Duplicate MSA row id: {}", row.id));
        }
        row.name = row.name.trim().to_string();
        if row.name.is_empty() {
            row.name = format!("Sequence {}", index + 1);
        }
        row.description = row.description.replace(['\r', '\n'], " ");
        row.sequence = row
            .sequence
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if row.sequence.is_empty() {
            return Err(format!("Sequence '{}' is empty", row.name));
        }
        if let Some(c) = row
            .sequence
            .chars()
            .find(|c| !c.is_ascii_alphabetic() && *c != '*' && *c != '?')
        {
            return Err(format!(
                "Invalid character '{}' in sequence '{}'",
                c, row.name
            ));
        }
        row.aligned_sequence = row
            .aligned_sequence
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if !row.aligned_sequence.is_empty()
            && (row.aligned_sequence.replace('-', "") != row.sequence
                || row
                    .aligned_sequence
                    .chars()
                    .any(|c| !c.is_ascii_alphabetic() && c != '-' && c != '*' && c != '?'))
        {
            return Err(format!(
                "Aligned sequence '{}' does not match its ungapped sequence",
                row.name
            ));
        }
    }
    Ok(rows)
}

fn apply_rows(
    project: &mut ProjectData,
    rows: Vec<MsaRow>,
    molecule_type: &str,
    aligned: bool,
    reference_row_id: Option<String>,
) -> Result<(), String> {
    if project.project_kind != "msa" {
        return Err("Project is not a multiple sequence alignment".to_string());
    }
    let mut rows = normalize_rows(rows)?;
    let aligned = aligned
        && rows.iter().all(|row| !row.aligned_sequence.is_empty())
        && rows.first().is_some_and(|first| {
            rows.iter()
                .all(|row| row.aligned_sequence.len() == first.aligned_sequence.len())
        });
    if !aligned {
        for row in &mut rows {
            row.aligned_sequence.clear();
        }
    }
    project.length = if aligned {
        rows.first()
            .map(|row| row.aligned_sequence.len())
            .unwrap_or(0)
    } else {
        rows.iter().map(|row| row.sequence.len()).max().unwrap_or(0)
    } as i64;
    project.msa_rows = rows;
    project.msa_aligned = aligned;
    project.molecule_type = match molecule_type {
        "rna" | "protein" => molecule_type.to_string(),
        _ => "dna".to_string(),
    };
    project.reference_row_id =
        reference_row_id.filter(|id| project.msa_rows.iter().any(|row| row.id == *id));
    Ok(())
}

#[tauri::command]
pub(crate) async fn open_msa_fasta(
    state: State<'_, AppState>,
    path: String,
) -> Result<serde_json::Value, String> {
    validate_user_path(
        &path,
        &["fasta", "fa", "fna", "fas", "ffn", "fsa", "faa", "frn"],
    )?;
    let path = normalize_project_path(&path)?;
    let parse_path = PathBuf::from(&path);
    let project = tokio::task::spawn_blocking(move || {
        file_io::fasta::parse_msa_fasta(&parse_path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?;
    let project = match project {
        Ok(project) => project,
        Err(error) => return Ok(serde_json::json!({"error": error})),
    };
    let mut result = {
        let mut pm = state.pm.write().await;
        if pm.is_dirty(&path) {
            return Ok(serde_json::json!({
                "error": "Project is already open with unsaved changes; save or discard them before reopening the file"
            }));
        }
        if let Err(error) = pm.load(&path, project) {
            return Ok(serde_json::json!({"error": error}));
        }
        pm.mark_clean(&path);
        response(&pm, &path).unwrap_or_else(|error| error)
    };
    result["id"] = serde_json::json!(path);
    prune_orphan_bindings(&state.pm, &state.agent_tabs, &state.workspace).await;
    let (projects, active_id) = sidebar_project_list(&state.pm, &state.agent_tabs).await;
    Ok(with_projects_list(result, &projects, active_id.as_deref()))
}

#[tauri::command]
pub(crate) async fn create_msa_project(
    state: State<'_, AppState>,
    name: String,
    sequence: String,
    molecule_type: Option<String>,
) -> Result<serde_json::Value, String> {
    let id = format!(
        "untitled-msa-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let row = MsaRow {
        id: "row-1".to_string(),
        name: "Sequence 1".to_string(),
        sequence,
        ..Default::default()
    };
    let molecule_type = molecule_type.unwrap_or_else(|| {
        file_io::fasta::detect_msa_molecule_type(std::slice::from_ref(&row))
    });
    let mut project = ProjectData {
        name: if name.trim().is_empty() {
            "Untitled MSA".to_string()
        } else {
            name.trim().to_string()
        },
        topology: "linear".to_string(),
        project_kind: "msa".to_string(),
        ..Default::default()
    };
    if let Err(error) = apply_rows(&mut project, vec![row], &molecule_type, false, None) {
        return Ok(serde_json::json!({"error": error}));
    }
    let result = {
        let mut pm = state.pm.write().await;
        if let Err(error) = pm.load(&id, project) {
            return Ok(serde_json::json!({"error": error}));
        }
        pm.mark_dirty(&id);
        response(&pm, &id).unwrap_or_else(|error| error)
    };
    let mut result = result;
    result["id"] = serde_json::json!(id);
    prune_orphan_bindings(&state.pm, &state.agent_tabs, &state.workspace).await;
    let (projects, active_id) = sidebar_project_list(&state.pm, &state.agent_tabs).await;
    Ok(with_projects_list(result, &projects, active_id.as_deref()))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn update_msa(
    webview_window: tauri::WebviewWindow,
    app_handle: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    rows: Vec<MsaRow>,
    molecule_type: String,
    aligned: bool,
    reference_row_id: Option<String>,
) -> Result<serde_json::Value, String> {
    do_update_msa(
        webview_window,
        app_handle,
        state,
        project_id,
        rows,
        molecule_type,
        aligned,
        reference_row_id,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn do_update_msa<R: tauri::Runtime>(
    webview_window: tauri::WebviewWindow<R>,
    app_handle: AppHandle<R>,
    state: State<'_, AppState>,
    project_id: String,
    rows: Vec<MsaRow>,
    molecule_type: String,
    aligned: bool,
    reference_row_id: Option<String>,
) -> Result<serde_json::Value, String> {
    {
        let mut pm = state.pm.write().await;
        let Some(project) = pm.get_project_mut_by_id(&project_id) else {
            return Ok(serde_json::json!({"error": "Project not found"}));
        };
        if let Err(error) = apply_rows(project, rows, &molecule_type, aligned, reference_row_id) {
            return Ok(serde_json::json!({"error": error}));
        }
        pm.mark_dirty(&project_id);
    }
    broadcast_project_for_id(
        &app_handle,
        &state,
        &project_id,
        Some(webview_window.label()),
    )
    .await;
    Ok(response_with_projects(&state.pm, &state.agent_tabs, &project_id).await)
}

fn muscle_file_name() -> String {
    let triple = match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        ("aarch64", "windows") => "aarch64-pc-windows-msvc",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        _ => "x86_64-unknown-linux-gnu",
    };
    format!(
        "muscle-{}{}",
        triple,
        if cfg!(windows) { ".exe" } else { "" }
    )
}

fn muscle_path() -> PathBuf {
    if let Some(path) = std::env::var_os("LIBREGENE_MUSCLE_PATH") {
        return PathBuf::from(path);
    }
    let file_name = muscle_file_name();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let bundled = parent.join(&file_name);
            if bundled.is_file() {
                return bundled;
            }
        }
    }
    let development = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(&file_name);
    if development.is_file() {
        return development;
    }
    PathBuf::from(if cfg!(windows) {
        "muscle.exe"
    } else {
        "muscle"
    })
}

fn run_muscle(rows: Vec<MsaRow>, molecule_type: String) -> Result<Vec<String>, String> {
    if rows.len() == 1 {
        return Ok(vec![rows[0].sequence.clone()]);
    }
    let unique = format!(
        "libregene-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let input = std::env::temp_dir().join(format!("{unique}.msa.fa"));
    let output = std::env::temp_dir().join(format!("{unique}-aligned.msa.fa"));
    let project = ProjectData {
        project_kind: "msa".to_string(),
        molecule_type,
        msa_rows: rows,
        ..Default::default()
    };
    file_io::fasta::write_msa_fasta(&project, &input, true).map_err(|e| e.to_string())?;
    let result = Command::new(muscle_path())
        .arg("-align")
        .arg(&input)
        .arg("-output")
        .arg(&output)
        .arg("-threads")
        .arg("1")
        .arg("-quiet")
        .output();
    let output_result = match result {
        Ok(process) if process.status.success() => file_io::fasta::parse_msa_fasta(&output)
            .map_err(|e| format!("Cannot parse MUSCLE output: {e}")),
        Ok(process) => Err(format!(
            "MUSCLE failed: {}",
            String::from_utf8_lossy(&process.stderr).trim()
        )),
        Err(error) => Err(format!(
            "Cannot run bundled MUSCLE at {}: {error}",
            muscle_path().display()
        )),
    };
    std::fs::remove_file(&input).ok();
    std::fs::remove_file(&output).ok();
    let aligned = output_result?;
    if aligned.msa_rows.len() != project.msa_rows.len() {
        return Err("MUSCLE returned a different number of sequences".to_string());
    }
    let mut by_sequence: HashMap<(String, String), VecDeque<String>> = HashMap::new();
    for row in aligned.msa_rows {
        by_sequence
            .entry((row.name, row.sequence))
            .or_default()
            .push_back(row.aligned_sequence);
    }
    let mut sequences = Vec::new();
    for input_row in &project.msa_rows {
        let key = (input_row.name.clone(), input_row.sequence.clone());
        let Some(sequence) = by_sequence
            .get_mut(&key)
            .and_then(|queue| queue.pop_front())
        else {
            return Err(format!(
                "MUSCLE output cannot be mapped safely to sequence '{}'",
                input_row.name
            ));
        };
        sequences.push(sequence);
    }
    Ok(sequences)
}

#[tauri::command]
pub(crate) async fn align_msa(
    webview_window: tauri::WebviewWindow,
    app_handle: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    do_align_msa(webview_window, app_handle, state, project_id).await
}

async fn do_align_msa<R: tauri::Runtime>(
    webview_window: tauri::WebviewWindow<R>,
    app_handle: AppHandle<R>,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    let (rows, molecule_type, revision) = {
        let pm = state.pm.read().await;
        let Some(project) = pm.get_project_by_id(&project_id) else {
            return Ok(serde_json::json!({"error": "Project not found"}));
        };
        if project.project_kind != "msa" {
            return Ok(serde_json::json!({"error": "Project is not an MSA"}));
        }
        (
            project.msa_rows.clone(),
            project.molecule_type.clone(),
            pm.revision(&project_id),
        )
    };
    let aligned = tokio::task::spawn_blocking(move || run_muscle(rows, molecule_type))
        .await
        .map_err(|e| format!("task join error: {e}"))?;
    let sequences = match aligned {
        Ok(sequences) => sequences,
        Err(error) => return Ok(serde_json::json!({"error": error})),
    };
    {
        let mut pm = state.pm.write().await;
        if pm.revision(&project_id) != revision {
            return Ok(serde_json::json!({
                "error": "The alignment changed while MUSCLE was running; run Align again"
            }));
        }
        let Some(project) = pm.get_project_mut_by_id(&project_id) else {
            return Ok(serde_json::json!({"error": "Project not found"}));
        };
        for (row, sequence) in project.msa_rows.iter_mut().zip(sequences) {
            row.aligned_sequence = sequence;
        }
        project.msa_aligned = true;
        project.length = project
            .msa_rows
            .first()
            .map(|row| row.aligned_sequence.len())
            .unwrap_or(0) as i64;
        pm.mark_dirty(&project_id);
    }
    broadcast_project_for_id(
        &app_handle,
        &state,
        &project_id,
        Some(webview_window.label()),
    )
    .await;
    Ok(response_with_projects(&state.pm, &state.agent_tabs, &project_id).await)
}

#[tauri::command]
pub(crate) async fn export_msa_fasta(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    unaligned: bool,
) -> Result<serde_json::Value, String> {
    validate_user_path(&path, &["fa", "fasta"])?;
    let path = normalize_project_path(&path)?;
    let project = {
        let pm = state.pm.read().await;
        pm.get_project_by_id(&project_id).cloned()
    };
    let Some(project) = project else {
        return Ok(serde_json::json!({"error": "Project not found"}));
    };
    if project.project_kind != "msa" {
        return Ok(serde_json::json!({"error": "Project is not an MSA"}));
    }
    let write_path = PathBuf::from(&path);
    let result = tokio::task::spawn_blocking(move || {
        file_io::fasta::write_msa_fasta(&project, &write_path, unaligned)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?;
    if let Err(error) = result {
        return Ok(serde_json::json!({"error": error.to_string()}));
    }
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Ok(serde_json::json!({"status": "ok", "path": path, "bytesWritten": bytes}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    fn row(name: &str, sequence: &str) -> MsaRow {
        MsaRow {
            id: format!("id-{name}"),
            name: name.to_string(),
            sequence: sequence.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn apply_rows_validates_alignment_and_reference() {
        let mut project = ProjectData {
            project_kind: "msa".to_string(),
            ..Default::default()
        };
        let mut a = row("a", "ACGT");
        a.aligned_sequence = "AC-GT".to_string();
        let mut b = row("b", "AGT");
        b.aligned_sequence = "A--GT".to_string();
        apply_rows(
            &mut project,
            vec![a, b],
            "dna",
            true,
            Some("id-a".to_string()),
        )
        .unwrap();
        assert!(project.msa_aligned);
        assert_eq!(project.length, 5);
        assert_eq!(project.reference_row_id.as_deref(), Some("id-a"));

        let invalid = row("bad", "ACGT-");
        assert!(apply_rows(&mut project, vec![invalid], "dna", false, None).is_err());
    }

    #[tokio::test]
    async fn desktop_ipc_create_align_save_reopen_and_export() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        app.manage(AppState {
            pm: pm.clone(),
            agent_tabs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workspace: Arc::new(RwLock::new(Vec::new())),
            session_edit_log: Arc::new(RwLock::new(Default::default())),
            pending_opens: Arc::new(std::sync::Mutex::new(Vec::new())),
            tray_status: Arc::new(std::sync::Mutex::new(None)),
        });
        let window = tauri::WebviewWindowBuilder::new(
            &app,
            "main",
            tauri::WebviewUrl::default(),
        )
        .build()
        .unwrap();
        let state = app.state::<AppState>();
        let created = create_msa_project(
            state.clone(),
            "Workflow".to_string(),
            "ACGTACGT".to_string(),
            Some("dna".to_string()),
        )
        .await
        .unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        let updated = do_update_msa(
            window.clone(),
            app.handle().clone(),
            state.clone(),
            id.clone(),
            vec![row("a", "ACGTACGT"), row("b", "ACGTT")],
            "dna".to_string(),
            false,
            None,
        )
        .await
        .unwrap();
        assert!(updated.get("error").is_none(), "{updated}");
        let aligned = do_align_msa(
            window.clone(),
            app.handle().clone(),
            state.clone(),
            id.clone(),
        )
        .await
        .unwrap();
        assert_eq!(aligned["msaAligned"], true);
        let rows = aligned["msaRows"].as_array().unwrap();
        assert_eq!(rows[0]["alignedSequence"], "ACGTACGT");
        assert_eq!(rows[1]["alignedSequence"], "ACGT---T");

        let dir = std::env::temp_dir().join(format!(
            "libregene-msa-workflow-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("workflow.msa.fa");
        let saved = crate::commands::projects::save_file_impl(
            app.handle().clone(),
            state.clone(),
            id,
            path.to_string_lossy().into_owned(),
            None,
        )
        .await
        .unwrap();
        assert!(saved.get("error").is_none(), "{saved}");
        let project_id = saved["projectId"].as_str().unwrap().to_string();
        let reopened = open_msa_fasta(state.clone(), path.to_string_lossy().into_owned())
            .await
            .unwrap();
        assert_eq!(reopened["msaAligned"], true);
        assert_eq!(reopened["msaRows"][1]["alignedSequence"], "ACGT---T");

        let unaligned_path = dir.join("workflow-unaligned.fa");
        let exported = export_msa_fasta(
            state,
            project_id,
            unaligned_path.to_string_lossy().into_owned(),
            true,
        )
        .await
        .unwrap();
        assert!(exported.get("error").is_none(), "{exported}");
        let unaligned = std::fs::read_to_string(&unaligned_path).unwrap();
        assert!(!unaligned.contains('-'));
        assert!(unaligned.contains(">a\nACGTACGT\n"));
        assert!(unaligned.contains(">b\nACGTT\n"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn muscle_output_maps_back_to_input_order() {
        let rows = vec![
            row("a", "ACGTACGT"),
            row("b", "ACGTT"),
            row("c", "ACGTTCGT"),
        ];
        let aligned = run_muscle(rows.clone(), "dna".to_string()).unwrap();
        assert_eq!(aligned.len(), rows.len());
        assert!(aligned.iter().all(|s| s.len() == aligned[0].len()));
        for (input, output) in rows.iter().zip(aligned) {
            assert_eq!(output.replace('-', ""), input.sequence);
        }
    }
}
