//! Fast multi-primer screening via Aho-Corasick automaton.
//!
//! Builds an automaton from the 3' anchors of all primers (with IUPAC
//! expansion), then scans the template once to find all annealing positions.
//! Based on the pydna `primer_screen` module.

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, AhoCorasickKind};
use std::collections::HashMap;

use super::iupac;
use crate::models::Primer;

/// Default 3' anchor length for the automaton (matching pydna).
pub const DEFAULT_LIMIT: usize = 16;

// ---------------------------------------------------------------------------
// Automaton
// ---------------------------------------------------------------------------

/// Pre-built Aho-Corasick automaton for a primer list.
///
/// Each pattern in the automaton is an expanded IUPAC sequence from the
/// 3' anchor of a primer. The pattern ID maps back to the primer index.
pub struct PrimerAutomaton {
    automaton: AhoCorasick,
    /// Maps pattern ID → primer index in the input list.
    pattern_to_primer: Vec<usize>,
    /// Anchor length used to build the automaton.
    pub limit: usize,
}

impl PrimerAutomaton {
    /// Build an automaton from a primer list.
    ///
    /// For each primer, the 3'-most `limit` bases are taken as the anchor.
    /// IUPAC ambiguous codes are expanded to all possible DNA sequences,
    /// each added as a separate pattern pointing to the same primer.
    ///
    /// Primers shorter than `limit` are skipped.
    pub fn build(primers: &[Primer], limit: usize) -> Self {
        let mut patterns: Vec<String> = Vec::new();
        let mut pattern_to_primer: Vec<usize> = Vec::new();

        for (pi, p) in primers.iter().enumerate() {
            let seq = p.primer_seq.to_ascii_uppercase();
            if seq.len() < limit {
                continue;
            }
            // Take 3'-most `limit` bases.
            let anchor = &seq.as_bytes()[seq.len() - limit..];
            let expanded = iupac::expand_iupac_sequence(anchor);
            for e in expanded {
                patterns.push(String::from_utf8_lossy(&e).to_string());
                pattern_to_primer.push(pi);
            }
        }

        let automaton = AhoCorasickBuilder::new()
            .kind(Some(AhoCorasickKind::DFA))
            .build(&patterns)
            .expect("Failed to build Aho-Corasick automaton");

        Self {
            automaton,
            pattern_to_primer,
            limit,
        }
    }

    /// Number of patterns in the automaton.
    pub fn pattern_count(&self) -> usize {
        self.automaton.patterns_len()
    }

    /// Number of unique primers represented.
    pub fn primer_count(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        for &pi in &self.pattern_to_primer {
            seen.insert(pi);
        }
        seen.len()
    }
}

// ---------------------------------------------------------------------------
// Forward / reverse primer search
// ---------------------------------------------------------------------------

/// Find all forward primer annealing positions on a template.
///
/// Forward primers anneal to the Crick (bottom) strand, so we search for
/// their 3' anchors on the reverse-complement of the template Watson strand.
/// Returns a map from primer index → list of 3'-end positions on the Watson
/// strand (0-based, the position AFTER the anchor end).
pub fn forward_primers(
    template: &str,
    _primers: &[Primer],
    automaton: &PrimerAutomaton,
) -> HashMap<usize, Vec<usize>> {
    // Forward primers bind the Crick strand. The primer anchor as-is
    // should match the reverse complement of the template.
    // Equivalent to: search for primer anchor on rc(template).
    let rc = crate::utils::reverse_complement(template);
    let rc_upper = rc.to_ascii_uppercase();

    let mut result: HashMap<usize, Vec<usize>> = HashMap::new();
    let tlen = template.len();

    for m in automaton.automaton.find_iter(&rc_upper) {
        let pi = automaton.pattern_to_primer[m.pattern().as_usize()];
        let end_index = m.end(); // end of match on rc(template)
        // Map back to Watson position: the 3' end of the primer on Watson =
        // template_len - (end_index on rc)
        // Actually, the position on the forward (Watson) strand is:
        //   watson_pos = tlen - end_index
        let watson_pos = tlen - end_index;
        result.entry(pi).or_default().push(watson_pos);
    }

    result
}

