use super::thermodynamics::{get_nn_map, TmAlgorithm, TmParams};

const T0: f64 = 310.15;
const R: f64 = 1.9872;

const NN_G: &[(&[u8; 2], f64)] = &[
    (b"AA", -1.00),
    (b"AC", -1.44),
    (b"AG", -1.28),
    (b"AT", -0.88),
    (b"CA", -1.45),
    (b"CC", -1.84),
    (b"CG", -2.17),
    (b"CT", -1.28),
    (b"GA", -1.30),
    (b"GC", -2.24),
    (b"GG", -1.84),
    (b"GT", -1.44),
    (b"TA", -0.58),
    (b"TC", -1.30),
    (b"TG", -1.45),
    (b"TT", -1.00),
];

const DE_FIRST_KEYS: &[&str] = &[
    "AA/.T", "CA/.T", "GA/.T", "TA/.T", "AC/.G", "CC/.G", "GC/.G", "TC/.G", "AG/.C", "CG/.C",
    "GG/.C", "TG/.C", "AT/.A", "CT/.A", "GT/.A", "TT/.A",
];
const DE_SECOND_KEYS: &[&str] = &[
    ".T/AA", ".T/CA", ".T/GA", ".T/TA", ".G/AC", ".G/CC", ".G/GC", ".G/TC", ".C/AG", ".C/CG",
    ".C/GG", ".C/TG", ".A/AT", ".A/CT", ".A/GT", ".A/TT",
];
const DE_SECOND_ENCODED: &[&str] = &[
    "AA_T", "AC_T", "AG_T", "AT_T", "CA_G", "CC_G", "CG_G", "CT_G", "GA_C", "GC_C", "GG_C", "GT_C",
    "TA_A", "TC_A", "TG_A", "TT_A",
];

const DE_G: &[(&str, f64)] = &[
    ("AA/.T", -0.51),
    ("AC/.G", -0.96),
    ("AG/.C", -0.58),
    ("AT/.A", -0.50),
    ("CA/.T", -0.42),
    ("CC/.G", -0.52),
    ("CG/.C", -0.34),
    ("CT/.A", -0.02),
    ("GA/.T", -0.62),
    ("GC/.G", -0.72),
    ("GG/.C", -0.56),
    ("GT/.A", 0.48),
    ("TA/.T", -0.71),
    ("TC/.G", -0.58),
    ("TG/.C", -0.61),
    ("TT/.A", -0.10),
    (".A/AT", -0.48),
    (".C/AG", -0.92),
    (".G/AC", -0.82),
    (".T/AA", -0.12),
    (".A/CT", -0.19),
    (".C/CG", -0.23),
    (".G/CC", -0.31),
    (".T/CA", 0.28),
    (".A/GT", -0.50),
    (".C/GG", -0.44),
    (".G/GC", -0.01),
    (".T/GA", -0.01),
    (".A/TT", -0.29),
    (".C/TG", -0.35),
    (".G/TC", -0.52),
    (".T/TA", 0.13),
];

pub(crate) fn supports_params(params: &TmParams) -> bool {
    params.algorithm == TmAlgorithm::SnapGeneCompatible
        && params.na_conc == 0.05
        && params.mg_conc == 0.0
        && params.dntp_conc == 0.0
        && params.tris_conc == 0.0
}

fn lookup_nn_g(b1: u8, b2: u8) -> Option<f64> {
    NN_G.iter()
        .find_map(|(key, g)| (**key == [b1, b2]).then_some(*g))
}

fn lookup_de_g(key: &str) -> Option<f64> {
    DE_G.iter()
        .find_map(|(candidate, g)| (*candidate == key).then_some(*g))
}

fn de_motif(raw: &str, second: bool) -> Option<&'static str> {
    let mut result = None;
    for i in 0..16 {
        let (key, encoded) = if second {
            (DE_SECOND_KEYS[i], DE_SECOND_ENCODED[i].to_string())
        } else {
            (
                DE_FIRST_KEYS[i],
                DE_FIRST_KEYS[i].replace('/', "").replace('.', "_"),
            )
        };
        let forward = encoded.replace('_', "");
        let reverse = encoded.chars().rev().collect::<String>().replace('_', "");
        if raw == forward || raw == reverse {
            result = Some(key);
        }
    }
    result
}

