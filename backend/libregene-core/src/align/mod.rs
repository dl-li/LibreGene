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

/// Build the render-oriented [`Alignment`] from a traceback, or the reason
/// the alignment is too weak to be meaningful.
fn build_alignment(sw: &SwResult, oriented_read: String, strand: &str, read_len: usize, tlen: usize) -> Result<Alignment, AlignReject> {
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

    if identity < MIN_IDENTITY {
        return Err(AlignReject::LowIdentity { identity, span });
    }
    if span < MIN_ALIGNED_LEN {
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

    Ok(Alignment {
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
            if tb.map_or(true, |t| t.to_ascii_uppercase() != ch) {
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
    let fwd = smith_waterman(t2.as_bytes(), r.as_bytes())
        .ok_or(AlignReject::NoSignificantAlignment)
        .and_then(|f| build_alignment(&f, r.clone(), "+", read.len(), tlen));
    if let Ok(ref a) = fwd {
        if a.identity >= 0.9 {
            return fwd;
        }
    }

    let rc = crate::utils::reverse_complement(&r);
    let rev = smith_waterman(t2.as_bytes(), rc.as_bytes())
        .ok_or(AlignReject::NoSignificantAlignment)
        .and_then(|v| build_alignment(&v, rc, "-", read.len(), tlen));

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

/// Restriction-enzyme recognition sites whose span intersects any alignment
/// difference: a mismatch position, a deletion interval, or an insertion
/// point (pos-1 or pos within the site). Coordinates 0-based inclusive;
/// each site is listed once, sorted by rec_start. Reuses the already
/// computed engine results (`ProjectData.enzymes`), so no recompute runs.
pub fn destroyed_enzyme_sites(
    enzymes: &[crate::models::Enzyme],
    diff: &AlignmentDiff,
) -> Vec<crate::models::DestroyedEnzymeSite> {
    let mut out = Vec::new();
    for e in enzymes {
        if site_destroyed(e.rec_start, e.rec_end, diff) {
            out.push(crate::models::DestroyedEnzymeSite {
                enzyme: e.name.clone(),
                rec_start: e.rec_start,
                rec_end: e.rec_end,
                rec_seq: e.rec_seq.clone(),
            });
        }
    }
    out.sort_by_key(|s| (s.rec_start, s.rec_end));
    out
}

fn site_destroyed(rec_start: i64, rec_end: i64, diff: &AlignmentDiff) -> bool {
    let hits = |p: i64| p >= rec_start && p <= rec_end;
    diff.mismatches.iter().any(|m| hits(m.pos as i64))
        || diff.deletions.iter().any(|d| {
            let a = d.pos as i64;
            let b = (d.pos + d.length.saturating_sub(1)) as i64;
            !(b < rec_start || rec_end < a)
        })
        || diff.insertions.iter().any(|i| {
            let p = i.pos as i64;
            hits(p) || hits(p - 1)
        })
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

    fn site(name: &str, start: i64, end: i64) -> crate::models::Enzyme {
        crate::models::Enzyme {
            id: name.to_string(),
            name: name.to_string(),
            rec_seq: "GAATTC".to_string(),
            rec_start: start,
            rec_end: end,
            ..Default::default()
        }
    }

    #[test]
    fn test_destroyed_sites_by_mismatch() {
        let sites = vec![site("EcoRI", 55, 60), site("BamHI", 90, 95)];
        let diff = AlignmentDiff {
            mismatches: vec![AlignMismatch {
                pos: 58,
                template_base: "A".to_string(),
                read_base: "C".to_string(),
            }],
            ..Default::default()
        };
        let out = destroyed_enzyme_sites(&sites, &diff);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].enzyme, "EcoRI");
        assert_eq!((out[0].rec_start, out[0].rec_end), (55, 60));
        assert_eq!(out[0].rec_seq, "GAATTC");
    }

    #[test]
    fn test_destroyed_sites_empty_when_no_intersection() {
        let sites = vec![site("BamHI", 90, 95)];
        let diff = AlignmentDiff {
            mismatches: vec![AlignMismatch {
                pos: 10,
                template_base: "A".to_string(),
                read_base: "C".to_string(),
            }],
            deletions: vec![AlignDeletion {
                pos: 20,
                length: 3,
                bases: "ACG".to_string(),
            }],
            insertions: vec![AlignInsertionDetail {
                pos: 30,
                bases: "GG".to_string(),
                length: 2,
            }],
            ..Default::default()
        };
        assert!(destroyed_enzyme_sites(&sites, &diff).is_empty());
    }

    #[test]
    fn test_destroyed_sites_by_deletion() {
        let sites = vec![site("EcoRI", 55, 60)];
        // Deletion of template 60..62 touches rec_end = 60.
        let diff = AlignmentDiff {
            deletions: vec![AlignDeletion {
                pos: 60,
                length: 3,
                bases: "AAA".to_string(),
            }],
            ..Default::default()
        };
        assert_eq!(destroyed_enzyme_sites(&sites, &diff).len(), 1);
        // Adjacent deletion 61..63 does not overlap 55..60.
        let diff2 = AlignmentDiff {
            deletions: vec![AlignDeletion {
                pos: 61,
                length: 3,
                bases: "AAA".to_string(),
            }],
            ..Default::default()
        };
        assert!(destroyed_enzyme_sites(&sites, &diff2).is_empty());
    }

    #[test]
    fn test_destroyed_sites_by_insertion() {
        let sites = vec![site("EcoRI", 55, 60)];
        // Insertion before template column 60 (between 59 and 60): pos = 60.
        let diff = AlignmentDiff {
            insertions: vec![AlignInsertionDetail {
                pos: 60,
                bases: "GG".to_string(),
                length: 2,
            }],
            ..Default::default()
        };
        assert_eq!(destroyed_enzyme_sites(&sites, &diff).len(), 1);
        // Insertion between 60 and 61: pos - 1 = 60 still inside the site.
        let diff2 = AlignmentDiff {
            insertions: vec![AlignInsertionDetail {
                pos: 61,
                bases: "GG".to_string(),
                length: 2,
            }],
            ..Default::default()
        };
        assert_eq!(destroyed_enzyme_sites(&sites, &diff2).len(), 1);
        // Far away insertion does not touch the site.
        let diff3 = AlignmentDiff {
            insertions: vec![AlignInsertionDetail {
                pos: 70,
                bases: "GG".to_string(),
                length: 2,
            }],
            ..Default::default()
        };
        assert!(destroyed_enzyme_sites(&sites, &diff3).is_empty());
        // pos = 0 means between tlen-1 and 0; pos - 1 = -1 is never in a site.
        let diff4 = AlignmentDiff {
            insertions: vec![AlignInsertionDetail {
                pos: 0,
                bases: "GG".to_string(),
                length: 2,
            }],
            ..Default::default()
        };
        assert!(destroyed_enzyme_sites(&sites, &diff4).is_empty());
    }

    #[test]
    fn test_destroyed_sites_sorted() {
        let sites = vec![
            site("EcoRI", 90, 95),
            site("BamHI", 20, 25),
            site("HindIII", 58, 62),
        ];
        let diff = AlignmentDiff {
            mismatches: vec![
                AlignMismatch {
                    pos: 58,
                    template_base: "A".to_string(),
                    read_base: "C".to_string(),
                },
                AlignMismatch {
                    pos: 22,
                    template_base: "A".to_string(),
                    read_base: "C".to_string(),
                },
                AlignMismatch {
                    pos: 92,
                    template_base: "A".to_string(),
                    read_base: "C".to_string(),
                },
                AlignMismatch {
                    pos: 59,
                    template_base: "A".to_string(),
                    read_base: "C".to_string(),
                },
            ],
            ..Default::default()
        };
        let out = destroyed_enzyme_sites(&sites, &diff);
        assert_eq!(out.len(), 3);
        let names: Vec<&str> = out.iter().map(|s| s.enzyme.as_str()).collect();
        assert_eq!(names, vec!["BamHI", "HindIII", "EcoRI"]);
    }
}
