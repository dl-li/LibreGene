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

/// EcoKI methylation: A(m6A)CNNNNNNNGTGC (13 bp)
fn is_ecoki_site(seq: &[u8]) -> bool {
    seq.len() >= 13
        && seq[0].eq_ignore_ascii_case(&b'A')
        && seq[1].eq_ignore_ascii_case(&b'A')
        && seq[2].eq_ignore_ascii_case(&b'C')
        && seq[8].eq_ignore_ascii_case(&b'G')
        && seq[9].eq_ignore_ascii_case(&b'T')
        && seq[10].eq_ignore_ascii_case(&b'G')
        && seq[11].eq_ignore_ascii_case(&b'C')
}

/// Find a methylation target site anywhere within `window`.
fn find_site_in_window(window: &[u8], sys: &str) -> bool {
    match sys {
        "dam"   => window.windows(4).any(|w| is_dam_site(w)),
        "dcm"   => window.windows(5).any(|w| is_dcm_site(w)),
        "ecoki" => window.windows(13).any(|w| is_ecoki_site(w)),
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
            let (win_start, win_end) = match sys.as_str() {
                "dam" | "dcm" => (rec_s.saturating_sub(ov), rec_e + ov),
                "ecoki" => (rec_s.saturating_sub(13), rec_e + 13 + ov),
                _ => continue,
            };
            let window: Vec<u8> = if win_end <= tlen {
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
    fn test_dpni_requires_dam_when_inactive() {
        let mut enzyme = make_enzyme(10, 13, true, false);
        let template = "NNNNNNNNNNGATCNNNN";
        let active_systems: Vec<String> = vec![];

        apply_methylation(&mut enzyme, template, &active_systems, 2);

        assert!(enzyme.methylation_required, "DpnI should require methylation when Dam is inactive");
        assert_eq!(enzyme.methyl_required_sources, vec!["Dam"]);
        assert!(!enzyme.methylation_blocked);
    }

    #[test]
    fn test_dpni_cuts_when_dam_active() {
        let mut enzyme = make_enzyme(10, 13, true, false);
        let template = "NNNNNNNNNNGATCNNNN";
        let active_systems: Vec<String> = vec!["dam".to_string()];

        apply_methylation(&mut enzyme, template, &active_systems, 2);

        assert!(!enzyme.methylation_required);
        assert!(enzyme.methyl_required_sources.is_empty());
    }

    #[test]
    fn test_methylation_sensitive_blocked_by_dam() {
        let mut enzyme = make_enzyme(8, 15, false, true);
        let template = "NNNNNNNNGATCNNNNNN";
        let active_systems: Vec<String> = vec!["dam".to_string()];

        apply_methylation(&mut enzyme, template, &active_systems, 2);

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

        apply_methylation(&mut enzyme, template, &active_systems, 2);

        assert!(
            enzyme.methylation_blocked,
            "Dam target upstream within overlap should block the enzyme, but it was missed"
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

        apply_methylation(&mut enzyme, &template, &active_systems, 2);

        assert!(
            enzyme.methylation_required,
            "Origin-spanning DpnI site must still be evaluated for methylation dependence (Dam inactive → required), but the logic was skipped"
        );
    }
}
