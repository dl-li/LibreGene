//! SnapGene `.dna` primer XML parsing helpers.
//!
//! Re-exports the SnapGene component types from file_io::dna and provides
//! helper functions for the primer engine.

use crate::models::Primer;

/// Extract primer information from a SnapGene primer XML element.
/// This is a thin wrapper that delegates to the file_io layer.
pub fn parse_dna_primers(xml: &str, full_seq: &str) -> Vec<Primer> {
    crate::file_io::dna::parse_dna_primers(xml, full_seq)
}
