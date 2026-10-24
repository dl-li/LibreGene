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

use crate::models::{AlignmentRenderData, Primer, PrimerBindingSite};

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
        template,
        &primer_seq,
        primer_id,
        1,
        false,
        is_circular,
        tm_threshold,
    ));

    // Rev (strand -1): primer complements top strand.
    // Search the primer AS-IS with complement regex on the template.
    results.extend(search_one_strand(
        template,
        &primer_seq,
        primer_id,
        -1,
        true,
        is_circular,
        tm_threshold,
    ));

    // Dedup by (start, end, strand) keeping the highest-Tm entry of each
    // group, then present results sorted by Tm descending (best first).
    results.sort_by(|a, b| {
        b.tm.partial_cmp(&a.tm)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                (a.template_start, a.template_end, a.strand).cmp(&(
                    b.template_start,
                    b.template_end,
                    b.strand,
                ))
            })
    });
    let mut seen: HashSet<(i64, i64, i8)> = HashSet::new();
    results.retain(|r| seen.insert((r.template_start, r.template_end, r.strand)));

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
    is_circular: bool,
    tm_threshold: f64,
) -> Vec<PrimerBindingSite> {
    let tpl_bytes = template.as_bytes();
    let primer_bytes = primer.as_bytes();
    let plen = primer_bytes.len();
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
        // template_end is 0-based exclusive; a site ending exactly on the last
        // base keeps tlen (== 1-based inclusive end). Only a genuine overshoot
        // wraps to a small residue — % tlen would wrongly map tlen → 0 and make
        // the site look like it spans the whole circle.
        let raw_end = t_start + site.footprint_len;
        let t_end = if is_circular && raw_end > tlen {
            raw_end % tlen
        } else {
            raw_end
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
            primer_bytes[footprint_start..]
                .iter()
                .rev()
                .copied()
                .collect()
        } else {
            primer_bytes[footprint_start..].to_vec()
        };

        // Tm uses the footprint in the primer's own 5'→3' orientation for BOTH
        // strands: the NN table is duplex-symmetric (params(XY) == params of
        // comp(Y)comp(X)), so Tm(F) == Tm(RC(F)), but Tm(rev(F)) differs —
        // feeding the reversed (display-oriented) footprint would give the same
        // physical site a different Tm per strand.
        let matched_str = String::from_utf8_lossy(&primer_bytes[footprint_start..]).to_string();
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

        // Display render data: align the FULL primer with the same engine the
        // alignment dialog uses, so the editor's per-column display reflects
        // the real alignment (mismatches, `-` gaps, `··` insertions) instead
        // of the bare anneal + hanging-tail split. Display-only — the anchor
        // coordinates, tails and Tm above are untouched. On failure fall back
        // to the footprint-only display with the whole tail hanging.
        const DISPLAY_SLACK: usize = 10;
        let display = if use_complement {
            // Rev: display walks the primer 3'→5' (reversed) left→right from
            // the anchor's left edge; the tail context extends rightward.
            let rev_primer: Vec<u8> = primer_bytes.iter().rev().copied().collect();
            let w = (plen + DISPLAY_SLACK).min(if is_circular { tlen } else { tlen - t_start });
            let region = if is_circular {
                super::alignment::wrap_template_region(tpl_bytes, t_start, (t_start + w) % tlen)
            } else {
                tpl_bytes[t_start..t_start + w].to_vec()
            };
            super::alignment::align_first_base_constrained_rev(&rev_primer, &region).and_then(
                |res| {
                    // Trim the 5' edge (= trailing end in display orientation).
                    let rops: Vec<_> = res.ops.iter().rev().cloned().collect();
                    let (start, hang_plus, _) = trim_display_edge(&rops);
                    let ops: Vec<_> = rops[start..].iter().rev().cloned().collect();
                    let hang_len = plen - res.primer_end + hang_plus;
                    if ops.is_empty() {
                        return None;
                    }
                    let (cols, mism) = formatter::display_columns(&ops, &rev_primer);
                    let hang: String = primer_bytes[..hang_len]
                        .iter()
                        .map(|&b| b as char)
                        .collect();
                    Some((
                        AlignmentRenderData {
                            display_sequence: cols,
                            mismatch_indices: mism,
                            start_offset: res.template_start as i64,
                            ..Default::default()
                        },
                        hang,
                    ))
                },
            )
        } else {
            // Fwd: the window ends at the anchor's 3' end (raw_end, unwrapped).
            let w = (plen + DISPLAY_SLACK).min(if is_circular { tlen } else { raw_end });
            let win_start_raw = raw_end as i64 - w as i64;
            let region = if is_circular {
                super::alignment::wrap_template_region(
                    tpl_bytes,
                    win_start_raw.rem_euclid(tlen as i64) as usize,
                    raw_end % tlen,
                )
            } else {
                tpl_bytes[win_start_raw as usize..raw_end].to_vec()
            };
            super::alignment::align_3prime_constrained(primer_bytes, &region).and_then(|res| {
                let (start, hang_plus, tpl_skip) = trim_display_edge(&res.ops);
                let ops = &res.ops[start..];
                let hang_len = res.primer_start + hang_plus;
                let ts = res.template_start + tpl_skip;
                if ops.is_empty() {
                    return None;
                }
                let (cols, mism) = formatter::display_columns(ops, primer_bytes);
                let hang: String = primer_bytes[..hang_len]
                    .iter()
                    .map(|&b| b as char)
                    .collect();
                Some((
                    AlignmentRenderData {
                        display_sequence: cols,
                        mismatch_indices: mism,
                        start_offset: win_start_raw + ts as i64 - t_start as i64,
                        ..Default::default()
                    },
                    hang,
                ))
            })
        };
        let (final_rd, display_tail) = match display {
            Some(d) => d,
            None => {
                let Some(rd) = formatter::build_render_data(&footprint_bases, &template_region)
                else {
                    continue;
                };
                (rd, five_tail.clone())
            }
        };

        let has_3prime = site.has_3_prime_mismatch;

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
            display_tail,
            alignment: final_rd,
        });
    }

    results
}

