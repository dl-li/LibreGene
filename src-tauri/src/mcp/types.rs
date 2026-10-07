//! MCP tool request payloads (also used to generate JSON input schemas via
//! rmcp's schemars re-export). Wire field names are camelCase — the same
//! convention as every tool response.

use rmcp::schemars;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Tool request payloads (also used to generate JSON input schemas)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OverviewRequest {
    /// Required: the project to inspect (see list_workspace).
    pub(crate) project_id: String,
    #[schemars(with = "Option<i64>")]
    pub(crate) max_features: Option<usize>,
    /// Feature name (case-insensitive substring) or exact ftype to keep.
    pub(crate) feature_filter: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegionRequest {
    /// Required: the project to inspect (see list_workspace).
    pub(crate) project_id: String,
    /// Window start, 1-based inclusive; start > end wraps the origin on
    /// circular sequences.
    pub(crate) start: i64,
    /// Window end, 1-based inclusive.
    pub(crate) end: i64,
    #[schemars(with = "Option<i64>")]
    pub(crate) max_features: Option<usize>,
    /// Feature name (case-insensitive substring) or exact ftype to keep.
    pub(crate) feature_filter: Option<String>,
    /// Collapse the enzyme cut list into one count line (default true;
    /// false = every cut in the window).
    pub(crate) compact: Option<bool>,
    /// Also emit the per-read ALIGNMENT VIEW column block (template / match
    /// mask / read rows) for reads overlapping the window. Default false — the
    /// structured ALIGNMENT DIFFS lines are always included. The block is
    /// capped: a window whose covered columns exceed 500 bp gets an omission
    /// note instead of rows, so narrow the window when you need the columns.
    pub(crate) show_alignment_columns: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceRequest {
    /// Required: the project to read from (see list_workspace).
    pub(crate) project_id: String,
    /// Window mode: window start, 1-based inclusive; start > end wraps the
    /// origin on circular sequences. Requires `end`; mutually exclusive with
    /// the coordinate modes.
    pub(crate) start: Option<i64>,
    /// Window mode: window end, 1-based inclusive. See `start`.
    pub(crate) end: Option<i64>,
    /// Coordinate mode: absolute template position (1-based inclusive).
    /// Mutually exclusive with the feature forms.
    pub(crate) position: Option<i64>,
    /// Coordinate mode: feature id for a feature-relative or amino-acid
    /// lookup. Pair with exactly one of `featureOffset` / `aaPosition`.
    pub(crate) feature_id: Option<String>,
    /// Coordinate mode: 1-based offset along the feature's own 5'→3'
    /// direction. Mutually exclusive with `position` / `aaPosition`.
    pub(crate) feature_offset: Option<i64>,
    /// Coordinate mode: 1-based amino-acid position inside a CDS/mRNA feature,
    /// INCLUDING the initiator Met (Met = 1). Literature numbering that skips
    /// the Met is this value minus 1; the response echoes the same numbering as
    /// `codonIndex`. Mutually exclusive with `position` / `featureOffset`.
    pub(crate) aa_position: Option<i64>,
    /// Coordinate mode: context bases on each side of the position for the
    /// returned window (default 30; clamped at the sequence ends).
    pub(crate) flank: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenProjectRequest {
    /// Sequence file to open (.gbk/.gb/.genbank, .dna/.rna/.prot, .gpt,
    /// .fa/.fasta, .ab1, ...). The project id IS this path.
    pub(crate) path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveFileRequest {
    /// Required: the project to save (see list_workspace).
    pub(crate) project_id: String,
    /// Output path (.gbk/.gb/.genbank for DNA/RNA, .gpt for protein).
    pub(crate) path: String,
    /// Optional: export only a region instead of the whole molecule (exactly
    /// one selector inside — see RegionSpec). The export is always linear and
    /// the project is NOT marked clean.
    pub(crate) region: Option<RegionSpec>,
    /// Required (true) when `path` exists and is not the project's own source
    /// path (overwriting the project's own file needs no flag).
    pub(crate) overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditSequenceRequest {
    /// Required: the project to edit (must be bound as your agent tab).
    pub(crate) project_id: String,
    /// First base of the replaced range, 1-based inclusive. Ranges must not
    /// wrap; a pure insertion before base N is start=N, end=N-1.
    pub(crate) start: i64,
    /// Last base of the replaced range, 1-based inclusive (>= start-1).
    pub(crate) end: i64,
    /// Replacement sequence as plain text (empty = delete). Exactly one of
    /// `replacement` / `replacementPath` / `replacementHash`. Short hand-authored
    /// edits only (point mutations, short inserts) — otherwise use
    /// `replacementPath` or `replacementHash`.
    pub(crate) replacement: Option<String>,
    /// PREFERRED input: read the replacement from a local sequence file
    /// (.gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 — the open_project
    /// formats); its annotations travel with the sequence.
    pub(crate) replacement_path: Option<String>,
    /// Workspace input: a hash "fwd7" or "fwd7/rev7" from list_workspace
    /// (a workspace fragment or an open project). A swapped
    /// "revCompHash/sequenceHash" pair inserts the entry's reverse complement
    /// with its annotations flipped; combined with `strand: "-"` the two flips
    /// cancel (XOR). Annotations travel with the sequence like
    /// `replacementPath`.
    pub(crate) replacement_hash: Option<String>,
    /// Insertion direction: "+" (default, insert as given) or "-"
    /// (reverse-complement first). DNA projects only.
    pub(crate) strand: Option<String>,
    /// Guard: must equal the current [start..end] content case-insensitively,
    /// or the edit is rejected with the actual content (copy it from
    /// `currentContent` and retry).
    pub(crate) expected_old: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConvertItem {
    /// Project mode (dna→dna codon optimization only): the open DNA project.
    pub(crate) project_id: Option<String>,
    /// Feature id to optimize (project mode: required) or to lift from
    /// `inputPath` (optional; otherwise the whole file sequence is used).
    pub(crate) feature_id: Option<String>,
    /// Standalone mode: raw sequence text (whitespace/digits ignored). Short
    /// hand-authored sequences only; otherwise use `inputPath`.
    pub(crate) sequence: Option<String>,
    /// Standalone mode (PREFERRED): local sequence file (.gbk/.gb/.genbank/
    /// .dna/.rna/.fasta/.fa/.ab1 — nucleotide; .gpt/.prot — protein).
    /// `from` defaults to the file's molecule type.
    pub(crate) input_path: Option<String>,
    /// Standalone mode: a workspace entry hash ("fwd7" or "fwd7/rev7" from
    /// list_workspace — swapped order = reverse complement). `from` defaults
    /// to the entry's molecule type; behaves like `sequence` mode otherwise.
    pub(crate) hash: Option<String>,
    /// Input molecule type: "dna" | "rna" | "protein". Defaults: project
    /// mode → "dna"; `inputPath` → the file's type; `sequence` → "dna".
    pub(crate) from: Option<String>,
    /// Output molecule type: "dna" | "rna" | "protein". Defaults: "dna" for a
    /// protein input (reverse translation), otherwise same as `from`.
    pub(crate) to: Option<String>,
    /// Reverse-complement the input first (nucleotide → nucleotide only).
    pub(crate) rev_comp: Option<bool>,
    /// Built-in species key (e.g. "e_coli", "h_sapiens" — the convert_sequence
    /// description lists every key); required for every codon-optimizing
    /// conversion.
    pub(crate) species: Option<String>,
    /// use_best_codon (default) | match_codon_usage | harmonize_rca.
    pub(crate) method: Option<String>,
    /// Source codon table for harmonize_rca (falls back to match_codon_usage).
    pub(crate) original_species: Option<String>,
    /// Recognition sequences the optimized sequence must avoid (IUPAC codes
    /// allowed).
    pub(crate) avoid_enzyme_sites: Option<Vec<String>>,
    /// Project mode: false (default) = read-only preview, true = replace the
    /// feature's bases in the project. Ignored in the other input modes.
    pub(crate) apply: Option<bool>,
    /// Optional output file (sequence/inputPath modes only; rejected in project
    /// mode — apply then save_file). .gbk/.gb/.genbank → GenBank, .gpt →
    /// protein GenBank, .fa/.fasta/.txt → bare sequence text. PREFERRED way to
    /// collect the result.
    pub(crate) output_path: Option<String>,
    /// Required (true) when `outputPath` already exists.
    pub(crate) overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConvertSequenceRequest {
    /// Batch of 1–64 conversion items, each converted independently (a failing
    /// item is reported in its slot and does not abort the others). A single
    /// conversion may instead pass the item fields at the top level.
    pub(crate) items: Option<Vec<ConvertItem>>,
    #[serde(flatten)]
    pub(crate) single: ConvertItem,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FeatureSegmentSpec {
    /// Segment start, 1-based inclusive.
    pub(crate) start: i64,
    /// Segment end, 1-based inclusive (>= start).
    pub(crate) end: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetFeatureRequest {
    /// Required: the project to modify (must be bound as your agent tab).
    pub(crate) project_id: String,
    /// Omitted = CREATE a feature (name/ftype plus start+end or segments).
    /// Given = UPDATE that feature (at least one other field required).
    pub(crate) feature_id: Option<String>,
    /// Create: required. Update: new name.
    pub(crate) name: Option<String>,
    /// Create: required (e.g. "CDS", "misc_feature"). Update: new ftype.
    pub(crate) ftype: Option<String>,
    /// Start, 1-based inclusive — required together with `end` unless
    /// `segments` is given; mutually exclusive with `segments`. On update,
    /// replaces the whole span.
    pub(crate) start: Option<i64>,
    /// End, 1-based inclusive (>= start). See `start`.
    pub(crate) end: Option<i64>,
    /// Segmented feature (e.g. multi-exon CDS): [{start, end}] in 5'→3' order,
    /// ascending starts (a cross-origin feature leads with its tail).
    /// Mutually exclusive with `start`/`end`.
    pub(crate) segments: Option<Vec<FeatureSegmentSpec>>,
    /// "." | "+" | "-" (create default "+"; omitted on update = keep).
    pub(crate) strand: Option<String>,
    /// Hex color, e.g. "#60A5FA" (create default "#60A5FA"; on update also
    /// recolors existing segments).
    pub(crate) color: Option<String>,
    /// Create-only initial notes (rejected on update).
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AddPrimerRequest {
    /// Required: the project to modify (must be bound as your agent tab).
    pub(crate) project_id: String,
    /// Unique in the project's primer AND feature namespace.
    pub(crate) name: String,
    /// "fwd" | "rev".
    #[serde(rename = "type")]
    pub(crate) r#type: String,
    /// Primer sequence, plain text (short, ~20–60 nt — the intended input
    /// form). Non-letters are stripped and the sequence is uppercased.
    /// Exactly one of `seq` / `hash`.
    pub(crate) seq: Option<String>,
    /// Workspace entry hash ("fwd7" or "fwd7/rev7" — swapped order = reverse
    /// complement); the entry's sequence becomes the primer sequence.
    pub(crate) hash: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AddToWorkspaceRequest {
    /// Required: the project to extract from (see list_workspace). Read-only —
    /// the project does NOT need to be bound as your agent tab.
    pub(crate) project_id: String,
    /// Selector: add this feature's sequence (segments joined 5'→3',
    /// reverse-complemented for a minus-strand DNA feature).
    pub(crate) feature_id: Option<String>,
    /// Selector: region start, 1-based inclusive (start > end wraps the origin
    /// on circular sequences). Requires `end`.
    pub(crate) start: Option<i64>,
    /// Selector: region end, 1-based inclusive. Requires `start`.
    pub(crate) end: Option<i64>,
    /// Selector (DNA projects only): 1 enzyme name that cuts exactly twice, or
    /// 2 enzyme names that each cut exactly once (the fragment runs from the
    /// first enzyme's cut to the second's).
    pub(crate) enzymes: Option<Vec<String>>,
    /// Optional custom name for the fragment (only when exactly one fragment
    /// is produced).
    pub(crate) name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AddAlignmentRequest {
    /// Required: the project to modify (must be bound as your agent tab).
    pub(crate) project_id: String,
    /// Name for the new alignment.
    pub(crate) name: String,
    /// Read sequence as plain text — short hand-authored reads only; prefer
    /// `path`.
    #[serde(alias = "seq")]
    pub(crate) bases: Option<String>,
    /// PREFERRED input: read the sequence from a file (.gbk/.gb/.genbank,
    /// .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1). Export a project region first
    /// with save_file's `region` if needed.
    pub(crate) path: Option<String>,
    /// Workspace entry hash ("fwd7" or "fwd7/rev7" from list_workspace —
    /// swapped order = reverse complement); only the entry's sequence is used.
    pub(crate) hash: Option<String>,
    /// true = omit `orientedSequence` and `text` from the response; the new
    /// alignment's difference details and coverage are still returned.
    pub(crate) compact: Option<bool>,
    /// Focus window (1-based inclusive; start > end wraps the origin on
    /// circular templates): detail lists are filtered to it and `window` gives
    /// the in-window counts. Mutually exclusive with `featureId`.
    pub(crate) region: Option<SegParam>,
    /// Focus window from a feature's bounding span, plus `flank` bp on each
    /// side. Mutually exclusive with `region`.
    pub(crate) feature_id: Option<String>,
    /// Extra template bp on each side of the focus window (default 0; clamped
    /// at the sequence ends).
    pub(crate) flank: Option<i64>,
    /// "blast" (default; chains any number of colinear segments) or
    /// "smith-waterman" (single local block plus at most one flank).
    pub(crate) algorithm: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindOrfsRequest {
    /// Required: the project to scan (see list_workspace).
    pub(crate) project_id: String,
    /// Minimum ORF length in amino acids (default 75).
    #[schemars(with = "Option<i64>")]
    pub(crate) min_aa: Option<usize>,
    /// true = append the ORFs as CDS features; false/omitted = report them.
    pub(crate) add_as_features: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FindRestrictionSitesRequest {
    /// Required: the project to scan (see list_workspace).
    pub(crate) project_id: String,
    /// Enzyme names to report (case-insensitive); omitted/empty = every enzyme
    /// with a recognition site on this sequence.
    pub(crate) enzymes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TestPrimersRequest {
    /// Required: the project to test against (see list_workspace).
    pub(crate) project_id: String,
    /// Required: TEST these primers against the project without persisting
    /// them (DNA projects only). Exactly one binding fwd + one binding rev
    /// additionally report an `amplicon`.
    pub(crate) primers: Vec<PrimerInput>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SegParam {
    /// Start, 1-based inclusive (start > end wraps the origin on circular
    /// sequences).
    pub(crate) start: i64,
    /// End, 1-based inclusive.
    pub(crate) end: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesignPrimersRequest {
    /// Required: the project to design against (see list_workspace).
    pub(crate) project_id: String,
    /// "amplify" | "oepcr" | "mutagenesis".
    pub(crate) mode: String,
    /// Primary target segment (all modes).
    pub(crate) seg: Option<SegParam>,
    /// Second segment (oepcr only).
    pub(crate) seg2: Option<SegParam>,
    /// Amplify: product name (default "Amplicon").
    pub(crate) name: Option<String>,
    /// oepcr: first fragment name (default "Fragment 1").
    pub(crate) name1: Option<String>,
    /// oepcr: second fragment name (default "Fragment 2").
    pub(crate) name2: Option<String>,
    /// mutagenesis: mutation label (default "Mutation").
    pub(crate) site_name: Option<String>,
    /// Target melting temperature in °C.
    pub(crate) target_tm: f64,
    /// oepcr: overlap length in bases (default 20, min 8).
    #[schemars(with = "Option<i64>")]
    pub(crate) overlap_len: Option<usize>,
    /// mutagenesis: annealing-arm length in bases (default 20, min 8).
    #[schemars(with = "Option<i64>")]
    pub(crate) arm_len: Option<usize>,
    /// mutagenesis: desired PLUS-strand content of `seg` after the edit, same
    /// length as `seg`, differing at <= 3 bases.
    pub(crate) mut_seq: Option<String>,
    /// amplify: enzyme whose recognition site is appended as the fwd 5' tail.
    pub(crate) fwd_enzyme: Option<String>,
    /// amplify: enzyme whose recognition site is appended as the rev 5' tail.
    pub(crate) rev_enzyme: Option<String>,
    /// amplify: GC protection bases in front of a tail (default 3).
    #[schemars(with = "Option<i64>")]
    pub(crate) protect_bases: Option<usize>,
    /// Sodium concentration in M (default 0.050).
    pub(crate) na_conc: Option<f64>,
    /// Magnesium concentration in M (default 0).
    pub(crate) mg_conc: Option<f64>,
    /// dNTP concentration in M (default 0).
    pub(crate) dntp_conc: Option<f64>,
    /// Tris concentration in M (default 0).
    pub(crate) tris_conc: Option<f64>,
    /// Primer concentration in M (default 2.5e-7).
    pub(crate) primer_conc: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrimerInput {
    /// Caller-chosen label, echoed in the result entry.
    pub(crate) name: String,
    /// "fwd" | "rev".
    #[serde(rename = "type")]
    pub(crate) r#type: String,
    /// Primer sequence, plain text (short, ~20–60 nt — the intended input
    /// form). Exactly one of `seq` / `hash`.
    pub(crate) seq: Option<String>,
    /// Workspace entry hash ("fwd7" or "fwd7/rev7" — swapped order = reverse
    /// complement); the entry's sequence becomes the primer sequence.
    pub(crate) hash: Option<String>,
}

/// Optional region selector of `save_file` (subsequence export). Exactly one
/// of the three modes: start+end / featureId / cut1+cut2. Enzyme or primer
/// coordinates come from find_restriction_sites / test_primers, so the
/// selector stays a pair of numbers instead of re-deriving engine results.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegionSpec {
    /// Region mode: export start, 1-based inclusive.
    pub(crate) start: Option<i64>,
    /// Region mode: export end, 1-based inclusive (start > end wraps the
    /// origin on circular sequences).
    pub(crate) end: Option<i64>,
    /// Feature mode: export this feature's sequence (segments joined 5'→3',
    /// reverse-complemented for minus-strand DNA features).
    pub(crate) feature_id: Option<String>,
    /// Cut mode: first cut position — a cut at N severs the DNA between the
    /// 1-based bases N and N+1 (N = len is after the last base on linear
    /// sequences, between the last and the first base on circular ones).
    pub(crate) cut1: Option<i64>,
    /// Cut mode: second cut position (same convention).
    pub(crate) cut2: Option<i64>,
}

// ---------------------------------------------------------------------------
// convert_sequence input resolution (project / raw sequence / file)
// ---------------------------------------------------------------------------

/// One of the mutually exclusive input modes of `convert_sequence`.
pub(crate) enum OptimizeInput {
    /// Open-project mode: `feature_id` names the CDS/mRNA feature to optimize.
    Project {
        project_id: String,
        feature_id: String,
    },
    /// Raw DNA coding sequence text.
    Sequence(String),
    /// Workspace entry resolved from a `hash` input; behaves like `Sequence`
    /// except `from` defaults to the entry's own molecule type.
    Workspace { sequence: String, molecule_type: String },
    /// Local sequence/protein file (`file_io::parse_file`).
    File {
        path: String,
        feature_id: Option<String>,
    },
}
