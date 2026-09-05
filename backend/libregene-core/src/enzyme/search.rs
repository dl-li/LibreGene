//! Enzyme database loading and IUPAC tables.

use std::sync::OnceLock;

use crate::enzyme::data::{EnzymeDb, EnzymeRecord};

/// Global enzyme database, loaded once.
static DB: OnceLock<EnzymeDb> = OnceLock::new();

pub fn get_db() -> &'static EnzymeDb {
    DB.get_or_init(|| {
        let json = include_str!("../../data/comm_only_enzymes.json");
        let enzymes: Vec<EnzymeRecord> =
            serde_json::from_str(json).expect("failed to parse comm_only_enzymes.json");
        EnzymeDb { enzymes }
    })
}

/// DNA complement (reverse complement) — `match`-based, no HashMap.
pub fn dna_complement(seq: &str) -> String {
    let bytes: Vec<u8> = seq
        .bytes()
        .rev()
        .map(dna_complement_byte)
        .collect();
    String::from_utf8(bytes).unwrap_or_default()
}

fn dna_complement_byte(b: u8) -> u8 {
    match b {
        b'A' => b'T',
        b'T' => b'A',
        b'G' => b'C',
        b'C' => b'G',
        b'a' => b't',
        b't' => b'a',
        b'g' => b'c',
        b'c' => b'g',
        _ => b,
    }
}

/// IUPAC complement — handles ambiguity codes, `match`-based.
pub fn iupac_complement(seq: &str) -> String {
    let bytes: Vec<u8> = seq
        .bytes()
        .rev()
        .map(iupac_complement_byte)
        .collect();
    String::from_utf8(bytes).unwrap_or_default()
}

fn iupac_complement_byte(b: u8) -> u8 {
    match b {
        b'A' => b'T',
        b'T' => b'A',
        b'G' => b'C',
        b'C' => b'G',
        b'R' => b'Y',
        b'Y' => b'R',
        b'W' => b'W',
        b'S' => b'S',
        b'K' => b'M',
        b'M' => b'K',
        b'B' => b'V',
        b'D' => b'H',
        b'H' => b'D',
        b'V' => b'B',
        b'N' => b'N',
        _ => b,
    }
}

/// IUPAC → regex pattern mapping, `match`-based.
pub fn iupac_to_regex(site: &str) -> String {
    let mut pat = String::with_capacity(site.len() * 3);
    for c in site.chars() {
        match c {
            'N' => pat.push('.'),
            'R' => pat.push_str("[AG]"),
            'Y' => pat.push_str("[CT]"),
            'W' => pat.push_str("[AT]"),
            'S' => pat.push_str("[CG]"),
            'K' => pat.push_str("[GT]"),
            'M' => pat.push_str("[AC]"),
            'B' => pat.push_str("[CGT]"),
            'D' => pat.push_str("[AGT]"),
            'H' => pat.push_str("[ACT]"),
            'V' => pat.push_str("[ACG]"),
            other => pat.push(other),
        }
    }
    pat
}
