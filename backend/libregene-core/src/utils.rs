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

/// Shift/clip feature coordinates after replacing `[edit_start, edit_end]`
/// (0-based inclusive) with `new_len` bases. A pure insertion is
/// `edit_end = edit_start - 1`. Mirrors the frontend `adjustAnnotations`.
pub fn adjust_features_for_edit(
    features: &mut Vec<crate::models::Feature>,
    edit_start: i64,
    edit_end: i64,
    new_len: i64,
) {
    let old_len = edit_end - edit_start + 1;
    let delta = new_len - old_len;
    if delta == 0 && old_len == 0 {
        return;
    }

    let adjust_span = |s: i64, e: i64| -> Option<(i64, i64)> {
        if e < edit_start {
            return Some((s, e));
        }
        if s > edit_end {
            return Some((s + delta, e + delta));
        }
        let ns = if s < edit_start { s } else { edit_start + new_len };
        let ne = if e > edit_end { e + delta } else { edit_start + new_len - 1 };
        if ns > ne {
            None
        } else {
            Some((ns, ne))
        }
    };

    features.retain_mut(|f| {
        if f.segments.is_empty() {
            match adjust_span(f.start, f.end) {
                Some((s, e)) => {
                    f.start = s;
                    f.end = e;
                    true
                }
                None => false,
            }
        } else {
            f.segments.retain_mut(|seg| match adjust_span(seg.start, seg.end) {
                Some((s, e)) => {
                    seg.start = s;
                    seg.end = e;
                    true
                }
                None => false,
            });
            if f.segments.is_empty() {
                return false;
            }
            f.start = f.segments.iter().map(|s| s.start).min().unwrap();
            f.end = f.segments.iter().map(|s| s.end).max().unwrap();
            true
        }
    });
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

    fn feat(id: &str, start: i64, end: i64, segments: Vec<(i64, i64)>) -> crate::models::Feature {
        crate::models::Feature {
            id: id.to_string(),
            name: id.to_string(),
            start,
            end,
            color: "#000000".to_string(),
            ftype: "CDS".to_string(),
            segments: segments
                .into_iter()
                .map(|(start, end)| crate::models::Segment {
                    start,
                    end,
                    color: None,
                })
                .collect(),
            strand: "+".to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        }
    }

    #[test]
    fn adjust_pure_insertion() {
        // Insert 10 bp at position 100 (edit_end = start - 1).
        let mut feats = vec![
            feat("before", 0, 50, vec![]),
            feat("at", 100, 120, vec![]),
            feat("after", 200, 300, vec![]),
        ];
        adjust_features_for_edit(&mut feats, 100, 99, 10);
        assert_eq!((feats[0].start, feats[0].end), (0, 50));
        assert_eq!((feats[1].start, feats[1].end), (110, 130));
        assert_eq!((feats[2].start, feats[2].end), (210, 310));
    }

    #[test]
    fn adjust_pure_deletion_removes_inner_feature() {
        // Delete [100..199]; feature fully inside is dropped, downstream shifts.
        let mut feats = vec![
            feat("before", 0, 50, vec![]),
            feat("inside", 110, 150, vec![]),
            feat("after", 200, 300, vec![]),
        ];
        adjust_features_for_edit(&mut feats, 100, 199, 0);
        assert_eq!(feats.len(), 2);
        assert_eq!(feats[0].id, "before");
        assert_eq!((feats[1].start, feats[1].end), (100, 200));
    }

    #[test]
    fn adjust_replace_clips_boundary_features() {
        // Replace [100..199] (100 bp) with 10 bp; delta = -90.
        let mut feats = vec![
            feat("left_overlap", 50, 150, vec![]),
            feat("right_overlap", 150, 250, vec![]),
            feat("spanning", 50, 250, vec![]),
        ];
        adjust_features_for_edit(&mut feats, 100, 199, 10);
        assert_eq!((feats[0].start, feats[0].end), (50, 109));
        assert_eq!((feats[1].start, feats[1].end), (110, 160));
        assert_eq!((feats[2].start, feats[2].end), (50, 160));
    }

    #[test]
    fn adjust_segmented_feature() {
        // Delete [100..199]; seg1 clipped, seg2 (inside) removed, seg3 shifted.
        let mut feats = vec![feat(
            "seg",
            50,
            300,
            vec![(50, 120), (130, 150), (200, 300)],
        )];
        adjust_features_for_edit(&mut feats, 100, 199, 0);
        let f = &feats[0];
        let spans: Vec<(i64, i64)> = f.segments.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(spans, vec![(50, 99), (100, 200)]);
        assert_eq!((f.start, f.end), (50, 200));
    }

    #[test]
    fn adjust_all_segments_removed_drops_feature() {
        let mut feats = vec![
            feat("gone", 100, 200, vec![(100, 150), (160, 200)]),
            feat("kept", 300, 400, vec![(300, 400)]),
        ];
        adjust_features_for_edit(&mut feats, 100, 200, 0);
        assert_eq!(feats.len(), 1);
        assert_eq!(feats[0].id, "kept");
        assert_eq!((feats[0].start, feats[0].end), (199, 299));
    }
}
