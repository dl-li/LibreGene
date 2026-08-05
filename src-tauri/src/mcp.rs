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
use libregene_core::models::{Enzyme, Feature, Primer, ProjectData};
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
    /// Collapse the UNIQUE CUTTERS list into a single count line (default true;
    /// pass false for the full per-enzyme list).
    compact_cutters: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct RegionRequest {
    project_id: Option<String>,
    start: i64,
    end: i64,
    max_features: Option<usize>,
    feature_filter: Option<String>,
    /// Collapse the enzyme cut list into a count line (default true; pass
    /// false for the full list).
    compact: Option<bool>,
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
struct UpdateFeatureRequest {
    project_id: Option<String>,
    feature_id: String,
    name: Option<String>,
    ftype: Option<String>,
    color: Option<String>,
    /// ".", "+" or "-"
    strand: Option<String>,
    /// GenBank 1-based location string (e.g. "100..200", "complement(50..80)",
    /// "join(1..100,200..300)"); stored as 0-based inclusive.
    location: Option<String>,
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
struct SetMethylationRequest {
    project_id: Option<String>,
    systems: Vec<String>,
    overlap: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AddAlignmentRequest {
    project_id: Option<String>,
    name: String,
    #[serde(alias = "seq")]
    bases: Option<String>,
    path: Option<String>,
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

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct FindRestrictionSitesRequest {
    project_id: Option<String>,
    /// Enzyme names to report (case-insensitive); empty/omitted = all enzymes
    /// that have a recognition site on this sequence.
    enzymes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ListPrimersRequest {
    project_id: Option<String>,
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
    fwd_enzyme: Option<String>,
    rev_enzyme: Option<String>,
    protect_bases: Option<usize>,
    na_conc: Option<f64>,
    mg_conc: Option<f64>,
    dntp_conc: Option<f64>,
    tris_conc: Option<f64>,
    primer_conc: Option<f64>,
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

/// Stored feature coordinates rendered GenBank-style but **0-based inclusive**
/// (e.g. "99..199", "complement(49..79)", "join(0..99,199..299)").
fn stored_location(f: &Feature) -> String {
    let segs: Vec<(i64, i64)> = if f.segments.is_empty() {
        vec![(f.start, f.end)]
    } else {
        f.segments.iter().map(|s| (s.start, s.end)).collect()
    };
    let inner = segs
        .iter()
        .map(|(s, e)| format!("{}..{}", s, e))
        .collect::<Vec<_>>()
        .join(",");
    let loc = if segs.len() > 1 { format!("join({})", inner) } else { inner };
    if f.strand == "-" {
        format!("complement({})", loc)
    } else {
        loc
    }
}

/// Look up an enzyme's recognition site by name (case-insensitive); the error
/// lists near matches so the caller can fix the name.
fn resolve_enzyme_site(name: &str) -> Result<String, String> {
    let db = libregene_core::enzyme::search::get_db();
    if let Some(e) = db.enzymes.iter().find(|e| e.name.eq_ignore_ascii_case(name)) {
        return Ok(e.site.to_ascii_uppercase());
    }
    let q = name.to_lowercase();
    let suggestions: Vec<&str> = db
        .enzymes
        .iter()
        .map(|e| e.name.as_str())
        .filter(|n| n.to_lowercase().contains(&q))
        .take(5)
        .collect();
    if suggestions.is_empty() {
        Err(format!("Unknown enzyme '{}'; no similar names in the enzyme database", name))
    } else {
        Err(format!("Unknown enzyme '{}'; similar: {}", name, suggestions.join(", ")))
    }
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
    /// whole project when `None`. `compact` collapses the enzyme cut list into
    /// a count line (mutation tools use it to keep regionView small).
    async fn digest_region(
        &self,
        project_id: &str,
        region: Option<(i64, i64)>,
        compact: bool,
    ) -> Option<String> {
        let pm = self.pm.read().await;
        let project = pm.get_project_by_id(project_id)?;
        let opts = DigestOptions {
            compact_enzymes: compact,
            ..DigestOptions::default()
        };
        project_digest(project, &opts, region).ok()
    }

    /// Text digest of the region around a feature (looked up by id); compact
    /// enzyme rendering (only mutation tools call this).
    async fn digest_feature_region(&self, project_id: &str, feature_id: &str) -> Option<String> {
        let pm = self.pm.read().await;
        let project = pm.get_project_by_id(project_id)?;
        let f = project.features.iter().find(|f| f.id == feature_id)?;
        // Clamp the +/-5 context window with saturating arithmetic so a
        // feature near an end (or a maliciously huge coordinate that slipped
        // past validation) can't underflow/overflow and panic the process.
        let s = f.start.saturating_sub(5);
        let e = (f.end.saturating_add(5)).min(project.length.saturating_sub(1));
        let opts = DigestOptions {
            compact_enzymes: true,
            ..DigestOptions::default()
        };
        project_digest(project, &opts, Some((s, e))).ok()
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
    /// (case-insensitive substring) or exact ftype. Primers render as a PRIMERS
    /// section (or "PRIMERS (none)" when the project has none). The UNIQUE
    /// CUTTERS list (90+ lines on real plasmids) is collapsed to a single count
    /// line by default; pass `compactCutters: false` for the full per-enzyme
    /// list. Returns {projectId, text}.
    #[tool]
    async fn get_project_overview(
        &self,
        Parameters(request): Parameters<OverviewRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
            compact_enzymes: false,
            compact_cutters: request.compact_cutters.unwrap_or(true),
        };
        let text = project_digest(&project, &opts, None)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "text": text })))
    }

    /// Compact text digest of a region of a project. Coordinates are 0-based
    /// inclusive; on circular sequences start > end wraps the origin. Only
    /// features, primer binding sites and enzyme cut positions overlapping
    /// [start, end] are included. The enzyme cut list is collapsed into a
    /// single count line by default; pass `compact: false` for every cut in
    /// the window. Returns {projectId, text}.
    #[tool]
    async fn get_region_view(
        &self,
        Parameters(request): Parameters<RegionRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
            compact_enzymes: request.compact.unwrap_or(true),
            compact_cutters: false,
        };
        let text = project_digest(&project, &opts, Some((request.start, request.end)))
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "text": text })))
    }

    /// Read bases of a project's sequence. Returns {projectId, sequence, text}
    /// — `sequence` is the plain uppercase base string (machine-readable);
    /// `text` is the same window with a coordinate ruler (10 bp groups, 60 bp
    /// per line). Coordinates are 0-based inclusive; on circular sequences
    /// start > end wraps the origin. Windows larger than 10000 bp are rejected.
    #[tool]
    async fn read_sequence(
        &self,
        Parameters(request): Parameters<SequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let text = read_sequence(&project, request.start, request.end)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let bases = libregene_core::digest::read_sequence_bases(&project, request.start, request.end)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "sequence": bases, "text": text })))
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

    /// List restriction-enzyme recognition sites on a project's sequence.
    /// `enzymes` is an optional list of enzyme names (case-insensitive, names
    /// from get_enzyme_database); omit it (or pass []) to report every enzyme
    /// that has a site. Unknown names are rejected with near-match
    /// suggestions. Sites are the already-computed engine results the UI
    /// shows (circular-normalized, methylation-aware), so no recompute runs.
    /// Returns {projectId, enzymes: [{name, sites: [{recStart, recEnd,
    /// recSeq, strand, cuts: [{topCutIndex, botCutIndex}], methylationBlocked,
    /// unique}]}]}. recStart/recEnd are 0-based inclusive; a cut happens
    /// BETWEEN cut-1 and cut (0-based); strand is "top" or "bottom"
    /// (recognition orientation); unique = exactly one site for that enzyme.
    #[tool]
    async fn find_restriction_sites(
        &self,
        Parameters(request): Parameters<FindRestrictionSitesRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let wanted: Option<Vec<String>> = request.enzymes.map(|v| {
            v.into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        });
        // The engine only stores entries with at least one recognition site,
        // so validate requested names against those.
        let requested: Vec<String> = match &wanted {
            Some(list) if !list.is_empty() => {
                let names: Vec<&str> = project.enzymes.iter().map(|e| e.name.as_str()).collect();
                let mut resolved: Vec<String> = Vec::new();
                for n in list {
                    match names.iter().find(|a| a.eq_ignore_ascii_case(n)) {
                        Some(found) if !resolved.iter().any(|r| r.eq_ignore_ascii_case(found)) => {
                            resolved.push(found.to_string());
                        }
                        Some(_) => {}
                        None => {
                            let q = n.to_lowercase();
                            let sugg: Vec<&str> = names
                                .iter()
                                .copied()
                                .filter(|a| a.to_lowercase().contains(&q))
                                .take(5)
                                .collect();
                            let msg = if sugg.is_empty() {
                                format!(
                                    "Unknown enzyme '{}': no enzyme with a recognition site in this project has a similar name (names come from get_enzyme_database)",
                                    n
                                )
                            } else {
                                format!(
                                    "Unknown enzyme '{}'; enzymes cutting this sequence with similar names: {}",
                                    n,
                                    sugg.join(", ")
                                )
                            };
                            return Ok(Json(fail_envelope(&id, msg)));
                        }
                    }
                }
                resolved
            }
            _ => Vec::new(),
        };
        let mut by_name: HashMap<&str, Vec<&Enzyme>> = HashMap::new();
        for e in &project.enzymes {
            if requested.is_empty() || requested.iter().any(|w| w.eq_ignore_ascii_case(&e.name)) {
                by_name.entry(e.name.as_str()).or_default().push(e);
            }
        }
        let mut enzyme_names: Vec<&str> = by_name.keys().copied().collect();
        enzyme_names.sort();
        let enzymes_json: Vec<serde_json::Value> = enzyme_names
            .into_iter()
            .map(|n| {
                let mut sites = by_name[n].clone();
                sites.sort_by_key(|e| e.rec_start);
                serde_json::json!({
                    "name": n,
                    "sites": sites.iter().map(|e| serde_json::json!({
                        "recStart": e.rec_start,
                        "recEnd": e.rec_end,
                        "recSeq": e.rec_seq,
                        "strand": e.recognition_strand,
                        "cuts": e.cut_pairs.iter().map(|p| serde_json::json!({
                            "topCutIndex": p.top_cut_index,
                            "botCutIndex": p.bot_cut_index,
                        })).collect::<Vec<_>>(),
                        "methylationBlocked": e.methylation_blocked,
                        "unique": e.is_unique,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(Json(serde_json::json!({ "projectId": id, "enzymes": enzymes_json })))
    }

    /// List the primers stored in a project (read-only; never recomputes or
    /// checks binding). Returns {projectId, primers: [{id, name, type, seq,
    /// bindingSiteCount, sites: [{strand, templateStart, templateEnd}]}]}.
    /// templateStart is 0-based inclusive, templateEnd 0-based EXCLUSIVE
    /// (range spans templateStart..templateEnd-1). bindingSiteCount is the
    /// number of recomputed binding sites (0 when the primer does not bind);
    /// sites are best-first (Tm descending, as the UI orders them).
    #[tool]
    async fn list_primers(
        &self,
        Parameters(request): Parameters<ListPrimersRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let primers: Vec<serde_json::Value> = project
            .primers
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "type": p.r#type,
                    "seq": p.primer_seq,
                    "bindingSiteCount": p.binding_sites.len(),
                    "sites": p.binding_sites.iter().map(|s| serde_json::json!({
                        "strand": s.strand,
                        "templateStart": s.template_start,
                        "templateEnd": s.template_end,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(Json(serde_json::json!({ "projectId": id, "primers": primers })))
    }

    // -----------------------------------------------------------------------
    // Mutations
    // -----------------------------------------------------------------------

    /// Open a GenBank/FASTA file into the project manager (project id = file
    /// path). Enzyme and primer recompute run on a background thread; the UI is
    /// refreshed via broadcast. Returns {ok, message, projectId, regionView}
    /// where regionView is the compact overview digest of the opened project
    /// (enzyme cutters collapsed to a count line).
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
        let region = self.digest_region(&id, None, true).await;
        Ok(Json(ok_envelope(&id, summary, region)))
    }

    /// Save a project to a GenBank file on disk. Uses the same serializer and
    /// mark-clean logic as the save_file command. Returns the uniform envelope
    /// with the overview digest plus `bytesWritten` (file size in bytes, for
    /// write verification).
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
        let bytes_written = payload.get("bytesWritten").and_then(|v| v.as_u64());
        crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, None).await;
        let region = self.digest_region(&id, None, true).await;
        let mut env = ok_envelope(&id, format!("Saved {}", path), region);
        if let Some(b) = bytes_written {
            env["bytesWritten"] = serde_json::json!(b);
        }
        Ok(Json(env))
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
        let region = self.digest_region(&id, None, true).await;
        Ok(Json(ok_envelope(&id, format!("Activated {}", id), region)))
    }

    /// Replace sequence [start..end] (0-based inclusive) with `replacement`
    /// (empty = delete). A pure insertion is `end = start - 1`; ranges must not
    /// wrap (start > end+1 rejected). Feature coordinates are shifted/clipped
    /// for the edit (features fully inside a deleted range are removed). When
    /// `expected_old` is given it must match the current [start..end] content
    /// case-insensitively or the edit is rejected with the actual content. Uses
    /// the same primer+enzyme recompute path as update_sequence. Returns
    /// newLength, old/new region views, 30 bp sequence context on each side of
    /// the edit, and side-effect echo `removedFeatures`/`clippedFeatures`
    /// (both always present, empty arrays when none): removed lists features
    /// fully inside the deleted/replaced span ({name, ftype, location} with
    /// the pre-edit 0-based "start..end"); clipped lists features whose
    /// coordinates changed other than a pure translation ({name, ftype,
    /// before, after} as {start, end}).
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
                let exp = expected.as_bytes();
                let cur = current.as_bytes();
                let diff_at = exp
                    .iter()
                    .zip(cur.iter())
                    .position(|(a, b)| !a.eq_ignore_ascii_case(b))
                    .unwrap_or(exp.len().min(cur.len()));
                let ctx_lo = diff_at.saturating_sub(20);
                let exp_hi = (diff_at + 20).min(exp.len());
                let cur_hi = (diff_at + 20).min(cur.len());
                let mut v = fail_envelope(
                    &id,
                    format!(
                        "expected_old mismatch at index {} (within [{}..{}], 0-based): expected context '{}' vs current context '{}'",
                        diff_at,
                        start,
                        end,
                        String::from_utf8_lossy(&exp[ctx_lo..exp_hi]),
                        String::from_utf8_lossy(&cur[ctx_lo..cur_hi]),
                    ),
                );
                v["currentContent"] = serde_json::json!(current);
                v["mismatch"] = serde_json::json!({
                    "index": diff_at,
                    "expectedContext": String::from_utf8_lossy(&exp[ctx_lo..exp_hi]),
                    "currentContext": String::from_utf8_lossy(&cur[ctx_lo..cur_hi]),
                    "expectedLength": exp.len(),
                    "currentLength": cur.len(),
                });
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
        let old_opts = DigestOptions {
            compact_enzymes: true,
            ..DigestOptions::default()
        };
        let old_region = project_digest(&project, &old_opts, Some(old_win)).ok();

        let new_seq = format!(
            "{}{}{}",
            &project.sequence[..start as usize],
            request.replacement,
            &project.sequence[(end + 1) as usize..]
        );
        let new_len = new_seq.len() as i64;

        // Side effects on features, derived from the pre-edit list with the
        // same span math as the adjust below (no snapshot/compare needed).
        let impact = libregene_core::utils::features_edit_impact(
            &project.features,
            start,
            end,
            request.replacement.len() as i64,
        );

        // Shift/clip features for the edit before the sequence swap: the
        // update_sequence core never touches feature coordinates (the frontend
        // adjusts them client-side), so the MCP path must do it here.
        {
            let mut pm = self.pm.write().await;
            if let Some(p) = pm.get_project_mut_by_id(&id) {
                libregene_core::utils::adjust_features_for_edit(
                    &mut p.features,
                    start,
                    end,
                    request.replacement.len() as i64,
                );
            }
        }

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
        let new_region = self.digest_region(&id, Some(new_win), true).await;

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
            "removedFeatures": serde_json::to_value(&impact.removed_features)
                .unwrap_or_else(|_| serde_json::json!([])),
            "clippedFeatures": serde_json::to_value(&impact.clipped_features)
                .unwrap_or_else(|_| serde_json::json!([])),
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
    /// Returns {ok, message, projectId, featureId, regionView} around the new
    /// feature.
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

        // Reject coordinates outside [1, project.length]. parse_location_string
        // only checks start<=end (no upper bound), so without this a caller
        // could write a feature with end = i64::MAX and later panic downstream
        // code that slices the sequence by these coordinates.
        {
            let pm = self.pm.read().await;
            let plen = pm
                .get_project_by_id(&id)
                .map(|p| p.length)
                .unwrap_or(0);
            if start < 1 || end < 1 || end > plen {
                return Ok(Json(fail_envelope(
                    &id,
                    format!(
                        "feature location {}..{} is out of range for project length {}",
                        start, end, plen
                    ),
                )));
            }
        }

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

        let stored = stored_location(&feature);
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
        let mut v = ok_envelope(
            &id,
            format!("Added {} {} at {} (0-based; input location was 1-based {})", ftype, name, stored, location),
            region,
        );
        v["featureId"] = serde_json::json!(feature_id);
        Ok(Json(v))
    }

    /// Update a feature's attributes in one call. `feature_id` is required;
    /// give at least one of name/ftype/color/strand/location or the call is
    /// rejected. `location` is a GenBank 1-based location string (same formats
    /// as add_feature, e.g. "100..200", "complement(50..80)",
    /// "join(1..100,200..300)"); the stored coordinates are 0-based inclusive
    /// and echoed back as such. strand must be ".", "+" or "-"; color is hex
    /// (e.g. "#F87171") and also recolors existing segments. Returns
    /// {ok, message, projectId, regionView} around the feature.
    #[tool]
    async fn update_feature(
        &self,
        Parameters(request): Parameters<UpdateFeatureRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        if !self.feature_exists(&id, &request.feature_id).await {
            return Ok(Json(fail_envelope(&id, format!("Feature not found: {}", request.feature_id))));
        }
        if request.name.is_none()
            && request.ftype.is_none()
            && request.color.is_none()
            && request.strand.is_none()
            && request.location.is_none()
        {
            return Ok(Json(fail_envelope(
                &id,
                "Nothing to update: give at least one of name/ftype/color/strand/location".to_string(),
            )));
        }
        if let Some(s) = &request.strand {
            if !matches!(s.as_str(), "." | "+" | "-") {
                return Ok(Json(fail_envelope(&id, "Invalid strand: must be ., +, or -".to_string())));
            }
        }
        let feature_id = request.feature_id.clone();
        let location = request.location.clone();
        // Pre-validate a new location against the project length (same reason
        // as add_feature). parse_location_string has no upper bound on its own.
        if let Some(loc) = &location {
            let pm = self.pm.read().await;
            let plen = pm.get_project_by_id(&id).map(|p| p.length).unwrap_or(0);
            if let Some((_, start, end, _)) =
                libregene_core::file_io::gbk::parse_location_string(loc)
            {
                if start < 1 || end < 1 || end > plen {
                    return Ok(Json(fail_envelope(
                        &id,
                        format!(
                            "feature location {}..{} is out of range for project length {}",
                            start, end, plen
                        ),
                    )));
                }
            }
        }
        let payload = crate::do_update_feature(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            &feature_id,
            move |f| {
                if let Some(loc) = &location {
                    let parsed = libregene_core::file_io::gbk::parse_location_string(loc)
                        .ok_or_else(|| format!("Invalid location: {}", loc))?;
                    let (segments, start, end, strand) = parsed;
                    f.segments = segments;
                    f.start = start;
                    f.end = end;
                    f.strand = strand;
                }
                if let Some(v) = request.name {
                    f.name = v;
                }
                if let Some(v) = request.ftype {
                    f.ftype = v;
                }
                if let Some(v) = &request.color {
                    f.color = v.clone();
                    for seg in f.segments.iter_mut() {
                        seg.color = Some(v.clone());
                    }
                }
                if let Some(v) = request.strand {
                    f.strand = v;
                }
                Ok(())
            },
        )
        .await
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(Json(fail_envelope(&id, err)));
        }
        let message = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .and_then(|p| p.features.iter().find(|f| f.id == feature_id))
                .map(|f| {
                    format!(
                        "Updated feature {}: {} {} at {} (0-based), strand {}",
                        feature_id,
                        f.ftype,
                        f.name,
                        stored_location(f),
                        f.strand
                    )
                })
                .unwrap_or_else(|| format!("Updated feature {}", feature_id))
        };
        let region = self.digest_feature_region(&id, &feature_id).await;
        Ok(Json(ok_envelope(&id, message, region)))
    }

    /// Add a primer ("fwd" or "rev") and recompute its binding sites against
    /// the template. Returns {ok, message, projectId, bindingSites, regionView}
    /// — bindingSites: [{strand, templateStart, templateEnd, tm, annealLen}].
    /// templateStart is 0-based inclusive, templateEnd 0-based EXCLUSIVE (the
    /// bound range spans templateStart..templateEnd-1). annealLen is the number
    /// of contiguous 3'-end bases matching the template (the anneal core; a
    /// non-pairing 5' tail is excluded).
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
                                "annealLen": libregene_core::primer::align::anneal_len(
                                    &p.sequence, &p.topology, &pr.primer_seq, s,
                                ),
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
            Some(r) => self.digest_region(&id, Some(r), true).await,
            None => self.digest_region(&id, None, true).await,
        };
        let mut env = ok_envelope(
            &id,
            format!("Added primer {} ({} binding site(s))", name, sites.len()),
            region_view,
        );
        env["bindingSites"] = serde_json::json!(sites);
        Ok(Json(env))
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
        let region = self.digest_region(&id, None, true).await;
        Ok(Json(ok_envelope(&id, "Updated methylation systems".to_string(), region)))
    }

    /// Align a read against the project template and APPEND it as a new
    /// alignment (never overwrites existing ones; ids are aln-1, aln-2, ...).
    /// Provide exactly one of:
    /// - `bases`: the read sequence as a plain string (whitespace/non-ACGT
    ///   chars are stripped).
    /// - `path`: read the sequence from a file. Supported file types:
    ///   `.gbk`/`.gb`/`.genbank` (GenBank), `.dna` (SnapGene),
    ///   `.fa`/`.fasta` (FASTA / plain text sequence), `.ab1` (ABIF
    ///   chromatogram; the basecalled PBAS sequence is extracted).
    /// Giving neither or both is an error. A name is always required.
    ///
    /// Returns {ok, message, projectId, regionView, significant, identity,
    /// strand, segmentCount, alignedLength, mismatches, insertions,
    /// deletions, mismatchDetails, deletionDetails, insertionDetails,
    /// destroyedSites, name, alignmentId}.
    /// - `identity`: 0–1 fraction, full precision (not rounded).
    /// - `alignedLength`: template positions covered by the alignment (sum of
    ///   segment spans, bp).
    /// - `mismatches`/`insertions`/`deletions`: total base counts (identity
    ///   alone rounds away single mismatches).
    /// - `mismatchDetails`: [{pos, templateBase, readBase}] — one entry per
    ///   mismatched column; `pos` is the 0-based inclusive template position;
    ///   `readBase` is oriented to the template strand (already rev-comp'd
    ///   when strand is "-").
    /// - `deletionDetails`: [{pos, length, bases}] — consecutive deleted
    ///   template columns grouped into one entry; `pos` is the 0-based
    ///   inclusive template position of the first deleted base; entries
    ///   straddling the circular origin are merged.
    /// - `insertionDetails`: [{pos, bases, length}] — `pos` is the 0-based
    ///   template position before which the extra read bases were inserted
    ///   (between pos-1 and pos; on circular templates pos=0 means between
    ///   tlen-1 and 0).
    /// - `destroyedSites`: [{enzyme, recStart, recEnd, recSeq}] — restriction
    ///   sites (already-computed engine results, 0-based inclusive) whose span
    ///   intersects any difference: a mismatch position, a deletion interval,
    ///   or an insertion point (pos-1 or pos inside the site). Always present
    ///   (empty array when nothing is destroyed), sorted by recStart.
    /// On failure returns {ok: false, message, projectId, significant: false};
    /// a message starting with "No significant alignment found" states the
    /// reason (identity below the 0.60 minimum, or aligned span below the
    /// 50 bp minimum).
    #[tool]
    async fn add_alignment(
        &self,
        Parameters(request): Parameters<AddAlignmentRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let name = request.name.clone();

        let seq = match (request.bases, request.path) {
            (Some(_), Some(_)) => {
                return Ok(Json(fail_envelope(
                    &id,
                    "Provide exactly one of `bases` or `path`, not both".to_string(),
                )));
            }
            (None, None) => {
                return Ok(Json(fail_envelope(
                    &id,
                    "Provide exactly one of `bases` (sequence string) or `path` (sequence file)".to_string(),
                )));
            }
            (Some(bases), None) => bases,
            (None, Some(path)) => {
                crate::validate_user_path(&path, crate::SEQ_EXTS).map_err(|e| {
                    ErrorData::internal_error(format!("invalid path: {}", e), None)
                })?;
                let parsed = tokio::task::spawn_blocking(move || {
                    libregene_core::file_io::parse_file(std::path::Path::new(&path))
                })
                .await
                .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
                match parsed {
                    Ok(data) => data.sequence,
                    Err(e) => {
                        return Ok(Json(fail_envelope(
                            &id,
                            format!(
                                "Failed to read alignment sequence file (supported: .gbk/.gb/.genbank, .dna, .fa/.fasta, .ab1): {}",
                                e
                            ),
                        )));
                    }
                }
            }
        };

        let payload = match crate::do_add_alignment_seq(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            request.name,
            seq,
        )
        .await
        {
            Ok(p) => p,
            Err(e) if e.starts_with("No significant alignment found") => {
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
                .and_then(|p| {
                    let a = p.alignments.last()?;
                    let diff = libregene_core::align::alignment_diff(a, &p.sequence);
                    let destroyed = libregene_core::align::destroyed_enzyme_sites(&p.enzymes, &diff);
                    Some((
                        serde_json::json!({
                            "identity": a.identity,
                            "strand": a.strand,
                            "segmentCount": a.segments.len(),
                            "alignedLength": diff.aligned_length,
                            "mismatches": diff.mismatches.len(),
                            "insertions": diff.insertions.iter().map(|i| i.length).sum::<usize>(),
                            "deletions": diff.deletions.iter().map(|d| d.length).sum::<usize>(),
                            "mismatchDetails": diff.mismatches,
                            "deletionDetails": diff.deletions,
                            "insertionDetails": diff.insertions,
                            "destroyedSites": destroyed,
                            "name": a.name,
                            "alignmentId": a.id,
                        }),
                        a.segments.first().map(|s| (s.start as i64, s.end as i64)),
                    ))
                })
        };
        let region = match summary {
            Some((_, Some((s, e)))) => self.digest_region(&id, Some((s, e)), true).await,
            _ => self.digest_region(&id, None, true).await,
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

    /// Remove an alignment by id (alignment ids are listed in the digest
    /// ALIGNMENTS section, e.g. "aln-1"). Returns the uniform
    /// {ok, message, projectId, regionView} envelope; regionView covers the
    /// removed alignment's first segment.
    #[tool]
    async fn remove_alignment(
        &self,
        Parameters(request): Parameters<RemoveAlignmentRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let alignment_id = request.alignment_id.clone();
        let region = {
            let pm = self.pm.read().await;
            let p = pm
                .get_project_by_id(&id)
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
            match p.alignments.iter().find(|a| a.id == alignment_id) {
                Some(a) => a.segments.first().map(|s| (s.start as i64, s.end as i64)),
                None => {
                    return Ok(Json(fail_envelope(
                        &id,
                        format!("Alignment not found: {}", alignment_id),
                    )));
                }
            }
        };
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
        let region_view = self.digest_region(&id, region, true).await;
        Ok(Json(ok_envelope(
            &id,
            format!("Removed alignment {}", alignment_id),
            region_view,
        )))
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
        let region = self.digest_region(&id, Some((min_s, max_e)), true).await;
        Ok(Json(ok_envelope(&id, "Added ORFs as CDS features".to_string(), region)))
    }

    /// Design primer candidates — same modes/parameters as the
    /// design_primer_candidates command. mode: "amplify" | "oepcr" |
    /// "mutagenesis"; segments are {start, end} 0-based inclusive.
    /// amplify: optional `fwd_enzyme`/`rev_enzyme` (enzyme names from
    /// get_enzyme_database, e.g. "BamHI") add a 5' tail of
    /// `protect_bases` (default 3) GC protection bases + the recognition
    /// site; candidates expose tail/tailLen/annealLen and Tm covers the
    /// anneal core only.
    /// mutagenesis: `mut_seq` is the desired PLUS-strand content of `seg`
    /// after the edit; it must be the same length as `seg` and differ at
    /// <= 3 bases or the call fails with the current template sequence.
    /// The response includes a `mutation` self-check block (diffs, plus/minus
    /// strand context, and CDS codon/amino-acid change when `seg` lies inside
    /// a CDS — joined multi-segment CDS features are supported — mind the CDS
    /// strand: for a minus-strand CDS the coding change is the reverse
    /// complement of the plus-strand edit). In that block `cds.codonIndex`
    /// is 0-based within the CDS and `cds.aaPosition1Based` is the 1-based
    /// amino-acid position (codonIndex + 1); `cds.aaPositionExcludingMet` is
    /// aaPosition1Based minus the initiator Met (absent for the first codon).
    /// Replacing every base of `seg`
    /// adds a `warning` (likely wrong strand/location) but is not rejected.
    /// In amplify mode the response always includes an `internalSites` array
    /// (empty when no enzyme recognition site occurs inside the amplified
    /// segment; non-empty entries {enzyme, start, end, strand}, 0-based
    /// inclusive, plus a `warning` that digestion would cut the product).
    /// Returns {projectId, groups: [PrimerGroup], mutation?,
    /// internalSites (amplify)}.
    ///
    /// Tm/annealLen here describe the DESIGNED anneal core only. If you then
    /// verify a designed primer with check_primer_binding, its annealLen/Tm
    /// can be HIGHER: check recomputes the actual contiguous 3'-end match,
    /// which can extend into tail bases that happen to match the template
    /// (e.g. an enzyme tail sitting next to a matching downstream site).
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

        let mut fwd_tail = None;
        let mut rev_tail = None;
        let mut enzyme_sites: Vec<(String, String)> = Vec::new();
        if request.mode == "amplify" && (request.fwd_enzyme.is_some() || request.rev_enzyme.is_some())
        {
            let protect = libregene_core::primer::design::protect_sequence(
                request.protect_bases.unwrap_or(3),
            );
            for (enzyme, slot) in [
                (&request.fwd_enzyme, &mut fwd_tail),
                (&request.rev_enzyme, &mut rev_tail),
            ] {
                if let Some(name) = enzyme {
                    match resolve_enzyme_site(name) {
                        Ok(site) => {
                            *slot = Some(format!("{}{}", protect, site));
                            enzyme_sites.push((name.clone(), site));
                        }
                        Err(e) => return Ok(Json(fail_envelope(&id, e))),
                    }
                }
            }
        }

        // amplify + enzyme tails: warn when the recognition site also occurs
        // INSIDE the amplified segment (digestion would cut the product).
        let mut internal_sites: Vec<serde_json::Value> = Vec::new();
        if request.mode == "amplify" && !enzyme_sites.is_empty() {
            if let Some(seg_ref) = seg.as_ref() {
                let (sequence, topology) = {
                    let pm = self.pm.read().await;
                    let p = pm
                        .get_project_by_id(&id)
                        .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
                    (p.sequence.clone(), p.topology.clone())
                };
                let len = sequence.len() as i64;
                let (s, e) = (seg_ref.start, seg_ref.end);
                let amplicon: Option<String> = if s >= 0 && e < len && s <= e {
                    Some(sequence[s as usize..=e as usize].to_string())
                } else if topology == "circular" && s >= 0 && e < len {
                    Some(format!("{}{}", &sequence[s as usize..], &sequence[..=e as usize]))
                } else {
                    None
                };
                if let Some(amplicon) = amplicon {
                    for (enzyme_name, site) in &enzyme_sites {
                        for m in libregene_core::search::find_seq_matches(&amplicon, site) {
                            internal_sites.push(serde_json::json!({
                                "enzyme": enzyme_name,
                                "start": (s + m.start) % len,
                                "end": (s + m.end) % len,
                                "strand": m.strand,
                            }));
                        }
                    }
                }
            }
        }

        let mut mutation_info = None;
        if request.mode == "mutagenesis" {
            let (sequence, features) = {
                let pm = self.pm.read().await;
                let p = pm
                    .get_project_by_id(&id)
                    .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
                (p.sequence.clone(), p.features.clone())
            };
            let seg_ref = seg.as_ref().ok_or_else(|| {
                ErrorData::invalid_params("seg required for mutagenesis", None)
            })?;
            match libregene_core::primer::design::analyze_mutagenesis(
                &sequence,
                seg_ref,
                request.mut_seq.as_deref().unwrap_or(""),
                &features,
            ) {
                Ok(info) => mutation_info = Some(serde_json::to_value(info).unwrap_or_default()),
                Err(e) => {
                    let mut v = fail_envelope(&id, e);
                    let lo = seg_ref.start.max(0) as usize;
                    let hi = ((seg_ref.end + 1).min(sequence.len() as i64)) as usize;
                    if lo < hi {
                        v["templateBases"] =
                            serde_json::json!(sequence[lo..hi].to_ascii_uppercase());
                    }
                    return Ok(Json(v));
                }
            }
        }

        let mode_is_amplify = request.mode == "amplify";
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
            fwd_tail,
            rev_tail,
            request.na_conc,
            request.mg_conc,
            request.dntp_conc,
            request.tris_conc,
            request.primer_conc,
        )
        .await
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut v = serde_json::json!({ "projectId": id, "groups": groups });
        if let Some(info) = mutation_info {
            v["mutation"] = info;
        }
        if mode_is_amplify {
            v["internalSites"] = serde_json::json!(internal_sites);
            if !internal_sites.is_empty() {
                v["warning"] = serde_json::json!(
                    "The enzyme recognition site occurs inside the amplified segment; digestion will cut the product"
                );
            }
        }
        Ok(Json(v))
    }

    /// Check whether the given primers (each {name, type: "fwd"|"rev", seq})
    /// can bind to a project's sequence, without persisting them. Same engine
    /// as check_primers_binding. Returns {projectId, tmBasis, results: [{id,
    /// binds, site: {strand, templateStart, templateEnd, tm, annealLen,
    /// mismatchedTail} | null}]}. templateStart is 0-based inclusive,
    /// templateEnd 0-based EXCLUSIVE (range spans templateStart..templateEnd-1);
    /// cuts are not involved. `binds: true` means the 3' anneal core matched —
    /// the primer may still carry mismatches at its 5' end. `mismatchedTail`
    /// is the number of 5'-most bases NOT part of the contiguous 3' match
    /// (0 when the whole primer anneals; >0 for mutagenesis primers and
    /// enzyme-tail primers). `annealLen` counts only the contiguous 3' match.
    /// `tmBasis` (always present) states the Tm/annealLen basis: they reflect
    /// the ACTUAL contiguous 3' match, so tail bases that happen to match the
    /// template extend annealLen and raise tm beyond design_primers' values.
    /// Unlike design_primers (which reports the DESIGNED anneal core), this
    /// recomputes the actual contiguous 3'-end match: tail bases that happen
    /// to match the template (e.g. an enzyme tail next to a matching
    /// downstream site) extend annealLen and raise tm beyond design's values.
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
        let mut v = payload;
        v["projectId"] = serde_json::json!(id);
        v["tmBasis"] = serde_json::json!(
            "3' continuous match; tail bases that accidentally match the template are included in annealLen/Tm"
        );
        Ok(Json(v))
    }
}

// ---------------------------------------------------------------------------
// Server bootstrap + settings (start/stop/restart without app restart)
// ---------------------------------------------------------------------------

#[tool_handler(name = "LibreGene")]
impl<R: Runtime> ServerHandler for LibreGeneMcp<R> {
    // Tools return Json<serde_json::Value>, so the generated outputSchema has
    // no top-level "type". The MCP spec requires outputSchema.type == "object";
    // strict clients (e.g. kimi-code) reject the list otherwise.
    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        let mut tools = Self::tool_router().list_all();
        for tool in &mut tools {
            if let Some(schema) = &mut tool.output_schema {
                let patched = Arc::make_mut(schema);
                patched
                    .entry("type")
                    .or_insert_with(|| serde_json::Value::String("object".into()));
            }
        }
        Ok(rmcp::model::ListToolsResult {
            result_type: Some(rmcp::model::ResultType::COMPLETE),
            tools,
            meta: None,
            next_cursor: None,
            ttl_ms: None,
            cache_scope: None,
        })
    }
}

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
    /// Random bearer token generated at startup; required on every MCP
    /// request so that other local processes (or a browser via DNS
    /// rebinding) can't drive the MCP tools. Exposed to the trusted frontend
    /// via `get_mcp_token`.
    auth_token: Arc<String>,
}

