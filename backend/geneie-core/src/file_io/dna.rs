//! SnapGene `.dna` parser.
//!
//! The `.dna` format uses a TLV (Type-Length-Value) binary structure:
//!
//! ```text
//! Cookie:  1 byte (0x09) + 4 bytes BE u32 (14) + 8 bytes "SnapGene"
//! Header:  3 × uint16 (file ver, DNA type, export ver) = 6 bytes
//! Blocks:  [1 byte type | 4 bytes BE u32 length | payload ...]
//! ```
//!
//! Block types used:
//! - 0: DNA sequence (plain text)
//! - 5: Primers (XML)
//! - 8: Additional sequence properties (XML, contains topology)
//! - 10: Features (XML)

use std::fs;
use std::io::{self};
use std::path::Path;

use serde::Deserialize;

use crate::file_io::color::{adjust_color_readability, default_color, normalize_color};
use crate::models::{Feature, Primer, ProjectData, Segment};

// ---------------------------------------------------------------------------
// XML deserialisation structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SnapGeneFeatures {
    #[serde(rename = "Feature", default)]
    features: Vec<SnapGeneFeature>,
}

#[derive(Debug, Deserialize)]
struct SnapGeneFeature {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@type", default)]
    feature_type: Option<String>,
    #[serde(rename = "@directionality", default)]
    directionality: Option<String>,
    #[serde(rename = "Segment", default)]
    segments: Vec<SnapGeneSegment>,
    #[serde(rename = "Q", default)]
    qualifiers: Vec<SnapGeneQualifier>,
}

#[derive(Debug, Deserialize)]
struct SnapGeneSegment {
    #[serde(rename = "@range")]
    range: String,
    #[serde(rename = "@color", default)]
    color: Option<String>,
    #[serde(rename = "@type", default)]
    seg_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SnapGeneQualifier {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "V", default)]
    values: Vec<SnapGeneQualifierValue>,
}

