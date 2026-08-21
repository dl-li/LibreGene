//! Primer engine — parsing, analysis, and serialisation for primer data.
//!
//! Handles:
//! - `.gbk` `primer_bind` features
//! - `.dna` primer XML blocks (SnapGene)
//! - Smith-Waterman alignment with 3' asymmetry (alignment)
//! - SantaLucia 2004 nearest-neighbour Tm (thermodynamics)
//! - IUPAC ambiguous base support (iupac)
//! - Compact render-data formatting (formatter)

pub mod align;
pub mod alignment;
pub mod design;
pub mod display;
pub mod dna;
pub mod formatter;
pub mod gbk;
pub mod iupac;
pub mod matcher;
pub mod screening;
pub mod thermodynamics;
pub mod tm;

use crate::models::ProjectData;

/// Recompute binding sites for all primers in the project.
pub fn recompute(project: &mut ProjectData) {
    // Primers bind double-stranded DNA only — single-strand molecules carry none.
    if !project.is_dna() {
        project.primers.clear();
        return;
    }
    if project.primers.is_empty() {
        return;
    }
    let updated = align::recompute_all_primers(
        &project.sequence,
        &project.topology,
        &project.primers,
    );
    project.primers = updated;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Primer;

    #[test]
    fn recompute_skips_non_dna_projects() {
        let mut project = ProjectData {
            sequence: "ACGT".repeat(10),
            topology: "circular".to_string(),
            molecule_type: "rna".to_string(),
            primers: vec![Primer {
                id: "p1".into(),
                name: "P1".into(),
                r#type: "fwd".into(),
                primer_seq: "ACGTACGTAC".into(),
                binding_sites: Vec::new(),
            }],
            ..Default::default()
        };
        recompute(&mut project);
        assert!(project.primers.is_empty(), "rna projects must not carry primers");
    }
}
