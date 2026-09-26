//! Karlin-Altschul statistics (NCBI blast_stat.c / blast_hits.c:1901
//! equivalent). Ported from GenePad (https://genepad.cn, GenePad team).
//! E = searchsp · exp(−λ·S + ln K); bit_score = (S·λ − ln K) / ln 2

#[derive(Clone, Copy, Debug)]
pub struct KarlinBlk {
    pub lambda: f64,
    pub k: f64,
    pub h: f64,
}

impl KarlinBlk {
    pub fn log_k(&self) -> f64 {
        self.k.ln()
    }
}

/// Look up λ/K/H for (reward, penalty, gap_open, gap_extend). Values come
/// from the NCBI nucleotide precomputed tables (approximate); unknown
/// combinations return `None`.
pub fn lookup_karlin(reward: i32, penalty: i32, gap_open: i32, gap_extend: i32) -> Option<KarlinBlk> {
    // NCBI blastn default (1,-3,5,2): λ≈1.33, K≈0.621, H≈1.12;
    // megablast (1,-3,0,0) maps internally to the (2,2)-gap λ/K.
    match (reward, penalty, gap_open, gap_extend) {
        (1, -3, 5, 2) => Some(KarlinBlk { lambda: 1.33, k: 0.621, h: 1.12 }),
        (1, -3, 0, 0) => Some(KarlinBlk { lambda: 1.33, k: 0.621, h: 1.12 }),
        (1, -3, 4, 2) => Some(KarlinBlk { lambda: 1.28, k: 0.460, h: 1.06 }),
        (1, -3, 2, 2) => Some(KarlinBlk { lambda: 1.22, k: 0.342, h: 0.95 }),
        (1, -4, 5, 2) => Some(KarlinBlk { lambda: 1.13, k: 0.378, h: 0.85 }),
        _ => None,
    }
}

/// Effective search space (simplified Karlin length adjustment).
pub fn effective_searchsp(query_len: usize, db_len: usize, kb: &KarlinBlk) -> f64 {
    // length adjustment L ≈ max(0, ln(K·m·n) / H)
    let m = query_len as f64;
    let n = db_len as f64;
    let l = ((kb.k * m * n).ln() / kb.h).max(0.0).floor();
    let eff_m = (m - l).max(1.0);
    let eff_n = (n - l).max(1.0);
    eff_m * eff_n
}

/// Raw score → E-value (BLAST_KarlinStoE_simple).
pub fn score_to_evalue(score: i32, searchsp: f64, kb: &KarlinBlk) -> f64 {
    searchsp * (-kb.lambda * score as f64 + kb.log_k()).exp()
}

/// Raw score → bit score (blast_hits.c:1901).
pub fn score_to_bitscore(score: i32, kb: &KarlinBlk) -> f64 {
    (score as f64 * kb.lambda - kb.log_k()) / std::f64::consts::LN_2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_default_blastn() {
        let kb = lookup_karlin(1, -3, 5, 2).unwrap();
        assert!((kb.lambda - 1.33).abs() < 1e-6);
        assert!((kb.k - 0.621).abs() < 1e-3);
    }

    #[test]
    fn higher_score_lower_evalue() {
        let kb = lookup_karlin(1, -3, 5, 2).unwrap();
        let ss = effective_searchsp(1000, 1000, &kb);
        let e_lo = score_to_evalue(20, ss, &kb);
        let e_hi = score_to_evalue(100, ss, &kb);
        assert!(e_hi < e_lo, "higher score must give lower E");
    }

    #[test]
    fn bitscore_increases_with_score() {
        let kb = lookup_karlin(1, -3, 5, 2).unwrap();
        let b_lo = score_to_bitscore(20, &kb);
        let b_hi = score_to_bitscore(100, &kb);
        assert!(b_hi > b_lo);
    }

    #[test]
    fn unknown_combo_returns_none() {
        assert!(lookup_karlin(5, -5, 9, 9).is_none());
    }
}
