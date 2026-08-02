//! ORF search — ports `findOrfs` from `src/plugins/orf/index.js`.
//!
//! Scans both strands in all three reading frames for ATG→stop ORFs and
//! returns them as virtual CDS features (display only, never persisted).
//! Circular templates are scanned twice so origin-wrapping ORFs are found;
//! a dedup set keeps each (strand, start, end) feature unique.

use std::collections::HashSet;

use crate::models::{Feature, Segment};

const ORF_FWD: &str = "#8BB29A";
const ORF_REV: &str = "#A58AC6";

fn rev_comp(seq: &[u8]) -> Vec<u8> {
    seq.iter()
        .rev()
        .map(|&b| match b {
            b'A' => b'T',
            b'T' => b'A',
            b'G' => b'C',
            b'C' => b'G',
            _ => b'N',
        })
        .collect()
}

/// Scan one strand (5'→3') for ORFs, mirroring the JS `scanStrand`.
///
/// Returns (start, end) pairs in strand coordinates, 0-based inclusive, where
/// end may exceed `tlen` for ORFs wrapping the origin of a circular template.
/// The first in-frame ATG after a stop is used as the start (longest ORF per
/// stop codon); starts found in the second lap are dropped because their
/// first-lap twin is already reported.
fn scan_strand(seq: &[u8], circular: bool, min_aa: usize) -> Vec<(usize, usize)> {
    let tlen = seq.len();
    let limit = if circular { tlen * 2 } else { tlen };
    let mut orfs = Vec::new();
    for frame in 0..3usize {
        let mut orf_start: i64 = -1;
        let mut p = frame;
        while p + 2 < limit {
            let codon = [seq[p % tlen], seq[(p + 1) % tlen], seq[(p + 2) % tlen]];
            if codon == *b"ATG" {
                if orf_start < 0 {
                    orf_start = p as i64;
                }
            } else if codon == *b"TAA" || codon == *b"TAG" || codon == *b"TGA" {
                if orf_start >= 0 {
                    if (p as i64 - orf_start) / 3 >= min_aa as i64 && orf_start < tlen as i64 {
                        orfs.push((orf_start as usize, p + 2));
                    }
                    orf_start = -1;
                }
            }
            p += 3;
        }
    }
    orfs
}

