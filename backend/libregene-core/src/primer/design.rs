//! Primer design candidate generation — ports
//! `src/plugins/primerDesign/candidates.js` (`buildAmplifyGroups`,
//! `buildOepcrGroups`, `buildMutagenesisGroups`).
//!
//! The injected JS `tmOf` callback is replaced by the nearest-neighbour
//! [`TmParams`]-based engine from [`super::thermodynamics`], rounded to one
//! decimal like the Tauri `compute_tm` command does.

use serde::Serialize;

use super::thermodynamics::{compute_tm_with_params, TmParams};
use crate::models::Segment;

/// A single primer candidate (one anneal-core length variant).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimerCandidate {
    /// Full primer sequence 5'→3' (tail + anneal core).
    pub seq: String,
    /// 5' tail length in bases.
    pub tail_len: usize,
    /// Anneal-core length in bases.
    pub anneal_len: usize,
    /// Melting temperature of the anneal core (°C), rounded to 0.1.
    pub tm: f64,
    /// GC% of the full primer sequence, one decimal.
    pub gc: f64,
}

/// One designed primer (fwd/rev) with its length variants.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimerGroup {
    pub name: String,
    /// "fwd" | "rev"
    #[serde(rename = "type")]
    pub r#type: String,
    /// Variants ordered by anneal-core length, ascending.
    pub candidates: Vec<PrimerCandidate>,
    /// Index of the candidate whose Tm is closest to the target Tm.
    pub default_index: usize,
}

fn rev_comp(s: &str) -> String {
    s.chars()
        .rev()
        .map(|c| match c.to_ascii_uppercase() {
            'A' => 'T',
            'C' => 'G',
            'G' => 'C',
            'T' => 'A',
            _ => 'N',
        })
        .collect()
}

fn gc_percent(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let gc = s
        .bytes()
        .filter(|b| matches!(b, b'G' | b'g' | b'C' | b'c'))
        .count();
    ((gc as f64 / s.len() as f64) * 1000.0).round() / 10.0
}

/// Modulo indexing so anneal/tail regions can wrap across the origin of a
/// circular sequence; linear templates clamp to the sequence bounds.
fn slice_wrap(seq: &str, start: i64, len: usize, circular: bool) -> String {
    let n = seq.len();
    if n == 0 {
        return String::new();
    }
    if !circular {
        let s = start.max(0) as usize;
        let e = (start + len as i64).clamp(0, n as i64) as usize;
        if s >= e {
            return String::new();
        }
        return seq[s..e].to_string();
    }
    let bytes = seq.as_bytes();
    let n64 = n as i64;
    let mut out = String::with_capacity(len);
    for i in 0..len as i64 {
        let idx = ((start + i) % n64 + n64) % n64;
        out.push(bytes[idx as usize] as char);
    }
    out
}

fn round1(tm: f64) -> f64 {
    (tm * 10.0).round() / 10.0
}

/// Shortest anneal-core length (18..40) whose Tm reaches `target_tm`.
fn core_len_for_tm(
    anneal: impl Fn(usize) -> String,
    target_tm: f64,
    tm_of: impl Fn(&str) -> f64,
) -> usize {
    let mut len = 18;
    while len < 40 {
        if round1(tm_of(&anneal(len))) >= target_tm {
            break;
        }
        len += 1;
    }
    len
}

/// Build the ±3 length variants and pick the default (closest Tm to target).
fn build_variants(
    build: impl Fn(usize) -> (String, String, usize),
    core_len: usize,
    target_tm: f64,
    tm_of: impl Fn(&str) -> f64,
) -> (Vec<PrimerCandidate>, usize) {
    let mut candidates = Vec::with_capacity(7);
    for l in core_len.saturating_sub(3)..=core_len + 3 {
        let (seq, anneal, tail_len) = build(l);
        candidates.push(PrimerCandidate {
            tm: round1(tm_of(&anneal)),
            gc: gc_percent(&seq),
            seq,
            tail_len,
            anneal_len: anneal.len(),
        });
    }
    let mut default_index = 0;
    for (i, c) in candidates.iter().enumerate() {
        if (c.tm - target_tm).abs() < (candidates[default_index].tm - target_tm).abs() {
            default_index = i;
        }
    }
    (candidates, default_index)
}

