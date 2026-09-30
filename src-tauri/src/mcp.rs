//! Embedded MCP (Model Context Protocol) server for LibreGene.
//!
//! Exposes tools over Streamable HTTP on `127.0.0.1:8766` so an external LLM
//! agent can operate the app like a real user. Mutations go through the same
//! shared cores as the Tauri commands (`crate::do_*`), so recompute, dirty
//! marking and `broadcast_project()` behave identically and the UI updates
//! live. Every mutation tool returns a uniform `{ok, message, projectId,
//! regionView?}` envelope (plus tool-specific fields).
//!
//! Sequence-change detection: every project-targeting tool response carries
//! `sequenceHash`/`revCompHash` — 7-hex hashes of the current biological
//! sequence and its reverse complement (case-insensitive, annotations/
//! primers/whitespace ignored; `revCompHash` is null for proteins). Comparing
//! them across calls detects any sequence edit; a project and its
//! reverse-complemented file share one (sequenceHash, revCompHash) pair.
//! Text digests carry the same values as a `SEQHASH:` header line.
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
use tauri::{AppHandle, Emitter, Manager, Runtime};

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
    /// Required: the project to inspect (see list_projects).
    project_id: String,
    #[schemars(with = "Option<i64>")]
    max_features: Option<usize>,
    feature_filter: Option<String>,
    /// Collapse the UNIQUE CUTTERS list into a single count line (default true;
    /// pass false for the full per-enzyme list).
    compact_cutters: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct RegionRequest {
    /// Required: the project to inspect (see list_projects).
    project_id: String,
    /// Window start, 1-based inclusive; on circular sequences start > end
    /// wraps the origin.
    start: i64,
    /// Window end, 1-based inclusive.
    end: i64,
    #[schemars(with = "Option<i64>")]
    max_features: Option<usize>,
    feature_filter: Option<String>,
    /// Collapse the enzyme cut list into a count line (default true; pass
    /// false for the full list).
    compact: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct SequenceRequest {
    /// Required: the project to read from (see list_projects).
    project_id: String,
    /// Window mode: window start, 1-based inclusive; on circular sequences
    /// start > end wraps the origin. Mutually exclusive with the coordinate
    /// modes below; requires `end`.
    start: Option<i64>,
    /// Window mode: window end, 1-based inclusive. See `start`.
    end: Option<i64>,
    /// Coordinate mode: full-file template coordinate (1-based inclusive).
    /// Mutually exclusive with feature_id + feature_offset and feature_id +
    /// aa_position.
    position: Option<i64>,
    /// Coordinate mode: feature ID for feature-relative or amino-acid
    /// lookups. Must be paired with exactly one of `feature_offset` or
    /// `aa_position`.
    feature_id: Option<String>,
    /// Coordinate mode: 1-based offset along the feature's own 5'→3'
    /// direction. Mutually exclusive with `position` and `aa_position`.
    feature_offset: Option<i64>,
    /// Coordinate mode: 1-based amino-acid position within a CDS/mRNA feature
    /// — INCLUDING the initiator Met (Met = 1). Literature numbering that
    /// skips the Met (e.g. mEGFP A206K) maps to the response's
    /// `aaPositionExcludingMet`, not to this input. Mutually exclusive with
    /// `position` and `feature_offset`.
    aa_position: Option<i64>,
    /// Coordinate mode: bases of context on each side of the resolved
    /// position for the returned window sequence (default 30; clamped at the
    /// sequence ends).
    flank: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct SearchRequest {
    query: String,
    /// Required: the project to search (see list_projects).
    project_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct OpenProjectRequest {
    /// Sequence file to open (.gbk/.gb/.genbank, .dna/.rna/.prot, .gpt,
    /// .fa/.fasta, .ab1, ...). The project id IS this path.
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct SaveFileRequest {
    /// Required: the project to save (see list_projects).
    project_id: String,
    /// Output file path (.gbk/.gb for DNA/RNA projects, .gpt for protein
    /// projects).
    path: String,
    /// Optional: export only a region of the project instead of the whole
    /// molecule (exactly one selector inside — see RegionSpec fields). The
    /// exported file is always linear and the project is NOT marked clean.
    region: Option<RegionSpec>,
    /// Required (true) when `path` already exists and is NOT the project's
    /// own source path (saving over the project's own file needs no flag).
    overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct CloseProjectRequest {
    /// Required: the project to close (must be bound as your agent tab).
    project_id: String,
    /// Required (true) to close a project with unsaved changes.
    force: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct EditSequenceRequest {
    /// Required: the project to edit (must be bound as your agent tab).
    project_id: String,
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
    /// save_file's `region` if needed) — pasted long sequences are error-prone.
    replacement: Option<String>,
    /// PREFERRED input: read the replacement sequence from a local file
    /// (.gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 etc., same formats as
    /// open_project). A file cannot be mistyped or truncated, so use it whenever
    /// the sequence exists on disk.
    replacement_path: Option<String>,
    /// Direction of the inserted replacement: "+" (default — insert exactly
    /// as given) or "-" (reverse-complement the replacement before inserting,
    /// e.g. when the source sequence is oriented on the opposite strand).
    /// DNA projects only; rejected on RNA/protein projects.
    strand: Option<String>,
    expected_old: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
struct ConvertItem {
    /// Project mode (only for from=dna, to=dna codon optimization): optimize
    /// a CDS/mRNA feature inside an open project. Must be absent in
    /// `sequence`/`input_path` modes.
    project_id: Option<String>,
    /// Feature id (project mode: required; input_path mode: optional — pick
    /// the file's CDS/mRNA feature with this id, otherwise the whole file
    /// sequence is used).
    feature_id: Option<String>,
    /// Standalone mode: raw sequence text (whitespace/digits ignored). Use
    /// ONLY for short hand-authored sequences; for anything from a file or an
    /// open project use `input_path` (export regions first with save_file's
    /// `region`) — pasted long sequences are error-prone.
    sequence: Option<String>,
    /// Standalone mode (PREFERRED for real sequences): local sequence file
    /// (.gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 — nucleotide; .gpt/.prot —
    /// protein). A file cannot be mistyped or truncated. `from` defaults to
    /// the file's molecule type.
    input_path: Option<String>,
    /// Input molecule type: "dna" | "rna" | "protein". Defaults: project
    /// mode → "dna"; `input_path` → the file's molecule type; `sequence` →
    /// "dna".
    from: Option<String>,
    /// Output molecule type: "dna" | "rna" | "protein". Defaults: "dna" for
    /// a protein input (reverse translation), otherwise same as `from`.
    to: Option<String>,
    /// Reverse-complement the input before converting (nucleotide →
    /// nucleotide only; rejected for protein input or output).
    rev_comp: Option<bool>,
    /// Species key from list_species (e.g. "e_coli", "h_sapiens"). Required
    /// for codon optimization (dna→dna with optimization, protein→dna/rna
    /// reverse translation, project mode).
    species: Option<String>,
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
    /// Optional: write the result to a file (sequence/input_path modes only;
    /// REJECTED in project mode — use apply=true, then save_file).
    /// .gbk/.gb/.genbank → GenBank of the output molecule; .gpt → protein
    /// GenBank; .fa/.fasta/.txt → bare sequence text. PREFERRED way to collect
    /// the result — use the file (open_project afterwards) rather than copying
    /// the result `sequence` text.
    output_path: Option<String>,
    /// Required (true) when `output_path` already exists (same overwrite rule
    /// as save_file).
    overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ConvertSequenceRequest {
    /// Batch of conversion items (1-64). Each item is converted independently:
    /// a failing item does not abort the others — it is reported as
    /// {ok: false, error} in its slot of `results`. A single conversion can
    /// also be passed WITHOUT `items` by putting the item fields at the top
    /// level (same shape as one item).
    items: Option<Vec<ConvertItem>>,
    #[serde(flatten)]
    single: ConvertItem,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct FeatureSegmentSpec {
    /// Segment start, 1-based inclusive.
    start: i64,
    /// Segment end, 1-based inclusive (must be >= start).
    end: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct SetFeatureRequest {
    /// Required: the project to modify (must be bound as your agent tab).
    project_id: String,
    /// Omitted = CREATE a feature (name/ftype and start+end or segments are
    /// required). Given = UPDATE that feature (at least one other field
    /// required).
    feature_id: Option<String>,
    /// Create: required. Update: new name.
    name: Option<String>,
    /// Create: required (e.g. "CDS", "misc_feature"). Update: new ftype.
    ftype: Option<String>,
    /// Create: feature start, 1-based inclusive — required together with
    /// `end` unless `segments` is given; mutually exclusive with `segments`.
    /// Update: new start (same rules); replaces the whole span.
    start: Option<i64>,
    /// Feature end, 1-based inclusive (>= start). See `start`.
    end: Option<i64>,
    /// Segmented feature (e.g. multi-exon CDS): [{start, end}] 1-based
    /// inclusive, in 5'→3' order. Mutually exclusive with `start`/`end`.
    segments: Option<Vec<FeatureSegmentSpec>>,
    /// ".", "+" or "-" (create default "+"; neither form touches the strand
    /// unless given).
    strand: Option<String>,
    /// Hex color, e.g. "#60A5FA" (create default "#60A5FA"; on update also
    /// recolors existing segments).
    color: Option<String>,
    /// Create-only initial notes (passing notes on update is rejected —
    /// notes update is not supported).
    notes: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AddPrimerRequest {
    /// Required: the project to modify (must be bound as your agent tab).
    project_id: String,
    name: String,
    #[serde(rename = "type")]
    r#type: String,
    /// Primer sequence as plain text (short, ~20-60 nt — intended input form).
    seq: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AddAlignmentRequest {
    /// Required: the project to modify (must be bound as your agent tab).
    project_id: String,
    name: String,
    /// Read sequence as a plain string — short hand-authored reads only;
    /// prefer `path` (a file cannot be mistyped or truncated).
    #[serde(alias = "seq")]
    bases: Option<String>,
    /// PREFERRED input: read the sequence from a file (.gbk/.gb/.genbank,
    /// .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1). If the read is a region of an
    /// open project, export it first with save_file's `region`.
    path: Option<String>,
    /// When true, omit the full `orientedSequence` and the post-alignment
    /// `regionView` to reduce response size. The newly added alignment's
    /// difference details and coverage are still returned (filtered to the
    /// focus window, with `outsideWindow` counts, when `region`/`feature_id`
    /// is also given); previously stored alignments stay stats-only. Use
    /// read_sequence/get_region_view when you need the bases.
    compact: Option<bool>,
    /// Focus window (1-based inclusive; start > end wraps the origin on
    /// circular templates): `mismatchDetails`/`deletionDetails`/
    /// `insertionDetails` are filtered to entries overlapping the window, the
    /// full `orientedSequence` is omitted, and the `regionView` shows this
    /// window (its ALIGNMENT VIEW section gives the window's read bases
    /// column-by-column). Use it when you only care whether a specific site
    /// (e.g. a restriction site) is mutated. Mutually exclusive with
    /// `feature_id`. The total mismatches/insertions/deletions counts still
    /// describe the WHOLE read.
    region: Option<SegParam>,
    /// Focus window from a project feature's bounding span (plus `flank` bp
    /// on each side) — same effect as `region` without hand-computing
    /// coordinates. Mutually exclusive with `region`.
    feature_id: Option<String>,
    /// Extra template bp on each side of the focus window (default 0;
    /// clamped at the sequence ends).
    flank: Option<i64>,
    /// Alignment engine: "blast" (default; NCBI blastn port — chains any
    /// number of colinear segments, handles split/multi-hit reads) or
    /// "smith-waterman" (single local block plus at most one flank).
    algorithm: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct FindOrfsRequest {
    /// Required: the project to scan (see list_projects).
    project_id: String,
    #[schemars(with = "Option<i64>")]
    min_aa: Option<usize>,
    add_as_features: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct FindRestrictionSitesRequest {
    /// Required: the project to scan (see list_projects).
    project_id: String,
    /// Enzyme names to report (case-insensitive); empty/omitted = all enzymes
    /// that have a recognition site on this sequence.
    enzymes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ListPrimersRequest {
    /// Required: the project to inspect (see list_projects).
    project_id: String,
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
    /// Required: the project to design against (see list_projects).
    project_id: String,
    mode: String,
    seg: Option<SegParam>,
    seg2: Option<SegParam>,
    name: Option<String>,
    name1: Option<String>,
    name2: Option<String>,
    site_name: Option<String>,
    target_tm: f64,
    #[schemars(with = "Option<i64>")]
    overlap_len: Option<usize>,
    #[schemars(with = "Option<i64>")]
    arm_len: Option<usize>,
    mut_seq: Option<String>,
    fwd_enzyme: Option<String>,
    rev_enzyme: Option<String>,
    #[schemars(with = "Option<i64>")]
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
    /// Required: the project to check against (see list_projects).
    project_id: String,
    primers: Vec<PrimerInput>,
}

/// Optional region selector of `save_file` (subsequence export). Exactly one
/// of the four modes must be given inside: start+end / feature_id /
/// enzyme1+enzyme2 or cut1+cut2 / fwd_primer+rev_primer.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
struct RegionSpec {
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
    /// Amplicon mode: fwd primer (project primer name or raw sequence). The
    /// exported amplicon spans the fwd primer's forward-strand site start to
    /// the rev primer's reverse-strand site end — its length is the primer
    /// pair's product size (also derivable from check_primer_binding's site
    /// coordinates without exporting anything).
    fwd_primer: Option<String>,
    /// Amplicon mode: rev primer (project primer name or raw sequence).
    rev_primer: Option<String>,
}

// ---------------------------------------------------------------------------
// convert_sequence input resolution (project / raw sequence / file)
// ---------------------------------------------------------------------------

/// One of the three mutually exclusive input modes of `convert_sequence`.
enum OptimizeInput {
    /// Open-project mode: `feature_id` names the CDS/mRNA feature to optimize.
    Project {
        project_id: String,
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
            let project_id = project_id.ok_or_else(|| {
                "`project_id` is required in project mode (or pass `sequence` or `input_path` for standalone input)"
                    .to_string()
            })?;
            let feature_id = feature_id.ok_or_else(|| {
                "`feature_id` is required in project mode (or pass `sequence` or `input_path` for standalone input)"
                    .to_string()
            })?;
            Ok(OptimizeInput::Project {
                project_id: project_id.to_string(),
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
    if !cleaned.len().is_multiple_of(3) {
        return Err(format!(
            "sequence length {} not divisible by 3 (expected a complete coding sequence)",
            cleaned.len()
        ));
    }
    Ok(cleaned)
}

/// Strip whitespace/digits and uppercase, requiring every letter in
/// `alphabet` ("ACGT" for DNA, "ACGU" for RNA). No length constraint.
fn clean_na_sequence(seq: &str, alphabet: &str) -> Result<String, String> {
    let cleaned: String = seq
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_ascii_digit())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.is_empty() {
        return Err("sequence is empty".to_string());
    }
    for (i, c) in cleaned.char_indices() {
        if !alphabet.contains(c) {
            return Err(format!(
                "invalid base '{}' at position {} in sequence (expected one of {})",
                c, i, alphabet
            ));
        }
    }
    Ok(cleaned)
}

/// The shared convert_sequence preview fields for codon-optimizing items
/// (identical across all input modes). The backend reports repairs/unresolved
/// as raw 0-based values; MCP bumps them to the 1-based inclusive convention
/// here (codonIndex 1 = first codon; unresolved ranges are 1-based inclusive
/// base offsets within the optimized sequence).
fn codon_preview_json(
    result: &libregene_core::codon::OptimizeResult,
    aa: &str,
    codon_count: usize,
    method: &str,
    species: &str,
) -> serde_json::Value {
    let repairs: Vec<serde_json::Value> = result
        .repairs
        .iter()
        .map(|r| {
            serde_json::json!({
                "codonIndex": r.codon_index + 1,
                "old": r.old,
                "new": r.new,
                "reason": r.reason,
            })
        })
        .collect();
    let unresolved: Vec<serde_json::Value> = result
        .unresolved
        .iter()
        .map(|u| {
            // Backend format: "<reason> <start>..<end>" (0-based inclusive).
            match u.rsplit_once(' ') {
                Some((reason, range)) => match range.split_once("..") {
                    Some((a, b)) => match (a.parse::<i64>(), b.parse::<i64>()) {
                        (Ok(s), Ok(e)) => {
                            serde_json::json!(format!("{} {}..{}", reason, s + 1, e + 1))
                        }
                        _ => serde_json::json!(u),
                    },
                    None => serde_json::json!(u),
                },
                None => serde_json::json!(u),
            }
        })
        .collect();
    serde_json::json!({
        "aa": aa,
        "codonCount": codon_count,
        "newCodons": result.new_codons,
        "caiBefore": result.cai_before,
        "caiAfter": result.cai_after,
        "gcBefore": result.gc_before,
        "gcAfter": result.gc_after,
        "repairs": repairs,
        "repairCount": repairs.len(),
        "unresolved": unresolved,
        "method": method,
        "species": species,
    })
}

/// Write a conversion result to `output_path`. The extension decides the
/// format: .gbk/.gb/.genbank → GenBank of `molecule` ("dna" | "rna" |
/// "protein"; `source` replaces the default minimal project when the input
/// file already carried features), .gpt → protein GenBank, .fa/.fasta/.txt →
/// bare sequence text. `cds_name` labels the whole-length CDS feature of a
/// minimal project (typically the source file stem); it falls back to the
/// output file stem. Returns the written path.
fn write_convert_output(
    output_path: &str,
    sequence: &str,
    molecule: &str,
    source: Option<&ProjectData>,
    cds_name: Option<&str>,
) -> Result<String, String> {
    let ext = crate::validate_user_path(output_path, crate::CONVERT_OUTPUT_EXTS)?;
    let path = std::path::Path::new(output_path);
    match ext.as_str() {
        "gbk" | "gb" | "genbank" => {
            let project = match source {
                Some(p) => p.clone(),
                None => minimal_na_project(output_path, sequence, molecule, cds_name),
            };
            libregene_core::file_io::gbk::write_gbk(&project, path)
                .map_err(|e| format!("failed to write {}: {}", output_path, e))?;
        }
        "gpt" => {
            if molecule != "protein" {
                return Err(format!(
                    ".gpt is a protein GenBank format; the {} output cannot be written as .gpt (use .gbk/.fa/.fasta/.txt)",
                    molecule
                ));
            }
            let project = minimal_protein_project(output_path, sequence, cds_name);
            libregene_core::file_io::gpt::write_gpt(&project, path)
                .map_err(|e| format!("failed to write {}: {}", output_path, e))?;
        }
        "fa" | "fasta" | "txt" => {
            std::fs::write(path, format!("{}\n", sequence))
                .map_err(|e| format!("failed to write {}: {}", output_path, e))?;
        }
        other => {
            return Err(format!(
                "unsupported output extension '.{}' (allowed: gbk, gb, genbank, gpt, fa, fasta, txt)",
                other
            ))
        }
    }
    Ok(output_path.to_string())
}

fn minimal_na_project(
    output_path: &str,
    seq: &str,
    molecule: &str,
    cds_name: Option<&str>,
) -> ProjectData {
    let name = output_project_name(output_path);
    let len = seq.len() as i64;
    ProjectData {
        features: vec![whole_cds_feature(len, cds_name.unwrap_or(&name))],
        name,
        sequence: seq.to_string(),
        length: len,
        topology: "linear".to_string(),
        molecule_type: molecule.to_string(),
        ..Default::default()
    }
}

fn minimal_protein_project(output_path: &str, aa: &str, cds_name: Option<&str>) -> ProjectData {
    minimal_na_project(output_path, aa, "protein", cds_name)
}

fn output_project_name(output_path: &str) -> String {
    std::path::Path::new(output_path)
        .file_stem()
        .map(|s| s.to_string_lossy().replace(' ', "_"))
        .unwrap_or_else(|| "optimized".to_string())
}

fn whole_cds_feature(len: i64, name: &str) -> Feature {
    Feature {
        id: "cds".to_string(),
        name: name.to_string(),
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
    agent_tabs: crate::AgentTabs,
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
/// Saturating: every caller range-checks the result afterwards, and an
/// i64::MIN input must not panic a debug build.
fn from1(x: i64) -> i64 {
    x.saturating_sub(1)
}

/// Upper bound for caller-supplied `flank` context windows (read_sequence
/// coordinate mode, add_alignment focus). Bounds i64 arithmetic and stops
/// absurd requests; anything larger is clamped at the sequence ends anyway.
const MAX_FLANK: i64 = 10_000;

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
/// A circular site ending exactly at the last base stores `templateEnd` 0
/// (wrapped); report the last base (`tlen`) instead of the out-of-domain 0.
fn site_json_to_1based(site: &mut serde_json::Value, tlen: i64, circular: bool) {
    if let Some(s) = site.get("templateStart").and_then(|v| v.as_i64()) {
        site["templateStart"] = serde_json::json!(s + 1);
    }
    if circular && site.get("templateEnd").and_then(|v| v.as_i64()) == Some(0) {
        site["templateEnd"] = serde_json::json!(tlen);
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
/// so multi-read responses stay small. Counts only: the diff detail vectors
/// are never serialized.
fn alignment_stats_json_1based(
    a: &libregene_core::models::Alignment,
    template: &str,
    _tlen: i64,
    _circular: bool,
) -> serde_json::Value {
    let diff = libregene_core::align::alignment_diff(a, template);
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
        "coverage": a.segments.iter().map(|s| serde_json::json!({
            "start": s.start + 1,
            "end": s.end + 1,
        })).collect::<Vec<_>>(),
    })
}

/// 1-based inclusive window membership, wrap-aware (s > e on circular
/// templates means the window crosses the origin).
fn in_window_1based(p: i64, s: i64, e: i64) -> bool {
    if s <= e {
        p >= s && p <= e
    } else {
        p >= s || p <= e
    }
}

/// Filter an alignment JSON's diff-detail arrays to entries overlapping the
/// 1-based inclusive focus window (wrap-aware). Insertions sit BETWEEN
/// template bases `pos` and `pos + 1`, so they are kept when either flanking
/// base is inside the window; a `pos + 1` past the last base wraps to 1 on
/// circular templates. Also adds an `outsideWindow` block with the
/// whole-read diff totals minus the in-window base counts, so callers can
/// see at a glance whether the window hides further differences.
fn filter_alignment_json_focus(
    v: &mut serde_json::Value,
    s1: i64,
    e1: i64,
    tlen: i64,
    circular: bool,
) {
    let obj = match v.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    if let Some(arr) = obj.get_mut("mismatchDetails").and_then(|a| a.as_array_mut()) {
        arr.retain(|m| {
            m.get("pos")
                .and_then(|p| p.as_i64())
                .is_some_and(|p| in_window_1based(p, s1, e1))
        });
    }
    // A deletion merged across the circular origin (terminal run + origin
    // run in alignment_diff) can carry pos + length past tlen; map those
    // coordinates back into 1..=tlen before comparing with the window.
    let wrap_x = |x: i64| if circular && x > tlen { x - tlen } else { x };
    if let Some(arr) = obj.get_mut("deletionDetails").and_then(|a| a.as_array_mut()) {
        arr.retain(|d| {
            match (
                d.get("pos").and_then(|p| p.as_i64()),
                d.get("length").and_then(|l| l.as_i64()),
            ) {
                (Some(p), Some(l)) => (p..p + l.max(1)).any(|x| in_window_1based(wrap_x(x), s1, e1)),
                _ => false,
            }
        });
    }
    if let Some(arr) = obj.get_mut("insertionDetails").and_then(|a| a.as_array_mut()) {
        arr.retain(|i| {
            i.get("pos").and_then(|p| p.as_i64()).is_some_and(|p| {
                let next = if circular && p == tlen { 1 } else { p + 1 };
                in_window_1based(p, s1, e1) || in_window_1based(next, s1, e1)
            })
        });
    }
    // In-window base counts: mismatch entries are one column each, insertion
    // entries count fully by their anchor, and deletion entries count only
    // the bases actually inside the window (a partially overlapping deletion
    // is kept but its outside bases are not window content).
    let in_mismatches = obj
        .get("mismatchDetails")
        .and_then(|a| a.as_array())
        .map(|a| a.len() as i64)
        .unwrap_or(0);
    let in_deletions: i64 = obj
        .get("deletionDetails")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .map(|d| {
                    match (
                        d.get("pos").and_then(|p| p.as_i64()),
                        d.get("length").and_then(|l| l.as_i64()),
                    ) {
                        (Some(p), Some(l)) => (p..p + l.max(1))
                            .filter(|&x| in_window_1based(wrap_x(x), s1, e1))
                            .count() as i64,
                        _ => 0,
                    }
                })
                .sum()
        })
        .unwrap_or(0);
    let in_insertions: i64 = obj
        .get("insertionDetails")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|d| d.get("length").and_then(|l| l.as_i64()))
                .sum()
        })
        .unwrap_or(0);
    let total = |key: &str| obj.get(key).and_then(|n| n.as_i64()).unwrap_or(0);
    obj.insert(
        "outsideWindow".to_string(),
        serde_json::json!({
            "mismatches": total("mismatches") - in_mismatches,
            "deletions": total("deletions") - in_deletions,
            "insertions": total("insertions") - in_insertions,
        }),
    );
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
            let segs = |v: &[libregene_core::models::EditSpan]| {
                v.iter()
                    .map(|s| serde_json::json!({"start": to1(s.start), "end": to1(s.end)}))
                    .collect::<Vec<_>>()
            };
            serde_json::json!({
                "name": c.name,
                "ftype": c.ftype,
                "before": {"start": to1(c.before.start), "end": to1(c.before.end)},
                "after": {"start": to1(c.after.start), "end": to1(c.after.end)},
                "beforeSegments": segs(&c.before_segments),
                "afterSegments": segs(&c.after_segments),
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
    v["orientationHint"] = serde_json::json!(orientation_hint(info));
    v
}

/// Plain-language restatement of the mutagenesis strand semantics with the
/// ACTUAL outcome, so a coding-strand/plus-strand slip is called out instead
/// of silently producing the wrong amino acid.
fn orientation_hint(info: &libregene_core::primer::design::MutagenesisAnalysis) -> String {
    let base = format!(
        "mut_seq was applied as the PLUS-strand (top-strand) content of seg {}..{}.",
        info.seg_start + 1,
        info.seg_end + 1
    );
    match &info.cds {
        Some(cds) if cds.strand == "-" => {
            let aa_pos = cds
                .aa_position_excluding_met
                .map(|p| p.to_string())
                .unwrap_or_else(|| cds.aa_position_1_based.to_string());
            format!(
                "{} CDS '{}' is on the MINUS strand: the coding-strand effect is the reverse complement of the plus-strand edit — codonAfter '{}' = {} at aa {}. If {} is NOT the amino acid you intended, you most likely passed CODING-strand sequence as mut_seq; reverse-complement it and retry.",
                base, cds.name, cds.codon_after, cds.aa_after, aa_pos, cds.aa_after
            )
        }
        Some(cds) => {
            let aa_pos = cds
                .aa_position_excluding_met
                .map(|p| p.to_string())
                .unwrap_or_else(|| cds.aa_position_1_based.to_string());
            format!(
                "{} CDS '{}' is on the PLUS strand: the coding-strand codon after the edit is '{}' = {} at aa {}, read directly from the plus-strand edit.",
                base, cds.name, cds.codon_after, cds.aa_after, aa_pos
            )
        }
        None => format!(
            "{} seg is not inside any CDS feature, so no codon-level self-check was possible; verify strand and location via plusContext/minusContext.",
            base
        ),
    }
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

/// Inject the `sequenceHash`/`revCompHash` pair into a tool response JSON.
fn insert_seq_hashes(v: &mut serde_json::Value, hashes: &(String, Option<String>)) {
    v["sequenceHash"] = serde_json::json!(hashes.0);
    v["revCompHash"] = match &hashes.1 {
        Some(h) => serde_json::json!(h),
        None => serde_json::Value::Null,
    };
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

/// Resolve set_feature span parameters (given 1-based inclusive, the MCP
/// interface convention) into model segments and overall bounds (internal
/// 0-based inclusive). Bounds against the project length are checked by the
/// caller (`span_within_bounds`).
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
            // Encoding order (same rule as project creation): ascending
            // starts; an origin-wrapping feature leads with its tail, the one
            // descending transition marking the origin. Out-of-order segments
            // would make the first/last-derived bounds wrong (a phantom wrap).
            let descents = out.windows(2).filter(|w| w[1].start < w[0].start).count();
            let ordered = descents <= 1
                && (descents == 0 || out.first().unwrap().start > out.last().unwrap().end);
            if !ordered {
                return Err(
                    "segments are not in encoding order (ascending starts; a wrapping feature leads with its tail, e.g. join(8886..9326, 1..219))"
                        .to_string(),
                );
            }
            // Bounds come from the first segment's start and the last
            // segment's end (segments are in encoding order), so an
            // origin-wrapping feature keeps its `start > end` semantics
            // instead of being flattened by min/max.
            let s = out.first().unwrap().start;
            let e = out.last().unwrap().end;
            Ok((out, s, e))
        }
        (Some(_), None, None) | (None, Some(_), None) => {
            Err("start and end must be given together".to_string())
        }
        (None, None, None) => Err("give start+end or segments".to_string()),
        _ => Err("invalid span parameters".to_string()),
    }
}

/// Clean a caller-supplied primer sequence (letters only, uppercase) and
/// validate `type` against the frontend's fwd/rev convention — an empty
/// cleaned sequence would silently persist as a 0-site primer, and an
/// arbitrary type string breaks the UI's fwd/rev rendering.
fn clean_primer_input(name: &str, r#type: &str, seq: &str) -> Result<String, String> {
    let clean: String = seq
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_uppercase();
    if clean.is_empty() {
        return Err(format!(
            "primer '{}' seq is empty after removing non-letter characters",
            name
        ));
    }
    if !matches!(r#type, "fwd" | "rev") {
        return Err(format!(
            "primer '{}' has invalid type '{}': must be \"fwd\" or \"rev\"",
            name, r#type
        ));
    }
    Ok(clean)
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

/// Constant-time string equality for the bearer token, so a local caller
/// cannot recover it byte by byte via timing. Length leaks are accepted (the
/// token is a fixed-length random string).
fn token_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub fn new(
        app_handle: AppHandle<R>,
        pm: Arc<RwLock<ProjectManager>>,
        wp: Arc<RwLock<HashMap<String, String>>>,
        agent_tabs: crate::AgentTabs,
    ) -> Self {
        Self { app_handle, pm, wp, agent_tabs }
    }

    /// Resolving a project also re-locks any agent tab bound to it — the user
    /// may unlock the tab, but the next tool call on the project locks it
    /// again.
    async fn resolve_project_id(&self, project_id: String) -> Result<String, ErrorData> {
        crate::lock_agent_tab_for_project(&self.app_handle, &self.agent_tabs, &project_id).await;
        Ok(project_id)
    }

    /// Clone the project's data out of the lock (and re-lock its agent tab).
    async fn resolve_project(&self, project_id: String) -> Result<(String, ProjectData), ErrorData> {
        self.resolve_project_impl(project_id, false).await
    }

    /// Same as resolve_project but strips the enzyme list — the single
    /// heaviest field on large plasmids — from the clone. Only for tools
    /// that never render a digest or consult restriction sites.
    async fn resolve_project_light(
        &self,
        project_id: String,
    ) -> Result<(String, ProjectData), ErrorData> {
        self.resolve_project_impl(project_id, true).await
    }

    async fn resolve_project_impl(
        &self,
        project_id: String,
        strip_enzymes: bool,
    ) -> Result<(String, ProjectData), ErrorData> {
        let project = {
            let pm = self.pm.read().await;
            let p = pm.get_project_by_id(&project_id).cloned().ok_or_else(|| {
                ErrorData::invalid_params(format!("Project not found: {}", project_id), None)
            })?;
            if strip_enzymes {
                ProjectData { enzymes: Vec::new(), ..p }
            } else {
                p
            }
        };
        crate::lock_agent_tab_for_project(&self.app_handle, &self.agent_tabs, &project_id).await;
        Ok((project_id, project))
    }

    /// Mutating tools may only operate on projects bound as MCP agent tabs,
    /// so the user's own projects stay untouched. Read-only tools are
    /// unrestricted; `open_project` performs the binding.
    async fn require_agent_tab(&self, project_id: &str) -> Result<(), ErrorData> {
        let at = self.agent_tabs.read().await;
        if at.contains_key(project_id) {
            return Ok(());
        }
        Err(ErrorData::invalid_params(
            format!(
                "Project '{}' is not bound as an MCP agent tab (it was opened by the user, not via MCP open_project). Mutating tools refuse to operate on projects the user opened — copy the file (e.g. bash `cp`) to a new path and open_project the copy.",
                project_id
            ),
            None,
        ))
    }

    /// Reuse the agent tab binding of an already-loaded project (re-locking
    /// it), or reject when the project is loaded but NOT bound — that means
    /// the user opened it, and their projects stay under user control.
    /// Shared by open_project's fast path and the lost-race path after its
    /// atomic check-and-load.
    async fn reuse_agent_tab_or_reject(&self, id: &str) -> Result<Json<serde_json::Value>, ErrorData> {
        // Emit agent-tab-lock only on an unlocked → locked transition
        // (same semantics as lock_agent_tab_for_project).
        enum Reuse {
            Relocked,
            AlreadyLocked,
            NotBound,
        }
        let reuse = {
            let mut at = self.agent_tabs.write().await;
            match at.get_mut(id) {
                Some(meta) if meta.locked => Reuse::AlreadyLocked,
                Some(meta) => {
                    meta.locked = true;
                    Reuse::Relocked
                }
                None => Reuse::NotBound,
            }
        };
        match reuse {
            Reuse::Relocked | Reuse::AlreadyLocked => {
                if matches!(reuse, Reuse::Relocked) {
                    let _ = self.app_handle.emit(
                        "agent-tab-lock",
                        serde_json::json!({ "projectId": id, "locked": true }),
                    );
                }
                let mut v = serde_json::json!({
                    "ok": true,
                    "projectId": id,
                    "locked": true,
                    "reused": true,
                    "message": format!("Project '{}' is already open and bound as your agent tab (re-locked)", id),
                });
                if let Some(h) = self.project_seq_hashes(id).await {
                    insert_seq_hashes(&mut v, &h);
                }
                Ok(Json(v))
            }
            Reuse::NotBound => Err(ErrorData::invalid_params(
                format!(
                    "Project '{}' is already open and was NOT opened via MCP open_project (it was opened by the user, or in a separate window). To work on a copy, copy the file with bash `cp` to a new path and open_project the copy.",
                    id
                ),
                None,
            )),
        }
    }

    async fn project_summary(&self, project_id: &str) -> Option<String> {
        let pm = self.pm.read().await;
        let p = pm.get_project_by_id(project_id)?;
        let unit = match p.molecule_type.as_str() {
            "rna" => "nt",
            "protein" => "aa",
            _ => "bp",
        };
        let desc = format!("{} {} {}", p.length, unit, p.topology);
        Some(if p.name.is_empty() {
            desc
        } else {
            format!("{}: {}", p.name, desc)
        })
    }

    /// (sequenceHash, revCompHash) of the project's CURRENT in-memory
    /// sequence, re-read from pm — call after a mutation so the hash reflects
    /// the edit. Never call while holding a pm guard (lock recursion).
    async fn project_seq_hashes(&self, project_id: &str) -> Option<(String, Option<String>)> {
        let pm = self.pm.read().await;
        pm.get_project_by_id(project_id)
            .map(|p| libregene_core::utils::orientation_hashes(&p.sequence, &p.molecule_type))
    }

    /// Resolve the project and reject non-DNA projects for DNA-only tools.
    async fn require_dna_project(&self, project_id: String) -> Result<String, ErrorData> {
        let (id, project) = self.resolve_project_light(project_id).await?;
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
    /// The project is cloned out of the lock and rendered on a blocking
    /// thread — rendering the enzyme list needs the full project, and
    /// holding the pm read lock across it would starve UI edits (writers).
    async fn digest_region(
        &self,
        project_id: &str,
        region: Option<(i64, i64)>,
        compact: bool,
    ) -> Option<String> {
        let project = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(project_id).cloned()?
        };
        let opts = DigestOptions {
            compact_enzymes: compact,
            ..DigestOptions::default()
        };
        tokio::task::spawn_blocking(move || project_digest(&project, &opts, region).ok())
            .await
            .ok()
            .flatten()
    }

    /// Text digest of the region around a feature (looked up by id); compact
    /// enzyme rendering (only mutation tools call this).
    async fn digest_feature_region(&self, project_id: &str, feature_id: &str) -> Option<String> {
        let (project, s, e) = {
            let pm = self.pm.read().await;
            let project = pm.get_project_by_id(project_id)?;
            let f = project.features.iter().find(|f| f.id == feature_id)?;
            // Clamp the +/-5 context window with saturating arithmetic so a
            // feature near an end (or a maliciously huge coordinate that slipped
            // past validation) can't underflow/overflow and panic the process.
            // A wrapping feature (start > end) yields s > e, which
            // project_digest interprets as an origin-wrapping region window
            // on circular projects — the intended span.
            let last = project.length.saturating_sub(1);
            let s = f.start.saturating_sub(5).min(last);
            let e = (f.end.saturating_add(5)).min(last);
            (project.clone(), s, e)
        };
        let opts = DigestOptions {
            compact_enzymes: true,
            ..DigestOptions::default()
        };
        tokio::task::spawn_blocking(move || project_digest(&project, &opts, Some((s, e))).ok())
            .await
            .ok()
            .flatten()
    }

    async fn feature_exists(&self, project_id: &str, feature_id: &str) -> bool {
        let pm = self.pm.read().await;
        pm.get_project_by_id(project_id)
            .map(|p| p.features.iter().any(|f| f.id == feature_id))
            .unwrap_or(false)
    }

    /// Reject feature spans outside [1, project.length] (1-based).
    /// resolve_feature_span only checks start<=end (no upper bound), so
    /// without this a caller could write a feature with end = i64::MAX and
    /// later panic downstream code that slices the sequence by these
    /// coordinates. `start`/`end` are internal 0-based inclusive; for a
    /// wrapping feature start > end, so every segment end is checked too.
    async fn span_within_bounds(
        &self,
        project_id: &str,
        segments: &[Segment],
        start: i64,
        end: i64,
    ) -> Result<(), String> {
        let pm = self.pm.read().await;
        let plen = pm.get_project_by_id(project_id).map(|p| p.length).unwrap_or(0);
        let max_coord = segments
            .iter()
            .map(|s| s.end.max(s.start))
            .max()
            .unwrap_or(0)
            .max(start)
            .max(end);
        if max_coord >= plen {
            return Err(format!(
                "feature span {}..{} is out of range for project length {} (1-based inclusive)",
                start + 1,
                end + 1,
                plen
            ));
        }
        Ok(())
    }

    /// A `{"error": ...}` payload from a shared core means a tool-level failure.
    fn payload_error(payload: &serde_json::Value) -> Option<String> {
        payload.get("error").and_then(|v| v.as_str()).map(String::from)
    }

    /// Convert one batch item of `convert_sequence`: resolve the input mode,
    /// infer/validate the from→to pair, run the conversion and build the
    /// per-item result JSON (the caller adds `index` / `ok`).
    async fn convert_one(&self, item: &ConvertItem) -> Result<serde_json::Value, String> {
        let mode = resolve_optimize_input(
            item.project_id.as_deref(),
            item.feature_id.as_deref(),
            item.sequence.as_deref(),
            item.input_path.as_deref(),
        )?;

        let apply = item.apply.unwrap_or(false);
        if !matches!(mode, OptimizeInput::Project { .. }) && apply && item.output_path.is_none() {
            return Err(
                "apply=true is only meaningful in project mode; in sequence/input_path mode pass `output_path` to write the result to a file (or set apply=false)"
                    .to_string(),
            );
        }
        if matches!(mode, OptimizeInput::Project { .. }) && item.output_path.is_some() {
            return Err(
                "output_path is only supported in sequence/input_path modes; in project mode use apply=true to write the optimized CDS back into the project, then save_file to export a file"
                    .to_string(),
            );
        }
        if let Some(op) = &item.output_path {
            crate::validate_user_path(op, crate::CONVERT_OUTPUT_EXTS)
                .map_err(|e| format!("invalid output_path: {}", e))?;
            if !item.overwrite.unwrap_or(false) && std::path::Path::new(op).exists() {
                return Err(format!(
                    "{} already exists — pass overwrite: true to replace it, or choose a different output_path",
                    op
                ));
            }
        }

        match mode {
            OptimizeInput::Project { project_id, feature_id } => {
                if let Some(f) = &item.from {
                    if f != "dna" {
                        return Err(format!(
                            "project mode is dna→dna codon optimization; from=\"{}\" is not supported (projects hold the molecule they hold — export a region with save_file and use input_path/sequence for {} input)",
                            f, f
                        ));
                    }
                }
                let to = item.to.clone().unwrap_or_else(|| "dna".to_string());
                if to != "dna" {
                    return Err(format!(
                        "project mode only supports dna→dna codon optimization (to=\"{}\" requested); for conversions export the region with save_file first, then use input_path",
                        to
                    ));
                }
                let species = item.species.clone().ok_or_else(|| {
                    "species is required in project mode (codon optimization needs a codon usage table from list_species)"
                        .to_string()
                })?;
                self.convert_project_item(item, project_id, feature_id, &species, apply)
                    .await
            }
            OptimizeInput::Sequence(seq) => {
                let from = item.from.clone().unwrap_or_else(|| "dna".to_string());
                let to = default_to(item.to.as_deref(), &from);
                check_conversion(&from, &to, item)?;
                require_species_for(&from, &to, item)?;
                self.convert_sequence_input(item, seq, &from, &to).await
            }
            OptimizeInput::File { path, feature_id } => {
                self.convert_file_input(item, path, feature_id).await
            }
        }
    }

    /// Project mode: codon-optimize a CDS/mRNA feature inside an open DNA
    /// project (preview by default, write-back on apply=true).
    async fn convert_project_item(
        &self,
        item: &ConvertItem,
        project_id: String,
        feature_id: String,
        species: &str,
        apply: bool,
    ) -> Result<serde_json::Value, String> {
        let id = self.resolve_project_id(project_id).await.map_err(|e| e.message.to_string())?;
        if apply {
            self.require_agent_tab(&id).await.map_err(|e| e.message.to_string())?;
        }
        let project = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .cloned()
                .ok_or_else(|| format!("Project not found: {}", id))?
        };
        if !project.is_dna() {
            return Err(format!(
                "convert_sequence project mode re-encodes a CDS feature inside a DNA project; a {} project has no coding DNA to re-encode — pass `sequence` or `input_path` instead (a protein .gpt/.prot file or sequence with from=\"protein\" is reverse-translated to optimized DNA)",
                project.molecule_type
            ));
        }
        let method = item.method.clone().unwrap_or_else(|| "use_best_codon".to_string());
        let f_id = feature_id.clone();
        let sp = species.to_string();
        let m = method.clone();
        let os = item.original_species.clone();
        let aes = item.avoid_enzyme_sites.clone();
        let (new_sequence, result, coding) = tokio::task::spawn_blocking(move || {
            crate::codon_optimize(&project, &f_id, &sp, &m, None, os.as_deref(), aes, None)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))??;

        let mut v = codon_preview_json(&result, &coding.aa, coding.codons.len(), &method, species);
        v["from"] = serde_json::json!("dna");
        v["to"] = serde_json::json!("dna");
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
            let payload = crate::do_update_sequence(
                &self.app_handle,
                &self.pm,
                &self.wp,
                &self.agent_tabs,
                None,
                id.clone(),
                new_sequence,
                None,
            )
            .await
            .map_err(|e| e.to_string())?;
            if let Some(err) = Self::payload_error(&payload) {
                return Err(err);
            }
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
        // After apply this is the post-mutation hash; a preview leaves the
        // sequence unchanged, so the same read serves both.
        if let Some(h) = self.project_seq_hashes(&id).await {
            insert_seq_hashes(&mut v, &h);
        }
        Ok(v)
    }

    /// Standalone `sequence` input: clean per `from`, run the from→to
    /// conversion, optionally write the result to a file.
    async fn convert_sequence_input(
        &self,
        item: &ConvertItem,
        sequence: String,
        from: &str,
        to: &str,
    ) -> Result<serde_json::Value, String> {
        let optimize = from == "dna" && to == "dna" && wants_optimization(item);
        let cleaned = match from {
            "dna" if optimize => clean_coding_sequence(&sequence)?,
            "dna" => clean_na_sequence(&sequence, "ACGT")?,
            "rna" => clean_na_sequence(&sequence, "ACGU")?,
            _ => sequence.clone(),
        };
        let species = item.species.clone();
        let method = item.method.clone().unwrap_or_else(|| "use_best_codon".to_string());
        let os = item.original_species.clone();
        let aes = item.avoid_enzyme_sites.clone();
        let rev = item.rev_comp.unwrap_or(false);
        let f = from.to_string();
        let t = to.to_string();
        let conv = tokio::task::spawn_blocking(move || {
            convert_sequence_text(&cleaned, &f, &t, optimize, rev, species, &method, os, aes)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))??;

        let mut v = standalone_result_json(&conv, from, to);
        // Hashes describe the INPUT sequence (whitespace/case-insensitive, so
        // hashing the raw text equals hashing the cleaned form).
        insert_seq_hashes(&mut v, &libregene_core::utils::orientation_hashes(&sequence, from));
        if let Some(op) = &item.output_path {
            let written = write_convert_output(op, &conv.sequence, to, None, None)?;
            v["path"] = serde_json::json!(written);
        }
        Ok(v)
    }

    /// Standalone `input_path` input: parse the file (its molecule type
    /// defaults `from`), run the conversion, optionally write the result.
    async fn convert_file_input(
        &self,
        item: &ConvertItem,
        path: String,
        feature_id: Option<String>,
    ) -> Result<serde_json::Value, String> {
        crate::validate_user_path(&path, crate::SEQ_EXTS)
            .map_err(|e| format!("invalid input_path: {}", e))?;
        let p = path.clone();
        let project = tokio::task::spawn_blocking(move || {
            libregene_core::file_io::parse_file(std::path::Path::new(&p)).map_err(|e| {
                format!(
                    "failed to read {} (supported: .gbk/.gb/.genbank, .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1): {}",
                    p, e
                )
            })
        })
        .await
        .map_err(|e| format!("task join error: {}", e))??;

        let file_mol = if project.molecule_type.is_empty() {
            "dna"
        } else {
            project.molecule_type.as_str()
        };
        let from = match &item.from {
            Some(f) => {
                if f != file_mol {
                    return Err(format!(
                        "input file {} is a {} sequence but from=\"{}\" was given — drop `from` to use the file's molecule type",
                        path, file_mol, f
                    ));
                }
                f.clone()
            }
            None => file_mol.to_string(),
        };
        let to = default_to(item.to.as_deref(), &from);
        check_conversion(&from, &to, item)?;
        require_species_for(&from, &to, item)?;

        let species = item.species.clone();
        let method = item.method.clone().unwrap_or_else(|| "use_best_codon".to_string());
        let os = item.original_species.clone();
        let aes = item.avoid_enzyme_sites.clone();
        let rev = item.rev_comp.unwrap_or(false);

        // Feature-CDS codon optimization inside a DNA file: write-back
        // through the template so the full sequence (CDS replaced) survives.
        if from == "dna" && to == "dna" && feature_id.is_some() && wants_optimization(item) {
            let fid = feature_id.unwrap();
            let fid_label = fid.clone();
            let sp = species.clone().expect("require_species_for gates optimization");
            let m = method.clone();
            let proj = project.clone();
            let (new_sequence, result, coding) = tokio::task::spawn_blocking(move || {
                crate::codon_optimize(&proj, &fid, &sp, &m, None, os.as_deref(), aes, None)
            })
            .await
            .map_err(|e| format!("task join error: {}", e))??;
            let message = format!(
                "Codon optimization for {} (file input, {}): CAI {:.3} → {:.3}, {} repairs, {} unresolved",
                fid_label,
                species.clone().unwrap_or_default(),
                result.cai_before,
                result.cai_after,
                result.repairs.len(),
                result.unresolved.len(),
            );
            let mut v = codon_preview_json(
                &result,
                &coding.aa,
                coding.codons.len(),
                &method,
                species.as_deref().unwrap_or(""),
            );
            v["from"] = serde_json::json!("dna");
            v["to"] = serde_json::json!("dna");
            v["sequence"] = serde_json::json!(new_sequence);
            v["length"] = serde_json::json!(new_sequence.len());
            v["message"] = serde_json::json!(message);
            insert_seq_hashes(
                &mut v,
                &libregene_core::utils::orientation_hashes(&project.sequence, "dna"),
            );
            if let Some(op) = &item.output_path {
                let mut source_project = project.clone();
                source_project.sequence = new_sequence.clone();
                source_project.length = new_sequence.len() as i64;
                let cds_name = std::path::Path::new(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().replace(' ', "_"));
                let written =
                    write_convert_output(op, &new_sequence, "dna", Some(&source_project), cds_name.as_deref())?;
                v["path"] = serde_json::json!(written);
            }
            return Ok(v);
        }
        if feature_id.is_some() {
            return Err(
                "feature_id is only meaningful for dna→dna codon optimization (pass `species` to optimize, or drop `feature_id` to convert the whole file sequence)"
                    .to_string(),
            );
        }

        let raw = project.sequence.to_ascii_uppercase();
        let optimize = from == "dna" && to == "dna" && wants_optimization(item);
        let cleaned = match from.as_str() {
            "dna" if optimize => clean_coding_sequence(&raw)
                .map_err(|e| format!("invalid file sequence: {}", e))?,
            "dna" => clean_na_sequence(&raw, "ACGT")
                .map_err(|e| format!("invalid file sequence: {}", e))?,
            "rna" => clean_na_sequence(&raw, "ACGU")
                .map_err(|e| format!("invalid file sequence: {}", e))?,
            _ => raw,
        };
        let f = from.clone();
        let t = to.clone();
        let conv = tokio::task::spawn_blocking(move || {
            convert_sequence_text(&cleaned, &f, &t, optimize, rev, species, &method, os, aes)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))??;

        let mut v = standalone_result_json(&conv, &from, &to);
        // Hashes describe the input file's parsed sequence.
        insert_seq_hashes(
            &mut v,
            &libregene_core::utils::orientation_hashes(&project.sequence, &from),
        );
        if let Some(op) = &item.output_path {
            // Label the minimal project's CDS after the source file (e.g.
            // CAR.gpt → a "CAR" CDS), saving a rename step downstream.
            let cds_name = std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().replace(' ', "_"));
            let written =
                write_convert_output(op, &conv.sequence, &to, None, cds_name.as_deref())?;
            v["path"] = serde_json::json!(written);
        }
        Ok(v)
    }
}

/// The outcome of a standalone (sequence/file) conversion: the converted
/// sequence plus, for codon-optimizing paths, the optimizer preview fields.
struct Conversion {
    sequence: String,
    preview: Option<serde_json::Value>,
    message: String,
}

/// `to` defaults: reverse translation for a protein input, otherwise a
/// molecule-preserving conversion.
fn default_to(to: Option<&str>, from: &str) -> String {
    match to {
        Some(t) => t.to_string(),
        None if from == "protein" => "dna".to_string(),
        None => from.to_string(),
    }
}

/// The item asks for codon optimization (dna→dna) when any optimizer
/// parameter is present.
fn wants_optimization(item: &ConvertItem) -> bool {
    item.species.is_some()
        || item.method.is_some()
        || item.original_species.is_some()
        || item.avoid_enzyme_sites.is_some()
}

/// Validate a from→to pair and its revComp combination (per item).
fn check_conversion(from: &str, to: &str, item: &ConvertItem) -> Result<(), String> {
    for m in [from, to] {
        if !matches!(m, "dna" | "rna" | "protein") {
            return Err(format!(
                "invalid molecule type \"{}\" (expected \"dna\" | \"rna\" | \"protein\")",
                m
            ));
        }
    }
    if from == "protein" && to == "protein" {
        return Err("protein→protein conversion is not supported".to_string());
    }
    if item.rev_comp.unwrap_or(false) && (from == "protein" || to == "protein") {
        return Err(
            "revComp is only meaningful for nucleotide→nucleotide conversions (a protein has no complement)"
                .to_string(),
        );
    }
    if item.rev_comp.unwrap_or(false) && from == "dna" && to == "dna" && wants_optimization(item) {
        return Err("revComp cannot be combined with codon optimization".to_string());
    }
    Ok(())
}

/// Codon-optimizing paths (dna→dna with optimizer parameters, protein→dna/rna
/// reverse translation) need a codon usage table.
fn require_species_for(from: &str, to: &str, item: &ConvertItem) -> Result<(), String> {
    let optimizing =
        from == "protein" || (from == "dna" && to == "dna" && wants_optimization(item));
    if optimizing && item.species.is_none() {
        return Err(
            "species is required for codon optimization / reverse translation (a key from list_species, e.g. \"e_coli\")"
                .to_string(),
        );
    }
    Ok(())
}

/// Run a standalone sequence conversion (shared by `sequence` and
/// `input_path` modes; `input` is already cleaned, in the `from` alphabet).
fn convert_sequence_text(
    input: &str,
    from: &str,
    to: &str,
    optimize: bool,
    rev_comp: bool,
    species: Option<String>,
    method: &str,
    original_species: Option<String>,
    avoid_enzyme_sites: Option<Vec<String>>,
) -> Result<Conversion, String> {
    if from == "protein" {
        let sp = species.expect("require_species_for gates protein input");
        let aa: String = input
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let table = crate::codon_usage_table(&sp, None)?;
        let opts =
            crate::codon_optimize_options(method, original_species.as_deref(), avoid_enzyme_sites, None)?;
        let result = libregene_core::codon::optimize_from_aa(&aa, &table, &opts)?;
        let dna: String = result.new_codons.concat();
        let out = if to == "rna" {
            libregene_core::utils::to_rna(&dna)
        } else {
            dna
        };
        let unit = if to == "rna" { "nt RNA" } else { "bp DNA" };
        let message = format!(
            "Reverse translation ({} → {}, {}): {} aa → {} {}, {} repairs, {} unresolved",
            from,
            to,
            sp,
            aa.chars().count(),
            out.len(),
            unit,
            result.repairs.len(),
            result.unresolved.len(),
        );
        return Ok(Conversion {
            sequence: out,
            preview: Some(codon_preview_json(&result, &aa, aa.chars().count(), method, &sp)),
            message,
        });
    }
    if optimize {
        let sp = species.expect("require_species_for gates codon optimization");
        let codons: Vec<String> = input
            .as_bytes()
            .chunks(3)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        let table = crate::codon_usage_table(&sp, None)?;
        let opts =
            crate::codon_optimize_options(method, original_species.as_deref(), avoid_enzyme_sites, None)?;
        let result = libregene_core::codon::optimize_codons(&codons, &table, &opts);
        let aa: String = codons
            .iter()
            .map(|c| table.aa_of.get(c).copied().unwrap_or('?'))
            .collect();
        let optimized: String = result.new_codons.concat();
        let message = format!(
            "Codon optimization ({}): CAI {:.3} → {:.3}, GC {:.1}% → {:.1}%, {} repairs, {} unresolved",
            sp,
            result.cai_before,
            result.cai_after,
            result.gc_before * 100.0,
            result.gc_after * 100.0,
            result.repairs.len(),
            result.unresolved.len(),
        );
        return Ok(Conversion {
            sequence: optimized,
            preview: Some(codon_preview_json(&result, &aa, codons.len(), method, &sp)),
            message,
        });
    }
    let dna = if from == "rna" {
        libregene_core::utils::to_dna(input)
    } else {
        input.to_string()
    };
    let dna = if rev_comp {
        libregene_core::utils::reverse_complement(&dna)
    } else {
        dna
    };
    let out = match to {
        "rna" => libregene_core::utils::to_rna(&dna),
        "protein" => String::from_utf8(libregene_core::translate::translate_nt(dna.as_bytes()))
            .map_err(|e| e.to_string())?,
        _ => dna,
    };
    let unit = if to == "protein" { "aa" } else { "nt" };
    let message = format!(
        "Converted {} → {}{}: {} {}",
        from,
        to,
        if rev_comp { " (reverse-complemented)" } else { "" },
        out.len(),
        unit
    );
    Ok(Conversion { sequence: out, preview: None, message })
}

/// The per-item result JSON shared by the sequence/input_path modes.
fn standalone_result_json(conv: &Conversion, from: &str, to: &str) -> serde_json::Value {
    let mut v = conv.preview.clone().unwrap_or_else(|| serde_json::json!({}));
    v["from"] = serde_json::json!(from);
    v["to"] = serde_json::json!(to);
    v["sequence"] = serde_json::json!(conv.sequence);
    v["length"] = serde_json::json!(conv.sequence.len());
    v["message"] = serde_json::json!(conv.message);
    v
}

// ---------------------------------------------------------------------------
// save_file region mode: region resolution + export data building
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

/// Resolve a save_file `region` selector to the template pieces it exports:
/// linear internal 0-based inclusive spans in EXPORT order, `flip` (each
/// piece's sequence is reverse-complemented when exporting a minus-strand
/// feature) and a human-readable description of the selected region (1-based,
/// like every agent-facing string). The selector's region `start`/`end` are
/// already converted to internal 0-based by the caller; `cut1`/`cut2` are
/// still the raw 1-based flanking-base numbers and are converted here.
/// Exactly one selector must be given; mixing selectors is rejected.
fn resolve_export_region(
    project: &ProjectData,
    req: &RegionSpec,
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
            // Same shape as the region-selector check above; feature
            // coordinates come from parsed files, so every bound (including
            // e < 0 from a crafted range and s >= len from a wrap split of an
            // out-of-range start) must be rejected before the pieces are
            // sliced. saturating_add: a rejected piece may hold the i64 seeds.
            if s < 0 || e < 0 || s >= len || e >= len {
                return Err(format!(
                    "feature {} coordinate {}..{} out of range for sequence of length {} (1-based inclusive)",
                    f.id,
                    s.saturating_add(1),
                    e.saturating_add(1),
                    len
                ));
            }
        }
        // Keep join order: for origin-wrapping (or otherwise non-ascending)
        // segments the biological 5'→3' order is the stored join order —
        // reversed for the minus strand — NOT coordinate-ascending order.
        let minus = f.strand == "-";
        if minus && !project.is_dna() {
            // Minus-strand export reverse-complements each piece, which is
            // only meaningful for DNA — on RNA/protein projects (single-
            // strand sequences) it would corrupt the exported sequence.
            return Err(format!(
                "feature '{}' is on the minus strand, but minus-strand export (reverse complement) is only supported for DNA projects; this is a {} project",
                f.id, project.molecule_type
            ));
        }
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
                // Type-IIS enzymes cut outside their recognition site; on a
                // linear molecule a site near an end can place the cut before
                // base 1 or past the last base (circular cuts are normalized
                // into [0, len) by the engine). Slicing there would panic.
                if !circular {
                    for (name, c) in [(e1, c1), (e2, c2)] {
                        if c < 1 || c > len {
                            return Err(format!(
                                "cut of enzyme {} at position {} falls outside the linear molecule (1..={})",
                                name, c, len
                            ));
                        }
                    }
                }
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
    // A rev site wrapping the origin stores template_end = (start +
    // footprint) % len, so template_end < template_start (the == 0 case is
    // already mapped to r_end = len - 1 above). The amplicon then always
    // spans the origin — even when f_start <= r_end numerically, the forward
    // arc from the fwd 5' end reaches the rev 5' end only across the origin.
    let rev_wraps = circular && rsite.template_end != 0 && rsite.template_end < rsite.template_start;
    let pieces = if circular {
        if !rev_wraps && f_start <= r_end {
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
        // Circular origin-wrapping sites are stored with template_end =
        // (start + footprint) % len, i.e. se < ss; split the span into its
        // two arcs so the overlap test below sees both (otherwise the
        // inverted span matches nothing and the primer is silently dropped).
        let spans: Vec<(i64, i64)> = if se < ss && project.topology == "circular" {
            vec![(ss, project.length - 1), (0, se)]
        } else {
            vec![(ss, se)]
        };
        let mut mapped: Vec<(i64, i64)> = Vec::new();
        for (pi, &(ps, pe)) in pieces.iter().enumerate() {
            let (wo, _) = windows[pi];
            for &(s, e) in &spans {
                let os = s.max(ps);
                let oe = e.min(pe);
                if os > oe {
                    continue;
                }
                let (ns, ne) = if flip {
                    (wo + (pe - oe), wo + (pe - os))
                } else {
                    (wo + (os - ps), wo + (oe - ps))
                };
                mapped.push((ns, ne));
            }
        }
        let merged = merge_sorted_segments(mapped);
        let (ns, ne) = match merged.len() {
            0 => continue,
            1 => merged[0],
            // Both arcs of an origin-wrapping site were exported but stay
            // disjoint in the linear export (e.g. a full-circle export):
            // keep the wrapped form (template_end < template_start) so every
            // bound base survives — gbk.rs writes it as a join.
            _ => (merged[merged.len() - 1].0, merged[0].1),
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

/// Feature/translation hits containing internal 0-based `position`,
/// serialized for read_sequence: every containing feature with its 1-based
/// feature-relative offset, plus CDS/mRNA codon/amino-acid details. Used for
/// the window mode's startContext/endContext and the coordinate modes' hit
/// details.
fn position_context_json(
    position: i64,
    sequence: &str,
    features: &[Feature],
) -> serde_json::Value {
    let feature_hits = libregene_core::coords::position_to_features(position, features);
    let translation_hits =
        libregene_core::coords::position_to_translations(position, sequence, features);
    serde_json::json!({
        "position": position + 1,
        "features": feature_hits.iter().map(|h| serde_json::json!({
            "featureId": h.feature_id,
            "name": h.name,
            "ftype": h.ftype,
            "strand": h.strand,
            "featureOffset": h.offset,
            "featureLength": h.length,
        })).collect::<Vec<_>>(),
        "translations": translation_hits.iter().map(|h| serde_json::json!({
            "featureId": h.feature_id,
            "name": h.name,
            "strand": h.strand,
            "codonIndex": h.codon_index,
            "aaPosition1Based": h.aa_position_1_based,
            "aaPositionExcludingMet": h.aa_position_excluding_met,
            "codon": h.codon,
            "aminoAcid": h.amino_acid.to_string(),
            "codonBaseIndex": h.codon_base_index,
        })).collect::<Vec<_>>(),
    })
}

#[tool_router]
impl<R: Runtime> LibreGeneMcp<R> {
    /// List all open projects — the files currently loaded into memory.
    /// A "project" is an open file: `open_project` loads a file as a project
    /// (bound as your agent tab) and returns its `projectId`; every other
    /// tool addresses a project by its required `project_id`. Returns
    /// {"projects": [{id, name, length, topology, dirty}], "activeId":
    /// id-or-null}. `activeId` is the project the USER is currently viewing —
    /// informational only; it does not influence tool routing, and when
    /// several agents work in parallel you should avoid touching it.
    /// Each project entry also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
    #[tool]
    async fn list_projects(&self) -> Result<Json<serde_json::Value>, ErrorData> {
        let pm = self.pm.read().await;
        let mut projects = pm.list_projects();
        for entry in projects.iter_mut() {
            if let Some(p) = entry
                .get("id")
                .and_then(|v| v.as_str())
                .and_then(|id| pm.get_project_by_id(id))
            {
                insert_seq_hashes(
                    entry,
                    &libregene_core::utils::orientation_hashes(&p.sequence, &p.molecule_type),
                );
            }
        }
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
    /// The response carries `sequenceHash`/`revCompHash` (7-hex hashes of the
    /// current biological sequence and its reverse complement,
    /// case-insensitive, ignoring features/primers/alignments; revCompHash is
    /// null for proteins) — compare across calls to detect any sequence
    /// change; a project and its reverse-complemented file share one
    /// (sequenceHash, revCompHash) pair. The `text` digest carries the same
    /// values as a `SEQHASH:` header line.
    #[tool]
    async fn get_project_overview(
        &self,
        Parameters(request): Parameters<OverviewRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
            compact_enzymes: false,
            compact_cutters: request.compact_cutters.unwrap_or(true),
            include_auto_annotation: true,
        };
        // Auto-annotation scans the whole feature database — CPU-heavy, so
        // render off the tokio worker.
        let text = tokio::task::spawn_blocking(move || project_digest(&project, &opts, None))
            .await
            .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut v = serde_json::json!({ "projectId": id, "text": text });
        insert_seq_hashes(&mut v, &hashes);
        Ok(Json(v))
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
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview; the digest `text` carries the same values as a
    /// `SEQHASH:` header line).
    #[tool]
    async fn get_region_view(
        &self,
        Parameters(request): Parameters<RegionRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
            compact_enzymes: request.compact.unwrap_or(true),
            compact_cutters: false,
            include_auto_annotation: false,
        };
        let region = (from1(request.start), from1(request.end));
        let text = tokio::task::spawn_blocking(move || project_digest(&project, &opts, Some(region)))
            .await
            .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut v = serde_json::json!({ "projectId": id, "text": text });
        insert_seq_hashes(&mut v, &hashes);
        Ok(Json(v))
    }

    /// Read bases of a project's sequence, or resolve a coordinate. Exactly
    /// one input form:
    ///
    /// WINDOW MODE — `start` + `end` (both required, 1-based inclusive; on
    /// circular sequences start > end wraps the origin; windows larger than
    /// 10000 bp are rejected). Returns {projectId, sequence, text,
    /// startContext, endContext} — `sequence` is the plain uppercase base
    /// string (machine-readable); `text` is the same window with a coordinate
    /// ruler (10 bp groups, 60 bp per line; the ruler line is omitted for
    /// windows of 60 bp or less, where the per-line position prefix is
    /// enough). `startContext`/`endContext` annotate the window's first/last
    /// base: each is {position, features, translations} listing every feature
    /// containing that position (with its 1-based feature-relative offset)
    /// and, inside a CDS/mRNA, the codon index, amino-acid position (two
    /// conventions) and amino acid.
    ///
    /// COORDINATE MODE — exactly one of:
    /// 1. `position`: a full-file 1-based inclusive template coordinate.
    /// 2. `feature_id` + `feature_offset`: 1-based offset along the feature's
    ///    own 5'→3' direction (reverse-complemented features count from their
    ///    3' end on the template).
    /// 3. `feature_id` + `aa_position`: 1-based amino-acid position within a
    ///    CDS/mRNA feature, INCLUDING the initiator Met (Met = 1). Literature
    ///    numbering that skips the Met (e.g. mEGFP A206K) corresponds to the
    ///    response's `aaPositionExcludingMet`, so convert before calling:
    ///    literature position + 1 (when the Met is present) is the
    ///    `aa_position` to send.
    /// Returns {projectId, input, position, base, codonPositions?, features,
    /// translations, sequence, text}: `position` echoes the resolved absolute
    /// coordinate (1-based) and `base` the template base there (plus-strand,
    /// uppercase; the residue letter on protein projects); `features`/
    /// `translations` are the full hit details for that position (same shape
    /// as the window mode's contexts); for amino-acid input `codonPositions`
    /// holds the three template positions of the requested codon in 5'→3'
    /// biological order (and `position` is the first of them); `sequence`/
    /// `text` give the `flank`-bp window around the position (default 30,
    /// clamped at the sequence ends). Translation hits only appear when the
    /// position falls inside a CDS/mRNA feature.
    ///
    /// This tool is for INSPECTING bases only: if you need to hand this
    /// sequence (or part of it) to another tool or file, write it to a file
    /// with save_file's `region` instead of copying the text.
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
    #[tool]
    async fn read_sequence(
        &self,
        Parameters(request): Parameters<SequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        let seq_hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        // Reads never mutate, so the resolve-time hash stays valid; the
        // closure also serves fail paths that run inside a lock below (no
        // lock acquisition allowed there).
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            insert_seq_hashes(&mut v, &seq_hashes);
            Json(v)
        };
        let window_active = request.start.is_some() || request.end.is_some();
        let coord_active = request.position.is_some()
            || request.feature_id.is_some()
            || request.feature_offset.is_some()
            || request.aa_position.is_some();
        if window_active == coord_active {
            return Ok(fail(
                "Provide exactly one input form: `start` + `end` (window read); `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
            ));
        }

        if window_active {
            let (s, e) = match (request.start, request.end) {
                (Some(s), Some(e)) => (from1(s), from1(e)),
                _ => {
                    return Ok(fail(
                        "start and end are both required (1-based inclusive)".to_string(),
                    ))
                }
            };
            let text = read_sequence(&project, s, e)
                .map_err(|e| ErrorData::invalid_params(e, None))?;
            let bases = libregene_core::digest::read_sequence_bases(&project, s, e)
                .map_err(|e| ErrorData::invalid_params(e, None))?;
            let mut v = serde_json::json!({
                "projectId": id,
                "sequence": bases,
                "text": text,
                "startContext": position_context_json(s, &project.sequence, &project.features),
                "endContext": position_context_json(e, &project.sequence, &project.features),
            });
            insert_seq_hashes(&mut v, &seq_hashes);
            return Ok(Json(v));
        }

        // Coordinate mode: resolve the input form to an absolute 0-based
        // template position, then report hit details + a flank window.
        let sequence = &project.sequence;
        let features = &project.features;
        let len = sequence.len() as i64;

        let position: i64;
        let input_json: serde_json::Value;
        let codon_positions_opt: Option<[i64; 3]>;

        let has_position = request.position.is_some() as u8;
        let has_feature_offset = request.feature_id.is_some() && request.feature_offset.is_some();
        let has_aa_position = request.feature_id.is_some() && request.aa_position.is_some();
        if has_position + has_feature_offset as u8 + has_aa_position as u8 != 1 {
            return Ok(fail(
                "Provide exactly one of: `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
            ));
        }

        if let Some(pos1) = request.position {
            if pos1 < 1 || pos1 > len {
                return Ok(fail(
                    format!("position {} out of bounds (1..={})", pos1, len),
                ));
            }
            position = pos1 - 1;
            input_json = serde_json::json!({ "kind": "template", "position": pos1 });
            codon_positions_opt = None;
        } else if let Some(feature_id) = &request.feature_id {
            let f = features
                .iter()
                .find(|f| &f.id == feature_id)
                .ok_or_else(|| ErrorData::invalid_params(format!("feature '{}' not found", feature_id), None))?;
            if let Some(offset1) = request.feature_offset {
                match libregene_core::coords::position_from_feature_offset(f, offset1) {
                    Ok(pos0) => {
                        // Feature coordinates are file-derived and not
                        // range-checked at parse time; reject before the
                        // sequence indexing below panics.
                        if pos0 < 0 || pos0 >= len {
                            return Ok(fail(
                                format!(
                                    "feature '{}' coordinates fall outside the sequence (length {})",
                                    f.name, len
                                ),
                            ));
                        }
                        position = pos0;
                        input_json = serde_json::json!({
                            "kind": "featureOffset",
                            "featureId": feature_id,
                            "featureOffset": offset1,
                        });
                        codon_positions_opt = None;
                    }
                    Err(e) => return Ok(fail(e)),
                }
            } else if let Some(aa1) = request.aa_position {
                match libregene_core::coords::codon_from_aa(f, sequence, aa1) {
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
                    Err(e) => return Ok(fail(e)),
                }
            } else {
                // Unreachable because of the mutual-exclusion check above.
                return Ok(fail(
                    "Provide exactly one of: `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
                ));
            }
        } else {
            return Ok(fail(
                "Provide exactly one of: `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
            ));
        }

        let flank = request.flank.unwrap_or(30).clamp(0, MAX_FLANK);
        let ws = position.saturating_sub(flank).max(0);
        let we = position.saturating_add(flank).min(len - 1);
        let ctx = position_context_json(position, sequence, features);
        let text = read_sequence(&project, ws, we)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let bases = libregene_core::digest::read_sequence_bases(&project, ws, we)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut v = serde_json::json!({
            "projectId": id,
            "input": input_json,
            "position": position + 1,
            "base": sequence[position as usize..position as usize + 1].to_ascii_uppercase(),
            "features": ctx["features"],
            "translations": ctx["translations"],
            "sequence": bases,
            "text": text,
        });
        if let Some(positions) = codon_positions_opt {
            v["codonPositions"] = serde_json::json!(
                positions.iter().map(|p| p + 1).collect::<Vec<_>>()
            );
        }
        insert_seq_hashes(&mut v, &seq_hashes);
        Ok(Json(v))
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
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
    #[tool]
    async fn search_sequence(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        let hashes = self.project_seq_hashes(&id).await;
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
        let mut v = serde_json::json!({ "projectId": id, "matches": matches });
        if let Some(h) = &hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }

    /// List restriction-enzyme recognition sites on a project's sequence.
    /// `enzymes` is an optional list of enzyme names (case-insensitive); omit
    /// it (or pass []) to report every enzyme that has a site. Requested names
    /// fall into three classes: cutting this sequence (normal entry), in the
    /// enzyme database but WITHOUT a site on this sequence (entry with empty
    /// `sites` and a `note` saying so — this is the answer for e.g. a site
    /// destroyed by cloning), and unknown to the database. A batch query
    /// degrades gracefully: known names return normally and unknown names are
    /// listed under `unknownEnzymes` ([{name, error, similar}]) without
    /// failing the whole call; only when EVERY requested name is unknown does
    /// the call fail with near-match suggestions — use that error to probe
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
    /// for that enzyme. On circular molecules every coordinate stays within
    /// 1..=len; a recognition sequence spanning the origin reads
    /// recStart > recEnd. Sites whose cuts fall OUTSIDE the recognition
    /// sequence (type IIS enzymes like BbsI) carry
    /// `cutsOutsideRecognitionSite: true` plus a `note`; for those,
    /// topCutIndex/botCutIndex — not recStart/recEnd — give the actual break
    /// points.
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
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
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
        let hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let wanted: Option<Vec<String>> = request.enzymes.map(|v| {
            v.into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        });
        // The engine only stores entries with at least one recognition site,
        // so requested names fall into three classes: cutting this sequence
        // (resolved), in the enzyme database but no site here (no_site), and
        // unknown to the database (unknown). Batch queries degrade
        // gracefully: known names return normally and only truly unknown
        // names are listed under `unknownEnzymes`; a query where EVERY name
        // is unknown still fails with near-match suggestions (the enzyme-name
        // probe).
        let mut requested: Vec<String> = Vec::new();
        let mut no_site: Vec<String> = Vec::new();
        let mut unknown: Vec<(String, Vec<String>)> = Vec::new();
        if let Some(list) = wanted.as_ref().filter(|l| !l.is_empty()) {
            let names: Vec<&str> = project.enzymes.iter().map(|e| e.name.as_str()).collect();
            let db = libregene_core::enzyme::search::get_db();
            for n in list {
                if let Some(found) = names.iter().find(|a| a.eq_ignore_ascii_case(n)) {
                    if !requested.iter().any(|r| r.eq_ignore_ascii_case(found)) {
                        requested.push(found.to_string());
                    }
                    continue;
                }
                if let Some(e) = db.enzymes.iter().find(|e| e.name.eq_ignore_ascii_case(n)) {
                    if !no_site.iter().any(|r| r.eq_ignore_ascii_case(&e.name)) {
                        no_site.push(e.name.clone());
                    }
                    continue;
                }
                let q = n.to_lowercase();
                let sugg: Vec<String> = names
                    .iter()
                    .copied()
                    .filter(|a| a.to_lowercase().contains(&q))
                    .take(5)
                    .map(|s| s.to_string())
                    .collect();
                unknown.push((n.clone(), sugg));
            }
            if !unknown.is_empty() && requested.is_empty() && no_site.is_empty() {
                let msg = unknown
                    .iter()
                    .map(|(n, sugg)| {
                        if sugg.is_empty() {
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
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                let mut v = fail_envelope(&id, msg);
                insert_seq_hashes(&mut v, &hashes);
                return Ok(Json(v));
            }
        }
        let filter_active = !requested.is_empty() || !no_site.is_empty() || !unknown.is_empty();
        let mut by_name: HashMap<&str, Vec<&Enzyme>> = HashMap::new();
        for e in &project.enzymes {
            if !filter_active || requested.iter().any(|w| w.eq_ignore_ascii_case(&e.name)) {
                by_name.entry(e.name.as_str()).or_default().push(e);
            }
        }
        let mut enzyme_names: Vec<&str> = by_name.keys().copied().collect();
        enzyme_names.sort();
        let circular = project.topology == "circular";
        let tlen = project.length;
        let mut enzymes_json: Vec<serde_json::Value> = enzyme_names
            .into_iter()
            .map(|n| {
                let mut sites = by_name[n].clone();
                // Circular display frames can store rec_start/rec_end shifted
                // by a whole sequence length (the engine's comment notes the
                // frontend wraps indices mod seq_len); wrap at this boundary so
                // MCP coordinates always land in 1..=len. A recognition
                // spanning the origin then reads recStart > recEnd.
                let wrap = |x: i64| -> i64 {
                    if circular { x.rem_euclid(tlen) } else { x }
                };
                sites.sort_by_key(|e| wrap(e.rec_start));
                serde_json::json!({
                    "name": n,
                    "sites": sites.iter().map(|e| {
                        let rec_start = to1(wrap(e.rec_start));
                        let rec_end = to1(wrap(e.rec_end));
                        let cuts: Vec<serde_json::Value> = e.cut_pairs.iter().map(|p| serde_json::json!({
                            "topCutIndex": cut_flanks(p.top_cut_index, tlen, circular).0,
                            "botCutIndex": cut_flanks(p.bot_cut_index, tlen, circular).0,
                        })).collect();
                        // Type IIS and similar enzymes cut outside their
                        // recognition sequence; flag those sites so the
                        // cut-vs-recognition offset does not have to be
                        // inferred from the coordinates alone.
                        let outside = if circular {
                            // Compare in the engine's unwrapped frame: a cut is
                            // inside the recognition when its modular distance
                            // from rec_start falls in 1..=span+1 (the same
                            // boundary semantics as the linear check below).
                            let span = e.rec_end - e.rec_start;
                            e.cut_pairs.iter().any(|p| {
                                let dt = (p.top_cut_index - e.rec_start).rem_euclid(tlen);
                                let db = (p.bot_cut_index - e.rec_start).rem_euclid(tlen);
                                dt == 0 || db == 0 || dt > span + 1 || db > span + 1
                            })
                        } else {
                            cuts.iter().any(|c| {
                                let t = c["topCutIndex"].as_i64().unwrap_or(0);
                                let b = c["botCutIndex"].as_i64().unwrap_or(0);
                                t < rec_start || t > rec_end || b < rec_start || b > rec_end
                            })
                        };
                        let mut site = serde_json::json!({
                            "recStart": rec_start,
                            "recEnd": rec_end,
                            "recSeq": e.rec_seq,
                            "strand": e.recognition_strand,
                            "cuts": cuts,
                            "cutsOutsideRecognitionSite": outside,
                            "methylationBlocked": e.methylation_blocked,
                            "unique": e.is_unique,
                        });
                        if outside {
                            site["note"] = serde_json::json!(
                                "cut positions lie outside the recognition sequence (type IIS-style); topCutIndex/botCutIndex give the actual break points"
                            );
                        }
                        site
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        // Known-in-database enzymes without a site on this sequence are
        // reported with empty sites and an explicit note, so an agent can
        // tell "no site here" apart from "unknown enzyme".
        for n in &no_site {
            enzymes_json.push(serde_json::json!({
                "name": n,
                "sites": [],
                "note": "enzyme exists in the enzyme database but has no recognition site on this sequence",
            }));
        }
        enzymes_json.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
        let mut resp = serde_json::json!({ "projectId": id, "enzymes": enzymes_json });
        insert_seq_hashes(&mut resp, &hashes);
        if !unknown.is_empty() {
            resp["unknownEnzymes"] = unknown
                .iter()
                .map(|(n, sugg)| {
                    serde_json::json!({
                        "name": n,
                        "error": format!("Unknown enzyme '{}': not in the enzyme database", n),
                        "similar": sugg,
                    })
                })
                .collect();
        }
        Ok(Json(resp))
    }

    /// List the primers stored in a project (read-only; never recomputes or
    /// checks binding). Returns {projectId, primers: [{id, name, type, seq,
    /// bindingSiteCount, sites: [{strand, templateStart, templateEnd}]}]}.
    /// templateStart/templateEnd are 1-based inclusive (the bound range spans
    /// templateStart..templateEnd, GenBank-style). bindingSiteCount is the
    /// number of recomputed binding sites (0 when the primer does not bind);
    /// sites are best-first (Tm descending, as the UI orders them).
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
    #[tool]
    async fn list_primers(
        &self,
        Parameters(request): Parameters<ListPrimersRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        let tlen = project.length;
        let circular = project.topology == "circular";
        let primers: Vec<serde_json::Value> = project
            .primers
            .iter()
            .map(|p| {
                let sites: Vec<serde_json::Value> = p
                    .binding_sites
                    .iter()
                    .map(|s| {
                        let mut site = serde_json::json!({
                            "strand": s.strand,
                            "templateStart": s.template_start,
                            "templateEnd": s.template_end,
                        });
                        site_json_to_1based(&mut site, tlen, circular);
                        site
                    })
                    .collect();
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "type": p.r#type,
                    "seq": p.primer_seq,
                    "bindingSiteCount": p.binding_sites.len(),
                    "sites": sites,
                })
            })
            .collect();
        let mut v = serde_json::json!({ "projectId": id, "primers": primers });
        insert_seq_hashes(
            &mut v,
            &libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type),
        );
        Ok(Json(v))
    }

    // -----------------------------------------------------------------------
    // Mutations
    // -----------------------------------------------------------------------

    /// Open a sequence file, load it as a new project AND bind it as your
    /// agent tab in one step (project id = file path; the returned
    /// `projectId` is how every other tool refers to it — see list_projects).
    /// Binding means: the project stays in the main window's sidebar (marked
    /// with a bot badge) and is LOCKED against user keyboard/pointer input
    /// while you work (the user can temporarily unlock it via an on-screen
    /// button, but any further MCP tool call on the project re-locks it).
    /// This is the entry point for handing a file to the app: whenever a
    /// sequence already exists as a file on disk, bring it in through this
    /// tool rather than pasting its text into other tools. Files are also the
    /// recommended way to move a sequence between projects (write with
    /// save_file, read back with open_project). Enzyme and primer recompute
    /// run on a background thread; the UI is refreshed via broadcast.
    /// If the path is already loaded: a project already bound to you is
    /// re-locked and returns {ok, projectId, locked, reused: true}; a project
    /// the USER opened is REFUSED — copy the file with bash `cp` to a new
    /// path and open_project the copy. Mutating tools (edit_sequence,
    /// set_feature, add_primer, add_alignment, save_file, convert_sequence/apply,
    /// find_orfs/add_as_features) REFUSE to run on projects not bound as an
    /// agent tab. Multiple agents each open their own copy and work in
    /// parallel without interfering.
    /// A fresh open returns {ok, message, projectId, regionView} where
    /// regionView is the compact overview digest of the opened project
    /// (enzyme cutters collapsed to a count line); a reused binding returns
    /// {ok, message, projectId, locked, reused: true} WITHOUT regionView —
    /// call get_project_overview if you need the digest. Both forms carry
    /// `sequenceHash`/`revCompHash` (see get_project_overview).
    #[tool]
    async fn open_project(
        &self,
        Parameters(request): Parameters<OpenProjectRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = request.path;
        // Fast path: already loaded → reuse the binding or reject. No load
        // happens on this path, so there is nothing to race with.
        let already_loaded = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id).is_some()
        };
        if already_loaded {
            return self.reuse_agent_tab_or_reject(&id).await;
        }
        crate::validate_user_path(&id, crate::SEQ_EXTS)
            .map_err(|e| ErrorData::invalid_params(format!("invalid path: {}", e), None))?;
        // Parse + recompute off the executor with no locks held (the same
        // pipeline do_open_file runs for the frontend open_file command).
        let path_buf = std::path::PathBuf::from(&id);
        let parsed = tokio::task::spawn_blocking(move || -> Result<ProjectData, String> {
            let mut project =
                libregene_core::file_io::parse_file(&path_buf).map_err(|e| e.to_string())?;
            libregene_core::enzyme::recompute(&mut project);
            libregene_core::primer::recompute(&mut project);
            Ok(project)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
        let project = match parsed {
            Ok(p) => p,
            Err(e) => return Ok(Json(fail_envelope(&id, e))),
        };
        // Atomic check-and-load under the pm write lock: the frontend's
        // open_file loads under the same write lock, so whoever takes it
        // first wins — if the user opened this file while we were parsing,
        // we see "already loaded" here and take the reuse/reject path
        // instead of clobbering their project and binding it as ours.
        let loaded = {
            let mut pm = self.pm.write().await;
            if pm.get_project_by_id(&id).is_some() {
                false
            } else {
                match pm.load(&id, project) {
                    Ok(()) => {
                        // A fresh load reflects the file on disk — clear any
                        // stale dirty marker from a previous incarnation.
                        pm.mark_clean(&id);
                        true
                    }
                    Err(e) => return Ok(Json(fail_envelope(&id, e))),
                }
            }
        };
        if !loaded {
            return self.reuse_agent_tab_or_reject(&id).await;
        }
        // The load may have evicted another project; drop its bindings.
        crate::prune_orphan_bindings(&self.pm, &self.wp, &self.agent_tabs).await;
        // Bind the freshly opened project as this agent's tab (locked).
        {
            let mut at = self.agent_tabs.write().await;
            at.insert(id.clone(), crate::AgentTabMeta { locked: true });
        }
        let _ = self.app_handle.emit(
            "agent-tab-lock",
            serde_json::json!({ "projectId": id, "locked": true }),
        );
        // A concurrent load between our load and the bind above could have
        // evicted this project; never leave a binding to a ghost behind.
        let still_loaded = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id).is_some()
        };
        if !still_loaded {
            self.agent_tabs.write().await.remove(&id);
            return Err(ErrorData::internal_error(
                format!(
                    "Project '{}' was evicted before it could be bound as an agent tab; retry open_project",
                    id
                ),
                None,
            ));
        }
        // The open_file command does not broadcast (frontend applies the
        // response) — the MCP server must notify the UI itself.
        crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, &self.agent_tabs, None).await;
        let summary = self.project_summary(&id).await.unwrap_or_else(|| format!("Opened {}", id));
        let region = self.digest_region(&id, None, true).await;
        let mut v = ok_envelope(&id, summary, region);
        if let Some(h) = self.project_seq_hashes(&id).await {
            insert_seq_hashes(&mut v, &h);
        }
        Ok(Json(v))
    }

    /// Save a project (addressed by the required `project_id`) to a file on
    /// disk — the reverse of open_project.
    ///
    /// WITHOUT `region`: the whole project (current sequence + features) is
    /// written through the same serializer and mark-clean logic as the
    /// save_file command (.gbk/.gb/.genbank for DNA/RNA projects, .gpt for
    /// protein projects; topology preserved). Returns the uniform envelope
    /// with the
    /// overview digest plus `bytesWritten` (file size in bytes, for write
    /// verification).
    ///
    /// WITH `region`: exports only that subsequence — THE recommended way to
    /// create a sequence file from a known region of an open project (then
    /// open_project the result to work with it as a project). NEVER retype or
    /// paste the sequence into edit_sequence/other tools to build a new
    /// construct — pasted sequences are error-prone. The file holds the
    /// region's sequence (uppercase; template strand except as noted) plus
    /// every feature overlapping it (partially covered features are clipped
    /// to the region) with coordinates translated to the new linear
    /// coordinate system, and every primer whose primary binding site
    /// overlaps the region at all; circular projects always export linear
    /// fragments and the project is NOT marked clean. Exactly ONE selector
    /// inside `region` (mixing selectors is rejected):
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
    /// Overwrite rule: when `path` already exists and is NOT the project's
    /// own source path, `overwrite: true` is required — otherwise the call
    /// fails with a hint to choose a different path or overwrite explicitly.
    /// A WHOLE-PROJECT save over the project's own file (scratch-copy
    /// iteration) needs no flag; a REGION export over the project's own file
    /// also requires `overwrite: true` (it would replace the full source
    /// file with just the fragment).
    ///
    /// Region mode returns {ok, message, projectId, outputPath, length,
    /// primers?, regionView?}: `length` is the exported sequence length
    /// (bp/nt/aa); `primers` lists the names of primers written with the file
    /// (omitted when none); `regionView` is a compact digest of the source
    /// project over the exported region's bounding box. The exported sequence
    /// itself is NOT echoed — read it back with open_project/read_sequence on
    /// the written file.
    /// The response also carries `sequenceHash`/`revCompHash` of the project's
    /// current in-memory sequence (see get_project_overview) — they describe
    /// the project, not the exported region.
    #[tool]
    async fn save_file(
        &self,
        Parameters(request): Parameters<SaveFileRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            insert_seq_hashes(&mut v, &seq_hashes);
            Json(v)
        };
        let path = request.path.clone();
        // Overwrite rule: a pre-existing target that is not the project's own
        // source file needs an explicit overwrite flag.
        if path != id
            && !request.overwrite.unwrap_or(false)
            && std::path::Path::new(&path).exists()
        {
            return Ok(fail(
                format!(
                    "{} already exists — pass overwrite: true to replace it, or choose a different path",
                    path
                ),
            ));
        }

        let Some(spec) = request.region else {
            // Whole-project save.
            let payload = crate::do_save_file(&self.pm, id.clone(), path.clone())
                .await
                .map_err(|e| ErrorData::internal_error(e, None))?;
            if let Some(err) = Self::payload_error(&payload) {
                return Ok(fail(err));
            }
            let bytes_written = payload.get("bytesWritten").and_then(|v| v.as_u64());
            crate::broadcast_project_arcs(&self.app_handle, &self.pm, &self.wp, &self.agent_tabs, None).await;
            let region = self.digest_region(&id, None, true).await;
            let mut env = ok_envelope(&id, format!("Saved {}", path), region);
            insert_seq_hashes(&mut env, &seq_hashes);
            if let Some(b) = bytes_written {
                env["bytesWritten"] = serde_json::json!(b);
            }
            return Ok(Json(env));
        };

        // Region mode: subsequence export.
        if path == id && !request.overwrite.unwrap_or(false) {
            return Ok(fail(
                format!(
                    "{} is the project's own source file — a region export would replace the full file with just the fragment; pass overwrite: true if that is really intended, or choose a different path",
                    path
                ),
            ));
        }
        let ext = crate::validate_user_path(&path, crate::CODON_OUTPUT_EXTS)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let is_protein = project.molecule_type == "protein";
        let wants_gpt = ext == "gpt";
        if wants_gpt != is_protein {
            return Ok(fail(format!(
                "molecule type '{}' exports as {} (DNA/RNA → .gbk/.gb/.genbank, protein → .gpt)",
                project.molecule_type,
                if is_protein { ".gpt" } else { ".gbk/.gb/.genbank" }
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
        let mut spec = spec;
        spec.start = spec.start.map(from1);
        spec.end = spec.end.map(from1);

        let (pieces, flip, desc) = resolve_export_region(&project, &spec)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let bbox = region_bbox(&pieces, project.length, project.topology == "circular");
        let out_name = output_project_name(&path);
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

        let write_path = message_path.clone();
        let written = tokio::task::spawn_blocking(move || {
            let res = if wants_gpt {
                libregene_core::file_io::gpt::write_gpt(&out_project, std::path::Path::new(&write_path))
            } else {
                libregene_core::file_io::gbk::write_gbk(&out_project, std::path::Path::new(&write_path))
            };
            res.map_err(|e| format!("failed to write {}: {}", write_path, e))
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
        insert_seq_hashes(&mut v, &seq_hashes);
        Ok(Json(v))
    }

    /// Close (unload) one of YOUR agent-tab projects from memory without
    /// saving. Only projects bound via open_project can be closed — the
    /// user's own projects are refused. Closing is NOT a file operation: the
    /// file on disk is untouched. A project with unsaved changes is refused
    /// unless `force: true` (save first with save_file, or force to discard).
    /// Mirrors delete_project; the UI updates via broadcast. Returns {ok,
    /// message, projectId}. The response also carries
    /// `sequenceHash`/`revCompHash` (see get_project_overview) of the project
    /// state just before closing.
    #[tool]
    async fn close_project(
        &self,
        Parameters(request): Parameters<CloseProjectRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = request.project_id.clone();
        self.require_agent_tab(&id).await?;
        let hashes = self.project_seq_hashes(&id).await;
        // The dirty/force check runs inside do_delete_project's pm write
        // critical section, so a concurrent mutation cannot slip in between
        // the check and the removal (TOCTOU).
        let payload = crate::do_delete_project(
            &self.app_handle,
            &self.pm,
            &self.wp,
            &self.agent_tabs,
            None,
            request.project_id,
            request.force.unwrap_or(false),
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            if err == "project not found" {
                return Err(ErrorData::invalid_params(format!("Project not found: {}", id), None));
            }
            let mut v = fail_envelope(&id, err);
            if let Some(h) = &hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        let mut v = serde_json::json!({
            "ok": true,
            "message": format!("Closed project {}", id),
            "projectId": id,
        });
        if let Some(h) = &hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }

    /// Replace sequence [start..end] (1-based inclusive) with `replacement`
    /// (empty = delete). A pure insertion before base N is `start=N, end=N-1`;
    /// ranges must not wrap (start > end+1 rejected). The replacement sequence
    /// is given either
    /// as a plain string (`replacement`) or read from a local sequence file
    /// (`replacement_path` — .gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 etc.,
    /// the same formats open_project accepts; exactly one of the two must be
    /// given). PREFER `replacement_path`: a file cannot be mistyped or
    /// truncated, so whenever the insert already exists as a file — or is a
    /// region of an open project you can export first with save_file's
    /// `region` — use the file. Use the `replacement` string only for short
    /// hand-authored
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
    /// retry instead of hand-building a long check string. Uses the same
    /// recompute path as update_sequence: enzymes, primer binding sites,
    /// feature translations AND every stored read alignment are rebuilt (an
    /// edit moves the template under the reads), so alignment data read
    /// earlier in the session may be superseded — re-read the region view if
    /// you rely on it. A no-op edit is also the way to refresh alignments
    /// stored by an older engine. Returns
    /// newLength, old/new region views, 30 bp sequence context on each side of
    /// the edit, and side-effect echo `removedFeatures`/`clippedFeatures`
    /// (both always present, empty arrays when none): removed lists features
    /// fully inside the deleted/replaced span ({name, ftype, location} with
    /// the pre-edit 1-based "start..end"); clipped lists features where at
    /// least one segment actually lost or gained bases ({name, ftype, before,
    /// after} as 1-based {start, end} bounding spans plus beforeSegments/
    /// afterSegments with the individual 1-based ranges in join order; a
    /// feature whose segments all merely shifted — e.g. a cross-origin feature
    /// downstream of a deletion — is NOT clipped). An equal-length replacement
    /// (deleted length == inserted length) keeps ALL features at their
    /// current coordinates — nothing is removed or clipped, so case
    /// normalization and point-mutation edits are safe inside features. `transferredFeatures`/
    /// `transferredPrimers` list annotation names brought in by
    /// `replacement_path` (omitted when none). The replacement is normalized
    /// to uppercase on every molecule type (matching update_sequence). On
    /// protein projects it must additionally be amino-acid letters (A-Z,
    /// optional trailing '*' stop codon); lengths are reported in aa (nt for
    /// RNA, bp for DNA). On DNA projects any U in the replacement is
    /// converted to T, and on RNA projects T to U (a cross-alphabet source
    /// such as an .rna file inserted into a DNA project would otherwise
    /// silently pollute the sequence — the enzyme recompute does not
    /// recognize U); when any base is converted the response carries a
    /// `note` field describing the direction and count. The response also
    /// carries `sequenceHash`/`revCompHash` of the NEW sequence (see
    /// get_project_overview).
    #[tool]
    async fn edit_sequence(
        &self,
        Parameters(request): Parameters<EditSequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            insert_seq_hashes(&mut v, &seq_hashes);
            Json(v)
        };
        let len = project.length;
        // Validate the raw 1-based inclusive inputs BEFORE any arithmetic on
        // them (from1 / end+1 would overflow on extreme i64 inputs); the
        // bounds check uses comparisons only, so once it passes, u_end <= len
        // and the wrap check's `u_end + 1` cannot overflow either.
        let (u_start, u_end) = (request.start, request.end);
        if u_start < 1 || u_start > len + 1 || u_end < 0 || u_end > len {
            return Ok(fail(format!(
                "range {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                u_start, u_end, len
            )));
        }
        if u_start > u_end + 1 {
            return Ok(fail(format!(
                "invalid range {}..{}: start > end+1; ranges must not wrap (a pure insertion before base N is start=N, end=N-1)",
                u_start, u_end
            )));
        }
        let start = from1(u_start);
        let end = from1(u_end);

        let (replacement, parsed_annotations) = match (request.replacement, request.replacement_path) {
            (Some(_), Some(_)) => {
                return Ok(fail(
                    "Provide exactly one of `replacement` or `replacement_path`, not both"
                        .to_string(),
                ));
            }
            (None, None) => {
                return Ok(fail(
                    "Provide exactly one of `replacement` (sequence string, empty = delete) or `replacement_path` (sequence file)"
                        .to_string(),
                ));
            }
            (Some(s), None) => (s, None),
            (None, Some(path)) => {
                crate::validate_user_path(&path, crate::SEQ_EXTS).map_err(|e| {
                    ErrorData::invalid_params(format!("invalid replacement_path: {}", e), None)
                })?;
                let parsed = tokio::task::spawn_blocking(move || {
                    libregene_core::file_io::parse_file(std::path::Path::new(&path))
                })
                .await
                .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
                match parsed {
                    // Annotations travel with the sequence: features/primers
                    // from the file land on the inserted region below.
                    Ok(data) => {
                        // The file's molecule type must match the target
                        // project (e.g. a protein .gpt into a DNA project
                        // would corrupt the sequence).
                        if project.molecule_type == "protein" && data.molecule_type != "protein" {
                            return Ok(fail(format!(
                                "replacement_path is a {} file but the project is protein — pass a protein file (.gpt/.prot)",
                                data.molecule_type
                            )));
                        }
                        if project.molecule_type != "protein" && data.molecule_type == "protein" {
                            return Ok(fail(
                                "replacement_path is a protein file (.gpt/.prot) but the project is DNA/RNA — pass a nucleotide sequence file".to_string(),
                            ));
                        }
                        (
                            data.sequence,
                            Some((data.features, data.primers)),
                        )
                    }
                    Err(e) => {
                        return Ok(fail(format!(
                            "Failed to read replacement sequence file (supported: .gbk/.gb/.genbank, .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1): {}",
                            e
                        )));
                    }
                }
            }
        };

        // Normalize the replacement to uppercase on every molecule type
        // (matching update_sequence): lowercase bases would evade the
        // case-sensitive enzyme recompute and leak into GenBank output.
        // Protein projects additionally require the amino-acid alphabet
        // (A-Z, optional single trailing '*' stop); DNA/RNA projects require
        // the IUPAC nucleotide alphabet (ACGTURYSWKMBDHVN).
        let mut replacement = replacement.to_ascii_uppercase();
        if project.molecule_type == "protein" {
            let body = replacement.strip_suffix('*').unwrap_or(&replacement);
            if replacement.matches('*').count() > 1 || !body.chars().all(|c| c.is_ascii_alphabetic()) {
                return Ok(fail(
                    "Invalid protein replacement: only amino-acid letters (A-Z) and an optional trailing '*' (stop codon) are allowed".to_string(),
                ));
            }
        } else if !replacement.is_empty()
            && !replacement
                .chars()
                .all(|c| c.is_ascii() && !libregene_core::primer::iupac::iupac_expand(c as u8).is_empty())
        {
            return Ok(fail(
                "Invalid DNA/RNA replacement: only IUPAC nucleotide bases (ACGTURYSWKMBDHVN) are allowed".to_string(),
            ));
        }

        // U and T both pass the IUPAC check above, but a cross-alphabet
        // insert would silently pollute the project (the enzyme recompute
        // does not recognize U; an RNA project must not gain T). Normalize
        // to the target project's alphabet and tell the caller when any
        // base was actually converted. Protein projects are untouched (the
        // amino-acid alphabet check above already covers them).
        let alphabet_note: Option<String> = if project.molecule_type == "protein" {
            None
        } else {
            let (from, to, project_kind, source_kind) = if project.molecule_type == "rna" {
                ('T', 'U', "RNA", "DNA")
            } else {
                ('U', 'T', "DNA", "RNA")
            };
            let count = replacement.matches(from).count();
            if count == 0 {
                None
            } else {
                replacement = replacement.replace(from, &to.to_string());
                Some(format!(
                    "Converted {} {}→{} to match the {} project (source looked like {})",
                    count, from, to, project_kind, source_kind
                ))
            }
        };

        // Insertion direction: "-" reverse-complements the replacement (DNA
        // only — revcomp is meaningless for RNA/protein sequences here).
        let reverse = match request.strand.as_deref() {
            None | Some("+") | Some(".") => false,
            Some("-") => true,
            Some(other) => {
                return Ok(fail(format!(
                    "Invalid strand '{}': must be \"+\" (default, insert as given) or \"-\" (reverse-complement before inserting)",
                    other
                )));
            }
        };
        if reverse {
            if !project.is_dna() {
                return Ok(fail(
                    "strand \"-\" (reverse complement) is only supported on DNA projects".to_string(),
                ));
            }
            replacement = libregene_core::utils::reverse_complement(&replacement);
        }

        let is_insertion = end + 1 == start;

        // One write-lock critical section: re-read the LIVE sequence, check
        // expected_old against it, adjust/transfer annotations and swap the
        // sequence atomically. Checking the resolve-time snapshot and mutating
        // in separate locks would let a concurrent edit slip between check
        // and apply, desyncing feature coordinates from the sequence. The
        // update_sequence core never touches feature coordinates (the
        // frontend adjusts them client-side), so the MCP path does it here.
        let mut transferred_feature_names: Vec<String> = Vec::new();
        let mut transferred_primer_names: Vec<String> = Vec::new();
        let (new_seq, context_before, context_after, impact) = {
            let mut pm = self.pm.write().await;
            let Some(p) = pm.get_project_mut_by_id(&id) else {
                return Ok(fail("Project not found".to_string()));
            };
            // The resolve-time bounds check ran against a snapshot; a
            // concurrent edit may have shrunk the sequence since — re-check
            // against the live sequence or the slices below would panic.
            let live_len = p.sequence.len() as i64;
            let out_of_bounds = if is_insertion { start > live_len } else { end >= live_len };
            if out_of_bounds {
                return Ok(fail(format!(
                    "range {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                    u_start, u_end, live_len
                )));
            }
            let current: String = if is_insertion {
                String::new()
            } else {
                p.sequence[start as usize..=end as usize].to_string()
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
                    insert_seq_hashes(&mut v, &seq_hashes);
                    return Ok(Json(v));
                }
            }

            let context_before =
                p.sequence[(start - 30).max(0) as usize..start as usize].to_string();
            let context_after_end = (end + 1 + 30).min(p.length) as usize;
            let context_after = p.sequence[(end + 1) as usize..context_after_end].to_string();

            // Side effects on features, derived from the pre-edit list with
            // the same span math as the adjust below.
            let impact = libregene_core::utils::features_edit_impact(
                &p.features,
                start,
                end,
                replacement.len() as i64,
            );

            let new_seq = format!(
                "{}{}{}",
                &p.sequence[..start as usize],
                replacement,
                &p.sequence[(end + 1) as usize..]
            );

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
            p.sequence = new_seq.clone();
            p.length = p.sequence.len() as i64;
            pm.mark_dirty(&id);
            (new_seq, context_before, context_after, impact)
        };
        let new_len = new_seq.len() as i64;

        let old_win = (
            (start - 30).max(0),
            (end + 30).min(len - 1),
        );
        let old_opts = DigestOptions {
            compact_enzymes: true,
            ..DigestOptions::default()
        };
        let old_region = project_digest(&project, &old_opts, Some(old_win)).ok();

        crate::recompute_after_sequence_change(
            &self.app_handle,
            &self.pm,
            &self.wp,
            &self.agent_tabs,
            None,
            &id,
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;

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
        // Equal-length replacements keep every feature (by design, for
        // synonymous-substitution edits), but when the replacement CONTENT
        // differs beyond case the covered features' annotations now describe
        // different bases — and removed/clipped stay empty, so the agent
        // would get no signal at all. Surface the covered feature names in
        // that specific case (length-changing edits already report via
        // removed/clipped).
        let content_changed_features: Vec<String> = {
            let old_span = &project.sequence[start as usize..=(end) as usize];
            let equal_length_content_differs = !is_insertion
                && old_span.len() == replacement.len()
                && !old_span.eq_ignore_ascii_case(&replacement);
            if equal_length_content_differs {
                project
                    .features
                    .iter()
                    .filter(|f| {
                        let (fs, fe) = if f.segments.is_empty() {
                            (f.start, f.end)
                        } else {
                            (
                                f.segments.iter().map(|s| s.start).min().unwrap_or(f.start),
                                f.segments.iter().map(|s| s.end).max().unwrap_or(f.end),
                            )
                        };
                        fe >= start && fs <= end
                    })
                    .map(|f| f.name.clone())
                    .collect()
            } else {
                Vec::new()
            }
        };
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
        if !content_changed_features.is_empty() {
            v["contentChangedFeatures"] = serde_json::json!(content_changed_features);
        }
        if !transferred_feature_names.is_empty() {
            v["transferredFeatures"] = serde_json::json!(transferred_feature_names);
        }
        if !transferred_primer_names.is_empty() {
            v["transferredPrimers"] = serde_json::json!(transferred_primer_names);
        }
        if let Some(note) = alphabet_note {
            v["note"] = serde_json::json!(note);
        }
        if let Some(rv) = old_region {
            v["regionViewBefore"] = serde_json::json!(rv);
        }
        if let Some(rv) = new_region {
            v["regionView"] = serde_json::json!(rv);
        }
        insert_seq_hashes(
            &mut v,
            &libregene_core::utils::orientation_hashes(&new_seq, &project.molecule_type),
        );
        Ok(Json(v))
    }

    /// Create or update a feature in one tool.
    ///
    /// `feature_id` OMITTED = create: `name` and `ftype` are required, plus a
    /// span — `start`+`end` (1-based inclusive, GenBank convention) for a
    /// simple feature or `segments` ([{start, end}], 5'→3' order) for a
    /// segmented one; the two forms are mutually exclusive. strand (".", "+",
    /// "-", default "+") and color (hex, default "#60A5FA") are optional.
    /// Returns {ok, message, projectId, featureId, regionView} around the new
    /// feature.
    ///
    /// `feature_id` GIVEN = update that feature's attributes in one call:
    /// give at least one of name/ftype/color/strand/start+end/segments or the
    /// call is rejected. `start`+`end` replace the whole span, `segments`
    /// replaces the segment breakdown — the two forms are mutually exclusive
    /// and neither touches the strand. strand must be ".", "+" or "-"; color
    /// is hex and also recolors existing segments. Returns {ok, message,
    /// projectId, regionView} around the feature. The response also carries
    /// `sequenceHash`/`revCompHash` (see get_project_overview).
    #[tool]
    async fn set_feature(
        &self,
        Parameters(request): Parameters<SetFeatureRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id.clone()).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            Json(v)
        };

        if let Some(feature_id) = request.feature_id.clone() {
            // ---- update mode ----
            if !self.feature_exists(&id, &feature_id).await {
                return Ok(fail(format!("Feature not found: {}", feature_id)));
            }
            if request.notes.is_some() {
                return Ok(fail(
                    "notes update is not supported by set_feature (notes can only be set when creating a feature)".to_string(),
                ));
            }
            if let Some(n) = &request.name {
                if n.is_empty() {
                    return Ok(fail("name must not be empty".to_string()));
                }
            }
            if request.name.is_none()
                && request.ftype.is_none()
                && request.color.is_none()
                && request.strand.is_none()
                && request.start.is_none()
                && request.end.is_none()
                && request.segments.is_none()
            {
                return Ok(fail(
                    "Nothing to update: give at least one of name/ftype/color/strand/start+end/segments".to_string(),
                ));
            }
            if let Some(s) = &request.strand {
                if !matches!(s.as_str(), "." | "+" | "-") {
                    return Ok(fail("Invalid strand: must be ., +, or -".to_string()));
                }
            }
            let has_span = request.start.is_some()
                || request.end.is_some()
                || request.segments.is_some();
            let new_span = if has_span {
                let span = resolve_feature_span(request.start, request.end, request.segments)
                    .map_err(|e| ErrorData::invalid_params(e, None))?;
                if let Err(e) = self.span_within_bounds(&id, &span.0, span.1, span.2).await {
                    return Ok(fail(e));
                }
                Some(span)
            } else {
                None
            };
            let payload = crate::do_update_feature(
                &self.app_handle,
                &self.pm,
                &self.wp,
                &self.agent_tabs,
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
                return Ok(fail(err));
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
            let mut v = ok_envelope(&id, message, region);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }

        // ---- create mode ----
        let name = match request.name.clone() {
            Some(n) if !n.is_empty() => n,
            _ => {
                return Ok(fail(
                    "name is required when creating a feature (omit feature_id = create; pass feature_id to update)".to_string(),
                ))
            }
        };
        let ftype = match request.ftype.clone() {
            Some(t) if !t.is_empty() => t,
            _ => {
                return Ok(fail(
                    "ftype is required when creating a feature (omit feature_id = create; pass feature_id to update)".to_string(),
                ))
            }
        };
        let (segments, start, end) = resolve_feature_span(
            request.start,
            request.end,
            request.segments,
        )
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        let strand = request.strand.clone().unwrap_or_else(|| "+".to_string());
        if !matches!(strand.as_str(), "." | "+" | "-") {
            return Ok(fail("Invalid strand: must be ., +, or -".to_string()));
        }
        if let Err(e) = self.span_within_bounds(&id, &segments, start, end).await {
            return Ok(fail(e));
        }

        let feature_id = next_id("feature");
        let feature = Feature {
            id: feature_id.clone(),
            name: name.clone(),
            start,
            end,
            color: request.color.unwrap_or_else(|| "#60A5FA".to_string()),
            ftype: ftype.clone(),
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
            &self.agent_tabs,
            None,
            &id,
            vec![feature],
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(fail(err));
        }
        let region = self.digest_feature_region(&id, &feature_id).await;
        let mut v = ok_envelope(
            &id,
            format!("Added {} {} at {} (1-based inclusive)", ftype, name, stored),
            region,
        );
        v["featureId"] = serde_json::json!(feature_id);
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }

    /// Add a primer ("fwd" or "rev") and recompute its binding sites against
    /// the template. Primer sequences are short (~20-60 nt), so passing `seq`
    /// as plain text is the intended input here — no file input needed.
    /// The primer `name` must not collide with an existing primer or FEATURE
    /// name in the project — a name taken by a feature is rejected (choose a
    /// distinct name, e.g. append "-F"/"-R").
    /// Returns {ok, message, projectId, bindingSites, regionView}
    /// — bindingSites: [{strand, templateStart, templateEnd, tm, annealLen}].
    /// templateStart/templateEnd are 1-based inclusive (the bound range spans
    /// templateStart..templateEnd, GenBank-style). annealLen is the number
    /// of contiguous 3'-end bases matching the template (the anneal core; a
    /// non-pairing 5' tail is excluded).
    /// DNA-only: rejects RNA/protein projects (single-strand molecules carry
    /// no primers). The response also carries `sequenceHash`/`revCompHash`
    /// (see get_project_overview).
    #[tool]
    async fn add_primer(
        &self,
        Parameters(request): Parameters<AddPrimerRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let primer_id = next_id("primer");
        let name = request.name.clone();
        let clean_seq = match clean_primer_input(&request.name, &request.r#type, &request.seq) {
            Ok(s) => s,
            Err(e) => {
                let mut v = fail_envelope(&id, e);
                if let Some(h) = &seq_hashes {
                    insert_seq_hashes(&mut v, h);
                }
                return Ok(Json(v));
            }
        };
        let primer = Primer {
            id: primer_id.clone(),
            name: request.name,
            r#type: request.r#type,
            primer_seq: clean_seq,
            binding_sites: Vec::new(),
        };
        let payload = crate::do_add_primer(&self.app_handle, &self.pm, &self.wp, &self.agent_tabs, None, &id, primer)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            let mut v = fail_envelope(&id, err);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
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
                            let mut site = serde_json::json!({
                                "strand": s.strand,
                                "templateStart": s.template_start,
                                "templateEnd": s.template_end,
                                "tm": (s.tm * 10.0).round() / 10.0,
                                "3PrimeMismatch": s.has_3_prime_mismatch,
                                "annealLen": libregene_core::primer::align::anneal_len(
                                    &p.sequence, &p.topology, &pr.primer_seq, s,
                                ),
                            });
                            site_json_to_1based(&mut site, p.length, p.topology == "circular");
                            site
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
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut env, h);
        }
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
    ///   save_file's `region`), use this — a file cannot be mistyped or
    ///   truncated. Supported file types:
    ///   `.gbk`/`.gb`/`.genbank` (GenBank), `.dna`/`.rna`/`.prot` (SnapGene
    ///   binary), `.gpt` (protein GenBank), `.fa`/`.fasta` (FASTA / plain
    ///   text sequence), `.ab1` (ABIF chromatogram; the basecalled PBAS
    ///   sequence is extracted).
    /// Giving neither or both is an error. A name is always required.
    ///
    /// `algorithm` selects the alignment engine: "blast" (default; BLAST
    /// engine ported from GenePad's gene-core, modelled on the NCBI blastn
    /// algorithm — finds every colinear segment, so split/multi-hit reads
    /// and reads with unalignable junk tails align in full) or
    /// "smith-waterman" (classic single local block plus at most one flank;
    /// a read that spans two distant template loci may lose one of them).
    /// Prefer the default unless the user asks for Smith-Waterman.
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
    ///   stay stats-only so multi-read responses don't balloon (in compact
    ///   mode too). Pass
    ///   `compact: true` to omit `orientedSequence` from the top-level
    ///   summary and from the new alignment's entry, and to skip
    ///   the post-alignment `regionView`; use `read_sequence` or
    ///   `get_region_view` when you need the bases.
    ///   FOCUS: pass `region` ({start, end} 1-based inclusive, start > end
    ///   wraps the origin on circular templates) or `feature_id` (a project
    ///   feature's bounding span; `flank` adds context bp on each side) to
    ///   focus the response on a window — the diff-detail lists of the new
    ///   alignment are filtered to the window, the full `orientedSequence`
    ///   is omitted, and the `regionView` shows the window (its ALIGNMENT
    ///   VIEW section renders the window's read bases column-by-column).
    ///   This is the recommended way to check "is this site mutated?"
    ///   without digesting a full-length read. The total
    ///   mismatches/insertions/deletions counts still describe the WHOLE
    ///   read, the response echoes the applied window as `focus`, and an
    ///   `outsideWindow` block ({mismatches, insertions, deletions}) gives
    ///   the diff base counts OUTSIDE the window (all zero = every
    ///   difference of this read is inside the window).
    ///   `compact: true` additionally suppresses the `regionView`.
    /// A `coverageNote` is added (top-level and on the new alignment's entry)
    /// when the read's coverage is multi-segment with uncovered template bp
    /// between the segments — the engine never produces such gaps for
    /// origin-spanning reads (their segments are adjacent), so a non-zero note
    /// means the template region between segments was not covered by this
    /// read, not that the alignment is broken.
    /// On failure returns {ok: false, message, projectId, significant: false};
    /// a message starting with "No significant alignment found" states the
    /// reason (identity below the 0.60 minimum, or aligned span below the
    /// 50 bp minimum). The response also carries
    /// `sequenceHash`/`revCompHash` of the template (see get_project_overview).
    #[tool]
    async fn add_alignment(
        &self,
        Parameters(request): Parameters<AddAlignmentRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            Json(v)
        };
        let name = request.name.clone();
        let compact = request.compact.unwrap_or(false);

        // Resolve the optional focus window to internal 0-based inclusive
        // coordinates ((s, e), s > e wraps the origin on circular templates).
        let focus: Option<(i64, i64)> = {
            if request.region.is_some() && request.feature_id.is_some() {
                return Ok(fail(
                    "region and feature_id are mutually exclusive".to_string(),
                ));
            }
            let flank = request.flank.unwrap_or(0).clamp(0, MAX_FLANK);
            let pm = self.pm.read().await;
            let p = pm
                .get_project_by_id(&id)
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
            let len = p.length;
            let circular = p.topology == "circular";
            if let Some(r) = &request.region {
                if r.start < 1 || r.start > len || r.end < 1 || r.end > len {
                    return Ok(fail(format!(
                        "region {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                        r.start, r.end, len
                    )));
                }
                if r.start > r.end && !circular {
                    return Ok(fail(
                        "region start > end wraps the origin and is only allowed on circular sequences".to_string(),
                    ));
                }
                let s = r.start.saturating_sub(1).saturating_sub(flank).max(0);
                let e = r.end.saturating_sub(1).saturating_add(flank).min(len - 1);
                Some((s, e))
            } else if let Some(fid) = &request.feature_id {
                let f = match p.features.iter().find(|f| &f.id == fid) {
                    Some(f) => f,
                    None => {
                        return Ok(fail(format!(
                            "feature '{}' not found in project; list feature ids with get_project_overview",
                            fid
                        )));
                    }
                };
                let s = f.start.saturating_sub(flank).max(0);
                let e = f.end.saturating_add(flank).min(len - 1);
                Some((s, e))
            } else {
                None
            }
        };

        let mut trace_path: Option<String> = None;
        let seq = match (request.bases, request.path) {
            (Some(_), Some(_)) => {
                return Ok(fail(
                    "Provide exactly one of `bases` or `path`, not both".to_string(),
                ));
            }
            (None, None) => {
                return Ok(fail(
                    "Provide exactly one of `bases` (sequence string) or `path` (sequence file)".to_string(),
                ));
            }
            (Some(bases), None) => bases,
            (None, Some(path)) => {
                let ext = crate::validate_user_path(&path, crate::SEQ_EXTS).map_err(|e| {
                    ErrorData::invalid_params(format!("invalid path: {}", e), None)
                })?;
                if ext == "ab1" {
                    trace_path = Some(path.clone());
                }
                let parsed = tokio::task::spawn_blocking(move || {
                    libregene_core::file_io::parse_file(std::path::Path::new(&path))
                })
                .await
                .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
                match parsed {
                    Ok(data) => data.sequence,
                    Err(e) => {
                        return Ok(fail(format!(
                            "Failed to read alignment sequence file (supported: .gbk/.gb/.genbank, .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1): {}",
                            e
                        )));
                    }
                }
            }
        };

        let payload = match crate::do_add_alignment_seq(
            &self.app_handle,
            &self.pm,
            &self.wp,
            &self.agent_tabs,
            None,
            &id,
            request.name,
            seq,
            trace_path,
            crate::parse_align_algorithm(request.algorithm.as_deref()),
        )
        .await
        {
            Ok(p) => p,
            Err(e) if e.starts_with("No significant alignment found") => {
                let mut v = serde_json::json!({
                    "ok": false,
                    "message": e,
                    "projectId": id.clone(),
                    "significant": false,
                });
                if let Some(h) = &seq_hashes {
                    insert_seq_hashes(&mut v, h);
                }
                return Ok(Json(v));
            }
            Err(e) => return Err(ErrorData::internal_error(e, None)),
        };
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(fail(err));
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
                            if idx + 1 == total {
                                // The newly added alignment keeps its diff
                                // details (focused to the window when given);
                                // orientedSequence only in a non-compact,
                                // unfocused response.
                                let mut v = alignment_json_1based(
                                    a,
                                    &p.sequence,
                                    p.length,
                                    circular,
                                    compact || focus.is_some(),
                                );
                                if let Some((s, e)) = focus {
                                    filter_alignment_json_focus(
                                        &mut v,
                                        s + 1,
                                        e + 1,
                                        p.length,
                                        circular,
                                    );
                                }
                                v
                            } else {
                                alignment_stats_json_1based(a, &p.sequence, p.length, circular)
                            }
                        })
                        .collect();
                    let last = p.alignments.last();
                    let region = focus.or_else(|| {
                        last.and_then(|a| {
                            a.segments.first().map(|s| (s.start as i64, s.end as i64))
                        })
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
        if let Some((s, e)) = focus {
            env["focus"] = serde_json::json!({
                "start": s + 1,
                "end": e + 1,
                "featureId": request.feature_id,
                "note": "mismatchDetails/deletionDetails/insertionDetails are filtered to this window; total counts still describe the whole read; outsideWindow gives the diff counts outside this window",
            });
        }
        if let Some(note) = coverage_note {
            env["coverageNote"] = serde_json::json!(note);
            if let Some(last) = env["alignments"].as_array_mut().and_then(|arr| arr.last_mut()) {
                last["coverageNote"] = serde_json::json!(note);
            }
        }
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut env, h);
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
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
    #[tool]
    async fn find_orfs(
        &self,
        Parameters(request): Parameters<FindOrfsRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let orfs = crate::do_find_orfs(&self.pm, &id, request.min_aa)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;

        if !request.add_as_features.unwrap_or(false) {
            let orfs_json: Vec<serde_json::Value> = orfs.iter().map(feature_json_1based).collect();
            let mut v = serde_json::json!({ "projectId": id, "orfs": orfs_json });
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        self.require_agent_tab(&id).await?;
        if orfs.is_empty() {
            let mut v = serde_json::json!({
                "ok": true,
                "message": "No ORFs found",
                "projectId": id,
            });
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        let min_s = orfs.iter().map(|f| f.start).min().unwrap_or(0);
        let max_e = orfs.iter().map(|f| f.end).max().unwrap_or(0);
        let payload = crate::do_add_features(
            &self.app_handle,
            &self.pm,
            &self.wp,
            &self.agent_tabs,
            None,
            &id,
            orfs,
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            let mut v = fail_envelope(&id, err);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        let region = self.digest_region(&id, Some((min_s, max_e)), true).await;
        let mut v = ok_envelope(&id, "Added ORFs as CDS features".to_string(), region);
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
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
    /// STRAND WARNING (most common agent mistake): `mut_seq` is ALWAYS
    /// PLUS-strand (template top-strand) content, even when the CDS you are
    /// editing is on the minus strand — for a minus-strand CDS you must
    /// reverse-complement the intended coding-strand edit yourself (e.g. a
    /// coding-strand GCG→AAG Ala→Lys change is `mut_seq: "CTT"`, the rev-comp
    /// of AAG). If you pass coding-strand sequence instead, the self-check
    /// block will show the WRONG amino acid.
    /// The response includes a `mutation` self-check block (diffs, plus/minus
    /// strand context, and CDS codon/amino-acid change when `seg` lies inside
    /// a CDS — joined multi-segment CDS features are supported — mind the CDS
    /// strand: for a minus-strand CDS the coding change is the reverse
    /// complement of the plus-strand edit) plus an `orientationHint` string
    /// that restates the strand semantics WITH the actual outcome (CDS
    /// strand, codonAfter, amino acid after) — ALWAYS read `aaAfter`/
    /// `orientationHint` and confirm it is the amino acid you intended before
    /// using the primers. In that block `segStart`/`segEnd`
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
    /// single-strand molecules). The response also carries
    /// `sequenceHash`/`revCompHash` of the template (see get_project_overview).
    #[tool]
    async fn design_primers(
        &self,
        Parameters(request): Parameters<DesignPrimersRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            Json(v)
        };
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

        // Validate seg/seg2 against the project: the design engine slices
        // with modulo/clamping instead of erroring, so an out-of-bounds
        // segment would silently yield garbage candidates. seg/seg2 are
        // 0-based here; messages are 1-based inclusive like every other tool.
        let (tlen, circular) = {
            let pm = self.pm.read().await;
            let p = pm
                .get_project_by_id(&id)
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
            (p.length, p.topology == "circular")
        };
        for (label, s) in [("seg", &seg), ("seg2", &seg2)] {
            let Some(s) = s else { continue };
            if s.start < 0 || s.end < 0 || s.start >= tlen || s.end >= tlen {
                return Ok(fail(format!(
                    "{} {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                    label,
                    s.start + 1,
                    s.end + 1,
                    tlen
                )));
            }
            if s.start > s.end && (!circular || request.mode == "mutagenesis") {
                return Ok(fail(format!(
                    "{} {}..{}: start > end wraps the origin — only allowed on circular templates in amplify/oepcr mode",
                    label,
                    s.start + 1,
                    s.end + 1
                )));
            }
        }

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
                        Err(e) => return Ok(fail(e)),
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
                    // analyze_mutagenesis reports internal 0-based seg
                    // coordinates; restate them 1-based inclusive for the
                    // agent (bounds are pre-validated above, so this covers
                    // the length/identity/diff-count errors).
                    let e = e.replace(
                        &format!("seg {}..{}", seg_ref.start, seg_ref.end),
                        &format!("seg {}..{} (1-based inclusive)", seg_ref.start + 1, seg_ref.end + 1),
                    );
                    let mut v = fail_envelope(&id, e);
                    if let Some(h) = &seq_hashes {
                        insert_seq_hashes(&mut v, h);
                    }
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
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
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
    /// Primer-pair amplicon size can be derived from the binding sites
    /// reported here: the product spans the fwd primer's forward-strand site
    /// start (templateStart) to the rev primer's reverse-strand site end
    /// (templateEnd), inclusive. To obtain the amplicon itself (as a file),
    /// use save_file's fwd_primer/rev_primer region mode.
    /// DNA-only: rejects RNA/protein projects (no primer binding on
    /// single-strand molecules). The response also carries
    /// `sequenceHash`/`revCompHash` of the template (see get_project_overview).
    #[tool]
    async fn check_primer_binding(
        &self,
        Parameters(request): Parameters<CheckPrimerBindingRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let mut primers: Vec<Primer> = Vec::with_capacity(request.primers.len());
        for p in request.primers {
            let clean_seq = match clean_primer_input(&p.name, &p.r#type, &p.seq) {
                Ok(s) => s,
                Err(e) => {
                    let mut v = fail_envelope(&id, e);
                    if let Some(h) = &seq_hashes {
                        insert_seq_hashes(&mut v, h);
                    }
                    return Ok(Json(v));
                }
            };
            primers.push(Primer {
                id: p.name.clone(),
                name: p.name,
                r#type: p.r#type,
                primer_seq: clean_seq,
                binding_sites: Vec::new(),
            });
        }
        let payload = crate::do_check_primers_binding(&self.pm, &id, primers)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        let (tlen, circular) = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .map(|p| (p.length, p.topology == "circular"))
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?
        };
        let mut v = payload;
        // The core reports internal 0-based coordinates; convert every site's
        // templateStart/templateEnd to the 1-based inclusive MCP convention.
        if let Some(results) = v.get_mut("results").and_then(|r| r.as_array_mut()) {
            for result in results.iter_mut() {
                if let Some(site) = result.get_mut("site") {
                    if !site.is_null() {
                        site_json_to_1based(site, tlen, circular);
                    }
                }
                if let Some(sites) = result.get_mut("sites").and_then(|s| s.as_array_mut()) {
                    for site in sites.iter_mut() {
                        site_json_to_1based(site, tlen, circular);
                    }
                }
            }
        }
        v["projectId"] = serde_json::json!(id);
        v["tmBasis"] = serde_json::json!(
            "3' continuous match; 5' tail bases that accidentally match the adjacent template are included in annealLen/Tm (expected for tailed primers — see per-site alignedTemplate/matchMask)"
        );
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }

    /// Convert sequences between molecule types — DNA/RNA/protein — with
    /// codon optimization where it applies. BATCH: `items` takes 1-64
    /// independent conversion items; a failing item does not abort the others
    /// (its slot in `results` carries {ok: false, error}). A single
    /// conversion may omit `items` and put the item fields at the top level.
    ///
    /// Conversion matrix (per item, `from`/`to` ∈ "dna" | "rna" | "protein"):
    /// - dna→dna: codon optimization when `species` (or another optimizer
    ///   parameter) is given, else the sequence passes through unchanged;
    ///   `revComp: true` reverse-complements (not combinable with
    ///   optimization). Project mode lives here.
    /// - dna→rna / rna→dna: T↔U conversion (optional revComp).
    /// - dna→protein / rna→protein: translation (frame 0; a trailing
    ///   partial codon is dropped).
    /// - protein→dna / protein→rna: REVERSE TRANSLATION with codon
    ///   optimization (`species` required — a key from list_species such as
    ///   "e_coli", "h_sapiens"; `method` = use_best_codon (default) |
    ///   match_codon_usage | harmonize_rca, the latter using
    ///   `original_species` as the source table; `avoid_enzyme_sites` takes
    ///   IUPAC recognition sequences to avoid).
    /// - protein→protein: rejected. revComp with a protein side: rejected.
    /// `from` defaults: project mode → dna; `input_path` → the file's
    /// molecule type; `sequence` → dna. `to` defaults: dna for a protein
    /// input, otherwise same as `from`.
    ///
    /// Exactly one input mode per item:
    /// - `project_id` + `feature_id` (both required; dna→dna codon
    ///   optimization ONLY, DNA projects only): optimize the CDS/mRNA feature
    ///   inside an open project. `apply=false` (default) is a read-only
    ///   preview; `apply=true` replaces the feature's coding bases in the
    ///   template (equal-length synonymous substitution, coordinates
    ///   unchanged) through the same recompute+broadcast path as
    ///   edit_sequence. `output_path` is REJECTED here — apply then save_file.
    /// - `sequence`: raw sequence text (whitespace/digits ignored). Use ONLY
    ///   for short hand-authored sequences — pasted long sequences are
    ///   error-prone, so whenever the sequence exists as a file use
    ///   `input_path`, and when it is a region of an open project export it
    ///   first with save_file's `region`.
    /// - `input_path` (PREFERRED for real sequences): local file parsed with
    ///   file_io (.gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 nucleotide,
    ///   .gpt/.prot protein). A file cannot be mistyped or truncated. With
    ///   `feature_id` on a DNA file + `species`, that file CDS is optimized
    ///   and the written sequence carries the full file with the CDS
    ///   replaced.
    ///
    /// Returns {ok, results: [{index, ok, from, to, sequence?, length?,
    /// message?, path?, projectId?, regionView?, error?, ...}]}. Successful
    /// items carry the converted `sequence` text and its `length`; codon-
    /// optimizing items additionally carry the optimizer fields (aa,
    /// codonCount, newCodons, caiBefore, caiAfter, gcBefore, gcAfter, repairs,
    /// repairCount, unresolved, method, species). `aa` includes a trailing
    /// '*' for the stop codon and `codonCount` counts it. repairs[].codonIndex
    /// is 1-based (1 = first codon, including the stop) and each unresolved
    /// entry is "<reason> <start>..<end>" with 1-based inclusive base offsets
    /// within the optimized sequence. `path` appears when
    /// `output_path` was given; `regionView` after an apply=true project
    /// write-back. Failed items carry {ok: false, error}; when EVERY item
    /// fails the whole call returns an error.
    ///
    /// `output_path` (sequence/input_path modes only): .gbk/.gb/.genbank →
    /// GenBank of the output molecule (DNA/RNA with the CDS annotated),
    /// .gpt → protein GenBank (protein output only), .fa/.fasta/.txt → bare
    /// sequence text. When the output carries a single whole-length CDS, the
    /// CDS feature is labeled after the SOURCE file stem (falling back to the
    /// output file stem). PREFER writing the result to a file (and
    /// open_project it afterwards) over reading the `sequence` text —
    /// sequences move between tools as files, not pasted text. When
    /// `output_path` already exists, `overwrite: true` is required (same rule
    /// as save_file).
    /// Each result also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview) of the INPUT sequence: project mode → the
    /// project after a possible apply; input_path → the input file's
    /// sequence; sequence → the given text.
    #[tool]
    async fn convert_sequence(
        &self,
        Parameters(request): Parameters<ConvertSequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let ConvertSequenceRequest { items, single } = request;
        let items = match items {
            Some(items) if items.is_empty() => {
                return Err(ErrorData::invalid_params(
                    "items must not be empty — give 1-64 conversion items",
                    None,
                ));
            }
            Some(items) => items,
            None => {
                if single.project_id.is_some() || single.sequence.is_some() || single.input_path.is_some() {
                    vec![single]
                } else {
                    return Err(ErrorData::invalid_params(
                        "items is required: a batch of 1-64 conversion items (a single conversion may put the item fields at the top level instead — one of project_id / sequence / input_path)",
                        None,
                    ));
                }
            }
        };
        if items.len() > 64 {
            return Err(ErrorData::invalid_params(
                format!("items is capped at 64 entries per call (got {})", items.len()),
                None,
            ));
        }

        let mut results = Vec::with_capacity(items.len());
        let mut ok_count = 0usize;
        for (i, item) in items.iter().enumerate() {
            match self.convert_one(item).await {
                Ok(mut v) => {
                    v["index"] = serde_json::json!(i);
                    v["ok"] = serde_json::json!(true);
                    ok_count += 1;
                    results.push(v);
                }
                Err(e) => {
                    results.push(serde_json::json!({ "index": i, "ok": false, "error": e }));
                }
            }
        }
        if ok_count == 0 {
            let detail = results
                .iter()
                .map(|r| format!("[{}] {}", r["index"], r["error"].as_str().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ErrorData::invalid_params(
                format!("all {} item(s) failed: {}", results.len(), detail),
                None,
            ));
        }
        Ok(Json(serde_json::json!({ "ok": true, "results": results })))
    }
}

// ---------------------------------------------------------------------------
// Server bootstrap + settings (start/stop/restart without app restart)
// ---------------------------------------------------------------------------

#[tool_handler(name = "LibreGene", instructions = "Agent tabs: open_project opens a sequence file AND binds it as your agent tab in the main window in one step (locked against user input; every tool call re-locks it). A path that is already open but not bound belongs to the user — copy the file with bash `cp` to a new path and open_project the copy. Mutating tools refuse projects not bound as an agent tab. Files over pasted text: whenever a sequence exists as a file (or can be written to one), prefer file-based I/O over pasting sequence text into tool arguments — pasted sequences are error-prone (transcription slips, truncation, wrong strand). Open sequence files with open_project; insert/replace from a file via edit_sequence's replacement_path; hand reads to add_alignment via path; feed convert_sequence via input_path and collect its result via output_path; to create a new file from a known region of an open project, save_file with `region` (by coordinates, feature, enzymes/cuts, or primers) then open_project the result — never retype the sequence into another tool. Plain-text sequence parameters stay available for short hand-authored input (primers ~20-60 nt, point mutations, short inserts) or when no file exists. read_sequence is for inspecting bases (and resolving coordinates), not for moving sequences between tools. Every tool takes a required project_id; list_projects' activeId is the project the user is viewing (informational only) — avoid it when several agents work in parallel.")]
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
    agent_tabs: crate::AgentTabs,
    config: Arc<StdMutex<McpConfig>>,
    task: Arc<StdMutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    /// Bearer token required on every MCP request so that other local
    /// processes (or a browser via DNS rebinding) can't drive the MCP tools.
    /// Persisted to `<app_config_dir>/mcp_auth_token` so it survives app
    /// restarts; only regenerated when the user explicitly asks. Exposed to
    /// the trusted frontend via `get_mcp_token` / `regenerate_mcp_token`.
    auth_token: Arc<StdMutex<String>>,
    /// Called when the server's effective state changes outside a
    /// `set_config` call (currently: bind failure) so the caller (tray /
    /// frontend status) can reflect the real state instead of "running".
    status: Arc<dyn Fn(bool, u16) + Send + Sync>,
}

impl<R: Runtime> Clone for McpServer<R> {
    fn clone(&self) -> Self {
        Self {
            app_handle: self.app_handle.clone(),
            pm: self.pm.clone(),
            wp: self.wp.clone(),
            agent_tabs: self.agent_tabs.clone(),
            config: self.config.clone(),
            task: self.task.clone(),
            auth_token: self.auth_token.clone(),
            status: self.status.clone(),
        }
    }
}

impl<R: Runtime> McpServer<R> {
    pub fn new(
        app_handle: AppHandle<R>,
        pm: Arc<RwLock<ProjectManager>>,
        wp: Arc<RwLock<HashMap<String, String>>>,
        agent_tabs: crate::AgentTabs,
        status: impl Fn(bool, u16) + Send + Sync + 'static,
    ) -> Self {
        let auth_token = load_or_create_token(&app_handle);
        Self {
            app_handle,
            pm,
            wp,
            agent_tabs,
            config: Arc::new(StdMutex::new(McpConfig::default())),
            task: Arc::new(StdMutex::new(None)),
            auth_token: Arc::new(StdMutex::new(auth_token)),
            status: Arc::new(status),
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
        if port == 0 {
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
            let agent_tabs = self.agent_tabs.clone();
            let port = cfg.port;
            let token = self.auth_token.clone();
            let config = self.config.clone();
            let status = self.status.clone();
            let handle = tauri::async_runtime::spawn(async move {
                if let Err(e) = serve_mcp(app.clone(), pm, wp, agent_tabs, port, token).await {
                    log::error!("MCP server error on port {}: {}", port, e);
                    // Give-up (e.g. the port is held by another app): the
                    // server never came up. Flip the config so get_mcp_config
                    // and the tray stop claiming it is running; guarded so a
                    // concurrent re-enable/relocate via set_config isn't
                    // clobbered — a stale task must neither flip the newer
                    // config nor report its own failure as the tray status.
                    let gave_up = {
                        let mut cfg = config.lock().unwrap();
                        if cfg.enabled && cfg.port == port {
                            cfg.enabled = false;
                            true
                        } else {
                            false
                        }
                    };
                    if gave_up {
                        status(false, port);
                    }
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
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("failed to create MCP token dir {}: {}", parent.display(), e);
            }
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
    if let Err(e) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(token.as_bytes()))
    {
        // The in-memory token keeps working; only the persistence diverged.
        log::warn!("failed to persist MCP auth token to {}: {}", path.display(), e);
    }
}

#[cfg(not(unix))]
fn write_token_file(path: &std::path::Path, token: &str) {
    if let Err(e) = std::fs::write(path, token) {
        log::warn!("failed to persist MCP auth token to {}: {}", path.display(), e);
    }
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
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
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
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            // Unreadable token file (permissions, I/O): regenerating silently
            // would desync the running server from the on-disk token.
            Err(e) => log::warn!(
                "failed to read persisted MCP auth token from {}: {} — generating a fresh one",
                path.display(),
                e
            ),
        }
        let token = generate_auth_token();
        persist_token(app, &token);
        return token;
    }
    generate_auth_token()
}

/// Generate a 32-byte random bearer token, hex-encoded (64 chars).
/// Filled from the OS CSPRNG (getrandom) on every platform; the time/pid
/// mixing below is only a last-resort fallback when the CSPRNG itself fails
/// (a local-only shared secret, so no crypto crate is pulled in).
fn generate_auth_token() -> String {
    let mut buf = [0u8; 32];
    if let Err(e) = getrandom::getrandom(&mut buf) {
        log::warn!("OS CSPRNG failed ({e}); falling back to time/pid mixing for the MCP auth token");
        use std::time::{SystemTime, UNIX_EPOCH};
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
    agent_tabs: crate::AgentTabs,
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
                agent_tabs.clone(),
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
                    // Compared in constant time; the lock tolerates poisoning
                    // (a panicking request handler must not lock out auth).
                    .map(|h| {
                        h.strip_prefix("Bearer ")
                            .map(|t| {
                                token_eq(t, &auth_token.lock().unwrap_or_else(|e| e.into_inner()))
                            })
                            .unwrap_or(false)
                    })
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
    // instance's socket release still binds; give up (with a log trail) after
    // ~2 s so a port held by another app doesn't spin forever.
    const MAX_BIND_ATTEMPTS: u32 = 40;
    let mut attempt = 0u32;
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                log::info!("MCP server listening on http://{addr}/mcp (auth enabled)");
                return axum::serve(listener, router).await.map_err(Into::into);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                attempt += 1;
                if attempt >= MAX_BIND_ATTEMPTS {
                    log::error!(
                        "MCP server: {addr} still in use after {MAX_BIND_ATTEMPTS} bind attempts; giving up"
                    );
                    return Err(Box::new(e));
                }
                log::warn!("MCP server: {addr} in use, retrying ({attempt}/{MAX_BIND_ATTEMPTS})");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(e) => return Err(Box::new(e)),
        }
    }
}

#[cfg(test)]
mod tests;
