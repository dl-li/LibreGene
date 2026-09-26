//! HSP (High-scoring Segment Pair) container (NCBI BlastHSP/BlastHSPList
//! equivalent). Ported from GenePad (https://genepad.cn, GenePad team).

use super::editblock::{EditBlock, EditOp};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Strand {
    Plus,
    Minus,
}

#[derive(Clone, Debug)]
pub struct BlastHSP {
    pub score: i32,     // raw score
    pub bit_score: f64, // computed in finalize
    pub evalue: f64,    // computed in finalize
    pub q_start: usize, // 0-based, inclusive
    pub q_end: usize,   // 0-based, exclusive
    pub s_start: usize,
    pub s_end: usize,
    pub edit_block: EditBlock,
    pub num_ident: usize,
    pub strand: Strand,
}

impl BlastHSP {
    /// Alignment length including gaps.
    pub fn aligned_len(&self) -> usize {
        self.edit_block
            .ops
            .iter()
            .map(|op| match op {
                EditOp::Sub(n) | EditOp::Ins(n) | EditOp::Del(n) => *n,
            })
            .sum()
    }

    pub fn identity(&self) -> f64 {
        let len = self.aligned_len();
        if len == 0 {
            0.0
        } else {
            self.num_ident as f64 / len as f64
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct HSPList {
    pub hsps: Vec<BlastHSP>,
}

impl HSPList {
    pub fn new() -> Self {
        Self { hsps: Vec::new() }
    }

    /// Add an HSP (Blast_HSPListSaveHSP equivalent).
    pub fn save(&mut self, hsp: BlastHSP) {
        self.hsps.push(hsp);
    }

    /// Dedup: drop HSPs that overlap >50% in BOTH query and subject with a
    /// higher-scoring kept HSP. Simplified NCBI HSP linkage dedup.
    pub fn dedup(&mut self) {
        self.hsps.sort_by(|a, b| b.score.cmp(&a.score));
        let mut kept: Vec<BlastHSP> = Vec::new();
        for hsp in self.hsps.drain(..) {
            let overlaps = kept.iter().any(|k| {
                let q_ol = overlap(hsp.q_start, hsp.q_end, k.q_start, k.q_end);
                let s_ol = overlap(hsp.s_start, hsp.s_end, k.s_start, k.s_end);
                let q_min = (hsp.q_end - hsp.q_start).min(k.q_end - k.q_start);
                let s_min = (hsp.s_end - hsp.s_start).min(k.s_end - k.s_start);
                q_min > 0
                    && s_min > 0
                    && q_ol as f64 / q_min as f64 > 0.5
                    && s_ol as f64 / s_min as f64 > 0.5
            });
            if !overlaps {
                kept.push(hsp);
            }
        }
        self.hsps = kept;
    }

    /// Prune by E-value (Blast_HSPListReapByEvalue equivalent).
    pub fn reap_by_evalue(&mut self, evalue_threshold: f64) {
        self.hsps.retain(|h| h.evalue <= evalue_threshold);
    }

    /// Sort by bit score descending (Blast_HSPListSortByScore equivalent).
    pub fn sort_by_score(&mut self) {
        self.hsps.sort_by(|a, b| {
            b.bit_score
                .partial_cmp(&a.bit_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

fn overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> usize {
    a_end.min(b_end).saturating_sub(a_start.max(b_start))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hsp(score: i32, qs: usize, qe: usize, ss: usize, se: usize) -> BlastHSP {
        BlastHSP {
            score,
            bit_score: score as f64,
            evalue: 1.0 / (score as f64),
            q_start: qs,
            q_end: qe,
            s_start: ss,
            s_end: se,
            edit_block: EditBlock::default(),
            num_ident: 0,
            strand: Strand::Plus,
        }
    }

    #[test]
    fn dedup_keeps_high_score_removes_overlapping() {
        let mut list = HSPList::new();
        list.save(hsp(100, 0, 50, 0, 50));
        list.save(hsp(80, 10, 40, 10, 40)); // fully overlapped, lower score → dropped
        list.dedup();
        assert_eq!(list.hsps.len(), 1);
        assert_eq!(list.hsps[0].score, 100);
    }

    #[test]
    fn dedup_keeps_non_overlapping() {
        let mut list = HSPList::new();
        list.save(hsp(100, 0, 20, 0, 20));
        list.save(hsp(90, 100, 120, 100, 120)); // no overlap → kept
        list.dedup();
        assert_eq!(list.hsps.len(), 2);
    }

    #[test]
    fn reap_by_evalue_filters() {
        let mut list = HSPList::new();
        let mut a = hsp(100, 0, 10, 0, 10);
        a.evalue = 0.5;
        let mut b = hsp(50, 20, 30, 20, 30);
        b.evalue = 20.0;
        list.save(a);
        list.save(b);
        list.reap_by_evalue(10.0);
        assert_eq!(list.hsps.len(), 1);
        assert!((list.hsps[0].evalue - 0.5).abs() < 1e-9);
    }

    #[test]
    fn sort_by_score_desc() {
        let mut list = HSPList::new();
        list.save(hsp(50, 0, 10, 0, 10));
        list.save(hsp(100, 20, 30, 20, 30));
        list.sort_by_score();
        assert!(list.hsps[0].bit_score >= list.hsps[1].bit_score);
    }
}
