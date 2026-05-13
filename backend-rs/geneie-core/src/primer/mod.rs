//! Primer engine — parsing, analysis, and serialisation for primer data.
//!
//! Handles:
//! - `.gbk` `primer_bind` features
//! - `.dna` primer XML blocks (SnapGene)
//! - Melting temperature computation (Wallace rule)
//! - Semi-global alignment and binding-site search

pub mod align;
pub mod tm;
pub mod gbk;
pub mod dna;

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
