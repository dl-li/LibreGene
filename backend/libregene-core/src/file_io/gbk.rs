//! GenBank (.gbk) parser and writer using the `gb-io` crate.
//!
//! # Coordinate conventions
//!
//! - **gb-io `Location::Range`**: 0-based, end-exclusive.
//! - **`ProjectData` models**: 0-based, inclusive start **and** end.
//!
//! Conversion:
//!   model_start = range.start.0
//!   model_end   = range.end.0   - 1

use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;

use chrono::Datelike;

use gb_io::reader::SeqReader;
use gb_io::seq::{After, Before, Feature as GbFeature, Location, Seq, Topology};
use gb_io::writer::SeqWriter;

use crate::file_io::color::{adjust_color_readability, default_color, normalize_color};
use crate::models::{Feature, Primer, ProjectData, Segment};

// ---------------------------------------------------------------------------
// Methylation annotation in KEYWORDS ("methylation: Dam,Dcm,EcoKI" / "none")
// ---------------------------------------------------------------------------

const METHYLATION_KW: &str = "methylation:";

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Conventional display form of a methylation system name.
fn display_system(s: &str) -> String {
    match s.to_lowercase().as_str() {
        "ecoki" => "EcoKI".to_string(),
        other => capitalize(other),
    }
}

/// Split KEYWORDS into (remaining keywords, methylation systems). The
/// annotation is a `;`-separated `methylation: A,B,C` segment; a `none`/empty
/// value means explicitly no methylation. Returns None when absent.
fn parse_methylation_keyword(keywords: &str) -> (String, Option<Vec<String>>) {
    let mut systems = None;
    let mut kept: Vec<&str> = Vec::new();
    for part in keywords.split(';') {
        let p = part.trim().trim_end_matches('.').trim();
        if p.is_empty() {
            continue;
        }
        if p.len() >= METHYLATION_KW.len() && p[..METHYLATION_KW.len()].eq_ignore_ascii_case(METHYLATION_KW) {
            let value = p[METHYLATION_KW.len()..].trim();
            if value.is_empty() || value.eq_ignore_ascii_case("none") {
                systems = Some(Vec::new());
            } else {
                systems = Some(
                    value
                        .split(',')
                        .map(|s| s.trim().to_lowercase())
                        .filter(|s| !s.is_empty())
                        .collect(),
                );
            }
        } else {
            kept.push(p);
        }
    }
    (kept.join("; "), systems)
}

/// KEYWORDS with the methylation annotation merged in (any previous
/// annotation replaced). Empty systems are written as `methylation: none`.
fn keywords_with_methylation(keywords: &str, systems: &[String]) -> String {
    let (base, _) = parse_methylation_keyword(keywords);
    let value = if systems.is_empty() {
        "none".to_string()
    } else {
        systems.iter().map(|s| display_system(s)).collect::<Vec<_>>().join(",")
    };
    let token = format!("methylation: {}", value);
    if base.is_empty() {
        token
    } else {
        format!("{}; {}", base, token)
    }
}

// ---------------------------------------------------------------------------
// Shared helper: build a Primer from individual qualifier values
// ---------------------------------------------------------------------------

