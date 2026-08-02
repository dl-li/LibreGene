//! Embedded MCP (Model Context Protocol) server for LibreGene.
//!
//! Exposes tools over Streamable HTTP on `127.0.0.1:8766` so an external LLM
//! agent can operate the app like a real user. Mutations go through the same
//! shared cores as the Tauri commands (`crate::do_*`), so recompute, dirty
//! marking and `broadcast_project()` behave identically and the UI updates
//! live. Every mutation tool returns a uniform `{ok, message, projectId,
//! regionView?}` envelope (plus tool-specific fields).
//!
//! Coordinate conventions (stated again in every tool description):
//! - features, primer binding sites and read ranges are **0-based inclusive**
//! - primer `template_end` is **exclusive** (render range is `start..end-1`)
//! - enzyme cuts happen **between `pos-1` and `pos`**
//! - circular sequences allow `start > end` to wrap the origin for reads;
//!   edit ranges must not wrap (`end = start - 1` is a pure insertion)

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};

use rmcp::{
    ErrorData, ServerHandler,
    handler::server::wrapper::{Json, Parameters},
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session,
    },
};
use serde::Deserialize;
use tokio::sync::RwLock;
use tauri::{AppHandle, Runtime};

use libregene_core::digest::{DigestOptions, project_digest, read_sequence};
use libregene_core::models::{Feature, Primer, ProjectData};
use libregene_core::project::ProjectManager;

/// Loopback port for the embedded MCP server (settings toggle comes later).
pub const MCP_PORT: u16 = 8766;

