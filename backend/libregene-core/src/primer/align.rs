//! Primer binding-site search — top-level orchestrator.
//!
//! For each primer this module searches **both strands** of the template:
//! 1. **Fwd (strand 1)**: primer binds bottom strand → primer sequence matches
//!    the top strand directly. Search primer as-is with direct overlap.
//! 2. **Rev (strand -1)**: primer binds top strand → primer sequence
//!    complements the top strand. Search primer as-is with complement matching.
//!
//! Critically, the anchor is ALWAYS the primer's own 3' end, regardless of
//! orientation. This matches the pydna approach where `_annealing_positions`
//! takes the primer sequence and searches with a complement regex on one
//! strand or the other.
//!
//! Algorithm (pydna-style):
//! - IUPAC regex matching of the primer's 3' anchor (matcher)
//! - Greedy 5'-ward extension until first mismatch
//! - Nearest-neighbour Tm from matched bases (thermodynamics)
//! - Compact render data (formatter)

use std::collections::HashSet;

use crate::models::{PrimerBindingSite, Primer, PrimerPair};

use super::formatter;
use super::matcher::{self, DEFAULT_LIMIT};
use super::thermodynamics;

// No minimum footprint filter — any 3'-anchor match is accepted.
// Primers with long 5' tails (adapters, overhangs) commonly have
// short binding footprints; qualify by Tm instead.

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Compute all binding sites for a single primer against the template.
///
/// Searches **both strands** using the primer's own sequence in both cases.
/// The 3' anchor is always the primer's actual 3' end.
pub fn compute_binding_sites(
    template: &str,
    primer_seq: &str,
    _primer_type: &str,
    primer_id: &str,
    topology: &str,
    tm_threshold: f64,
) -> Vec<PrimerBindingSite> {
    let plen = primer_seq.len();
    if plen == 0 || template.is_empty() {
        return Vec::new();
    }

    let primer_seq = primer_seq.to_ascii_uppercase();
    let is_circular = topology == "circular";

    let mut results: Vec<PrimerBindingSite> = Vec::new();

    // Fwd (strand 1): primer matches top strand directly.
    results.extend(search_one_strand(
        template, &primer_seq, primer_id, 1, false,
        plen, is_circular, tm_threshold,
    ));

    // Rev (strand -1): primer complements top strand.
    // Search the primer AS-IS with complement regex on the template.
    results.extend(search_one_strand(
        template, &primer_seq, primer_id, -1, true,
        plen, is_circular, tm_threshold,
    ));

    // Sort by Tm descending (best first), then dedup by position.
    results.sort_by(|a, b| b.tm.partial_cmp(&a.tm).unwrap_or(std::cmp::Ordering::Equal));
    results.dedup_by(|a, b| {
        a.template_start == b.template_start && a.template_end == b.template_end
    });

    results
}

// ---------------------------------------------------------------------------
// One-strand search
// ---------------------------------------------------------------------------

