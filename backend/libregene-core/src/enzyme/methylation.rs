//! Methylation-aware enzyme filtering.
//!
//! Two types of methylation interactions:
//! - **Sensitive** (database `methylation == "sensitive"`): blocked when a methylation
//!   target overlaps the recognition ± overlap.
//! - **Dependent** (recognition site IS a known methylation target, e.g. DpnI/GATC=Dam):
//!   requires that methylation system to be active to cut.
//!
//! A "dependent" enzyme's recognition site *exactly* matches a known methylation target
//! pattern. For example, DpnI's 4 bp site GATC is the Dam target. Enzymes whose longer
//! site merely *contains* GATC (e.g. CGATCG) are NOT dependent — they are unaffected.

use crate::models::Enzyme;
use crate::primer::alignment::wrap_template_region;

/// All known methylation systems (lowercase, canonical form). A circular DNA
/// project whose GBK file carries no methylation annotation defaults to these.
pub const ALL_SYSTEMS: [&str; 3] = ["dam", "dcm", "ecoki"];

/// Dam methylation: G(m6A)TC
fn is_dam_site(seq: &[u8]) -> bool {
    seq.len() >= 4
        && seq[0].eq_ignore_ascii_case(&b'G')
        && seq[1].eq_ignore_ascii_case(&b'A')
        && seq[2].eq_ignore_ascii_case(&b'T')
        && seq[3].eq_ignore_ascii_case(&b'C')
}

/// Dcm methylation: C(m5C)WGG (W = A or T)
fn is_dcm_site(seq: &[u8]) -> bool {
    seq.len() >= 5
        && seq[0].eq_ignore_ascii_case(&b'C')
        && seq[1].eq_ignore_ascii_case(&b'C')
        && (seq[2].eq_ignore_ascii_case(&b'A') || seq[2].eq_ignore_ascii_case(&b'T'))
        && seq[3].eq_ignore_ascii_case(&b'G')
        && seq[4].eq_ignore_ascii_case(&b'G')
}

/// EcoKI methylation: A(m6A)CNNNNNNGTGC (13 bp)
fn is_ecoki_site(seq: &[u8]) -> bool {
    seq.len() >= 13
        && seq[0].eq_ignore_ascii_case(&b'A')
        && seq[1].eq_ignore_ascii_case(&b'A')
        && seq[2].eq_ignore_ascii_case(&b'C')
        && seq[9].eq_ignore_ascii_case(&b'G')
        && seq[10].eq_ignore_ascii_case(&b'T')
        && seq[11].eq_ignore_ascii_case(&b'G')
        && seq[12].eq_ignore_ascii_case(&b'C')
}

/// EcoKI reverse-strand target: GCACNNNNNNGTT — the recognition site is
/// asymmetric, so the reverse complement must be scanned separately. The
/// checked positions mirror `is_ecoki_site` exactly.
fn is_ecoki_site_rc(seq: &[u8]) -> bool {
    seq.len() >= 13
        && seq[0].eq_ignore_ascii_case(&b'G')
        && seq[1].eq_ignore_ascii_case(&b'C')
        && seq[2].eq_ignore_ascii_case(&b'A')
        && seq[3].eq_ignore_ascii_case(&b'C')
        && seq[10].eq_ignore_ascii_case(&b'G')
        && seq[11].eq_ignore_ascii_case(&b'T')
        && seq[12].eq_ignore_ascii_case(&b'T')
}

/// Find a methylation target site anywhere within `window`.
fn find_site_in_window(window: &[u8], sys: &str) -> bool {
    match sys {
        "dam"   => window.windows(4).any(is_dam_site),
        "dcm"   => window.windows(5).any(is_dcm_site),
        "ecoki" => window.windows(13).any(|w| is_ecoki_site(w) || is_ecoki_site_rc(w)),
        _ => false,
    }
}

/// Determine required methylation system for an enzyme whose rec site IS a known
/// methylation target (e.g. DpnI/GATC = Dam). Tries each known target pattern.
fn find_required_system(rec_window: &[u8]) -> Option<&'static str> {
    match rec_window.len() {
        4 if is_dam_site(rec_window)   => Some("dam"),
        5 if is_dcm_site(rec_window)   => Some("dcm"),
        13 if is_ecoki_site(rec_window) => Some("ecoki"),
        _ => None,
    }
}