// ---------------------------------------------------------------------------
// Tool request payloads (also used to generate JSON input schemas)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct OverviewRequest {
    project_id: Option<String>,
    max_features: Option<usize>,
    feature_filter: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct RegionRequest {
    project_id: Option<String>,
    start: i64,
    end: i64,
    max_features: Option<usize>,
    feature_filter: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct SequenceRequest {
    project_id: Option<String>,
    start: i64,
    end: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct SearchRequest {
    query: String,
    project_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct OpenFileRequest {
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct SaveFileRequest {
    project_id: Option<String>,
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct CloseProjectRequest {
    project_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ActivateProjectRequest {
    project_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct EditSequenceRequest {
    project_id: Option<String>,
    start: i64,
    end: i64,
    replacement: String,
    expected_old: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AddFeatureRequest {
    project_id: Option<String>,
    name: String,
    ftype: String,
    location: String,
    strand: Option<String>,
    color: Option<String>,
    notes: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct FeatureIdValueRequest {
    project_id: Option<String>,
    feature_id: String,
    value: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct FeatureIdRequest {
    project_id: Option<String>,
    feature_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AddPrimerRequest {
    project_id: Option<String>,
    name: String,
    #[serde(rename = "type")]
    r#type: String,
    seq: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct DeletePrimerRequest {
    project_id: Option<String>,
    primer_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct SetMethylationRequest {
    project_id: Option<String>,
    systems: Vec<String>,
    overlap: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AddAlignmentRequest {
    project_id: Option<String>,
    name: String,
    seq: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct RemoveAlignmentRequest {
    project_id: Option<String>,
    alignment_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct FindOrfsRequest {
    project_id: Option<String>,
    min_aa: Option<usize>,
    add_as_features: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
struct SegParam {
    start: i64,
    end: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct DesignPrimersRequest {
    project_id: Option<String>,
    mode: String,
    seg: Option<SegParam>,
    seg2: Option<SegParam>,
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AnalyzePcrRequest {
    project_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct PrimerInput {
    name: String,
    #[serde(rename = "type")]
    r#type: String,
    seq: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct CheckPrimerBindingRequest {
    project_id: Option<String>,
    primers: Vec<PrimerInput>,
}

// ---------------------------------------------------------------------------
// Server handler
// ---------------------------------------------------------------------------

pub struct LibreGeneMcp<R: Runtime> {
    app_handle: AppHandle<R>,
    pm: Arc<RwLock<ProjectManager>>,
    wp: Arc<RwLock<HashMap<String, String>>>,
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_id(prefix: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}", prefix, millis, n)
}

fn ok_envelope(project_id: &str, message: String, region_view: Option<String>) -> serde_json::Value {
    let mut v = serde_json::json!({
        "ok": true,
        "message": message,
        "projectId": project_id,
    });
    if let Some(rv) = region_view {
        v["regionView"] = serde_json::json!(rv);
    }
    v
}

fn fail_envelope(project_id: &str, message: String) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "message": message,
        "projectId": project_id,
    })
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub fn new(
        app_handle: AppHandle<R>,
        pm: Arc<RwLock<ProjectManager>>,
        wp: Arc<RwLock<HashMap<String, String>>>,
    ) -> Self {
        Self { app_handle, pm, wp }
    }

    /// Explicit project id or the active project.
    async fn resolve_project_id(&self, project_id: Option<String>) -> Result<String, ErrorData> {
        let pm = self.pm.read().await;
        match project_id {
            Some(id) => Ok(id),
            None => pm.active_id().map(|s| s.to_string()).ok_or_else(|| {
                ErrorData::invalid_params("No project loaded — open a file or pass project_id", None)
            }),
        }
    }

    /// Resolve the project id and clone its data out of the lock.
    async fn resolve_project(&self, project_id: Option<String>) -> Result<(String, ProjectData), ErrorData> {
        let pm = self.pm.read().await;
        let id = match project_id {
            Some(id) => id,
            None => pm.active_id().map(|s| s.to_string()).ok_or_else(|| {
                ErrorData::invalid_params("No project loaded — open a file or pass project_id", None)
            })?,
        };
        let project = pm.get_project_by_id(&id).cloned().ok_or_else(|| {
            ErrorData::invalid_params(format!("Project not found: {}", id), None)
        })?;
        Ok((id, project))
    }

    async fn project_summary(&self, project_id: &str) -> Option<String> {
        let pm = self.pm.read().await;
        let p = pm.get_project_by_id(project_id)?;
        Some(format!("{}: {} bp {}", p.name, p.length, p.topology))
    }

    /// Text digest of `region` (0-based inclusive, may wrap on circular) or the
    /// whole project when `None`.
    async fn digest_region(&self, project_id: &str, region: Option<(i64, i64)>) -> Option<String> {
        let pm = self.pm.read().await;
        let project = pm.get_project_by_id(project_id)?;
        project_digest(project, &DigestOptions::default(), region).ok()
    }

    /// Text digest of the region around a feature (looked up by id).
    async fn digest_feature_region(&self, project_id: &str, feature_id: &str) -> Option<String> {
        let pm = self.pm.read().await;
        let project = pm.get_project_by_id(project_id)?;
        let f = project.features.iter().find(|f| f.id == feature_id)?;
        let s = (f.start - 5).max(0);
        let e = (f.end + 5).min(project.length - 1);
        project_digest(project, &DigestOptions::default(), Some((s, e))).ok()
    }

    async fn feature_exists(&self, project_id: &str, feature_id: &str) -> bool {
        let pm = self.pm.read().await;
        pm.get_project_by_id(project_id)
            .map(|p| p.features.iter().any(|f| f.id == feature_id))
            .unwrap_or(false)
    }

    /// A `{"error": ...}` payload from a shared core means a tool-level failure.
    fn payload_error(payload: &serde_json::Value) -> Option<String> {
        payload.get("error").and_then(|v| v.as_str()).map(String::from)
    }
}

#[tool_router]
impl<R: Runtime> LibreGeneMcp<R> {
    /// List all open projects. Returns {"projects": [{id, name, length, topology, dirty}], "activeId": id-or-null}.
    #[tool]
    async fn list_projects(&self) -> Result<Json<serde_json::Value>, ErrorData> {
        let pm = self.pm.read().await;
        let projects = pm.list_projects();
        let active_id = pm.active_id().map(|s| s.to_string());
        Ok(Json(serde_json::json!({
            "projects": projects,
            "activeId": active_id,
        })))
    }

    /// Compact text digest of a whole project. Coordinates are 0-based inclusive
    /// (features, primers, read ranges); primer template_end is exclusive; enzyme
    /// cuts happen between pos-1 and pos. feature_filter matches feature name
    /// (case-insensitive substring) or exact ftype. Returns {projectId, text}.
    #[tool]
    async fn get_project_overview(
        &self,
        Parameters(request): Parameters<OverviewRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
        };
        let text = project_digest(&project, &opts, None)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "text": text })))
    }

    /// Compact text digest of a region of a project. Coordinates are 0-based
    /// inclusive; on circular sequences start > end wraps the origin. Only
    /// features, primer binding sites and enzyme cut positions overlapping
    /// [start, end] are included. Returns {projectId, text}.
    #[tool]
    async fn get_region_view(
        &self,
        Parameters(request): Parameters<RegionRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
        };
        let text = project_digest(&project, &opts, Some((request.start, request.end)))
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "text": text })))
    }

    /// Read bases of a project's sequence with a coordinate ruler (10 bp groups,
    /// 60 bp per line). Coordinates are 0-based inclusive; on circular sequences
    /// start > end wraps the origin. Windows larger than 10000 bp are rejected.
    /// Returns {projectId, text}.
    #[tool]
    async fn read_sequence(
        &self,
        Parameters(request): Parameters<SequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let text = read_sequence(&project, request.start, request.end)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "text": text })))
    }

    /// IUPAC-aware search of a project's sequence on both strands (reverse
    /// strand skipped for palindromic queries). Hits are 0-based inclusive.
    /// Returns {projectId, matches: [{start, end, strand}]}.
    #[tool]
    async fn search_sequence(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let matches = crate::do_search_sequence(&self.pm, &id, request.query)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "matches": matches })))
    }

    /// Full static enzyme database (~196 KB, ~1100 entries) as JSON: name,
    /// recognition site, cut offsets, cut type, methylation sensitivity.
    /// The project digest already summarizes which enzymes actually cut the
    /// current sequence — pull this only when you need the full catalog.
    #[tool]
    async fn get_enzyme_database(&self) -> Result<Json<serde_json::Value>, ErrorData> {
        let db = libregene_core::enzyme::search::get_db();
        let value = serde_json::to_value(&db.enzymes)
            .map_err(|e| ErrorData::internal_error(format!("serialize enzyme db: {e}"), None))?;
        Ok(Json(value))
    }

    // -----------------------------------------------------------------------
    // Mutations
    // -----------------------------------------------------------------------

    /// Open a GenBank/FASTA file into the project manager (project id = file
    /// path). Enzyme and primer recompute run on a background thread; the UI is
    /// refreshed via broadcast. Returns {ok, message, projectId, regionView}
    /// where regionView is the full overview digest of the opened project.
    #[tool]
    async fn open_file(
        &self,
        Parameters(request): Parameters<OpenFileRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = request.path.clone();
        let payload = crate::do_open_file(&self.pm, request.path)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        // The open_file command does not broadcast (frontend applies the
        // response) — the MCP server must notify the UI itself.
        crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, None).await;
        let summary = self.project_summary(&id).await.unwrap_or_else(|| format!("Opened {}", id));
        let region = self.digest_region(&id, None).await;
        Ok(Json(ok_envelope(&id, summary, region)))
    }

    /// Save a project to a GenBank file on disk. Uses the same serializer and
    /// mark-clean logic as the save_file command. Returns the uniform envelope
    /// with the overview digest.
    #[tool]
    async fn save_file(
        &self,
        Parameters(request): Parameters<SaveFileRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let path = request.path.clone();
        let payload = crate::do_save_file(&self.pm, id.clone(), request.path)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, None).await;
        let region = self.digest_region(&id, None).await;
        Ok(Json(ok_envelope(&id, format!("Saved {}", path), region)))
    }

    /// Close (unload) a project without saving. Mirrors delete_project; the UI
    /// updates via broadcast. Returns {ok, message, projectId}.
    #[tool]
    async fn close_project(
        &self,
        Parameters(request): Parameters<CloseProjectRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = request.project_id.clone();
        let payload = crate::do_delete_project(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            request.project_id,
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        Ok(Json(serde_json::json!({
            "ok": true,
            "message": format!("Closed project {}", id),
            "projectId": id,
        })))
    }

    /// Make a project the active one (mirrors activate_project). Returns
    /// {ok, message, projectId, regionView}.
    #[tool]
    async fn activate_project(
        &self,
        Parameters(request): Parameters<ActivateProjectRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = request.project_id.clone();
        let payload = crate::do_activate_project(&self.pm, request.project_id)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, None).await;
        let region = self.digest_region(&id, None).await;
        Ok(Json(ok_envelope(&id, format!("Activated {}", id), region)))
    }

    /// Replace sequence [start..end] (0-based inclusive) with `replacement`
    /// (empty = delete). A pure insertion is `end = start - 1`; ranges must not
    /// wrap (start > end+1 rejected). When `expected_old` is given it must match
    /// the current [start..end] content case-insensitively or the edit is
    /// rejected with the actual content. Uses the same primer+enzyme recompute
    /// path as update_sequence. Returns newLength, old/new region views and
    /// 30 bp sequence context on each side of the edit.
    #[tool]
    async fn edit_sequence(
        &self,
        Parameters(request): Parameters<EditSequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let len = project.length;
        let start = request.start;
        let end = request.end;

        if start > end + 1 {
            return Ok(Json(fail_envelope(
                &id,
                format!(
                    "invalid range {}..{}: start > end+1; ranges must not wrap (end = start-1 is a pure insertion)",
                    start, end
                ),
            )));
        }
        if start < 0 || start > len || end < -1 || end >= len {
            return Ok(Json(fail_envelope(
                &id,
                format!(
                    "range {}..{} out of bounds for sequence of length {} (0-based inclusive)",
                    start, end, len
                ),
            )));
        }

        let is_insertion = end + 1 == start;
        let current: String = if is_insertion {
            String::new()
        } else {
            project.sequence[start as usize..=end as usize].to_string()
        };
        if let Some(expected) = &request.expected_old {
            if !current.eq_ignore_ascii_case(expected) {
                let mut v = fail_envelope(
                    &id,
                    format!(
                        "expected_old mismatch: expected '{}' but current [{}..{}] is '{}'",
                        expected, start, end, current
                    ),
                );
                v["currentContent"] = serde_json::json!(current);
                return Ok(Json(v));
            }
        }

        let context_before = project.sequence[(start - 30).max(0) as usize..start as usize].to_string();
        let context_after_end = (end + 1 + 30).min(len) as usize;
        let context_after = project.sequence[(end + 1) as usize..context_after_end].to_string();

        let old_win = (
            (start - 30).max(0),
            (end + 30).min(len - 1),
        );
        let old_region = project_digest(&project, &DigestOptions::default(), Some(old_win))
            .ok();

        let new_seq = format!(
            "{}{}{}",
            &project.sequence[..start as usize],
            request.replacement,
            &project.sequence[(end + 1) as usize..]
        );
        let new_len = new_seq.len() as i64;

        let payload = crate::do_update_sequence(&self.pm, id.clone(), new_seq)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        // update_sequence core does not broadcast — notify the UI ourselves.
        crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, None).await;

        let repl_len = request.replacement.len() as i64;
        let new_win = (
            (start - 30).max(0),
            (start + repl_len + 30 - 1).min(new_len - 1),
        );
        let new_region = self.digest_region(&id, Some(new_win)).await;

        let mut v = serde_json::json!({
            "ok": true,
            "message": format!(
                "Replaced [{}..{}] ({} bp) with {} bp; new length {} (was {})",
                start, end,
                if is_insertion { 0 } else { end - start + 1 },
                repl_len,
                new_len,
                len
            ),
            "projectId": id,
            "oldLength": len,
            "newLength": new_len,
            "contextBefore": context_before,
            "contextAfter": context_after,
        });
        if let Some(rv) = old_region {
            v["regionViewBefore"] = serde_json::json!(rv);
        }
        if let Some(rv) = new_region {
            v["regionView"] = serde_json::json!(rv);
        }
        Ok(Json(v))
    }

    /// Add a feature. `location` is a GenBank 1-based location string
    /// (e.g. "100..200", "complement(50..80)", "join(1..100,200..300)"); the
    /// stored coordinates are 0-based inclusive. strand (".", "+", "-") and
    /// color (hex, e.g. "#60A5FA") are optional and override the location.
    /// Returns {ok, message, projectId, regionView} around the new feature.
    #[tool]
    async fn add_feature(
        &self,
        Parameters(request): Parameters<AddFeatureRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let feature_id = next_id("feature");
        let name = request.name.clone();
        let location = request.location.clone();
        let ftype = request.ftype.clone();

        let parsed = libregene_core::file_io::gbk::parse_location_string(&location)
            .ok_or_else(|| ErrorData::invalid_params(format!("Invalid location: {}", location), None))?;
        let (segments, start, end, location_strand) = parsed;
        let strand = request.strand.clone().unwrap_or(location_strand);

        let feature = Feature {
            id: feature_id.clone(),
            name: request.name,
            start,
            end,
            color: request.color.unwrap_or_else(|| "#60A5FA".to_string()),
            ftype: request.ftype,
            segments,
            strand,
            notes: request.notes.unwrap_or_default(),
            translation: String::new(),
            qualifiers: Vec::new(),
        };

        let payload = crate::do_add_features(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            vec![feature],
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let region = self.digest_feature_region(&id, &feature_id).await;
        Ok(Json(ok_envelope(
            &id,
            format!("Added {} {} at {}", ftype, name, location),
            region,
        )))
    }

    /// Update a feature's location (GenBank 1-based string, same formats as
    /// add_feature). Returns the uniform envelope with the region digest.
    #[tool]
    async fn update_feature_location(
        &self,
        Parameters(request): Parameters<FeatureIdValueRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        if !self.feature_exists(&id, &request.feature_id).await {
            return Ok(Json(fail_envelope(&id, format!("Feature not found: {}", request.feature_id))));
        }
        let feature_id = request.feature_id.clone();
        let location = request.value.clone();
        let payload = crate::do_update_feature(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            &request.feature_id,
            |f| {
                let parsed = libregene_core::file_io::gbk::parse_location_string(&location)
                    .ok_or_else(|| format!("Invalid location: {}", location))?;
                let (segments, start, end, strand) = parsed;
                f.segments = segments;
                f.start = start;
                f.end = end;
                f.strand = strand;
                Ok(())
            },
        )
        .await
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let region = self.digest_feature_region(&id, &feature_id).await;
        Ok(Json(ok_envelope(&id, format!("Moved feature {}", feature_id), region)))
    }

    /// Rename a feature. Returns the uniform envelope with the region digest.
    #[tool]
    async fn update_feature_name(
        &self,
        Parameters(request): Parameters<FeatureIdValueRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        if !self.feature_exists(&id, &request.feature_id).await {
            return Ok(Json(fail_envelope(&id, format!("Feature not found: {}", request.feature_id))));
        }
        let feature_id = request.feature_id.clone();
        let payload = crate::do_update_feature(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            &request.feature_id,
            |f| {
                f.name = request.value;
                Ok(())
            },
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let region = self.digest_feature_region(&id, &feature_id).await;
        Ok(Json(ok_envelope(&id, format!("Renamed feature {}", feature_id), region)))
    }

    /// Recolor a feature (hex like "#F87171"). Returns the uniform envelope.
    #[tool]
    async fn update_feature_color(
        &self,
        Parameters(request): Parameters<FeatureIdValueRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        if !self.feature_exists(&id, &request.feature_id).await {
            return Ok(Json(fail_envelope(&id, format!("Feature not found: {}", request.feature_id))));
        }
        let feature_id = request.feature_id.clone();
        let payload = crate::do_update_feature(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            &request.feature_id,
            |f| {
                f.color = request.value.clone();
                for seg in f.segments.iter_mut() {
                    seg.color = Some(request.value.clone());
                }
                Ok(())
            },
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let region = self.digest_feature_region(&id, &feature_id).await;
        Ok(Json(ok_envelope(&id, format!("Recolored feature {}", feature_id), region)))
    }

    /// Change a feature's ftype (CDS, promoter, terminator, ...). Returns the
    /// uniform envelope.
    #[tool]
    async fn update_feature_ftype(
        &self,
        Parameters(request): Parameters<FeatureIdValueRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        if !self.feature_exists(&id, &request.feature_id).await {
            return Ok(Json(fail_envelope(&id, format!("Feature not found: {}", request.feature_id))));
        }
        let feature_id = request.feature_id.clone();
        let payload = crate::do_update_feature(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            &request.feature_id,
            |f| {
                f.ftype = request.value;
                Ok(())
            },
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let region = self.digest_feature_region(&id, &feature_id).await;
        Ok(Json(ok_envelope(&id, format!("Changed ftype of feature {}", feature_id), region)))
    }

    /// Change a feature's strand (".", "+" or "-"). Returns the uniform envelope.
    #[tool]
    async fn update_feature_strand(
        &self,
        Parameters(request): Parameters<FeatureIdValueRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        if !self.feature_exists(&id, &request.feature_id).await {
            return Ok(Json(fail_envelope(&id, format!("Feature not found: {}", request.feature_id))));
        }
        if !matches!(request.value.as_str(), "." | "+" | "-") {
            return Ok(Json(fail_envelope(&id, "Invalid strand: must be ., +, or -".to_string())));
        }
        let feature_id = request.feature_id.clone();
        let payload = crate::do_update_feature(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            &request.feature_id,
            |f| {
                f.strand = request.value;
                Ok(())
            },
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let region = self.digest_feature_region(&id, &feature_id).await;
        Ok(Json(ok_envelope(&id, format!("Set strand of feature {}", feature_id), region)))
    }

    /// Delete a feature. Returns {ok, message, projectId, regionView} — the
    /// region digest covers the deleted feature's location.
    #[tool]
    async fn delete_feature(
        &self,
        Parameters(request): Parameters<FeatureIdRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let range = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .and_then(|p| p.features.iter().find(|f| f.id == request.feature_id))
                .map(|f| (f.start, f.end))
        };
        let feature_id = request.feature_id.clone();
        let payload = crate::do_delete_feature(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            request.feature_id,
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let region = match range {
            Some((s, e)) => self.digest_region(&id, Some((s, e))).await,
            None => self.digest_region(&id, None).await,
        };
        Ok(Json(ok_envelope(&id, format!("Deleted feature {}", feature_id), region)))
    }

    /// Add a primer ("fwd" or "rev") and recompute its binding sites against
    /// the template. Returns {ok, message, projectId, bindingSites, regionView}
    /// — bindingSites: [{strand, templateStart, templateEnd, tm}], 0-based.
    #[tool]
    async fn add_primer(
        &self,
        Parameters(request): Parameters<AddPrimerRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let primer_id = next_id("primer");
        let name = request.name.clone();
        let primer = Primer {
            id: primer_id.clone(),
            name: request.name,
            r#type: request.r#type,
            primer_seq: request.seq,
            binding_sites: Vec::new(),
        };
        let payload = crate::do_add_primer(&self.app_handle, &self.pm, &self.wp, None, &id, primer)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let (sites, region) = {
            let pm = self.pm.read().await;
            let project = pm.get_project_by_id(&id);
            let primer = project.and_then(|p| p.primers.iter().find(|pr| pr.id == primer_id));
            match (project, primer) {
                (Some(p), Some(pr)) => {
                    let sites: Vec<serde_json::Value> = pr
                        .binding_sites
                        .iter()
                        .map(|s| {
                            serde_json::json!({
                                "strand": s.strand,
                                "templateStart": s.template_start,
                                "templateEnd": s.template_end,
                                "tm": (s.tm * 10.0).round() / 10.0,
                                "3PrimeMismatch": s.has_3_prime_mismatch,
                            })
                        })
                        .collect();
                    let region = pr.binding_sites.first().map(|s| {
                        (
                            (s.template_start - 10).max(0),
                            (s.template_end - 1 + 10).min(p.length - 1),
                        )
                    });
                    (sites, region)
                }
                _ => (Vec::new(), None),
            }
        };
        let region_view = match region {
            Some(r) => self.digest_region(&id, Some(r)).await,
            None => self.digest_region(&id, None).await,
        };
        let mut env = ok_envelope(
            &id,
            format!("Added primer {} ({} binding site(s))", name, sites.len()),
            region_view,
        );
        env["bindingSites"] = serde_json::json!(sites);
        Ok(Json(env))
    }

    /// Delete a primer by id. Returns the uniform envelope; regionView covers
    /// the deleted primer's former binding site when known.
    #[tool]
    async fn delete_primer(
        &self,
        Parameters(request): Parameters<DeletePrimerRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let region = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .and_then(|p| p.primers.iter().find(|pr| pr.id == request.primer_id))
                .and_then(|pr| pr.binding_sites.first())
                .map(|s| (s.template_start, s.template_end - 1))
        };
        let primer_id = request.primer_id.clone();
        let payload = crate::do_delete_primer(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            request.primer_id,
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let region_view = match region {
            Some((s, e)) => self.digest_region(&id, Some((s, e))).await,
            None => self.digest_region(&id, None).await,
        };
        Ok(Json(ok_envelope(
            &id,
            format!("Deleted primer {}", primer_id),
            region_view,
        )))
    }

    /// Set the project's methylation systems (e.g. ["Dam","Dcm"], case-insensitive)
    /// and optional +/- bp overlap beyond recognition sites. Recomputes enzyme
    /// methylation flags like the set_methylation command. Returns the uniform
    /// envelope with the overview digest.
    #[tool]
    async fn set_methylation(
        &self,
        Parameters(request): Parameters<SetMethylationRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let payload = crate::do_set_methylation(&self.pm, &id, request.systems, request.overlap)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        // set_methylation core does not broadcast.
        crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, None).await;
        let region = self.digest_region(&id, None).await;
        Ok(Json(ok_envelope(&id, "Updated methylation systems".to_string(), region)))
    }

    /// Align a read sequence (name + bases) against the project template using
    /// the same aligner as add_alignment_seq; whitespace/non-ACGT chars are
    /// stripped and the result is stored as a real alignment. Returns
    /// {ok, message, projectId, regionView, significant, identity, strand,
    /// segmentCount, insertions}.
    #[tool]
    async fn add_alignment(
        &self,
        Parameters(request): Parameters<AddAlignmentRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let name = request.name.clone();
        let payload = match crate::do_add_alignment_seq(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            request.name,
            request.seq,
        )
        .await
        {
            Ok(p) => p,
            Err(e) if e == "No significant alignment found" => {
                return Ok(Json(serde_json::json!({
                    "ok": false,
                    "message": e,
                    "projectId": id,
                    "significant": false,
                })));
            }
            Err(e) => return Err(ErrorData::internal_error(e, None)),
        };
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let summary = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .and_then(|p| p.alignments.last())
                .map(|a| {
                    (
                        serde_json::json!({
                            "identity": (a.identity * 100.0).round() / 100.0,
                            "strand": a.strand,
                            "segmentCount": a.segments.len(),
                            "insertions": a.insertions.len(),
                            "name": a.name,
                        }),
                        a.segments.first().map(|s| (s.start as i64, s.end as i64)),
                    )
                })
        };
        let region = match summary {
            Some((_, Some((s, e)))) => self.digest_region(&id, Some((s, e))).await,
            _ => self.digest_region(&id, None).await,
        };
        let mut env = ok_envelope(&id, format!("Aligned {}", name), region);
        if let Some((s, _)) = summary {
            env["significant"] = serde_json::json!(true);
            for (k, v) in s.as_object().unwrap_or(&serde_json::Map::new()) {
                env[k] = v.clone();
            }
        }
        Ok(Json(env))
    }

    /// Remove an alignment by id. Returns the uniform envelope with the
    /// overview digest.
    #[tool]
    async fn remove_alignment(
        &self,
        Parameters(request): Parameters<RemoveAlignmentRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let payload = crate::do_remove_alignment(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            request.alignment_id,
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let region = self.digest_region(&id, None).await;
        Ok(Json(ok_envelope(&id, "Removed alignment".to_string(), region)))
    }

    // -----------------------------------------------------------------------
    // Analysis
    // -----------------------------------------------------------------------

    /// Find open reading frames (ATG→stop, both strands, all frames) on a
    /// project. min_aa defaults to 75. When add_as_features is true the ORFs
    /// are appended as real CDS features (through the add-feature path, with
    /// recompute/broadcast) and {ok, message, projectId, regionView} is
    /// returned; otherwise returns {projectId, orfs: [Feature]}.
    #[tool]
    async fn find_orfs(
        &self,
        Parameters(request): Parameters<FindOrfsRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let orfs = crate::do_find_orfs(&self.pm, &id, request.min_aa)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;

        if !request.add_as_features.unwrap_or(false) {
            return Ok(Json(serde_json::json!({ "projectId": id, "orfs": orfs })));
        }
        if orfs.is_empty() {
            return Ok(Json(serde_json::json!({
                "ok": true,
                "message": "No ORFs found",
                "projectId": id,
            })));
        }
        let min_s = orfs.iter().map(|f| f.start).min().unwrap_or(0);
        let max_e = orfs.iter().map(|f| f.end).max().unwrap_or(0);
        let payload = crate::do_add_features(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            orfs,
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let region = self.digest_region(&id, Some((min_s, max_e))).await;
        Ok(Json(ok_envelope(&id, "Added ORFs as CDS features".to_string(), region)))
    }

    /// Design primer candidates — same modes/parameters as the
    /// design_primer_candidates command. mode: "amplify" | "oepcr" |
    /// "mutagenesis"; segments are {start, end} 0-based inclusive. Returns
    /// {projectId, groups: [PrimerGroup]}.
    #[tool]
    async fn design_primers(
        &self,
        Parameters(request): Parameters<DesignPrimersRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let seg = request.seg.map(|s| libregene_core::models::Segment {
            start: s.start,
            end: s.end,
            color: None,
        });
        let seg2 = request.seg2.map(|s| libregene_core::models::Segment {
            start: s.start,
            end: s.end,
            color: None,
        });
        let groups = crate::do_design_primer_candidates(
            &self.pm,
            &id,
            request.mode,
            seg,
            seg2,
            request.name,
            request.name1,
            request.name2,
            request.site_name,
            request.target_tm,
            request.overlap_len,
            request.arm_len,
            request.mut_seq,
            request.na_conc,
            request.mg_conc,
            request.dntp_conc,
            request.tris_conc,
            request.primer_conc,
        )
        .await
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "groups": groups })))
    }

    /// Predict PCR product pairs from the project's primers (fwd/rev binding
    /// sites) using compute_primer_pairs. Returns
    /// {projectId, pairs: [{fwdPrimerId, revPrimerId, fwdPosition,
    /// revPosition, productSize, ta}]}.
    #[tool]
    async fn analyze_pcr(
        &self,
        Parameters(request): Parameters<AnalyzePcrRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let (template, primers) = {
            let pm = self.pm.read().await;
            let project = pm
                .get_project_by_id(&id)
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
            (project.sequence.clone(), project.primers.clone())
        };
        let pairs = tokio::task::spawn_blocking(move || {
            libregene_core::primer::align::compute_primer_pairs(&template, &primers)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {e}"), None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "pairs": pairs })))
    }

    /// Check whether the given primers (each {name, type: "fwd"|"rev", seq})
    /// can bind to a project's sequence, without persisting them. Same engine
    /// as check_primers_binding. Returns {projectId, results: [{id, binds,
    /// site: {strand, templateStart, templateEnd, tm} | null}]}.
    #[tool]
    async fn check_primer_binding(
        &self,
        Parameters(request): Parameters<CheckPrimerBindingRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let primers: Vec<Primer> = request
            .primers
            .into_iter()
            .map(|p| Primer {
                id: p.name.clone(),
                name: p.name,
                r#type: p.r#type,
                primer_seq: p.seq,
                binding_sites: Vec::new(),
            })
            .collect();
        let payload = crate::do_check_primers_binding(&self.pm, &id, primers)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        Ok(Json(payload))
    }
}

// ---------------------------------------------------------------------------
// Server bootstrap + settings (start/stop/restart without app restart)
// ---------------------------------------------------------------------------

#[tool_handler(name = "LibreGene")]
impl<R: Runtime> ServerHandler for LibreGeneMcp<R> {}

/// Runtime MCP server configuration. The frontend persists the source of truth
/// in localStorage and pushes it here via `set_mcp_config` on startup and on
/// every settings change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McpConfig {
    pub enabled: bool,
    pub port: u16,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: MCP_PORT,
        }
    }
}