/// Find ORFs on both strands of `sequence`.
///
/// `topology` is "circular" (default behavior) or "linear"; ORFs shorter than
/// `min_aa` in-frame codons are dropped (the JS plugin hardcodes 75). Returns
/// virtual `Feature` values shaped like the JS plugin's output: `id`
/// `orf-<strand><start>:<end>`, name `ORF <start+1>..<end+1>`, ftype `CDS`,
/// 0-based inclusive start/end, per-strand colors, and `qualifiers`
/// `[("orf", "true")]` marking the virtual nature.
pub fn find_orfs(sequence: &str, topology: &str, min_aa: usize) -> Vec<Feature> {
    let seq = sequence.to_ascii_uppercase();
    let tlen = seq.len();
    if tlen < min_aa.saturating_add(1).saturating_mul(3) {
        return Vec::new();
    }
    let circular = topology != "linear";
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    let mut push = |strand: &str, lo: i64, hi: i64, segments: Vec<Segment>| {
        let key = format!("{strand}{lo}:{hi}");
        if !seen.insert(key) {
            return;
        }
        let color = if strand == "+" { ORF_FWD } else { ORF_REV };
        out.push(Feature {
            id: format!("orf-{strand}{lo}:{hi}"),
            name: format!("ORF {}..{}", lo + 1, hi + 1),
            start: lo,
            end: hi,
            color: color.to_string(),
            ftype: "CDS".to_string(),
            segments,
            strand: strand.to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: vec![("orf".to_string(), "true".to_string())],
        });
    };

    for (start, end) in scan_strand(seq.as_bytes(), circular, min_aa) {
        let s = start % tlen;
        let e = end % tlen;
        let segments = if end < tlen {
            vec![Segment {
                start: s as i64,
                end: e as i64,
                color: Some(ORF_FWD.into()),
            }]
        } else {
            vec![
                Segment {
                    start: s as i64,
                    end: tlen as i64 - 1,
                    color: Some(ORF_FWD.into()),
                },
                Segment {
                    start: 0,
                    end: e as i64,
                    color: Some(ORF_FWD.into()),
                },
            ]
        };
        push("+", s as i64, e as i64, segments);
    }

    let rc = rev_comp(seq.as_bytes());
    for (start, end) in scan_strand(&rc, circular, min_aa) {
        // rc index i maps to template index tlen-1-i.
        let ts = tlen - 1 - (start % tlen); // template position of the 5' end
        let te = tlen - 1 - (end % tlen); // template position of the 3' end
        let segments = if end < tlen {
            vec![Segment {
                start: te as i64,
                end: ts as i64,
                color: Some(ORF_REV.into()),
            }]
        } else {
            vec![
                Segment {
                    start: te as i64,
                    end: tlen as i64 - 1,
                    color: Some(ORF_REV.into()),
                },
                Segment {
                    start: 0,
                    end: ts as i64,
                    color: Some(ORF_REV.into()),
                },
            ]
        };
        push("-", te as i64, ts as i64, segments);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs(feature: &Feature) -> Vec<(i64, i64, Option<String>)> {
        feature
            .segments
            .iter()
            .map(|s| (s.start, s.end, s.color.clone()))
            .collect()
    }

    #[test]
    fn forward_orfs_both_strands() {
        // Forward: ATG + 3 codons + TAA → one ORF 0..14 on '+'.
        let seq = "ATGGGGAAACCCTAA";
        let features = find_orfs(seq, "circular", 3);
        assert_eq!(features.len(), 1);
        let f = &features[0];
        assert_eq!(f.id, "orf-+0:14");
        assert_eq!(f.name, "ORF 1..15");
        assert_eq!(f.ftype, "CDS");
        assert_eq!(f.strand, "+");
        assert_eq!(f.start, 0);
        assert_eq!(f.end, 14);
        assert_eq!(f.color, ORF_FWD);
        assert_eq!(segs(f), vec![(0, 14, Some(ORF_FWD.into()))]);
        assert_eq!(f.qualifiers, vec![("orf".to_string(), "true".to_string())]);

        // 4 codons: still ≥ min_aa 4; min_aa 5 drops it.
        assert_eq!(find_orfs(seq, "circular", 4).len(), 1);
        assert!(find_orfs(seq, "circular", 5).is_empty());
    }

    #[test]
    fn reverse_strand_orf() {
        // Reverse complement is ATG GGC AAT TAA → ORF 0..11 in rc coordinates,
        // mapping to template positions 0..11 on '-'.
        let seq = "TTAATTGCCCAT";
        let features = find_orfs(seq, "circular", 3);
        assert_eq!(features.len(), 1);
        let f = &features[0];
        assert_eq!(f.id, "orf--0:11");
        assert_eq!(f.name, "ORF 1..12");
        assert_eq!(f.strand, "-");
        assert_eq!(f.start, 0);
        assert_eq!(f.end, 11);
        assert_eq!(f.color, ORF_REV);
        assert_eq!(segs(f), vec![(0, 11, Some(ORF_REV.into()))]);
    }

    #[test]
    fn wrap_around_circular_orf() {
        // ORF starts at 9 (ATG), wraps past the origin, stop TAA at 3..5.
        let seq = "GGGTAACCTATG";
        let features = find_orfs(seq, "circular", 2);
        assert_eq!(features.len(), 1);
        let f = &features[0];
        assert_eq!(f.id, "orf-+9:5");
        assert_eq!(f.name, "ORF 10..6");
        assert_eq!(f.strand, "+");
        assert_eq!(f.start, 9);
        assert_eq!(f.end, 5);
        assert_eq!(
            segs(f),
            vec![(9, 11, Some(ORF_FWD.into())), (0, 5, Some(ORF_FWD.into()))]
        );

        // Linear scan never reaches the wrapping stop → no ORFs.
        assert!(find_orfs(seq, "linear", 2).is_empty());
    }

    #[test]
    fn below_min_length_rejected() {
        // The wrap ORF above spans 2 in-frame codons; min_aa 3 drops it.
        let seq = "GGGTAACCTATG";
        assert!(find_orfs(seq, "circular", 3).is_empty());
    }

    #[test]
    fn early_length_guard() {
        // Same guard as JS: sequence too short for any ORF of min_aa → empty.
        assert!(find_orfs("ATGGGGAAACCCTAA", "circular", 75).is_empty());
        // Boundary: exactly (min_aa+1)*3 is long enough to scan.
        assert_eq!(find_orfs("ATGGGGAAACCCTAA", "circular", 4).len(), 1);
    }

    #[test]
    fn empty_and_lowercase_inputs() {
        assert!(find_orfs("", "circular", 1).is_empty());
        let features = find_orfs("atggggaaaccctaa", "linear", 3);
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].id, "orf-+0:14");
    }

    #[test]
    fn matches_js_find_orfs_on_designed_sequences() {
        // Differential fixtures: sequences built exactly like the node probe,
        // expected output captured from the real JS findOrfs (MIN_AA = 75).
        let filler = "C".repeat(300);
        let orf = format!("ATG{}TAA", "GCA".repeat(75));

        // Forward ORF at 69..299, non-wrapping.
        let fwd = format!("{}{}", &filler[..69], orf);
        let f = find_orfs(&fwd, "circular", 75);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].id, "orf-+69:299");
        assert_eq!(f[0].name, "ORF 70..300");
        assert_eq!(f[0].strand, "+");
        assert_eq!((f[0].start, f[0].end), (69, 299));
        assert_eq!(segs(&f[0]), vec![(69, 299, Some(ORF_FWD.into()))]);

        // Reverse-strand ORF: rc(orf) placed at 40.
        let t = String::from_utf8(rev_comp(orf.as_bytes())).unwrap();
        let rev = format!("{}{}{}", &filler[..40], t, &filler[40 + t.len()..]);
        let f = find_orfs(&rev, "circular", 75);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].id, "orf--40:270");
        assert_eq!(f[0].name, "ORF 41..271");
        assert_eq!(f[0].strand, "-");
        assert_eq!((f[0].start, f[0].end), (40, 270));
        assert_eq!(segs(&f[0]), vec![(40, 270, Some(ORF_REV.into()))]);

        // Origin-wrapping ORF: ATG at 290, 74 GCA codons across the origin,
        // stop TAA at 215..217.
        let mut tpl: Vec<char> = vec!['C'; 300];
        let orf_linear = format!("ATG{}TAA", "GCA".repeat(74));
        for (i, ch) in orf_linear.chars().enumerate() {
            tpl[(290 + i) % 300] = ch;
        }
        let wrap: String = tpl.iter().collect();
        let f = find_orfs(&wrap, "circular", 75);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].id, "orf-+290:217");
        assert_eq!(f[0].name, "ORF 291..218");
        assert_eq!(f[0].strand, "+");
        assert_eq!((f[0].start, f[0].end), (290, 217));
        assert_eq!(
            segs(&f[0]),
            vec![
                (290, 299, Some(ORF_FWD.into())),
                (0, 217, Some(ORF_FWD.into()))
            ]
        );
        assert!(find_orfs(&wrap, "linear", 75).is_empty());

        // Stop-free periodic template: no ORFs on either topology (JS agrees).
        let periodic = "ATGC".repeat(75);
        assert!(find_orfs(&periodic, "circular", 75).is_empty());
        assert!(find_orfs(&periodic, "linear", 75).is_empty());
    }
}
