//! Ungapped X-drop extension (NCBI na_ungapped.c:s_NuclUngappedExtend
//! equivalent). Ported from GenePad (https://genepad.cn, GenePad team).

#[derive(Clone, Debug, Default)]
pub struct UngappedData {
    pub q_start: usize,
    pub q_end: usize, // exclusive
    pub s_start: usize,
    pub s_end: usize, // exclusive
    pub score: i32,
}

impl UngappedData {
    pub fn length(&self) -> usize {
        self.q_end.saturating_sub(self.q_start)
    }
}

#[derive(Clone, Copy)]
pub struct UngappedScoring {
    pub reward: i32,
    pub penalty: i32,
    pub x_drop: i32,
}

/// Ungapped X-drop extension from seed (q_off, s_off). Returns `None` when
/// even the word itself cannot reach a positive score.
pub fn ungapped_extend(
    query: &[u8],
    subject: &[u8],
    q_off: usize,
    s_off: usize,
    word_size: usize,
    scoring: UngappedScoring,
) -> Option<UngappedData> {
    let m = query.len();
    let n = subject.len();
    if q_off + word_size > m || s_off + word_size > n {
        return None;
    }

    let right = extend_one_dir(
        query,
        subject,
        q_off + word_size - 1,
        s_off + word_size - 1,
        true,
        scoring,
    );
    let left = extend_one_dir(query, subject, q_off, s_off, false, scoring);

    let q_start = left.0;
    let s_start = left.1;
    let q_end = right.0 + 1;
    let s_end = right.1 + 1;
    let score = left.2 + right.2 + (word_size as i32) * scoring.reward;

    if score <= 0 {
        return None;
    }
    Some(UngappedData {
        q_start,
        q_end,
        s_start,
        s_end,
        score,
    })
}

/// One-direction extension returning (best_q, best_s, best_score).
fn extend_one_dir(
    query: &[u8],
    subject: &[u8],
    q: usize,
    s: usize,
    right: bool,
    scoring: UngappedScoring,
) -> (usize, usize, i32) {
    let step: isize = if right { 1 } else { -1 };
    let mut cur = 0i32;
    let mut best = 0i32;
    let mut best_q = q;
    let mut best_s = s;
    let mut qi = q as isize;
    let mut si = s as isize;
    loop {
        qi += step;
        si += step;
        if qi < 0 || si < 0 {
            break;
        }
        let (qi_u, si_u) = (qi as usize, si as usize);
        if qi_u >= query.len() || si_u >= subject.len() {
            break;
        }
        let score_step = if query[qi_u] == subject[si_u] {
            scoring.reward
        } else {
            scoring.penalty
        };
        cur += score_step;
        if cur > best {
            best = cur;
            best_q = qi_u;
            best_s = si_u;
        } else if best - cur > scoring.x_drop {
            break;
        }
    }
    (best_q, best_s, best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_word_extends_full() {
        let q = b"ACGTACGTACGTACGT";
        let s = b"ACGTACGTACGTACGT";
        let d = ungapped_extend(
            q,
            s,
            0,
            0,
            4,
            UngappedScoring {
                reward: 1,
                penalty: -3,
                x_drop: 20,
            },
        )
        .unwrap();
        assert_eq!(d.score, 16);
        assert_eq!(d.q_start, 0);
        assert_eq!(d.q_end, 16);
    }

    #[test]
    fn stops_at_x_drop_in_mismatch_run() {
        let q = b"ACGTACGGGGGGGGGGGGGGGG";
        let s = b"ACGTACGTTTTTTTTTTTTTTT";
        let d = ungapped_extend(
            q,
            s,
            0,
            0,
            4,
            UngappedScoring {
                reward: 1,
                penalty: -3,
                x_drop: 20,
            },
        )
        .unwrap();
        assert!(d.q_end <= 8, "q_end={} should be small", d.q_end);
    }

    #[test]
    fn extends_left_and_right() {
        let q = b"AAAACGTACGTAAAA";
        let s = b"AAAACGTACGTAAAA";
        let d = ungapped_extend(
            q,
            s,
            4,
            4,
            4,
            UngappedScoring {
                reward: 1,
                penalty: -3,
                x_drop: 20,
            },
        )
        .unwrap();
        assert_eq!(d.q_start, 0);
        assert_eq!(d.q_end, 15);
    }
}
