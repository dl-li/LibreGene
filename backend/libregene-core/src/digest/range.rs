use crate::models::{Enzyme, Feature, ProjectData};

// ---------------------------------------------------------------------------
// Range helpers (internal 0-based inclusive; circular wrap when start > end)
// ---------------------------------------------------------------------------

/// 1-based inclusive flanking bases of a cut at internal 0-based index C
/// (severing between bases C-1 and C): (C, C+1). A cut at the origin of a
/// circular molecule (C == 0) sits between the last and the first base.
pub fn cut_flanks(cut: i64, len: i64, circular: bool) -> (i64, i64) {
    if cut == 0 && circular {
        (len, 1)
    } else {
        (cut, cut + 1)
    }
}

/// Cut rendered as `N^M` — between the 1-based bases N and M.
pub fn cut_notation(cut: i64, len: i64, circular: bool) -> String {
    let (a, b) = cut_flanks(cut, len, circular);
    format!("{}^{}", a, b)
}

pub(crate) fn pos_in_range(p: i64, s: i64, e: i64, circular: bool) -> bool {
    if s <= e {
        p >= s && p <= e
    } else {
        circular && (p >= s || p <= e)
    }
}

pub(crate) fn seg_in_range(seg_s: i64, seg_e: i64, s: i64, e: i64, circular: bool) -> bool {
    if s <= e {
        seg_s <= e && seg_e >= s
    } else {
        circular && (seg_e >= s || seg_s <= e)
    }
}

pub(crate) fn validate_range(project: &ProjectData, start: i64, end: i64) -> Result<(i64, i64), String> {
    if project.length <= 0 || project.sequence.is_empty() {
        return Err("Sequence is empty".to_string());
    }
    let circular = project.topology == "circular";
    if start > end && !circular {
        return Err(
            "start > end is only allowed on circular sequences (wraps the origin)".to_string(),
        );
    }
    let len = project.length;
    if start < 0 || end < 0 || start >= len || end >= len {
        return Err(format!(
            "range {}..{} out of bounds for sequence of length {} (1-based inclusive)",
            start + 1,
            end + 1,
            len
        ));
    }
    Ok((start, end))
}

pub(crate) fn feature_in_region(f: &Feature, s: i64, e: i64, circular: bool) -> bool {
    if f.segments.is_empty() {
        seg_in_range(f.start, f.end, s, e, circular)
    } else {
        f.segments
            .iter()
            .any(|seg| seg_in_range(seg.start, seg.end, s, e, circular))
    }
}

pub(crate) fn enzyme_in_region(en: &Enzyme, s: i64, e: i64, circular: bool) -> bool {
    let cuts: Vec<i64> = if en.cut_pairs.is_empty() {
        vec![en.cut_index, en.bot_cut_index]
    } else {
        en.cut_pairs
            .iter()
            .flat_map(|p| [p.top_cut_index, p.bot_cut_index])
            .collect()
    };
    cuts.iter().any(|&c| pos_in_range(c, s, e, circular))
}
