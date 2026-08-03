//! Text digest renderers — compact, LLM-friendly project summaries for the MCP server.
//!
//! One coordinate convention everywhere: **0-based inclusive** (features, primer
//! binding sites, read ranges). Primer `template_end` is exclusive; enzyme cuts
//! happen between `pos-1` and `pos`. Circular sequences allow `start > end` to
//! wrap the origin.

use std::collections::HashMap;

use crate::models::{Enzyme, Feature, PrimerBindingSite, ProjectData};

/// Cap for `read_sequence` windows — protects LLM context from accidental dumps.
pub const MAX_READ_BASES: usize = 10_000;

#[derive(Debug, Clone, Default)]
pub struct DigestOptions {
    pub max_features: Option<usize>,
    /// Substring match on feature name (case-insensitive) or exact ftype match.
    pub feature_filter: Option<String>,
    /// Collapse the enzyme cut list into a single count line. The full list
    /// can reach tens of KB (one line per cutter), which blows up MCP
    /// mutation responses — mutation tools default this to true.
    pub compact_enzymes: bool,
    /// Whole-project digests only: collapse the UNIQUE CUTTERS list (one line
    /// per single-cut enzyme, 90+ lines on real plasmids) into a single count
    /// line. Independent of `compact_enzymes` (which governs region views);
    /// `get_project_overview` defaults this to true, pass compactCutters=false
    /// for the full list.
    pub compact_cutters: bool,
}

// ---------------------------------------------------------------------------
// Range helpers (0-based inclusive; circular wrap when start > end)
// ---------------------------------------------------------------------------

fn pos_in_range(p: i64, s: i64, e: i64, circular: bool) -> bool {
    if s <= e {
        p >= s && p <= e
    } else {
        circular && (p >= s || p <= e)
    }
}

fn seg_in_range(seg_s: i64, seg_e: i64, s: i64, e: i64, circular: bool) -> bool {
    if s <= e {
        seg_s <= e && seg_e >= s
    } else {
        circular && (seg_e >= s || seg_s <= e)
    }
}

fn validate_range(project: &ProjectData, start: i64, end: i64) -> Result<(i64, i64), String> {
    if project.length <= 0 || project.sequence.is_empty() {
        return Err("Sequence is empty".to_string());
    }
    let circular = project.topology == "circular";
    if start > end && !circular {
        return Err(
            "start > end is only allowed on circular sequences (wraps the origin)".to_string(),
        );
    }
    let len = project.length;
    if start < 0 || end < 0 || start >= len || end >= len {
        return Err(format!(
            "range {}..{} out of bounds for sequence of length {} (0-based inclusive)",
            start, end, len
        ));
    }
    Ok((start, end))
}

fn feature_in_region(f: &Feature, s: i64, e: i64, circular: bool) -> bool {
    if f.segments.is_empty() {
        seg_in_range(f.start, f.end, s, e, circular)
    } else {
        f.segments
            .iter()
            .any(|seg| seg_in_range(seg.start, seg.end, s, e, circular))
    }
}

fn enzyme_in_region(en: &Enzyme, s: i64, e: i64, circular: bool) -> bool {
    let cuts: Vec<i64> = if en.cut_pairs.is_empty() {
        vec![en.cut_index, en.bot_cut_index]
    } else {
        en.cut_pairs
            .iter()
            .flat_map(|p| [p.top_cut_index, p.bot_cut_index])
            .collect()
    };
    cuts.iter().any(|&c| pos_in_range(c, s, e, circular))
}

// ---------------------------------------------------------------------------
// Line renderers
// ---------------------------------------------------------------------------

