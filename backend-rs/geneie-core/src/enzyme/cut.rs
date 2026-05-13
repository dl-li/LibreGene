//! `_closest` — find the recognition position closest to the expected position.
//!
//! Tiebreaker: prefer the match where `(cut_pos - pos)` is closest to `top_off`,
//! i.e., the cut falls at the right offset within the recognition.

/// Return the position closest to `target`.
///
/// Tiebreak: prefer the one where `(cut_pos - pos)` is closer to `top_off`.
pub fn closest(positions: &[usize], target: usize, cut_pos: usize, top_off: i64) -> usize {
    positions
        .iter()
        .min_by_key(|&&p| {
            let dist = if p > target {
                p - target
            } else {
                target - p
            };
            let cut_dist = ((cut_pos as i64 - p as i64) - top_off).unsigned_abs() as usize;
            (dist, cut_dist)
        })
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_closest_simple() {
        let pos = vec![5, 10, 15];
        assert_eq!(closest(&pos, 10, 12, 2), 10);
    }

    #[test]
    fn test_closest_tiebreak() {
        // Both 5 and 15 are 5 away from 10, but for 5: cut_pos-p=7, top_off=2 → diff=5
        // For 15: cut_pos-p=-3, top_off=2 → diff=5 → same, first wins
        let pos = vec![5, 15];
        assert_eq!(closest(&pos, 10, 12, 2), 5);
    }
}
