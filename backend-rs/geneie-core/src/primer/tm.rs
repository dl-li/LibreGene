//! Melting temperature computation.
//!
//! Uses the Wallace rule (Tm = 4*(G+C) + 2*(A+T)) for short oligos (<14bp)
//! and a salt-adjusted nearest-neighbour approximation for longer primers.

/// Compute the melting temperature (Tm) for a primer sequence.
///
/// For short sequences (<14bp), uses the Wallace rule:
///   Tm = 4*(G+C) + 2*(A+T)
///
/// For longer sequences (>=14bp), uses the salt-adjusted formula
/// (Howley et al., 1979; SantaLucia, 1998), assuming 50 mM Na⁺:
///   Tm = 59.9 + 41.0 * GC_ratio - 500.0 / N
///
/// Ambiguous bases are ignored.
pub fn compute_tm(seq: &str) -> f64 {
    let mut gc = 0usize;
    let mut at = 0usize;
    for &b in seq.as_bytes() {
        match b {
            b'G' | b'C' | b'g' | b'c' => gc += 1,
            b'A' | b'T' | b'a' | b't' => at += 1,
            _ => {}
        }
    }
    let n = gc + at;
    if n == 0 {
        return 0.0;
    }

    if n < 14 {
        // Wallace rule for short oligos.
        (4 * gc + 2 * at) as f64
    } else {
        // Salt-adjusted: 81.5 + 16.6*log10(0.05) + 0.41*(100*GC_ratio) - 500/N
        // simplifies to 59.9 + 41.0*GC_ratio - 500.0/N.
        let gc_ratio = gc as f64 / n as f64;
        59.9 + 41.0 * gc_ratio - 500.0 / n as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_tm_all_gc() {
        assert_eq!(compute_tm("GGCC"), 16.0);
    }

    #[test]
    fn test_compute_tm_all_at() {
        assert_eq!(compute_tm("AATT"), 8.0);
    }

    #[test]
    fn test_compute_tm_mixed() {
        assert_eq!(compute_tm("CGTACGCTAG"), 32.0);
    }

    #[test]
    fn test_compute_tm_lowercase() {
        assert_eq!(compute_tm("ggcc"), 16.0);
    }

    #[test]
    fn test_compute_tm_empty() {
        assert_eq!(compute_tm(""), 0.0);
    }

    #[test]
    fn test_compute_tm_santalucia() {
        // 20-mer with 50% GC, 50 mM Na⁺:
        // Tm = 59.9 + 41.0*0.5 - 500.0/20 = 59.9 + 20.5 - 25.0 = 55.4°C
        let tm = compute_tm("CGTACGTACGTACGTACGTA");
        assert!((tm - 55.4).abs() < 0.01);
    }

    #[test]
    fn test_compute_tm_long() {
        // 23-mer, 19 GC + 4 AT, ratio=19/23≈0.826:
        // Tm = 59.9 + 41.0*19/23 - 500.0/23 ≈ 59.9 + 33.87 - 21.74 = 72.03°C
        let tm = compute_tm("CGCGCGCGCGCGCGCGAATTCGC");
        assert!((tm - 72.03).abs() < 0.1);
    }
}