pub fn apply_methylation(
    enzyme: &mut Enzyme,
    template: &str,
    active_systems: &[String],
    overlap: i64,
    is_circular: bool,
) {
    // Preserve the database-set methylation_required flag (e.g. DpnI).
    let initially_required = enzyme.methylation_required;
    enzyme.methylation_blocked = false;
    enzyme.methylation_sources.clear();
    enzyme.methyl_required_sources.clear();
    enzyme.methylation_required = false;

    let tpl = template.as_bytes();
    let tlen = tpl.len();
    let rec_s = enzyme.rec_start as usize;
    let rec_e = enzyme.rec_end as usize;
    let ov = overlap as usize;

    // Recognition site window. For circular templates, normalize_rec represents
    // an origin-spanning site as rec_end >= tlen; we must wrap such a window
    // around the origin instead of bailing out (which used to silently skip all
    // methylation logic for origin-spanning sites).
    let rec_window_owned: Vec<u8>;
    let rec_window: &[u8] = if rec_e < tlen {
        match tpl.get(rec_s..=rec_e) {
            Some(w) => w,
            None => return,
        }
    } else {
        // Origin-spanning recognition site on a circular template.
        rec_window_owned = wrap_template_region(tpl, rec_s, rec_e + 1);
        &rec_window_owned
    };

    if enzyme.is_methylation_sensitive {
        // --- Methylation-sensitive: check if any active system's target overlaps rec ± overlap ---
        for sys in active_systems {
            let k = match sys.as_str() {
                "dam" => 4usize,
                "dcm" => 5,
                "ecoki" => 13,
                _ => continue,
            };
            // The overlap region is rec ± ov, inclusive on both sides; the
            // scan window covers every k-mer target intersecting that region.
            let lo = rec_s.saturating_sub(ov);
            let hi = rec_e + ov;
            let win_start = lo.saturating_sub(k - 1);
            let win_end = hi + k; // half-open
            let window: Vec<u8> = if !is_circular {
                // Linear templates have no wraparound — clamp the scan window
                // to the template instead of pulling in the other end.
                let start = win_start.min(tlen);
                let end = win_end.min(tlen);
                tpl.get(start..end).map(|w| w.to_vec()).unwrap_or_default()
            } else if win_end.saturating_sub(win_start) >= tlen {
                // Window spans the whole circle — wrapping would truncate it to
                // one turn and skip targets; scan the entire template instead.
                tpl.to_vec()
            } else if win_end <= tlen {
                match tpl.get(win_start..win_end) {
                    Some(w) => w.to_vec(),
                    None => continue,
                }
            } else {
                // Window extends beyond the template — wrap around for circular support.
                wrap_template_region(tpl, win_start, win_end)
            };

            if find_site_in_window(&window, sys) {
                enzyme.methylation_blocked = true;
                let name = match sys.as_str() {
                    "dam" => "Dam",
                    "dcm" => "Dcm",
                    "ecoki" => "EcoKI",
                    _ => continue,
                };
                if !enzyme.methylation_sources.contains(&name.to_string()) {
                    enzyme.methylation_sources.push(name.to_string());
                }
            }
        }
    }

    // --- Check for methylation-dependent enzymes (from database field) ---
    if initially_required {
        // Find which methylation system this enzyme depends on by checking its rec site.
        if let Some(target_sys) = find_required_system(rec_window) {
            let has_system = active_systems.iter().any(|s| s.as_str() == target_sys);
            if has_system {
                // System is active → the target IS methylated → enzyme cuts.
                enzyme.methylation_required = false;
            } else {
                // System not active → mark as required.
                enzyme.methylation_required = true;
                let source = match target_sys {
                    "dam" => "Dam",
                    "dcm" => "Dcm",
                    "ecoki" => "EcoKI",
                    _ => "Unknown",
                };
                enzyme.methyl_required_sources.push(source.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_enzyme(rec_start: i64, rec_end: i64, methylation_required: bool, is_methylation_sensitive: bool) -> Enzyme {
        Enzyme {
            rec_start,
            rec_end,
            methylation_required,
            is_methylation_sensitive,
            ..Default::default()
        }
    }

    #[test]
    fn test_linear_template_does_not_wrap_scan_window() {
        // Linear 20 bp template: Dam target GATC at the 5' end [0, 3], sensitive
        // enzyme rec site at the 3' end [15, 19]. The rec ± ov + k scan window
        // extends past the end; a circular wrap would pull the 5' GATC into the
        // window and falsely block the enzyme.
        let mut enzyme = make_enzyme(15, 19, false, true);
        let template = "GATCNNNNNNNNNNNNNNNN";
        let active_systems: Vec<String> = vec!["dam".to_string()];

        apply_methylation(&mut enzyme, template, &active_systems, 2, false);
        assert!(
            !enzyme.methylation_blocked,
            "linear template must not wrap the methylation scan window into the 5' end"
        );

        // Same template treated as circular DOES see the 5' Dam site.
        let mut enzyme = make_enzyme(15, 19, false, true);
        apply_methylation(&mut enzyme, template, &active_systems, 2, true);
        assert!(enzyme.methylation_blocked);
    }

    #[test]
    fn test_dpni_requires_dam_when_inactive() {
        let mut enzyme = make_enzyme(10, 13, true, false);
        let template = "NNNNNNNNNNGATCNNNN";
        let active_systems: Vec<String> = vec![];

        apply_methylation(&mut enzyme, template, &active_systems, 2, true);

        assert!(enzyme.methylation_required, "DpnI should require methylation when Dam is inactive");
        assert_eq!(enzyme.methyl_required_sources, vec!["Dam"]);
        assert!(!enzyme.methylation_blocked);
    }

    #[test]
    fn test_dpni_cuts_when_dam_active() {
        let mut enzyme = make_enzyme(10, 13, true, false);
        let template = "NNNNNNNNNNGATCNNNN";
        let active_systems: Vec<String> = vec!["dam".to_string()];

        apply_methylation(&mut enzyme, template, &active_systems, 2, true);

        assert!(!enzyme.methylation_required);
        assert!(enzyme.methyl_required_sources.is_empty());
    }

    #[test]
    fn test_methylation_sensitive_blocked_by_dam() {
        let mut enzyme = make_enzyme(8, 15, false, true);
        let template = "NNNNNNNNGATCNNNNNN";
        let active_systems: Vec<String> = vec!["dam".to_string()];

        apply_methylation(&mut enzyme, template, &active_systems, 2, true);

        assert!(enzyme.methylation_blocked);
        assert_eq!(enzyme.methylation_sources, vec!["Dam"]);
    }

    /// RED test for bug: dam/dcm upstream overlap not checked.
    ///
    /// A Dam target (GATC) sitting just UPSTREAM of the recognition site,
    /// within `overlap` bp of rec_start, should block a methylation-sensitive
    /// enzyme (the target overlaps the rec ± overlap window per the module's
    /// documented contract). The current window for dam/dcm starts at rec_s
    /// without subtracting `ov`, so this upstream target is missed.
    #[test]
    fn test_methylation_sensitive_blocked_by_dam_upstream_overlap() {
        // Recognition site [10, 17]. Dam target GATC at [8, 11] — its start
        // is 2bp upstream of rec_start=10, exactly within overlap=2.
        let mut enzyme = make_enzyme(10, 17, false, true);
        //       index: 0123456789...
        let template = "NNNNNNNNGATCNNNNNNNNN";
        //                        ^^^^ GATC at [8,11], upstream of rec [10,17]
        let active_systems: Vec<String> = vec!["dam".to_string()];

        apply_methylation(&mut enzyme, template, &active_systems, 2, true);

        assert!(
            enzyme.methylation_blocked,
            "Dam target upstream within overlap should block the enzyme, but it was missed"
        );
    }

    /// RED test for bug: an over-length scan window is truncated by wrapping.
    ///
    /// The rec ± ov scan window spans 17 bp but the template is only 10 bp;
    /// `wrap_template_region` returned just `tpl[0..7]` and skipped the Dam
    /// target at [6, 10). The fix scans the whole template whenever the
    /// window covers the entire circle.
    #[test]
    fn test_methylation_sensitive_short_template_full_scan() {
        let mut enzyme = make_enzyme(2, 9, false, true);
        //       index: 0123456789
        let template = "NNNNNNGATC"; // Dam target GATC at [6, 10)
        let active_systems: Vec<String> = vec!["dam".to_string()];

        apply_methylation(&mut enzyme, template, &active_systems, 4, true);

        assert!(
            enzyme.methylation_blocked,
            "Dam target in the truncated tail of an over-length scan window must still block the enzyme"
        );
    }

    /// RED test for bug: recognition site spanning the origin skips methylation.
    ///
    /// `normalize_rec` represents an origin-spanning recognition site by setting
    /// rec_end = norm_rec_end + seq_len (i.e. rec_end >= tlen). `apply_methylation`
    /// then did `tpl.get(rec_s..=rec_e)` which returns None when rec_e >= tlen,
    /// causing an early `return` that skipped ALL methylation logic. A
    /// methylation-dependent enzyme (DpnI, rec = GATC) whose recognition site
    /// itself spans the origin was never marked `methylation_required` even when
    /// Dam was inactive — it appeared to cut when it shouldn't.
    #[test]
    fn test_methylation_dependent_spanning_origin_not_skipped() {
        // Circular template of length 12. DpnI recognition GATC occupies
        // template positions [10,11,0,1] — i.e. the site spans the origin.
        // normalize_rec maps this to rec_start=10, rec_end=13 (>= tlen=12).
        // Dam is NOT active, so DpnI must be marked methylation_required.
        let mut enzyme = make_enzyme(10, 13, true, false);
        //       index: 012345678901
        let template = "TCNNNNNNNNGA"; // length 12: G@10, A@11, wraps to T@0, C@1 → GATC
        let active_systems: Vec<String> = vec![]; // Dam inactive

        apply_methylation(&mut enzyme, &template, &active_systems, 2, true);

        assert!(
            enzyme.methylation_required,
            "Origin-spanning DpnI site must still be evaluated for methylation dependence (Dam inactive → required), but the logic was skipped"
        );
    }

    /// EcoKI recognition is asymmetric (AACNNNNNNGTGC); its reverse-strand
    /// target GCACNNNNNNGTT must also block methylation-sensitive enzymes.
    #[test]
    fn test_methylation_sensitive_blocked_by_ecoki_reverse_strand() {
        // Sensitive enzyme rec site [20, 27], overlap 2. EcoKI reverse target
        // GCACNNNNNNGTT at [8, 20] intersects the rec ± ov region [18, 29].
        let mut tpl = vec![b'N'; 50];
        tpl[8..21].copy_from_slice(b"GCACNNNNNNGTT");
        let template = String::from_utf8(tpl).unwrap();
        let mut enzyme = make_enzyme(20, 27, false, true);
        let active_systems: Vec<String> = vec!["ecoki".to_string()];

        apply_methylation(&mut enzyme, &template, &active_systems, 2, true);

        assert!(
            enzyme.methylation_blocked,
            "EcoKI reverse-strand target overlapping rec ± overlap should block the enzyme"
        );
        assert_eq!(enzyme.methylation_sources, vec!["EcoKI"]);
    }

    #[test]
    fn test_methylation_sensitive_blocked_by_ecoki_forward_strand() {
        // Forward target AACNNNNNNGTGC at [8, 20] (as detected by is_ecoki_site).
        let mut tpl = vec![b'N'; 50];
        tpl[8..21].copy_from_slice(b"AACNNNNNNGTGC");
        let template = String::from_utf8(tpl).unwrap();
        let mut enzyme = make_enzyme(20, 27, false, true);
        let active_systems: Vec<String> = vec!["ecoki".to_string()];

        apply_methylation(&mut enzyme, &template, &active_systems, 2, true);

        assert!(enzyme.methylation_blocked);
        assert_eq!(enzyme.methylation_sources, vec!["EcoKI"]);
    }

    #[test]
    fn test_ecoki_reverse_site_ignored_when_system_inactive() {
        let mut tpl = vec![b'N'; 50];
        tpl[8..21].copy_from_slice(b"GCACNNNNNNGTT");
        let template = String::from_utf8(tpl).unwrap();
        let mut enzyme = make_enzyme(20, 27, false, true);
        let active_systems: Vec<String> = vec!["dam".to_string()];

        apply_methylation(&mut enzyme, &template, &active_systems, 2, true);

        assert!(!enzyme.methylation_blocked);
    }
}
