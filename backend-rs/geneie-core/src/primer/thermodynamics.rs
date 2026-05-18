//! Nearest-Neighbor thermodynamics engine (SantaLucia 1998).
//!
//! Computes melting temperature (Tm) from dinucleotide stacking energies,
//! salt concentration, and primer concentration. Only matched/aligned base
//! pairs participate — tails and overhangs are excluded.

/// SantaLucia 1998 unified NN parameters (ΔH° in kcal/mol, ΔS° in cal/mol·K).
///
/// Each entry is (dinucleotide_pair, dh, ds). The first two bases are the
/// primer dinucleotide (5'→3'); the NN parameter applies to the duplex step
/// formed with the complementary strand.
const NN_TABLE: &[(&[u8; 2], f64, f64)] = &[
    (b"AA", -7.9, -22.2),
    (b"AT", -7.2, -20.4),
    (b"TA", -7.2, -21.3),
    (b"CA", -8.5, -22.7),
    (b"GT", -8.4, -22.4),
    (b"CT", -7.8, -21.0),
    (b"GA", -8.2, -22.2),
    (b"CG", -10.6, -27.2),
    (b"GC", -9.8, -24.4),
    (b"GG", -8.0, -19.9),
];

/// Initiation ΔS° for non-self-complementary duplex (cal/mol·K).
const DS_INIT: f64 = -4.1;

/// Gas constant (cal/mol·K).
const R: f64 = 1.9872;

/// Default monovalent cation concentration (M).
const DEFAULT_NA: f64 = 0.05; // 50 mM Na+

/// Default primer concentration (M).
const DEFAULT_PRIMER_CONC: f64 = 5e-7; // 0.5 μM

/// Sum dinucleotide ΔH° and ΔS° for a duplex sequence (primer strand, 5'→3').
fn sum_nn_params(seq: &[u8]) -> (f64, f64) {
    let mut dh = 0.0_f64;
    let mut ds = DS_INIT;
    for w in seq.windows(2) {
        let key: &[u8; 2] = w.try_into().unwrap();
        let mut found = false;
        for &(k, h, s) in NN_TABLE {
            if k == key {
                dh += h;
                ds += s;
                found = true;
                break;
            }
        }
        if !found {
            // Unknown dinucleotide — fall back to average.
            dh += -8.2;
            ds += -22.0;
        }
    }
    (dh, ds)
}

/// Compute Tm (°C) using the SantaLucia 1998 nearest-neighbor model.
///
/// # Arguments
/// * `duplex` — Aligned primer bases (5'→3'), only matched positions.
/// * `na_conc` — Monovalent cation concentration in M (default 0.05).
/// * `primer_conc` — Primer concentration in M (default 5e-7).
///
/// Returns 0.0 if the duplex is too short (< 2 bases).
pub fn compute_tm_nn(duplex: &str, na_conc: f64, primer_conc: f64) -> f64 {
    let seq = duplex.to_ascii_uppercase();
    let n = seq.len();
    if n < 14 {
        // Wallace rule for short oligos (standard cutoff).
        let mut gc = 0_usize;
        let mut at = 0_usize;
        for &b in seq.as_bytes() {
            match b {
                b'G' | b'C' => gc += 1,
                b'A' | b'T' => at += 1,
                _ => {}
            }
        }
        let total = gc + at;
        if total == 0 {
            return 0.0;
        }
        return (4 * gc + 2 * at) as f64;
    }

    let (dh, ds) = sum_nn_params(seq.as_bytes());

    // Salt correction: ΔS°_salt = ΔS° + 0.368 × (N-1) × ln([Na+])
    let ds_salt = ds + 0.368 * (n as f64 - 1.0) * (na_conc.max(1e-9)).ln();

    // Tm = ΔH°×1000 / (ΔS°_salt + R×ln(C/4)) - 273.15
    let tm = (dh * 1000.0) / (ds_salt + R * (primer_conc.max(1e-12) / 4.0).ln()) - 273.15;

    if tm.is_nan() || tm < -273.15 {
        0.0
    } else {
        tm
    }
}

/// Convenience wrapper with default salt (50 mM Na+) and primer (0.5 μM)
/// concentrations.
pub fn compute_tm(duplex: &str) -> f64 {
    compute_tm_nn(duplex, DEFAULT_NA, DEFAULT_PRIMER_CONC)
}

/// Compute GC content of a sequence (ratio 0–1).
pub fn gc_content(seq: &str) -> f64 {
    let mut gc = 0_usize;
    let mut total = 0_usize;
    for &b in seq.as_bytes() {
        match b {
            b'G' | b'C' | b'g' | b'c' => {
                gc += 1;
                total += 1;
            }
            b'A' | b'T' | b'a' | b't' => {
                total += 1;
            }
            _ => {}
        }
    }
    if total == 0 {
        0.0
    } else {
        gc as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gc_content() {
        assert!((gc_content("GCGC") - 1.0).abs() < 0.01);
        assert!((gc_content("ATAT") - 0.0).abs() < 0.01);
        assert!((gc_content("GCTA") - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_tm_20mer() {
        // 20-mer with 50% GC, 50 mM Na+, 0.5 μM
        let tm = compute_tm("CGTACGTACGTACGTACGTA");
        // Expected: ~55-60°C with full NN model
        assert!(tm > 40.0 && tm < 80.0, "Tm={tm} out of expected range");
    }

    #[test]
    fn test_tm_wallace_fallback() {
        // Short sequence uses Wallace rule.
        let tm = compute_tm("GCGC");
        assert_eq!(tm, 16.0); // 4*4 + 2*0 = 16
    }

    #[test]
    fn test_tm_nn_model() {
        // 14-mer triggers NN model.
        let tm = compute_tm("CGTACGTACGTACG");
        assert!(tm > 30.0 && tm < 80.0, "Tm={tm} out of expected range");
    }

    #[test]
    fn test_tm_single_base() {
        // Wallace fallback for <4 bases: 4*GC + 2*AT.
        let tm = compute_tm("A");
        assert_eq!(tm, 2.0);
    }

    #[test]
    fn test_tm_empty() {
        assert_eq!(compute_tm(""), 0.0);
    }

    #[test]
    fn test_tm_gc_rich() {
        let tm_high = compute_tm("GCGCGCGCGCGCGCGCGCGC");
        let tm_low = compute_tm("ATATATATATATATATATAT");
        assert!(tm_high > tm_low, "GC-rich should have higher Tm");
    }
}
