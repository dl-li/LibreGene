//! Enzyme database loading and IUPAC tables.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::enzyme::data::{EnzymeDb, EnzymeRecord};

/// IUPAC → regex pattern mapping.
static IUPAC: OnceLock<HashMap<char, &'static str>> = OnceLock::new();

fn iupac_table() -> &'static HashMap<char, &'static str> {
    IUPAC.get_or_init(|| {
        HashMap::from([
            ('N', "."),
            ('R', "[AG]"),
            ('Y', "[CT]"),
            ('W', "[AT]"),
            ('S', "[CG]"),
            ('K', "[GT]"),
            ('M', "[AC]"),
            ('B', "[CGT]"),
            ('D', "[AGT]"),
            ('H', "[ACT]"),
            ('V', "[ACG]"),
        ])
    })
}

/// DNA complement table (upper+lower).
static DNA_COMP: OnceLock<HashMap<u8, u8>> = OnceLock::new();

fn dna_comp_table() -> &'static HashMap<u8, u8> {
    DNA_COMP.get_or_init(|| {
        HashMap::from([
            (b'A', b'T'),
            (b'T', b'A'),
            (b'G', b'C'),
            (b'C', b'G'),
            (b'a', b't'),
            (b't', b'a'),
            (b'g', b'c'),
            (b'c', b'g'),
        ])
    })
}

/// IUPAC complement table (includes ambiguity codes).
static IUPAC_COMP_TABLE: OnceLock<HashMap<u8, u8>> = OnceLock::new();

fn iupac_comp_table() -> &'static HashMap<u8, u8> {
    IUPAC_COMP_TABLE.get_or_init(|| {
        HashMap::from([
            (b'A', b'T'),
            (b'T', b'A'),
            (b'G', b'C'),
            (b'C', b'G'),
            (b'R', b'Y'),
            (b'Y', b'R'),
            (b'W', b'W'),
            (b'S', b'S'),
            (b'K', b'M'),
            (b'M', b'K'),
            (b'B', b'V'),
            (b'D', b'H'),
            (b'H', b'D'),
            (b'V', b'B'),
            (b'N', b'N'),
        ])
    })
}

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

pub fn dna_complement(seq: &str) -> String {
    let table = dna_comp_table();
    let bytes: Vec<u8> = seq
        .bytes()
        .rev()
        .map(|b| table.get(&b).copied().unwrap_or(b))
        .collect();
    String::from_utf8(bytes).unwrap_or_default()
}

pub fn iupac_complement(seq: &str) -> String {
    let table = iupac_comp_table();
    let bytes: Vec<u8> = seq
        .bytes()
        .rev()
        .map(|b| table.get(&b).copied().unwrap_or(b))
        .collect();
    String::from_utf8(bytes).unwrap_or_default()
}

pub fn iupac_to_regex(site: &str) -> String {
    let table = iupac_table();
    let mut pat = String::with_capacity(site.len() * 3);
    for c in site.chars() {
        match table.get(&c) {
            Some(re) => pat.push_str(re),
            None => pat.push(c),
        }
    }
    pat
}
