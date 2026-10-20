//! Per-strand sequence text colors.
//!
//! Storage formats:
//! - GBK: one `misc_feature` carrier with `/libregene_textcolor_top` /
//!   `/libregene_textcolor_bottom` qualifiers, each a `;`-separated list of
//!   `start..end:#rrggbb` runs, 1-based inclusive like every other GBK
//!   coordinate. Unlisted positions keep the default text color.
//! - SnapGene `.dna`: block 0x14 `<StrandColors>` XML, 0-based inclusive
//!   `start..end` ranges with named colors (see [`snapgene_name_to_hex`]).
//!
//! Both are normalized through [`canonicalize`] before hitting the model or
//! disk, so the stored form is always sorted, clamped, non-overlapping and
//! free of adjacent same-color duplicates.

use crate::file_io::color::normalize_color;
use crate::models::TextColorRange;

/// Normalize a run list into its canonical form: clamped to
/// `[0, seq_len-1]`, overlaps resolved last-write-wins (later entries
/// override earlier ones), sorted by start, adjacent same-color runs merged,
/// empty/invalid runs dropped.
pub fn canonicalize(ranges: &[TextColorRange], seq_len: i64) -> Vec<TextColorRange> {
    let mut out: Vec<TextColorRange> = Vec::new();
    for r in ranges {
        let s = r.start.max(0);
        let e = r.end.min(seq_len - 1);
        let color = normalize_color(&r.color);
        if s > e || color.is_empty() {
            continue;
        }
        // Carve the new run out of the existing ones (later write wins).
        let mut next: Vec<TextColorRange> = Vec::with_capacity(out.len());
        for o in out {
            if o.end < s || o.start > e {
                next.push(o);
                continue;
            }
            if o.start < s {
                next.push(TextColorRange {
                    start: o.start,
                    end: s - 1,
                    color: o.color.clone(),
                });
            }
            if o.end > e {
                next.push(TextColorRange {
                    start: e + 1,
                    end: o.end,
                    color: o.color,
                });
            }
        }
        out = next;
        out.push(TextColorRange {
            start: s,
            end: e,
            color,
        });
    }
    out.sort_by_key(|r| r.start);
    let mut merged: Vec<TextColorRange> = Vec::with_capacity(out.len());
    for r in out {
        if let Some(last) = merged.last_mut() {
            if last.color == r.color && last.end + 1 >= r.start {
                last.end = last.end.max(r.end);
                continue;
            }
        }
        merged.push(r);
    }
    merged
}

/// Encode canonical 0-based runs as the GBK qualifier value:
/// `start..end:#rrggbb; ...` with 1-based inclusive coordinates.
pub fn encode_gbk_value(ranges: &[TextColorRange]) -> String {
    ranges
        .iter()
        .map(|r| format!("{}..{}:{}", r.start + 1, r.end + 1, r.color))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Decode a GBK qualifier value back into 0-based runs. The writer line-wraps
/// qualifier values, so callers must rejoin wrapped lines first (the token
/// alphabet contains no whitespace, so stripping all of it is safe).
/// Unparseable tokens are skipped.
pub fn decode_gbk_value(value: &str) -> Vec<TextColorRange> {
    let joined: String = value.split_whitespace().collect();
    let mut out = Vec::new();
    for token in joined.split(';') {
        let Some((range, color)) = token.split_once(':') else {
            continue;
        };
        let Some((s, e)) = range.split_once("..") else {
            continue;
        };
        let (Ok(s1), Ok(e1)) = (s.parse::<i64>(), e.parse::<i64>()) else {
            continue;
        };
        let color = normalize_color(color);
        if color.is_empty() {
            continue;
        }
        out.push(TextColorRange {
            start: s1 - 1,
            end: e1 - 1,
            color,
        });
    }
    out
}

/// Map a SnapGene `<StrandColors>` color name to hex. SnapGene offers a fixed
/// named palette for text colors; unknown names return `None` and the run is
/// dropped.
pub fn snapgene_name_to_hex(name: &str) -> Option<&'static str> {
    match name.trim().to_ascii_lowercase().as_str() {
        "black" => Some("#000000"),
        "gray" | "grey" => Some("#808080"),
        "white" => Some("#ffffff"),
        "red" => Some("#ff0000"),
        "orange" => Some("#ffa500"),
        "yellow" => Some("#e6e600"),
        "green" => Some("#00b050"),
        "cyan" => Some("#00b0f0"),
        "blue" => Some("#0070c0"),
        "purple" => Some("#7030a0"),
        "magenta" => Some("#ff00ff"),
        "pink" => Some("#ffc0cb"),
        "brown" => Some("#a52a2a"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(start: i64, end: i64, color: &str) -> TextColorRange {
        TextColorRange {
            start,
            end,
            color: color.to_string(),
        }
    }

    #[test]
    fn canonicalize_clamps_and_drops_invalid() {
        let out = canonicalize(&[r(-5, 10, "#FF0000"), r(20, 5, "#00ff00")], 100);
        assert_eq!(out, vec![r(0, 10, "#ff0000")]);
    }

    #[test]
    fn canonicalize_later_write_wins_and_splits() {
        let out = canonicalize(
            &[r(10, 50, "#ff0000"), r(20, 30, "#00ff00")],
            100,
        );
        assert_eq!(
            out,
            vec![r(10, 19, "#ff0000"), r(20, 30, "#00ff00"), r(31, 50, "#ff0000")]
        );
    }

    #[test]
    fn canonicalize_merges_adjacent_same_color() {
        let out = canonicalize(&[r(0, 9, "#ff0000"), r(10, 20, "#ff0000")], 100);
        assert_eq!(out, vec![r(0, 20, "#ff0000")]);
    }

    #[test]
    fn canonicalize_keeps_adjacent_distinct_colors() {
        let out = canonicalize(&[r(0, 9, "#ff0000"), r(10, 20, "#00ff00")], 100);
        assert_eq!(out, vec![r(0, 9, "#ff0000"), r(10, 20, "#00ff00")]);
    }

    #[test]
    fn gbk_value_roundtrip() {
        let ranges = vec![r(449, 1168, "#00b050"), r(2381, 3241, "#0070c0")];
        let encoded = encode_gbk_value(&ranges);
        assert_eq!(encoded, "450..1169:#00b050; 2382..3242:#0070c0");
        assert_eq!(decode_gbk_value(&encoded), ranges);
    }

    #[test]
    fn gbk_decode_tolerates_wrapped_lines_and_junk() {
        // gb-io wraps qualifier values; newlines/indentation must not matter.
        let v = "450..1169:#00b050;\n                     2382..3242:#0070c0; bogus; 1..x:#fff";
        assert_eq!(
            decode_gbk_value(v),
            vec![r(449, 1168, "#00b050"), r(2381, 3241, "#0070c0")]
        );
    }

    #[test]
    fn snapgene_names_map_to_hex() {
        assert_eq!(snapgene_name_to_hex("green"), Some("#00b050"));
        assert_eq!(snapgene_name_to_hex(" Orange "), Some("#ffa500"));
        assert_eq!(snapgene_name_to_hex("chartreuse"), None);
    }
}