fn make_group(
    name: &str,
    group_type: &str,
    anneal_fn: impl Fn(usize) -> String,
    tail: String,
    target_tm: f64,
    tm_of: impl Fn(&str) -> f64,
) -> PrimerGroup {
    let core_len = core_len_for_tm(&anneal_fn, target_tm, &tm_of);
    let tail_len = tail.len();
    let (candidates, default_index) = build_variants(
        move |l| {
            let anneal = anneal_fn(l);
            let seq = format!("{tail}{anneal}");
            (seq, anneal, tail_len)
        },
        core_len,
        target_tm,
        tm_of,
    );
    PrimerGroup {
        name: name.to_string(),
        r#type: group_type.to_string(),
        candidates,
        default_index,
    }
}

fn build_amplify_groups_with(
    seq: &str,
    seg: &Segment,
    name: &str,
    target_tm: f64,
    topology: &str,
    tm_of: impl Fn(&str) -> f64,
) -> Vec<PrimerGroup> {
    let circular = topology == "circular";
    let fwd_anneal = |l: usize| slice_wrap(seq, seg.start, l, circular);
    let rev_anneal = |l: usize| rev_comp(&slice_wrap(seq, seg.end - l as i64 + 1, l, circular));
    vec![
        make_group(
            &format!("{name}-Fwd"),
            "fwd",
            fwd_anneal,
            String::new(),
            target_tm,
            &tm_of,
        ),
        make_group(
            &format!("{name}-Rev"),
            "rev",
            rev_anneal,
            String::new(),
            target_tm,
            tm_of,
        ),
    ]
}

/// Amplification primers flanking a target segment (no tails).
pub fn build_amplify_groups(
    seq: &str,
    seg: &Segment,
    name: &str,
    target_tm: f64,
    topology: &str,
    params: &TmParams,
) -> Vec<PrimerGroup> {
    build_amplify_groups_with(seq, seg, name, target_tm, topology, |s| {
        compute_tm_with_params(s, params)
    })
}

fn build_oepcr_groups_with(
    seq: &str,
    seg1: &Segment,
    seg2: &Segment,
    name1: &str,
    name2: &str,
    target_tm: f64,
    overlap_len: usize,
    topology: &str,
    tm_of: impl Fn(&str) -> f64,
) -> Vec<PrimerGroup> {
    let circular = topology == "circular";
    let seg1_fwd_anneal = |l: usize| slice_wrap(seq, seg1.start, l, circular);
    let seg1_rev_anneal =
        |l: usize| rev_comp(&slice_wrap(seq, seg1.end - l as i64 + 1, l, circular));
    let seg2_fwd_anneal = |l: usize| slice_wrap(seq, seg2.start, l, circular);
    let seg2_rev_anneal =
        |l: usize| rev_comp(&slice_wrap(seq, seg2.end - l as i64 + 1, l, circular));
    let seg1_end_tail = rev_comp(&slice_wrap(
        seq,
        seg1.end - overlap_len as i64 + 1,
        overlap_len,
        circular,
    ));
    let seg2_start_tail = slice_wrap(seq, seg2.start, overlap_len, circular);

    vec![
        make_group(
            &format!("{name1}-Fwd"),
            "fwd",
            seg1_fwd_anneal,
            String::new(),
            target_tm,
            &tm_of,
        ),
        make_group(
            &format!("{name1}-Rev"),
            "rev",
            seg1_rev_anneal,
            seg2_start_tail,
            target_tm,
            &tm_of,
        ),
        make_group(
            &format!("{name2}-Fwd"),
            "fwd",
            seg2_fwd_anneal,
            seg1_end_tail,
            target_tm,
            &tm_of,
        ),
        make_group(
            &format!("{name2}-Rev"),
            "rev",
            seg2_rev_anneal,
            String::new(),
            target_tm,
            tm_of,
        ),
    ]
}

/// Overlap-extension PCR primers for two adjacent segments; each inner primer
/// carries the overlap of the other segment as a 5' tail.
pub fn build_oepcr_groups(
    seq: &str,
    seg1: &Segment,
    seg2: &Segment,
    name1: &str,
    name2: &str,
    target_tm: f64,
    overlap_len: usize,
    topology: &str,
    params: &TmParams,
) -> Vec<PrimerGroup> {
    build_oepcr_groups_with(
        seq,
        seg1,
        seg2,
        name1,
        name2,
        target_tm,
        overlap_len,
        topology,
        |s| compute_tm_with_params(s, params),
    )
}

