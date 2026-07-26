//! AB1 (ABIF) file parser — extracts the basecalled sequence only.
//!
//! ABIF layout: 4-byte magic "ABIF" + u16 version, then the 28-byte root
//! directory entry at offset 6. Directory entries are 28 bytes:
//! tag name (4), tag number (u32), element type (u16), element size (u16),
//! num elements (u32), data size (u32), data offset / inline value (u32),
//! data handle (u32). All integers big-endian. The basecalled sequence is
//! the "PBAS" entry (tag number 1 or 2, element type 2 = char).

use std::fs;
use std::io;
use std::path::Path;

use crate::models::ProjectData;

const ROOT_ENTRY_OFFSET: usize = 6;
const DIR_ENTRY_LEN: usize = 28;

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

fn u32_at(buf: &[u8], off: usize) -> io::Result<u32> {
    buf.get(off..off + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| invalid("truncated ABIF file"))
}

/// Extract the PBAS basecalled sequence from raw ABIF bytes.
pub(crate) fn extract_pbas_sequence(buf: &[u8]) -> io::Result<String> {
    if buf.len() < ROOT_ENTRY_OFFSET + DIR_ENTRY_LEN || &buf[0..4] != b"ABIF" {
        return Err(invalid("not an ABIF file (bad magic or too short)"));
    }

    let num_entries = u32_at(buf, ROOT_ENTRY_OFFSET + 12)? as usize;
    let entries_offset = u32_at(buf, ROOT_ENTRY_OFFSET + 20)? as usize;

    for i in 0..num_entries {
        let e = entries_offset + i * DIR_ENTRY_LEN;
        let entry = buf
            .get(e..e + DIR_ENTRY_LEN)
            .ok_or_else(|| invalid("directory entry out of bounds"))?;

        let tag_number = u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]);
        let elem_type = u16::from_be_bytes([entry[8], entry[9]]);
        if &entry[0..4] != b"PBAS" || (tag_number != 1 && tag_number != 2) || elem_type != 2 {
            continue;
        }

        let data_size = u32::from_be_bytes([entry[16], entry[17], entry[18], entry[19]]) as usize;
        let data_offset = u32::from_be_bytes([entry[20], entry[21], entry[22], entry[23]]) as usize;

        // Values of 4 bytes or fewer are stored inline in the offset field.
        let raw: Vec<u8> = if data_size <= 4 {
            entry[20..20 + data_size].to_vec()
        } else {
            buf.get(data_offset..data_offset + data_size)
                .ok_or_else(|| invalid("PBAS data out of bounds"))?
                .to_vec()
        };

        let seq: String = raw
            .iter()
            .filter(|&&b| b.is_ascii_alphabetic())
            .map(|&b| b.to_ascii_uppercase() as char)
            .collect();
        if seq.is_empty() {
            return Err(invalid("PBAS entry contains no sequence"));
        }
        return Ok(seq);
    }

    Err(invalid("no PBAS (basecalled sequence) entry found"))
}

/// Parse an AB1 file. Returns a project with just the called sequence.
pub fn parse_ab1(path: &Path) -> io::Result<ProjectData> {
    let data = fs::read(path)?;
    let sequence = extract_pbas_sequence(&data)?;
    let length = sequence.len() as i64;

    Ok(ProjectData {
        sequence,
        length,
        topology: "linear".to_string(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_entry(
        buf: &mut Vec<u8>,
        name: &[u8; 4],
        number: u32,
        elem_type: u16,
        elem_size: u16,
        num: u32,
        data_size: u32,
        data_offset: u32,
    ) {
        buf.extend_from_slice(name);
        buf.extend_from_slice(&number.to_be_bytes());
        buf.extend_from_slice(&elem_type.to_be_bytes());
        buf.extend_from_slice(&elem_size.to_be_bytes());
        buf.extend_from_slice(&num.to_be_bytes());
        buf.extend_from_slice(&data_size.to_be_bytes());
        buf.extend_from_slice(&data_offset.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes());
    }

    fn build_abif(seq: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"ABIF");
        buf.extend_from_slice(&101u16.to_be_bytes());

        let entries_offset = (ROOT_ENTRY_OFFSET + DIR_ENTRY_LEN) as u32;
        write_entry(&mut buf, b"tdir", 1, 1023, DIR_ENTRY_LEN as u16, 1, DIR_ENTRY_LEN as u32, entries_offset);

        let seq_offset = entries_offset + DIR_ENTRY_LEN as u32;
        write_entry(&mut buf, b"PBAS", 1, 2, 1, seq.len() as u32, seq.len() as u32, seq_offset);
        buf.extend_from_slice(seq);
        buf
    }

    #[test]
    fn test_parse_synthetic_abif() {
        let buf = build_abif(b"ACGTNacgt");
        assert_eq!(extract_pbas_sequence(&buf).unwrap(), "ACGTNACGT");
    }

    #[test]
    fn test_bad_magic() {
        assert!(extract_pbas_sequence(b"XXXX....").is_err());
        assert!(extract_pbas_sequence(b"AB").is_err());
    }

    #[test]
    fn test_no_pbas_entry() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"ABIF");
        buf.extend_from_slice(&101u16.to_be_bytes());
        let entries_offset = (ROOT_ENTRY_OFFSET + DIR_ENTRY_LEN) as u32;
        write_entry(&mut buf, b"tdir", 1, 1023, DIR_ENTRY_LEN as u16, 1, DIR_ENTRY_LEN as u32, entries_offset);
        write_entry(&mut buf, b"PCON", 1, 2, 1, 4, 4, entries_offset + DIR_ENTRY_LEN as u32);
        buf.extend_from_slice(b"ACGT");
        assert!(extract_pbas_sequence(&buf).is_err());
    }
}
