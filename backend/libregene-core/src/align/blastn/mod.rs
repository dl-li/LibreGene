//! Local BLAST (blastn) engine.
//!
//! Ported from GenePad's `gene-core` blast module (https://genepad.cn),
//! contributed by the GenePad team. The algorithm follows the NCBI BLAST
//! 2.9.0+ C++ sources: scan → ungapped X-drop → gapped X-drop DP → HSP →
//! Karlin-Altschul statistics, plus full-length fast paths for
//! near-identical whole sequences and colinear hit-chain normalization for
//! multi-segment reads.
//!
//! 引擎移植自 GenePad 项目（https://genepad.cn），由 GenePad 团队贡献。

pub mod dust;
pub mod editblock;
pub mod engine;
pub mod full_length;
pub mod gapped;
pub mod greedy;
pub mod hsp;
pub mod lookup;
pub mod normalize;
pub mod options;
pub mod scan;
pub mod seqblk;
pub mod stat;
pub mod ungapped;

pub use engine::{run_blastn, BlastHit as EngineHit, BlastResult};
pub use hsp::Strand;
pub use options::BlastnOptions;

/// Column kind of a rendered alignment column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColType {
    Match,
    Mismatch,
    Insertion,
    Deletion,
}

/// One rendered alignment column, 1-based coordinates (ported from GenePad's
/// `AlignmentColumn`). `ref_base`/`query_base` is `'-'` on gap columns.
/// `ref_position` of an insertion column is the subject column to its left
/// (i.e. the next subject column, 0-based); `query_position` of a deletion
/// column is 0.
#[derive(Clone, Debug, PartialEq)]
pub struct AlignColumn {
    pub ref_base: u8,
    pub query_base: u8,
    pub col_type: ColType,
    pub ref_position: u64,
    pub query_position: u64,
}

/// A rendered BLAST hit: expanded alignment columns plus spans and stats
/// (GenePad's `BlastHit`). `ref_start`/`ref_end` are 1-based inclusive;
/// query positions inside `columns` are 1-based in the read orientation the
/// hit was built from (reverse-complemented read when `strand` is `Minus`).
#[derive(Clone, Debug)]
pub struct ColumnHit {
    pub hit_index: usize,
    pub columns: Vec<AlignColumn>,
    pub ref_start: u64,
    pub ref_end: u64,
    pub score: i32,
    pub identity: f64,
    pub e_value: f64,
    pub bit_score: f64,
    pub strand: Strand,
}

/// BLAST-standard identity over columns: matches / (matches + mismatches).
/// Gap columns are excluded from the denominator, matching the "% identity"
/// NCBI BLAST reports (GenePad's `computeIdentity`).
pub fn compute_identity(columns: &[AlignColumn]) -> f64 {
    let mut matches = 0usize;
    let mut compared = 0usize;
    for col in columns {
        match col.col_type {
            ColType::Match => {
                matches += 1;
                compared += 1;
            }
            ColType::Mismatch => compared += 1,
            ColType::Insertion | ColType::Deletion => {}
        }
    }
    if compared > 0 {
        matches as f64 / compared as f64
    } else {
        0.0
    }
}

/// Raw score → (bit score, E-value) with the default blastn Karlin
/// parameters (GenePad's `score_to_stats`).
pub(crate) fn score_to_stats(score: i32, ref_len: usize, query_len: usize) -> (f64, f64) {
    let lambda = 1.33_f64;
    let k_param = 0.621_f64;
    let bit_score = (lambda * score as f64 - k_param.ln()) / 2_f64.ln();
    let e_value = k_param * ref_len as f64 * query_len as f64 * (-(lambda * score as f64)).exp();
    (bit_score, e_value)
}

