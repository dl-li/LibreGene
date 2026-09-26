//! Main pipeline orchestration (NCBI blast_engine.c:s_BlastSearchEngineCore
//! equivalent). Ported from GenePad (https://genepad.cn, GenePad team);
//! GenePad's batch/indexed entry points were not carried over.
//!
//! Flow: clean → DUST-mask query → build lookup → scan subject on both
//! strands → ungapped X-drop → (trigger reached) gapped/greedy → collect
//! HSPs → dedup → Karlin-Altschul → reap → sort → BlastResult.

use super::editblock::{EditBlock, EditOp};
use super::gapped;
use super::greedy;
use super::hsp::{BlastHSP, HSPList, Strand};
use super::lookup::LookupTable;
use super::options::BlastnOptions;
use super::scan::{self, WordHit};
use super::stat::{self, KarlinBlk};
use super::ungapped;

#[derive(Clone, Debug)]
pub struct BlastResult {
    pub hits: Vec<BlastHit>,
}

#[derive(Clone, Debug)]
pub struct BlastHit {
    pub score: i32,
    pub bit_score: f64,
    pub evalue: f64,
    pub identity: f64,
    pub q_start: usize, // 0-based, inclusive
    pub q_end: usize,   // 0-based, exclusive
    pub s_start: usize,
    pub s_end: usize,
    pub strand: Strand,
    pub edit_block: EditBlock,
    pub num_ident: usize,
}

/// Run blastn of `query` against `subject`, returning all significant hits.
pub fn run_blastn(query: &str, subject: &str, opts: &BlastnOptions) -> BlastResult {
    let q = super::seqblk::to_upper_clean(query);
    let s = super::seqblk::to_upper_clean(subject);
    run_blastn_cleaned(&q, &s, opts)
}

/// Byte entry for pre-cleaned sequences (uppercase ACGTN).
pub fn run_blastn_bytes(query: &[u8], subject: &[u8]) -> BlastResult {
    run_blastn_cleaned(query, subject, &BlastnOptions::blastn_default())
}

fn run_blastn_cleaned(q: &[u8], s: &[u8], opts: &BlastnOptions) -> BlastResult {
    if q.is_empty() || s.is_empty() || opts.word_size > q.len() {
        return BlastResult { hits: vec![] };
    }

    // Karlin parameters (unknown combinations cannot be scored → empty).
    let kb = match stat::lookup_karlin(opts.reward, opts.penalty, opts.gap_open, opts.gap_extend) {
        Some(k) => k,
        None => return BlastResult { hits: vec![] },
    };
    // Gapped X-drop converted from bits to raw score (≈ bits/λ).
    let x_drop_gapped = (opts.x_drop_gapped_bits / kb.lambda) as i32;
    let gap_trigger_score = (opts.gap_trigger_bits / kb.lambda) as i32;

    let dust_fwd = if opts.dust {
        super::dust::mask(q, 20, 64)
    } else {
        super::dust::DustMask::none()
    };

    let q_rc = super::seqblk::reverse_complement(q);
    let dust_rev = if opts.dust {
        super::dust::mask(&q_rc, 20, 64)
    } else {
        super::dust::DustMask::none()
    };

    let mut hsp_list = HSPList::new();
    extend_strand(q, s, opts, &dust_fwd, Strand::Plus, x_drop_gapped, gap_trigger_score, &mut hsp_list);
    extend_strand(
        &q_rc,
        s,
        opts,
        &dust_rev,
        Strand::Minus,
        x_drop_gapped,
        gap_trigger_score,
        &mut hsp_list,
    );

    // Statistics + pruning + sorting + reverse-strand coordinate remap.
    finalize_hsps(q.len(), s.len(), hsp_list, opts, &kb)
}

/// HSP list finalization: Karlin statistics → dedup → E-value prune → sort
/// → truncate → remap reverse-strand query coordinates to the original
/// query.
fn finalize_hsps(
    q_len: usize,
    s_len: usize,
    mut hsp_list: HSPList,
    opts: &BlastnOptions,
    kb: &KarlinBlk,
) -> BlastResult {
    let searchsp = stat::effective_searchsp(q_len, s_len, kb);
    for hsp in hsp_list.hsps.iter_mut() {
        hsp.bit_score = stat::score_to_bitscore(hsp.score, kb);
        hsp.evalue = stat::score_to_evalue(hsp.score, searchsp, kb);
    }
    hsp_list.dedup();
    hsp_list.reap_by_evalue(opts.expect_value);
    hsp_list.sort_by_score();
    hsp_list.hsps.truncate(opts.max_hsps);

    let qlen = q_len;
    let hits: Vec<BlastHit> = hsp_list
        .hsps
        .into_iter()
        .map(|h| {
            let (qs, qe) = if h.strand == Strand::Minus {
                // rc index i maps to original-query index qlen-1-i
                (qlen - h.q_end, qlen - h.q_start)
            } else {
                (h.q_start, h.q_end)
            };
            BlastHit {
                score: h.score,
                bit_score: h.bit_score,
                evalue: h.evalue,
                identity: h.identity(),
                q_start: qs,
                q_end: qe,
                s_start: h.s_start,
                s_end: h.s_end,
                strand: h.strand,
                edit_block: h.edit_block,
                num_ident: h.num_ident,
            }
        })
        .collect();

    BlastResult { hits }
}