// ---------------------------------------------------------------------------
// Display edge trimming
// ---------------------------------------------------------------------------

/// Minimum run of consecutive matches that anchors the primer's 5' edge on
/// the display. A shorter edge run that only aligns via indels is
/// incidental; it dissolves into the hanging tail instead of forcing `-` /
/// `··` placeholders.
const MIN_EDGE_RUN: usize = 4;

/// Trim the primer's 5' edge (index-0 side of `ops`) for display:
/// hang edge insertion runs, drop deletion columns left dangling under the
/// overhang, and dissolve an incidental fringe (a short match run that only
/// aligns via indels) into the hanging tail. A fringe of matches and
/// mismatches without indels stays aligned.
///
/// Returns (ops_start, extra_hang_bases, template_columns_skipped).
fn trim_display_edge(ops: &[super::alignment::AlignedPair]) -> (usize, usize, usize) {
    use super::alignment::Op;
    let mut start = 0;
    let mut hang = 0;
    let mut skip = 0;
    loop {
        let ins = ops[start..].iter().take_while(|p| p.op == Op::Ins).count();
        if ins > 0 {
            hang += ins;
            start += ins;
            continue;
        }
        if hang > 0 {
            let del = ops[start..].iter().take_while(|p| p.op == Op::Del).count();
            if del > 0 {
                skip += del;
                start += del;
                continue;
            }
        }
        break;
    }
    let rest = &ops[start..];
    let mut i = 0;
    let run_start = loop {
        while i < rest.len() && rest[i].op != Op::Match {
            i += 1;
        }
        let s = i;
        while i < rest.len() && rest[i].op == Op::Match {
            i += 1;
        }
        if i - s >= MIN_EDGE_RUN {
            break Some(s);
        }
        if i >= rest.len() {
            break None;
        }
    };
    if let Some(rs) = run_start {
        let fringe = &rest[..rs];
        if fringe.iter().any(|p| matches!(p.op, Op::Ins | Op::Del)) {
            hang += fringe.iter().filter(|p| p.op != Op::Del).count();
            skip += fringe.iter().filter(|p| p.op != Op::Ins).count();
            start += rs;
        }
    }
    (start, hang, skip)
}

