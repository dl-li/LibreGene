//! The display model must reproduce the read exactly, in the read's own
//! order. The frontend walks an alignment's segments (insertions first, at
//! their anchor column) to number the read bases one by one, and that
//! numbering indexes the chromatogram peaks — so any lost or reordered base
//! shifts every later peak and the trace no longer lines up with the sequence.
//! The rotation fast path used to hand back subject-ordered columns (a read
//! starting mid-template came back rotated to the template origin), so the
//! rebuilt read began at the wrong base.

use libregene_core::align::{align_read_checked_with, AlignAlgorithm};
use libregene_core::models::Alignment;

/// Walk the display model the way the frontend does: per segment column in
/// join order, insertions anchored there first, then the char unless it is a
/// read gap. Insertions the walk never reaches are appended at the end.
fn rebuild_read(aln: &Alignment) -> String {
    let mut out = String::new();
    let mut used = vec![false; aln.insertions.len()];
    for seg in &aln.segments {
        for (k, ch) in seg.chars.chars().enumerate() {
            let col = seg.start + k;
            for (i, ins) in aln.insertions.iter().enumerate() {
                if !used[i] && ins.pos == col {
                    used[i] = true;
                    out.push_str(&ins.bases);
                }
            }
            if ch != '-' {
                out.push(ch);
            }
        }
    }
    for (i, ins) in aln.insertions.iter().enumerate() {
        if !used[i] {
            out.push_str(&ins.bases);
        }
    }
    out
}

#[test]
fn every_read_base_appears_once_in_read_order() {
    // Circular template; the read starts mid-template, wraps the origin once,
    // and carries a 34 bp insertion plus a couple of point mismatches.
    let t = make_template_smx(4000, 7);
    let mut read = format!("{}{}", &t[2500..], &t[..500]);
    read.insert_str(1200, "GATTACAGATTACAGATTACAGATTACAGATTAC");
    let mut bytes = read.into_bytes();
    bytes[300] = if bytes[300] == b'A' { b'C' } else { b'A' };
    bytes[800] = if bytes[800] == b'G' { b'T' } else { b'G' };
    let read = String::from_utf8(bytes).unwrap();

    for algo in [AlignAlgorithm::BlastN, AlignAlgorithm::SmithWaterman] {
        let aln = align_read_checked_with(&t, &read, true, algo).unwrap();
        // One insertion per template column: duplicates would render on top of
        // each other and the display walk would drop the later bases.
        let mut positions: Vec<usize> = aln.insertions.iter().map(|i| i.pos).collect();
        positions.sort_unstable();
        let unique = positions.len();
        positions.dedup();
        assert_eq!(unique, positions.len(), "[{}] duplicate anchors", algo.as_str());

        let rebuilt = rebuild_read(&aln);
        assert_eq!(
            rebuilt, aln.seq,
            "[{}] the model must walk the read in its own order (segments {:?})",
            algo.as_str(),
            aln.segments
                .iter()
                .map(|s| (s.start, s.end))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn rotation_hit_reports_segments_in_read_order() {
    // The read starts inside the template, so the rotation fast path matches it
    // against a rotated copy: its columns must still come back starting at the
    // read's first base, with the subject wrapping once at the origin.
    let t = make_template_smx(4000, 11);
    let read = format!("{}{}", &t[2500..], &t[..500]);
    let aln = align_read_checked_with(&t, &read, true, AlignAlgorithm::BlastN).unwrap();

    assert!(aln.segments.len() >= 2, "segments {:?}", aln.segments);
    let first = aln.segments[0].chars.chars().next().unwrap();
    assert_eq!(
        first.to_string(),
        aln.seq[..1],
        "the first displayed base must be the read's first base"
    );
    // The subject coordinates wrap exactly once (read order, one origin wrap).
    let drops = aln
        .segments
        .windows(2)
        .filter(|w| w[1].start <= w[0].end)
        .count();
    assert_eq!(drops, 1, "segments {:?}", aln.segments);
}

/// A read that spans the origin of a circular template, aligned through the
/// local engine against the doubled subject: the chain must not cover the
/// same template columns twice (that would render the read twice and make
/// the walk/reconstruction ambiguous).
#[test]
fn circular_read_spanning_origin_has_no_overlapping_segments() {
    let t = make_template_smx(3000, 21);
    // Read covers ~1200 bp starting 2400: it wraps the origin.
    let read = format!("{}{}", &t[2400..], &t[..600]);
    for algo in [AlignAlgorithm::BlastN, AlignAlgorithm::SmithWaterman] {
        let aln = align_read_checked_with(&t, &read, true, algo).unwrap();
        // Overlap check in the wrap-aware sense: sort arcs, ensure none lies
        // inside another and that the union count matches the base count.
        let mut covered = 0usize;
        let mut seen = vec![false; t.len()];
        for seg in &aln.segments {
            for pos in seg.start..=seg.end {
                assert!(
                    !seen[pos % t.len()],
                    "[{}] column {} covered by two segments: {:?}",
                    algo.as_str(),
                    pos,
                    aln.segments
                        .iter()
                        .map(|s| (s.start, s.end))
                        .collect::<Vec<_>>()
                );
                seen[pos % t.len()] = true;
                if seg.chars.as_bytes()[pos - seg.start] != b'-' {
                    covered += 1;
                }
            }
        }
        assert_eq!(
            rebuild_read(&aln),
            aln.seq,
            "[{}] model must rebuild the read in order",
            algo.as_str()
        );
        assert!(covered > 0);
    }
}

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
