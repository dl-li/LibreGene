//! Semi-global (overlap) alignment engine for the primer display dialog.
//!
//! Display-only: binding decisions live in [`super::matcher`]. Scoring mirrors
//! the read aligner's full-length path (Gotoh with blastn-flavoured ratios):
//! match +2, mismatch −4, affine gaps (open −5, first base; extend −1). A
//! 1bp gap (−6) always loses to a mismatch (−4), so SNPs render as mismatch
//! columns; longer indels bridge as ONE contiguous gap run, and each extra
//! gap base (−1) is cheaper than re-aligning random junk (expected ≈ −2.5 per
//! base), so chance matches (+2) cannot fund frameshifted mismatch drift.
//! The traceback walks the DP as a state machine, so gap runs rebuild
//! contiguously instead of shattering around coincidental ties.
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
const MISMATCH: i32 = -4;
const GAP_OPEN: i32 = -5;
const GAP_EXTEND: i32 = -1;
const KMER: usize = 6;

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
        // Seeds are expanded uppercase; fold the template window so a
        // lowercase (case-preserving) template still seeds.
        if seeds.contains(&template[i..i + k].to_ascii_uppercase()) {
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
// Alignment core
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum OverlapMode {
    /// Primer sequence matches the template top strand directly (fwd).
    Direct,
    /// Primer base pairs with the template base (rev).
    Complement,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EndScan {
    /// Free end-gaps on both sequences: best of last row and last column.
    LastRowAndCol,
    /// 3' end constrained: best of the last row only.
    LastRow,
    /// First base constrained: best of any cell.
    Any,
}

/// Weight of pairing `a` against `b` (0.0 = no overlap).
fn overlap_weight(a: u8, b: u8, mode: OverlapMode) -> f64 {
    match mode {
        OverlapMode::Direct => {
            if iupac::bases_overlap(a, b) {
                iupac::overlap_weight(a, b)
            } else {
                0.0
            }
        }
        OverlapMode::Complement => iupac::pair_fraction(a, b),
    }
}

/// Whether `a` and `b` pair at all (drives match vs mismatch classification).
fn bases_pair(a: u8, b: u8, mode: OverlapMode) -> bool {
    match mode {
        OverlapMode::Direct => iupac::bases_overlap(a, b),
        OverlapMode::Complement => iupac::bases_pair(a, b),
    }
}

/// Score of a diagonal step, and whether it classifies as a match (≥ 50%).
fn step_score(a: u8, b: u8, mode: OverlapMode) -> (i32, bool) {
    let pairs = bases_pair(a, b, mode);
    let w = if pairs { overlap_weight(a, b, mode) } else { 0.0 };
    if w > 0.0 {
        ((MATCH_SCORE as f64 * w) as i32, w >= 0.5)
    } else {
        (MISMATCH, false)
    }
}

/// Gotoh semi-global overlap alignment of `query` against `template_region`.
///
/// First template row and (optionally) first query column are free end-gaps.
/// The end scan picks the terminal cell per `scan`; the traceback walks the
/// DP as a state machine — once inside a gap state it stays there while the
/// extension predecessor holds, so a gap run rebuilds contiguously instead of
/// breaking on coincidental diagonal ties.
fn run_align(
    query: &[u8],
    template_region: &[u8],
    mode: OverlapMode,
    free_left_query_col: bool,
    scan: EndScan,
) -> Option<AlignmentResult> {
    let n = query.len();
    let m = template_region.len();
    if n == 0 || m == 0 {
        return None;
    }

    let neg_inf = i32::MIN / 2;
    let mut dp = vec![vec![neg_inf; m + 1]; n + 1];
    let mut gp = vec![vec![neg_inf; m + 1]; n + 1]; // gap in query (Del)
    let mut gt = vec![vec![neg_inf; m + 1]; n + 1]; // gap in template (Ins)

    // Template left end is always free.
    for j in 0..=m {
        dp[0][j] = 0;
    }
    for i in 0..=n {
        dp[i][0] = if free_left_query_col { 0 } else { neg_inf };
    }

    for i in 1..=n {
        for j in 1..=m {
            gp[i][j] = (dp[i][j - 1] + GAP_OPEN)
                .max(gp[i][j - 1] + GAP_EXTEND);
            gt[i][j] = (dp[i - 1][j] + GAP_OPEN)
                .max(gt[i - 1][j] + GAP_EXTEND);

            let (s, _) = step_score(query[i - 1], template_region[j - 1], mode);
            dp[i][j] = (dp[i - 1][j - 1] + s)
                .max(gp[i][j])
                .max(gt[i][j]);
        }
    }

    // Terminal cell: max over the allowed scan region.
    let mut best_i = n;
    let mut best_j = m;
    let mut best_score = dp[n][m];
    match scan {
        EndScan::LastRowAndCol => {
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
        }
        EndScan::LastRow => {
            for j in 0..=m {
                if dp[n][j] > best_score {
                    best_score = dp[n][j];
                    best_j = j;
                }
            }
        }
        EndScan::Any => {
            for i in 1..=n {
                for j in 1..=m {
                    if dp[i][j] > best_score {
                        best_score = dp[i][j];
                        best_i = i;
                        best_j = j;
                    }
                }
            }
        }
    }

    if best_score <= 0 {
        return None;
    }

    // ---- Traceback as a state machine ----
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum St {
        Diag,
        GapQuery,   // consuming template bases (Del)
        GapTempl,   // consuming query bases (Ins)
    }

    // Winning state of dp[i][j], same priority as the fill (diag > gp > gt).
    // Gap states unreachable from this cell never win.
    let winner = |dp: &Vec<Vec<i32>>, gp: &Vec<Vec<i32>>, gt: &Vec<Vec<i32>>,
                  i: usize, j: usize| -> Option<St> {
        if i == 0 || j == 0 {
            return None;
        }
        let (s, _) = step_score(query[i - 1], template_region[j - 1], mode);
        let diag = dp[i - 1][j - 1] + s;
        if dp[i][j] == diag {
            Some(St::Diag)
        } else if gp[i][j] != neg_inf && dp[i][j] == gp[i][j] {
            Some(St::GapQuery)
        } else if gt[i][j] != neg_inf && dp[i][j] == gt[i][j] {
            Some(St::GapTempl)
        } else {
            None
        }
    };

    let mut ops: Vec<AlignedPair> = Vec::new();
    let (mut i, mut j) = (best_i, best_j);
    let mut state = winner(&dp, &gp, &gt, i, j);

    while i > 0 && j > 0 {
        match state {
            Some(St::Diag) => {
                let (_, is_match) = step_score(query[i - 1], template_region[j - 1], mode);
                ops.push(AlignedPair {
                    op: if is_match { Op::Match } else { Op::Mismatch },
                    primer_pos: Some(i - 1),
                    template_pos: Some(j - 1),
                });
                i -= 1;
                j -= 1;
                state = winner(&dp, &gp, &gt, i, j);
            }
            Some(St::GapQuery) => {
                ops.push(AlignedPair {
                    op: Op::Del,
                    primer_pos: None,
                    template_pos: Some(j - 1),
                });
                j -= 1;
                // Stay in the gap state while the extension chain holds; only
                // re-derive the state at the cell that opened the gap.
                if gp[i][j] == neg_inf || gp[i][j + 1] != gp[i][j] + GAP_EXTEND {
                    state = winner(&dp, &gp, &gt, i, j);
                }
            }
            Some(St::GapTempl) => {
                ops.push(AlignedPair {
                    op: Op::Ins,
                    primer_pos: Some(i - 1),
                    template_pos: None,
                });
                i -= 1;
                if gt[i][j] == neg_inf || gt[i + 1][j] != gt[i][j] + GAP_EXTEND {
                    state = winner(&dp, &gp, &gt, i, j);
                }
            }
            None => break,
        }
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

/// Run overlap alignment with free end-gaps on both sequences.
pub fn align(primer: &[u8], template_region: &[u8]) -> Option<AlignmentResult> {
    run_align(
        primer,
        template_region,
        OverlapMode::Direct,
        true,
        EndScan::LastRowAndCol,
    )
}

/// Run alignment with the primer's **3' end constrained** to the template.
///
/// Uses direct sequence matching (A matches A). Appropriate for fwd primers
/// where the primer sequence matches the template top strand directly.
pub fn align_3prime_constrained(
    primer: &[u8],
    template_region: &[u8],
) -> Option<AlignmentResult> {
    run_align(
        primer,
        template_region,
        OverlapMode::Direct,
        true,
        EndScan::LastRow,
    )
}

/// Run 3'-constrained alignment using **Watson-Crick complement matching**.
///
/// A matches T, C matches G, etc. Appropriate for rev primers — allows
/// displaying the primer sequence as-is while checking complementarity.
pub fn align_3prime_constrained_rev(
    primer: &[u8],
    template_region: &[u8],
) -> Option<AlignmentResult> {
    run_align(
        primer,
        template_region,
        OverlapMode::Complement,
        true,
        EndScan::LastRow,
    )
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
    run_align(
        query,
        template_region,
        OverlapMode::Direct,
        false,
        EndScan::Any,
    )
}

/// Run first-base-constrained alignment with complement matching.
///
/// A matches T, C matches G, etc.  The first base of the query is constrained.
pub fn align_first_base_constrained_rev(
    query: &[u8],
    template_region: &[u8],
) -> Option<AlignmentResult> {
    run_align(
        query,
        template_region,
        OverlapMode::Complement,
        false,
        EndScan::Any,
    )
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
    fn test_align_snp_never_renders_as_gap() {
        // Single-base differences at any position (including near the 3' end)
        // must stay mismatch columns: a 1bp gap (-7) always loses to a
        // mismatch (-3), so no Ins/Del ops appear.
        let primer = b"AAAAAAAACCCCCCCCGGGGGGGG";
        for pos in [0, 5, 11, 16, 22] {
            let mut tpl = primer.to_vec();
            tpl[pos] = match tpl[pos] {
                b'A' => b'T',
                b'C' => b'A',
                b'G' => b'T',
                _ => b'A',
            };
            let result = align_3prime_constrained(primer, &tpl)
                .unwrap_or_else(|| panic!("SNP at {pos} must align"));
            assert!(
                result.ops.iter().all(|p| p.op == Op::Match || p.op == Op::Mismatch),
                "SNP at {pos} produced a gap op: {:?}",
                result.ops.iter().map(|p| p.op).collect::<Vec<_>>()
            );
            let mismatches = result.ops.iter().filter(|p| p.op == Op::Mismatch).count();
            assert_eq!(mismatches, 1, "SNP at {pos}");
            assert_eq!(result.template_end - result.template_start, primer.len());
        }
    }

    #[test]
    fn test_align_snp_does_not_shift_past_end() {
        // A 3' terminal SNP once rode the free left end into a shifted
        // alignment; the constrained 3' end must keep the full footprint.
        let primer = b"AAAAAAAACCCCCCCC";
        let mut tpl = primer.to_vec();
        tpl[15] = b'T';
        let result = align_3prime_constrained(primer, &tpl).unwrap();
        assert_eq!(result.template_end - result.template_start, primer.len());
        assert!(result.ops.iter().any(|p| p.op == Op::Mismatch));
    }

    #[test]
    fn test_indel_bridges_as_one_contiguous_gap_run() {
        // A 12bp template insertion (gap in primer) in the middle of the
        // binding site: exactly one Del run, no mismatch drift.
        let primer = b"ATGCGGCCGATCGTACGATCGGATCCGACT";
        let mut tpl = b"GGGGGGGGGG".to_vec();
        tpl.extend_from_slice(&primer[..14]);
        tpl.extend_from_slice(b"AAGCTTGGCCTA");
        tpl.extend_from_slice(&primer[14..]);
        tpl.extend_from_slice(b"GGGGGGGGGG");
        let result = align_3prime_constrained(primer, &tpl).unwrap();

        let mut runs: Vec<(Op, usize)> = Vec::new();
        for op in &result.ops {
            if let Some(last) = runs.last_mut() {
                if last.0 == op.op {
                    last.1 += 1;
                    continue;
                }
            }
            runs.push((op.op, 1));
        }
        assert_eq!(
            runs,
            vec![(Op::Match, 14), (Op::Del, 12), (Op::Match, primer.len() - 14)],
            "one contiguous 12bp gap run expected"
        );
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

    #[test]
    fn test_first_base_constrained_free_right_tail() {
        // Reverse-primer style: query = reversed primer, first base (original
        // 3' end) constrained. The 5' tail "GAGCTCGCC" does not complement the
        // template past the binding site and must be left unaligned instead of
        // sinking the whole alignment below zero.
        let query = b"CCTCGTTAGTGTCCACTCGTTTTTTGAGCTCGCC";
        let template = b"NNNNGGAGCAATCACAGGTGAGCAAAAAAGCCACCATGGNNNN";
        let result = align_first_base_constrained_rev(query, template)
            .expect("tail overhang must not fail the alignment");
        assert_eq!(result.primer_start, 0, "first base constrained");
        assert!(result.primer_end < query.len(), "non-matching tail excluded");
        let matches = result.ops.iter().filter(|p| p.op == Op::Match).count();
        assert_eq!(matches, 25);
    }
}