/// Owns the MCP server task. `set_config` stops/restarts the loopback server
/// in place so the settings toggle takes effect without an app restart.
pub struct McpServer<R: Runtime> {
    app_handle: AppHandle<R>,
    pm: Arc<RwLock<ProjectManager>>,
    wp: Arc<RwLock<HashMap<String, String>>>,
    config: Arc<StdMutex<McpConfig>>,
    task: Arc<StdMutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
}

impl<R: Runtime> Clone for McpServer<R> {
    fn clone(&self) -> Self {
        Self {
            app_handle: self.app_handle.clone(),
            pm: self.pm.clone(),
            wp: self.wp.clone(),
            config: self.config.clone(),
            task: self.task.clone(),
        }
    }
}

impl<R: Runtime> McpServer<R> {
    pub fn new(
        app_handle: AppHandle<R>,
        pm: Arc<RwLock<ProjectManager>>,
        wp: Arc<RwLock<HashMap<String, String>>>,
    ) -> Self {
        Self {
            app_handle,
            pm,
            wp,
            config: Arc::new(StdMutex::new(McpConfig::default())),
            task: Arc::new(StdMutex::new(None)),
        }
    }

    pub fn config(&self) -> McpConfig {
        *self.config.lock().unwrap()
    }

    /// Update the config and restart the server only when something changed.
    pub async fn set_config(&self, enabled: bool, port: u16) -> Result<McpConfig, String> {
        if !(1..=65535).contains(&port) {
            return Err(format!("Invalid port: {port} (must be 1-65535)"));
        }
        let changed = {
            let mut c = self.config.lock().unwrap();
            let changed = c.enabled != enabled || c.port != port;
            c.enabled = enabled;
            c.port = port;
            changed
        };
        if changed {
            self.apply().await;
        }
        Ok(self.config())
    }

