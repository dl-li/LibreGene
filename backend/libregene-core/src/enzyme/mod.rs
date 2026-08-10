//! Enzyme engine — recognition site detection and precise cut coordinate computation.
//!
//! For each enzyme in the database:
//! 1. Search for all recognition sites on both strands via IUPAC regex.
//! 2. Compute absolute cut positions using fst5/fst3 (and scd5/scd3 for cut-twice).
//! 3. Normalize coordinates for circular sequences.
//! 4. Apply methylation filtering.
//!
//! ## Cut position formulas (0-based template coordinates)
//!
//! fst5 = distance from 5' end of recognition to top-strand cut.
//! fst3 = distance from 3' end of recognition to bottom-strand cut.
//!
//! **Top-strand recognition:**
//!   top_cut = rec_start + fst5
//!   bot_cut = rec_start + rec_len + fst3
//!
//! **Bottom-strand recognition:**
//!   top_cut = rec_start - fst3
//!   bot_cut = rec_start + rec_len - fst5

pub mod data;
pub mod matching;
pub mod methylation;
pub mod search;

use rayon::prelude::*;
use crate::models::{CutPair, Enzyme, ProjectData, Segment};

/// A recognition site hit found on the template.
#[derive(Debug, Clone)]
struct SiteHit {
    /// Absolute template position of the rec start (0-based, inclusive).
    rec_start: usize,
    /// Whether this hit is on the reverse-complement (bottom) strand.
    is_bottom: bool,
}

/// Recompute all enzyme sites for the current project.
pub fn recompute(project: &mut ProjectData) {
    // Restriction sites are a DNA concept — single-strand molecules never get them.
    if !project.is_dna() {
        project.enzymes.clear();
        return;
    }
    if project.sequence.len() < 4 {
        project.enzymes.clear();
        return;
    }

    let db = search::get_db();
    let seq = project.sequence.as_bytes();
    let seq_str = &project.sequence;
    let is_circular = project.topology == "circular";
    let seq_len = seq.len() as i64;

    // Precompute extended sequence for circular search (avoids O(N) allocations).
    let max_site_len = db.enzymes.iter().map(|e| e.site.len()).max().unwrap_or(0);
    let ext_seq: Option<Vec<u8>> = if is_circular && max_site_len > 1 {
        let wrap = max_site_len - 1;
        let mut ext = Vec::with_capacity(seq.len() + wrap);
        ext.extend_from_slice(seq);
        ext.extend_from_slice(&seq[..wrap.min(seq.len())]);
        Some(ext)
    } else {
        None
    };

    let mut result: Vec<Enzyme> = db
        .enzymes
        .par_iter()
        .flat_map(|record| {
            process_enzyme(record, seq, seq_str, is_circular, seq_len, &ext_seq)
        })
        .collect();

    // Apply methylation filtering (always, to also mark dependent enzymes like DpnI).
    let systems = project.methylation_systems.clone();
    for enz in &mut result {
        methylation::apply_methylation(
            enz,
            &project.sequence,
            &systems,
            project.methylation_overlap,
        );
    }

    project.enzymes = result;
}

/// Re-apply methylation filtering only — avoids full enzyme search when only methylation changes.
pub fn recompute_methylation_only(project: &mut ProjectData) {
    let systems = project.methylation_systems.clone();
    for enz in &mut project.enzymes {
        // Reset methylation state before re-applying
        enz.methylation_blocked = false;
        enz.methylated_offsets.clear();
        enz.methylation_sources.clear();
        enz.methyl_required_offsets.clear();
        enz.methyl_required_sources.clear();
        methylation::apply_methylation(
            enz,
            &project.sequence,
            &systems,
            project.methylation_overlap,
        );
    }
}

