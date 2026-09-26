//! Full-length fast paths for near-identical whole-sequence alignment.
//! Ported from GenePad (https://genepad.cn, GenePad team): a long read that
//! covers (nearly) the whole template aligns in one pass — ungapped when
//! lengths match, banded global Gotoh when they differ by indels, and a
//! rotation search when the query is a rotated copy of a circular
//! reference. Local blastn would otherwise fragment such pairs into
//! multiple HSPs with spurious large indels between them.

use super::hsp::Strand;
use super::{compute_identity, score_to_stats, AlignColumn, ColType, ColumnHit};

const BLAST_MATCH: i32 = 1; // NCBI blastn default reward=1
const BLAST_MISMATCH: i32 = -3; // NCBI blastn default penalty=-3
const BLAST_GAP_OPEN: i32 = -5; // NCBI blastn default gap_open=5
const BLAST_GAP_EXT: i32 = -2; // NCBI blastn default gap_ext=2
const FULL_LENGTH_MIN_IDENTITY: f64 = 0.85;
const FULL_LENGTH_MIN_LENGTH_RATIO: f64 = 0.80;
// Cap on band-local cells (1 byte/cell trace) for the full-length banded
// path; band width is |delta| + 2*band + 1, so this bounds memory at ~50 MB
// and keeps multi-kb near-identical pairs in one pass without OOM.
const FULL_LENGTH_MAX_BAND_CELLS: usize = 50_000_000;
const ROTATION_WORD_SIZE: usize = 20;
const ROTATION_MAX_CANDIDATES: usize = 32;
const NEG_INF: i32 = -1_000_000_000;

/// Entry: one full-length hit when the pair is near-identical, else `None`.
pub fn try_full_length(ref_seq: &[u8], query_seq: &[u8], strand: Strand) -> Option<ColumnHit> {
    full_length_ungapped_hit(ref_seq, query_seq, strand)
        .or_else(|| full_length_banded_hit(ref_seq, query_seq, strand))
        .or_else(|| full_length_rotated_hit(ref_seq, query_seq, strand))
}

fn build_hit_from_columns(
    columns: Vec<AlignColumn>,
    score: i32,
    ref_len: usize,
    query_len: usize,
    strand: Strand,
) -> Option<ColumnHit> {
    let mut ref_start = u64::MAX;
    let mut ref_end = 0u64;
    for col in &columns {
        if col.ref_base != b'-' && col.ref_position > 0 {
            ref_start = ref_start.min(col.ref_position);
            ref_end = ref_end.max(col.ref_position);
        }
    }
    if ref_start == u64::MAX {
        return None;
    }

    let identity = compute_identity(&columns);
    let (bit_score, e_value) = score_to_stats(score, ref_len, query_len);
    Some(ColumnHit {
        hit_index: 0,
        columns,
        ref_start,
        ref_end,
        score,
        identity,
        e_value,
        bit_score,
        strand,
    })
}

fn count_equal_bases(a: &[u8], b: &[u8]) -> usize {
    debug_assert_eq!(a.len(), b.len());

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx2") {
            return unsafe { count_equal_bases_avx2(a, b) };
        }
        if std::is_x86_feature_detected!("sse2") {
            return unsafe { count_equal_bases_sse2(a, b) };
        }
    }

    count_equal_bases_scalar(a, b)
}

