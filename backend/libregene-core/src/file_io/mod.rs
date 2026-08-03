pub mod ab1;
pub mod color;
pub mod dna;
pub mod fasta;
pub mod gbk;

use std::io;
use std::path::Path;

use crate::models::ProjectData;

/// Parse a file, dispatching on extension.
pub fn parse_file(path: &Path) -> io::Result<ProjectData> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "gbk" | "gb" | "genbank" => gbk::parse_gbk(path),
        "dna" => dna::parse_dna(path),
        "fasta" | "fa" | "fna" => fasta::parse_fasta(path),
        "ab1" => ab1::parse_ab1(path),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported file extension: .{}", other),
        )),
    }
}
