//! Nearest-Neighbour thermodynamics engine (SantaLucia 2004).
//!
//! Computes melting temperature (Tm) from dinucleotide stacking energies,
//! salt concentration, Mg²⁺/dNTPs correction, and primer concentration.
//! Only matched/aligned base pairs participate — tails and overhangs are excluded.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

use super::iupac;

// ---------------------------------------------------------------------------
// SantaLucia & Hicks (2004) unified NN parameters
// ΔH° in kcal/mol, ΔS° in cal/mol·K
// All 16 dinucleotide pairs explicitly tabulated.
// ---------------------------------------------------------------------------

const NN_TABLE: &[(&[u8; 2], f64, f64)] = &[
    (b"AA", -7.9, -22.2),
    (b"AC", -8.4, -22.4),
    (b"AG", -7.8, -21.0),
    (b"AT", -7.2, -20.4),
    (b"CA", -8.5, -22.7),
    (b"CC", -8.0, -19.9),
    (b"CG", -10.6, -27.2),
    (b"CT", -7.8, -21.0),
    (b"GA", -8.2, -22.2),
    (b"GC", -9.8, -24.4),
    (b"GG", -8.0, -19.9),
    (b"GT", -8.4, -22.4),
    (b"TA", -7.2, -21.3),
    (b"TC", -8.2, -22.2),
    (b"TG", -8.5, -22.7),
    (b"TT", -7.9, -22.2),
];

/// Initiation ΔS° for non-self-complementary duplex (cal/mol·K).
const DS_INIT: f64 = -4.1;

/// Gas constant (cal/mol·K).
const R: f64 = 1.9872;

// ---------------------------------------------------------------------------
// Default PCR conditions (standard 1× Taq buffer)
// 50 mM KCl, 10 mM Tris-HCl (pH 8.3), 1.5 mM MgCl₂,
// 0.2 mM each dNTP, 0.2 μM each primer.
// ---------------------------------------------------------------------------

const DEFAULT_NA: f64 = 0.050;   // 50 mM monovalent (K⁺ from KCl)
const DEFAULT_MG: f64 = 0.0015;  // 1.5 mM Mg²⁺
const DEFAULT_DNTP: f64 = 0.0008; // 0.8 mM total dNTPs
const DEFAULT_TRIS: f64 = 0.010;  // 10 mM Tris-HCl
const DEFAULT_PRIMER_CONC: f64 = 2e-7; // 0.2 μM each primer

// ---------------------------------------------------------------------------
// TmParams — configurable PCR conditions
// ---------------------------------------------------------------------------

/// Configurable parameters for Tm calculation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmParams {
    /// Monovalent cation concentration in M (K⁺ + Na⁺). Default 0.050.
    pub na_conc: f64,
    /// Mg²⁺ concentration in M. Default 0.0015.
    pub mg_conc: f64,
    /// dNTP concentration in M (total). Default 0.0008.
    pub dntp_conc: f64,
    /// Tris-HCl concentration in M. Default 0.010.
    pub tris_conc: f64,
    /// Primer concentration in M. Default 2e-7.
    pub primer_conc: f64,
}

impl Default for TmParams {
    fn default() -> Self {
        Self {
            na_conc: DEFAULT_NA,
            mg_conc: DEFAULT_MG,
            dntp_conc: DEFAULT_DNTP,
            tris_conc: DEFAULT_TRIS,
            primer_conc: DEFAULT_PRIMER_CONC,
        }
    }
}

impl TmParams {
    /// Standard Taq buffer (as used by pydna tm_default).
    pub fn taq() -> Self {
        Self::default()
    }

    /// DNA-binding domain polymerase buffer (Phusion/Pfu-Sso7d).
    pub fn dbd() -> Self {
        Self {
            na_conc: 0.050,
            mg_conc: 0.0015,
            dntp_conc: 0.0008,
            tris_conc: 0.0,
            primer_conc: 2.5e-7, // 250 nM
        }
    }
}

