//! GenBank primer parser and serializer.
//!
//! Simplified: only reads primer name/id + sequence from qualifiers.
//! Binding sites are always recomputed by the alignment engine.

use std::collections::HashMap;

use crate::models::{Primer, ProjectData};

// ---------------------------------------------------------------------------
// DB entry for the GBK writer
// ---------------------------------------------------------------------------

/// Opaque representation of a primer to be written as a `primer_bind`
/// feature by the GenBank serializer.
pub struct PrimerGbEntry {
    pub match_start: i64,
    pub match_end: i64,
    /// (key, value) pairs.
    pub qualifiers: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a single `primer_bind` feature from GenBank qualifiers.
///
/// Only extracts `label`, `geneie_primer_id`, `geneie_primer_type`,
/// `geneie_primer_seq`, and `geneie_color`. All binding site
/// information is recomputed by the alignment engine — stored values
/// are ignored.
pub fn parse_gbk_feature(
    qualifiers: &HashMap<String, String>,
    _start: i64,
    _end: i64,
    _seq: &str,
) -> Option<Primer> {
    let label = qualifiers
        .get("label")
        .map(|s| s.as_str())
        .unwrap_or("unknown");
    let primer_id = qualifiers
        .get("geneie_primer_id")
        .map(|s| s.as_str())
        .unwrap_or(label);
    let ptype = qualifiers
        .get("geneie_primer_type")
        .map(|s| s.as_str())
        .unwrap_or("fwd");
    let color = qualifiers
        .get("geneie_color")
        .map(|s| s.as_str())
        .unwrap_or("#166534");

    // Read primer_seq from qualifier if present (otherwise empty — caller
    // will need to set it). For SnapGene GBK, the sequence is extracted
    // from the note field by the file_io layer before calling here.
    let primer_seq = qualifiers
        .get("geneie_primer_seq")
        .map(|s| s.as_str())
        .unwrap_or("")
        .to_string();

    Some(Primer {
        id: primer_id.to_string(),
        name: label.to_string(),
        r#type: ptype.to_string(),
        primer_seq,
        color: color.to_string(),
        binding_sites: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Serializer
// ---------------------------------------------------------------------------

/// Serialize all primers in a project to [`PrimerGbEntry`] records suitable
/// for the GenBank writer.
pub fn serialize_primers_gbk(project: &ProjectData) -> Vec<PrimerGbEntry> {
    project
        .primers
        .iter()
        .map(|p| {
            let mut qualifiers: Vec<(String, String)> = Vec::new();

            qualifiers.push(("label".to_string(), p.name.clone()));
            qualifiers.push(("geneie_primer_id".to_string(), p.id.clone()));
            qualifiers.push(("geneie_primer_type".to_string(), p.r#type.clone()));
            qualifiers.push(("geneie_primer_seq".to_string(), p.primer_seq.clone()));
            qualifiers.push(("geneie_color".to_string(), p.color.clone()));

            if !p.binding_sites.is_empty() {
                let parts: Vec<String> = p
                    .binding_sites
                    .iter()
                    .map(|bs| format!("{},{},{:.1}", bs.match_start, bs.match_end, bs.tm))
                    .collect();
                qualifiers.push(("geneie_bindings".to_string(), parts.join(";")));
            }

            // match_start/end from best binding site, or default.
            let best = p.binding_sites.first();
            PrimerGbEntry {
                match_start: best.map(|b| b.match_start).unwrap_or(0),
                match_end: best.map(|b| b.match_end).unwrap_or(0),
                qualifiers,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::BindingSite;

    fn qualifiers_from_pairs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_parse_basic() {
        let qual = qualifiers_from_pairs(&[
            ("label", "M13F(-47)"),
            ("geneie_primer_id", "M13F(-47)"),
            ("geneie_primer_type", "fwd"),
            ("geneie_primer_seq", "CGCCAGGGTTTTCCCAGTCACGAC"),
        ]);
        let primer = parse_gbk_feature(&qual, 0, 0, "").unwrap();
        assert_eq!(primer.name, "M13F(-47)");
        assert_eq!(primer.r#type, "fwd");
        assert_eq!(primer.primer_seq, "CGCCAGGGTTTTCCCAGTCACGAC");
        assert!(primer.binding_sites.is_empty());
    }

    #[test]
    fn test_parse_minimal() {
        let qual = qualifiers_from_pairs(&[("label", "TestPrimer")]);
        let primer = parse_gbk_feature(&qual, 10, 20, "").unwrap();
        assert_eq!(primer.name, "TestPrimer");
        assert_eq!(primer.id, "TestPrimer"); // falls back to label
        assert_eq!(primer.r#type, "fwd"); // default
        assert_eq!(primer.primer_seq, "");
    }

    #[test]
    fn test_serialize_basic() {
        let project = ProjectData {
            primers: vec![Primer {
                id: "P1".to_string(),
                name: "Primer1".to_string(),
                r#type: "fwd".to_string(),
                primer_seq: "AAAACGTACGCTAG".to_string(),
                color: "#166534".to_string(),
                binding_sites: vec![BindingSite {
                    match_start: 10,
                    match_end: 19,
                    tm: 32.0,
                    ..Default::default()
                }],
            }],
            ..Default::default()
        };

        let entries = serialize_primers_gbk(&project);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.match_start, 10);
        assert_eq!(e.match_end, 19);

        let qual_map: HashMap<&str, &str> = e
            .qualifiers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert_eq!(qual_map.get("label"), Some(&"Primer1"));
        assert_eq!(qual_map.get("geneie_primer_id"), Some(&"P1"));
        assert_eq!(qual_map.get("geneie_primer_seq"), Some(&"AAAACGTACGCTAG"));
        assert_eq!(qual_map.get("geneie_bindings"), Some(&"10,19,32.0"));
    }
}
