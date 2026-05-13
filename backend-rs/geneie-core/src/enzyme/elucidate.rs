//! `_parse_elucidate` — parse Biopython elucidate string into (top_cut, bot_cut) offsets.

/// Parse a Biopython elucidate string.
///
/// Returns `(top_cut, bot_cut)` — 0-indexed offsets within (or relative to) the
/// recognition site where the top and bottom strand cuts occur.
///
/// When no explicit bottom cut mark (`_`) is present, the bottom cut defaults to
/// the symmetrical position from the end of the recognition site.
pub fn parse_elucidate(eluc: &str, site: &str) -> (i64, i64) {
    let clean: String = eluc.chars().filter(|&c| c != '^' && c != '_').collect();

    let mut tc = eluc.find('^').map(|i| i as i64).unwrap_or(-1);
    let mut bc = eluc.find('_').map(|i| i as i64).unwrap_or(-1);

    if tc >= 0 {
        tc -= eluc[..tc as usize].chars().filter(|&c| c == '_').count() as i64;
    }
    if bc >= 0 {
        bc -= eluc[..bc as usize].chars().filter(|&c| c == '^').count() as i64;
    }

    if tc < 0 {
        tc = 0;
    }
    if bc < 0 {
        bc = site.len() as i64 - tc;
    }

    // Adjust for flanking context bases in elucidate
    if let Some(ss) = clean.find(site) {
        tc -= ss as i64;
        bc -= ss as i64;
    }

    (tc.max(0), bc.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aan_i() {
        let (tc, bc) = parse_elucidate("TTA^_TAA", "TTATAA");
        assert_eq!(tc, 3);
        assert_eq!(bc, 3);
    }

    #[test]
    fn test_eco_ri() {
        let (tc, bc) = parse_elucidate("G^AATT_C", "GAATTC");
        assert_eq!(tc, 1);
        assert_eq!(bc, 5);
    }

    #[test]
    fn test_aar_i() {
        let (tc, bc) = parse_elucidate("CACCTGCNNNN^NNNN_N", "CACCTGC");
        assert_eq!(tc, 11);
        assert_eq!(bc, 15);
    }
}
