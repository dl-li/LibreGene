//! DUST low-complexity soft masking (NCBI CSymDustMasker equivalent,
//! level=20, window=64). Ported from GenePad (https://genepad.cn, GenePad
//! team).

/// Query intervals flagged as low complexity (0-based, end-exclusive).
#[derive(Clone, Debug, Default)]
pub struct DustMask {
    intervals: Vec<(usize, usize)>,
}

impl DustMask {
    pub fn none() -> Self {
        Self { intervals: Vec::new() }
    }

    pub fn from_intervals(intervals: Vec<(usize, usize)>) -> Self {
        Self { intervals }
    }

    /// Whether this position is masked.
    pub fn is_masked(&self, pos: usize) -> bool {
        self.intervals.iter().any(|(s, e)| pos >= *s && pos < *e)
    }

    /// Whether the whole word [start, start+len) is masked (lookup use).
    pub fn word_masked(&self, start: usize, len: usize) -> bool {
        (start..start + len).all(|p| self.is_masked(p))
    }

    pub fn intervals(&self) -> &[(usize, usize)] {
        &self.intervals
    }
}

/// DUST-mask the query (level=20, window=64, linker=1). Simplified port:
/// sliding windows score triplet counts; windows above the threshold are
/// flagged.
pub fn mask(seq: &[u8], level: u32, window: usize) -> DustMask {
    if seq.len() < 3 {
        return DustMask::none();
    }
    let mut masked: Vec<bool> = vec![false; seq.len()];
    let half = window / 2;
    for start in (0..seq.len()).step_by(half.max(1)) {
        let end = (start + window).min(seq.len());
        if end - start < 4 {
            continue;
        }
        let mut triplet_counts = [0u32; 64];
        let mut count = 0u32;
        for i in start..end.saturating_sub(2) {
            if let Some(t) = triplet_index(&seq[i..i + 3]) {
                triplet_counts[t as usize] += 1;
                count += 1;
            }
        }
        if count == 0 {
            continue;
        }
        let score: u32 = triplet_counts.iter().map(|&c| c * (c.saturating_sub(1)) / 2).sum();
        if 10 * score > level * count {
            for p in start..end {
                masked[p] = true;
            }
        }
    }
    let mut intervals = Vec::new();
    let mut i = 0;
    while i < masked.len() {
        if masked[i] {
            let s = i;
            while i < masked.len() && masked[i] {
                i += 1;
            }
            intervals.push((s, i));
        } else {
            i += 1;
        }
    }
    DustMask::from_intervals(intervals)
}

fn triplet_index(t: &[u8]) -> Option<u8> {
    if t.len() < 3 {
        return None;
    }
    let mut idx = 0u8;
    for &b in &t[0..3] {
        idx = idx * 4
            + match b {
                b'A' => 0,
                b'C' => 1,
                b'G' => 2,
                b'T' => 3,
                _ => return None,
            };
    }
    Some(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_masks_nothing() {
        let m = DustMask::none();
        assert!(!m.is_masked(0));
        assert!(!m.word_masked(0, 11));
    }

    #[test]
    fn from_intervals_masks() {
        let m = DustMask::from_intervals(vec![(0, 4)]);
        assert!(m.is_masked(0));
        assert!(m.is_masked(3));
        assert!(!m.is_masked(4));
        assert!(m.word_masked(0, 4));
    }

    #[test]
    fn mask_detects_low_complexity() {
        let m = mask(b"AAAAAAAAAAAAAAAAAAAA", 20, 64);
        assert!(m.intervals().iter().any(|(s, e)| *e - *s >= 4));
    }

    #[test]
    fn mask_leaves_normal_sequence() {
        let seq = b"ACGTACGTGCTAGCTAGCTA";
        let m = mask(seq, 20, 64);
        let total: usize = m.intervals().iter().map(|(s, e)| e - s).sum();
        assert!(total < seq.len());
    }
}
