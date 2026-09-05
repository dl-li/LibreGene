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
    parse_fasta_all_with_molecule_type(path, molecule_type).map(|mut records| {
        if records.is_empty() {
            ProjectData {
                topology: "linear".to_string(),
                molecule_type: molecule_type.to_string(),
                ..Default::default()
            }
        } else {
            records.remove(0)
        }
    })
}

/// Parse ALL records of a multi-record FASTA file, one `ProjectData` per
/// `>` header. Record name is the first whitespace token of the header.
pub fn parse_fasta_all_with_molecule_type(
    path: &Path,
    molecule_type: &str,
) -> io::Result<Vec<ProjectData>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut records: Vec<ProjectData> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if trimmed.starts_with('>') {
            let name = trimmed[1..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            records.push(ProjectData {
                name,
                topology: "linear".to_string(),
                molecule_type: molecule_type.to_string(),
                ..Default::default()
            });
            continue;
        }

        if let Some(record) = records.last_mut() {
            record
                .sequence
                .extend(trimmed.chars().flat_map(|c| c.to_uppercase()));
        }
    }

    for record in &mut records {
        record.length = record.sequence.len() as i64;
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, content: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("libregene_fasta_test_{}_{}", std::process::id(), name));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn single_record() {
        let path = write_temp("single.fa", ">seq1 some description\nACGT\nACGT\n");
        let records = parse_fasta_all_with_molecule_type(&path, "dna").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "seq1");
        assert_eq!(records[0].sequence, "ACGTACGT");
        assert_eq!(records[0].length, 8);
        // First-record parser agrees.
        let first = parse_fasta(&path).unwrap();
        assert_eq!(first.name, "seq1");
        assert_eq!(first.sequence, "ACGTACGT");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn multi_record() {
        let path = write_temp(
            "multi.fa",
            ">a\nACGT\n\n>b desc here\nttgg\ncc\n>\nGG\n",
        );
        let records = parse_fasta_all_with_molecule_type(&path, "dna").unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].name, "a");
        assert_eq!(records[0].sequence, "ACGT");
        assert_eq!(records[1].name, "b");
        assert_eq!(records[1].sequence, "TTGGCC");
        assert_eq!(records[2].name, "");
        assert_eq!(records[2].sequence, "GG");
        let first = parse_fasta(&path).unwrap();
        assert_eq!(first.name, "a");
        assert_eq!(first.sequence, "ACGT");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn protein_multi_record() {
        let path = write_temp("multi.faa", ">p1\nMKV\n>p2\nGLA*\n");
        let records = parse_fasta_all_with_molecule_type(&path, "protein").unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.molecule_type == "protein"));
        assert_eq!(records[1].sequence, "GLA*");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn no_records() {
        let path = write_temp("empty.fa", "\n\nno header here\n");
        let records = parse_fasta_all_with_molecule_type(&path, "dna").unwrap();
        assert!(records.is_empty());
        let first = parse_fasta(&path).unwrap();
        assert_eq!(first.sequence, "");
        std::fs::remove_file(&path).ok();
    }
}
