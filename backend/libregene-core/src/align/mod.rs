//! Smith–Waterman local alignment of a read against the project sequence.
//!
//! Scoring: match +2, mismatch −1, linear gap −2. Non-ACGT bases never
//! match. Circular templates are aligned as template concatenated with
//! itself, then coordinates are mapped back via `% tlen` and the aligned
//! range is split into non-wrapping segments at the origin.

use crate::models::{
    AlignDeletion, AlignInsertion, AlignInsertionDetail, AlignMismatch, AlignSegment, Alignment,
    AlignmentDiff,
};

const MATCH: i32 = 2;
const MISMATCH: i32 = -1;
const GAP: i32 = -2;

/// Minimum identity (fraction) for an alignment to be kept.
pub const MIN_IDENTITY: f64 = 0.6;
/// Minimum aligned span (template positions) for an alignment to be kept.
pub const MIN_ALIGNED_LEN: usize = 50;
/// Minimum aligned span for a secondary flank of a split-read alignment.
const MIN_FLANK_LEN: usize = 20;
/// Minimum identity for a secondary flank (it extends an already-accepted
/// primary alignment, so the bar is higher than [`MIN_IDENTITY`]).
const MIN_FLANK_IDENTITY: f64 = 0.75;
/// A run of this many non-match columns within [`GARBAGE_WINDOW`] marks a
/// garbage stretch: SW bridged a template region that is absent from the
/// read (split read) with mismatches and gaps.
const GARBAGE_CLUSTER: usize = 6;
const GARBAGE_WINDOW: usize = 50;

/// Why `align_read_checked` rejected an alignment candidate.
#[derive(Debug, Clone, PartialEq)]
pub enum AlignReject {
    /// No local alignment with a positive score.
    NoSignificantAlignment,
    /// Aligned span below [`MIN_ALIGNED_LEN`].
    TooShort { span: usize },
    /// Identity below [`MIN_IDENTITY`].
    LowIdentity { identity: f64, span: usize },
}

/// Traceback-matrix cell budget for a full-template Smith–Waterman run.
/// Above this, alignment goes through seed-and-extend instead. 16M cells
/// keeps debug builds responsive (~0.5 s) while small plasmid × Sanger-read
/// cases stay on the exact full-matrix path.
const FULL_SW_CELL_CAP: u64 = 16_000_000;
/// Cell budget for the banded SW around a seeded diagonal. Caps the band
/// width for long reads so the banded pass stays fast in debug builds.
const MAX_BANDED_CELLS: usize = 8_000_000;
/// Exact-match anchor length for seed-and-extend.
const SEED_K: usize = 15;
/// Seeds more frequent than this are treated as repetitive and ignored.
const MAX_SEED_HITS: usize = 100;

fn matches_base(a: u8, b: u8) -> bool {
    a == b && matches!(a, b'A' | b'C' | b'G' | b'T')
}

/// 2-bit encode a k-mer; `None` if any base is not ACGT.
fn encode_kmer(s: &[u8]) -> Option<u64> {
    let mut v = 0u64;
    for &b in s {
        let bits = match b {
            b'A' => 0,
            b'C' => 1,
            b'G' => 2,
            b'T' => 3,
            _ => return None,
        };
        v = (v << 2) | bits;
    }
    Some(v)
}

struct SwResult {
    /// Best local score (from the DP).
    score: i32,
    /// Aligned template chars ('-' = gap in template), 5'→3'.
    t_aln: Vec<u8>,
    /// Aligned read chars ('-' = gap in read), 5'→3'.
    r_aln: Vec<u8>,
    /// Template coordinates (in the possibly doubled template) of the
    /// first and last template-consuming column, inclusive.
    t_start: usize,
    t_end: usize,
    /// Read coordinates of the first and last read base consumed by the
    /// traceback, inclusive.
    r_start: usize,
    r_end: usize,
}