fn build_mutagenesis_groups_with(
    seq: &str,
    seg: &Segment,
    site_name: &str,
    mut_seq: &str,
    target_tm: f64,
    arm_len: usize,
    tm_of: impl Fn(&str) -> f64,
) -> Vec<PrimerGroup> {
    let mut_clean: String = mut_seq
        .to_ascii_uppercase()
        .chars()
        .filter(|c| matches!(c, 'A' | 'C' | 'G' | 'T'))
        .collect();
    let up_arm = slice_wrap(seq, seg.start - arm_len as i64, arm_len, true);
    let down_arm = slice_wrap(seq, seg.end + 1, arm_len, true);
    let fwd_anneal = |l: usize| slice_wrap(seq, seg.end + 1, l, true);
    let rev_anneal = |l: usize| rev_comp(&slice_wrap(seq, seg.start - l as i64, l, true));
    let fwd_tail = format!("{up_arm}{mut_clean}");
    let rev_tail = rev_comp(&format!("{down_arm}{mut_clean}"));

    vec![
        make_group(
            &format!("{site_name}-Fwd"),
            "fwd",
            fwd_anneal,
            fwd_tail,
            target_tm,
            &tm_of,
        ),
        make_group(
            &format!("{site_name}-Rev"),
            "rev",
            rev_anneal,
            rev_tail,
            target_tm,
            tm_of,
        ),
    ]
}

