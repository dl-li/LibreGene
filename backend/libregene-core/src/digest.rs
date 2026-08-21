//! Text digest renderers — compact, LLM-friendly project summaries for the MCP server.
//!
//! The digest is a user/agent-facing layer: every RENDERED coordinate is
//! **1-based inclusive** (GenBank convention), while all INPUTS (the project
//! model fields and the `region`/`start`/`end` parameters) stay in the
//! internal **0-based inclusive** convention. Conversion happens at the render
//! points: an internal inclusive [s, e] prints as [s+1, e+1]; a primer site's
//! 0-based-exclusive `template_end` prints as-is (the 1-based inclusive end of
//! the site); an enzyme cut at 0-based index C (severing between bases C-1 and
//! C) prints as `N^N+1` — between the 1-based bases N=C and N+1. Circular
//! sequences allow `start > end` to wrap the origin.

use std::collections::HashMap;

use crate::models::{AlignDeletion, Enzyme, Feature, PrimerBindingSite, ProjectData};
use std::fmt::Write as _;

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
    /// Whole-project digests only: append a brief auto-annotation section
    /// (`DETECTED COMMON FEATURES (auto)`) listing non-fragment features the
    /// annotate engine found against the embedded SnapGene database, one line
    /// each with identity and an `(already annotated)` marker. Fragments are
    /// omitted to avoid misleading partial hits. Never affects region views.
    /// The section itself is already compact, so it is independent of
    /// `compact_enzymes`/`compact_cutters`.
    pub include_auto_annotation: bool,
}

// ---------------------------------------------------------------------------
// Range helpers (internal 0-based inclusive; circular wrap when start > end)
// ---------------------------------------------------------------------------

/// 1-based inclusive flanking bases of a cut at internal 0-based index C
/// (severing between bases C-1 and C): (C, C+1). A cut at the origin of a
/// circular molecule (C == 0) sits between the last and the first base.
pub fn cut_flanks(cut: i64, len: i64, circular: bool) -> (i64, i64) {
    if cut == 0 && circular {
        (len, 1)
    } else {
        (cut, cut + 1)
    }
}

/// Cut rendered as `N^M` — between the 1-based bases N and M.
pub fn cut_notation(cut: i64, len: i64, circular: bool) -> String {
    let (a, b) = cut_flanks(cut, len, circular);
    format!("{}^{}", a, b)
}

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
            "range {}..{} out of bounds for sequence of length {} (1-based inclusive)",
            start + 1,
            end + 1,
            len
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

/// Length unit for the molecule type (LOCUS line, read windows, error messages).
fn unit_for(molecule_type: &str) -> &'static str {
    match molecule_type {
        "rna" => "nt",
        "protein" => "aa",
        _ => "bp",
    }
}

/// Human-readable molecule type label for the LOCUS line.
fn molecule_label(molecule_type: &str) -> &'static str {
    match molecule_type {
        "rna" => "RNA",
        "protein" => "Protein",
        _ => "DNA",
    }
}

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
            site.template_start + 1,
            site.template_end,
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

/// A deletion covers template columns `pos .. pos+length-1`; a merged deletion
/// straddling the circular origin wraps (pos + length may exceed tlen).
fn deletion_in_region(d: &AlignDeletion, s: i64, e: i64, circular: bool, tlen: i64) -> bool {
    let start = d.pos as i64;
    let end = d.pos as i64 + d.length as i64 - 1;
    if end < tlen {
        seg_in_range(start, end, s, e, circular)
    } else {
        seg_in_range(start, tlen - 1, s, e, circular) || seg_in_range(0, end - tlen, s, e, circular)
    }
}

