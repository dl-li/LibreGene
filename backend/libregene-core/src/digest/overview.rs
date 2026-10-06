use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::annotate_section::push_auto_annotation;
use super::lines::{
    alignment_in_region, alignment_line, classify_enzymes, cut_type_label, cuts_desc,
    deletion_in_region, feature_line, feature_matches_filter, molecule_label, primer_site_line,
    push_alignment_view, translation_diff_pos, unit_for,
};
use super::DigestOptions;
use super::range::{
    cut_flanks, enzyme_in_region, feature_in_region, pos_in_range, seg_in_range, validate_range,
};
use crate::models::{Enzyme, Feature, ProjectData};

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
    let (seq_hash, rev_comp_hash) =
        crate::utils::orientation_hashes(&project.sequence, &project.molecule_type);
    match rev_comp_hash {
        Some(rh) => {
            let _ = writeln!(out, "SEQHASH: {} (rev-comp {})", seq_hash, rh);
        }
        None => {
            let _ = writeln!(out, "SEQHASH: {}", seq_hash);
        }
    }
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
            region.is_none_or(|(s, e)| feature_in_region(f, s, e, circular))
        })
        .collect();
    out.push_str("FEATURES (1-based, inclusive):\n");
    match opts.max_features {
        Some(max) if features.len() > max => {
            for f in features.iter().take(max) {
                out.push_str(&feature_line(f));
                out.push('\n');
            }
            let _ = writeln!(out,
                "        ... and {} more features (narrow with feature_filter)",
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

    // DNA overview only: cross-check stored /translation qualifiers of CDS/mRNA
    // features against the current sequence (open_file keeps the file's value,
    // so a stale qualifier after sequence edits is detectable). A mismatch
    // reports the first disagreeing amino-acid position.
    if is_dna && region.is_none() {
        for f in &features {
            if (f.ftype == "CDS" || f.ftype == "mRNA") && !f.translation.is_empty() {
                let derived = crate::translate::translate_feature(&project.sequence, f);
                if let Some((pos, stored, derived_aa)) =
                    translation_diff_pos(&f.translation, &derived)
                {
                    let _ = writeln!(
                        out,
                        "WARNING: /translation of {} '{}' (id: {}) disagrees with the DNA sequence at aa {} (stored {}, derived {})",
                        f.ftype, f.name, f.id, pos, stored, derived_aa
                    );
                }
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
            if region.is_none_or(|(rs, re)| {
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
            let _ = writeln!(out,
                "Primers without binding sites: {}",
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
            region.is_none_or(|(s, e)| alignment_in_region(a, s, e, circular))
        })
        .collect();
    if !alignments.is_empty() {
        out.push_str("ALIGNMENTS (1-based, inclusive):\n");
        for a in &alignments {
            out.push_str(&alignment_line(a));
            out.push('\n');
        }
        // Overview only: template positions where ≥2 reads share the same
        // mismatch — a hint that the template may be outdated.
        if region.is_none() {
            let mut counts: BTreeMap<(i64, String, String), usize> = BTreeMap::new();
            for a in &alignments {
                let diff = crate::align::alignment_diff(a, &project.sequence);
                for m in &diff.mismatches {
                    *counts
                        .entry((
                            m.pos as i64,
                            m.template_base.to_ascii_uppercase(),
                            m.read_base.to_ascii_uppercase(),
                        ))
                        .or_insert(0) += 1;
                }
            }
            let consensus: Vec<((i64, String, String), usize)> = counts
                .into_iter()
                .filter(|(_, n)| *n >= 2)
                .collect();
            if !consensus.is_empty() {
                let parts: Vec<String> = consensus
                    .iter()
                    .map(|((pos, tb, rb), n)| format!("{} {}>{} ({} reads)", pos + 1, tb, rb, n))
                    .collect();
                let _ = writeln!(
                    out,
                    "SHARED MISMATCHES (positions where ≥2 stored reads carry the same mismatch; may be biological, clonal or template differences): {}",
                    parts.join(", ")
                );
            }
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
                let _ = writeln!(section,
                    "          mismatch at {}: {} > {}",
                    m.pos + 1,
                    m.template_base,
                    m.read_base
                );
            }
            for d in deletions {
                let _ = writeln!(section,
                    "          deletion at {}: {} bp ({})",
                    d.pos + 1,
                    d.length,
                    d.bases
                );
            }
            for i in insertions {
                let (a1, b1) = cut_flanks(i.pos as i64, project.length, circular);
                let _ = writeln!(section,
                    "          insertion between {} and {}: {} ({} bp)",
                    a1, b1, i.bases, i.length
                );
            }
        }
        out.push_str(&section);
    }

    // Region views only, and only on request: per-read column view (template /
    // mask / read rows), so an agent can read the actual read bases in a window
    // without unwinding circular wraps and gap offsets from orientedSequence.
    // The structured ALIGNMENT DIFFS lines above carry the coordinates either way.
    if let Some((s, e)) = region.filter(|_| opts.include_alignment_view) {
        let mut view = String::new();
        for a in &alignments {
            push_alignment_view(&mut view, a, &project.sequence, s, e, circular);
        }
        if !view.is_empty() {
            view.insert_str(
                0,
                "ALIGNMENT VIEW IN REGION (per-read column view; rows: template / match mask / read; mask: | match, . mismatch, - read gap; insertions and uncovered template listed below; 1-based inclusive):\n",
            );
            out.push_str(&view);
        }
    }

    // Enzymes (DNA only — single-strand molecules have no restriction sites)
    if is_dna {
        match region {
            None => {
                let (single, double, multi) = classify_enzymes(project);
                let mut double_names: Vec<&str> = double.iter().map(|e| e.name.as_str()).collect();
                double_names.sort_unstable();
                double_names.dedup();
                if opts.compact_enzymes {
                    if !single.is_empty() || !double_names.is_empty() || multi > 0 {
                        let _ = writeln!(out,
                            "ENZYMES (compact): {} single-cut, {} double-cut, {} multi-site (cuts shown as N^N+1, 1-based)",
                            single.len(),
                            double_names.len(),
                            multi
                        );
                    }
                } else {
                    if !single.is_empty() {
                        out.push_str("SINGLE CUTTERS (top-strand cut N; enzymes sharing N are grouped):\n");
                        let mut groups: BTreeMap<i64, Vec<&str>> = BTreeMap::new();
                        for e in single {
                            let (n, _) = cut_flanks(e.cut_index, project.length, circular);
                            groups.entry(n).or_default().push(e.name.as_str());
                        }
                        for names in groups.values_mut() {
                            names.sort_unstable();
                        }
                        for (n, names) in groups {
                            let _ = writeln!(out, "        {:<30} {}", names.join(", "), n);
                        }
                    }
                    if !double_names.is_empty() {
                        let _ = writeln!(out, "DOUBLE CUTTERS: {}", double_names.join(", "));
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
                        let _ = writeln!(out,
                            "ENZYMES CUTTING IN REGION (compact): {} cuts (cuts shown as N^N+1, 1-based)",
                            in_region.len()
                        );
                    } else {
                        out.push_str("ENZYMES CUTTING IN REGION (cuts shown as N^N+1 = between 1-based bases N and N+1):\n");
                        for en in in_region {
                            let _ = writeln!(out,
                                "        {:<10} {}   {}",
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
