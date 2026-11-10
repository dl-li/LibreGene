//! FASTA parser — extracts the first sequence record.

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use crate::models::{MsaRow, ProjectData};

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

fn validate_ascii_sequence(sequence: &str, name: &str) -> io::Result<()> {
    if let Some(c) = sequence
        .chars()
        .find(|c| !c.is_ascii_alphabetic() && *c != '-' && *c != '*' && *c != '?')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid character '{}' in sequence '{}'", c, name),
        ));
    }
    Ok(())
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

        if let Some(header) = trimmed.strip_prefix('>') {
            let name = header.split_whitespace().next().unwrap_or("").to_string();
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
        validate_ascii_sequence(&record.sequence, &record.name)?;
        record.length = record.sequence.len() as i64;
    }

    Ok(records)
}

pub fn is_msa_fasta_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.ends_with(".msa.fa") || name.ends_with(".msa.fasta")
}

pub fn detect_msa_molecule_type(rows: &[MsaRow]) -> String {
    let mut has_u = false;
    let mut has_t = false;
    for row in rows {
        for c in row.sequence.chars().map(|c| c.to_ascii_uppercase()) {
            if !c.is_ascii_alphabetic() && c != '*' && c != '?' {
                continue;
            }
            if !"ACGTURYSWKMBDHVN".contains(c) {
                return "protein".to_string();
            }
            has_u |= c == 'U';
            has_t |= c == 'T';
        }
    }
    if has_u && !has_t {
        "rna".to_string()
    } else {
        "dna".to_string()
    }
}

pub fn parse_msa_fasta(path: &Path) -> io::Result<ProjectData> {
    super::ensure_within_size_limit(path)?;
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut rows: Vec<MsaRow> = Vec::new();
    let mut aligned_rows: Vec<String> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if let Some(header) = trimmed.strip_prefix('>') {
            let header = header.trim();
            let split = header.find(char::is_whitespace);
            let (name, description) = match split {
                Some(i) => (&header[..i], header[i..].trim()),
                None => (header, ""),
            };
            let index = rows.len();
            rows.push(MsaRow {
                id: format!("row-{}", index + 1),
                name: if name.is_empty() {
                    format!("Sequence {}", index + 1)
                } else {
                    name.to_string()
                },
                description: description.to_string(),
                ..Default::default()
            });
            aligned_rows.push(String::new());
            continue;
        }
        if let Some(sequence) = aligned_rows.last_mut() {
            sequence.extend(
                trimmed
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .map(|c| c.to_ascii_uppercase()),
            );
        }
    }
    if rows.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "MSA FASTA contains no sequence records",
        ));
    }

    let aligned = aligned_rows.iter().all(|s| !s.is_empty())
        && aligned_rows
            .first()
            .is_some_and(|first| aligned_rows.iter().all(|s| s.len() == first.len()));
    for (row, aligned_sequence) in rows.iter_mut().zip(aligned_rows) {
        validate_ascii_sequence(&aligned_sequence, &row.name)?;
        row.sequence = aligned_sequence.replace('-', "");
        if aligned {
            row.aligned_sequence = aligned_sequence;
        }
    }
    let molecule_type = detect_msa_molecule_type(&rows);
    if molecule_type == "rna" {
        for row in &mut rows {
            row.sequence = row.sequence.replace('T', "U");
            row.aligned_sequence = row.aligned_sequence.replace('T', "U");
        }
    }
    let length = if aligned {
        rows.first().map(|r| r.aligned_sequence.len()).unwrap_or(0)
    } else {
        rows.iter().map(|r| r.sequence.len()).max().unwrap_or(0)
    };
    let mut name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("MSA")
        .to_string();
    if name.to_ascii_lowercase().ends_with(".msa") {
        name.truncate(name.len() - 4);
    }
    Ok(ProjectData {
        name,
        topology: "linear".to_string(),
        molecule_type,
        project_kind: "msa".to_string(),
        msa_rows: rows,
        msa_aligned: aligned,
        length: length as i64,
        ..Default::default()
    })
}

