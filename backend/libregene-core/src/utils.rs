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

/// Map one 0-based inclusive span after replacing `[edit_start, edit_end]`
/// with `new_len` bases; `None` when the span collapses to nothing. Shared by
/// `adjust_features_for_edit` and `features_edit_impact` so both stay in sync.
fn adjust_span(
    s: i64,
    e: i64,
    edit_start: i64,
    edit_end: i64,
    new_len: i64,
) -> Option<(i64, i64)> {
    let delta = new_len - (edit_end - edit_start + 1);
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
}

/// Shift/clip feature coordinates after replacing `[edit_start, edit_end]`
/// (0-based inclusive) with `new_len` bases. A pure insertion is
/// `edit_end = edit_start - 1`. An equal-length replacement
/// (`new_len == edit_end - edit_start + 1`) leaves every feature untouched —
/// all coordinates stay valid, including features fully inside the replaced
/// span (e.g. case normalization or point mutations must not delete them).
/// Mirrors the frontend `adjustAnnotations` for length-changing edits.
pub fn adjust_features_for_edit(
    features: &mut Vec<crate::models::Feature>,
    edit_start: i64,
    edit_end: i64,
    new_len: i64,
) {
    let old_len = edit_end - edit_start + 1;
    let delta = new_len - old_len;
    if delta == 0 {
        return;
    }

    features.retain_mut(|f| {
        if f.segments.is_empty() {
            match adjust_span(f.start, f.end, edit_start, edit_end, new_len) {
                Some((s, e)) => {
                    f.start = s;
                    f.end = e;
                    true
                }
                None => false,
            }
        } else {
            f.segments.retain_mut(|seg| {
                match adjust_span(seg.start, seg.end, edit_start, edit_end, new_len) {
                    Some((s, e)) => {
                        seg.start = s;
                        seg.end = e;
                        true
                    }
                    None => false,
                }
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

/// Side effects of an edit on feature coordinates, derived with the same span
/// math as [`adjust_features_for_edit`]: features fully inside the deleted or
/// replaced span are `removed`, and features whose span changed other than a
/// pure translation (a boundary clipped or the length altered) are `clipped`.
/// Equal-length replacements keep every feature (coordinates unchanged), so
/// both lists are empty there.
pub fn features_edit_impact(
    features: &[crate::models::Feature],
    edit_start: i64,
    edit_end: i64,
    new_len: i64,
) -> crate::models::FeaturesEditImpact {
    let mut impact = crate::models::FeaturesEditImpact::default();
    let delta = new_len - (edit_end - edit_start + 1);
    if delta == 0 {
        return impact;
    }
    for f in features {
        let mut new_spans: Vec<(i64, i64)> = Vec::new();
        if f.segments.is_empty() {
            if let Some((s, e)) = adjust_span(f.start, f.end, edit_start, edit_end, new_len) {
                new_spans.push((s, e));
            }
        } else {
            for seg in &f.segments {
                if let Some((s, e)) = adjust_span(seg.start, seg.end, edit_start, edit_end, new_len)
                {
                    new_spans.push((s, e));
                }
            }
        }
        let Some((ns, ne)) = new_spans
            .iter()
            .map(|&(s, e)| (s, e))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
        else {
            impact.removed_features.push(crate::models::RemovedFeatureImpact {
                name: f.name.clone(),
                ftype: f.ftype.clone(),
                location: format!("{}..{}", f.start, f.end),
            });
            continue;
        };
        // A pure translation moves both boundaries by the same delta (length
        // preserved); anything else is a boundary change worth reporting.
        if (ns - f.start) != (ne - f.end) {
            impact.clipped_features.push(crate::models::ClippedFeatureImpact {
                name: f.name.clone(),
                ftype: f.ftype.clone(),
                before: crate::models::EditSpan {
                    start: f.start,
                    end: f.end,
                },
                after: crate::models::EditSpan { start: ns, end: ne },
            });
        }
    }
    impact
}

/// Rebase features parsed from a replacement file onto the region they were
/// inserted at: local coordinates `[0, insert_len)` become
/// `[offset, offset + insert_len)` (0-based inclusive), clipped to the
/// inserted span. With `reverse` (the replacement was reverse-complemented
/// before insertion) coordinates are mirrored, strands flipped and segments
/// reversed. Translations are dropped (recomputed downstream); ids are
/// regenerated in the `name_start` convention to avoid collisions.
pub fn transfer_features_for_insert(
    features: &[crate::models::Feature],
    insert_len: i64,
    offset: i64,
    reverse: bool,
) -> Vec<crate::models::Feature> {
    let map_span = |s: i64, e: i64| -> Option<(i64, i64)> {
        let s = s.max(0);
        let e = e.min(insert_len - 1);
        if s > e {
            return None;
        }
        let (s, e) = if reverse {
            (insert_len - 1 - e, insert_len - 1 - s)
        } else {
            (s, e)
        };
        Some((s + offset, e + offset))
    };
    let flip_strand = |strand: &str| -> String {
        match strand {
            "+" => "-".to_string(),
            "-" => "+".to_string(),
            other => other.to_string(),
        }
    };

    let mut out = Vec::new();
    for f in features {
        // (source color, mapped span) pairs, so per-segment colors survive
        // clipping and reversal without fragile index math.
        let mut mapped: Vec<(Option<String>, (i64, i64))> = if f.segments.is_empty() {
            map_span(f.start, f.end)
                .map(|span| (None, span))
                .into_iter()
                .collect()
        } else {
            f.segments
                .iter()
                .filter_map(|s| map_span(s.start, s.end).map(|span| (s.color.clone(), span)))
                .collect()
        };
        if mapped.is_empty() {
            continue;
        }
        if reverse {
            mapped.reverse();
        }
        let mut nf = f.clone();
        if f.segments.is_empty() {
            nf.start = mapped[0].1 .0;
            nf.end = mapped[0].1 .1;
        } else {
            nf.segments = mapped
                .into_iter()
                .map(|(color, (start, end))| crate::models::Segment { start, end, color })
                .collect();
            nf.start = nf.segments.iter().map(|s| s.start).min().unwrap();
            nf.end = nf.segments.iter().map(|s| s.end).max().unwrap();
        }
        if reverse {
            nf.strand = flip_strand(&f.strand);
        }
        nf.translation = String::new();
        nf.id = format!("{}_{}", nf.name, nf.start);
        out.push(nf);
    }
    out
}

/// Pick `desired`, or `desired (2)`, `desired (3)`, … when taken.
pub fn unique_name(desired: &str, taken: &std::collections::HashSet<String>) -> String {
    if !taken.contains(desired) {
        return desired.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{} ({})", desired, n);
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
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

    #[test]
    fn impact_pure_deletion_removes_inner_feature() {
        let feats = vec![
            feat("before", 0, 50, vec![]),
            feat("inside", 110, 150, vec![]),
            feat("after", 200, 300, vec![]),
        ];
        let impact = features_edit_impact(&feats, 100, 199, 0);
        assert_eq!(impact.removed_features.len(), 1);
        assert_eq!(impact.removed_features[0].name, "inside");
        assert_eq!(impact.removed_features[0].ftype, "CDS");
        assert_eq!(impact.removed_features[0].location, "110..150");
        assert!(impact.clipped_features.is_empty());
    }

    #[test]
    fn impact_replace_clips_boundary_features() {
        let feats = vec![
            feat("left_overlap", 50, 150, vec![]),
            feat("right_overlap", 150, 250, vec![]),
            feat("after", 300, 400, vec![]),
        ];
        let impact = features_edit_impact(&feats, 100, 199, 10);
        assert!(impact.removed_features.is_empty());
        assert_eq!(impact.clipped_features.len(), 2);
        let l = impact
            .clipped_features
            .iter()
            .find(|c| c.name == "left_overlap")
            .unwrap();
        assert_eq!((l.before.start, l.before.end), (50, 150));
        assert_eq!((l.after.start, l.after.end), (50, 109));
        let r = impact
            .clipped_features
            .iter()
            .find(|c| c.name == "right_overlap")
            .unwrap();
        assert_eq!((r.before.start, r.before.end), (150, 250));
        assert_eq!((r.after.start, r.after.end), (110, 160));
    }

    #[test]
    fn impact_pure_insertion_shift_is_not_clipped() {
        let feats = vec![
            feat("at", 100, 120, vec![]),
            feat("after", 200, 300, vec![]),
        ];
        let impact = features_edit_impact(&feats, 100, 99, 10);
        assert!(impact.removed_features.is_empty());
        assert!(impact.clipped_features.is_empty());
    }

    #[test]
    fn impact_segmented_feature_clip_and_remove() {
        let feats = vec![feat("seg", 50, 300, vec![(50, 120), (130, 150), (200, 300)])];
        let impact = features_edit_impact(&feats, 100, 199, 0);
        assert!(impact.removed_features.is_empty());
        assert_eq!(impact.clipped_features.len(), 1);
        let c = &impact.clipped_features[0];
        assert_eq!((c.before.start, c.before.end), (50, 300));
        assert_eq!((c.after.start, c.after.end), (50, 200));
    }

    #[test]
    fn impact_all_segments_removed_drops_feature() {
        let feats = vec![
            feat("gone", 100, 200, vec![(100, 150), (160, 200)]),
            feat("kept", 300, 400, vec![(300, 400)]),
        ];
        let impact = features_edit_impact(&feats, 100, 200, 0);
        assert_eq!(impact.removed_features.len(), 1);
        assert_eq!(impact.removed_features[0].name, "gone");
        assert_eq!(impact.removed_features[0].location, "100..200");
        assert!(impact.clipped_features.is_empty());
    }

    #[test]
    fn impact_noop_edit_is_empty() {
        let feats = vec![feat("a", 0, 50, vec![])];
        let impact = features_edit_impact(&feats, 100, 99, 0);
        assert!(impact.removed_features.is_empty());
        assert!(impact.clipped_features.is_empty());
    }

    #[test]
    fn adjust_equal_length_replacement_keeps_all_features() {
        // Replace [100..199] with 100 new bases (e.g. case normalization or
        // point mutations): coordinates stay valid, nothing is removed or
        // clipped — including features fully inside the replaced span.
        let mut feats = vec![
            feat("before", 0, 50, vec![]),
            feat("inside", 110, 150, vec![]),
            feat("spanning", 50, 250, vec![]),
            feat("seg", 60, 300, vec![(60, 120), (180, 300)]),
            feat("after", 200, 300, vec![]),
        ];
        adjust_features_for_edit(&mut feats, 100, 199, 100);
        assert_eq!(feats.len(), 5);
        assert_eq!((feats[1].start, feats[1].end), (110, 150));
        assert_eq!((feats[2].start, feats[2].end), (50, 250));
        let spans: Vec<(i64, i64)> = feats[3].segments.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(spans, vec![(60, 120), (180, 300)]);
        assert_eq!((feats[4].start, feats[4].end), (200, 300));
    }

    #[test]
    fn impact_equal_length_replacement_is_empty() {
        let feats = vec![
            feat("inside", 110, 150, vec![]),
            feat("spanning", 50, 250, vec![]),
        ];
        let impact = features_edit_impact(&feats, 100, 199, 100);
        assert!(impact.removed_features.is_empty());
        assert!(impact.clipped_features.is_empty());
    }

    #[test]
    fn transfer_plain_offset() {
        let feats = vec![
            feat("gene", 10, 90, vec![]),
            feat("outside", 500, 600, vec![]),
        ];
        let out = transfer_features_for_insert(&feats, 100, 1000, false);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].start, out[0].end), (1010, 1090));
        assert_eq!(out[0].id, "gene_1010");
    }

    #[test]
    fn transfer_clips_to_insert_span() {
        let feats = vec![feat("overhang", 90, 150, vec![])];
        let out = transfer_features_for_insert(&feats, 100, 0, false);
        assert_eq!((out[0].start, out[0].end), (90, 99));
    }

    #[test]
    fn transfer_reverse_mirrors_and_flips() {
        // 100 bp insert; feature [10..19] on "+" becomes [80..89] on "-".
        let mut f = feat("gene", 10, 19, vec![]);
        f.translation = "MKT".to_string();
        let out = transfer_features_for_insert(&[f], 100, 1000, true);
        assert_eq!((out[0].start, out[0].end), (1080, 1089));
        assert_eq!(out[0].strand, "-");
        assert!(out[0].translation.is_empty());
    }

    #[test]
    fn transfer_reverse_reverses_segments() {
        // seg colors must follow their segment through the reversal.
        let mut f = feat("seg", 0, 99, vec![(0, 29), (70, 99)]);
        f.segments[0].color = Some("#111111".to_string());
        f.segments[1].color = Some("#222222".to_string());
        let out = transfer_features_for_insert(&[f], 100, 0, true);
        let spans: Vec<(i64, i64)> = out[0].segments.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(spans, vec![(0, 29), (70, 99)]);
        assert_eq!(out[0].segments[0].color.as_deref(), Some("#222222"));
        assert_eq!(out[0].segments[1].color.as_deref(), Some("#111111"));
    }

    #[test]
    fn unique_name_appends_counter() {
        let taken: std::collections::HashSet<String> =
            ["gfp".to_string(), "gfp (2)".to_string()].into_iter().collect();
        assert_eq!(unique_name("gfp", &taken), "gfp (3)");
        assert_eq!(unique_name("rfp", &taken), "rfp");
    }
}
