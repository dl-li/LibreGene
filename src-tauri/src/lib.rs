use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::RwLock;

use libregene_core::enzyme;
use libregene_core::file_io;
use libregene_core::models::{Feature, Primer, ProjectData, Segment};
use libregene_core::primer;
use libregene_core::project::ProjectManager;

mod mcp;

// ---------------------------------------------------------------------------
// Path validation for filesystem-touching commands
// ---------------------------------------------------------------------------

/// Reject path-traversal and require a sequence-file extension.
/// Returns Ok(normalized lowercase extension without dot) if acceptable.
///
/// We don't lock paths to a fixed directory (users open/save anywhere), but we
/// do refuse parent-directory traversal components and require a known
/// sequence/export extension so a raw path string from an untrusted caller
/// (e.g. the MCP bridge) can't read/write arbitrary files.
fn validate_user_path(path: &str, allowed_exts: &[&str]) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("path is empty".to_string());
    }
    // Reject any `..` component (works for both separators and mixed forms
    // after we normalize). canonicalize() would also resolve symlinks, but
    // the file may not exist yet (save target), so we inspect components.
    let p = std::path::Path::new(trimmed);
    for comp in p.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => {
                return Err("parent-directory traversal (..) is not allowed".to_string())
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) | Component::CurDir => {}
        }
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !allowed_exts.iter().any(|a| *a == ext) {
        return Err(format!(
            "file extension '.{}' is not allowed (allowed: {})",
            ext,
            allowed_exts.join(", ")
        ));
    }
    Ok(ext)
}

const SEQ_EXTS: &[&str] = &[
    "gbk", "gb", "genbank", "gbf", "gbff",
    "dna", "rna", "prot",
    "gpt", "gp", "gpe", "gpff",
    "fasta", "fa", "fna", "fas", "ffn", "fsa", "faa", "frn",
    "ab1", "seq",
];
const TEXT_EXPORT_EXTS: &[&str] = &["txt", "csv", "json"];
/// Output extensions accepted by MCP `optimize_cds`'s `output_path`
/// (.gbk/.gb/.genbank → DNA GenBank, .gpt → protein GenBank).
const CODON_OUTPUT_EXTS: &[&str] = &["gbk", "gb", "genbank", "gpt"];

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

/// Per-agent-tab metadata. Agent tabs (projects bound by the MCP
/// `open_project` tool) stay in the main window's sidebar; this map adds
/// the lock state.
#[derive(Clone)]
pub struct AgentTabMeta {
    pub locked: bool,
}

/// Agent-bound projects, keyed by project id.
pub type AgentTabs = Arc<RwLock<HashMap<String, AgentTabMeta>>>;

pub struct AppState {
    pub pm: Arc<RwLock<ProjectManager>>,
    /// Maps window labels to project IDs for multi-window support.
    /// Main window ("main") is NOT in this map — it uses the active project.
    /// Project windows ("project-{sanitized_id}") are mapped to their project.
    pub window_projects: Arc<RwLock<HashMap<String, String>>>,
    /// MCP-agent-bound projects, keyed by project id. Lock order: never take
    /// this lock while holding `pm` or `window_projects`.
    pub agent_tabs: AgentTabs,
    /// Paths handed to us by the OS (Open With / double-click / second
    /// instance) that the frontend hasn't consumed yet. The frontend drains
    /// this via `take_pending_opens` on mount so cold-start events that
    /// arrive before the webview is ready are not lost.
    pub pending_opens: Arc<std::sync::Mutex<Vec<String>>>,
    /// The disabled status line at the top of the tray menu, kept so
    /// `set_mcp_config` can refresh its text when the MCP config changes.
    pub tray_status: Arc<std::sync::Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>>,
}

/// Filter OS-supplied open targets (argv / Opened events) to existing files
/// with a supported sequence extension.
fn collect_open_targets<I: IntoIterator<Item = String>>(args: I) -> Vec<String> {
    args.into_iter()
        .filter(|a| !a.starts_with('-'))
        .filter(|a| {
            let p = std::path::Path::new(a);
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| SEQ_EXTS.contains(&e.to_ascii_lowercase().as_str()))
                    .unwrap_or(false)
        })
        .collect()
}