/// Clean the inputs, run the full-length fast paths on both strands and
/// return the single best full-length hit, if any.
pub fn full_length_hits(ref_seq: &[u8], query_seq: &[u8]) -> Option<ColumnHit> {
    let q_clean = seqblk::to_upper_clean_bytes(query_seq);
    let s_clean = seqblk::to_upper_clean_bytes(ref_seq);
    if q_clean.is_empty() || s_clean.is_empty() {
        return None;
    }
    let q_rc = seqblk::reverse_complement(&q_clean);

    let full_fwd = full_length::try_full_length(&s_clean, &q_clean, Strand::Plus);
    let full_rev = full_length::try_full_length(&s_clean, &q_rc, Strand::Minus);
    match (full_fwd, full_rev) {
        (Some(f), Some(r)) => Some(if f.score >= r.score { f } else { r }),
        (f, r) => f.or(r),
    }
}

/// Local blastn path: engine run plus colinear hit-chain normalization of
/// the dominant strand. Inputs are raw bytes; cleaning happens inside.
pub fn local_hits(ref_seq: &[u8], query_seq: &[u8]) -> Vec<ColumnHit> {
    let q_clean = seqblk::to_upper_clean_bytes(query_seq);
    let s_clean = seqblk::to_upper_clean_bytes(ref_seq);
    if q_clean.is_empty() || s_clean.is_empty() {
        return Vec::new();
    }
    let q_rc = seqblk::reverse_complement(&q_clean);

    let result = engine::run_blastn_bytes(&q_clean, &s_clean);
    let hits = render_engine_hits(&result, &q_clean, &q_rc, &s_clean);
    normalize::normalize_blast_hits(hits, s_clean.len(), q_clean.len())
}

/// Combined GenePad `blast_align` pipeline: full-length fast paths first
/// (near-identical whole sequences, including circular rotation), then the
/// local blastn engine with colinear-hit normalization.
pub fn blast_hits(ref_seq: &[u8], query_seq: &[u8]) -> Option<Vec<ColumnHit>> {
    if let Some(hit) = full_length_hits(ref_seq, query_seq) {
        return Some(vec![hit]);
    }
    let hits = local_hits(ref_seq, query_seq);
    (!hits.is_empty()).then_some(hits)
}

/// Expand engine HSP edit blocks into `ColumnHit`s (GenePad's `blast_align`
/// column rebuild): minus-strand hits are rebuilt against the
/// reverse-complemented query so query positions ascend 5'→3' in read order.
fn render_engine_hits(
    result: &BlastResult,
    q_clean: &[u8],
    q_rc: &[u8],
    s_clean: &[u8],
) -> Vec<ColumnHit> {
    result
        .hits
        .iter()
        .map(|h| {
            let (query, q_start, strand) = if h.strand == Strand::Minus {
                (q_rc, q_clean.len().saturating_sub(h.q_end), Strand::Minus)
            } else {
                (q_clean, h.q_start, Strand::Plus)
            };
            let columns: Vec<AlignColumn> = h
                .edit_block
                .to_columns(query, s_clean, q_start, h.s_start)
                .into_iter()
                .map(|c| AlignColumn {
                    ref_base: c.ref_base,
                    query_base: c.query_base,
                    col_type: match c.col_type {
                        "match" => ColType::Match,
                        "mismatch" => ColType::Mismatch,
                        "insertion" => ColType::Insertion,
                        _ => ColType::Deletion,
                    },
                    ref_position: c.ref_position,
                    query_position: c.query_position,
                })
                .collect();
            let mut ref_start = u64::MAX;
            let mut ref_end = 0u64;
            for col in &columns {
                if col.ref_base != b'-' && col.ref_position > 0 {
                    ref_start = ref_start.min(col.ref_position);
                    ref_end = ref_end.max(col.ref_position);
                }
            }
            ColumnHit {
                hit_index: 0,
                columns,
                ref_start: ref_start.max(1),
                ref_end,
                score: h.score,
                identity: h.identity,
                e_value: h.evalue,
                bit_score: h.bit_score,
                strand,
            }
        })
        .collect()
}


#[cfg(test)]
#[path = "genepad_port_tests.rs"]
mod genepad_port_tests;
