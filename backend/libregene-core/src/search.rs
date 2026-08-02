//! Sequence search — ports `findSeqMatches` from `src/searchUtils.js`.
//!
//! Scans the template for a query that may contain IUPAC ambiguity codes,
//! on both strands. The reverse-complement scan is skipped when the pattern
//! is palindromic (its reverse complement equals itself).

use serde::{Deserialize, Serialize};

use crate::primer::iupac;

/// A query hit on the template, 0-based inclusive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeqMatch {
    pub start: i64,
    pub end: i64,
    /// "+" forward strand, "-" reverse strand.
    pub strand: String,
}

/// IUPAC code → its complement code (A↔T, C↔G, R↔Y, K↔M, B↔V, D↔H, …).
fn complement_code(c: u8) -> Option<u8> {
    Some(match c.to_ascii_uppercase() {
        b'A' => b'T',
        b'T' | b'U' => b'A',
        b'G' => b'C',
        b'C' => b'G',
        b'R' => b'Y',
        b'Y' => b'R',
        b'W' => b'W',
        b'S' => b'S',
        b'K' => b'M',
        b'M' => b'K',
        b'B' => b'V',
        b'D' => b'H',
        b'H' => b'D',
        b'V' => b'B',
        b'N' => b'N',
        _ => return None,
    })
}

fn reverse_complement_iupac(pattern: &[u8]) -> Vec<u8> {
    pattern
        .iter()
        .rev()
        .map(|&b| complement_code(b).unwrap_or(b'N'))
        .collect()
}

/// Trim/uppercase the query; None if empty or it contains a non-IUPAC char.
fn normalize_query(query: &str) -> Option<String> {
    let q = query.trim().to_ascii_uppercase();
    if q.is_empty() || q.bytes().any(|b| iupac::iupac_expand(b).is_empty()) {
        return None;
    }
    Some(q)
}

fn scan_strand(seq: &str, pattern: &[u8], strand: &str, out: &mut Vec<SeqMatch>) {
    let n = seq.len();
    let m = pattern.len();
    if m == 0 || m > n {
        return;
    }
    let seq_bytes = seq.as_bytes();
    for i in 0..=n - m {
        let mut ok = true;
        for (j, &p) in pattern.iter().enumerate() {
            if !iupac::bases_overlap(seq_bytes[i + j], p) {
                ok = false;
                break;
            }
        }
        if ok {
            out.push(SeqMatch {
                start: i as i64,
                end: (i + m - 1) as i64,
                strand: strand.to_string(),
            });
        }
    }
}

/// Find all hits of `query` (IUPAC-aware) on both strands of `seq`.
pub fn find_seq_matches(seq: &str, query: &str) -> Vec<SeqMatch> {
    let Some(pattern) = normalize_query(query) else {
        return Vec::new();
    };
    if seq.is_empty() {
        return Vec::new();
    }
    let pattern_bytes = pattern.as_bytes();
    let mut out = Vec::new();
    scan_strand(seq, pattern_bytes, "+", &mut out);
    let rc = reverse_complement_iupac(pattern_bytes);
    if rc.as_slice() != pattern_bytes {
        scan_strand(seq, &rc, "-", &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_matches() {
        assert_eq!(
            find_seq_matches("ACGTACGT", "ACGT"),
            vec![
                SeqMatch {
                    start: 0,
                    end: 3,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 4,
                    end: 7,
                    strand: "+".into()
                },
            ]
        );
    }

    #[test]
    fn palindromic_query_skips_reverse_scan() {
        // "ACGT" and "AT" are self-complementary → only '+' hits.
        assert_eq!(
            find_seq_matches("ATAT", "AT"),
            vec![
                SeqMatch {
                    start: 0,
                    end: 1,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 2,
                    end: 3,
                    strand: "+".into()
                },
            ]
        );
    }

    #[test]
    fn iupac_ambiguity_on_both_strands() {
        // R = A|G; query "AR" is not self-complementary (rc "YT"), so both
        // strands are scanned. '+' hits 0..1 and 4..5, '-' hits 2..3.
        assert_eq!(
            find_seq_matches("ARYTAR", "AR"),
            vec![
                SeqMatch {
                    start: 0,
                    end: 1,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 4,
                    end: 5,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 2,
                    end: 3,
                    strand: "-".into()
                },
            ]
        );
        // "RY" is self-complementary (R↔Y), so the reverse scan is skipped.
        assert_eq!(
            find_seq_matches("GCGC", "RY"),
            vec![
                SeqMatch {
                    start: 0,
                    end: 1,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 2,
                    end: 3,
                    strand: "+".into()
                },
            ]
        );
    }

    #[test]
    fn n_matches_any_base() {
        assert_eq!(
            find_seq_matches("ACGT", "N"),
            vec![
                SeqMatch {
                    start: 0,
                    end: 0,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 1,
                    end: 1,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 2,
                    end: 2,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 3,
                    end: 3,
                    strand: "+".into()
                },
            ]
        );
    }

    #[test]
    fn u_treated_as_t() {
        assert_eq!(
            find_seq_matches("AT", "U"),
            vec![
                SeqMatch {
                    start: 1,
                    end: 1,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 0,
                    end: 0,
                    strand: "-".into()
                },
            ]
        );
    }

    #[test]
    fn invalid_or_empty_query() {
        assert!(find_seq_matches("ACGT", "X").is_empty());
        assert!(find_seq_matches("ACGT", "A C").is_empty());
        assert!(find_seq_matches("ACGT", "  ").is_empty());
        assert!(find_seq_matches("ACGT", "").is_empty());
        assert!(find_seq_matches("", "ACGT").is_empty());
    }

    #[test]
    fn query_longer_than_seq() {
        assert!(find_seq_matches("AC", "ACGT").is_empty());
    }

    #[test]
    fn lowercase_input() {
        assert_eq!(
            find_seq_matches("acgtacgt", "acgt"),
            find_seq_matches("ACGTACGT", "ACGT")
        );
        assert_eq!(
            find_seq_matches("gcgc", "ry"),
            find_seq_matches("GCGC", "RY")
        );
    }
}
