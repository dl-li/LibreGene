use std::fmt::Write as _;

use super::range::seg_in_range;
use crate::models::ProjectData;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// annotate.rs appends " (fragment)" to partial hits; strip it here so the
/// marker column is authoritative and fragment names don't double-mark.
fn auto_feature_display_name(f: &crate::annotate::AnnotatedFeature) -> &str {
    f.name
        .strip_suffix(" (fragment)")
        .unwrap_or(f.name.as_str())
}

/// True when any existing project feature overlaps the auto-detected feature
/// by name (case-insensitive) or by coordinates — i.e. it is likely already
/// annotated in the project.
fn auto_feature_already_annotated(
    project: &ProjectData,
    f: &crate::annotate::AnnotatedFeature,
) -> bool {
    let circular = project.topology == "circular";
    let display_name = auto_feature_display_name(f);
    project.features.iter().any(|ef| {
        let name_match = ef.name.eq_ignore_ascii_case(display_name);
        let coord_match = if ef.segments.is_empty() {
            seg_in_range(ef.start, ef.end, f.start, f.end, circular)
        } else {
            ef.segments
                .iter()
                .any(|s| seg_in_range(s.start, s.end, f.start, f.end, circular))
        };
        name_match || coord_match
    })
}

/// Brief auto-annotation section for whole-project overviews: one line per
/// detected common feature. The engine builds a k-mer index once per process
/// (first call only); the section is kept intentionally compact so no
/// `compact_*` option affects it. DNA projects match nucleotide + protein
/// level; protein projects match the aa sequence against CDS translations.
pub(crate) fn push_auto_annotation(out: &mut String, project: &ProjectData) {
    out.push_str("DETECTED COMMON FEATURES (auto):\n");
    let all = if project.is_dna() {
        crate::annotate::annotate_sequence(&project.sequence, project.topology == "circular")
    } else {
        crate::annotate::annotate_protein(&project.sequence, project.topology == "circular")
    };
    let detected: Vec<_> = all.into_iter().filter(|f| !f.fragment).collect();
    if detected.is_empty() {
        out.push_str("(none)\n");
        return;
    }
    for f in &detected {
        let _ = write!(out,
            "        {} | {} | {} | {}..{} | {:.1}%",
            auto_feature_display_name(f),
            f.ftype,
            f.strand,
            f.start + 1,
            f.end + 1,
            f.identity
        );
        if f.match_level == "aa" {
            out.push_str(" | (protein-level)");
        }
        if auto_feature_already_annotated(project, f) {
            out.push_str(" | (already annotated)");
        }
        out.push('\n');
    }
}