/// Create a [`Primer`] from individual qualifier values.
///
/// Shared with `crate::primer::gbk::parse_gbk_feature` to avoid duplicating
/// the Primer construction logic.
pub(crate) fn primer_from_qualifier_values(
    label: &str,
    primer_id: &str,
    ptype: &str,
    primer_seq: &str,
) -> Primer {
    Primer {
        id: primer_id.to_string(),
        name: label.to_string(),
        r#type: ptype.to_string(),
        primer_seq: primer_seq.to_string(),
        binding_sites: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Parse
// ---------------------------------------------------------------------------

/// Parse a GenBank file and return a [`ProjectData`].
pub fn parse_gbk(path: &Path) -> io::Result<ProjectData> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut seq_reader = SeqReader::new(reader);

    // Take the first sequence record.
    let seq = seq_reader
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty GenBank file"))?
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

    let sequence: String = std::str::from_utf8(&seq.seq)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("sequence is not valid UTF-8: {e}")))?
        .to_string();

    let name = seq.name.clone().unwrap_or_default();
    let definition = seq.definition.clone().unwrap_or_default();
    let keywords = seq.keywords.clone().unwrap_or_default();
    let strip_dot = |s: String| if s == "." { String::new() } else { s };
    let definition = strip_dot(definition);
    let keywords = strip_dot(keywords);

    let topology = match seq.topology {
        Topology::Circular => "circular".to_string(),
        Topology::Linear => "linear".to_string(),
    };

    // Molecule type from the LOCUS molecule-type field (e.g. "ss-RNA").
    let molecule_type = if seq
        .molecule_type
        .as_deref()
        .map(|mt| {
            let mt = mt.to_ascii_uppercase();
            mt.contains("RNA") || mt.contains("RRNA")
        })
        .unwrap_or(false)
    {
        "rna".to_string()
    } else {
        "dna".to_string()
    };

    let mut features: Vec<Feature> = Vec::new();
    let mut primers: Vec<Primer> = Vec::new();
    let mut alignment_reads: Vec<(String, String)> = Vec::new();
    let mut lab_host = String::new();

    for f in &seq.features {
        let kind = f.kind.as_ref();

        if kind == "primer_bind" {
            if let Some(p) = parse_snapgene_primer(f, &sequence) {
                primers.push(p);
            }
            continue;
        }

        // Alignments are persisted as misc_features carrying libregene_align_seq;
        // they are NOT features and are re-aligned below.
        if kind == "misc_feature" {
            if let Some(aseq) = f.qualifier_values("libregene_align_seq").next() {
                let name = f
                    .qualifier_values("label")
                    .next()
                    .unwrap_or("alignment")
                    .to_string();
                // GenBank writer line-wraps qualifier values; rejoin.
                let cleaned: String = aseq.chars().filter(|c| c.is_ascii_alphabetic()).collect();
                alignment_reads.push((name, cleaned));
                continue;
            }
        }

        if kind == "source" {
            if let Some(host) = f.qualifier_values("lab_host").next() {
                lab_host = host.to_string();
            }
            continue;
        }

        // Resolve label
        let label = f
            .qualifier_values("label")
            .next()
            .or_else(|| f.qualifier_values("gene").next())
            .unwrap_or(kind)
            .to_string();

        // Collect all notes (SnapGene stores color/direction in notes).
        // gb-io keeps raw newlines from line-wrapped qualifier values; collapse
        // them to a single space for ordinary notes (segments notes are parsed
        // separately and keep their structure).
        let note_values: Vec<String> = f
            .qualifier_values("note")
            .map(|n| {
                if n.contains("segments:") {
                    n.to_string()
                } else {
                    n.split_whitespace().collect::<Vec<_>>().join(" ")
                }
            })
            .collect();
        let note_values: Vec<&str> = note_values.iter().map(|s| s.as_str()).collect();

        // Resolve colour: try ApE/libregene qualifiers first, then SnapGene note format
        let mut color = normalize_color(
            f.qualifier_values("ApEinfo_fwdcolor")
                .next()
                .or_else(|| f.qualifier_values("libregene_color").next())
                .unwrap_or(""),
        );
        if color.is_empty() {
            // Try SnapGene-style: /note="color: #XXXXXX" or /note="color: #XXXXXX; direction: RIGHT"
            color = parse_snapgene_color_from_notes(&note_values);
        }
        if color.is_empty() {
            color = default_color(kind).to_string();
        }
        let color = adjust_color_readability(&color);

        // Resolve strand: Complement location, or SnapGene /direction=LEFT/RIGHT, or note direction
        let mut strand = strand_from_gb_location(&f.location);
        if strand == "." {
            // Check /direction qualifier
            let dir_qual = f.qualifier_values("direction").next().unwrap_or("");
            if dir_qual == "LEFT" {
                strand = "-".to_string();
            } else if dir_qual == "RIGHT" {
                strand = "+".to_string();
            }
        }
        if strand == "." {
            // Check SnapGene note: "direction: RIGHT" or "direction: LEFT"
            strand = parse_snapgene_direction_from_notes(&note_values);
        }

        // Extract coordinates; a SnapGene "This feature has N segments" note
        // takes precedence over the location for segment info.
        let (loc_segments, loc_start, loc_end) = extract_location_bounds(&f.location);
        let seg_note = note_values
            .iter()
            .find_map(|n| parse_segments_note(n));
        let (segments, start, end) = match &seg_note {
            Some((segs, _)) if !segs.is_empty() => {
                let s = segs.iter().map(|g| g.start).min().unwrap_or(loc_start);
                let e = segs.iter().map(|g| g.end).max().unwrap_or(loc_end);
                (segs.clone(), s, e)
            }
            _ => (loc_segments, loc_start, loc_end),
        };

        let translation = f
            .qualifier_values("translation")
            .next()
            .unwrap_or("")
            .to_string();

        // Build notes string, filtering out SnapGene color/direction notes and
        // the segments note (kept only in `segments`); leftover text from the
        // segments note (e.g. "Cleavage site after base 2417") is preserved.
        let mut note_parts: Vec<&str> = note_values
            .iter()
            .filter(|n| {
                !is_snapgene_color_note(n)
                    && !is_snapgene_direction_note(n)
                    && parse_segments_note(n).is_none()
            })
            .copied()
            .collect();
        let seg_note_rest;
        if let Some((_, rest)) = &seg_note {
            seg_note_rest = rest.clone();
            if !seg_note_rest.is_empty() {
                note_parts.push(&seg_note_rest);
            }
        }
        let notes = note_parts.join("; ");

        // Collect remaining qualifiers (filter out ones we store separately or add synthetically)
        let skip_keys: std::collections::HashSet<&str> = [
            "label", "translation", "ApEinfo_fwdcolor", "ApEinfo_revcolor",
            "libregene_color", "direction", "libregene_primer_id", "libregene_primer_seq",
            "libregene_primer_type",
        ].into_iter().collect();
        // Keep "note" in qualifiers — the notes field is a ";"-joined copy that can't
        // round-trip note values whose content contains "; ".  The dialog uses qualifiers
        // when available and falls back to the notes field for backward compat.
        let qualifiers: Vec<(String, String)> = f
            .qualifiers
            .iter()
            .filter(|(k, v)| {
                !skip_keys.contains(k.as_ref()) && v.is_some()
            })
            .map(|(k, v)| (k.to_string(), v.as_deref().unwrap_or("").to_string()))
            .collect();

        features.push(Feature {
            id: format!("{}_{}", label, start),
            name: label,
            start,
            end,
            color,
            ftype: kind.to_string(),
            segments,
            strand,
            notes,
            translation,
            qualifiers,
        });
    }

    let length = sequence.len() as i64;

    // Methylation is annotated in KEYWORDS ("methylation: Dam,Dcm,EcoKI" or
    // "methylation: none"); a circular DNA plasmid without any annotation
    // defaults to all three known systems.
    let (keywords, methyl) = parse_methylation_keyword(&keywords);
    let methylation_systems = match methyl {
        Some(s) => s,
        None if topology == "circular" && molecule_type == "dna" => {
            crate::enzyme::methylation::ALL_SYSTEMS
                .iter()
                .map(|s| s.to_string())
                .collect()
        }
        None => Vec::new(),
    };

    // Re-run the alignment so segments/identity match the current sequence.
    let mut alignments = Vec::new();
    for (name, read) in &alignment_reads {
        if let Some(mut aln) =
            crate::align::align_read(&sequence, read, topology == "circular")
        {
            aln.name = name.clone();
            aln.id = crate::align::next_alignment_id(&alignments);
            alignments.push(aln);
        }
    }

    Ok(ProjectData {
        name,
        definition,
        keywords,
        lab_host,
        sequence,
        length,
        topology,
        molecule_type,
        features,
        primers,
        alignments,
        methylation_systems,
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Write
// ---------------------------------------------------------------------------

/// Write a [`ProjectData`] as a SnapGene-flavoured GenBank file.
pub fn write_gbk(project: &ProjectData, path: &Path) -> io::Result<()> {
    let mut record = Seq::empty();

    let name = if project.name.is_empty() {
        path.file_stem()
            .map(|s| s.to_string_lossy().replace(' ', "_"))
            .unwrap_or_else(|| "libregene".to_string())
    } else {
        project.name.replace(' ', "_")
    };
    record.name = Some(name);
    record.topology = if project.topology == "circular" {
        Topology::Circular
    } else {
        Topology::Linear
    };
    record.molecule_type = Some(match project.molecule_type.as_str() {
        "rna" => "ss-RNA".to_string(),
        _ => "DNA".to_string(),
    });
    record.division = "SYN".to_string();
    record.seq = project.sequence.as_bytes().to_vec();
    record.len = Some(project.sequence.len());
    let now = chrono::Local::now();
    record.date = Some(gb_io::seq::Date::from_ymd(
        now.year(),
        now.month(),
        now.day(),
    ).unwrap());
    record.definition = Some(if project.definition.is_empty() {
        ".".to_string()
    } else {
        project.definition.clone()
    });
    record.accession = Some(".".to_string());
    record.version = Some(".".to_string());
    let keywords = if project.topology == "circular" && project.molecule_type == "dna" {
        // Persist the plasmid's methylation systems in KEYWORDS so they
        // survive a save/load round-trip.
        keywords_with_methylation(&project.keywords, &project.methylation_systems)
    } else {
        project.keywords.clone()
    };
    record.keywords = Some(if keywords.is_empty() {
        ".".to_string()
    } else {
        keywords
    });
    record.source = Some(gb_io::seq::Source {
        source: "synthetic DNA construct".to_string(),
        organism: Some("synthetic DNA construct".to_string()),
    });

    // Add source feature — SnapGene style with /lab_host
    let source_loc = Location::Range(
        (0, Before(false)),
        (project.sequence.len() as i64, After(false)),
    );
    record.features.push(GbFeature {
        kind: Cow::Borrowed("source"),
        location: source_loc,
        qualifiers: vec![
            (
                Cow::Borrowed("lab_host"),
                Some(if project.lab_host.is_empty() {
                    "Escherichia coli".to_string()
                } else {
                    project.lab_host.clone()
                }),
            ),
            (
                Cow::Borrowed("mol_type"),
                Some(match project.molecule_type.as_str() {
                    "rna" => "other RNA".to_string(),
                    _ => "other DNA".to_string(),
                }),
            ),
            (
                Cow::Borrowed("organism"),
                Some("synthetic DNA construct".to_string()),
            ),
        ],
    });

    // Add features — SnapGene-style with color and direction in /note
    for f in &project.features {
        let loc = model_range_to_gb_location(f);

        let mut qualifiers: Vec<(Cow<'static, str>, Option<String>)> = Vec::new();

        // Original qualifiers first (gene, product, codon_start, bound_moiety…)
        let skip_keys: std::collections::HashSet<&str> = [
            "label", "note", "translation", "direction",
        ].into_iter().collect();
        for (k, v) in &f.qualifiers {
            if skip_keys.contains(k.as_str())
                || k.starts_with("ApEinfo_")
                || k.starts_with("libregene_")
            {
                continue;
            }
            qualifiers.push((Cow::Owned(k.clone()), Some(v.clone())));
        }

        qualifiers.push((Cow::Borrowed("label"), Some(f.name.clone())));

        // Descriptive notes
        if !f.notes.is_empty() {
            for note_line in f.notes.split("; ") {
                if !note_line.is_empty() {
                    qualifiers.push((Cow::Borrowed("note"), Some(note_line.to_string())));
                }
            }
        }

        // Segments note (SnapGene multi-segment convention)
        if f.segments.len() > 1 {
            let mut seg_note = format!("This feature has {} segments:", f.segments.len());
            for (i, seg) in f.segments.iter().enumerate() {
                seg_note.push_str(&format!("\n  {}: {} .. {}", i + 1, seg.start + 1, seg.end + 1));
                if let Some(c) = &seg.color {
                    seg_note.push_str(&format!(" / {}", c));
                }
            }
            qualifiers.push((Cow::Borrowed("note"), Some(seg_note)));
        }

        // Color note; reverse strand merges "direction: LEFT" (SnapGene style)
        let color_note = if f.strand == "-" {
            format!("color: {}; direction: LEFT", f.color)
        } else {
            format!("color: {}", f.color)
        };
        qualifiers.push((Cow::Borrowed("note"), Some(color_note)));

        // Translation for CDS
        if !f.translation.is_empty() {
            qualifiers.push((
                Cow::Borrowed("translation"),
                Some(f.translation.clone()),
            ));
        }

        record.features.push(GbFeature {
            kind: Cow::Owned(if f.ftype.is_empty() {
                "misc_feature".to_string()
            } else {
                f.ftype.clone()
            }),
            location: loc,
            qualifiers,
        });
    }

    // Serialize primers — SnapGene-style with color + sequence in /note
    serialize_primers_snapgene(project, &mut record);

    // Serialize alignments as misc_features with libregene_align_* qualifiers
    serialize_alignments_gbk(project, &mut record);

    let file = File::create(path)?;
    let mut writer = SeqWriter::new(file);
    writer.write(&record)
}

/// Serialize alignments as `misc_feature`s spanning the first segment
/// (full length when there are no segments). The read sequence and strand
/// travel in custom qualifiers; position is recomputed on parse.
fn serialize_alignments_gbk(project: &ProjectData, record: &mut Seq) {
    for a in &project.alignments {
        let (start, end) = match a.segments.first() {
            Some(seg) => (seg.start as i64, seg.end as i64),
            None => (0, project.sequence.len().saturating_sub(1) as i64),
        };
        let loc = Location::Range((start, Before(false)), (end + 1, After(false)));

        record.features.push(GbFeature {
            kind: Cow::Borrowed("misc_feature"),
            location: loc,
            qualifiers: vec![
                (Cow::Borrowed("label"), Some(a.name.clone())),
                (Cow::Borrowed("libregene_align_strand"), Some(a.strand.clone())),
                (Cow::Borrowed("libregene_align_seq"), Some(a.seq.clone())),
            ],
        });
    }
}

/// Serialize primers in SnapGene format.
fn serialize_primers_snapgene(project: &ProjectData, record: &mut Seq) {
    for p in &project.primers {
        let best = match p.binding_sites.first() {
            Some(bs) => bs,
            // Skip primers without binding sites — no valid template position to serialize.
            None => continue,
        };
        let ms = best.template_start;
        let me = best.template_end;

        // SnapGene primer sequence: 5' tail lowercase + binding region uppercase
        let seq = if best.five_prime_tail.is_empty() {
            p.primer_seq.to_uppercase()
        } else {
            let tail_len = best.five_prime_tail.len().min(p.primer_seq.len());
            format!(
                "{}{}",
                p.primer_seq[..tail_len].to_lowercase(),
                p.primer_seq[tail_len..].to_uppercase()
            )
        };
        let direction = if p.r#type == "rev" { "LEFT" } else { "RIGHT" };
        let note = format!("direction: {}; sequence: {}", direction, seq);

        let loc = if me <= ms {
            // Binding site wraps the origin of a circular template.
            let tlen = project.sequence.len() as i64;
            Location::Join(vec![
                Location::Range((ms, Before(false)), (tlen, After(false))),
                Location::Range((0, Before(false)), (me, After(false))),
            ])
        } else {
            Location::Range((ms, Before(false)), (me, After(false)))
        };
        let loc = if p.r#type == "rev" {
            Location::Complement(Box::new(loc))
        } else {
            loc
        };

        record.features.push(GbFeature {
            kind: Cow::Borrowed("primer_bind"),
            location: loc,
            qualifiers: vec![
                (Cow::Borrowed("label"), Some(p.name.clone())),
                (Cow::Borrowed("note"), Some(note)),
            ],
        });
    }
}

// ---------------------------------------------------------------------------
// Coordinate helpers
// ---------------------------------------------------------------------------

/// Extract (segments, overall_start, overall_end) from a [`Location`].
///
/// All returned coordinates are 0-based inclusive.
fn extract_location_bounds(loc: &Location) -> (Vec<Segment>, i64, i64) {
    match loc {
        Location::Join(parts) => {
            let mut segs = Vec::with_capacity(parts.len());
            let mut start = i64::MAX;
            let mut end = i64::MIN;
            for p in parts {
                let range = match p {
                    Location::Range((s, _), (e, _)) => Some((*s, *e)),
                    Location::Complement(inner) => {
                        if let Location::Range((s, _), (e, _)) = inner.as_ref() {
                            // Complement's coordinates are the same — only the
                            // strand is affected, which is handled by strand_from_gb_location.
                            Some((*s, *e))
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some((seg_start, seg_end_raw)) = range {
                    let seg_end = seg_end_raw - 1; // 0-based inclusive
                    segs.push(Segment {
                        start: seg_start,
                        end: seg_end,
                        color: None,
                    });
                    start = start.min(seg_start);
                    end = end.max(seg_end);
                }
            }
            (segs, start, end)
        }
        Location::Complement(inner) => {
            let (segs, s, e) = extract_location_bounds(inner);
            (segs, s, e)
        }
        Location::Range((s, _), (e, _)) => {
            let seg_end = e - 1;
            let seg = Segment { start: *s, end: seg_end, color: None };
            (vec![seg], *s, seg_end)
        }
        _ => (vec![], 0, 0),
    }
}

/// Determine strand from a GenBank location.
fn strand_from_gb_location(loc: &Location) -> String {
    match loc {
        Location::Complement(_) => "-".to_string(),
        _ => ".".to_string(),
    }
}

/// Convert a model feature location to a `gb-io` [`Location`].
///
/// Multi-segment features are written as the overall range (SnapGene style);
/// the segment breakdown travels in a "This feature has N segments" note.
fn model_range_to_gb_location(f: &Feature) -> Location {
    // Model coordinates: 0-based inclusive start/end
    // gb-io Range: 0-based, end-exclusive

    let loc = Location::Range(
        (f.start, Before(false)),
        (f.end + 1, After(false)),
    );
    if f.strand == "-" {
        Location::Complement(Box::new(loc))
    } else {
        loc
    }
}

/// Parse a GenBank location string (e.g. "complement(1..100)", "join(1..50,60..100)")
/// and return (segments, overall_start, overall_end, strand).
/// GenBank locations are 1-based inclusive; the returned coordinates are
/// shifted to the model's 0-based inclusive convention.
/// This is the FILE-FORMAT parser — use it only where GenBank spec semantics
/// are required (.gbk/.gpt file parsing). App-internal APIs use
/// [`parse_location_string_0based`].
/// Returns None on parse failure.
pub fn parse_location_string(s: &str) -> Option<(Vec<Segment>, i64, i64, String)> {
    parse_location_string_impl(s, true)
}

/// Parse a 0-based inclusive location string (e.g. "99..199",
/// "complement(49..79)", "join(0..99,199..299)") — the App-internal coordinate
/// convention shared by the model, digests and all MCP/Tauri APIs. Same
/// grammar as [`parse_location_string`] but without the GenBank 1-based shift.
/// Returns None on parse failure.
pub fn parse_location_string_0based(s: &str) -> Option<(Vec<Segment>, i64, i64, String)> {
    parse_location_string_impl(s, false)
}

fn parse_location_string_impl(s: &str, one_based: bool) -> Option<(Vec<Segment>, i64, i64, String)> {
    let s = s.trim();
    let (inner, strand) = if let Some(rest) = s.strip_prefix("complement(") {
        rest.strip_suffix(')').map(|r| (r, "-"))
    } else if let Some(rest) = s.strip_prefix("Complement(") {
        rest.strip_suffix(')').map(|r| (r, "-"))
    } else {
        Some((s, "+"))
    }?;

    let inner = inner.trim();

    let parts: Vec<&str> = if let Some(rest) = inner.strip_prefix("join(") {
        rest.strip_suffix(')')?.split(',').map(|p| p.trim()).collect()
    } else if let Some(rest) = inner.strip_prefix("order(") {
        rest.strip_suffix(')')?.split(',').map(|p| p.trim()).collect()
    } else {
        vec![inner]
    };

    if parts.is_empty() {
        return None;
    }

    let shift = if one_based { 1 } else { 0 };
    let mut segments = Vec::new();
    for part in &parts {
        if let Some(dotdot) = part.find("..") {
            let start_str = part[..dotdot].trim();
            let end_str = part[dotdot + 2..].trim();
            let start: i64 = start_str.parse().ok()?;
            let end: i64 = end_str.parse().ok()?;
            if start < shift || end < shift || start > end {
                return None;
            }
            segments.push(Segment {
                start: start - shift,
                end: end - shift,
                color: None,
            });
        } else {
            // Single position
            let pos: i64 = part.trim().parse().ok()?;
            if pos < shift {
                return None;
            }
            segments.push(Segment {
                start: pos - shift,
                end: pos - shift,
                color: None,
            });
        }
    }

    let overall_start = segments.iter().map(|s| s.start).min().unwrap_or(0);
    let overall_end = segments.iter().map(|s| s.end).max().unwrap_or(0);

    Some((segments, overall_start, overall_end, strand.to_string()))
}

// ---------------------------------------------------------------------------
//  Fallback primer parser / serializer
// ---------------------------------------------------------------------------

/// Build the core qualifier pairs for a primer (libregene format).
///
/// Shared between [`serialize_primers_fallback`] and
/// `crate::primer::gbk::serialize_primers_gbk` to avoid duplicating
/// the qualifier-building logic.
///
/// Returns `(match_start, match_end, qualifier_pairs)`.
pub(crate) fn build_primer_qualifier_pairs(
    p: &Primer,
) -> (i64, i64, Vec<(String, String)>) {
    let best = p.binding_sites.first();
    let match_start = best.map(|b| b.template_start).unwrap_or(0);
    let match_end = best.map(|b| b.template_end).unwrap_or(0);

    let mut qualifiers: Vec<(String, String)> = Vec::new();
    qualifiers.push(("label".to_string(), p.name.clone()));
    qualifiers.push(("libregene_primer_id".to_string(), p.id.clone()));
    qualifiers.push(("libregene_primer_type".to_string(), p.r#type.clone()));
    qualifiers.push(("libregene_primer_seq".to_string(), p.primer_seq.clone()));

    if !p.binding_sites.is_empty() {
        let parts: Vec<String> = p
            .binding_sites
            .iter()
            .map(|bs| format!("{},{},{:.1}", bs.template_start, bs.template_end, bs.tm))
            .collect();
        qualifiers.push(("libregene_bindings".to_string(), parts.join(";")));
    }

    (match_start, match_end, qualifiers)
}



/// Fallback `.gbk` primer serializer.
#[allow(dead_code)]
fn serialize_primers_fallback(project: &ProjectData, record: &mut Seq) {
    for p in &project.primers {
        let (ms, me, qualifier_pairs) = build_primer_qualifier_pairs(p);
        let qualifiers: Vec<(Cow<'static, str>, Option<String>)> = qualifier_pairs
            .into_iter()
            .map(|(k, v)| (Cow::Owned(k), Some(v)))
            .collect();

        let loc = Location::Range((ms, Before(false)), (me + 1, After(false)));
        let loc = if p.r#type == "rev" {
            Location::Complement(Box::new(loc))
        } else {
            loc
        };

        record.features.push(GbFeature {
            kind: Cow::Borrowed("primer_bind"),
            location: loc,
            qualifiers,
        });
    }
}

// ---------------------------------------------------------------------------
// SnapGene note format helpers
// ---------------------------------------------------------------------------

/// Check if a note string looks like a SnapGene color note: `color: #XXXXXX`
fn is_snapgene_color_note(note: &str) -> bool {
    note.starts_with("color:") || note.starts_with("color：")
}

/// Check if a note string looks like a SnapGene direction note.
fn is_snapgene_direction_note(note: &str) -> bool {
    note.contains("direction:") || note.contains("direction：")
}

/// Parse color from SnapGene note format: `color: #XXXXXX` or `color: 黑色`
fn parse_snapgene_color_from_notes(notes: &[&str]) -> String {
    for note in notes {
        if let Some(rest) = note.strip_prefix("color:").or_else(|| note.strip_prefix("color：")) {
            let color_part = rest.split(';').next().unwrap_or("").trim();
            // Handle Chinese color names
            if let Some(hex) = chinese_color_to_hex(color_part) {
                return hex.to_string();
            }
            // Handle hex colors
            let normalized = normalize_color(color_part);
            if !normalized.is_empty() {
                return normalized;
            }
        }
    }
    String::new()
}

/// Parse direction from SnapGene note: `direction: RIGHT` or `direction: LEFT`
fn parse_snapgene_direction_from_notes(notes: &[&str]) -> String {
    for note in notes {
        if note.contains("direction: RIGHT") || note.contains("direction： RIGHT") {
            return "+".to_string();
        }
        if note.contains("direction: LEFT") || note.contains("direction： LEFT") {
            return "-".to_string();
        }
    }
    ".".to_string()
}

/// Parse primer info from SnapGene note: `color: COLOR; sequence: ACTG...; added: DATE`
fn parse_snapgene_primer_note(note: &str) -> Option<(String, String)> {
    // Expected format: "color: COLOR; sequence: SEQUENCE; added: DATE"
    let note = note.trim_matches('"').trim();
    let mut color = String::new();
    let mut sequence = String::new();

    for part in note.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("color:").or_else(|| part.strip_prefix("color：")) {
            let c = val.trim();
            if let Some(hex) = chinese_color_to_hex(c) {
                color = hex.to_string();
            } else {
                let normalized = normalize_color(c);
                if !normalized.is_empty() {
                    color = normalized;
                }
            }
        } else if let Some(val) = part
            .strip_prefix("sequence:")
            .or_else(|| part.strip_prefix("sequence："))
        {
            sequence = val.trim().to_string();
        }
    }

    if sequence.is_empty() {
        None
    } else {
        Some((color, sequence))
    }
}

/// Parse a SnapGene multi-segment note:
/// `This feature has N segments:\n  1: start .. end / #color / name\n  ...`
///
/// Coordinates in the note are 1-based; returned segments are 0-based inclusive.
/// Returns the segments plus any leftover descriptive text after the segment
/// list (e.g. "Cleavage site after base 2417"). Returns None for ordinary notes.
fn parse_segments_note(note: &str) -> Option<(Vec<Segment>, String)> {
    let idx = note.find("segments:")?;
    if !note[..idx].contains("has") {
        return None;
    }
    let body = &note[idx + "segments:".len()..];
    static SEGMENTS_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = SEGMENTS_RE.get_or_init(|| {
        regex::Regex::new(r"(\d+)\s*:\s*(\d+)\s*\.\.\s*(\d+)(?:\s*/\s*(#[0-9A-Fa-f]+))?").unwrap()
    });
    let mut segs = Vec::new();
    let mut last_end = 0;
    for cap in re.captures_iter(body) {
        let m = cap.get(0).unwrap();
        let start: i64 = cap[2].parse().ok()?;
        let end: i64 = cap[3].parse().ok()?;
        segs.push(Segment {
            start: start - 1,
            end: end - 1,
            color: cap.get(4).map(|c| c.as_str().to_string()),
        });
        last_end = m.end();
    }
    if segs.is_empty() {
        return None;
    }
    let mut rest = body[last_end..]
        .trim()
        .trim_matches(|c| c == '"' || c == ';')
        .trim()
        .to_string();
    // A leading "/ name" is the last segment's name — drop it, keep any
    // following descriptive text (e.g. "Cleavage site after base 2417").
    if rest.starts_with('/') {
        rest = match rest.find('\n') {
            Some(nl) => rest[nl + 1..].trim().to_string(),
            None => String::new(),
        };
    }
    Some((segs, rest))
}

/// Map Chinese color names to hex values (common in SnapGene exports).
fn chinese_color_to_hex(name: &str) -> Option<&'static str> {
    match name {
        "黑色" => Some("#000000"),
        "白色" => Some("#FFFFFF"),
        "红色" => Some("#FF0000"),
        "绿色" => Some("#00FF00"),
        "蓝色" => Some("#0000FF"),
        "黄色" => Some("#FFFF00"),
        "橙色" => Some("#FFA500"),
        _ => None,
    }
}

/// Extract the binding (match) portion and 5' tail from a SnapGene primer sequence.
///
/// SnapGene uses lowercase for 5' tail (and internal non-binding regions) and
/// UPPERCASE for the binding region. The last contiguous uppercase block is the
/// binding region; everything before it is the 5' tail.
///
/// Returns `(mismatch_str, match_str)`.
fn split_snapgene_primer_seq(seq: &str) -> (String, String) {
    // Find the LAST contiguous uppercase block
    let chars: Vec<char> = seq.chars().collect();
    let mut upper_end = chars.len();
    let mut upper_start = chars.len();

    // Scan backwards to find the last uppercase block
    let mut in_upper = false;
    for i in (0..chars.len()).rev() {
        let is_upper = chars[i].is_ascii_uppercase();
        if is_upper && !in_upper {
            upper_end = i + 1;
            in_upper = true;
        } else if !is_upper && in_upper {
            upper_start = i + 1;
            break;
        }
    }
    if in_upper && upper_start == chars.len() {
        // The uppercase block extends to the beginning
        upper_start = 0;
    }

    let mismatch = seq[..upper_start].to_string();
    let match_str = seq[upper_start..upper_end].to_string();
    (mismatch, match_str)
}

/// Parse a primer_bind feature into a [`Primer`].
///
/// Tries these sources for the primer sequence (in order):
/// 1. `libregene_primer_seq` qualifier
/// 2. SnapGene note format (`sequence: ...`)
/// 3. Infer from template DNA at the feature's location
///
/// Binding sites are recomputed later by the alignment engine.
fn parse_snapgene_primer(f: &GbFeature, seq: &str) -> Option<Primer> {
    let label = f.qualifier_values("label").next().unwrap_or("unknown");
    let ptype = match &f.location {
        Location::Complement(_) => "rev",
        _ => "fwd",
    };

    let primer_seq = f
        .qualifier_values("libregene_primer_seq")
        .next()
        .map(|s| s.to_string())
        .or_else(|| {
            f.qualifier_values("note")
                .filter_map(parse_snapgene_primer_note)
                .map(|(_, seq)| {
                    let (mism, mstr) = split_snapgene_primer_seq(&seq);
                    format!("{}{}", mism.to_lowercase(), mstr.to_uppercase())
                })
                .next()
        })
        .or_else(|| {
            let (segs, _, _) = extract_location_bounds(&f.location);
            let seg = segs.first()?;
            if seg.start < 0 || (seg.end as usize) >= seq.len() {
                return None;
            }
            let sub = &seq[seg.start as usize..=seg.end as usize];
            Some(if ptype == "rev" {
                crate::utils::reverse_complement(sub)
            } else {
                sub.to_string()
            })
        })?;

    if primer_seq.is_empty() {
        return None;
    }

    let primer_id = f
        .qualifier_values("libregene_primer_id")
        .next()
        .unwrap_or(label);

    Some(Primer {
        id: primer_id.to_string(),
        name: label.to_string(),
        r#type: ptype.to_string(),
        primer_seq,
        binding_sites: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_location_string_1based() {
        // GenBank file-format semantics: 1-based inclusive input → 0-based model.
        let (segs, start, end, strand) = parse_location_string("100..200").unwrap();
        assert_eq!((start, end, strand.as_str()), (99, 199, "+"));
        assert_eq!(segs.len(), 1);
        assert_eq!((segs[0].start, segs[0].end), (99, 199));

        let (segs, start, end, strand) = parse_location_string("complement(50..80)").unwrap();
        assert_eq!((start, end, strand.as_str()), (49, 79, "-"));
        assert_eq!(segs.len(), 1);

        let (segs, start, end, strand) = parse_location_string("join(1..100,200..300)").unwrap();
        assert_eq!((start, end, strand.as_str()), (0, 299, "+"));
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].start, segs[0].end), (0, 99));
        assert_eq!((segs[1].start, segs[1].end), (199, 299));

        let (segs, start, end, _) = parse_location_string("order(10..20,5..8)").unwrap();
        assert_eq!((start, end), (4, 19));
        assert_eq!(segs.len(), 2);

        // Single position
        let (segs, start, end, _) = parse_location_string("42").unwrap();
        assert_eq!((start, end), (41, 41));
        assert_eq!(segs.len(), 1);

        // 1-based rejects 0 and reversed ranges
        assert!(parse_location_string("0..10").is_none());
        assert!(parse_location_string("0").is_none());
        assert!(parse_location_string("200..100").is_none());
        assert!(parse_location_string("abc").is_none());
        assert!(parse_location_string("join()").is_none());
    }

    #[test]
    fn test_parse_location_string_0based() {
        // App-internal semantics: coordinates are already 0-based inclusive.
        let (segs, start, end, strand) = parse_location_string_0based("99..199").unwrap();
        assert_eq!((start, end, strand.as_str()), (99, 199, "+"));
        assert_eq!((segs[0].start, segs[0].end), (99, 199));

        // 0 is a valid coordinate here
        let (segs, start, end, _) = parse_location_string_0based("0..99").unwrap();
        assert_eq!((start, end), (0, 99));
        assert_eq!((segs[0].start, segs[0].end), (0, 99));

        let (_, start, end, strand) = parse_location_string_0based("complement(49..79)").unwrap();
        assert_eq!((start, end, strand.as_str()), (49, 79, "-"));

        let (segs, start, end, _) = parse_location_string_0based("join(0..99,199..299)").unwrap();
        assert_eq!((start, end), (0, 299));
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[1].start, segs[1].end), (199, 299));

        let (_, start, end, _) = parse_location_string_0based("order(9..19,4..7)").unwrap();
        assert_eq!((start, end), (4, 19));

        // Single position
        let (_, start, end, _) = parse_location_string_0based("41").unwrap();
        assert_eq!((start, end), (41, 41));
        let (_, start, end, _) = parse_location_string_0based("0").unwrap();
        assert_eq!((start, end), (0, 0));

        // Negative and reversed ranges are rejected
        assert!(parse_location_string_0based("-1..10").is_none());
        assert!(parse_location_string_0based("199..99").is_none());
        assert!(parse_location_string_0based("abc").is_none());
        assert!(parse_location_string_0based("join()").is_none());
    }

    #[test]
    fn test_reverse_complement() {
        assert_eq!(crate::utils::reverse_complement("ATGC"), "GCAT");
        assert_eq!(crate::utils::reverse_complement("AATT"), "AATT");
    }

    #[test]
    fn test_split_snapgene_primer_seq() {
        let (mism, mstr) = split_snapgene_primer_seq("ggACTAGTgccaccATGGTGAGCAAGGGCGAG");
        assert_eq!(mism, "ggACTAGTgccacc");
        assert_eq!(mstr, "ATGGTGAGCAAGGGCGAG");
    }

    #[test]
    fn test_split_snapgene_no_tail() {
        let (mism, mstr) = split_snapgene_primer_seq("ATGGTGAGCAAGGGCGAG");
        assert_eq!(mism, "");
        assert_eq!(mstr, "ATGGTGAGCAAGGGCGAG");
    }

    #[test]
    fn test_chinese_color() {
        assert_eq!(chinese_color_to_hex("黑色"), Some("#000000"));
        assert_eq!(chinese_color_to_hex("蓝色"), Some("#0000FF"));
    }

    #[test]
    fn test_parse_snapgene_color() {
        let notes = vec!["color: #ccffcc"];
        assert_eq!(parse_snapgene_color_from_notes(&notes), "#ccffcc");
    }

    #[test]
    fn test_parse_snapgene_direction() {
        let notes = vec!["color: #ffffff; direction: RIGHT"];
        assert_eq!(parse_snapgene_direction_from_notes(&notes), "+");
    }

    #[test]
    fn test_parse_snapgene_primer_note() {
        let note = "color: 黑色; sequence: ggACTAGTgccaccATGGTGAGCAAGGGCGAG; added: 2026-02-26";
        let (color, seq) = parse_snapgene_primer_note(note).unwrap();
        assert_eq!(color, "#000000");
        assert_eq!(seq, "ggACTAGTgccaccATGGTGAGCAAGGGCGAG");
    }

    #[test]
    fn test_alignment_roundtrip() {
        let mut x = 42u64;
        let sequence: String = (0..300)
            .map(|_| {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                b"ACGT"[(x >> 33) as usize & 3] as char
            })
            .collect();
        let read = sequence[100..180].to_string();

        let mut project = ProjectData {
            sequence: sequence.clone(),
            length: sequence.len() as i64,
            topology: "circular".to_string(),
            ..Default::default()
        };
        let mut aln = crate::align::align_read(&sequence, &read, true).unwrap();
        aln.id = "aln-1".to_string();
        aln.name = "read1".to_string();
        project.alignments.push(aln);

        let path = std::env::temp_dir().join(format!("libregene_aln_roundtrip_{}.gbk", std::process::id()));
        write_gbk(&project, &path).unwrap();
        let parsed = parse_gbk(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(parsed.alignments.len(), 1);
        let a = &parsed.alignments[0];
        assert_eq!(a.name, "read1");
        assert_eq!(a.strand, "+");
        assert_eq!(a.seq, read);
        assert_eq!(a.segments.len(), 1);
        assert_eq!(a.segments[0].start, 100);
        assert_eq!(a.segments[0].end, 179);
        // Alignment misc_features must not leak into the features list.
        assert!(parsed.features.iter().all(|f| !f
            .qualifiers
            .iter()
            .any(|(k, _)| k == "libregene_align_seq")));
    }

    fn tiny_project(topology: &str) -> ProjectData {
        ProjectData {
            sequence: "ACGTACGTACGT".to_string(),
            length: 12,
            topology: topology.to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_parse_methylation_keyword() {
        let (rest, sys) = parse_methylation_keyword("methylation: Dam,Dcm,EcoKI");
        assert_eq!(rest, "");
        assert_eq!(sys, Some(vec!["dam".to_string(), "dcm".to_string(), "ecoki".to_string()]));

        let (rest, sys) = parse_methylation_keyword("cloning vector; Methylation: dam; other");
        assert_eq!(rest, "cloning vector; other");
        assert_eq!(sys, Some(vec!["dam".to_string()]));

        let (rest, sys) = parse_methylation_keyword("methylation: none");
        assert_eq!(rest, "");
        assert_eq!(sys, Some(Vec::new()));

        let (rest, sys) = parse_methylation_keyword("cloning vector");
        assert_eq!(rest, "cloning vector");
        assert_eq!(sys, None);
    }

    #[test]
    fn test_methylation_written_to_keywords_and_roundtrip() {
        let mut project = tiny_project("circular");
        project.methylation_systems = vec!["dam".to_string(), "dcm".to_string()];

        let path = std::env::temp_dir().join(format!("libregene_methyl_rt_{}.gbk", std::process::id()));
        write_gbk(&project, &path).unwrap();
        let parsed = parse_gbk(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(text.contains("KEYWORDS    methylation: Dam,Dcm"), "{}", text);
        assert_eq!(parsed.methylation_systems, vec!["dam".to_string(), "dcm".to_string()]);
        assert_eq!(parsed.keywords, "", "annotation must be stripped from keywords");
    }

    #[test]
    fn test_methylation_defaults_to_all_three_when_unannotated() {
        // A GBK without any methylation annotation (e.g. written by another
        // tool) defaults to all three systems for circular DNA. Build a valid
        // file with write_gbk, then strip the annotation like a foreign tool.
        let project = tiny_project("circular");
        let path = std::env::temp_dir().join(format!("libregene_methyl_default_{}.gbk", std::process::id()));
        write_gbk(&project, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let stripped = text.replacen("methylation: none", ".", 1);
        std::fs::write(&path, stripped).unwrap();

        let parsed = parse_gbk(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            parsed.methylation_systems,
            vec!["dam".to_string(), "dcm".to_string(), "ecoki".to_string()]
        );

        // ...and saving that project then annotates the file explicitly.
        let out = std::env::temp_dir().join(format!("libregene_methyl_default_out_{}.gbk", std::process::id()));
        write_gbk(&parsed, &out).unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        let _ = std::fs::remove_file(&out);
        assert!(text.contains("methylation: Dam,Dcm,EcoKI"), "{}", text);
    }

    #[test]
    fn test_methylation_none_and_linear_not_annotated() {
        // Explicitly no methylation → persisted as "methylation: none".
        let project = tiny_project("circular");
        let path = std::env::temp_dir().join(format!("libregene_methyl_none_{}.gbk", std::process::id()));
        write_gbk(&project, &path).unwrap();
        let parsed = parse_gbk(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(text.contains("methylation: none"), "{}", text);
        assert!(parsed.methylation_systems.is_empty(), "'none' must stay empty, not default to three");

        // Linear DNA is not annotated and does not default.
        let project = tiny_project("linear");
        write_gbk(&project, &path).unwrap();
        let parsed = parse_gbk(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(!text.contains("methylation"), "{}", text);
        assert!(parsed.methylation_systems.is_empty());
    }
}
