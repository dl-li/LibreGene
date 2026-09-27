//! The near-identical full-length path must return the optimal alignment, and
//! a round plasmid read that carries one indel must come back as one insertion
//! at that place — not as a scatter of small ones with compensating deletions.
//!
//! Two bugs lived here:
//!   * the banded Gotoh traceback ignored the "gap extended" bits, so every
//!     gap run longer than one base was rebuilt with diagonal steps mixed in
//!     (a clean 30bp insertion came out as ten small runs), while the reported
//!     score stayed the optimal one;
//!   * the rotation search probed only a handful of subject positions, so a
//!     read carrying an indel yielded only drifted offsets (each probe after
//!     the indel shifts the subject↔query mapping by the indel's length) and
//!     the DP had to absorb the difference with a large compensating
//!     insertion+deletion pair.
//!
//! Real data: the shipped pVA task (pVA-MCS.dna + pVA-read-1.ab1).

use libregene_core::align::blastn::{full_length_hits, ColType};
use libregene_core::align::{align_read_checked_with, realign_project_with, AlignAlgorithm};
use libregene_core::models::{Alignment, ProjectData};
use std::path::Path;

fn test_data(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("test_data")
        .join(name)
}

/// Insertion runs of a hit/model, as (anchor, length).
fn insertion_runs(cols: &[libregene_core::align::blastn::AlignColumn]) -> Vec<(u64, usize)> {
    let mut runs: Vec<(u64, usize)> = Vec::new();
    for c in cols {
        if c.col_type != ColType::Insertion {
            continue;
        }
        match runs.last_mut() {
            Some(last) if last.0 == c.ref_position => last.1 += 1,
            _ => runs.push((c.ref_position, 1)),
        }
    }
    runs
}

/// The read bases the display walk emits, in walk order (mirrors the frontend).
fn walked_read(aln: &Alignment) -> String {
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

fn model_insertion_runs(aln: &Alignment) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for ins in &aln.insertions {
        match runs.last_mut() {
            Some(last) if last.0 + last.1 == ins.pos => {
                last.1 += ins.bases.len();
            }
            _ => runs.push((ins.pos, ins.bases.len())),
        }
    }
    runs
}

/// A perfect 30bp insertion (the template's own piece put back) must come out
/// as one run even though the DP's optimum always was one run: the traceback
/// used to chop it up.
#[test]
fn perfect_insertion_is_a_single_run() {
    let tpl = libregene_core::file_io::dna::parse_dna(&test_data("pVA-MCS.dna")).unwrap();
    let piece = &tpl.sequence[1000..1030];
    let query = format!("{}{}{}", &tpl.sequence[..1000], piece, &tpl.sequence[1000..]);

    for (label, q) in [
        ("plain", query.clone()),
        (
            "rotated",
            format!("{}{}", &query[2000..], &query[..2000]),
        ),
    ] {
        let hit = full_length_hits(tpl.sequence.as_bytes(), q.as_bytes())
            .unwrap_or_else(|| panic!("{label}: no full-length hit"));
        let runs = insertion_runs(&hit.columns);
        assert_eq!(runs.len(), 1, "{label}: runs {runs:?}");
        assert_eq!(runs[0].1, 30, "{label}: runs {runs:?}");
        assert!(
            hit.columns.iter().all(|c| c.col_type != ColType::Deletion),
            "{label}: no template bases may be dropped"
        );
        assert!((hit.identity - 1.0).abs() < 1e-9, "{label}: identity {}", hit.identity);
    }
}

/// Editing the template must leave the reads' alignments as one clean insertion
/// at the edit point, with nothing left that splits the template row into
/// pieces shorter than five bases.
#[test]
fn edited_template_keeps_one_insertion() {
    let tpl = libregene_core::file_io::dna::parse_dna(&test_data("pVA-MCS.dna")).unwrap();
    let read = libregene_core::file_io::ab1::parse_ab1(&test_data("pVA-read-1.ab1")).unwrap();
    let circular = tpl.topology == "circular";

    let (del_start, del_len) = (1000usize, 30usize);
    let mut edited = tpl.sequence.clone();
    edited.replace_range(del_start..del_start + del_len, "");

    let mut aln = align_read_checked_with(&tpl.sequence, &read.sequence, circular, AlignAlgorithm::BlastN)
        .expect("initial alignment");
    aln.id = "aln-1".to_string();
    aln.name = "pVA-read-1".to_string();
    let mut p = ProjectData {
        sequence: edited.clone(),
        length: edited.len() as i64,
        topology: tpl.topology.clone(),
        ..Default::default()
    };
    p.alignments.push(aln);
    realign_project_with(&mut p, AlignAlgorithm::BlastN);

    let aln = &p.alignments[0];
    let runs = model_insertion_runs(aln);
    let big: Vec<(usize, usize)> = runs.iter().copied().filter(|(_, len)| *len >= 20).collect();
    assert_eq!(big.len(), 1, "one long insertion expected, got runs {runs:?}");
    assert!(
        big[0].1.abs_diff(del_len) <= 3,
        "the insertion should cover the deleted {del_len}bp: {runs:?}"
    );
    assert!(
        big[0].0.abs_diff(del_start) <= 3,
        "the insertion should sit at the edit: {runs:?}"
    );
    // No isolated template piece shorter than five bases between insertions.
    let mut anchors: Vec<usize> = runs.iter().map(|(pos, _)| *pos).collect();
    anchors.sort_unstable();
    for w in anchors.windows(2) {
        assert!(
            w[1] - w[0] - 1 >= 5,
            "short template piece between {} and {}: runs {runs:?}",
            w[0],
            w[1]
        );
    }
    // Read bases are untouched and still in order.
    assert_eq!(walked_read(aln), aln.seq);
    // The alignment is otherwise clean: at most a handful of mismatches.
    let mismatches = aln
        .segments
        .iter()
        .map(|s| s.chars.matches('-').count())
        .sum::<usize>();
    assert!(mismatches <= 4, "read gaps {mismatches}: check the edit mapping");
}
