//! Recompute the `/translation` qualifier of translatable features (CDS, mRNA)
//! from the template sequence, mirroring the frontend's `buildCDSData`.

use crate::models::{Feature, ProjectData, Segment};

/// Standard genetic code, codons ordered TCAG per position (1-letter codes).
pub const GENETIC_CODE: [char; 64] = [
    'F', 'F', 'L', 'L', 'S', 'S', 'S', 'S', 'Y', 'Y', '*', '*', 'C', 'C', '*', 'W',
    'L', 'L', 'L', 'L', 'P', 'P', 'P', 'P', 'H', 'H', 'Q', 'Q', 'R', 'R', 'R', 'R',
    'I', 'I', 'I', 'M', 'T', 'T', 'T', 'T', 'N', 'N', 'K', 'K', 'S', 'S', 'R', 'R',
    'V', 'V', 'V', 'V', 'A', 'A', 'A', 'A', 'D', 'D', 'E', 'E', 'G', 'G', 'G', 'G',
];

pub fn codon_index(b: u8) -> Option<usize> {
    match b {
        b'T' | b't' => Some(0),
        b'C' | b'c' => Some(1),
        b'A' | b'a' => Some(2),
        b'G' | b'g' => Some(3),
        _ => None,
    }
}

/// Translate a nucleotide byte string to a 1-letter amino-acid byte string,
/// dropping a trailing incomplete codon (`*` stop, `?` ambiguous).
pub fn translate_nt(seq: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(seq.len() / 3);
    for chunk in seq.chunks(3) {
        if chunk.len() < 3 {
            break;
        }
        out.push(translate_codon(chunk[0], chunk[1], chunk[2]) as u8);
    }
    out
}

fn translate_codon(a: u8, b: u8, c: u8) -> char {
    let (Some(i0), Some(i1), Some(i2)) = (codon_index(a), codon_index(b), codon_index(c)) else {
        return '?';
    };
    GENETIC_CODE[i0 * 16 + i1 * 4 + i2]
}

/// Translate a feature's coding sequence to a 1-letter amino-acid string.
/// Coding bases are collected 5'→3' (reverse-complemented for minus-strand
/// features); a trailing incomplete codon is dropped, exactly like the
/// frontend `buildCDSData`.
pub fn translate_feature(seq: &str, f: &Feature) -> String {
    let segs: Vec<Segment> = if f.segments.is_empty() {
        vec![Segment {
            start: f.start,
            end: f.end,
            color: None,
        }]
    } else {
        f.segments.clone()
    };
    let bytes = seq.as_bytes();
    let n = bytes.len() as i64;

    let mut coding: Vec<u8> = Vec::new();
    if f.strand == "-" {
        for seg in segs.iter().rev() {
            for j in (seg.start..=seg.end).rev() {
                if j >= 0 && j < n {
                    coding.push(crate::utils::complement_char(bytes[j as usize] as char) as u8);
                }
            }
        }
    } else {
        for seg in &segs {
            for j in seg.start..=seg.end {
                if j >= 0 && j < n {
                    coding.push(bytes[j as usize]);
                }
            }
        }
    }

    let mut out = String::with_capacity(coding.len() / 3 + 1);
    for chunk in coding.chunks(3) {
        if chunk.len() < 3 {
            break;
        }
        out.push(translate_codon(chunk[0], chunk[1], chunk[2]));
    }
    out
}

