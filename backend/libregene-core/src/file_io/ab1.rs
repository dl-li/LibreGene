//! AB1 (ABIF) file parser — extracts the basecalled sequence and the raw
//! chromatogram channels.
//!
//! Chromatogram extraction (PLOC peaks + trace channels, with the DATA.1-4
//! / FWO_ raw-channel fallback) is ported from GenePad
//! (https://github.com/GenePad), provided by the GenePad team /
//! https://github.com/Masterchiefm.
//!
//! ABIF layout: 4-byte magic "ABIF" + u16 version, then the 28-byte root
//! directory entry at offset 6. Directory entries are 28 bytes:
//! tag name (4), tag number (u32), element type (u16), element size (u16),
//! num elements (u32), data size (u32), data offset / inline value (u32),
//! data handle (u32). All integers big-endian. The basecalled sequence is
//! the "PBAS" entry (tag number 1 or 2, element type 2 = char), peak
//! positions are "PLOC" (i16 trace sample per base) and the processed trace
//! channels are DATA.9–12 (G/A/T/C on modern instruments; older files only
//! carry DATA.1–4 in the FWO_ filter-wheel order, default "GATC").

use std::fs;
use std::io;
use std::path::Path;

use crate::models::{Chromatogram, ProjectData};

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
        return Err(invalid("not an AB1 file (bad magic or too short)"));
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

struct TagEntry {
    tag_number: u32,
    data_size: usize,
    data_offset: usize,
}

fn tag_entry(buf: &[u8], entry: &[u8]) -> Option<TagEntry> {
    let data_size = u32::from_be_bytes([entry[16], entry[17], entry[18], entry[19]]) as usize;
    let data_offset = u32::from_be_bytes([entry[20], entry[21], entry[22], entry[23]]) as usize;
    if data_size > 4 && data_offset.checked_add(data_size)? > buf.len() {
        return None;
    }
    Some(TagEntry {
        tag_number: u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]),
        data_size,
        data_offset,
    })
}

fn read_short_array(buf: &[u8], tag: &TagEntry) -> Vec<i16> {
    let count = tag.data_size / 2;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let off = tag.data_offset + i * 2;
        let Some(b) = buf.get(off..off + 2) else {
            break;
        };
        out.push(i16::from_be_bytes([b[0], b[1]]));
    }
    out
}

