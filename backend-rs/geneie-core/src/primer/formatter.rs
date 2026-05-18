//! Formatter: converts alignment operations into compact `AlignmentRenderData`
//! for the frontend.
//!
//! The core invariant: `display_sequence.len()` must equal the number of
//! template positions spanned by the binding site. Each character represents
//! one template column:
//!
//!   - match/mismatch → primer base
//!   - gap in primer   → `-`
//!   - insertion       → numeric placeholder (e.g. `1`), detailed in `insertion_map`
//!
//! Returns trailing insertion bases separately so the caller can append them
//! to the 3' tail.

use std::collections::HashMap;

use crate::models::{AlignmentRenderData, InsertionDetail};

use super::alignment::{AlignmentResult, Op};

/// Build [`AlignmentRenderData`] from an alignment result.
///
/// Returns the render data and any trailing insertion bases (which belong to
/// the 3' tail).
pub fn build_render_data(
    result: &AlignmentResult,
    primer: &[u8],
    template_region: &[u8],
) -> (Option<AlignmentRenderData>, String) {
    let ops = &result.ops;
    if ops.is_empty() {
        return (None, String::new());
    }

    let mut display = String::new();
    let mut insertion_map: HashMap<String, InsertionDetail> = HashMap::new();
    let mut mismatch_indices: Vec<usize> = Vec::new();
    let mut insertion_counter: u32 = 0;

    let mut op_idx = 0;

    // Skip leading Ins ops (already extracted as 5' tail).
    while op_idx < ops.len() && ops[op_idx].op == Op::Ins {
        op_idx += 1;
    }

    // Skip leading Del ops (template overhang before primer alignment starts).
    while op_idx < ops.len() && ops[op_idx].op == Op::Del {
        op_idx += 1;
    }

    // Collect pending insertions. When non-empty, the NEXT template-consuming
    // op (Match/Mismatch/Del) will emit a digit placeholder INSTEAD of its
    // normal character, keeping displaySequence length == template span.
    let mut pending_insertion: Option<String> = None;

    while op_idx < ops.len() {
        match ops[op_idx].op {
            Op::Ins => {
                let mut ins = pending_insertion.take().unwrap_or_default();
                while op_idx < ops.len() && ops[op_idx].op == Op::Ins {
                    if let Some(pp) = ops[op_idx].primer_pos {
                        ins.push(primer[pp] as char);
                    }
                    op_idx += 1;
                }
                pending_insertion = Some(ins);
                continue;
            }
            Op::Match | Op::Mismatch | Op::Del => {
                let tpos = ops[op_idx].template_pos;
                let tbase = tpos
                    .and_then(|tp| {
                        if tp < template_region.len() {
                            Some(template_region[tp] as char)
                        } else {
                            None
                        }
                    })
                    .unwrap_or('N');

                let is_mismatch = ops[op_idx].op == Op::Mismatch;

                if let Some(inserted) = pending_insertion.take() {
                    // Emit a digit placeholder for this template position.
                    let placeholder = insertion_counter.to_string();
                    insertion_counter += 1;
                    let idx = display.len();
                    display.push_str(&placeholder);

                    let full_string = format!("{}[{}]", tbase, inserted);
                    insertion_map.insert(
                        placeholder,
                        InsertionDetail {
                            base_char: tbase.to_string(),
                            inserted_bases: inserted,
                            full_string,
                        },
                    );

                    if is_mismatch {
                        mismatch_indices.push(idx);
                    }
                } else {
                    match ops[op_idx].op {
                        Op::Match => {
                            if let Some(pp) = ops[op_idx].primer_pos {
                                display.push(primer[pp] as char);
                            }
                        }
                        Op::Mismatch => {
                            if let Some(pp) = ops[op_idx].primer_pos {
                                let idx = display.len();
                                display.push(primer[pp] as char);
                                mismatch_indices.push(idx);
                            }
                        }
                        Op::Del => {
                            display.push('-');
                        }
                        _ => {}
                    }
                }
                op_idx += 1;
            }
        }
    }

    // Trailing insertions → returned as 3' tail contribution.
    let trailing_ins = pending_insertion.unwrap_or_default();

    if display.is_empty() {
        return (None, trailing_ins);
    }

    let rd = AlignmentRenderData {
        display_sequence: display,
        insertion_map,
        mismatch_indices,
    };

    (Some(rd), trailing_ins)
}

/// Build the complementary render data for a reverse-primer binding site.
///
/// The displaySequence is indexed by template position (left→right). For a rev
/// primer binding to the top strand, each template position pairs with a primer
/// base that is the complement of the RC-aligned base. So we only complement
/// each character — the positions stay the same because they're template-indexed.
pub fn rev_complement_render_data(data: &AlignmentRenderData) -> AlignmentRenderData {
    use crate::utils;

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

    let comp_imap: HashMap<String, InsertionDetail> = data
        .insertion_map
        .iter()
        .map(|(k, v)| {
            let comp_base = v
                .base_char
                .chars()
                .map(utils::complement_char)
                .collect::<String>();
            let comp_inserted = utils::complement(&v.inserted_bases);
            let comp_full = format!("{}[{}]", comp_base, comp_inserted);
            (
                k.clone(),
                InsertionDetail {
                    base_char: comp_base,
                    inserted_bases: comp_inserted,
                    full_string: comp_full,
                },
            )
        })
        .collect();

    AlignmentRenderData {
        display_sequence: comp_display,
        insertion_map: comp_imap,
        mismatch_indices: data.mismatch_indices.clone(),
    }
}

/// Legacy: reverse-complement render data. Replaced by [`rev_complement_render_data`]
/// which only complements (no reversal), since displaySequence is template-indexed.
#[deprecated]
pub fn reverse_complement_render_data(data: &AlignmentRenderData) -> AlignmentRenderData {
    rev_complement_render_data(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primer::alignment::align;

    #[test]
    fn test_build_render_data_exact_match() {
        let primer = b"ATGC";
        let tmpl = b"ATGC";
        let result = align(primer, tmpl).unwrap();
        let (rd, trailing) = build_render_data(&result, primer, tmpl);
        let rd = rd.unwrap();
        assert_eq!(rd.display_sequence, "ATGC");
        assert!(rd.mismatch_indices.is_empty());
        assert!(rd.insertion_map.is_empty());
        assert!(trailing.is_empty());
    }

    #[test]
    fn test_build_render_data_with_mismatch() {
        let primer = b"AAAATAAAA";
        let tmpl   = b"AAAACAAAA";
        let result = align(primer, tmpl).unwrap();
        let (rd, _) = build_render_data(&result, primer, tmpl);
        let rd = rd.unwrap();
        assert!(!rd.mismatch_indices.is_empty());
    }

    #[test]
    fn test_build_render_data_with_gap() {
        let primer = b"ATGC";
        let tmpl = b"ATGGC";
        let result = align(primer, tmpl).unwrap();
        let (rd, _) = build_render_data(&result, primer, tmpl);
        let rd = rd.unwrap();
        assert!(rd.display_sequence.contains('-'));
    }

    #[test]
    fn test_display_length_matches_template_span() {
        // displaySequence length must equal template span
        let primer = b"ATGCATGC";
        let tmpl   = b"ATGCATGC";
        let result = align(primer, tmpl).unwrap();
        let t_span = result.template_end - result.template_start;
        let (rd, _) = build_render_data(&result, primer, tmpl);
        let rd = rd.unwrap();
        assert_eq!(rd.display_sequence.len(), t_span);
    }
}
