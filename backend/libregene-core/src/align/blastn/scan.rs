//! Subject scanning (NCBI blast_nascan.c equivalent). Ported from GenePad
//! (https://genepad.cn, GenePad team).

use super::lookup::LookupTable;

#[derive(Clone, Copy, Debug)]
pub struct WordHit {
    pub q_off: usize,
    pub s_off: usize,
}

/// Scan the subject and return all word hits (ascending subject order).
pub fn scan_subject(subject: &[u8], lookup: &LookupTable) -> Vec<WordHit> {
    let ws = lookup.word_size;
    if subject.len() < ws {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for s_off in 0..=subject.len() - ws {
        let word = &subject[s_off..s_off + ws];
        if let Some(q_offs) = lookup.get(word) {
            for &q_off in q_offs {
                hits.push(WordHit { q_off, s_off });
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::blastn::dust::DustMask;

    #[test]
    fn finds_all_word_hits() {
        let lt = LookupTable::build(b"ACGTACGT", 4, &DustMask::none());
        let hits = scan_subject(b"TTACGTAAT", &lt);
        let s_offs: Vec<usize> = hits.iter().map(|h| h.s_off).collect();
        assert!(s_offs.contains(&2));
    }

    #[test]
    fn returns_empty_when_subject_too_short() {
        let lt = LookupTable::build(b"ACGTACGTACGT", 4, &DustMask::none());
        let hits = scan_subject(b"AC", &lt);
        assert!(hits.is_empty());
    }
}
