//! Gapped X-drop affine DP (NCBI blast_gapalign.c:s_BlastAlignPackedNucl
//! equivalent). Ported from GenePad (https://genepad.cn, GenePad team).
//! Per-base three-state affine DP with traceback; unlike full-matrix SW the
//! local scores restart from 0 and the traceback yields an incremental edit
//! block.

use super::editblock::{EditBlock, EditOp};

/// Initial band width (bases either side of the diagonal). When the best
/// path touches the band edge the band widens and recomputes, degrading to
/// the full matrix when band ≥ max(m, n).
const INITIAL_BAND: usize = 64;

/// Banded local alignment with traceback from (q_start, s_start).
/// Returns (score, q_end, s_end, EditBlock) — the best end point plus the
/// edit block. query/subject are the full sequences; offsets are 0-based.
pub fn gapped_extend(
    query: &[u8],
    subject: &[u8],
    q_start: usize,
    s_start: usize,
    reward: i32,
    penalty: i32,
    gap_open: i32,
    gap_extend: i32,
    x_drop: i32,
) -> Option<(i32, usize, usize, EditBlock)> {
    let m = query.len() - q_start;
    let n = subject.len() - s_start;
    if m == 0 || n == 0 {
        return None;
    }
    // First gap cell costs open + extend (NCBI affine: open includes the
    // first extension).
    let gap_open_extend = gap_open + gap_extend;
    let x_drop = x_drop.max(0);

    let Some(full_cells) = (m + 1).checked_mul(n + 1) else {
        return None;
    };
    if full_cells > 100_000_000 {
        return None;
    }

    let mut band = INITIAL_BAND.min(m.max(n));
    loop {
        let (res, touched) = banded_sw(
            query,
            q_start,
            subject,
            s_start,
            reward,
            penalty,
            gap_open_extend,
            gap_extend,
            x_drop,
            band,
        );
        if !touched || band >= m.max(n) {
            return res;
        }
        band = (band.saturating_mul(4)).min(m.max(n));
    }
}

/// Banded affine SW (diagonal d = j - i ∈ [±band]); returns the best local
/// alignment plus whether the best path touched the band edge. Out-of-band
/// cells behave as M=0 / Ix=Iy=NEG_INF.
#[allow(clippy::too_many_arguments)]
fn banded_sw(
    query: &[u8],
    q_start: usize,
    subject: &[u8],
    s_start: usize,
    reward: i32,
    penalty: i32,
    gap_open_extend: i32,
    gap_extend: i32,
    x_drop: i32,
    band: usize,
) -> (Option<(i32, usize, usize, EditBlock)>, bool) {
    let m = query.len() - q_start;
    let n = subject.len() - s_start;
    if m == 0 || n == 0 {
        return (None, false);
    }
    let w = (2 * band + 1).min(n + 1); // slots per row: j - base(i)
    let base = |i: usize| i.saturating_sub(band); // smallest j representable in row i

    let mut prev_m = vec![0i32; w];
    let mut cur_m = vec![0i32; w];
    let mut prev_ix = vec![NEG_INF; w];
    let mut cur_ix = vec![NEG_INF; w];
    let mut prev_iy = vec![NEG_INF; w];
    let mut cur_iy = vec![NEG_INF; w];
    // trace: 0=stop, 1=diag(M), 2=up(Ix), 3=left(Iy)
    let Some(trace_len) = (m + 1).checked_mul(w) else {
        return (None, false);
    };
    let mut trace = vec![0u8; trace_len];

    let mut best_score = 0i32;
    let mut best_i = 0usize;
    let mut best_j = 0usize;

    // Out-of-band / row 0: M=0, Ix=Iy=NEG_INF (defaults when slot missing).
    let at = |arr: &[i32], base: usize, j: usize, default: i32| -> i32 {
        if j < base || j - base >= w {
            default
        } else {
            arr[j - base]
        }
    };

    for i in 1..=m {
        cur_m.fill(0);
        cur_ix.fill(NEG_INF);
        cur_iy.fill(NEG_INF);
        let b = base(i);
        let pb = base(i - 1);
        let j_lo = b.max(1);
        let j_hi = n.min(i + band);
        let qi = query[q_start + i - 1];
        for j in j_lo..=j_hi {
            let sj = subject[s_start + j - 1];
            let s = if qi == sj { reward } else { penalty };
            let diag_prev = at(&prev_m, pb, j - 1, 0)
                .max(at(&prev_ix, pb, j - 1, NEG_INF))
                .max(at(&prev_iy, pb, j - 1, NEG_INF));
            let diag = if diag_prev <= NEG_INF / 2 {
                NEG_INF
            } else {
                diag_prev + s
            };
            let m_val = 0.max(diag);

            let ix_open = at(&prev_m, pb, j, 0) - gap_open_extend;
            let ix_ext = at(&prev_ix, pb, j, NEG_INF) - gap_extend;
            let ix_val = ix_open.max(ix_ext);

            let slot_cur = j - b;
            let iy_open = if slot_cur == 0 {
                0 - gap_open_extend
            } else {
                cur_m[slot_cur - 1] - gap_open_extend
            };
            let iy_ext = if slot_cur == 0 {
                NEG_INF
            } else {
                cur_iy[slot_cur - 1] - gap_extend
            };
            let iy_val = iy_open.max(iy_ext);

            let mut best = m_val;
            let mut t = 1u8;
            if ix_val > best {
                best = ix_val;
                t = 2;
            }
            if iy_val > best {
                best = iy_val;
                t = 3;
            }
            if best <= 0 || (best_score > 0 && best_score - best > x_drop) {
                continue; // trace slot stays 0, cur slot keeps row-start defaults
            }

            cur_m[slot_cur] = m_val;
            cur_ix[slot_cur] = ix_val;
            cur_iy[slot_cur] = iy_val;
            trace[i * w + slot_cur] = t;

            if best > best_score {
                best_score = best;
                best_i = i;
                best_j = j;
            }
        }
        std::mem::swap(&mut prev_m, &mut cur_m);
        std::mem::swap(&mut prev_ix, &mut cur_ix);
        std::mem::swap(&mut prev_iy, &mut cur_iy);
    }

    if best_score <= 0 {
        return (None, false);
    }

    // Traceback from (best_i, best_j); detect whether the path touches the
    // band edge (as opposed to a sequence boundary).
    let mut touched = false;
    let mut ops_rev: Vec<EditOp> = Vec::new();
    let mut i = best_i;
    let mut j = best_j;
    while i > 0 && j > 0 {
        let b = base(i);
        if j < b || j - b >= w {
            break;
        }
        if (j == i + band && j < n) || (j == b && b > 1) {
            touched = true;
        }
        let t = trace[i * w + (j - b)];
        if t == 0 {
            break;
        }
        if t == 1 {
            push_or_merge(&mut ops_rev, EditOp::Sub(1));
            i -= 1;
            j -= 1;
        } else if t == 2 {
            push_or_merge(&mut ops_rev, EditOp::Ins(1));
            i -= 1;
        } else {
            push_or_merge(&mut ops_rev, EditOp::Del(1));
            j -= 1;
        }
    }
    ops_rev.reverse();
    let block = EditBlock { ops: ops_rev };

    let q_end = q_start + best_i;
    let s_end = s_start + best_j;
    (Some((best_score, q_end, s_end, block)), touched)
}

