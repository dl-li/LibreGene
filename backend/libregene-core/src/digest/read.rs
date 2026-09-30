use std::fmt::Write as _;

use super::lines::unit_for;
use super::range::validate_range;
use super::MAX_READ_BASES;
use crate::models::ProjectData;

/// Plain bases of `[start, end]` (no ruler); circular wrap supported.
pub fn read_sequence_bases(project: &ProjectData, start: i64, end: i64) -> Result<String, String> {
    let (s, e) = validate_range(project, start, end)?;
    let count = if s <= e { e - s + 1 } else { project.length - s + e + 1 };
    if count as usize > MAX_READ_BASES {
        let unit = unit_for(&project.molecule_type);
        return Err(format!(
            "requested {} {} exceeds the {} {} read limit; request a narrower window",
            count, unit, MAX_READ_BASES, unit
        ));
    }
    let bytes = project.sequence.as_bytes();
    let mut window = Vec::with_capacity(count as usize);
    if s <= e {
        window.extend_from_slice(&bytes[s as usize..=e as usize]);
    } else {
        window.extend_from_slice(&bytes[s as usize..]);
        window.extend_from_slice(&bytes[..=e as usize]);
    }
    Ok(String::from_utf8_lossy(&window).to_ascii_uppercase())
}

/// Bases in `[start, end]` with a coordinate ruler; circular wrap supported.
pub fn read_sequence(project: &ProjectData, start: i64, end: i64) -> Result<String, String> {
    let (s, e) = validate_range(project, start, end)?;
    let circular = project.topology == "circular";
    let count = if s <= e { e - s + 1 } else { project.length - s + e + 1 };
    if count as usize > MAX_READ_BASES {
        let unit = unit_for(&project.molecule_type);
        return Err(format!(
            "requested {} {} exceeds the {} {} read limit; request a narrower window",
            count, unit, MAX_READ_BASES, unit
        ));
    }
    let bytes = project.sequence.as_bytes();
    let mut window = Vec::with_capacity(count as usize);
    if s <= e {
        window.extend_from_slice(&bytes[s as usize..=e as usize]);
    } else {
        window.extend_from_slice(&bytes[s as usize..]);
        window.extend_from_slice(&bytes[..=e as usize]);
    }

    const GROUP: usize = 10;
    const COLS: usize = 6;
    const LINE_BASES: usize = GROUP * COLS;

    let mut out = String::new();
    let unit = unit_for(&project.molecule_type);
    let _ = writeln!(out,
        "COORDS: 1-based inclusive. Window {}..{} ({} {}) of {} {} {} (wrap: {})",
        s + 1,
        e + 1,
        count,
        unit,
        project.length,
        unit,
        project.topology,
        circular
    );
    // Ruler labels the group start positions of the first line. Small windows
    // (a single sequence line) skip it — the per-line coordinate prefix
    // already anchors the position and the ruler would dominate the output.
    if count as usize > LINE_BASES {
        out.push_str(&" ".repeat(7));
        for i in 0..COLS {
            let _ = write!(out, "{:>11}", (s + (i as i64) * GROUP as i64) % project.length + 1);
        }
        out.push('\n');
    }
    for (idx, &base) in window.iter().enumerate() {
        if idx % LINE_BASES == 0 {
            if idx > 0 {
                out.push('\n');
            }
            let _ = write!(out, "{:>6} ", (s + idx as i64) % project.length + 1);
        }
        out.push((base as char).to_ascii_uppercase());
        if (idx + 1) % GROUP == 0 && (idx + 1) % LINE_BASES != 0 {
            out.push(' ');
        }
    }
    out.push('\n');
    Ok(out)
}