// ---------------------------------------------------------------------------
// Anneal-core length
// ---------------------------------------------------------------------------

/// Full-length template coverage of a binding site: for every primer base
/// (5'→3') the template base it faces and whether it matches. The contiguous
/// 3' footprint is extended 5'-ward into the adjacent template, so 5' tail
/// bases that happen to pair with the template are visible (they extend
/// `anneal_len` and raise Tm beyond design values — expected, not anomalous
/// binding). Tail positions hanging off the end of a LINEAR template have no
/// template base; circular templates wrap.
///
/// Returns `(aligned_template, match_mask)`, both exactly `primer_seq.len()`
/// chars, 5'→3': `aligned_template` holds the template base (`'-'` where none
/// exists; bases are complemented for strand -1 so they compare directly
/// against the primer); `match_mask` is `'|'` (match), `'.'` (mismatch) or
/// `'-'` (no template base).
pub fn template_coverage(
    template: &str,
    topology: &str,
    primer_seq: &str,
    site: &PrimerBindingSite,
) -> (String, String) {
    let tlen = template.len() as i64;
    let plen = primer_seq.len() as i64;
    let mut aligned = String::with_capacity(primer_seq.len());
    let mut mask = String::with_capacity(primer_seq.len());
    if tlen == 0 || plen == 0 {
        return (aligned, mask);
    }
    let tpl = template.as_bytes();
    let pri = primer_seq.as_bytes();
    let circular = topology == "circular";
    let footprint_start = site.five_prime_tail.len() as i64;
    for i in 0..plen {
        // Offset of primer position i relative to the footprint's 5' end.
        let rel = i - footprint_start;
        let pos = if site.strand == 1 {
            site.template_start + rel
        } else {
            site.template_end - 1 - rel
        };
        if !circular && (pos < 0 || pos >= tlen) {
            aligned.push('-');
            mask.push('-');
            continue;
        }
        let mut tb = tpl[pos.rem_euclid(tlen) as usize].to_ascii_uppercase();
        if site.strand != 1 {
            tb = crate::utils::complement_char(tb as char) as u8;
        }
        aligned.push(tb as char);
        mask.push(if tb == pri[i as usize].to_ascii_uppercase() {
            '|'
        } else {
            '.'
        });
    }
    (aligned, mask)
}

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

