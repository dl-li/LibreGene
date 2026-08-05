use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Helper structs
// ---------------------------------------------------------------------------

/// A simple start/end segment, used for feature segments and enzyme spacers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start: i64,
    pub end: i64,
    #[serde(default)]
    pub color: Option<String>,
}

// ---------------------------------------------------------------------------
// Primer types
// ---------------------------------------------------------------------------

/// Per-template-column alignment status for a primer binding site.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignedColumn {
    /// Absolute template column (0-based).
    pub template_col: i64,
    /// "match" | "mismatch" | "gap"
    pub kind: String,
    /// Primer base at this position, or "-" for gap.
    pub primer_base: String,
    /// Template base at this position.
    pub template_base: String,
    /// Primer insertion bases after this template column (5'→3'), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insertion_after: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingSite {
    /// inclusive
    pub match_start: i64,
    /// inclusive
    pub match_end: i64,
    /// annealing temperature (Celsius)
    #[serde(default)]
    pub tm: f64,
    /// 5' tail bases in primer (primer 5'→3').
    #[serde(default)]
    pub five_prime_tail: String,
    /// 3' tail bases in primer (primer 5'→3').
    #[serde(default)]
    pub three_prime_tail: String,
    /// Per-template-column alignment.
    #[serde(default)]
    pub alignment: Vec<AlignedColumn>,
}

// ---------------------------------------------------------------------------
// New primer binding site types (v2 — compact render-oriented format)
// ---------------------------------------------------------------------------

/// Detail for a single insertion event within an alignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsertionDetail {
    /// Template base at this position.
    pub base_char: String,
    /// Inserted primer bases (5'→3').
    pub inserted_bases: String,
    /// Full display string, e.g. "A[TGA]".
    pub full_string: String,
}

/// Compact alignment data for frontend per-column rendering.
///
/// `display_sequence` has exactly `template_end - template_start` characters,
/// one per template position. Gaps in the primer are `-`; insertions are
/// collapsed into numeric placeholders (e.g. `1`) detailed in `insertion_map`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignmentRenderData {
    pub display_sequence: String,
    #[serde(default)]
    pub insertion_map: HashMap<String, InsertionDetail>,
    /// Indices within `display_sequence` where primer base ≠ template base.
    #[serde(default)]
    pub mismatch_indices: Vec<usize>,
}

/// A single primer binding site with compact render data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimerBindingSite {
    pub primer_id: String,
    /// 1 = forward (top) strand, -1 = reverse (bottom) strand.
    pub strand: i8,
    /// 0-based start on template.
    pub template_start: i64,
    /// 0-based end on template (exclusive).
    pub template_end: i64,
    /// Melting temperature in °C.
    pub tm: f64,
    /// GC content ratio (0–1).
    pub gc_content: f64,
    /// Raw alignment score.
    pub match_score: i32,
    /// Whether the 3'-most 5 bases contain a mismatch or gap.
    #[serde(default)]
    pub has_3_prime_mismatch: bool,
    /// Unaligned primer bases at the 5' end (primer 5'→3').
    #[serde(default)]
    pub five_prime_tail: String,
    /// Unaligned primer bases at the 3' end (primer 5'→3').
    #[serde(default)]
    pub three_prime_tail: String,
    /// Compact render data for the frontend.
    pub alignment: AlignmentRenderData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Primer {
    pub id: String,
    pub name: String,
    /// "fwd" | "rev"
    #[serde(rename = "type")]
    pub r#type: String,
    /// Full primer sequence 5'→3'.
    #[serde(default)]
    pub primer_seq: String,
    /// Computed binding sites, sorted by Tm descending (best first).
    #[serde(default)]
    pub binding_sites: Vec<PrimerBindingSite>,
}

/// A predicted primer pair (one fwd + one rev) that could form a PCR product.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimerPair {
    pub fwd_primer_id: String,
    pub rev_primer_id: String,
    /// 0-based template start (inclusive).
    pub fwd_position: i64,
    /// 0-based template end (inclusive).
    pub rev_position: i64,
    /// Expected PCR product size in bp (including primers).
    pub product_size: usize,
    /// Optimal annealing temperature in °C (Taq).
    #[serde(default)]
    pub ta: f64,
}