fn extend_strand(
    query: &[u8],
    subject: &[u8],
    opts: &BlastnOptions,
    dust_mask: &super::dust::DustMask,
    strand: Strand,
    x_drop_gapped: i32,
    gap_trigger_score: i32,
    hsp_list: &mut HSPList,
) {
    let lookup = LookupTable::build(query, opts.word_size, dust_mask);
    let word_hits = scan::scan_subject(subject, &lookup);
    extend_strand_hits(
        query,
        subject,
        opts,
        opts.word_size,
        strand,
        word_hits,
        x_drop_gapped,
        gap_trigger_score,
        hsp_list,
    );
}

/// Extension stage (ungapped X-drop → gapped/greedy → HSP collection),
/// decoupled from scanning so batch entry points could reuse it. word_hits
/// must be in ascending s_off order (matching scan_subject output).
#[allow(clippy::too_many_arguments)]
fn extend_strand_hits(
    query: &[u8],
    subject: &[u8],
    opts: &BlastnOptions,
    word_size: usize,
    strand: Strand,
    word_hits: Vec<WordHit>,
    x_drop_gapped: i32,
    gap_trigger_score: i32,
    hsp_list: &mut HSPList,
) {
    // Diagonal dedup (NCBI BLAST_DiagTable): on the same diagonal, a word hit
    // falling inside an already-extended interval is skipped, avoiding
    // O(n*m) re-extension on near-identical sequences.
    let mut diag_extents: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    // Gapped window cap: the ungapped interval plus word_size, bounded so a
    // degenerate window cannot blow up.
    const MAX_GAPPED_WINDOW: usize = 8192;

    for wh in word_hits {
        let diag = wh.s_off as i64 - wh.q_off as i64;
        if let Some(&last_s) = diag_extents.get(&diag) {
            if wh.s_off < last_s {
                continue; // inside an already-extended interval
            }
        }

        let ud = match ungapped::ungapped_extend(
            query,
            subject,
            wh.q_off,
            wh.s_off,
            word_size,
            opts.reward,
            opts.penalty,
            opts.x_drop_ungapped,
        ) {
            Some(d) => d,
            None => continue,
        };
        diag_extents.insert(diag, ud.s_end + word_size);

        // Gapped extension over the ungapped interval (plus a word of
        // margin) via sub-slices starting at the interval starts.
        let win_q_end = (ud.q_end + word_size).min(query.len());
        let win_s_end = (ud.s_end + word_size).min(subject.len());
        let q_win = if win_q_end - ud.q_start > MAX_GAPPED_WINDOW {
            &query[ud.q_start..ud.q_start + MAX_GAPPED_WINDOW]
        } else {
            &query[ud.q_start..win_q_end]
        };
        let s_win = if win_s_end - ud.s_start > MAX_GAPPED_WINDOW {
            &subject[ud.s_start..ud.s_start + MAX_GAPPED_WINDOW]
        } else {
            &subject[ud.s_start..win_s_end]
        };

        let use_gapped = ud.score >= gap_trigger_score;
        if use_gapped && should_keep_ungapped_without_gapped(query, subject, &ud, q_win.len(), s_win.len()) {
            save_ungapped_hsp(query, subject, &ud, strand, hsp_list);
            continue;
        }

        let gapped = if use_gapped {
            if opts.megablast {
                greedy::greedy_extend(q_win, s_win, 0, 0, opts.reward, opts.penalty, x_drop_gapped)
            } else {
                gapped::gapped_extend(
                    q_win,
                    s_win,
                    0,
                    0,
                    opts.reward,
                    opts.penalty,
                    opts.gap_open,
                    opts.gap_extend,
                    x_drop_gapped,
                )
            }
        } else {
            None
        };

        if let Some((score, q_off_end, s_off_end, block)) = gapped {
            // Gapped offsets are sub-slice-relative; map back to full
            // sequence coordinates.
            let q_start = ud.q_start;
            let s_start = ud.s_start;
            let num_ident = block.num_ident(q_win, s_win, 0, 0);
            hsp_list.save(BlastHSP {
                score,
                bit_score: 0.0,
                evalue: f64::INFINITY,
                q_start,
                q_end: q_start + q_off_end,
                s_start,
                s_end: s_start + s_off_end,
                edit_block: block,
                num_ident,
                strand,
            });
        } else {
            save_ungapped_hsp(query, subject, &ud, strand, hsp_list);
        }
    }
}

fn should_keep_ungapped_without_gapped(
    query: &[u8],
    subject: &[u8],
    ud: &ungapped::UngappedData,
    q_window_len: usize,
    s_window_len: usize,
) -> bool {
    let len = ud.length();
    if len < 512 {
        return false;
    }

    let min_window = q_window_len.min(s_window_len);
    if min_window == 0 || len.saturating_mul(100) < min_window.saturating_mul(95) {
        return false;
    }

    let mut matches = 0usize;
    for offset in 0..len {
        if query[ud.q_start + offset] == subject[ud.s_start + offset] {
            matches += 1;
        }
    }
    matches.saturating_mul(100) >= len.saturating_mul(97)
}

fn save_ungapped_hsp(
    query: &[u8],
    subject: &[u8],
    ud: &ungapped::UngappedData,
    strand: Strand,
    hsp_list: &mut HSPList,
) {
    let len = ud.length();
    let block = EditBlock {
        ops: vec![EditOp::Sub(len)],
    };
    let num_ident = block.num_ident(query, subject, ud.q_start, ud.s_start);
    hsp_list.save(BlastHSP {
        score: ud.score,
        bit_score: 0.0,
        evalue: f64::INFINITY,
        q_start: ud.q_start,
        q_end: ud.q_end,
        s_start: ud.s_start,
        s_end: ud.s_end,
        edit_block: block,
        num_ident,
        strand,
    });
}
