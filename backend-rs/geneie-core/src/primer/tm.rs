//! Melting temperature computation.
//!
//! Uses the Wallace rule: Tm = 4*(G+C) + 2*(A+T), suitable for short oligos.

/// Wallace rule: Tm = 4*(G+C) + 2*(A+T).
pub fn compute_tm(seq: &str) -> f64 {
    let gc = seq
        .bytes()
        .filter(|&b| b == b'G' || b == b'C' || b == b'g' || b == b'c')
        .count();
    let at = seq
        .bytes()
        .filter(|&b| b == b'A' || b == b'T' || b == b'a' || b == b't')
        .count();
    (4 * gc + 2 * at) as f64
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
}
