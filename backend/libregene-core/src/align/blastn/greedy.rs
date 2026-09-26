//! megablast greedy alignment (NCBI greedy_align.c:BLAST_AffineGreedyAlign
//! equivalent). Ported from GenePad (https://genepad.cn, GenePad team).
//!
//! NCBI's greedy algorithm exploits gap_open=0/gap_extend=0 for run-anchored
//! alignment. This port delegates to the X-drop DP with zero gap costs,
//! which is optimal for the same parameters (correctness over raw speed).

use super::editblock::EditBlock;
use super::gapped;

pub fn greedy_extend(
    query: &[u8],
    subject: &[u8],
    q_start: usize,
    s_start: usize,
    reward: i32,
    penalty: i32,
    x_drop: i32,
) -> Option<(i32, usize, usize, EditBlock)> {
    // megablast: gap_open=0, gap_extend=0 (no penalty)
    gapped::gapped_extend(query, subject, q_start, s_start, reward, penalty, 0, 0, x_drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::blastn::editblock::EditOp;

    #[test]
    fn exact_match_matches_gapped() {
        let q = b"ACGTACGTACGTACGT";
        let s = b"ACGTACGTACGTACGT";
        let (score, _, _, block) = greedy_extend(q, s, 0, 0, 1, -3, 25).unwrap();
        assert_eq!(score, 16);
        assert_eq!(block.ops, vec![EditOp::Sub(16)]);
    }
}