// ---------------------------------------------------------------------------
// Na⁺ equivalent concentration (Owens et al. / BioPython saltcorr=7)
// ---------------------------------------------------------------------------

/// Compute effective monovalent cation concentration accounting for
/// Mg²⁺, dNTPs, and Tris. Returns concentration in M.
///
/// Formula from BioPython `salt_correction` method 7.  Note that the
/// 120×√(Mg−dNTPs) term expects mM units internally, so we convert
/// to mM for the calculation and back to M for the result.
///
///   [Na⁺_eq] = [Na⁺] + [K⁺] + [Tris]/2 + 120 × sqrt([Mg²⁺] − [dNTPs])
///
/// The sqrt term is clamped to ≥ 0 (Mg²⁺ is chelated by dNTPs).
pub fn na_equivalent(params: &TmParams) -> f64 {
    let na_mm = params.na_conc * 1000.0;
    let tris_mm = params.tris_conc * 1000.0;
    let mg_mm = params.mg_conc * 1000.0;
    let dntp_mm = params.dntp_conc * 1000.0;
    let mg_free = (mg_mm - dntp_mm).max(0.0);
    let na_eq_mm = na_mm + tris_mm / 2.0 + 120.0 * mg_free.sqrt();
    na_eq_mm / 1000.0 // back to M
}

// ---------------------------------------------------------------------------
// NN parameter lookup with IUPAC support
// ---------------------------------------------------------------------------

/// Cached NN parameter lookup map — built once, shared globally.
static NN_MAP: OnceLock<HashMap<[u8; 2], (f64, f64)>> = OnceLock::new();

fn get_nn_map() -> &'static HashMap<[u8; 2], (f64, f64)> {
    NN_MAP.get_or_init(|| {
        let mut m = HashMap::with_capacity(16);
        for &(k, dh, ds) in NN_TABLE {
            m.insert(*k, (dh, ds));
        }
        m
    })
}

