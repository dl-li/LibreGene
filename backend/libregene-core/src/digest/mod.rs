//! Text digest renderers — compact, LLM-friendly project summaries for the MCP server.
//!
//! The digest is a user/agent-facing layer: every RENDERED coordinate is
//! **1-based inclusive** (GenBank convention), while all INPUTS (the project
//! model fields and the `region`/`start`/`end` parameters) stay in the
//! internal **0-based inclusive** convention. Conversion happens at the render
//! points: an internal inclusive [s, e] prints as [s+1, e+1]; a primer site's
//! 0-based-exclusive `template_end` prints as-is (the 1-based inclusive end of
//! the site); an enzyme cut at 0-based index C (severing between bases C-1 and
//! C) prints as `N^N+1` — between the 1-based bases N=C and N+1. Circular
//! sequences allow `start > end` to wrap the origin.

/// Cap for `read_sequence` windows — protects LLM context from accidental dumps.
pub const MAX_READ_BASES: usize = 10_000;

/// Column-view line width (bp per template/read row) and per-read cap: reads
/// whose covered window exceeds the cap get an omission note instead of rows.
const ALIGNMENT_VIEW_LINE: usize = 60;
const ALIGNMENT_VIEW_MAX_COLS: usize = 500;

#[derive(Debug, Clone, Default)]
pub struct DigestOptions {
    pub max_features: Option<usize>,
    /// Substring match on feature name (case-insensitive) or exact ftype match.
    pub feature_filter: Option<String>,
    /// Collapse the enzyme cut list into a single count line. The full list
    /// can reach tens of KB (one line per cutter), which blows up MCP
    /// mutation responses — mutation tools default this to true.
    pub compact_enzymes: bool,
    /// Whole-project digests only: collapse the UNIQUE CUTTERS list (one line
    /// per single-cut enzyme, 90+ lines on real plasmids) into a single count
    /// line. Independent of `compact_enzymes` (which governs region views);
    /// `get_project_overview` defaults this to true, pass compactCutters=false
    /// for the full list.
    pub compact_cutters: bool,
    /// Whole-project digests only: append a brief auto-annotation section
    /// (`DETECTED COMMON FEATURES (auto)`) listing non-fragment features the
    /// annotate engine found against the embedded SnapGene database, one line
    /// each with identity and an `(already annotated)` marker. Fragments are
    /// omitted to avoid misleading partial hits. Never affects region views.
    /// The section itself is already compact, so it is independent of
    /// `compact_enzymes`/`compact_cutters`.
    pub include_auto_annotation: bool,
}

mod annotate_section;
mod lines;
mod overview;
mod range;
mod read;
#[cfg(test)]
mod tests;

pub use overview::project_digest;
pub use range::{cut_flanks, cut_notation};
pub use read::{read_sequence, read_sequence_bases};
