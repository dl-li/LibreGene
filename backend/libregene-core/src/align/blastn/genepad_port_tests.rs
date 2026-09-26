//! Behavioural tests ported from GenePad's vitest suites
//! (blastNormalize.test.ts, blastEdge.test.ts, blastFullLength.test.ts,
//! alignmentInsertionCoverage.test.ts) plus coverage of the multi-segment
//! Alignment conversion. Included from blastn/mod.rs.

use super::*;
use crate::align::{blast_align_read, alignment_diff, align_read_with, AlignAlgorithm, AlignReject, MIN_ALIGNED_LEN};

// xorshift32 taking the high 2 bits: LCG low bits have long same-value runs
// that DUST masks (same generator as the GenePad tests).
struct XorShift(u32);
impl XorShift {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn base(&mut self) -> u8 {
        b"ACGT"[(self.next() >> 16) as usize & 3]
    }
    fn seq(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.base()).collect()
    }
}

fn hit_query_range(hit: &ColumnHit) -> (u64, u64) {
    let positions: Vec<u64> = hit
        .columns
        .iter()
        .filter(|c| c.query_position > 0)
        .map(|c| c.query_position)
        .collect();
    (*positions.iter().min().unwrap(), *positions.iter().max().unwrap())
}

fn hit_ref_coverage(hit: &ColumnHit) -> usize {
    hit.columns
        .iter()
        .filter(|c| c.ref_position > 0 && c.ref_base != b'-')
        .map(|c| c.ref_position)
        .collect::<std::collections::HashSet<_>>()
        .len()
}

fn hit_query_coverage(hit: &ColumnHit) -> usize {
    hit.columns
        .iter()
        .filter(|c| c.query_position > 0 && c.query_base != b'-')
        .map(|c| c.query_position)
        .collect::<std::collections::HashSet<_>>()
        .len()
}

fn last_query_pos(hit: &ColumnHit) -> u64 {
    hit.columns
        .iter()
        .filter(|c| c.query_base != b'-')
        .map(|c| c.query_position)
        .max()
        .unwrap_or(0)
}

fn first_query_pos(hit: &ColumnHit) -> u64 {
    hit.columns
        .iter()
        .filter(|c| c.query_base != b'-')
        .map(|c| c.query_position)
        .min()
        .unwrap_or(0)
}

/// --- blastFullLength.test.ts ---
#[test]
fn full_length_16kb_near_identical_single_hit() {
    let flank = b"ACGTACGT".repeat(1000);
    let ref_mid = b"CAGTTTTACGCATATACAGAATTCAGCTAA"
        .to_vec()
        .into_iter()
        .chain(b"TTTTTTTTTTTTTTTTTT".iter().copied())
        .chain(b"GATTACAGATTACA".iter().copied())
        .collect::<Vec<u8>>();
    let query_mid = b"TAAGTGAAAGCTATTTCTCTGTATAACTCAGTTTTACGCATATACAGAATTCAGCTAA"
        .to_vec()
        .into_iter()
        .chain(b"GATTACAGATTACA".iter().copied())
        .collect::<Vec<u8>>();
    let mut ref_seq = flank.clone();
    ref_seq.extend_from_slice(&ref_mid);
    ref_seq.extend_from_slice(&flank);
    let mut query = flank.clone();
    query.extend_from_slice(&query_mid);
    query.extend_from_slice(&flank);

    let hit = full_length::try_full_length(&ref_seq, &query, Strand::Plus).expect("full-length hit");
    assert_eq!(hit_ref_coverage(&hit), ref_seq.len(), "100% ref coverage");
    assert_eq!(hit_query_coverage(&hit), query.len(), "100% query coverage");
    assert!(hit.identity > 0.9);
}

#[test]
fn full_length_32kb_no_oom_single_hit() {
    let flank = b"ACGTACGT".repeat(2000);
    let ref_mid = b"CAGTTTTACGCATATACAGAATTCAGCTAA"
        .to_vec()
        .into_iter()
        .chain(b"TTTTTTTTTTTTTTTTTT".iter().copied())
        .chain(b"GATTACAGATTACA".iter().copied())
        .collect::<Vec<u8>>();
    let query_mid = b"TAAGTGAAAGCTATTTCTCTGTATAACTCAGTTTTACGCATATACAGAATTCAGCTAA"
        .to_vec()
        .into_iter()
        .chain(b"GATTACAGATTACA".iter().copied())
        .collect::<Vec<u8>>();
    let mut ref_seq = flank.clone();
    ref_seq.extend_from_slice(&ref_mid);
    ref_seq.extend_from_slice(&flank);
    let mut query = flank.clone();
    query.extend_from_slice(&query_mid);
    query.extend_from_slice(&flank);

    let hit = full_length::try_full_length(&ref_seq, &query, Strand::Plus).expect("full-length hit");
    assert_eq!(hit_ref_coverage(&hit), ref_seq.len());
    assert_eq!(hit_query_coverage(&hit), query.len());
}

