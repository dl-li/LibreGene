//! The display model must reproduce the read exactly, in the read's own
//! order. The frontend walks an alignment's segments (insertions first, at
//! their anchor column) to number the read bases one by one, and that
//! numbering indexes the chromatogram peaks — so any lost or reordered base
//! shifts every later peak and the trace no longer lines up with the sequence.
//! Real data: ***REMOVED*** template (circular) + PVA-T1-T3 read, which starts
//! mid-plasmid: the rotation fast path used to hand back subject-ordered
//! columns (the read rotated to the template origin), so the rebuilt read
//! began at the wrong base.

use libregene_core::align::{align_read_checked_with, AlignAlgorithm};
use libregene_core::models::Alignment;
use std::path::Path;

fn test_data(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("test_data")
        .join(name)
}

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
    let template = libregene_core::file_io::gbk::parse_gbk(&test_data("***REMOVED***.gbk")).unwrap();
    let read = libregene_core::file_io::ab1::parse_ab1(&test_data("PVA-T1-T3-1.86349.ab1")).unwrap();
    let circular = template.topology == "circular";

    for algo in [AlignAlgorithm::BlastN, AlignAlgorithm::SmithWaterman] {
        let aln =
            align_read_checked_with(&template.sequence, &read.sequence, circular, algo).unwrap();
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
    // The read starts inside the plasmid, so the rotation fast path matches it
    // against a rotated copy: its columns must still come back starting at the
    // read's first base, with the subject wrapping once at the origin.
    let template = libregene_core::file_io::gbk::parse_gbk(&test_data("***REMOVED***.gbk")).unwrap();
    let read = libregene_core::file_io::ab1::parse_ab1(&test_data("PVA-T1-T3-1.86349.ab1")).unwrap();
    let aln =
        align_read_checked_with(&template.sequence, &read.sequence, true, AlignAlgorithm::BlastN)
            .unwrap();

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
