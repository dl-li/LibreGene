//! Sequence cleaning and reverse complement (NCBI BLAST_SequenceBlk
//! equivalent). Ported from GenePad (https://genepad.cn, GenePad team).

/// Clean an arbitrary string into uppercase ACGTN bytes:
/// lowercase → uppercase; ACGTN kept; other characters (IUPAC ambiguity
/// codes, punctuation) → N; whitespace and digits dropped.
pub fn to_upper_clean(seq: &str) -> Vec<u8> {
    to_upper_clean_bytes(seq.as_bytes())
}

/// Byte-slice variant of [`to_upper_clean`].
pub fn to_upper_clean_bytes(seq: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(seq.len());
    for &b in seq {
        match b {
            b'A' | b'C' | b'G' | b'T' => out.push(b),
            b'a' | b'c' | b'g' | b't' => out.push(b - 32),
            b'N' | b'n' => out.push(b'N'),
            b'0'..=b'9' | b' ' | b'\t' | b'\r' | b'\n' => { /* skip */ }
            _ => out.push(b'N'),
        }
    }
    out
}

/// Reverse complement: A<->T, C<->G, N->N, anything else (incl. IUPAC) -> N.
pub fn reverse_complement(seq: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(seq.len());
    for &b in seq.iter().rev() {
        out.push(match b {
            b'A' => b'T',
            b'T' => b'A',
            b'C' => b'G',
            b'G' => b'C',
            b'N' => b'N',
            _ => b'N',
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_to_uppercase_acgtn_only() {
        let out = to_upper_clean("acgtNxyzACGT");
        assert_eq!(std::str::from_utf8(&out).unwrap(), "ACGTNNNNACGT");
    }

    #[test]
    fn strips_whitespace_and_digits() {
        let out = to_upper_clean("ac 12 gt");
        assert_eq!(std::str::from_utf8(&out).unwrap(), "ACGT");
    }

    #[test]
    fn reverse_complement_basic() {
        let out = reverse_complement(b"ACGTAAA");
        assert_eq!(&out, b"TTTACGT");
    }

    #[test]
    fn reverse_complement_handles_n_and_iupac() {
        let out = reverse_complement(b"ACGTN");
        assert_eq!(&out, b"NACGT");
    }
}