#[test]
fn full_length_clean_single_insertion_one_gap_run() {
    let flank = b"ACGTACGT".repeat(20);
    let insert = b"GGGGGGGGGG";
    let mut ref_seq = flank.clone();
    ref_seq.extend_from_slice(&flank);
    let mut query = flank.clone();
    query.extend_from_slice(insert);
    query.extend_from_slice(&flank);

    let hit = full_length::try_full_length(&ref_seq, &query, Strand::Plus).expect("full-length hit");
    assert_eq!(hit_ref_coverage(&hit), ref_seq.len());
    assert_eq!(hit_query_coverage(&hit), query.len());
    let ins = hit.columns.iter().filter(|c| c.col_type == ColType::Insertion).count();
    let del = hit.columns.iter().filter(|c| c.col_type == ColType::Deletion).count();
    let mis = hit.columns.iter().filter(|c| c.col_type == ColType::Mismatch).count();
    assert_eq!(ins, insert.len());
    assert_eq!(del, 0);
    assert_eq!(mis, 0);
    // one contiguous gap run
    let mut longest = 0usize;
    let mut cur = 0usize;
    for c in &hit.columns {
        if c.ref_base == b'-' || c.query_base == b'-' {
            cur += 1;
            longest = longest.max(cur);
        } else {
            cur = 0;
        }
    }
    assert_eq!(longest, insert.len());
}

#[test]
fn full_length_divergent_below_identity_floor_returns_none() {
    let ref_seq = b"ACGTACGTACGTACGTACGTACGTACGTACGT";
    let query = b"TTTTAAAAGGGGCCCCAAAACCCCTTTTGGGG";
    assert!(full_length::try_full_length(ref_seq, query, Strand::Plus).is_none());
}

/// --- blastEdge.test.ts: band-local trace crash regressions ---
#[test]
fn full_length_survives_many_small_random_pairs() {
    let mut rnd = XorShift(12345);
    for _ in 0..400 {
        let n = 20 + (rnd.next() % 60) as usize;
        let m = 20 + (rnd.next() % 60) as usize;
        let ref_seq = rnd.seq(n);
        let query = rnd.seq(m);
        let _ = full_length::try_full_length(&ref_seq, &query, Strand::Plus);
        let rc = seqblk::reverse_complement(&query);
        let _ = full_length::try_full_length(&ref_seq, &rc, Strand::Minus);
    }
}

#[test]
fn full_length_never_panics_when_length_delta_exceeds_band_cap() {
    // |n - m| >= 4097 pins the band at its 4096 cap; the per-row trace store
    // must cover |delta| + 2*band + 1 columns or the last row writes past the
    // vector end (GenePad production crash).
    let mut rnd = XorShift(7);
    for _ in 0..8 {
        let n = 5000 + (rnd.next() % 1000) as usize;
        let m = 200 + (rnd.next() % 100) as usize;
        let ref_seq = rnd.seq(n);
        let query = rnd.seq(m);
        let _ = full_length::try_full_length(&ref_seq, &query, Strand::Plus);
    }
}

#[test]
fn blast_engine_reverse_strand_rebuilds_columns_from_reverse_complement() {
    let target = b"ATGCGTACGATCGTACCTAGGCTAACGTTAGCCTAGCTA";
    let mut ref_seq = b"GGGACCACTT".to_vec();
    ref_seq.extend_from_slice(target);
    ref_seq.extend_from_slice(b"TTAACCGGAA");
    let query = seqblk::reverse_complement(target);

    let hits = blast_hits(&ref_seq, &query).expect("reverse-complement hit");
    assert!(!hits.is_empty());
    let hit = &hits[0];
    assert_eq!(hit.strand, Strand::Minus);
    assert!(hit.identity > 0.95);
    assert!(hit.columns.iter().all(|c| c.col_type == ColType::Match));
    assert_eq!(hit.columns.first().map(|c| c.query_position), Some(1));
    assert_eq!(
        hit.columns.last().map(|c| c.query_position),
        Some(target.len() as u64)
    );
}

