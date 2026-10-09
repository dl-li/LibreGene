//! Internal-consistency checks for the enzyme database
//! (`data/comm_only_enzymes.json`, exported by `scripts/export_enzymes.py`).

use libregene_core::enzyme::search::{get_db, iupac_complement};
use std::collections::HashSet;

const IUPAC: &str = "ACGTRYSWKMBDHVN";

#[test]
fn enzyme_data_is_internally_consistent() {
    let db = get_db();
    let mut names = HashSet::new();

    for e in &db.enzymes {
        assert!(names.insert(e.name.as_str()), "duplicate enzyme {}", e.name);
        assert!(
            !e.site.is_empty() && e.site.chars().all(|c| IUPAC.contains(c)),
            "{}: invalid site {:?}",
            e.name,
            e.site
        );
        assert!(
            ["blunt", "5overhang", "3overhang"].contains(&e.cut_type.as_str()),
            "{}: invalid cut_type {:?}",
            e.name,
            e.cut_type
        );
        assert!(
            ["none", "sensitive"].contains(&e.methylation.as_str()),
            "{}: invalid methylation {:?}",
            e.name,
            e.methylation
        );
        assert_eq!(
            e.is_palindromic,
            e.site == iupac_complement(&e.site),
            "{}: is_palindromic disagrees with site {}",
            e.name,
            e.site
        );

        if e.is_cut_twice {
            // Double cutters (elucidate "not yet implemented") carry scd5/scd3
            // and no usable ^/_ markers; skip cut-geometry checks for them.
            assert!(
                e.scd5.is_some() && e.scd3.is_some(),
                "{}: cut-twice enzyme missing scd5/scd3",
                e.name
            );
            continue;
        }
        assert!(
            e.scd5.is_none() && e.scd3.is_none(),
            "{}: scd5/scd3 set on a non-cut-twice enzyme",
            e.name
        );

        // Positions of ^ (top cut) and _ (bottom cut) in stripped coordinates.
        let mut stripped = String::new();
        let mut top_cut = None;
        let mut bot_cut = None;
        for c in e.elucidate.chars() {
            match c {
                '^' => {
                    assert!(top_cut.is_none(), "{}: multiple ^", e.name);
                    top_cut = Some(stripped.len() as i64);
                }
                '_' => {
                    assert!(bot_cut.is_none(), "{}: multiple _", e.name);
                    bot_cut = Some(stripped.len() as i64);
                }
                _ => stripped.push(c),
            }
        }
        let top_cut = top_cut.unwrap_or_else(|| panic!("{}: no ^ in elucidate", e.name));
        let bot_cut = bot_cut.unwrap_or_else(|| panic!("{}: no _ in elucidate", e.name));

        let site_start = stripped
            .find(&e.site)
            .unwrap_or_else(|| panic!("{}: site {} not in elucidate {:?}", e.name, e.site, e.elucidate))
            as i64;
        let rec_len = e.site.len() as i64;

        assert_eq!(
            e.fst5,
            top_cut - site_start,
            "{}: fst5 mismatch with ^ in {:?}",
            e.name,
            e.elucidate
        );
        assert_eq!(
            e.fst3,
            bot_cut - site_start - rec_len,
            "{}: fst3 mismatch with _ in {:?}",
            e.name,
            e.elucidate
        );

        let overhang = e.fst5 - (rec_len + e.fst3);
        assert_eq!(
            e.overhang_len, overhang,
            "{}: overhang_len {} != {}",
            e.name, e.overhang_len, overhang
        );
        let expected_cut = if overhang > 0 {
            "3overhang"
        } else if overhang < 0 {
            "5overhang"
        } else {
            "blunt"
        };
        assert_eq!(
            e.cut_type, expected_cut,
            "{}: cut_type {} != {}",
            e.name, e.cut_type, expected_cut
        );
    }
}