pub fn recompute_all_primers(template: &str, topology: &str, primers: &[Primer]) -> Vec<Primer> {
    primers
        .iter()
        .map(|p| {
            let mut updated = p.clone();
            updated.binding_sites =
                compute_binding_sites(template, &p.primer_seq, &p.r#type, &p.id, topology, 0.0);
            updated
        })
        .collect()
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
        let sites = compute_binding_sites(template, "CGTACGCTAG", "fwd", "P1", "linear", 20.0);
        let has_fwd = sites.iter().any(|s| s.strand == 1);
        let has_rev = sites.iter().any(|s| s.strand == -1);
        assert!(has_fwd, "should have fwd binding site");
        assert!(has_rev, "should have rev binding site on opposite strand");
    }

    #[test]
    fn test_compute_binding_sites_exact_match() {
        let template = "NNNNNCGTACGCTAGNNNNN";
        let sites = compute_binding_sites(template, "CGTACGCTAG", "fwd", "P1", "linear", 20.0);
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
        let sites = compute_binding_sites(template, "CGTACGCTAG", "rev", "P1", "linear", 20.0);
        assert!(
            sites.len() >= 1,
            "expected at least 1 binding site for rev primer"
        );
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
        assert!(
            bad_sites.is_empty(),
            "primer with 3' mismatch should NOT bind"
        );
    }

    #[test]
    fn test_circular_site_ending_at_last_base_keeps_tlen_end() {
        // Footprint ends exactly on the last template base: template_end is
        // 0-based exclusive and must stay tlen (== 1-based inclusive end),
        // not wrap to 0 via % tlen (which renders as a full-circle site).
        let primer = "AAAACCCCGGGGTTTTACGT";
        let rc = crate::utils::reverse_complement(primer);
        let template = format!("TTGAC{}", rc);
        let tlen = template.len() as i64;
        let sites = compute_binding_sites(&template, primer, "rev", "P1", "circular", 0.0);
        let s = sites.iter().find(|s| s.strand == -1).expect("rev site");
        assert_eq!(s.template_start, 5);
        assert_eq!(s.template_end, tlen);

        // Same for a fwd primer matching the template end directly.
        let fwd_template = format!("TTGAC{}", primer);
        let sites = compute_binding_sites(&fwd_template, primer, "fwd", "P2", "circular", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert_eq!(s.template_start, 5);
        assert_eq!(s.template_end, tlen);
    }

    #[test]
    fn test_circular_multi_lap_primer_end_is_normalized() {
        let template = "ACGTACG";
        let primer = template.repeat(4);
        let sites = compute_binding_sites(template, &primer, "fwd", "P1", "circular", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert!(s.match_score as usize > template.len());
        assert_eq!(
            s.template_end,
            (s.template_start + i64::from(s.match_score)) % template.len() as i64
        );
    }

    #[test]
    fn test_compute_binding_sites_below_threshold() {
        let template = "NNNNNCGTACGCTAGNNNNN";
        let sites = compute_binding_sites(template, "CGTACGCTAG", "fwd", "P1", "linear", 100.0);
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
        let sites = compute_binding_sites(template, "CGTACGCTAG", "fwd", "P1", "linear", 0.0);
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
    fn test_template_coverage_fwd_tail_overhang() {
        // 13-bp core fills the whole linear template; the 5' tail "GG" hangs
        // off the left end and gets '-' (no template base).
        let template = "CGTACGCTAGCTA";
        let primer = "GGCGTACGCTAGCTA";
        let sites = compute_binding_sites(template, primer, "fwd", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert_eq!(s.five_prime_tail, "GG");
        let (aligned, mask) = template_coverage(template, "linear", primer, s);
        assert_eq!(aligned, "--CGTACGCTAGCTA");
        assert_eq!(mask, "--|||||||||||||");
        assert_eq!(aligned.len(), primer.len());
    }

    #[test]
    fn test_template_coverage_fwd_tail_mismatch_visible() {
        // The greedy footprint stops at the first 5'-ward mismatch, but the
        // coverage still reports the tail base's pairing ('.' entry).
        let template = "TCGTACGCTAGCTA";
        let primer = "ACGTACGCTAGCTA";
        let sites = compute_binding_sites(template, primer, "fwd", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert_eq!(s.five_prime_tail, "A");
        let (aligned, mask) = template_coverage(template, "linear", primer, s);
        assert_eq!(aligned, "TCGTACGCTAGCTA");
        assert_eq!(mask, ".|||||||||||||");
    }

    #[test]
    fn test_template_coverage_rev_strand() {
        // Rev primer: coverage compares against the complemented template, so
        // aligned_template reads like the primer itself. Tail bases extend
        // rightwards on the top strand and mismatch there.
        let template = "TAGCTAGCGTACGCC";
        let primer = "TTCGTACGCTAGCTA";
        let sites = compute_binding_sites(template, primer, "rev", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == -1).expect("rev site");
        assert_eq!(s.five_prime_tail, "TT");
        let (aligned, mask) = template_coverage(template, "linear", primer, s);
        assert_eq!(aligned, "GGCGTACGCTAGCTA");
        assert_eq!(mask, "..|||||||||||||");
    }

    #[test]
    fn test_template_coverage_circular_wraps() {
        // Circular template: tail bases wrap across the origin instead of
        // hanging off an end (primer[0] happens to match, primer[1] not).
        let template = "CGTACGCTAGCTA";
        let primer = "TTCGTACGCTAGCTA";
        let sites = compute_binding_sites(template, primer, "fwd", "P1", "circular", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert_eq!(s.five_prime_tail, "TT");
        let (aligned, mask) = template_coverage(template, "circular", primer, s);
        assert_eq!(aligned, "TACGTACGCTAGCTA");
        assert_eq!(mask, "|.|||||||||||||");
    }

    #[test]
    fn test_display_render_tail_mismatch_and_hang() {
        // 13 bp footprint with a 5' tail "TA": T pairs the template, A does
        // not — the alignment absorbs both, so the display span extends two
        // columns left of the anchor with the A flagged as mismatch.
        let template = "CCCTGCGTACGCTAGCGACCC";
        let primer = "TACGTACGCTAGCGA";
        let sites = compute_binding_sites(template, primer, "fwd", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert_eq!((s.template_start, s.template_end), (5, 18));
        assert_eq!(s.five_prime_tail, "TA");
        assert_eq!(s.alignment.display_sequence, "TACGTACGCTAGCGA");
        assert_eq!(s.alignment.mismatch_indices, vec![1]);
        assert_eq!(s.alignment.start_offset, -2);
        assert_eq!(s.display_tail, "");
    }

    #[test]
    fn test_display_render_edge_insertion_dissolves_to_tail() {
        // The user's example transplanted into a tail: primer ...TACTAGTACGC
        // vs template ...TACCGC — the short TAC match before the insertion is
        // incidental, so the whole fringe hangs as a tail instead of forcing
        // `··` dots.
        let template = "GGGTACCGCCGTACGCTAGCGAGGG";
        let primer = "TACTAGTACGCCGTACGCTAGCGA";
        let sites = compute_binding_sites(template, primer, "fwd", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert_eq!(s.alignment.display_sequence, "CGCCGTACGCTAGCGA");
        assert!(s.alignment.mismatch_indices.is_empty());
        assert_eq!(s.alignment.start_offset, 0);
        assert_eq!(s.display_tail, "TACTAGTA");
    }

    #[test]
    fn test_display_render_interior_insertion_collapses_to_dots() {
        // Insertion with a solid match run (>= 4) on both sides stays aligned:
        // the inserted bases plus one flanking column per side render as `··`.
        let template = "GGGTTTTCCCGTACGCTAGCGAGGG";
        let primer = "TTTTACACACCCGTACGCTAGCGA";
        let sites = compute_binding_sites(template, primer, "fwd", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert_eq!(s.alignment.display_sequence, "TTT··CCGTACGCTAGCGA");
        assert!(s.alignment.mismatch_indices.is_empty());
        assert_eq!(s.display_tail, "");
    }

    #[test]
    fn test_display_render_edge_gap_dissolves_to_tail() {
        // Template has two extra bases the primer lacks, but the short TT
        // match before the gap is incidental: the fringe hangs as a tail
        // instead of rendering `TT--`.
        let template = "GGGTTAACCCGTACGCTAGCGAGGG";
        let primer = "TTCCCGTACGCTAGCGA";
        let sites = compute_binding_sites(template, primer, "fwd", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert_eq!(s.alignment.display_sequence, "CCCGTACGCTAGCGA");
        assert_eq!(s.alignment.start_offset, 0);
        assert_eq!(s.display_tail, "TT");
    }

    #[test]
    fn test_display_render_interior_gap_renders_dashes() {
        // Gap with a solid match run (>= 4) on both sides stays aligned: one
        // `-` column per template base the primer lacks.
        let template = "GGGTTTTAACCCGTACGCTAGCGAGGG";
        let primer = "TTTTCCCGTACGCTAGCGA";
        let sites = compute_binding_sites(template, primer, "fwd", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert_eq!(s.alignment.display_sequence, "TTTT--CCCGTACGCTAGCGA");
        assert_eq!(s.display_tail, "");
    }

    #[test]
    fn test_display_render_unalignable_tail_hangs() {
        // A 5' adapter with no pairing to the template stays a hanging tail.
        let template = "GGGGGGCGTACGCTAGCGAGGG";
        let primer = "TTAATTAACGTACGCTAGCGA";
        let sites = compute_binding_sites(template, primer, "fwd", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == 1).expect("fwd site");
        assert_eq!(s.alignment.display_sequence, "CGTACGCTAGCGA");
        assert_eq!(s.alignment.start_offset, 0);
        assert_eq!(s.display_tail, "TTAATTAA");
    }

    #[test]
    fn test_display_render_rev_tail_extends_right() {
        // Rev primer whose 5' tail "TA" pairs the template right of the
        // footprint: the display span absorbs both columns (matches).
        let footprint = "CGTACGCTAGCGA";
        let rc = crate::utils::reverse_complement(footprint);
        let template = format!("GGG{}TAGGG", rc);
        let primer = format!("TA{}", footprint);
        let sites = compute_binding_sites(&template, &primer, "rev", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == -1).expect("rev site");
        let expect: String = footprint.chars().rev().collect::<String>() + "AT";
        assert_eq!(s.alignment.display_sequence, expect);
        assert_eq!(s.alignment.start_offset, 0);
        assert_eq!(s.display_tail, "");
        assert!(s.alignment.mismatch_indices.is_empty());
    }

    #[test]
    fn test_display_render_rev_tail_hangs() {
        // Rev tail that pairs nothing right of the footprint hangs as before.
        let footprint = "CGTACGCTAGCGA";
        let rc = crate::utils::reverse_complement(footprint);
        let template = format!("GGG{}GGGGG", rc);
        let primer = format!("TA{}", footprint);
        let sites = compute_binding_sites(&template, &primer, "rev", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == -1).expect("rev site");
        let expect: String = footprint.chars().rev().collect();
        assert_eq!(s.alignment.display_sequence, expect);
        assert_eq!(s.display_tail, "TA");
    }

    #[test]
    fn test_display_render_rev_edge_gap_dissolves_to_tail() {
        // Rev mirror of the fwd edge-gap case: the tail's incidental AA match
        // across a two-column gap dissolves into the hanging tail.
        let footprint = "CGTACGCTAGCGA";
        let rc = crate::utils::reverse_complement(footprint);
        let template = format!("GGG{}AATTGGG", rc);
        let primer = format!("AA{}", footprint);
        let sites = compute_binding_sites(&template, &primer, "rev", "P1", "linear", 0.0);
        let s = sites.iter().find(|s| s.strand == -1).expect("rev site");
        let expect: String = footprint.chars().rev().collect();
        assert_eq!(s.alignment.display_sequence, expect);
        assert_eq!(s.display_tail, "AA");
    }

    #[test]
    fn test_binding_sites_symmetric_under_template_rc() {
        // Aligning a primer against T vs against RC(T) must yield mirrored
        // sites: flipped strand, mirrored coordinates, identical Tm / score /
        // tail. The same physical duplex must not report two Tm values.
        let template = "ATGCGGTACCTAGGCTAACGGTTCAGGCCATGGATCCGTAACGTTAGCCGGTAA";
        let rct = crate::utils::reverse_complement(template);
        let tlen = template.len() as i64;

        for topology in ["linear", "circular"] {
            for primer in [
                template[10..30].to_string(),                    // exact 20-mer
                format!("GGC{}", &template[10..28]),             // 5' tail
                format!("{}R{}", &template[10..20], &template[21..30]), // IUPAC
            ] {
                let on_t = compute_binding_sites(template, &primer, "fwd", "P1", topology, 0.0);
                let on_rc = compute_binding_sites(&rct, &primer, "fwd", "P1", topology, 0.0);
                assert_eq!(on_t.len(), on_rc.len(), "{topology} primer {primer}");
                for s in &on_t {
                    let m_start = (tlen - s.template_end).rem_euclid(tlen);
                    let m_end = (tlen - s.template_start).rem_euclid(tlen);
                    let m = on_rc
                        .iter()
                        .find(|r| {
                            r.strand == -s.strand
                                && r.template_start == m_start
                                && r.template_end == m_end
                        })
                        .unwrap_or_else(|| {
                            panic!(
                                "{topology} primer {primer}: no mirror for strand={} {}..{}",
                                s.strand, s.template_start, s.template_end
                            )
                        });
                    assert_eq!(m.tm, s.tm, "Tm must be identical under template RC");
                    assert_eq!(m.match_score, s.match_score);
                    assert_eq!(m.five_prime_tail, s.five_prime_tail);
                    assert_eq!(m.has_3_prime_mismatch, s.has_3_prime_mismatch);
                }
            }
        }
    }
}
