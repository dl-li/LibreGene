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
///
/// This delegates to the shared [`crate::file_io::gbk::primer_from_qualifier_values`]
/// helper to avoid duplicating Primer construction logic.
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
    let primer_seq = qualifiers
        .get("geneie_primer_seq")
        .map(|s| s.as_str())
        .unwrap_or("");

    Some(crate::file_io::gbk::primer_from_qualifier_values(
        label, primer_id, ptype, color, primer_seq,
    ))
}

// ---------------------------------------------------------------------------
// Serializer
// ---------------------------------------------------------------------------

/// Serialize all primers in a project to [`PrimerGbEntry`] records suitable
/// for the GenBank writer.
///
/// Delegates to [`crate::file_io::gbk::build_primer_qualifier_pairs`] to
/// share qualifier-building logic with the file_io layer.
pub fn serialize_primers_gbk(project: &ProjectData) -> Vec<PrimerGbEntry> {
    project
        .primers
        .iter()
        .map(|p| {
            let (match_start, match_end, qualifiers) =
                crate::file_io::gbk::build_primer_qualifier_pairs(p);
            PrimerGbEntry {
                match_start,
                match_end,
                qualifiers,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PrimerBindingSite;
    use crate::models::AlignmentRenderData;

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
                binding_sites: vec![PrimerBindingSite {
                    primer_id: "P1".to_string(),
                    strand: 1,
                    template_start: 10,
                    template_end: 19,
                    tm: 32.0,
                    gc_content: 0.5,
                    match_score: 20,
                    has_3_prime_mismatch: false,
                    five_prime_tail: String::new(),
                    three_prime_tail: String::new(),
                    alignment: AlignmentRenderData {
                        display_sequence: "CGTACGCTA".to_string(),
                        ..Default::default()
                    },
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
