//! Smith–Waterman local alignment of a read against the project sequence.
//!
//! Scoring: match +2, mismatch −1, linear gap −2. Non-ACGT bases never
//! match. Circular templates are aligned as template concatenated with
//! itself, then coordinates are mapped back via `% tlen` and the aligned
//! range is split into non-wrapping segments at the origin.

use crate::models::{AlignInsertion, AlignSegment, Alignment};

const MATCH: i32 = 2;
const MISMATCH: i32 = -1;
const GAP: i32 = -2;

const MIN_IDENTITY: f64 = 0.6;
const MIN_ALIGNED_LEN: usize = 50;

fn matches_base(a: u8, b: u8) -> bool {
    a == b && matches!(a, b'A' | b'C' | b'G' | b'T')
}

struct SwResult {
    /// Aligned template chars ('-' = gap in template), 5'→3'.
    t_aln: Vec<u8>,
    /// Aligned read chars ('-' = gap in read), 5'→3'.
    r_aln: Vec<u8>,
    /// Template coordinates (in the possibly doubled template) of the
    /// first and last template-consuming column, inclusive.
    t_start: usize,
    t_end: usize,
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
        t_aln,
        r_aln,
        t_start: j,
        t_end: bj - 1,
    })
}

/// Build the render-oriented [`Alignment`] from a traceback, or `None` when
/// the alignment is too weak to be meaningful.
fn build_alignment(sw: &SwResult, oriented_read: String, strand: &str, read_len: usize, tlen: usize) -> Option<Alignment> {
    // Clip read-only overhang: leading/trailing columns that don't consume template.
    let lead = sw.t_aln.iter().take_while(|&&c| c == b'-').count();
    let trail = sw.t_aln.iter().rev().take_while(|&&c| c == b'-').count();
    let cols = &sw.t_aln[lead..sw.t_aln.len() - trail];
    let rcols = &sw.r_aln[lead..sw.r_aln.len() - trail];
    if cols.is_empty() {
        return None;
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

    if identity < MIN_IDENTITY || span < MIN_ALIGNED_LEN {
        return None;
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
            .map_or(false, |s: &AlignSegment| mapped <= s.end);
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

    Some(Alignment {
        id: String::new(),
        name: String::new(),
        length: read_len,
        strand: strand.to_string(),
        identity,
        segments,
        insertions,
        seq: oriented_read,
    })
}

/// Align `read` against `template`. A strong forward match (identity ≥ 0.9)
/// is returned immediately; otherwise the reverse complement is aligned too
/// and the better orientation wins. Returns `None` when neither works.
pub fn align_read(template: &str, read: &str, circular: bool) -> Option<Alignment> {
    let t = template.to_ascii_uppercase();
    let r = read.to_ascii_uppercase();
    let tlen = t.len();
    if tlen == 0 || r.is_empty() {
        return None;
    }

    let t2 = if circular { format!("{}{}", t, t) } else { t };
    let fwd = smith_waterman(t2.as_bytes(), r.as_bytes())
        .and_then(|f| build_alignment(&f, r.clone(), "+", read.len(), tlen));
    if let Some(ref a) = fwd {
        if a.identity >= 0.9 {
            return fwd;
        }
    }

    let rc = crate::utils::reverse_complement(&r);
    let rev = smith_waterman(t2.as_bytes(), rc.as_bytes())
        .and_then(|v| build_alignment(&v, rc, "-", read.len(), tlen));

    match (fwd, rev) {
        (Some(f), Some(v)) => {
            let fq = f.identity * f.segments.iter().map(|s| s.end - s.start + 1).sum::<usize>() as f64;
            let vq = v.identity * v.segments.iter().map(|s| s.end - s.start + 1).sum::<usize>() as f64;
            if vq > fq { Some(v) } else { Some(f) }
        }
        (Some(f), None) => Some(f),
        (None, Some(v)) => Some(v),
        (None, None) => None,
    }
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
}
