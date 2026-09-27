//! Edit block: compact traceback representation as runs of consecutive
//! operations. Ported from GenePad (https://genepad.cn, GenePad team).
//! Sub(n) = n (mis)match columns; Ins(n) = n extra query bases (subject
//! gap); Del(n) = n extra subject bases (query gap).

/// One consecutive run: Sub/Ins/Del with an associated length.
#[derive(Clone, Debug, PartialEq)]
pub enum EditOp {
    Sub(usize),
    Ins(usize),
    Del(usize),
}

#[derive(Clone, Debug, Default)]
pub struct EditBlock {
    pub ops: Vec<EditOp>,
}

/// Alignment column (engine-internal representation).
#[derive(Clone, Debug, PartialEq)]
pub struct AlignColumn {
    pub ref_base: u8,
    pub query_base: u8,
    pub col_type: &'static str, // "match"|"mismatch"|"insertion"|"deletion"
    pub ref_position: u64,      // 1-based; insertion → subject column to its left
    pub query_position: u64,    // 1-based; deletion → 0
}

impl EditBlock {
    /// Expand the edit block into alignment columns. q0/s0 are the 0-based
    /// query/subject start offsets.
    pub fn to_columns(&self, query: &[u8], subject: &[u8], q0: usize, s0: usize) -> Vec<AlignColumn> {
        let mut cols = Vec::new();
        let mut qi = q0;
        let mut si = s0;
        for op in &self.ops {
            match *op {
                EditOp::Sub(n) => {
                    for _ in 0..n {
                        let qb = query[qi];
                        let rb = subject[si];
                        let ct = if qb == rb { "match" } else { "mismatch" };
                        cols.push(AlignColumn {
                            ref_base: rb,
                            query_base: qb,
                            col_type: ct,
                            ref_position: (si + 1) as u64,
                            query_position: (qi + 1) as u64,
                        });
                        qi += 1;
                        si += 1;
                    }
                }
                EditOp::Ins(n) => {
                    for _ in 0..n {
                        let qb = query[qi];
                        cols.push(AlignColumn {
                            ref_base: b'-',
                            query_base: qb,
                            col_type: "insertion",
                            ref_position: si as u64, // subject column to the left
                            query_position: (qi + 1) as u64,
                        });
                        qi += 1;
                    }
                }
                EditOp::Del(n) => {
                    for _ in 0..n {
                        let rb = subject[si];
                        cols.push(AlignColumn {
                            ref_base: rb,
                            query_base: b'-',
                            col_type: "deletion",
                            ref_position: (si + 1) as u64,
                            query_position: 0,
                        });
                        si += 1;
                    }
                }
            }
        }
        cols
    }

    /// Count exact matches (gaps excluded).
    pub fn num_ident(&self, query: &[u8], subject: &[u8], q0: usize, s0: usize) -> usize {
        let mut ident = 0;
        let mut qi = q0;
        let mut si = s0;
        for op in &self.ops {
            match *op {
                EditOp::Sub(n) => {
                    for _ in 0..n {
                        if query[qi] == subject[si] {
                            ident += 1;
                        }
                        qi += 1;
                        si += 1;
                    }
                }
                EditOp::Ins(n) => qi += n,
                EditOp::Del(n) => si += n,
            }
        }
        ident
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_columns_all_match() {
        let eb = EditBlock { ops: vec![EditOp::Sub(4)] };
        let cols = eb.to_columns(b"ACGT", b"ACGT", 0, 0);
        assert_eq!(cols.len(), 4);
        for c in &cols {
            assert_eq!(c.col_type, "match");
        }
        assert_eq!(cols[0].ref_base, b'A');
        assert_eq!(cols[3].query_position, 4);
    }

    #[test]
    fn to_columns_with_insertion() {
        // query = ACGTT, subject = ACGT: Sub(4) then one extra query T
        let eb = EditBlock { ops: vec![EditOp::Sub(4), EditOp::Ins(1)] };
        let cols = eb.to_columns(b"ACGTT", b"ACGT", 0, 0);
        assert_eq!(cols.len(), 5);
        assert_eq!(cols[4].col_type, "insertion");
        assert_eq!(cols[4].query_base, b'T');
        assert_eq!(cols[4].ref_base, b'-');
    }

    #[test]
    fn num_ident_counts_matches() {
        let eb = EditBlock { ops: vec![EditOp::Sub(3), EditOp::Sub(1)] };
        // ACG vs ACG = 3 matches; T vs G = 1 mismatch → 3 ident
        assert_eq!(eb.num_ident(b"ACGT", b"ACGG", 0, 0), 3);
    }
}