// ---------------------------------------------------------------------------
// Feature
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feature {
    pub id: String,
    pub name: String,
    /// overall min (inclusive)
    pub start: i64,
    /// overall max (inclusive)
    pub end: i64,
    #[serde(default = "default_feature_color")]
    pub color: String,
    /// CDS, promoter, terminator, etc.
    #[serde(default)]
    pub ftype: String,
    #[serde(default)]
    pub segments: Vec<Segment>,
    /// "+" forward, "-" reverse, "." unknown
    #[serde(default = "default_strand")]
    pub strand: String,
    #[serde(default)]
    pub notes: String,
    /// AA sequence (CDS only)
    #[serde(default)]
    pub translation: String,
    /// Raw GenBank qualifier key-value pairs
    #[serde(default)]
    pub qualifiers: Vec<(String, String)>,
}

fn default_feature_color() -> String {
    "#60A5FA".to_string()
}

fn default_strand() -> String {
    ".".to_string()
}

// ---------------------------------------------------------------------------
// Enzyme
// ---------------------------------------------------------------------------

/// A pair of top/bottom strand cut positions (absolute template coordinates).
/// For standard enzymes, one pair per recognition site.
/// For cut-twice enzymes, two pairs per recognition site.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CutPair {
    /// absolute top-strand cut on template (cut between cut_index-1 and cut_index, 0-based)
    pub top_cut_index: i64,
    /// absolute bottom-strand cut on template (cut between bot_cut_index-1 and bot_cut_index, 0-based)
    pub bot_cut_index: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Enzyme {
    pub id: String,
    pub name: String,
    /// recognition sequence (actual template)
    pub rec_seq: String,
    /// absolute start of recognition (inclusive, 0-based)
    pub rec_start: i64,
    /// absolute end of recognition (inclusive, 0-based)
    pub rec_end: i64,
    /// absolute start of tooltip display window
    pub display_start: i64,
    /// absolute end of tooltip display window
    pub display_end: i64,
    /// absolute top-strand cut position of first cut pair (backwards compat)
    pub cut_index: i64,
    /// absolute bottom-strand cut position of first cut pair (backwards compat)
    pub bot_cut_index: i64,
    /// all cut pairs for this recognition site (1 for standard, 2 for cut-twice)
    #[serde(default)]
    pub cut_pairs: Vec<CutPair>,
    /// "top" if recognition is on the template strand, "bottom" if on complement
    #[serde(default = "default_recognition_strand")]
    pub recognition_strand: String,
    #[serde(default)]
    pub comp_seq: String,
    /// enzyme recognition pattern (may include IUPAC codes)
    #[serde(default)]
    pub rec_seq_pattern: String,
    /// non-recognition regions in display window (relative); None or list of segments
    #[serde(default)]
    pub spacers: Option<Vec<Segment>>,
    #[serde(default = "default_true")]
    pub is_unique: bool,
    /// Whether this enzyme is methylation-sensitive (true) or methylation-unaffected (false).
    #[serde(default)]
    pub is_methylation_sensitive: bool,
    #[serde(default)]
    pub methylation_blocked: bool,
    /// relative positions within rec (0-indexed)
    #[serde(default)]
    pub methylated_offsets: Vec<i64>,
    /// ["Dam","Dcm","EcoKI"]
    #[serde(default)]
    pub methylation_sources: Vec<String>,
    #[serde(default)]
    pub methylation_required: bool,
    /// positions needing methylation (0-indexed within rec)
    #[serde(default)]
    pub methyl_required_offsets: Vec<i64>,
    /// ["Dam"]
    #[serde(default)]
    pub methyl_required_sources: Vec<String>,
    /// "blunt" | "5overhang" | "3overhang"
    #[serde(default)]
    pub cut_type: String,
    /// this recognition site has two cut pairs (cut-twice enzyme)
    #[serde(default)]
    pub cut_twice: bool,
    /// the enzyme definition is palindromic (site equals its reverse complement)
    #[serde(default = "default_true")]
    pub is_palindromic: bool,
}

fn default_true() -> bool {
    true
}

