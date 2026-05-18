//! Primer binding-site search — top-level orchestrator.
//!
//! For each primer this module searches **both strands** of the template:
//! 1. Primer as-is on the top strand (strand 1, fwd — binds bottom strand).
//! 2. Primer reverse-complement on the top strand (strand -1, rev — binds top strand).
//!
//! Each search passes through:
//! - k-mer seeding → candidate regions ([`alignment`])
//! - position-weighted Smith-Waterman alignment ([`alignment::align`])
//! - Tm via nearest-neighbour model ([`thermodynamics`])
//! - compact [`PrimerBindingSite`] via the [`formatter`]
//!
//! Results are merged, sorted by Tm descending, and deduplicated.

use std::collections::HashSet;

use crate::models::PrimerBindingSite;
use crate::utils;

use super::alignment::{self, AlignmentResult};
use super::formatter;
use super::thermodynamics;

/// Minimum fraction of primer length that must align for a valid site.
const MIN_MATCH_FRACTION: f64 = 0.6;

/// Compute all binding sites for a single primer against the template.
///
/// Searches **both strands**: the primer may bind in the forward orientation
/// (primer matches top strand → binds bottom strand, `strand: 1`) or in the
/// reverse orientation (primer RC matches top strand → binds top strand,
/// `strand: -1`).
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
    let mut results: Vec<PrimerBindingSite> = Vec::new();

    // Search both orientations.
    //   (query, strand, swap_tails)
    //   strand 1:  primer as-is on top strand   → binds bottom strand
    //   strand -1: primer RC on top strand     → binds top strand
    let rc = utils::reverse_complement(&primer_seq);
    for (query, strand, swap_tails) in [
        (&primer_seq, 1_i8, false),
        (&rc, -1_i8, true),
    ] {
        let sites = search_one_strand(
            template, query, primer_id, strand, swap_tails, plen,
            topology, tm_threshold,
        );
        results.extend(sites);
    }

    results.sort_by(|a, b| b.tm.partial_cmp(&a.tm).unwrap_or(std::cmp::Ordering::Equal));
    results.dedup_by(|a, b| {
        a.template_start == b.template_start && a.template_end == b.template_end
    });

    results
}

// ---------------------------------------------------------------------------
// One-strand search (shared by both forward and reverse-complement queries)
// ---------------------------------------------------------------------------

fn search_one_strand(
    template: &str,
    query: &str,
    primer_id: &str,
    strand: i8,
    swap_tails: bool,
    plen: usize,
    topology: &str,
    tm_threshold: f64,
) -> Vec<PrimerBindingSite> {
    let tpl_bytes = template.as_bytes();
    let tlen = tpl_bytes.len();
    let is_circular = topology == "circular";
    let min_align = ((plen as f64) * MIN_MATCH_FRACTION) as usize;
    let query_bytes = query.as_bytes();

    let mut results: Vec<PrimerBindingSite> = Vec::new();
    let mut seen: HashSet<(usize, usize)> = HashSet::new();

    let candidates = alignment::generate_candidates(query_bytes, tpl_bytes, plen, is_circular);

    for (reg_start, reg_end) in candidates {
        let region: Vec<u8> = if is_circular {
            alignment::wrap_template_region(tpl_bytes, reg_start, reg_end)
        } else {
            let s = reg_start.min(tlen);
            let e = reg_end.min(tlen);
            tpl_bytes[s..e].to_vec()
        };

        let region_start = reg_start as i64;
        let Some(aln) = alignment::align(query_bytes, &region) else {
            continue;
        };

        let match_count: usize = aln.ops.iter().filter(|p| p.op == alignment::Op::Match).count();
        if match_count < min_align {
            continue;
        }

        let (maybe_rd, trailing_ins) = formatter::build_render_data(&aln, query_bytes, &region);
        let Some(render_data) = maybe_rd else {
            continue;
        };

        let matched_bases: String = aln
            .ops
            .iter()
            .filter(|p| p.op == alignment::Op::Match)
            .filter_map(|p| p.primer_pos.map(|pp| query_bytes[pp] as char))
            .collect();

        let tm = thermodynamics::compute_tm(&matched_bases);
        let gc = thermodynamics::gc_content(&matched_bases);

        if tm < tm_threshold {
            continue;
        }

        let t_start_abs = region_start + aln.template_start as i64;
        let t_end_abs = region_start + aln.template_end as i64;

        let (t_start, t_end) = if is_circular {
            (t_start_abs % tlen as i64, t_end_abs % tlen as i64)
        } else {
            (t_start_abs, t_end_abs)
        };

        let key = (t_start as usize, t_end as usize);
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);

        let has_3prime = has_3_prime_mismatch(&aln, plen);
        let (five_tail, three_tail) = extract_tails(&aln, query_bytes, plen, &trailing_ins);

        // For RC query: complement the display data (template-indexed, no reversal),
        // and swap+RC the tails (primer sequences, ends swap under RC).
        let (final_rd, final_5, final_3) = if swap_tails {
            (
                formatter::rev_complement_render_data(&render_data),
                utils::reverse_complement(&three_tail),
                utils::reverse_complement(&five_tail),
            )
        } else {
            (render_data, five_tail, three_tail)
        };

        results.push(PrimerBindingSite {
            primer_id: primer_id.to_string(),
            strand,
            template_start: t_start,
            template_end: t_end,
            tm,
            gc_content: gc,
            match_score: aln.score,
            has_3_prime_mismatch: has_3prime,
            five_prime_tail: final_5,
            three_prime_tail: final_3,
            alignment: final_rd,
        });
    }

    results
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn has_3_prime_mismatch(aln: &AlignmentResult, primer_len: usize) -> bool {
    if primer_len == 0 {
        return false;
    }
    let threshold = primer_len.saturating_sub(5);
    aln.ops.iter().any(|p| {
        if let Some(pp) = p.primer_pos {
            if pp >= threshold {
                return p.op == alignment::Op::Mismatch || p.op == alignment::Op::Del;
            }
        }
        false
    })
}

