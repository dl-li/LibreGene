//! Formatter: builds compact `AlignmentRenderData` from annealing positions.
//!
//! With the pydna-style approach (perfect 3' match + greedy 5' extension),
//! every base in the footprint is a perfect match. There are no mismatches,
//! gaps, or insertions — this greatly simplifies the render data.
//!
//! `display_sequence` shows the primer bases (fwd) or their complement (rev),
//! indexed by template column. Length equals the template span.

use std::collections::HashMap;

use crate::models::AlignmentRenderData;
use crate::utils;

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
    })
}

/// Build render data for a reverse-primer binding site.
///
/// The displaySequence is indexed by template position (left→right). For a rev
/// primer binding to the top strand, each template position pairs with a primer
/// base that is the complement of the RC-aligned base. So we complement each
/// character — the positions stay the same because they're template-indexed.
pub fn rev_complement_render_data(data: &AlignmentRenderData) -> AlignmentRenderData {
    let comp_display: String = data
        .display_sequence
        .chars()
        .map(|c| match c {
            '-' => '-',
            '0'..='9' => c,
            'A' | 'T' | 'G' | 'C' | 'a' | 't' | 'g' | 'c' => {
                utils::complement_char(c)
            }
            _ => c,
        })
        .collect();

    AlignmentRenderData {
        display_sequence: comp_display,
        insertion_map: data.insertion_map.clone(),
        mismatch_indices: Vec::new(),
    }
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
    fn test_rev_complement() {
        let rd = AlignmentRenderData {
            display_sequence: "ATGC".into(),
            insertion_map: HashMap::new(),
            mismatch_indices: vec![],
        };
        let comp = rev_complement_render_data(&rd);
        assert_eq!(comp.display_sequence, "TACG");
    }

    #[test]
    fn test_display_length_matches_template_span() {
        let primer = b"ATGCATGC";
        let tmpl = b"ATGCATGC";
        let rd = build_render_data(primer, tmpl).unwrap();
        assert_eq!(rd.display_sequence.len(), 8);
    }
}
