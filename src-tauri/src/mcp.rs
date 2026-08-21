//! Embedded MCP (Model Context Protocol) server for LibreGene.
//!
//! Exposes tools over Streamable HTTP on `127.0.0.1:8766` so an external LLM
//! agent can operate the app like a real user. Mutations go through the same
//! shared cores as the Tauri commands (`crate::do_*`), so recompute, dirty
//! marking and `broadcast_project()` behave identically and the UI updates
//! live. Every mutation tool returns a uniform `{ok, message, projectId,
//! regionView?}` envelope (plus tool-specific fields).
//!
//! Coordinate conventions (stated again in every tool description). This MCP
//! layer is agent-facing, so all coordinates in tool inputs and outputs are
//! **1-based inclusive** (the GenBank convention); the internal model and the
//! shared `crate::do_*` cores stay **0-based inclusive**, and this module
//! converts at the boundary (`to1`/`from1`):
//! - internal inclusive [s, e] ↔ interface [s+1, e+1]
//! - a primer site's internal 0-based-EXCLUSIVE `template_end` equals the
//!   1-based inclusive end of the site, so its value crosses the boundary
//!   unchanged (only `template_start` shifts by one)
//! - an enzyme cut at internal 0-based index C (severing between bases C-1
//!   and C) is described as "between the 1-based bases C and C+1" and rendered
//!   `C^C+1` (`cut_notation`; a cut at the origin of a circular molecule is
//!   `len^1`)
//! - a pure insertion into `edit_sequence` before base N is `start=N,
//!   end=N-1`; ranges must not wrap
//! - circular sequences allow `start > end` to wrap the origin for reads
//!   (values are 1-based)

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

