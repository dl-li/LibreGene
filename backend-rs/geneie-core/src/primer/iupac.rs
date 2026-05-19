//! IUPAC ambiguous nucleotide code support.
//!
//! Provides expansion, complement, and match-checking for the extended
//! IUPAC DNA alphabet (ACGTURYSWKMBDHVN). Used by alignment scoring,
//! k-mer seeding, and thermodynamics to handle degenerate primers.

// ---------------------------------------------------------------------------
// IUPAC expansion
// ---------------------------------------------------------------------------

/// Return the set of standard DNA bases an IUPAC code represents.
/// Returns empty slice for unrecognised characters.
pub fn iupac_expand(base: u8) -> &'static [u8] {
    match base {
        b'A' | b'a' => &[b'A'],
        b'C' | b'c' => &[b'C'],
        b'G' | b'g' => &[b'G'],
        b'T' | b't' | b'U' | b'u' => &[b'T'],
        b'R' | b'r' => &[b'A', b'G'],
        b'Y' | b'y' => &[b'C', b'T'],
        b'S' | b's' => &[b'G', b'C'],
        b'W' | b'w' => &[b'A', b'T'],
        b'K' | b'k' => &[b'G', b'T'],
        b'M' | b'm' => &[b'A', b'C'],
        b'B' | b'b' => &[b'C', b'G', b'T'],
        b'D' | b'd' => &[b'A', b'G', b'T'],
        b'H' | b'h' => &[b'A', b'C', b'T'],
        b'V' | b'v' => &[b'A', b'C', b'G'],
        b'N' | b'n' => &[b'A', b'C', b'G', b'T'],
        _ => &[],
    }
}

/// Return the complement set of an IUPAC code.
/// The complement set is the IUPAC code for bases that pair with this one.
pub fn iupac_complement(base: u8) -> &'static [u8] {
    match base {
        b'A' | b'a' => &[b'T'],
        b'C' | b'c' => &[b'G'],
        b'G' | b'g' => &[b'C'],
        b'T' | b't' | b'U' | b'u' => &[b'A'],
        b'R' | b'r' => &[b'C', b'T'], // complement of A|G = T|C = Y
        b'Y' | b'y' => &[b'A', b'G'], // complement of C|T = G|A = R
        b'S' | b's' => &[b'G', b'C'], // complement of G|C = C|G = S
        b'W' | b'w' => &[b'A', b'T'], // complement of A|T = T|A = W
        b'K' | b'k' => &[b'A', b'C'], // complement of G|T = C|A = M
        b'M' | b'm' => &[b'G', b'T'], // complement of A|C = T|G = K
        b'B' | b'b' => &[b'A', b'C', b'G'], // complement of CGT = GCA = V
        b'D' | b'd' => &[b'A', b'C', b'T'], // complement of AGT = TCA = H
        b'H' | b'h' => &[b'A', b'G', b'T'], // complement of ACT = TGA = D
        b'V' | b'v' => &[b'C', b'G', b'T'], // complement of ACG = TGC = B
        b'N' | b'n' => &[b'A', b'C', b'G', b'T'],
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// Direct overlap — do two bases represent the same nucleotide?
// ---------------------------------------------------------------------------

/// Check whether two IUPAC codes could represent the **same** nucleotide.
/// This is the correct comparison for aligning a primer against the template
/// top strand: the primer matches the top strand directly (it binds the
/// complementary bottom strand).
///
/// Returns true if expand(a) ∩ expand(b) ≠ ∅.
///
/// # Examples
/// - `bases_overlap(b'A', b'A')` → true
/// - `bases_overlap(b'R', b'A')` → true  (R could be A)
/// - `bases_overlap(b'R', b'G')` → true  (R could be G)
/// - `bases_overlap(b'R', b'C')` → false (R is never C)
/// - `bases_overlap(b'N', b'A')` → true  (N could be anything)
pub fn bases_overlap(a: u8, b: u8) -> bool {
    let ea = iupac_expand(a);
    let eb = iupac_expand(b);
    if ea.is_empty() || eb.is_empty() {
        return false;
    }
    for &x in ea {
        if eb.contains(&x) {
            return true;
        }
    }
    false
}

/// Fraction of possible nucleotides that overlap.
/// `|expand(a) ∩ expand(b)| / |expand(a)|`
pub fn overlap_fraction(a: u8, b: u8) -> f64 {
    let ea = iupac_expand(a);
    let eb = iupac_expand(b);
    if ea.is_empty() || eb.is_empty() {
        return 0.0;
    }
    let mut matched = 0u32;
    for &x in ea {
        if eb.contains(&x) {
            matched += 1;
        }
    }
    matched as f64 / ea.len() as f64
}

/// Result of comparing two bases for direct overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapClass {
    /// Both are unambiguous and equal.
    Exact,
    /// IUPAC-ambiguous overlap (e.g. R and A, N and C).
    Ambiguous,
    /// No possible overlap.
    Mismatch,
}