fn core_g_h(seq: &[u8], na: f64) -> Option<(f64, f64)> {
    if seq.len() < 2 {
        return None;
    }
    let nn_map = get_nn_map();
    let mut g = 1.96 - 0.114 * (seq.len() as f64 - 1.0) * na.ln();
    let mut h = 0.2;
    for w in seq.windows(2) {
        g += lookup_nn_g(w[0], w[1])?;
        h += nn_map.get(&[w[0], w[1]])?.0;
    }
    for end in [seq.first(), seq.last()].into_iter().flatten() {
        if matches!(end, b'A' | b'T') {
            g += 0.05;
            h += 2.2;
        }
    }
    Some((g, h))
}

fn tm_from_g_h(g: f64, h: f64, seq: &[u8], concentration: f64) -> Option<f64> {
    let sequence = std::str::from_utf8(seq).ok()?;
    let self_complementary = crate::utils::reverse_complement(sequence).as_bytes() == seq;
    let g = if self_complementary { g + 0.43 } else { g };
    let concentration = concentration.max(1e-12);
    let concentration_term = if self_complementary {
        R * (4.0 * concentration).ln()
    } else {
        R * concentration.ln()
    };
    let entropy = (h - g) * 1000.0 / T0;
    let tm = h * 1000.0 / (entropy + concentration_term) - 273.15;
    tm.is_finite().then_some(tm)
}

pub(crate) fn compute_tm(seq: &str, params: &TmParams) -> Option<f64> {
    let seq = seq.to_ascii_uppercase();
    let (g, h) = core_g_h(seq.as_bytes(), params.na_conc)?;
    tm_from_g_h(g, h, seq.as_bytes(), params.primer_conc)
}

