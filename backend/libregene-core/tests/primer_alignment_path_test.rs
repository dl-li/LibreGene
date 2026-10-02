//! Regression tests: primer display alignment must stay complete and
//! unfragmented around internal indels (same principle as the read
//! aligner's full-length Gotoh fix — one real indel is ONE contiguous gap
//! run, never shattered into alternating mismatch/gap fragments by chance
//! matches, and never drifted into a frameshifted alignment).

use libregene_core::primer::alignment::{
    align_3prime_constrained, align_first_base_constrained_rev, Op,
};
use libregene_core::utils::reverse_complement;

const P: &[u8] = b"ATGCATGCGGCCGATCGTACGATCGGATCCGACT";

/// 24 bp insertion sharing no 4-mer with the primer.
const INS24: &[u8] = b"AATTCCAATTCCAATTCCAATTCC";

fn runs_of(ops: &[libregene_core::primer::alignment::AlignedPair]) -> Vec<(Op, usize)> {
    let mut runs: Vec<(Op, usize)> = Vec::new();
    for op in ops {
        if let Some(last) = runs.last_mut() {
            if last.0 == op.op {
                last.1 += 1;
                continue;
            }
        }
        runs.push((op.op, 1));
    }
    runs
}

fn region_with(parts: &[&[u8]]) -> Vec<u8> {
    let mut v = b"GGGGGGGGGG".to_vec();
    for p in parts {
        v.extend_from_slice(p);
    }
    v.extend_from_slice(b"GGGGGGGGGG");
    v
}

#[test]
fn template_insertion_5p_half_fwd_bridges_as_one_run() {
    let tpl = region_with(&[&P[..18], INS24, &P[18..]]);
    let r = align_3prime_constrained(P, &tpl).expect("must align");
    assert_eq!(
        runs_of(&r.ops),
        vec![(Op::Match, 18), (Op::Del, 24), (Op::Match, 16)],
        "one contiguous 24bp gap run: {:?}",
        runs_of(&r.ops)
    );
    // Every primer base participates — the alignment is complete.
    assert_eq!(r.primer_end - r.primer_start, P.len());
}

#[test]
fn template_deletion_mid_fwd_bridges_as_one_run() {
    // 8 template bases missing: the primer carries 8 extra bases.
    let tpl = region_with(&[&P[..18], &P[26..]]);
    let r = align_3prime_constrained(P, &tpl).expect("must align");
    assert_eq!(
        runs_of(&r.ops),
        vec![(Op::Match, 18), (Op::Ins, 8), (Op::Match, 8)],
        "one contiguous 8bp gap run: {:?}",
        runs_of(&r.ops)
    );
    assert_eq!(r.primer_end - r.primer_start, P.len());
}

#[test]
fn template_insertion_near_3prime_end_anchors_after_it() {
    // 10bp insertion 6 bases from the 3' end: the 3' terminal must land on
    // the template base AFTER the insertion (true extension position), with
    // the insertion as a single gap run — not parked on a coincidental base
    // inside the insertion.
    let ins = b"TTAGGCCTAG";
    let tpl = region_with(&[&P[..28], ins, &P[28..]]);
    let r = align_3prime_constrained(P, &tpl).expect("must align");
    assert_eq!(
        runs_of(&r.ops),
        vec![(Op::Match, 28), (Op::Del, 10), (Op::Match, 6)],
        "3' end must anchor past the insertion: {:?}",
        runs_of(&r.ops)
    );
    // Region coords: flank 10 + 28 matched + 10 inserted + 6 matched.
    assert_eq!(r.template_end, 10 + 28 + 10 + 6);
}

#[test]
fn template_insertion_5p_half_rev_bridges_as_one_run() {
    // Reverse primer: template carries RC(primer) with the insertion inside.
    let rc = reverse_complement(std::str::from_utf8(P).unwrap());
    let tpl = region_with(&[&rc.as_bytes()[..18], INS24, &rc.as_bytes()[18..]]);
    let rev_p: Vec<u8> = P.iter().rev().copied().collect();
    let r = align_first_base_constrained_rev(&rev_p, &tpl).expect("must align");
    assert_eq!(
        runs_of(&r.ops),
        vec![(Op::Match, 17), (Op::Del, 24), (Op::Match, 17)],
        "rev: one contiguous 24bp gap run: {:?}",
        runs_of(&r.ops)
    );
    assert_eq!(r.primer_end - r.primer_start, P.len());
}

#[test]
fn single_snps_never_render_as_gaps() {
    // A single substituted base at various depths stays a mismatch column: a
    // 1bp gap (−6) always loses to a mismatch (−4).
    let primer = b"ATGCGGCCGATCGTACGATCGGATCCGACT";
    for pos in [3usize, 12, 20, 27] {
        let mut tpl = primer.to_vec();
        tpl[pos] = match tpl[pos] {
            b'A' => b'T',
            b'C' => b'A',
            b'G' => b'T',
            _ => b'A',
        };
        let r = align_3prime_constrained(primer, &tpl)
            .unwrap_or_else(|| panic!("SNP at {pos} must align"));
        assert!(
            r.ops.iter().all(|p| p.op != Op::Del && p.op != Op::Ins),
            "SNP at {pos} produced a gap: {:?}",
            runs_of(&r.ops)
        );
        let mismatches = r.ops.iter().filter(|p| p.op == Op::Mismatch).count();
        assert_eq!(mismatches, 1, "SNP at {pos}");
    }
}
