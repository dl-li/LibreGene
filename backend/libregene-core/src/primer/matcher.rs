//! Pydna-style primer annealing search with IUPAC regex + greedy extension.
//!
//! The algorithm:
//! 1. Take the primer's **own 3' end** (last `limit` bases) as the anchor.
//! 2. Build a regex from the anchor — either direct match (fwd) or complement
//!    match (rev) on the template top strand.
//! 3. For each regex hit, greedily extend leftward (5' direction) until the
//!    first mismatching base.
//!
//! Critically, the anchor is ALWAYS the primer's 3' end, regardless of whether
//! we're searching for fwd or rev binding. The only difference is whether we
//! match the anchor directly (primer = top strand → fwd) or via complement
//! (primer complements top strand → rev).

use regex::Regex;

use super::iupac;

/// Default minimum 3' anchor length (matching pydna).
pub const DEFAULT_LIMIT: usize = 13;

/// Result of an annealing position search.
#[derive(Debug, Clone)]
pub struct AnnealingSite {
    /// Template start position (0-based, inclusive) — the 5'-most matched base.
    pub template_start: usize,
    /// Length of the perfectly-matching footprint (in bases).
    pub footprint_len: usize,
    /// Whether the 3'-most 5 bases of the footprint contain a mismatch or an
    /// ambiguous (IUPAC-degenerate) pairing.
    pub has_3_prime_mismatch: bool,
}

/// Build a regex character class for a single IUPAC base (direct match).
fn base_to_regex(b: u8) -> String {
    let expanded = iupac::iupac_expand(b);
    if expanded.is_empty() {
        return ".".to_string();
    }
    if expanded.len() == 1 {
        regex::escape(&(expanded[0] as char).to_string())
    } else {
        let chars: String = expanded.iter().map(|&x| x as char).collect();
        format!("[{}]", chars)
    }
}

/// Test whether `primer_base` on the primer strand can pair with
/// `template_base` on the template strand. The mode determines whether
/// we check direct overlap (primer = template) or complement pairing.
fn bases_compatible(primer_base: u8, template_base: u8, use_complement: bool) -> bool {
    if use_complement {
        iupac::bases_pair(primer_base, template_base)
    } else {
        iupac::bases_overlap(primer_base, template_base)
    }
}

/// Find all annealing positions of `primer` on `template` (top strand).
///
/// The primer sequence is always in the 5'→3' orientation. The anchor is
/// always the primer's own 3' end.
///
/// * `use_complement = false` → fwd primer: primer binds bottom strand,
///   so the primer sequence matches the top strand directly.
/// * `use_complement = true`  → rev primer: primer binds top strand,
///   so we search for the complement of the primer's 3' end on the top strand.
pub fn find_annealing_positions(
    primer: &[u8],
    template: &[u8],
    limit: usize,
    use_complement: bool,
) -> Vec<AnnealingSite> {
    if primer.len() < limit || template.len() < limit {
        return vec![];
    }

    let plen = primer.len();

    // 3' anchor = last `limit` bases of the PRIMER (always primer's own 3' end).
    let anchor = &primer[plen - limit..];

    // Build regex. For rev primers, each base is matched via its complement.
    // pydna: reversed anchor + complement regex → matches RC(anchor) on template.
    let anchor_re: String = if use_complement {
        anchor
            .iter()
            .rev()
            .map(|&b| iupac::iupac_compl_regex(b))
            .collect()
    } else {
        anchor.iter().map(|&b| base_to_regex(b)).collect()
    };
    // Templates may carry lowercase bases (SnapGene ORIGIN); match case-insensitively.
    let anchor_re = format!("(?i){}", anchor_re);

    let re = match Regex::new(&anchor_re) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    // Template is guaranteed ASCII DNA — use zero-alloc str conversion
    let template_str = std::str::from_utf8(template).unwrap_or("");
    let primer_upper: Vec<u8> = primer.iter().map(|&b| b.to_ascii_uppercase()).collect();

    let mut results: Vec<AnnealingSite> = Vec::new();

    let tlen = template.len();

    for m in re.find_iter(template_str) {
        let anchor_start = m.start();
        let anchor_end = m.end(); // anchor_start + limit

        let five_prime_part = &primer_upper[..plen - limit];
        let mut ext = 0usize;

        if use_complement {
            // Rev primer: 5' end extends RIGHT (higher template coords).
            for (i, &pbase) in five_prime_part.iter().rev().enumerate() {
                let tpos = anchor_end + i;
                if tpos >= tlen {
                    break;
                }
                let mut tbase = template[tpos].to_ascii_uppercase();
                if tbase == b'N' { break; }
                if tbase == b'U' { tbase = b'T'; }

                if bases_compatible(pbase, tbase, true) {
                    ext += 1;
                } else {
                    break;
                }
            }
        } else {
            // Fwd primer: 5' end extends LEFT (lower template coords).
            for (i, &pbase) in five_prime_part.iter().rev().enumerate() {
                let tpos = anchor_start as isize - 1 - i as isize;
                if tpos < 0 { break; }
                let mut tbase = template[tpos as usize].to_ascii_uppercase();
                if tbase == b'N' { break; }
                if tbase == b'U' { tbase = b'T'; }

                if bases_compatible(pbase, tbase, false) {
                    ext += 1;
                } else {
                    break;
                }
            }
        }

        let template_start = if use_complement {
            anchor_start
        } else {
            anchor_start - ext
        };
        let footprint_len = limit + ext;

        // The 3'-most 5 aligned bases decide priming quality: flag a mismatch
        // or an ambiguous (degenerate) pairing there. Issues further 5'-ward
        // (deeper in the anchor or in the extension) do not set the flag.
        let mut has_3_prime_mismatch = false;
        for k in 0..footprint_len.min(5) {
            let pbase = primer_upper[plen - 1 - k];
            let tpos = if use_complement {
                anchor_start + k
            } else {
                anchor_end - 1 - k
            };
            let mut tbase = template[tpos].to_ascii_uppercase();
            if tbase == b'U' { tbase = b'T'; }
            if !bases_compatible(pbase, tbase, use_complement)
                || iupac::iupac_expand(pbase).len() > 1
                || iupac::iupac_expand(tbase).len() > 1
            {
                has_3_prime_mismatch = true;
                break;
            }
        }

        results.push(AnnealingSite { template_start, footprint_len, has_3_prime_mismatch });
    }

    results.sort_by(|a, b| b.footprint_len.cmp(&a.footprint_len));
    results.dedup_by(|a, b| a.template_start == b.template_start);
    results
}