    /// Reconcile the running server with the current config: stop any existing
    /// task, then start one if enabled.
    pub async fn apply(&self) {
        let cfg = self.config();
        let old = self.task.lock().unwrap().take();
        if let Some(handle) = old {
            handle.abort();
        }
        if cfg.enabled {
            let app = self.app_handle.clone();
            let pm = self.pm.clone();
            let wp = self.wp.clone();
            let port = cfg.port;
            let handle = tauri::async_runtime::spawn(async move {
                if let Err(e) = serve_mcp(app, pm, wp, port).await {
                    log::error!("MCP server error on port {}: {}", port, e);
                }
            });
            *self.task.lock().unwrap() = Some(handle);
        }
    }
}

async fn serve_mcp<R: Runtime>(
    app_handle: AppHandle<R>,
    pm: Arc<RwLock<ProjectManager>>,
    wp: Arc<RwLock<HashMap<String, String>>>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

    let service = StreamableHttpService::new(
        move || Ok(LibreGeneMcp::new(app_handle.clone(), pm.clone(), wp.clone())),
        Arc::new(session::local::LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new()
        .route("/mcp", axum::routing::any_service(service.clone()))
        .fallback_service(service);

    // Retry briefly on AddrInUse so a restart that races the previous
    // instance's socket release still binds.
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                log::info!("MCP server listening on http://{addr}/mcp");
                return axum::serve(listener, router).await.map_err(Into::into);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(e) => return Err(Box::new(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Raw HTTP POST /mcp initialize; true when the MCP handshake succeeds.
    async fn handshake_ok(port: u16) -> bool {
        let addr = format!("127.0.0.1:{}", port);
        let Ok(mut stream) = tokio::net::TcpStream::connect(&addr).await else {
            return false;
        };
        let body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"cfg-test\",\"version\":\"0\"}}}";
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        if stream.write_all(req.as_bytes()).await.is_err() {
            return false;
        }
        let mut buf = vec![0u8; 4096];
        match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => {
                let text = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                text.contains("200 ok") && text.contains("mcp-session-id")
            }
            _ => false,
        }
    }

    async fn wait_up(port: u16) -> bool {
        for _ in 0..40 {
            if handshake_ok(port).await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    fn test_server() -> McpServer<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        McpServer::new(
            app.handle().clone(),
            Arc::new(RwLock::new(ProjectManager::new())),
            Arc::new(RwLock::new(HashMap::new())),
        )
    }

    #[tokio::test]
    async fn config_defaults_to_enabled_on_mcp_port() {
        let server = test_server();
        let cfg = server.config();
        assert!(cfg.enabled);
        assert_eq!(cfg.port, MCP_PORT);
    }

    #[tokio::test]
    async fn set_config_rejects_bad_ports() {
        let server = test_server();
        assert!(server.set_config(true, 0).await.is_err());
        // rejected change must not alter the stored config
        assert_eq!(server.config().port, MCP_PORT);
    }

    #[tokio::test]
    async fn server_starts_stops_and_restarts_on_port_change() {
        let server = test_server();

        // start on a fresh port
        server.set_config(true, 19999).await.unwrap();
        assert!(wait_up(19999).await, "server should be up on 19999");

        // disable → port released
        server.set_config(false, 19999).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!handshake_ok(19999).await, "server should be down after disable");

        // re-enable on a new port without an app restart
        server.set_config(true, 20001).await.unwrap();
        assert!(wait_up(20001).await, "server should be up on 20001 after restart");
        assert!(!handshake_ok(19999).await, "old port must stay free");

        // unchanged config → no restart churn
        server.set_config(true, 20001).await.unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(handshake_ok(20001).await, "server must survive a no-op set_config");

        // clean up
        server.set_config(false, 20001).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!handshake_ok(20001).await);
    }
}
