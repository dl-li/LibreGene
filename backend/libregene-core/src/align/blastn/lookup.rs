//! Lookup-table construction (NCBI blast_nalookup.h equivalent). Ported from
//! GenePad (https://genepad.cn, GenePad team); the batch-mode combined table
//! was not carried over.

use super::dust::DustMask;
use std::collections::HashMap;

pub struct LookupTable {
    pub word_size: usize,
    map: HashMap<u64, Vec<usize>>,
}

impl LookupTable {
    pub fn build(query: &[u8], word_size: usize, mask: &DustMask) -> Self {
        let mut map: HashMap<u64, Vec<usize>> = HashMap::new();
        if query.len() >= word_size && word_size <= 32 {
            for i in 0..=query.len() - word_size {
                if mask.word_masked(i, word_size) {
                    continue;
                }
                if let Some(key) = encode_word(&query[i..i + word_size]) {
                    map.entry(key).or_default().push(i);
                }
            }
        }
        LookupTable { word_size, map }
    }

    pub fn get(&self, word: &[u8]) -> Option<&[usize]> {
        let key = encode_word(word)?;
        self.map.get(&key).map(|v| v.as_slice())
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

pub fn encode_word(word: &[u8]) -> Option<u64> {
    if word.len() > 32 {
        return None;
    }
    let mut key = 0u64;
    for &base in word {
        let bits = match base {
            b'A' => 0,
            b'C' => 1,
            b'G' => 2,
            b'T' => 3,
            _ => return None,
        };
        key = (key << 2) | bits;
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_word_offsets() {
        let q = b"ACGTACGTACGT";
        let lt = LookupTable::build(q, 4, &DustMask::none());
        let offs = lt.get(b"ACGT").unwrap();
        assert_eq!(offs, &[0, 4, 8]);
    }

    #[test]
    fn misses_unknown_word() {
        let lt = LookupTable::build(b"AAAACCCC", 4, &DustMask::none());
        assert!(lt.get(b"GGGG").is_none());
    }

    #[test]
    fn skips_dust_masked_words() {
        let mask = DustMask::from_intervals(vec![(0, 4)]);
        let lt = LookupTable::build(b"AAAACCCC", 4, &mask);
        assert!(lt.get(b"AAAA").is_none());
        assert!(lt.get(b"CCCC").is_some());
    }

    #[test]
    fn skips_ambiguous_seed_words() {
        let lt = LookupTable::build(b"AAAANCCCC", 4, &DustMask::none());
        assert!(lt.get(b"AAAN").is_none());
        assert!(lt.get(b"NCCC").is_none());
        assert!(lt.get(b"CCCC").is_some());
    }
}
