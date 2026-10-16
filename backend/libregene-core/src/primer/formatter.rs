//! Formatter: builds compact `AlignmentRenderData` from annealing positions.
//!
//! `display_sequence` shows the primer bases in display orientation, indexed
//! by template column. Gaps in the primer render as `-`; each insertion run
//! collapses together with one flanking column on each side into `·`
//! placeholders, so the display always maps 1:1 onto template columns.

use std::collections::HashMap;

use crate::models::AlignmentRenderData;
use crate::primer::alignment::{AlignedPair, Op};

/// Build template-column display chars from display-alignment ops.
///
/// `query` holds the primer bases in the same orientation the ops refer to
/// (primer 5'→3' for fwd sites, reversed for rev sites). Matches/mismatches
/// show the query base, deletions show `-`, and each insertion run collapses
/// with one flanking column per side into `·` placeholders (display-only).
/// Returns the column string plus the indices of genuine mismatch columns.
pub fn display_columns(ops: &[AlignedPair], query: &[u8]) -> (String, Vec<usize>) {
    let mut cols: Vec<(char, bool)> = Vec::new(); // (char, is_mismatch)
    let mut dots: Vec<usize> = Vec::new(); // columns dotted by an insertion run
    let mut prev_ins = false;
    for p in ops {
        match p.op {
            Op::Ins => {
                if !prev_ins {
                    prev_ins = true;
                    if !cols.is_empty() {
                        dots.push(cols.len() - 1);
                    }
                }
            }
            Op::Del => {
                if prev_ins {
                    dots.push(cols.len());
                    prev_ins = false;
                }
                cols.push(('-', false));
            }
            Op::Match | Op::Mismatch => {
                if prev_ins {
                    dots.push(cols.len());
                    prev_ins = false;
                }
                let b = query[p.primer_pos.unwrap_or(0)].to_ascii_uppercase() as char;
                cols.push((b, p.op == Op::Mismatch));
            }
        }
    }
    for &d in &dots {
        cols[d] = ('·', false);
    }
    let s: String = cols.iter().map(|c| c.0).collect();
    let mism: Vec<usize> = cols
        .iter()
        .enumerate()
        .filter_map(|(i, c)| if c.1 { Some(i) } else { None })
        .collect();
    (s, mism)
}

/// Build [`AlignmentRenderData`] from a simple perfect-match footprint.
///
/// `primer` is the **query** sequence (the primer for fwd, RC for rev).
/// `template_region` is the matching template segment.
/// Both must be equal length and fully matching (checked by caller).
pub fn build_render_data(
    primer: &[u8],
    template_region: &[u8],
) -> Option<AlignmentRenderData> {
    if primer.is_empty() || template_region.is_empty() {
        return None;
    }
    if primer.len() != template_region.len() {
        return None;
    }

    let display: String = primer
        .iter()
        .map(|&b| b.to_ascii_uppercase() as char)
        .collect();

    Some(AlignmentRenderData {
        display_sequence: display,
        insertion_map: HashMap::new(),
        mismatch_indices: Vec::new(),
        start_offset: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_render_data_exact_match() {
        let primer = b"ATGC";
        let tmpl = b"ATGC";
        let rd = build_render_data(primer, tmpl).unwrap();
        assert_eq!(rd.display_sequence, "ATGC");
        assert!(rd.mismatch_indices.is_empty());
        assert!(rd.insertion_map.is_empty());
    }

    #[test]
    fn test_build_render_data_empty() {
        assert!(build_render_data(b"", b"ATGC").is_none());
        assert!(build_render_data(b"ATGC", b"").is_none());
    }

    #[test]
    fn test_build_render_data_mismatched_lengths() {
        assert!(build_render_data(b"ATGC", b"ATGCNN").is_none());
    }

    #[test]
    fn test_display_length_matches_template_span() {
        let primer = b"ATGCATGC";
        let tmpl = b"ATGCATGC";
        let rd = build_render_data(primer, tmpl).unwrap();
        assert_eq!(rd.display_sequence.len(), 8);
    }

    #[test]
    fn test_display_columns_mismatch_and_gap() {
        // Template has one base the primer lacks (Del) and one mismatch.
        let query = b"ATGC";
        let ops = vec![
            AlignedPair { op: Op::Match, primer_pos: Some(0), template_pos: Some(0) },
            AlignedPair { op: Op::Mismatch, primer_pos: Some(1), template_pos: Some(1) },
            AlignedPair { op: Op::Del, primer_pos: None, template_pos: Some(2) },
            AlignedPair { op: Op::Match, primer_pos: Some(2), template_pos: Some(3) },
            AlignedPair { op: Op::Match, primer_pos: Some(3), template_pos: Some(4) },
        ];
        let (s, mism) = display_columns(&ops, query);
        assert_eq!(s, "AT-GC");
        assert_eq!(mism, vec![1]);
    }

    #[test]
    fn test_display_columns_insertion_collapses_with_flanks() {
        // The user's example: primer TACTAGTACGC vs template TACCGC — the
        // insertion TAGTA plus one flanking column per side becomes `··`.
        let query = b"TACTAGTACGC";
        let mut ops = Vec::new();
        for i in 0..3 {
            ops.push(AlignedPair { op: Op::Match, primer_pos: Some(i), template_pos: Some(i) });
        }
        for i in 3..8 {
            ops.push(AlignedPair { op: Op::Ins, primer_pos: Some(i), template_pos: None });
        }
        for i in 8..11 {
            ops.push(AlignedPair {
                op: Op::Match,
                primer_pos: Some(i),
                template_pos: Some(i - 5),
            });
        }
        let (s, mism) = display_columns(&ops, query);
        assert_eq!(s, "TA··GC");
        assert!(mism.is_empty());
    }

    #[test]
    fn test_display_columns_insertion_at_span_edge_single_dot() {
        // Insertion run at the very start/end has only one flanking column.
        let query = b"ZZATGC"; // ZZ inserted before the span
        let ops = vec![
            AlignedPair { op: Op::Ins, primer_pos: Some(0), template_pos: None },
            AlignedPair { op: Op::Ins, primer_pos: Some(1), template_pos: None },
            AlignedPair { op: Op::Match, primer_pos: Some(2), template_pos: Some(0) },
            AlignedPair { op: Op::Match, primer_pos: Some(3), template_pos: Some(1) },
        ];
        let (s, _) = display_columns(&ops, query);
        assert_eq!(s, "·T");
    }
}