/// Convenience: search on an extended (doubled) template for circular sequences.
pub fn find_annealing_circular(
    primer: &[u8],
    template: &[u8],
    limit: usize,
    use_complement: bool,
) -> Vec<AnnealingSite> {
    let tlen = template.len();
    if tlen == 0 || primer.is_empty() {
        return vec![];
    }
    let mut doubled = Vec::with_capacity(tlen * 2);
    doubled.extend_from_slice(template);
    doubled.extend_from_slice(template);

    let mut sites = find_annealing_positions(primer, &doubled, limit, use_complement);
    // Dedup must happen after mod-normalization: the same physical site found
    // once per lap has distinct doubled-template coordinates and survives the
    // pre-normalization dedup inside find_annealing_positions.
    for s in &mut sites {
        s.template_start %= tlen;
    }
    let mut seen = std::collections::HashSet::new();
    sites.retain(|s| seen.insert(s.template_start));
    sites
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Direct mode (fwd) ----

    #[test]
    fn test_exact_match_direct() {
        let template = b"NNNNNCGTACGCTAGNNNNN";
        let primer = b"CGTACGCTAG";
        let sites = find_annealing_positions(primer, template, 8, false);
        assert!(!sites.is_empty());
        assert_eq!(sites[0].footprint_len, primer.len());
    }

    #[test]
    fn test_5prime_mismatch_direct() {
        let template = b"NNNNNTACGCTAGNNNNN";
        let primer = b"AAAATACGCTAG";
        let sites = find_annealing_positions(primer, template, 8, false);
        assert!(!sites.is_empty());
        // N in template before anchor → stops at N, ext=0, footprint=8
        assert_eq!(sites[0].footprint_len, 8);
    }

    #[test]
    fn test_no_match_direct() {
        let template = b"ATATATATATATATAT";
        let primer = b"CGTACGCTAG";
        let sites = find_annealing_positions(primer, template, 8, false);
        assert!(sites.is_empty());
    }

    // ---- Complement mode (rev) ----

    #[test]
    fn test_exact_match_complement() {
        // Primer "CGTACGCTAG", anchor "TACGCTAG".
        // RC = "CTAGCGTA". 5' part "CG": primer[1]='G' pairs 'C', primer[0]='C' pairs 'G'.
        // Template: RC_anchor + "CG" = "CTAGCGTA" + "CG".
        let template = b"NNCTAGCGTACGNN";
        let primer = b"CGTACGCTAG";
        let sites = find_annealing_positions(primer, template, 8, true);
        assert!(!sites.is_empty());
        assert_eq!(sites[0].footprint_len, 10);
    }

    #[test]
    fn test_rev_primer_binding() {
        // Primer "GCTAGCATCG", anchor "TAGCATCG".
        // RC = "CGATGCTA". 5' part "GC": primer[1]='C' pairs 'G', primer[0]='G' pairs 'C'.
        // Template: RC_anchor + "GC" = "CGATGCTA" + "GC" = "CGATGCTAGC"
        let template = b"CGATGCTAGCNN";
        let primer = b"GCTAGCATCG";
        let sites = find_annealing_positions(primer, template, 8, true);
        assert!(!sites.is_empty());
        assert_eq!(sites[0].footprint_len, 10);
    }

    #[test]
    fn test_complement_extension_stops_at_mismatch() {
        // Primer "AAGCTAGCATCG", anchor "TAGCATCG", RC="CGATGCTA".
        // 5' part "AAGC". N after RC → extension stops immediately.
        let template = b"CGATGCTANNNA";
        let primer = b"AAGCTAGCATCG";
        let sites = find_annealing_positions(primer, template, 8, true);
        assert!(!sites.is_empty());
        assert_eq!(sites[0].footprint_len, 8);
    }

    #[test]
    fn test_rev_3prime_mismatch_blocks() {
        // Good primer "GCTAGCATCG": anchor "TAGCATCG", RC="CGATGCTA".
        // Bad primer "GCTAGCATTG": anchor "AGCATTG", RC="CAATGCT".
        // Template has RC of good anchor → only good primer matches.
        let template = b"CGATGCTANNNA";
        let good_primer = b"GCTAGCATCG";
        let bad_primer = b"GCTAGCATTG";
        let good_sites = find_annealing_positions(good_primer, template, 8, true);
        assert!(!good_sites.is_empty(), "good primer should match");
        let bad_sites = find_annealing_positions(bad_primer, template, 8, true);
        assert!(bad_sites.is_empty(), "3' mismatch should prevent binding");
    }

    #[test]
    fn test_short_primer() {
        let template = b"ATGCATGC";
        let primer = b"ATGC";
        assert!(find_annealing_positions(primer, template, 8, false).is_empty());
    }

    #[test]
    fn test_circular_search() {
        let template = b"GCATGCAT";
        let primer = b"ATGCATGC";
        let sites = find_annealing_circular(primer, template, 6, false);
        assert!(!sites.is_empty());
    }

    #[test]
    fn test_circular_search_dedups_lap_duplicates() {
        // The site at template position 1 is found once per lap on the doubled
        // template (doubled starts 1 and 9) with identical footprint. After
        // mod-normalization both hits map to template_start 1 and must
        // collapse to a single site.
        let template = b"GCTTGCAA";
        let primer = b"CCTTGCAA";
        let sites = find_annealing_circular(primer, template, 6, false);
        assert_eq!(sites.len(), 1, "lap duplicates must be deduped, got {:?}", sites);
        assert_eq!(sites[0].template_start, 1);
    }

    #[test]
    fn test_iupac_primer() {
        let template = b"NNTACGATAGNN";
        let primer = b"CGTACGRTAG"; // R=A|G, 3' anchor = TACGRTAG
        let sites = find_annealing_positions(primer, template, 8, false);
        assert!(!sites.is_empty());
    }

    // ---- 3'-mismatch flag ----

    #[test]
    fn test_3prime_flag_perfect_match_is_false() {
        let template = b"NNNNNCGTACGCTAGNNNNN";
        let primer = b"CGTACGCTAG";
        let sites = find_annealing_positions(primer, template, 8, false);
        assert!(!sites.is_empty());
        assert!(!sites[0].has_3_prime_mismatch);
    }

    #[test]
    fn test_3prime_flag_mismatch_in_anchor() {
        // 'X' is not a valid IUPAC base: the anchor regex falls back to a
        // wildcard so the site still binds, but the pairing at the 3'
        // penultimate base is effectively a mismatch.
        let template = b"NNTACGCTAGNN";
        let primer = b"CGTACGCTXG";
        let sites = find_annealing_positions(primer, template, 8, false);
        assert!(!sites.is_empty());
        assert!(sites[0].has_3_prime_mismatch);
    }

    #[test]
    fn test_3prime_flag_ambiguous_pair_in_anchor() {
        // R (A|G) in the primer pairs with template A — compatible but
        // ambiguous, and it sits 3 bases from the 3' end.
        let template = b"NNTACGATAGNN";
        let primer = b"CGTACGRTAG";
        let sites = find_annealing_positions(primer, template, 8, false);
        assert!(!sites.is_empty());
        assert!(sites[0].has_3_prime_mismatch);
    }

    #[test]
    fn test_3prime_flag_ambiguous_pair_in_anchor_rev() {
        // Rev primer whose 3'-terminal base is R: it pairs with template C
        // (G-C) at the left edge of the anchor footprint.
        let template = b"NNCTAGCGTANN";
        let primer = b"CGTACGCTAR";
        let sites = find_annealing_positions(primer, template, 8, true);
        assert!(!sites.is_empty());
        assert!(sites[0].has_3_prime_mismatch);
    }

    #[test]
    fn test_3prime_flag_ambiguous_only_in_far_5prime_extension_is_false() {
        // Primer 15-mer; the degenerate R is the 5'-terminal base, reached
        // only via extension — 14 bases away from the 3' end. The 3'-most 5
        // bases are all exact, so the flag must stay false.
        let template = b"NNAGTACGCTAGCATGCNN";
        let primer = b"RGTACGCTAGCATGC";
        let sites = find_annealing_positions(primer, template, 13, false);
        assert!(!sites.is_empty());
        assert_eq!(sites[0].footprint_len, 15, "extension should reach the R");
        assert!(!sites[0].has_3_prime_mismatch);
    }
}