/// --- blastNormalize end-to-end test: noisy read stays one colinear
/// forward-ordered track ---
#[test]
fn noisy_read_keeps_colinear_forward_ordered_track() {
    let mut rnd = XorShift(20260915);
    let ref_seq = rnd.seq(8206);
    let target = ref_seq[4307..4880].to_vec();
    let mut query = rnd.seq(6);
    query.extend_from_slice(&target);
    query.extend_from_slice(&rnd.seq(262));

    let hits = blast_hits(&ref_seq, &query).expect("hits");
    assert!(!hits.is_empty());
    let strand = hits[0].strand;
    assert!(hits.iter().all(|h| h.strand == strand));
    for w in hits.windows(2) {
        let (q0, q1) = (hit_query_range(&w[0]), hit_query_range(&w[1]));
        assert!(q1.0 > q0.1, "query ranges must not zig-zag");
        assert!(w[1].ref_start > w[0].ref_end, "ref ranges must advance");
    }
    assert!(hits[0].identity > 0.95, "identity {}", hits[0].identity);
    let indices: Vec<usize> = hits.iter().map(|h| h.hit_index).collect();
    assert_eq!(indices, (0..hits.len()).collect::<Vec<_>>());
}

/// --- alignmentInsertionCoverage.test.ts: a mid-read insertion resembling
/// the flanking reference must surface as a junction insertion, not a silent
/// hole; every read base is either in a hit column or a junction gap ---
#[test]
fn mid_read_insertion_keeps_full_query_coverage() {
    let slice_start = 1200usize;
    let insert_length = 25usize;
    let insert_at = 300usize;
    let mutate_offsets = [0usize, 1, 3, 4, 7, 9, 10, 13, 15, 18, 19, 21];

    let mut rnd = XorShift(20260919);
    let ref_seq = rnd.seq(4000);
    let ref_slice = ref_seq[slice_start..slice_start + 600].to_vec();

    let mut insert = ref_slice[insert_at..insert_at + insert_length].to_vec();
    for &offset in &mutate_offsets {
        insert[offset] = match insert[offset] {
            b'A' => b'C',
            b'C' => b'G',
            b'G' => b'T',
            _ => b'A',
        };
    }
    let mut query = ref_slice[..insert_at].to_vec();
    query.extend_from_slice(&insert);
    query.extend_from_slice(&ref_slice[insert_at..]);

    let hits = blast_hits(&ref_seq, &query).expect("hits");
    assert!(!hits.is_empty());

    let hit_covered: std::collections::HashSet<u64> = hits
        .iter()
        .flat_map(|h| h.columns.iter())
        .filter(|c| c.query_position > 0 && c.query_base != b'-')
        .map(|c| c.query_position)
        .collect();
    // Reconstruct junction coverage: every read base between hit boundaries
    // belongs to a junction insertion.
    let mut covered = hit_covered.clone();
    for w in hits.windows(2) {
        let prev_last = last_query_pos(&w[0]);
        let next_first = first_query_pos(&w[1]);
        for q in prev_last..next_first {
            covered.insert(q);
        }
    }
    let first_first = first_query_pos(&hits[0]);
    let last_last = last_query_pos(hits.last().unwrap());
    for q in 1..first_first {
        covered.insert(q);
    }
    for q in last_last + 1..=query.len() as u64 {
        covered.insert(q);
    }
    for q in 1..=query.len() as u64 {
        assert!(covered.contains(&q), "read base {} uncovered", q);
    }

    // Both flanks must align as hits (10bp junction tolerance).
    for q in 1..=(insert_at - 10) as u64 {
        assert!(hit_covered.contains(&q), "left flank {} must be a hit", q);
    }
    for q in (insert_at + insert_length + 10) as u64..=query.len() as u64 {
        assert!(hit_covered.contains(&q), "right flank {} must be a hit", q);
    }

    // The inserted bases must land in the junction(s).
    let mut junction = Vec::new();
    for w in hits.windows(2) {
        let prev_last = last_query_pos(&w[0]) as usize;
        let next_first = first_query_pos(&w[1]) as usize;
        junction.extend_from_slice(&query[prev_last..next_first - 1]);
    }
    let core = &insert[1..insert.len() - 1];
    assert!(
        junction.windows(core.len()).any(|w| w == core),
        "junction must contain the inserted bases, got {}",
        String::from_utf8_lossy(&junction)
    );
}

