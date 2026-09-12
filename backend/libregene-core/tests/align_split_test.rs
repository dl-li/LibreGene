//! Split-read alignment: a Sanger read whose template has a large internal
//! region absent in the read must align as two flanks plus a junction
//! insertion. Real data: flyTIGRi template + PVA-T1-T3 read (template
//! 4432..5513 replaced by the 19 bp insert CTGCTAGCTGGGAATTCCG).
use std::path::Path;

fn test_data(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join("test_data")
        .join(name)
}

#[test]
fn split_read_two_flanks_with_junction_insert() {
    let template = libregene_core::file_io::gbk::parse_gbk(&test_data("flyTIGRi.gbk"))
        .expect("parse template gbk");
    let read = libregene_core::file_io::ab1::parse_ab1(&test_data("PVA-T1-T3-1.86349.ab1"))
        .expect("parse ab1 read");
    let circular = template.topology == "circular";

    let aln = libregene_core::align::align_read(&template.sequence, &read.sequence, circular)
        .expect("alignment");
    eprintln!(
        "strand={} identity={:.4} segments={:?} insertions={:?}",
        aln.strand, aln.identity, aln.segments, aln.insertions
    );

    // FlankA spans the circular origin, so it renders as two segments; the
    // read order is [6739..8699, 0..~4431, ~5514..6738]. The insert's suffix
    // (an NheI/EcoRI linker) coincidentally matches the tail of the absent
    // region, so the second flank resumes a few bases before 5514.
    assert_eq!(aln.segments.len(), 3, "segments {:?}", aln.segments);
    let (a, b, c) = (&aln.segments[0], &aln.segments[1], &aln.segments[2]);
    assert_eq!(a.end, template.sequence.len() - 1, "flankA reaches the origin");
    assert_eq!(b.start, 0, "flankA resumes at the origin");
    assert!(b.end >= 4_300 && b.end <= 4_432, "flankA end {}", b.end);
    assert!(c.start >= 5_113 && c.start <= 5_600, "flankB start {}", c.start);

    // The 19 bp insert sits at the junction: the part that does not match
    // the template tail is the junction insertion, the rest is interleaved
    // in the flank head (chars plus internal insertions).
    let mut junction_read = String::new();
    for (k, ch) in c.chars[..30].chars().enumerate() {
        let pos = c.start + k;
        if let Some(ins) = aln.insertions.iter().find(|i| i.pos == pos) {
            junction_read.push_str(&ins.bases);
        }
        if ch != '-' {
            junction_read.push(ch);
        }
    }
    assert!(
        junction_read.contains("CTGCTAGCTGGGAATTCCG"),
        "junction read {junction_read}"
    );

    assert!(aln.identity >= 0.98, "identity {}", aln.identity);

    // The template gap between the flanks is reported as a deletion.
    let diff = libregene_core::align::alignment_diff(&aln, &template.sequence);
    let gap = diff
        .deletions
        .iter()
        .find(|d| d.pos > b.end && d.pos + d.length <= c.start + 1)
        .unwrap_or_else(|| panic!("no inter-flank deletion: {:?}", diff.deletions));
    assert!((gap.length as i64 - 1_082).abs() <= 30, "gap {gap:?}");
}