fn default_recognition_strand() -> String {
    "top".to_string()
}

// ---------------------------------------------------------------------------
// Alignment (read aligned against the project's main sequence)
// ---------------------------------------------------------------------------

/// A non-wrapping aligned range on the template, 0-based inclusive.
/// `chars.len() == end - start + 1`; '-' where the read has a gap.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlignSegment {
    pub start: usize,
    pub end: usize,
    pub chars: String,
}

/// Extra read bases inserted before template column `pos` (0-based).
/// Insertions never add columns to the template.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlignInsertion {
    pub pos: usize,
    pub bases: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Alignment {
    pub id: String,
    pub name: String,
    /// Original read length.
    pub length: usize,
    /// "+" or "-" ("-" = reverse complement matched).
    pub strand: String,
    /// Fraction of matched columns over aligned columns.
    pub identity: f64,
    #[serde(default)]
    pub segments: Vec<AlignSegment>,
    #[serde(default)]
    pub insertions: Vec<AlignInsertion>,
    /// Read sequence as oriented for display (rev-comp when strand is "-").
    pub seq: String,
}

/// A single mismatched column: template position (0-based inclusive) and the
/// template/read bases. `read_base` is oriented to the template strand
/// (already rev-comp'd when the alignment strand is "-").
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlignMismatch {
    pub pos: usize,
    pub template_base: String,
    pub read_base: String,
}

/// Consecutive deleted template columns grouped into one event.
/// `pos` is the 0-based inclusive position of the first deleted base.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlignDeletion {
    pub pos: usize,
    pub length: usize,
    pub bases: String,
}

/// Extra read bases inserted before template column `pos` (0-based inclusive;
/// the insertion sits between `pos - 1` and `pos`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlignInsertionDetail {
    pub pos: usize,
    pub bases: String,
    pub length: usize,
}

/// Machine-readable difference details derived from an [`Alignment`] model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlignmentDiff {
    pub mismatches: Vec<AlignMismatch>,
    pub deletions: Vec<AlignDeletion>,
    pub insertions: Vec<AlignInsertionDetail>,
    /// Template positions covered by the alignment (sum of segment spans).
    pub aligned_length: usize,
}

// ---------------------------------------------------------------------------
// ProjectData
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectData {
    /// LOCUS name (GenBank header)
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub definition: String,
    #[serde(default)]
    pub keywords: String,
    #[serde(default)]
    pub lab_host: String,
    pub sequence: String,
    pub length: i64,
    /// "circular" | "linear"
    #[serde(default = "default_topology")]
    pub topology: String,
    #[serde(default)]
    pub features: Vec<Feature>,
    #[serde(default)]
    pub primers: Vec<Primer>,
    #[serde(default)]
    pub alignments: Vec<Alignment>,
    #[serde(default)]
    pub enzymes: Vec<Enzyme>,
    /// ["dam","dcm","ecoki"]
    #[serde(default)]
    pub methylation_systems: Vec<String>,
    /// +/- bp extension beyond recognition site
    #[serde(default = "default_methylation_overlap")]
    pub methylation_overlap: i64,
    /// current region-of-interest; serialized as [start, end] array or omitted when None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roi: Option<(i64, i64)>,
}

fn default_topology() -> String {
    "circular".to_string()
}

fn default_methylation_overlap() -> i64 {
    2
}

// ---------------------------------------------------------------------------
// Edit impact (MCP edit_sequence side-effect reporting)
// ---------------------------------------------------------------------------

/// A feature removed entirely by a sequence edit (fully inside the deleted or
/// replaced span). `location` is the pre-edit 0-based inclusive "start..end".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedFeatureImpact {
    pub name: String,
    pub ftype: String,
    pub location: String,
}

/// A feature whose coordinates changed other than a pure translation (either
/// a boundary was clipped by the edit or the span length changed).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClippedFeatureImpact {
    pub name: String,
    pub ftype: String,
    pub before: EditSpan,
    pub after: EditSpan,
}

/// 0-based inclusive coordinate span.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditSpan {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeaturesEditImpact {
    pub removed_features: Vec<RemovedFeatureImpact>,
    pub clipped_features: Vec<ClippedFeatureImpact>,
}
