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

/// Amino acid residue → degenerate IUPAC codon pattern (standard code).
/// B = Asx (D/N), Z = Glx (E/Q), X = any, '*' = stop.
fn aa_codon_pattern(c: u8) -> Option<&'static str> {
    Some(match c {
        b'A' => "GCN",
        b'R' => "MGN",
        b'N' => "AAY",
        b'D' => "GAY",
        b'C' => "TGY",
        b'Q' => "CAR",
        b'E' => "GAR",
        b'G' => "GGN",
        b'H' => "CAY",
        b'I' => "ATH",
        b'L' => "YTN",
        b'K' => "AAR",
        b'M' => "ATG",
        b'F' => "TTY",
        b'P' => "CCN",
        b'S' => "WSN",
        b'T' => "ACN",
        b'W' => "TGG",
        b'Y' => "TAY",
        b'V' => "GTN",
        b'B' => "RAY",
        b'Z' => "SAR",
        b'X' => "NNN",
        b'*' => "TRR",
        _ => return None,
    })
}

/// A query that looks like a peptide (contains a letter outside the
/// nucleotide IUPAC alphabet, e.g. E/F/I/L/P/Q/*) is treated as one: each
/// residue expands to its degenerate IUPAC codon pattern, so the scanner
/// matches any coding region that translates to it. Pure nucleotide-letter
/// queries stay nucleotide searches. None if the query is not a valid peptide.
fn normalize_peptide_query(query: &str) -> Option<String> {
    let q = query.trim().to_ascii_uppercase();
    if q.is_empty() {
        return None;
    }
    let mut peptide_only = false;
    let mut out = String::with_capacity(q.len() * 3);
    for b in q.bytes() {
        out.push_str(aa_codon_pattern(b)?);
        if iupac::iupac_expand(b).is_empty() {
            peptide_only = true;
        }
    }
    if peptide_only {
        Some(out)
    } else {
        None
    }
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
/// A query that looks like a peptide (letters outside the nucleotide IUPAC
/// alphabet) is expanded to degenerate IUPAC codons before scanning.
pub fn find_seq_matches(seq: &str, query: &str) -> Vec<SeqMatch> {
    let q = query.trim().to_ascii_uppercase();
    // '*' expands to TRR which also covers TGG (Trp), so stop-codon
    // positions are re-checked against the actual bases after scanning.
    let stop_residues: Vec<usize> = q
        .bytes()
        .enumerate()
        .filter_map(|(i, b)| (b == b'*').then_some(i))
        .collect();
    let pattern = match normalize_peptide_query(query) {
        Some(p) => p,
        None => match normalize_query(query) {
            Some(p) => p,
            None => return Vec::new(),
        },
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
    if !stop_residues.is_empty() {
        out.retain(|m| stop_codons_match(seq, m, &stop_residues));
    }
    out
}

/// Check that every '*' residue of the peptide query maps to a real stop
/// codon (TAA/TAG/TGA) at the hit, in the peptide's 5'→3' orientation.
fn stop_codons_match(seq: &str, m: &SeqMatch, stop_residues: &[usize]) -> bool {
    let hit = &seq.as_bytes()[m.start as usize..=m.end as usize];
    let oriented;
    let bases: &[u8] = if m.strand == "-" {
        oriented = reverse_complement_iupac(hit);
        &oriented
    } else {
        hit
    };
    stop_residues.iter().all(|&i| {
        let codon = &bases[i * 3..i * 3 + 3];
        matches!(codon, b"TAA" | b"TAG" | b"TGA")
    })
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
        assert!(find_seq_matches("ACGT", "J").is_empty());
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
    fn peptide_query_matches_coding_regions() {
        // "M*" expands to ATGTRR: ATG followed by any stop codon.
        assert_eq!(
            find_seq_matches("GGATGTAA", "M*"),
            vec![SeqMatch {
                start: 2,
                end: 7,
                strand: "+".into()
            }]
        );
        // TCACAT is the reverse complement of ATGTGA (M* with TGA stop).
        assert_eq!(
            find_seq_matches("GTCACATG", "M*"),
            vec![SeqMatch {
                start: 1,
                end: 6,
                strand: "-".into()
            }]
        );
    }

    #[test]
    fn peptide_stop_matches_all_stop_codons() {
        // "*" expands to TRR (TAA/TAG/TGA); rc YYA also hits on '-' strand.
        assert_eq!(
            find_seq_matches("CTAGC", "*"),
            vec![
                SeqMatch {
                    start: 1,
                    end: 3,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 0,
                    end: 2,
                    strand: "-".into()
                },
            ]
        );
    }

    #[test]
    fn peptide_stop_does_not_match_trp() {
        // TRR also covers TGG (Trp); stop queries must not hit it.
        assert!(find_seq_matches("ATGG", "*").is_empty());
        assert!(find_seq_matches("GGATGTGG", "M*").is_empty());
        // CCA is the reverse complement of TGG.
        assert!(find_seq_matches("GCCAG", "*").is_empty());
        for stop in ["TAA", "TAG", "TGA"] {
            let seq = format!("GGATG{}", stop);
            assert_eq!(find_seq_matches(&seq, "M*").len(), 1, "{}", stop);
        }
    }

    #[test]
    fn peptide_query_without_stop() {
        // "MQW" expands to ATGCARTGG; Q is not a nucleotide letter, so the
        // query is treated as a peptide even without '*'.
        assert_eq!(
            find_seq_matches("ATGCAATGG", "MQW"),
            vec![SeqMatch {
                start: 0,
                end: 8,
                strand: "+".into()
            }]
        );
        // Pure nucleotide-letter queries are never treated as peptides.
        assert_eq!(
            find_seq_matches("ATGCAA", "TGC"),
            vec![
                SeqMatch {
                    start: 1,
                    end: 3,
                    strand: "+".into()
                },
                SeqMatch {
                    start: 2,
                    end: 4,
                    strand: "-".into()
                },
            ]
        );
    }

    #[test]
    fn peptide_query_with_invalid_residue() {
        assert!(find_seq_matches("ATGTAA", "M*1").is_empty());
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
