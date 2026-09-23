//! Fuzzy IUPAC matching — `_fuzzy_find_all` port.
//!
//! Converts a site with IUPAC ambiguity codes to a regex pattern and finds
//! all matches in a context window.

use regex::bytes::Regex;

use crate::enzyme::search::iupac_to_regex;

/// Return all match positions for an IUPAC pattern in `ctx`.
pub fn fuzzy_find_all(ctx: &[u8], site: &str) -> Vec<usize> {
    let pat = format!("(?i){}", iupac_to_regex(site));
    let re = match Regex::new(&pat) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let mut hits = Vec::new();
    // Advance by 1 so overlapping occurrences (bordered sites) are all found.
    let mut pos = 0;
    while let Some(m) = re.find_at(ctx, pos) {
        hits.push(m.start());
        pos = m.start() + 1;
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_find_exact() {
        let hits = fuzzy_find_all(b"NNGAATTCNN", "GAATTC");
        assert_eq!(hits, vec![2]);
    }

    #[test]
    fn test_fuzzy_find_iupac() {
        // N matches any base
        let hits = fuzzy_find_all(b"GGCTCTGG", "GGNTCTGG");
        assert_eq!(hits, vec![0]);
    }

    #[test]
    fn test_fuzzy_find_multiple() {
        let hits = fuzzy_find_all(b"GAATTCNNGAATTC", "GAATTC");
        assert_eq!(hits, vec![0, 8]);
    }

    #[test]
    fn test_fuzzy_find_overlapping() {
        // Bordered site (prefix GC == suffix GC): overlapping occurrences
        // must all be reported.
        let hits = fuzzy_find_all(b"GCGCGC", "GCGC");
        assert_eq!(hits, vec![0, 2]);
    }
}
