use std::collections::HashMap;
use std::fmt::Write as _;

use super::range::{cut_flanks, cut_notation, pos_in_range, seg_in_range};
use super::{ALIGNMENT_VIEW_LINE, ALIGNMENT_VIEW_MAX_COLS};
use crate::models::{AlignDeletion, Enzyme, Feature, PrimerBindingSite, ProjectData};

// ---------------------------------------------------------------------------
// Line renderers
// ---------------------------------------------------------------------------

/// Length unit for the molecule type (LOCUS line, read windows, error messages).
pub(crate) fn unit_for(molecule_type: &str) -> &'static str {
    match molecule_type {
        "rna" => "nt",
        "protein" => "aa",
        _ => "bp",
    }
}

/// Human-readable molecule type label for the LOCUS line.
pub(crate) fn molecule_label(molecule_type: &str) -> &'static str {
    match molecule_type {
        "rna" => "RNA",
        "protein" => "Protein",
        _ => "DNA",
    }
}

pub(crate) fn feature_matches_filter(feat: &Feature, filter: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(f) => {
            let f = f.trim();
            f.is_empty()
                || feat.ftype.eq_ignore_ascii_case(f)
                || feat.name.to_lowercase().contains(&f.to_lowercase())
        }
    }
}

pub(crate) fn feature_location(f: &Feature) -> String {
    let segments: Vec<(i64, i64)> = if f.segments.is_empty() {
        vec![(f.start, f.end)]
    } else {
        f.segments.iter().map(|s| (s.start, s.end)).collect()
    };
    let inner = segments
        .iter()
        .map(|(s, e)| format!("{}..{}", s + 1, e + 1))
        .collect::<Vec<_>>()
        .join(",");
    let loc = if segments.len() > 1 {
        format!("join({})", inner)
    } else {
        inner
    };
    if f.strand == "-" {
        format!("complement({})", loc)
    } else {
        loc
    }
}

pub(crate) fn feature_line(f: &Feature) -> String {
    format!(
        "        {:<12} {:<24} {}  [#{}]  (id: {})",
        f.ftype,
        feature_location(f),
        f.name,
        f.color.trim_start_matches('#'),
        f.id
    )
}

pub(crate) fn primer_site_line(site: &PrimerBindingSite, primer: &crate::models::Primer) -> (i64, String) {
    let strand = if site.strand == 1 { "+ strand" } else { "- strand" };
    let mismatch = if site.has_3_prime_mismatch { ", 3' mismatch" } else { "" };
    (
        site.template_start,
        format!(
            "        primer_bind     {}..{}   {} ({}, {} nt, {})  [Tm {:.1}, {}{}]  (id: {})",
            site.template_start + 1,
            site.template_end,
            primer.name,
            primer.r#type,
            primer.primer_seq.len(),
            primer.primer_seq,
            site.tm,
            strand,
            mismatch,
            primer.id
        ),
    )
}

pub(crate) fn alignment_in_region(a: &crate::models::Alignment, s: i64, e: i64, circular: bool) -> bool {
    a.segments
        .iter()
        .any(|seg| seg_in_range(seg.start as i64, seg.end as i64, s, e, circular))
}

/// A deletion covers template columns `pos .. pos+length-1`; a merged deletion
/// straddling the circular origin wraps (pos + length may exceed tlen).
pub(crate) fn deletion_in_region(d: &AlignDeletion, s: i64, e: i64, circular: bool, tlen: i64) -> bool {
    let start = d.pos as i64;
    let end = d.pos as i64 + d.length as i64 - 1;
    if end < tlen {
        seg_in_range(start, end, s, e, circular)
    } else {
        seg_in_range(start, tlen - 1, s, e, circular) || seg_in_range(0, end - tlen, s, e, circular)
    }
}

pub(crate) fn alignment_line(a: &crate::models::Alignment) -> String {
    let inner = a
        .segments
        .iter()
        .map(|seg| format!("{}..{}", seg.start + 1, seg.end + 1))
        .collect::<Vec<_>>()
        .join(",");
    let loc = if a.segments.len() > 1 {
        format!("join({})", inner)
    } else {
        inner
    };
    let strand = if a.strand == "-" { "- strand" } else { "+ strand" };
    format!(
        "        {:<24} {:<24} {}  [identity {:.1}%, significant]  (id: {})",
        a.name,
        loc,
        strand,
        a.identity * 100.0,
        a.id
    )
}

