//! Smith-Waterman alignment engine with 3'-end asymmetry penalty.
//!
//! Uses k-mer seeding to find candidate regions, then runs position-weighted
//! semi-global (overlap) alignment on each candidate. The 3' end of the primer
//! receives a heavy non-linear mismatch penalty because polymerase extension
//! depends critically on 3' complementarity.
//!
//! IUPAC ambiguous bases are handled via the [`super::iupac`] module:
//! exact matches score fully, ambiguous matches (e.g. R-Y) score partially,
//! and only non-pairing bases receive the full mismatch penalty.

use std::collections::HashSet;

use super::iupac;

// ---------------------------------------------------------------------------
// Scoring constants
// ---------------------------------------------------------------------------

const MATCH_SCORE: i32 = 2;
const GAP_OPEN: i32 = -15;
const GAP_EXTEND: i32 = -6;
const KMER: usize = 6;

// ---------------------------------------------------------------------------
// Position-dependent mismatch penalty
// ---------------------------------------------------------------------------

/// Returns the mismatch penalty at primer position `i` (0-based, 5'→3').
///
/// The penalty is heavily weighted toward the 3' end.
/// The **entire 3' seed region** (last 13 bases — matching pydna's anchor)
/// receives elevated penalties so that the anchor must be well-conserved:
///   - 3' terminal (last base):     -20
///   - 3'-1:                        -18
///   - 3'-2:                        -15
///   - 3'-3:                        -12
///   - 3'-4:                        -10
///   - 3'-5:                         -8
///   - 3'-6:                         -7
///   - 3'-7 to 3'-9:                 -6
///   - 3'-10 to 3'-13:               -5
///   - 5' end (first 20% of primer): -1
///   - Middle:                       -2
fn mismatch_penalty(i: usize, primer_len: usize) -> i32 {
    let from_3prime = primer_len.saturating_sub(i); // 1-based from 3' end
    match from_3prime {
        1 => -20,
        2 => -18,
        3 => -15,
        4 => -12,
        5 => -10,
        6 => -8,
        7 => -7,
        8..=9 => -6,
        10..=13 => -5,
        _ => {
            // 5' end light penalty (first 20%).
            if (i as f64) < (primer_len as f64) * 0.2 {
                -1
            } else {
                -2
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Alignment operation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Match,
    Mismatch,
    /// Gap in primer (template has base, primer doesn't).
    Del,
    /// Gap in template (primer has base, template doesn't — insertion).
    Ins,
}

/// A single aligned column produced by traceback.
#[derive(Debug, Clone)]
pub struct AlignedPair {
    pub op: Op,
    /// Primer position (0-based, 5'→3'), if primer participates.
    pub primer_pos: Option<usize>,
    /// Template position within the region (0-based).
    pub template_pos: Option<usize>,
}

/// Result of aligning a primer against a template region.
#[derive(Debug, Clone)]
pub struct AlignmentResult {
    /// Sequence of alignment operations.
    pub ops: Vec<AlignedPair>,
    /// Primer start position in the alignment (0-based in primer).
    pub primer_start: usize,
    /// Primer end position (exclusive).
    pub primer_end: usize,
    /// Template start position in the region (0-based in region).
    pub template_start: usize,
    /// Template end position in the region (exclusive).
    pub template_end: usize,
    /// Raw alignment score.
    pub score: i32,
    /// Index of the last aligned base from 3' end (for mismatch detection).
    pub last_3prime_aligned_pos: Option<usize>,
}

// ---------------------------------------------------------------------------
// K-mer seeding
// ---------------------------------------------------------------------------

fn find_kmer_seeds(query: &[u8], template: &[u8], k: usize) -> Vec<usize> {
    if query.len() < k || template.len() < k {
        return Vec::new();
    }

    // Build seed set: for each k-mer in query, expand IUPAC codes into all
    // possible unambiguous DNA sequences so that degenerate primers match
    // all compatible template regions.
    let mut seeds: HashSet<Vec<u8>> = HashSet::new();
    for i in 0..=query.len().saturating_sub(k) {
        let kmer = &query[i..i + k];
        let expanded = iupac::expand_iupac_sequence(kmer);
        for e in expanded {
            seeds.insert(e);
        }
    }

    let mut hits: Vec<usize> = Vec::new();
    for i in 0..=template.len().saturating_sub(k) {
        if seeds.contains(&template[i..i + k].to_vec()) {
            hits.push(i);
        }
    }

    hits.sort();
    hits.dedup();
    hits
}

fn seeds_to_candidates(
    seeds: &[usize],
    tlen: usize,
    plen: usize,
    is_circular: bool,
) -> Vec<(usize, usize)> {
    let padding = plen;
    let mut merged: Vec<(usize, usize)> = Vec::new();

    for &seed in seeds {
        let start = if is_circular {
            (seed + tlen - padding) % tlen
        } else {
            seed.saturating_sub(padding)
        };
        let end = if is_circular {
            (seed + plen + padding) % tlen
        } else {
            (seed + plen + padding).min(tlen)
        };

        if let Some(last) = merged.last_mut() {
            if start <= last.1 + plen {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

fn sliding_window_candidates(
    tlen: usize,
    plen: usize,
    is_circular: bool,
) -> Vec<(usize, usize)> {
    let step = plen.max(1);
    let padding = plen / 2;
    let mut candidates = Vec::new();
    let limit = if is_circular {
        tlen
    } else {
        tlen.saturating_sub(plen)
    };
    let mut pos = 0;
    while pos < limit {
        let start = pos.saturating_sub(padding);
        let end = (pos + plen + padding).min(tlen);
        candidates.push((start, end));
        pos += step;
    }
    if is_circular {
        candidates.push((tlen.saturating_sub(plen + padding), plen + padding));
    }
    candidates
}

pub fn wrap_template_region(template: &[u8], start: usize, end: usize) -> Vec<u8> {
    let tlen = template.len();
    if tlen == 0 {
        return Vec::new();
    }
    let start = start % tlen;
    let end = end % tlen;
    if start < end {
        // Linear region within template bounds.
        template[start..end].to_vec()
    } else if start > end {
        // Region wraps around the origin.
        let mut v = Vec::with_capacity((tlen - start) + end);
        v.extend_from_slice(&template[start..]);
        v.extend_from_slice(&template[..end]);
        v
    } else {
        // start == end after modulo: the region spans a whole number of full
        // turns (e.g. span == tlen) — return the full circle from `start`.
        let mut v = Vec::with_capacity(tlen);
        v.extend_from_slice(&template[start..]);
        v.extend_from_slice(&template[..start]);
        v
    }
}

// ---------------------------------------------------------------------------
// Smith-Waterman alignment with 3' asymmetry
// ---------------------------------------------------------------------------

/// Run position-weighted overlap alignment of `primer` against `template_region`.
///
/// Free end-gaps on both sequences (overlap / semi-global mode). Internal gaps
/// are penalised with affine gap costs. Mismatch penalty depends on the primer
/// position (3' end → heavy, 5' end → light).
pub fn align(primer: &[u8], template_region: &[u8]) -> Option<AlignmentResult> {
    let n = primer.len();
    let m = template_region.len();
    if n == 0 || m == 0 {
        return None;
    }

    // DP tables: score, gap-in-primer, gap-in-template
    let neg_inf = i32::MIN / 2;
    let mut dp = vec![vec![neg_inf; m + 1]; n + 1];
    let mut gp = vec![vec![neg_inf; m + 1]; n + 1]; // gap in primer (Del)
    let mut gt = vec![vec![neg_inf; m + 1]; n + 1]; // gap in template (Ins)

    // Free end-gaps: first row and column = 0.
    for i in 0..=n {
        dp[i][0] = 0;
    }
    for j in 0..=m {
        dp[0][j] = 0;
    }

    for i in 1..=n {
        for j in 1..=m {
            // Gap in primer (template has base, primer has gap).
            gp[i][j] = (dp[i][j - 1] + GAP_OPEN)
                .max(gp[i][j - 1] + GAP_EXTEND);

            // Gap in template (primer has base, template has gap).
            gt[i][j] = (dp[i - 1][j] + GAP_OPEN)
                .max(gt[i - 1][j] + GAP_EXTEND);

            // Match/mismatch — IUPAC-aware: ambiguous overlaps score partially.
            // N in template + specific primer base → weight 0.25 → score 0 (ignored).
            let w = if iupac::bases_overlap(primer[i - 1], template_region[j - 1]) {
                iupac::overlap_weight(primer[i - 1], template_region[j - 1])
            } else {
                0.0
            };
            let s = if w > 0.0 {
                (MATCH_SCORE as f64 * w) as i32
            } else {
                mismatch_penalty(i - 1, n)
            };

            dp[i][j] = (dp[i - 1][j - 1] + s)
                .max(gp[i][j])
                .max(gt[i][j]);
        }
    }

    // Find best end position: max in last row or last column.
    let mut best_i = n;
    let mut best_j = m;
    let mut best_score = dp[n][m];
    for j in 0..=m {
        if dp[n][j] > best_score {
            best_score = dp[n][j];
            best_i = n;
            best_j = j;
        }
    }
    for i in 0..=n {
        if dp[i][m] > best_score {
            best_score = dp[i][m];
            best_i = i;
            best_j = m;
        }
    }

    if best_score <= 0 {
        return None;
    }

    // Traceback.
    let mut ops: Vec<AlignedPair> = Vec::new();
    let (mut i, mut j) = (best_i, best_j);

    while i > 0 || j > 0 {
        if i > 0 && j > 0 {
            // Check match/mismatch — IUPAC-aware direct overlap.
            let bases_ov = iupac::bases_overlap(primer[i - 1], template_region[j - 1]);
            let weight = if bases_ov {
                iupac::overlap_weight(primer[i - 1], template_region[j - 1])
            } else {
                0.0
            };
            let step_score = if weight > 0.0 {
                (MATCH_SCORE as f64 * weight) as i32
            } else {
                mismatch_penalty(i - 1, n)
            };

            if dp[i][j] == dp[i - 1][j - 1] + step_score {
                ops.push(AlignedPair {
                    // Only classify as Match if at least 50% confidence.
                    op: if weight >= 0.5 { Op::Match } else { Op::Mismatch },
                    primer_pos: Some(i - 1),
                    template_pos: Some(j - 1),
                });
                i -= 1;
                j -= 1;
                continue;
            }
        }

        // Check gap in primer (Del).
        if j > 0 && dp[i][j] == gp[i][j] {
            ops.push(AlignedPair {
                op: Op::Del,
                primer_pos: None,
                template_pos: Some(j - 1),
            });
            j -= 1;
            continue;
        }

        // Check gap in template (Ins).
        if i > 0 && dp[i][j] == gt[i][j] {
            ops.push(AlignedPair {
                op: Op::Ins,
                primer_pos: Some(i - 1),
                template_pos: None,
            });
            i -= 1;
            continue;
        }

        // Free end-gap: remaining bases are unaligned.
        break;
    }

    ops.reverse();

    let primer_start = i;
    let template_start = j;
    let primer_end = best_i;
    let template_end = best_j;

    // Find last aligned position from 3' end (for has_3_prime_mismatch).
    let last_3prime = ops.iter().rev().find_map(|p| {
        if p.op == Op::Mismatch || p.op == Op::Del {
            p.primer_pos
        } else {
            None
        }
    });

    let has_3prime_issue = last_3prime
        .map(|pos| n.saturating_sub(pos) <= 5)
        .unwrap_or(false);
    let last_3prime_aligned_pos = if has_3prime_issue {
        last_3prime
    } else {
        None
    };

    Some(AlignmentResult {
        ops,
        primer_start,
        primer_end,
        template_start,
        template_end,
        score: best_score,
        last_3prime_aligned_pos,
    })
}

/// Run alignment with the primer's **3' end constrained** to the template.
///
/// Uses direct sequence matching (A matches A). Appropriate for fwd primers
/// where the primer sequence matches the template top strand directly.
pub fn align_3prime_constrained(
    primer: &[u8],
    template_region: &[u8],
) -> Option<AlignmentResult> {
    align_3prime_constrained_impl(primer, template_region, false)
}

/// Run 3'-constrained alignment using **Watson-Crick complement matching**.
///
/// A matches T, C matches G, etc. Appropriate for rev primers — allows
/// displaying the primer sequence as-is while checking complementarity.
pub fn align_3prime_constrained_rev(
    primer: &[u8],
    template_region: &[u8],
) -> Option<AlignmentResult> {
    align_3prime_constrained_impl(primer, template_region, true)
}

fn align_3prime_constrained_impl(
    primer: &[u8],
    template_region: &[u8],
    use_complement: bool,
) -> Option<AlignmentResult> {
    let n = primer.len();
    let m = template_region.len();
    if n == 0 || m == 0 {
        return None;
    }

    // DP tables: score, gap-in-primer, gap-in-template
    let neg_inf = i32::MIN / 2;
    let mut dp = vec![vec![neg_inf; m + 1]; n + 1];
    let mut gp = vec![vec![neg_inf; m + 1]; n + 1];
    let mut gt = vec![vec![neg_inf; m + 1]; n + 1];

    // Free end-gaps on the LEFT side (primer 5' end, template start).
    for i in 0..=n {
        dp[i][0] = 0;
    }
    for j in 0..=m {
        dp[0][j] = 0;
    }

    // Choose overlap function based on mode.
    let overlap_fn = |a: u8, b: u8| {
        if use_complement {
            iupac::pair_fraction(a, b)
        } else {
            if iupac::bases_overlap(a, b) {
                iupac::overlap_weight(a, b)
            } else {
                0.0
            }
        }
    };

    let bases_fn = |a: u8, b: u8| {
        if use_complement {
            iupac::bases_pair(a, b)
        } else {
            iupac::bases_overlap(a, b)
        }
    };

    for i in 1..=n {
        for j in 1..=m {
            gp[i][j] = (dp[i][j - 1] + GAP_OPEN)
                .max(gp[i][j - 1] + GAP_EXTEND);

            gt[i][j] = (dp[i - 1][j] + GAP_OPEN)
                .max(gt[i - 1][j] + GAP_EXTEND);

            let w = overlap_fn(primer[i - 1], template_region[j - 1]);
            let s = if w > 0.0 {
                (MATCH_SCORE as f64 * w) as i32
            } else {
                mismatch_penalty(i - 1, n)
            };

            dp[i][j] = (dp[i - 1][j - 1] + s)
                .max(gp[i][j])
                .max(gt[i][j]);
        }
    }

    // 3' CONSTRAINT: only search the LAST ROW (primer's 3' terminal must
    // participate). We do NOT consider dp[i][m] (free primer end-gap).
    let mut best_j = m;
    let mut best_score = dp[n][m];
    for j in 0..=m {
        if dp[n][j] > best_score {
            best_score = dp[n][j];
            best_j = j;
        }
    }

    if best_score <= 0 {
        return None;
    }

    // Traceback.
    let mut ops: Vec<AlignedPair> = Vec::new();
    let (mut i, mut j) = (n, best_j);

    while i > 0 || j > 0 {
        if i > 0 && j > 0 {
            let bases_ov = bases_fn(primer[i - 1], template_region[j - 1]);
            let weight = if bases_ov {
                overlap_fn(primer[i - 1], template_region[j - 1])
            } else {
                0.0
            };
            let step_score = if weight > 0.0 {
                (MATCH_SCORE as f64 * weight) as i32
            } else {
                mismatch_penalty(i - 1, n)
            };

            if dp[i][j] == dp[i - 1][j - 1] + step_score {
                ops.push(AlignedPair {
                    op: if weight >= 0.5 { Op::Match } else { Op::Mismatch },
                    primer_pos: Some(i - 1),
                    template_pos: Some(j - 1),
                });
                i -= 1;
                j -= 1;
                continue;
            }
        }

        if j > 0 && dp[i][j] == gp[i][j] {
            ops.push(AlignedPair {
                op: Op::Del,
                primer_pos: None,
                template_pos: Some(j - 1),
            });
            j -= 1;
            continue;
        }

        if i > 0 && dp[i][j] == gt[i][j] {
            ops.push(AlignedPair {
                op: Op::Ins,
                primer_pos: Some(i - 1),
                template_pos: None,
            });
            i -= 1;
            continue;
        }

        break;
    }

    ops.reverse();

    let primer_start = i;
    let template_start = j;
    let primer_end = n;
    let template_end = best_j;

    let last_3prime = ops.iter().rev().find_map(|p| {
        if p.op == Op::Mismatch || p.op == Op::Del {
            p.primer_pos
        } else {
            None
        }
    });

    let has_3prime_issue = last_3prime
        .map(|pos| n.saturating_sub(pos) <= 5)
        .unwrap_or(false);
    let last_3prime_aligned_pos = if has_3prime_issue {
        last_3prime
    } else {
        None
    };

    Some(AlignmentResult {
        ops,
        primer_start,
        primer_end,
        template_start,
        template_end,
        score: best_score,
        last_3prime_aligned_pos,
    })
}

/// Run alignment with the primer's **5' end (first base) constrained**.
///
/// The first base of the query MUST participate in the alignment.  The last
/// base (and the template right end) are free.  Uses direct sequence matching.
///
/// This is appropriate for reverse-mode probe alignment where the 3' end of
/// the original primer sits at the query's first position after reversal.
pub fn align_first_base_constrained(
    query: &[u8],
    template_region: &[u8],
) -> Option<AlignmentResult> {
    align_first_base_constrained_impl(query, template_region, false)
}

/// Run first-base-constrained alignment with complement matching.
///
/// A matches T, C matches G, etc.  The first base of the query is constrained.
pub fn align_first_base_constrained_rev(
    query: &[u8],
    template_region: &[u8],
) -> Option<AlignmentResult> {
    align_first_base_constrained_impl(query, template_region, true)
}

fn align_first_base_constrained_impl(
    query: &[u8],
    template_region: &[u8],
    use_complement: bool,
) -> Option<AlignmentResult> {
    let n = query.len();
    let m = template_region.len();
    if n == 0 || m == 0 {
        return None;
    }

    let neg_inf = i32::MIN / 2;
    let mut dp = vec![vec![neg_inf; m + 1]; n + 1];
    let mut gp = vec![vec![neg_inf; m + 1]; n + 1];
    let mut gt = vec![vec![neg_inf; m + 1]; n + 1];

    // FIRST BASE constrained: no free end-gaps at the query front.
    dp[0][0] = 0;
    for i in 1..=n {
        dp[i][0] = neg_inf;
    }
    // Template start is always free.
    for j in 0..=m {
        dp[0][j] = 0;
    }

    let overlap_fn = |a: u8, b: u8| {
        if use_complement {
            iupac::pair_fraction(a, b)
        } else {
            if iupac::bases_overlap(a, b) {
                iupac::overlap_weight(a, b)
            } else {
                0.0
            }
        }
    };

    let bases_fn = |a: u8, b: u8| {
        if use_complement {
            iupac::bases_pair(a, b)
        } else {
            iupac::bases_overlap(a, b)
        }
    };

    for i in 1..=n {
        for j in 1..=m {
            gp[i][j] = (dp[i][j - 1] + GAP_OPEN)
                .max(gp[i][j - 1] + GAP_EXTEND);
            gt[i][j] = (dp[i - 1][j] + GAP_OPEN)
                .max(gt[i - 1][j] + GAP_EXTEND);

            let w = overlap_fn(query[i - 1], template_region[j - 1]);
            let s = if w > 0.0 {
                (MATCH_SCORE as f64 * w) as i32
            } else {
                mismatch_penalty(i - 1, n)
            };

            dp[i][j] = (dp[i - 1][j - 1] + s)
                .max(gp[i][j])
                .max(gt[i][j]);
        }
    }

    // Find best score: query RIGHT end is free (5' side), so only dp[n][*].
    let mut best_j = m;
    let mut best_score = dp[n][m];
    for j in 0..=m {
        if dp[n][j] > best_score {
            best_score = dp[n][j];
            best_j = j;
        }
    }

    if best_score <= 0 {
        return None;
    }

    // Traceback.
    let mut ops: Vec<AlignedPair> = Vec::new();
    let (mut i, mut j) = (n, best_j);

    while i > 0 || j > 0 {
        if i > 0 && j > 0 {
            let bases_ov = bases_fn(query[i - 1], template_region[j - 1]);
            let weight = if bases_ov {
                overlap_fn(query[i - 1], template_region[j - 1])
            } else {
                0.0
            };
            let step_score = if weight > 0.0 {
                (MATCH_SCORE as f64 * weight) as i32
            } else {
                mismatch_penalty(i - 1, n)
            };

            if dp[i][j] == dp[i - 1][j - 1] + step_score {
                ops.push(AlignedPair {
                    op: if weight >= 0.5 { Op::Match } else { Op::Mismatch },
                    primer_pos: Some(i - 1),
                    template_pos: Some(j - 1),
                });
                i -= 1;
                j -= 1;
                continue;
            }
        }

        if j > 0 && dp[i][j] == gp[i][j] {
            ops.push(AlignedPair {
                op: Op::Del,
                primer_pos: None,
                template_pos: Some(j - 1),
            });
            j -= 1;
            continue;
        }

        if i > 0 && dp[i][j] == gt[i][j] {
            ops.push(AlignedPair {
                op: Op::Ins,
                primer_pos: Some(i - 1),
                template_pos: None,
            });
            i -= 1;
            continue;
        }

        break;
    }

    ops.reverse();

    let primer_start = i;
    let template_start = j;
    let primer_end = n;
    let template_end = best_j;

    let last_3prime = ops.iter().rev().find_map(|p| {
        if p.op == Op::Mismatch || p.op == Op::Del {
            p.primer_pos
        } else {
            None
        }
    });

    let has_3prime_issue = last_3prime
        .map(|pos| n.saturating_sub(pos) <= 5)
        .unwrap_or(false);
    let last_3prime_aligned_pos = if has_3prime_issue {
        last_3prime
    } else {
        None
    };

    Some(AlignmentResult {
        ops,
        primer_start,
        primer_end,
        template_start,
        template_end,
        score: best_score,
        last_3prime_aligned_pos,
    })
}

// ---------------------------------------------------------------------------
// Candidate generation (public for use by align.rs)
// ---------------------------------------------------------------------------

/// Generate candidate regions for a primer against a template.
pub fn generate_candidates(
    query: &[u8],
    template: &[u8],
    plen: usize,
    is_circular: bool,
) -> Vec<(usize, usize)> {
    let tlen = template.len();
    let seeds = find_kmer_seeds(query, template, KMER);
    if seeds.is_empty() {
        sliding_window_candidates(tlen, plen, is_circular)
    } else {
        seeds_to_candidates(&seeds, tlen, plen, is_circular)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_template_region_full_circle() {
        let tpl = b"ACGTAC";
        // span == tlen starting at 2: full circle from position 2.
        assert_eq!(wrap_template_region(tpl, 2, 8), b"GTACAC");
        // start == 0, end == tlen: whole template.
        assert_eq!(wrap_template_region(tpl, 0, 6), b"ACGTAC");
    }

    #[test]
    fn test_mismatch_penalty_3prime() {
        // 20-mer: last position (i=19) is 3' terminal.
        assert_eq!(mismatch_penalty(19, 20), -20);
        assert_eq!(mismatch_penalty(18, 20), -18);
        assert_eq!(mismatch_penalty(17, 20), -15);
        assert_eq!(mismatch_penalty(16, 20), -12);
        assert_eq!(mismatch_penalty(15, 20), -10);
        // Seed region: positions 7-13 from 3' end (i=8..13)
        assert_eq!(mismatch_penalty(14, 20), -8);
        assert_eq!(mismatch_penalty(13, 20), -7);
        // from_3prime=9 => 8..=9 => -6
        assert_eq!(mismatch_penalty(11, 20), -6);
        // from_3prime=10..=13 => -5
        assert_eq!(mismatch_penalty(10, 20), -5);
        assert_eq!(mismatch_penalty(7, 20), -5);
    }

    #[test]
    fn test_mismatch_penalty_5prime() {
        // 5' end (first 20% of 20 = positions 0–3).
        assert_eq!(mismatch_penalty(0, 20), -1);
        assert_eq!(mismatch_penalty(3, 20), -1);
    }

    #[test]
    fn test_mismatch_penalty_middle() {
        // Outside seed region (from_3prime > 13) and not in first 20%.
        // 20-mer: i=4 => from_3prime=16 > 13, and 4 < 20*0.2=false => middle→-2
        assert_eq!(mismatch_penalty(4, 20), -2);
    }

    #[test]
    fn test_align_exact_match() {
        let primer = b"ATGCATGC";
        let tmpl = b"NNATGCATGCNN";
        let result = align(primer, tmpl).unwrap();
        assert!(result.score > 0);
        assert!(result.ops.iter().all(|p| p.op == Op::Match || p.op == Op::Ins || p.op == Op::Del));
        let matches = result.ops.iter().filter(|p| p.op == Op::Match).count();
        assert_eq!(matches, 8);
    }

    #[test]
    fn test_align_with_mismatches() {
        // Use a longer template so the alignment can't simply shift past the mismatch.
        // Primer "ATGCATGC" has A at pos 4, template has T at the corresponding position.
        let primer = b"ATGCATGC";
        let tmpl = b"NNATGCTTGCAA"; // one mismatch at primer pos 4 (A vs T)
        let result = align(primer, tmpl).unwrap();
        assert!(result.ops.iter().any(|p| p.op == Op::Mismatch),
            "expected at least one mismatch but alignment avoided it entirely");
    }

    #[test]
    fn test_align_3prime_mismatch_penalized() {
        // The semi-global alignment allows end-gaps at both ends, so single-end
        // mismatches can be avoided by shifting. Instead we verify:
        //   1. Exact match has the correct score.
        //   2. A mismatch in the 3' seed region forces a lower score than the
        //      same mismatch further 5' when embedded deep enough to be forced.
        //
        // Force both mismatches by making the primer's 5' half and 3' half
        // flanking sequences that can't shift past.
        let primer = b"AAAAAAAACCCCCCCC"; // 16-mer: 8A + 8C
        // Mismatch at 3' seed: change a C to T
        let forced_3prime = b"AAAAAAAAACCCCCCT"; // last C→T at primer pos 15
        // Mismatch at 5' outside seed: change an A to T
        let forced_5prime = b"ATAAAAAACCCCCCCC"; // second A→T at primer pos 1
        // The forced_5prime template must be unambiguously alignable at full length

        // Accept = score_3prime < score_5prime < score_exact
        let score_exact = align(primer, primer).unwrap().score;
        let score_3prime = align(primer, forced_3prime).unwrap().score;
        let score_5prime = align(primer, forced_5prime).unwrap().score;

        // Both mismatches should reduce the score
        assert!(score_3prime < score_exact, "3' mismatch should reduce score");
        assert!(score_5prime < score_exact, "5' mismatch should reduce score");
        // 3' penalty (-20 at terminal) >> middle penalty (-2) so 3' wins
        assert!(score_3prime < score_5prime,
            "3' mismatch (score={}) should be penalized more than 5' mismatch (score={})",
            score_3prime, score_5prime);
    }

    #[test]
    fn test_align_empty() {
        assert!(align(b"", b"ATGC").is_none());
        assert!(align(b"ATGC", b"").is_none());
    }

    #[test]
    fn test_kmer_seeds() {
        let seeds = find_kmer_seeds(b"ATGCAT", b"NNATGCATNN", 6);
        assert_eq!(seeds, vec![2]);
    }
}
