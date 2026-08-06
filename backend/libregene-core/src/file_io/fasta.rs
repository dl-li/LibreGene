//! FASTA parser — extracts the first sequence record.

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use crate::models::ProjectData;

/// Parse a FASTA file. Only the first sequence is read.
pub fn parse_fasta(path: &Path) -> io::Result<ProjectData> {
    parse_fasta_with_molecule_type(path, "dna")
}

/// Parse a FASTA file with an explicit molecule type — `.faa` protein FASTA is
/// opened as a protein project without alphabet sniffing.
pub fn parse_fasta_with_molecule_type(path: &Path, molecule_type: &str) -> io::Result<ProjectData> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut sequence = String::new();
    let mut in_seq = false;
    let mut _name = String::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if trimmed.starts_with('>') {
            if in_seq {
                // Only take first sequence.
                break;
            }
            in_seq = true;
            _name = trimmed[1..].trim().to_string();
            continue;
        }

        if in_seq {
            sequence.extend(trimmed.chars().flat_map(|c| c.to_uppercase()));
        }
    }

    let length = sequence.len() as i64;

    Ok(ProjectData {
        sequence,
        length,
        topology: "linear".to_string(),
        molecule_type: molecule_type.to_string(),
        ..Default::default()
    })
}