fn smith_waterman(t: &[u8], r: &[u8]) -> Option<SwResult> {
    let n = t.len();
    let m = r.len();
    if n == 0 || m == 0 {
        return None;
    }

    // H[i][j]: best local score ending with read prefix i / template prefix j.
    // trace: 0 = stop, 1 = diag, 2 = up (read char vs template gap), 3 = left (template char vs read gap).
    let width = n + 1;
    let mut trace = vec![0u8; width * (m + 1)];
    let mut prev = vec![0i32; width];
    let mut cur = vec![0i32; width];
    let mut best = 0i32;
    let (mut bi, mut bj) = (0usize, 0usize);

    for i in 1..=m {
        cur[0] = 0;
        for j in 1..=n {
            let s = if matches_base(r[i - 1], t[j - 1]) { MATCH } else { MISMATCH };
            let diag = prev[j - 1] + s;
            let up = prev[j] + GAP;
            let left = cur[j - 1] + GAP;
            let mut v = 0i32;
            let mut dir = 0u8;
            if diag > v {
                v = diag;
                dir = 1;
            }
            if up > v {
                v = up;
                dir = 2;
            }
            if left > v {
                v = left;
                dir = 3;
            }
            cur[j] = v;
            trace[i * width + j] = dir;
            if v > best {
                best = v;
                bi = i;
                bj = j;
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }

    if best <= 0 {
        return None;
    }

    let mut t_aln = Vec::new();
    let mut r_aln = Vec::new();
    let (mut i, mut j) = (bi, bj);
    while i > 0 && j > 0 {
        match trace[i * width + j] {
            1 => {
                t_aln.push(t[j - 1]);
                r_aln.push(r[i - 1]);
                i -= 1;
                j -= 1;
            }
            2 => {
                t_aln.push(b'-');
                r_aln.push(r[i - 1]);
                i -= 1;
            }
            3 => {
                t_aln.push(t[j - 1]);
                r_aln.push(b'-');
                j -= 1;
            }
            _ => break,
        }
    }
    t_aln.reverse();
    r_aln.reverse();

    Some(SwResult {
        score: best,
        t_aln,
        r_aln,
        t_start: j,
        t_end: bj - 1,
        r_start: i,
        r_end: bi - 1,
    })
}

/// Dispatch: full-matrix SW when the traceback matrix fits the cell budget,
/// otherwise seed-and-extend (exact k-mer anchors → windowed SW).
fn sw_dispatch(t: &[u8], r: &[u8]) -> Option<SwResult> {
    let cells = (t.len() as u64 + 1) * (r.len() as u64 + 1);
    if cells <= FULL_SW_CELL_CAP {
        smith_waterman(t, r)
    } else {
        smith_waterman_seeded(t, r)
    }
}

/// Banded Smith–Waterman around diagonal `diag` (template_pos − read_pos):
/// only cells with |j − (i + diag)| ≤ band are computed, everything else
/// floors to 0. Returns the best in-band alignment plus an `edge` flag;
/// `edge = true` means the best cell or its traceback runs along the band
/// boundary, so the true optimum may lie outside the band and the caller
/// should widen (windowed full SW) instead of trusting this result.
fn smith_waterman_banded(t: &[u8], r: &[u8], diag: i64, band: usize) -> (Option<SwResult>, bool) {
    let n = t.len();
    let m = r.len();
    let width = 2 * band + 1;
    let lo_of = |i: usize| ((diag + i as i64 - band as i64).max(1)) as usize;
    let hi_of = |i: usize| ((diag + i as i64 + band as i64).clamp(0, n as i64)) as usize;

    // prev/cur are indexed by absolute template column; only the band
    // window [lo, hi] is written per row and the left fringe is zeroed, so
    // out-of-band reads always see 0 (the window shifts right by ≤ 1/row).
    let mut trace = vec![0u8; width * (m + 1)];
    let mut prev = vec![0i32; n + 1];
    let mut cur = vec![0i32; n + 1];
    let mut best = 0i32;
    let (mut bi, mut bj) = (0usize, 0usize);
    let mut edge = false;

    for i in 1..=m {
        let lo = lo_of(i);
        let hi = hi_of(i);
        if lo > hi {
            continue;
        }
        cur[lo - 1] = 0;
        for j in lo..=hi {
            let s = if matches_base(r[i - 1], t[j - 1]) { MATCH } else { MISMATCH };
            let diag_s = prev[j - 1] + s;
            let up = prev[j] + GAP;
            let left = cur[j - 1] + GAP;
            let mut v = 0i32;
            let mut dir = 0u8;
            if diag_s > v {
                v = diag_s;
                dir = 1;
            }
            if up > v {
                v = up;
                dir = 2;
            }
            if left > v {
                v = left;
                dir = 3;
            }
            cur[j] = v;
            trace[i * width + (j - lo)] = dir;
            if v > best {
                best = v;
                bi = i;
                bj = j;
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }

    if best <= 0 {
        return (None, true);
    }

    let mut t_aln = Vec::new();
    let mut r_aln = Vec::new();
    let (mut i, mut j) = (bi, bj);
    while i > 0 && j > 0 {
        let lo = lo_of(i);
        let c = j.wrapping_sub(lo);
        if c >= width {
            edge = true;
            break;
        }
        if c == 0 || c == width - 1 {
            edge = true;
        }
        match trace[i * width + c] {
            1 => {
                t_aln.push(t[j - 1]);
                r_aln.push(r[i - 1]);
                i -= 1;
                j -= 1;
            }
            2 => {
                t_aln.push(b'-');
                r_aln.push(r[i - 1]);
                i -= 1;
            }
            3 => {
                t_aln.push(t[j - 1]);
                r_aln.push(b'-');
                j -= 1;
            }
            _ => break,
        }
    }
    // Best cell sitting on the band boundary is also untrusted.
    if (bj as i64 - (diag + bi as i64)).unsigned_abs() as usize >= band {
        edge = true;
    }
    t_aln.reverse();
    r_aln.reverse();

    (
        Some(SwResult {
            score: best,
            t_aln,
            r_aln,
            t_start: j,
            t_end: bj - 1,
            r_start: i,
            r_end: bi - 1,
        }),
        edge,
    )
}

/// Seed-and-extend for matrices too large for a full traceback: index
/// template k-mers, vote on the implied diagonal (template_pos − read_pos),
/// then run banded SW around the top diagonals, widening to a read-length
/// windowed full SW when the band is too narrow (indels shift the diagonal
/// mid-read). Returns `None` when no anchor exists (divergent read).
fn smith_waterman_seeded(t: &[u8], r: &[u8]) -> Option<SwResult> {
    use std::collections::HashMap;
    let n = t.len();
    let m = r.len();
    if n < SEED_K || m < SEED_K {
        return None;
    }

    let mut index: HashMap<u64, Vec<u32>> = HashMap::with_capacity(n / 2);
    for j in 0..=(n - SEED_K) {
        if let Some(k) = encode_kmer(&t[j..j + SEED_K]) {
            index.entry(k).or_default().push(j as u32);
        }
    }

    let mut votes: HashMap<i64, u32> = HashMap::new();
    for i in 0..=(m - SEED_K) {
        if let Some(kmer) = encode_kmer(&r[i..i + SEED_K]) {
            if let Some(positions) = index.get(&kmer) {
                if positions.len() > MAX_SEED_HITS {
                    continue;
                }
                for &p in positions {
                    *votes.entry(p as i64 - i as i64).or_default() += 1;
                }
            }
        }
    }

    // Deterministic order: vote count desc, then diagonal asc (ties are
    // common on repetitive templates; HashMap iteration order is not stable).
    let mut diagonals: Vec<(i64, u32)> = votes.into_iter().collect();
    diagonals.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let band = ((m / 8) + 64).min((MAX_BANDED_CELLS / (2 * m + 1)).max(64));
    // Good enough that no other diagonal is worth trying (≈ ≥90% identity).
    let good_enough = MATCH * m as i32 * 9 / 10;
    let mut best: Option<SwResult> = None;
    let mut tried: Vec<i64> = Vec::new();
    let mut edged: Vec<i64> = Vec::new();
    for (diag, _) in diagonals.iter().take(8) {
        // Diagonals within one band of a tried one cover the same cells.
        if tried.iter().any(|d| (d - diag).abs() <= band as i64) {
            continue;
        }
        tried.push(*diag);
        let (res, edge) = smith_waterman_banded(t, r, *diag, band);
        match (res, edge) {
            (Some(sw), false) => {
                if sw.score >= good_enough {
                    return Some(sw);
                }
                if best.as_ref().is_none_or(|b| sw.score > b.score) {
                    best = Some(sw);
                }
            }
            (Some(_), true) | (None, true) => edged.push(*diag),
            (None, false) => {}
        }
    }
    if let Some(sw) = best {
        return Some(sw);
    }

    // Every anchored diagonal hit the band edge (or none scored): widen to a
    // windowed full SW. The margin must absorb the indel-driven diagonal shift.
    let margin = (m / 4 + 32) as i64;
    for diag in edged.into_iter().take(3) {
        let start = (diag - margin).max(0) as usize;
        let end = ((diag + m as i64 + margin).min(n as i64)).max(start as i64) as usize;
        if let Some(mut sw) = smith_waterman(&t[start..end], r) {
            sw.t_start += start;
            sw.t_end += start;
            return Some(sw);
        }
    }
    None
}

/// A [`SwResult`] clipped and rendered into an [`Alignment`], keeping the
/// counts needed to merge several blocks (split-read flanks) afterwards.
struct BuiltAln {
    aln: Alignment,
    matched: usize,
    /// Clipped aligned columns (template-consuming + internal read-only).
    cols: usize,
    /// Clipped template span in the possibly doubled template, inclusive.
    t_start: usize,
    t_end: usize,
    /// Read interval consumed by the traceback, inclusive.
    r_start: usize,
    r_end: usize,
}

/// Build the render-oriented [`Alignment`] from a traceback, or the reason
/// the alignment is too weak to be meaningful.
#[allow(clippy::too_many_arguments)]
fn build_alignment(
    sw: &SwResult,
    oriented_read: String,
    strand: &str,
    read_len: usize,
    tlen: usize,
    min_identity: f64,
    min_span: usize,
) -> Result<BuiltAln, AlignReject> {
    // Clip read-only overhang: leading/trailing columns that don't consume template.
    let lead = sw.t_aln.iter().take_while(|&&c| c == b'-').count();
    let trail = sw.t_aln.iter().rev().take_while(|&&c| c == b'-').count();
    let cols = &sw.t_aln[lead..sw.t_aln.len() - trail];
    let rcols = &sw.r_aln[lead..sw.r_aln.len() - trail];
    if cols.is_empty() {
        return Err(AlignReject::NoSignificantAlignment);
    }

    let t_start = sw.t_start + lead;
    let t_end = sw.t_end - trail;
    let span = t_end - t_start + 1;

    let matched = cols
        .iter()
        .zip(rcols)
        .filter(|(&t, &r)| t != b'-' && r != b'-' && matches_base(t, r))
        .count();
    let identity = matched as f64 / cols.len() as f64;

    if identity < min_identity {
        return Err(AlignReject::LowIdentity { identity, span });
    }
    if span < min_span {
        return Err(AlignReject::TooShort { span });
    }

    let mut segments: Vec<AlignSegment> = Vec::new();
    let mut insertions: Vec<AlignInsertion> = Vec::new();
    let mut consumed = 0usize; // template columns consumed so far within the span
    let mut pending_ins = String::new();

    for (&t, &r) in cols.iter().zip(rcols) {
        if t == b'-' {
            // Gap in template: extra read bases, attached to the next template column.
            pending_ins.push(r as char);
            continue;
        }
        let mapped = (t_start + consumed) % tlen;
        if !pending_ins.is_empty() {
            insertions.push(AlignInsertion {
                pos: mapped,
                bases: std::mem::take(&mut pending_ins),
            });
        }
        let split = segments
            .last()
            .is_some_and(|s: &AlignSegment| mapped <= s.end);
        if split || segments.is_empty() {
            segments.push(AlignSegment {
                start: mapped,
                end: mapped,
                chars: String::new(),
            });
        }
        let seg = segments.last_mut().unwrap();
        seg.end = mapped;
        seg.chars.push(r as char);
        consumed += 1;
    }
    // Trailing read-only columns were clipped; any leftover pending insertion is dropped.

    Ok(BuiltAln {
        aln: Alignment {
            id: String::new(),
            name: String::new(),
            length: read_len,
            strand: strand.to_string(),
            identity,
            segments,
            insertions,
            seq: oriented_read,
            trace_path: None,
        },
        matched,
        cols: cols.len(),
        t_start,
        t_end,
        r_start: sw.r_start,
        r_end: sw.r_end,
    })
}

struct FlankHit {
    flank: BuiltAln,
    downstream: bool,
    /// Read bases between the two aligned blocks (oriented coords).
    junction: String,
    /// Template column the second block (in read order) resumes at.
    junction_pos: usize,
}

/// Try to align a leftover read piece (the part the primary block did not
/// consume) as a second flank against the template arc the primary block
/// left unused.
#[allow(clippy::too_many_arguments)]
fn find_flank(
    t2: &[u8],
    piece: &[u8],
    piece_offset: usize,
    downstream: bool,
    primary: &BuiltAln,
    oriented: &str,
    strand: &str,
    read_len: usize,
    tlen: usize,
) -> Option<FlankHit> {
    if piece.len() < MIN_FLANK_LEN {
        return None;
    }
    let sw = sw_dispatch(t2, piece)?;
    let flank = build_alignment(
        &sw,
        oriented.to_string(),
        strand,
        read_len,
        tlen,
        MIN_FLANK_IDENTITY,
        MIN_FLANK_LEN,
    )
    .ok()?;
    // The flank must sit on the template arc the primary block did not cover.
    // On a circular template SW may report the flank in either copy of the
    // doubled template, so shift it by whole template lengths onto the copy
    // that lies on the expected side of the primary block.
    if downstream {
        let k = (primary.t_end + 1).saturating_sub(flank.t_start).div_ceil(tlen);
        if flank.t_start + k * tlen <= primary.t_end || flank.t_end + k * tlen >= primary.t_start + tlen {
            return None;
        }
    } else {
        let k = primary.t_start.saturating_sub(flank.t_end + 1) / tlen;
        if flank.t_end + k * tlen >= primary.t_start || flank.t_start + (k + 1) * tlen <= primary.t_end {
            return None;
        }
    }

    // Read bases between the two blocks (insert, plus any unalignable
    // low-quality bases) become an insertion at the junction.
    let junction = if downstream {
        &oriented[primary.r_end + 1..piece_offset + flank.r_start]
    } else {
        &oriented[piece_offset + flank.r_end + 1..primary.r_start]
    };
    Some(FlankHit {
        junction_pos: if downstream { flank.t_start % tlen } else { primary.t_start % tlen },
        flank,
        downstream,
        junction: junction.to_string(),
    })
}

fn merge_flank(primary: BuiltAln, hit: FlankHit) -> Alignment {
    let (first, second) = if hit.downstream {
        (primary, hit.flank)
    } else {
        (hit.flank, primary)
    };
    let matched = first.matched + second.matched;
    let cols = first.cols + second.cols + hit.junction.len();

    let BuiltAln { aln: mut merged, .. } = first;
    let second = second.aln;
    merged.identity = matched as f64 / cols as f64;
    if !hit.junction.is_empty() {
        merged.insertions.push(AlignInsertion {
            pos: hit.junction_pos,
            bases: hit.junction,
        });
    }
    merged.segments.extend(second.segments);
    merged.insertions.extend(second.insertions);
    merged
}

/// Confident core of a clipped alignment: trim end stretches where
/// mismatches/gaps cluster (≥ [`GARBAGE_CLUSTER`] non-match columns within
/// [`GARBAGE_WINDOW`]), which is how SW renders a template region that is
/// absent from the read. Returns `(left, right)` column bounds into the
/// clipped columns; `(0, cols.len())` when the whole block is confident.
fn confident_core(cols: &[u8], rcols: &[u8]) -> (usize, usize) {
    let bads: Vec<usize> = cols
        .iter()
        .zip(rcols)
        .enumerate()
        .filter(|(_, (&t, &r))| !(t != b'-' && r != b'-' && matches_base(t, r)))
        .map(|(i, _)| i)
        .collect();
    let mut left = 0;
    let mut right = cols.len();
    if bads.len() >= GARBAGE_CLUSTER {
        let is_cluster = |w: &[usize]| w[GARBAGE_CLUSTER - 1] - w[0] < GARBAGE_WINDOW;
        if let Some(w) = bads.windows(GARBAGE_CLUSTER).find(|w| is_cluster(w)) {
            right = w[0];
        }
        if let Some(w) = bads.windows(GARBAGE_CLUSTER).rev().find(|w| is_cluster(w) && w[GARBAGE_CLUSTER - 1] < right) {
            left = w[GARBAGE_CLUSTER - 1] + 1;
        }
        if left >= right {
            // A single cluster: keep the longer confident side.
            let w0 = bads.windows(GARBAGE_CLUSTER).find(|w| is_cluster(w)).unwrap()[0];
            let w1 = bads.windows(GARBAGE_CLUSTER).rev().find(|w| is_cluster(w)).unwrap()[GARBAGE_CLUSTER - 1] + 1;
            if w0 >= cols.len() - w1 {
                left = 0;
                right = w0;
            } else {
                left = w1;
                right = cols.len();
            }
        }
    }
    (left, right)
}

/// Split-read path for a primary traceback that bridged a large absent
/// template region: trim the block back to its confident core, then re-align
/// the remaining read piece against the template arc the core left unused.
fn split_at_garbage(sw: &SwResult, t2: &[u8], oriented: &str, strand: &str, read_len: usize, tlen: usize) -> Option<Alignment> {
    let lead = sw.t_aln.iter().take_while(|&&c| c == b'-').count();
    let trail = sw.t_aln.iter().rev().take_while(|&&c| c == b'-').count();
    let cols = &sw.t_aln[lead..sw.t_aln.len() - trail];
    let rcols = &sw.r_aln[lead..sw.r_aln.len() - trail];
    let (l, r) = confident_core(cols, rcols);
    if l == 0 && r == cols.len() {
        return None;
    }
    // Confident core bounded by template-consuming columns on both sides.
    let sub = &cols[l..r];
    let c0 = l + sub.iter().position(|&c| c != b'-')?;
    let c1 = l + sub.iter().rposition(|&c| c != b'-')? + 1;
    let t_base = sw.t_start + lead;
    let r_base = sw.r_start + lead;
    let t_skip: usize = cols[..c0].iter().filter(|&&c| c != b'-').count();
    let t_take: usize = cols[c0..c1].iter().filter(|&&c| c != b'-').count();
    let r_skip: usize = rcols[..c0].iter().filter(|&&c| c != b'-').count();
    let r_take: usize = rcols[c0..c1].iter().filter(|&&c| c != b'-').count();
    let trimmed = SwResult {
        score: sw.score,
        t_aln: cols[c0..c1].to_vec(),
        r_aln: rcols[c0..c1].to_vec(),
        t_start: t_base + t_skip,
        t_end: t_base + t_skip + t_take - 1,
        r_start: r_base + r_skip,
        r_end: r_base + r_skip + r_take - 1,
    };
    let core = build_alignment(&trimmed, oriented.to_string(), strand, read_len, tlen, MIN_IDENTITY, MIN_ALIGNED_LEN).ok()?;
    let read = oriented.as_bytes();
    let hit = find_flank(t2, &read[core.r_end + 1..], core.r_end + 1, true, &core, oriented, strand, read_len, tlen)
        .or_else(|| find_flank(t2, &read[..core.r_start], 0, false, &core, oriented, strand, read_len, tlen))?;
    Some(merge_flank(core, hit))
}

/// Align one read orientation, then look for a second flank in the read
/// bases the primary block left unaligned (split read: two template flanks
/// with a large internal template region absent from the read).
fn align_oriented(t2: &[u8], oriented: &str, strand: &str, read_len: usize, tlen: usize) -> Result<Alignment, AlignReject> {
    let sw = sw_dispatch(t2, oriented.as_bytes()).ok_or(AlignReject::NoSignificantAlignment)?;
    if let Some(aln) = split_at_garbage(&sw, t2, oriented, strand, read_len, tlen) {
        return Ok(aln);
    }
    let primary = build_alignment(&sw, oriented.to_string(), strand, read_len, tlen, MIN_IDENTITY, MIN_ALIGNED_LEN)?;
    let r = oriented.as_bytes();
    let hit = find_flank(t2, &r[primary.r_end + 1..], primary.r_end + 1, true, &primary, oriented, strand, read_len, tlen)
        .or_else(|| find_flank(t2, &r[..primary.r_start], 0, false, &primary, oriented, strand, read_len, tlen));
    Ok(match hit {
        Some(h) => merge_flank(primary, h),
        None => primary.aln,
    })
}

/// Machine-readable difference details derived from an [`Alignment`] model
/// (no re-alignment): per-position mismatches, grouped deletions and
/// insertions. All coordinates are 0-based template coordinates; a deletion
/// that straddles the circular origin is merged into a single entry.
pub fn alignment_diff(a: &Alignment, template: &str) -> AlignmentDiff {
    let tbytes = template.as_bytes();
    let tlen = tbytes.len();
    let mut mismatches: Vec<AlignMismatch> = Vec::new();
    let mut deletions: Vec<AlignDeletion> = Vec::new();

    for seg in &a.segments {
        let mut run_start: Option<usize> = None;
        for (i, ch) in seg.chars.bytes().enumerate() {
            let pos = seg.start + i;
            if ch == b'-' {
                if run_start.is_none() {
                    run_start = Some(pos);
                }
                continue;
            }
            if let Some(rs) = run_start.take() {
                deletions.push(deletion_at(tbytes, rs, pos - 1));
            }
            let tb = tbytes.get(pos).copied();
            if tb.is_none_or(|t| t.to_ascii_uppercase() != ch) {
                mismatches.push(AlignMismatch {
                    pos,
                    template_base: tb.map_or_else(String::new, |b| (b as char).to_string()),
                    read_base: (ch as char).to_string(),
                });
            }
        }
        if let Some(rs) = run_start.take() {
            deletions.push(deletion_at(tbytes, rs, seg.end));
        }
    }

    // Template gap between consecutive segments (split-read flanks): the
    // region absent from the read between two blocks, reported as a deletion.
    if tlen > 0 {
        for w in a.segments.windows(2) {
            let gap = (w[1].start + tlen - w[0].end - 1) % tlen;
            if gap == 0 {
                continue;
            }
            let start = (w[0].end + 1) % tlen;
            deletions.push(AlignDeletion {
                pos: start,
                length: gap,
                bases: (0..gap).map(|k| tbytes[(start + k) % tlen] as char).collect(),
            });
        }
    }

    if tlen > 0 {
        for k in 0..deletions.len().saturating_sub(1) {
            if deletions[k].pos + deletions[k].length == tlen && deletions[k + 1].pos == 0 {
                let tail = deletions.swap_remove(k + 1);
                deletions[k].length += tail.length;
                deletions[k].bases.push_str(&tail.bases);
                break;
            }
        }
    }

    AlignmentDiff {
        mismatches,
        deletions,
        insertions: a
            .insertions
            .iter()
            .map(|i| AlignInsertionDetail {
                pos: i.pos,
                bases: i.bases.clone(),
                length: i.bases.len(),
            })
            .collect(),
        aligned_length: a.segments.iter().map(|s| s.end - s.start + 1).sum(),
    }
}

fn deletion_at(tbytes: &[u8], start: usize, end: usize) -> AlignDeletion {
    AlignDeletion {
        pos: start,
        length: end - start + 1,
        bases: (start..=end).filter_map(|p| tbytes.get(p).map(|b| *b as char)).collect(),
    }
}

/// Align `read` against `template`, returning the better orientation, or the
/// reason no alignment is significant. A strong forward match (identity
/// ≥ 0.9) is returned immediately; otherwise the reverse complement is
/// aligned too and the better orientation wins.
pub fn align_read_checked(template: &str, read: &str, circular: bool) -> Result<Alignment, AlignReject> {
    let t = template.to_ascii_uppercase();
    let r = read.to_ascii_uppercase();
    let tlen = t.len();
    if tlen == 0 || r.is_empty() {
        return Err(AlignReject::NoSignificantAlignment);
    }

    let t2 = if circular { format!("{}{}", t, t) } else { t };
    let fwd = align_oriented(t2.as_bytes(), &r, "+", read.len(), tlen);
    if let Ok(ref a) = fwd {
        if a.identity >= 0.9 {
            return fwd;
        }
    }

    let rc = crate::utils::reverse_complement(&r);
    let rev = align_oriented(t2.as_bytes(), &rc, "-", read.len(), tlen);

    match (fwd, rev) {
        (Ok(f), Ok(v)) => {
            let fq = f.identity * f.segments.iter().map(|s| s.end - s.start + 1).sum::<usize>() as f64;
            let vq = v.identity * v.segments.iter().map(|s| s.end - s.start + 1).sum::<usize>() as f64;
            if vq > fq { Ok(v) } else { Ok(f) }
        }
        (Ok(f), Err(_)) => Ok(f),
        (Err(_), Ok(v)) => Ok(v),
        (Err(fe), Err(ve)) => Err(better_reject(fe, ve)),
    }
}

fn better_reject(a: AlignReject, b: AlignReject) -> AlignReject {
    let rank = |r: &AlignReject| match r {
        AlignReject::LowIdentity { .. } => 2,
        AlignReject::TooShort { .. } => 1,
        AlignReject::NoSignificantAlignment => 0,
    };
    if rank(&b) > rank(&a) { b } else { a }
}

/// Align `read` against `template`. A strong forward match (identity ≥ 0.9)
/// is returned immediately; otherwise the reverse complement is aligned too
/// and the better orientation wins. Returns `None` when neither works.
pub fn align_read(template: &str, read: &str, circular: bool) -> Option<Alignment> {
    align_read_checked(template, read, circular).ok()
}

/// Next free incrementing alignment id (`aln-1`, `aln-2`, ...).
pub fn next_alignment_id(existing: &[Alignment]) -> String {
    let mut n = existing.len() + 1;
    while existing.iter().any(|a| a.id == format!("aln-{}", n)) {
        n += 1;
    }
    format!("aln-{}", n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Non-repetitive pseudo-random template (LCG over ACGT).
    fn make_template(len: usize, seed: u64) -> String {
        let mut x = seed;
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                b"ACGT"[(x >> 33) as usize & 3] as char
            })
            .collect()
    }

    /// Higher-quality random template (splitmix64): the LCG above correlates
    /// with itself at kilobase offsets, which lets SW bridge large deletions
    /// and defeats the split-read tests.
    fn make_template_smx(len: usize, seed: u64) -> String {
        let mut x = seed;
        (0..len)
            .map(|_| {
                x = x.wrapping_add(0x9E3779B97F4A7C15);
                let mut z = x;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
                z ^= z >> 31;
                b"ACGT"[(z & 3) as usize] as char
            })
            .collect()
    }

    #[test]
    fn test_perfect_match() {
        let t = make_template(200, 7);
        let read = &t[50..120];
        let aln = align_read(&t, read, false).unwrap();
        assert_eq!(aln.strand, "+");
        assert_eq!(aln.length, 70);
        assert!((aln.identity - 1.0).abs() < 1e-9);
        assert_eq!(aln.segments.len(), 1);
        assert_eq!(aln.segments[0].start, 50);
        assert_eq!(aln.segments[0].end, 119);
        assert_eq!(aln.segments[0].chars, t[50..120]);
        assert!(aln.insertions.is_empty());
    }

    #[test]
    fn test_read_with_deletion() {
        let t = make_template(200, 11);
        // 5 bp deleted from the middle of the read
        let read = format!("{}{}", &t[50..100], &t[105..120]);
        let aln = align_read(&t, &read, false).unwrap();
        assert_eq!(aln.segments.len(), 1);
        assert_eq!(aln.segments[0].start, 50);
        assert_eq!(aln.segments[0].end, 119);
        assert_eq!(aln.segments[0].chars.len(), 70);
        assert!(aln.segments[0].chars.contains("-----"));
        assert!(aln.insertions.is_empty());
        assert!(aln.identity >= 0.6);
    }

    #[test]
    fn test_read_with_insertion() {
        let mut t = make_template(200, 13);
        // Keep G out of the flanking columns so the 5xG insertion cannot
        // slide into an equally-scored position.
        for i in [98, 99, 100] {
            if t.as_bytes()[i] == b'G' {
                t.replace_range(i..i + 1, "A");
            }
        }
        let read = format!("{}GGGGG{}", &t[50..100], &t[100..120]);
        let aln = align_read(&t, &read, false).unwrap();
        assert_eq!(aln.segments.len(), 1);
        assert_eq!(aln.segments[0].start, 50);
        assert_eq!(aln.segments[0].end, 119);
        assert!(!aln.segments[0].chars.contains('-'));
        assert_eq!(aln.insertions.len(), 1);
        assert_eq!(aln.insertions[0].pos, 100);
        assert_eq!(aln.insertions[0].bases, "GGGGG");
        assert!(aln.identity >= 0.6);
    }

    #[test]
    fn test_reverse_complement_match() {
        let t = make_template(200, 17);
        let read = crate::utils::reverse_complement(&t[50..120]);
        let aln = align_read(&t, &read, false).unwrap();
        assert_eq!(aln.strand, "-");
        // seq is stored as oriented for display: rev-comp of the raw read.
        assert_eq!(aln.seq, t[50..120]);
        assert_eq!(aln.segments.len(), 1);
        assert_eq!(aln.segments[0].start, 50);
        assert_eq!(aln.segments[0].end, 119);
        assert!((aln.identity - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_circular_spanning_origin() {
        let t = make_template(80, 23);
        // Read wraps the origin: last 30 bp + first 30 bp
        let read = format!("{}{}", &t[50..80], &t[0..30]);
        let aln = align_read(&t, &read, true).unwrap();
        assert_eq!(aln.segments.len(), 2);
        assert_eq!(aln.segments[0].start, 50);
        assert_eq!(aln.segments[0].end, 79);
        assert_eq!(aln.segments[0].chars, t[50..80]);
        assert_eq!(aln.segments[1].start, 0);
        assert_eq!(aln.segments[1].end, 29);
        assert_eq!(aln.segments[1].chars, t[0..30]);
        assert!((aln.identity - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_no_significant_alignment() {
        let t = make_template(200, 29);
        // Homopolymer read has no significant local alignment against random template.
        assert!(align_read(&t, &"A".repeat(70), false).is_none());
        // Too short
        assert!(align_read(&t, &t[10..40], false).is_none());
    }

    #[test]
    fn test_short_read_reject_reason() {
        let t = make_template(200, 47);
        assert_eq!(
            align_read_checked(&t, &t[10..40], false).unwrap_err(),
            AlignReject::TooShort { span: 30 }
        );
        assert!(align_read_checked(&t, &"A".repeat(70), false).is_err());
    }

    #[test]
    fn test_alignment_diff_details() {
        let mut t = make_template(200, 41);
        // Keep G out of the columns around the insertion point so the 2xG
        // insertion cannot slide into an equally-scored position.
        for i in 88..=93 {
            if t.as_bytes()[i] == b'G' {
                t.replace_range(i..i + 1, "A");
            }
        }
        let sub_base = if t.as_bytes()[60] == b'A' { 'C' } else { 'A' };
        // Read: substitution at template 60, t[80..83] deleted from the read,
        // "GG" inserted before template column 90.
        let read = format!(
            "{}{}{}{}{}{}",
            &t[50..60],
            sub_base,
            &t[61..80],
            &t[83..90],
            "GG",
            &t[90..120]
        );
        let aln = align_read(&t, &read, false).unwrap();
        assert_eq!(aln.strand, "+");
        let diff = alignment_diff(&aln, &t);

        assert_eq!(
            diff.aligned_length,
            aln.segments.iter().map(|s| s.end - s.start + 1).sum::<usize>()
        );
        assert_eq!(diff.mismatches.len(), 1);
        assert_eq!(diff.mismatches[0].pos, 60);
        assert_eq!(diff.mismatches[0].template_base, t[60..61]);
        assert_eq!(diff.mismatches[0].read_base, sub_base.to_string());

        assert_eq!(diff.deletions.len(), 1);
        assert_eq!(diff.deletions[0].pos, 80);
        assert_eq!(diff.deletions[0].length, 3);
        assert_eq!(diff.deletions[0].bases, t[80..83]);

        assert_eq!(diff.insertions.len(), 1);
        assert_eq!(diff.insertions[0].pos, 90);
        assert_eq!(diff.insertions[0].bases, "GG");
        assert_eq!(diff.insertions[0].length, 2);

        // Totals match the legacy per-column counters.
        assert_eq!(diff.deletions.iter().map(|d| d.length).sum::<usize>(), 3);
        assert_eq!(diff.insertions.iter().map(|i| i.length).sum::<usize>(), 2);
    }

    #[test]
    fn test_alignment_diff_circular_deletion_merge() {
        let t = make_template(80, 43);
        // Delete template bases 78, 79, 0, 1 from the read: one 4 bp deletion
        // straddling the circular origin.
        let read = format!("{}{}", &t[50..78], &t[2..30]);
        let aln = align_read(&t, &read, true).unwrap();
        assert_eq!(aln.segments.len(), 2);
        let diff = alignment_diff(&aln, &t);

        assert_eq!(diff.deletions.len(), 1);
        assert_eq!(diff.deletions[0].pos, 78);
        assert_eq!(diff.deletions[0].length, 4);
        assert_eq!(diff.deletions[0].bases, format!("{}{}", &t[78..80], &t[0..2]));
        assert!(diff.mismatches.is_empty());
        assert!(diff.insertions.is_empty());
        assert_eq!(
            diff.aligned_length,
            aln.segments.iter().map(|s| s.end - s.start + 1).sum::<usize>()
        );
    }

    // Long-template tests force the seed-and-extend path (matrix over the
    // full-SW cell cap) and must stay fast.

    #[test]
    fn test_long_template_anchored() {
        let t = make_template(100_000, 101);
        // 3 kb read from mid-template with 3 substitutions, a 4 bp deletion
        // and a 3 bp insertion.
        let mut read: Vec<u8> = t[40_000..43_000].bytes().collect();
        for (i, b) in [(100, b'C'), (1_500, b'A'), (2_900, b'G')] {
            if read[i] == b {
                read[i] = b'T';
            } else {
                read[i] = b;
            }
        }
        read.drain(2_000..2_004); // 4 bp deletion
        read.splice(500..500, b"TTT".iter().copied()); // 3 bp insertion
        let read = String::from_utf8(read).unwrap();

        let aln = align_read(&t, &read, false).unwrap();
        assert_eq!(aln.strand, "+");
        assert_eq!(aln.segments.len(), 1);
        assert_eq!(aln.segments[0].start, 40_000);
        assert_eq!(aln.segments[0].end, 42_999);
        assert!(aln.identity >= 0.99, "identity {}", aln.identity);
        let diff = alignment_diff(&aln, &t);
        assert_eq!(diff.mismatches.len(), 3);
        // The 4 bp deletion sits in an A run and may be split by tie-breaking;
        // assert the total rather than the grouping.
        assert_eq!(diff.deletions.iter().map(|d| d.length).sum::<usize>(), 4);
        assert_eq!(diff.insertions.len(), 1);
        assert_eq!(diff.insertions[0].bases, "TTT");
    }

    #[test]
    fn test_long_template_reverse_complement() {
        let t = make_template(100_000, 103);
        let read = crate::utils::reverse_complement(&t[50_000..53_000]);
        let aln = align_read(&t, &read, false).unwrap();
        assert_eq!(aln.strand, "-");
        assert_eq!(aln.seq, t[50_000..53_000]);
        assert_eq!(aln.segments.len(), 1);
        assert_eq!(aln.segments[0].start, 50_000);
        assert_eq!(aln.segments[0].end, 52_999);
        assert!((aln.identity - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_long_template_circular_wrap() {
        let t = make_template(60_000, 107);
        // 3 kb read straddling the origin: last 1500 bp + first 1500 bp.
        let read = format!("{}{}", &t[58_500..60_000], &t[0..1_500]);
        let aln = align_read(&t, &read, true).unwrap();
        assert_eq!(aln.segments.len(), 2);
        assert_eq!(aln.segments[0].start, 58_500);
        assert_eq!(aln.segments[0].end, 59_999);
        assert_eq!(aln.segments[0].chars, t[58_500..60_000]);
        assert_eq!(aln.segments[1].start, 0);
        assert_eq!(aln.segments[1].end, 1_499);
        assert_eq!(aln.segments[1].chars, t[0..1_500]);
        assert!((aln.identity - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_long_template_divergent_read() {
        let t = make_template(100_000, 109);
        let read = make_template(3_000, 113);
        assert!(align_read(&t, &read, false).is_none());
        assert!(align_read_checked(&t, &read, false).is_err());
    }

    /// Random template restricted to a subset alphabet: regions with
    /// disjoint composition cannot be bridged by a gapped SW extension,
    /// which forces a clean split at the junction in split-read tests.
    fn make_biased(len: usize, seed: u64, alphabet: &[u8]) -> String {
        let mut x = seed;
        (0..len)
            .map(|_| {
                x = x.wrapping_add(0x9E3779B97F4A7C15);
                let mut z = x;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
                z ^= z >> 31;
                alphabet[z as usize % alphabet.len()] as char
            })
            .collect()
    }

    /// Template: left flank (AC-only), 1 kb middle (GT-only, absent from the
    /// read), right flank (AC-only). Read = left + insert + right.
    fn split_read_fixture() -> (String, String, String) {
        let left = make_biased(500, 61, b"AC");
        let middle = make_biased(1_000, 63, b"GT");
        let right = make_biased(500, 67, b"AC");
        let insert = "AACCCAAACACCAACCCAC".to_string();
        let t = format!("{}{}{}", left, middle, right);
        let read = format!("{}{}{}", left, insert, right);
        (t, read, insert)
    }

    #[test]
    fn test_split_read_internal_deletion_with_insert() {
        let (t, read, insert) = split_read_fixture();
        let aln = align_read(&t, &read, false).unwrap();

        assert_eq!(aln.segments.len(), 2, "segments {:?}", aln.segments);
        let (a, b) = (&aln.segments[0], &aln.segments[1]);
        assert!((a.start as i64).abs() <= 2, "seg0 start {}", a.start);
        assert!((a.end as i64 - 499).abs() <= 2, "seg0 end {}", a.end);
        assert!((b.start as i64 - 1500).abs() <= 4, "seg1 start {}", b.start);
        assert!((b.end as i64 - 1999).abs() <= 2, "seg1 end {}", b.end);
        let gap = b.start - a.end - 1;
        assert!((gap as i64 - 1000).abs() <= 6, "template gap {}", gap);

        assert_eq!(aln.insertions.len(), 1, "insertions {:?}", aln.insertions);
        assert_eq!(aln.insertions[0].pos, b.start);
        assert!(aln.insertions[0].bases.contains(&insert[2..insert.len() - 2]), "bases {}", aln.insertions[0].bases);
        assert!(aln.identity >= 0.95, "identity {}", aln.identity);

        let diff = alignment_diff(&aln, &t);
        assert_eq!(diff.deletions.len(), 1, "deletions {:?}", diff.deletions);
        assert!((diff.deletions[0].length as i64 - 1000).abs() <= 6);
    }

    #[test]
    fn test_split_read_full_match_unchanged() {
        let t = make_template_smx(3_000, 63);
        let read = &t[500..1500];
        let aln = align_read(&t, read, false).unwrap();
        assert_eq!(aln.segments.len(), 1);
        assert_eq!(aln.segments[0].start, 500);
        assert_eq!(aln.segments[0].end, 1499);
        assert!(aln.insertions.is_empty());
        assert!((aln.identity - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_split_read_junk_tail_unchanged() {
        let t = make_template_smx(3_000, 67);
        // Matching body + a poly-A tail that aligns nowhere.
        let read = format!("{}{}", &t[1000..1500], "A".repeat(200));
        let aln = align_read(&t, &read, false).unwrap();
        assert_eq!(aln.segments.len(), 1, "segments {:?}", aln.segments);
        assert_eq!(aln.segments[0].start, 1000);
        assert!(aln.segments[0].end <= 1_510, "end {}", aln.segments[0].end);
        assert!(aln.identity >= 0.9, "identity {}", aln.identity);
        assert!(!aln.insertions.iter().any(|i| i.bases.len() >= 20));
    }

    #[test]
    fn test_split_read_minus_strand() {
        let (t, fwd, insert) = split_read_fixture();
        let read = crate::utils::reverse_complement(&fwd);
        let aln = align_read(&t, &read, false).unwrap();
        assert_eq!(aln.strand, "-");
        assert_eq!(aln.segments.len(), 2, "segments {:?}", aln.segments);
        assert_eq!(aln.insertions.len(), 1);
        assert!(aln.insertions[0].bases.contains(&insert[2..insert.len() - 2]), "bases {}", aln.insertions[0].bases);
    }
}