fn alignment_line(a: &crate::models::Alignment) -> String {
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

fn cut_type_label(cut_type: &str) -> &str {
    match cut_type {
        "5overhang" => "5' overhang",
        "3overhang" => "3' overhang",
        _ => "blunt",
    }
}

fn cuts_desc(e: &Enzyme, len: i64, circular: bool) -> String {
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

/// annotate.rs appends " (fragment)" to partial hits; strip it here so the
/// marker column is authoritative and fragment names don't double-mark.
fn auto_feature_display_name(f: &crate::annotate::AnnotatedFeature) -> &str {
    f.name
        .strip_suffix(" (fragment)")
        .unwrap_or(f.name.as_str())
}

/// True when any existing project feature overlaps the auto-detected feature
/// by name (case-insensitive) or by coordinates — i.e. it is likely already
/// annotated in the project.
fn auto_feature_already_annotated(
    project: &ProjectData,
    f: &crate::annotate::AnnotatedFeature,
) -> bool {
    let circular = project.topology == "circular";
    let display_name = auto_feature_display_name(f);
    project.features.iter().any(|ef| {
        let name_match = ef.name.eq_ignore_ascii_case(display_name);
        let coord_match = if ef.segments.is_empty() {
            seg_in_range(ef.start, ef.end, f.start, f.end, circular)
        } else {
            ef.segments
                .iter()
                .any(|s| seg_in_range(s.start, s.end, f.start, f.end, circular))
        };
        name_match || coord_match
    })
}

/// Brief auto-annotation section for whole-project overviews: one line per
/// detected common feature. The engine builds a k-mer index once per process
/// (first call only); the section is kept intentionally compact so no
/// `compact_*` option affects it. DNA projects match nucleotide + protein
/// level; protein projects match the aa sequence against CDS translations.
fn push_auto_annotation(out: &mut String, project: &ProjectData) {
    out.push_str("DETECTED COMMON FEATURES (auto):\n");
    let all = if project.is_dna() {
        crate::annotate::annotate_sequence(&project.sequence, project.topology == "circular")
    } else {
        crate::annotate::annotate_protein(&project.sequence, project.topology == "circular")
    };
    let detected: Vec<_> = all.into_iter().filter(|f| !f.fragment).collect();
    if detected.is_empty() {
        out.push_str("(none)\n");
        return;
    }
    for f in &detected {
        let _ = write!(out, 
            "        {} | {} | {} | {}..{} | {:.1}%",
            auto_feature_display_name(f),
            f.ftype,
            f.strand,
            f.start + 1,
            f.end + 1,
            f.identity
        );
        if f.match_level == "aa" {
            out.push_str(" | (protein-level)");
        }
        if auto_feature_already_annotated(project, f) {
            out.push_str(" | (already annotated)");
        }
        out.push('\n');
    }
}

/// Full or region-filtered project digest. `region` is internal 0-based
/// inclusive (`start > end` wraps the origin on circular sequences); all
/// rendered coordinates are 1-based inclusive.
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
    let is_dna = project.is_dna();
    let mut out = String::new();

    // LOCUS line
    let mut locus = format!(
        "LOCUS       {}    {} {}    {} {}",
        project.name,
        project.length,
        unit_for(&project.molecule_type),
        project.topology,
        molecule_label(&project.molecule_type),
    );
    if is_dna && !project.methylation_systems.is_empty() {
        let systems: Vec<String> = project
            .methylation_systems
            .iter()
            .map(|s| {
                if s.eq_ignore_ascii_case("ecoki") {
                    "EcoKI".to_string()
                } else {
                    let mut c = s.chars();
                    match c.next() {
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                }
            })
            .collect();
        let _ = write!(locus, "    methylation: {}", systems.join(","));
    }
    if let Some((rs, re)) = project.roi {
        let _ = write!(locus, "    ROI: {}..{}", rs + 1, re + 1);
    }
    if let Some((s, e)) = region {
        let _ = write!(locus, "    REGION: {}..{}", s + 1, e + 1);
    }
    out.push_str(&locus);
    out.push('\n');
    if is_dna {
        out.push_str(
            "COORDS: 1-based inclusive (features, primers, read ranges); enzyme cuts shown as N^N+1 = between bases N and N+1\n",
        );
    } else {
        out.push_str("COORDS: 1-based inclusive (features, read ranges)\n");
    }

    // Features
    let features: Vec<&Feature> = project
        .features
        .iter()
        .filter(|f| feature_matches_filter(f, opts.feature_filter.as_deref()))
        .filter(|f| {
            region.map_or(true, |(s, e)| feature_in_region(f, s, e, circular))
        })
        .collect();
    out.push_str("FEATURES (1-based, inclusive):\n");
    match opts.max_features {
        Some(max) if features.len() > max => {
            for f in features.iter().take(max) {
                out.push_str(&feature_line(f));
                out.push('\n');
            }
            let _ = write!(out, 
                "        ... and {} more features (narrow with feature_filter)\n",
                features.len() - max
            );
        }
        _ => {
            for f in &features {
                out.push_str(&feature_line(f));
                out.push('\n');
            }
        }
    }

    // Primers: one line per binding site overlapping the region (sorted by start).
    // Single-strand molecules (rna/protein) carry no primers.
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
    if is_dna && (!site_lines.is_empty() || !unbound.is_empty()) {
        out.push_str("PRIMERS (1-based, inclusive):\n");
        site_lines.sort_by_key(|(start, _)| *start);
        for (_, line) in &site_lines {
            out.push_str(line);
            out.push('\n');
        }
        if !unbound.is_empty() {
            let _ = write!(out, 
                "Primers without binding sites: {}\n",
                unbound.join(", ")
            );
        }
    } else if is_dna && region.is_none() {
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
        out.push_str("ALIGNMENTS (1-based, inclusive):\n");
        for a in &alignments {
            out.push_str(&alignment_line(a));
            out.push('\n');
        }
    }

    // Region views only: per-alignment differences inside the window, so an
    // agent can check whether a site is mutated without eyeballing raw reads.
    if let Some((s, e)) = region {
        let mut section = String::new();
        for a in &alignments {
            let diff = crate::align::alignment_diff(a, &project.sequence);
            let mismatches: Vec<_> = diff
                .mismatches
                .iter()
                .filter(|m| pos_in_range(m.pos as i64, s, e, circular))
                .collect();
            let deletions: Vec<_> = diff
                .deletions
                .iter()
                .filter(|d| deletion_in_region(d, s, e, circular, project.length))
                .collect();
            let insertions: Vec<_> = diff
                .insertions
                .iter()
                .filter(|i| pos_in_range(i.pos as i64, s, e, circular))
                .collect();
            if section.is_empty() {
                section.push_str("ALIGNMENT DIFFS IN REGION (1-based inclusive):\n");
            }
            let _ = write!(section, "        {}  (id: {}):", a.name, a.id);
            if mismatches.is_empty() && deletions.is_empty() && insertions.is_empty() {
                section.push_str(" no differences in window\n");
                continue;
            }
            section.push('\n');
            for m in mismatches {
                let _ = write!(section, 
                    "          mismatch at {}: {} > {}\n",
                    m.pos + 1,
                    m.template_base,
                    m.read_base
                );
            }
            for d in deletions {
                let _ = write!(section, 
                    "          deletion at {}: {} bp ({})\n",
                    d.pos + 1,
                    d.length,
                    d.bases
                );
            }
            for i in insertions {
                let (a1, b1) = cut_flanks(i.pos as i64, project.length, circular);
                let _ = write!(section, 
                    "          insertion between {} and {}: {} ({} bp)\n",
                    a1, b1, i.bases, i.length
                );
            }
        }
        out.push_str(&section);
    }

    // Enzymes (DNA only — single-strand molecules have no restriction sites)
    if is_dna {
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
                        let _ = write!(out, 
                            "ENZYMES (compact): {} single-cut, {} multi-cut (cuts shown as N^N+1, 1-based)\n",
                            unique.len(),
                            multi
                        );
                    }
                } else if opts.compact_cutters {
                    if !unique.is_empty() {
                        let _ = write!(out, 
                            "UNIQUE CUTTERS: {} single-cut enzymes (pass compactCutters=false for full list)\n",
                            unique.len()
                        );
                    }
                    if multi > 0 {
                        let _ = write!(out, 
                            "... and {} enzymes with >1 cut (use get_enzyme_database for details)\n",
                            multi
                        );
                    }
                } else {
                    if !unique.is_empty() {
                        out.push_str("UNIQUE CUTTERS (cuts shown as N^N+1 = between 1-based bases N and N+1):\n");
                        for e in unique {
                            let _ = write!(out, 
                                "        {:<10} {:<28} {:<10} {}\n",
                                e.name,
                                cuts_desc(e, project.length, circular),
                                e.rec_seq,
                                cut_type_label(&e.cut_type)
                            );
                        }
                    }
                    if multi > 0 {
                        let _ = write!(out, 
                            "... and {} enzymes with >1 cut (use get_enzyme_database for details)\n",
                            multi
                        );
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
                        let _ = write!(out, 
                            "ENZYMES CUTTING IN REGION (compact): {} cuts (cuts shown as N^N+1, 1-based)\n",
                            in_region.len()
                        );
                    } else {
                        out.push_str("ENZYMES CUTTING IN REGION (cuts shown as N^N+1 = between 1-based bases N and N+1):\n");
                        for en in in_region {
                            let _ = write!(out, 
                                "        {:<10} {}   {}\n",
                                en.name,
                                cuts_desc(en, project.length, circular),
                                cut_type_label(&en.cut_type)
                            );
                        }
                    }
                }
            }
        }
    }

    // Auto-annotation is a whole-project overview concern only (and the DNA
    // feature database is meaningless for single-strand molecules); region
    // views keep the digest focused on the requested window.
    let is_protein = project.molecule_type == "protein";
    if (is_dna || is_protein) && region.is_none() && opts.include_auto_annotation {
        push_auto_annotation(&mut out, project);
    }

    Ok(out)
}