/// Leading/trailing unaligned read tails stay outside the core hit
/// (junction tolerance ±3), and the core hit keeps near-full identity.
#[test]
fn read_with_unaligned_tails_keeps_core_hit() {
    let mut rnd = XorShift(42);
    let ref_seq = rnd.seq(3000);
    let core = ref_seq[500..900].to_vec();
    let leading = rnd.seq(33);
    let trailing = rnd.seq(47);
    let mut query = leading.clone();
    query.extend_from_slice(&core);
    query.extend_from_slice(&trailing);

    let hits = blast_hits(&ref_seq, &query).expect("hits");
    assert!(!hits.is_empty());
    let hit = &hits[0];
    let (q_start, q_end) = hit_query_range(hit);
    assert!(q_start <= 34 + 3, "q_start {}", q_start);
    assert!(q_end >= (33 + 400 - 3) as u64, "q_end {}", q_end);
    assert!(hit.identity > 0.95, "identity {}", hit.identity);
}

/// --- multi-segment conversion: a synthetic split read (two distant
/// template loci + junction insert) becomes one Alignment with 2 segments;
/// the template gap between the flanks is reported as a deletion ---
#[test]
fn blast_split_read_two_segments_conversion() {
    let mut rnd = XorShift(31337);
    let template = rnd.seq(3000);
    let mut read = template[0..500].to_vec();
    read.extend_from_slice(b"ACGTTACGCA");
    read.extend_from_slice(&template[1500..2000]);
    let read_len = read.len();

    let aln = blast_align_read(
        &String::from_utf8(template.clone()).unwrap(),
        &String::from_utf8(read).unwrap(),
        false,
    )
    .expect("blast split alignment");
    assert_eq!(aln.strand, "+");
    assert_eq!(aln.segments.len(), 2, "segments {:?}", aln.segments);
    assert!(aln.segments[0].start <= 2, "seg0 {:?}", aln.segments[0]);
    assert!(aln.segments[0].end >= 498, "seg0 {:?}", aln.segments[0]);
    assert!(aln.segments[1].start >= 1498, "seg1 {:?}", aln.segments[1]);
    assert!(aln.segments[1].end <= 2000, "seg1 {:?}", aln.segments[1]);
    assert!(aln.insertions.len() >= 1, "junction insert {:?}", aln.insertions);
    assert!(aln.identity >= 0.6, "identity {}", aln.identity);
    assert_eq!(aln.length, read_len);

    let diff = alignment_diff(&aln, &String::from_utf8(template).unwrap());
    let gap_del = diff.deletions.iter().find(|d| d.length >= 990);
    assert!(gap_del.is_some(), "deletions {:?}", diff.deletions);
}

#[test]
fn blast_perfect_match_conversion_full_length() {
    let mut rnd = XorShift(777);
    let template = rnd.seq(3000);
    let read = template[500..1200].to_vec();
    let aln = blast_align_read(
        &String::from_utf8(template).unwrap(),
        &String::from_utf8(read.clone()).unwrap(),
        false,
    )
    .expect("alignment");
    assert_eq!(aln.strand, "+");
    assert_eq!(aln.segments.len(), 1);
    assert_eq!(aln.segments[0].start, 500);
    assert_eq!(aln.segments[0].end, 1199);
    assert_eq!(aln.segments[0].chars, String::from_utf8(read).unwrap());
    assert!((aln.identity - 1.0).abs() < 1e-9);
    assert!(aln.insertions.is_empty());
}

#[test]
fn blast_circular_origin_spanning_read_two_segments() {
    let mut rnd = XorShift(9001);
    let template = rnd.seq(3000);
    let mut read = template[2800..3000].to_vec();
    read.extend_from_slice(&template[0..200]);
    let aln = blast_align_read(
        &String::from_utf8(template.clone()).unwrap(),
        &String::from_utf8(read).unwrap(),
        true,
    )
    .expect("alignment");
    assert_eq!(aln.segments.len(), 2, "segments {:?}", aln.segments);
    let (a, b) = (&aln.segments[0], &aln.segments[1]);
    assert!(a.start >= 2798 && a.end == 2999, "seg0 {:?}", a);
    assert!(b.start == 0 && b.end <= 202, "seg1 {:?}", b);
    assert!(aln.identity > 0.95, "identity {}", aln.identity);
    assert_eq!(
        aln.segments[0].chars,
        String::from_utf8(template[2800..3000].to_vec()).unwrap()
    );
}

#[test]
fn blast_divergent_read_rejected() {
    let mut rnd = XorShift(424242);
    let template = rnd.seq(5000);
    let read = rnd.seq(3000);
    let err = blast_align_read(
        &String::from_utf8(template).unwrap(),
        &String::from_utf8(read).unwrap(),
        false,
    )
    .unwrap_err();
    assert_eq!(err, AlignReject::NoSignificantAlignment);
}