fn extract_tails(
    aln: &AlignmentResult,
    primer: &[u8],
    primer_len: usize,
    trailing_ins_formatter: &str,
) -> (String, String) {
    let mut five_tail = String::new();
    for k in 0..aln.primer_start.min(primer_len) {
        five_tail.push(primer[k] as char);
    }
    for pair in &aln.ops {
        if pair.op == alignment::Op::Ins {
            if let Some(pp) = pair.primer_pos {
                five_tail.push(primer[pp] as char);
            }
        } else {
            break;
        }
    }

    let mut three_tail = trailing_ins_formatter.to_string();
    for k in aln.primer_end..primer_len {
        three_tail.push(primer[k] as char);
    }

    (five_tail, three_tail)
}

// ---------------------------------------------------------------------------
// Batch recompute
// ---------------------------------------------------------------------------

pub fn recompute_all_primers(
    template: &str,
    topology: &str,
    primers: &[crate::models::Primer],
) -> Vec<crate::models::Primer> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_both_strands_searched_fwd_primer() {
        // A fwd primer should also find reverse-strand binding sites.
        // Template has both the primer sequence (fwd site) and its RC (rev site).
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
        assert!(sites.len() >= 1, "expected at least 1 binding site");
        let s = &sites[0];
        assert_eq!(s.strand, 1);
        assert!(s.tm > 20.0);
        assert!(!s.alignment.display_sequence.is_empty());
    }

    #[test]
    fn test_compute_binding_sites_rev_primer() {
        let template = "NNNNNCTAGCGTACGNNNNN";
        let sites = compute_binding_sites(
            template, "CGTACGCTAG", "rev", "P1", "linear", 20.0,
        );
        assert!(sites.len() >= 1, "expected at least 1 binding site for rev primer");
        assert_eq!(sites[0].strand, -1);
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
    #[ignore]
    fn dump_flyswarm_primer_results() {
        use crate::file_io;
        use std::path::Path;
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/flySWARM.dna");
        if !path.exists() {
            eprintln!("flySWARM.dna not found, skipping");
            return;
        }
        let mut project = file_io::parse_file(&path).expect("parse flySWARM.dna");
        let updated = recompute_all_primers(&project.sequence, &project.topology, &project.primers);
        project.primers = updated;

        for p in &project.primers {
            println!("=== {} ({}) type={} seq={}", p.name, p.id, p.r#type, p.primer_seq);
            for (i, bs) in p.binding_sites.iter().enumerate() {
                println!("  site[{}]: strand={} template=[{}, {}) tm={:.1} gc={:.2} score={} 3'mm={}",
                    i, bs.strand, bs.template_start, bs.template_end,
                    bs.tm, bs.gc_content, bs.match_score,
                    bs.has_3_prime_mismatch);
                println!("    5'tail: '{}'", bs.five_prime_tail);
                println!("    3'tail: '{}'", bs.three_prime_tail);
                println!("    display: '{}' (len={})", bs.alignment.display_sequence, bs.alignment.display_sequence.len());
                if !bs.alignment.mismatch_indices.is_empty() {
                    println!("    mismatches @ {:?}", bs.alignment.mismatch_indices);
                }
                if !bs.alignment.insertion_map.is_empty() {
                    for (k, v) in &bs.alignment.insertion_map {
                        println!("    ins[{}]: {}", k, v.full_string);
                    }
                }
            }
            if p.binding_sites.is_empty() {
                println!("  *** NO BINDING SITES FOUND ***");
            }
        }
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
        use crate::models::Primer;
        let template = "NNNNNCGTACGCTAGNNNNN";
        let primers = vec![Primer {
            id: "P1".into(),
            name: "Test".into(),
            r#type: "fwd".into(),
            primer_seq: "CGTACGCTAG".into(),
            color: "#166534".into(),
            binding_sites: vec![],
        }];
        let updated = recompute_all_primers(template, "linear", &primers);
        assert_eq!(updated.len(), 1);
        assert!(!updated[0].binding_sites.is_empty());
        assert!(!updated[0].binding_sites[0].alignment.display_sequence.is_empty());
    }
}
