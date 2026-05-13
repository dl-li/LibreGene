//! FASTA parser — extracts the first sequence record.

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use crate::models::ProjectData;

/// Parse a FASTA file. Only the first sequence is read.
pub fn parse_fasta(path: &Path) -> io::Result<ProjectData> {
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
            sequence.push_str(&trimmed.to_uppercase());
        }
    }

    let length = sequence.len() as i64;

    Ok(ProjectData {
        sequence,
        length,
        topology: "linear".to_string(),
        ..Default::default()
    })
}