impl<R: Runtime> Clone for McpServer<R> {
    fn clone(&self) -> Self {
        Self {
            app_handle: self.app_handle.clone(),
            pm: self.pm.clone(),
            wp: self.wp.clone(),
            config: self.config.clone(),
            task: self.task.clone(),
            auth_token: self.auth_token.clone(),
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
            auth_token: Arc::new(generate_auth_token()),
        }
    }

    /// The bearer token the trusted frontend must send to use the MCP server.
    pub fn auth_token(&self) -> &str {
        &self.auth_token
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
            let token = self.auth_token.clone();
            let handle = tauri::async_runtime::spawn(async move {
                if let Err(e) = serve_mcp(app, pm, wp, port, token).await {
                    log::error!("MCP server error on port {}: {}", port, e);
                }
            });
            *self.task.lock().unwrap() = Some(handle);
        }
    }
}

/// Generate a 32-byte random bearer token, hex-encoded (64 chars).
fn generate_auth_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Mix in process id + a high-resolution counter for uniqueness without
    // pulling a crypto crate. This is a local-only shared secret (the threat
    // is other local processes / browser rebinding, not a remote attacker who
    // can guess 64 hex chars); randomness quality matters less than presence.
    let mut buf = [0u8; 32];
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (std::process::id() as u64);
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    for b in buf.iter_mut() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *b = (s >> 33) as u8;
    }
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

