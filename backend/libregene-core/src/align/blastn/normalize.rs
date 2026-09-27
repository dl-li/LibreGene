//! Colinear hit-chain normalization for multi-HSP alignments. Ported from
//! GenePad's `normalizeBlastHits` (https://genepad.cn, GenePad team).
//!
//! A single read aligns in one orientation with query order and reference
//! order moving together (a Sanger read vs a plasmid cannot zig-zag). Build
//! the best colinear chain per strand by greedy score order, then keep the
//! chain with the higher total score; hits of the losing strand and
//! non-colinear hits are noise for the row view. Split alignments share a
//! repeated stretch at the junction, so HSPs may overlap slightly in query
//! (RCA/concatemer reads, ITR junctions); overlapping candidates are trimmed
//! against the higher-scoring accepted hit (the lower-scoring side gives up
//! its copy of the repeat) instead of being dropped whole, and the remnant
//! must still clear the E-value floor.

use super::hsp::Strand;
use super::{compute_identity, score_to_stats, AlignColumn, ColumnHit};

/// Chance-level floor: matches at E >= 0.05 (roughly an isolated 11-14bp
/// exact word depending on ref/query lengths) are indistinguishable from
/// background.
const BLAST_EVALUE_FLOOR: f64 = 0.05;

#[derive(Clone, Copy, Debug)]
struct HitSpan {
    q_start: u64,
    q_end: u64,
    r_start: u64,
    r_end: u64,
}

fn span_of_columns(columns: &[AlignColumn]) -> Option<HitSpan> {
    let mut span = HitSpan {
        q_start: u64::MAX,
        q_end: 0,
        r_start: u64::MAX,
        r_end: 0,
    };
    for col in columns {
        if col.query_position > 0 && col.query_base != b'-' {
            span.q_start = span.q_start.min(col.query_position);
            span.q_end = span.q_end.max(col.query_position);
        }
        if col.ref_base != b'-' && col.ref_position > 0 {
            span.r_start = span.r_start.min(col.ref_position);
            span.r_end = span.r_end.max(col.ref_position);
        }
    }
    (span.q_start != u64::MAX && span.r_start != u64::MAX).then_some(span)
}

fn stats_from_columns(columns: &[AlignColumn], ref_len: usize, query_len: usize) -> (i32, f64, f64) {
    let mut score = 0i32;
    for col in columns {
        match col.col_type {
            super::ColType::Match => score += 1,
            super::ColType::Mismatch => score -= 3,
            super::ColType::Insertion | super::ColType::Deletion => score += -5 + (-2),
        }
    }
    let (bit_score, e_value) = score_to_stats(score, ref_len, query_len);
    (score, e_value, bit_score)
}

/// Normalize engine hits into the ordered colinear chain of the dominant
/// strand. Segments stay in original query order (hit_index 0 = leftmost
/// segment), not best-score-first.
pub fn normalize_blast_hits(mut hits: Vec<ColumnHit>, ref_len: usize, query_len: usize) -> Vec<ColumnHit> {
    hits.retain(|hit| hit.e_value < BLAST_EVALUE_FLOOR && span_of_columns(&hit.columns).is_some());
    if hits.is_empty() {
        return Vec::new();
    }

    let forward_chain = build_chain(&hits, Strand::Plus, ref_len, query_len);
    let reverse_chain = build_chain(&hits, Strand::Minus, ref_len, query_len);
    let chain_score = |chain: &[ColumnHit]| chain.iter().map(|h| h.score).sum::<i32>();
    let mut best_chain =
        if chain_score(&forward_chain) >= chain_score(&reverse_chain) { forward_chain } else { reverse_chain };

    best_chain.sort_by_key(|hit| span_of_columns(&hit.columns).map_or(0, |s| s.q_start));
    for (hit_index, hit) in best_chain.iter_mut().enumerate() {
        hit.hit_index = hit_index;
        if let Some(span) = span_of_columns(&hit.columns) {
            hit.ref_start = span.r_start;
            hit.ref_end = span.r_end;
            hit.identity = compute_identity(&hit.columns);
        }
    }
    best_chain
}

fn build_chain(significant: &[ColumnHit], strand: Strand, ref_len: usize, query_len: usize) -> Vec<ColumnHit> {
    let mut candidates: Vec<&ColumnHit> = significant.iter().filter(|h| h.strand == strand).collect();
    candidates.sort_by(|a, b| b.score.cmp(&a.score));

    let mut accepted: Vec<(ColumnHit, HitSpan)> = Vec::new();
    for candidate in candidates {
        let mut columns = candidate.columns.clone();
        let mut trimmed = false;
        let mut ok = true;

        for (_, acc_span) in &accepted {
            let Some(span) = span_of_columns(&columns) else {
                ok = false;
                break;
            };

            let overlap_q = span.q_start <= acc_span.q_end && acc_span.q_start <= span.q_end;
            let overlap_r = span.r_start <= acc_span.r_end && acc_span.r_start <= span.r_end;

            if !overlap_q && !overlap_r {
                let before = span.q_end < acc_span.q_start && span.r_end < acc_span.r_start;
                let after = span.q_start > acc_span.q_end && span.r_start > acc_span.r_end;
                if !before && !after {
                    ok = false;
                    break;
                }
                continue;
            }

            let before_in_query = span.q_start < acc_span.q_start;
            // Trim against BOTH coordinates: query-overlapped columns are
            // the shared repeat copy, and ref-overlapped columns handle
            // junctions where one HSP over-extends a base or two onto the
            // neighbour's diagonal — without the ref trim the tied span
            // never satisfies the strict colinearity check below and the
            // whole segment (a perfectly matching flank) gets dropped.
            columns.retain(|col| {
                if col.query_position <= 0 {
                    return true;
                }
                if before_in_query {
                    col.query_position < acc_span.q_start && col.ref_position < acc_span.r_start
                } else {
                    col.query_position > acc_span.q_end && col.ref_position > acc_span.r_end
                }
            });
            trimmed = true;

            let Some(trimmed_span) = span_of_columns(&columns) else {
                ok = false;
                break;
            };
            let trimmed_before =
                trimmed_span.q_end < acc_span.q_start && trimmed_span.r_end < acc_span.r_start;
            let trimmed_after =
                trimmed_span.q_start > acc_span.q_end && trimmed_span.r_start > acc_span.r_end;
            if !trimmed_before && !trimmed_after {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }

        let Some(span) = span_of_columns(&columns) else {
            continue;
        };

        if !trimmed {
            accepted.push((candidate.clone(), span));
            continue;
        }
        let (score, e_value, bit_score) = stats_from_columns(&columns, ref_len, query_len);
        if e_value >= BLAST_EVALUE_FLOOR {
            continue;
        }
        let mut hit = candidate.clone();
        hit.columns = columns;
        hit.ref_start = span.r_start;
        hit.ref_end = span.r_end;
        hit.score = score;
        hit.e_value = e_value;
        hit.bit_score = bit_score;
        hit.identity = compute_identity(&hit.columns);
        accepted.push((hit, span));
    }
    accepted.into_iter().map(|(hit, _)| hit).collect()
}