/// Process a single enzyme record: find recognition sites and compute enzyme entries.
fn process_enzyme(
    record: &data::EnzymeRecord,
    seq: &[u8],
    seq_str: &str,
    is_circular: bool,
    seq_len: i64,
    ext_seq: &Option<Vec<u8>>,
) -> Vec<Enzyme> {
    let site = &record.site;
    let rec_len = site.len() as i64;
    let fst5 = record.fst5;
    let fst3 = record.fst3;
    let hits = find_recognition_sites(seq, site, is_circular, ext_seq);
    if hits.is_empty() {
        return Vec::new();
    }

    let unique_hits = deduplicate_hits(&hits, rec_len as usize, record.is_palindromic);
    if unique_hits.len() > 200 {
        return Vec::new();
    }

    let is_unique = unique_hits.len() == 1;

    unique_hits
        .iter()
        .enumerate()
        .map(|(enz_idx, hit)| {
            let rec_start = hit.rec_start as i64;
            let rec_end = rec_start + rec_len - 1;
            let matched_seq: String = if is_circular
                && hit.rec_start + rec_len as usize > seq.len()
            {
                // Recognition site spans the origin: manually concatenate wrapped sequence.
                let mut s = String::with_capacity(rec_len as usize);
                s.push_str(&seq_str[hit.rec_start..]);
                s.push_str(&seq_str[..(hit.rec_start + rec_len as usize) % seq.len()]);
                s
            } else {
                seq_str[hit.rec_start..hit.rec_start + rec_len as usize].to_string()
            };

            // Compute cut pairs.
            let mut pairs: Vec<CutPair> = Vec::new();

            if hit.is_bottom {
                // Bottom-strand recognition: cuts are on the "other side" of the rec.
                let p1_top = rec_start - fst3;
                let p1_bot = rec_start + rec_len - fst5;
                pairs.push(normalize_pair(p1_top, p1_bot, is_circular, seq_len));

                if let (Some(scd5), Some(scd3)) = (record.scd5, record.scd3) {
                    let p2_top = rec_start - scd3;
                    let p2_bot = rec_start + rec_len - scd5;
                    pairs.push(normalize_pair(p2_top, p2_bot, is_circular, seq_len));
                }
            } else {
                // Top-strand recognition.
                let p1_top = rec_start + fst5;
                let p1_bot = rec_start + rec_len + fst3;
                pairs.push(normalize_pair(p1_top, p1_bot, is_circular, seq_len));

                if let (Some(scd5), Some(scd3)) = (record.scd5, record.scd3) {
                    let p2_top = rec_start + scd5;
                    let p2_bot = rec_start + rec_len + scd3;
                    pairs.push(normalize_pair(p2_top, p2_bot, is_circular, seq_len));
                }
            }

            // Backwards-compat cut_index / bot_cut_index from first pair.
            let first_pair = &pairs[0];
            let cut_index = first_pair.top_cut_index;
            let bot_cut_index = first_pair.bot_cut_index;

            // Use database-stored classification rather than computing from coordinates.
            let cut_type = record.cut_type.clone();
            let cut_twice = record.is_cut_twice;

            // Display window: recognition + all cut positions.
            // Normalize rec coordinates for circular display.
            let (norm_rec_start, norm_rec_end) = normalize_rec(
                rec_start, rec_end, &pairs, is_circular, seq_len,
            );

            let mut disp_start = norm_rec_start;
            let mut disp_end = norm_rec_end;
            if is_circular {
                // Unwrap cut positions into the recognition site's coordinate
                // frame; otherwise a site near the origin yields a display
                // window spanning the whole sequence.
                for p in &pairs {
                    let t = unwrap_near(p.top_cut_index, norm_rec_start, seq_len);
                    let b = unwrap_near(p.bot_cut_index, norm_rec_start, seq_len);
                    disp_start = disp_start.min(t).min(b);
                    disp_end = disp_end.max(t).max(b);
                }
            } else {
                for p in &pairs {
                    disp_start = disp_start.min(p.top_cut_index).min(p.bot_cut_index);
                    disp_end = disp_end.max(p.top_cut_index).max(p.bot_cut_index);
                }
            }
            // Add 1bp left buffer when a cut falls at the display left edge,
            // so the cut line isn't flush against the tooltip edge.
            if is_circular || disp_start > 0 {
                for p in &pairs {
                    let t = if is_circular {
                        unwrap_near(p.top_cut_index, norm_rec_start, seq_len)
                    } else {
                        p.top_cut_index
                    };
                    let b = if is_circular {
                        unwrap_near(p.bot_cut_index, norm_rec_start, seq_len)
                    } else {
                        p.bot_cut_index
                    };
                    if t == disp_start || b == disp_start {
                        disp_start -= 1;
                        break;
                    }
                }
            }
            // For circular sequences, shift the whole frame so disp_start lands
            // in [0, seq_len); disp_end may extend past seq_len (window wraps
            // the origin), and the frontend wraps indices mod seq_len.
            let (norm_rec_start, norm_rec_end, disp_start, disp_end) = if is_circular {
                let shift = disp_start.div_euclid(seq_len) * seq_len;
                (
                    norm_rec_start - shift,
                    norm_rec_end - shift,
                    disp_start - shift,
                    disp_end - shift,
                )
            } else {
                (norm_rec_start, norm_rec_end, disp_start, disp_end)
            };
            // Build spacers: segments in the display window that are NOT part of the recognition.
            let spacers = build_spacers(norm_rec_start, norm_rec_end, disp_start, disp_end);

            // Recognition pattern.
            let rec_pattern = if hit.is_bottom {
                search::iupac_complement(site)
            } else {
                site.clone()
            };

            // Complement sequence.
            let comp = search::dna_complement(&matched_seq);

            Enzyme {
                id: format!("{}_{}_{}", record.name, hit.rec_start, enz_idx),
                name: record.name.clone(),
                rec_seq: matched_seq.to_string(),
                rec_seq_pattern: rec_pattern,
                rec_start: norm_rec_start,
                rec_end: norm_rec_end,
                display_start: disp_start,
                display_end: disp_end,
                cut_index,
                bot_cut_index,
                cut_pairs: pairs,
                recognition_strand: if hit.is_bottom {
                    "bottom".to_string()
                } else {
                    "top".to_string()
                },
                comp_seq: comp,
                spacers,
                is_unique,
                is_palindromic: record.is_palindromic,
                is_methylation_sensitive: record.is_methylation_sensitive(),
                methylation_required: record.methylation_dependent,
                methylation_blocked: false,
                methylated_offsets: Vec::new(),
                methylation_sources: Vec::new(),
                methyl_required_offsets: Vec::new(),
                methyl_required_sources: Vec::new(),
                cut_type,
                cut_twice,
                ..Default::default()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a normalized position p (in [0, seq_len)) into the coordinate frame
/// anchored at `anchor`, choosing the representative closest to anchor.
fn unwrap_near(p: i64, anchor: i64, seq_len: i64) -> i64 {
    let d = (p - anchor).rem_euclid(seq_len);
    if d > seq_len / 2 {
        anchor + d - seq_len
    } else {
        anchor + d
    }
}

/// Normalize a cut pair position to [0, seq_len) for circular sequences.
fn normalize_pair(top: i64, bot: i64, is_circular: bool, seq_len: i64) -> CutPair {
    if !is_circular {
        return CutPair {
            top_cut_index: top,
            bot_cut_index: bot,
        };
    }
    let norm = |p: i64| -> i64 { ((p % seq_len) + seq_len) % seq_len };
    CutPair {
        top_cut_index: norm(top),
        bot_cut_index: norm(bot),
    }
}

/// Normalize recognition coordinates for circular sequences.
/// If the recognition itself spans the origin, we keep the original coordinates
/// and expand the display window accordingly. Otherwise normalize.
fn normalize_rec(
    rec_start: i64,
    rec_end: i64,
    _pairs: &[CutPair],
    is_circular: bool,
    seq_len: i64,
) -> (i64, i64) {
    if !is_circular {
        return (rec_start, rec_end);
    }

    let rec_start_n = ((rec_start % seq_len) + seq_len) % seq_len;
    // rec_end comes from `rec_start + rec_len - 1` with rec_start >= 0, so it is
    // always non-negative and `% seq_len` never goes negative — no clamp needed.
    let norm_rec_end = rec_end % seq_len;

    if rec_start_n <= norm_rec_end {
        // Recognition doesn't span origin.
        return (rec_start_n, norm_rec_end);
    }

    // Recognition spans origin: shift the wrapped end to preserve total length.
    (rec_start_n, norm_rec_end + seq_len)
}

/// Build spacer segments: portions of the display window outside the recognition site.
fn build_spacers(
    rec_start: i64,
    rec_end: i64,
    disp_start: i64,
    disp_end: i64,
) -> Option<Vec<Segment>> {
    let mut segs = Vec::new();
    let rel_rec_start = rec_start - disp_start;
    let rel_rec_end = rec_end - disp_start;
    let total_range = disp_end - disp_start + 1;

    if rel_rec_start > 0 {
        segs.push(Segment {
            start: 0,
            end: rel_rec_start,
            color: None,
        });
    }
    if rel_rec_end + 1 < total_range {
        segs.push(Segment {
            start: rel_rec_end + 1,
            end: total_range,
            color: None,
        });
    }
    if segs.is_empty() {
        None
    } else {
        Some(segs)
    }
}

/// Find all recognition site starts on both strands.
/// Compiled regexes are cached globally — never recompiled.
fn find_recognition_sites(
    seq: &[u8],
    site: &str,
    is_circular: bool,
    ext_seq: &Option<Vec<u8>>,
) -> Vec<SiteHit> {
    use std::collections::HashMap;
    use std::sync::OnceLock;

    type ReCache = HashMap<String, (regex::bytes::Regex, regex::bytes::Regex)>;
    static RE_CACHE: OnceLock<ReCache> = OnceLock::new();

    let cache = RE_CACHE.get_or_init(|| {
        let db = search::get_db();
        let mut map = HashMap::new();
        for record in &db.enzymes {
            if map.contains_key(&record.site) {
                continue;
            }
            let fwd_pat = search::iupac_to_regex(&record.site);
            let rc_site = search::iupac_complement(&record.site);
            let rc_pat = search::iupac_to_regex(&rc_site);
            if let (Ok(fwd_re), Ok(rc_re)) =
                (regex::bytes::Regex::new(&fwd_pat), regex::bytes::Regex::new(&rc_pat))
            {
                map.insert(record.site.clone(), (fwd_re, rc_re));
            }
        }
        map
    });

    let (fwd_re, rc_re) = match cache.get(site) {
        Some(r) => r,
        None => return vec![],
    };

    let mut hits: Vec<SiteHit> = Vec::new();

    // For circular sequences, use the precomputed extended sequence to catch
    // sites that span the origin.
    if is_circular && site.len() > 1 {
        let ext = match ext_seq {
            Some(e) => e.as_slice(),
            None => seq,
        };

        for m in fwd_re.find_iter(ext) {
            let start = m.start();
            if start < seq.len() {
                hits.push(SiteHit { rec_start: start, is_bottom: false });
            }
        }
        for m in rc_re.find_iter(ext) {
            let start = m.start();
            if start < seq.len() {
                hits.push(SiteHit { rec_start: start, is_bottom: true });
            }
        }
    } else {
        for m in fwd_re.find_iter(seq) {
            hits.push(SiteHit { rec_start: m.start(), is_bottom: false });
        }
        for m in rc_re.find_iter(seq) {
            hits.push(SiteHit { rec_start: m.start(), is_bottom: true });
        }
    }

    hits.sort_by_key(|h| h.rec_start);
    hits
}

/// Deduplicate: for palindromic enzymes, forward and reverse hits at the same
/// position are duplicates (since site == reverse_complement).
fn deduplicate_hits(hits: &[SiteHit], _rec_len: usize, is_palindromic: bool) -> Vec<SiteHit> {
    if !is_palindromic || hits.is_empty() {
        return hits.to_vec();
    }

    let mut result: Vec<SiteHit> = Vec::new();
    let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for hit in hits {
        if seen.insert(hit.rec_start) {
            result.push(SiteHit {
                rec_start: hit.rec_start,
                is_bottom: false, // Normalize – palindromic sites are the same on both strands
            });
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Molecule-type guard
    // -------------------------------------------------------------------------

    #[test]
    fn recompute_skips_non_dna_projects() {
        let mut project = ProjectData {
            sequence: "GAATTC".repeat(3), // three EcoRI recognition sites
            topology: "circular".to_string(),
            molecule_type: "protein".to_string(),
            ..Default::default()
        };
        project.length = project.sequence.len() as i64;
        recompute(&mut project);
        assert!(
            project.enzymes.is_empty(),
            "protein projects must never get restriction sites"
        );
    }

    // -------------------------------------------------------------------------
    // Cut position formula tests
    // -------------------------------------------------------------------------

    /// Palindromic enzyme: EcoRI GAATTC, top strand
    /// fst5=1, fst3=-1, rec_len=6
    /// Expected: top_cut = rec_start + 1, bot_cut = rec_start + 5
    #[test]
    fn test_palindromic_top_strand() {
        let fst5: i64 = 1;
        let fst3: i64 = -1;
        let rec_len: i64 = 6;
        let rec_start: i64 = 3;
        // top_cut = rec_start + fst5, bot_cut = rec_start + rec_len + fst3
        assert_eq!(rec_start + fst5, 4);        // cut between 4 and 5
        assert_eq!(rec_start + rec_len + fst3, 8); // cut between 8 and 9
    }

    /// Non-palindromic enzyme BbsI (GAAGAC), top strand
    /// fst5=8, fst3=6, rec_len=6
    #[test]
    fn test_nonpal_top_strand() {
        let fst5: i64 = 8;
        let fst3: i64 = 6;
        let rec_len: i64 = 6;
        let rec_start: i64 = 100;

        let top_cut = rec_start + fst5;
        let bot_cut = rec_start + rec_len + fst3;
        assert_eq!(top_cut, 108);
        assert_eq!(bot_cut, 112);
        // 5' overhang on top strand: top_cut < bot_cut
        assert!(top_cut < bot_cut);
    }

    /// Non-palindromic enzyme BbsI, bottom strand
    /// The reverse complement match is at template position 100.
    /// top_cut = rec_start - fst3, bot_cut = rec_start + rec_len - fst5
    #[test]
    fn test_nonpal_bottom_strand() {
        let fst5: i64 = 8;
        let fst3: i64 = 6;
        let rec_len: i64 = 6;
        let rec_start: i64 = 100;

        // Standard Biopython formulas
        let top_cut_bot = rec_start - fst3;           // = 100 - 6 = 94
        let bot_cut_bot = rec_start + rec_len - fst5; // = 100 + 6 - 8 = 98

        assert_eq!(top_cut_bot, 94);
        assert_eq!(bot_cut_bot, 98);
        // Still 5' overhang on template: top_cut < bot_cut
        assert!(top_cut_bot < bot_cut_bot);

        // Verify old swap formula gives DIFFERENT result
        let old_top = rec_start + (rec_len + fst3);  // = 100 + 12 = 112
        let old_bot = rec_start + fst5;               // = 100 + 8 = 108
        assert_ne!(top_cut_bot, old_top);
        assert_ne!(bot_cut_bot, old_bot);
    }

    /// Cut-twice enzyme BaeI (ACNNNNGTAYC), top strand
    /// fst5=-10, fst3=-26, scd5=23, scd3=7, rec_len=12
    #[test]
    fn test_cut_twice_top_strand() {
        let fst5: i64 = -10;
        let fst3: i64 = -26;
        let scd5: i64 = 23;
        let scd3: i64 = 7;
        let rec_len: i64 = 12;
        let rec_start: i64 = 100;

        // First cut pair (upstream)
        let p1_top = rec_start + fst5;               // 90
        let p1_bot = rec_start + rec_len + fst3;     // 86
        assert_eq!(p1_top, 90);
        assert_eq!(p1_bot, 86);

        // Second cut pair (downstream)
        let p2_top = rec_start + scd5;               // 123
        let p2_bot = rec_start + rec_len + scd3;     // 119
        assert_eq!(p2_top, 123);
        assert_eq!(p2_bot, 119);
    }

    /// Cut-twice enzyme BaeI, bottom strand
    #[test]
    fn test_cut_twice_bottom_strand() {
        let fst5: i64 = -10;
        let fst3: i64 = -26;
        let scd5: i64 = 23;
        let scd3: i64 = 7;
        let rec_len: i64 = 12;
        let rec_start: i64 = 100;

        // First cut pair (right side, was left side before flipping)
        let p1_top = rec_start - fst3;               // 100 - (-26) = 126
        let p1_bot = rec_start + rec_len - fst5;     // 100 + 12 - (-10) = 122
        assert_eq!(p1_top, 126);
        assert_eq!(p1_bot, 122);

        // Second cut pair (left side, was right side before flipping)
        let p2_top = rec_start - scd3;               // 100 - 7 = 93
        let p2_bot = rec_start + rec_len - scd5;     // 100 + 12 - 23 = 89
        assert_eq!(p2_top, 93);
        assert_eq!(p2_bot, 89);
    }

    // -------------------------------------------------------------------------
    // Circular normalization tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_normalize_pair_linear() {
        let pair = normalize_pair(10, 15, false, 100);
        assert_eq!(pair.top_cut_index, 10);
        assert_eq!(pair.bot_cut_index, 15);
    }

    #[test]
    fn test_normalize_pair_circular_in_bounds() {
        let pair = normalize_pair(50, 60, true, 100);
        assert_eq!(pair.top_cut_index, 50);
        assert_eq!(pair.bot_cut_index, 60);
    }

    #[test]
    fn test_normalize_pair_circular_out_of_bounds() {
        let pair = normalize_pair(105, -10, true, 100);
        assert_eq!(pair.top_cut_index, 5);
        assert_eq!(pair.bot_cut_index, 90);
    }

    // -------------------------------------------------------------------------
    // Palindromic consistency: top and bottom formulas give same result
    // -------------------------------------------------------------------------

    #[test]
    fn test_palindromic_consistency() {
        // For palindromic enzymes: top_off + bot_off = rec_len
        // EcoRI: fst5=1, fst3=-1 → bot_off = rec_len + fst3 = 5
        let fst5: i64 = 1;
        let fst3: i64 = -1;
        let rec_len: i64 = 6;
        let rec_start: i64 = 50;

        // Top strand
        let top_cut_top = rec_start + fst5;           // 51
        let bot_cut_top = rec_start + rec_len + fst3; // 55

        // Bottom strand (should give same as top for palindromic)
        let top_cut_bot = rec_start - fst3;           // 50 - (-1) = 51 ✓
        let bot_cut_bot = rec_start + rec_len - fst5; // 50 + 6 - 1 = 55 ✓

        assert_eq!(top_cut_top, top_cut_bot);
        assert_eq!(bot_cut_top, bot_cut_bot);
    }

    // -------------------------------------------------------------------------
    // Methylation integration tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_dpni_methylation_required_when_dam_inactive() {
        let mut project = ProjectData {
            sequence: "NNNNGATCNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN".to_string(),
            topology: "circular".to_string(),
            methylation_systems: vec![],  // Dam NOT active
            ..Default::default()
        };
        project.length = project.sequence.len() as i64;

        recompute(&mut project);

        let dpni_entries: Vec<_> = project.enzymes.iter().filter(|e| e.name == "DpnI").collect();
        assert!(!dpni_entries.is_empty(), "Should find DpnI sites in GATC-containing sequence");
        for e in &dpni_entries {
            assert!(e.methylation_required, "DpnI should require Dam when Dam is inactive (rec_start={})", e.rec_start);
            assert!(e.methyl_required_sources.contains(&"Dam".to_string()), "Should have Dam as required source");
            assert!(!e.methylation_blocked, "DpnI should not be methylation-blocked");
        }
    }

    #[test]
    fn test_dpni_no_requirement_when_dam_active() {
        let mut project = ProjectData {
            sequence: "NNNNGATCNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN".to_string(),
            topology: "circular".to_string(),
            methylation_systems: vec!["dam".to_string()],
            ..Default::default()
        };
        project.length = project.sequence.len() as i64;

        recompute(&mut project);

        let dpni_entries: Vec<_> = project.enzymes.iter().filter(|e| e.name == "DpnI").collect();
        assert!(!dpni_entries.is_empty());
        for e in &dpni_entries {
            assert!(!e.methylation_required, "DpnI should NOT require Dam when Dam is active");
            assert!(e.methyl_required_sources.is_empty());
        }
    }

    // -------------------------------------------------------------------------
    // Circular display window tests (tooltip range near the origin)
    // -------------------------------------------------------------------------

    fn bbsi_record() -> data::EnzymeRecord {
        data::EnzymeRecord {
            name: "BbsI".to_string(),
            site: "GAAGAC".to_string(),
            fst5: 8,
            fst3: 6,
            scd5: None,
            scd3: None,
            is_palindromic: false,
            cut_type: String::new(),
            overhang_len: 0,
            is_cut_twice: false,
            methylation: "none".to_string(),
            methylation_dependent: false,
            elucidate: String::new(),
        }
    }

    /// Recognition site right after the origin (bottom strand), cuts land
    /// before the origin: the display window must wrap, not span the whole
    /// sequence.
    #[test]
    fn test_display_window_wraps_origin_bottom_strand() {
        let seq_len: i64 = 100;
        let mut seq = vec![b'A'; seq_len as usize];
        // Reverse complement of GAAGAC at position 3 → bottom-strand hit.
        seq[3..9].copy_from_slice(b"GTCTTC");
        let seq_str = String::from_utf8(seq.clone()).unwrap();

        let result = process_enzyme(&bbsi_record(), &seq, &seq_str, true, seq_len, &None);
        assert_eq!(result.len(), 1);
        let e = &result[0];
        // p1_top = 3 - 6 = -3 → wraps to seq_len - 3
        assert_eq!(e.cut_pairs[0].top_cut_index, seq_len - 3);
        // Window: disp_start must be near seq_len, disp_end past seq_len
        // (wrapped), with a small total length — not the whole sequence.
        let win_len = e.display_end - e.display_start + 1;
        assert_eq!(e.display_start, seq_len - 4); // -3 cut + 1bp buffer
        assert_eq!(win_len, 13);
        assert!(e.rec_start >= e.display_start);
        assert!(e.rec_end <= e.display_end);
    }

    /// Recognition site spanning the origin (top strand): display window
    /// extends past seq_len, wrapped by the frontend.
    #[test]
    fn test_display_window_rec_spans_origin() {
        let seq_len: i64 = 100;
        let mut seq = vec![b'A'; seq_len as usize];
        // Top-strand site GAAGAC starting at 98: wraps to positions 0..4.
        seq[98] = b'G';
        seq[99] = b'A';
        seq[0..4].copy_from_slice(b"AGAC");
        let seq_str = String::from_utf8(seq.clone()).unwrap();
        let mut ext = seq.clone();
        ext.extend_from_slice(&seq[..5]);
        let ext_seq = Some(ext);

        let result = process_enzyme(&bbsi_record(), &seq, &seq_str, true, seq_len, &ext_seq);
        assert_eq!(result.len(), 1);
        let e = &result[0];
        // p1_top = 98 + 8 = 106 → wraps to 6; p1_bot = 110 → wraps to 10.
        assert_eq!(e.cut_pairs[0].top_cut_index, 6);
        assert_eq!(e.cut_pairs[0].bot_cut_index, 10);
        // Window: 98 .. 110 in the unwrapped frame (disp_end past seq_len).
        assert_eq!(e.display_start, 98);
        assert_eq!(e.display_end, 110);
        assert_eq!(e.rec_start, 98);
        assert_eq!(e.rec_end, 103);
        // Recognition sequence is the wrapped concatenation.
        assert_eq!(e.rec_seq, "GAAGAC");
    }
}