/// Find all reverse primer annealing positions on a template.
///
/// Reverse primers anneal to the Watson (top) strand, so we search for
/// their 3' anchors directly on the Watson strand.
/// Returns a map from primer index → list of 3'-end positions on the Watson
/// strand (0-based).
pub fn reverse_primers(
    template: &str,
    _primers: &[Primer],
    automaton: &PrimerAutomaton,
) -> HashMap<usize, Vec<usize>> {
    let upper = template.to_ascii_uppercase();

    let mut result: HashMap<usize, Vec<usize>> = HashMap::new();

    for m in automaton.automaton.find_iter(&upper) {
        let pi = automaton.pattern_to_primer[m.pattern().as_usize()];
        let end_index = m.end(); // end of match on Watson
        result.entry(pi).or_default().push(end_index);
    }

    result
}

// ---------------------------------------------------------------------------
// Primer pair finding
// ---------------------------------------------------------------------------

/// A candidate primer pair found by the automaton screen.
#[derive(Debug, Clone)]
pub struct PrimerPairCandidate {
    /// Index of the forward primer in the primer list.
    pub fwd_index: usize,
    /// Index of the reverse primer in the primer list.
    pub rev_index: usize,
    /// 3' end position of the forward primer on the Watson strand.
    pub fwd_position: usize,
    /// 3' end position of the reverse primer on the Watson strand.
    pub rev_position: usize,
    /// Expected PCR product size (including both primers).
    pub product_size: usize,
}