/// Queue OS-opened paths for the frontend and notify it, then focus the main
/// window. The frontend opens each path through the normal `open_file` path.
fn queue_open_targets(app: &AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    if let Ok(mut pending) = state.pending_opens.lock() {
        for p in &paths {
            if !pending.contains(p) {
                pending.push(p.clone());
            }
        }
    }
    for p in paths {
        let _ = app.emit("file-opened", p);
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
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
/// - Project windows look up their mapping in `window_projects`; a miss means
///   the project was evicted — never fall back to the main window's active
///   project, or the window's mutations would silently hit the wrong file.
/// - The main window falls back to `ProjectManager::active_id()`.
async fn resolve_project_id(
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
fn excluded_project_ids(wp: &tokio::sync::RwLockReadGuard<HashMap<String, String>>) -> HashSet<String> {
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

/// The main-window sidebar project list: projects bound to dedicated project
/// windows filtered out, activeId adjusted, and `agentLocked` injected from
/// the agent-tabs map. This is the exact same payload shape
/// `broadcast_project_arcs` emits, so mutation responses and broadcasts never
/// diverge. Lock order: agent_tabs is read (and dropped) BEFORE pm/wp; pm is
/// taken before window_projects.
async fn sidebar_project_list(
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
async fn prune_orphan_bindings(
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
async fn feature_mutation_response(
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
fn filter_project(project: &ProjectData, params: &ProjectParams) -> serde_json::Value {
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
        &state.agent_tabs,
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

// ---------------------------------------------------------------------------
// Shared mutation/analysis cores — called by both the Tauri commands and the
// MCP server so every path goes through identical logic (recompute, dirty,
// broadcast). Each mirrors the command it was extracted from.
// ---------------------------------------------------------------------------

async fn do_open_file(
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    path: String,
) -> Result<serde_json::Value, String> {
    validate_user_path(&path, SEQ_EXTS)?;
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
                // Reopening a file whose in-memory copy has unsaved changes
                // would silently discard them — refuse instead.
                if pm.is_dirty(&id) {
                    return Ok(serde_json::json!({
                        "error": "Project is already open with unsaved changes; save or discard them before reopening the file"
                    }));
                }
                if let Err(e) = pm.load(&id, project) {
                    return Ok(serde_json::json!({"error": e}));
                }
                // A fresh load reflects the file on disk — clear any stale
                // dirty marker from a previous in-memory incarnation.
                pm.mark_clean(&id);
                (pm.list_projects(), pm.active_id().map(|s| s.to_string()))
            };
            // The load may have evicted another project; drop its bindings.
            prune_orphan_bindings(pm, wp, agent_tabs).await;

            Ok(with_projects_list(return_data, &projects, active_id.as_deref()))
        }
        Err(e) => Ok(serde_json::json!({"error": e})),
    }
}

/// A feature to embed in a newly created project (see `create_project`).
/// Coordinates are 0-based inclusive; origin-wrapping features arrive as
/// multiple `segments`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewFeatureInput {
    pub name: String,
    pub ftype: String,
    pub color: String,
    pub strand: String,
    pub segments: Vec<Segment>,
}

/// Convert a [`NewFeatureInput`] into a project [`Feature`], deriving the
/// overall bounds from the segment list. None for empty segments. Bounds come
/// from the first segment's start and the last segment's end (segments are in
/// encoding order), so an origin-wrapping feature keeps its `start > end`
/// semantics instead of being flattened by min/max.
fn feature_from_input(input: NewFeatureInput, id: &str) -> Option<Feature> {
    if input.segments.is_empty() {
        return None;
    }
    let start = input.segments.first().map(|s| s.start).unwrap_or(0);
    let end = input.segments.last().map(|s| s.end).unwrap_or(0);
    Some(Feature {
        id: id.to_string(),
        name: input.name,
        start,
        end,
        color: input.color,
        ftype: input.ftype,
        segments: input.segments,
        strand: input.strand,
        notes: String::new(),
        translation: String::new(),
        qualifiers: Vec::new(),
    })
}

/// Create a new in-memory project from pasted sequence (empty-page "New
/// Sequence"). The virtual id is `untitled-{millis}` (no extension) so the
/// frontend canDirectSave check fails and the first save goes through
/// Save As + rekey_project. Returns the same shape as `do_open_file` plus the
/// generated project id.
async fn do_create_project(
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    name: String,
    sequence: String,
    molecule_type: String,
    topology: String,
    features: Vec<NewFeatureInput>,
) -> Result<serde_json::Value, String> {
    let seq = sequence.to_ascii_uppercase();
    if seq.is_empty() {
        return Ok(serde_json::json!({"error": "Sequence is empty"}));
    }
    let molecule = molecule_type.trim().to_ascii_lowercase();
    let molecule_type = match molecule.as_str() {
        "rna" | "protein" => molecule,
        _ => "dna".to_string(),
    };
    // RNA/Protein are single-strand: always linear. DNA honors the toggle.
    let topo = topology.trim().to_ascii_lowercase();
    let topology = if molecule_type != "dna" {
        "linear".to_string()
    } else if topo == "linear" {
        "linear".to_string()
    } else {
        "circular".to_string()
    };

    let length = seq.len() as i64;
    for f in &features {
        for s in &f.segments {
            if s.start < 0 || s.end < 0 || s.start >= length || s.end >= length {
                return Ok(serde_json::json!({
                    "error": format!(
                        "feature '{}' segment {}..{} is out of bounds for a sequence of length {} (1-based inclusive)",
                        f.name, s.start + 1, s.end + 1, length
                    )
                }));
            }
        }
        // Encoding order (same shape annotate.rs emits): linear segments
        // ascending; an origin-wrapping feature leads with its tail, the one
        // descending transition marking the origin. Out-of-order segments
        // would make the first/last-derived bounds wrong (a phantom wrap).
        let descents = f
            .segments
            .windows(2)
            .filter(|w| w[1].start < w[0].start)
            .count();
        let ordered = f.segments.iter().all(|s| s.start <= s.end)
            && descents <= 1
            && (descents == 0
                || f.segments.first().unwrap().start > f.segments.last().unwrap().end);
        if !ordered {
            return Ok(serde_json::json!({
                "error": format!(
                    "feature '{}' segments are not in encoding order (ascending starts; a wrapping feature leads with its tail)",
                    f.name
                )
            }));
        }
    }

    let features: Vec<Feature> = features
        .into_iter()
        .enumerate()
        .filter_map(|(i, f)| feature_from_input(f, &format!("feature_{}", i)))
        .collect();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let id = format!("untitled-{}", ts);

    let mut project = ProjectData {
        name,
        sequence: seq,
        length,
        topology,
        molecule_type,
        features,
        ..Default::default()
    };
    let computed = tokio::task::spawn_blocking(move || {
        enzyme::recompute(&mut project);
        primer::recompute(&mut project);
        libregene_core::translate::refresh_feature_translations(&mut project);
        project
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;

    let params = ProjectParams {
        enzyme_filter: Some("all".to_string()),
        row_start: None,
        row_end: None,
        cpl: None,
    };
    let mut return_data = filter_project(&computed, &params);
    if let Some(ref mut map) = return_data.as_object_mut() {
        map.insert("id".to_string(), serde_json::json!(id));
    }

    let (projects, active_id) = {
        let mut pm = pm.write().await;
        if let Err(e) = pm.load(&id, computed) {
            return Ok(serde_json::json!({"error": e}));
        }
        (pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };
    // The load may have evicted another project; drop its bindings.
    prune_orphan_bindings(pm, wp, agent_tabs).await;

    Ok(with_projects_list(return_data, &projects, active_id.as_deref()))
}

async fn do_save_file(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: String,
    path: String,
) -> Result<serde_json::Value, String> {
    let ext = validate_user_path(&path, CODON_OUTPUT_EXTS)?;
    let save_path = std::path::PathBuf::from(&path);
    let project = {
        let pm = pm.read().await;
        pm.get_project_by_id(&project_id).cloned()
    };
    match project {
        Some(ref p) => {
            // Protein projects cannot round-trip through the DNA GenBank
            // writer (amino-acid letters would corrupt the file) — force .gpt.
            if p.molecule_type == "protein" && ext != "gpt" {
                return Ok(serde_json::json!({
                    "error": "Protein projects must be saved as .gpt (GenBank protein format); .gbk/.gb cannot represent an amino-acid sequence"
                }));
            }
            let p = p.clone();
            let write_path = save_path.clone();
            let write_ext = ext.clone();
            let result = tokio::task::spawn_blocking(move || {
                if write_ext == "gpt" {
                    file_io::gpt::write_gpt(&p, &write_path)
                } else {
                    file_io::gbk::write_gbk(&p, &write_path)
                }
            })
            .await
            .map_err(|e| format!("task join error: {}", e))?;
            match result {
                Ok(()) => {
                    let bytes = std::fs::metadata(&save_path).map(|m| m.len()).unwrap_or(0);
                    let mut pm = pm.write().await;
                    pm.mark_clean(&project_id);
                    Ok(serde_json::json!({"status": "ok", "bytesWritten": bytes}))
                }
                Err(e) => Ok(serde_json::json!({"error": e.to_string()})),
            }
        }
        None => Ok(serde_json::json!({"error": "Project not found"})),
    }
}

/// CAS write-back of the off-lock recomputed fields after a sequence edit:
/// land only when the live sequence still equals the one the recompute ran
/// on — a concurrent edit that landed in between triggered its own recompute,
/// so writing stale results back would clobber the newer state. Primers merge
/// by id: a concurrently added primer survives, a concurrently deleted one
/// stays gone, existing ones get their fresh binding sites.
fn merge_recomputed_after_edit(live: &mut ProjectData, computed: ProjectData) -> bool {
    if live.sequence != computed.sequence {
        return false;
    }
    live.enzymes = computed.enzymes;
    for cp in computed.primers {
        if let Some(pr) = live.primers.iter_mut().find(|pr| pr.id == cp.id) {
            *pr = cp;
        }
    }
    // Refresh translations on features still at their cloned coordinates;
    // features edited concurrently keep their state.
    for cf in &computed.features {
        if let Some(f) = live
            .features
            .iter_mut()
            .find(|f| f.id == cf.id && f.start == cf.start && f.end == cf.end)
        {
            f.translation = cf.translation.clone();
        }
    }
    true
}

/// Clone the project, recompute enzymes/primers/translations off-lock, CAS
/// write the computed fields back and broadcast. Shared by
/// do_update_sequence and the MCP edit_sequence path (which writes the
/// sequence inside its own critical section, then calls this).
pub(crate) async fn recompute_after_sequence_change<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
) -> Result<(), String> {
    let project_clone = {
        let pm = pm.read().await;
        pm.get_project_by_id(project_id).map(|p| ProjectData {
            // enzyme::recompute below rebuilds the whole enzyme list from the
            // embedded database, so cloning the existing Vec<Enzyme> (the
            // single heaviest field on large plasmids) is pure waste.
            // primers are NOT skipped: primer::recompute reads the existing
            // primer definitions to recompute their binding sites.
            enzymes: Vec::new(),
            name: p.name.clone(),
            definition: p.definition.clone(),
            keywords: p.keywords.clone(),
            lab_host: p.lab_host.clone(),
            sequence: p.sequence.clone(),
            length: p.length,
            topology: p.topology.clone(),
            molecule_type: p.molecule_type.clone(),
            features: p.features.clone(),
            primers: p.primers.clone(),
            alignments: p.alignments.clone(),
            methylation_systems: p.methylation_systems.clone(),
            methylation_overlap: p.methylation_overlap,
            roi: p.roi,
        })
    };

    if let Some(mut p) = project_clone {
        let computed = tokio::task::spawn_blocking(move || {
            enzyme::recompute(&mut p);
            primer::recompute(&mut p);
            libregene_core::translate::refresh_feature_translations(&mut p);
            p
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        // Write back ONLY the computed fields (CAS on the sequence) so
        // concurrent edits made while spawn_blocking ran (e.g. a feature
        // rename, a newer sequence edit) are not clobbered; this also leaves
        // the global active project untouched (no open_project).
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            merge_recomputed_after_edit(p, computed);
        }
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;
    Ok(())
}

/// Replace a project's sequence (and optionally its whole primer list, e.g.
/// an undo/redo snapshot — `Some` replaces `p.primers` wholesale and binding
/// sites are recomputed; `None` keeps the current primers), recompute
/// enzymes/primers/translations off-lock, write the computed fields back
/// under a sequence CAS (a stale recompute is dropped), mark dirty and
/// broadcast.
#[allow(clippy::too_many_arguments)]
async fn do_update_sequence<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: String,
    sequence: String,
    primers: Option<Vec<Primer>>,
) -> Result<serde_json::Value, String> {
    {
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.sequence = sequence;
            p.length = p.sequence.len() as i64;
            if let Some(primers) = primers {
                p.primers = primers;
            }
            pm.mark_dirty(&project_id);
        }
    }

    recompute_after_sequence_change(app_handle, pm, wp, agent_tabs, source, &project_id).await?;

    let result = {
        let pm = pm.read().await;
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        pm.get_project_by_id(&project_id)
            .map(|p| filter_project(p, &params))
            .unwrap_or(serde_json::json!({"error": "Project not found after update"}))
    };
    let (projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;

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

/// Close (unload) a project and broadcast. `force` is required to close a
/// project with unsaved changes — the dirty check and the removal happen in
/// the same `pm` write critical section so a concurrent mutation cannot slip
/// in between (TOCTOU).
async fn do_delete_project<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    id: String,
    force: bool,
) -> Result<serde_json::Value, String> {
    enum Outcome {
        Closed,
        Dirty,
        Missing,
    }
    let outcome = {
        let mut pm = pm.write().await;
        if pm.get_project_by_id(&id).is_none() {
            Outcome::Missing
        } else if pm.is_dirty(&id) && !force {
            Outcome::Dirty
        } else {
            pm.close_project(&id);
            Outcome::Closed
        }
    };
    match outcome {
        Outcome::Closed => {
            // Collect the window labels bound to this project, then drop the
            // mappings. Keeping a window open after its project is deleted
            // leaves a ghost webview whose commands would fail — and before
            // the evicted-window fix, silently redirected to the MAIN
            // window's active project, overwriting a different file.
            let orphan_labels: Vec<String> = {
                let mut wp = wp.write().await;
                let labels: Vec<String> = wp
                    .iter()
                    .filter(|(_, v)| *v == &id)
                    .map(|(k, _)| k.clone())
                    .collect();
                wp.retain(|_, v| v != &id);
                labels
            };
            {
                let mut at = agent_tabs.write().await;
                at.remove(&id);
            }
            broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;
            for label in orphan_labels {
                if let Some(win) = app_handle.get_webview_window(&label) {
                    let _ = win.close();
                }
            }
            Ok(serde_json::json!({"status": "ok"}))
        }
        Outcome::Dirty => Ok(serde_json::json!({
            "error": "Project has unsaved changes — save_file first, or pass force: true to discard them"
        })),
        Outcome::Missing => Ok(serde_json::json!({"error": "project not found"})),
    }
}

/// Add or replace features (by id) and broadcast. The location string has
/// already been parsed into the Feature by the caller.
async fn do_add_features<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
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

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    Ok(feature_mutation_response(pm, wp, agent_tabs, project_id).await)
}

async fn do_delete_feature<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    id: String,
) -> Result<serde_json::Value, String> {
    let feats = {
        let mut pm = pm.write().await;
        let exists = pm
            .get_project_by_id(project_id)
            .map(|p| p.features.iter().any(|f| f.id == id))
            .unwrap_or(false);
        if !exists {
            return Err(format!("Feature not found: {}", id));
        }
        let feats: Vec<Feature> = pm
            .get_project_by_id(project_id)
            .map(|p| p.features.iter().filter(|f| f.id != id).cloned().collect())
            .unwrap_or_default();
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            p.features = feats.clone();
        }
        pm.mark_dirty(project_id);
        feats
    };

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    let (projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;
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
    agent_tabs: &AgentTabs,
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
        match pm.get_project_mut_by_id(project_id) {
            Some(p) => match p.features.iter_mut().find(|f| f.id == feature_id) {
                Some(f) => apply(f)?,
                None => return Err(format!("Feature not found: {}", feature_id)),
            },
            None => return Err(format!("Project not found: {}", project_id)),
        }
        pm.mark_dirty(project_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    Ok(feature_mutation_response(pm, wp, agent_tabs, project_id).await)
}

/// Cheap structural equality for the primers list (Primer has no PartialEq).
/// The snapshot/merge write-back guard only needs to detect whether the list
/// membership changed (add/remove/replace), not binding-site internals.
fn primers_equal(a: &[Primer], b: &[Primer]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.id == y.id && x.name == y.name && x.primer_seq == y.primer_seq)
}

async fn do_add_primer<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    primer: Primer,
) -> Result<serde_json::Value, String> {
    // Snapshot → merge → recompute (off-lock) → write-back. The write-back
    // only lands when the primers list is unchanged since the snapshot;
    // otherwise a concurrent add slipped in and we retry onto the fresh list
    // (binding sites depend only on the template, which does not change).
    loop {
        let name_conflict = {
            let pm = pm.read().await;
            pm.get_project_by_id(project_id).map(|p| {
                if p.primers.iter().any(|p| p.id != primer.id && p.name == primer.name) {
                    Some("primer")
                } else if p.features.iter().any(|f| f.name == primer.name) {
                    Some("feature")
                } else {
                    None
                }
            }).unwrap_or(None)
        };
        if let Some(kind) = name_conflict {
            return Ok(serde_json::json!({"error": format!("Primer name '{}' already exists as a {}", primer.name, kind)}));
        }

        let snapshot = {
            let pm = pm.read().await;
            pm.get_project_by_id(project_id).map(|p| {
                (p.sequence.clone(), p.topology.clone(), p.primers.clone())
            })
        };
        let Some((template, topology, existing)) = snapshot else {
            return Ok(serde_json::json!({"error": "Project not found"}));
        };

        let mut merged = existing.clone();
        if let Some(pos) = merged.iter().position(|p| p.id == primer.id) {
            merged[pos] = primer.clone();
        } else {
            merged.push(primer.clone());
        }

        let updated = tokio::task::spawn_blocking(move || {
            libregene_core::primer::align::recompute_all_primers(&template, &topology, &merged)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        let wrote = {
            let mut pm = pm.write().await;
            match pm.get_project_mut_by_id(project_id) {
                Some(p) if primers_equal(&p.primers, &existing) => {
                    p.primers = updated;
                    pm.mark_dirty(project_id);
                    true
                }
                // The list changed under us (another window added a primer):
                // re-merge onto the fresh state instead of overwriting.
                Some(_) => false,
                None => return Ok(serde_json::json!({"error": "Project not found"})),
            }
        };
        if wrote {
            break;
        }
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

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
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    id: String,
) -> Result<serde_json::Value, String> {
    let primers = {
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
        primers
    };

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    let (projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;
    Ok(with_projects_list(
        serde_json::json!({ "primers": primers }),
        &projects,
        active_id.as_deref(),
    ))
}

async fn do_set_methylation<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
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

        // recompute_methylation_only only touches the enzyme list — write
        // back just that field so concurrent edits survive, and leave the
        // global active project untouched (no open_project).
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            p.enzymes = computed.enzymes;
        }
        pm.mark_dirty(project_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    let result = {
        let pm = pm.read().await;
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        pm.get_project_by_id(project_id)
            .map(|p| filter_project(p, &params))
            .unwrap_or(serde_json::json!({"error": "Project not found after methylation"}))
    };
    let (projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;

    Ok(with_projects_list(result, &projects, active_id.as_deref()))
}

/// Human-readable rejection reason for a failed read alignment. The prefix is
/// kept stable so callers can detect the family (`starts_with`).
fn alignment_reject_message(r: libregene_core::align::AlignReject) -> String {
    use libregene_core::align::{AlignReject, MIN_ALIGNED_LEN, MIN_IDENTITY};
    match r {
        AlignReject::LowIdentity { identity, .. } => format!(
            "No significant alignment found: identity {:.3} is below the {:.2} minimum",
            identity, MIN_IDENTITY
        ),
        AlignReject::TooShort { span } => format!(
            "No significant alignment found: aligned span {} bp is below the {} bp minimum",
            span, MIN_ALIGNED_LEN
        ),
        AlignReject::NoSignificantAlignment => "No significant alignment found".to_string(),
    }
}

/// CAS/merge write-back for a freshly computed alignment (same discipline as
/// do_add_primer): land the computed list wholesale only when the live
/// alignment ids still match the snapshot the new alignment's id was
/// allocated from; otherwise a concurrent add/remove slipped in — append the
/// new alignment onto the live list with an id re-allocated from the CURRENT
/// list, so the other writer's entry survives.
async fn commit_computed_alignment(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
    snapshot_ids: &[String],
    computed: ProjectData,
) {
    let mut pm = pm.write().await;
    if let Some(p) = pm.get_project_mut_by_id(project_id) {
        if p.alignments.len() == snapshot_ids.len()
            && p
                .alignments
                .iter()
                .zip(snapshot_ids)
                .all(|(a, id)| &a.id == id)
        {
            p.alignments = computed.alignments;
        } else if let Some(mut aln) = computed.alignments.into_iter().last() {
            aln.id = libregene_core::align::next_alignment_id(&p.alignments);
            p.alignments.push(aln);
        }
    }
    pm.mark_dirty(project_id);
}

async fn do_add_alignment_seq<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
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
    let snapshot_ids: Vec<String> = project_clone.alignments.iter().map(|a| a.id.clone()).collect();

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
        let mut aln = libregene_core::align::align_read_checked(&p.sequence, &clean_seq, circular)
            .map_err(alignment_reject_message)?;
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
        Err(e) if e.starts_with("No significant alignment found") => return Err(e),
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    commit_computed_alignment(pm, project_id, &snapshot_ids, computed).await;

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

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
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    alignment_id: String,
) -> Result<serde_json::Value, String> {
    {
        let mut pm = pm.write().await;
        pm.remove_alignment(project_id, &alignment_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

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

/// Run automatic annotation of a project's sequence against the embedded
/// SnapGene feature database. Read-only: returns detected features, never
/// modifies the project (no dirty/broadcast). Coordinates are 0-based
/// inclusive; circular sequences may report origin-wrapping features with
/// `start > end` and split `segments`. DNA projects match at the nucleotide
/// level plus the protein level for CDS (codon-optimization-proof); protein
/// projects match the amino-acid sequence against the CDS translations; RNA
/// projects are unsupported and return an empty list.
async fn do_annotate_features(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
) -> Result<Vec<libregene_core::annotate::AnnotatedFeature>, String> {
    let (sequence, topology, molecule_type) = {
        let pm = pm.read().await;
        let project = pm
            .get_project_by_id(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        (
            project.sequence.clone(),
            project.topology.clone(),
            project.molecule_type.clone(),
        )
    };

    tokio::task::spawn_blocking(move || {
        if molecule_type == "protein" {
            libregene_core::annotate::annotate_protein(&sequence, topology == "circular")
        } else if molecule_type == "rna" {
            Vec::new()
        } else {
            libregene_core::annotate::annotate_sequence(&sequence, topology == "circular")
        }
    })
    .await
    .map_err(|e| format!("task join error: {}", e))
}

fn parse_optimize_method(s: &str) -> Result<libregene_core::codon::OptimizeMethod, String> {
    let normalized = s.trim().to_ascii_lowercase().replace('_', "");
    match normalized.as_str() {
        "usebestcodon" => Ok(libregene_core::codon::OptimizeMethod::UseBestCodon),
        "matchcodonusage" => Ok(libregene_core::codon::OptimizeMethod::MatchCodonUsage),
        "harmonizerca" => Ok(libregene_core::codon::OptimizeMethod::HarmonizeRca),
        _ => Err(format!(
            "unknown method '{}' (expected use_best_codon, match_codon_usage, or harmonize_rca)",
            s
        )),
    }
}

/// Resolve the codon-usage table: a caller-supplied custom table wins,
/// otherwise the built-in table for `species`.
pub(crate) fn codon_usage_table(
    species: &str,
    custom_table: Option<Vec<(char, String, f64)>>,
) -> Result<libregene_core::codon::CodonUsageTable, String> {
    match custom_table {
        Some(rows) => Ok(libregene_core::codon::table_from_custom(&rows)),
        None => match libregene_core::codon::get_table(species) {
            Some(t) => Ok(t.clone()),
            None => Err(format!(
                "unknown species '{}' (available: {})",
                species,
                libregene_core::codon::list_species().join(", ")
            )),
        },
    }
}

/// Build [`OptimizeOptions`] from the string `method` and the optional
/// source-table / avoidance / GC-window parameters shared by all callers.
pub(crate) fn codon_optimize_options(
    method: &str,
    original_species: Option<&str>,
    avoid_enzyme_sites: Option<Vec<String>>,
    gc_window: Option<(usize, f64, f64)>,
) -> Result<libregene_core::codon::OptimizeOptions, String> {
    let original_table = match original_species {
        Some(s) => Some(
            libregene_core::codon::get_table(s)
                .ok_or_else(|| format!("unknown species '{}'", s))?
                .clone(),
        ),
        None => None,
    };
    Ok(libregene_core::codon::OptimizeOptions {
        method: parse_optimize_method(method)?,
        original_table,
        avoid_enzyme_sites: avoid_enzyme_sites.unwrap_or_default(),
        gc_window,
        ..libregene_core::codon::OptimizeOptions::default()
    })
}

/// Shared codon-optimization core (Tauri commands + MCP `optimize_cds`): find
/// the CDS/mRNA feature, extract its coding sequence, run the optimizer, and
/// build the equal-length replacement sequence via `segments_on_template`
/// write-back (minus-strand pieces reverse-complemented). Read-only — callers
/// decide whether to apply via `do_update_sequence`.
pub(crate) fn codon_optimize(
    project: &ProjectData,
    feature_id: &str,
    species: &str,
    method: &str,
    custom_table: Option<Vec<(char, String, f64)>>,
    original_species: Option<&str>,
    avoid_enzyme_sites: Option<Vec<String>>,
    gc_window: Option<(usize, f64, f64)>,
) -> Result<
    (
        String,
        libregene_core::codon::OptimizeResult,
        libregene_core::codon::CodingDna,
    ),
    String,
> {
    let feature = project
        .features
        .iter()
        .find(|f| f.id == feature_id)
        .ok_or_else(|| format!("Feature not found: {}", feature_id))?;
    if feature.ftype != "CDS" && feature.ftype != "mRNA" {
        return Err(format!(
            "feature '{}' is {} (only CDS/mRNA can be codon-optimized)",
            feature_id, feature.ftype
        ));
    }
    let coding =
        libregene_core::codon::extract_codons(&project.sequence, feature, &project.topology)?;

    let table = codon_usage_table(species, custom_table)?;
    let opts = codon_optimize_options(method, original_species, avoid_enzyme_sites, gc_window)?;
    let result = libregene_core::codon::optimize_codons(&coding.codons, &table, &opts);

    let new_coding: String = result.new_codons.concat();
    let mut bytes = project.sequence.as_bytes().to_vec();
    let minus = feature.strand == "-";
    let mut off = 0usize;
    for &(s, e) in &coding.segments_on_template {
        let len = e - s + 1;
        let piece = &new_coding[off..off + len];
        if minus {
            let rc = libregene_core::utils::reverse_complement(piece);
            bytes[s as usize..=e as usize].copy_from_slice(rc.as_bytes());
        } else {
            bytes[s as usize..=e as usize].copy_from_slice(piece.as_bytes());
        }
        off += len;
    }
    let new_sequence = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    Ok((new_sequence, result, coding))
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
                let sites: Vec<serde_json::Value> = p
                    .binding_sites
                    .iter()
                    .map(|s| {
                        let (aligned_template, match_mask) =
                            libregene_core::primer::align::template_coverage(
                                &template, &topology, &p.primer_seq, s,
                            );
                        serde_json::json!({
                            "strand": s.strand,
                            "templateStart": s.template_start,
                            "templateEnd": s.template_end,
                            "tm": s.tm,
                            "annealLen": libregene_core::primer::align::anneal_len(
                                &template, &topology, &p.primer_seq, s,
                            ),
                            "mismatchedTail": s.five_prime_tail.len(),
                            "alignedTemplate": aligned_template,
                            "matchMask": match_mask,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "id": p.id,
                    "binds": !p.binding_sites.is_empty(),
                    "site": sites.first().cloned(),
                    "bindingSiteCount": p.binding_sites.len(),
                    "sites": sites,
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
        mg_conc: mg_conc.unwrap_or(0.0),
        dntp_conc: dntp_conc.unwrap_or(0.0),
        tris_conc: tris_conc.unwrap_or(0.0),
        primer_conc: primer_conc.unwrap_or(2.5e-7),
    };

    let seg1 = seg.ok_or_else(|| "Segment required for primer design".to_string())?;
    let overlap_len = overlap_len.unwrap_or(20).max(8);
    let arm_len = arm_len.unwrap_or(20).max(8);
    let mut_seq = mut_seq.unwrap_or_default();

    tokio::task::spawn_blocking(move || -> Result<Vec<libregene_core::primer::design::PrimerGroup>, String> {
        let mut groups = match mode.as_str() {
            "amplify" => {
                let name = name.unwrap_or_else(|| "Amplicon".to_string());
                match (fwd_tail, rev_tail) {
                    (None, None) => libregene_core::primer::design::build_amplify_groups(
                        &sequence, &seg1, &name, target_tm, &topology, &tm_params,
                    ),
                    (f, r) => libregene_core::primer::design::build_amplify_groups_tailed(
                        &sequence, &seg1, &name, target_tm, &topology,
                        f.as_deref().unwrap_or(""), r.as_deref().unwrap_or(""), &tm_params,
                    ),
                }
            }
            "oepcr" => {
                let seg2 = seg2.as_ref().ok_or_else(|| "Second segment required for OE-PCR".to_string())?;
                let name1 = name1.unwrap_or_else(|| "Fragment 1".to_string());
                let name2 = name2.unwrap_or_else(|| "Fragment 2".to_string());
                libregene_core::primer::design::build_oepcr_groups(
                    &sequence, &seg1, seg2, &name1, &name2, target_tm, overlap_len, &topology, &tm_params,
                )
            }
            "mutagenesis" => {
                let site_name = site_name.unwrap_or_else(|| "Mutation".to_string());
                libregene_core::primer::design::build_mutagenesis_groups(
                    &sequence, &seg1, &site_name, &mut_seq, target_tm, arm_len, &topology, &tm_params,
                )
            }
            other => return Err(format!("Unknown primer design mode: {other}")),
        };
        let segs: Vec<libregene_core::models::Segment> = match mode.as_str() {
            "amplify" | "mutagenesis" => vec![seg1],
            "oepcr" => {
                let seg2 = seg2.ok_or_else(|| "Second segment required for OE-PCR".to_string())?;
                vec![seg1, seg2]
            }
            _ => Vec::new(),
        };
        libregene_core::primer::design::unify_candidate_tm(
            &sequence, &topology, &segs, &mut groups, &tm_params,
        );
        Ok(groups)
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
    let project_id = match resolve_project_id(&state, &window_label).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
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
    do_open_file(&state.pm, &state.window_projects, &state.agent_tabs, path).await
}

/// Drain OS-opened file paths queued before the frontend was ready (cold
/// start via Open With / double-click / second-instance forwarding). The
/// frontend opens each through the normal `open_file` command.
#[tauri::command]
fn take_pending_opens(state: State<'_, AppState>) -> Vec<String> {
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
async fn create_project(
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
async fn save_file(
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
async fn write_text_file(path: String, contents: String) -> Result<serde_json::Value, String> {
    validate_user_path(&path, TEXT_EXPORT_EXTS)?;
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
    app_handle: AppHandle,
    sequence: String,
    features: Option<Vec<Feature>>,
    primers: Option<Vec<Primer>>,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };
    if let Some(features) = features {
        // Feature coordinates arrive from the frontend (paste-merge, adjust)
        // and serde only checks types — clamp to the NEW sequence length so a
        // forged clipboard payload can't seed out-of-range spans that later
        // panic coordinate consumers. Mirrors MCP set_feature's bounds gate.
        let new_len = sequence.len() as i64;
        let mut features = features;
        features.retain(|f| {
            let (lo, hi) = if f.segments.is_empty() {
                (f.start, f.end)
            } else {
                (
                    f.segments.iter().map(|s| s.start).min().unwrap_or(f.start),
                    f.segments.iter().map(|s| s.end).max().unwrap_or(f.end),
                )
            };
            hi >= 0 && lo < new_len
        });
        // clamp(0, -1) panics, so floor the upper bound for empty sequences.
        let max_pos = (new_len - 1).max(0);
        for f in features.iter_mut() {
            f.start = f.start.clamp(0, max_pos);
            f.end = f.end.clamp(0, max_pos);
            for seg in f.segments.iter_mut() {
                seg.start = seg.start.clamp(0, max_pos);
                seg.end = seg.end.clamp(0, max_pos);
            }
        }
        let mut pm = state.pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.features = features;
        }
    }
    do_update_sequence(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        project_id,
        sequence,
        primers,
    )
    .await
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
    match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => {
            let mut pm = state.pm.write().await;
            if let Some(p) = pm.get_project_mut_by_id(&id) {
                p.roi = Some((start, end));
                pm.mark_dirty(&id);
            }
            Ok(serde_json::json!({"status": "ok"}))
        }
        Err(e) => Ok(serde_json::json!({"error": e})),
    }
}

#[tauri::command]
async fn clear_roi(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => {
            let mut pm = state.pm.write().await;
            if let Some(p) = pm.get_project_mut_by_id(&id) {
                p.roi = None;
                pm.mark_dirty(&id);
            }
            Ok(serde_json::json!({"status": "ok"}))
        }
        Err(e) => Ok(serde_json::json!({"error": e})),
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
    match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => {
            let pm = state.pm.read().await;
            match pm.get_project_by_id(&id) {
                Some(p) => Ok(serde_json::to_value(&p.features).unwrap_or(serde_json::json!([]))),
                None => Ok(serde_json::json!([])),
            }
        }
        Err(_) => Ok(serde_json::json!([])),
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
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    // If location_str is provided, parse and validate it (0-based inclusive,
    // the App-wide convention, e.g. "99..199", "join(0..99,199..299)")
    let mut resolved = feature;
    if let Some(loc_str) = location_str {
        let trimmed = loc_str.trim().to_string();
        if trimmed.is_empty() {
            return Ok(serde_json::json!({"error": "Location cannot be empty".to_string()}));
        }
        let parsed = libregene_core::file_io::gbk::parse_location_string_0based(&trimmed)
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
        &state.agent_tabs,
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
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_delete_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
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
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
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
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
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
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
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
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    let valid = strand == "." || strand == "+" || strand == "-";
    if !valid {
        return Ok(serde_json::json!({"error": "Invalid strand: must be ., +, or -".to_string()}));
    }

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
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
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_update_feature(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        &feature_id,
        |f| {
            // location_str is 0-based inclusive (App-wide convention).
            let parsed = libregene_core::file_io::gbk::parse_location_string_0based(&location_str)
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
    match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => {
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
        Err(_) => Ok(serde_json::json!([])),
    }
}

#[tauri::command]
async fn add_primer(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    primer: Primer,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_add_primer(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
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
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_delete_primer(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        id,
    )
    .await
}

/// Check which of the given primers can bind to the current project's sequence.
/// Returns a per-primer summary (binds + best `site`, plus the full best-first
/// `sites` array and `bindingSiteCount`), reusing the same binding-site engine
/// as the editor for consistency.
#[tauri::command]
async fn check_primers_binding(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    primers: Vec<Primer>,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

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
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    // Snapshot → merge → recompute (off-lock) → write-back. The write-back
    // only lands when the primers list is unchanged since the snapshot;
    // otherwise a concurrent add slipped in and we retry onto the fresh list
    // (binding sites depend only on the template, which does not change).
    loop {
        let snapshot = {
            let pm = state.pm.read().await;
            pm.get_project_by_id(&project_id).map(|p| {
                (p.sequence.clone(), p.topology.clone(), p.primers.clone())
            })
        };
        let Some((template, topology, existing)) = snapshot else {
            return Ok(serde_json::json!({"error": "Project not found"}));
        };

        let mut merged = existing.clone();
        for primer in &primers {
            let seq_conflict = merged
                .iter()
                .any(|p| p.primer_seq.to_uppercase() == primer.primer_seq.to_uppercase());
            let name_conflict =
                merged.iter().any(|p| p.id != primer.id && p.name == primer.name);
            if seq_conflict || name_conflict {
                continue;
            }
            if let Some(pos) = merged.iter().position(|p| p.id == primer.id) {
                merged[pos] = primer.clone();
            } else {
                merged.push(primer.clone());
            }
        }

        let updated = tokio::task::spawn_blocking(move || {
            libregene_core::primer::align::recompute_all_primers(&template, &topology, &merged)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        let wrote = {
            let mut pm = state.pm.write().await;
            match pm.get_project_mut_by_id(&project_id) {
                Some(p) if primers_equal(&p.primers, &existing) => {
                    p.primers = updated;
                    pm.mark_dirty(&project_id);
                    true
                }
                // The list changed under us (another window added a primer):
                // re-merge onto the fresh state instead of overwriting.
                Some(_) => false,
                None => return Ok(serde_json::json!({"error": "Project not found"})),
            }
        };
        if wrote {
            break;
        }
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
    let project_id = resolve_project_id(&state, webview_window.label()).await?;

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
        mg_conc: mg_conc.unwrap_or(0.0),
        dntp_conc: dntp_conc.unwrap_or(0.0),
        tris_conc: tris_conc.unwrap_or(0.0),
        primer_conc: primer_conc.unwrap_or(2.5e-7),
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
    if tlen < seed_len {
        return Err(format!(
            "Template too short ({}bp < {}bp seed)",
            tlen, seed_len
        ));
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
            // On circular templates the footprint may wrap the origin; cap the
            // extension so it cannot run into its own seed.
            let max_ext = if is_circular {
                plen.saturating_sub(seed_len).min(tlen.saturating_sub(seed_len))
            } else {
                plen.saturating_sub(seed_len)
            };
            while ext < max_ext {
                let p_pos = plen - seed_len - ext - 1;
                let t_pos = if is_rev {
                    let raw = seed_tstart + seed_len + ext;
                    if is_circular { raw % tlen } else { raw }
                } else if is_circular {
                    (seed_tstart + tlen - 1 - (ext % tlen)) % tlen
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
    let project_id = resolve_project_id(&state, webview_window.label()).await?;

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
    let project_id = resolve_project_id(&state, webview_window.label()).await?;

    do_search_sequence(&state.pm, &project_id, query).await
}

/// Run automatic annotation of a project's sequence against the embedded
/// SnapGene feature database. Read-only: returns detected features (camelCase,
/// 0-based inclusive coordinates), does not modify the project. `project_id`
/// defaults to the calling window's project.
#[tauri::command]
async fn annotate_features(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<Vec<libregene_core::annotate::AnnotatedFeature>, String> {
    let project_id = match project_id {
        Some(id) => id,
        None => resolve_project_id(&state, webview_window.label()).await?,
    };

    do_annotate_features(&state.pm, &project_id).await
}

/// Run automatic annotation on a bare sequence (no project required), for the
/// Empty-page "New Sequence" dialog's live feature preview. `circular` doubles
/// the query so origin-wrapping features are found. Read-only; returns
/// camelCase AnnotatedFeature with 0-based inclusive coordinates. Protein
/// sequences match against the translated CDS features of the database.
#[tauri::command]
async fn annotate_sequence(
    sequence: String,
    circular: bool,
    molecule_type: Option<String>,
) -> Result<Vec<libregene_core::annotate::AnnotatedFeature>, String> {
    tokio::task::spawn_blocking(move || {
        if molecule_type.as_deref() == Some("protein") {
            libregene_core::annotate::annotate_protein(&sequence, circular)
        } else {
            libregene_core::annotate::annotate_sequence(&sequence, circular)
        }
    })
    .await
    .map_err(|e| format!("task join error: {}", e))
}

// ---------------------------------------------------------------------------
// Tauri commands — codon optimization
// ---------------------------------------------------------------------------

/// List the built-in codon-usage species keys (e.g. "e_coli", "h_sapiens").
#[tauri::command]
async fn list_codon_species() -> Vec<String> {
    libregene_core::codon::list_species()
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}

/// Preview a CDS/mRNA feature's synonymous codon optimization without
/// modifying the project. Returns the translated aa, the replacement codons,
/// CAI/GC before and after, per-codon repairs, and unresolved violations.
/// `method` is use_best_codon | match_codon_usage | harmonize_rca;
/// harmonize_rca additionally uses `original_species` as the source table.
#[tauri::command]
async fn preview_codon_optimization(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    feature_id: String,
    species: String,
    method: String,
    custom_table: Option<Vec<(char, String, f64)>>,
    original_species: Option<String>,
    avoid_enzyme_sites: Option<Vec<String>>,
    gc_window: Option<(usize, f64, f64)>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await?;
    let project = {
        let pm = state.pm.read().await;
        pm.get_project_by_id(&project_id)
            .ok_or_else(|| "Project not found".to_string())?
            .clone()
    };
    let f_id = feature_id.clone();
    let sp = species.clone();
    let m = method.clone();
    let (_, result, coding) = tokio::task::spawn_blocking(move || {
        codon_optimize(
            &project,
            &f_id,
            &sp,
            &m,
            custom_table,
            original_species.as_deref(),
            avoid_enzyme_sites,
            gc_window,
        )
    })
    .await
    .map_err(|e| format!("task join error: {}", e))??;
    Ok(codon_optimization_json(&result, &coding, &method, &species))
}

/// Apply a synonymous codon optimization: replace the feature's coding bases
/// in the template sequence (equal-length, so feature coordinates are
/// unchanged), then recompute enzymes/primers/translations and mark the
/// project dirty. Returns the same summary as preview plus ok/message.
#[tauri::command]
async fn apply_codon_optimization(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    species: String,
    method: String,
    custom_table: Option<Vec<(char, String, f64)>>,
    original_species: Option<String>,
    avoid_enzyme_sites: Option<Vec<String>>,
    gc_window: Option<(usize, f64, f64)>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await?;
    let project = {
        let pm = state.pm.read().await;
        pm.get_project_by_id(&project_id)
            .ok_or_else(|| "Project not found".to_string())?
            .clone()
    };
    let f_id = feature_id.clone();
    let sp = species.clone();
    let m = method.clone();
    let (new_sequence, result, coding) = tokio::task::spawn_blocking(move || {
        codon_optimize(
            &project,
            &f_id,
            &sp,
            &m,
            custom_table,
            original_species.as_deref(),
            avoid_enzyme_sites,
            gc_window,
        )
    })
    .await
    .map_err(|e| format!("task join error: {}", e))??;
    do_update_sequence(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        project_id.clone(),
        new_sequence,
        None,
    )
    .await?;
    let mut v = codon_optimization_json(&result, &coding, &method, &species);
    v["ok"] = serde_json::json!(true);
    v["message"] = serde_json::json!(format!(
        "Optimized CDS {} ({}): CAI {:.3} → {:.3}, {} repairs, {} unresolved",
        feature_id,
        species,
        result.cai_before,
        result.cai_after,
        result.repairs.len(),
        result.unresolved.len()
    ));
    Ok(v)
}

fn codon_optimization_json(
    result: &libregene_core::codon::OptimizeResult,
    coding: &libregene_core::codon::CodingDna,
    method: &str,
    species: &str,
) -> serde_json::Value {
    serde_json::json!({
        "aa": coding.aa,
        "codonCount": coding.codons.len(),
        "newCodons": result.new_codons,
        "caiBefore": result.cai_before,
        "caiAfter": result.cai_after,
        "gcBefore": result.gc_before,
        "gcAfter": result.gc_after,
        "repairs": result.repairs,
        "unresolved": result.unresolved,
        "method": method,
        "species": species,
    })
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
    let project_id = resolve_project_id(&state, webview_window.label()).await?;

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
        let mut aln = libregene_core::align::align_read_checked(&p.sequence, &read_project.sequence, circular)
            .map_err(alignment_reject_message)?;
        aln.name = name;
        aln.id = libregene_core::align::next_alignment_id(&p.alignments);
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
async fn add_alignment_seq(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    name: String,
    seq: String,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_add_alignment_seq(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
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

// ---------------------------------------------------------------------------
// Tauri commands — methylation
// ---------------------------------------------------------------------------

#[tauri::command]
async fn set_methylation(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    systems: Vec<String>,
    overlap: Option<i64>,
) -> Result<serde_json::Value, String> {
    let project_id = match resolve_project_id(&state, webview_window.label()).await {
        Ok(id) => id,
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    do_set_methylation(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        &project_id,
        systems,
        overlap,
    )
    .await
}

// ---------------------------------------------------------------------------
// Tauri commands — multi-project management
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_projects(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let (projects, active_id) =
        sidebar_project_list(&state.pm, &state.window_projects, &state.agent_tabs).await;
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
        mg_conc: mg_conc.unwrap_or(0.0),
        dntp_conc: dntp_conc.unwrap_or(0.0),
        tris_conc: tris_conc.unwrap_or(0.0),
        primer_conc: primer_conc.unwrap_or(2.5e-7),
    };
    let tm = libregene_core::primer::thermodynamics::compute_tm_with_params(&seq, &params);
    Ok((tm * 10.0).round() / 10.0)
}

/// Submit a sequence to NCBI BLAST (fixed preset per molecule type) and open
/// the official results page in the system browser. Returns the results URL.
#[tauri::command]
async fn blast_submit(
    app: tauri::AppHandle,
    sequence: String,
    molecule_type: String,
) -> Result<String, String> {
    let submission = tauri::async_runtime::spawn_blocking(move || {
        libregene_core::blast::submit(&sequence, &molecule_type)
    })
    .await
    .map_err(|e| e.to_string())??;
    let url = libregene_core::blast::results_url(&submission.rid);
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(url)
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

/// Return the lock state of the agent tab bound to `project_id`:
/// `{projectId, locked}` for agent-bound projects, null otherwise.
#[tauri::command]
async fn get_agent_tab_state(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let at = state.agent_tabs.read().await;
    Ok(match at.get(&project_id) {
        Some(meta) => serde_json::json!({
            "projectId": project_id,
            "locked": meta.locked,
        }),
        None => serde_json::Value::Null,
    })
}

/// Set an agent tab's lock state (the unlock/lock button in the UI). The next
/// MCP tool call on the bound project re-locks it via
/// `lock_agent_tab_for_project`.
#[tauri::command]
async fn set_agent_tab_locked(
    project_id: String,
    app_handle: AppHandle,
    state: State<'_, AppState>,
    locked: bool,
) -> Result<serde_json::Value, String> {
    let mut at = state.agent_tabs.write().await;
    match at.get_mut(&project_id) {
        Some(meta) => {
            meta.locked = locked;
            let _ = app_handle.emit(
                "agent-tab-lock",
                serde_json::json!({
                    "projectId": project_id,
                    "locked": locked,
                }),
            );
            Ok(serde_json::json!({"status": "ok", "locked": locked}))
        }
        None => Ok(serde_json::json!({"error": "not an agent tab"})),
    }
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
    state: State<'_, AppState>,
    enabled: bool,
    port: u16,
) -> Result<serde_json::Value, String> {
    let cfg = mcp.set_config(enabled, port).await?;
    if let Ok(tray_status) = state.tray_status.lock() {
        if let Some(item) = tray_status.as_ref() {
            let _ = item.set_text(mcp_status_text(cfg.enabled, cfg.port));
        }
    }
    Ok(serde_json::json!({
        "enabled": cfg.enabled,
        "port": cfg.port,
        "status": "ok",
    }))
}

/// Return the bearer token the trusted frontend must send to talk to the
/// loopback MCP server. Only the in-app webview can reach this command;
/// combined with the Host check on the server it keeps other local processes
/// (and browser pages via DNS rebinding) from driving MCP tools.
#[tauri::command]
async fn get_mcp_token(
    mcp: State<'_, mcp::McpServer<tauri::Wry>>,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "token": mcp.auth_token() }))
}

/// Rotate the MCP bearer token on user request. The new token is persisted
/// and takes effect immediately for the running server.
#[tauri::command]
async fn regenerate_mcp_token(
    mcp: State<'_, mcp::McpServer<tauri::Wry>>,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "token": mcp.regenerate_auth_token() }))
}

// ---------------------------------------------------------------------------
// System tray — closing the main window hides it (close-to-tray) so the
// process and the embedded MCP server stay alive for agents.
// ---------------------------------------------------------------------------

fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Quit immediately, discarding unsaved changes. The frontend calls this only
/// after the user confirmed the dialog triggered by the tray's
/// `quit-requested` event.
#[tauri::command]
async fn force_quit(app: AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

fn mcp_status_text(enabled: bool, port: u16) -> String {
    if enabled {
        format!("MCP: running · port {port}")
    } else {
        "MCP: disabled".to_string()
    }
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let mcp_cfg = app.state::<mcp::McpServer<tauri::Wry>>().config();
    let status =
        MenuItemBuilder::with_id("mcp-status", mcp_status_text(mcp_cfg.enabled, mcp_cfg.port))
            .enabled(false)
            .build(app)?;
    *app.state::<AppState>().tray_status.lock().unwrap() = Some(status.clone());

    let menu = MenuBuilder::new(app)
        .item(&status)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&MenuItemBuilder::with_id("show", "Show LibreGene").build(app)?)
        .item(&MenuItemBuilder::with_id("quit", "Quit LibreGene").build(app)?)
        .build()?;

    TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .icon(
            app.default_window_icon()
                .expect("app icon missing")
                .clone(),
        )
        .tooltip("LibreGene")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                // With unsaved projects, defer to the frontend's unsaved-
                // changes flow instead of exiting abruptly: show the main
                // window and let it confirm/discard first.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let dirty_ids: Vec<String> = {
                        let state = app.state::<AppState>();
                        let pm = state.pm.read().await;
                        pm.all_project_ids()
                            .into_iter()
                            .filter(|id| pm.is_dirty(id))
                            .collect()
                    };
                    if dirty_ids.is_empty() {
                        app.exit(0);
                    } else {
                        show_main_window(&app);
                        let _ = app.emit("quit-requested", dirty_ids);
                    }
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// App entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_decoration::init())
        // Keep one process on Windows/Linux: a second launch (e.g. opening
        // another file from Explorer) forwards its argv to the running
        // instance instead of spawning a new one. No-op on macOS, where the
        // OS routes open requests to the running app via RunEvent::Opened.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            queue_open_targets(app, collect_open_targets(argv.into_iter().skip(1)));
        }))
        // Close-to-tray: closing the main window only hides it, keeping the
        // process (and the MCP server) alive. Project windows close normally.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .manage(AppState {
            pm: Arc::new(RwLock::new(ProjectManager::new())),
            window_projects: Arc::new(RwLock::new(HashMap::new())),
            agent_tabs: Arc::new(RwLock::new(HashMap::new())),
            pending_opens: Arc::new(std::sync::Mutex::new(Vec::new())),
            tray_status: Arc::new(std::sync::Mutex::new(None)),
        })
        .setup(|app| {
            let state = app.state::<AppState>();
            let mcp = mcp::McpServer::new(
                app.handle().clone(),
                state.pm.clone(),
                state.window_projects.clone(),
                state.agent_tabs.clone(),
                {
                    // Server state can change without a set_config call (bind
                    // failure): refresh the tray status line so it matches
                    // what get_mcp_config reports.
                    let app = app.handle().clone();
                    move |enabled, port| {
                        let tray_status = app.state::<AppState>().tray_status.clone();
                        let guard = tray_status.lock();
                        if let Ok(guard) = guard {
                            if let Some(item) = guard.as_ref() {
                                let _ = item.set_text(mcp_status_text(enabled, port));
                            }
                        }
                    }
                },
            );
            app.manage(mcp.clone());
            // Start with the default config (enabled on MCP_PORT); the frontend
            // reconciles with the persisted localStorage config on mount.
            tauri::async_runtime::block_on(async move { mcp.apply().await });
            setup_tray(app)?;
            // Cold-start file open (Windows/Linux: path passed in argv).
            queue_open_targets(
                app.handle(),
                collect_open_targets(std::env::args().skip(1)),
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_project,
            get_project_by_id,
            open_file,
            take_pending_opens,
            create_project,
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
            annotate_features,
            annotate_sequence,
            list_codon_species,
            preview_codon_optimization,
            apply_codon_optimization,
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
            get_agent_tab_state,
            set_agent_tab_locked,
            rekey_project,
            compute_tm,
            blast_submit,
            get_mcp_config,
            set_mcp_config,
            get_mcp_token,
            regenerate_mcp_token,
            activate_custom_titlebar,
            reassert_traffic_lights,
            restore_native_titlebar,
            force_quit,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|app_handle, event| {
        // macOS routes Open With / double-click / dock drops here (including
        // cold start). Windows/Linux open targets arrive via argv instead.
        #[cfg(target_os = "macos")]
        match event {
            tauri::RunEvent::Opened { urls } => {
                let paths = urls
                    .into_iter()
                    .filter_map(|u| u.to_file_path().ok())
                    .filter_map(|p| p.to_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>();
                queue_open_targets(app_handle, collect_open_targets(paths));
            }
            // Clicking the Dock icon with no visible windows reopens the
            // (hidden) main window.
            tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => show_main_window(app_handle),
            _ => {}
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app_handle, event);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_path_accepts_normal_sequence_file() {
        assert_eq!(validate_user_path("C:/some/dir/plasmid.gbk", SEQ_EXTS).unwrap(), "gbk");
        assert_eq!(validate_user_path("plasmid.fa", SEQ_EXTS).unwrap(), "fa");
        assert_eq!(validate_user_path("reads.faa", SEQ_EXTS).unwrap(), "faa");
        assert_eq!(validate_user_path("genome.gbff", SEQ_EXTS).unwrap(), "gbff");
        assert_eq!(validate_user_path("entry.gp", SEQ_EXTS).unwrap(), "gp");
        assert_eq!(validate_user_path("mystery.seq", SEQ_EXTS).unwrap(), "seq");
    }

    #[test]
    fn validate_path_rejects_empty() {
        assert!(validate_user_path("   ", SEQ_EXTS).is_err());
    }

    #[test]
    fn validate_path_rejects_parent_traversal() {
        // The headline case: a raw path string from an untrusted caller must
        // not be able to escape a directory or point at arbitrary files via ..
        assert!(validate_user_path("../etc/passwd", SEQ_EXTS).is_err());
        assert!(validate_user_path("dir/../../secret.gbk", SEQ_EXTS).is_err());
        assert!(validate_user_path("../../../../Windows/System32/x.gbk", SEQ_EXTS).is_err());
    }

    #[test]
    fn validate_path_rejects_wrong_extension() {
        // Even a benign-looking path must have a sequence/export extension,
        // so an untrusted caller can't read/write e.g. .bashrc by extension swap.
        assert!(validate_user_path("notes.txt", SEQ_EXTS).is_err());
        assert!(validate_user_path("noext", SEQ_EXTS).is_err());
    }

    #[test]
    fn validate_path_accepts_text_exports_for_write() {
        assert_eq!(validate_user_path("enzymes.csv", TEXT_EXPORT_EXTS).unwrap(), "csv");
        assert_eq!(validate_user_path("data.json", TEXT_EXPORT_EXTS).unwrap(), "json");
        assert!(validate_user_path("evil.exe", TEXT_EXPORT_EXTS).is_err());
    }

    #[test]
    fn validate_path_accepts_codon_output_exts() {
        assert_eq!(validate_user_path("out.gbk", CODON_OUTPUT_EXTS).unwrap(), "gbk");
        assert_eq!(validate_user_path("out.GB", CODON_OUTPUT_EXTS).unwrap(), "gb");
        assert_eq!(validate_user_path("out.gpt", CODON_OUTPUT_EXTS).unwrap(), "gpt");
        assert!(validate_user_path("out.fasta", CODON_OUTPUT_EXTS).is_err());
        assert!(validate_user_path("out.ab1", CODON_OUTPUT_EXTS).is_err());
        assert!(validate_user_path("out.txt", CODON_OUTPUT_EXTS).is_err());
    }

    #[tokio::test]
    async fn check_primer_binding_returns_all_sites_best_first() {
        // "GATTACAGTC" occurs twice in the template (linear): 0..10 and 10..20.
        // (10-mer — a 7-mer has negative Tm under the SnapGene-aligned defaults
        // and would be filtered out by the tm threshold.)
        let tpl = "GATTACAGTCGATTACAGTC";
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write().await.open_project(
            "p1".to_string(),
            ProjectData {
                sequence: tpl.to_string(),
                length: tpl.len() as i64,
                topology: "linear".to_string(),
                ..Default::default()
            },
        ).unwrap();
        let primers = vec![Primer {
            id: "f1".to_string(),
            name: "f1".to_string(),
            r#type: "fwd".to_string(),
            primer_seq: "GATTACAGTC".to_string(),
            binding_sites: Vec::new(),
        }];
        let out = do_check_primers_binding(&pm, "p1", primers).await.unwrap();
        let results = out["results"].as_array().expect("results array");
        assert_eq!(results.len(), 1);
        let r0 = &results[0];
        assert_eq!(r0["id"], "f1");
        assert_eq!(r0["binds"], true);
        assert_eq!(r0["bindingSiteCount"], 2);
        let sites = r0["sites"].as_array().expect("sites array");
        assert_eq!(sites.len(), 2);
        // Every site carries the documented fields.
        for s in sites {
            for key in [
                "strand",
                "templateStart",
                "templateEnd",
                "tm",
                "annealLen",
                "mismatchedTail",
                "alignedTemplate",
                "matchMask",
            ] {
                assert!(s.get(key).is_some(), "site missing field {}", key);
            }
        }
        // `site` is the best (first) site; `sites` is best-first (Tm desc).
        let starts: Vec<i64> = sites
            .iter()
            .map(|s| s["templateStart"].as_i64().unwrap())
            .collect();
        assert_eq!(r0["site"]["templateStart"], sites[0]["templateStart"]);
        assert!(starts.contains(&0) && starts.contains(&10));
        let tms: Vec<f64> = sites
            .iter()
            .map(|s| s["tm"].as_f64().unwrap())
            .collect();
        assert!(tms.windows(2).all(|w| w[0] >= w[1]));
    }

    #[tokio::test]
    async fn check_primer_binding_reports_tail_coverage() {
        // Enzyme-tail primer: "GCG" protect+site-like tail whose 3'-most base
        // happens to match the template next to the anneal core. The footprint
        // stops at the first 5'-ward mismatch, but alignedTemplate/matchMask
        // must expose the tail's per-base pairing against the template.
        let tpl = "TGCGTACGCTAGCTA";
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write().await.open_project(
            "p1".to_string(),
            ProjectData {
                sequence: tpl.to_string(),
                length: tpl.len() as i64,
                topology: "linear".to_string(),
                ..Default::default()
            },
        ).unwrap();
        let primers = vec![Primer {
            id: "t1".to_string(),
            name: "t1".to_string(),
            r#type: "fwd".to_string(),
            primer_seq: "AGCGTACGCTAGCTA".to_string(),
            binding_sites: Vec::new(),
        }];
        let out = do_check_primers_binding(&pm, "p1", primers).await.unwrap();
        let r0 = &out["results"][0];
        let site = &r0["sites"][0];
        // Footprint: primer[1..] "GCGTACGCTAGCTA" matches template[1..15];
        // primer[0] 'A' faces template[0] 'T' — a mismatch the mask must show.
        assert_eq!(site["mismatchedTail"], 1);
        assert_eq!(site["alignedTemplate"], "TGCGTACGCTAGCTA");
        assert_eq!(site["matchMask"], ".||||||||||||||");
        // annealLen covers the tail bases that pair (14), beyond a nominal
        // 13-bp design core — the documented design/check discrepancy.
        assert_eq!(site["annealLen"], 14);
    }

    #[tokio::test]
    async fn resolve_project_id_never_falls_back_for_evicted_project_windows() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write()
            .await
            .open_project(
                "p1".to_string(),
                ProjectData {
                    sequence: "GATTACA".to_string(),
                    length: 7,
                    topology: "linear".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        app.manage(AppState {
            pm: pm.clone(),
            window_projects: Arc::new(RwLock::new(HashMap::new())),
            agent_tabs: Arc::new(RwLock::new(HashMap::new())),
            pending_opens: Arc::new(std::sync::Mutex::new(Vec::new())),
            tray_status: Arc::new(std::sync::Mutex::new(None)),
        });
        let state = app.state::<AppState>();

        // Main window falls back to the active project.
        assert_eq!(resolve_project_id(&state, "main").await.unwrap(), "p1");

        // Project windows resolve through their mapping.
        state
            .window_projects
            .write()
            .await
            .insert("project-x-1".to_string(), "p1".to_string());
        assert_eq!(resolve_project_id(&state, "project-x-1").await.unwrap(), "p1");

        // Evicted (mapping pruned): the window errors instead of silently
        // falling back to the main window's active project — which would
        // route its mutations to the wrong file.
        state.window_projects.write().await.remove("project-x-1");
        let err = resolve_project_id(&state, "project-x-1").await.unwrap_err();
        assert!(err.contains("evicted"), "{err}");
    }

    #[tokio::test]
    async fn concurrent_primer_adds_do_not_clobber_each_other() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write()
            .await
            .open_project(
                "p1".to_string(),
                ProjectData {
                    sequence: "GATTACAGATTACAGATTACA".to_string(),
                    length: 21,
                    topology: "linear".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        let wp: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));
        let agent_tabs: AgentTabs = Arc::new(RwLock::new(HashMap::new()));
        let mk = |id: &str| Primer {
            id: id.to_string(),
            name: id.to_string(),
            r#type: "fwd".to_string(),
            primer_seq: "GATTACA".to_string(),
            binding_sites: Vec::new(),
        };

        // Two adds racing on the same empty list: each snapshots the same
        // state, so a naive last-writer-wins write-back would drop one.
        let (a, b) = tokio::join!(
            do_add_primer(
                app.handle(),
                &pm,
                &wp,
                &agent_tabs,
                None,
                "p1",
                mk("f1"),
            ),
            do_add_primer(
                app.handle(),
                &pm,
                &wp,
                &agent_tabs,
                None,
                "p1",
                mk("f2"),
            ),
        );
        a.unwrap();
        b.unwrap();

        let pm = pm.read().await;
        let p = pm.get_project_by_id("p1").unwrap();
        let names: Vec<&str> = p.primers.iter().map(|pr| pr.name.as_str()).collect();
        assert!(
            names.contains(&"f1") && names.contains(&"f2"),
            "both primers must survive a concurrent add: {:?}",
            names
        );
    }

    #[tokio::test]
    async fn create_project_validates_feature_segment_encoding_order() {
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let wp: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));
        let agent_tabs: AgentTabs = Arc::new(RwLock::new(HashMap::new()));
        let seq = "A".repeat(5000);
        let nf = |name: &str, segments: Vec<Segment>| NewFeatureInput {
            name: name.to_string(),
            ftype: "gene".to_string(),
            color: "#fff".to_string(),
            strand: "+".to_string(),
            segments,
        };

        // Origin-wrapping feature in encoding order (tail first) keeps
        // start > end semantics instead of being flattened.
        let ok = do_create_project(
            &pm,
            &wp,
            &agent_tabs,
            "circ".to_string(),
            seq.clone(),
            "dna".to_string(),
            "circular".to_string(),
            vec![nf(
                "wrap",
                vec![
                    Segment { start: 4900, end: 4999, color: None },
                    Segment { start: 0, end: 99, color: None },
                ],
            )],
        )
        .await
        .unwrap();
        let feats = ok["features"].as_array().unwrap();
        assert_eq!(feats[0]["start"], 4900);
        assert_eq!(feats[0]["end"], 99);

        // Head-before-tail disorder → explicit error instead of a phantom wrap.
        let bad = do_create_project(
            &pm,
            &wp,
            &agent_tabs,
            "circ2".to_string(),
            seq.clone(),
            "dna".to_string(),
            "circular".to_string(),
            vec![nf(
                "bad",
                vec![
                    Segment { start: 0, end: 99, color: None },
                    Segment { start: 4900, end: 4999, color: None },
                    Segment { start: 200, end: 299, color: None },
                ],
            )],
        )
        .await
        .unwrap();
        assert!(
            bad["error"].as_str().unwrap().contains("encoding order"),
            "{}",
            bad
        );

        // A segment with start > end is not a linear span → error.
        let bad2 = do_create_project(
            &pm,
            &wp,
            &agent_tabs,
            "circ3".to_string(),
            seq.clone(),
            "dna".to_string(),
            "circular".to_string(),
            vec![nf("bad2", vec![Segment { start: 100, end: 50, color: None }])],
        )
        .await
        .unwrap();
        assert!(bad2["error"].as_str().is_some(), "{}", bad2);

        // Multi-segment linear feature (ascending, no wrap) stays valid.
        let ok2 = do_create_project(
            &pm,
            &wp,
            &agent_tabs,
            "linear".to_string(),
            seq.clone(),
            "dna".to_string(),
            "linear".to_string(),
            vec![nf(
                "multi",
                vec![
                    Segment { start: 10, end: 20, color: None },
                    Segment { start: 30, end: 40, color: None },
                ],
            )],
        )
        .await
        .unwrap();
        let feats2 = ok2["features"].as_array().unwrap();
        assert_eq!(feats2[0]["start"], 10);
        assert_eq!(feats2[0]["end"], 40);
    }

    #[tokio::test]
    async fn concurrent_alignment_adds_do_not_clobber_each_other() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        // Pseudo-random 200 bp template (repeats would confuse the aligner).
        let mut seq = String::new();
        let mut x = 7u64;
        for _ in 0..200 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            seq.push(b"ACGT"[(x >> 33) as usize & 3] as char);
        }
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write()
            .await
            .open_project(
                "p1".to_string(),
                ProjectData {
                    sequence: seq.clone(),
                    length: 200,
                    topology: "linear".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        let wp: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));
        let agent_tabs: AgentTabs = Arc::new(RwLock::new(HashMap::new()));
        let read = seq[20..170].to_string();

        // Two adds racing on the same empty alignment list: each snapshots
        // the same state, so a naive last-writer-wins write-back would drop
        // one alignment (and both would allocate the same id).
        let (a, b) = tokio::join!(
            do_add_alignment_seq(
                app.handle(),
                &pm,
                &wp,
                &agent_tabs,
                None,
                "p1",
                "r1".to_string(),
                read.clone(),
            ),
            do_add_alignment_seq(
                app.handle(),
                &pm,
                &wp,
                &agent_tabs,
                None,
                "p1",
                "r2".to_string(),
                read.clone(),
            ),
        );
        a.unwrap();
        b.unwrap();

        let pm = pm.read().await;
        let p = pm.get_project_by_id("p1").unwrap();
        let names: Vec<&str> = p.alignments.iter().map(|al| al.name.as_str()).collect();
        assert!(
            names.contains(&"r1") && names.contains(&"r2"),
            "both alignments must survive a concurrent add: {:?}",
            names
        );
        let ids: std::collections::HashSet<&str> =
            p.alignments.iter().map(|al| al.id.as_str()).collect();
        assert_eq!(ids.len(), p.alignments.len(), "ids must be unique");
    }

    #[tokio::test]
    async fn commit_computed_alignment_merges_onto_changed_list() {
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write()
            .await
            .open_project(
                "p1".to_string(),
                ProjectData {
                    sequence: "ACGT".repeat(50),
                    length: 200,
                    topology: "linear".to_string(),
                    alignments: vec![
                        libregene_core::models::Alignment {
                            id: "aln-1".to_string(),
                            name: "old".to_string(),
                            ..Default::default()
                        },
                        // A concurrent add that slipped in after the snapshot.
                        libregene_core::models::Alignment {
                            id: "aln-9".to_string(),
                            name: "concurrent".to_string(),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
            )
            .unwrap();
        // The computed state derives from a snapshot of [aln-1] plus the new
        // alignment (id allocated from the snapshot).
        let computed = ProjectData {
            alignments: vec![
                libregene_core::models::Alignment {
                    id: "aln-1".to_string(),
                    name: "old".to_string(),
                    ..Default::default()
                },
                libregene_core::models::Alignment {
                    id: "aln-2".to_string(),
                    name: "new".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        commit_computed_alignment(&pm, "p1", &["aln-1".to_string()], computed).await;
        let pmr = pm.read().await;
        let p = pmr.get_project_by_id("p1").unwrap();
        let ids: Vec<&str> = p.alignments.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids.len(), 3, "{ids:?}");
        assert!(ids.contains(&"aln-1") && ids.contains(&"aln-9"), "{ids:?}");
        let new = p.alignments.iter().find(|a| a.name == "new").unwrap();
        assert!(
            new.id != "aln-2" || !ids[..2].contains(&"aln-2"),
            "id re-allocated from the live list: {ids:?}"
        );
    }

    #[test]
    fn merge_recomputed_after_edit_cas_and_primer_merge() {
        let mk_primer = |id: &str, sites: usize| Primer {
            id: id.to_string(),
            name: id.to_string(),
            r#type: "fwd".to_string(),
            primer_seq: "ACGT".to_string(),
            binding_sites: vec![
                libregene_core::models::PrimerBindingSite {
                    primer_id: id.to_string(),
                    strand: 1,
                    template_start: 0,
                    template_end: 4,
                    tm: 60.0,
                    gc_content: 0.5,
                    match_score: 4,
                    has_3_prime_mismatch: false,
                    five_prime_tail: String::new(),
                    three_prime_tail: String::new(),
                    alignment: Default::default(),
                };
                sites
            ],
        };
        // Same sequence: enzymes replaced, existing primer's sites updated,
        // concurrently added primer kept, concurrently deleted primer gone.
        let mut live = ProjectData {
            sequence: "ACGTACGT".to_string(),
            length: 8,
            primers: vec![mk_primer("keep", 0), mk_primer("added", 1)],
            ..Default::default()
        };
        let computed = ProjectData {
            sequence: "ACGTACGT".to_string(),
            length: 8,
            enzymes: vec![libregene_core::models::Enzyme {
                name: "EcoRI".to_string(),
                ..Default::default()
            }],
            primers: vec![mk_primer("keep", 2), mk_primer("deleted", 3)],
            ..Default::default()
        };
        assert!(merge_recomputed_after_edit(&mut live, computed));
        assert_eq!(live.enzymes.len(), 1);
        assert_eq!(live.enzymes[0].name, "EcoRI");
        let ids: Vec<&str> = live.primers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["keep", "added"], "{ids:?}");
        assert_eq!(live.primers[0].binding_sites.len(), 2);
        assert_eq!(live.primers[1].binding_sites.len(), 1);

        // Sequence changed under the recompute: nothing is written back.
        let mut live = ProjectData {
            sequence: "TTTT".to_string(),
            length: 4,
            ..Default::default()
        };
        let computed = ProjectData {
            sequence: "ACGTACGT".to_string(),
            length: 8,
            enzymes: vec![libregene_core::models::Enzyme {
                name: "EcoRI".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(!merge_recomputed_after_edit(&mut live, computed));
        assert!(live.enzymes.is_empty());
    }
}
