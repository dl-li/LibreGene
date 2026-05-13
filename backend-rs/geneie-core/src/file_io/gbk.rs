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

use gb_io::reader::SeqReader;
use gb_io::seq::{After, Before, Feature as GbFeature, Location, Seq, Topology};
use gb_io::writer::SeqWriter;

use crate::file_io::color::{adjust_color_readability, default_color, normalize_color};
use crate::models::{Feature, Primer, ProjectData, Segment};

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
        .unwrap_or("")
        .to_uppercase();

    let topology = match seq.topology {
        Topology::Circular => "circular".to_string(),
        Topology::Linear => "linear".to_string(),
    };

    let mut features: Vec<Feature> = Vec::new();
    let mut primers: Vec<Primer> = Vec::new();

    for f in &seq.features {
        let kind = f.kind.as_ref();

        if kind == "primer_bind" {
            if let Some(p) = parse_snapgene_primer(f, &sequence) {
                primers.push(p);
            }
            continue;
        }

        if kind == "source" {
            continue;
        }

        // Resolve label
        let label = f
            .qualifier_values("label")
            .next()
            .or_else(|| f.qualifier_values("gene").next())
            .unwrap_or(kind)
            .to_string();

        // Collect all notes (SnapGene stores color/direction in notes)
        let note_values: Vec<&str> = f.qualifier_values("note").collect();

        // Resolve colour: try ApE/geneie qualifiers first, then SnapGene note format
        let mut color = normalize_color(
            f.qualifier_values("ApEinfo_fwdcolor")
                .next()
                .or_else(|| f.qualifier_values("geneie_color").next())
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

        // Extract coordinates
        let (segments, start, end) = extract_location_bounds(&f.location);

        let translation = f
            .qualifier_values("translation")
            .next()
            .unwrap_or("")
            .to_string();

        // Build notes string, filtering out SnapGene color/direction notes
        let notes = note_values
            .iter()
            .filter(|n| !is_snapgene_color_note(n) && !is_snapgene_direction_note(n))
            .copied()
            .collect::<Vec<_>>()
            .join("; ");

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
        });
    }

    let length = sequence.len() as i64;

    Ok(ProjectData {
        sequence,
        length,
        topology,
        features,
        primers,
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Write
// ---------------------------------------------------------------------------

/// Write a [`ProjectData`] as a SnapGene-flavoured GenBank file.
pub fn write_gbk(project: &ProjectData, path: &Path) -> io::Result<()> {
    let mut record = Seq::empty();

    record.name = Some("geneie".to_string());
    record.topology = if project.topology == "circular" {
        Topology::Circular
    } else {
        Topology::Linear
    };
    record.molecule_type = Some("DNA".to_string());
    record.division = "SYN".to_string();
    record.seq = project.sequence.as_bytes().to_vec();
    record.len = Some(project.sequence.len());

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
                Some("Synthetic".to_string()),
            ),
            (
                Cow::Borrowed("mol_type"),
                Some("other DNA".to_string()),
            ),
            (
                Cow::Borrowed("organism"),
                Some("synthetic DNA construct".to_string()),
            ),
        ],
    });

    // Add features — SnapGene-style with color and direction in /note
    for f in &project.features {
        let loc = model_range_to_gb_location(&f);

        let mut qualifiers: Vec<(Cow<'static, str>, Option<String>)> = vec![
            (Cow::Borrowed("label"), Some(f.name.clone())),
        ];

        // Build SnapGene color note
        let color_note = format!("color: {}", f.color);
        qualifiers.push((Cow::Borrowed("note"), Some(color_note)));

        // Add direction note for forward strand features
        if f.strand == "+" {
            qualifiers.push((
                Cow::Borrowed("note"),
                Some("direction: RIGHT".to_string()),
            ));
        }

        // Add direction qualifier for reverse strand (SnapGene convention)
        if f.strand == "-" {
            qualifiers.push((Cow::Borrowed("direction"), Some("LEFT".to_string())));
        }

        // Other notes
        if !f.notes.is_empty() {
            for note_line in f.notes.split("; ") {
                if !note_line.is_empty() {
                    qualifiers.push((Cow::Borrowed("note"), Some(note_line.to_string())));
                }
            }
        }

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

    let file = File::create(path)?;
    let mut writer = SeqWriter::new(file);
    writer.write(&record)
}