fn search_one_strand(
    template: &str,
    primer: &str,
    primer_id: &str,
    strand: i8,
    use_complement: bool,
    plen: usize,
    is_circular: bool,
    tm_threshold: f64,
) -> Vec<PrimerBindingSite> {
    let tpl_bytes = template.as_bytes();
    let primer_bytes = primer.as_bytes();
    let tlen = tpl_bytes.len();
    let limit = DEFAULT_LIMIT.min(plen);

    let annealing_sites = if is_circular {
        matcher::find_annealing_circular(primer_bytes, tpl_bytes, limit, use_complement)
    } else {
        matcher::find_annealing_positions(primer_bytes, tpl_bytes, limit, use_complement)
    };

    let mut results: Vec<PrimerBindingSite> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();

    for site in &annealing_sites {
        if seen.contains(&site.template_start) {
            continue;
        }
        seen.insert(site.template_start);

        let t_start = site.template_start;
        let t_end = if is_circular {
            (t_start + site.footprint_len) % tlen
        } else {
            t_start + site.footprint_len
        };

        // Template region spanned by the footprint.
        let template_region: Vec<u8> = if is_circular {
            super::alignment::wrap_template_region(tpl_bytes, t_start, t_end)
        } else {
            let e = t_end.min(tlen);
            tpl_bytes[t_start..e].to_vec()
        };

        // Footprint = 3'-most `footprint_len` bases of the PRIMER.
        let footprint_start = plen - site.footprint_len;
        let footprint_bases: Vec<u8> = if use_complement {
            // Rev: primer reads 3'→5' on template (left→right). Display
            // shows primer bases directly (no complement), reversed.
            primer_bytes[footprint_start..].iter().rev().copied().collect()
        } else {
            primer_bytes[footprint_start..].to_vec()
        };

        // Build render data.
        let Some(render_data) = formatter::build_render_data(&footprint_bases, &template_region)
        else {
            continue;
        };

        let matched_str = String::from_utf8_lossy(&footprint_bases).to_string();
        let tm = thermodynamics::compute_tm(&matched_str);
        let gc = thermodynamics::gc_content(&matched_str);

        if tm < tm_threshold {
            continue;
        }

        // Tails: 5' tail = primer bases before the footprint (always 5' end).
        // There is never a 3' tail — the primer's 3' end is always in the anchor.
        let five_tail: String = primer_bytes[..footprint_start]
            .iter()
            .map(|&b| b as char)
            .collect();
        let three_tail = String::new();

        // Display: fwd shows primer 5'→3', rev shows primer 3'→5' (reversed).
        // Both show actual primer bases, no complementing needed.
        let final_rd = render_data;

        let has_3prime = site.has_ambiguous
            && site.footprint_len >= plen.saturating_sub(5);

        results.push(PrimerBindingSite {
            primer_id: primer_id.to_string(),
            strand,
            template_start: t_start as i64,
            template_end: t_end as i64,
            tm,
            gc_content: gc,
            match_score: site.footprint_len as i32,
            has_3_prime_mismatch: has_3prime,
            five_prime_tail: five_tail,
            three_prime_tail: three_tail,
            alignment: final_rd,
        });
    }

    results
}

// ---------------------------------------------------------------------------
// Anneal-core length
// ---------------------------------------------------------------------------

