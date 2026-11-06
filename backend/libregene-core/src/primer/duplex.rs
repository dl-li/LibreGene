//! Nearest-neighbour parameters for non-ideal DNA duplexes.
//!
//! Internal mismatches: Allawi/SantaLucia (1997–1998) and Peyret et al. (1999).
//! Terminal mismatches: SantaLucia & Peyret (2001).
//! Dangling ends: Bommarito et al. (2000).

use super::thermodynamics::{lookup_nn, tm_from_nn_params, TmParams};

const IMM: &[(&str, f64, f64)] = &[
    ("AG/TT", 1.0, 0.9),
    ("AT/TG", -2.5, -8.3),
    ("CG/GT", -4.1, -11.7),
    ("CT/GG", -2.8, -8.0),
    ("GG/CT", 3.3, 10.4),
    ("GG/TT", 5.8, 16.3),
    ("GT/CG", -4.4, -12.3),
    ("GT/TG", 4.1, 9.5),
    ("TG/AT", -0.1, -1.7),
    ("TG/GT", -1.4, -6.2),
    ("TT/AG", -1.3, -5.3),
    ("AA/TG", -0.6, -2.3),
    ("AG/TA", -0.7, -2.3),
    ("CA/GG", -0.7, -2.3),
    ("CG/GA", -4.0, -13.2),
    ("GA/CG", -0.6, -1.0),
    ("GG/CA", 0.5, 3.2),
    ("TA/AG", 0.7, 0.7),
    ("TG/AA", 3.0, 7.4),
    ("AC/TT", 0.7, 0.2),
    ("AT/TC", -1.2, -6.2),
    ("CC/GT", -0.8, -4.5),
    ("CT/GC", -1.5, -6.1),
    ("GC/CT", 2.3, 5.4),
    ("GT/CC", 5.2, 13.5),
    ("TC/AT", 1.2, 0.7),
    ("TT/AC", 1.0, 0.7),
    ("AA/TC", 2.3, 4.6),
    ("AC/TA", 5.3, 14.6),
    ("CA/GC", 1.9, 3.7),
    ("CC/GA", 0.6, -0.6),
    ("GA/CC", 5.2, 14.2),
    ("GC/CA", -0.7, -3.8),
    ("TA/AC", 3.4, 8.0),
    ("TC/AA", 7.6, 20.2),
    ("AA/TA", 1.2, 1.7),
    ("CA/GA", -0.9, -4.2),
    ("GA/CA", -2.9, -9.8),
    ("TA/AA", 4.7, 12.9),
    ("AC/TC", 0.0, -4.4),
    ("CC/GC", -1.5, -7.2),
    ("GC/CC", 3.6, 8.9),
    ("TC/AC", 6.1, 16.4),
    ("AG/TG", -3.1, -9.5),
    ("CG/GG", -4.9, -15.3),
    ("GG/CG", -6.0, -15.8),
    ("TG/AG", 1.6, 3.6),
    ("AT/TT", -2.7, -10.8),
    ("CT/GT", -5.0, -15.8),
    ("GT/CT", -2.2, -8.4),
    ("TT/AT", 0.2, -1.5),
];

