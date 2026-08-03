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
    /// 5' tail sequence (empty when the primer has no tail).
    pub tail: String,
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
    build: impl Fn(usize) -> (String, String, String),
    core_len: usize,
    target_tm: f64,
    tm_of: impl Fn(&str) -> f64,
) -> (Vec<PrimerCandidate>, usize) {
    let mut candidates = Vec::with_capacity(7);
    for l in core_len.saturating_sub(3)..=core_len + 3 {
        let (seq, anneal, tail) = build(l);
        candidates.push(PrimerCandidate {
            tm: round1(tm_of(&anneal)),
            gc: gc_percent(&seq),
            seq,
            tail_len: tail.len(),
            tail,
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
    let (candidates, default_index) = build_variants(
        move |l| {
            let anneal = anneal_fn(l);
            let seq = format!("{tail}{anneal}");
            (seq, anneal, tail.clone())
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
    fwd_tail: String,
    rev_tail: String,
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
            fwd_tail,
            target_tm,
            &tm_of,
        ),
        make_group(
            &format!("{name}-Rev"),
            "rev",
            rev_anneal,
            rev_tail,
            target_tm,
            tm_of,
        ),
    ]
}

/// Deterministic 5' protection bases (alternating GC) for enzyme tails.
pub fn protect_sequence(n: usize) -> String {
    (0..n).map(|i| if i % 2 == 0 { 'G' } else { 'C' }).collect()
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
    build_amplify_groups_tailed(seq, seg, name, target_tm, topology, "", "", params)
}

/// Amplification primers with explicit 5' tails (e.g. protect + enzyme site).
/// Tm is computed on the anneal core only.
#[allow(clippy::too_many_arguments)]
pub fn build_amplify_groups_tailed(
    seq: &str,
    seg: &Segment,
    name: &str,
    target_tm: f64,
    topology: &str,
    fwd_tail: &str,
    rev_tail: &str,
    params: &TmParams,
) -> Vec<PrimerGroup> {
    build_amplify_groups_with(
        seq,
        seg,
        name,
        target_tm,
        topology,
        fwd_tail.to_ascii_uppercase(),
        rev_tail.to_ascii_uppercase(),
        |s| compute_tm_with_params(s, params),
    )
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

// ---------------------------------------------------------------------------
// Mutagenesis validation / self-check info
// ---------------------------------------------------------------------------

/// Max accepted base differences between `seg` and `mut_seq`.
pub const MAX_MUTAGENESIS_DIFFS: usize = 3;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationDiff {
    /// 0-based offset inside `seg`.
    pub offset: usize,
    pub template_base: String,
    pub new_base: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CdsMutation {
    pub feature_id: String,
    pub name: String,
    pub strand: String,
    /// 0-based codon index within the CDS (the codon that contains the first diff).
    pub codon_index: usize,
    /// 1-based amino-acid position within the CDS (codonIndex + 1) — the
    /// residue that changes, in CDS order.
    pub aa_position_1_based: usize,
    /// Codons on the CDS coding strand.
    pub codon_before: String,
    pub codon_after: String,
    pub aa_before: String,
    pub aa_after: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutagenesisAnalysis {
    pub seg_start: i64,
    pub seg_end: i64,
    pub template_bases: String,
    pub new_bases: String,
    pub diffs: Vec<MutationDiff>,
    /// Plus-strand context: 10 bp flanks + the seg, seg wrapped in [brackets].
    pub plus_context: String,
    /// Reverse complement of the same window (seg in [brackets]).
    pub minus_context: String,
    pub cds: Option<CdsMutation>,
    /// Set when every base of `seg` is replaced (likely wrong strand/location).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

// Standard genetic code, codons ordered TCAG per position.
const CODON_TABLE: [&str; 64] = [
    "Phe", "Phe", "Leu", "Leu", "Ser", "Ser", "Ser", "Ser", "Tyr", "Tyr", "Ter", "Ter", "Cys",
    "Cys", "Ter", "Trp", "Leu", "Leu", "Leu", "Leu", "Pro", "Pro", "Pro", "Pro", "His", "His",
    "Gln", "Gln", "Arg", "Arg", "Arg", "Arg", "Ile", "Ile", "Ile", "Met", "Thr", "Thr", "Thr",
    "Thr", "Asn", "Asn", "Lys", "Lys", "Ser", "Ser", "Arg", "Arg", "Val", "Val", "Val", "Val",
    "Ala", "Ala", "Ala", "Ala", "Asp", "Asp", "Glu", "Glu", "Gly", "Gly", "Gly", "Gly",
];

fn codon_index_of(b: u8) -> Option<usize> {
    match b {
        b'T' | b't' => Some(0),
        b'C' | b'c' => Some(1),
        b'A' | b'a' => Some(2),
        b'G' | b'g' => Some(3),
        _ => None,
    }
}

pub fn translate_codon(codon: &str) -> Option<&'static str> {
    let b = codon.as_bytes();
    if b.len() != 3 {
        return None;
    }
    let i = codon_index_of(b[0])? * 16 + codon_index_of(b[1])? * 4 + codon_index_of(b[2])?;
    Some(CODON_TABLE[i])
}

/// Validate a mutagenesis request and return structured self-check info.
/// `mut_seq` is the desired plus-strand content of `seg` after the edit.
pub fn analyze_mutagenesis(
    seq: &str,
    seg: &Segment,
    mut_seq: &str,
    features: &[crate::models::Feature],
) -> Result<MutagenesisAnalysis, String> {
    if seg.start < 0 || seg.end >= seq.len() as i64 || seg.start > seg.end {
        return Err(format!(
            "seg {}..{} out of bounds for sequence of length {} (0-based inclusive)",
            seg.start,
            seg.end,
            seq.len()
        ));
    }
    let template = seq[seg.start as usize..=seg.end as usize].to_ascii_uppercase();
    let new_bases: String = mut_seq
        .to_ascii_uppercase()
        .chars()
        .filter(|c| matches!(c, 'A' | 'C' | 'G' | 'T'))
        .collect();
    if new_bases.len() != template.len() {
        return Err(format!(
            "mut_seq length {} does not match seg {}..{} length {}; current seg sequence is '{}'",
            new_bases.len(),
            seg.start,
            seg.end,
            template.len(),
            template
        ));
    }
    let diffs: Vec<MutationDiff> = template
        .bytes()
        .zip(new_bases.bytes())
        .enumerate()
        .filter(|(_, (t, n))| t != n)
        .map(|(i, (t, n))| MutationDiff {
            offset: i,
            template_base: (t as char).to_string(),
            new_base: (n as char).to_string(),
        })
        .collect();
    if diffs.is_empty() {
        return Err(format!(
            "mut_seq is identical to the current seg {}..{} sequence '{}'; nothing to mutate",
            seg.start, seg.end, template
        ));
    }
    if diffs.len() > MAX_MUTAGENESIS_DIFFS {
        return Err(format!(
            "mut_seq differs from the template at {} positions (max {}); check the strand and location. Current seg {}..{} is '{}', you gave '{}'",
            diffs.len(),
            MAX_MUTAGENESIS_DIFFS,
            seg.start,
            seg.end,
            template,
            new_bases
        ));
    }

    let lo = (seg.start - 10).max(0) as usize;
    let hi = ((seg.end + 11).min(seq.len() as i64)) as usize;
    let plus = format!(
        "{}[{}]{}",
        &seq[lo..seg.start as usize],
        template,
        &seq[seg.end as usize + 1..hi]
    );
    let minus = {
        let rc = rev_comp(&seq[lo..hi]);
        let open = rc.len() - (seg.end as usize + 1 - lo);
        let close = rc.len() - (seg.start as usize - lo);
        format!("{}[{}]{}", &rc[..open], &rc[open..close], &rc[close..])
    };

    let cds = features
        .iter()
        .filter(|f| f.ftype.eq_ignore_ascii_case("cds"))
        .filter(|f| {
            let spans: Vec<(i64, i64)> = if f.segments.is_empty() {
                vec![(f.start, f.end)]
            } else {
                f.segments.iter().map(|s| (s.start, s.end)).collect()
            };
            (seg.start..=seg.end)
                .all(|p| spans.iter().any(|&(s, e)| p >= s && p <= e))
        })
        .find_map(|f| {
            // Build the coding sequence in coding order: plus strand takes
            // segments ascending, minus strand descending (each segment
            // reverse-complemented). pos_of maps coding offset → plus coord.
            let mut spans: Vec<(i64, i64)> = if f.segments.is_empty() {
                vec![(f.start, f.end)]
            } else {
                f.segments.iter().map(|s| (s.start, s.end)).collect()
            };
            if f.strand == "-" {
                spans.sort_by(|a, b| b.0.cmp(&a.0));
            } else {
                spans.sort_by_key(|s| s.0);
            }
            let minus = f.strand == "-";
            let mut coding = Vec::new();
            let mut pos_of: Vec<i64> = Vec::new();
            for (s, e) in &spans {
                if minus {
                    for p in (*s..=*e).rev() {
                        coding.push(crate::utils::complement_char(seq.as_bytes()[p as usize] as char) as u8);
                        pos_of.push(p);
                    }
                } else {
                    for p in *s..=*e {
                        coding.push(seq.as_bytes()[p as usize].to_ascii_uppercase());
                        pos_of.push(p);
                    }
                }
            }
            // Map every diff to coding-strand coordinates, then apply all
            // diffs landing in the first diff's codon.
            let coding_diffs: Vec<(usize, u8)> = diffs
                .iter()
                .map(|d| {
                    let pos = seg.start + d.offset as i64;
                    let offset = pos_of.iter().position(|&p| p == pos)?;
                    let new_plus = new_bases.as_bytes()[d.offset];
                    let new_base = if minus {
                        crate::utils::complement_char(new_plus as char) as u8
                    } else {
                        new_plus
                    };
                    Some((offset, new_base))
                })
                .collect::<Option<Vec<_>>>()?;
            let codon_index = coding_diffs[0].0 / 3;
            if (codon_index + 1) * 3 > coding.len() {
                return None;
            }
            let codon_before =
                String::from_utf8(coding[codon_index * 3..codon_index * 3 + 3].to_vec()).ok()?;
            let mut after = codon_before.clone().into_bytes();
            for (offset, new_base) in &coding_diffs {
                if offset / 3 == codon_index {
                    after[offset % 3] = new_base.to_ascii_uppercase();
                }
            }
            let codon_after = String::from_utf8(after).ok()?;
            Some(CdsMutation {
                feature_id: f.id.clone(),
                name: f.name.clone(),
                strand: f.strand.clone(),
                codon_index,
                aa_position_1_based: codon_index + 1,
                aa_before: translate_codon(&codon_before).unwrap_or("???").to_string(),
                aa_after: translate_codon(&codon_after).unwrap_or("???").to_string(),
                codon_before,
                codon_after,
            })
        });

    let warning = if diffs.len() == template.len() {
        Some(format!(
            "all {} bases of seg {}..{} are replaced; confirm mut_seq is the PLUS-strand sequence at the right location (mind the CDS strand)",
            template.len(), seg.start, seg.end
        ))
    } else {
        None
    };

    Ok(MutagenesisAnalysis {
        seg_start: seg.start,
        seg_end: seg.end,
        template_bases: template,
        new_bases,
        diffs,
        plus_context: plus,
        minus_context: minus,
        cds,
        warning,
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
        let groups = build_amplify_groups_with(
            SEQ,
            &seg,
            "Amp",
            30.0,
            "linear",
            String::new(),
            String::new(),
            |s| s.len() as f64,
        );
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
    fn amplify_with_enzyme_tails() {
        let seg = Segment {
            start: 4,
            end: 34,
            color: None,
        };
        let fwd_tail = format!("{}GGATCC", protect_sequence(3));
        let rev_tail = format!("{}GAATTC", protect_sequence(3));
        let groups = build_amplify_groups_tailed(
            SEQ,
            &seg,
            "Tail",
            30.0,
            "linear",
            &fwd_tail,
            &rev_tail,
            &TmParams::default(),
        );
        assert_eq!(fwd_tail, "GCGGGATCC");
        assert_eq!(groups[0].candidates[0].tail, "GCGGGATCC");
        assert_eq!(groups[1].candidates[0].tail, "GCGGAATTC");
        for (g, tail) in groups.iter().zip([&fwd_tail, &rev_tail]) {
            for c in &g.candidates {
                assert_eq!(c.tail_len, tail.len());
                assert!(c.seq.starts_with(tail));
                // Tm reflects the anneal core only.
                assert_eq!(
                    c.tm,
                    round1(compute_tm_with_params(&c.seq[c.tail_len..], &TmParams::default()))
                );
            }
        }
    }

    #[test]
    fn analyze_mutagenesis_plus_strand_cds() {
        // mEGFP-style A206K on a plus-strand CDS: CGC -> CTT gives Arg -> Leu.
        let seq: String = "AAAAAA".repeat(20);
        let mut seq = seq.into_bytes();
        // CDS 30..89 (plus); codon at 60..62 = CGC.
        seq[60] = b'C';
        seq[61] = b'G';
        seq[62] = b'C';
        let seq = String::from_utf8(seq).unwrap();
        let cds = crate::models::Feature {
            id: "cds1".into(),
            name: "orf".into(),
            start: 30,
            end: 89,
            color: "#000000".into(),
            ftype: "CDS".into(),
            segments: vec![],
            strand: "+".into(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        };
        let seg = Segment {
            start: 60,
            end: 62,
            color: None,
        };
        let a = analyze_mutagenesis(&seq, &seg, "CTT", &[cds]).unwrap();
        assert_eq!(a.template_bases, "CGC");
        assert_eq!(a.new_bases, "CTT");
        assert_eq!(a.diffs.len(), 2);
        assert_eq!(a.diffs[0].offset, 1);
        assert!(a.plus_context.contains("[CGC]"));
        assert!(a.minus_context.contains("[GCG]"));
        let cds = a.cds.unwrap();
        assert_eq!(cds.strand, "+");
        assert_eq!(cds.codon_index, 10);
        assert_eq!(cds.codon_before, "CGC");
        assert_eq!(cds.codon_after, "CTT");
        assert_eq!(cds.aa_before, "Arg");
        assert_eq!(cds.aa_after, "Leu");
    }

    #[test]
    fn analyze_mutagenesis_minus_strand_cds() {
        // Coding strand is minus: plus-strand CGC -> CTT means GCG -> AAG on
        // the coding strand, i.e. Ala -> Lys.
        let seq: String = "AAAAAA".repeat(20);
        let mut seq = seq.into_bytes();
        seq[60] = b'C';
        seq[61] = b'G';
        seq[62] = b'C';
        let seq = String::from_utf8(seq).unwrap();
        let cds = crate::models::Feature {
            id: "cds2".into(),
            name: "mEGFP".into(),
            start: 30,
            end: 89,
            color: "#000000".into(),
            ftype: "CDS".into(),
            segments: vec![],
            strand: "-".into(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        };
        let seg = Segment {
            start: 60,
            end: 62,
            color: None,
        };
        let a = analyze_mutagenesis(&seq, &seg, "CTT", &[cds]).unwrap();
        let cds = a.cds.unwrap();
        assert_eq!(cds.name, "mEGFP");
        // coding_offset for pos 60 = 89-60 = 29 → codon 9, phase 2.
        assert_eq!(cds.codon_index, 9);
        // coding codon = revcomp(seq[60..=62]) = GCG.
        assert_eq!(cds.codon_before, "GCG");
        assert_eq!(cds.codon_after, "AAG");
        assert_eq!(cds.aa_before, "Ala");
        assert_eq!(cds.aa_after, "Lys");
    }

    #[test]
    fn analyze_mutagenesis_rejects_bad_input() {
        let seq = "ACGT".repeat(20);
        let seg = Segment {
            start: 10,
            end: 12,
            color: None,
        };
        // Length mismatch.
        let err = analyze_mutagenesis(&seq, &seg, "AC", &[]).unwrap_err();
        assert!(err.contains("does not match"));
        // Identical.
        let err = analyze_mutagenesis(&seq, &seg, "GTA", &[]).unwrap_err();
        assert!(err.contains("identical"));
        // Too many diffs.
        let seg5 = Segment {
            start: 10,
            end: 14,
            color: None,
        };
        let err = analyze_mutagenesis(&seq, &seg5, "CCCCC", &[]).unwrap_err();
        assert!(err.contains("max 3"));
        assert!(err.contains("GTACG"));
        // Out of bounds.
        assert!(analyze_mutagenesis(&seq, &Segment { start: 0, end: 500, color: None }, "AAA", &[]).is_err());
        // No CDS → cds is None.
        let ok = analyze_mutagenesis(&seq, &seg, "GTT", &[]).unwrap();
        assert!(ok.cds.is_none());
        assert_eq!(ok.diffs.len(), 1);
        assert!(ok.warning.is_none());
    }

    fn cds_feature(id: &str, strand: &str, segments: Vec<(i64, i64)>) -> crate::models::Feature {
        let start = segments.iter().map(|s| s.0).min().unwrap();
        let end = segments.iter().map(|s| s.1).max().unwrap();
        crate::models::Feature {
            id: id.into(),
            name: id.into(),
            start,
            end,
            color: "#000000".into(),
            ftype: "CDS".into(),
            segments: segments
                .into_iter()
                .map(|(start, end)| Segment {
                    start,
                    end,
                    color: None,
                })
                .collect(),
            strand: strand.into(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        }
    }

    #[test]
    fn analyze_mutagenesis_minus_strand_joined_cds() {
        // mEGFP-like: complement(join(449..1162,1163..1165,1166..1168)).
        // Plus-strand seg 548..550 = CGC -> CTT is coding GCG -> AAG (Ala->Lys).
        let mut seq = "A".repeat(1200).into_bytes();
        seq[548] = b'C';
        seq[549] = b'G';
        seq[550] = b'C';
        let seq = String::from_utf8(seq).unwrap();
        let cds = cds_feature("mEGFP", "-", vec![(449, 1162), (1163, 1165), (1166, 1168)]);
        let seg = Segment {
            start: 548,
            end: 550,
            color: None,
        };
        let a = analyze_mutagenesis(&seq, &seg, "CTT", &[cds]).unwrap();
        let cds = a.cds.expect("joined minus-strand CDS must be annotated");
        assert_eq!(cds.name, "mEGFP");
        assert_eq!(cds.strand, "-");
        // Coding order: (1166..1168) 3 + (1163..1165) 3 + (1162-550) = 618.
        assert_eq!(cds.codon_index, 206);
        assert_eq!(cds.codon_before, "GCG");
        assert_eq!(cds.codon_after, "AAG");
        assert_eq!(cds.aa_before, "Ala");
        assert_eq!(cds.aa_after, "Lys");
    }

    #[test]
    fn analyze_mutagenesis_plus_strand_joined_cds() {
        // join(30..59,70..89) plus strand; seg 72..74 in the second segment.
        let mut seq = "A".repeat(120).into_bytes();
        seq[72] = b'C';
        seq[73] = b'G';
        seq[74] = b'C';
        seq[75] = b'C';
        let seq = String::from_utf8(seq).unwrap();
        let cds = cds_feature("orf", "+", vec![(30, 59), (70, 89)]);
        let seg = Segment {
            start: 72,
            end: 74,
            color: None,
        };
        let a = analyze_mutagenesis(&seq, &seg, "CTT", &[cds]).unwrap();
        let cds = a.cds.expect("joined plus-strand CDS must be annotated");
        assert_eq!(cds.strand, "+");
        // Coding offset of plus 73 = 30 + (73-70) = 33 → codon 11 (plus 73..75).
        assert_eq!(cds.codon_index, 11);
        assert_eq!(cds.codon_before, "GCC");
        assert_eq!(cds.codon_after, "TTC");
        assert_eq!(cds.aa_before, "Ala");
        assert_eq!(cds.aa_after, "Phe");
    }

    #[test]
    fn analyze_mutagenesis_full_replacement_warns() {
        let seq = "ACGT".repeat(20);
        let seg = Segment {
            start: 10,
            end: 12,
            color: None,
        };
        // Template "GTA" fully replaced (e.g. coding-strand bases given as plus).
        let a = analyze_mutagenesis(&seq, &seg, "CCC", &[]).unwrap();
        assert_eq!(a.diffs.len(), 3);
        let w = a.warning.expect("full replacement must warn");
        assert!(w.contains("PLUS-strand"));
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
        let groups = build_amplify_groups_with(
            seq,
            &seg,
            "Sat",
            60.0,
            "linear",
            String::new(),
            String::new(),
            |s| s.len() as f64,
        );
        let lens: Vec<usize> = groups[0].candidates.iter().map(|c| c.anneal_len).collect();
        assert_eq!(lens, vec![37, 38, 39, 40, 41, 42, 43]);
    }
}
