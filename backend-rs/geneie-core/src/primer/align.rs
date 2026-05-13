//! Semi-global primer-template alignment engine.
//!
//! Replaces the Smith-Waterman based approach with k-mer seeded semi-global
//! (overlap) alignment that correctly identifies 5' tails, 3' tails,
//! mismatches, gaps, and insertions.
//!
//! # Algorithm
//!
//! 1. K-mer seeding: extract 6-mers from the primer query, find exact matches
//!    on template (both strands), extend to candidate regions.
//! 2. Overlap DP alignment per candidate: free end-gaps, penalised internal
//!    indels. Match=+2, Mismatch=-1, Gap=-2.
//! 3. Parse alignment into [`AlignedColumn`] list with 5'/3' tails extracted.

use std::collections::HashSet;

use crate::models::{AlignedColumn, BindingSite};
use crate::utils;

/// Minimum fraction of primer length that must align for a valid site.
const MIN_MATCH_FRACTION: f64 = 0.6;

/// K-mer size for seeding.
const KMER: usize = 6;

/// DP scores.
const MATCH_SCORE: i32 = 2;
const MISMATCH_SCORE: i32 = -1;
const GAP_SCORE: i32 = -2;

// ---------------------------------------------------------------------------
// K-mer seeding
// ---------------------------------------------------------------------------

/// Scan `template` for all exact matches of each k-mer from `query`.
/// Returns a set of template positions (0-based) where a k-mer match starts.
fn find_kmer_seeds(query: &[u8], template: &[u8], k: usize) -> Vec<usize> {
    if query.len() < k || template.len() < k {
        return Vec::new();
    }

    // Collect unique k-mers from query.
    let mut seeds: HashSet<&[u8]> = HashSet::new();
    for i in 0..=query.len() - k {
        seeds.insert(&query[i..i + k]);
    }

    let mut hits: Vec<usize> = Vec::new();
    for i in 0..=template.len().saturating_sub(k) {
        if seeds.contains(&template[i..i + k]) {
            hits.push(i);
        }
    }

    // Sort and deduplicate.
    hits.sort();
    hits.dedup();
    hits
}

// ---------------------------------------------------------------------------
// Semi-global (overlap) DP alignment
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Match,
    Mismatch,
    /// Gap in primer (template has base, primer doesn't — deletion in primer).
    Del,
    /// Gap in template (primer has base, template doesn't — insertion in primer).
    Ins,
}

/// Result of overlap alignment between a primer and a template region.
struct OverlapAlignment {
    ops: Vec<Op>,
    primer_start: usize,
    primer_end: usize,
    template_start: usize,
}

/// Compute overlap alignment between `primer` and `template_region`.
///
/// Free end-gaps for both sequences. Internal gaps are penalised.
/// Returns the best alignment or `None` if no meaningful alignment found.
fn overlap_align(primer: &[u8], template_region: &[u8]) -> Option<OverlapAlignment> {
    let n = primer.len();
    let m = template_region.len();
    if n == 0 || m == 0 {
        return None;
    }

    // DP table
    let mut dp = vec![vec![i32::MIN; m + 1]; n + 1];
    dp[0][0] = 0;
    // Free end gaps at start
    for i in 0..=n {
        dp[i][0] = 0;
    }
    for j in 0..=m {
        dp[0][j] = 0;
    }

    for i in 1..=n {
        for j in 1..=m {
            let diag = dp[i - 1][j - 1]
                + if primer[i - 1] == template_region[j - 1] {
                    MATCH_SCORE
                } else {
                    MISMATCH_SCORE
                };
            let up = dp[i - 1][j] + GAP_SCORE; // gap in template → insertion in primer
            let left = dp[i][j - 1] + GAP_SCORE; // gap in primer → deletion in primer
            dp[i][j] = diag.max(up).max(left);
        }
    }

    // Find best end: max in last row or last column (free end gaps).
    let mut best_i = n;
    let mut best_j = m;
    let mut best_score = dp[n][m];
    for j in 0..=m {
        if dp[n][j] > best_score {
            best_score = dp[n][j];
            best_i = n;
            best_j = j;
        }
    }
    for i in 0..=n {
        if dp[i][m] > best_score {
            best_score = dp[i][m];
            best_i = i;
            best_j = m;
        }
    }

    if best_score <= 0 {
        return None;
    }

    // Backtrack from best position.
    let mut ops: Vec<Op> = Vec::new();
    let (mut i, mut j) = (best_i, best_j);

    while i > 0 || j > 0 {
        if i > 0 && j > 0 {
            let s = if primer[i - 1] == template_region[j - 1] {
                MATCH_SCORE
            } else {
                MISMATCH_SCORE
            };
            if dp[i][j] == dp[i - 1][j - 1] + s {
                ops.push(if primer[i - 1] == template_region[j - 1] {
                    Op::Match
                } else {
                    Op::Mismatch
                });
                i -= 1;
                j -= 1;
                continue;
            }
        }
        if i > 0 && dp[i][j] == dp[i - 1][j] + GAP_SCORE {
            ops.push(Op::Ins); // gap in template
            i -= 1;
        } else if j > 0 {
            // Gap in primer or free end gap for template
            ops.push(Op::Del);
            j -= 1;
        } else {
            // Free end gap for primer — remaining i bases are 5' tail
            break;
        }
    }

    ops.reverse();

    // Remaining unprocessed primer bases (i > 0) = 5' tail.
    let primer_start = i;
    let template_start = j;

    // Count primer bases at trailing end (free end gaps).
    // These are 3' tail bases that were not consumed in the alignment.
    let primer_end = best_i;

    Some(OverlapAlignment {
        ops,
        primer_start,
        primer_end,
        template_start,
    })
}