pub fn msa_fasta_text(project: &ProjectData, unaligned: bool) -> io::Result<String> {
    if project.msa_rows.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "MSA has no sequences",
        ));
    }
    let aligned = !unaligned && project.msa_aligned;
    let sequences: Vec<&str> = project
        .msa_rows
        .iter()
        .map(|row| {
            if aligned {
                row.aligned_sequence.as_str()
            } else {
                row.sequence.as_str()
            }
        })
        .collect();
    if aligned
        && sequences
            .first()
            .is_some_and(|first| sequences.iter().any(|s| s.len() != first.len()))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Aligned MSA rows must all have the same length",
        ));
    }

    let mut out = String::new();
    for (row, sequence) in project.msa_rows.iter().zip(sequences) {
        out.push('>');
        out.push_str(if row.name.trim().is_empty() {
            &row.id
        } else {
            row.name.trim()
        });
        if !row.description.trim().is_empty() {
            out.push(' ');
            out.push_str(row.description.trim());
        }
        out.push('\n');
        let chars: Vec<char> = sequence.chars().collect();
        for chunk in chars.chunks(80) {
            out.extend(chunk);
            out.push('\n');
        }
    }
    Ok(out)
}

pub fn write_msa_fasta(project: &ProjectData, path: &Path, unaligned: bool) -> io::Result<()> {
    let text = msa_fasta_text(project, unaligned)?;
    super::write_atomic(path, |file| {
        use std::io::Write;
        file.write_all(text.as_bytes())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "libregene_fasta_test_{}_{}",
            std::process::id(),
            name
        ));
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
        let path = write_temp("multi.fa", ">a\nACGT\n\n>b desc here\nttgg\ncc\n>\nGG\n");
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

    #[test]
    fn rejects_non_ascii_sequences_before_coordinate_use() {
        let path = write_temp("unicode.fa", ">bad\nAC😀GT\n");
        let error = parse_fasta_all_with_molecule_type(&path, "dna").unwrap_err();
        assert!(error.to_string().contains("Invalid character"));
        std::fs::remove_file(&path).ok();

        let path = write_temp("unicode.msa.fa", ">bad\nAC😀GT\n");
        let error = parse_msa_fasta(&path).unwrap_err();
        assert!(error.to_string().contains("Invalid character"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn msa_roundtrip_preserves_gaps_and_descriptions() {
        let path = write_temp("roundtrip.msa.fa", "");
        let project = ProjectData {
            project_kind: "msa".to_string(),
            molecule_type: "dna".to_string(),
            msa_aligned: true,
            msa_rows: vec![
                MsaRow {
                    id: "stable-a".to_string(),
                    name: "a".to_string(),
                    description: "first record".to_string(),
                    sequence: "ACGT".to_string(),
                    aligned_sequence: "AC-GT".to_string(),
                },
                MsaRow {
                    id: "stable-b".to_string(),
                    name: "a".to_string(),
                    sequence: "AGT".to_string(),
                    aligned_sequence: "A--GT".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        write_msa_fasta(&project, &path, false).unwrap();
        let parsed = parse_msa_fasta(&path).unwrap();
        assert_eq!(parsed.project_kind, "msa");
        assert!(parsed.msa_aligned);
        assert_eq!(parsed.msa_rows[0].description, "first record");
        assert_eq!(parsed.msa_rows[0].sequence, "ACGT");
        assert_eq!(parsed.msa_rows[0].aligned_sequence, "AC-GT");
        assert_eq!(parsed.msa_rows[1].name, "a");
        let unaligned = msa_fasta_text(&parsed, true).unwrap();
        assert!(!unaligned.contains('-'));
        assert!(unaligned.contains(">a first record\nACGT\n"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn msa_detects_protein_and_unequal_rows_as_unaligned() {
        let path = write_temp("protein.msa.fa", ">p1\nMVLIP\n>p2\nMVL\n");
        let parsed = parse_msa_fasta(&path).unwrap();
        assert_eq!(parsed.molecule_type, "protein");
        assert!(!parsed.msa_aligned);
        assert!(parsed
            .msa_rows
            .iter()
            .all(|r| r.aligned_sequence.is_empty()));
        std::fs::remove_file(&path).ok();
    }
}
