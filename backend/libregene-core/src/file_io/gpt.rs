//! Protein GenBank (`.gpt`) parser/writer.
//!
//! Hand-rolled because gb-io rejects the amino-acid alphabet (the terminal `*`
//! in particular). Format mirrors SnapGene's protein export:
//!
//! ```text
//! LOCUS       mCherry         237 aa            linear   UNA 06-AUG-2026
//! DEFINITION  .
//! ...
//! DBSOURCE    .
//! FEATURES             Location/Qualifiers
//!      Region          1..237
//!                      /note="color: #ff0000"
//!                      /label=mCherry
//! ORIGIN
//!         1 mvskgeednm ...
//! //
//! ```

use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use chrono::Datelike;

use crate::file_io::color::{adjust_color_readability, default_color, normalize_color};
use crate::models::{Feature, ProjectData};

// ---------------------------------------------------------------------------
// Parse
// ---------------------------------------------------------------------------

/// Parse a protein GenBank (`.gpt`) file and return a [`ProjectData`].
pub fn parse_gpt(path: &Path) -> io::Result<ProjectData> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut name = String::new();
    let mut definition = String::new();
    let mut keywords = String::new();
    let mut topology = "linear".to_string();
    let mut sequence = String::new();
    let mut features: Vec<Feature> = Vec::new();

    enum Section {
        Header,
        Features,
        Origin,
    }
    let mut section = Section::Header;
    // (kind, location, qualifiers) of the feature currently being collected.
    let mut cur_kind: Option<String> = None;
    let mut cur_loc = String::new();
    let mut cur_quals: Vec<(String, String)> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if trimmed == "//" {
            break;
        }

        match section {
            Section::Header => {
                if let Some(rest) = line.strip_prefix("LOCUS").or_else(|| line.strip_prefix("locus")) {
                    // LOCUS       mCherry         237 aa            linear   UNA 06-AUG-2026
                    let mut toks = rest.split_whitespace();
                    name = toks.next().unwrap_or("").to_string();
                    // Length may be "237" followed by "aa", or "237aa" in one token.
                    let mut len_tok = toks.next().unwrap_or("");
                    if len_tok.ends_with("aa") {
                        len_tok = &len_tok[..len_tok.len() - 2];
                    } else if toks.next().map(|t| t == "aa").unwrap_or(false) {
                        // unit token consumed
                    }
                    let _len: i64 = len_tok.parse().unwrap_or(0);
                    if let Some(t) = toks.next() {
                        topology = t.to_string();
                    }
                } else if let Some(rest) = line.strip_prefix("DEFINITION") {
                    definition = rest.trim().to_string();
                } else if let Some(rest) = line.strip_prefix("KEYWORDS") {
                    keywords = rest.trim().to_string();
                } else if trimmed.starts_with("FEATURES") {
                    section = Section::Features;
                }
            }
            Section::Features => {
                if trimmed.starts_with("ORIGIN") {
                    section = Section::Origin;
                    continue;
                }
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.starts_with('/') {
                    // Qualifier line: /key="value" or /key=value
                    if let Some((k, v)) = parse_qualifier(trimmed) {
                        cur_quals.push((k, v));
                    }
                } else if is_qualifier_continuation(&line) {
                    // Continuation of a multi-line qualifier value: GenBank
                    // wraps long values at the qualifier column, keeping a
                    // word boundary between the lines.
                    if let Some((_, v)) = cur_quals.last_mut() {
                        let cont = trimmed.strip_suffix('"').unwrap_or(trimmed);
                        v.push(' ');
                        v.push_str(cont);
                    }
                } else if let Some(kind) = trimmed.split_whitespace().next() {
                    // New feature line: kind + location
                    if let Some(k) = cur_kind.take() {
                        if k != "source" {
                            if let Some(f) = build_feature(&k, &cur_loc, &cur_quals) {
                                features.push(f);
                            }
                        }
                    }
                    cur_kind = Some(kind.to_string());
                    cur_loc = trimmed
                        .split_whitespace()
                        .skip(1)
                        .collect::<Vec<_>>()
                        .join(" ");
                    cur_quals.clear();
                }
            }
            Section::Origin => {
                // "        1 mvskgeednm aiikefmrfk ..." — strip the position prefix
                let body = trimmed
                    .split_whitespace()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join("");
                sequence.push_str(&body);
            }
        }
    }
    // Flush the trailing feature
    if let Some(k) = cur_kind {
        if k != "source" {
            if let Some(f) = build_feature(&k, &cur_loc, &cur_quals) {
                features.push(f);
            }
        }
    }

    let strip_dot = |s: String| if s == "." { String::new() } else { s };
    let definition = strip_dot(definition);
    let keywords = strip_dot(keywords);
    let length = sequence.len() as i64;

    Ok(ProjectData {
        name,
        definition,
        keywords,
        sequence,
        length,
        topology,
        molecule_type: "protein".to_string(),
        features,
        ..Default::default()
    })
}