pub(crate) fn compute_duplex_tm(seq: &[u8], cseq: &[u8], params: &TmParams) -> Option<f64> {
    if seq.len() != cseq.len() || seq.len() < 6 {
        return None;
    }
    let seq = seq.to_ascii_uppercase();
    let cseq = cseq.to_ascii_uppercase();
    let mut start = 0;
    let mut end = seq.len();
    let mut g = 0.0;
    let mut h = 0.0;

    if seq[0] == b'.' || cseq[0] == b'.' {
        let raw = format!("{}{}{}", seq[1] as char, cseq[0] as char, cseq[1] as char);
        let key = de_motif(&raw, true)?;
        g += lookup_de_g(key)? - 0.057 * params.na_conc.ln();
        h += super::duplex::lookup(super::duplex::DE, key)?.0;
        start += 1;
    }
    if seq[end - 1] == b'.' || cseq[end - 1] == b'.' {
        let raw = format!(
            "{}{}{}",
            seq[end - 2] as char,
            cseq[end - 2] as char,
            cseq[end - 1] as char
        );
        let key = de_motif(&raw, false)?;
        g += lookup_de_g(key)? - 0.057 * params.na_conc.ln();
        h += super::duplex::lookup(super::duplex::DE, key)?.0;
        end -= 1;
    }
    if end <= start {
        return None;
    }

    let paired = &seq[start..end];
    let paired_cseq = &cseq[start..end];
    if !paired
        .iter()
        .all(|b| matches!(b, b'A' | b'C' | b'G' | b'T'))
        || !paired
            .iter()
            .zip(paired_cseq)
            .all(|(&a, &b)| crate::utils::complement_char(a as char) as u8 == b)
    {
        return None;
    }

    let (core_g, core_h) = core_g_h(paired, params.na_conc)?;
    g += core_g;
    h += core_h;
    tm_from_g_h(g, h, paired, params.primer_conc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> TmParams {
        TmParams {
            algorithm: TmAlgorithm::SnapGeneCompatible,
            ..TmParams::default()
        }
    }

    fn template() -> String {
        let gbk = include_str!("../../../../examples/BlueScribe-mEGFP.gbk");
        gbk.split("ORIGIN")
            .nth(1)
            .unwrap()
            .split("//")
            .next()
            .unwrap()
            .chars()
            .filter(char::is_ascii_alphabetic)
            .collect::<String>()
            .to_ascii_uppercase()
    }

    fn region_tm(template: &str, start: usize, end: usize, reverse: bool) -> f64 {
        let region = &template[start - 1..end];
        let primer = if reverse {
            crate::utils::reverse_complement(region)
        } else {
            region.to_string()
        };
        let complement = if reverse {
            region.chars().rev().collect::<String>()
        } else {
            crate::utils::reverse_complement(region)
                .chars()
                .rev()
                .collect::<String>()
        };
        let mut seq = primer.clone().into_bytes();
        let mut cseq = complement.into_bytes();
        if reverse {
            if end < template.len() {
                seq.insert(0, b'.');
                cseq.insert(0, template.as_bytes()[end]);
            }
            if start > 1 {
                seq.push(b'.');
                cseq.push(template.as_bytes()[start - 2]);
            }
        } else {
            if start > 1 {
                seq.insert(0, b'.');
                cseq.insert(
                    0,
                    crate::utils::complement_char(template.as_bytes()[start - 2] as char) as u8,
                );
            }
            if end < template.len() {
                seq.push(b'.');
                cseq.push(crate::utils::complement_char(template.as_bytes()[end] as char) as u8);
            }
        }
        compute_duplex_tm(&seq, &cseq, &params()).unwrap_or_else(|| {
            panic!(
                "no Tm for {} / {}",
                String::from_utf8_lossy(&seq),
                String::from_utf8_lossy(&cseq)
            )
        })
    }

    #[test]
    fn reference_regions_match_except_known_counterexamples() {
        let template = template();
        let regions = [
            (3001, 3020, 61, 62),
            (2981, 3000, 61, 62),
            (101, 116, 63, 60),
            (201, 218, 57, 60),
            (379, 395, 56, 55),
            (1221, 1237, 49, 49),
            (501, 525, 67, 67),
            (2101, 2125, 61, 60),
            (1201, 1216, 56, 54),
            (1701, 1722, 57, 56),
            (3301, 3320, 40, 42),
            (2301, 2320, 50, 50),
        ];
        let mut mismatches = Vec::new();
        for (start, end, forward_expected, reverse_expected) in regions {
            for (reverse, expected) in [(false, forward_expected), (true, reverse_expected)] {
                let actual = region_tm(&template, start, end, reverse).round() as i32;
                if actual != expected {
                    mismatches.push((start, end, reverse, actual, expected));
                }
            }
        }
        assert_eq!(
            mismatches,
            [
                (3001, 3020, true, 63, 62),
                (201, 218, false, 58, 57),
                (1221, 1237, false, 50, 49),
            ]
        );
    }

    #[test]
    fn reference_sequence_cases_match_except_known_counterexamples() {
        let template = template();
        let cases = [
            ("CGGGATAATACCGCGCCACA", 61),
            ("CGGGATAATACCGCGCCAC", 60),
            ("CGGGATAATACCGCGCCA", 56),
            ("CGGGATAATACCGCGC", 54),
            ("CGGGATAATACCGC", 46),
            ("GCTCTTGCCCGGCGTCAAT", 64),
            ("GCTCTTGCCCGGCGTCAA", 64),
            ("GCCCGGCGTCAATACGGG", 64),
            ("GCCCGGCGTCAATACG", 59),
            ("TAGCAGAACTTTAAAAGTGCTCATC", 56),
            ("TTTAAAAGTGCTCATC", 43),
            ("GTCACTCGGGATAATACCGCGCCACA", 61),
            ("TGTGGCGCGGTATTATCCCG", 62),
            ("GTCACTTGTGGCGCGGTATTATCCCG", 62),
            ("GGATCCTTACTTGTACAGCTCGTCCA", 61),
            ("GTCACTGGATCCTTACTTGTACAGCTCGTCCA", 61),
        ];
        let mut mismatches = Vec::new();
        for (sequence, expected) in cases {
            let core = sequence.strip_prefix("GTCACT").unwrap_or(sequence);
            let reverse_core = crate::utils::reverse_complement(core);
            let forward = template.match_indices(core).collect::<Vec<_>>();
            let reverse = template.match_indices(&reverse_core).collect::<Vec<_>>();
            let (start, reverse) = match (forward.as_slice(), reverse.as_slice()) {
                ([(start, _)], []) => (*start, false),
                ([], [(start, _)]) => (*start, true),
                _ => panic!("binding site is not unique: {core}"),
            };
            let actual =
                region_tm(&template, start + 1, start + core.len(), reverse).round() as i32;
            if actual != expected {
                mismatches.push((sequence, actual, expected));
            }
        }
        assert_eq!(
            mismatches,
            [
                ("TGTGGCGCGGTATTATCCCG", 63, 62),
                ("GTCACTTGTGGCGCGGTATTATCCCG", 63, 62),
            ]
        );
    }

    #[test]
    fn bare_core_matches_reference_value() {
        let core = compute_tm("GGATCCTTACTTGTACAGCTCGTCCA", &params()).unwrap();
        assert!((core - 60.967126).abs() < 0.000001);
    }

    #[test]
    fn custom_salt_conditions_are_not_supported() {
        let mut custom = params();
        custom.mg_conc = 0.001;
        assert!(!supports_params(&custom));
    }
}