/// Serialize primers in SnapGene format.
fn serialize_primers_snapgene(project: &ProjectData, record: &mut Seq) {
    for p in &project.primers {
        let best = p.binding_sites.first();
        let ms = best.map(|b| b.match_start).unwrap_or(0);
        let me = best.map(|b| b.match_end).unwrap_or(0);

        let color = if p.color.is_empty() {
            "#166534"
        } else {
            &p.color
        };

        // SnapGene primer note with full primer_seq
        let note = format!("color: {}; sequence: {}", color, p.primer_seq);

        let loc = Location::Range((ms, Before(false)), (me + 1, After(false)));
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
                if let Location::Range((s, _), (e, _)) = p {
                    let seg_start = *s; // 0-based
                    let seg_end = e - 1; // 0-based inclusive
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
            let start = *s;
            let end = e - 1;
            (vec![], start, end)
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
fn model_range_to_gb_location(f: &Feature) -> Location {
    // Model coordinates: 0-based inclusive start/end
    // gb-io Range: 0-based, end-exclusive

    if f.segments.is_empty() {
        let loc = Location::Range(
            (f.start, Before(false)),
            (f.end + 1, After(false)),
        );
        if f.strand == "-" {
            Location::Complement(Box::new(loc))
        } else {
            loc
        }
    } else {
        let parts: Vec<Location> = f
            .segments
            .iter()
            .map(|seg| {
                Location::Range(
                    (seg.start, Before(false)),
                    (seg.end + 1, After(false)),
                )
            })
            .collect();
        let loc = Location::Join(parts);
        if f.strand == "-" {
            Location::Complement(Box::new(loc))
        } else {
            loc
        }
    }
}

// ---------------------------------------------------------------------------
//  Fallback primer parser / serializer
// ---------------------------------------------------------------------------

/// Fallback `.gbk` primer parser — only extracts name, type, primer_seq, and color.
/// Binding sites are recomputed by the alignment engine after loading.
fn parse_primer_feature_fallback(f: &GbFeature, _seq: &str) -> Option<Primer> {
    let label = f.qualifier_values("label").next().unwrap_or("unknown");
    let primer_id = f
        .qualifier_values("geneie_primer_id")
        .next()
        .unwrap_or(label);
    let ptype = f
        .qualifier_values("geneie_primer_type")
        .next()
        .unwrap_or("fwd");
    let color = f.qualifier_values("geneie_color").next().unwrap_or("#166534");

    // Read primer_seq from qualifier (preferred) or try SnapGene note.
    let primer_seq = f
        .qualifier_values("geneie_primer_seq")
        .next()
        .map(|s| s.to_string())
        .or_else(|| {
            f.qualifier_values("note")
                .filter_map(|n| parse_snapgene_primer_note(n))
                .map(|(_, seq)| seq)
                .next()
        })
        .unwrap_or_default();

    Some(Primer {
        id: primer_id.to_string(),
        name: label.to_string(),
        r#type: ptype.to_string(),
        primer_seq,
        color: color.to_string(),
        binding_sites: Vec::new(),
    })
}

/// Fallback `.gbk` primer serializer.
#[allow(dead_code)]
fn serialize_primers_fallback(project: &ProjectData, record: &mut Seq) {
    for p in &project.primers {
        let best = p.binding_sites.first();
        let ms = best.map(|b| b.match_start).unwrap_or(0);
        let me = best.map(|b| b.match_end).unwrap_or(0);

        let mut qualifiers: Vec<(Cow<'static, str>, Option<String>)> = vec![
            (Cow::Borrowed("label"), Some(p.name.clone())),
            (Cow::Borrowed("geneie_primer_id"), Some(p.id.clone())),
            (Cow::Borrowed("geneie_primer_type"), Some(p.r#type.clone())),
            (Cow::Borrowed("geneie_primer_seq"), Some(p.primer_seq.clone())),
            (Cow::Borrowed("geneie_color"), Some(p.color.clone())),
        ];

        if !p.binding_sites.is_empty() {
            let parts: Vec<String> = p
                .binding_sites
                .iter()
                .map(|bs| format!("{},{},{:.1}", bs.match_start, bs.match_end, bs.tm))
                .collect();
            qualifiers.push((
                Cow::Borrowed("geneie_bindings"),
                Some(parts.join(";")),
            ));
        }

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
#[allow(dead_code)]
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

/// Parse a SnapGene primer_bind feature from its notes.
/// Only extracts name, type, color, and primer_seq.
/// Binding sites are recomputed later by the alignment engine.
fn parse_snapgene_primer(f: &GbFeature, seq: &str) -> Option<Primer> {
    // First, try the standard geneie_* qualifier format
    let has_geneie = f.qualifier_values("geneie_primer_id").next().is_some()
        || f.qualifier_values("geneie_primer_seq").next().is_some();
    if has_geneie {
        return parse_primer_feature_fallback(f, seq);
    }

    // Try SnapGene note format
    let notes: Vec<&str> = f.qualifier_values("note").collect();
    let mut primer_seq = String::new();
    let mut color = String::new();

    for note in &notes {
        if let Some((c, s)) = parse_snapgene_primer_note(note) {
            primer_seq = s;
            color = c;
            break;
        }
    }

    if primer_seq.is_empty() {
        return parse_primer_feature_fallback(f, seq);
    }

    let label = f.qualifier_values("label").next().unwrap_or("unknown");
    let primer_id = f
        .qualifier_values("geneie_primer_id")
        .next()
        .unwrap_or(label);

    let ptype = match &f.location {
        Location::Complement(_) => "rev",
        _ => "fwd",
    };

    let primer_color = if color.is_empty() {
        "#166534".to_string()
    } else {
        color
    };

    Some(Primer {
        id: primer_id.to_string(),
        name: label.to_string(),
        r#type: ptype.to_string(),
        primer_seq,
        color: primer_color,
        binding_sites: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