/// Parse `/key="value"` or `/key=value`. A value whose closing quote sits on a
/// later line keeps its opening quote here; the continuation handler strips the
/// closing one.
fn parse_qualifier(s: &str) -> Option<(String, String)> {
    let s = s.strip_prefix('/')?.trim();
    let (k, v) = s.split_once('=')?;
    let v = v.trim();
    let v = match v.strip_prefix('"') {
        Some(r) => r.strip_suffix('"').unwrap_or(r),
        None => v,
    };
    Some((k.trim().to_string(), v.to_string()))
}

/// Continuation lines of a multi-line qualifier sit at the qualifier column
/// (≥ 16 leading spaces; feature keys start at column 5) and don't begin with
/// `/` (qualifiers themselves are handled before this check).
fn is_qualifier_continuation(line: &str) -> bool {
    line.len() - line.trim_start().len() >= 16
}

/// Build a [`Feature`] from a `.gpt` feature line using the same qualifier
/// conventions as the `.gbk` parser (label → name, color note → color, …).
fn build_feature(kind: &str, location: &str, qualifiers: &[(String, String)]) -> Option<Feature> {
    let (segments, start, end, strand) =
        crate::file_io::gbk::parse_location_string(location)?;

    let label = qualifiers
        .iter()
        .find(|(k, _)| k == "label")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| kind.to_string());

    let note_values: Vec<&str> = qualifiers
        .iter()
        .filter(|(k, _)| k == "note")
        .map(|(_, v)| v.as_str())
        .collect();

    let mut color = note_values
        .iter()
        .find_map(|n| parse_color_note(n))
        .unwrap_or_default();
    if color.is_empty() {
        color = default_color(kind).to_string();
    }
    let color = adjust_color_readability(&color);

    let notes = note_values
        .iter()
        .filter(|n| !n.contains("color:"))
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("; ");

    let skip_keys: std::collections::HashSet<&str> = [
        "label", "note", "translation",
    ].into_iter().collect();
    let quals: Vec<(String, String)> = qualifiers
        .iter()
        .filter(|(k, _)| !skip_keys.contains(k.as_str()))
        .cloned()
        .collect();

    Some(Feature {
        id: format!("{}_{}", label, start),
        name: label,
        start,
        end,
        color,
        ftype: kind.to_string(),
        segments,
        strand,
        notes,
        translation: String::new(),
        qualifiers: quals,
    })
}

