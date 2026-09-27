//! Split-read alignment: a read whose template has a large internal region
//! absent in the read must align as two flanks plus a junction insertion.

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
fn split_read_two_flanks_with_junction_insert() {
    // Synthetic circular template: the read covers flank A (wrapping the
    // origin), then a 19 bp junction insert, then flank B; the template
    // region 4000..5200 is absent from the read.
    let t = make_template_smx(9000, 21);
    let insert = "CTGCTAGCTGGGAATTCCG";
    let read = format!("{}{}{}{}", &t[6500..], &t[..4000], insert, &t[5200..6500]);

    let aln = libregene_core::align::align_read(&t, &read, true).expect("alignment");

    // Flank A spans the circular origin, so it renders as two segments; the
    // read order is [6500..8999, 0..3999, 5200..6499].
    assert_eq!(aln.segments.len(), 3, "segments {:?}", aln.segments);
    let (a, b, c) = (&aln.segments[0], &aln.segments[1], &aln.segments[2]);
    assert_eq!(a.end, t.len() - 1, "flankA reaches the origin");
    assert_eq!(b.start, 0, "flankA resumes at the origin");
    assert!(b.end >= 3_900 && b.end <= 4_000, "flankA end {}", b.end);
    assert!(c.start >= 5_200 && c.start <= 5_300, "flankB start {}", c.start);

    // The 19 bp insert is reported as insertion bases anchored at the
    // junction (allowing a few columns of equivalent left/right sliding).
    let junction_bases: usize = aln
        .insertions
        .iter()
        .filter(|i| i.pos + 30 > b.end && i.pos <= c.start + 30)
        .map(|i| i.bases.len())
        .sum();
    assert!(
        junction_bases >= insert.len(),
        "junction insertions {:?}",
        aln.insertions
    );

    // The walk rebuilds the read exactly, in read order.
    assert_eq!(rebuild_read(&aln), aln.seq, "model must rebuild the read");

    assert!(aln.identity >= 0.98, "identity {}", aln.identity);

    // The template gap between the flanks is reported as a deletion.
    let diff = libregene_core::align::alignment_diff(&aln, &t);
    let gap = diff
        .deletions
        .iter()
        .find(|d| d.pos > b.end && d.pos + d.length <= c.start + 1)
        .unwrap_or_else(|| panic!("no inter-flank deletion: {:?}", diff.deletions));
    assert!((gap.length as i64 - 1_200).abs() <= 30, "gap {gap:?}");
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