#[test]
fn blast_too_short_rejected() {
    let mut rnd = XorShift(5150);
    let template = rnd.seq(2000);
    let read = template[100..130].to_vec();
    let err = blast_align_read(
        &String::from_utf8(template).unwrap(),
        &String::from_utf8(read).unwrap(),
        false,
    )
    .unwrap_err();
    assert_eq!(err, AlignReject::TooShort { span: 30 });
    assert_eq!(MIN_ALIGNED_LEN, 50);
}

#[test]
fn algorithm_parse_and_defaults() {
    assert_eq!(AlignAlgorithm::parse("blast"), Some(AlignAlgorithm::BlastN));
    assert_eq!(AlignAlgorithm::parse("blastn"), Some(AlignAlgorithm::BlastN));
    assert_eq!(
        AlignAlgorithm::parse("smith-waterman"),
        Some(AlignAlgorithm::SmithWaterman)
    );
    assert_eq!(AlignAlgorithm::parse("SW"), Some(AlignAlgorithm::SmithWaterman));
    assert_eq!(AlignAlgorithm::parse("nonsense"), None);
    assert_eq!(AlignAlgorithm::default(), AlignAlgorithm::BlastN);
}

#[test]
fn align_read_with_selects_engine() {
    let mut rnd = XorShift(616);
    let template = rnd.seq(2000);
    let read = template[300..800].to_vec();
    let t = String::from_utf8(template).unwrap();
    let r = String::from_utf8(read).unwrap();

    for algorithm in [AlignAlgorithm::SmithWaterman, AlignAlgorithm::BlastN] {
        let aln = align_read_with(&t, &r, false, algorithm).unwrap();
        assert_eq!(aln.strand, "+");
        assert_eq!(aln.segments[0].start, 300);
        assert_eq!(aln.segments[0].end, 799);
        assert!((aln.identity - 1.0).abs() < 1e-9);
    }
}

/// Unaligned read tails (leading/trailing junk) must be stored as junction
/// insertions so every read base is either in a hit column or an insertion —
/// GenePad's gapSegments "no holes" invariant. Without them the frontend
/// chromatogram mapping (which counts read bases from index 0) shifts by the
/// leading length and paints the wrong peaks under every column.
#[test]
fn blast_read_tails_stored_as_insertions() {
    let mut rnd = XorShift(20260926);
    let template = rnd.seq(3000);
    let leading = rnd.seq(21);
    let trailing = rnd.seq(14);
    let mut read = leading.clone();
    read.extend_from_slice(&template[500..1200]);
    read.extend_from_slice(&trailing);

    let aln = blast_align_read(
        &String::from_utf8(template).unwrap(),
        &String::from_utf8(read.clone()).unwrap(),
        false,
    )
    .expect("alignment");
    assert_eq!(aln.segments.len(), 1, "segments {:?}", aln.segments);
    let seg_start = aln.segments[0].start;
    let seg_end = aln.segments[0].end;
    assert!((seg_start as i64 - 500).abs() <= 1, "seg start {}", seg_start);
    assert!((seg_end as i64 - 1199).abs() <= 1, "seg end {}", seg_end);

    let mapped: usize = aln
        .segments
        .iter()
        .map(|s| s.chars.bytes().filter(|&c| c != b'-').count())
        .sum();
    let ins_bases: usize = aln.insertions.iter().map(|i| i.bases.len()).sum();
    assert_eq!(mapped + ins_bases, read.len(), "no read base may be dropped");

    // Leading tail anchored at the first template column of the first
    // segment; the engine may absorb a trailing base or two of the junk into
    // the HSP along its diagonal, so only the prefix is guaranteed.
    let lead = aln
        .insertions
        .iter()
        .find(|i| i.pos == seg_start)
        .expect("leading insertion at segment start");
    assert!(
        String::from_utf8(leading.clone()).unwrap().starts_with(&lead.bases),
        "leading {:?} must be a prefix of the read's junk tail",
        lead.bases
    );
    // Trailing tail anchored right after the last aligned column.
    let tail = aln
        .insertions
        .iter()
        .find(|i| i.pos == seg_end + 1)
        .expect("trailing insertion after segment end");
    assert!(
        String::from_utf8(trailing.clone()).unwrap().ends_with(&tail.bases),
        "trailing {:?} must be a suffix of the read's junk tail",
        tail.bases
    );
}

