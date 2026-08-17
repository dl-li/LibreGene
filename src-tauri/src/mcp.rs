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
use std::time::Duration;

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
use tauri::{AppHandle, Manager, Runtime};

use libregene_core::digest::{DigestOptions, project_digest, read_sequence};
use libregene_core::models::{Enzyme, Feature, Primer, PrimerBindingSite, ProjectData, Segment};
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
    /// Replacement sequence as a plain string (empty = delete). Exactly one
    /// of `replacement` / `replacement_path` must be given.
    replacement: Option<String>,
    /// Read the replacement sequence from a local file instead of a string
    /// (.gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 etc., same formats as
    /// open_file) — the recommended way to hand a long insert to this tool.
    replacement_path: Option<String>,
    expected_old: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct OptimizeCdsRequest {
    /// Project mode: optimize a CDS/mRNA feature inside an open project.
    project_id: Option<String>,
    /// Feature id (project mode: required; input_path mode: optional — pick
    /// the file's CDS/mRNA feature with this id, otherwise the whole file
    /// sequence is treated as the coding sequence).
    feature_id: Option<String>,
    /// Standalone mode: raw DNA coding sequence text (whitespace/digits
    /// ignored, ACGT only, length divisible by 3; a trailing stop codon is
    /// fine).
    sequence: Option<String>,
    /// Standalone mode: local sequence file (.gbk/.gb/.genbank/.dna/.rna/
    /// .fasta/.fa/.fna/.ab1 — DNA) or protein file (.gpt/.prot — reverse
    /// translation).
    input_path: Option<String>,
    /// Optional: write the result to a file. .gbk/.gb/.genbank → DNA GenBank
    /// with the optimized CDS annotated; .gpt → protein GenBank of the
    /// translated sequence.
    output_path: Option<String>,
    /// Species key from list_species (e.g. "e_coli", "h_sapiens").
    species: String,
    /// use_best_codon (default) | match_codon_usage | harmonize_rca.
    method: Option<String>,
    /// Source table for harmonize_rca; falls back to match_codon_usage when absent.
    original_species: Option<String>,
    /// Restriction-site recognition sequences to avoid (IUPAC codes allowed).
    avoid_enzyme_sites: Option<Vec<String>>,
    /// false = read-only preview; true = replace the sequence in the project.
    /// Only meaningful in project mode (in sequence/input_path mode pass
    /// `output_path` instead).
    apply: Option<bool>,
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
struct AddAlignmentRequest {
    project_id: Option<String>,
    name: String,
    #[serde(alias = "seq")]
    bases: Option<String>,
    path: Option<String>,
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

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ExportSubsequenceRequest {
    /// Project to export from (defaults to the active project).
    project_id: Option<String>,
    /// Required: output file path (.gbk/.gb/.genbank for DNA/RNA projects,
    /// .gpt for protein projects).
    output_path: String,
    /// Region mode: start of the export window, 0-based inclusive.
    start: Option<i64>,
    /// Region mode: end of the export window, 0-based inclusive (start > end
    /// wraps the origin on circular sequences).
    end: Option<i64>,
    /// Feature mode: export this feature's sequence (segments joined 5'→3',
    /// reverse-complemented for minus-strand features).
    feature_id: Option<String>,
    /// Fragment mode (enzyme names): first enzyme; its first recognition
    /// site's top-strand cut starts the fragment.
    enzyme1: Option<String>,
    /// Fragment mode (enzyme names): second enzyme (may equal `enzyme1` to
    /// use that enzyme's first two sites).
    enzyme2: Option<String>,
    /// Fragment mode (explicit cuts): first cut index, 0-based (a cut at
    /// index C severs the DNA between C-1 and C).
    cut1: Option<i64>,
    /// Fragment mode (explicit cuts): second cut index (same convention).
    cut2: Option<i64>,
    /// Amplicon mode: fwd primer (project primer name or raw sequence).
    fwd_primer: Option<String>,
    /// Amplicon mode: rev primer (project primer name or raw sequence).
    rev_primer: Option<String>,
}

// ---------------------------------------------------------------------------
// optimize_cds input resolution (project / raw sequence / file)
// ---------------------------------------------------------------------------

/// One of the three mutually exclusive input modes of `optimize_cds`.
enum OptimizeInput {
    /// Open-project mode: `feature_id` names the CDS/mRNA feature to optimize.
    Project {
        project_id: Option<String>,
        feature_id: String,
    },
    /// Raw DNA coding sequence text.
    Sequence(String),
    /// Local sequence/protein file (`file_io::parse_file`).
    File {
        path: String,
        feature_id: Option<String>,
    },
}

/// Validate the input-mode combination and resolve it to exactly one
/// [`OptimizeInput`]. Error messages name the offending combination.
fn resolve_optimize_input(
    project_id: Option<&str>,
    feature_id: Option<&str>,
    sequence: Option<&str>,
    input_path: Option<&str>,
) -> Result<OptimizeInput, String> {
    match (sequence, input_path) {
        (Some(_), Some(_)) => Err(
            "provide exactly one input: `project_id` (+`feature_id`), `sequence`, or `input_path` — not both `sequence` and `input_path`"
                .to_string(),
        ),
        (Some(seq), None) => {
            if project_id.is_some() {
                return Err(
                    "`project_id` cannot be combined with `sequence`; use exactly one input mode"
                        .to_string(),
                );
            }
            if feature_id.is_some() {
                return Err(
                    "`feature_id` is only valid with `project_id` (project mode) or an `input_path` file that has features"
                        .to_string(),
                );
            }
            Ok(OptimizeInput::Sequence(seq.to_string()))
        }
        (None, Some(path)) => {
            if project_id.is_some() {
                return Err(
                    "`project_id` cannot be combined with `input_path`; use exactly one input mode"
                        .to_string(),
                );
            }
            Ok(OptimizeInput::File {
                path: path.to_string(),
                feature_id: feature_id.map(str::to_string),
            })
        }
        (None, None) => {
            let feature_id = feature_id.ok_or_else(|| {
                "`feature_id` is required in project mode (or pass `sequence` or `input_path` for standalone input)"
                    .to_string()
            })?;
            Ok(OptimizeInput::Project {
                project_id: project_id.map(str::to_string),
                feature_id: feature_id.to_string(),
            })
        }
    }
}

/// Strip whitespace/digits, uppercase, and require a valid DNA coding
/// sequence: only A/C/G/T and a length divisible by 3 (a trailing stop codon
/// is fine — it is just another codon).
fn clean_coding_sequence(seq: &str) -> Result<String, String> {
    let cleaned: String = seq
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_ascii_digit())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.is_empty() {
        return Err("sequence is empty".to_string());
    }
    for (i, c) in cleaned.char_indices() {
        if c != 'A' && c != 'C' && c != 'G' && c != 'T' {
            return Err(format!(
                "invalid base '{}' at position {} in sequence (expected A/C/G/T)",
                c, i
            ));
        }
    }
    if cleaned.len() % 3 != 0 {
        return Err(format!(
            "sequence length {} not divisible by 3 (expected a complete coding sequence)",
            cleaned.len()
        ));
    }
    Ok(cleaned)
}

/// The shared optimize_cds preview fields (identical across all input modes).
fn codon_preview_json(
    result: &libregene_core::codon::OptimizeResult,
    aa: &str,
    codon_count: usize,
    method: &str,
    species: &str,
) -> serde_json::Value {
    serde_json::json!({
        "aa": aa,
        "codonCount": codon_count,
        "newCodons": result.new_codons,
        "caiBefore": result.cai_before,
        "caiAfter": result.cai_after,
        "gcBefore": result.gc_before,
        "gcAfter": result.gc_after,
        "repairs": result.repairs,
        "repairCount": result.repairs.len(),
        "unresolved": result.unresolved,
        "method": method,
        "species": species,
    })
}

/// Write an optimization result to `output_path`. The extension decides the
/// format: .gbk/.gb/.genbank → DNA GenBank with the optimized CDS annotated
/// (`source` replaces the default minimal project when the input file already
/// carried features), .gpt → protein GenBank of the translated sequence.
/// Returns the written path.
fn write_optimization_output(
    output_path: &str,
    dna: Option<&str>,
    aa: &str,
    source: Option<&ProjectData>,
) -> Result<String, String> {
    let ext = crate::validate_user_path(output_path, crate::CODON_OUTPUT_EXTS)?;
    let path = std::path::Path::new(output_path);
    match ext.as_str() {
        "gbk" | "gb" | "genbank" => {
            let project = match source {
                Some(p) => p.clone(),
                None => {
                    let dna = dna
                        .ok_or_else(|| "no DNA sequence available for GenBank output".to_string())?;
                    minimal_dna_project(output_path, dna)
                }
            };
            libregene_core::file_io::gbk::write_gbk(&project, path)
                .map_err(|e| format!("failed to write {}: {}", output_path, e))?;
        }
        "gpt" => {
            let project = minimal_protein_project(output_path, aa);
            libregene_core::file_io::gpt::write_gpt(&project, path)
                .map_err(|e| format!("failed to write {}: {}", output_path, e))?;
        }
        other => {
            return Err(format!(
                "unsupported output extension '.{}' (allowed: gbk, gb, genbank, gpt)",
                other
            ))
        }
    }
    Ok(output_path.to_string())
}

fn minimal_dna_project(output_path: &str, dna: &str) -> ProjectData {
    let name = output_project_name(output_path);
    let len = dna.len() as i64;
    ProjectData {
        name,
        sequence: dna.to_string(),
        length: len,
        topology: "linear".to_string(),
        molecule_type: "dna".to_string(),
        features: vec![whole_cds_feature(len)],
        ..Default::default()
    }
}

fn minimal_protein_project(output_path: &str, aa: &str) -> ProjectData {
    let name = output_project_name(output_path);
    let len = aa.len() as i64;
    ProjectData {
        name,
        sequence: aa.to_string(),
        length: len,
        topology: "linear".to_string(),
        molecule_type: "protein".to_string(),
        features: vec![whole_cds_feature(len)],
        ..Default::default()
    }
}

fn output_project_name(output_path: &str) -> String {
    std::path::Path::new(output_path)
        .file_stem()
        .map(|s| s.to_string_lossy().replace(' ', "_"))
        .unwrap_or_else(|| "optimized".to_string())
}

fn whole_cds_feature(len: i64) -> Feature {
    Feature {
        id: "cds".to_string(),
        name: "CDS".to_string(),
        start: 0,
        end: len - 1,
        color: "#60A5FA".to_string(),
        ftype: "CDS".to_string(),
        segments: Vec::new(),
        strand: "+".to_string(),
        notes: String::new(),
        translation: String::new(),
        qualifiers: Vec::new(),
    }
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
        let unit = match p.molecule_type.as_str() {
            "rna" => "nt",
            "protein" => "aa",
            _ => "bp",
        };
        Some(format!("{}: {} {} {}", p.name, p.length, unit, p.topology))
    }

    /// Resolve the project id and reject non-DNA projects for DNA-only tools.
    async fn require_dna_project(&self, project_id: Option<String>) -> Result<String, ErrorData> {
        let (id, project) = self.resolve_project(project_id).await?;
        if !project.is_dna() {
            return Err(ErrorData::invalid_params(
                format!(
                    "This tool only supports DNA projects; project '{}' is a {} project",
                    id, project.molecule_type
                ),
                None,
            ));
        }
        Ok(id)
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

    /// Standalone `sequence` input for optimize_cds: clean + validate the DNA
    /// coding sequence, optimize, optionally write the result to a file.
    async fn optimize_sequence_input(
        &self,
        sequence: String,
        species: String,
        method: String,
        original_species: Option<String>,
        avoid_enzyme_sites: Option<Vec<String>>,
        output_path: Option<String>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let cleaned = clean_coding_sequence(&sequence)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let codons: Vec<String> = cleaned
            .as_bytes()
            .chunks(3)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        let codon_count = codons.len();
        let sp = species.clone();
        let m = method.clone();
        let os = original_species.clone();
        let aes = avoid_enzyme_sites.clone();
        let (result, aa) = tokio::task::spawn_blocking(move || -> Result<_, String> {
            let table = crate::codon_usage_table(&sp, None)?;
            let opts = crate::codon_optimize_options(&m, os.as_deref(), aes, None)?;
            let result = libregene_core::codon::optimize_codons(&codons, &table, &opts);
            let aa: String = codons
                .iter()
                .map(|c| table.aa_of.get(c).copied().unwrap_or('?'))
                .collect();
            Ok((result, aa))
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
        .map_err(|e| ErrorData::invalid_params(e, None))?;

        let optimized: String = result.new_codons.concat();
        let mut v = codon_preview_json(&result, &aa, codon_count, &method, &species);
        v["ok"] = serde_json::json!(true);
        v["optimizedSequence"] = serde_json::json!(optimized);
        v["message"] = serde_json::json!(format!(
            "Codon optimization (sequence input, {}): CAI {:.3} → {:.3}, GC {:.1}% → {:.1}%, {} repairs, {} unresolved",
            species,
            result.cai_before,
            result.cai_after,
            result.gc_before * 100.0,
            result.gc_after * 100.0,
            result.repairs.len(),
            result.unresolved.len(),
        ));
        if let Some(op) = output_path {
            let written = write_optimization_output(&op, Some(&optimized), &aa, None)
                .map_err(|e| ErrorData::invalid_params(e, None))?;
            v["outputPath"] = serde_json::json!(written);
        }
        Ok(Json(v))
    }

    /// Standalone `input_path` input for optimize_cds: parse the file, run the
    /// optimizer (feature CDS, whole sequence, or protein reverse translation),
    /// optionally write the result to a file.
    async fn optimize_file_input(
        &self,
        path: String,
        feature_id: Option<String>,
        species: String,
        method: String,
        original_species: Option<String>,
        avoid_enzyme_sites: Option<Vec<String>>,
        output_path: Option<String>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        crate::validate_user_path(&path, crate::SEQ_EXTS)
            .map_err(|e| ErrorData::invalid_params(format!("invalid input_path: {}", e), None))?;

        let sp = species.clone();
        let m = method.clone();
        let os = original_species.clone();
        let aes = avoid_enzyme_sites.clone();
        let fid = feature_id.clone();
        let p = path.clone();
        let (outcome, result, codon_count, aa, message) = tokio::task::spawn_blocking(
            move || -> Result<
                (
                    FileOutcome,
                    libregene_core::codon::OptimizeResult,
                    usize,
                    String,
                    String,
                ),
                String,
            > {
            let project = libregene_core::file_io::parse_file(std::path::Path::new(&p)).map_err(
                |e| {
                    format!(
                        "failed to read {} (supported: .gbk/.gb/.genbank, .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1): {}",
                        p, e
                    )
                },
            )?;
            if project.molecule_type == "protein" {
                // Reverse translation: aa file → optimized DNA coding sequence.
                let aa = project.sequence.to_ascii_uppercase();
                let table = crate::codon_usage_table(&sp, None)?;
                let opts = crate::codon_optimize_options(&m, os.as_deref(), aes, None)?;
                let result = libregene_core::codon::optimize_from_aa(&aa, &table, &opts)?;
                let dna: String = result.new_codons.concat();
                let message = format!(
                    "Reverse translation (protein file input, {}): {} aa → {} bp DNA, {} repairs, {} unresolved",
                    sp,
                    aa.chars().count(),
                    dna.len(),
                    result.repairs.len(),
                    result.unresolved.len(),
                );
                return Ok((
                    FileOutcome::Plain { dna },
                    result,
                    aa.chars().count(),
                    aa,
                    message,
                ));
            }
            if let Some(fid) = &fid {
                // Feature CDS inside the DNA file: write-back through the
                // template so the full sequence (CDS replaced) is available.
                let (new_sequence, result, coding) = crate::codon_optimize(
                    &project,
                    fid,
                    &sp,
                    &m,
                    None,
                    os.as_deref(),
                    aes,
                    None,
                )?;
                let message = format!(
                    "Codon optimization for {} (file input, {}): CAI {:.3} → {:.3}, {} repairs, {} unresolved",
                    fid,
                    sp,
                    result.cai_before,
                    result.cai_after,
                    result.repairs.len(),
                    result.unresolved.len(),
                );
                let mut source_project = project.clone();
                source_project.sequence = new_sequence.clone();
                source_project.length = new_sequence.len() as i64;
                Ok((
                    FileOutcome::Feature { new_sequence, source_project },
                    result,
                    coding.codons.len(),
                    coding.aa,
                    message,
                ))
            } else {
                // Whole file sequence as the coding sequence.
                let cleaned = clean_coding_sequence(&project.sequence)
                    .map_err(|e| format!("invalid file sequence: {}", e))?;
                let codons: Vec<String> = cleaned
                    .as_bytes()
                    .chunks(3)
                    .map(|c| String::from_utf8_lossy(c).into_owned())
                    .collect();
                let table = crate::codon_usage_table(&sp, None)?;
                let opts = crate::codon_optimize_options(&m, os.as_deref(), aes, None)?;
                let result = libregene_core::codon::optimize_codons(&codons, &table, &opts);
                let aa: String = codons
                    .iter()
                    .map(|c| table.aa_of.get(c).copied().unwrap_or('?'))
                    .collect();
                let dna: String = result.new_codons.concat();
                let message = format!(
                    "Codon optimization (file input, whole sequence as CDS, {}): CAI {:.3} → {:.3}, {} repairs, {} unresolved",
                    sp,
                    result.cai_before,
                    result.cai_after,
                    result.repairs.len(),
                    result.unresolved.len(),
                );
                Ok((
                    FileOutcome::Plain { dna },
                    result,
                    codons.len(),
                    aa,
                    message,
                ))
            }
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
        .map_err(|e| ErrorData::invalid_params(e, None))?;

        let mut v = codon_preview_json(&result, &aa, codon_count, &method, &species);
        v["ok"] = serde_json::json!(true);
        v["optimizedSequence"] = serde_json::json!(match &outcome {
            FileOutcome::Feature { new_sequence, .. } => new_sequence,
            FileOutcome::Plain { dna } => dna,
        });
        v["message"] = serde_json::json!(message);
        if let Some(op) = output_path {
            let (dna, source) = match &outcome {
                FileOutcome::Feature { source_project, .. } => (None, Some(source_project)),
                FileOutcome::Plain { dna, .. } => (Some(dna.as_str()), None),
            };
            let written = write_optimization_output(&op, dna, &aa, source)
                .map_err(|e| ErrorData::invalid_params(e, None))?;
            v["outputPath"] = serde_json::json!(written);
        }
        Ok(Json(v))
    }
}

/// The optimized sequence carried out of `optimize_file_input`'s compute
/// closure: a full template write-back (feature mode) or a bare DNA string.
enum FileOutcome {
    /// Full file sequence with the optimized CDS written back in place.
    Feature {
        new_sequence: String,
        source_project: ProjectData,
    },
    /// The optimized coding sequence itself.
    Plain { dna: String },
}

// ---------------------------------------------------------------------------
// export_subsequence: region resolution + export data building
// ---------------------------------------------------------------------------

/// Linear template pieces for a 0-based inclusive region; on circular
/// sequences a wrap (start > end) becomes two pieces.
fn region_pieces(project: &ProjectData, s: i64, e: i64) -> Vec<(i64, i64)> {
    if project.topology == "circular" && s > e {
        vec![(s, project.length - 1), (0, e)]
    } else {
        vec![(s, e)]
    }
}

/// The fragment between two cuts (0-based; a cut at index C severs the DNA
/// between C-1 and C). Linear: the span between the smaller and the larger
/// cut ([lo, hi-1]). Circular: the forward arc from cut1 to cut2, wrapping
/// over the origin when cut1 > cut2, the whole molecule when they coincide.
fn fragment_pieces(project: &ProjectData, c1: i64, c2: i64) -> Result<Vec<(i64, i64)>, String> {
    let len = project.length;
    if project.topology == "circular" {
        if c1 < c2 {
            Ok(vec![(c1, c2 - 1)])
        } else if c1 > c2 {
            let mut v = vec![(c1, len - 1)];
            if c2 > 0 {
                v.push((0, c2 - 1));
            }
            Ok(v)
        } else {
            Ok(vec![(0, len - 1)])
        }
    } else {
        let (lo, hi) = (c1.min(c2), c1.max(c2));
        if lo == hi {
            return Err("cut1 and cut2 are equal — the fragment between them is empty".to_string());
        }
        Ok(vec![(lo, hi - 1)])
    }
}

/// The top-strand cut index (0-based) of an enzyme's `ordinal`-th recognition
/// site (sorted by rec_start) from the already-computed engine results.
/// Unknown enzymes error with near-match suggestions, mirroring
/// find_restriction_sites.
fn enzyme_cut_index(project: &ProjectData, name: &str, ordinal: usize) -> Result<i64, String> {
    let mut hits: Vec<&Enzyme> = project
        .enzymes
        .iter()
        .filter(|e| e.name.eq_ignore_ascii_case(name))
        .collect();
    hits.sort_by_key(|e| e.rec_start);
    match hits.get(ordinal) {
        Some(site) => Ok(if site.cut_pairs.is_empty() {
            site.cut_index
        } else {
            site.cut_pairs[0].top_cut_index
        }),
        None => {
            let q = name.to_lowercase();
            let sugg: Vec<&str> = project
                .enzymes
                .iter()
                .map(|e| e.name.as_str())
                .filter(|n| n.to_lowercase().contains(&q))
                .take(5)
                .collect();
            if hits.is_empty() {
                if sugg.is_empty() {
                    Err(format!(
                        "Unknown enzyme '{}': no enzyme with a recognition site in this project has a similar name",
                        name
                    ))
                } else {
                    Err(format!(
                        "Unknown enzyme '{}'; enzymes cutting this sequence with similar names: {}",
                        name,
                        sugg.join(", ")
                    ))
                }
            } else {
                Err(format!(
                    "Enzyme '{}' has only {} recognition site(s) on this sequence; cannot select site {} (0-based)",
                    name,
                    hits.len(),
                    ordinal
                ))
            }
        }
    }
}

/// Resolve a fwd/rev primer argument — a project primer name (stored binding
/// sites are reused, recomputed when empty; name lookup wins) or a raw
/// sequence (binding sites recomputed with the primer engine) — to its best
/// binding site on the wanted strand. Mirrors check_primer_binding's strand
/// semantics: strand 1 = forward, strand -1 = reverse.
fn resolve_primer_binding_site(
    project: &ProjectData,
    input: &str,
    want_strand: i8,
    role: &str,
) -> Result<PrimerBindingSite, String> {
    let seq: String;
    let sites: Vec<PrimerBindingSite>;
    if let Some(p) = project
        .primers
        .iter()
        .find(|p| p.name == input || p.id == input)
    {
        seq = p.primer_seq.clone();
        sites = if p.binding_sites.is_empty() {
            libregene_core::primer::align::compute_binding_sites(
                &project.sequence,
                &p.primer_seq,
                &p.r#type,
                &p.id,
                &project.topology,
                0.0,
            )
        } else {
            p.binding_sites.clone()
        };
    } else {
        let cleaned: String = input
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_uppercase();
        if cleaned.is_empty() {
            return Err(format!(
                "{} '{}' is neither a primer name in the project nor a sequence",
                role, input
            ));
        }
        seq = cleaned.clone();
        let probe = Primer {
            id: role.to_string(),
            name: role.to_string(),
            r#type: "fwd".to_string(),
            primer_seq: cleaned,
            binding_sites: Vec::new(),
        };
        sites = libregene_core::primer::align::compute_binding_sites(
            &project.sequence,
            &probe.primer_seq,
            &probe.r#type,
            &probe.id,
            &project.topology,
            0.0,
        );
    }
    sites
        .iter()
        .find(|s| s.strand == want_strand)
        .cloned()
        .ok_or_else(|| {
            let strand_name = if want_strand == 1 { "forward" } else { "reverse" };
            format!(
                "{} '{}' ({} bp) does not bind the {} strand of the template: {} binding site(s) found, none on the {} strand",
                role, input, seq.len(), strand_name, sites.len(), strand_name
            )
        })
}

/// Bounding box for the export regionView digest. min/max over all pieces:
/// first/last is wrong for multi-segment minus-strand features, whose pieces
/// come back from `resolve_export_region` in descending order. On circular
/// templates, pieces that straddle the origin ((s, len-1) + (0, e)) collapse
/// to the wrap window (s, e) — tighter than a full-length min/max span and
/// meaningful to `project_digest`.
fn region_bbox(pieces: &[(i64, i64)], len: i64, circular: bool) -> (i64, i64) {
    if circular {
        let wrap_start = pieces.iter().filter(|p| p.1 == len - 1).map(|p| p.0).max();
        let wrap_end = pieces.iter().filter(|p| p.0 == 0).map(|p| p.1).min();
        if let (Some(s), Some(e)) = (wrap_start, wrap_end) {
            if s > e {
                return (s, e);
            }
        }
    }
    (
        pieces.iter().map(|p| p.0).min().unwrap_or(0),
        pieces.iter().map(|p| p.1).max().unwrap_or(0),
    )
}

/// Resolve an export_subsequence request to the template pieces it exports:
/// linear 0-based inclusive spans in EXPORT order, `flip` (each piece's
/// sequence is reverse-complemented when exporting a minus-strand feature)
/// and a human-readable description of the selected region. Exactly one
/// selector must be given; mixing selectors is rejected.
fn resolve_export_region(
    project: &ProjectData,
    req: &ExportSubsequenceRequest,
) -> Result<(Vec<(i64, i64)>, bool, String), String> {
    let region_active = req.start.is_some() || req.end.is_some();
    let feature_active = req.feature_id.is_some();
    let fragment_active = req.enzyme1.is_some()
        || req.enzyme2.is_some()
        || req.cut1.is_some()
        || req.cut2.is_some();
    let amplicon_active = req.fwd_primer.is_some() || req.rev_primer.is_some();
    let active = [region_active, feature_active, fragment_active, amplicon_active]
        .into_iter()
        .filter(|a| *a)
        .count();
    if active != 1 {
        return Err(
            "exactly one region selector required: (start+end), (feature_id), (enzyme1+enzyme2 | cut1+cut2), or (fwd_primer+rev_primer)"
                .to_string(),
        );
    }
    if project.length <= 0 || project.sequence.is_empty() {
        return Err("Sequence is empty".to_string());
    }
    let len = project.length;
    let circular = project.topology == "circular";

    if region_active {
        let (s, e) = match (req.start, req.end) {
            (Some(s), Some(e)) => (s, e),
            _ => return Err("start and end are both required (0-based inclusive)".to_string()),
        };
        if s > e && !circular {
            return Err(
                "start > end is only allowed on circular sequences (wraps the origin)".to_string(),
            );
        }
        if s < 0 || e < 0 || s >= len || e >= len {
            return Err(format!(
                "range {}..{} out of bounds for sequence of length {} (0-based inclusive)",
                s, e, len
            ));
        }
        return Ok((
            region_pieces(project, s, e),
            false,
            format!("region {}..{}", s, e),
        ));
    }

    if feature_active {
        let fid = req.feature_id.as_deref().unwrap_or("");
        let f = project
            .features
            .iter()
            .find(|f| f.id == fid)
            .ok_or_else(|| format!("Feature not found: {}", fid))?;
        let segs: Vec<(i64, i64)> = if f.segments.is_empty() {
            vec![(f.start, f.end)]
        } else {
            f.segments.iter().map(|s| (s.start, s.end)).collect()
        };
        let mut pieces: Vec<(i64, i64)> = Vec::new();
        for &(s, e) in &segs {
            if s <= e {
                pieces.push((s, e));
            } else if circular {
                pieces.push((s, len - 1));
                pieces.push((0, e));
            } else {
                return Err(format!(
                    "feature {} spans the origin but the project is linear",
                    f.id
                ));
            }
        }
        for &(s, e) in &pieces {
            if s < 0 || e >= len {
                return Err(format!(
                    "feature {} coordinate {}..{} out of range for sequence of length {}",
                    f.id, s, e, len
                ));
            }
        }
        pieces.sort_unstable();
        let minus = f.strand == "-";
        if minus {
            pieces.reverse();
        }
        return Ok((pieces, minus, format!("feature '{}' ({})", f.name, f.id)));
    }

    if fragment_active {
        let (c1, c2, desc) = match (&req.enzyme1, &req.enzyme2, req.cut1, req.cut2) {
            (Some(e1), Some(e2), None, None) => {
                let c1 = enzyme_cut_index(project, e1, 0)?;
                let c2 = if e1.eq_ignore_ascii_case(e2) {
                    enzyme_cut_index(project, e2, 1)?
                } else {
                    enzyme_cut_index(project, e2, 0)?
                };
                (
                    c1,
                    c2,
                    format!(
                        "fragment between {} (cut {}) and {} (cut {})",
                        e1, c1, e2, c2
                    ),
                )
            }
            (None, None, Some(a), Some(b)) => {
                let max_cut = if circular { len - 1 } else { len };
                if a < 0 || b < 0 || a > max_cut || b > max_cut {
                    return Err(format!(
                        "cut indices {} and {} out of range (0..={} for a {} bp {})",
                        a, b, max_cut, len, project.topology
                    ));
                }
                let (a, b) = if circular { (a % len, b % len) } else { (a, b) };
                (a, b, format!("fragment between cuts {} and {}", a, b))
            }
            _ => {
                return Err(
                    "fragment mode needs enzyme1+enzyme2 (names) OR cut1+cut2 (indices), not a mix"
                        .to_string(),
                )
            }
        };
        return Ok((fragment_pieces(project, c1, c2)?, false, desc));
    }

    let fwd = req.fwd_primer.as_deref().unwrap_or("");
    let rev = req.rev_primer.as_deref().unwrap_or("");
    if fwd.is_empty() || rev.is_empty() {
        return Err(
            "fwd_primer and rev_primer are both required (name or sequence)".to_string(),
        );
    }
    let fsite = resolve_primer_binding_site(project, fwd, 1, "fwd primer")?;
    let rsite = resolve_primer_binding_site(project, rev, -1, "rev primer")?;
    let f_start = fsite.template_start;
    // Rev primer's 5' end is the last template base it covers (template_end
    // is exclusive); the amplicon runs from the fwd 5' end to that base.
    let r_end = if rsite.template_end == 0 {
        len - 1
    } else {
        rsite.template_end - 1
    };
    let pieces = if circular {
        if f_start <= r_end {
            vec![(f_start, r_end)]
        } else {
            vec![(f_start, len - 1), (0, r_end)]
        }
    } else {
        if f_start > r_end {
            return Err(format!(
                "fwd primer's 5' end (position {}) is downstream of the rev primer's 5' end (position {}); the pair does not define an amplicon on a linear sequence",
                f_start, r_end
            ));
        }
        vec![(f_start, r_end)]
    };
    Ok((
        pieces,
        false,
        format!("amplicon fwd '{}' → rev '{}'", fwd, rev),
    ))
}

/// Build the exported sequence (template bases of the pieces, uppercase,
/// reverse-complemented per piece when `flip`) and the features overlapping
/// the pieces with coordinates translated to the new linear coordinate
/// system (strand flipped when `flip`). A feature-mode export's own feature
/// naturally lands on the full [0, len-1] span.
fn build_export_data(project: &ProjectData, pieces: &[(i64, i64)], flip: bool) -> (String, Vec<Feature>) {
    let mut sequence = String::new();
    let mut windows: Vec<(i64, i64)> = Vec::with_capacity(pieces.len());
    let mut off: i64 = 0;
    for &(s, e) in pieces {
        let span = &project.sequence[s as usize..=e as usize];
        if flip {
            sequence.push_str(&libregene_core::utils::reverse_complement(span));
        } else {
            sequence.push_str(span);
        }
        windows.push((off, off + (e - s + 1)));
        off += e - s + 1;
    }
    let mut features: Vec<Feature> = Vec::new();
    for f in &project.features {
        let segs: Vec<(i64, i64)> = if f.segments.is_empty() {
            vec![(f.start, f.end)]
        } else {
            f.segments.iter().map(|s| (s.start, s.end)).collect()
        };
        let mut mapped: Vec<(i64, i64)> = Vec::new();
        for (pi, &(ps, pe)) in pieces.iter().enumerate() {
            let (wo, _) = windows[pi];
            for &(s, e) in &segs {
                let os = s.max(ps);
                let oe = e.min(pe);
                if os <= oe {
                    let (ns, ne) = if flip {
                        (wo + (pe - oe), wo + (pe - os))
                    } else {
                        (wo + (os - ps), wo + (oe - ps))
                    };
                    mapped.push((ns, ne));
                }
            }
        }
        if mapped.is_empty() {
            continue;
        }
        let merged = merge_sorted_segments(mapped);
        let nstart = merged[0].0;
        let nend = merged[merged.len() - 1].1;
        let nstrand = if flip {
            match f.strand.as_str() {
                "+" => "-".to_string(),
                "-" => "+".to_string(),
                s => s.to_string(),
            }
        } else {
            f.strand.clone()
        };
        features.push(Feature {
            id: f.id.clone(),
            name: f.name.clone(),
            start: nstart,
            end: nend,
            color: f.color.clone(),
            ftype: f.ftype.clone(),
            segments: merged
                .iter()
                .map(|&(s, e)| Segment {
                    start: s,
                    end: e,
                    color: None,
                })
                .collect(),
            strand: nstrand,
            notes: f.notes.clone(),
            translation: f.translation.clone(),
            qualifiers: f.qualifiers.clone(),
        });
    }
    (sequence.to_ascii_uppercase(), features)
}

/// Sort spans ascending and merge overlapping/touching ones.
fn merge_sorted_segments(mut segs: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    segs.sort_unstable();
    let mut out: Vec<(i64, i64)> = Vec::new();
    for (s, e) in segs {
        if let Some(last) = out.last_mut() {
            if s <= last.1 + 1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        out.push((s, e));
    }
    out
}

#[tool_router]
impl<R: Runtime> LibreGeneMcp<R> {
    /// List all open projects — the files currently loaded into memory.
    /// A "project" is an open file: `open_file` loads a file as a project and
    /// returns its `projectId`; every other tool addresses that project by
    /// `project_id`. Returns {"projects": [{id, name, length, topology,
    /// dirty}], "activeId": id-or-null}.
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
    /// list. The digest ends with a `DETECTED COMMON FEATURES (auto)` section
    /// listing non-fragment features auto-annotated against the embedded
    /// SnapGene database, one line each (name | type | strand | start..end |
    /// identity%) with an `(already annotated)` marker. Fragment hits are
    /// omitted to avoid misleading partial matches. Length units and sections
    /// follow the molecule type: DNA projects get bp + PRIMERS/ENZYMES/
    /// methylation/auto-annotation; RNA/protein projects use nt/aa and omit all
    /// DNA-only sections (features still render). Returns {projectId, text}.
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
            include_auto_annotation: true,
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
            include_auto_annotation: false,
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
    /// DNA-only: rejects RNA/protein projects (single-strand, no reverse
    /// strand to search).
    #[tool]
    async fn search_sequence(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        let matches = crate::do_search_sequence(&self.pm, &id, request.query)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "matches": matches })))
    }

    /// List restriction-enzyme recognition sites on a project's sequence.
    /// `enzymes` is an optional list of enzyme names (case-insensitive); omit
    /// it (or pass []) to report every enzyme that has a site. Unknown names
    /// are rejected with near-match suggestions — use that error to probe
    /// which enzyme names exist on this sequence (this is the replacement for
    /// the removed full-database dump: query per name instead of pulling the
    /// whole ~196 KB catalog). Sites are the already-computed engine results
    /// the UI shows (circular-normalized, methylation-aware), so no recompute
    /// runs.
    /// Returns {projectId, enzymes: [{name, sites: [{recStart, recEnd,
    /// recSeq, strand, cuts: [{topCutIndex, botCutIndex}], methylationBlocked,
    /// unique}]}]}. recStart/recEnd are 0-based inclusive; a cut happens
    /// BETWEEN cut-1 and cut (0-based); strand is "top" or "bottom"
    /// (recognition orientation); unique = exactly one site for that enzyme.
    /// DNA-only: rejects RNA/protein projects (no restriction sites).
    #[tool]
    async fn find_restriction_sites(
        &self,
        Parameters(request): Parameters<FindRestrictionSitesRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        if !project.is_dna() {
            return Err(ErrorData::invalid_params(
                format!(
                    "This tool only supports DNA projects; project '{}' is a {} project",
                    id, project.molecule_type
                ),
                None,
            ));
        }
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
                                    "Unknown enzyme '{}': no enzyme with a recognition site in this project has a similar name",
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

    /// Open a sequence file and load it into the project manager as a new
    /// project (project id = file path; the returned `projectId` is how every
    /// other tool refers to it — see list_projects). This is the entry point
    /// for handing a file to the app; files are also the recommended way to
    /// move a sequence between projects (write with save_file/export_subsequence,
    /// read back with open_file). Enzyme and primer recompute run on a
    /// background thread; the UI is refreshed via broadcast. Returns
    /// {ok, message, projectId, regionView} where regionView is the compact
    /// overview digest of the opened project (enzyme cutters collapsed to a
    /// count line).
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

    /// Save a project (addressed by `project_id`, defaults to the active one)
    /// to a GenBank file on disk — the reverse of open_file: the file holds
    /// the project's current sequence + features. Uses the same serializer and
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

    /// Close (unload) a project from memory without saving. Projects are
    /// addressed by `project_id` (see list_projects); closing is NOT a file
    /// operation — the file on disk is untouched. Mirrors delete_project; the
    /// UI updates via broadcast. Returns {ok, message, projectId}.
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

    /// Make a project (addressed by `project_id`) the active one — the one
    /// tools use when they omit `project_id`. Mirrors activate_project.
    /// Returns {ok, message, projectId, regionView}.
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
    /// wrap (start > end+1 rejected). The replacement sequence is given either
    /// as a plain string (`replacement`) or read from a local sequence file
    /// (`replacement_path` — .gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 etc.,
    /// the same formats open_file accepts; exactly one of the two must be
    /// given, and a file is the recommended way to hand a long insert to this
    /// tool). Feature coordinates are shifted/clipped
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
    /// before, after} as {start, end}). On protein projects the replacement is
    /// uppercased and must be amino-acid letters (A-Z, optional trailing '*'
    /// stop codon); lengths are reported in aa (nt for RNA, bp for DNA).
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

        let replacement = match (request.replacement, request.replacement_path) {
            (Some(_), Some(_)) => {
                return Ok(Json(fail_envelope(
                    &id,
                    "Provide exactly one of `replacement` or `replacement_path`, not both"
                        .to_string(),
                )));
            }
            (None, None) => {
                return Ok(Json(fail_envelope(
                    &id,
                    "Provide exactly one of `replacement` (sequence string, empty = delete) or `replacement_path` (sequence file)"
                        .to_string(),
                )));
            }
            (Some(s), None) => s,
            (None, Some(path)) => {
                crate::validate_user_path(&path, crate::SEQ_EXTS).map_err(|e| {
                    ErrorData::internal_error(format!("invalid replacement_path: {}", e), None)
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
                                "Failed to read replacement sequence file (supported: .gbk/.gb/.genbank, .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1): {}",
                                e
                            ),
                        )));
                    }
                }
            }
        };

        // Protein projects: normalize the replacement to uppercase and require
        // the amino-acid alphabet (A-Z, optional single trailing '*' stop).
        let mut replacement = replacement;
        if project.molecule_type == "protein" {
            let up = replacement.to_ascii_uppercase();
            let body = up.strip_suffix('*').unwrap_or(&up);
            if up.matches('*').count() > 1 || !body.chars().all(|c| c.is_ascii_alphabetic()) {
                return Ok(Json(fail_envelope(
                    &id,
                    "Invalid protein replacement: only amino-acid letters (A-Z) and an optional trailing '*' (stop codon) are allowed".to_string(),
                )));
            }
            replacement = up;
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
            replacement,
            &project.sequence[(end + 1) as usize..]
        );
        let new_len = new_seq.len() as i64;

        // Side effects on features, derived from the pre-edit list with the
        // same span math as the adjust below (no snapshot/compare needed).
        let impact = libregene_core::utils::features_edit_impact(
            &project.features,
            start,
            end,
            replacement.len() as i64,
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
                    replacement.len() as i64,
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

        let repl_len = replacement.len() as i64;
        let unit = match project.molecule_type.as_str() {
            "rna" => "nt",
            "protein" => "aa",
            _ => "bp",
        };
        let new_win = (
            (start - 30).max(0),
            (start + repl_len + 30 - 1).min(new_len - 1),
        );
        let new_region = self.digest_region(&id, Some(new_win), true).await;

        let mut v = serde_json::json!({
            "ok": true,
            "message": format!(
                "Replaced [{}..{}] ({} {}) with {} {}; new length {} (was {})",
                start, end,
                if is_insertion { 0 } else { end - start + 1 },
                unit,
                repl_len,
                unit,
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
    /// DNA-only: rejects RNA/protein projects (single-strand molecules carry
    /// no primers).
    #[tool]
    async fn add_primer(
        &self,
        Parameters(request): Parameters<AddPrimerRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
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

    /// Align a read against the project template and APPEND it as a new
    /// alignment (never overwrites existing ones; ids are aln-1, aln-2, ...).
    /// Provide exactly one of:
    /// - `bases`: the read sequence as a plain string (whitespace/non-ACGT
    ///   chars are stripped). For long reads prefer `path` — a file is the
    ///   recommended way to hand a sequence to this tool.
    /// - `path`: read the sequence from a file. Supported file types:
    ///   `.gbk`/`.gb`/`.genbank` (GenBank), `.dna`/`.rna`/`.prot` (SnapGene
    ///   binary), `.gpt` (protein GenBank), `.fa`/`.fasta` (FASTA / plain
    ///   text sequence), `.ab1` (ABIF chromatogram; the basecalled PBAS
    ///   sequence is extracted).
    /// Giving neither or both is an error. A name is always required.
    ///
    /// Returns {ok, message, projectId, regionView, significant, identity,
    /// strand, segmentCount, alignedLength, mismatches, insertions,
    /// deletions, mismatchDetails, deletionDetails, insertionDetails, name,
    /// alignmentId, alignments}.
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
    /// - `alignments`: the project's FULL alignment list (including the one
    ///   just added), each {alignmentId, name, identity, strand,
    ///   segmentCount, alignedLength, mismatches, insertions, deletions,
    ///   mismatchDetails, deletionDetails, insertionDetails} — lets a caller
    ///   inspect every stored alignment without a separate read tool. The
    ///   top-level fields above describe the newly added alignment.
    /// On failure returns {ok: false, message, projectId, significant: false};
    /// a message starting with "No significant alignment found" states the
    /// reason (identity below the 0.60 minimum, or aligned span below the
    /// 50 bp minimum).
    #[tool]
    async fn add_alignment(
        &self,
        Parameters(request): Parameters<AddAlignmentRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
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
                                "Failed to read alignment sequence file (supported: .gbk/.gb/.genbank, .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1): {}",
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
        let (summary, alignments, region) = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .map(|p| {
                    let alignments: Vec<serde_json::Value> = p
                        .alignments
                        .iter()
                        .map(|a| {
                            let diff = libregene_core::align::alignment_diff(a, &p.sequence);
                            serde_json::json!({
                                "alignmentId": a.id,
                                "name": a.name,
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
                            })
                        })
                        .collect();
                    let last = p.alignments.last();
                    let region = last.and_then(|a| {
                        a.segments.first().map(|s| (s.start as i64, s.end as i64))
                    });
                    (alignments.last().cloned(), alignments, region)
                })
                .unwrap_or((None, Vec::new(), None))
        };
        let region_view = match region {
            Some((s, e)) => self.digest_region(&id, Some((s, e)), true).await,
            None => self.digest_region(&id, None, true).await,
        };
        let mut env = ok_envelope(&id, format!("Aligned {}", name), region_view);
        if let Some(s) = summary {
            env["significant"] = serde_json::json!(true);
            for (k, v) in s.as_object().unwrap_or(&serde_json::Map::new()) {
                env[k] = v.clone();
            }
        }
        env["alignments"] = serde_json::json!(alignments);
        Ok(Json(env))
    }

    // -----------------------------------------------------------------------
    // Analysis
    // -----------------------------------------------------------------------

    /// Find open reading frames (ATG→stop, both strands, all frames) on a
    /// project. min_aa defaults to 75. When add_as_features is true the ORFs
    /// are appended as real CDS features (through the add-feature path, with
    /// recompute/broadcast) and {ok, message, projectId, regionView} is
    /// returned; otherwise returns {projectId, orfs: [Feature]}.
    /// DNA-only: rejects RNA/protein projects (single-strand, no ORFs).
    #[tool]
    async fn find_orfs(
        &self,
        Parameters(request): Parameters<FindOrfsRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
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
    /// amplify: optional `fwd_enzyme`/`rev_enzyme` (enzyme names, e.g.
    /// "BamHI" — probe valid names via find_restriction_sites' unknown-name
    /// suggestions) add a 5' tail of
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
    /// DNA-only: rejects RNA/protein projects (no primer design on
    /// single-strand molecules).
    #[tool]
    async fn design_primers(
        &self,
        Parameters(request): Parameters<DesignPrimersRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
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
    /// binds, bindingSiteCount, site, sites}]}. `bindingSiteCount` is the
    /// number of binding sites (0 when the primer does not bind); `site` is
    /// the best one ({strand, templateStart, templateEnd, tm, annealLen,
    /// mismatchedTail} or null) and `sites` lists ALL sites best-first (Tm
    /// descending, same field shape as `site`) — use `sites` for off-target
    /// detection. templateStart is 0-based inclusive, templateEnd 0-based
    /// EXCLUSIVE (range spans templateStart..templateEnd-1); cuts are not
    /// involved. `binds: true` means the 3' anneal core matched —
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
    /// DNA-only: rejects RNA/protein projects (no primer binding on
    /// single-strand molecules).
    #[tool]
    async fn check_primer_binding(
        &self,
        Parameters(request): Parameters<CheckPrimerBindingRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
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

    /// Optimize a coding sequence's codons (DNA Chisel ports: use_best_codon /
    /// match_codon_usage / harmonize_rca) — project, raw sequence, or file
    /// input. `species` is a key from list_species (e.g. "e_coli",
    /// "h_sapiens", "s_cerevisiae"); `method` defaults to use_best_codon;
    /// harmonize_rca additionally uses `original_species` as the source table.
    ///
    /// Exactly one input mode:
    /// - `project_id` + `feature_id`: optimize the CDS/mRNA feature inside an
    ///   open project (`project_id` omitted = active project). `apply=false`
    ///   (default) is a read-only preview; `apply=true` replaces the feature's
    ///   coding bases in the template (equal-length synonymous substitution,
    ///   coordinates unchanged) through the same recompute+broadcast path as
    ///   edit_sequence. Project mode requires a DNA project — protein projects
    ///   are rejected with a hint to use `sequence`/`input_path` instead
    ///   (amino acids are reverse-translated there).
    /// - `sequence`: raw DNA coding sequence text. Whitespace/digits are
    ///   ignored, letters must be A/C/G/T, length must be divisible by 3 (a
    ///   trailing stop codon is fine). No project is involved. For long
    ///   sequences prefer `input_path` (file) — the recommended way to pass a
    ///   large sequence to this tool.
    /// - `input_path`: local file parsed with file_io. DNA files (.gbk/.gb/
    ///   .genbank/.dna/.rna/.fasta/.fa/.fna/.ab1): with `feature_id` the
    ///   file's CDS/mRNA feature is optimized (the written sequence carries
    ///   the full file sequence with that CDS replaced); without `feature_id`
    ///   the whole file sequence is treated as the coding sequence. Protein
    ///   files (.gpt/.prot) mean REVERSE TRANSLATION: the amino acid sequence
    ///   is turned directly into an optimized DNA coding sequence (codons
    ///   chosen per `method` and the `species` table).
    ///
    /// Returns {ok, message, projectId?, aa, codonCount, newCodons,
    /// caiBefore, caiAfter, gcBefore, gcAfter, repairs, repairCount,
    /// unresolved, method, species, optimizedSequence?, outputPath?,
    /// regionView?}. `optimizedSequence` (the full optimized DNA) is added in
    /// sequence/input_path modes; `outputPath` when `output_path` was given;
    /// `regionView` after an apply=true project write-back.
    ///
    /// `output_path` (any input mode, optional): writes the result to a file
    /// — .gbk/.gb/.genbank → DNA GenBank with the optimized CDS annotated,
    /// .gpt → protein GenBank of the translated sequence; other extensions
    /// are rejected. `apply=true` is only meaningful in project mode: in
    /// sequence/input_path mode it requires `output_path` (there is no
    /// project to update).
    #[tool]
    async fn optimize_cds(
        &self,
        Parameters(request): Parameters<OptimizeCdsRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let mode = resolve_optimize_input(
            request.project_id.as_deref(),
            request.feature_id.as_deref(),
            request.sequence.as_deref(),
            request.input_path.as_deref(),
        )
        .map_err(|e| ErrorData::invalid_params(e, None))?;

        let apply = request.apply.unwrap_or(false);
        if !matches!(mode, OptimizeInput::Project { .. }) && apply && request.output_path.is_none()
        {
            return Err(ErrorData::invalid_params(
                "apply=true is only meaningful in project mode; in sequence/input_path mode pass `output_path` to write the result to a file (or set apply=false)",
                None,
            ));
        }
        if let Some(op) = &request.output_path {
            crate::validate_user_path(op, crate::CODON_OUTPUT_EXTS).map_err(|e| {
                ErrorData::invalid_params(format!("invalid output_path: {}", e), None)
            })?;
        }

        let species = request.species.clone();
        let method = request
            .method
            .clone()
            .unwrap_or_else(|| "use_best_codon".to_string());
        let original_species = request.original_species.clone();
        let avoid_enzyme_sites = request.avoid_enzyme_sites.clone();

        match mode {
            OptimizeInput::Project { project_id, feature_id } => {
                let id = self.resolve_project_id(project_id).await?;
                let project = {
                    let pm = self.pm.read().await;
                    pm.get_project_by_id(&id).cloned().ok_or_else(|| {
                        ErrorData::invalid_params(format!("Project not found: {}", id), None)
                    })?
                };
                if project.molecule_type == "protein" {
                    return Err(ErrorData::invalid_params(
                        "optimize_cds project mode re-encodes a CDS feature inside a DNA project; a protein project has no coding DNA to re-encode — pass `sequence` (raw coding DNA) or `input_path` instead (a protein .gpt/.prot file is reverse-translated to optimized DNA)".to_string(),
                        None,
                    ));
                }
                let f_id = feature_id.clone();
                let sp = species.clone();
                let m = method.clone();
                let (new_sequence, result, coding) = tokio::task::spawn_blocking(move || {
                    crate::codon_optimize(
                        &project,
                        &f_id,
                        &sp,
                        &m,
                        None,
                        original_species.as_deref(),
                        avoid_enzyme_sites,
                        None,
                    )
                })
                .await
                .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
                .map_err(|e| ErrorData::invalid_params(e, None))?;

                let mut v =
                    codon_preview_json(&result, &coding.aa, coding.codons.len(), &method, &species);
                v["ok"] = serde_json::json!(true);
                v["projectId"] = serde_json::json!(id);
                v["message"] = serde_json::json!(format!(
                    "Codon optimization preview for {} ({}): CAI {:.3} → {:.3}, GC {:.1}% → {:.1}%, {} repairs, {} unresolved",
                    feature_id,
                    species,
                    result.cai_before,
                    result.cai_after,
                    result.gc_before * 100.0,
                    result.gc_after * 100.0,
                    result.repairs.len(),
                    result.unresolved.len(),
                ));
                if apply {
                    let payload = crate::do_update_sequence(&self.pm, id.clone(), new_sequence)
                        .await
                        .map_err(|e| ErrorData::internal_error(e, None))?;
                    if let Some(err) = Self::payload_error(&payload) {
                        return Ok(Json(fail_envelope(&id, err)));
                    }
                    crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, None).await;
                    if let Some(rv) = self.digest_feature_region(&id, &feature_id).await {
                        v["regionView"] = serde_json::json!(rv);
                    }
                    v["message"] = serde_json::json!(format!(
                        "Optimized CDS {} ({}): CAI {:.3} → {:.3}, {} repairs, {} unresolved",
                        feature_id,
                        species,
                        result.cai_before,
                        result.cai_after,
                        result.repairs.len(),
                        result.unresolved.len(),
                    ));
                }
                Ok(Json(v))
            }
            OptimizeInput::Sequence(seq) => {
                self.optimize_sequence_input(
                    seq,
                    species,
                    method,
                    original_species,
                    avoid_enzyme_sites,
                    request.output_path,
                )
                .await
            }
            OptimizeInput::File { path, feature_id } => {
                self.optimize_file_input(
                    path,
                    feature_id,
                    species,
                    method,
                    original_species,
                    avoid_enzyme_sites,
                    request.output_path,
                )
                .await
            }
        }
    }

    /// Export a subsequence of a project to a new file (GenBank) — the
    /// recommended way to hand a sequence to another tool: export it with
    /// this tool, then pass the output_path (instead of pasting large
    /// sequences into tool arguments). The file holds the region's sequence
    /// (uppercase; template strand except as noted) plus every feature
    /// overlapping it with coordinates translated to the new linear
    /// coordinate system; circular projects always export linear fragments.
    ///
    /// `project_id` defaults to the active project. `output_path` is required
    /// — .gbk/.gb/.genbank for DNA/RNA projects, .gpt for protein projects
    /// (other extensions are rejected).
    ///
    /// Exactly ONE region selector (mixing selectors is rejected):
    /// - `start` + `end`: 0-based inclusive template coordinates; on circular
    ///   sequences `start > end` wraps the origin.
    /// - `feature_id`: the feature's sequence with its segments joined in
    ///   biological order (5'→3', reverse-complemented for minus-strand
    ///   features). The exported feature spans the whole exported sequence;
    ///   other features overlapping its segments are carried along
    ///   (coordinates translated, strand flipped to match the rev-comp'd
    ///   orientation).
    /// - `enzyme1` + `enzyme2`: the fragment between the two enzymes' cut
    ///   sites (names — unknown names are rejected with near-match
    ///   suggestions, the same probe find_restriction_sites uses). Each
    ///   enzyme contributes the top-strand cut of its first recognition site
    ///   on the sequence; passing the same name twice uses that enzyme's
    ///   first two sites. On circular sequences the fragment is the forward
    ///   arc from enzyme1's cut to enzyme2's cut (wrapping the origin when
    ///   needed); on linear sequences the two cuts may be given in either
    ///   order.
    /// - `cut1` + `cut2` (alternative to the enzyme names): explicit cut
    ///   indices, 0-based — a cut at index C severs the DNA between C-1 and
    ///   C (0..=len; the fragment is [min, max-1] on linear sequences, the
    ///   forward arc on circular ones).
    /// - `fwd_primer` + `rev_primer`: the amplicon between the two primers'
    ///   binding sites. Each is a project primer name (stored binding sites
    ///   are used; name lookup wins) or a raw sequence (binding sites
    ///   recomputed with the primer engine, like check_primer_binding). The
    ///   fwd primer's best forward-strand site and the rev primer's best
    ///   reverse-strand site define the amplicon [fwdStart, revEnd-1]
    ///   (0-based inclusive) — the PCR product's top strand. A primer that
    ///   does not bind the strand its role needs is an error.
    ///
    /// Returns {ok, message, projectId, outputPath, length, regionView?}.
    /// `length` is the exported sequence length (bp/nt/aa); `regionView` is
    /// a compact digest of the source project over the exported region's
    /// bounding box. The exported sequence itself is NOT echoed — read it
    /// back with open_file/read_sequence on the written file.
    #[tool]
    async fn export_subsequence(
        &self,
        Parameters(request): Parameters<ExportSubsequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id.clone()).await?;
        let ext = crate::validate_user_path(&request.output_path, crate::CODON_OUTPUT_EXTS)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let is_protein = project.molecule_type == "protein";
        let wants_gpt = ext == "gpt";
        if wants_gpt != is_protein {
            return Ok(Json(fail_envelope(
                &id,
                format!(
                    "molecule type '{}' exports as {} (DNA/RNA → .gbk/.gb/.genbank, protein → .gpt)",
                    project.molecule_type,
                    if is_protein { ".gpt" } else { ".gbk/.gb/.genbank" }
                ),
            )));
        }
        let unit = match project.molecule_type.as_str() {
            "rna" => "nt",
            "protein" => "aa",
            _ => "bp",
        };

        let (pieces, flip, desc) = resolve_export_region(&project, &request)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let bbox = region_bbox(&pieces, project.length, project.topology == "circular");
        let out_name = output_project_name(&request.output_path);
        let path = request.output_path.clone();
        let message_path = path.clone();
        let out_project = tokio::task::spawn_blocking(move || {
            let (sequence, features) = build_export_data(&project, &pieces, flip);
            let length = sequence.len() as i64;
            ProjectData {
                name: out_name,
                sequence,
                length,
                topology: "linear".to_string(),
                molecule_type: project.molecule_type.clone(),
                features,
                ..Default::default()
            }
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
        let length = out_project.length;

        let written = tokio::task::spawn_blocking(move || {
            let res = if wants_gpt {
                libregene_core::file_io::gpt::write_gpt(&out_project, std::path::Path::new(&path))
            } else {
                libregene_core::file_io::gbk::write_gbk(&out_project, std::path::Path::new(&path))
            };
            res.map_err(|e| format!("failed to write {}: {}", path, e))
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
        if let Err(e) = written {
            return Err(ErrorData::invalid_params(e, None));
        }

        let region = self.digest_region(&id, Some(bbox), true).await;
        let mut v = ok_envelope(
            &id,
            format!("Exported {} ({} {}) to {}", desc, length, unit, message_path),
            region,
        );
        v["outputPath"] = serde_json::json!(message_path);
        v["length"] = serde_json::json!(length);
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
    /// Bearer token required on every MCP request so that other local
    /// processes (or a browser via DNS rebinding) can't drive the MCP tools.
    /// Persisted to `<app_config_dir>/mcp_auth_token` so it survives app
    /// restarts; only regenerated when the user explicitly asks. Exposed to
    /// the trusted frontend via `get_mcp_token` / `regenerate_mcp_token`.
    auth_token: Arc<StdMutex<String>>,
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
        let auth_token = load_or_create_token(&app_handle);
        Self {
            app_handle,
            pm,
            wp,
            config: Arc::new(StdMutex::new(McpConfig::default())),
            task: Arc::new(StdMutex::new(None)),
            auth_token: Arc::new(StdMutex::new(auth_token)),
        }
    }

    /// The bearer token the trusted frontend must send to use the MCP server.
    pub fn auth_token(&self) -> String {
        self.auth_token.lock().unwrap().clone()
    }

    /// Generate and persist a fresh bearer token. Takes effect immediately for
    /// the running server (the auth middleware reads the shared token per
    /// request), so no restart is needed.
    pub fn regenerate_auth_token(&self) -> String {
        let token = generate_auth_token();
        *self.auth_token.lock().unwrap() = token.clone();
        persist_token(&self.app_handle, &token);
        token
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

/// Path of the persisted bearer token file. Best-effort: returns None when
/// the config dir is unavailable (e.g. under the mock test runtime).
fn token_file_path<R: Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("mcp_auth_token"))
}

fn persist_token<R: Runtime>(app: &AppHandle<R>, token: &str) {
    if let Some(path) = token_file_path(app) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, token);
    }
}

/// Load the persisted token, or generate and persist a fresh one on first run.
fn load_or_create_token<R: Runtime>(app: &AppHandle<R>) -> String {
    if let Some(path) = token_file_path(app) {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            let token = contents.trim().to_string();
            if !token.is_empty() {
                return token;
            }
        }
        let token = generate_auth_token();
        persist_token(app, &token);
        return token;
    }
    generate_auth_token()
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

/// Build a JSON-RPC error HTTP response (`Content-Type: application/json`).
/// Used by the request middleware so protocol-level failures (401/406/404)
/// carry a readable, structured body instead of rmcp's bare status text.
fn jsonrpc_error_response(
    status: axum::http::StatusCode,
    id: Option<serde_json::Value>,
    code: i64,
    message: &str,
) -> axum::response::Response {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(serde_json::Value::Null),
        "error": { "code": code, "message": message },
    });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    axum::http::Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(bytes))
        .expect("valid response")
}

/// Extract the JSON-RPC request id from a raw body so error responses can
/// echo it back; None (rendered as `id: null`) when the body isn't parseable
/// or carries no id.
fn jsonrpc_id_from_body(bytes: &[u8]) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    match v {
        serde_json::Value::Object(o) => o.get("id").cloned(),
        serde_json::Value::Array(a) => a.first().and_then(|e| e.get("id").cloned()),
        _ => None,
    }
}

async fn serve_mcp<R: Runtime>(
    app_handle: AppHandle<R>,
    pm: Arc<RwLock<ProjectManager>>,
    wp: Arc<RwLock<HashMap<String, String>>>,
    port: u16,
    auth_token: Arc<StdMutex<String>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let expected_host = format!("127.0.0.1:{}", port);

    // Session durability: rmcp's default SessionConfig.keep_alive closes a
    // session after 5 minutes of inactivity. An LLM agent can pause longer
    // than that between tool calls (the 2026-08 MCP test lost sessions this
    // way through a 4 s keep-alive local proxy). Sessions are keyed in
    // memory, not bound to a TCP connection — a dropped connection does not
    // close them; only explicit DELETE, the idle timeout, or app exit does.
    // Extend the idle timeout to 24 h. Trade-off: abandoned sessions linger
    // until the app exits (bounded in practice — a desktop app holds a
    // handful), which is why rmcp's own docs advise against disabling the
    // timeout entirely on long-running public servers.
    let mut session_manager = session::local::LocalSessionManager::default();
    session_manager.session_config.keep_alive = Some(Duration::from_secs(24 * 60 * 60));

    // SSE keep-alive pings every 3 s (rmcp default is 15 s): long-lived SSE
    // streams (GET notification channels) stay busy enough that aggressive
    // local proxies with short idle timeouts (e.g. 4 s on 127.0.0.1:7890)
    // don't drop them mid-stream.
    let mut server_config = StreamableHttpServerConfig::default();
    server_config.sse_keep_alive = Some(Duration::from_secs(3));

    let service = StreamableHttpService::new(
        move || Ok(LibreGeneMcp::new(app_handle.clone(), pm.clone(), wp.clone())),
        Arc::new(session_manager),
        server_config,
    );

    // Middleware: (1) require a local bearer token AND a matching Host header
    // (the token stops other local processes / a browser page via DNS
    // rebinding from driving the MCP tools; the Host check blocks
    // cross-origin/rebinding requests that don't target 127.0.0.1:<port>);
    // (2) reject missing/wrong Accept headers with a JSON-RPC error body
    // instead of rmcp's bare 406; (3) rewrite rmcp's plain-text 404
    // "Session not found" into a structured JSON-RPC error (code -32001) so
    // clients can tell the session expired and must re-initialize. The
    // request body is buffered (bounded, same 4 MiB limit as rmcp) only to
    // echo the JSON-RPC id back in error bodies; success responses are
    // passed through untouched (their SSE bodies must never be consumed).
    const MCP_BODY_LIMIT: usize = 4 * 1024 * 1024;
    let auth_token_for_layer = auth_token.clone();
    let expected_host_for_layer = expected_host.clone();
    let auth_layer = axum::middleware::from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| {
            let auth_token = auth_token_for_layer.clone();
            let expected_host = expected_host_for_layer.clone();
            async move {
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
                    // Read the shared token per request so a user-triggered
                    // regeneration takes effect without restarting the server.
                    .map(|h| h.strip_prefix("Bearer ").map(|t| t == auth_token.lock().unwrap().as_str()).unwrap_or(false))
                    .unwrap_or(false);
                if !(host_ok && bearer_ok) {
                    return jsonrpc_error_response(
                        axum::http::StatusCode::UNAUTHORIZED,
                        None,
                        -32000,
                        "Unauthorized: every MCP request must include 'Authorization: Bearer <token>' and 'Host: 127.0.0.1:<port>'",
                    );
                }

                let (parts, body) = req.into_parts();
                let bytes = match axum::body::to_bytes(body, MCP_BODY_LIMIT).await {
                    Ok(b) => b,
                    Err(_) => {
                        return jsonrpc_error_response(
                            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                            None,
                            -32000,
                            "Request body too large",
                        );
                    }
                };
                let req_id = jsonrpc_id_from_body(&bytes);

                // Mirror rmcp's Accept requirement (it otherwise answers with
                // a bare 406 and no readable body).
                let accept_ok = if parts.method == axum::http::Method::GET {
                    parts
                        .headers
                        .get(axum::http::header::ACCEPT)
                        .and_then(|h| h.to_str().ok())
                        .is_some_and(|h| h.contains("text/event-stream"))
                } else if parts.method == axum::http::Method::POST {
                    parts
                        .headers
                        .get(axum::http::header::ACCEPT)
                        .and_then(|h| h.to_str().ok())
                        .is_some_and(|h| h.contains("application/json") && h.contains("text/event-stream"))
                } else {
                    true
                };
                if !accept_ok {
                    return jsonrpc_error_response(
                        axum::http::StatusCode::NOT_ACCEPTABLE,
                        req_id,
                        -32600,
                        "Not Acceptable: MCP Streamable HTTP requires an Accept header — POST /mcp needs 'Accept: application/json, text/event-stream', GET needs 'Accept: text/event-stream'",
                    );
                }

                let req = axum::http::Request::from_parts(parts, axum::body::Body::from(bytes));
                let resp = next.run(req).await;
                if resp.status() == axum::http::StatusCode::NOT_FOUND {
                    let (rparts, rbody) = resp.into_parts();
                    match axum::body::to_bytes(rbody, 64 * 1024).await {
                        Ok(rbytes) => {
                            let text = String::from_utf8_lossy(&rbytes);
                            if text.contains("Session not found") {
                                return jsonrpc_error_response(
                                    axum::http::StatusCode::NOT_FOUND,
                                    req_id,
                                    -32001,
                                    "Session not found: the MCP session has expired or was closed (e.g. the connection was dropped by a proxy or an idle timeout). Call initialize again to create a new session.",
                                );
                            }
                            return axum::http::Response::from_parts(
                                rparts,
                                axum::body::Body::from(rbytes),
                            );
                        }
                        Err(_) => {
                            return axum::http::Response::from_parts(rparts, axum::body::Body::empty())
                        }
                    }
                }
                resp
            }
        },
    );

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
    async fn handshake_ok(port: u16, token: &str) -> bool {
        let addr = format!("127.0.0.1:{}", port);
        let Ok(mut stream) = tokio::net::TcpStream::connect(&addr).await else {
            return false;
        };
        let body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"cfg-test\",\"version\":\"0\"}}}";
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

    /// Raw HTTP POST /mcp returning the full response (status line + body).
    /// `extra_headers` must be pre-formatted header lines each ending with
    /// `\r\n` (e.g. Accept, Mcp-Session-Id); Content-Type,
    /// Content-Length and `Connection: close` are added automatically.
    async fn raw_post(port: u16, token: &str, extra_headers: &str, body: &str) -> String {
        let addr = format!("127.0.0.1:{}", port);
        let mut stream = tokio::net::TcpStream::connect(&addr)
            .await
            .expect("connect");
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).await.expect("write request");
        let mut buf = Vec::new();
        let mut tmp = [0u8; 8192];
        loop {
            match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut tmp)).await {
                Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&tmp[..n]),
                _ => break,
            }
        }
        String::from_utf8_lossy(&buf).to_string()
    }

    async fn wait_up(port: u16, token: &str) -> bool {
        for _ in 0..40 {
            if handshake_ok(port, token).await {
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
        let token = server.auth_token();

        // start on a fresh port
        server.set_config(true, 19999).await.unwrap();
        assert!(wait_up(19999, &token).await, "server should be up on 19999");

        // disable → port released
        server.set_config(false, 19999).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!handshake_ok(19999, &token).await, "server should be down after disable");

        // re-enable on a new port without an app restart
        server.set_config(true, 20001).await.unwrap();
        assert!(wait_up(20001, &token).await, "server should be up on 20001 after restart");
        assert!(!handshake_ok(19999, &token).await, "old port must stay free");

        // unchanged config → no restart churn
        server.set_config(true, 20001).await.unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(handshake_ok(20001, &token).await, "server must survive a no-op set_config");

        // clean up
        server.set_config(false, 20001).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!handshake_ok(20001, &token).await);
    }

    #[tokio::test]
    async fn regenerated_token_takes_effect_without_restart() {
        let server = test_server();
        server.set_config(true, 20003).await.unwrap();
        let old = server.auth_token();
        assert!(wait_up(20003, &old).await, "server should be up with the initial token");

        let new = server.regenerate_auth_token();
        assert_ne!(old, new);
        // the running server must accept the new token and reject the old one
        assert!(handshake_ok(20003, &new).await, "new token should be accepted");
        assert!(!handshake_ok(20003, &old).await, "old token should be rejected");

        server.set_config(false, 20003).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn missing_accept_header_returns_structured_406() {
        let server = test_server();
        let token = server.auth_token();
        server.set_config(true, 20005).await.unwrap();
        assert!(wait_up(20005, &token).await);

        // No Accept header at all: rmcp would answer a bare 406 with no
        // readable body; the middleware must return a JSON-RPC error body
        // naming the required Accept header and echoing the request id.
        let resp = raw_post(
            20005,
            &token,
            "",
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/list\"}",
        )
        .await;
        assert!(resp.contains("406"), "expected 406, got: {resp}");
        assert!(resp.contains("application/json"), "{resp}");
        assert!(resp.contains("text/event-stream"), "{resp}");
        assert!(resp.contains("-32600"), "{resp}");
        assert!(resp.contains("\"id\":7"), "{resp}");

        server.set_config(false, 20005).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn unknown_session_returns_structured_404() {
        let server = test_server();
        let token = server.auth_token();
        server.set_config(true, 20006).await.unwrap();
        assert!(wait_up(20006, &token).await);

        // A request for a session that never existed: rmcp answers 404 with
        // plain text; the middleware must rewrite it into a JSON-RPC error
        // with code -32001 so clients know the session is gone and must
        // re-initialize.
        let resp = raw_post(
            20006,
            &token,
            "Accept: application/json, text/event-stream\r\nMcp-Session-Id: does-not-exist\r\n",
            "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"tools/list\"}",
        )
        .await;
        assert!(resp.contains("404"), "expected 404, got: {resp}");
        assert!(resp.contains("-32001"), "{resp}");
        assert!(resp.contains("jsonrpc"), "{resp}");
        assert!(resp.contains("\"id\":9"), "{resp}");

        server.set_config(false, 20006).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // ------------------------------------------------------------------
    // optimize_cds: input resolution / sequence cleaning / file output
    // ------------------------------------------------------------------

    #[test]
    fn clean_coding_sequence_strips_junk_and_validates() {
        assert_eq!(
            clean_coding_sequence("atg gtg agc\n1 2 3\ntaa").unwrap(),
            "ATGGTGAGCTAA"
        );
        assert_eq!(clean_coding_sequence("ATG").unwrap(), "ATG");
        assert_eq!(clean_coding_sequence("1atg2").unwrap(), "ATG");
        assert!(clean_coding_sequence("ATGN").is_err()); // ambiguous base
        assert!(clean_coding_sequence("ATGGT").is_err()); // length not %3
        assert!(clean_coding_sequence("").is_err()); // empty
        assert!(clean_coding_sequence("   \n\t ").is_err()); // only junk
    }

    #[test]
    fn resolve_optimize_input_requires_exactly_one_mode() {
        // project mode: no sequence/input_path → feature_id required
        assert!(matches!(
            resolve_optimize_input(None, Some("f1"), None, None),
            Ok(OptimizeInput::Project { feature_id, .. }) if feature_id == "f1"
        ));
        assert!(resolve_optimize_input(None, None, None, None).is_err());
        // sequence mode
        assert!(matches!(
            resolve_optimize_input(None, None, Some("ATG"), None),
            Ok(OptimizeInput::Sequence(_))
        ));
        // file mode with optional feature_id
        assert!(matches!(
            resolve_optimize_input(None, Some("f1"), None, Some("x.gbk")),
            Ok(OptimizeInput::File { feature_id: Some(_), .. })
        ));
        assert!(matches!(
            resolve_optimize_input(None, None, None, Some("x.gbk")),
            Ok(OptimizeInput::File { feature_id: None, .. })
        ));
        // conflicts must error
        assert!(resolve_optimize_input(None, None, Some("ATG"), Some("x.gbk")).is_err());
        assert!(resolve_optimize_input(Some("p1"), None, Some("ATG"), None).is_err());
        assert!(resolve_optimize_input(Some("p1"), None, None, Some("x.gbk")).is_err());
        assert!(resolve_optimize_input(None, Some("f1"), Some("ATG"), None).is_err());
    }

    #[test]
    fn write_optimization_output_rejects_unknown_extension() {
        assert!(write_optimization_output("out.fasta", Some("ATG"), "M", None).is_err());
        assert!(write_optimization_output("out.ab1", Some("ATG"), "M", None).is_err());
        assert!(write_optimization_output("out.txt", Some("ATG"), "M", None).is_err());
        assert!(write_optimization_output("../esc.gbk", Some("ATG"), "M", None).is_err());
    }

    #[test]
    fn write_optimization_output_roundtrips_gbk_and_gpt() {
        let dir = std::env::temp_dir().join(format!("libregene-codon-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let gbk_path = dir.join("out.gbk");
        let written =
            write_optimization_output(gbk_path.to_str().unwrap(), Some("ATGGTGAGCTAA"), "MVS*", None)
                .unwrap();
        assert_eq!(written, gbk_path.to_str().unwrap());
        let parsed = libregene_core::file_io::parse_file(&gbk_path).unwrap();
        assert_eq!(parsed.sequence, "ATGGTGAGCTAA");
        assert_eq!(parsed.molecule_type, "dna");
        assert!(parsed.features.iter().any(|f| f.ftype == "CDS"));

        let gpt_path = dir.join("out.gpt");
        write_optimization_output(gpt_path.to_str().unwrap(), None, "MVS*", None).unwrap();
        let parsed = libregene_core::file_io::parse_file(&gpt_path).unwrap();
        assert_eq!(parsed.molecule_type, "protein");
        assert_eq!(parsed.sequence, "mvs*"); // the gpt writer lower-cases

        std::fs::remove_dir_all(&dir).ok();
    }

    fn test_handler() -> LibreGeneMcp<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        LibreGeneMcp::new(
            app.handle().clone(),
            Arc::new(RwLock::new(ProjectManager::new())),
            Arc::new(RwLock::new(HashMap::new())),
        )
    }

    #[tokio::test]
    async fn optimize_cds_sequence_preview_and_validation() {
        let server = test_handler();
        // happy path: sequence input → optimizedSequence, no projectId
        let req = OptimizeCdsRequest {
            species: "e_coli".to_string(),
            sequence: Some("GAG GAG GAG\nTAA".to_string()),
            ..Default::default()
        };
        let out = server.optimize_cds(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["aa"], "EEE*");
        assert_eq!(v["codonCount"], 4);
        assert_eq!(v["optimizedSequence"], "GAAGAAGAATAA"); // E→GAA, stop→TAA (e_coli best)
        assert!(v.get("projectId").is_none());

        // apply=true without output_path in sequence mode → clear error
        let req = OptimizeCdsRequest {
            species: "e_coli".to_string(),
            sequence: Some("ATGGTGAGCTAA".to_string()),
            apply: Some(true),
            ..Default::default()
        };
        let err = match server.optimize_cds(Parameters(req)).await {
            Err(e) => e,
            Ok(_) => panic!("expected apply validation error"),
        };
        assert!(err.message.contains("apply=true"), "{}", err.message);
        assert!(err.message.contains("output_path"), "{}", err.message);

        // sequence + input_path conflict
        let req = OptimizeCdsRequest {
            species: "e_coli".to_string(),
            sequence: Some("ATG".to_string()),
            input_path: Some("x.gbk".to_string()),
            ..Default::default()
        };
        assert!(server.optimize_cds(Parameters(req)).await.is_err());

        // project mode without feature_id → clear error
        let req = OptimizeCdsRequest {
            species: "e_coli".to_string(),
            ..Default::default()
        };
        let err = match server.optimize_cds(Parameters(req)).await {
            Err(e) => e,
            Ok(_) => panic!("expected missing-feature_id error"),
        };
        assert!(err.message.contains("feature_id"), "{}", err.message);
    }

    #[tokio::test]
    async fn optimize_cds_file_reverse_translates_protein_gpt() {
        let gpt = include_str!("../../backend/test_data/mCherry.gpt");
        let path = std::env::temp_dir().join(format!("libregene-mcp-revtest-{}.gpt", std::process::id()));
        std::fs::write(&path, gpt).unwrap();
        let server = test_handler();
        let req = OptimizeCdsRequest {
            species: "e_coli".to_string(),
            input_path: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let out = server.optimize_cds(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        let aa = v["aa"].as_str().unwrap();
        assert!(aa.starts_with("MVSKGEEDNM"), "aa: {}", aa);
        assert!(aa.ends_with('*'), "aa: {}", aa);
        let dna = v["optimizedSequence"].as_str().unwrap();
        assert_eq!(dna.len(), aa.chars().count() * 3);
        assert!(dna.bytes().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T')));
        assert!(v["message"].as_str().unwrap().contains("Reverse translation"));
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn optimize_cds_sequence_writes_output_file() {
        let out_path = std::env::temp_dir().join(format!("libregene-mcp-outtest-{}.gbk", std::process::id()));
        let server = test_handler();
        let req = OptimizeCdsRequest {
            species: "e_coli".to_string(),
            sequence: Some("ATGGTGAGCTAA".to_string()),
            output_path: Some(out_path.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let out = server.optimize_cds(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["outputPath"], out_path.to_str().unwrap());
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, "ATGGTGAGCTAA");
        assert!(parsed.features.iter().any(|f| f.ftype == "CDS"));
        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn optimize_cds_file_with_feature_writes_optimized_gbk() {
        let dir = std::env::temp_dir().join(format!("libregene-mcp-feattest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.gbk");
        // GAG GAG GAG TAA = EEE*; E's best codon is GAA, so the optimized
        // whole-file sequence (CDS replaced in place) must be GAAGAAGAATAA.
        let project = ProjectData {
            name: "test".to_string(),
            sequence: "GAGGAGGAGTAA".to_string(),
            length: 12,
            topology: "linear".to_string(),
            features: vec![Feature {
                id: "cds".to_string(),
                name: "cds".to_string(),
                start: 0,
                end: 11,
                color: "#60A5FA".to_string(),
                ftype: "CDS".to_string(),
                segments: Vec::new(),
                strand: "+".to_string(),
                notes: String::new(),
                translation: String::new(),
                qualifiers: Vec::new(),
            }],
            ..Default::default()
        };
        libregene_core::file_io::gbk::write_gbk(&project, &src).unwrap();

        let out_path = dir.join("out.gbk");
        let server = test_handler();
        let req = OptimizeCdsRequest {
            species: "e_coli".to_string(),
            input_path: Some(src.to_string_lossy().into_owned()),
            feature_id: Some("cds_0".to_string()), // id rebuilt as {label}_{start} on parse
            output_path: Some(out_path.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let out = server.optimize_cds(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["aa"], "EEE*");
        assert_eq!(v["optimizedSequence"], "GAAGAAGAATAA");
        assert_eq!(v["outputPath"], out_path.to_str().unwrap());
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, "GAAGAAGAATAA");
        assert!(parsed.features.iter().any(|f| f.ftype == "CDS"));
        std::fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------
    // export_subsequence
    // ------------------------------------------------------------------

    /// Deterministic pseudo-random ACGT sequence (unique long substrings).
    fn synthetic_dna(length: usize, mut seed: u64) -> String {
        let mut out = String::with_capacity(length);
        for _ in 0..length {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            out.push(b"ACGT"[(seed >> 33) as usize & 3] as char);
        }
        out
    }

    fn feature(id: &str, name: &str, start: i64, end: i64, strand: &str) -> Feature {
        Feature {
            id: id.to_string(),
            name: name.to_string(),
            start,
            end,
            color: "#60A5FA".to_string(),
            ftype: "CDS".to_string(),
            segments: Vec::new(),
            strand: strand.to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        }
    }

    /// Handler whose project manager holds one project (id = name, active).
    async fn handler_with_project(project: ProjectData) -> LibreGeneMcp<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let id = project.name.clone();
        pm.write().await.load(&id, project);
        LibreGeneMcp::new(
            app.handle().clone(),
            pm,
            Arc::new(RwLock::new(HashMap::new())),
        )
    }

    #[tokio::test]
    async fn export_subsequence_region_writes_gbk_with_translated_features() {
        let seq = synthetic_dna(200, 7);
        let project = ProjectData {
            name: "region_test".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("f1", "gene", 50, 100, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-region-{}.gbk", std::process::id()));
        let req = ExportSubsequenceRequest {
            start: Some(40),
            end: Some(160),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 121);
        assert_eq!(v["outputPath"], out_path.to_str().unwrap());
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, seq[40..=160].to_ascii_uppercase());
        let f = parsed
            .features
            .iter()
            .find(|f| f.name == "gene")
            .expect("overlapping feature carried over");
        assert_eq!((f.start, f.end), (10, 60), "feature translated by -40");
        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn export_subsequence_region_wraps_on_circular() {
        let seq = synthetic_dna(100, 11);
        // Cross-origin feature 95..99 + 0..5 (stored as two segments).
        let mut f = feature("f1", "ori", 95, 5, "+");
        f.segments = vec![
            Segment { start: 95, end: 99, color: None },
            Segment { start: 0, end: 5, color: None },
        ];
        let project = ProjectData {
            name: "circ_test".to_string(),
            sequence: seq.clone(),
            length: 100,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![f],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-circ-{}.gbk", std::process::id()));
        let req = ExportSubsequenceRequest {
            start: Some(90),
            end: Some(9),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["length"], 20);
        let expected = format!("{}{}", &seq[90..], &seq[..=9]);
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, expected);
        // the cross-origin feature becomes one contiguous span 5..16
        let f = parsed
            .features
            .iter()
            .find(|f| f.name == "ori")
            .expect("feature carried over");
        assert_eq!((f.start, f.end), (5, 15));
        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn export_subsequence_feature_joins_segments_5_to_3() {
        let seq = synthetic_dna(100, 13);
        let mut cds = feature("cds", "spliced", 10, 39, "+");
        cds.segments = vec![
            Segment { start: 10, end: 19, color: None },
            Segment { start: 30, end: 39, color: None },
        ];
        let inner = feature("in", "inner", 32, 35, "+");
        let project = ProjectData {
            name: "feat_test".to_string(),
            sequence: seq.clone(),
            length: 100,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![cds, inner],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-feat-{}.gbk", std::process::id()));
        let req = ExportSubsequenceRequest {
            feature_id: Some("cds".to_string()),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 20);
        let expected = format!("{}{}", &seq[10..=19], &seq[30..=39]);
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, expected);
        let exported = parsed
            .features
            .iter()
            .find(|f| f.name == "spliced")
            .expect("exported feature spans the whole sequence");
        assert_eq!((exported.start, exported.end), (0, 19));
        let inner = parsed
            .features
            .iter()
            .find(|f| f.name == "inner")
            .expect("inner feature carried over");
        assert_eq!((inner.start, inner.end), (12, 15));
        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn export_subsequence_minus_strand_feature_is_reverse_complemented() {
        let seq = synthetic_dna(100, 17);
        let project = ProjectData {
            name: "minus_test".to_string(),
            sequence: seq.clone(),
            length: 100,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![
                feature("rev", "repressor", 40, 59, "-"),
                feature("fwd", "promoter", 45, 50, "+"),
            ],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-minus-{}.gbk", std::process::id()));
        let req = ExportSubsequenceRequest {
            feature_id: Some("rev".to_string()),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 20);
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        let expected = libregene_core::utils::reverse_complement(&seq[40..=59]);
        assert_eq!(parsed.sequence, expected);
        let exported = parsed
            .features
            .iter()
            .find(|f| f.name == "repressor")
            .expect("exported feature carried over");
        assert_eq!((exported.start, exported.end), (0, 19));
        assert_eq!(
            exported.strand, ".",
            "plus-strand round-trips as '.' (gbk only encodes '-' via complement)"
        );
        let prom = parsed
            .features
            .iter()
            .find(|f| f.name == "promoter")
            .expect("overlapping plus-strand feature carried over, flipped");
        assert_eq!((prom.start, prom.end), (9, 14));
        assert_eq!(prom.strand, "-", "plus-strand feature flips in a rev-comp export");
        std::fs::remove_file(&out_path).ok();
    }

    /// Regression: a multi-segment minus-strand feature (e.g. spliced CDS)
    /// produced a regionView bbox with start > end before the fix, which
    /// either dropped the regionView digest (linear) or showed a wrong
    /// wrap-around window (circular). The bbox must cover the full span
    /// occupied by the feature on the template, regardless of piece order.
    #[tokio::test]
    async fn export_subsequence_multi_segment_minus_strand_regionview_span() {
        use libregene_core::models::Segment;
        let seq = synthetic_dna(120, 23);
        // Two-segment minus-strand feature: pieces (after resolve) are
        // descending, which is what triggered the original bbox bug.
        let multi_seg = Feature {
            id: "split".to_string(),
            name: "split_cds".to_string(),
            start: 10,
            end: 60,
            color: "#60A5FA".to_string(),
            ftype: "CDS".to_string(),
            segments: vec![
                Segment { start: 40, end: 60, color: None },
                Segment { start: 10, end: 30, color: None },
            ],
            strand: "-".to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        };
        let project = ProjectData {
            name: "multi_minus".to_string(),
            sequence: seq.clone(),
            length: 120,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![multi_seg],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-multi-minus-{}.gbk", std::process::id()));
        let req = ExportSubsequenceRequest {
            feature_id: Some("split".to_string()),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "export should succeed");
        // The headline assertion: regionView must be present (non-null) and
        // describe a span within the feature's real coordinates [10, 60].
        // Before the fix, bbox was (40, 30) → start > end → regionView dropped.
        let region = v["regionView"].as_str().unwrap_or("");
        assert!(
            !region.is_empty(),
            "regionView must not be empty for a multi-segment minus-strand feature (was dropped by bbox bug)"
        );
        std::fs::remove_file(&out_path).ok();
    }

    /// Circular wrap variant: a minus-strand feature whose pieces straddle the
    /// origin ((90, 119) + (0, 20)) must report the wrap window 90..20, not a
    /// full-length min/max span.
    #[tokio::test]
    async fn export_subsequence_wrap_origin_minus_strand_regionview_span() {
        use libregene_core::models::Segment;
        let seq = synthetic_dna(120, 24);
        let wrap_feat = Feature {
            id: "wrap".to_string(),
            name: "wrap_cds".to_string(),
            start: 90,
            end: 20,
            color: "#60A5FA".to_string(),
            ftype: "CDS".to_string(),
            segments: vec![
                Segment { start: 90, end: 119, color: None },
                Segment { start: 0, end: 20, color: None },
            ],
            strand: "-".to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        };
        let project = ProjectData {
            name: "wrap_minus".to_string(),
            sequence: seq.clone(),
            length: 120,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![wrap_feat],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-wrap-minus-{}.gbk", std::process::id()));
        let req = ExportSubsequenceRequest {
            feature_id: Some("wrap".to_string()),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "export should succeed");
        let region = v["regionView"].as_str().unwrap_or("");
        assert!(
            region.contains("REGION: 90..20"),
            "regionView should show the wrap window 90..20, got: {}",
            region.lines().next().unwrap_or("")
        );
        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn export_subsequence_enzyme_fragment_and_explicit_cuts() {
        // "ACGT" repeat has no EcoRI/BamHI recognition sites, so the placed
        // sites are the only ones: EcoRI cuts G^AATTC (cut 41), BamHI G^GATCC
        // (cut 101).
        let mut seq = "ACGT".repeat(50);
        seq.replace_range(40..46, "GAATTC");
        seq.replace_range(100..106, "GGATCC");
        let mut project = ProjectData {
            name: "enz_test".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        libregene_core::enzyme::recompute(&mut project);
        assert_eq!(enzyme_cut_index(&project, "EcoRI", 0).unwrap(), 41);
        assert_eq!(enzyme_cut_index(&project, "BamHI", 0).unwrap(), 101);

        let server = handler_with_project(project.clone()).await;
        let dir = std::env::temp_dir().join(format!("libregene-mcp-export-enz-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let out_path = dir.join("frag.gbk");
        let req = ExportSubsequenceRequest {
            enzyme1: Some("EcoRI".to_string()),
            enzyme2: Some("BamHI".to_string()),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 60, "fragment [41..=100] = 60 bp");
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, seq[41..=100].to_string());

        // explicit cut indices mode (cuts may be given in either order)
        let out_path2 = dir.join("cuts.gbk");
        let req = ExportSubsequenceRequest {
            cut1: Some(70),
            cut2: Some(30),
            output_path: out_path2.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["length"], 40, "[30, 69] = 40 bp");
        let parsed = libregene_core::file_io::parse_file(&out_path2).unwrap();
        assert_eq!(parsed.sequence, seq[30..=69].to_string());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn export_subsequence_primer_amplicon() {
        let seq = synthetic_dna(200, 19);
        let project = ProjectData {
            name: "amp_test".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let dir = std::env::temp_dir().join(format!("libregene-mcp-export-amp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // raw primer sequences
        let out_path = dir.join("amp.gbk");
        let req = ExportSubsequenceRequest {
            fwd_primer: Some(seq[50..70].to_string()),
            rev_primer: Some(libregene_core::utils::reverse_complement(&seq[100..120])),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 70, "amplicon [50, 119]");
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, seq[50..=119].to_string());

        // same amplicon via stored project primers (name lookup path)
        let fwd = Primer {
            id: "F1".to_string(),
            name: "F1".to_string(),
            r#type: "fwd".to_string(),
            primer_seq: seq[50..70].to_string(),
            binding_sites: Vec::new(),
        };
        let rev = Primer {
            id: "R1".to_string(),
            name: "R1".to_string(),
            r#type: "rev".to_string(),
            primer_seq: libregene_core::utils::reverse_complement(&seq[100..120]),
            binding_sites: Vec::new(),
        };
        let mut project = ProjectData {
            name: "amp_name_test".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            primers: vec![fwd, rev],
            ..Default::default()
        };
        libregene_core::primer::recompute(&mut project);
        let server = handler_with_project(project).await;
        let out_path2 = dir.join("amp-name.gbk");
        let req = ExportSubsequenceRequest {
            fwd_primer: Some("F1".to_string()),
            rev_primer: Some("R1".to_string()),
            output_path: out_path2.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        assert_eq!(out.0["length"], 70);
        let parsed = libregene_core::file_io::parse_file(&out_path2).unwrap();
        assert_eq!(parsed.sequence, seq[50..=119].to_string());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn export_subsequence_circular_primer_amplicon_wraps_origin() {
        let seq = synthetic_dna(200, 23);
        let project = ProjectData {
            name: "amp_circ".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let dir = std::env::temp_dir().join(format!("libregene-mcp-export-ampc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("amp.gbk");
        // fwd primer sits at the very end (188..200, its site wraps the origin),
        // rev primer at 30..45: the amplicon wraps 188..199 + 0..44.
        let req = ExportSubsequenceRequest {
            fwd_primer: Some(seq[188..200].to_string()),
            rev_primer: Some(libregene_core::utils::reverse_complement(&seq[30..45])),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 57, "12 bp (188..199) + 45 bp (0..44)");
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        let expected = format!("{}{}", &seq[188..], &seq[..=44]);
        assert_eq!(parsed.sequence, expected);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn export_subsequence_protein_writes_gpt() {
        let aa = "MVSKGEEDNMAAEF".to_string();
        let project = ProjectData {
            name: "prot_test".to_string(),
            sequence: aa.clone(),
            length: aa.len() as i64,
            topology: "linear".to_string(),
            molecule_type: "protein".to_string(),
            features: vec![feature("prot", "mCherry", 0, (aa.len() - 1) as i64, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let dir = std::env::temp_dir().join(format!("libregene-mcp-export-prot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let out_path = dir.join("out.gpt");
        let req = ExportSubsequenceRequest {
            start: Some(0),
            end: Some((aa.len() - 1) as i64),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        assert_eq!(out.0["ok"], true);
        assert_eq!(out.0["length"], aa.len() as i64);
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.molecule_type, "protein");
        assert_eq!(parsed.sequence, aa.to_lowercase(), "the gpt writer lower-cases");

        // protein project must not go to a .gbk path
        let req = ExportSubsequenceRequest {
            start: Some(0),
            end: Some(3),
            output_path: dir.join("out.gbk").to_string_lossy().into_owned(),
            ..Default::default()
        };
        let out = server.export_subsequence(Parameters(req)).await.unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(out.0["message"].as_str().unwrap().contains(".gpt"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn export_subsequence_rejects_bad_requests() {
        let seq = synthetic_dna(100, 29);
        let project = ProjectData {
            name: "bad_test".to_string(),
            sequence: seq.clone(),
            length: 100,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("f1", "gene", 10, 50, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-bad-{}.gbk", std::process::id()));
        let expect_err = |req: ExportSubsequenceRequest| async {
            match server.export_subsequence(Parameters(req)).await {
                Err(e) => e.message.into_owned(),
                Ok(v) => v.0["message"].as_str().unwrap_or("").to_string(),
            }
        };

        // multiple selectors
        let msg = expect_err(ExportSubsequenceRequest {
            start: Some(0),
            end: Some(9),
            feature_id: Some("f1".to_string()),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await;
        assert!(msg.contains("exactly one region selector"), "{}", msg);

        // start without end
        let msg = expect_err(ExportSubsequenceRequest {
            start: Some(0),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await;
        assert!(msg.contains("both required"), "{}", msg);

        // linear start > end
        let msg = expect_err(ExportSubsequenceRequest {
            start: Some(50),
            end: Some(10),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await;
        assert!(msg.contains("only allowed on circular"), "{}", msg);

        // unknown feature
        let msg = expect_err(ExportSubsequenceRequest {
            feature_id: Some("nope".to_string()),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await;
        assert!(msg.contains("Feature not found"), "{}", msg);

        // unknown enzyme
        let msg = expect_err(ExportSubsequenceRequest {
            enzyme1: Some("EcoRI".to_string()),
            enzyme2: Some("NotARealEnzyme".to_string()),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await;
        assert!(msg.contains("Unknown enzyme"), "{}", msg);

        // mixed fragment selectors
        let msg = expect_err(ExportSubsequenceRequest {
            enzyme1: Some("EcoRI".to_string()),
            cut1: Some(10),
            cut2: Some(20),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await;
        assert!(msg.contains("not a mix"), "{}", msg);

        // equal cuts on a linear sequence
        let msg = expect_err(ExportSubsequenceRequest {
            cut1: Some(10),
            cut2: Some(10),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await;
        assert!(msg.contains("equal"), "{}", msg);

        // primer that is neither a name nor a sequence
        let msg = expect_err(ExportSubsequenceRequest {
            fwd_primer: Some("!!!".to_string()),
            rev_primer: Some(seq[20..40].to_string()),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await;
        assert!(msg.contains("neither a primer name"), "{}", msg);

        // primer that only binds the reverse strand in the fwd role
        let msg = expect_err(ExportSubsequenceRequest {
            fwd_primer: Some(libregene_core::utils::reverse_complement(&seq[20..40])),
            rev_primer: Some(libregene_core::utils::reverse_complement(&seq[50..70])),
            output_path: out_path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await;
        assert!(msg.contains("does not bind the forward strand"), "{}", msg);

        // DNA project must not go to a .gpt path
        let msg = expect_err(ExportSubsequenceRequest {
            start: Some(0),
            end: Some(9),
            output_path: out_path.to_string_lossy().replace("bad", "bad2").replace(".gbk", ".gpt"),
            ..Default::default()
        })
        .await;
        assert!(msg.contains(".gpt"), "{}", msg);

        std::fs::remove_file(&out_path).ok();
    }

    // ------------------------------------------------------------------
    // edit_sequence: replacement from file
    // ------------------------------------------------------------------

    fn edit_test_project() -> ProjectData {
        let seq = synthetic_dna(200, 42);
        ProjectData {
            name: "edit_test".to_string(),
            sequence: seq,
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("f1", "gene", 50, 100, "+")],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn edit_sequence_replacement_from_fasta_file() {
        let server = handler_with_project(edit_test_project()).await;
        let insert = "AAACCCGGGTTT";
        let fasta = std::env::temp_dir()
            .join(format!("libregene-mcp-edit-ins-{}.fasta", std::process::id()));
        std::fs::write(&fasta, format!(">insert\n{}\n", insert)).unwrap();

        // pure insertion at position 60 via replacement_path
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 60,
                end: 59,
                replacement_path: Some(fasta.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["newLength"], 212);

        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(p.sequence.len(), 212);
        assert_eq!(&p.sequence[60..72], insert);
        // feature 50..100 spans the insertion point → end shifted by 12
        let f = p.features.iter().find(|f| f.name == "gene").unwrap();
        assert_eq!((f.start, f.end), (50, 112));
        drop(pm);
        std::fs::remove_file(&fasta).ok();
    }

    #[tokio::test]
    async fn edit_sequence_replacement_input_validation() {
        let server = handler_with_project(edit_test_project()).await;

        // both replacement and replacement_path → error
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 10,
                end: 20,
                replacement: Some("ACGT".to_string()),
                replacement_path: Some("x.fasta".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(out.0["message"].as_str().unwrap().contains("exactly one"), "{}", out.0);

        // neither → error
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 10,
                end: 20,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(out.0["message"].as_str().unwrap().contains("exactly one"), "{}", out.0);

        // bad extension → hard error from validate_user_path
        let res = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 10,
                end: 20,
                replacement_path: Some("notes.txt".to_string()),
                ..Default::default()
            }))
            .await;
        assert!(res.is_err(), "txt path must be rejected");

        // unreadable/missing file → fail envelope
        let missing = std::env::temp_dir()
            .join(format!("libregene-mcp-edit-missing-{}.fasta", std::process::id()));
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 10,
                end: 20,
                replacement_path: Some(missing.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("Failed to read replacement"),
            "{}",
            out.0
        );

        // sequence must be untouched after all these failures
        let pm = server.pm.read().await;
        assert_eq!(pm.get_project_by_id("edit_test").unwrap().sequence.len(), 200);
    }

    #[tokio::test]
    async fn edit_sequence_string_replacement_still_works() {
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 10,
                end: 19,
                replacement: Some("TT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["newLength"], 192);
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(&p.sequence[10..12], "TT");
    }

    // ------------------------------------------------------------------
    // Molecule-type gates
    // ------------------------------------------------------------------

    fn protein_test_project() -> ProjectData {
        ProjectData {
            name: "prot".to_string(),
            sequence: "MVSKGEEDNM".repeat(5),
            length: 50,
            topology: "linear".to_string(),
            molecule_type: "protein".to_string(),
            features: vec![feature("f1", "mCherry", 0, 49, "+")],
            ..Default::default()
        }
    }

    fn rna_test_project() -> ProjectData {
        ProjectData {
            name: "rna".to_string(),
            sequence: "ACGU".repeat(25),
            length: 100,
            topology: "linear".to_string(),
            molecule_type: "rna".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn dna_only_tools_reject_protein_and_rna_projects() {
        let server = handler_with_project(protein_test_project()).await;

        let err = match server
            .search_sequence(Parameters(SearchRequest {
                project_id: Some("prot".to_string()),
                query: "ACG".to_string(),
            }))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("search_sequence should reject a protein project"),
        };
        assert!(err.message.contains("only supports DNA"), "{}", err.message);

        let err = match server
            .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
                project_id: Some("prot".to_string()),
                enzymes: None,
            }))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("find_restriction_sites should reject a protein project"),
        };
        assert!(err.message.contains("only supports DNA"), "{}", err.message);

        // RNA projects are gated the same way
        let server = handler_with_project(rna_test_project()).await;
        let err = match server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: Some("rna".to_string()),
                primers: vec![PrimerInput {
                    name: "p1".to_string(),
                    r#type: "fwd".to_string(),
                    seq: "ACGTACGTAC".to_string(),
                }],
            }))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("check_primer_binding should reject an rna project"),
        };
        assert!(err.message.contains("only supports DNA"), "{}", err.message);
    }

    #[tokio::test]
    async fn optimize_cds_project_mode_rejects_protein_project() {
        let server = handler_with_project(protein_test_project()).await;
        let req = OptimizeCdsRequest {
            project_id: Some("prot".to_string()),
            feature_id: Some("f1".to_string()),
            species: "e_coli".to_string(),
            ..Default::default()
        };
        let err = match server.optimize_cds(Parameters(req)).await {
            Err(e) => e,
            Ok(_) => panic!("expected protein project-mode rejection"),
        };
        assert!(
            err.message.contains("input_path") && err.message.contains("reverse-translated"),
            "{}",
            err.message
        );
    }

    #[tokio::test]
    async fn edit_sequence_protein_uppercases_and_validates_alphabet() {
        let server = handler_with_project(protein_test_project()).await;
        // lowercase replacement is normalized to uppercase and stored as-is
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 0,
                end: 3,
                replacement: Some("mvs*".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert!(
            v["message"].as_str().unwrap().contains("aa"),
            "message should use aa units: {}",
            v["message"]
        );
        let pm = server.pm.read().await;
        assert_eq!(&pm.get_project_by_id("prot").unwrap().sequence[0..4], "MVS*");

        // non-amino-acid characters are rejected
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 5,
                end: 8,
                replacement: Some("MVS1".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("amino-acid"),
            "{}",
            out.0
        );

        // a '*' anywhere but the end is rejected too
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 5,
                end: 8,
                replacement: Some("M*VS".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        // the failed edits must not have mutated the sequence
        let pm = server.pm.read().await;
        assert_eq!(&pm.get_project_by_id("prot").unwrap().sequence[5..9], "EEDN");
    }

    #[tokio::test]
    async fn get_project_overview_protein_omits_dna_sections() {
        let server = handler_with_project(protein_test_project()).await;
        let out = server
            .get_project_overview(Parameters(OverviewRequest {
                project_id: Some("prot".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap();
        assert!(text.contains("50 aa"), "overview: {text}");
        assert!(!text.contains("PRIMERS"), "overview: {text}");
        assert!(!text.contains("ENZYMES"), "overview: {text}");
        // Auto-annotation runs on protein projects (aa-level CDS matching);
        // this synthetic 50 aa sequence matches nothing.
        assert!(
            text.contains("DETECTED COMMON FEATURES (auto):\n(none)"),
            "overview: {text}"
        );
    }
}