fn feature_matches_filter(feat: &Feature, filter: Option<&str>) -> bool {
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

fn feature_location(f: &Feature) -> String {
    let segments: Vec<(i64, i64)> = if f.segments.is_empty() {
        vec![(f.start, f.end)]
    } else {
        f.segments.iter().map(|s| (s.start, s.end)).collect()
    };
    let inner = segments
        .iter()
        .map(|(s, e)| format!("{}..{}", s, e))
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

fn feature_line(f: &Feature) -> String {
    format!(
        "        {:<12} {:<24} {}  [#{}]  (id: {})",
        f.ftype,
        feature_location(f),
        f.name,
        f.color.trim_start_matches('#'),
        f.id
    )
}

fn primer_site_line(site: &PrimerBindingSite, primer: &crate::models::Primer) -> (i64, String) {
    let strand = if site.strand == 1 { "+ strand" } else { "- strand" };
    let mismatch = if site.has_3_prime_mismatch { ", 3' mismatch" } else { "" };
    (
        site.template_start,
        format!(
            "        primer_bind     {}..{}   {}  [Tm {:.1}, {}{}]  (id: {})",
            site.template_start,
            site.template_end - 1,
            primer.name,
            site.tm,
            strand,
            mismatch,
            primer.id
        ),
    )
}

fn alignment_in_region(a: &crate::models::Alignment, s: i64, e: i64, circular: bool) -> bool {
    a.segments
        .iter()
        .any(|seg| seg_in_range(seg.start as i64, seg.end as i64, s, e, circular))
}

fn alignment_line(a: &crate::models::Alignment) -> String {
    let inner = a
        .segments
        .iter()
        .map(|seg| format!("{}..{}", seg.start, seg.end))
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

fn cut_type_label(cut_type: &str) -> &str {
    match cut_type {
        "5overhang" => "5' overhang",
        "3overhang" => "3' overhang",
        _ => "blunt",
    }
}

fn cuts_desc(e: &Enzyme) -> String {
    if e.cut_pairs.is_empty() {
        format!("top {}^ bot {}", e.cut_index, e.bot_cut_index)
    } else {
        e.cut_pairs
            .iter()
            .map(|p| format!("top {}^ bot {}", p.top_cut_index, p.bot_cut_index))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Group `project.enzymes` by name and classify (mirrors `filter_project`:
/// uniqueness = the name appears exactly once; cut-twice = one site with two
/// cut pairs, or two sites).
fn classify_enzymes(project: &ProjectData) -> (Vec<&Enzyme>, Vec<&Enzyme>, usize) {
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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Full or region-filtered project digest. `region` is 0-based inclusive;
/// `start > end` wraps the origin on circular sequences.
pub fn project_digest(
    project: &ProjectData,
    opts: &DigestOptions,
    region: Option<(i64, i64)>,
) -> Result<String, String> {
    let region = match region {
        Some((s, e)) => Some(validate_range(project, s, e)?),
        None => None,
    };
    let circular = project.topology == "circular";
    let mut out = String::new();

    // LOCUS line
    let mut locus = format!(
        "LOCUS       {}    {} bp    {}",
        project.name, project.length, project.topology
    );
    if !project.methylation_systems.is_empty() {
        let systems: Vec<String> = project
            .methylation_systems
            .iter()
            .map(|s| {
                let mut c = s.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            })
            .collect();
        locus.push_str(&format!("    methylation: {}", systems.join(",")));
    }
    if let Some((rs, re)) = project.roi {
        locus.push_str(&format!("    ROI: {}..{}", rs, re));
    }
    if let Some((s, e)) = region {
        locus.push_str(&format!("    REGION: {}..{}", s, e));
    }
    out.push_str(&locus);
    out.push('\n');
    out.push_str(
        "COORDS: 0-based inclusive (features, primers, read ranges); primer template_end exclusive; enzyme cuts between pos-1 and pos\n",
    );

    // Features
    let features: Vec<&Feature> = project
        .features
        .iter()
        .filter(|f| feature_matches_filter(f, opts.feature_filter.as_deref()))
        .filter(|f| {
            region.map_or(true, |(s, e)| feature_in_region(f, s, e, circular))
        })
        .collect();
    out.push_str("FEATURES (0-based, inclusive):\n");
    match opts.max_features {
        Some(max) if features.len() > max => {
            for f in features.iter().take(max) {
                out.push_str(&feature_line(f));
                out.push('\n');
            }
            out.push_str(&format!(
                "        ... and {} more features (narrow with feature_filter)\n",
                features.len() - max
            ));
        }
        _ => {
            for f in &features {
                out.push_str(&feature_line(f));
                out.push('\n');
            }
        }
    }

    // Primers: one line per binding site overlapping the region (sorted by start)
    let mut site_lines: Vec<(i64, String)> = Vec::new();
    let mut unbound: Vec<String> = Vec::new();
    for p in &project.primers {
        if p.binding_sites.is_empty() {
            unbound.push(format!("{} (id: {})", p.name, p.id));
            continue;
        }
        for s in &p.binding_sites {
            let covered = (s.template_start, s.template_end - 1);
            if region.map_or(true, |(rs, re)| {
                seg_in_range(covered.0, covered.1, rs, re, circular)
            }) {
                site_lines.push(primer_site_line(s, p));
            }
        }
    }
    if !site_lines.is_empty() || !unbound.is_empty() {
        out.push_str("PRIMERS (0-based, inclusive):\n");
        site_lines.sort_by_key(|(start, _)| *start);
        for (_, line) in &site_lines {
            out.push_str(line);
            out.push('\n');
        }
        if !unbound.is_empty() {
            out.push_str(&format!(
                "Primers without binding sites: {}\n",
                unbound.join(", ")
            ));
        }
    } else if region.is_none() {
        out.push_str("PRIMERS (none)\n");
    }

    // Alignments: one line per stored read overlapping the region.
    // Stored alignments always passed the significant-match thresholds.
    let alignments: Vec<&crate::models::Alignment> = project
        .alignments
        .iter()
        .filter(|a| {
            region.map_or(true, |(s, e)| alignment_in_region(a, s, e, circular))
        })
        .collect();
    if !alignments.is_empty() {
        out.push_str("ALIGNMENTS (0-based, inclusive):\n");
        for a in alignments {
            out.push_str(&alignment_line(a));
            out.push('\n');
        }
    }

    // Enzymes
    match region {
        None => {
            let (unique, twice, others) = classify_enzymes(project);
            // Multi-cut enzymes are summarized to keep the digest compact;
            // names are counted once even when they appear as several sites.
            let mut multi_names: Vec<&str> = twice.iter().map(|e| e.name.as_str()).collect();
            multi_names.sort_unstable();
            multi_names.dedup();
            let multi = multi_names.len() + others;
            if opts.compact_enzymes {
                if !unique.is_empty() || multi > 0 {
                    out.push_str(&format!(
                        "ENZYMES (compact): {} single-cut, {} multi-cut (cut between pos-1 and pos, 0-based)\n",
                        unique.len(),
                        multi
                    ));
                }
            } else if opts.compact_cutters {
                if !unique.is_empty() {
                    out.push_str(&format!(
                        "UNIQUE CUTTERS: {} single-cut enzymes (pass compactCutters=false for full list)\n",
                        unique.len()
                    ));
                }
                if multi > 0 {
                    out.push_str(&format!(
                        "... and {} enzymes with >1 cut (use get_enzyme_database for details)\n",
                        multi
                    ));
                }
            } else {
                if !unique.is_empty() {
                    out.push_str("UNIQUE CUTTERS (cut between pos-1 and pos, 0-based):\n");
                    for e in unique {
                        out.push_str(&format!(
                            "        {:<10} {:<28} {:<10} {}\n",
                            e.name,
                            cuts_desc(e),
                            e.rec_seq,
                            cut_type_label(&e.cut_type)
                        ));
                    }
                }
                if multi > 0 {
                    out.push_str(&format!(
                        "... and {} enzymes with >1 cut (use get_enzyme_database for details)\n",
                        multi
                    ));
                }
            }
        }
        Some((s, e)) => {
            let in_region: Vec<&Enzyme> = project
                .enzymes
                .iter()
                .filter(|en| enzyme_in_region(en, s, e, circular))
                .collect();
            if !in_region.is_empty() {
                if opts.compact_enzymes {
                    out.push_str(&format!(
                        "ENZYMES CUTTING IN REGION (compact): {} cuts (cut between pos-1 and pos, 0-based)\n",
                        in_region.len()
                    ));
                } else {
                    out.push_str("ENZYMES CUTTING IN REGION (cut between pos-1 and pos, 0-based):\n");
                    for en in in_region {
                        out.push_str(&format!(
                            "        {:<10} {}   {}\n",
                            en.name,
                            cuts_desc(en),
                            cut_type_label(&en.cut_type)
                        ));
                    }
                }
            }
        }
    }

    Ok(out)
}

/// Plain bases of `[start, end]` (no ruler); circular wrap supported.
pub fn read_sequence_bases(project: &ProjectData, start: i64, end: i64) -> Result<String, String> {
    let (s, e) = validate_range(project, start, end)?;
    let count = if s <= e { e - s + 1 } else { project.length - s + e + 1 };
    if count as usize > MAX_READ_BASES {
        return Err(format!(
            "requested {} bp exceeds the {} bp read limit; request a narrower window",
            count, MAX_READ_BASES
        ));
    }
    let mut window = String::with_capacity(count as usize);
    if s <= e {
        window.push_str(&project.sequence[s as usize..=e as usize]);
    } else {
        window.push_str(&project.sequence[s as usize..]);
        window.push_str(&project.sequence[..=e as usize]);
    }
    Ok(window.to_ascii_uppercase())
}

/// Bases in `[start, end]` with a coordinate ruler; circular wrap supported.
pub fn read_sequence(project: &ProjectData, start: i64, end: i64) -> Result<String, String> {
    let (s, e) = validate_range(project, start, end)?;
    let circular = project.topology == "circular";
    let count = if s <= e { e - s + 1 } else { project.length - s + e + 1 };
    if count as usize > MAX_READ_BASES {
        return Err(format!(
            "requested {} bp exceeds the {} bp read limit; request a narrower window",
            count, MAX_READ_BASES
        ));
    }
    let bytes = project.sequence.as_bytes();
    let mut window = Vec::with_capacity(count as usize);
    if s <= e {
        window.extend_from_slice(&bytes[s as usize..=e as usize]);
    } else {
        window.extend_from_slice(&bytes[s as usize..]);
        window.extend_from_slice(&bytes[..=e as usize]);
    }

    const GROUP: usize = 10;
    const COLS: usize = 6;
    const LINE_BASES: usize = GROUP * COLS;

    let mut out = String::new();
    out.push_str(&format!(
        "COORDS: 0-based inclusive. Window {}..{} ({} bp) of {} bp {} (wrap: {})\n",
        s, e, count, project.length, project.topology, circular
    ));
    // Ruler labels the group start positions of the first line.
    out.push_str(&" ".repeat(7));
    for i in 0..COLS {
        out.push_str(&format!("{:>11}", s + (i as i64) * GROUP as i64));
    }
    out.push('\n');
    for (idx, &base) in window.iter().enumerate() {
        if idx % LINE_BASES == 0 {
            if idx > 0 {
                out.push('\n');
            }
            out.push_str(&format!("{:>6} ", s + idx as i64));
        }
        out.push((base as char).to_ascii_uppercase());
        if (idx + 1) % GROUP == 0 && (idx + 1) % LINE_BASES != 0 {
            out.push(' ');
        }
    }
    out.push('\n');
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CutPair, Primer, Segment};

    fn synthetic_project() -> ProjectData {
        let mut p = ProjectData {
            name: "TestPlasmid".to_string(),
            definition: String::new(),
            keywords: String::new(),
            lab_host: String::new(),
            sequence: "ACGT".repeat(15),
            length: 60,
            topology: "circular".to_string(),
            features: vec![
                Feature {
                    id: "f1".into(),
                    name: "repA".into(),
                    start: 10,
                    end: 30,
                    color: "#60A5FA".into(),
                    ftype: "CDS".into(),
                    segments: vec![Segment {
                        start: 10,
                        end: 30,
                        color: None,
                    }],
                    strand: "-".into(),
                    notes: String::new(),
                    translation: String::new(),
                    qualifiers: Vec::new(),
                },
                Feature {
                    id: "f2".into(),
                    name: "segFeat".into(),
                    start: 0,
                    end: 49,
                    color: "#F87171".into(),
                    ftype: "regulatory".into(),
                    segments: vec![
                        Segment {
                            start: 0,
                            end: 5,
                            color: None,
                        },
                        Segment {
                            start: 40,
                            end: 49,
                            color: None,
                        },
                    ],
                    strand: "+".into(),
                    notes: String::new(),
                    translation: String::new(),
                    qualifiers: Vec::new(),
                },
            ],
            primers: vec![
                Primer {
                    id: "p1".into(),
                    name: "P1".into(),
                    r#type: "fwd".into(),
                    primer_seq: "ACGTACGTAC".into(),
                    binding_sites: vec![
                        PrimerBindingSite {
                            primer_id: "p1".into(),
                            strand: 1,
                            template_start: 2,
                            template_end: 12,
                            tm: 58.3,
                            gc_content: 0.5,
                            match_score: 10,
                            has_3_prime_mismatch: false,
                            five_prime_tail: String::new(),
                            three_prime_tail: String::new(),
                            alignment: Default::default(),
                        },
                        PrimerBindingSite {
                            primer_id: "p1".into(),
                            strand: -1,
                            template_start: 50,
                            template_end: 60,
                            tm: 60.1,
                            gc_content: 0.5,
                            match_score: 10,
                            has_3_prime_mismatch: true,
                            five_prime_tail: String::new(),
                            three_prime_tail: String::new(),
                            alignment: Default::default(),
                        },
                    ],
                },
                Primer {
                    id: "p2".into(),
                    name: "orphan".into(),
                    r#type: "rev".into(),
                    primer_seq: "GGGGGG".into(),
                    binding_sites: Vec::new(),
                },
            ],
            alignments: Vec::new(),
            enzymes: vec![
                Enzyme {
                    id: "ecori".into(),
                    name: "EcoRI".into(),
                    rec_seq: "GAATTC".into(),
                    rec_start: 4,
                    rec_end: 9,
                    display_start: 4,
                    display_end: 14,
                    cut_index: 10,
                    bot_cut_index: 14,
                    cut_pairs: vec![CutPair {
                        top_cut_index: 10,
                        bot_cut_index: 14,
                    }],
                    recognition_strand: "top".into(),
                    comp_seq: String::new(),
                    rec_seq_pattern: "GAATTC".into(),
                    spacers: None,
                    is_unique: true,
                    is_methylation_sensitive: false,
                    methylation_blocked: false,
                    methylated_offsets: Vec::new(),
                    methylation_sources: Vec::new(),
                    methylation_required: false,
                    methyl_required_offsets: Vec::new(),
                    methyl_required_sources: Vec::new(),
                    cut_type: "5overhang".into(),
                    cut_twice: false,
                    is_palindromic: true,
                },
                // BsaI: two recognition sites
                Enzyme {
                    id: "bsai_0".into(),
                    name: "BsaI".into(),
                    rec_seq: "GGTCTC".into(),
                    rec_start: 14,
                    rec_end: 19,
                    display_start: 14,
                    display_end: 24,
                    cut_index: 20,
                    bot_cut_index: 24,
                    cut_pairs: vec![CutPair {
                        top_cut_index: 20,
                        bot_cut_index: 24,
                    }],
                    recognition_strand: "top".into(),
                    comp_seq: String::new(),
                    rec_seq_pattern: "GGTCTC".into(),
                    spacers: None,
                    is_unique: false,
                    is_methylation_sensitive: false,
                    methylation_blocked: false,
                    methylated_offsets: Vec::new(),
                    methylation_sources: Vec::new(),
                    methylation_required: false,
                    methyl_required_offsets: Vec::new(),
                    methyl_required_sources: Vec::new(),
                    cut_type: "5overhang".into(),
                    cut_twice: false,
                    is_palindromic: true,
                },
                Enzyme {
                    id: "bsai_1".into(),
                    name: "BsaI".into(),
                    rec_seq: "GGTCTC".into(),
                    rec_start: 34,
                    rec_end: 39,
                    display_start: 34,
                    display_end: 44,
                    cut_index: 40,
                    bot_cut_index: 44,
                    cut_pairs: vec![CutPair {
                        top_cut_index: 40,
                        bot_cut_index: 44,
                    }],
                    recognition_strand: "top".into(),
                    comp_seq: String::new(),
                    rec_seq_pattern: "GGTCTC".into(),
                    spacers: None,
                    is_unique: false,
                    is_methylation_sensitive: false,
                    methylation_blocked: false,
                    methylated_offsets: Vec::new(),
                    methylation_sources: Vec::new(),
                    methylation_required: false,
                    methyl_required_offsets: Vec::new(),
                    methyl_required_sources: Vec::new(),
                    cut_type: "5overhang".into(),
                    cut_twice: false,
                    is_palindromic: true,
                },
                // BbsI: one site, cut-twice (two cut pairs)
                Enzyme {
                    id: "bbsi".into(),
                    name: "BbsI".into(),
                    rec_seq: "GAAGAC".into(),
                    rec_start: 10,
                    rec_end: 15,
                    display_start: 10,
                    display_end: 34,
                    cut_index: 15,
                    bot_cut_index: 19,
                    cut_pairs: vec![
                        CutPair {
                            top_cut_index: 15,
                            bot_cut_index: 19,
                        },
                        CutPair {
                            top_cut_index: 30,
                            bot_cut_index: 34,
                        },
                    ],
                    recognition_strand: "top".into(),
                    comp_seq: String::new(),
                    rec_seq_pattern: "GAAGAC".into(),
                    spacers: None,
                    is_unique: true,
                    is_methylation_sensitive: false,
                    methylation_blocked: false,
                    methylated_offsets: Vec::new(),
                    methylation_sources: Vec::new(),
                    methylation_required: false,
                    methyl_required_offsets: Vec::new(),
                    methyl_required_sources: Vec::new(),
                    cut_type: "5overhang".into(),
                    cut_twice: true,
                    is_palindromic: true,
                },
            ],
            methylation_systems: vec!["dam".into(), "dcm".into()],
            methylation_overlap: 2,
            roi: Some((5, 20)),
        };
        // Three names that cut 3+ times
        for name in ["AatII", "HaeII", "EcoRV"] {
            for i in 0..3 {
                p.enzymes.push(Enzyme {
                    id: format!("{}_{}", name, i),
                    name: name.into(),
                    rec_seq: "GGCC".into(),
                    rec_start: i * 10,
                    rec_end: i * 10 + 3,
                    display_start: i * 10,
                    display_end: i * 10 + 4,
                    cut_index: i * 10 + 2,
                    bot_cut_index: i * 10 + 6,
                    cut_pairs: vec![CutPair {
                        top_cut_index: i * 10 + 2,
                        bot_cut_index: i * 10 + 6,
                    }],
                    recognition_strand: "top".into(),
                    comp_seq: String::new(),
                    rec_seq_pattern: "GGCC".into(),
                    spacers: None,
                    is_unique: false,
                    is_methylation_sensitive: false,
                    methylation_blocked: false,
                    methylated_offsets: Vec::new(),
                    methylation_sources: Vec::new(),
                    methylation_required: false,
                    methyl_required_offsets: Vec::new(),
                    methyl_required_sources: Vec::new(),
                    cut_type: "blunt".into(),
                    cut_twice: false,
                    is_palindromic: true,
                });
            }
        }
        p
    }

    #[test]
    fn overview_contains_header_and_coords() {
        let out = project_digest(&synthetic_project(), &DigestOptions::default(), None).unwrap();
        assert!(out.starts_with(
            "LOCUS       TestPlasmid    60 bp    circular    methylation: Dam,Dcm    ROI: 5..20\n"
        ));
        assert!(out.contains("COORDS: 0-based inclusive"));
        assert!(out.contains("FEATURES (0-based, inclusive):\n"));
    }

    #[test]
    fn overview_renders_segmented_and_complement_features() {
        let out = project_digest(&synthetic_project(), &DigestOptions::default(), None).unwrap();
        assert!(out.contains("complement(10..30)"));
        assert!(out.contains("join(0..5,40..49)"));
        assert!(out.contains("repA  [#60A5FA]  (id: f1)"));
        assert!(out.contains("segFeat  [#F87171]  (id: f2)"));
    }

    #[test]
    fn overview_renders_primer_sites_and_unbound() {
        let out = project_digest(&synthetic_project(), &DigestOptions::default(), None).unwrap();
        assert!(out.contains("primer_bind     2..11   P1  [Tm 58.3, + strand]  (id: p1)"));
        assert!(out.contains("primer_bind     50..59   P1  [Tm 60.1, - strand, 3' mismatch]  (id: p1)"));
        assert!(out.contains("Primers without binding sites: orphan (id: p2)"));
    }

    #[test]
    fn overview_lists_unique_cutters_and_summarizes_multi_cutters() {
        let out = project_digest(&synthetic_project(), &DigestOptions::default(), None).unwrap();
        assert!(out.contains("UNIQUE CUTTERS (cut between pos-1 and pos, 0-based):"));
        assert!(out.contains("EcoRI"));
        assert!(out.contains("top 10^ bot 14"));
        assert!(out.contains("GAATTC"));
        assert!(out.contains("5' overhang"));
        // Multi-cut enzymes (BsaI 2 sites, BbsI cut-twice, + 3 names with 3+
        // sites) collapse into a single summary line.
        assert!(!out.contains("TWICE CUTTERS:"));
        assert!(!out.contains("BsaI"));
        assert!(!out.contains("BbsI"));
        assert!(out.contains("... and 5 enzymes with >1 cut"));
    }

    #[test]
    fn overview_max_features_caps_feature_lines() {
        let opts = DigestOptions {
            max_features: Some(1),
            feature_filter: None,
            ..DigestOptions::default()
        };
        let out = project_digest(&synthetic_project(), &opts, None).unwrap();
        assert!(out.contains("... and 1 more features"));
        assert!(out.matches("CDS").count() == 1);
    }

    #[test]
    fn overview_feature_filter_narrows_by_name() {
        let opts = DigestOptions {
            max_features: None,
            feature_filter: Some("seg".into()),
            ..DigestOptions::default()
        };
        let out = project_digest(&synthetic_project(), &opts, None).unwrap();
        assert!(out.contains("segFeat"));
        assert!(!out.contains("repA"));
    }

    #[test]
    fn region_view_excludes_non_overlapping_features() {
        let out = project_digest(
            &synthetic_project(),
            &DigestOptions::default(),
            Some((0, 1)),
        )
        .unwrap();
        assert!(out.contains("REGION: 0..1"));
        // CDS 10..30 does not overlap 0..1
        assert!(!out.contains("complement(10..30)"));
        // join(0..5, 40..49) overlaps via 0..5
        assert!(out.contains("join(0..5,40..49)"));
        // no enzyme cuts within 0..1
        assert!(!out.contains("ENZYMES CUTTING IN REGION"));
    }

    #[test]
    fn region_view_circular_wrap_covers_origin() {
        // 30..5 wraps: [30..59] ∪ [0..5]
        let out = project_digest(
            &synthetic_project(),
            &DigestOptions::default(),
            Some((30, 5)),
        )
        .unwrap();
        assert!(out.contains("REGION: 30..5"));
        // CDS 10..30 touches 30 (inclusive); join 0..5 touches origin
        assert!(out.contains("complement(10..30)"));
        assert!(out.contains("join(0..5,40..49)"));
        // BsaI second site cuts at 40/44 in [30..59]
        assert!(out.contains("ENZYMES CUTTING IN REGION"));
        assert!(out.contains("BsaI"));
        // P1 sites 2..11 (overlaps 0..5) and 50..59 (in [30..59])
        assert!(out.contains("P1  [Tm 58.3"));
        assert!(out.contains("P1  [Tm 60.1"));
    }

    #[test]
    fn read_sequence_bases_plain_and_wrap() {
        let p = synthetic_project();
        assert_eq!(read_sequence_bases(&p, 0, 9).unwrap(), "ACGTACGTAC");
        assert_eq!(read_sequence_bases(&p, 55, 4).unwrap(), "TACGTACGTA");
        assert!(read_sequence_bases(&p, 0, 100).is_err());
    }

    #[test]
    fn read_sequence_linear_window() {
        let out = read_sequence(&synthetic_project(), 0, 19).unwrap();
        assert!(out.contains("COORDS: 0-based inclusive. Window 0..19 (20 bp)"));
        assert!(out.contains("ACGTACGTAC GTACGTACGT"));
    }

    #[test]
    fn read_sequence_circular_wrap() {
        // positions 55..59 = TACGT, 0..4 = ACGTA
        let out = read_sequence(&synthetic_project(), 55, 4).unwrap();
        assert!(out.contains("Window 55..4 (10 bp) of 60 bp circular (wrap: true)"));
        assert!(out.contains("TACGTACGTA"));
    }

    #[test]
    fn read_sequence_rejects_bad_ranges() {
        let linear = ProjectData {
            name: "lin".into(),
            sequence: "ACGT".repeat(10),
            length: 40,
            topology: "linear".into(),
            ..Default::default()
        };
        assert!(read_sequence(&linear, 30, 5).is_err());
        assert!(read_sequence(&linear, 0, 100).is_err());
        assert!(read_sequence(&linear, -1, 5).is_err());
    }

    #[test]
    fn read_sequence_caps_huge_windows() {
        let big = ProjectData {
            name: "big".into(),
            sequence: "A".repeat(MAX_READ_BASES + 10),
            length: (MAX_READ_BASES + 10) as i64,
            topology: "linear".into(),
            ..Default::default()
        };
        let err = read_sequence(&big, 0, (MAX_READ_BASES + 9) as i64).unwrap_err();
        assert!(err.contains("read limit"));
    }

    #[test]
    fn overview_renders_alignments() {
        let mut p = synthetic_project();
        p.alignments.push(crate::models::Alignment {
            id: "aln-1".into(),
            name: "read1".into(),
            length: 70,
            strand: "+".into(),
            identity: 0.9857,
            segments: vec![crate::models::AlignSegment {
                start: 50,
                end: 59,
                chars: "ACGTACGTAC".into(),
            }],
            insertions: Vec::new(),
            seq: String::new(),
        });
        p.alignments.push(crate::models::Alignment {
            id: "aln-2".into(),
            name: "wrapped".into(),
            length: 60,
            strand: "-".into(),
            identity: 1.0,
            segments: vec![
                crate::models::AlignSegment {
                    start: 55,
                    end: 59,
                    chars: "ACGTA".into(),
                },
                crate::models::AlignSegment {
                    start: 0,
                    end: 4,
                    chars: "ACGTA".into(),
                },
            ],
            insertions: Vec::new(),
            seq: String::new(),
        });
        let out = project_digest(&p, &DigestOptions::default(), None).unwrap();
        assert!(out.contains("ALIGNMENTS (0-based, inclusive):\n"));
        assert!(out.contains("read1"));
        assert!(out.contains("50..59"));
        assert!(out.contains("+ strand  [identity 98.6%, significant]  (id: aln-1)"));
        assert!(out.contains("join(55..59,0..4)"));
        assert!(out.contains("- strand  [identity 100.0%, significant]  (id: aln-2)"));

        // Region view lists only overlapping alignments.
        let region = project_digest(&p, &DigestOptions::default(), Some((10, 30))).unwrap();
        assert!(!region.contains("ALIGNMENTS"));
        let region = project_digest(&p, &DigestOptions::default(), Some((45, 55))).unwrap();
        assert!(region.contains("read1"));
        assert!(region.contains("wrapped"));
        let region = project_digest(&p, &DigestOptions::default(), Some((0, 4))).unwrap();
        assert!(!region.contains("read1"));
        assert!(region.contains("wrapped"));
    }

    #[test]
    fn binding_site_helpers_agree_with_range() {
        // site covering [2..11]
        assert!(seg_in_range(2, 11, 30, 5, true));
        assert!(!seg_in_range(2, 11, 12, 20, false));
        assert!(seg_in_range(50, 59, 30, 5, true));
        assert!(pos_in_range(40, 30, 5, true));
        assert!(!pos_in_range(20, 30, 5, true));
    }

    #[test]
    fn compact_enzymes_collapses_cutter_lists() {
        let p = synthetic_project();
        let opts = DigestOptions {
            compact_enzymes: true,
            ..DigestOptions::default()
        };
        let out = project_digest(&p, &opts, None).unwrap();
        // EcoRI unique + BsaI/BbsI (2 multi names) + 3 triple-cut names
        assert!(out.contains("ENZYMES (compact): 1 single-cut, 5 multi-cut"));
        assert!(!out.contains("UNIQUE CUTTERS"));
        assert!(!out.contains("EcoRI"));
        let region = project_digest(&p, &opts, Some((30, 5))).unwrap();
        assert!(region.contains("ENZYMES CUTTING IN REGION (compact): "));
        assert!(!region.contains("BsaI"));
    }

    #[test]
    fn compact_cutters_collapses_unique_cutter_list() {
        let p = synthetic_project();
        // compactCutters=true (get_project_overview default): single count line,
        // no per-enzyme rows; multi-cut summary line stays.
        let opts = DigestOptions {
            compact_cutters: true,
            ..DigestOptions::default()
        };
        let out = project_digest(&p, &opts, None).unwrap();
        assert!(out.contains(
            "UNIQUE CUTTERS: 1 single-cut enzymes (pass compactCutters=false for full list)"
        ));
        assert!(!out.contains("UNIQUE CUTTERS (cut between pos-1 and pos, 0-based):"));
        assert!(!out.contains("EcoRI"));
        assert!(out.contains("... and 5 enzymes with >1 cut"));
        // compactCutters=false: the full per-enzyme list is back.
        let opts = DigestOptions {
            compact_cutters: false,
            ..DigestOptions::default()
        };
        let out = project_digest(&p, &opts, None).unwrap();
        assert!(out.contains("UNIQUE CUTTERS (cut between pos-1 and pos, 0-based):"));
        assert!(out.contains("EcoRI"));
        // compact_cutters is overview-only: region views ignore it.
        let region = project_digest(&p, &opts, Some((30, 5))).unwrap();
        assert!(region.contains("ENZYMES CUTTING IN REGION"));
    }

    #[test]
    fn overview_prints_primers_none_placeholder() {
        let mut p = synthetic_project();
        p.primers.clear();
        let out = project_digest(&p, &DigestOptions::default(), None).unwrap();
        assert!(out.contains("PRIMERS (none)\n"));
        // region views never emit the placeholder (a primer may exist elsewhere)
        let region = project_digest(&p, &DigestOptions::default(), Some((0, 10))).unwrap();
        assert!(!region.contains("PRIMERS"));
    }
}