/// Extract the chromatogram (PLOC peaks + four trace channels). Prefers the
/// processed DATA.9–12 channels; falls back to raw DATA.1–4 mapped through
/// the FWO_ filter-wheel order (default GATC) for legacy files.
pub fn extract_chromatogram(buf: &[u8]) -> io::Result<Chromatogram> {
    if buf.len() < ROOT_ENTRY_OFFSET + DIR_ENTRY_LEN || &buf[0..4] != b"ABIF" {
        return Err(invalid("not an AB1 file (bad magic or too short)"));
    }

    let num_entries = u32_at(buf, ROOT_ENTRY_OFFSET + 12)? as usize;
    let entries_offset = u32_at(buf, ROOT_ENTRY_OFFSET + 20)? as usize;

    let mut peaks: Option<Vec<i16>> = None;
    let mut processed: [Option<Vec<i16>>; 4] = [None, None, None, None]; // DATA.9..12
    let mut raw: [Option<Vec<i16>>; 4] = [None, None, None, None]; // DATA.1..4
    let mut fwo = *b"GATC";

    for i in 0..num_entries {
        let e = entries_offset + i * DIR_ENTRY_LEN;
        let Some(entry) = buf.get(e..e + DIR_ENTRY_LEN) else {
            break;
        };
        let Some(tag) = tag_entry(buf, entry) else {
            continue;
        };
        match &entry[0..4] {
            b"PLOC" => {
                if tag.tag_number == 1 && peaks.is_none() {
                    peaks = Some(read_short_array(buf, &tag));
                }
            }
            b"FWO_" => {
                if tag.tag_number == 1 && tag.data_size >= 4 {
                    let off = if tag.data_size <= 4 {
                        e + 20
                    } else {
                        tag.data_offset
                    };
                    if let Some(b) = buf.get(off..off + 4) {
                        fwo = [b[0], b[1], b[2], b[3]];
                    }
                }
            }
            b"DATA" => {
                let slot = match tag.tag_number {
                    9 => Some(&mut processed[0]),
                    10 => Some(&mut processed[1]),
                    11 => Some(&mut processed[2]),
                    12 => Some(&mut processed[3]),
                    1 => Some(&mut raw[0]),
                    2 => Some(&mut raw[1]),
                    3 => Some(&mut raw[2]),
                    4 => Some(&mut raw[3]),
                    _ => None,
                };
                if let Some(slot) = slot {
                    if slot.is_none() {
                        *slot = Some(read_short_array(buf, &tag));
                    }
                }
            }
            _ => {}
        }
    }

    let peak_locations: Vec<i32> = peaks
        .ok_or_else(|| invalid("no PLOC (peak positions) entry found"))?
        .into_iter()
        .map(i32::from)
        .collect();

    let channels: Vec<Vec<i16>> = if processed.iter().all(|c| c.is_some()) {
        // DATA.9..12 carry G/A/T/C in that order.
        let g = processed[0].take().unwrap();
        let a = processed[1].take().unwrap();
        let t = processed[2].take().unwrap();
        let c = processed[3].take().unwrap();
        vec![a, c, g, t]
    } else if raw.iter().all(|c| c.is_some()) {
        // FWO_ lists the base carried by each raw channel, e.g. "GATC" → DATA.1=G.
        let mut by_base: [Option<Vec<i16>>; 4] = [None, None, None, None]; // A, C, G, T
        for (i, ch) in raw.into_iter().enumerate() {
            let slot = match fwo[i].to_ascii_uppercase() {
                b'A' => Some(&mut by_base[0]),
                b'C' => Some(&mut by_base[1]),
                b'G' => Some(&mut by_base[2]),
                b'T' => Some(&mut by_base[3]),
                _ => None,
            };
            if let Some(s) = slot {
                *s = ch;
            }
        }
        if by_base.iter().any(|c| c.is_none()) {
            return Err(invalid("FWO_ order does not map raw channels to ACGT"));
        }
        by_base.into_iter().map(|c| c.unwrap()).collect()
    } else {
        return Err(invalid("no trace channels (DATA.9-12 or DATA.1-4) found"));
    };

    Ok(Chromatogram {
        trace_a: channels[0].clone(),
        trace_c: channels[1].clone(),
        trace_g: channels[2].clone(),
        trace_t: channels[3].clone(),
        peak_locations,
    })
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
        trace_path: Some(path.to_string_lossy().into_owned()),
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
        write_entry(
            &mut buf,
            b"tdir",
            1,
            1023,
            DIR_ENTRY_LEN as u16,
            1,
            DIR_ENTRY_LEN as u32,
            entries_offset,
        );

        let seq_offset = entries_offset + DIR_ENTRY_LEN as u32;
        write_entry(
            &mut buf,
            b"PBAS",
            1,
            2,
            1,
            seq.len() as u32,
            seq.len() as u32,
            seq_offset,
        );
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
        write_entry(
            &mut buf,
            b"tdir",
            1,
            1023,
            DIR_ENTRY_LEN as u16,
            1,
            DIR_ENTRY_LEN as u32,
            entries_offset,
        );
        write_entry(
            &mut buf,
            b"PCON",
            1,
            2,
            1,
            4,
            4,
            entries_offset + DIR_ENTRY_LEN as u32,
        );
        buf.extend_from_slice(b"ACGT");
        assert!(extract_pbas_sequence(&buf).is_err());
    }

    /// Assemble a minimal ABIF: root tdir entry + contiguous directory +
    /// appended blobs. Entries are given as (name, number, elem_type,
    /// elem_size, data bytes); inline 4-byte values (FWO_) are stored in the
    /// offset field.
    fn build_abif_tags(tags: Vec<(&[u8; 4], u32, u16, u16, Vec<u8>)>) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"ABIF");
        buf.extend_from_slice(&101u16.to_be_bytes());
        let entries_offset = (ROOT_ENTRY_OFFSET + DIR_ENTRY_LEN) as usize;
        let n = tags.len();
        write_entry(
            &mut buf,
            b"tdir",
            1,
            1023,
            DIR_ENTRY_LEN as u16,
            n as u32,
            (n * DIR_ENTRY_LEN) as u32,
            entries_offset as u32,
        );
        let mut blob_off = entries_offset + n * DIR_ENTRY_LEN;
        for (name, number, elem_type, elem_size, data) in &tags {
            // Only FWO_ (exactly 4 bytes) rides inline in the offset field;
            // every other tag goes to the blob region, as real files do.
            let inline = *name == b"FWO_" && data.len() <= 4;
            let offset = if inline {
                let mut field = [0u8; 4];
                field[..data.len()].copy_from_slice(data);
                u32::from_be_bytes(field)
            } else {
                let o = blob_off as u32;
                blob_off += data.len();
                o
            };
            let count = if *elem_size == 0 {
                data.len() as u32
            } else {
                (data.len() / *elem_size as usize) as u32
            };
            write_entry(
                &mut buf,
                name,
                *number,
                *elem_type,
                *elem_size,
                count,
                data.len() as u32,
                offset,
            );
        }
        for (name, _, _, _, data) in &tags {
            if data.len() > 4 || *name != b"FWO_" {
                buf.extend_from_slice(data);
            }
        }
        buf
    }

    fn shorts(v: &[i16]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }

    #[test]
    fn test_chromatogram_processed_channels() {
        let buf = build_abif_tags(vec![
            (b"PBAS", 1, 2, 1, b"ACGT".to_vec()),
            (b"PLOC", 1, 4, 2, shorts(&[0, 1, 2, 3])),
            (b"DATA", 9, 4, 2, shorts(&[5, 6, 7, 8])), // G
            (b"DATA", 10, 4, 2, shorts(&[50, 51, 52, 53])), // A
            (b"DATA", 11, 4, 2, shorts(&[500, 501, 502, 503])), // T
            (b"DATA", 12, 4, 2, shorts(&[9, 10, 11, 12])), // C
            (b"FWO_", 1, 2, 1, b"GATC".to_vec()),
        ]);
        let chrom = extract_chromatogram(&buf).unwrap();
        assert_eq!(chrom.peak_locations, vec![0, 1, 2, 3]);
        assert_eq!(chrom.trace_g, vec![5, 6, 7, 8]);
        assert_eq!(chrom.trace_a, vec![50, 51, 52, 53]);
        assert_eq!(chrom.trace_t, vec![500, 501, 502, 503]);
        assert_eq!(chrom.trace_c, vec![9, 10, 11, 12]);
    }

    #[test]
    fn test_chromatogram_raw_fallback_uses_fwo() {
        // Legacy file: only DATA.1-4, FWO_ = "CTAG" → DATA.1=C, 2=T, 3=A, 4=G.
        let buf = build_abif_tags(vec![
            (b"PLOC", 1, 4, 2, shorts(&[2, 5])),
            (b"DATA", 1, 4, 2, shorts(&[11, 12])), // C
            (b"DATA", 2, 4, 2, shorts(&[21, 22])), // T
            (b"DATA", 3, 4, 2, shorts(&[31, 32])), // A
            (b"DATA", 4, 4, 2, shorts(&[41, 42])), // G
            (b"FWO_", 1, 2, 1, b"CTAG".to_vec()),
        ]);
        let chrom = extract_chromatogram(&buf).unwrap();
        assert_eq!(chrom.peak_locations, vec![2, 5]);
        assert_eq!(chrom.trace_a, vec![31, 32]);
        assert_eq!(chrom.trace_c, vec![11, 12]);
        assert_eq!(chrom.trace_g, vec![41, 42]);
        assert_eq!(chrom.trace_t, vec![21, 22]);
    }

    #[test]
    fn test_chromatogram_missing_channels() {
        let buf = build_abif(b"ACGT");
        assert!(extract_chromatogram(&buf).is_err());
    }
}

