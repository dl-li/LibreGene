//! Smith-Waterman alignment engine with 3'-end asymmetry penalty.
//!
//! Uses k-mer seeding to find candidate regions, then runs position-weighted
//! semi-global (overlap) alignment on each candidate. The 3' end of the primer
//! receives a heavy non-linear mismatch penalty because polymerase extension
//! depends critically on 3' complementarity.

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Scoring constants
// ---------------------------------------------------------------------------

const MATCH_SCORE: i32 = 2;
const GAP_OPEN: i32 = -5;
const GAP_EXTEND: i32 = -2;
const KMER: usize = 6;

// ---------------------------------------------------------------------------
// Position-dependent mismatch penalty
// ---------------------------------------------------------------------------

/// Returns the mismatch penalty at primer position `i` (0-based, 5'→3').
///
/// The penalty is heavily weighted toward the 3' end:
///   - 3' terminal (last base):     -16
///   - 3'-1:                        -12
///   - 3'-2:                         -8
///   - 3'-3:                         -5
///   - 3'-4:                         -3
///   - 5' end (first 20% of primer): -1
///   - Middle:                       -2
fn mismatch_penalty(i: usize, primer_len: usize) -> i32 {
    let from_3prime = primer_len.saturating_sub(i); // 1-based from 3' end
    match from_3prime {
        1 => -16,
        2 => -12,
        3 => -8,
        4 => -5,
        5 => -3,
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

    let mut seeds: HashSet<&[u8]> = HashSet::new();
    for i in 0..=query.len().saturating_sub(k) {
        seeds.insert(&query[i..i + k]);
    }

    let mut hits: Vec<usize> = Vec::new();
    for i in 0..=template.len().saturating_sub(k) {
        if seeds.contains(&template[i..i + k]) {
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
    if start <= end {
        template[start..end].to_vec()
    } else {
        let mut v = Vec::with_capacity((tlen - start) + end);
        v.extend_from_slice(&template[start..]);
        v.extend_from_slice(&template[..end]);
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

            // Match/mismatch.
            let s = if primer[i - 1] == template_region[j - 1] {
                MATCH_SCORE
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
            // Check match/mismatch.
            if dp[i][j] == dp[i - 1][j - 1]
                + if primer[i - 1] == template_region[j - 1] {
                    MATCH_SCORE
                } else {
                    mismatch_penalty(i - 1, n)
                }
            {
                ops.push(AlignedPair {
                    op: if primer[i - 1] == template_region[j - 1] {
                        Op::Match
                    } else {
                        Op::Mismatch
                    },
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
    fn test_mismatch_penalty_3prime() {
        // 20-mer: last position (i=19) is 3' terminal.
        assert_eq!(mismatch_penalty(19, 20), -16);
        assert_eq!(mismatch_penalty(18, 20), -12);
        assert_eq!(mismatch_penalty(17, 20), -8);
        assert_eq!(mismatch_penalty(16, 20), -5);
        assert_eq!(mismatch_penalty(15, 20), -3);
    }

    #[test]
    fn test_mismatch_penalty_5prime() {
        // 5' end (first 20% of 20 = positions 0–3).
        assert_eq!(mismatch_penalty(0, 20), -1);
        assert_eq!(mismatch_penalty(3, 20), -1);
    }

    #[test]
    fn test_mismatch_penalty_middle() {
        // Middle region (positions 4–14 for 20-mer).
        assert_eq!(mismatch_penalty(10, 20), -2);
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
        let primer = b"ATGCATGC";
        let tmpl = b"ATGCTTGC"; // one mismatch at pos 4
        let result = align(primer, tmpl).unwrap();
        assert!(result.ops.iter().any(|p| p.op == Op::Mismatch));
    }

    #[test]
    fn test_align_3prime_mismatch_penalized() {
        // Primer with 3' mismatch should score much lower than internal mismatch.
        let primer = b"AAAAAACCC";
        let good = b"AAAAAACCC"; // exact match
        let bad_3prime = b"AAAAAACCT"; // mismatch at 3' end
        let bad_5prime = b"TAAAAACCC"; // mismatch at 5' end

        let score_exact = align(primer, good).unwrap().score;
        let score_3prime = align(primer, bad_3prime).unwrap().score;
        let score_5prime = align(primer, bad_5prime).unwrap().score;

        assert!(score_3prime < score_5prime,
            "3' mismatch should score lower than 5' mismatch: {score_3prime} vs {score_5prime}");
        assert_eq!(score_exact, 9 * MATCH_SCORE);
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