fn count_equal_bases_scalar(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).filter(|(left, right)| left == right).count()
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn count_equal_bases_avx2(a: &[u8], b: &[u8]) -> usize {
    use std::arch::x86_64::{__m256i, _mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8};

    let mut count = 0usize;
    let mut i = 0usize;
    while i + 32 <= a.len() {
        let left = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
        let right = _mm256_loadu_si256(b.as_ptr().add(i) as *const __m256i);
        let eq = _mm256_cmpeq_epi8(left, right);
        count += (_mm256_movemask_epi8(eq) as u32).count_ones() as usize;
        i += 32;
    }
    count + count_equal_bases_sse2(&a[i..], &b[i..])
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "avx2")]
unsafe fn count_equal_bases_avx2(a: &[u8], b: &[u8]) -> usize {
    use std::arch::x86::{__m256i, _mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8};

    let mut count = 0usize;
    let mut i = 0usize;
    while i + 32 <= a.len() {
        let left = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
        let right = _mm256_loadu_si256(b.as_ptr().add(i) as *const __m256i);
        let eq = _mm256_cmpeq_epi8(left, right);
        count += (_mm256_movemask_epi8(eq) as u32).count_ones() as usize;
        i += 32;
    }
    count + count_equal_bases_sse2(&a[i..], &b[i..])
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn count_equal_bases_sse2(a: &[u8], b: &[u8]) -> usize {
    use std::arch::x86_64::{_mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, __m128i};

    let mut count = 0usize;
    let mut i = 0usize;
    while i + 16 <= a.len() {
        let left = _mm_loadu_si128(a.as_ptr().add(i) as *const __m128i);
        let right = _mm_loadu_si128(b.as_ptr().add(i) as *const __m128i);
        let eq = _mm_cmpeq_epi8(left, right);
        count += (_mm_movemask_epi8(eq) as u32).count_ones() as usize;
        i += 16;
    }
    count + count_equal_bases_scalar(&a[i..], &b[i..])
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "sse2")]
unsafe fn count_equal_bases_sse2(a: &[u8], b: &[u8]) -> usize {
    use std::arch::x86::{_mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, __m128i};

    let mut count = 0usize;
    let mut i = 0usize;
    while i + 16 <= a.len() {
        let left = _mm_loadu_si128(a.as_ptr().add(i) as *const __m128i);
        let right = _mm_loadu_si128(b.as_ptr().add(i) as *const __m128i);
        let eq = _mm_cmpeq_epi8(left, right);
        count += (_mm_movemask_epi8(eq) as u32).count_ones() as usize;
        i += 16;
    }
    count + count_equal_bases_scalar(&a[i..], &b[i..])
}

fn full_length_ungapped_hit(ref_seq: &[u8], query_seq: &[u8], strand: Strand) -> Option<ColumnHit> {
    if ref_seq.len() != query_seq.len() || ref_seq.is_empty() {
        return None;
    }

    let matches = count_equal_bases(ref_seq, query_seq);
    let identity = matches as f64 / ref_seq.len() as f64;
    if identity < FULL_LENGTH_MIN_IDENTITY {
        return None;
    }

    let mismatches = ref_seq.len() - matches;
    let score = matches as i32 * BLAST_MATCH + mismatches as i32 * BLAST_MISMATCH;
    let mut columns = Vec::with_capacity(ref_seq.len());
    for i in 0..ref_seq.len() {
        let is_match = ref_seq[i] == query_seq[i];
        columns.push(AlignColumn {
            ref_base: ref_seq[i],
            query_base: query_seq[i],
            col_type: if is_match { ColType::Match } else { ColType::Mismatch },
            ref_position: (i + 1) as u64,
            query_position: (i + 1) as u64,
        });
    }

    build_hit_from_columns(columns, score, ref_seq.len(), query_seq.len(), strand)
}