/// Parse `color: #XXXXXX` from a SnapGene-style note.
fn parse_color_note(note: &str) -> Option<String> {
    let rest = note.strip_prefix("color:")?;
    let color_part = rest.split(';').next()?.trim();
    let normalized = normalize_color(color_part);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

// ---------------------------------------------------------------------------
// Write
// ---------------------------------------------------------------------------

/// Write a [`ProjectData`] as a protein GenBank (`.gpt`) file.
pub fn write_gpt(project: &ProjectData, path: &Path) -> io::Result<()> {
    let mut out = String::new();

    let name = if project.name.is_empty() {
        path.file_stem()
            .map(|s| s.to_string_lossy().replace(' ', "_"))
            .unwrap_or_else(|| "libregene".to_string())
    } else {
        project.name.replace(' ', "_")
    };

    let now = chrono::Local::now();
    let date = format!(
        "{:02}-{}-{}",
        now.day(),
        now.format("%b").to_string().to_uppercase(),
        now.year()
    );

    out.push_str(&format!(
        "LOCUS       {}         {} aa            linear   UNA {}\n",
        name, project.sequence.len(), date
    ));
    out.push_str(&format!("DEFINITION  {}\n", dot_or(&project.definition)));
    out.push_str("ACCESSION   .\n");
    out.push_str("VERSION     .\n");
    out.push_str("DBSOURCE    .\n");
    out.push_str(&format!("KEYWORDS    {}\n", dot_or(&project.keywords)));
    out.push_str("SOURCE      natural protein sequence\n");
    out.push_str("  ORGANISM  unspecified\n");
    out.push_str("FEATURES             Location/Qualifiers\n");

    let len = project.sequence.len() as i64;
    out.push_str(&format!("     source          {}..{}\n", 1, len));
    out.push_str(&"                     /organism=\"unspecified\"\n".to_string());

    for f in &project.features {
        serialize_feature_gpt(f, &mut out);
    }

    out.push_str("ORIGIN\n");
    let seq_lower = project.sequence.to_lowercase();
    let chars: Vec<char> = seq_lower.chars().collect();
    for (i, chunk) in chars.chunks(60).enumerate() {
        let groups: Vec<String> = chunk.chunks(10).map(|g| g.iter().collect()).collect();
        out.push_str(&format!("{:>9} {}\n", i * 60 + 1, groups.join(" ")));
    }
    out.push_str("//\n");

    let mut file = File::create(path)?;
    file.write_all(out.as_bytes())
}

fn dot_or(s: &str) -> &str {
    if s.is_empty() { "." } else { s }
}

fn serialize_feature_gpt(f: &Feature, out: &mut String) {
    let kind = if f.ftype.is_empty() {
        "misc_feature".to_string()
    } else {
        f.ftype.clone()
    };
    let loc = if f.strand == "-" {
        format!("complement({}..{})", f.start + 1, f.end + 1)
    } else {
        format!("{}..{}", f.start + 1, f.end + 1)
    };
    out.push_str(&format!("     {:11} {}\n", kind, loc));

    out.push_str(&format!("                     /label={}\n", f.name));
    if !f.notes.is_empty() {
        for note_line in f.notes.split("; ") {
            if !note_line.is_empty() {
                out.push_str(&format!("                     /note=\"{}\"\n", note_line));
            }
        }
    }
    let color_note = if f.strand == "-" {
        format!("color: {}; direction: LEFT", f.color)
    } else {
        format!("color: {}", f.color)
    };
    out.push_str(&format!("                     /note=\"{}\"\n", color_note));

    let skip_keys: std::collections::HashSet<&str> = [
        "label", "note", "translation",
    ].into_iter().collect();
    for (k, v) in &f.qualifiers {
        if skip_keys.contains(k.as_str())
            || k.starts_with("ApEinfo_")
            || k.starts_with("libregene_")
        {
            continue;
        }
        out.push_str(&format!("                     /{}=\"{}\"\n", k, v));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_io::gbk::write_gbk;

    fn test_data_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("test_data")
    }

    #[test]
    fn parse_example_gpt() {
        let project = parse_gpt(&test_data_dir().join("mCherry.gpt")).unwrap();
        assert_eq!(project.molecule_type, "protein");
        assert_eq!(project.topology, "linear");
        assert_eq!(project.length, 237);
        assert!(project.sequence.ends_with('*'));
        assert!(project.sequence.chars().all(|c| c.is_ascii_alphabetic() || c == '*'));

        let feat = project.features.iter().find(|f| f.name == "mCherry").expect("mCherry feature");
        assert_eq!(feat.ftype, "Region");
        assert_eq!(feat.color, "#ff0000");
        assert_eq!((feat.start, feat.end), (0, 236));
    }

    #[test]
    fn parse_ncbi_genpept_sample() {
        // NCBI GenPept style: no /label (name falls back to kind), /coded_by,
        // multi-line qualifier continuations.
        let project = parse_gpt(&test_data_dir().join("mCherry-gp.gp")).unwrap();
        assert_eq!(project.molecule_type, "protein");
        assert_eq!(project.topology, "linear");
        assert_eq!(project.length, 237);
        assert!(project.sequence.ends_with('*'));

        let protein = project
            .features
            .iter()
            .find(|f| f.ftype == "Protein")
            .expect("Protein feature");
        assert_eq!(protein.name, "Protein", "no /label → name falls back to kind");
        assert_eq!((protein.start, protein.end), (0, 236));

        let mat = project
            .features
            .iter()
            .find(|f| f.ftype == "mat_peptide")
            .expect("mat_peptide feature");
        assert_eq!((mat.start, mat.end), (7, 236));
        let coded_by = mat
            .qualifiers
            .iter()
            .find(|(k, _)| k == "coded_by")
            .expect("coded_by qualifier");
        assert_eq!(coded_by.1, "complement(join(500..1000,1200..1400))");
        assert_eq!(mat.notes, "N-terminally processed mature form");

        let region = project
            .features
            .iter()
            .find(|f| f.ftype == "Region")
            .expect("Region feature");
        assert_eq!((region.start, region.end), (19, 79));
        assert_eq!(region.notes, "beta barrel domain spanning residues");
    }

    #[test]
    fn gpt_roundtrip() {
        let original = parse_gpt(&test_data_dir().join("mCherry.gpt")).unwrap();
        // Source-feature qualifiers must not leak into real features.
        let orig_feat = original.features.iter().find(|f| f.name == "mCherry").unwrap();
        assert!(orig_feat.qualifiers.iter().all(|(k, _)| k != "organism"), "no leaked organism qualifier");

        let tmp = std::env::temp_dir().join(format!("libregene_gpt_rt_{}.gpt", std::process::id()));
        write_gpt(&original, &tmp).unwrap();
        let reloaded = parse_gpt(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(reloaded.molecule_type, "protein");
        assert_eq!(reloaded.sequence, original.sequence);
        assert_eq!(reloaded.features.len(), original.features.len());
        let feat = reloaded.features.iter().find(|f| f.name == "mCherry").unwrap();
        assert_eq!(feat.color, "#ff0000");
        assert!(feat.qualifiers.iter().all(|(k, _)| k != "organism"), "no leaked organism after roundtrip");
    }

    #[test]
    fn gpt_features_are_gbk_compatible() {
        // The .gpt feature model must match what the .gbk path produces so a
        // protein project can round-trip through either writer.
        let project = parse_gpt(&test_data_dir().join("mCherry.gpt")).unwrap();
        let tmp_gbk = std::env::temp_dir().join(format!("libregene_prot_as_gbk_{}.gbk", std::process::id()));
        let tmp_gpt = std::env::temp_dir().join(format!("libregene_prot_as_gpt_{}.gpt", std::process::id()));
        write_gbk(&project, &tmp_gbk).unwrap();
        write_gpt(&project, &tmp_gpt).unwrap();
        let _ = std::fs::remove_file(&tmp_gbk);
        let _ = std::fs::remove_file(&tmp_gpt);
    }
}