/// Classify the direct overlap between two bases.
pub fn classify_overlap(a: u8, b: u8) -> OverlapClass {
    if !bases_overlap(a, b) {
        return OverlapClass::Mismatch;
    }
    let ea = iupac_expand(a);
    let eb = iupac_expand(b);
    if ea.len() == 1 && eb.len() == 1 {
        OverlapClass::Exact
    } else {
        OverlapClass::Ambiguous
    }
}

/// Weight (0–1) for scoring a direct base overlap.
/// - Exact match → 1.0
/// - N in primer, specific in template → 1.0 (primer matches any base)
/// - Specific in primer, N in template → 0.25 (only 1/4 chance)
/// - Other ambiguous overlap → overlap_fraction
/// - Mismatch → 0.0
pub fn overlap_weight(a: u8, b: u8) -> f64 {
    match classify_overlap(a, b) {
        OverlapClass::Exact => 1.0,
        OverlapClass::Ambiguous => {
            let a_up = a.to_ascii_uppercase();
            let b_up = b.to_ascii_uppercase();
            if a_up == b'N' {
                // Primer has N — designed to match any template base.
                1.0
            } else if b_up == b'N' {
                // Template has N — unknown base, low confidence match.
                0.25
            } else {
                overlap_fraction(a, b)
            }
        }
        OverlapClass::Mismatch => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Watson-Crick pairing (for complement-based comparison)
// ---------------------------------------------------------------------------

/// Check whether `primer_base` can form a valid Watson-Crick base pair with
/// `template_base`.  This checks complement(primer_base) ∩ {template_base} ≠ ∅.
///
/// Used for Tm calculation where we need to know if two bases pair across
/// the double helix.
///
/// # Examples
/// - `bases_pair(b'A', b'T')` → true  (A pairs with T)
/// - `bases_pair(b'R', b'C')` → true  (R=A|G; G pairs with C)
/// - `bases_pair(b'A', b'A')` → false (A does not pair with A)
pub fn bases_pair(primer_base: u8, template_base: u8) -> bool {
    let comp = iupac_complement(primer_base);
    let tmpl = iupac_expand(template_base);
    if comp.is_empty() || tmpl.is_empty() {
        return false;
    }
    for &cb in comp {
        if tmpl.contains(&cb) {
            return true;
        }
    }
    false
}

/// Fraction of possible Watson-Crick base pairs that match.
pub fn pair_fraction(primer_base: u8, template_base: u8) -> f64 {
    let comp = iupac_complement(primer_base);
    let tmpl = iupac_expand(template_base);
    let prim = iupac_expand(primer_base);
    if comp.is_empty() || tmpl.is_empty() || prim.is_empty() {
        return 0.0;
    }
    let mut matched = 0u32;
    for &cb in comp {
        if tmpl.contains(&cb) {
            matched += 1;
        }
    }
    matched as f64 / prim.len() as f64
}

// ---------------------------------------------------------------------------
// IUPAC regex helpers (for constructing search patterns)
// ---------------------------------------------------------------------------

/// Return regex character class string for an IUPAC code and its complement.
/// Used for constructing regex patterns that match template bases which can
/// pair with the given primer base.
///
/// Example: `iupac_compl_regex(b'R')` → `"(?:T|C)"` because R (A|G) pairs
/// with Y = C|T.
pub fn iupac_compl_regex(base: u8) -> String {
    let comp = iupac_complement(base);
    if comp.is_empty() {
        return String::new();
    }
    if comp.len() == 1 {
        format!("{}", comp[0] as char)
    } else {
        let inner: Vec<String> = comp.iter().map(|&b| (b as char).to_string()).collect();
        format!("(?:{})", inner.join("|"))
    }
}

// ---------------------------------------------------------------------------
// Sequence expansion (for k-mer seeding and automaton building)
// ---------------------------------------------------------------------------

/// Expand an IUPAC sequence into all possible unambiguous DNA sequences.
/// Returns an empty vec if any character is unrecognised.
pub fn expand_iupac_sequence(seq: &[u8]) -> Vec<Vec<u8>> {
    if seq.is_empty() {
        return vec![vec![]];
    }

    let expansions: Vec<&[u8]> = seq.iter().map(|&b| iupac_expand(b)).collect();

    let mut results: Vec<Vec<u8>> = vec![vec![]];
    for exps in expansions {
        if exps.is_empty() {
            return vec![];
        }
        let mut next: Vec<Vec<u8>> = Vec::with_capacity(results.len() * exps.len());
        for existing in &results {
            for &b in exps {
                let mut v = existing.clone();
                v.push(b);
                next.push(v);
            }
        }
        results = next;
    }
    results
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iupac_expand_standard() {
        assert_eq!(iupac_expand(b'A'), &[b'A']);
        assert_eq!(iupac_expand(b'C'), &[b'C']);
        assert_eq!(iupac_expand(b'G'), &[b'G']);
        assert_eq!(iupac_expand(b'T'), &[b'T']);
    }

    #[test]
    fn test_iupac_expand_ambiguous() {
        assert_eq!(iupac_expand(b'R'), &[b'A', b'G']);
        assert_eq!(iupac_expand(b'Y'), &[b'C', b'T']);
        assert_eq!(iupac_expand(b'N'), &[b'A', b'C', b'G', b'T']);
    }

    #[test]
    fn test_iupac_expand_lowercase() {
        assert_eq!(iupac_expand(b'a'), &[b'A']);
        assert_eq!(iupac_expand(b'n'), &[b'A', b'C', b'G', b'T']);
    }

    #[test]
    fn test_iupac_complement() {
        assert_eq!(iupac_complement(b'A'), &[b'T']);
        assert_eq!(iupac_complement(b'T'), &[b'A']);
        assert_eq!(iupac_complement(b'G'), &[b'C']);
        assert_eq!(iupac_complement(b'C'), &[b'G']);
    }

    #[test]
    fn test_iupac_complement_ambiguous() {
        assert_eq!(iupac_complement(b'R'), &[b'C', b'T']); // R → Y
        assert_eq!(iupac_complement(b'Y'), &[b'A', b'G']); // Y → R
        assert_eq!(iupac_complement(b'S'), &[b'G', b'C']); // S → S
        assert_eq!(iupac_complement(b'W'), &[b'A', b'T']); // W → W
        assert_eq!(iupac_complement(b'K'), &[b'A', b'C']); // K → M
        assert_eq!(iupac_complement(b'M'), &[b'G', b'T']); // M → K
    }

    #[test]
    fn test_bases_pair_exact() {
        assert!(bases_pair(b'A', b'T'));
        assert!(bases_pair(b'C', b'G'));
        assert!(bases_pair(b'G', b'C'));
        assert!(bases_pair(b'T', b'A'));
    }

    #[test]
    fn test_bases_pair_mismatch() {
        assert!(!bases_pair(b'A', b'A'));
        assert!(!bases_pair(b'A', b'C'));
        assert!(!bases_pair(b'C', b'T'));
    }

    #[test]
    fn test_bases_pair_iupac() {
        // R (A|G) pairs with Y (C|T): A-T or G-C works
        assert!(bases_pair(b'R', b'Y'));
        // R vs C: G-C works (A-C does not)
        assert!(bases_pair(b'R', b'C'));
        // R vs A: A-A and G-A don't work
        assert!(!bases_pair(b'R', b'A'));
        // N vs N: always works (some pairing)
        assert!(bases_pair(b'N', b'N'));
        // N vs any: always works
        assert!(bases_pair(b'N', b'A'));
        assert!(bases_pair(b'A', b'N'));
    }

    #[test]
    fn test_pair_fraction() {
        assert!((pair_fraction(b'A', b'T') - 1.0).abs() < 0.01);
        assert!((pair_fraction(b'R', b'C') - 0.5).abs() < 0.01); // only G-C, not A-C
        assert_eq!(pair_fraction(b'A', b'A'), 0.0);
    }

    #[test]
    fn test_bases_overlap() {
        assert!(bases_overlap(b'A', b'A'));
        assert!(bases_overlap(b'R', b'A'));
        assert!(bases_overlap(b'R', b'G'));
        assert!(!bases_overlap(b'R', b'C'));
        assert!(bases_overlap(b'N', b'A'));
        assert!(!bases_overlap(b'A', b'C'));
    }

    #[test]
    fn test_overlap_fraction() {
        assert!((overlap_fraction(b'A', b'A') - 1.0).abs() < 0.01);
        assert!((overlap_fraction(b'R', b'A') - 0.5).abs() < 0.01);
        assert!((overlap_fraction(b'N', b'A') - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_classify_overlap() {
        assert_eq!(classify_overlap(b'A', b'A'), OverlapClass::Exact);
        assert_eq!(classify_overlap(b'R', b'A'), OverlapClass::Ambiguous);
        assert_eq!(classify_overlap(b'N', b'A'), OverlapClass::Ambiguous);
        assert_eq!(classify_overlap(b'A', b'C'), OverlapClass::Mismatch);
    }

    #[test]
    fn test_overlap_weight() {
        assert!((overlap_weight(b'A', b'A') - 1.0).abs() < 0.01);
        assert!((overlap_weight(b'R', b'A') - 0.5).abs() < 0.01);
        assert!((overlap_weight(b'A', b'C') - 0.0).abs() < 0.01);
        // N in primer → full match
        assert!((overlap_weight(b'N', b'A') - 1.0).abs() < 0.01);
        // N in template → low-confidence match (25%)
        assert!((overlap_weight(b'A', b'N') - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_expand_iupac_sequence() {
        let expanded = expand_iupac_sequence(b"AR");
        // A (1) × R (2: A,G) = 2 sequences: AA, AG
        assert_eq!(expanded.len(), 2);
        assert!(expanded.contains(&vec![b'A', b'A']));
        assert!(expanded.contains(&vec![b'A', b'G']));
    }

    #[test]
    fn test_expand_iupac_sequence_all_n() {
        let expanded = expand_iupac_sequence(b"NN");
        assert_eq!(expanded.len(), 16); // 4 × 4
    }

    #[test]
    fn test_expand_empty() {
        let expanded: Vec<Vec<u8>> = expand_iupac_sequence(b"");
        let expected: Vec<Vec<u8>> = vec![vec![]];
        assert_eq!(expanded, expected);
    }

    #[test]
    fn test_iupac_compl_regex() {
        assert_eq!(iupac_compl_regex(b'A'), "T");
        assert_eq!(iupac_compl_regex(b'R'), "(?:C|T)"); // complement of R (A|G) = Y = C|T
    }
}