fn push_or_merge(ops: &mut Vec<EditOp>, op: EditOp) {
    if let Some(last) = ops.last_mut() {
        let merged = match (last, &op) {
            (EditOp::Sub(a), EditOp::Sub(b)) => {
                *a += b;
                true
            }
            (EditOp::Ins(a), EditOp::Ins(b)) => {
                *a += b;
                true
            }
            (EditOp::Del(a), EditOp::Del(b)) => {
                *a += b;
                true
            }
            _ => false,
        };
        if merged {
            return;
        }
    }
    ops.push(op);
}

const NEG_INF: i32 = -1_000_000_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_single_sub() {
        let q = b"ACGTACGTACGT";
        let s = b"ACGTACGTACGT";
        let (score, _, _, block) = gapped_extend(q, s, 0, 0, 1, -3, 5, 2, 30).unwrap();
        assert_eq!(score, 12);
        assert_eq!(block.ops, vec![EditOp::Sub(12)]);
    }

    #[test]
    fn handles_single_insertion() {
        // One extra base mid-query; long matching flanks make the gapped path
        // (24 matches - 7 gap = 17) beat any pure matching sub-segment (12).
        let q = b"ACGTACGTACGTTACGTACGTACGT";
        let s = b"ACGTACGTACGTACGTACGTACGT";
        let (_, _, _, block) = gapped_extend(q, s, 0, 0, 1, -3, 5, 2, 30).unwrap();
        let total: usize = block
            .ops
            .iter()
            .map(|o| match o {
                EditOp::Sub(n) | EditOp::Ins(n) | EditOp::Del(n) => *n,
            })
            .sum();
        assert!(total >= s.len());
        let ins: usize = block
            .ops
            .iter()
            .map(|o| match o {
                EditOp::Ins(n) => *n,
                _ => 0,
            })
            .sum();
        assert_eq!(ins, 1);
    }

    #[test]
    fn handles_single_deletion() {
        let q = b"ACGTACGTACGTACGTACGTACGT";
        let s = b"ACGTACGTACGTTACGTACGTACGT";
        let (_, _, _, block) = gapped_extend(q, s, 0, 0, 1, -3, 5, 2, 30).unwrap();
        let del: usize = block
            .ops
            .iter()
            .map(|o| match o {
                EditOp::Del(n) => *n,
                _ => 0,
            })
            .sum();
        assert_eq!(del, 1);
    }
}