#[derive(Debug, Deserialize)]
struct SnapGeneQualifierValue {
    #[serde(rename = "@text", default)]
    text: Option<String>,
    #[serde(rename = "@predef", default)]
    predef: Option<String>,
    #[serde(rename = "@int", default)]
    int_val: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SnapGenePrimers {
    #[serde(rename = "Primer", default)]
    primers: Vec<SnapGenePrimer>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SnapGenePrimer {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@sequence", default)]
    sequence: Option<String>,
    #[serde(rename = "@description", default)]
    description: Option<String>,
    #[serde(rename = "BindingSite", default)]
    binding_sites: Vec<SnapGeneBindingSite>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SnapGeneBindingSite {
    #[serde(rename = "@location")]
    location: String,
    #[serde(rename = "@boundStrand", default)]
    bound_strand: Option<String>,
    #[serde(rename = "@meltingTemperature", default)]
    melting_temp: Option<String>,
    #[serde(rename = "@simplified", default)]
    simplified: Option<String>,
    #[serde(rename = "@visible", default)]
    visible: Option<String>,
    #[serde(rename = "Component", default)]
    components: Vec<SnapGeneComponent>,
}

#[derive(Debug, Deserialize)]
pub struct SnapGeneComponent {
    #[serde(rename = "@bases", default)]
    pub bases: Option<String>,
    #[serde(rename = "@hybridizedRange", default)]
    pub hybridized_range: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SnapGeneProperties {
    #[serde(rename = "@topology", default)]
    topology: Option<String>,
}

// ---------------------------------------------------------------------------
// TLV reader helpers
// ---------------------------------------------------------------------------

fn read_be_u32(data: &[u8], offset: &mut usize) -> io::Result<u32> {
    if *offset + 4 > data.len() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated u32"));
    }
    let bytes: [u8; 4] = data[*offset..*offset + 4].try_into().unwrap();
    *offset += 4;
    Ok(u32::from_be_bytes(bytes))
}

fn read_be_u16(data: &[u8], offset: &mut usize) -> io::Result<u16> {
    if *offset + 2 > data.len() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated u16"));
    }
    let bytes: [u8; 2] = data[*offset..*offset + 2].try_into().unwrap();
    *offset += 2;
    Ok(u16::from_be_bytes(bytes))
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a SnapGene `.dna` file and return a [`ProjectData`].
pub fn parse_dna(path: &Path) -> io::Result<ProjectData> {
    let data = fs::read(path)?;

    let mut offset = 0usize;

    // --- Cookie ---
    if offset + 13 > data.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too short"));
    }
    if data[offset] != 0x09 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid cookie byte",
        ));
    }
    offset += 1;

    let cookie_len = read_be_u32(&data, &mut offset)?;
    if cookie_len != 14 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected cookie length 14, got {}", cookie_len),
        ));
    }

    let magic = &data[offset..offset + 8];
    if magic != b"SnapGene" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing SnapGene magic",
        ));
    }
    offset += 8;

    // --- Header (6 bytes: 3 × uint16) ---
    if offset + 6 > data.len() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "header truncated"));
    }
    let _file_ver = read_be_u16(&data, &mut offset)?;
    let _dna_type = read_be_u16(&data, &mut offset)?;
    let _export_ver = read_be_u16(&data, &mut offset)?;

    // --- TLV blocks ---
    let mut sequence = String::new();
    let mut topology = "circular".to_string();
    let mut features_xml = String::new();
    let mut primers_xml = String::new();

    while offset < data.len() {
        if offset + 5 > data.len() {
            break;
        }
        let block_type = data[offset];
        offset += 1;
        let block_len = read_be_u32(&data, &mut offset)? as usize;
        if offset + block_len > data.len() {
            break;
        }

        let payload = &data[offset..offset + block_len];
        offset += block_len;

        match block_type {
            0 => {
                // DNA sequence — plain UTF-8 text
                sequence = String::from_utf8_lossy(payload)
                    .to_uppercase()
                    .chars()
                    .filter(|c| c.is_ascii_alphabetic())
                    .collect();
            }
            5 => {
                // Primers XML
                primers_xml = String::from_utf8_lossy(payload).to_string();
            }
            8 => {
                // Additional sequence properties XML (topology)
                let xml_str = String::from_utf8_lossy(payload);
                if let Ok(props) = quick_xml::de::from_str::<SnapGeneProperties>(&xml_str) {
                    if let Some(topo) = props.topology {
                        topology = topo;
                    }
                }
            }
            10 => {
                // Features XML
                features_xml = String::from_utf8_lossy(payload).to_string();
            }
            _ => {
                // skip unknown block types
            }
        }
    }

    // --- Parse features ---
    let mut features = Vec::new();
    if !features_xml.is_empty() {
        if let Ok(sg_features) = quick_xml::de::from_str::<SnapGeneFeatures>(&features_xml) {
            for sf in &sg_features.features {
                let ftype = sf.feature_type.as_deref().unwrap_or("misc_feature");
                let name = sf.name.clone();

                // Parse segments — filter to @type="standard"
                let std_segs: Vec<&SnapGeneSegment> = sf
                    .segments
                    .iter()
                    .filter(|s| s.seg_type.as_deref().unwrap_or("standard") == "standard")
                    .collect();

                let (start, end, segs, raw_color) = if std_segs.len() > 1 {
                    let mut parsed_segs = Vec::new();
                    let mut min_s = i64::MAX;
                    let mut max_e = i64::MIN;
                    let mut seg_color = String::new();
                    for seg in &std_segs {
                        if let Some((s, e)) = parse_range_1based(&seg.range) {
                            let s0 = s - 1;
                            let e0 = e - 1;
                            parsed_segs.push(Segment {
                                start: s0,
                                end: e0,
                                color: seg.color.clone(),
                            });
                            min_s = min_s.min(s0);
                            max_e = max_e.max(e0);
                            if seg_color.is_empty() {
                                seg_color = seg.color.clone().unwrap_or_default();
                            }
                        }
                    }
                    (min_s, max_e, parsed_segs, seg_color)
                } else if let Some(first) = std_segs.first() {
                    if let Some((s, e)) = parse_range_1based(&first.range) {
                        (
                            s - 1,
                            e - 1,
                            vec![],
                            first.color.clone().unwrap_or_default(),
                        )
                    } else {
                        continue; // skip unparseable feature
                    }
                } else if let Some(first) = sf.segments.first() {
                    // No standard segments — use overall first segment
                    if let Some((s, e)) = parse_range_1based(&first.range) {
                        (
                            s - 1,
                            e - 1,
                            vec![],
                            first.color.clone().unwrap_or_default(),
                        )
                    } else {
                        continue;
                    }
                } else {
                    continue; // no segments at all
                };

                // Resolve colour
                let color = normalize_color(&raw_color);
                let color = if color.is_empty() {
                    // Fallback to qualifiers
                    let from_qual = sf.qualifiers.iter().find_map(|q| {
                        if q.name == "ApEinfo_fwdcolor" || q.name == "geneie_color" {
                            q.values.first().and_then(|v| {
                                v.text
                                    .as_deref()
                                    .or(v.predef.as_deref())
                                    .or(v.int_val.as_deref())
                            })
                        } else {
                            None
                        }
                    });
                    let from_qual = from_qual.unwrap_or("");
                    let nc = normalize_color(from_qual);
                    if nc.is_empty() {
                        default_color(ftype).to_string()
                    } else {
                        nc
                    }
                } else {
                    color
                };
                let color = adjust_color_readability(&color);

                // Directionality → strand
                let strand = match sf.directionality.as_deref() {
                    Some("1") => "+",
                    Some("2") => "-",
                    _ => ".",
                };

                // Qualifier values
                let notes = sf
                    .qualifiers
                    .iter()
                    .filter(|q| q.name == "note")
                    .filter_map(|q| {
                        q.values.first().and_then(|v| {
                            v.text
                                .as_deref()
                                .or(v.predef.as_deref())
                                .or(v.int_val.as_deref())
                        })
                    })
                    .collect::<Vec<_>>()
                    .join("; ");

                let translation = sf
                    .qualifiers
                    .iter()
                    .filter(|q| q.name == "translation")
                    .filter_map(|q| {
                        q.values.first().and_then(|v| {
                            v.text
                                .as_deref()
                                .or(v.predef.as_deref())
                                .or(v.int_val.as_deref())
                        })
                    })
                    .next()
                    .unwrap_or("")
                    .replace(',', "");   // SnapGene uses commas as CDS gap markers; strip them

                // Collect raw qualifier key-value pairs (filter out internal ones like gbk.rs does)
                let skip_keys: std::collections::HashSet<&str> = [
                    "label", "translation", "ApEinfo_fwdcolor", "ApEinfo_revcolor",
                    "geneie_color", "direction", "directionality",
                    "geneie_primer_id", "geneie_primer_seq", "geneie_primer_type",
                ].into_iter().collect();
                let qualifiers: Vec<(String, String)> = sf
                    .qualifiers
                    .iter()
                    .filter(|q| !skip_keys.contains(q.name.as_str()))
                    .filter_map(|q| {
                        q.values.first().and_then(|v| {
                            v.text
                                .as_deref()
                                .or(v.predef.as_deref())
                                .or(v.int_val.as_deref())
                                .map(|val| (q.name.clone(), val.to_string()))
                        })
                    })
                    .collect();

                features.push(Feature {
                    id: format!("{}_{}", name, start),
                    name,
                    start,
                    end,
                    color,
                    ftype: ftype.to_string(),
                    segments: segs,
                    strand: strand.to_string(),
                    notes,
                    translation,
                    qualifiers,
                });
            }
        }
    }

    // --- Parse primers ---
    let primers = parse_dna_primers(&primers_xml, &sequence);

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
// Primer parsing (block 5 XML)
// ---------------------------------------------------------------------------