const TMM: &[(&str, f64, f64)] = &[
    ("AA/TA", -3.1, -7.8),
    ("TA/AA", -2.5, -6.3),
    ("CA/GA", -4.3, -10.7),
    ("GA/CA", -8.0, -22.5),
    ("AC/TC", -0.1, 0.5),
    ("TC/AC", -0.7, -1.3),
    ("CC/GC", -2.1, -5.1),
    ("GC/CC", -3.9, -10.6),
    ("AG/TG", -1.1, -2.1),
    ("TG/AG", -1.1, -2.7),
    ("CG/GG", -3.8, -9.5),
    ("GG/CG", -0.7, -19.2),
    ("AT/TT", -2.4, -6.5),
    ("TT/AT", -3.2, -8.9),
    ("CT/GT", -6.1, -16.9),
    ("GT/CT", -7.4, -21.2),
    ("AA/TC", -1.6, -4.0),
    ("AC/TA", -1.8, -3.8),
    ("CA/GC", -2.6, -5.9),
    ("CC/GA", -2.7, -6.0),
    ("GA/CC", -5.0, -13.8),
    ("GC/CA", -3.2, -7.1),
    ("TA/AC", -2.3, -5.9),
    ("TC/AA", -2.7, -7.0),
    ("AC/TT", -0.9, -1.7),
    ("AT/TC", -2.3, -6.3),
    ("CC/GT", -3.2, -8.0),
    ("CT/GC", -3.9, -10.6),
    ("GC/CT", -4.9, -13.5),
    ("GT/CC", -3.0, -7.8),
    ("TC/AT", -2.5, -6.3),
    ("TT/AC", -0.7, -1.2),
    ("AA/TG", -1.9, -4.4),
    ("AG/TA", -2.5, -5.9),
    ("CA/GG", -3.9, -9.6),
    ("CG/GA", -6.0, -15.5),
    ("GA/CG", -4.3, -11.1),
    ("GG/CA", -4.6, -11.4),
    ("TA/AG", -2.0, -4.7),
    ("TG/AA", -2.4, -5.8),
    ("AG/TT", -3.2, -8.7),
    ("AT/TG", -3.5, -9.4),
    ("CG/GT", -3.8, -9.0),
    ("CT/GG", -6.6, -18.7),
    ("GG/CT", -5.7, -15.9),
    ("GT/CG", -5.9, -16.1),
    ("TG/AT", -3.9, -10.5),
    ("TT/AG", -3.6, -9.8),
];

const DE: &[(&str, f64, f64)] = &[
    ("AA/.T", 0.2, 2.3),
    ("AC/.G", -6.3, -17.1),
    ("AG/.C", -3.7, -10.0),
    ("AT/.A", -2.9, -7.6),
    ("CA/.T", 0.6, 3.3),
    ("CC/.G", -4.4, -12.6),
    ("CG/.C", -4.0, -11.9),
    ("CT/.A", -4.1, -13.0),
    ("GA/.T", -1.1, -1.6),
    ("GC/.G", -5.1, -14.0),
    ("GG/.C", -3.9, -10.9),
    ("GT/.A", -4.2, -15.0),
    ("TA/.T", -6.9, -20.0),
    ("TC/.G", -4.0, -10.9),
    ("TG/.C", -4.9, -13.8),
    ("TT/.A", -0.2, -0.5),
    (".A/AT", -0.7, -0.8),
    (".C/AG", -2.1, -3.9),
    (".G/AC", -5.9, -16.5),
    (".T/AA", -0.5, -1.1),
    (".A/CT", 4.4, 14.9),
    (".C/CG", -0.2, -0.1),
    (".G/CC", -2.6, -7.4),
    (".T/CA", 4.7, 14.2),
    (".A/GT", -1.6, -3.6),
    (".C/GG", -3.9, -11.2),
    (".G/GC", -3.2, -10.4),
    (".T/GA", -4.1, -13.1),
    (".A/TT", 2.9, 10.4),
    (".C/TG", -4.4, -13.1),
    (".G/TC", -5.2, -15.0),
    (".T/TA", -3.8, -12.6),
];

fn lookup(table: &[(&str, f64, f64)], key: &str) -> Option<(f64, f64)> {
    table
        .iter()
        .find_map(|(candidate, dh, ds)| (*candidate == key).then_some((*dh, *ds)))
}

fn lookup_reversible(table: &[(&str, f64, f64)], key: &str) -> Option<(f64, f64)> {
    lookup(table, key).or_else(|| {
        let reversed: String = key.chars().rev().collect();
        lookup(table, &reversed)
    })
}

fn valid_structure(seq: &[u8], cseq: &[u8]) -> bool {
    seq.len() == cseq.len()
        && seq.len() >= 6
        && seq
            .iter()
            .chain(cseq)
            .all(|b| matches!(b, b'A' | b'C' | b'G' | b'T' | b'.'))
        && seq.iter().zip(cseq).all(|(a, b)| *a != b'.' || *b != b'.')
        && seq
            .iter()
            .enumerate()
            .all(|(i, b)| *b != b'.' || i == 0 || i == seq.len() - 1)
        && cseq
            .iter()
            .enumerate()
            .all(|(i, b)| *b != b'.' || i == 0 || i == cseq.len() - 1)
}