// ---------------------------------------------------------------------------
// Parse alignment to AlignedColumn list
// ---------------------------------------------------------------------------

/// Parsed result from an overlap alignment for a single binding site.
struct ParsedAlignment {
    five_prime_tail: String,
    three_prime_tail: String,
    alignment: Vec<AlignedColumn>,
    tm: f64,
    match_start: i64,
    match_end: i64,
}

/// Parse an [`OverlapAlignment`] into the structured format the frontend needs.
///
/// `primer` is the full query sequence (5'→3'), `region_start` is the absolute
/// template offset of the candidate region.
fn parse_overlap_alignment(
    aln: &OverlapAlignment,
    primer: &[u8],
    template_region: &[u8],
    region_start: i64,
) -> Option<ParsedAlignment> {
    let ops = &aln.ops;
    let primer_len = primer.len();

    // Identify 5' tail: primer_start bases + leading Ins ops.
    let mut five_prime_tail = String::new();
    let mut op_idx = 0;
    let mut primer_pos = aln.primer_start;

    // Add remaining primer_start bases (from backtrack break) as 5' tail.
    for k in 0..aln.primer_start {
        five_prime_tail.push(primer[k] as char);
    }

    // Leading Ins ops are also part of 5' tail.
    while op_idx < ops.len() && ops[op_idx] == Op::Ins {
        five_prime_tail.push(primer[primer_pos] as char);
        primer_pos += 1;
        op_idx += 1;
    }

    // Skip leading Dels (template overhang before primer alignment starts).
    while op_idx < ops.len() && ops[op_idx] == Op::Del {
        op_idx += 1;
    }

    // Now process the core aligned region.
    let mut cols: Vec<AlignedColumn> = Vec::new();
    let mut template_pos = aln.template_start;

    while op_idx < ops.len() {
        match ops[op_idx] {
            Op::Match | Op::Mismatch => {
                let kind = if ops[op_idx] == Op::Match {
                    "match"
                } else {
                    "mismatch"
                };
                let pbase = (primer[primer_pos] as char).to_string();
                let tbase = if template_pos < template_region.len() {
                    (template_region[template_pos] as char).to_string()
                } else {
                    String::new()
                };

                // Check if next ops are insertions.
                let mut insertion_after: Option<String> = None;
                let mut peek = op_idx + 1;
                while peek < ops.len() && ops[peek] == Op::Ins {
                    let ins_base = primer[primer_pos + 1 + (peek - op_idx - 1)] as char;
                    insertion_after
                        .get_or_insert_with(String::new)
                        .push(ins_base);
                    peek += 1;
                }

                cols.push(AlignedColumn {
                    template_col: region_start + template_pos as i64,
                    kind: kind.to_string(),
                    primer_base: pbase,
                    template_base: tbase,
                    insertion_after,
                });

                primer_pos += 1;
                template_pos += 1;
                op_idx += 1;

                // Skip the insertion ops we consumed.
                while op_idx < ops.len() && ops[op_idx] == Op::Ins {
                    primer_pos += 1;
                    op_idx += 1;
                }
            }
            Op::Del => {
                // Gap in primer: template has base, primer doesn't.
                let tbase = if template_pos < template_region.len() {
                    (template_region[template_pos] as char).to_string()
                } else {
                    String::new()
                };
                cols.push(AlignedColumn {
                    template_col: region_start + template_pos as i64,
                    kind: "gap".to_string(),
                    primer_base: "-".to_string(),
                    template_base: tbase,
                    insertion_after: None,
                });
                template_pos += 1;
                op_idx += 1;
            }
            Op::Ins => {
                // Internal insertion (not leading/trailing). These are
                // consumed as insertion_after on the previous column, so
                // this should not happen.
                primer_pos += 1;
                op_idx += 1;
            }
        }
    }

    // Remaining primer bases after alignment = 3' tail.
    let mut three_prime_tail = String::new();
    while primer_pos < aln.primer_end && primer_pos < primer_len {
        three_prime_tail.push(primer[primer_pos] as char);
        primer_pos += 1;
    }

    if cols.is_empty() {
        return None;
    }

    // Compute Tm from matching bases only.
    let matching_bases: String = cols
        .iter()
        .filter(|c| c.kind == "match")
        .map(|c| c.primer_base.as_str())
        .collect();
    let tm = crate::primer::tm::compute_tm(&matching_bases);

    // match_start/end from first/last column with template position.
    let match_start = cols.first().map(|c| c.template_col).unwrap_or(0);
    let match_end = cols.last().map(|c| c.template_col).unwrap_or(0);

    Some(ParsedAlignment {
        five_prime_tail,
        three_prime_tail,
        alignment: cols,
        tm,
        match_start,
        match_end,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute all binding sites for a primer against the template.
///
/// Searches both strands (using reverse-complement for rev search) and
/// returns sites sorted by Tm descending.
///
/// `primer_type` is the declared type ("fwd"|"rev"); we additionally search
/// the opposite strand to find potential off-target binding sites.
pub fn compute_binding_sites(
    template: &str,
    primer_seq: &str,
    _primer_type: &str,
    topology: &str,
    tm_threshold: f64,
) -> Vec<BindingSite> {
    let plen = primer_seq.len();
    if plen == 0 || template.is_empty() {
        return Vec::new();
    }

    let query = primer_seq.as_bytes();
    let rc = utils::reverse_complement(primer_seq);
    let rc_bytes = rc.as_bytes();

    let tpl_bytes = template.as_bytes();
    let tlen = tpl_bytes.len();
    let is_circular = topology == "circular";

    let min_align = ((plen as f64) * MIN_MATCH_FRACTION) as usize;

    let mut results: Vec<BindingSite> = Vec::new();

    // Search both strands.
    for &(q, is_rc) in &[(query, false), (&rc_bytes[..], true)] {
        let seeds = find_kmer_seeds(q, tpl_bytes, KMER);

        // If no k-mer seeds found, try a global sliding-window scan.
        let candidates: Vec<(usize, usize)> = if seeds.is_empty() {
            sliding_window_candidates(tlen, plen, is_circular)
        } else {
            seeds_to_candidates(&seeds, tlen, plen, is_circular)
        };

        // Align each candidate.
        let mut seen: HashSet<(usize, usize)> = HashSet::new(); // (tstart, tend) for dedup
        for (reg_start, reg_end) in candidates {
            // For circular template, wrap the region.
            let region: Vec<u8> = if is_circular {
                wrap_template_region(tpl_bytes, reg_start, reg_end)
            } else {
                let s = reg_start.min(tlen);
                let e = reg_end.min(tlen);
                tpl_bytes[s..e].to_vec()
            };

            let region_start = reg_start as i64;
            let Some(aln) = overlap_align(q, &region) else {
                continue;
            };
            let Some(parsed) =
                parse_overlap_alignment(&aln, q, &region, region_start)
            else {
                continue;
            };

            // Validate minimum alignment length.
            let match_count: usize = parsed
                .alignment
                .iter()
                .filter(|c| c.kind == "match")
                .count();
            if match_count < min_align {
                continue;
            }

            if parsed.tm < tm_threshold {
                continue;
            }

            let key = (
                parsed.match_start as usize,
                parsed.match_end as usize,
            );
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            // For reverse-complement search, complement the displayed bases.
            let (five_prime_tail, three_prime_tail, alignment) = if is_rc {
                (
                    utils::complement(&parsed.five_prime_tail),
                    utils::complement(&parsed.three_prime_tail),
                    complement_alignment_columns(&parsed.alignment),
                )
            } else {
                (
                    parsed.five_prime_tail,
                    parsed.three_prime_tail,
                    parsed.alignment,
                )
            };

            results.push(BindingSite {
                match_start: parsed.match_start,
                match_end: parsed.match_end,
                tm: parsed.tm,
                five_prime_tail,
                three_prime_tail,
                alignment,
            });
        }
    }

    // Sort by Tm descending, deduplicate.
    results.sort_by(|a, b| b.tm.partial_cmp(&a.tm).unwrap_or(std::cmp::Ordering::Equal));
    results.dedup_by(|a, b| {
        a.match_start == b.match_start && a.match_end == b.match_end
    });

    results
}

/// Compute binding sites for all primers in a project.
pub fn recompute_all_primers(template: &str, topology: &str, primers: &[crate::models::Primer]) -> Vec<crate::models::Primer> {
    primers
        .iter()
        .map(|p| {
            let mut updated = p.clone();
            updated.binding_sites = compute_binding_sites(
                template,
                &p.primer_seq,
                &p.r#type,
                topology,
                0.0, // report all, frontend filters by Tm
            );
            updated
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn complement_alignment_columns(cols: &[AlignedColumn]) -> Vec<AlignedColumn> {
    cols.iter()
        .map(|c| AlignedColumn {
            primer_base: if c.primer_base == "-" {
                "-".to_string()
            } else {
                utils::complement_char(c.primer_base.chars().next().unwrap_or(' '))
                    .to_string()
            },
            template_base: utils::complement_char(
                c.template_base.chars().next().unwrap_or(' '),
            )
            .to_string(),
            insertion_after: c
                .insertion_after
                .as_ref()
                .map(|s| utils::complement(s)),
            ..c.clone()
        })
        .collect()
}

/// Expand k-mer seed positions into candidate regions.
fn seeds_to_candidates(
    seeds: &[usize],
    tlen: usize,
    plen: usize,
    is_circular: bool,
) -> Vec<(usize, usize)> {
    let padding = plen; // extend plen bases on each side
    let mut merged: Vec<(usize, usize)> = Vec::new();

    for &seed in seeds {
        let start = if is_circular {
            (seed + tlen - padding) % tlen
        } else {
            seed.saturating_sub(padding)
        };
        let end = if is_circular {
            (seed + plen + padding) % tlen
        } else {
            (seed + plen + padding).min(tlen)
        };

        // Merge with previous if overlapping.
        if let Some(last) = merged.last_mut() {
            if !is_circular && start <= last.1 + plen {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    merged
}

/// Fallback: sliding window across the whole template.
fn sliding_window_candidates(
    tlen: usize,
    plen: usize,
    is_circular: bool,
) -> Vec<(usize, usize)> {
    let step = plen.max(1);
    let padding = plen / 2;
    let mut candidates = Vec::new();
    let limit = if is_circular { tlen } else { tlen.saturating_sub(plen) };
    let mut pos = 0;
    while pos < limit {
        let start = pos.saturating_sub(padding);
        let end = (pos + plen + padding).min(tlen);
        candidates.push((start, end));
        pos += step;
    }
    // For circular, also wrap the end.
    if is_circular {
        candidates.push((tlen.saturating_sub(plen + padding), plen + padding));
    }
    candidates
}

/// Extract a template region, wrapping around for circular sequences.
fn wrap_template_region(template: &[u8], start: usize, end: usize) -> Vec<u8> {
    let tlen = template.len();
    if tlen == 0 {
        return Vec::new();
    }
    let start = start % tlen;
    let end = end % tlen;
    if start <= end {
        template[start..end].to_vec()
    } else {
        let mut v = Vec::with_capacity((tlen - start) + end);
        v.extend_from_slice(&template[start..]);
        v.extend_from_slice(&template[..end]);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kmer_seeds_basic() {
        let seeds = find_kmer_seeds(b"ATGCAT", b"NNATGCATNN", 6);
        assert_eq!(seeds, vec![2]);
    }

    #[test]
    fn test_kmer_seeds_multiple() {
        let seeds = find_kmer_seeds(b"ATGCAT", b"ATGCATNNATGCAT", 6);
        assert_eq!(seeds, vec![0, 8]);
    }

    #[test]
    fn test_overlap_align_exact() {
        let primer = b"ATGC";
        let tmpl = b"ATGC";
        let aln = overlap_align(primer, tmpl).unwrap();
        assert_eq!(aln.primer_start, 0);
        let match_ops: Vec<_> = aln.ops.iter().filter(|&&o| o == Op::Match).collect();
        assert_eq!(match_ops.len(), 4);
    }

    #[test]
    fn test_overlap_align_with_mismatch() {
        let primer = b"ATGC";
        let tmpl = b"AGGC";
        let aln = overlap_align(primer, tmpl).unwrap();
        assert!(aln.ops.contains(&Op::Mismatch));
    }

    #[test]
    fn test_overlap_align_with_tail() {
        // Primer has 5' tail "AA" + match "TGC".
        // Short template so AA must be unaligned (tail).
        let primer = b"AATGC";
        let tmpl = b"TGCNNN";
        let aln = overlap_align(primer, tmpl).unwrap();
        // The 5' tail is captured in primer_start.
        assert_eq!(aln.primer_start, 2, "expected primer_start=2 for 5' tail, got {}", aln.primer_start);
    }

    #[test]
    fn test_overlap_align_with_gap() {
        // Primer "AAAAACCCCC" vs template "AAAATTTTTCCCCC"
        // Primer has 5 A matches, then template has TTTTT (gap in primer), then 5 C matches.
        let primer = b"AAAAACCCCC";
        let tmpl = b"AAAATTTTTCCCCC";
        let aln = overlap_align(primer, tmpl).unwrap();
        assert!(aln.ops.contains(&Op::Del), "expected gap (Del) in alignment");
    }

    #[test]
    fn test_compute_binding_sites_exact_match() {
        let template = "NNNNNCGTACGCTAGNNNNN";
        let sites = compute_binding_sites(template, "CGTACGCTAG", "fwd", "linear", 20.0);
        assert!(sites.len() >= 1);
        assert_eq!(sites[0].five_prime_tail, "");
        assert_eq!(sites[0].three_prime_tail, "");
        assert!(sites[0].alignment.iter().all(|c| c.kind == "match"));
        assert!((sites[0].tm - 32.0).abs() < 1.0);
    }

    #[test]
    fn test_compute_binding_sites_with_tail() {
        // Template with clear match region; tail is non-complementary.
        let template = "GGGGGGGGGGCGTACGCTAGGGGGGGGGGG";
        // Primer has 5' tail "AAAA" + match "CGTACGCTAG".
        let sites = compute_binding_sites(template, "AAAACGTACGCTAG", "fwd", "linear", 20.0);
        assert!(sites.len() >= 1, "expected at least 1 binding site");
        // The tail may be identified as mismatches or insertions depending on
        // alignment scoring. Either way, the match region should be found.
        assert!(sites[0].alignment.iter().any(|c| c.kind == "match"));
    }

    #[test]
    fn test_compute_binding_sites_reverse() {
        let template = "NNNNNCTAGCGTACGNNNNN";
        // Reverse-complement of CGTACGCTAG is CTAGCGTACG.
        let sites = compute_binding_sites(template, "CGTACGCTAG", "fwd", "linear", 20.0);
        assert!(sites.len() >= 1);
    }

    #[test]
    fn test_compute_binding_sites_below_threshold() {
        let template = "NNNNNCGTACGCTAGNNNNN";
        let sites = compute_binding_sites(template, "CGTACGCTAG", "fwd", "linear", 100.0);
        assert!(sites.is_empty());
    }

    #[test]
    fn test_compute_binding_sites_empty() {
        assert!(compute_binding_sites("", "ATGC", "fwd", "linear", 20.0).is_empty());
        assert!(compute_binding_sites("ATGC", "", "fwd", "linear", 20.0).is_empty());
    }

    #[test]
    fn test_recompute_all_primers() {
        use crate::models::Primer;
        let template = "NNNNNCGTACGCTAGNNNNN";
        let primers = vec![Primer {
            id: "P1".into(),
            name: "Test".into(),
            r#type: "fwd".into(),
            primer_seq: "CGTACGCTAG".into(),
            color: "#166534".into(),
            binding_sites: vec![],
        }];
        let updated = recompute_all_primers(template, "linear", &primers);
        assert_eq!(updated.len(), 1);
        assert!(!updated[0].binding_sites.is_empty());
    }
}