fn full_length_banded_hit(ref_seq: &[u8], query_seq: &[u8], strand: Strand) -> Option<ColumnHit> {
    let n = ref_seq.len();
    let m = query_seq.len();
    if n == 0 || m == 0 || n == m || !full_length_lengths_are_close(n, m) {
        return None;
    }

    let delta = n as isize - m as isize;
    let band = (delta.unsigned_abs() + 128).clamp(256, 4096);
    // A row's active window spans [max(0, i + min(0,delta) - band),
    // min(n, i + max(0,delta) + band)] — up to |delta| + 2*band + 1 columns
    // wide. Per-row storage must cover that full width so the band never
    // overruns its reserved memory (a 2*band+1 underallocation overflows
    // into the next row and past the vector end on the last row).
    let band_width = delta.unsigned_abs() + 2 * band + 1;
    if (m + 1).saturating_mul(band_width) > FULL_LENGTH_MAX_BAND_CELLS {
        return None;
    }

    let open_extend = BLAST_GAP_OPEN + BLAST_GAP_EXT;
    let row_bounds = |i: usize| -> (usize, usize) {
        let lower = (i as isize + delta.min(0) - band as isize).max(0) as usize;
        let upper = (i as isize + delta.max(0) + band as isize).min(n as isize) as usize;
        (lower, upper)
    };
    let trace_idx = |i: usize, j: usize, lower: usize| -> usize { i * band_width + (j - lower) };

    let mut prev_m = vec![NEG_INF; n + 1];
    let mut cur_m = vec![NEG_INF; n + 1];
    let mut prev_ix = vec![NEG_INF; n + 1];
    let mut cur_ix = vec![NEG_INF; n + 1];
    let mut prev_iy = vec![NEG_INF; n + 1];
    let mut cur_iy = vec![NEG_INF; n + 1];
    prev_m[0] = 0;

    // Compact band-local trace: 1 byte per active cell. traceStart[i] is the
    // flat index of row i's first active column (j == lower_i):
    //   1 = M (diagonal)  2/3 = Ix opened/extended  4/5 = Iy opened/extended
    let mut trace = vec![0_i8; (m + 1) * band_width];
    let (first_lower, first_upper) = row_bounds(0);
    for j in 1..=first_upper {
        prev_iy[j] = (if j == 1 { prev_m[0] } else { prev_iy[j - 1] })
            + if j == 1 { open_extend } else { BLAST_GAP_EXT };
        trace[trace_idx(0, j, first_lower)] = if j == 1 { 4 } else { 5 };
    }

    for i in 1..=m {
        cur_m.fill(NEG_INF);
        cur_ix.fill(NEG_INF);
        cur_iy.fill(NEG_INF);
        let (lower, upper) = row_bounds(i);
        if lower == 0 {
            cur_ix[0] = (if i == 1 { prev_m[0] } else { prev_ix[0] })
                + if i == 1 { open_extend } else { BLAST_GAP_EXT };
            trace[trace_idx(i, 0, lower)] = if i == 1 { 2 } else { 3 };
        }

        for j in lower.max(1)..=upper {
            let s = if query_seq[i - 1] == ref_seq[j - 1] {
                BLAST_MATCH
            } else {
                BLAST_MISMATCH
            };

            cur_m[j] = prev_m[j - 1].max(prev_ix[j - 1]).max(prev_iy[j - 1]) + s;

            let ix_open = prev_m[j] + open_extend;
            let ix_ext = prev_ix[j] + BLAST_GAP_EXT;
            cur_ix[j] = ix_open.max(ix_ext);
            let ix_code = if ix_open >= ix_ext { 2 } else { 3 };

            let iy_open = cur_m[j - 1] + open_extend;
            let iy_ext = cur_iy[j - 1] + BLAST_GAP_EXT;
            cur_iy[j] = iy_open.max(iy_ext);
            let iy_code = if iy_open >= iy_ext { 4 } else { 5 };

            let code = if cur_ix[j] > cur_m[j] && cur_ix[j] >= cur_iy[j] {
                ix_code
            } else if cur_iy[j] > cur_m[j] && cur_iy[j] > cur_ix[j] {
                iy_code
            } else {
                1
            };
            trace[trace_idx(i, j, lower)] = code;
        }

        std::mem::swap(&mut prev_m, &mut cur_m);
        std::mem::swap(&mut prev_ix, &mut cur_ix);
        std::mem::swap(&mut prev_iy, &mut cur_iy);
    }

    // After the loop the "prev*" rows hold row m; pick the best ending
    // matrix at [m][n].
    let end_score = prev_m[n].max(prev_ix[n]).max(prev_iy[n]);
    if end_score <= NEG_INF / 2 {
        return None;
    }

    let mut columns = Vec::with_capacity(n.max(m));
    let mut i = m;
    let mut j = n;
    while i > 0 || j > 0 {
        if i == 0 {
            // No query left: remaining ref is a leading deletion run.
            columns.push(AlignColumn {
                ref_base: ref_seq[j - 1],
                query_base: b'-',
                col_type: ColType::Deletion,
                ref_position: j as u64,
                query_position: 0,
            });
            j -= 1;
            continue;
        }
        if j == 0 {
            // No ref left: remaining query is a leading insertion run.
            columns.push(AlignColumn {
                ref_base: b'-',
                query_base: query_seq[i - 1],
                col_type: ColType::Insertion,
                ref_position: 0,
                query_position: i as u64,
            });
            i -= 1;
            continue;
        }

        let (lower, upper) = row_bounds(i);
        let in_band = j >= lower && j <= upper;
        let t = if in_band { trace[trace_idx(i, j, lower)] } else { 1 };
        match t {
            2 | 3 => {
                columns.push(AlignColumn {
                    ref_base: b'-',
                    query_base: query_seq[i - 1],
                    col_type: ColType::Insertion,
                    ref_position: j as u64,
                    query_position: i as u64,
                });
                i -= 1;
            }
            4 | 5 => {
                columns.push(AlignColumn {
                    ref_base: ref_seq[j - 1],
                    query_base: b'-',
                    col_type: ColType::Deletion,
                    ref_position: j as u64,
                    query_position: 0,
                });
                j -= 1;
            }
            _ => {
                let is_match = ref_seq[j - 1] == query_seq[i - 1];
                columns.push(AlignColumn {
                    ref_base: ref_seq[j - 1],
                    query_base: query_seq[i - 1],
                    col_type: if is_match { ColType::Match } else { ColType::Mismatch },
                    ref_position: j as u64,
                    query_position: i as u64,
                });
                i -= 1;
                j -= 1;
            }
        }
    }
    columns.reverse();

    let hit = build_hit_from_columns(columns, end_score, n, m, strand)?;
    (hit.identity >= FULL_LENGTH_MIN_IDENTITY).then_some(hit)
}