pub fn compute_duplex_tm(seq: &[u8], cseq: &[u8], params: &TmParams) -> Option<f64> {
    if !valid_structure(seq, cseq) {
        return None;
    }
    let mut seq = seq.to_ascii_uppercase();
    let mut cseq = cseq.to_ascii_uppercase();
    let paired_len = seq.iter().filter(|&&b| b != b'.').count();
    let original_ends: Vec<u8> = seq.iter().copied().filter(|&b| b != b'.').collect();
    let mut dh = 0.2;
    let mut ds = -5.7;

    if seq[0] == b'.' || cseq[0] == b'.' {
        let key = format!(
            "{}{}/{}{}",
            seq[0] as char, seq[1] as char, cseq[0] as char, cseq[1] as char
        );
        let (h, s) = lookup(DE, &key)?;
        dh += h;
        ds += s;
        seq.remove(0);
        cseq.remove(0);
    }
    if seq[seq.len() - 1] == b'.' || cseq[cseq.len() - 1] == b'.' {
        let n = seq.len();
        let key = format!(
            "{}{}/{}{}",
            cseq[n - 1] as char,
            cseq[n - 2] as char,
            seq[n - 1] as char,
            seq[n - 2] as char
        );
        let (h, s) = lookup(DE, &key)?;
        dh += h;
        ds += s;
        seq.pop();
        cseq.pop();
    }

    let left_tmm = format!(
        "{}{}/{}{}",
        cseq[1] as char, cseq[0] as char, seq[1] as char, seq[0] as char
    );
    if let Some((h, s)) = lookup(TMM, &left_tmm) {
        dh += h;
        ds += s;
        seq.remove(0);
        cseq.remove(0);
    }
    let n = seq.len();
    let right_tmm = format!(
        "{}{}/{}{}",
        seq[n - 2] as char,
        seq[n - 1] as char,
        cseq[n - 2] as char,
        cseq[n - 1] as char
    );
    if let Some((h, s)) = lookup(TMM, &right_tmm) {
        dh += h;
        ds += s;
        seq.pop();
        cseq.pop();
    }

    for end in [original_ends.first(), original_ends.last()]
        .into_iter()
        .flatten()
    {
        if matches!(end, b'A' | b'T') {
            dh += 2.2;
            ds += 6.9;
        }
    }

    for i in 0..seq.len() - 1 {
        let key = format!(
            "{}{}/{}{}",
            seq[i] as char,
            seq[i + 1] as char,
            cseq[i] as char,
            cseq[i + 1] as char
        );
        let (h, s) = if let Some(values) = lookup_reversible(IMM, &key) {
            values
        } else if cseq[i] == crate::utils::complement_char(seq[i] as char) as u8
            && cseq[i + 1] == crate::utils::complement_char(seq[i + 1] as char) as u8
        {
            lookup_nn(seq[i], seq[i + 1], super::thermodynamics::get_nn_map())?
        } else {
            return None;
        };
        dh += h;
        ds += s;
    }

    Some(tm_from_nn_params(dh, ds, paired_len, params))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatch_matches_reference_model() {
        let tm = compute_duplex_tm(
            b"CGTTCCAAAGATGTGGGCATGAGCTTAC",
            b"GCAAGGcTTCTACACCCGTACTCGAATG"
                .to_ascii_uppercase()
                .as_slice(),
            &TmParams::default(),
        )
        .unwrap();
        assert!((tm - 57.1).abs() < 0.2, "Tm={tm}");
    }

    #[test]
    fn dangling_end_and_mismatch_match_reference_model() {
        let seq = b".CGTTCCAAAGATGTGGGCATGAGCTTAC.";
        let cseq = b"TGCAAGGcTTCTACACCCGTACTCGAATGC".to_ascii_uppercase();
        let tm = compute_duplex_tm(seq, &cseq, &TmParams::default()).unwrap();
        assert!((tm - 57.3).abs() < 0.3, "Tm={tm}");
    }

    #[test]
    fn internal_gap_is_not_a_dangling_end() {
        assert_eq!(
            compute_duplex_tm(
                b"CGTTCCAAAGATGTGGGCATGAGCTTAC",
                b"GCAAGC.TTCTACACCCGTACTCGAATG",
                &TmParams::default(),
            ),
            None
        );
    }
}