/// Recompute `translation` for every translatable feature (CDS, mRNA) from the
/// current sequence. Non-translatable features keep their stored value.
/// Translation is a DNA concept — non-DNA projects keep stored values untouched.
pub fn refresh_feature_translations(project: &mut ProjectData) {
    if !project.is_dna() {
        return;
    }
    for f in project.features.iter_mut() {
        if f.ftype == "CDS" || f.ftype == "mRNA" {
            f.translation = translate_feature(&project.sequence, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feat(id: &str, start: i64, end: i64, strand: &str, segments: Vec<(i64, i64)>) -> Feature {
        Feature {
            id: id.to_string(),
            name: id.to_string(),
            start,
            end,
            color: "#000000".to_string(),
            ftype: "CDS".to_string(),
            segments: segments
                .into_iter()
                .map(|(start, end)| Segment {
                    start,
                    end,
                    color: None,
                })
                .collect(),
            strand: strand.to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        }
    }

    #[test]
    fn translate_forward_feature() {
        let f = feat("gfp", 0, 11, "+", vec![]);
        assert_eq!(translate_feature("ATGGTGAGCAAATAA", &f), "MVSK");
    }

    #[test]
    fn translate_reverse_feature() {
        // Coding strand is reverse complement of template[0..=11].
        // Reverse complement of ATGAAATTTAAA is TTTAAATTTCAT.
        let f = feat("amp", 0, 11, "-", vec![]);
        assert_eq!(translate_feature("ATGAAATTTAAA", &f), "FKFH");
    }

    #[test]
    fn translate_segmented_feature_joins_in_5prime_order() {
        // segments listed in template order but read 5'→3' on the minus strand:
        // seg (3,5) bases A,A,A -> TTT, then seg (0,2) bases G,T,A -> C,A,T.
        let f = feat("m", 0, 5, "-", vec![(0, 2), (3, 5)]);
        assert_eq!(translate_feature("ATGAAATTTAAA", &f), "FH");
    }

    #[test]
    fn translate_drops_incomplete_codon() {
        let f = feat("x", 0, 7, "+", vec![]);
        // 8 bases -> 2 full codons + 2 dangling
        assert_eq!(translate_feature("ATGGTGAGC", &f), "MV");
    }

    #[test]
    fn translate_ambiguous_codon_uses_question_mark() {
        let f = feat("x", 0, 5, "+", vec![]);
        assert_eq!(translate_feature("ATGNNN", &f), "M?");
    }

    #[test]
    fn refresh_updates_only_translatable_features() {
        let mut p = ProjectData {
            name: "t".to_string(),
            definition: String::new(),
            keywords: String::new(),
            lab_host: String::new(),
            sequence: "ATGGTGAGC".to_string(),
            length: 9,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![
                Feature {
                    ftype: "CDS".to_string(),
                    translation: "stale".to_string(),
                    ..feat("cds", 0, 8, "+", vec![])
                },
                Feature {
                    ftype: "mRNA".to_string(),
                    translation: "stale".to_string(),
                    ..feat("mrna", 0, 8, "+", vec![])
                },
                Feature {
                    ftype: "promoter".to_string(),
                    translation: "keep".to_string(),
                    ..feat("prom", 0, 8, "+", vec![])
                },
            ],
            primers: Vec::new(),
            alignments: Vec::new(),
            enzymes: Vec::new(),
            methylation_systems: Vec::new(),
            methylation_overlap: 0,
            roi: None,
        };
        refresh_feature_translations(&mut p);
        assert_eq!(p.features[0].translation, "MVS");
        assert_eq!(p.features[1].translation, "MVS");
        assert_eq!(p.features[2].translation, "keep");
    }

    #[test]
    fn refresh_skips_non_dna_projects() {
        let mut p = ProjectData {
            name: "p".to_string(),
            definition: String::new(),
            keywords: String::new(),
            lab_host: String::new(),
            sequence: "MVS".to_string(),
            length: 3,
            topology: "linear".to_string(),
            molecule_type: "protein".to_string(),
            features: vec![Feature {
                ftype: "CDS".to_string(),
                translation: "keep".to_string(),
                ..feat("cds", 0, 2, "+", vec![])
            }],
            primers: Vec::new(),
            alignments: Vec::new(),
            enzymes: Vec::new(),
            methylation_systems: Vec::new(),
            methylation_overlap: 0,
            roi: None,
        };
        refresh_feature_translations(&mut p);
        assert_eq!(p.features[0].translation, "keep");
    }
}
