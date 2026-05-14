//! AB1 (ABI) chromatogram file parser — stub.
//!
//! The AB1 format contains trace data + basecalls. For now we only extract
//! the called sequence.

use std::fs;
use std::io;
use std::path::Path;

use crate::models::ProjectData;

/// Parse an AB1 file. Returns a project with just the called sequence.
pub fn parse_ab1(path: &Path) -> io::Result<ProjectData> {
    let _data = fs::read(path)?;

    // AB1 is a binary format with basecalls at known offsets.
    // Full parser is not yet implemented — return an error.
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AB1 file parsing is not yet implemented",
    ))
}
