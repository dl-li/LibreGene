pub fn complement_char(c: char) -> char {
    match c {
        'A' => 'T',
        'T' => 'A',
        'G' => 'C',
        'C' => 'G',
        'a' => 't',
        't' => 'a',
        'g' => 'c',
        'c' => 'g',
        _ => c,
    }
}

pub fn complement(seq: &str) -> String {
    seq.chars().map(complement_char).collect()
}

pub fn reverse_complement(seq: &str) -> String {
    seq.chars().rev().map(complement_char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reverse_complement() {
        assert_eq!(reverse_complement("ATGC"), "GCAT");
        assert_eq!(reverse_complement("AATT"), "AATT");
    }

    #[test]
    fn test_complement() {
        assert_eq!(complement("ATGC"), "TACG");
    }

    #[test]
    fn test_complement_char() {
        assert_eq!(complement_char('A'), 'T');
        assert_eq!(complement_char('G'), 'C');
    }
}