fn rotate_query_columns(mut hit: ColumnHit, offset: usize, query_len: usize) -> ColumnHit {
    if offset == 0 {
        return hit;
    }
    for col in &mut hit.columns {
        if col.query_position > 0 {
            col.query_position = ((offset as u64 + col.query_position - 1) % query_len as u64) + 1;
        }
    }
    hit
}

fn find_rotation_offsets(ref_seq: &[u8], query_seq: &[u8]) -> Vec<usize> {
    if ref_seq.len() < ROTATION_WORD_SIZE
        || query_seq.len() < ROTATION_WORD_SIZE
        || !full_length_lengths_are_close(ref_seq.len(), query_seq.len())
    {
        return Vec::new();
    }

    let mut doubled = Vec::with_capacity(query_seq.len() + ROTATION_WORD_SIZE - 1);
    doubled.extend_from_slice(query_seq);
    doubled.extend_from_slice(&query_seq[..ROTATION_WORD_SIZE - 1]);

    let mut ref_positions = vec![
        0,
        ref_seq.len() / 4,
        ref_seq.len() / 2,
        ref_seq.len() * 3 / 4,
        ref_seq.len().saturating_sub(ROTATION_WORD_SIZE),
    ];
    ref_positions.sort_unstable();
    ref_positions.dedup();
    ref_positions.retain(|pos| *pos <= ref_seq.len() - ROTATION_WORD_SIZE);

    let mut offsets = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for ref_pos in ref_positions {
        let word = &ref_seq[ref_pos..ref_pos + ROTATION_WORD_SIZE];
        let mut from = 0usize;
        while offsets.len() < ROTATION_MAX_CANDIDATES && from + ROTATION_WORD_SIZE <= doubled.len() {
            let found = doubled[from..]
                .windows(ROTATION_WORD_SIZE)
                .position(|candidate| candidate == word)
                .map(|pos| pos + from);
            let Some(found) = found else {
                break;
            };
            if found >= query_seq.len() {
                break;
            }
            let offset = (found + query_seq.len() - (ref_pos % query_seq.len())) % query_seq.len();
            if seen.insert(offset) {
                offsets.push(offset);
            }
            from = found + 1;
        }
        if offsets.len() >= ROTATION_MAX_CANDIDATES {
            break;
        }
    }
    offsets
}

fn full_length_rotated_hit(ref_seq: &[u8], query_seq: &[u8], strand: Strand) -> Option<ColumnHit> {
    if query_seq.is_empty() {
        return None;
    }

    let mut best: Option<ColumnHit> = None;
    for offset in find_rotation_offsets(ref_seq, query_seq) {
        if offset == 0 {
            continue;
        }
        let mut rotated = Vec::with_capacity(query_seq.len());
        rotated.extend_from_slice(&query_seq[offset..]);
        rotated.extend_from_slice(&query_seq[..offset]);

        let hit = full_length_ungapped_hit(ref_seq, &rotated, strand)
            .or_else(|| full_length_banded_hit(ref_seq, &rotated, strand));
        if let Some(hit) = hit {
            let adjusted = rotate_query_columns(hit, offset, query_seq.len());
            if best.as_ref().is_none_or(|current| adjusted.score > current.score) {
                best = Some(adjusted);
            }
        }
    }
    best
}

fn full_length_lengths_are_close(ref_len: usize, query_len: usize) -> bool {
    let shorter = ref_len.min(query_len) as f64;
    let longer = ref_len.max(query_len) as f64;
    longer > 0.0 && shorter / longer >= FULL_LENGTH_MIN_LENGTH_RATIO
}
