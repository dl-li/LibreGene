//! MCP tool request payload types (also used to generate JSON input
//! schemas via rmcp's schemars re-export).

use rmcp::schemars;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Tool request payloads (also used to generate JSON input schemas)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct OverviewRequest {
    /// Required: the project to inspect (see list_projects).
    pub(crate) project_id: String,
    #[schemars(with = "Option<i64>")]
    pub(crate) max_features: Option<usize>,
    pub(crate) feature_filter: Option<String>,
    /// Collapse the UNIQUE CUTTERS list into a single count line (default true;
    /// pass false for the full per-enzyme list).
    pub(crate) compact_cutters: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct RegionRequest {
    /// Required: the project to inspect (see list_projects).
    pub(crate) project_id: String,
    /// Window start, 1-based inclusive; on circular sequences start > end
    /// wraps the origin.
    pub(crate) start: i64,
    /// Window end, 1-based inclusive.
    pub(crate) end: i64,
    #[schemars(with = "Option<i64>")]
    pub(crate) max_features: Option<usize>,
    pub(crate) feature_filter: Option<String>,
    /// Collapse the enzyme cut list into a count line (default true; pass
    /// false for the full list).
    pub(crate) compact: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct SequenceRequest {
    /// Required: the project to read from (see list_projects).
    pub(crate) project_id: String,
    /// Window mode: window start, 1-based inclusive; on circular sequences
    /// start > end wraps the origin. Mutually exclusive with the coordinate
    /// modes below; requires `end`.
    pub(crate) start: Option<i64>,
    /// Window mode: window end, 1-based inclusive. See `start`.
    pub(crate) end: Option<i64>,
    /// Coordinate mode: full-file template coordinate (1-based inclusive).
    /// Mutually exclusive with feature_id + feature_offset and feature_id +
    /// aa_position.
    pub(crate) position: Option<i64>,
    /// Coordinate mode: feature ID for feature-relative or amino-acid
    /// lookups. Must be paired with exactly one of `feature_offset` or
    /// `aa_position`.
    pub(crate) feature_id: Option<String>,
    /// Coordinate mode: 1-based offset along the feature's own 5'→3'
    /// direction. Mutually exclusive with `position` and `aa_position`.
    pub(crate) feature_offset: Option<i64>,
    /// Coordinate mode: 1-based amino-acid position within a CDS/mRNA feature
    /// — INCLUDING the initiator Met (Met = 1). Literature numbering that
    /// skips the Met (e.g. mEGFP A206K) maps to the response's
    /// `aaPositionExcludingMet`, not to this input. Mutually exclusive with
    /// `position` and `feature_offset`.
    pub(crate) aa_position: Option<i64>,
    /// Coordinate mode: bases of context on each side of the resolved
    /// position for the returned window sequence (default 30; clamped at the
    /// sequence ends).
    pub(crate) flank: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct SearchRequest {
    pub(crate) query: String,
    /// Required: the project to search (see list_projects).
    pub(crate) project_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct OpenProjectRequest {
    /// Sequence file to open (.gbk/.gb/.genbank, .dna/.rna/.prot, .gpt,
    /// .fa/.fasta, .ab1, ...). The project id IS this path.
    pub(crate) path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct SaveFileRequest {
    /// Required: the project to save (see list_projects).
    pub(crate) project_id: String,
    /// Output file path (.gbk/.gb for DNA/RNA projects, .gpt for protein
    /// projects).
    pub(crate) path: String,
    /// Optional: export only a region of the project instead of the whole
    /// molecule (exactly one selector inside — see RegionSpec fields). The
    /// exported file is always linear and the project is NOT marked clean.
    pub(crate) region: Option<RegionSpec>,
    /// Required (true) when `path` already exists and is NOT the project's
    /// own source path (saving over the project's own file needs no flag).
    pub(crate) overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct CloseProjectRequest {
    /// Required: the project to close (must be bound as your agent tab).
    pub(crate) project_id: String,
    /// Required (true) to close a project with unsaved changes.
    pub(crate) force: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct EditSequenceRequest {
    /// Required: the project to edit (must be bound as your agent tab).
    pub(crate) project_id: String,
    /// First base of the replaced range, 1-based inclusive. Ranges must not
    /// wrap; a pure insertion before base N is start=N, end=N-1.
    pub(crate) start: i64,
    /// Last base of the replaced range, 1-based inclusive (>= start-1).
    pub(crate) end: i64,
    /// Replacement sequence as a plain string (empty = delete). Exactly one
    /// of `replacement` / `replacement_path` must be given. Use this ONLY for
    /// short hand-authored edits (point mutations, short oligo-length
    /// inserts); for anything longer or taken from an existing file or open
    /// project, use `replacement_path` instead (export the region first with
    /// save_file's `region` if needed) — pasted long sequences are error-prone.
    pub(crate) replacement: Option<String>,
    /// PREFERRED input: read the replacement sequence from a local file
    /// (.gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 etc., same formats as
    /// open_project). A file cannot be mistyped or truncated, so use it whenever
    /// the sequence exists on disk.
    pub(crate) replacement_path: Option<String>,
    /// Direction of the inserted replacement: "+" (default — insert exactly
    /// as given) or "-" (reverse-complement the replacement before inserting,
    /// e.g. when the source sequence is oriented on the opposite strand).
    /// DNA projects only; rejected on RNA/protein projects.
    pub(crate) strand: Option<String>,
    pub(crate) expected_old: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct ConvertItem {
    /// Project mode (only for from=dna, to=dna codon optimization): optimize
    /// a CDS/mRNA feature inside an open project. Must be absent in
    /// `sequence`/`input_path` modes.
    pub(crate) project_id: Option<String>,
    /// Feature id (project mode: required; input_path mode: optional — pick
    /// the file's CDS/mRNA feature with this id, otherwise the whole file
    /// sequence is used).
    pub(crate) feature_id: Option<String>,
    /// Standalone mode: raw sequence text (whitespace/digits ignored). Use
    /// ONLY for short hand-authored sequences; for anything from a file or an
    /// open project use `input_path` (export regions first with save_file's
    /// `region`) — pasted long sequences are error-prone.
    pub(crate) sequence: Option<String>,
    /// Standalone mode (PREFERRED for real sequences): local sequence file
    /// (.gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 — nucleotide; .gpt/.prot —
    /// protein). A file cannot be mistyped or truncated. `from` defaults to
    /// the file's molecule type.
    pub(crate) input_path: Option<String>,
    /// Input molecule type: "dna" | "rna" | "protein". Defaults: project
    /// mode → "dna"; `input_path` → the file's molecule type; `sequence` →
    /// "dna".
    pub(crate) from: Option<String>,
    /// Output molecule type: "dna" | "rna" | "protein". Defaults: "dna" for
    /// a protein input (reverse translation), otherwise same as `from`.
    pub(crate) to: Option<String>,
    /// Reverse-complement the input before converting (nucleotide →
    /// nucleotide only; rejected for protein input or output).
    pub(crate) rev_comp: Option<bool>,
    /// Species key from list_species (e.g. "e_coli", "h_sapiens"). Required
    /// for codon optimization (dna→dna with optimization, protein→dna/rna
    /// reverse translation, project mode).
    pub(crate) species: Option<String>,
    /// use_best_codon (default) | match_codon_usage | harmonize_rca.
    pub(crate) method: Option<String>,
    /// Source table for harmonize_rca; falls back to match_codon_usage when absent.
    pub(crate) original_species: Option<String>,
    /// Restriction-site recognition sequences to avoid (IUPAC codes allowed).
    pub(crate) avoid_enzyme_sites: Option<Vec<String>>,
    /// false = read-only preview; true = replace the sequence in the project.
    /// Only meaningful in project mode (in sequence/input_path mode pass
    /// `output_path` instead).
    pub(crate) apply: Option<bool>,
    /// Optional: write the result to a file (sequence/input_path modes only;
    /// REJECTED in project mode — use apply=true, then save_file).
    /// .gbk/.gb/.genbank → GenBank of the output molecule; .gpt → protein
    /// GenBank; .fa/.fasta/.txt → bare sequence text. PREFERRED way to collect
    /// the result — use the file (open_project afterwards) rather than copying
    /// the result `sequence` text.
    pub(crate) output_path: Option<String>,
    /// Required (true) when `output_path` already exists (same overwrite rule
    /// as save_file).
    pub(crate) overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct ConvertSequenceRequest {
    /// Batch of conversion items (1-64). Each item is converted independently:
    /// a failing item does not abort the others — it is reported as
    /// {ok: false, error} in its slot of `results`. A single conversion can
    /// also be passed WITHOUT `items` by putting the item fields at the top
    /// level (same shape as one item).
    pub(crate) items: Option<Vec<ConvertItem>>,
    #[serde(flatten)]
    pub(crate) single: ConvertItem,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct FeatureSegmentSpec {
    /// Segment start, 1-based inclusive.
    pub(crate) start: i64,
    /// Segment end, 1-based inclusive (must be >= start).
    pub(crate) end: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct SetFeatureRequest {
    /// Required: the project to modify (must be bound as your agent tab).
    pub(crate) project_id: String,
    /// Omitted = CREATE a feature (name/ftype and start+end or segments are
    /// required). Given = UPDATE that feature (at least one other field
    /// required).
    pub(crate) feature_id: Option<String>,
    /// Create: required. Update: new name.
    pub(crate) name: Option<String>,
    /// Create: required (e.g. "CDS", "misc_feature"). Update: new ftype.
    pub(crate) ftype: Option<String>,
    /// Create: feature start, 1-based inclusive — required together with
    /// `end` unless `segments` is given; mutually exclusive with `segments`.
    /// Update: new start (same rules); replaces the whole span.
    pub(crate) start: Option<i64>,
    /// Feature end, 1-based inclusive (>= start). See `start`.
    pub(crate) end: Option<i64>,
    /// Segmented feature (e.g. multi-exon CDS): [{start, end}] 1-based
    /// inclusive, in 5'→3' order. Mutually exclusive with `start`/`end`.
    pub(crate) segments: Option<Vec<FeatureSegmentSpec>>,
    /// ".", "+" or "-" (create default "+"; neither form touches the strand
    /// unless given).
    pub(crate) strand: Option<String>,
    /// Hex color, e.g. "#60A5FA" (create default "#60A5FA"; on update also
    /// recolors existing segments).
    pub(crate) color: Option<String>,
    /// Create-only initial notes (passing notes on update is rejected —
    /// notes update is not supported).
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct AddPrimerRequest {
    /// Required: the project to modify (must be bound as your agent tab).
    pub(crate) project_id: String,
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) r#type: String,
    /// Primer sequence as plain text (short, ~20-60 nt — intended input form).
    pub(crate) seq: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct AddAlignmentRequest {
    /// Required: the project to modify (must be bound as your agent tab).
    pub(crate) project_id: String,
    pub(crate) name: String,
    /// Read sequence as a plain string — short hand-authored reads only;
    /// prefer `path` (a file cannot be mistyped or truncated).
    #[serde(alias = "seq")]
    pub(crate) bases: Option<String>,
    /// PREFERRED input: read the sequence from a file (.gbk/.gb/.genbank,
    /// .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1). If the read is a region of an
    /// open project, export it first with save_file's `region`.
    pub(crate) path: Option<String>,
    /// When true, omit the full `orientedSequence` and the post-alignment
    /// `regionView` to reduce response size. The newly added alignment's
    /// difference details and coverage are still returned (filtered to the
    /// focus window, with `outsideWindow` counts, when `region`/`feature_id`
    /// is also given); previously stored alignments stay stats-only. Use
    /// read_sequence/get_region_view when you need the bases.
    pub(crate) compact: Option<bool>,
    /// Focus window (1-based inclusive; start > end wraps the origin on
    /// circular templates): `mismatchDetails`/`deletionDetails`/
    /// `insertionDetails` are filtered to entries overlapping the window, the
    /// full `orientedSequence` is omitted, and the `regionView` shows this
    /// window (its ALIGNMENT VIEW section gives the window's read bases
    /// column-by-column). Use it when you only care whether a specific site
    /// (e.g. a restriction site) is mutated. Mutually exclusive with
    /// `feature_id`. The total mismatches/insertions/deletions counts still
    /// describe the WHOLE read.
    pub(crate) region: Option<SegParam>,
    /// Focus window from a project feature's bounding span (plus `flank` bp
    /// on each side) — same effect as `region` without hand-computing
    /// coordinates. Mutually exclusive with `region`.
    pub(crate) feature_id: Option<String>,
    /// Extra template bp on each side of the focus window (default 0;
    /// clamped at the sequence ends).
    pub(crate) flank: Option<i64>,
    /// Alignment engine: "blast" (default; NCBI blastn port — chains any
    /// number of colinear segments, handles split/multi-hit reads) or
    /// "smith-waterman" (single local block plus at most one flank).
    pub(crate) algorithm: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct FindOrfsRequest {
    /// Required: the project to scan (see list_projects).
    pub(crate) project_id: String,
    #[schemars(with = "Option<i64>")]
    pub(crate) min_aa: Option<usize>,
    pub(crate) add_as_features: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct FindRestrictionSitesRequest {
    /// Required: the project to scan (see list_projects).
    pub(crate) project_id: String,
    /// Enzyme names to report (case-insensitive); empty/omitted = all enzymes
    /// that have a recognition site on this sequence.
    pub(crate) enzymes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct ListPrimersRequest {
    /// Required: the project to inspect (see list_projects).
    pub(crate) project_id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct SegParam {
    /// Segment start, 1-based inclusive (start > end wraps the origin on
    /// circular sequences).
    pub(crate) start: i64,
    /// Segment end, 1-based inclusive.
    pub(crate) end: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct DesignPrimersRequest {
    /// Required: the project to design against (see list_projects).
    pub(crate) project_id: String,
    pub(crate) mode: String,
    pub(crate) seg: Option<SegParam>,
    pub(crate) seg2: Option<SegParam>,
    pub(crate) name: Option<String>,
    pub(crate) name1: Option<String>,
    pub(crate) name2: Option<String>,
    pub(crate) site_name: Option<String>,
    pub(crate) target_tm: f64,
    #[schemars(with = "Option<i64>")]
    pub(crate) overlap_len: Option<usize>,
    #[schemars(with = "Option<i64>")]
    pub(crate) arm_len: Option<usize>,
    pub(crate) mut_seq: Option<String>,
    pub(crate) fwd_enzyme: Option<String>,
    pub(crate) rev_enzyme: Option<String>,
    #[schemars(with = "Option<i64>")]
    pub(crate) protect_bases: Option<usize>,
    pub(crate) na_conc: Option<f64>,
    pub(crate) mg_conc: Option<f64>,
    pub(crate) dntp_conc: Option<f64>,
    pub(crate) tris_conc: Option<f64>,
    pub(crate) primer_conc: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct PrimerInput {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) r#type: String,
    /// Primer sequence as plain text (short, ~20-60 nt — intended input form).
    pub(crate) seq: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct CheckPrimerBindingRequest {
    /// Required: the project to check against (see list_projects).
    pub(crate) project_id: String,
    pub(crate) primers: Vec<PrimerInput>,
}

/// Optional region selector of `save_file` (subsequence export). Exactly one
/// of the four modes must be given inside: start+end / feature_id /
/// enzyme1+enzyme2 or cut1+cut2 / fwd_primer+rev_primer.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
pub(crate) struct RegionSpec {
    /// Region mode: start of the export window, 1-based inclusive.
    pub(crate) start: Option<i64>,
    /// Region mode: end of the export window, 1-based inclusive (start > end
    /// wraps the origin on circular sequences).
    pub(crate) end: Option<i64>,
    /// Feature mode: export this feature's sequence (segments joined 5'→3',
    /// reverse-complemented for minus-strand features).
    pub(crate) feature_id: Option<String>,
    /// Fragment mode (enzyme names): first enzyme; its first recognition
    /// site's top-strand cut starts the fragment.
    pub(crate) enzyme1: Option<String>,
    /// Fragment mode (enzyme names): second enzyme (may equal `enzyme1` to
    /// use that enzyme's first two sites).
    pub(crate) enzyme2: Option<String>,
    /// Fragment mode (explicit cuts): first cut position — a cut at N severs
    /// the DNA between the 1-based bases N and N+1 (N = len: after the last
    /// base on linear, between the last and the first base on circular).
    pub(crate) cut1: Option<i64>,
    /// Fragment mode (explicit cuts): second cut position (same convention).
    pub(crate) cut2: Option<i64>,
    /// Amplicon mode: fwd primer (project primer name or raw sequence). The
    /// exported amplicon spans the fwd primer's forward-strand site start to
    /// the rev primer's reverse-strand site end — its length is the primer
    /// pair's product size (also derivable from check_primer_binding's site
    /// coordinates without exporting anything).
    pub(crate) fwd_primer: Option<String>,
    /// Amplicon mode: rev primer (project primer name or raw sequence).
    pub(crate) rev_primer: Option<String>,
}

// ---------------------------------------------------------------------------
// convert_sequence input resolution (project / raw sequence / file)
// ---------------------------------------------------------------------------

/// One of the three mutually exclusive input modes of `convert_sequence`.
pub(crate) enum OptimizeInput {
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