/// Per-read column view of a region window: template bases, a match mask (the
/// primer-site convention: `|` match, `.` mismatch, `-` read gap) and
/// the read bases. Only template columns the read covers are rendered, in
/// 1-based order (wrapping the origin on circular templates); insertions and
/// uncovered template runs are listed below the block. Views wider than
/// [`ALIGNMENT_VIEW_MAX_COLS`] are replaced by an omission note.
pub(crate) fn push_alignment_view(
    out: &mut String,
    a: &crate::models::Alignment,
    template: &str,
    s: i64,
    e: i64,
    circular: bool,
) {
    let tbytes = template.as_bytes();
    let tlen = tbytes.len() as i64;
    let mut cols: Vec<(i64, char, char)> = Vec::new();
    let mut uncovered: Vec<(i64, i64)> = Vec::new();
    let mut run_start: Option<i64> = None;
    let mut pos = s;
    loop {
        let read_char = a.segments.iter().find_map(|seg| {
            if pos >= seg.start as i64 && pos <= seg.end as i64 {
                seg.chars
                    .as_bytes()
                    .get((pos - seg.start as i64) as usize)
                    .copied()
            } else {
                None
            }
        });
        match read_char {
            Some(rc) => {
                if let Some(rs) = run_start.take() {
                    uncovered.push((rs, pos - 1));
                }
                let tb = tbytes
                    .get(pos as usize)
                    .map(|b| b.to_ascii_uppercase())
                    .unwrap_or(b'N');
                cols.push((pos, tb as char, rc.to_ascii_uppercase() as char));
            }
            None => {
                if run_start.is_none() {
                    run_start = Some(pos);
                }
            }
        }
        if pos == e {
            break;
        }
        pos = if circular { (pos + 1) % tlen } else { pos + 1 };
    }
    if let Some(rs) = run_start.take() {
        uncovered.push((rs, e));
    }
    if cols.is_empty() {
        return;
    }
    let strand = if a.strand == "-" { "-" } else { "+" };
    let _ = writeln!(out, "        {}  (id: {}, {} strand):", a.name, a.id, strand);
    if cols.len() > ALIGNMENT_VIEW_MAX_COLS {
        let _ = writeln!(
            out,
            "            column view omitted (covered window {} bp exceeds the {} bp cap; use ALIGNMENT DIFFS or a narrower window)",
            cols.len(),
            ALIGNMENT_VIEW_MAX_COLS
        );
        return;
    }
    let indent = " ".repeat(12);
    for chunk in cols.chunks(ALIGNMENT_VIEW_LINE) {
        let first = chunk[0].0 + 1;
        let t: String = chunk.iter().map(|(_, t, _)| *t).collect();
        let m: String = chunk
            .iter()
            .map(|(_, t, r)| {
                if *r == '-' {
                    '-'
                } else if *r == *t {
                    '|'
                } else {
                    '.'
                }
            })
            .collect();
        let r: String = chunk.iter().map(|(_, _, r)| *r).collect();
        let _ = writeln!(out, "{indent}{first:>6}  {t}");
        let _ = writeln!(out, "{indent}        {m}");
        let _ = writeln!(out, "{indent}{first:>6}  {r}");
    }
    for ins in &a.insertions {
        if pos_in_range(ins.pos as i64, s, e, circular) {
            let (a1, b1) = cut_flanks(ins.pos as i64, tlen, circular);
            let _ = writeln!(
                out,
                "{indent}insertion between {} and {}: +{} bp ({})",
                a1,
                b1,
                ins.bases.len(),
                ins.bases
            );
        }
    }
    for (us, ue) in uncovered {
        let n = if us <= ue { ue - us + 1 } else { tlen - us + ue + 1 };
        let _ = writeln!(out, "{indent}uncovered template {}..{} ({} bp)", us + 1, ue + 1, n);
    }
}

/// First amino-acid position (1-based) where a stored `/translation` and the
/// DNA-derived translation disagree, with the stored and derived letters.
/// A pure length mismatch reports the first position past the shared prefix.
/// GenBank/SnapGene convention writes the initial residue as M even when the
/// start codon is GTG/TTG (V/L internally), so a leading stored M against a
/// derived V or L is not a disagreement.
pub(crate) fn translation_diff_pos(stored: &str, derived: &str) -> Option<(usize, String, String)> {
    let s: Vec<char> = stored.to_ascii_uppercase().chars().collect();
    let d: Vec<char> = derived.to_ascii_uppercase().chars().collect();
    let n = s.len().min(d.len());
    for i in 0..n {
        if s[i] != d[i] {
            if i == 0 && s[0] == 'M' && (d[0] == 'V' || d[0] == 'L') {
                continue;
            }
            return Some((i + 1, s[i].to_string(), d[i].to_string()));
        }
    }
    if s.len() != d.len() {
        return Some((n + 1, format!("{} aa", s.len()), format!("{} aa", d.len())));
    }
    None
}

pub(crate) fn cut_type_label(cut_type: &str) -> &str {
    match cut_type {
        "5overhang" => "5' overhang",
        "3overhang" => "3' overhang",
        _ => "blunt",
    }
}

pub(crate) fn cuts_desc(e: &Enzyme, len: i64, circular: bool) -> String {
    if e.cut_pairs.is_empty() {
        format!(
            "top {} bot {}",
            cut_notation(e.cut_index, len, circular),
            cut_notation(e.bot_cut_index, len, circular)
        )
    } else {
        e.cut_pairs
            .iter()
            .map(|p| {
                format!(
                    "top {} bot {}",
                    cut_notation(p.top_cut_index, len, circular),
                    cut_notation(p.bot_cut_index, len, circular)
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Group `project.enzymes` by name and classify (mirrors `filter_project`:
/// uniqueness = the name appears exactly once; cut-twice = one site with two
/// cut pairs, or two sites).
pub(crate) fn classify_enzymes(project: &ProjectData) -> (Vec<&Enzyme>, Vec<&Enzyme>, usize) {
    let mut by_name: HashMap<&str, Vec<&Enzyme>> = HashMap::new();
    for e in &project.enzymes {
        by_name.entry(e.name.as_str()).or_default().push(e);
    }
    let mut unique = Vec::new();
    let mut twice = Vec::new();
    let mut others = 0usize;
    for group in by_name.into_values() {
        match group.len() {
            1 if !group[0].cut_twice => unique.push(group[0]),
            1 => twice.push(group[0]),
            2 => twice.extend(group),
            _ => others += 1,
        }
    }
    unique.sort_by_key(|e| e.cut_index);
    twice.sort_by_key(|e| e.cut_index);
    (unique, twice, others)
}