pub fn parse_dna_primers(xml: &str, _full_seq: &str) -> Vec<Primer> {
    if xml.is_empty() {
        return vec![];
    }

    let sg_primers: SnapGenePrimers = match quick_xml::de::from_str(xml) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    sg_primers
        .primers
        .iter()
        .filter_map(|sp| {
            // Filter out hidden primers.
            let visible_sites: Vec<&SnapGeneBindingSite> = sp
                .binding_sites
                .iter()
                .filter(|bs| !bs.is_hidden() && !bs.is_simplified())
                .collect();

            if visible_sites.is_empty() {
                return None;
            }

            // Determine type from first binding site.
            let ptype = match visible_sites[0].bound_strand.as_deref() {
                Some("0") => "fwd",
                Some("1") => "rev",
                _ => "fwd",
            };

            // primer_seq: prefer @sequence from the XML.
            let primer_seq = sp.sequence.clone().unwrap_or_default();
            if primer_seq.is_empty() {
                return None;
            }

            Some(Primer {
                id: sp.name.clone(),
                name: sp.name.clone(),
                r#type: ptype.to_string(),
                primer_seq,
                color: "#166534".to_string(),
                binding_sites: Vec::new(),
            })
        })
        .collect()
}

impl SnapGeneBindingSite {
    fn is_hidden(&self) -> bool {
        self.visible.as_deref() == Some("0")
    }

    fn is_simplified(&self) -> bool {
        self.simplified.as_deref() == Some("1")
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a "start-end" range string (1-based inclusive).
/// Returns `Some((start, end))`.
fn parse_range_1based(s: &str) -> Option<(i64, i64)> {
    let mut parts = s.splitn(2, '-');
    let start: i64 = parts.next()?.parse().ok()?;
    let end: i64 = parts.next()?.parse().ok()?;
    Some((start, end))
}

#[allow(dead_code)]
fn parse_range_0based(s: &str) -> Option<(i64, i64)> {
    parse_range_1based(s) // SnapGene uses 0-based for primers, so no conversion needed
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_range_1based() {
        assert_eq!(parse_range_1based("1-100"), Some((1, 100)));
        assert_eq!(parse_range_1based(""), None);
    }

    #[test]
    fn test_reverse_complement() {
        assert_eq!(crate::utils::reverse_complement("ATGC"), "GCAT");
    }
}