/// Find all primer pairs from a list of primers that produce products
/// within the given size range.
///
/// Only returns pairs where each primer binds at exactly one position
/// (unique binding). The forward primer must bind before the reverse
/// primer (fwd_position <= rev_position).
pub fn find_primer_pairs(
    template: &str,
    primers: &[Primer],
    automaton: &PrimerAutomaton,
    min_product: usize,
    max_product: usize,
) -> Vec<PrimerPairCandidate> {
    let fwd = forward_primers(template, primers, automaton);
    let rev = reverse_primers(template, primers, automaton);

    // Only consider primers with a single binding position.
    let unique_fwd: Vec<(usize, usize)> = fwd
        .into_iter()
        .filter(|(_, pos)| pos.len() == 1)
        .map(|(pi, pos)| (pi, pos[0]))
        .collect();

    let unique_rev: Vec<(usize, usize)> = rev
        .into_iter()
        .filter(|(_, pos)| pos.len() == 1)
        .map(|(pi, pos)| (pi, pos[0]))
        .collect();

    let mut pairs: Vec<PrimerPairCandidate> = Vec::new();

    for &(fi, fp) in &unique_fwd {
        for &(ri, rp) in &unique_rev {
            if fi == ri {
                continue; // same primer, can't be both fwd and rev
            }
            if fp > rp {
                continue; // fwd must be before rev
            }
            // Product size = length of fwd primer + middle + length of rev primer
            let mid = rp.saturating_sub(fp);
            let size = primers[fi].primer_seq.len() + mid + primers[ri].primer_seq.len();

            if size >= min_product && size <= max_product {
                pairs.push(PrimerPairCandidate {
                    fwd_index: fi,
                    rev_index: ri,
                    fwd_position: fp,
                    rev_position: rp,
                    product_size: size,
                });
            }
        }
    }

    // Sort by product size.
    pairs.sort_by_key(|p| p.product_size);
    pairs
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_primers() -> Vec<Primer> {
        vec![
            Primer {
                id: "P1".into(),
                name: "Fwd1".into(),
                r#type: "fwd".into(),
                primer_seq: "CGTACGCTAG".into(),
                binding_sites: vec![],
            },
            Primer {
                id: "P2".into(),
                name: "Rev1".into(),
                r#type: "rev".into(),
                primer_seq: "GCTAGCATCG".into(),
                binding_sites: vec![],
            },
        ]
    }

    #[test]
    fn test_build_automaton() {
        let primers = make_test_primers();
        let auto = PrimerAutomaton::build(&primers, 8);
        assert!(auto.primer_count() > 0);
        assert!(auto.pattern_count() > 0);
    }

    #[test]
    fn test_build_automaton_short_primer_skipped() {
        let primers = vec![Primer {
            id: "S".into(),
            name: "Short".into(),
            r#type: "fwd".into(),
            primer_seq: "ATGC".into(),
            binding_sites: vec![],
        }];
        let auto = PrimerAutomaton::build(&primers, 8);
        assert_eq!(auto.primer_count(), 0);
    }

    #[test]
    fn test_forward_primers() {
        // Template has fwd primer binding site (on Crick).
        // Primer "CGTACGCTAG" binds the Crick strand;
        // its anchor appears as the RC on Watson.
        let template = "NNNNNCTAGCGTACGNNNNN"; // RC of primer is CTAGCGTACG
        let primers = vec![Primer {
            id: "P1".into(),
            name: "Fwd".into(),
            r#type: "fwd".into(),
            primer_seq: "CGTACGCTAG".into(),
            binding_sites: vec![],
        }];
        let auto = PrimerAutomaton::build(&primers, 8);
        let fwd = forward_primers(template, &primers, &auto);
        assert!(!fwd.is_empty(), "should find fwd primer binding");
    }

    #[test]
    fn test_reverse_primers() {
        // Template has rev primer binding site (on Watson).
        // The 3' anchor of the rev primer should appear on Watson.
        let template = "NNNNNCGTACGCTAGNNNNN"; // contains primer as-is
        let primers = vec![Primer {
            id: "P1".into(),
            name: "Rev".into(),
            r#type: "rev".into(),
            primer_seq: "CGTACGCTAG".into(),
            binding_sites: vec![],
        }];
        let auto = PrimerAutomaton::build(&primers, 8);
        let rev = reverse_primers(template, &primers, &auto);
        assert!(!rev.is_empty(), "should find rev primer binding");
    }

    #[test]
    fn test_find_primer_pairs() {
        // Template where fwd and rev primers both bind uniquely.
        // RC of fwd primer (CGTACGCTAG) = CTAGCGTACG
        // Rev primer "GCTAGCATCG" binds directly on Watson
        let fwd_rc = "CTAGCGTACG";
        let rev_seq = "GCTAGCATCG";
        let spacer = "AAAAATTTTTGGGGGCCCCC";
        let template = format!("NN{fwd_rc}{spacer}{rev_seq}NN");

        let primers = vec![
            Primer {
                id: "F".into(),
                name: "Fwd".into(),
                r#type: "fwd".into(),
                primer_seq: "CGTACGCTAG".into(), // RC = CTAGCGTACG
                binding_sites: vec![],
            },
            Primer {
                id: "R".into(),
                name: "Rev".into(),
                r#type: "rev".into(),
                primer_seq: rev_seq.to_string(),
                binding_sites: vec![],
            },
        ];
        let auto = PrimerAutomaton::build(&primers, 8);
        let pairs = find_primer_pairs(&template, &primers, &auto, 20, 2000);
        // We expect at least one pair.
        assert!(!pairs.is_empty(), "should find a primer pair, got {}", pairs.len());
        let pair = &pairs[0];
        assert_eq!(pair.fwd_index, 0);
        assert_eq!(pair.rev_index, 1);
        assert!(pair.product_size > 20);
    }

    #[test]
    fn test_find_primer_pairs_no_unique_binding() {
        // Template where primers bind at multiple positions → no pairs.
        let repeat_template = "CGTACGTACGTACGTACGTACGTACGTACGTAC";
        let primers = vec![Primer {
            id: "P1".into(),
            name: "P1".into(),
            r#type: "fwd".into(),
            primer_seq: "CGTACGTA".into(),
            binding_sites: vec![],
        }];
        let auto = PrimerAutomaton::build(&primers, 4);
        let pairs = find_primer_pairs(repeat_template, &primers, &auto, 10, 1000);
        assert!(pairs.is_empty(), "should not find pairs with non-unique binding");
    }

    #[test]
    fn test_automaton_with_iupac_primer() {
        // Primer "CGTACGRTAG", 3' anchor (last 8) = "TACGRTAG"
        // R=A → "TACGATAG", RC("TACGATAG") = "CTATCGTA"
        // Template contains "CTATCGTA" → its RC contains "TACGATAG".
        let primers = vec![Primer {
            id: "IUP".into(),
            name: "Iupac".into(),
            r#type: "fwd".into(),
            primer_seq: "CGTACGRTAG".into(),
            binding_sites: vec![],
        }];
        // template = "NN" + "CTATCGTA" + "NN" (RC will contain "TACGATAG")
        let template = "NNCTATCGTANN";
        // RC = reverse_complement("NNCTATCGTANN") = "NNTACGATAGNN"
        let auto = PrimerAutomaton::build(&primers, 8);
        let fwd = forward_primers(template, &primers, &auto);
        assert!(!fwd.is_empty(), "should find IUPAC primer via expansion");
    }
}
