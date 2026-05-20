//! REST API routes matching the Python FastAPI server.
//!
//! All endpoints use `GET /project`-style paths and camelCase JSON bodies
//! (via serde `rename_all` on the model structs).

use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use geneie_core::enzyme;
use geneie_core::file_io;
use geneie_core::models::{Feature, Primer};
use geneie_core::primer;
use geneie_core::project::ProjectManager;

use crate::ws;

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub pm: Arc<RwLock<ProjectManager>>,
    pub ws_tx: broadcast::Sender<String>,
    pub base_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Query parameter structs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct OpenParams {
    pub path: String,
}

#[derive(Deserialize)]
pub struct SaveParams {
    pub path: String,
}

#[derive(Deserialize)]
pub struct RoiParams {
    pub s: i64,
    pub e: i64,
}

#[derive(Deserialize)]
pub struct MethylationParams {
    #[serde(default)]
    pub systems: Option<String>,
    #[serde(default = "default_methylation_overlap")]
    pub overlap: i64,
}

fn default_methylation_overlap() -> i64 { 2 }

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn build_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/project", get(get_project))
        .route("/open", post(post_open))
        .route("/save", post(post_save))
        .route("/sequence", put(put_sequence))
        .route("/roi", post(post_roi))
        .route("/roi/clear", post(post_roi_clear))
        .route("/features", get(get_features).post(post_feature))
        .route("/features/{id}", delete(delete_feature))
        .route("/primers", get(get_primers).post(post_primer))
        .route("/primers/{id}", delete(delete_primer))
        .route("/methylation", post(post_methylation))
        .route("/ws", get(ws::ws_handler))
        // Multi-project management
        .route("/projects", get(get_projects))
        .route("/projects/activate", post(post_activate))
        .route("/projects/{id}", delete(delete_project))
}

// ---------------------------------------------------------------------------
// Path validation (anti path-traversal)
// ---------------------------------------------------------------------------

/// Validate that a file path is safe — allows only absolute paths within the
/// current working directory, or relative paths (which are resolved relative
/// to the cwd at the time of the call).  Rejects paths containing `..`.
fn validate_path(path_str: &str, base_dir: &std::path::PathBuf) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(path_str);

    // Reject explicit parent-dir traversal
    if path.components().any(|c| c == std::path::Component::ParentDir) {
        return Err("Path traversal detected: '..' is not allowed".to_string());
    }

    // Resolve relative paths against the base_dir; absolute paths are used as-is
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };

    let canonical = resolved
        .canonicalize()
        .map_err(|e| format!("Invalid path '{}': {}", path_str, e))?;
    if !canonical.starts_with(base_dir) {
        return Err(format!(
            "Path is outside base directory: {}",
            path_str
        ));
    }

    Ok(canonical)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ProjectParams {
    /// Enzyme filter: "unique" (default) or "all"
    #[serde(default)]
    pub enzyme_filter: Option<String>,
    /// Visible row range for enzyme pagination (0-indexed, inclusive)
    #[serde(default)]
    pub row_start: Option<i64>,
    #[serde(default)]
    pub row_end: Option<i64>,
    /// Characters per line (used with row_start/row_end to compute index range)
    #[serde(default)]
    pub cpl: Option<i64>,
}

const DEFAULT_CPL: i64 = 60;

/// GET /project — full project state as camelCase JSON.
/// Supports ?enzyme_filter=all and ?row_start=N&row_end=M&cpl=60 for pagination.
async fn get_project(
    Query(params): Query<ProjectParams>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let pm = state.pm.read().await;
    match pm.get_project() {
        Some(p) => Json(filter_project(p, &params)),
        None => Json(serde_json::json!({"error": "No project loaded"})),
    }
}

fn filter_project(project: &geneie_core::models::ProjectData, params: &ProjectParams) -> serde_json::Value {
    let filter = params.enzyme_filter.as_deref().unwrap_or("unique");
    let cpl = params.cpl.unwrap_or(DEFAULT_CPL);
    // Clamp row_start/row_end to non-negative
    let row_start = params.row_start.map(|rs| rs.max(0));
    let row_end = params.row_end.map(|re| re.max(0));

    let enzymes: Vec<&geneie_core::models::Enzyme> = if filter == "all" {
        let all: Vec<_> = project.enzymes.iter().collect();
        if let (Some(rs), Some(re)) = (row_start, row_end) {
            let idx_s = rs * cpl;
            let idx_e = (re + 1) * cpl - 1;
            all.into_iter().filter(|e| e.cut_index >= idx_s && e.cut_index <= idx_e).collect()
        } else {
            all
        }
    } else {
        // Default: unique only, paginated by row range
        let mut seen: std::collections::HashMap<&str, Vec<&geneie_core::models::Enzyme>> = std::collections::HashMap::new();
        for e in &project.enzymes {
            seen.entry(&e.name).or_default().push(e);
        }
        let mut unique: Vec<&geneie_core::models::Enzyme> = seen
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
        "methylation_systems": &project.methylation_systems,
        "methylation_overlap": project.methylation_overlap,
        "roi": &project.roi,
        "enzymeCount": project.enzymes.len(),
        "enzymeFilter": filter,
        "enzymes": &enzymes,
    })
}