use libregene_core::digest::{DigestOptions, cut_flanks, cut_notation, project_digest, read_sequence};
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
    /// Window start, 1-based inclusive; on circular sequences start > end
    /// wraps the origin.
    start: i64,
    /// Window end, 1-based inclusive.
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
    /// Window start, 1-based inclusive; on circular sequences start > end
    /// wraps the origin.
    start: i64,
    /// Window end, 1-based inclusive.
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
struct RequestAgentWindowRequest {
    /// Project to bind the agent window to; defaults to the active project.
    project_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct EditSequenceRequest {
    project_id: Option<String>,
    /// First base of the replaced range, 1-based inclusive. Ranges must not
    /// wrap; a pure insertion before base N is start=N, end=N-1.
    start: i64,
    /// Last base of the replaced range, 1-based inclusive (>= start-1).
    end: i64,
    /// Replacement sequence as a plain string (empty = delete). Exactly one
    /// of `replacement` / `replacement_path` must be given. Use this ONLY for
    /// short hand-authored edits (point mutations, short oligo-length
    /// inserts); for anything longer or taken from an existing file or open
    /// project, use `replacement_path` instead (export the region first with
    /// export_subsequence if needed) — pasted long sequences are error-prone.
    replacement: Option<String>,
    /// PREFERRED input: read the replacement sequence from a local file
    /// (.gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 etc., same formats as
    /// open_file). A file cannot be mistyped or truncated, so use it whenever
    /// the sequence exists on disk.
    replacement_path: Option<String>,
    /// Direction of the inserted replacement: "+" (default — insert exactly
    /// as given) or "-" (reverse-complement the replacement before inserting,
    /// e.g. when the source sequence is oriented on the opposite strand).
    /// DNA projects only; rejected on RNA/protein projects.
    strand: Option<String>,
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
    /// fine). Use ONLY for short hand-authored sequences; for anything from a
    /// file or an open project use `input_path` (export regions first with
    /// export_subsequence) — pasted long sequences are error-prone.
    sequence: Option<String>,
    /// Standalone mode (PREFERRED for real sequences): local sequence file (.gbk/.gb/.genbank/.dna/.rna/
    /// .fasta/.fa/.fna/.ab1 — DNA) or protein file (.gpt/.prot — reverse
    /// translation). A file cannot be mistyped or truncated.
    input_path: Option<String>,
    /// Optional: write the result to a file. .gbk/.gb/.genbank → DNA GenBank
    /// with the optimized CDS annotated; .gpt → protein GenBank of the
    /// translated sequence. PREFERRED way to collect the result — use the file
    /// (open_file afterwards) rather than copying the `optimizedSequence` text.
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
struct FeatureSegmentSpec {
    /// Segment start, 1-based inclusive.
    start: i64,
    /// Segment end, 1-based inclusive (must be >= start).
    end: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AddFeatureRequest {
    project_id: Option<String>,
    name: String,
    ftype: String,
    /// Feature start, 1-based inclusive. Required together with `end` unless
    /// `segments` is given; mutually exclusive with `segments`.
    start: Option<i64>,
    /// Feature end, 1-based inclusive (>= start). See `start`.
    end: Option<i64>,
    /// Segmented feature (e.g. multi-exon CDS): [{start, end}] 1-based
    /// inclusive, in 5'→3' order. Mutually exclusive with `start`/`end`.
    segments: Option<Vec<FeatureSegmentSpec>>,
    /// ".", "+" or "-" (default "+").
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
    /// New start, 1-based inclusive; must be given together with `end` and is
    /// mutually exclusive with `segments`.
    start: Option<i64>,
    /// New end, 1-based inclusive (>= start). See `start`.
    end: Option<i64>,
    /// New segments [{start, end}] 1-based inclusive (5'→3' order); mutually
    /// exclusive with `start`/`end`.
    segments: Option<Vec<FeatureSegmentSpec>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AddPrimerRequest {
    project_id: Option<String>,
    name: String,
    #[serde(rename = "type")]
    r#type: String,
    /// Primer sequence as plain text (short, ~20-60 nt — intended input form).
    seq: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AddAlignmentRequest {
    project_id: Option<String>,
    name: String,
    /// Read sequence as a plain string — short hand-authored reads only;
    /// prefer `path` (a file cannot be mistyped or truncated).
    #[serde(alias = "seq")]
    bases: Option<String>,
    /// PREFERRED input: read the sequence from a file (.gbk/.gb/.genbank,
    /// .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1). If the read is a region of an
    /// open project, export it first with export_subsequence.
    path: Option<String>,
    /// When true, omit the full `orientedSequence` and the post-alignment
    /// `regionView` to reduce response size. Differences and coverage are still
    /// returned; use read_sequence/get_region_view when you need the bases.
    compact: Option<bool>,
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
    /// Segment start, 1-based inclusive (start > end wraps the origin on
    /// circular sequences).
    start: i64,
    /// Segment end, 1-based inclusive.
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
    /// Primer sequence as plain text (short, ~20-60 nt — intended input form).
    seq: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct CheckPrimerBindingRequest {
    project_id: Option<String>,
    primers: Vec<PrimerInput>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ConvertCoordinatesRequest {
    project_id: Option<String>,
    /// Full-file template coordinate (1-based inclusive). Mutually exclusive
    /// with feature_id + feature_offset and feature_id + aa_position.
    position: Option<i64>,
    /// Feature ID for feature-relative or amino-acid lookups. Must be paired
    /// with exactly one of `feature_offset` or `aa_position`.
    feature_id: Option<String>,
    /// 1-based offset along the feature's own 5'→3' direction. Mutually
    /// exclusive with `position` and `aa_position`.
    feature_offset: Option<i64>,
    /// 1-based amino-acid position within a CDS/mRNA feature — INCLUDING the
    /// initiator Met (Met = 1). Literature numbering that skips the Met (e.g.
    /// mEGFP A206K) maps to the response's `aaPositionExcludingMet`, not to
    /// this input. Mutually exclusive with `position` and `feature_offset`.
    aa_position: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ExportSubsequenceRequest {
    /// Project to export from (defaults to the active project).
    project_id: Option<String>,
    /// Required: output file path (.gbk/.gb/.genbank for DNA/RNA projects,
    /// .gpt for protein projects).
    output_path: String,
    /// Region mode: start of the export window, 1-based inclusive.
    start: Option<i64>,
    /// Region mode: end of the export window, 1-based inclusive (start > end
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
    /// Fragment mode (explicit cuts): first cut position — a cut at N severs
    /// the DNA between the 1-based bases N and N+1 (N = len: after the last
    /// base on linear, between the last and the first base on circular).
    cut1: Option<i64>,
    /// Fragment mode (explicit cuts): second cut position (same convention).
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
    agent_windows: crate::AgentWindows,
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

// ---------------------------------------------------------------------------
// Coordinate conversion: the MCP interface is 1-based inclusive, the internal
// model 0-based inclusive. All boundary crossings go through these helpers.
// ---------------------------------------------------------------------------

/// Internal 0-based inclusive coordinate → MCP-visible 1-based inclusive.
fn to1(x: i64) -> i64 {
    x + 1
}

/// MCP-visible 1-based inclusive coordinate → internal 0-based inclusive.
fn from1(x: i64) -> i64 {
    x - 1
}

/// Window-label sanitizer: keep only `[A-Za-z0-9-_]`; every other character
/// (path separators, '.', spaces, parentheses, ...) becomes '_' so a file
/// path never produces an invalid Tauri window label.
pub(crate) fn sanitize_window_label(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Serialize a feature for an MCP response with its coordinates bumped to
/// 1-based inclusive (the model stores 0-based inclusive).
fn feature_json_1based(f: &Feature) -> serde_json::Value {
    let mut v = serde_json::to_value(f).unwrap_or_default();
    v["start"] = serde_json::json!(to1(f.start));
    v["end"] = serde_json::json!(to1(f.end));
    if let Some(segs) = v.get_mut("segments").and_then(|s| s.as_array_mut()) {
        for seg in segs.iter_mut() {
            if let Some(s) = seg.get("start").and_then(|x| x.as_i64()) {
                seg["start"] = serde_json::json!(s + 1);
            }
            if let Some(e) = seg.get("end").and_then(|x| x.as_i64()) {
                seg["end"] = serde_json::json!(e + 1);
            }
        }
    }
    v
}

/// Convert a binding-site JSON object coming from a `crate::do_*` core
/// (0-based `templateStart`, 0-based-EXCLUSIVE `templateEnd`) to the 1-based
/// inclusive MCP convention: `templateStart` +1, while `templateEnd` keeps its
/// value (a 0-based exclusive end IS the 1-based inclusive end of the site).
fn site_json_to_1based(site: &mut serde_json::Value) {
    if let Some(s) = site.get("templateStart").and_then(|v| v.as_i64()) {
        site["templateStart"] = serde_json::json!(s + 1);
    }
}

/// Per-alignment JSON for add_alignment responses, with every template
/// coordinate converted to 1-based inclusive. An insertion at internal
/// 0-based `pos` (extra read bases before base `pos`) is reported as the
/// 1-based base BEFORE the break — the bases sit between `pos` and `pos + 1`
/// (`pos = len` on circular templates means between the last and the first
/// base).
fn alignment_json_1based(
    a: &libregene_core::models::Alignment,
    template: &str,
    tlen: i64,
    circular: bool,
    compact: bool,
) -> serde_json::Value {
    let diff = libregene_core::align::alignment_diff(a, template);
    let mut v = serde_json::json!({
        "alignmentId": a.id,
        "name": a.name,
        "identity": a.identity,
        "strand": a.strand,
        "segmentCount": a.segments.len(),
        "alignedLength": diff.aligned_length,
        "mismatches": diff.mismatches.len(),
        "insertions": diff.insertions.iter().map(|i| i.length).sum::<usize>(),
        "deletions": diff.deletions.iter().map(|d| d.length).sum::<usize>(),
        "mismatchDetails": diff.mismatches.iter().map(|m| serde_json::json!({
            "pos": m.pos + 1,
            "templateBase": m.template_base,
            "readBase": m.read_base,
        })).collect::<Vec<_>>(),
        "deletionDetails": diff.deletions.iter().map(|d| serde_json::json!({
            "pos": d.pos + 1,
            "length": d.length,
            "bases": d.bases,
        })).collect::<Vec<_>>(),
        "insertionDetails": diff.insertions.iter().map(|i| serde_json::json!({
            "pos": cut_flanks(i.pos as i64, tlen, circular).0,
            "bases": i.bases,
            "length": i.length,
        })).collect::<Vec<_>>(),
        "coverage": a.segments.iter().map(|s| serde_json::json!({
            "start": s.start + 1,
            "end": s.end + 1,
        })).collect::<Vec<_>>(),
    });
    if !compact {
        v["orientedSequence"] = serde_json::json!(a.seq);
    }
    v
}

/// Stats-only per-alignment JSON (no orientedSequence, no mismatch/deletion/
/// insertion details) — used for every alignment EXCEPT the one just added,
/// so multi-read responses stay small.
fn alignment_stats_json_1based(
    a: &libregene_core::models::Alignment,
    template: &str,
    tlen: i64,
    circular: bool,
) -> serde_json::Value {
    let mut v = alignment_json_1based(a, template, tlen, circular, true);
    if let Some(obj) = v.as_object_mut() {
        obj.remove("mismatchDetails");
        obj.remove("deletionDetails");
        obj.remove("insertionDetails");
    }
    v
}

/// Total template columns not covered by any segment, summed over the gaps
/// between consecutive segments. 0 for single-segment reads and for
/// origin-spanning circular reads whose segments are adjacent at the wrap.
fn uncovered_between_segments(a: &libregene_core::models::Alignment, tlen: usize, circular: bool) -> usize {
    let mut total = 0usize;
    for i in 0..a.segments.len().saturating_sub(1) {
        let end = a.segments[i].end as i64;
        let start = a.segments[i + 1].start as i64;
        let gap = if circular {
            (start - end - 1).rem_euclid(tlen as i64)
        } else {
            start - end - 1
        };
        if gap > 0 {
            total += gap as usize;
        }
    }
    total
}

/// `removedFeatures`/`clippedFeatures` echo for edit_sequence, 1-based
/// inclusive (the impact model is internal 0-based).
fn edit_impact_json(
    impact: &libregene_core::models::FeaturesEditImpact,
) -> (serde_json::Value, serde_json::Value) {
    let removed: Vec<serde_json::Value> = impact
        .removed_features
        .iter()
        .map(|r| {
            let loc = match r.location.split_once("..") {
                Some((a, b)) => match (a.parse::<i64>(), b.parse::<i64>()) {
                    (Ok(s), Ok(e)) => format!("{}..{}", s + 1, e + 1),
                    _ => r.location.clone(),
                },
                None => r.location.clone(),
            };
            serde_json::json!({"name": r.name, "ftype": r.ftype, "location": loc})
        })
        .collect();
    let clipped: Vec<serde_json::Value> = impact
        .clipped_features
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "ftype": c.ftype,
                "before": {"start": to1(c.before.start), "end": to1(c.before.end)},
                "after": {"start": to1(c.after.start), "end": to1(c.after.end)},
            })
        })
        .collect();
    (serde_json::json!(removed), serde_json::json!(clipped))
}

/// analyze_mutagenesis reports internal 0-based coordinates; bump the
/// template span, the diff offsets and the CDS codon index to the 1-based
/// inclusive MCP convention. `codonIndex` becomes 1-based within the CDS
/// (then equal to `aaPosition1Based`); `aaPosition1Based`/
/// `aaPositionExcludingMet` are amino-acid numbering (already 1-based
/// conventions) and stay untouched.
fn mutagenesis_json_1based(info: &libregene_core::primer::design::MutagenesisAnalysis) -> serde_json::Value {
    let mut v = serde_json::to_value(info).unwrap_or_default();
    v["segStart"] = serde_json::json!(to1(info.seg_start));
    v["segEnd"] = serde_json::json!(to1(info.seg_end));
    if let Some(diffs) = v.get_mut("diffs").and_then(|d| d.as_array_mut()) {
        for d in diffs.iter_mut() {
            if let Some(o) = d.get("offset").and_then(|x| x.as_i64()) {
                d["offset"] = serde_json::json!(o + 1);
            }
        }
    }
    if let Some(cds) = v.get_mut("cds") {
        if let Some(ci) = cds.get("codonIndex").and_then(|x| x.as_i64()) {
            cds["codonIndex"] = serde_json::json!(ci + 1);
        }
    }
    v
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

/// Stored feature coordinates rendered GenBank-style in the 1-based inclusive
/// MCP convention (e.g. "100..200", "complement(50..80)", "join(1..100,200..300)").
fn stored_location(f: &Feature) -> String {
    let segs: Vec<(i64, i64)> = if f.segments.is_empty() {
        vec![(f.start, f.end)]
    } else {
        f.segments.iter().map(|s| (s.start, s.end)).collect()
    };
    let inner = segs
        .iter()
        .map(|(s, e)| format!("{}..{}", s + 1, e + 1))
        .collect::<Vec<_>>()
        .join(",");
    let loc = if segs.len() > 1 { format!("join({})", inner) } else { inner };
    if f.strand == "-" {
        format!("complement({})", loc)
    } else {
        loc
    }
}

/// Resolve add_feature/update_feature span parameters (given 1-based
/// inclusive, the MCP interface convention) into model segments and overall
/// bounds (internal 0-based inclusive). Bounds against the project length are
/// checked by the caller.
fn resolve_feature_span(
    start: Option<i64>,
    end: Option<i64>,
    segments: Option<Vec<FeatureSegmentSpec>>,
) -> Result<(Vec<Segment>, i64, i64), String> {
    if segments.is_some() && (start.is_some() || end.is_some()) {
        return Err("segments is mutually exclusive with start/end".to_string());
    }
    match (start, end, segments) {
        (Some(s), Some(e), None) => {
            if s < 1 || e < s {
                return Err(format!(
                    "invalid span {}..{}: need 1 <= start <= end (1-based inclusive)",
                    s, e
                ));
            }
            Ok((vec![Segment { start: s - 1, end: e - 1, color: None }], s - 1, e - 1))
        }
        (None, None, Some(segs)) => {
            if segs.is_empty() {
                return Err("segments must not be empty".to_string());
            }
            let mut out = Vec::with_capacity(segs.len());
            for seg in &segs {
                if seg.start < 1 || seg.end < seg.start {
                    return Err(format!(
                        "invalid segment {}..{}: need 1 <= start <= end (1-based inclusive)",
                        seg.start, seg.end
                    ));
                }
                out.push(Segment { start: seg.start - 1, end: seg.end - 1, color: None });
            }
            let s = out.iter().map(|x| x.start).min().unwrap_or(0);
            let e = out.iter().map(|x| x.end).max().unwrap_or(0);
            Ok((out, s, e))
        }
        (Some(_), None, None) | (None, Some(_), None) => {
            Err("start and end must be given together".to_string())
        }
        (None, None, None) => Err("give start+end or segments".to_string()),
        _ => Err("invalid span parameters".to_string()),
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
        agent_windows: crate::AgentWindows,
    ) -> Self {
        Self { app_handle, pm, wp, agent_windows }
    }

    /// Explicit project id or the active project. Resolving a project also
    /// re-locks any agent window bound to it — the user may unlock the window,
    /// but the next tool call on the project locks it again.
    async fn resolve_project_id(&self, project_id: Option<String>) -> Result<String, ErrorData> {
        let id = {
            let pm = self.pm.read().await;
            match project_id {
                Some(id) => id,
                None => pm.active_id().map(|s| s.to_string()).ok_or_else(|| {
                    ErrorData::invalid_params("No project loaded — open a file or pass project_id", None)
                })?,
            }
        };
        crate::lock_agent_windows_for_project(&self.app_handle, &self.agent_windows, &id).await;
        Ok(id)
    }

    /// Resolve the project id and clone its data out of the lock.
    async fn resolve_project(&self, project_id: Option<String>) -> Result<(String, ProjectData), ErrorData> {
        let (id, project) = {
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
            (id, project)
        };
        crate::lock_agent_windows_for_project(&self.app_handle, &self.agent_windows, &id).await;
        Ok((id, project))
    }

    /// Mutating tools may only operate on projects bound to an agent window,
    /// so the user keeps an untouched main window. Read-only tools are
    /// unrestricted; `request_agent_window` performs the binding.
    async fn require_agent_window(&self, project_id: &str) -> Result<(), ErrorData> {
        let aw = self.agent_windows.read().await;
        if aw.values().any(|m| m.project_id == project_id) {
            return Ok(());
        }
        Err(ErrorData::invalid_params(
            format!(
                "Project '{}' is not bound to an agent window. Call request_agent_window first: it moves the project into a dedicated window that is locked against user input while you work. Mutating tools refuse to operate on projects shown in the user's own windows.",
                project_id
            ),
            None,
        ))
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

    /// Text digest of `region` (internal 0-based inclusive, may wrap on
    /// circular) or the whole project when `None`. `compact` collapses the
    /// enzyme cut list into a count line (mutation tools use it to keep
    /// regionView small). Rendered coordinates are 1-based inclusive.
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

/// Linear template pieces for an internal 0-based inclusive region; on
/// circular sequences a wrap (start > end) becomes two pieces.
fn region_pieces(project: &ProjectData, s: i64, e: i64) -> Vec<(i64, i64)> {
    if project.topology == "circular" && s > e {
        vec![(s, project.length - 1), (0, e)]
    } else {
        vec![(s, e)]
    }
}

/// The fragment between two cuts (internal 0-based; a cut at index C severs
/// the DNA between bases C-1 and C). Linear: the span between the smaller and
/// the larger cut ([lo, hi-1]). Circular: the forward arc from cut1 to cut2,
/// wrapping over the origin when cut1 > cut2, the whole molecule when they
/// coincide.
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

/// The top-strand cut index (internal 0-based) of an enzyme's `ordinal`-th
/// recognition site (sorted by rec_start) from the already-computed engine
/// results. Unknown enzymes error with near-match suggestions, mirroring
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
                    "Enzyme '{}' has only {} recognition site(s) on this sequence; cannot select site number {}",
                    name,
                    hits.len(),
                    ordinal + 1
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
/// linear internal 0-based inclusive spans in EXPORT order, `flip` (each
/// piece's sequence is reverse-complemented when exporting a minus-strand
/// feature) and a human-readable description of the selected region (1-based,
/// like every agent-facing string). The request's region `start`/`end` are
/// already converted to internal 0-based by the caller; `cut1`/`cut2` are
/// still the raw 1-based flanking-base numbers and are converted here.
/// Exactly one selector must be given; mixing selectors is rejected.
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
            _ => return Err("start and end are both required (1-based inclusive)".to_string()),
        };
        if s > e && !circular {
            return Err(
                "start > end is only allowed on circular sequences (wraps the origin)".to_string(),
            );
        }
        if s < 0 || e < 0 || s >= len || e >= len {
            return Err(format!(
                "range {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                s + 1,
                e + 1,
                len
            ));
        }
        return Ok((
            region_pieces(project, s, e),
            false,
            format!("region {}..{}", s + 1, e + 1),
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
                    "feature {} coordinate {}..{} out of range for sequence of length {} (1-based inclusive)",
                    f.id,
                    s + 1,
                    e + 1,
                    len
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
                        e1,
                        cut_notation(c1, len, circular),
                        e2,
                        cut_notation(c2, len, circular)
                    ),
                )
            }
            (None, None, Some(a), Some(b)) => {
                // 1-based input: a cut at N severs the DNA between the 1-based
                // bases N and N+1. An internal cut index C severs between the
                // 0-based bases C-1 and C, so the numeric value of N carries
                // over unchanged; on circular, N = len is the origin cut (0).
                if a < 1 || b < 1 || a > len || b > len {
                    return Err(format!(
                        "cut positions {} and {} out of range (1..={} for a {} bp {}; a cut at N severs the DNA between 1-based bases N and N+1)",
                        a, b, len, len, project.topology
                    ));
                }
                let (a, b) = if circular { (a % len, b % len) } else { (a, b) };
                (
                    a,
                    b,
                    format!(
                        "fragment between cuts {} and {}",
                        cut_notation(a, len, circular),
                        cut_notation(b, len, circular)
                    ),
                )
            }
            _ => {
                return Err(
                    "fragment mode needs enzyme1+enzyme2 (names) OR cut1+cut2 (positions), not a mix"
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
                "fwd primer's 5' end (1-based position {}) is downstream of the rev primer's 5' end (1-based position {}); the pair does not define an amplicon on a linear sequence",
                f_start + 1, r_end + 1
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
/// naturally lands on the full [0, len-1] span. Primers come along when
/// their primary binding site (binding_sites[0]) overlaps the pieces at all;
/// the site is clipped/translated like a feature span and reopening the
/// exported file recomputes exact sites anyway.
fn build_export_data(
    project: &ProjectData,
    pieces: &[(i64, i64)],
    flip: bool,
) -> (String, Vec<Feature>, Vec<Primer>) {
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
    let mut primers: Vec<Primer> = Vec::new();
    for p in &project.primers {
        let Some(site) = p.binding_sites.first() else {
            continue;
        };
        let ss = site.template_start;
        let se = site.template_end - 1;
        let mut best: Option<(i64, i64)> = None;
        for (pi, &(ps, pe)) in pieces.iter().enumerate() {
            let (wo, _) = windows[pi];
            let os = ss.max(ps);
            let oe = se.min(pe);
            if os > oe {
                continue;
            }
            let (ns, ne) = if flip {
                (wo + (pe - oe), wo + (pe - os))
            } else {
                (wo + (os - ps), wo + (oe - ps))
            };
            let replace = match best {
                None => true,
                Some((bs, be)) => ne - ns > be - bs,
            };
            if replace {
                best = Some((ns, ne));
            }
        }
        let Some((ns, ne)) = best else {
            continue;
        };
        let mut site = site.clone();
        site.template_start = ns;
        site.template_end = ne + 1;
        if flip {
            site.strand = -site.strand;
        }
        let mut np = p.clone();
        np.binding_sites = vec![site];
        primers.push(np);
    }
    (sequence.to_ascii_uppercase(), features, primers)
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

    /// Compact text digest of a whole project. Coordinates are 1-based
    /// inclusive (features, primers, read ranges); enzyme cuts render as
    /// N^N+1 (between the 1-based bases N and N+1). feature_filter matches
    /// feature name (case-insensitive substring) or exact ftype. Primers
    /// render as a PRIMERS
    /// section (or "PRIMERS (none)" when the project has none). The UNIQUE
    /// CUTTERS list (90+ lines on real plasmids) is collapsed to a single count
    /// line by default; pass `compactCutters: false` for the full per-enzyme
    /// list. The digest ends with a `DETECTED COMMON FEATURES (auto)` section
    /// listing non-fragment features auto-annotated against the embedded
    /// SnapGene database, one line each (name | type | strand | start..end |
    /// identity%) with an `(already annotated)` marker. Fragment hits are
    /// omitted to avoid misleading partial matches. CDS/mRNA features whose
    /// stored `/translation` qualifier disagrees with the current DNA
    /// sequence get a WARNING line (first disagreeing amino-acid position).
    /// When ≥2 stored reads share the same mismatch at the same template
    /// position, a MISMATCH CONSENSUS line flags the positions as possibly
    /// outdated template. Length units and sections
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

    /// Compact text digest of a region of a project. start/end are 1-based
    /// inclusive; on circular sequences start > end wraps the origin. Only
    /// features, primer binding sites and enzyme cut positions overlapping
    /// [start, end] are included. The enzyme cut list is collapsed into a
    /// single count line by default; pass `compact: false` for every cut in
    /// the window. When stored read alignments overlap the window, an
    /// ALIGNMENT DIFFS IN REGION section lists each read's mismatches,
    /// deletions and insertions inside the window (1-based coordinates and
    /// bases; reads with no differences in the window are marked
    /// "no differences in window") — use it to check whether a site is
    /// mutated without aligning reads by eye. An ALIGNMENT VIEW IN REGION
    /// section then shows each overlapping read as aligned columns: three rows
    /// per read — template bases, a match mask (`|` match, `.` mismatch, `-`
    /// read gap, the same convention as check_primer_binding's matchMask) and
    /// the read bases (a `-` marks a deleted template column) — with the
    /// 1-based start coordinate on each row, so you can read a window's read
    /// bases directly instead of unwinding circular wraps and gap offsets from
    /// orientedSequence. Rows wrap at 60 bp; insertions and template positions
    /// the read does not cover are listed as `+N bp` / `uncovered template`
    /// notes below the block. Reads whose covered window exceeds 500 bp get an
    /// omission note instead of the rows (use ALIGNMENT DIFFS or a narrower
    /// window). Returns {projectId, text}.
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
        let text = project_digest(&project, &opts, Some((from1(request.start), from1(request.end))))
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "text": text })))
    }

    /// Read bases of a project's sequence. Returns {projectId, sequence, text}
    /// — `sequence` is the plain uppercase base string (machine-readable);
    /// `text` is the same window with a coordinate ruler (10 bp groups, 60 bp
    /// per line; the ruler line is omitted for windows of 60 bp or less, where
    /// the per-line position prefix is enough). start/end are 1-based
    /// inclusive; on circular sequences start > end wraps the origin. Windows
    /// larger than 10000 bp are rejected.
    /// This tool is for INSPECTING bases only: if you need to hand this
    /// sequence (or part of it) to another tool or file, use
    /// export_subsequence to write it to a file instead of copying the text.
    #[tool]
    async fn read_sequence(
        &self,
        Parameters(request): Parameters<SequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let (s, e) = (from1(request.start), from1(request.end));
        let text = read_sequence(&project, s, e)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let bases = libregene_core::digest::read_sequence_bases(&project, s, e)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(Json(serde_json::json!({ "projectId": id, "sequence": bases, "text": text })))
    }

    /// IUPAC-aware search of a project's sequence on both strands (reverse
    /// strand skipped for palindromic queries). Hits are 1-based inclusive.
    /// Returns {projectId, matches: [{start, end, strand}]}.
    /// Self-complementary TARGETS (e.g. an shRNA stem: arm X followed later by
    /// its reverse complement X') legitimately produce one '+' hit at X and
    /// one '-' hit at X' — the two arms of the stem, not a duplicated
    /// sequence. Only exactly palindromic QUERIES (reverse complement == the
    /// query itself, e.g. "AT" or a restriction site) skip the reverse scan.
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
        let matches: Vec<serde_json::Value> = matches
            .iter()
            .map(|m| {
                serde_json::json!({
                    "start": to1(m.start),
                    "end": to1(m.end),
                    "strand": m.strand,
                })
            })
            .collect();
        Ok(Json(serde_json::json!({ "projectId": id, "matches": matches })))
    }

    /// List restriction-enzyme recognition sites on a project's sequence.
    /// `enzymes` is an optional list of enzyme names (case-insensitive); omit
    /// it (or pass []) to report every enzyme that has a site. Unknown names
    /// are rejected with near-match suggestions — use that error to probe
    /// which enzyme names exist on this sequence (this is the replacement for
    /// the removed full-database dump: query per name instead of pulling the
    /// whole ~196 KB catalog). When you need a full panorama of EVERY enzyme
    /// cut inside a region rather than per-enzyme probing, call
    /// get_region_view with `compact: false` on that window instead — it lists
    /// all cuts without naming enzymes one by one. Sites are the
    /// already-computed engine results
    /// the UI shows (circular-normalized, methylation-aware), so no recompute
    /// runs.
    /// Returns {projectId, enzymes: [{name, sites: [{recStart, recEnd,
    /// recSeq, strand, cuts: [{topCutIndex, botCutIndex}], methylationBlocked,
    /// unique}]}]}. recStart/recEnd are 1-based inclusive; topCutIndex/
    /// botCutIndex give the 1-based base BEFORE the break: the strand is
    /// severed between topCutIndex and topCutIndex+1 (topCutIndex = len on a
    /// circular sequence means between the last and the first base); strand is
    /// "top" or "bottom" (recognition orientation); unique = exactly one site
    /// for that enzyme.
    ///
    /// Half-site accounting for assembly: a cut at topCutIndex N severs the
    /// DNA between the 1-based bases N and N+1, so the UPSTREAM fragment ends
    /// with base N and the DOWNSTREAM fragment starts with base N+1 — each
    /// fragment keeps the half-site that lies on its side of the break. When
    /// you compute a ligation junction between two digests, the product is
    /// [fragment A .. its topCutIndex] + [fragment B .. its topCutIndex+1 ..].
    /// Example — NheI recognizes GCTAGC and cuts G^CTAGC on the top strand:
    /// topCutIndex = recStart (the 1-based G), so the upstream fragment keeps
    /// the "G" and the downstream fragment begins with "CTAGC"; the bottom
    /// strand is severed between the site's 5th and 6th bases (GCTAG^C),
    /// botCutIndex = recStart + 4. A cutter OUTSIDE the recognition site (e.g.
    /// BbsI, GAAGAC, topCutIndex = recStart + 7) leaves the intact GAAGAC on
    /// the upstream fragment while the 4-base 5' overhang belongs entirely to
    /// the downstream fragment — the sticky ends never overlap the recognition
    /// sequence, so the site survives digestion on the upstream side.
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
        let circular = project.topology == "circular";
        let tlen = project.length;
        let enzymes_json: Vec<serde_json::Value> = enzyme_names
            .into_iter()
            .map(|n| {
                let mut sites = by_name[n].clone();
                sites.sort_by_key(|e| e.rec_start);
                serde_json::json!({
                    "name": n,
                    "sites": sites.iter().map(|e| serde_json::json!({
                        "recStart": to1(e.rec_start),
                        "recEnd": to1(e.rec_end),
                        "recSeq": e.rec_seq,
                        "strand": e.recognition_strand,
                        "cuts": e.cut_pairs.iter().map(|p| serde_json::json!({
                            "topCutIndex": cut_flanks(p.top_cut_index, tlen, circular).0,
                            "botCutIndex": cut_flanks(p.bot_cut_index, tlen, circular).0,
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
    /// templateStart/templateEnd are 1-based inclusive (the bound range spans
    /// templateStart..templateEnd, GenBank-style). bindingSiteCount is the
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
                        "templateStart": to1(s.template_start),
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
    /// for handing a file to the app: whenever a sequence already exists as a
    /// file on disk, bring it in through this tool rather than pasting its
    /// text into other tools. Files are also the recommended way to
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
        self.require_agent_window(&id).await?;
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
            &self.agent_windows,
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
        {
            let aw = self.agent_windows.read().await;
            if aw.values().any(|m| m.project_id == id) {
                return Err(ErrorData::invalid_params(
                    format!(
                        "Project '{}' lives in a dedicated agent window and is hidden from the main window; it cannot be activated there. Use request_agent_window to focus its agent window instead.",
                        id
                    ),
                    None,
                ));
            }
        }
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

    /// Bind a project to a dedicated agent window: the project disappears
    /// from the user's main window and opens in a new window that is LOCKED
    /// against user keyboard/mouse input (bot watermark, not-allowed cursor;
    /// the user can temporarily unlock it via an on-screen button, but any
    /// further MCP tool call on the project re-locks it). Mutating tools
    /// (edit_sequence, add_feature, update_feature, add_primer, add_alignment,
    /// save_file, optimize_cds/apply, find_orfs/add_as_features) REFUSE to run
    /// on projects that are not bound to an agent window, so call this right
    /// after open_file, before any modification. Multiple agents each get
    /// their own window and work in parallel without interfering. Calling
    /// again for a project that already has an agent window reuses, re-locks
    /// and focuses it. Returns {ok, projectId, windowLabel, locked, reused?}.
    #[tool]
    async fn request_agent_window(
        &self,
        Parameters(request): Parameters<RequestAgentWindowRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        {
            let pm = self.pm.read().await;
            if pm.get_project_by_id(&id).is_none() {
                return Err(ErrorData::invalid_params(
                    format!("Project not found: {}", id),
                    None,
                ));
            }
        }
        // Reuse the existing agent window for this project when there is one.
        let existing = {
            let aw = self.agent_windows.read().await;
            aw.iter()
                .find(|(_, m)| m.project_id == id)
                .map(|(l, _)| l.clone())
        };
        if let Some(label) = existing {
            crate::lock_agent_windows_for_project(&self.app_handle, &self.agent_windows, &id).await;
            if let Some(w) = self.app_handle.get_webview_window(&label) {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            return Ok(Json(serde_json::json!({
                "ok": true,
                "projectId": id,
                "windowLabel": label,
                "locked": true,
                "reused": true,
                "message": format!("Project '{}' already has agent window '{}' (re-locked and focused)", id, label),
            })));
        }
        let safe = sanitize_window_label(&id);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let label = format!("agent-{safe}-{ts}");
        {
            let mut wp = self.wp.write().await;
            wp.insert(label.clone(), id.clone());
        }
        {
            let mut aw = self.agent_windows.write().await;
            aw.insert(
                label.clone(),
                crate::AgentWindowMeta { project_id: id.clone(), locked: true },
            );
        }
        if let Err(e) = crate::spawn_project_window(&self.app_handle, &label) {
            // Roll back the registrations when the OS window fails to build.
            let mut wp = self.wp.write().await;
            wp.remove(&label);
            drop(wp);
            let mut aw = self.agent_windows.write().await;
            aw.remove(&label);
            return Err(ErrorData::internal_error(
                format!("failed to create agent window: {e}"),
                None,
            ));
        }
        // The project is now excluded from the main window's sidebar.
        crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, None).await;
        Ok(Json(serde_json::json!({
            "ok": true,
            "projectId": id,
            "windowLabel": label,
            "locked": true,
            "message": format!("Opened agent window '{}' for project '{}' — locked against user input; it re-locks automatically on every tool call", label, id),
        })))
    }

    /// Replace sequence [start..end] (1-based inclusive) with `replacement`
    /// (empty = delete). A pure insertion before base N is `start=N, end=N-1`;
    /// ranges must not wrap (start > end+1 rejected). The replacement sequence
    /// is given either
    /// as a plain string (`replacement`) or read from a local sequence file
    /// (`replacement_path` — .gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 etc.,
    /// the same formats open_file accepts; exactly one of the two must be
    /// given). PREFER `replacement_path`: a file cannot be mistyped or
    /// truncated, so whenever the insert already exists as a file — or is a
    /// region of an open project you can export first with export_subsequence —
    /// use the file. Use the `replacement` string only for short hand-authored
    /// edits (point mutations, short oligo-length inserts). The optional
    /// `strand` parameter sets the insertion direction: "+" (default) inserts
    /// the replacement exactly as given; "-" reverse-complements it first
    /// (e.g. when the source sequence is oriented on the opposite strand) —
    /// DNA projects only, rejected on RNA/protein projects. When the
    /// replacement comes from `replacement_path` and that file carries
    /// annotations, they travel with the sequence: features are clipped to the
    /// inserted span and rebased onto it (mirrored and strand-flipped when
    /// strand="-"), and primers (DNA projects only) are added with binding
    /// sites recomputed; names colliding with existing features/primers get a
    /// " (2)" suffix. Feature coordinates are shifted/clipped
    /// for the edit (features fully inside a deleted range are removed). When
    /// `expected_old` is given it must match the current [start..end] content
    /// case-insensitively or the edit is rejected with the actual content. On
    /// such a mismatch the failure response carries `currentContent` — the
    /// authoritative current [start..end] bases — plus a ±20 bp `mismatch`
    /// context block; copy `currentContent` verbatim as `expected_old` and
    /// retry instead of hand-building a long check string. Uses
    /// the same primer+enzyme recompute path as update_sequence. Returns
    /// newLength, old/new region views, 30 bp sequence context on each side of
    /// the edit, and side-effect echo `removedFeatures`/`clippedFeatures`
    /// (both always present, empty arrays when none): removed lists features
    /// fully inside the deleted/replaced span ({name, ftype, location} with
    /// the pre-edit 1-based "start..end"); clipped lists features whose
    /// coordinates changed other than a pure translation ({name, ftype,
    /// before, after} as 1-based {start, end}). `transferredFeatures`/
    /// `transferredPrimers` list annotation names brought in by
    /// `replacement_path` (omitted when none). On protein projects the replacement is
    /// uppercased and must be amino-acid letters (A-Z, optional trailing '*'
    /// stop codon); lengths are reported in aa (nt for RNA, bp for DNA).
    #[tool]
    async fn edit_sequence(
        &self,
        Parameters(request): Parameters<EditSequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        self.require_agent_window(&id).await?;
        let len = project.length;
        // 1-based inclusive inputs; internal model coordinates are 0-based.
        let (u_start, u_end) = (request.start, request.end);
        let start = from1(u_start);
        let end = from1(u_end);

        if u_start > u_end + 1 {
            return Ok(Json(fail_envelope(
                &id,
                format!(
                    "invalid range {}..{}: start > end+1; ranges must not wrap (a pure insertion before base N is start=N, end=N-1)",
                    u_start, u_end
                ),
            )));
        }
        if u_start < 1 || u_start > len + 1 || u_end < 0 || u_end > len {
            return Ok(Json(fail_envelope(
                &id,
                format!(
                    "range {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                    u_start, u_end, len
                ),
            )));
        }

        let (replacement, parsed_annotations) = match (request.replacement, request.replacement_path) {
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
            (Some(s), None) => (s, None),
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
                    // Annotations travel with the sequence: features/primers
                    // from the file land on the inserted region below.
                    Ok(data) => (
                        data.sequence,
                        Some((data.features, data.primers)),
                    ),
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

        // Insertion direction: "-" reverse-complements the replacement (DNA
        // only — revcomp is meaningless for RNA/protein sequences here).
        let reverse = match request.strand.as_deref() {
            None | Some("+") | Some(".") => false,
            Some("-") => true,
            Some(other) => {
                return Ok(Json(fail_envelope(
                    &id,
                    format!(
                        "Invalid strand '{}': must be \"+\" (default, insert as given) or \"-\" (reverse-complement before inserting)",
                        other
                    ),
                )));
            }
        };
        if reverse {
            if !project.is_dna() {
                return Ok(Json(fail_envelope(
                    &id,
                    "strand \"-\" (reverse complement) is only supported on DNA projects".to_string(),
                )));
            }
            replacement = libregene_core::utils::reverse_complement(&replacement);
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
                        "expected_old mismatch at content position {} (1-based, within [{}..{}]): expected context '{}' vs current context '{}'",
                        diff_at + 1,
                        u_start,
                        u_end,
                        String::from_utf8_lossy(&exp[ctx_lo..exp_hi]),
                        String::from_utf8_lossy(&cur[ctx_lo..cur_hi]),
                    ),
                );
                v["currentContent"] = serde_json::json!(current);
                v["mismatch"] = serde_json::json!({
                    "index": diff_at + 1,
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
        let mut transferred_feature_names: Vec<String> = Vec::new();
        let mut transferred_primer_names: Vec<String> = Vec::new();
        {
            let mut pm = self.pm.write().await;
            if let Some(p) = pm.get_project_mut_by_id(&id) {
                libregene_core::utils::adjust_features_for_edit(
                    &mut p.features,
                    start,
                    end,
                    replacement.len() as i64,
                );
                if let Some((feats, primers)) = parsed_annotations {
                    // revcomp/uppercase preserve length, so replacement.len()
                    // is the local coordinate space of the parsed annotations.
                    let repl_len = replacement.len() as i64;
                    let mut transferred = libregene_core::utils::transfer_features_for_insert(
                        &feats, repl_len, start, reverse,
                    );
                    let mut taken: std::collections::HashSet<String> = p
                        .features
                        .iter()
                        .map(|f| f.name.clone())
                        .chain(p.primers.iter().map(|pr| pr.name.clone()))
                        .collect();
                    for f in &mut transferred {
                        f.name = libregene_core::utils::unique_name(&f.name, &taken);
                        taken.insert(f.name.clone());
                    }
                    transferred_feature_names =
                        transferred.iter().map(|f| f.name.clone()).collect();
                    p.features.extend(transferred);
                    if p.is_dna() {
                        for mut pr in primers {
                            pr.name = libregene_core::utils::unique_name(&pr.name, &taken);
                            taken.insert(pr.name.clone());
                            pr.id = pr.name.clone();
                            pr.binding_sites = Vec::new();
                            transferred_primer_names.push(pr.name.clone());
                            p.primers.push(pr);
                        }
                    }
                }
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

        let (removed_json, clipped_json) = edit_impact_json(&impact);
        let action = if is_insertion {
            format!("Inserted {} {} before base {}", repl_len, unit, u_start)
        } else {
            format!(
                "Replaced [{}..{}] ({} {}) with {} {}",
                u_start,
                u_end,
                end - start + 1,
                unit,
                repl_len,
                unit
            )
        };
        let mut v = serde_json::json!({
            "ok": true,
            "message": format!(
                "{}{}; new length {} (was {})",
                action,
                if reverse { " (reverse-complemented)" } else { "" },
                new_len,
                len
            ),
            "projectId": id,
            "oldLength": len,
            "newLength": new_len,
            "contextBefore": context_before,
            "contextAfter": context_after,
            "removedFeatures": removed_json,
            "clippedFeatures": clipped_json,
        });
        if !transferred_feature_names.is_empty() {
            v["transferredFeatures"] = serde_json::json!(transferred_feature_names);
        }
        if !transferred_primer_names.is_empty() {
            v["transferredPrimers"] = serde_json::json!(transferred_primer_names);
        }
        if let Some(rv) = old_region {
            v["regionViewBefore"] = serde_json::json!(rv);
        }
        if let Some(rv) = new_region {
            v["regionView"] = serde_json::json!(rv);
        }
        Ok(Json(v))
    }

    /// Add a feature. Coordinates are 1-based inclusive (GenBank convention):
    /// give `start`+`end` for a simple feature, or `segments`
    /// ([{start, end}], 5'→3' order) for a segmented one — the two forms are
    /// mutually exclusive. strand (".", "+", "-", default "+") and color (hex,
    /// e.g. "#60A5FA") are optional.
    /// Returns {ok, message, projectId, featureId, regionView} around the new
    /// feature.
    #[tool]
    async fn add_feature(
        &self,
        Parameters(request): Parameters<AddFeatureRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        self.require_agent_window(&id).await?;
        let feature_id = next_id("feature");
        let name = request.name.clone();
        let ftype = request.ftype.clone();

        let (segments, start, end) = resolve_feature_span(
            request.start,
            request.end,
            request.segments,
        )
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        let strand = request.strand.clone().unwrap_or_else(|| "+".to_string());
        if !matches!(strand.as_str(), "." | "+" | "-") {
            return Ok(Json(fail_envelope(&id, "Invalid strand: must be ., +, or -".to_string())));
        }

        // Reject coordinates outside [1, project.length] (1-based).
        // resolve_feature_span only checks start<=end (no upper bound), so
        // without this a caller could write a feature with end = i64::MAX and
        // later panic downstream code that slices the sequence by these
        // coordinates.
        {
            let pm = self.pm.read().await;
            let plen = pm
                .get_project_by_id(&id)
                .map(|p| p.length)
                .unwrap_or(0);
            if end >= plen {
                return Ok(Json(fail_envelope(
                    &id,
                    format!(
                        "feature span {}..{} is out of range for project length {} (1-based inclusive)",
                        start + 1,
                        end + 1,
                        plen
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
            format!("Added {} {} at {} (1-based inclusive)", ftype, name, stored),
            region,
        );
        v["featureId"] = serde_json::json!(feature_id);
        Ok(Json(v))
    }

    /// Update a feature's attributes in one call. `feature_id` is required;
    /// give at least one of name/ftype/color/strand/start+end/segments or the
    /// call is rejected. Coordinates are 1-based inclusive (GenBank
    /// convention): `start`+`end` replace the whole span, `segments`
    /// ([{start, end}], 5'→3' order) replaces the segment breakdown — the two
    /// forms are mutually exclusive and neither touches the strand. strand
    /// must be ".", "+" or "-"; color is hex (e.g. "#F87171") and also
    /// recolors existing segments. Returns {ok, message, projectId,
    /// regionView} around the feature.
    #[tool]
    async fn update_feature(
        &self,
        Parameters(request): Parameters<UpdateFeatureRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        self.require_agent_window(&id).await?;
        if !self.feature_exists(&id, &request.feature_id).await {
            return Ok(Json(fail_envelope(&id, format!("Feature not found: {}", request.feature_id))));
        }
        if request.name.is_none()
            && request.ftype.is_none()
            && request.color.is_none()
            && request.strand.is_none()
            && request.start.is_none()
            && request.end.is_none()
            && request.segments.is_none()
        {
            return Ok(Json(fail_envelope(
                &id,
                "Nothing to update: give at least one of name/ftype/color/strand/start+end/segments".to_string(),
            )));
        }
        if let Some(s) = &request.strand {
            if !matches!(s.as_str(), "." | "+" | "-") {
                return Ok(Json(fail_envelope(&id, "Invalid strand: must be ., +, or -".to_string())));
            }
        }
        let feature_id = request.feature_id.clone();
        let has_span = request.start.is_some()
            || request.end.is_some()
            || request.segments.is_some();
        let new_span = if has_span {
            let span = resolve_feature_span(request.start, request.end, request.segments)
                .map_err(|e| ErrorData::invalid_params(e, None))?;
            // Pre-validate the new span against the project length (same reason
            // as add_feature). resolve_feature_span has no upper bound on its own.
            let (_, start, end) = &span;
            let pm = self.pm.read().await;
            let plen = pm.get_project_by_id(&id).map(|p| p.length).unwrap_or(0);
            if *end >= plen {
                return Ok(Json(fail_envelope(
                    &id,
                    format!(
                        "feature span {}..{} is out of range for project length {} (1-based inclusive)",
                        start + 1,
                        end + 1,
                        plen
                    ),
                )));
            }
            Some(span)
        } else {
            None
        };
        let payload = crate::do_update_feature(
            &self.app_handle,
            &self.pm,
            &self.wp,
            None,
            &id,
            &feature_id,
            move |f| {
                if let Some((segments, start, end)) = new_span {
                    f.segments = segments;
                    f.start = start;
                    f.end = end;
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
                        "Updated feature {}: {} {} at {} (1-based inclusive), strand {}",
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
    /// the template. Primer sequences are short (~20-60 nt), so passing `seq`
    /// as plain text is the intended input here — no file input needed.
    /// Returns {ok, message, projectId, bindingSites, regionView}
    /// — bindingSites: [{strand, templateStart, templateEnd, tm, annealLen}].
    /// templateStart/templateEnd are 1-based inclusive (the bound range spans
    /// templateStart..templateEnd, GenBank-style). annealLen is the number
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
        self.require_agent_window(&id).await?;
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
                                "templateStart": to1(s.template_start),
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
    ///   chars are stripped). Use ONLY for short hand-authored reads; pasted
    ///   long sequences are error-prone.
    /// - `path` (PREFERRED): read the sequence from a file. If the read lives
    ///   in a file, or is a region of an open project (export it first with
    ///   export_subsequence), use this — a file cannot be mistyped or
    ///   truncated. Supported file types:
    ///   `.gbk`/`.gb`/`.genbank` (GenBank), `.dna`/`.rna`/`.prot` (SnapGene
    ///   binary), `.gpt` (protein GenBank), `.fa`/`.fasta` (FASTA / plain
    ///   text sequence), `.ab1` (ABIF chromatogram; the basecalled PBAS
    ///   sequence is extracted).
    /// Giving neither or both is an error. A name is always required.
    ///
    /// Returns {ok, message, projectId, regionView, significant, identity,
    /// strand, segmentCount, alignedLength, mismatches, insertions,
    /// deletions, mismatchDetails, deletionDetails, insertionDetails,
    /// orientedSequence, coverage, name, alignmentId, alignments}.
    /// - `identity`: 0–1 fraction, full precision (not rounded).
    /// - `alignedLength`: template positions covered by the alignment (sum of
    ///   segment spans, bp).
    /// - `mismatches`/`insertions`/`deletions`: total base counts (identity
    ///   alone rounds away single mismatches).
    /// - `mismatchDetails`: [{pos, templateBase, readBase}] — one entry per
    ///   mismatched column; `pos` is the 1-based inclusive template position;
    ///   `readBase` is oriented to the template strand (already rev-comp'd
    ///   when strand is "-").
    /// - `deletionDetails`: [{pos, length, bases}] — consecutive deleted
    ///   template columns grouped into one entry; `pos` is the 1-based
    ///   inclusive template position of the first deleted base; entries
    ///   straddling the circular origin are merged.
    /// - `insertionDetails`: [{pos, bases, length}] — the extra read bases sit
    ///   between the 1-based template bases `pos` and `pos + 1` (on circular
    ///   templates pos = len means between the last and the first base).
    /// - `orientedSequence`: the FULL read sequence oriented to the template
    ///   (reverse-complemented when strand is "-"), so read bases line up
    ///   with the template coordinates used by mismatchDetails/coverage —
    ///   eyeball a window's read bases directly instead of reconstructing
    ///   them from the diff lists. Returned untruncated; reads from .ab1
    ///   files can exceed 1000 bp.
    /// - `coverage`: [{start, end}] — 1-based inclusive template spans the
    ///   read covers, one entry per segment; a read spanning the circular
    ///   origin yields two entries.
    /// - `alignments`: the project's FULL alignment list (including the one
    ///   just added). Every entry carries the stats {alignmentId, name,
    ///   identity, strand, segmentCount, alignedLength, mismatches,
    ///   insertions, deletions, coverage}; only the newly added alignment is
    ///   expanded with `mismatchDetails`, `deletionDetails`,
    ///   `insertionDetails` and `orientedSequence` — previously stored reads
    ///   stay stats-only so multi-read responses don't balloon. Pass
    ///   `compact: true` to omit `orientedSequence` from the top-level
    ///   summary and from every entry (including the new one), and to skip
    ///   the post-alignment `regionView`; use `read_sequence` or
    ///   `get_region_view` when you need the bases.
    /// A `coverageNote` is added (top-level and on the new alignment's entry)
    /// when the read's coverage is multi-segment with uncovered template bp
    /// between the segments — the engine never produces such gaps for
    /// origin-spanning reads (their segments are adjacent), so a non-zero note
    /// means the template region between segments was not covered by this
    /// read, not that the alignment is broken.
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
        self.require_agent_window(&id).await?;
        let name = request.name.clone();
        let compact = request.compact.unwrap_or(false);

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
        let (summary, alignments, region, coverage_note) = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .map(|p| {
                    let circular = p.topology == "circular";
                    let total = p.alignments.len();
                    let alignments: Vec<serde_json::Value> = p
                        .alignments
                        .iter()
                        .enumerate()
                        .map(|(idx, a)| {
                            if compact {
                                alignment_json_1based(a, &p.sequence, p.length, circular, true)
                            } else if idx + 1 == total {
                                alignment_json_1based(a, &p.sequence, p.length, circular, false)
                            } else {
                                alignment_stats_json_1based(a, &p.sequence, p.length, circular)
                            }
                        })
                        .collect();
                    let last = p.alignments.last();
                    let region = last.and_then(|a| {
                        a.segments.first().map(|s| (s.start as i64, s.end as i64))
                    });
                    let coverage_note = last.and_then(|a| {
                        let gap = uncovered_between_segments(a, p.sequence.len(), circular);
                        (gap > 0).then(|| {
                            format!("{} template bp uncovered between the read's coverage segments", gap)
                        })
                    });
                    (alignments.last().cloned(), alignments, region, coverage_note)
                })
                .unwrap_or((None, Vec::new(), None, None))
        };
        let region_view = if compact {
            None
        } else {
            match region {
                Some((s, e)) => self.digest_region(&id, Some((s, e)), true).await,
                None => self.digest_region(&id, None, true).await,
            }
        };
        let mut env = ok_envelope(&id, format!("Aligned {}", name), region_view);
        if let Some(s) = summary {
            env["significant"] = serde_json::json!(true);
            for (k, v) in s.as_object().unwrap_or(&serde_json::Map::new()) {
                if compact && k == "orientedSequence" {
                    continue;
                }
                env[k] = v.clone();
            }
        }
        env["alignments"] = serde_json::json!(alignments);
        if let Some(note) = coverage_note {
            env["coverageNote"] = serde_json::json!(note);
            if let Some(last) = env["alignments"].as_array_mut().and_then(|arr| arr.last_mut()) {
                last["coverageNote"] = serde_json::json!(note);
            }
        }
        Ok(Json(env))
    }

    // -----------------------------------------------------------------------
    // Analysis
    // -----------------------------------------------------------------------

    /// Find open reading frames (ATG→stop, both strands, all frames) on a
    /// project. min_aa defaults to 75. When add_as_features is true the ORFs
    /// are appended as real CDS features (through the add-feature path, with
    /// recompute/broadcast) and {ok, message, projectId, regionView} is
    /// returned; otherwise returns {projectId, orfs: [Feature]} with all
    /// coordinates 1-based inclusive (start/end and segments).
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
            let orfs_json: Vec<serde_json::Value> = orfs.iter().map(feature_json_1based).collect();
            return Ok(Json(serde_json::json!({ "projectId": id, "orfs": orfs_json })));
        }
        self.require_agent_window(&id).await?;
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
    /// "mutagenesis"; segments are {start, end} 1-based inclusive.
    /// amplify: optional `fwd_enzyme`/`rev_enzyme` (enzyme names, e.g.
    /// "BamHI" — probe valid names via find_restriction_sites' unknown-name
    /// suggestions) add a 5' tail of
    /// `protect_bases` (default 3) GC protection bases + the recognition
    /// site; candidates expose tail/tailLen/annealLen and Tm covers the
    /// actual contiguous 3' match (see the field table below for how
    /// designedTm relates to it). The amplify response always carries an
    /// `orientation` note: the product's top strand IS the template top
    /// strand of seg — Fwd primes from its 5' (left) end, Rev from its 3'
    /// (right) end — so primer names follow the template top strand, NOT any
    /// feature's coding strand. When seg overlaps a CDS feature the response
    /// adds `cdsOverlaps` ([{featureId, name, strand, note}]); for a
    /// minus-strand CDS the note spells out that Fwd sits at the CDS's 3'
    /// end and Rev at its 5' end. Map primer names to coding direction via
    /// that `strand` — never assume Fwd = CDS 5'.
    /// mutagenesis: `mut_seq` is the desired PLUS-strand content of `seg`
    /// after the edit; it must be the same length as `seg` and differ at
    /// <= 3 bases or the call fails with the current template sequence.
    /// The response includes a `mutation` self-check block (diffs, plus/minus
    /// strand context, and CDS codon/amino-acid change when `seg` lies inside
    /// a CDS — joined multi-segment CDS features are supported — mind the CDS
    /// strand: for a minus-strand CDS the coding change is the reverse
    /// complement of the plus-strand edit). In that block `segStart`/`segEnd`
    /// are 1-based inclusive template coordinates and each diff's `offset` is
    /// the 1-based position within `seg`; `cds.codonIndex`
    /// is 1-based within the CDS (the codon that changes) and the amino-acid
    /// position is reported in
    /// TWO conventions: `cds.aaPosition1Based` counts the initiator Met as
    /// residue 1 (always equal to codonIndex), while
    /// `cds.aaPositionExcludingMet`
    /// excludes it (absent for the first codon) — the latter matches common
    /// literature numbering, e.g. mEGFP A206K shows up as
    /// aaPositionExcludingMet=206 / aaPosition1Based=207. Check which
    /// convention your task's numbering uses.
    /// Replacing every base of `seg`
    /// adds a `warning` (likely wrong strand/location) but is not rejected —
    /// EXCEPT when `seg` is exactly one or more complete codons of a CDS
    /// (codon-aligned, length divisible by 3, CDS context computable): a
    /// whole-codon swap (e.g. Ala→Lys, GCG→AAG) is an expected operation and
    /// does NOT warn. The warning is kept whenever the CDS context cannot be
    /// confirmed (seg outside any CDS, or not codon-aligned).
    /// In amplify mode the response always includes an `internalSites` array
    /// (empty when no enzyme recognition site occurs inside the amplified
    /// segment; non-empty entries {enzyme, start, end, strand}, 1-based
    /// inclusive, plus a `warning` that digestion would cut the product).
    /// Returns {projectId, groups: [PrimerGroup], mutation?, tmBasis,
    /// internalSites + orientation + cdsOverlaps? (amplify)}.
    ///
    /// Each PrimerGroup is {name, type: "fwd"|"rev", candidates,
    /// defaultIndex}: `candidates` are length variants ordered by anneal-core
    /// length ascending, and `defaultIndex` points at the RECOMMENDED
    /// candidate — the one whose Tm is closest to `target_tm` — use
    /// `groups[i].candidates[groups[i].defaultIndex]` instead of guessing.
    /// Each candidate is {seq, tail, tailLen, annealLen, tm, gc,
    /// designedAnnealLen?, designedTm?}:
    /// - `seq`: full primer sequence 5'→3' (tail + anneal core).
    /// - `tail`: 5' tail sequence (empty when the primer has no tail);
    ///   `tailLen` is its length in bases.
    /// - `annealLen`: anneal-core length in bases — the ACTUAL contiguous 3'
    ///   match against the template after unification.
    /// - `tm`: melting temperature (°C) of that actual contiguous 3' match,
    ///   rounded to 0.1. Because a 5' tail can accidentally pair with the
    ///   template adjacent to the designed site, `tm` may exceed the designed
    ///   core Tm for tailed primers.
    /// - `gc`: GC fraction of the FULL `seq`, one decimal.
    /// - `designedTm`/`designedAnnealLen`: the anneal-core values BEFORE
    ///   3'-end unification, preserved for reference and omitted when they
    ///   equal `tm`/`annealLen`. For PCR annealing temperature of a
    ///   5'-tailed primer, reference `designedTm` (the anneal core you
    ///   designed); `tm` is the actual 3' contiguous match that may include
    ///   accidental tail pairing.
    /// `tmBasis` always restates this basis.
    /// DNA-only: rejects RNA/protein projects (no primer design on
    /// single-strand molecules).
    #[tool]
    async fn design_primers(
        &self,
        Parameters(request): Parameters<DesignPrimersRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        let seg = request.seg.map(|s| libregene_core::models::Segment {
            start: from1(s.start),
            end: from1(s.end),
            color: None,
        });
        let seg2 = request.seg2.map(|s| libregene_core::models::Segment {
            start: from1(s.start),
            end: from1(s.end),
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
                                "start": (s + m.start) % len + 1,
                                "end": (s + m.end) % len + 1,
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
                Ok(info) => mutation_info = Some(mutagenesis_json_1based(&info)),
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
        let seg_bounds = seg.as_ref().map(|s| (s.start, s.end));
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
        let mut v = serde_json::json!({
            "projectId": id,
            "groups": groups,
            "tmBasis": "3' continuous match; 5' tail bases that accidentally match the adjacent template are included in annealLen/Tm (expected for tailed primers — see check_primer_binding per-site alignedTemplate/matchMask)",
        });
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
            if let Some((ss, se)) = seg_bounds {
                v["orientation"] = serde_json::json!(format!(
                    "Product top strand = template top strand of seg {}..{}: Fwd primes from its 5' (left) end, Rev from its 3' (right) end — primer names follow the template top strand, not any feature's coding strand",
                    ss + 1,
                    se + 1
                ));
                let cds_overlaps: Vec<serde_json::Value> = {
                    let pm = self.pm.read().await;
                    match pm.get_project_by_id(&id) {
                        Some(p) => {
                            let pieces = if p.topology == "circular" && ss > se {
                                vec![(ss, p.length - 1), (0, se)]
                            } else {
                                vec![(ss, se)]
                            };
                            p.features
                                .iter()
                                .filter(|f| f.ftype.eq_ignore_ascii_case("cds"))
                                .filter(|f| {
                                    let spans: Vec<(i64, i64)> = if f.segments.is_empty() {
                                        vec![(f.start, f.end)]
                                    } else {
                                        f.segments.iter().map(|s| (s.start, s.end)).collect()
                                    };
                                    pieces
                                        .iter()
                                        .any(|&(ps, pe)| spans.iter().any(|&(s, e)| s <= pe && ps <= e))
                                })
                                .map(|f| {
                                    let note = if f.strand == "-" {
                                        format!(
                                            "CDS '{}' is on the MINUS strand: its coding direction runs opposite to the product top strand — Fwd sits at the CDS 3' end and Rev at the CDS 5' end",
                                            f.name
                                        )
                                    } else {
                                        format!(
                                            "CDS '{}' is on the plus strand: its coding direction matches the product top strand (Fwd at the CDS 5' side, Rev at the 3' side)",
                                            f.name
                                        )
                                    };
                                    serde_json::json!({
                                        "featureId": f.id,
                                        "name": f.name,
                                        "strand": f.strand,
                                        "note": note,
                                    })
                                })
                                .collect()
                        }
                        None => Vec::new(),
                    }
                };
                if !cds_overlaps.is_empty() {
                    v["cdsOverlaps"] = serde_json::json!(cds_overlaps);
                }
            }
        }
        Ok(Json(v))
    }

    /// Check whether the given primers (each {name, type: "fwd"|"rev", seq})
    /// can bind to a project's sequence, without persisting them. Primer
    /// sequences are short (~20-60 nt), so plain text is the intended input
    /// here. Same engine as check_primers_binding. Returns {projectId, tmBasis, results: [{id,
    /// binds, bindingSiteCount, site, sites}]}. `bindingSiteCount` is the
    /// number of binding sites (0 when the primer does not bind); `site` is
    /// the best one ({strand, templateStart, templateEnd, tm, annealLen,
    /// mismatchedTail, alignedTemplate, matchMask} or null) and `sites`
    /// lists ALL sites best-first (Tm
    /// descending, same field shape as `site`) — use `sites` for off-target
    /// detection. templateStart/templateEnd are 1-based inclusive (the bound
    /// range spans templateStart..templateEnd, GenBank-style). `binds: true`
    /// means the 3' anneal core matched —
    /// the primer may still carry mismatches at its 5' end. `mismatchedTail`
    /// is the number of 5'-most bases NOT part of the contiguous 3' match
    /// (0 when the whole primer anneals; >0 for mutagenesis primers and
    /// enzyme-tail primers). `annealLen` counts only the contiguous 3' match.
    /// Every site also carries a full-length template coverage view:
    /// `alignedTemplate` and `matchMask` are exactly the primer's length,
    /// 5'→3' — `alignedTemplate` holds the template base each primer position
    /// faces (complemented for strand -1 so it compares directly against the
    /// primer; '-' where a 5' tail hangs off the end of a LINEAR template)
    /// and `matchMask` marks each position '|' (match), '.' (mismatch) or
    /// '-' (no template base). When `mismatchedTail` > 0, 5' tail bases that
    /// happen to match the template bases adjacent to the anneal core extend
    /// annealLen and raise Tm beyond design_primers' values — expected, not
    /// anomalous binding; read the mask to see exactly which tail bases pair.
    /// `tmBasis` (always present) states this Tm/annealLen basis. Unlike
    /// design_primers (which reports the DESIGNED anneal core), this tool
    /// recomputes the ACTUAL contiguous 3'-end match — the canonical case is
    /// an enzyme-tail primer (e.g. GCG+GGATCC+anneal core) whose tail's 3'
    /// side matches the template next to the binding site, so check's
    /// annealLen/Tm come out higher than design's.
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
        // The core reports internal 0-based coordinates; convert every site's
        // templateStart/templateEnd to the 1-based inclusive MCP convention.
        if let Some(results) = v.get_mut("results").and_then(|r| r.as_array_mut()) {
            for result in results.iter_mut() {
                if let Some(site) = result.get_mut("site") {
                    if !site.is_null() {
                        site_json_to_1based(site);
                    }
                }
                if let Some(sites) = result.get_mut("sites").and_then(|s| s.as_array_mut()) {
                    for site in sites.iter_mut() {
                        site_json_to_1based(site);
                    }
                }
            }
        }
        v["projectId"] = serde_json::json!(id);
        v["tmBasis"] = serde_json::json!(
            "3' continuous match; 5' tail bases that accidentally match the adjacent template are included in annealLen/Tm (expected for tailed primers — see per-site alignedTemplate/matchMask)"
        );
        Ok(Json(v))
    }

    /// Convert coordinates between template position, feature-relative offset,
    /// and CDS amino-acid position. Exactly one of these mutually exclusive
    /// input forms must be provided:
    ///
    /// 1. `position`: a full-file 1-based inclusive template coordinate.
    /// 2. `feature_id` + `feature_offset`: 1-based offset along the feature's
    ///    own 5'→3' direction (reverse-complemented features count from their
    ///    3' end on the template).
    /// 3. `feature_id` + `aa_position`: 1-based amino-acid position within a
    ///    CDS/mRNA feature, INCLUDING the initiator Met (Met = 1). Literature
    ///    numbering that skips the Met (e.g. mEGFP A206K) corresponds to the
    ///    response's `aaPositionExcludingMet`, so convert before calling:
    ///    literature position + 1 (when the Met is present) is the `aa_position`
    ///    to send.
    ///
    /// Returns {projectId, input, position, base, codonPositions?, features,
    /// translations}. `base` is the template base at `position` (plus-strand,
    /// uppercase; the residue letter on protein projects). `features` lists
    /// every feature containing the resolved
    /// position with its 1-based feature-relative offset and total length.
    /// `translations` lists CDS/mRNA hits with codon index, amino-acid position
    /// in two conventions (`aaPosition1Based` includes the initiator Met;
    /// `aaPositionExcludingMet` does not, matching literature numbering such as
    /// mEGFP A206K), the coding-strand codon, the amino acid, and which base of
    /// the codon the position is. For amino-acid input, `codonPositions`
    /// contains the three template positions of the requested codon in 5'→3'
    /// biological order and the top-level `position` is the first of them.
    ///
    /// Works for DNA, RNA and protein projects for a/b lookups; translation
    /// lookup (c) only returns hits when the position falls inside a CDS/mRNA
    /// feature.
    #[tool]
    async fn convert_coordinates(
        &self,
        Parameters(request): Parameters<ConvertCoordinatesRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id).await?;
        let (sequence, features) = {
            let pm = self.pm.read().await;
            let p = pm
                .get_project_by_id(&id)
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
            (p.sequence.clone(), p.features.clone())
        };
        let len = sequence.len() as i64;

        let position: i64;
        let input_json: serde_json::Value;
        let codon_positions_opt: Option<[i64; 3]>;

        let has_position = request.position.is_some() as u8;
        let has_feature_offset = request.feature_id.is_some() && request.feature_offset.is_some();
        let has_aa_position = request.feature_id.is_some() && request.aa_position.is_some();
        if has_position + has_feature_offset as u8 + has_aa_position as u8 != 1 {
            return Ok(Json(fail_envelope(
                &id,
                "Provide exactly one of: `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
            )));
        }

        if let Some(pos1) = request.position {
            if pos1 < 1 || pos1 > len {
                return Ok(Json(fail_envelope(
                    &id,
                    format!("position {} out of bounds (1..={})", pos1, len),
                )));
            }
            position = pos1 - 1;
            input_json = serde_json::json!({ "kind": "template", "position": pos1 });
            codon_positions_opt = None;
        } else if let Some(feature_id) = request.feature_id {
            let f = features
                .iter()
                .find(|f| f.id == feature_id)
                .ok_or_else(|| ErrorData::invalid_params(format!("feature '{}' not found", feature_id), None))?;
            if let Some(offset1) = request.feature_offset {
                match libregene_core::coords::position_from_feature_offset(f, offset1) {
                    Ok(pos0) => {
                        // Feature coordinates are file-derived and not
                        // range-checked at parse time; reject before the
                        // sequence indexing below panics.
                        if pos0 < 0 || pos0 >= len {
                            return Ok(Json(fail_envelope(
                                &id,
                                format!(
                                    "feature '{}' coordinates fall outside the sequence (length {})",
                                    f.name, len
                                ),
                            )));
                        }
                        position = pos0;
                        input_json = serde_json::json!({
                            "kind": "featureOffset",
                            "featureId": feature_id,
                            "featureOffset": offset1,
                        });
                        codon_positions_opt = None;
                    }
                    Err(e) => return Ok(Json(fail_envelope(&id, e))),
                }
            } else if let Some(aa1) = request.aa_position {
                match libregene_core::coords::codon_from_aa(f, &sequence, aa1) {
                    Ok((positions, codon, aa)) => {
                        position = positions[0];
                        input_json = serde_json::json!({
                            "kind": "aminoAcid",
                            "featureId": feature_id,
                            "aaPosition": aa1,
                            "codon": codon,
                            "aminoAcid": aa.to_string(),
                        });
                        codon_positions_opt = Some(positions);
                    }
                    Err(e) => return Ok(Json(fail_envelope(&id, e))),
                }
            } else {
                // Unreachable because of the mutual-exclusion check above.
                return Ok(Json(fail_envelope(
                    &id,
                    "Provide exactly one of: `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
                )));
            }
        } else {
            return Ok(Json(fail_envelope(
                &id,
                "Provide exactly one of: `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
            )));
        }

        let feature_hits = libregene_core::coords::position_to_features(position, &features);
        let translation_hits =
            libregene_core::coords::position_to_translations(position, &sequence, &features);

        let features_json: Vec<serde_json::Value> = feature_hits
            .iter()
            .map(|h| {
                serde_json::json!({
                    "featureId": h.feature_id,
                    "name": h.name,
                    "ftype": h.ftype,
                    "strand": h.strand,
                    "featureOffset": h.offset,
                    "featureLength": h.length,
                })
            })
            .collect();
        let translations_json: Vec<serde_json::Value> = translation_hits
            .iter()
            .map(|h| {
                serde_json::json!({
                    "featureId": h.feature_id,
                    "name": h.name,
                    "strand": h.strand,
                    "codonIndex": h.codon_index,
                    "aaPosition1Based": h.aa_position_1_based,
                    "aaPositionExcludingMet": h.aa_position_excluding_met,
                    "codon": h.codon,
                    "aminoAcid": h.amino_acid.to_string(),
                    "codonBaseIndex": h.codon_base_index,
                })
            })
            .collect();

        let mut v = serde_json::json!({
            "projectId": id,
            "input": input_json,
            "position": position + 1,
            "base": sequence[position as usize..position as usize + 1].to_ascii_uppercase(),
            "features": features_json,
            "translations": translations_json,
        });
        if let Some(positions) = codon_positions_opt {
            v["codonPositions"] = serde_json::json!(
                positions.iter().map(|p| p + 1).collect::<Vec<_>>()
            );
        }
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
    ///   trailing stop codon is fine). No project is involved. Use ONLY for
    ///   short hand-authored coding sequences — pasted long sequences are
    ///   error-prone, so whenever the sequence exists as a file use
    ///   `input_path`, and when it is a region of an open project export it
    ///   first with export_subsequence.
    /// - `input_path` (PREFERRED for real sequences): local file parsed with file_io. DNA files (.gbk/.gb/
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
    /// are rejected. PREFER writing the result to a file (and open_file it
    /// afterwards) over reading the `optimizedSequence` text — sequences move
    /// between tools as files, not pasted text. `apply=true` is only meaningful in project mode: in
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
                if apply {
                    self.require_agent_window(&id).await?;
                }
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

    /// Export a subsequence of a project to a new file (GenBank) — THE
    /// recommended way to create a sequence file from a known region of an
    /// open project: select the region by coordinates, feature, enzyme cuts,
    /// or primer amplicon, export it with this tool, then open_file the result
    /// to work with it as a project. NEVER retype or paste the sequence into
    /// edit_sequence/other tools to build a new construct — export the range
    /// instead (pasted sequences are error-prone). The file holds the region's sequence
    /// (uppercase; template strand except as noted) plus every feature
    /// overlapping it (partially covered features are clipped to the region)
    /// with coordinates translated to the new linear
    /// coordinate system, and every primer whose primary binding site
    /// overlaps the region at all; circular projects always export linear fragments.
    ///
    /// `project_id` defaults to the active project. `output_path` is required
    /// — .gbk/.gb/.genbank for DNA/RNA projects, .gpt for protein projects
    /// (other extensions are rejected).
    ///
    /// Exactly ONE region selector (mixing selectors is rejected):
    /// - `start` + `end`: 1-based inclusive template coordinates; on circular
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
    ///   positions, 1-based — a cut at N severs the DNA between the 1-based
    ///   bases N and N+1 (valid range 1..=len; N = len is after the last base
    ///   on linear sequences, between the last and the first base on circular
    ///   ones; the fragment is [min, max-1] internal-0-based on linear
    ///   sequences, the forward arc on circular ones).
    /// - `fwd_primer` + `rev_primer`: the amplicon between the two primers'
    ///   binding sites. Each is a project primer name (stored binding sites
    ///   are used; name lookup wins) or a raw sequence (binding sites
    ///   recomputed with the primer engine, like check_primer_binding). The
    ///   fwd primer's best forward-strand site and the rev primer's best
    ///   reverse-strand site define the amplicon [fwdStart, revEnd]
    ///   (1-based inclusive) — the PCR product's top strand. A primer that
    ///   does not bind the strand its role needs is an error.
    ///
    /// Returns {ok, message, projectId, outputPath, length, primers?,
    /// regionView?}.
    /// `length` is the exported sequence length (bp/nt/aa); `primers` lists
    /// the names of primers written with the file (omitted when none);
    /// `regionView` is
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

        // Region start/end arrive 1-based inclusive; convert to the internal
        // 0-based model. cut1/cut2 stay raw — resolve_export_region validates
        // and converts them (a cut at 1-based N = internal cut index N).
        let mut request = request;
        request.start = request.start.map(from1);
        request.end = request.end.map(from1);

        let (pieces, flip, desc) = resolve_export_region(&project, &request)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let bbox = region_bbox(&pieces, project.length, project.topology == "circular");
        let out_name = output_project_name(&request.output_path);
        let path = request.output_path.clone();
        let message_path = path.clone();
        let (out_project, primer_names) = tokio::task::spawn_blocking(move || {
            let (sequence, features, primers) = build_export_data(&project, &pieces, flip);
            let primer_names: Vec<String> = primers.iter().map(|p| p.name.clone()).collect();
            let length = sequence.len() as i64;
            (
                ProjectData {
                    name: out_name,
                    sequence,
                    length,
                    topology: "linear".to_string(),
                    molecule_type: project.molecule_type.clone(),
                    features,
                    primers,
                    ..Default::default()
                },
                primer_names,
            )
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
        if !primer_names.is_empty() {
            v["primers"] = serde_json::json!(primer_names);
        }
        Ok(Json(v))
    }
}

// ---------------------------------------------------------------------------
// Server bootstrap + settings (start/stop/restart without app restart)
// ---------------------------------------------------------------------------

#[tool_handler(name = "LibreGene", instructions = "Agent windows: before modifying any project, call request_agent_window to move it into a dedicated window that locks out user input while you work (mutating tools refuse unbound projects; every tool call re-locks the window). Files over pasted text: whenever a sequence exists as a file (or can be written to one), prefer file-based I/O over pasting sequence text into tool arguments — pasted sequences are error-prone (transcription slips, truncation, wrong strand). Open sequence files with open_file; insert/replace from a file via edit_sequence's replacement_path; hand reads to add_alignment via path; feed optimize_cds via input_path and collect its result via output_path; to create a new file from a known region of an open project, export_subsequence (by coordinates, feature, enzymes/cuts, or primers) then open_file the result — never retype the sequence into another tool. Plain-text sequence parameters stay available for short hand-authored input (primers ~20-60 nt, point mutations, short inserts) or when no file exists. read_sequence is for inspecting bases, not for moving sequences between tools.")]
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
    agent_windows: crate::AgentWindows,
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
            agent_windows: self.agent_windows.clone(),
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
        agent_windows: crate::AgentWindows,
    ) -> Self {
        let auth_token = load_or_create_token(&app_handle);
        Self {
            app_handle,
            pm,
            wp,
            agent_windows,
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
            let agent_windows = self.agent_windows.clone();
            let port = cfg.port;
            let token = self.auth_token.clone();
            let handle = tauri::async_runtime::spawn(async move {
                if let Err(e) = serve_mcp(app, pm, wp, agent_windows, port, token).await {
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
            restrict_token_dir(parent);
        }
        write_token_file(&path, token);
        // Covers files that already existed with loose permissions (mode()
        // only applies at creation time).
        restrict_token_file(&path);
    }
}

/// Create with 0600 from the start so the token never exists at the
/// umask-default 0644, not even between write and chmod.
#[cfg(unix)]
fn write_token_file(path: &std::path::Path, token: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(token.as_bytes()));
}

#[cfg(not(unix))]
fn write_token_file(path: &std::path::Path, token: &str) {
    let _ = std::fs::write(path, token);
}

/// Tighten permissions on the token file itself.
///
/// The token is the single shared secret guarding the loopback MCP server,
/// so on multi-user Unix a default 0644 would let other local users read it
/// and impersonate the MCP client. 0600 restricts it to the owner.
#[cfg(unix)]
fn restrict_token_file(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn restrict_token_file(_path: &std::path::Path) {
    // Windows %APPDATA% inherits a user-only ACL by default; token_file_path
    // already derives from app_config_dir, so no extra tightening is needed.
}

#[cfg(unix)]
fn restrict_token_dir(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o700);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn restrict_token_dir(_path: &std::path::Path) {}

/// Load the persisted token, or generate and persist a fresh one on first run.
fn load_or_create_token<R: Runtime>(app: &AppHandle<R>) -> String {
    if let Some(path) = token_file_path(app) {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            let token = contents.trim().to_string();
            if !token.is_empty() {
                // Token files persisted by older versions may still be 0644;
                // tighten on load so upgrading users are covered without
                // having to rotate the token.
                if let Some(parent) = path.parent() {
                    restrict_token_dir(parent);
                }
                restrict_token_file(&path);
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
    agent_windows: crate::AgentWindows,
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
        move || {
            Ok(LibreGeneMcp::new(
                app_handle.clone(),
                pm.clone(),
                wp.clone(),
                agent_windows.clone(),
            ))
        },
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
    /// The project is bound to a (fake) agent window so mutating tools pass
    /// the agent-window gate.
    async fn handler_with_project(project: ProjectData) -> LibreGeneMcp<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let id = project.name.clone();
        pm.write().await.load(&id, project);
        let agent_windows: crate::AgentWindows = Arc::new(RwLock::new(HashMap::new()));
        agent_windows.write().await.insert(
            "agent-test".to_string(),
            crate::AgentWindowMeta { project_id: id.clone(), locked: true },
        );
        LibreGeneMcp::new(
            app.handle().clone(),
            pm,
            Arc::new(RwLock::new(HashMap::new())),
            agent_windows,
        )
    }

    /// Same as handler_with_project but WITHOUT an agent window — for testing
    /// that mutating tools refuse projects not bound to an agent window.
    async fn handler_with_unbound_project(project: ProjectData) -> LibreGeneMcp<MockRuntime> {
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
            // 1-based inclusive interface → internal 0-based [40, 160]
            start: Some(41),
            end: Some(161),
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
            // 1-based wrap window 91..10 → internal 0-based 90..9
            start: Some(91),
            end: Some(10),
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
            region.contains("REGION: 91..21"),
            "regionView should show the 1-based wrap window 91..21, got: {}",
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
    async fn export_subsequence_exports_overlapping_primers() {
        // Export region [200..299]. P_in fully inside, P_part overlapping the
        // left edge, P_out fully outside → only P_in and P_part are written.
        let seq = synthetic_dna(400, 23);
        let mk = |name: &str, s: usize, e: usize| Primer {
            id: name.to_string(),
            name: name.to_string(),
            r#type: "fwd".to_string(),
            primer_seq: seq[s..e].to_string(),
            binding_sites: Vec::new(),
        };
        let mut project = ProjectData {
            name: "exp_primer_test".to_string(),
            sequence: seq.clone(),
            length: 400,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("f", "partial", 250, 350, "+")],
            primers: vec![mk("P_in", 220, 240), mk("P_part", 190, 210), mk("P_out", 320, 340)],
            ..Default::default()
        };
        libregene_core::primer::recompute(&mut project);
        assert!(project.primers.iter().all(|p| !p.binding_sites.is_empty()));

        // Direct mapping check: P_part's site clips to the overlap [200..209]
        // → [0..9] in export coordinates (template_end exclusive).
        let (_, _, primers) = build_export_data(&project, &[(200, 299)], false);
        assert_eq!(
            primers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["P_in", "P_part"]
        );
        let part = &primers.iter().find(|p| p.name == "P_part").unwrap().binding_sites[0];
        assert_eq!((part.template_start, part.template_end), (0, 10));

        let server = handler_with_project(project).await;
        let dir =
            std::env::temp_dir().join(format!("libregene-mcp-export-pr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("region.gbk");
        let out = server
            .export_subsequence(Parameters(ExportSubsequenceRequest {
                // 1-based inclusive interface → internal 0-based [200, 299]
                start: Some(201),
                end: Some(300),
                output_path: out_path.to_string_lossy().into_owned(),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["primers"], serde_json::json!(["P_in", "P_part"]));

        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        let mut names: Vec<&str> = parsed.primers.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["P_in", "P_part"]);
        // partially covered feature is clipped to the region: 250..299 → 50..99
        let f = parsed.features.iter().find(|f| f.name == "partial").unwrap();
        assert_eq!((f.start, f.end), (50, 99));
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
            // 1-based inclusive: the whole 14 aa protein
            start: Some(1),
            end: Some(aa.len() as i64),
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
            start: Some(1),
            end: Some(4),
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

        // pure insertion before base 61 (1-based; internal position 60) via
        // replacement_path
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 61,
                end: 60,
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

    /// Write a 60 bp fragment carrying a feature (10..29, "+"), a feature
    /// named "gene" (0..5, clashes with the target's "gene") and a primer
    /// binding 30..49; returns (dir, path).
    fn write_annotated_insert() -> (std::path::PathBuf, std::path::PathBuf) {
        let src_seq = synthetic_dna(60, 99);
        let mut src = ProjectData {
            name: "ann_src".to_string(),
            sequence: src_seq.clone(),
            length: 60,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![
                feature("f", "ins_feat", 10, 29, "-"),
                feature("f2", "gene", 0, 5, "+"),
            ],
            primers: vec![Primer {
                id: "ins_primer".to_string(),
                name: "ins_primer".to_string(),
                r#type: "fwd".to_string(),
                primer_seq: src_seq[30..50].to_string(),
                binding_sites: Vec::new(),
            }],
            ..Default::default()
        };
        libregene_core::primer::recompute(&mut src);
        let dir =
            std::env::temp_dir().join(format!("libregene-mcp-edit-ann-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gbk = dir.join("insert.gbk");
        libregene_core::file_io::gbk::write_gbk(&src, &gbk).unwrap();
        (dir, gbk)
    }

    #[tokio::test]
    async fn edit_sequence_transfers_annotations_from_gbk() {
        let (dir, gbk) = write_annotated_insert();
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 61,
                end: 60,
                replacement_path: Some(gbk.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(
            v["transferredFeatures"],
            serde_json::json!(["ins_feat", "gene (2)"])
        );
        assert_eq!(v["transferredPrimers"], serde_json::json!(["ins_primer"]));

        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(p.sequence.len(), 260);
        let f = p.features.iter().find(|f| f.name == "ins_feat").unwrap();
        assert_eq!((f.start, f.end), (70, 89));
        // name clash with the target's "gene" → renamed, rebased to 60..65
        let renamed = p.features.iter().find(|f| f.name == "gene (2)").unwrap();
        assert_eq!((renamed.start, renamed.end), (60, 65));
        let pr = p.primers.iter().find(|x| x.name == "ins_primer").unwrap();
        assert!(!pr.binding_sites.is_empty(), "primer site recomputed");
        let bs = &pr.binding_sites[0];
        assert_eq!((bs.template_start, bs.template_end), (90, 110));
        drop(pm);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn edit_sequence_transfers_annotations_reverse_complemented() {
        let (dir, gbk) = write_annotated_insert();
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 61,
                end: 60,
                replacement_path: Some(gbk.to_string_lossy().into_owned()),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);

        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        // local [10..29] in a 60 bp insert mirrors to [30..49] → +60 offset,
        // and the "-" strand survives the gbk round trip and flips to "+"
        let f = p.features.iter().find(|f| f.name == "ins_feat").unwrap();
        assert_eq!((f.start, f.end), (90, 109));
        assert_eq!(f.strand, "+");
        // the primer still binds (on the opposite strand) inside the insert
        let pr = p.primers.iter().find(|x| x.name == "ins_primer").unwrap();
        let bs = &pr.binding_sites[0];
        assert_eq!(bs.strand, -1);
        assert!(bs.template_start >= 60 && bs.template_end <= 120);
        drop(pm);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn edit_sequence_replacement_input_validation() {
        let server = handler_with_project(edit_test_project()).await;

        // both replacement and replacement_path → error
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 11,
                end: 21,
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
                start: 11,
                end: 21,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(out.0["message"].as_str().unwrap().contains("exactly one"), "{}", out.0);

        // bad extension → hard error from validate_user_path
        let res = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 11,
                end: 21,
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
                start: 11,
                end: 21,
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
    async fn edit_sequence_strand_minus_inserts_reverse_complement() {
        let server = handler_with_project(edit_test_project()).await;

        // pure insertion before base 61 (1-based) with strand "-" → revcomp inserted
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 61,
                end: 60,
                replacement: Some("AAACCCGGGTTG".to_string()),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert!(
            v["message"].as_str().unwrap().contains("reverse-complemented"),
            "{}",
            v
        );
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(p.sequence.len(), 212);
        assert_eq!(&p.sequence[60..72], "CAACCCGGGTTT");
        // feature 50..100 spans the insertion point → end shifted by 12
        let f = p.features.iter().find(|f| f.name == "gene").unwrap();
        assert_eq!((f.start, f.end), (50, 112));
    }

    #[tokio::test]
    async fn edit_sequence_strand_validation() {
        // invalid strand value → fail envelope
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 61,
                end: 60,
                replacement: Some("ACGT".to_string()),
                strand: Some("x".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("Invalid strand"),
            "{}",
            out.0
        );

        // strand "-" rejected on protein projects
        let server = handler_with_project(protein_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 11,
                end: 11,
                replacement: Some("AA".to_string()),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("only supported on DNA"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn edit_sequence_string_replacement_still_works() {
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 11,
                end: 20,
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
                start: 1,
                end: 4,
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
                start: 6,
                end: 9,
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
                start: 6,
                end: 9,
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

    // ------------------------------------------------------------------
    // add_feature / update_feature (1-based inclusive interface params)
    // ------------------------------------------------------------------

    fn dna_test_project() -> ProjectData {
        ProjectData {
            name: "feat".to_string(),
            sequence: synthetic_dna(100, 3),
            length: 100,
            topology: "linear".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn add_feature_converts_1based_to_internal_0based() {
        let server = handler_with_project(dna_test_project()).await;
        let out = server
            .add_feature(Parameters(AddFeatureRequest {
                project_id: Some("feat".to_string()),
                name: "cds1".to_string(),
                ftype: "CDS".to_string(),
                start: Some(1),
                end: Some(10),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        // The confirmation message echoes 1-based coordinates.
        assert!(
            out.0["message"].as_str().unwrap().contains("at 1..10"),
            "{}",
            out.0
        );
        let fid = out.0["featureId"].as_str().unwrap().to_string();
        let pm = server.pm.read().await;
        let f = pm
            .get_project_by_id("feat")
            .unwrap()
            .features
            .iter()
            .find(|f| f.id == fid)
            .unwrap()
            .clone();
        assert_eq!((f.start, f.end), (0, 9));
        assert_eq!(f.strand, "+");
        assert_eq!(f.segments.len(), 1);
        assert_eq!((f.segments[0].start, f.segments[0].end), (0, 9));
    }

    #[tokio::test]
    async fn add_feature_segments_and_bounds() {
        let server = handler_with_project(dna_test_project()).await;
        // Segmented (join) feature: 1-based interface, 0-based storage
        let out = server
            .add_feature(Parameters(AddFeatureRequest {
                project_id: Some("feat".to_string()),
                name: "seg1".to_string(),
                ftype: "CDS".to_string(),
                segments: Some(vec![
                    FeatureSegmentSpec { start: 1, end: 10 },
                    FeatureSegmentSpec { start: 20, end: 30 },
                ]),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        {
            let pm = server.pm.read().await;
            let f = &pm.get_project_by_id("feat").unwrap().features[0];
            assert_eq!((f.start, f.end), (0, 29));
            assert_eq!(f.strand, "-");
            assert_eq!(f.segments.len(), 2);
            assert_eq!((f.segments[1].start, f.segments[1].end), (19, 29));
        }

        // Single point (start == end)
        let out = server
            .add_feature(Parameters(AddFeatureRequest {
                project_id: Some("feat".to_string()),
                name: "pt".to_string(),
                ftype: "misc_feature".to_string(),
                start: Some(42),
                end: Some(42),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);

        // Out of range: 1-based end 101 is past the last valid base 100
        let out = server
            .add_feature(Parameters(AddFeatureRequest {
                project_id: Some("feat".to_string()),
                name: "oob".to_string(),
                ftype: "CDS".to_string(),
                start: Some(91),
                end: Some(101),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("out of range"),
            "{}",
            out.0
        );

        // Zero/negative start / reversed span / segments+start conflict / start alone
        for req in [
            AddFeatureRequest {
                project_id: Some("feat".to_string()),
                name: "bad".to_string(),
                ftype: "CDS".to_string(),
                start: Some(0),
                end: Some(5),
                ..Default::default()
            },
            AddFeatureRequest {
                project_id: Some("feat".to_string()),
                name: "bad".to_string(),
                ftype: "CDS".to_string(),
                start: Some(9),
                end: Some(5),
                ..Default::default()
            },
            AddFeatureRequest {
                project_id: Some("feat".to_string()),
                name: "bad".to_string(),
                ftype: "CDS".to_string(),
                start: Some(1),
                end: Some(10),
                segments: Some(vec![FeatureSegmentSpec { start: 1, end: 10 }]),
                ..Default::default()
            },
            AddFeatureRequest {
                project_id: Some("feat".to_string()),
                name: "bad".to_string(),
                ftype: "CDS".to_string(),
                start: Some(1),
                ..Default::default()
            },
            AddFeatureRequest {
                project_id: Some("feat".to_string()),
                name: "bad".to_string(),
                ftype: "CDS".to_string(),
                ..Default::default()
            },
        ] {
            assert!(
                server.add_feature(Parameters(req)).await.is_err(),
                "expected invalid_params error"
            );
        }
    }

    #[tokio::test]
    async fn update_feature_span_and_segments() {
        let server = handler_with_project(dna_test_project()).await;
        let out = server
            .add_feature(Parameters(AddFeatureRequest {
                project_id: Some("feat".to_string()),
                name: "cds1".to_string(),
                ftype: "CDS".to_string(),
                start: Some(1),
                end: Some(10),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let fid = out.0["featureId"].as_str().unwrap().to_string();

        // Move the span; strand must be left untouched.
        let out = server
            .update_feature(Parameters(UpdateFeatureRequest {
                project_id: Some("feat".to_string()),
                feature_id: fid.clone(),
                start: Some(11),
                end: Some(20),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        {
            let pm = server.pm.read().await;
            let f = &pm.get_project_by_id("feat").unwrap().features[0];
            assert_eq!((f.start, f.end), (10, 19));
            assert_eq!(f.strand, "-", "span update must not touch strand");
        }

        // Replace with segments (join)
        let out = server
            .update_feature(Parameters(UpdateFeatureRequest {
                project_id: Some("feat".to_string()),
                feature_id: fid.clone(),
                segments: Some(vec![
                    FeatureSegmentSpec { start: 1, end: 10 },
                    FeatureSegmentSpec { start: 91, end: 100 },
                ]),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        {
            let pm = server.pm.read().await;
            let f = &pm.get_project_by_id("feat").unwrap().features[0];
            assert_eq!(f.segments.len(), 2);
            assert_eq!((f.start, f.end), (0, 99));
        }

        // Nothing to update
        let out = server
            .update_feature(Parameters(UpdateFeatureRequest {
                project_id: Some("feat".to_string()),
                feature_id: fid.clone(),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("Nothing to update"),
            "{}",
            out.0
        );

        // Out-of-range span (1-based end 101 > length 100)
        let out = server
            .update_feature(Parameters(UpdateFeatureRequest {
                project_id: Some("feat".to_string()),
                feature_id: fid.clone(),
                start: Some(96),
                end: Some(101),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("out of range"),
            "{}",
            out.0
        );

        // start without end → invalid_params
        let req = UpdateFeatureRequest {
            project_id: Some("feat".to_string()),
            feature_id: fid.clone(),
            start: Some(1),
            ..Default::default()
        };
        assert!(server.update_feature(Parameters(req)).await.is_err());
    }

    // ------------------------------------------------------------------
    // add_alignment: orientedSequence / coverage + region-view diff section
    // ------------------------------------------------------------------

    fn alignment_test_project(topology: &str) -> ProjectData {
        ProjectData {
            name: "aln_test".to_string(),
            sequence: synthetic_dna(200, 7),
            length: 200,
            topology: topology.to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn add_alignment_returns_oriented_sequence_and_coverage() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        // Read = template[50..150] with one base flipped at index 60 (pos 110).
        let mut read = template[50..150].to_string();
        let i = 60;
        let orig = read.as_bytes()[i];
        let flipped = if orig == b'A' { b'C' } else { b'A' };
        read.replace_range(i..i + 1, &(flipped as char).to_string());

        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: Some("aln_test".to_string()),
                name: "read1".to_string(),
                bases: Some(read.clone()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["strand"], "+");
        assert_eq!(v["orientedSequence"], read, "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 51, "end": 150 }]),
            "{v}"
        );
        assert_eq!(
            v["mismatchDetails"],
            serde_json::json!([{
                "pos": 111,
                "templateBase": (orig as char).to_string(),
                "readBase": (flipped as char).to_string(),
            }]),
            "{v}"
        );
        // The alignments array carries the same new fields per entry.
        let entry = &v["alignments"][0];
        assert_eq!(entry["orientedSequence"], read, "{entry}");
        assert_eq!(
            entry["coverage"],
            serde_json::json!([{ "start": 51, "end": 150 }]),
            "{entry}"
        );

        // Region view over the mismatch lists it in the diff section.
        let out = server
            .get_region_view(Parameters(RegionRequest {
                project_id: Some("aln_test".to_string()),
                start: 101,
                end: 121,
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap().to_string();
        assert!(text.contains("ALIGNMENT DIFFS IN REGION"), "{text}");
        assert!(
            text.contains(&format!("mismatch at 111: {} > {}", orig as char, flipped as char)),
            "{text}"
        );

        // Window overlapping the read but not the diff.
        let out = server
            .get_region_view(Parameters(RegionRequest {
                project_id: Some("aln_test".to_string()),
                start: 51,
                end: 61,
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap().to_string();
        assert!(text.contains("no differences in window"), "{text}");
        assert!(!text.contains("mismatch at 111"), "{text}");

        // Window outside the read: no diff section at all.
        let out = server
            .get_region_view(Parameters(RegionRequest {
                project_id: Some("aln_test".to_string()),
                start: 1,
                end: 41,
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap().to_string();
        assert!(!text.contains("ALIGNMENT DIFFS IN REGION"), "{text}");
    }

    #[tokio::test]
    async fn add_alignment_reverse_strand_oriented_sequence_is_revcomp() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let read = libregene_core::utils::reverse_complement(&template[50..120]);
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: Some("aln_test".to_string()),
                name: "rev_read".to_string(),
                bases: Some(read),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["strand"], "-", "{v}");
        // Oriented to the template: the rev-comp of the raw read.
        assert_eq!(v["orientedSequence"], template[50..120], "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 51, "end": 120 }]),
            "{v}"
        );
    }

    #[tokio::test]
    async fn add_alignment_circular_coverage_splits_at_origin() {
        let project = alignment_test_project("circular");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let read = format!("{}{}", &template[170..200], &template[0..25]);
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: Some("aln_test".to_string()),
                name: "wrap_read".to_string(),
                bases: Some(read.clone()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["orientedSequence"], read, "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 171, "end": 200 }, { "start": 1, "end": 25 }]),
            "{v}"
        );
    }

    #[tokio::test]
    async fn add_alignment_compact_omits_oriented_sequence_and_region_view() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let read = template[50..150].to_string();

        // compact=true drops orientedSequence and regionView, keeps coverage.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: Some("aln_test".to_string()),
                name: "compact_read".to_string(),
                bases: Some(read.clone()),
                path: None,
                compact: Some(true),
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert!(v.get("orientedSequence").is_none(), "{v}");
        assert!(v.get("regionView").is_none(), "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 51, "end": 150 }]),
            "{v}"
        );
        let entry = &v["alignments"][0];
        assert!(entry.get("orientedSequence").is_none(), "{entry}");
        assert!(entry.get("coverage").is_some(), "{entry}");

        // compact=false / omitted keeps orientedSequence and regionView.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: Some("aln_test".to_string()),
                name: "full_read".to_string(),
                bases: Some(read),
                path: None,
                compact: Some(false),
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(v.get("orientedSequence").is_some(), "{v}");
        assert!(v.get("regionView").is_some(), "{v}");
        // alignments[1] is the newly added read → full detail; alignments[0]
        // is the earlier compact read → stats-only (no orientedSequence,
        // no per-column details).
        assert!(v["alignments"][1].get("orientedSequence").is_some(), "{v}");
        assert!(v["alignments"][0].get("orientedSequence").is_none(), "{v}");
        assert!(v["alignments"][0].get("mismatchDetails").is_none(), "{v}");
        assert!(v["alignments"][0].get("coverage").is_some(), "{v}");
    }

    #[tokio::test]
    async fn add_alignment_slims_existing_alignments_in_full_responses() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let mut read1 = template[50..150].to_string();
        let i = 60;
        let orig = read1.as_bytes()[i];
        let flipped = if orig == b'A' { b'C' } else { b'A' };
        read1.replace_range(i..i + 1, &(flipped as char).to_string());
        server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: Some("aln_test".to_string()),
                name: "read1".to_string(),
                bases: Some(read1.clone()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();

        let read2 = template[60..130].to_string();
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: Some("aln_test".to_string()),
                name: "read2".to_string(),
                bases: Some(read2),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["alignments"].as_array().unwrap().len(), 2, "{v}");
        // The new read is fully expanded.
        assert_eq!(v["alignments"][1]["name"], "read2", "{v}");
        assert!(v["alignments"][1].get("orientedSequence").is_some(), "{v}");
        // The previously stored read is stats-only.
        assert_eq!(v["alignments"][0]["name"], "read1", "{v}");
        assert!(v["alignments"][0].get("orientedSequence").is_none(), "{v}");
        assert!(v["alignments"][0].get("mismatchDetails").is_none(), "{v}");
        assert!(v["alignments"][0].get("deletionDetails").is_none(), "{v}");
        assert!(v["alignments"][0].get("insertionDetails").is_none(), "{v}");
        for key in [
            "alignmentId",
            "name",
            "identity",
            "strand",
            "segmentCount",
            "alignedLength",
            "mismatches",
            "insertions",
            "deletions",
            "coverage",
        ] {
            assert!(v["alignments"][0].get(key).is_some(), "missing {key}: {v}");
        }
        // No coverage gaps for a single-segment read: no coverageNote.
        assert!(v.get("coverageNote").is_none(), "{v}");
    }

    #[test]
    fn uncovered_between_segments_measures_gaps_only() {
        use libregene_core::models::{AlignSegment, Alignment};
        let aln = |segs: Vec<(usize, usize, &str)>| Alignment {
            id: String::new(),
            name: String::new(),
            length: 0,
            strand: "+".into(),
            identity: 1.0,
            segments: segs
                .into_iter()
                .map(|(s, e, c)| AlignSegment {
                    start: s,
                    end: e,
                    chars: c.to_string(),
                })
                .collect(),
            insertions: Vec::new(),
            seq: String::new(),
        };
        // Single segment: no gap.
        assert_eq!(uncovered_between_segments(&aln(vec![(10, 29, "x")]), 60, false), 0);
        // Two segments with a 10 bp hole between them (linear).
        assert_eq!(
            uncovered_between_segments(&aln(vec![(10, 29, "x"), (40, 49, "y")]), 60, false),
            10
        );
        // Origin-spanning circular read: segments are adjacent at the wrap.
        assert_eq!(
            uncovered_between_segments(&aln(vec![(50, 59, "x"), (0, 9, "y")]), 60, true),
            0
        );
        // Circular segments with a real hole.
        assert_eq!(
            uncovered_between_segments(&aln(vec![(10, 29, "x"), (40, 49, "y")]), 60, true),
            10
        );
    }

    #[tokio::test]
    async fn design_primers_amplify_reports_orientation_and_cds_strand() {
        // Minus-strand CDS overlapping the seg: the response must spell out
        // the product orientation and the CDS strand so Fwd/Rev are not
        // misread as coding-direction names.
        let project = ProjectData {
            name: "amp_test".to_string(),
            sequence: synthetic_dna(200, 5),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "mEGFP", 60, 120, "-")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: Some("amp_test".to_string()),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 51, end: 151 }),
                name: Some("Amp".to_string()),
                target_tm: 55.0,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(v["groups"].as_array().is_some_and(|g| g.len() == 2), "{v}");
        let orientation = v["orientation"].as_str().expect("orientation note");
        assert!(orientation.contains("seg 51..151"), "{orientation}");
        assert!(orientation.contains("template top strand"), "{orientation}");
        let overlaps = v["cdsOverlaps"].as_array().expect("cdsOverlaps");
        assert_eq!(overlaps.len(), 1, "{v}");
        assert_eq!(overlaps[0]["name"], "mEGFP");
        assert_eq!(overlaps[0]["strand"], "-");
        assert!(
            overlaps[0]["note"].as_str().unwrap().contains("MINUS strand"),
            "{}",
            overlaps[0]["note"]
        );
        assert!(v["internalSites"].as_array().unwrap().is_empty(), "{v}");
    }

    #[tokio::test]
    async fn design_primers_mutagenesis_whole_codon_skips_warning() {
        // Codon-aligned full replacement inside a CDS: no warning. The same
        // full replacement OUTSIDE any CDS keeps the warning.
        let mut bytes = vec![b'A'; 90];
        bytes[60] = b'C';
        bytes[61] = b'G';
        bytes[62] = b'C';
        let project = ProjectData {
            name: "mut_test".to_string(),
            sequence: String::from_utf8(bytes).unwrap(),
            length: 90,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "orf", 30, 89, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;

        // Whole-codon swap CGC -> AAA (Arg -> Lys): expected operation.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: Some("mut_test".to_string()),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 61, end: 63 }),
                site_name: Some("A11K".to_string()),
                target_tm: 55.0,
                mut_seq: Some("AAA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(
            v["mutation"].get("warning").is_none(),
            "whole-codon swap must not warn: {v}"
        );
        // The mutation block is 1-based: seg 61..63, codonIndex ==
        // aaPosition1Based (aa numbering conventions are untouched).
        assert_eq!(v["mutation"]["segStart"], 61, "{v}");
        assert_eq!(v["mutation"]["segEnd"], 63, "{v}");
        assert_eq!(v["mutation"]["cds"]["codonIndex"], 11, "{v}");
        assert_eq!(v["mutation"]["cds"]["aaBefore"], "Arg", "{v}");
        assert_eq!(v["mutation"]["cds"]["aaAfter"], "Lys", "{v}");
        assert_eq!(v["mutation"]["cds"]["aaPosition1Based"], 11, "{v}");
        assert_eq!(v["mutation"]["cds"]["aaPositionExcludingMet"], 10, "{v}");

        // Same 3-base full replacement outside any CDS: warning kept.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: Some("mut_test".to_string()),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 11, end: 13 }),
                site_name: Some("M1".to_string()),
                target_tm: 55.0,
                mut_seq: Some("CCC".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        let w = v["mutation"]["warning"]
            .as_str()
            .expect("non-CDS full replacement must warn");
        assert!(w.contains("PLUS-strand"), "{w}");
    }

    #[tokio::test]
    async fn design_primers_unified_tm_matches_check_primer_binding() {
        // Construct a template where the fwd enzyme tail's 3' side accidentally
        // pairs with the template upstream of the anneal core. The unified
        // annealLen/Tm must match a separate check_primer_binding call.
        let mut seq = synthetic_dna(120, 42);
        // BamHI site (GGATCC) is the 3'-most 6 bases of the default fwd tail
        // GCG + GGATCC. Place it immediately 5' of the fwd anneal core.
        seq.replace_range(30..36, "GGATCC");
        let project = ProjectData {
            name: "tail_test".to_string(),
            sequence: seq.clone(),
            length: 120,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: Some("tail_test".to_string()),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 37, end: 77 }),
                name: Some("Amp".to_string()),
                target_tm: 55.0,
                fwd_enzyme: Some("BamHI".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(v.get("tmBasis").is_some(), "{v}");
        let fwd_group = v["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["type"] == "fwd")
            .cloned()
            .expect("fwd group");
        let default_idx = fwd_group["defaultIndex"].as_u64().map(|n| n as usize).unwrap_or(0);
        let cand = &fwd_group["candidates"][default_idx];
        let primer_seq = cand["seq"].as_str().unwrap().to_string();
        let designed_len = cand["designedAnnealLen"].as_u64().unwrap() as usize;
        let unified_len = cand["annealLen"].as_u64().unwrap() as usize;
        assert!(
            unified_len > designed_len,
            "tail should extend anneal_len: designed={designed_len}, unified={unified_len}"
        );

        // Verify the same values come out of check_primer_binding.
        let chk = server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: Some("tail_test".to_string()),
                primers: vec![PrimerInput {
                    name: "cand".to_string(),
                    r#type: "fwd".to_string(),
                    seq: primer_seq,
                }],
            }))
            .await
            .unwrap();
        let site = &chk.0["results"][0]["site"];
        assert_eq!(
            site["annealLen"].as_u64().unwrap() as usize,
            unified_len,
            "annealLen mismatch"
        );
        assert!(
            (site["tm"].as_f64().unwrap() - cand["tm"].as_f64().unwrap()).abs() < 0.05,
            "tm mismatch: check={} design={}",
            site["tm"],
            cand["tm"]
        );
    }

    #[tokio::test]
    async fn design_primers_unified_tm_matches_check_primer_binding_rev() {
        // Rev enzyme tail whose 3' side accidentally pairs with the template
        // downstream of the rev anneal core. The unified Tm must match what
        // check_primer_binding reports (the engine reverses the matched bases
        // for rev primers before computing Tm).
        let mut seq = synthetic_dna(120, 42);
        // HindIII tail = protect GCG + AAGCTT. Place AAGCTT immediately 3' of
        // the rev anneal core (which ends at seg.end = 77 0-based) so the
        // tail's 3'-most 6 bases pair.
        seq.replace_range(77..83, "AAGCTT");
        let project = ProjectData {
            name: "tail_rev_test".to_string(),
            sequence: seq.clone(),
            length: 120,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: Some("tail_rev_test".to_string()),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 37, end: 77 }),
                name: Some("Amp".to_string()),
                target_tm: 55.0,
                rev_enzyme: Some("HindIII".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        let rev_group = v["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["type"] == "rev")
            .cloned()
            .expect("rev group");
        let default_idx = rev_group["defaultIndex"].as_u64().map(|n| n as usize).unwrap_or(0);
        let cand = &rev_group["candidates"][default_idx];
        let primer_seq = cand["seq"].as_str().unwrap().to_string();
        let designed_len = cand["designedAnnealLen"].as_u64().unwrap() as usize;
        let unified_len = cand["annealLen"].as_u64().unwrap() as usize;
        assert!(
            unified_len > designed_len,
            "rev tail should extend anneal_len: designed={designed_len}, unified={unified_len}"
        );

        let chk = server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: Some("tail_rev_test".to_string()),
                primers: vec![PrimerInput {
                    name: "cand".to_string(),
                    r#type: "rev".to_string(),
                    seq: primer_seq,
                }],
            }))
            .await
            .unwrap();
        let site = &chk.0["results"][0]["site"];
        assert_eq!(
            site["annealLen"].as_u64().unwrap() as usize,
            unified_len,
            "annealLen mismatch"
        );
        assert!(
            (site["tm"].as_f64().unwrap() - cand["tm"].as_f64().unwrap()).abs() < 0.05,
            "tm mismatch: check={} design={}",
            site["tm"],
            cand["tm"]
        );
    }

    #[tokio::test]
    async fn convert_coordinates_three_input_forms() {
        // "ATGGTATAA" -> M V *; CDS on plus strand covers positions 1..9.
        let seq = "ATGGTATAA".to_string();
        let project = ProjectData {
            name: "coord_test".to_string(),
            sequence: seq.clone(),
            length: 9,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "orf", 0, 8, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;

        // 1. Template position input.
        let out = server
            .convert_coordinates(Parameters(ConvertCoordinatesRequest {
                project_id: Some("coord_test".to_string()),
                position: Some(2),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["projectId"], "coord_test", "{v}");
        assert_eq!(v["input"]["kind"], "template", "{v}");
        assert_eq!(v["input"]["position"], 2, "{v}");
        assert_eq!(v["position"], 2, "{v}");
        assert_eq!(v["base"], "T", "{v}");
        assert_eq!(v["features"][0]["featureOffset"], 2, "{v}");
        assert_eq!(v["translations"][0]["codonIndex"], 1, "{v}");
        assert_eq!(v["translations"][0]["codonBaseIndex"], 2, "{v}");

        // 2. Feature offset input -> same position.
        let out = server
            .convert_coordinates(Parameters(ConvertCoordinatesRequest {
                project_id: Some("coord_test".to_string()),
                feature_id: Some("cds1".to_string()),
                feature_offset: Some(2),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["input"]["kind"], "featureOffset", "{v}");
        assert_eq!(v["position"], 2, "{v}");
        assert_eq!(v["features"][0]["featureOffset"], 2, "{v}");

        // 3. Amino-acid position input -> codon positions and translation.
        let out = server
            .convert_coordinates(Parameters(ConvertCoordinatesRequest {
                project_id: Some("coord_test".to_string()),
                feature_id: Some("cds1".to_string()),
                aa_position: Some(2),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["input"]["kind"], "aminoAcid", "{v}");
        assert_eq!(v["input"]["aaPosition"], 2, "{v}");
        assert_eq!(v["position"], 4, "{v}");
        assert_eq!(v["base"], "G", "{v}");
        assert_eq!(v["codonPositions"], serde_json::json!([4, 5, 6]), "{v}");
        assert_eq!(v["translations"][0]["codonIndex"], 2, "{v}");
        assert_eq!(v["translations"][0]["aaPositionExcludingMet"], 1, "{v}");
        assert_eq!(v["translations"][0]["codon"], "GTA", "{v}");
        assert_eq!(v["translations"][0]["aminoAcid"], "V", "{v}");

        // Mutual-exclusion error.
        let out = server
            .convert_coordinates(Parameters(ConvertCoordinatesRequest {
                project_id: Some("coord_test".to_string()),
                position: Some(2),
                feature_id: Some("cds1".to_string()),
                feature_offset: Some(2),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("exactly one of"),
            "{}",
            out.0
        );

        // Out-of-bounds error.
        let out = server
            .convert_coordinates(Parameters(ConvertCoordinatesRequest {
                project_id: Some("coord_test".to_string()),
                position: Some(100),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("out of bounds"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn convert_coordinates_minus_strand_segmented_round_trip() {
        // Same minus-strand segmented CDS used in coords.rs tests: translates to FH.
        let seq = "ATGAAATTTAAA".to_string();
        let mut f = feature("mEGFP", "mEGFP", 0, 5, "-");
        f.segments = vec![
            Segment {
                start: 0,
                end: 2,
                color: None,
            },
            Segment {
                start: 3,
                end: 5,
                color: None,
            },
        ];
        let project = ProjectData {
            name: "coord_minus_test".to_string(),
            sequence: seq.clone(),
            length: 12,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![f],
            ..Default::default()
        };
        let server = handler_with_project(project).await;

        // aaPosition=2 (H) on the minus strand: 5'→3' codon order runs from
        // the higher template coordinate to the lower one (positions 3,2,1 in
        // 1-based), because the CDS's 5' end is at the right-hand segment.
        let out = server
            .convert_coordinates(Parameters(ConvertCoordinatesRequest {
                project_id: Some("coord_minus_test".to_string()),
                feature_id: Some("mEGFP".to_string()),
                aa_position: Some(2),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["codonPositions"], serde_json::json!([3, 2, 1]), "{v}");
        assert_eq!(v["translations"][0]["codon"], "CAT", "{v}");
        assert_eq!(v["translations"][0]["aminoAcid"], "H", "{v}");

        // Convert the first codon position (5' end, 1-based 3) back via template input.
        let pos = v["codonPositions"][0].as_i64().unwrap();
        let out = server
            .convert_coordinates(Parameters(ConvertCoordinatesRequest {
                project_id: Some("coord_minus_test".to_string()),
                position: Some(pos),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["translations"][0]["codonIndex"], 2, "{v}");
        assert_eq!(v["translations"][0]["codonBaseIndex"], 1, "{v}");
    }

    #[tokio::test]
    async fn edit_sequence_1based_bounds_and_insertion_message() {
        let server = handler_with_project(edit_test_project()).await;

        // start = 0 is invalid on the 1-based interface.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 0,
                end: 5,
                replacement: Some("ACGT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("1-based inclusive"),
            "{}",
            out.0
        );

        // Wrapping ranges are rejected (start > end+1).
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 50,
                end: 40,
                replacement: Some("ACGT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("start > end+1"),
            "{}",
            out.0
        );

        // Pure insertion before base 61 is start=61, end=60.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                start: 61,
                end: 60,
                replacement: Some("TT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        assert!(
            out.0["message"]
                .as_str()
                .unwrap()
                .contains("Inserted 2 bp before base 61"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn check_primer_binding_reports_1based_sites() {
        let seq = synthetic_dna(200, 31);
        let project = ProjectData {
            name: "chk".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: Some("chk".to_string()),
                primers: vec![PrimerInput {
                    name: "p1".to_string(),
                    r#type: "fwd".to_string(),
                    seq: seq[50..70].to_string(),
                }],
            }))
            .await
            .unwrap();
        let v = out.0;
        // Internal site [50, 70) → 1-based inclusive 51..70: templateStart
        // shifts by one, templateEnd keeps its value.
        assert_eq!(v["results"][0]["bindingSiteCount"], 1, "{v}");
        assert_eq!(v["results"][0]["site"]["templateStart"], 51, "{v}");
        assert_eq!(v["results"][0]["site"]["templateEnd"], 70, "{v}");
        assert_eq!(v["results"][0]["sites"][0]["templateStart"], 51, "{v}");
        assert_eq!(v["results"][0]["sites"][0]["templateEnd"], 70, "{v}");
    }

    #[tokio::test]
    async fn find_restriction_sites_reports_1based_coordinates() {
        // EcoRI GAATTC placed at internal 0-based 40..45.
        let mut seq = "ACGT".repeat(50);
        seq.replace_range(40..46, "GAATTC");
        let mut project = ProjectData {
            name: "enz".to_string(),
            sequence: seq,
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        libregene_core::enzyme::recompute(&mut project);
        let internal = project
            .enzymes
            .iter()
            .find(|e| e.name == "EcoRI")
            .expect("EcoRI site")
            .clone();
        let server = handler_with_project(project).await;
        let out = server
            .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
                project_id: Some("enz".to_string()),
                enzymes: Some(vec!["EcoRI".to_string()]),
            }))
            .await
            .unwrap();
        let v = out.0;
        let site = &v["enzymes"][0]["sites"][0];
        // recStart/recEnd shift by one; cut positions keep their value (a cut
        // at internal index C sits between the 1-based bases C and C+1).
        assert_eq!(site["recStart"], internal.rec_start + 1, "{v}");
        assert_eq!(site["recEnd"], internal.rec_end + 1, "{v}");
        assert_eq!(
            site["cuts"][0]["topCutIndex"],
            internal.cut_pairs[0].top_cut_index,
            "{v}"
        );
        assert_eq!(
            site["cuts"][0]["botCutIndex"],
            internal.cut_pairs[0].bot_cut_index,
            "{v}"
        );
    }

    #[tokio::test]
    async fn read_sequence_window_is_1based() {
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: Some("edit_test".to_string()),
                start: 1,
                end: 10,
            }))
            .await
            .unwrap();
        let v = out.0;
        let text = v["text"].as_str().unwrap();
        assert!(text.contains("Window 1..10 (10 bp)"), "{text}");
        let seq = synthetic_dna(200, 42);
        assert_eq!(
            v["sequence"].as_str().unwrap(),
            seq[0..10].to_ascii_uppercase(),
            "1-based 1..10 reads internal bases 0..=9"
        );
    }

    #[tokio::test]
    async fn mutating_tools_require_agent_window() {
        let server = handler_with_unbound_project(edit_test_project()).await;
        let err = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: Some("edit_test".to_string()),
                start: 1,
                end: 2,
                replacement: Some("AA".to_string()),
                ..Default::default()
            }))
            .await
            .err()
            .expect("expected agent-window gate error");
        assert!(err.message.contains("request_agent_window"), "{err}");
        let err = server
            .add_feature(Parameters(AddFeatureRequest {
                project_id: Some("edit_test".to_string()),
                name: "x".to_string(),
                ftype: "misc_feature".to_string(),
                ..Default::default()
            }))
            .await
            .err()
            .expect("expected agent-window gate error");
        assert!(err.message.contains("request_agent_window"), "{err}");
        // Read-only tools stay usable without an agent window.
        server
            .read_sequence(Parameters(SequenceRequest {
                project_id: Some("edit_test".to_string()),
                start: 1,
                end: 10,
            }))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn tool_call_relocks_unlocked_agent_window() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let project = edit_test_project();
        let id = project.name.clone();
        pm.write().await.load(&id, project);
        let agent_windows: crate::AgentWindows = Arc::new(RwLock::new(HashMap::new()));
        // The user has unlocked the window.
        agent_windows.write().await.insert(
            "agent-test".to_string(),
            crate::AgentWindowMeta { project_id: id.clone(), locked: false },
        );
        let server = LibreGeneMcp::new(
            app.handle().clone(),
            pm,
            Arc::new(RwLock::new(HashMap::new())),
            agent_windows.clone(),
        );
        // Any tool call that resolves the project re-locks the window.
        server
            .read_sequence(Parameters(SequenceRequest {
                project_id: Some(id.clone()),
                start: 1,
                end: 10,
            }))
            .await
            .unwrap();
        assert!(agent_windows.read().await["agent-test"].locked);
    }

    #[tokio::test]
    async fn request_agent_window_reuses_existing_window() {
        // handler_with_project pre-binds the fake agent window "agent-test".
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .request_agent_window(Parameters(RequestAgentWindowRequest {
                project_id: Some("edit_test".to_string()),
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["reused"], true);
        assert_eq!(v["windowLabel"], "agent-test");
        assert_eq!(v["locked"], true);
    }

    #[test]
    fn sanitize_window_label_keeps_only_alnum_dash_underscore() {
        assert_eq!(
            sanitize_window_label("/tmp/my project (v2).gbk"),
            "_tmp_my_project__v2__gbk"
        );
        assert_eq!(sanitize_window_label("plain_path-1_2.gbk"), "plain_path-1_2_gbk");
        assert_eq!(sanitize_window_label("ABC-def_123"), "ABC-def_123");
    }

    #[tokio::test]
    async fn request_agent_window_sanitizes_paths_with_parens_and_spaces() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let id = "/tmp/my project (v2).gbk".to_string();
        pm.write().await.load(&id, ProjectData {
            name: id.clone(),
            sequence: synthetic_dna(200, 9),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        });
        let wp: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));
        let aw: crate::AgentWindows = Arc::new(RwLock::new(HashMap::new()));
        let server = LibreGeneMcp::new(
            app.handle().clone(),
            pm,
            wp.clone(),
            aw.clone(),
        );
        let out = server
            .request_agent_window(Parameters(RequestAgentWindowRequest {
                project_id: Some(id.clone()),
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        let label = v["windowLabel"].as_str().unwrap();
        assert!(label.starts_with("agent-_tmp_my_project__v2__gbk-"), "{label}");
        assert!(
            label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "window label must contain only [A-Za-z0-9-_]: {label}"
        );
        assert!(!label.contains(' '), "{label}");
        // The window → project registration uses the sanitized label.
        assert!(wp.read().await.contains_key(label), "{label}");
        assert!(aw.read().await.contains_key(label), "{label}");
    }

    #[tokio::test]
    async fn activate_project_rejects_agent_owned_project() {
        let server = handler_with_project(edit_test_project()).await;
        let err = server
            .activate_project(Parameters(ActivateProjectRequest {
                project_id: "edit_test".to_string(),
            }))
            .await
            .err()
            .expect("expected agent-window activation error");
        assert!(err.message.contains("agent window"), "{err}");
    }
}
