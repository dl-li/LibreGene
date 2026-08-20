//! Coordinate conversion helpers for template positions, feature-relative
//! offsets, and CDS amino-acid positions.
//!
//! All public helpers use the same 5'→3' iteration convention as
//! [`crate::translate::translate_feature`]: plus-strand features read segments
//! in storage order, ascending; minus-strand features read segments in reverse
//! order, descending, and complement each base.

use crate::models::Feature;
use crate::translate::translate_codon;
use crate::utils::complement_char;

/// Hit metadata for a feature that contains a template position.
#[derive(Debug, Clone)]
pub struct FeatureHit {
    pub feature_id: String,
    pub name: String,
    pub ftype: String,
    pub strand: String,
    /// 1-based offset along the feature's own 5'→3' direction.
    pub offset: i64,
    /// Feature length in bases (sum of segment spans).
    pub length: i64,
}

/// Translation metadata for a CDS/mRNA feature that contains a template
/// position.
#[derive(Debug, Clone)]
pub struct TranslationHit {
    pub feature_id: String,
    pub name: String,
    pub strand: String,
    /// 1-based codon index within the CDS.
    pub codon_index: usize,
    /// 1-based amino-acid position including the initiator Met.
    pub aa_position_1_based: usize,
    /// 1-based amino-acid position excluding the initiator Met (None for Met).
    pub aa_position_excluding_met: Option<usize>,
    /// Codon on the coding strand (3 bases).
    pub codon: String,
    /// Single-letter amino acid.
    pub amino_acid: char,
    /// Which base of the codon this position is (1, 2, or 3).
    pub codon_base_index: u8,
}

/// Return the template positions covered by `f` in biological 5'→3' order.
pub fn positions_5to3(f: &Feature) -> Vec<i64> {
    let segs: Vec<(i64, i64)> = if f.segments.is_empty() {
        vec![(f.start, f.end)]
    } else {
        f.segments.iter().map(|s| (s.start, s.end)).collect()
    };
    let mut out = Vec::new();
    if f.strand == "-" {
        for (s, e) in segs.iter().rev() {
            out.extend((*s..=*e).rev());
        }
    } else {
        for (s, e) in &segs {
            out.extend(*s..=*e);
        }
    }
    out
}

/// Return every feature containing `pos` (0-based template coordinate) together
/// with the 1-based offset of `pos` along that feature's 5'→3' direction.
pub fn position_to_features(pos: i64, features: &[Feature]) -> Vec<FeatureHit> {
    features
        .iter()
        .filter(|f| {
            let spans: Vec<(i64, i64)> = if f.segments.is_empty() {
                vec![(f.start, f.end)]
            } else {
                f.segments.iter().map(|s| (s.start, s.end)).collect()
            };
            spans.iter().any(|&(s, e)| pos >= s && pos <= e)
        })
        .filter_map(|f| {
            let positions = positions_5to3(f);
            let offset = positions.iter().position(|&p| p == pos)? as i64 + 1;
            Some(FeatureHit {
                feature_id: f.id.clone(),
                name: f.name.clone(),
                ftype: f.ftype.clone(),
                strand: f.strand.clone(),
                offset,
                length: positions.len() as i64,
            })
        })
        .collect()
}

/// Return translation context for every CDS/mRNA feature containing `pos`.
pub fn position_to_translations(pos: i64, seq: &str, features: &[Feature]) -> Vec<TranslationHit> {
    let bytes = seq.as_bytes();
    features
        .iter()
        .filter(|f| f.ftype.eq_ignore_ascii_case("cds") || f.ftype.eq_ignore_ascii_case("mrna"))
        .filter_map(|f| {
            let positions = positions_5to3(f);
            // Feature coordinates come from the file and are not range-checked
            // at parse time; skip features whose coordinates fall outside the
            // sequence instead of indexing out of bounds.
            if positions.iter().any(|&p| p < 0 || p >= bytes.len() as i64) {
                return None;
            }
            let idx = positions.iter().position(|&p| p == pos)?;
            let codon_idx = idx / 3;
            if codon_idx * 3 + 2 >= positions.len() {
                return None;
            }
            let codon_positions = [positions[codon_idx * 3], positions[codon_idx * 3 + 1], positions[codon_idx * 3 + 2]];
            let mut codon_bytes = [0u8; 3];
            for (i, &p) in codon_positions.iter().enumerate() {
                let mut b = bytes[p as usize].to_ascii_uppercase();
                if f.strand == "-" {
                    b = complement_char(b as char) as u8;
                }
                codon_bytes[i] = b;
            }
            let codon = String::from_utf8_lossy(&codon_bytes).to_string();
            let amino_acid = translate_codon(codon_bytes[0], codon_bytes[1], codon_bytes[2]);
            let aa_pos = codon_idx + 1;
            Some(TranslationHit {
                feature_id: f.id.clone(),
                name: f.name.clone(),
                strand: f.strand.clone(),
                codon_index: aa_pos,
                aa_position_1_based: aa_pos,
                aa_position_excluding_met: (aa_pos > 1).then_some(aa_pos - 1),
                codon,
                amino_acid,
                codon_base_index: (idx % 3 + 1) as u8,
            })
        })
        .collect()
}