/// Look up NN parameters for a dinucleotide pair, averaging over IUPAC
/// expansions when ambiguous bases are present.
fn lookup_nn(b1: u8, b2: u8, nn_map: &HashMap<[u8; 2], (f64, f64)>) -> Option<(f64, f64)> {
    let e1 = iupac::iupac_expand(b1);
    let e2 = iupac::iupac_expand(b2);
    if e1.is_empty() || e2.is_empty() {
        return None;
    }

    let mut sum_dh = 0.0;
    let mut sum_ds = 0.0;
    let mut count = 0u32;

    for &x in e1 {
        for &y in e2 {
            if let Some(&(dh, ds)) = nn_map.get(&[x, y]) {
                sum_dh += dh;
                sum_ds += ds;
                count += 1;
            }
        }
    }

    if count > 0 {
        Some((sum_dh / count as f64, sum_ds / count as f64))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Dinucleotide parameter summation
// ---------------------------------------------------------------------------

/// Sum dinucleotide ΔH° and ΔS° for a duplex sequence (primer strand, 5'→3').
/// Supports IUPAC ambiguous bases via weighted averaging.
fn sum_nn_params(seq: &[u8]) -> (f64, f64) {
    if seq.len() < 2 {
        return (0.0, DS_INIT);
    }

    let nn_map = get_nn_map();
    let mut dh = 0.0;
    let mut ds = DS_INIT;

    for w in seq.windows(2) {
        if let Some((h, s)) = lookup_nn(w[0], w[1], nn_map) {
            dh += h;
            ds += s;
        } else {
            // Unknown dinucleotide — fall back to average.
            dh += -8.2;
            ds += -22.0;
        }
    }

    (dh, ds)
}

// ---------------------------------------------------------------------------
// Tm computation
// ---------------------------------------------------------------------------

/// Compute Tm (°C) using the SantaLucia 2004 nearest-neighbour model with
/// full salt correction (Na⁺ equivalent + Mg²⁺).
///
/// For sequences shorter than 4 bases, falls back to the Wallace rule
/// (Tm = 4×GC + 2×AT). For all longer sequences, uses the NN model.
pub fn compute_tm_with_params(duplex: &str, params: &TmParams) -> f64 {
    let seq = duplex.to_ascii_uppercase();
    let n = seq.len();
    let seq_bytes = seq.as_bytes();

    // Wallace rule for oligos shorter than 6 bp (NN unreliable at this length).
    if n < 6 {
        let mut gc = 0_usize;
        let mut at = 0_usize;
        for &b in seq_bytes {
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

    let (dh, ds) = sum_nn_params(seq_bytes);

    let na_eq = na_equivalent(params);
    // Salt correction: ΔS°_salt = ΔS° + 0.368 × (N−1) × ln([Na⁺_eq])
    let ds_salt = ds + 0.368 * (n as f64 - 1.0) * (na_eq.max(1e-9)).ln();

    // Tm = ΔH°×1000 / (ΔS°_salt + R × ln(C/4)) − 273.15
    let tm =
        (dh * 1000.0) / (ds_salt + R * (params.primer_conc.max(1e-12) / 4.0).ln()) - 273.15;

    if tm.is_nan() || tm < -273.15 {
        0.0
    } else {
        tm
    }
}

/// Convenience wrapper with default PCR conditions (Taq buffer).
pub fn compute_tm(duplex: &str) -> f64 {
    compute_tm_with_params(duplex, &TmParams::default())
}

/// Legacy wrapper with explicit concentrations (backward-compatible).
pub fn compute_tm_nn(duplex: &str, na_conc: f64, primer_conc: f64) -> f64 {
    let params = TmParams {
        na_conc,
        primer_conc,
        ..TmParams::default()
    };
    compute_tm_with_params(duplex, &params)
}

// ---------------------------------------------------------------------------
// Product Tm (Rychlik 1990)
// ---------------------------------------------------------------------------

/// Compute amplicon/product Tm using the Rychlik formula.
///
/// Rychlik, Spencer, and Rhoads (1990). Optimization of the annealing
/// temperature for DNA amplification in vitro.
///   Tm = 81.5 + 0.41 × GC% + 16.6 × log₁₀([K⁺]) − 675 / N
pub fn compute_tm_product(seq: &str, k_conc: f64) -> f64 {
    let n = seq.len();
    if n == 0 {
        return 0.0;
    }
    let gc = gc_content(seq);
    81.5 + 0.41 * gc * 100.0 + 16.6 * k_conc.max(1e-9).log10() - 675.0 / n as f64
}

/// Product Tm with default salt concentration (50 mM K⁺).
pub fn tm_product_default(seq: &str) -> f64 {
    compute_tm_product(seq, 0.050)
}

// ---------------------------------------------------------------------------
// Annealing temperature
// ---------------------------------------------------------------------------

/// Compute annealing temperature (Ta) for Taq polymerase.
///
/// Rychlik formula:
///   Ta = 0.3 × min(Tm_fwd, Tm_rev) + 0.7 × Tm_product − 14.9
pub fn compute_ta(tm_fwd: f64, tm_rev: f64, tm_product: f64) -> f64 {
    0.3 * tm_fwd.min(tm_rev) + 0.7 * tm_product - 14.9
}

/// Annealing temperature for DBD polymerases (Phusion/Pfu-Sso7d).
///   Ta = min(Tm_fwd, Tm_rev) + 3  (capped at 72°C)
pub fn compute_ta_dbd(tm_fwd: f64, tm_rev: f64) -> f64 {
    (tm_fwd.min(tm_rev) + 3.0).min(72.0)
}

// ---------------------------------------------------------------------------
// GC content
// ---------------------------------------------------------------------------

/// Compute GC content of a sequence (ratio 0–1).
pub fn gc_content(seq: &str) -> f64 {
    let mut gc = 0_usize;
    let mut total = 0_usize;
    for &b in seq.as_bytes() {
        match b {
            b'G' | b'C' | b'g' | b'c' | b'S' | b's' => {
                // S = G or C — both are GC, so always count as GC.
                gc += 1;
                total += 1;
            }
            b'A' | b'T' | b'a' | b't' | b'U' | b'u' | b'W' | b'w' => {
                if b == b'W' || b == b'w' {
                    total += 2; // W = A or T
                } else {
                    total += 1;
                }
            }
            b'R' | b'r' => {
                // R = A or G → 0.5 GC
                gc += 1;
                total += 2;
            }
            b'Y' | b'y' => {
                // Y = C or T → 0.5 GC
                gc += 1;
                total += 2;
            }
            b'K' | b'k' => {
                // K = G or T → 0.5 GC
                gc += 1;
                total += 2;
            }
            b'M' | b'm' => {
                // M = A or C → 0.5 GC
                gc += 1;
                total += 2;
            }
            b'B' | b'b' => {
                // B = C|G|T → 2/3 GC
                gc += 2;
                total += 3;
            }
            b'D' | b'd' => {
                // D = A|G|T → 1/3 GC
                gc += 1;
                total += 3;
            }
            b'H' | b'h' => {
                // H = A|C|T → 1/3 GC
                gc += 1;
                total += 3;
            }
            b'V' | b'v' => {
                // V = A|C|G → 2/3 GC
                gc += 2;
                total += 3;
            }
            b'N' | b'n' => {
                // N = any → 0.5 GC
                gc += 2;
                total += 4;
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- GC content ----

    #[test]
    fn test_gc_content() {
        assert!((gc_content("GCGC") - 1.0).abs() < 0.01);
        assert!((gc_content("ATAT") - 0.0).abs() < 0.01);
        assert!((gc_content("GCTA") - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_gc_content_iupac() {
        // R = A|G → 0.5, Y = C|T → 0.5
        assert!((gc_content("RR") - 0.5).abs() < 0.01);
        assert!((gc_content("YY") - 0.5).abs() < 0.01);
        // S = G|C → 1.0
        assert!((gc_content("SS") - 1.0).abs() < 0.01);
        // N = any → 0.5
        assert!((gc_content("NNNN") - 0.5).abs() < 0.01);
    }

    // ---- Tm computation ----

    #[test]
    fn test_tm_20mer() {
        let tm = compute_tm("CGTACGTACGTACGTACGTA");
        // With SantaLucia 2004 + full salt correction, Tm is ~55–65°C.
        assert!(tm > 45.0 && tm < 85.0, "Tm={tm} out of expected range");
    }

    #[test]
    fn test_tm_wallace_fallback() {
        // 4-base "GCGC" uses Wallace rule (< 6 bp).
        let tm = compute_tm("GCGC");
        assert_eq!(tm, 16.0); // 4*4 + 2*0 = 16
    }

    #[test]
    fn test_tm_short_nn() {
        // 6-mer uses NN model.
        let tm = compute_tm("CGTACG");
        assert!(tm > 5.0 && tm < 60.0, "Tm={tm} out of expected range");
    }

    #[test]
    fn test_tm_nn_model() {
        let tm = compute_tm("CGTACGTACGTACG");
        assert!(tm > 30.0 && tm < 80.0, "Tm={tm} out of expected range");
    }

    #[test]
    fn test_tm_single_base() {
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

    #[test]
    fn test_tm_all_16_dinucleotides_have_params() {
        // Every combination of A/T/G/C should have NN parameters.
        let bases = [b'A', b'C', b'G', b'T'];
        let nn_map = get_nn_map();
        for &b1 in &bases {
            for &b2 in &bases {
                assert!(
                    nn_map.contains_key(&[b1, b2]),
                    "Missing NN params for {}{}",
                    b1 as char,
                    b2 as char
                );
            }
        }
    }

    #[test]
    fn test_tm_with_params_mg_effect() {
        // Higher Mg²⁺ should increase Tm.
        let seq = "CGTACGTACGTACGTACGTA";
        let tm_low_mg = compute_tm_with_params(
            seq,
            &TmParams {
                mg_conc: 0.001,
                ..TmParams::default()
            },
        );
        let tm_high_mg = compute_tm_with_params(
            seq,
            &TmParams {
                mg_conc: 0.005,
                ..TmParams::default()
            },
        );
        assert!(
            tm_high_mg > tm_low_mg,
            "Higher Mg²⁺ should increase Tm: {tm_low_mg:.1} vs {tm_high_mg:.1}"
        );
    }

    #[test]
    fn test_tm_params_dbd() {
        let dbd = TmParams::dbd();
        assert_eq!(dbd.na_conc, 0.050);
        assert_eq!(dbd.tris_conc, 0.0);
        assert_eq!(dbd.primer_conc, 2.5e-7);
    }

    #[test]
    fn test_na_equivalent() {
        let eq = na_equivalent(&TmParams::default());
        // mM: 40 + 75/2 + 120*sqrt(1.5-0.8) = 77.5 + 120*0.837 = 177.9 mM = 0.178 M
        assert!(eq > 0.1 && eq < 0.3, "Na⁺_eq={eq} out of expected range");
    }

    #[test]
    fn test_na_equivalent_mg_less_than_dntp() {
        // When Mg²⁺ < dNTPs, free Mg²⁺ is 0 (all chelated).
        let params = TmParams {
            mg_conc: 0.0005,
            dntp_conc: 0.001,
            ..TmParams::default()
        };
        let eq = na_equivalent(&params);
        // mM: 50 + 10/2 + 0 = 55 mM = 0.055 M
        assert!((eq - 0.055).abs() < 0.001, "Na⁺_eq={eq}");
    }

    #[test]
    fn test_tm_product() {
        let tm = compute_tm_product("GCGCGCGCGCATATATATAT", 0.050);
        // GC=50%, len=20, [K⁺]=0.050
        // 81.5 + 0.41*50 + 16.6*log10(0.050) - 675/20
        // = 81.5 + 20.5 + 16.6*(-1.301) - 33.75
        // = 102.0 - 21.6 - 33.75 = 46.65
        assert!(tm > 40.0 && tm < 55.0, "Tm_product={tm}");
    }

    #[test]
    fn test_compute_ta() {
        let ta = compute_ta(58.0, 60.0, 50.0);
        // 0.3 * 58 + 0.7 * 50 - 14.9 = 17.4 + 35.0 - 14.9 = 37.5
        assert!(ta > 30.0 && ta < 45.0, "Ta={ta}");
    }

    #[test]
    fn test_compute_ta_dbd() {
        let ta = compute_ta_dbd(58.0, 60.0);
        assert!((ta - 61.0).abs() < 0.01, "Ta_dbd={ta}");

        // Cap at 72°C
        let ta_capped = compute_ta_dbd(70.0, 75.0);
        assert_eq!(ta_capped, 72.0);
    }

    #[test]
    fn test_tm_iupac_sequence() {
        // R = A|G, Y = C|T — the NN params for "RA" average "AA" and "GA"
        let tm = compute_tm("CGTACGRACGTACGTACGTA");
        assert!(tm > 40.0 && tm < 80.0, "Tm={tm} out of expected range");
    }

    #[test]
    fn test_tm_iupac_all_n() {
        let tm = compute_tm("NNNNNNNNNNNNNNNNNNNN");
        assert!(tm > 20.0 && tm < 80.0, "Tm={tm} out of expected range");
    }

    #[test]
    fn test_tm_legacy_api() {
        // Old compute_tm_nn should still work.
        let tm = compute_tm_nn("CGTACGTACGTACGTA", 0.05, 5e-7);
        assert!(tm > 40.0 && tm < 80.0, "Tm={tm} out of expected range");
    }
}