/// Site-directed mutagenesis primers replacing `seg` with `mut_seq`; the
/// mutation is carried as a 5' tail between homology arms and the anneal core.
pub fn build_mutagenesis_groups(
    seq: &str,
    seg: &Segment,
    site_name: &str,
    mut_seq: &str,
    target_tm: f64,
    arm_len: usize,
    params: &TmParams,
) -> Vec<PrimerGroup> {
    build_mutagenesis_groups_with(seq, seg, site_name, mut_seq, target_tm, arm_len, |s| {
        compute_tm_with_params(s, params)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEQ: &str = "GGATCCATGGCGTCGATCGATCGATCGAATTCGATCGATCGATCGGTACCGGATCCATGGCGTCGATCGATC";

    fn check_groups(
        groups: &[PrimerGroup],
        expected: &[(&str, &str, usize, &[(&str, usize, usize, f64, f64)])],
    ) {
        assert_eq!(groups.len(), expected.len());
        for (g, (name, group_type, default_index, candidates)) in groups.iter().zip(expected) {
            assert_eq!(g.name, *name);
            assert_eq!(g.r#type, *group_type);
            assert_eq!(g.default_index, *default_index);
            assert_eq!(g.candidates.len(), candidates.len());
            for (c, (seq, tail_len, anneal_len, tm, gc)) in g.candidates.iter().zip(*candidates) {
                assert_eq!(c.seq, *seq, "{}: seq mismatch", g.name);
                assert_eq!(c.tail_len, *tail_len, "{}: tailLen mismatch", g.name);
                assert_eq!(c.anneal_len, *anneal_len, "{}: annealLen mismatch", g.name);
                assert!(
                    (c.tm - tm).abs() < 1e-9,
                    "{}: tm {} != {}",
                    g.name,
                    c.tm,
                    tm
                );
                assert!(
                    (c.gc - gc).abs() < 1e-9,
                    "{}: gc {} != {}",
                    g.name,
                    c.gc,
                    gc
                );
            }
        }
    }

    const AMP_EXPECTED: &[(&str, &str, usize, &[(&str, usize, usize, f64, f64)])] = &[
        (
            "Amp-Fwd",
            "fwd",
            3,
            &[
                ("CCATGGCGTCGATCGATCGATCGAATT", 0, 27, 27.0, 51.9),
                ("CCATGGCGTCGATCGATCGATCGAATTC", 0, 28, 28.0, 53.6),
                ("CCATGGCGTCGATCGATCGATCGAATTCG", 0, 29, 29.0, 55.2),
                ("CCATGGCGTCGATCGATCGATCGAATTCGA", 0, 30, 30.0, 53.3),
                ("CCATGGCGTCGATCGATCGATCGAATTCGAT", 0, 31, 31.0, 51.6),
                ("CCATGGCGTCGATCGATCGATCGAATTCGATC", 0, 32, 32.0, 53.1),
                ("CCATGGCGTCGATCGATCGATCGAATTCGATCG", 0, 33, 33.0, 54.5),
            ],
        ),
        (
            "Amp-Rev",
            "rev",
            3,
            &[
                ("ATCGAATTCGATCGATCGATCGACGCC", 0, 27, 27.0, 51.9),
                ("ATCGAATTCGATCGATCGATCGACGCCA", 0, 28, 28.0, 50.0),
                ("ATCGAATTCGATCGATCGATCGACGCCAT", 0, 29, 29.0, 48.3),
                ("ATCGAATTCGATCGATCGATCGACGCCATG", 0, 30, 30.0, 50.0),
                ("ATCGAATTCGATCGATCGATCGACGCCATGG", 0, 31, 31.0, 51.6),
                ("ATCGAATTCGATCGATCGATCGACGCCATGGA", 0, 32, 32.0, 50.0),
                ("ATCGAATTCGATCGATCGATCGACGCCATGGAT", 0, 33, 33.0, 48.5),
            ],
        ),
    ];

    #[test]
    fn amplify_matches_js_with_stubbed_tm() {
        // Mirrors the JS pipeline with tmOf = (s) => s.length; golden values
        // dumped from the real candidates.js by a node script.
        let seg = Segment {
            start: 4,
            end: 34,
            color: None,
        };
        let groups =
            build_amplify_groups_with(SEQ, &seg, "Amp", 30.0, "linear", |s| s.len() as f64);
        check_groups(&groups, AMP_EXPECTED);
    }

    #[test]
    fn oepcr_matches_js_with_stubbed_tm() {
        let seg1 = Segment {
            start: 4,
            end: 24,
            color: None,
        };
        let seg2 = Segment {
            start: 30,
            end: 50,
            color: None,
        };
        let groups =
            build_oepcr_groups_with(SEQ, &seg1, &seg2, "Seg1", "Seg2", 30.0, 8, "linear", |s| {
                s.len() as f64
            });
        // First group repeats the Amp-Fwd candidates; inner primers carry the
        // overlap tails; the Seg1-Rev core clamps at the linear origin so all
        // variants collapse to one 25-nt core (coreLen saturates at 40).
        let expected: &[(&str, &str, usize, &[(&str, usize, usize, f64, f64)])] = &[
            ("Seg1-Fwd", "fwd", 3, &AMP_EXPECTED[0].3),
            (
                "Seg1-Rev",
                "rev",
                0,
                &[
                    ("TCGATCGAATCGATCGATCGACGCCATGGATCC", 8, 25, 25.0, 54.5),
                    ("TCGATCGAATCGATCGATCGACGCCATGGATCC", 8, 25, 25.0, 54.5),
                    ("TCGATCGAATCGATCGATCGACGCCATGGATCC", 8, 25, 25.0, 54.5),
                    ("TCGATCGAATCGATCGATCGACGCCATGGATCC", 8, 25, 25.0, 54.5),
                    ("TCGATCGAATCGATCGATCGACGCCATGGATCC", 8, 25, 25.0, 54.5),
                    ("TCGATCGAATCGATCGATCGACGCCATGGATCC", 8, 25, 25.0, 54.5),
                    ("TCGATCGAATCGATCGATCGACGCCATGGATCC", 8, 25, 25.0, 54.5),
                ],
            ),
            (
                "Seg2-Fwd",
                "fwd",
                3,
                &[
                    ("ATCGATCGTCGATCGATCGATCGGTACCGGATCCA", 8, 27, 27.0, 54.3),
                    ("ATCGATCGTCGATCGATCGATCGGTACCGGATCCAT", 8, 28, 28.0, 52.8),
                    ("ATCGATCGTCGATCGATCGATCGGTACCGGATCCATG", 8, 29, 29.0, 54.1),
                    ("ATCGATCGTCGATCGATCGATCGGTACCGGATCCATGG", 8, 30, 30.0, 55.3),
                    ("ATCGATCGTCGATCGATCGATCGGTACCGGATCCATGGC", 8, 31, 31.0, 56.4),
                    (
                        "ATCGATCGTCGATCGATCGATCGGTACCGGATCCATGGCG",
                        8,
                        32,
                        32.0,
                        57.5,
                    ),
                    (
                        "ATCGATCGTCGATCGATCGATCGGTACCGGATCCATGGCGT",
                        8,
                        33,
                        33.0,
                        56.1,
                    ),
                ],
            ),
            (
                "Seg2-Rev",
                "rev",
                3,
                &[
                    ("CGGTACCGATCGATCGATCGAATTCGA", 0, 27, 27.0, 51.9),
                    ("CGGTACCGATCGATCGATCGAATTCGAT", 0, 28, 28.0, 50.0),
                    ("CGGTACCGATCGATCGATCGAATTCGATC", 0, 29, 29.0, 51.7),
                    ("CGGTACCGATCGATCGATCGAATTCGATCG", 0, 30, 30.0, 53.3),
                    ("CGGTACCGATCGATCGATCGAATTCGATCGA", 0, 31, 31.0, 51.6),
                    ("CGGTACCGATCGATCGATCGAATTCGATCGAT", 0, 32, 32.0, 50.0),
                    ("CGGTACCGATCGATCGATCGAATTCGATCGATC", 0, 33, 33.0, 51.5),
                ],
            ),
        ];
        check_groups(&groups, expected);
    }

    #[test]
    fn mutagenesis_matches_js_with_stubbed_tm() {
        let seg = Segment {
            start: 20,
            end: 26,
            color: None,
        };
        let groups =
            build_mutagenesis_groups_with(SEQ, &seg, "Site", "GGG", 30.0, 10, |s| s.len() as f64);
        let expected: &[(&str, &str, usize, &[(&str, usize, usize, f64, f64)])] = &[
            (
                "Site-Fwd",
                "fwd",
                3,
                &[
                    (
                        "CGTCGATCGAGGGAATTCGATCGATCGATCGGTACCGGAT",
                        13,
                        27,
                        27.0,
                        55.0,
                    ),
                    (
                        "CGTCGATCGAGGGAATTCGATCGATCGATCGGTACCGGATC",
                        13,
                        28,
                        28.0,
                        56.1,
                    ),
                    (
                        "CGTCGATCGAGGGAATTCGATCGATCGATCGGTACCGGATCC",
                        13,
                        29,
                        29.0,
                        57.1,
                    ),
                    (
                        "CGTCGATCGAGGGAATTCGATCGATCGATCGGTACCGGATCCA",
                        13,
                        30,
                        30.0,
                        55.8,
                    ),
                    (
                        "CGTCGATCGAGGGAATTCGATCGATCGATCGGTACCGGATCCAT",
                        13,
                        31,
                        31.0,
                        54.5,
                    ),
                    (
                        "CGTCGATCGAGGGAATTCGATCGATCGATCGGTACCGGATCCATG",
                        13,
                        32,
                        32.0,
                        55.6,
                    ),
                    (
                        "CGTCGATCGAGGGAATTCGATCGATCGATCGGTACCGGATCCATGG",
                        13,
                        33,
                        33.0,
                        56.5,
                    ),
                ],
            ),
            (
                "Site-Rev",
                "rev",
                3,
                &[
                    (
                        "CCCCGATCGAATTTCGATCGACGCCATGGATCCGATCGAT",
                        13,
                        27,
                        27.0,
                        55.0,
                    ),
                    (
                        "CCCCGATCGAATTTCGATCGACGCCATGGATCCGATCGATC",
                        13,
                        28,
                        28.0,
                        56.1,
                    ),
                    (
                        "CCCCGATCGAATTTCGATCGACGCCATGGATCCGATCGATCG",
                        13,
                        29,
                        29.0,
                        57.1,
                    ),
                    (
                        "CCCCGATCGAATTTCGATCGACGCCATGGATCCGATCGATCGA",
                        13,
                        30,
                        30.0,
                        55.8,
                    ),
                    (
                        "CCCCGATCGAATTTCGATCGACGCCATGGATCCGATCGATCGAC",
                        13,
                        31,
                        31.0,
                        56.8,
                    ),
                    (
                        "CCCCGATCGAATTTCGATCGACGCCATGGATCCGATCGATCGACG",
                        13,
                        32,
                        32.0,
                        57.8,
                    ),
                    (
                        "CCCCGATCGAATTTCGATCGACGCCATGGATCCGATCGATCGACGC",
                        13,
                        33,
                        33.0,
                        58.7,
                    ),
                ],
            ),
        ];
        check_groups(&groups, expected);
    }

    #[test]
    fn helpers_match_js() {
        assert_eq!(rev_comp("ACGT"), "ACGT");
        assert_eq!(rev_comp("AACCGGTT"), "AACCGGTT");
        assert_eq!(rev_comp("ATGCNx"), "NNGCAT");
        assert_eq!(gc_percent("ACGTACGTAC"), 50.0);
        assert_eq!(gc_percent(""), 0.0);
        assert_eq!(gc_percent("GGGG"), 100.0);
    }

    #[test]
    fn amplify_real_tm_structure() {
        // Public API with default TmParams: verify the structural invariants
        // (7 variants, consecutive lengths, seq = tail + anneal, tm rounded
        // from the NN engine, defaultIndex closest to target).
        let seg = Segment {
            start: 10,
            end: 40,
            color: None,
        };
        let groups =
            build_amplify_groups(SEQ, &seg, "RealAmp", 55.0, "circular", &TmParams::default());
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "RealAmp-Fwd");
        assert_eq!(groups[0].r#type, "fwd");
        assert_eq!(groups[1].name, "RealAmp-Rev");
        assert_eq!(groups[1].r#type, "rev");

        let params = TmParams::default();
        for group in &groups {
            assert_eq!(group.candidates.len(), 7);
            let core_len = group.candidates[0].anneal_len + 3;
            assert!((18..=40).contains(&core_len));
            for (i, c) in group.candidates.iter().enumerate() {
                assert_eq!(c.anneal_len, core_len - 3 + i);
                assert_eq!(c.seq.len(), c.tail_len + c.anneal_len);
                let anneal = &c.seq[c.tail_len..];
                assert_eq!(c.anneal_len, anneal.len());
                assert_eq!(c.tm, round1(compute_tm_with_params(anneal, &params)));
                assert_eq!(c.gc, gc_percent(&c.seq));
            }
            let best = group
                .candidates
                .iter()
                .enumerate()
                .min_by(|a, b| {
                    let da = (a.1.tm - 55.0).abs();
                    let db = (b.1.tm - 55.0).abs();
                    da.total_cmp(&db)
                })
                .unwrap()
                .0;
            assert_eq!(group.default_index, best);
        }
    }

    #[test]
    fn mutagenesis_cleans_and_wraps() {
        // mutSeq non-ACGT chars stripped; tails = upArm+mut / revComp(downArm+mut).
        let seq = "GGGAAACCCGGGAAACCCGGGAAACCCGGGAAACCCGGGAAACCC";
        let seg = Segment {
            start: 18,
            end: 23,
            color: None,
        };
        let groups =
            build_mutagenesis_groups(seq, &seg, "M", "g-g!T", 45.0, 9, &TmParams::default());
        assert_eq!(groups.len(), 2);
        // Up arm = seq[9..18], down arm = seq[24..33], mut = "GGT".
        assert_eq!(groups[0].candidates[0].tail_len, 12);
        assert_eq!(
            groups[0].candidates[0].seq[..12],
            format!("{}GGT", &seq[9..18])
        );
        assert_eq!(groups[1].candidates[0].tail_len, 12);
        assert_eq!(
            groups[1].candidates[0].seq[..12],
            rev_comp(&format!("{}GGT", &seq[24..33]))
        );
    }

    #[test]
    fn core_len_saturates_at_40() {
        // AT-only template with an unreachable target: coreLen stays at 40 and
        // the variants run 37..43 (all within the 60-nt template, so no clamp).
        let seq = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let seg = Segment {
            start: 0,
            end: 10,
            color: None,
        };
        let groups =
            build_amplify_groups_with(seq, &seg, "Sat", 60.0, "linear", |s| s.len() as f64);
        let lens: Vec<usize> = groups[0].candidates.iter().map(|c| c.anneal_len).collect();
        assert_eq!(lens, vec![37, 38, 39, 40, 41, 42, 43]);
    }
}