/// Convert a 1-based feature-relative offset to the corresponding 0-based
/// template position. Errors if the offset is out of bounds.
pub fn position_from_feature_offset(f: &Feature, offset1: i64) -> Result<i64, String> {
    let positions = positions_5to3(f);
    if offset1 < 1 || offset1 > positions.len() as i64 {
        return Err(format!(
            "offset {} out of bounds for feature '{}' (length {})",
            offset1,
            f.name,
            positions.len()
        ));
    }
    Ok(positions[offset1 as usize - 1])
}

/// Convert a 1-based amino-acid position within a CDS/mRNA feature to the three
/// template positions of that codon (5'→3' biological order), the coding-strand
/// codon, and the single-letter amino acid. Errors if the feature is not
/// translatable or the position is out of bounds.
pub fn codon_from_aa(
    f: &Feature,
    seq: &str,
    aa1: i64,
) -> Result<([i64; 3], String, char), String> {
    if !f.ftype.eq_ignore_ascii_case("cds") && !f.ftype.eq_ignore_ascii_case("mrna") {
        return Err(format!(
            "feature '{}' has type '{}'; codon lookup requires CDS or mRNA",
            f.name, f.ftype
        ));
    }
    let positions = positions_5to3(f);
    let total_aa = positions.len() / 3;
    if aa1 < 1 || aa1 > total_aa as i64 {
        return Err(format!(
            "amino-acid position {} out of bounds for feature '{}' ({} amino acids)",
            aa1, f.name, total_aa
        ));
    }
    let codon_idx = (aa1 as usize - 1) * 3;
    let codon_positions = [positions[codon_idx], positions[codon_idx + 1], positions[codon_idx + 2]];
    let bytes = seq.as_bytes();
    // Feature coordinates are file-derived and not range-checked at parse
    // time; reject instead of indexing out of bounds.
    if codon_positions.iter().any(|&p| p < 0 || p >= bytes.len() as i64) {
        return Err(format!(
            "feature '{}' coordinates fall outside the sequence (length {}); \
             the feature location is inconsistent with the loaded file",
            f.name,
            bytes.len()
        ));
    }
    let mut codon_bytes = [0u8; 3];
    for (i, &p) in codon_positions.iter().enumerate() {
        let mut b = bytes[p as usize].to_ascii_uppercase();
        if f.strand == "-" {
            b = complement_char(b as char) as u8;
        }
        codon_bytes[i] = b;
    }
    let codon = String::from_utf8_lossy(&codon_bytes).to_string();
    let aa = translate_codon(codon_bytes[0], codon_bytes[1], codon_bytes[2]);
    Ok((codon_positions, codon, aa))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Feature, Segment};

    fn cds(
        id: &str,
        strand: &str,
        segments: Vec<(i64, i64)>,
    ) -> Feature {
        let start = segments.iter().map(|s| s.0).min().unwrap();
        let end = segments.iter().map(|s| s.1).max().unwrap();
        Feature {
            id: id.into(),
            name: id.into(),
            start,
            end,
            color: "#000000".into(),
            ftype: "CDS".into(),
            segments: segments
                .into_iter()
                .map(|(start, end)| Segment {
                    start,
                    end,
                    color: None,
                })
                .collect(),
            strand: strand.into(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        }
    }

    #[test]
    fn positions_5to3_forward() {
        let f = cds("f", "+", vec![(0, 2), (5, 7)]);
        assert_eq!(positions_5to3(&f), vec![0, 1, 2, 5, 6, 7]);
    }

    /// Regression: feature coordinates come from the file and are not
    /// range-checked at parse time. A GBK whose feature location exceeds the
    /// ORIGIN length must not panic the coordinate lookups — it should be
    /// skipped (translations) or rejected (codon lookup).
    #[test]
    fn position_to_translations_skips_out_of_range_feature() {
        // Sequence is 10 bp; the CDS claims 50..60 — garbage but loadable.
        let seq = "ACGTACGTAC";
        let f = cds("evil", "+", vec![(50, 60)]);
        let hits = position_to_translations(50, seq, &[f]);
        assert!(hits.is_empty(), "out-of-range feature must be skipped, got {:?}", hits.len());
    }

    #[test]
    fn codon_from_aa_rejects_out_of_range_feature() {
        let seq = "ACGTACGTAC";
        let f = cds("evil", "+", vec![(50, 60)]);
        let res = codon_from_aa(&f, seq, 1);
        assert!(res.is_err(), "out-of-range feature must return Err, not panic");
        let msg = res.unwrap_err();
        assert!(
            msg.contains("outside the sequence"),
            "error should explain the mismatch: {}",
            msg
        );
    }

    #[test]
    fn positions_5to3_reverse_segmented() {
        // Minus strand: segments reversed, each descending.
        let f = cds("f", "-", vec![(0, 2), (5, 7)]);
        assert_eq!(positions_5to3(&f), vec![7, 6, 5, 2, 1, 0]);
    }

    #[test]
    fn position_to_features_offset_matches_5to3() {
        let f = cds("f", "+", vec![(0, 2), (5, 7)]);
        let hits = position_to_features(5, &[f]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].offset, 4);
        assert_eq!(hits[0].length, 6);
    }

    #[test]
    fn position_to_features_minus_strand_offset() {
        let f = cds("f", "-", vec![(0, 2), (5, 7)]);
        // Template pos 5 is the third base in 5'→3' order (7,6,5,...).
        let hits = position_to_features(5, &[f]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].offset, 3);
    }

    #[test]
    fn position_to_translation_forward() {
        // ATG GTA TAA -> M V *
        let seq = "ATGGTATAA";
        let f = cds("f", "+", vec![(0, 8)]);
        // Position 1 is the second base of the initiator codon ATG.
        let tr = position_to_translations(1, seq, &[f]);
        assert_eq!(tr.len(), 1);
        assert_eq!(tr[0].codon_index, 1);
        assert_eq!(tr[0].aa_position_1_based, 1);
        assert_eq!(tr[0].aa_position_excluding_met, None);
        assert_eq!(tr[0].codon, "ATG");
        assert_eq!(tr[0].amino_acid, 'M');
        assert_eq!(tr[0].codon_base_index, 2);
    }

    #[test]
    fn position_to_translation_minus_strand_segmented() {
        // Minus-strand CDS join(0..2,3..5) on "ATGAAATTTAAA" translates to FH.
        // Coding strand = rev-comp of segments read 5'→3': TTT CAT.
        let seq = "ATGAAATTTAAA";
        let f = cds("f", "-", vec![(0, 2), (3, 5)]);
        // Template pos 4 is the second base of the first coding codon (AAA -> TTT = F).
        let tr = position_to_translations(4, seq, &[f]);
        assert_eq!(tr.len(), 1);
        assert_eq!(tr[0].codon, "TTT");
        assert_eq!(tr[0].amino_acid, 'F');
        assert_eq!(tr[0].codon_base_index, 2);
        assert_eq!(tr[0].codon_index, 1);
        assert_eq!(tr[0].aa_position_1_based, 1);
        assert_eq!(tr[0].aa_position_excluding_met, None);
    }

    #[test]
    fn feature_offset_round_trip() {
        let f = cds("f", "-", vec![(0, 2), (5, 7)]);
        for offset1 in 1..=6 {
            let pos = position_from_feature_offset(&f, offset1).unwrap();
            let hits = position_to_features(pos, &[f.clone()]);
            assert_eq!(hits[0].offset, offset1);
        }
    }

    #[test]
    fn feature_offset_out_of_bounds() {
        let f = cds("f", "+", vec![(0, 2)]);
        let err = position_from_feature_offset(&f, 5).unwrap_err();
        assert!(err.contains("out of bounds"));
        assert!(err.contains("length 3"));
    }

    #[test]
    fn codon_from_aa_round_trip() {
        // ATG GTA TAA -> M V *
        let seq = "ATGGTATAA";
        let f = cds("f", "+", vec![(0, 8)]);
        let (positions, codon, aa) = codon_from_aa(&f, seq, 2).unwrap();
        assert_eq!(positions, [3, 4, 5]);
        assert_eq!(codon, "GTA");
        assert_eq!(aa, 'V');

        // Use the returned positions to look up the translation again.
        let tr = position_to_translations(positions[0], seq, &[f]);
        assert_eq!(tr[0].codon_index, 2);
        assert_eq!(tr[0].codon, "GTA");
        assert_eq!(tr[0].amino_acid, 'V');
    }

    #[test]
    fn codon_from_aa_out_of_bounds() {
        let seq = "ATGGTATAA";
        let f = cds("f", "+", vec![(0, 8)]);
        let err = codon_from_aa(&f, seq, 5).unwrap_err();
        assert!(err.contains("out of bounds"));
        assert!(err.contains("3 amino acids"));
    }

    #[test]
    fn codon_from_aa_rejects_non_cds() {
        let mut f = cds("f", "+", vec![(0, 8)]);
        f.ftype = "promoter".into();
        let err = codon_from_aa(&f, "ACGTACGTAC", 1).unwrap_err();
        assert!(err.contains("requires CDS or mRNA"));
    }

    #[test]
    fn non_cds_feature_has_no_translation() {
        let mut f = cds("f", "+", vec![(0, 8)]);
        f.ftype = "promoter".into();
        let tr = position_to_translations(1, "ACGTACGTAC", &[f]);
        assert!(tr.is_empty());
    }
}