/// POST /open?path=... — load a file (.gbk, .dna, .fasta).
async fn post_open(
    Query(params): Query<OpenParams>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let path = match validate_path(&params.path, &state.base_dir) {
        Err(e) => return Json(serde_json::json!({"error": e})),
        Ok(p) => p,
    };
    let id = params.path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut project = file_io::parse_file(&path)?;
        enzyme::recompute(&mut project);
        primer::recompute(&mut project);
        Ok::<_, std::io::Error>(project)
    })
    .await;

    match result {
        Ok(Ok(project)) => {
            let mut pm = state.pm.write().await;
            pm.load(&id, project);
            drop(pm);
            broadcast(&state).await;
            Json(serde_json::json!({"status": "ok"}))
        }
        Ok(Err(e)) => Json(serde_json::json!({"error": e.to_string()})),
        Err(e) => Json(serde_json::json!({"error": format!("task join error: {}", e)})),
    }
}

/// POST /save?path=... — save project as GenBank.
async fn post_save(
    Query(params): Query<SaveParams>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let save_path = match validate_path(&params.path, &state.base_dir) {
        Err(e) => return Json(serde_json::json!({"error": e})),
        Ok(p) => p,
    };
    let pm = state.pm.read().await;
    match pm.get_project() {
        Some(project) => {
            match file_io::gbk::write_gbk(project, &save_path) {
                Ok(()) => Json(serde_json::json!({"status": "ok"})),
                Err(e) => Json(serde_json::json!({"error": e.to_string()})),
            }
        }
        None => Json(serde_json::json!({"error": "No project loaded"})),
    }
}

/// PUT /sequence — replace the full sequence.
async fn put_sequence(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let seq = body["sequence"].as_str().unwrap_or("").to_string();

    // Capture project clone + active_id atomically while holding the write lock
    let (project, active_id) = {
        let mut pm = state.pm.write().await;
        pm.update_sequence(seq);
        (pm.get_project().cloned(), pm.active_id().map(|s| s.to_string()))
    };

    // Recompute enzymes and primers on the active project outside the lock
    if let Some(mut p) = project {
        let computed = tokio::task::spawn_blocking(move || {
            enzyme::recompute(&mut p);
            primer::recompute(&mut p);
            p
        }).await;
        if let Ok(new_p) = computed {
            let mut pm = state.pm.write().await;
            if let Some(ref id) = active_id {
                pm.open_project(id.clone(), new_p);
            }
        }
    }

    broadcast(&state).await;
    Json(serde_json::json!({"status": "ok"}))
}

/// POST /roi?s=...&e=... — set region of interest.
async fn post_roi(
    Query(params): Query<RoiParams>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let mut pm = state.pm.write().await;
    pm.set_roi(params.s, params.e);
    Json(serde_json::json!({"status": "ok"}))
}

/// POST /roi/clear — clear region of interest.
async fn post_roi_clear(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let mut pm = state.pm.write().await;
    pm.clear_roi();
    Json(serde_json::json!({"status": "ok"}))
}

/// GET /features — list all features.
async fn get_features(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let pm = state.pm.read().await;
    match pm.get_project() {
        Some(p) => Json(serde_json::to_value(&p.features).unwrap_or(serde_json::json!([]))),
        None => Json(serde_json::json!([])),
    }
}

/// POST /features — add or replace a feature (matched by id).
async fn post_feature(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Feature>,
) -> Json<serde_json::Value> {
    let mut pm = state.pm.write().await;
    let mut feats: Vec<Feature> = pm
        .get_project()
        .map(|p| p.features.clone())
        .unwrap_or_default();

    if let Some(pos) = feats.iter().position(|f| f.id == body.id) {
        feats[pos] = body;
    } else {
        feats.push(body);
    }
    pm.update_features(feats);
    drop(pm);
    broadcast(&state).await;
    Json(serde_json::json!({"status": "ok"}))
}

/// DELETE /features/{id} — delete a feature.
async fn delete_feature(
    Path(fid): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let mut pm = state.pm.write().await;
    let feats: Vec<Feature> = pm
        .get_project()
        .map(|p| p.features.iter().filter(|f| f.id != fid).cloned().collect())
        .unwrap_or_default();
    pm.update_features(feats);
    drop(pm);
    broadcast(&state).await;
    Json(serde_json::json!({"status": "ok"}))
}

