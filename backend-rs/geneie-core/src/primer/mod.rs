//! Primer engine — parsing, analysis, and serialisation for primer data.
//!
//! Handles:
//! - `.gbk` `primer_bind` features
//! - `.dna` primer XML blocks (SnapGene)
//! - Smith-Waterman alignment with 3' asymmetry (alignment)
//! - SantaLucia 1998 nearest-neighbour Tm (thermodynamics)
//! - Compact render-data formatting (formatter)

pub mod align;
pub mod alignment;
pub mod dna;
pub mod formatter;
pub mod gbk;
pub mod thermodynamics;
pub mod tm;

use crate::models::ProjectData;
use crate::project::ProjectManager;

/// Wire the primer engine into the project manager.
pub fn init(_pm: &mut ProjectManager) {}

/// Recompute binding sites for all primers in the project.
pub fn recompute(project: &mut ProjectData) {
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