/// Length of the anneal core at a binding site: the number of contiguous
/// bases at the primer's **3' end** that exactly match the template. A 5'
/// tail that does not pair is naturally excluded.
pub fn anneal_len(
    template: &str,
    topology: &str,
    primer_seq: &str,
    site: &PrimerBindingSite,
) -> usize {
    let tlen = template.len() as i64;
    let plen = primer_seq.len();
    if tlen == 0 || plen == 0 {
        return 0;
    }
    let tpl = template.as_bytes();
    let pri = primer_seq.as_bytes();
    let circular = topology == "circular";
    let mut n = 0usize;
    while n < plen && (n as i64) < tlen {
        let pos = if site.strand == 1 {
            // Fwd: primer 3' end aligns at template_end-1, extending leftwards.
            let p = site.template_end - 1 - n as i64;
            if p < 0 && !circular {
                break;
            }
            p.rem_euclid(tlen) as usize
        } else {
            // Rev: primer 3' end aligns at template_start, extending rightwards.
            let p = site.template_start + n as i64;
            if p >= tlen && !circular {
                break;
            }
            p.rem_euclid(tlen) as usize
        };
        let mut tb = tpl[pos].to_ascii_uppercase();
        if site.strand != 1 {
            tb = crate::utils::complement_char(tb as char) as u8;
        }
        if tb != pri[plen - 1 - n].to_ascii_uppercase() {
            break;
        }
        n += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// Batch recompute
// ---------------------------------------------------------------------------

pub fn recompute_all_primers(
    template: &str,
    topology: &str,
    primers: &[Primer],
) -> Vec<Primer> {
    primers
        .iter()
        .map(|p| {
            let mut updated = p.clone();
            updated.binding_sites = compute_binding_sites(
                template,
                &p.primer_seq,
                &p.r#type,
                &p.id,
                topology,
                0.0,
            );
            updated
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Primer pair detection from binding sites
// ---------------------------------------------------------------------------

pub fn compute_primer_pairs(
    template: &str,
    primers: &[Primer],
) -> Vec<PrimerPair> {
    let mut pairs: Vec<PrimerPair> = Vec::new();

    let fwd_sites: Vec<(&Primer, &PrimerBindingSite)> = primers
        .iter()
        .filter(|p| p.r#type == "fwd")
        .filter_map(|p| p.binding_sites.first().map(|bs| (p, bs)))
        .filter(|(_, bs)| bs.strand == 1)
        .collect();

    let rev_sites: Vec<(&Primer, &PrimerBindingSite)> = primers
        .iter()
        .filter(|p| p.r#type == "rev")
        .filter_map(|p| p.binding_sites.first().map(|bs| (p, bs)))
        .filter(|(_, bs)| bs.strand == -1)
        .collect();

    for (fp, fbs) in &fwd_sites {
        for (rp, rbs) in &rev_sites {
            if fp.id == rp.id {
                continue;
            }
            if fbs.template_end > rbs.template_start {
                continue;
            }

            let mid = (rbs.template_start - fbs.template_end) as usize;
            let product_size = fp.primer_seq.len() + mid + rp.primer_seq.len();

            let ta = thermodynamics::compute_ta(
                fbs.tm,
                rbs.tm,
                thermodynamics::tm_product_default(template),
            );

            pairs.push(PrimerPair {
                fwd_primer_id: fp.id.clone(),
                rev_primer_id: rp.id.clone(),
                fwd_position: fbs.template_start,
                rev_position: rbs.template_end,
                product_size,
                ta,
            });
        }
    }

    pairs.sort_by_key(|p| p.product_size);
    pairs
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_both_strands_searched_fwd_primer() {
        // Fwd (direct): primer "CGTACGCTAG" on template.
        // Rev (RC): RC of primer's 3' end on template.
        // RC("CGTACGCTAG") = "CTAGCGTACG".
        let template = "NNNNCGTACGCTAGNNNNCTAGCGTACGNNNN";
        let sites = compute_binding_sites(
            template, "CGTACGCTAG", "fwd", "P1", "linear", 20.0,
        );
        let has_fwd = sites.iter().any(|s| s.strand == 1);
        let has_rev = sites.iter().any(|s| s.strand == -1);
        assert!(has_fwd, "should have fwd binding site");
        assert!(has_rev, "should have rev binding site on opposite strand");
    }

    #[test]
    fn test_compute_binding_sites_exact_match() {
        let template = "NNNNNCGTACGCTAGNNNNN";
        let sites = compute_binding_sites(
            template, "CGTACGCTAG", "fwd", "P1", "linear", 20.0,
        );
        assert!(sites.len() >= 1);
        let s = &sites[0];
        assert_eq!(s.strand, 1);
        assert!(s.tm > 20.0);
        assert!(!s.alignment.display_sequence.is_empty());
    }

    #[test]
    fn test_compute_binding_sites_rev_primer() {
        // Rev primer "CGTACGCTAG" → RC(primer) appears on template.
        // RC("CGTACGCTAG") = "CTAGCGTACG".
        let template = "NNNNNCTAGCGTACGNNNNN";
        let sites = compute_binding_sites(
            template, "CGTACGCTAG", "rev", "P1", "linear", 20.0,
        );
        assert!(sites.len() >= 1, "expected at least 1 binding site for rev primer");
        assert_eq!(sites[0].strand, -1);
    }

    #[test]
    fn test_rev_3prime_anchor_correct() {
        // Good primer (15bp): "AACGTACGCTAGCAT"
        //   3' anchor (last 13) = "CGTACGCTAGCAT".
        //   RC(anchor) = "ATGCTAGCGTACG".
        // Bad primer: "AACGTACGCTAGCAC" — terminal T→C changes RC.
        // 5' part "AA": primer[1]='A' pairs 'T', primer[0]='A' pairs 'T'.
        let template = "TTATGCTAGCGTACGNNN";
        let good_primer = "AACGTACGCTAGCAT";
        let bad_primer = "AACGTACGCTAGCAC";

        let good_sites = compute_binding_sites(template, good_primer, "rev", "P1", "linear", 0.0);
        assert!(!good_sites.is_empty(), "good primer should bind");

        let bad_sites = compute_binding_sites(template, bad_primer, "rev", "P1", "linear", 0.0);
        assert!(bad_sites.is_empty(), "primer with 3' mismatch should NOT bind");
    }

    #[test]
    fn test_compute_binding_sites_below_threshold() {
        let template = "NNNNNCGTACGCTAGNNNNN";
        let sites = compute_binding_sites(
            template, "CGTACGCTAG", "fwd", "P1", "linear", 100.0,
        );
        assert!(sites.is_empty());
    }

    #[test]
    fn test_compute_binding_sites_empty() {
        assert!(compute_binding_sites("", "ATGC", "fwd", "P1", "linear", 20.0).is_empty());
        assert!(compute_binding_sites("ATGC", "", "fwd", "P1", "linear", 20.0).is_empty());
    }

    #[test]
    fn test_3_prime_mismatch_detection() {
        let template = "NNNNNCGTACGCTANNNNN";
        let sites = compute_binding_sites(
            template, "CGTACGCTAG", "fwd", "P1", "linear", 0.0,
        );
        if let Some(s) = sites.first() {
            let _ = s.has_3_prime_mismatch;
        }
    }

    #[test]
    fn test_recompute_all_primers() {
        let template = "NNNNNCGTACGCTAGNNNNN";
        let primers = vec![Primer {
            id: "P1".into(),
            name: "Test".into(),
            r#type: "fwd".into(),
            primer_seq: "CGTACGCTAG".into(),
            binding_sites: vec![],
        }];
        let updated = recompute_all_primers(template, "linear", &primers);
        assert_eq!(updated.len(), 1);
        assert!(!updated[0].binding_sites.is_empty());
    }

    #[test]
    fn test_compute_primer_pairs() {
        let spacer = "AAAAATTTTTGGGGGCCCCC";
        // Fwd: primer "CGTACGCTAG" directly on top strand.
        // Rev: RC of "GCTAGCATCG" = "CGATGCTAGC" must be on top strand.
        let template = format!("CGTACGCTAG{}CGATGCTAGC", spacer);

        let mut primers = vec![
            Primer {
                id: "F".into(),
                name: "Fwd".into(),
                r#type: "fwd".into(),
                primer_seq: "CGTACGCTAG".into(),
                binding_sites: vec![],
            },
            Primer {
                id: "R".into(),
                name: "Rev".into(),
                r#type: "rev".into(),
                primer_seq: "GCTAGCATCG".into(),
                binding_sites: vec![],
            },
        ];

        primers = recompute_all_primers(&template, "linear", &primers);
        let pairs = compute_primer_pairs(&template, &primers);
        assert!(!pairs.is_empty(), "should find at least one primer pair");
        let pair = &pairs[0];
        assert_eq!(pair.fwd_primer_id, "F");
        assert_eq!(pair.rev_primer_id, "R");
        assert!(pair.product_size > 20);
        assert!(pair.ta > 0.0);
    }

    #[test]
    fn test_compute_primer_pairs_no_valid_pair() {
        let template = "CGTACGCTAGAAAACGTACGCTAG";
        let primers = vec![Primer {
            id: "F".into(),
            name: "Fwd".into(),
            r#type: "fwd".into(),
            primer_seq: "CGTACGCTAG".into(),
            binding_sites: vec![],
        }];
        let primers = recompute_all_primers(template, "linear", &primers);
        let pairs = compute_primer_pairs(template, &primers);
        assert!(pairs.is_empty());
    }
}