/// GET /primers — list primers.
async fn get_primers(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let pm = state.pm.read().await;
    match pm.get_project() {
        Some(p) => {
            let primers: Vec<serde_json::Value> = p
                .primers
                .iter()
                .map(|primer| serde_json::to_value(primer).unwrap_or(serde_json::json!({})))
                .collect();
            Json(serde_json::json!(primers))
        }
        None => Json(serde_json::json!([])),
    }
}

/// POST /primers — create or update a primer (matched by id).
async fn post_primer(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Primer>,
) -> Json<serde_json::Value> {
    let mut pm = state.pm.write().await;
    let mut primers: Vec<Primer> = pm
        .get_project()
        .map(|p| p.primers.clone())
        .unwrap_or_default();

    if let Some(pos) = primers.iter().position(|p| p.id == body.id) {
        primers[pos] = body;
    } else {
        primers.push(body);
    }

    // Recompute binding sites for all primers.
    if let Some(project) = pm.get_project() {
        let template = project.sequence.clone();
        let topology = project.topology.clone();
        let updated = geneie_core::primer::align::recompute_all_primers(&template, &topology, &primers);
        pm.update_primers(updated);
    } else {
        pm.update_primers(primers);
    }
    drop(pm);
    broadcast(&state).await;
    Json(serde_json::json!({"status": "ok"}))
}

/// DELETE /primers/{id} — delete a primer.
async fn delete_primer(
    Path(pid): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let mut pm = state.pm.write().await;
    let primers: Vec<Primer> = pm
        .get_project()
        .map(|p| p.primers.iter().filter(|pr| pr.id != pid).cloned().collect())
        .unwrap_or_default();
    pm.update_primers(primers);
    drop(pm);
    broadcast(&state).await;
    Json(serde_json::json!({"status": "ok"}))
}

/// POST /methylation?systems=dam,dcm,ecoki — set active methylation systems.
async fn post_methylation(
    Query(params): Query<MethylationParams>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let systems: Vec<String> = params
        .systems
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_lowercase())
        .collect();

    // Set methylation systems + overlap, and clone project data outside the lock
    let project_data = {
        let mut pm = state.pm.write().await;
        pm.set_methylation_systems(systems);
        if let Some(p) = pm.get_project_mut() {
            p.methylation_overlap = params.overlap;
        }
        pm.get_project().cloned()
    };

    // Recompute enzymes in a blocking task, without holding the lock
    if let Some(mut p) = project_data {
        let computed = tokio::task::spawn_blocking(move || {
            enzyme::recompute_methylation_only(&mut p);
            p
        })
        .await;

        if let Ok(new_p) = computed {
            let mut pm = state.pm.write().await;
            if let Some(id) = pm.active_id().map(|s| s.to_string()) {
                pm.open_project(id, new_p);
            }
        }
    }

    broadcast(&state).await;
    Json(serde_json::json!({"status": "ok"}))
}

// ---------------------------------------------------------------------------
// Multi-project management
// ---------------------------------------------------------------------------

/// GET /projects — list all open projects (summary for tab bar).
async fn get_projects(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let pm = state.pm.read().await;
    let projects = pm.list_projects();
    let active = pm.active_id().map(|s| s.to_string());
    Json(serde_json::json!({
        "projects": projects,
        "activeId": active,
    }))
}

/// POST /projects/activate?id=... — switch the active project.
async fn post_activate(
    Query(params): Query<ActivateParams>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let activated = {
        let mut pm = state.pm.write().await;
        pm.activate_project(&params.id)
    };
    if activated {
        broadcast(&state).await;
        Json(serde_json::json!({"status": "ok"}))
    } else {
        Json(serde_json::json!({"error": format!("project not found: {}", params.id)}))
    }
}

#[derive(Deserialize)]
struct ActivateParams {
    id: String,
}

/// DELETE /projects/{id} — close a project.
async fn delete_project(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let closed = {
        let mut pm = state.pm.write().await;
        pm.close_project(&id)
    };
    if closed {
        broadcast(&state).await;
        Json(serde_json::json!({"status": "ok"}))
    } else {
        Json(serde_json::json!({"error": "project not found"}))
    }
}

// ---------------------------------------------------------------------------
// WebSocket broadcast helper
// ---------------------------------------------------------------------------

/// Broadcast the current project state + project list to all WebSocket clients.
/// Uses default filtering (unique enzymes only) to keep payload small.
pub async fn broadcast(state: &Arc<AppState>) {
    // Skip if no WebSocket clients are connected
    if state.ws_tx.receiver_count() == 0 {
        return;
    }
    let pm = state.pm.read().await;
    if let Some(project) = pm.get_project() {
        let projects = pm.list_projects();
        let active_id = pm.active_id().map(|s| s.to_string());
        let filter_params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        let filtered = filter_project(project, &filter_params);
        if let Ok(json) = serde_json::to_string(&serde_json::json!({
            "type": "project",
            "data": filtered,
            "projects": projects,
            "activeId": active_id,
        })) {
            let _ = state.ws_tx.send(json);
        }
    }
}