async fn serve_mcp<R: Runtime>(
    app_handle: AppHandle<R>,
    pm: Arc<RwLock<ProjectManager>>,
    wp: Arc<RwLock<HashMap<String, String>>>,
    port: u16,
    auth_token: Arc<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let expected_host = format!("127.0.0.1:{}", port);

    let service = StreamableHttpService::new(
        move || Ok(LibreGeneMcp::new(app_handle.clone(), pm.clone(), wp.clone())),
        Arc::new(session::local::LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    // Auth middleware: require a local bearer token AND a matching Host header.
    // The token prevents other local processes (or a browser page via DNS
    // rebinding) from driving the MCP tools; the Host check blocks
    // cross-origin/rebinding requests that don't target 127.0.0.1:<port>.
    let auth_layer = axum::middleware::from_fn(move |req: axum::extract::Request, next: axum::middleware::Next| {
        let token = auth_token.clone();
        let host_ok = req
            .headers()
            .get(axum::http::header::HOST)
            .and_then(|h| h.to_str().ok())
            .map(|h| h == expected_host.as_str())
            .unwrap_or(false);
        let bearer_ok = req
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .map(|h| h.strip_prefix("Bearer ").map(|t| t == token.as_str()).unwrap_or(false))
            .unwrap_or(false);
        async move {
            if host_ok && bearer_ok {
                Ok(next.run(req).await)
            } else {
                Err(axum::http::StatusCode::UNAUTHORIZED)
            }
        }
    });

    let router = axum::Router::new()
        .route("/mcp", axum::routing::any_service(service.clone()))
        .fallback_service(service)
        .layer(auth_layer);

    // Retry briefly on AddrInUse so a restart that races the previous
    // instance's socket release still binds.
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                log::info!("MCP server listening on http://{addr}/mcp (auth enabled)");
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