/// Plain bases of `[start, end]` (no ruler); circular wrap supported.
pub fn read_sequence_bases(project: &ProjectData, start: i64, end: i64) -> Result<String, String> {
    let (s, e) = validate_range(project, start, end)?;
    let count = if s <= e { e - s + 1 } else { project.length - s + e + 1 };
    if count as usize > MAX_READ_BASES {
        let unit = unit_for(&project.molecule_type);
        return Err(format!(
            "requested {} {} exceeds the {} {} read limit; request a narrower window",
            count, unit, MAX_READ_BASES, unit
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
        let unit = unit_for(&project.molecule_type);
        return Err(format!(
            "requested {} {} exceeds the {} {} read limit; request a narrower window",
            count, unit, MAX_READ_BASES, unit
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
    let unit = unit_for(&project.molecule_type);
    let _ = write!(out, 
        "COORDS: 1-based inclusive. Window {}..{} ({} {}) of {} {} {} (wrap: {})\n",
        s + 1,
        e + 1,
        count,
        unit,
        project.length,
        unit,
        project.topology,
        circular
    );
    // Ruler labels the group start positions of the first line. Small windows
    // (a single sequence line) skip it — the per-line coordinate prefix
    // already anchors the position and the ruler would dominate the output.
    if count as usize > LINE_BASES {
        out.push_str(&" ".repeat(7));
        for i in 0..COLS {
            let _ = write!(out, "{:>11}", s + 1 + (i as i64) * GROUP as i64);
        }
        out.push('\n');
    }
    for (idx, &base) in window.iter().enumerate() {
        if idx % LINE_BASES == 0 {
            if idx > 0 {
                out.push('\n');
            }
            let _ = write!(out, "{:>6} ", s + 1 + idx as i64);
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
            molecule_type: "dna".to_string(),
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
            "LOCUS       TestPlasmid    60 bp    circular DNA    methylation: Dam,Dcm    ROI: 6..21\n"
        ));
        assert!(out.contains("COORDS: 1-based inclusive"));
        assert!(out.contains("FEATURES (1-based, inclusive):\n"));
    }

    fn protein_project() -> ProjectData {
        let mut p = synthetic_project();
        p.name = "TestProtein".to_string();
        p.sequence = "MAAA".repeat(10);
        p.length = 40;
        p.topology = "linear".to_string();
        p.molecule_type = "protein".to_string();
        p
    }

    #[test]
    fn overview_protein_uses_aa_and_omits_dna_sections() {
        // Protein projects keep features but drop every DNA-only section,
        // even when the underlying model still carries primers/enzymes.
        let out = project_digest(&protein_project(), &DigestOptions::default(), None).unwrap();
        assert!(out.starts_with("LOCUS       TestProtein    40 aa    linear Protein"));
        assert!(!out.contains("methylation:"));
        assert!(!out.contains("PRIMERS"));
        assert!(!out.contains("ENZYMES"));
        assert!(!out.contains("UNIQUE CUTTERS"));
        assert!(!out.contains("cuts shown as N^N+1"));
        assert!(out.contains("FEATURES (1-based, inclusive):\n"));
        assert!(out.contains("repA  [#60A5FA]  (id: f1)"));
        // Region views also skip the enzyme layer for non-DNA.
        let region = project_digest(&protein_project(), &DigestOptions::default(), Some((0, 39))).unwrap();
        assert!(!region.contains("ENZYMES CUTTING IN REGION"));
        // Auto-annotation runs on protein projects too, matching the aa
        // sequence against the database's translated CDS features; the
        // MAAA-repeat test protein matches nothing.
        let opts = DigestOptions {
            include_auto_annotation: true,
            ..DigestOptions::default()
        };
        let out = project_digest(&protein_project(), &opts, None).unwrap();
        assert!(out.contains("DETECTED COMMON FEATURES (auto):\n(none)"));
    }

    #[test]
    fn overview_protein_auto_annotation_detects_cds_translation() {
        let mut p = protein_project();
        p.sequence = crate::annotate::db_protein_for_test("KanR_(3)").unwrap();
        p.length = p.sequence.len() as i64;
        let opts = DigestOptions {
            include_auto_annotation: true,
            ..DigestOptions::default()
        };
        let out = project_digest(&p, &opts, None).unwrap();
        assert!(
            out.contains("DETECTED COMMON FEATURES (auto):\n        KanR | CDS"),
            "KanR protein should be detected, got:\n{out}"
        );
    }

    #[test]
    fn overview_rna_uses_nt() {
        let mut p = synthetic_project();
        p.name = "TestRNA".to_string();
        p.sequence = "ACGU".repeat(15);
        p.length = 60;
        p.topology = "linear".to_string();
        p.molecule_type = "rna".to_string();
        let out = project_digest(&p, &DigestOptions::default(), None).unwrap();
        assert!(out.starts_with("LOCUS       TestRNA    60 nt    linear RNA"));
        assert!(!out.contains("PRIMERS"));
        assert!(!out.contains("ENZYMES"));
    }

    #[test]
    fn read_sequence_protein_uses_aa_units() {
        let p = protein_project();
        let out = read_sequence(&p, 0, 9).unwrap();
        assert!(out.contains("COORDS: 1-based inclusive. Window 1..10 (10 aa) of 40 aa linear (wrap: false)"));
        assert_eq!(read_sequence_bases(&p, 0, 9).unwrap(), "MAAAMAAAMA");
        // Read-limit error message uses the mapped unit too.
        let big = ProjectData {
            name: "bigProt".into(),
            sequence: "M".repeat(MAX_READ_BASES + 10),
            length: (MAX_READ_BASES + 10) as i64,
            topology: "linear".into(),
            molecule_type: "protein".into(),
            ..Default::default()
        };
        let err = read_sequence_bases(&big, 0, (MAX_READ_BASES + 9) as i64).unwrap_err();
        assert!(err.contains("aa"), "error should use aa units: {err}");
    }

    #[test]
    fn overview_renders_segmented_and_complement_features() {
        let out = project_digest(&synthetic_project(), &DigestOptions::default(), None).unwrap();
        assert!(out.contains("complement(11..31)"));
        assert!(out.contains("join(1..6,41..50)"));
        assert!(out.contains("repA  [#60A5FA]  (id: f1)"));
        assert!(out.contains("segFeat  [#F87171]  (id: f2)"));
    }

    #[test]
    fn overview_renders_primer_sites_and_unbound() {
        let out = project_digest(&synthetic_project(), &DigestOptions::default(), None).unwrap();
        assert!(out.contains("primer_bind     3..12   P1  [Tm 58.3, + strand]  (id: p1)"));
        assert!(out.contains("primer_bind     51..60   P1  [Tm 60.1, - strand, 3' mismatch]  (id: p1)"));
        assert!(out.contains("Primers without binding sites: orphan (id: p2)"));
    }

    #[test]
    fn overview_lists_unique_cutters_and_summarizes_multi_cutters() {
        let out = project_digest(&synthetic_project(), &DigestOptions::default(), None).unwrap();
        assert!(out.contains("UNIQUE CUTTERS (cuts shown as N^N+1 = between 1-based bases N and N+1):"));
        assert!(out.contains("EcoRI"));
        assert!(out.contains("top 10^11 bot 14^15"));
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
        assert!(out.contains("REGION: 1..2"));
        // CDS 10..30 does not overlap 0..1
        assert!(!out.contains("complement(11..31)"));
        // join(0..5, 40..49) overlaps via 0..5
        assert!(out.contains("join(1..6,41..50)"));
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
        assert!(out.contains("REGION: 31..6"));
        // CDS 10..30 touches 30 (inclusive); join 0..5 touches origin
        assert!(out.contains("complement(11..31)"));
        assert!(out.contains("join(1..6,41..50)"));
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
        assert!(out.contains("COORDS: 1-based inclusive. Window 1..20 (20 bp)"));
        assert!(out.contains("ACGTACGTAC GTACGTACGT"));
    }

    #[test]
    fn read_sequence_small_window_omits_ruler() {
        // A ≤60 bp window fits one sequence line: the 6-column ruler is
        // omitted, the per-line position prefix still anchors coordinates.
        let out = read_sequence(&synthetic_project(), 0, 19).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "small window must skip the ruler: {:?}", out);
        assert!(
            lines[1].starts_with("     1 "),
            "sequence line keeps its position prefix: {:?}",
            lines[1]
        );
    }

    #[test]
    fn read_sequence_ruler_threshold_boundary() {
        let big = ProjectData {
            name: "big".into(),
            sequence: "ACGT".repeat(30),
            length: 120,
            topology: "linear".into(),
            ..Default::default()
        };
        // Exactly 60 bp (one full line): still compact, no ruler.
        let out = read_sequence(&big, 0, 59).unwrap();
        assert_eq!(out.lines().count(), 2, "60 bp window: {:?}", out);
        // 61 bp spills onto a second line: the ruler comes back.
        let out = read_sequence(&big, 0, 60).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 4, "61 bp window keeps the ruler: {:?}", out);
        assert!(lines[1].contains("11"), "ruler line present: {:?}", lines[1]);
    }

    #[test]
    fn read_sequence_circular_wrap() {
        // positions 55..59 = TACGT, 0..4 = ACGTA
        let out = read_sequence(&synthetic_project(), 55, 4).unwrap();
        assert!(out.contains("Window 56..5 (10 bp) of 60 bp circular (wrap: true)"));
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
        assert!(out.contains("ALIGNMENTS (1-based, inclusive):\n"));
        assert!(out.contains("read1"));
        assert!(out.contains("51..60"));
        assert!(out.contains("+ strand  [identity 98.6%, significant]  (id: aln-1)"));
        assert!(out.contains("join(56..60,1..5)"));
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

    /// aln-1 covers 10..29 with a mismatch at 15, a 2 bp deletion at 18..19
    /// and a 2 bp insertion before 25 (template is "ACGT"*15, so t[15]=T,
    /// t[18..19]=GT).
    fn project_with_diff_alignment() -> ProjectData {
        let mut p = synthetic_project();
        let mut chars: Vec<char> = p.sequence[10..=29].chars().collect();
        chars[5] = 'A'; // mismatch at 15 (T > A)
        chars[8] = '-'; // deletion at 18..19
        chars[9] = '-';
        p.alignments.push(crate::models::Alignment {
            id: "aln-1".into(),
            name: "read1".into(),
            length: 21,
            strand: "+".into(),
            identity: 0.9,
            segments: vec![crate::models::AlignSegment {
                start: 10,
                end: 29,
                chars: chars.iter().collect(),
            }],
            insertions: vec![crate::models::AlignInsertion {
                pos: 25,
                bases: "GG".into(),
            }],
            seq: String::new(),
        });
        p
    }

    #[test]
    fn region_view_lists_alignment_diffs_in_window() {
        let p = project_with_diff_alignment();
        let out = project_digest(&p, &DigestOptions::default(), Some((10, 29))).unwrap();
        assert!(out.contains("ALIGNMENT DIFFS IN REGION (1-based inclusive):\n"), "{out}");
        assert!(out.contains("read1  (id: aln-1):\n"), "{out}");
        assert!(out.contains("mismatch at 16: T > A"), "{out}");
        assert!(out.contains("deletion at 19: 2 bp (GT)"), "{out}");
        assert!(out.contains("insertion between 25 and 26: GG (2 bp)"), "{out}");

        // Window overlapping the read but left of every diff.
        let out = project_digest(&p, &DigestOptions::default(), Some((10, 12))).unwrap();
        assert!(out.contains("read1  (id: aln-1): no differences in window\n"), "{out}");
        assert!(!out.contains("mismatch at 16"), "{out}");

        // Partial window: mismatch + overlapping deletion in, insertion out.
        let out = project_digest(&p, &DigestOptions::default(), Some((14, 18))).unwrap();
        assert!(out.contains("mismatch at 16: T > A"), "{out}");
        assert!(out.contains("deletion at 19: 2 bp (GT)"), "{out}");
        assert!(!out.contains("insertion between 25 and 26"), "{out}");

        // Whole-project digests never render the section.
        let out = project_digest(&p, &DigestOptions::default(), None).unwrap();
        assert!(out.contains("ALIGNMENTS (1-based, inclusive):\n"), "{out}");
        assert!(!out.contains("ALIGNMENT DIFFS IN REGION"), "{out}");
    }

    #[test]
    fn region_view_alignment_diffs_circular_wrap() {
        let mut p = project_with_diff_alignment();
        // aln-2 wraps the origin (56..59 + 0..5): a 4 bp deletion straddling
        // the origin (58,59,0,1 — merged into one entry) and a mismatch at 2.
        p.alignments.push(crate::models::Alignment {
            id: "aln-2".into(),
            name: "wrapped".into(),
            length: 10,
            strand: "+".into(),
            identity: 0.7,
            segments: vec![
                crate::models::AlignSegment {
                    start: 56,
                    end: 59,
                    chars: "AC--".into(),
                },
                crate::models::AlignSegment {
                    start: 0,
                    end: 5,
                    chars: "--ATAC".into(),
                },
            ],
            insertions: Vec::new(),
            seq: String::new(),
        });

        // Wrapping window 55..4 covers both diffs; aln-1 (10..29) stays out.
        let out = project_digest(&p, &DigestOptions::default(), Some((55, 4))).unwrap();
        assert!(out.contains("wrapped  (id: aln-2):\n"), "{out}");
        assert!(out.contains("mismatch at 3: G > A"), "{out}");
        assert!(out.contains("deletion at 59: 4 bp (GTAC)"), "{out}");
        assert!(!out.contains("read1"), "{out}");

        // Wrapping window 56..1: the straddling deletion still overlaps, the
        // mismatch at 2 does not.
        let out = project_digest(&p, &DigestOptions::default(), Some((56, 1))).unwrap();
        assert!(out.contains("deletion at 59: 4 bp (GTAC)"), "{out}");
        assert!(!out.contains("mismatch at 3"), "{out}");
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
    fn cut_notation_is_1based_flanking_bases() {
        // An internal cut index C severs between 0-based bases C-1 and C,
        // i.e. between the 1-based bases C and C+1.
        assert_eq!(cut_notation(10, 60, false), "10^11");
        assert_eq!(cut_notation(10, 60, true), "10^11");
        // A cut at the origin of a circular molecule sits between the last
        // and the first base.
        assert_eq!(cut_notation(0, 60, true), "60^1");
        // A cut at the very end of a linear molecule.
        assert_eq!(cut_notation(60, 60, false), "60^61");
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
        assert!(!out.contains("UNIQUE CUTTERS (cuts shown as N^N+1 = between 1-based bases N and N+1):"));
        assert!(!out.contains("EcoRI"));
        assert!(out.contains("... and 5 enzymes with >1 cut"));
        // compactCutters=false: the full per-enzyme list is back.
        let opts = DigestOptions {
            compact_cutters: false,
            ..DigestOptions::default()
        };
        let out = project_digest(&p, &opts, None).unwrap();
        assert!(out.contains("UNIQUE CUTTERS (cuts shown as N^N+1 = between 1-based bases N and N+1):"));
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

    fn seq_from_gbk(gbk: &str) -> String {
        let mut out = String::new();
        let mut in_seq = false;
        for line in gbk.lines() {
            if line.starts_with("ORIGIN") {
                in_seq = true;
                continue;
            }
            if in_seq {
                if line.starts_with("//") {
                    break;
                }
                out.extend(line.chars().filter(|c| c.is_ascii_alphabetic()));
            }
        }
        out
    }

    #[test]
    fn overview_auto_annotation_marks_existing_features() {
        let gbk = include_str!("../../../examples/pUC19 Annotated.gbk");
        let seq = seq_from_gbk(gbk);
        assert_eq!(seq.len(), 2686);
        let mut p = ProjectData {
            name: "pUC19".into(),
            sequence: seq,
            length: 2686,
            topology: "circular".into(),
            ..Default::default()
        };
        // The project already carries AmpR and the ori at the engine's known
        // coords (AmpR ~1625..2485, rep_origin ~866..1454).
        p.features = vec![
            Feature {
                id: "f-amp".into(),
                name: "AmpR".into(),
                start: 1625,
                end: 2485,
                color: "#60A5FA".into(),
                ftype: "CDS".into(),
                segments: vec![Segment {
                    start: 1625,
                    end: 2485,
                    color: None,
                }],
                strand: "-".into(),
                notes: String::new(),
                translation: String::new(),
                qualifiers: Vec::new(),
            },
            Feature {
                id: "f-ori".into(),
                name: "my ori".into(),
                start: 866,
                end: 1454,
                color: "#F87171".into(),
                ftype: "rep_origin".into(),
                segments: vec![Segment {
                    start: 866,
                    end: 1454,
                    color: None,
                }],
                strand: "+".into(),
                notes: String::new(),
                translation: String::new(),
                qualifiers: Vec::new(),
            },
        ];
        let opts = DigestOptions {
            include_auto_annotation: true,
            ..DigestOptions::default()
        };
        let out = project_digest(&p, &opts, None).unwrap();
        assert!(out.contains("DETECTED COMMON FEATURES (auto):\n"));
        let amp_line = out
            .lines()
            .find(|l| l.contains("AmpR | CDS"))
            .expect("AmpR auto line");
        assert!(amp_line.contains("(already annotated)"), "line: {amp_line}");
        let ori_line = out
            .lines()
            .find(|l| l.contains(" | rep_origin | "))
            .expect("rep_origin auto line");
        assert!(ori_line.contains("(already annotated)"), "line: {ori_line}");
        // Fragments are omitted entirely; remaining unannotated hits
        // (MCS, lac promoter, CAP binding site, ...) stay unmarked.
        assert!(!out.contains("(fragment)"), "fragments leaked:\n{out}");
        assert!(
            out.lines()
                .any(|l| l.contains("| promoter |") && !l.contains("(already annotated)")),
            "expected an unmarked promoter line:\n{out}"
        );
        // Region views never append the section.
        let region = project_digest(&p, &opts, Some((0, 100))).unwrap();
        assert!(!region.contains("DETECTED COMMON FEATURES"));
    }

    #[test]
    fn overview_auto_annotation_empty_prints_none() {
        // "ACGT"*15 hits nothing in the embedded SnapGene database.
        let p = synthetic_project();
        let opts = DigestOptions {
            include_auto_annotation: true,
            ..DigestOptions::default()
        };
        let out = project_digest(&p, &opts, None).unwrap();
        assert!(out.contains("DETECTED COMMON FEATURES (auto):\n(none)\n"));
        // Off by default: no section unless explicitly requested.
        let out = project_digest(&p, &DigestOptions::default(), None).unwrap();
        assert!(!out.contains("DETECTED COMMON FEATURES"));
    }
}
