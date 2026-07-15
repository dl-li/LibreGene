//! Round-trip test: parse a .gbk → modify → write → parse again.
//! Verifies that edits produce valid GenBank output.
use std::path::Path;

#[test]
fn roundtrip_feature_edit() {
    let test_file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("test")
        .join("pUC-GW-Amp.gb");

    // 1) Parse original
    let mut original = libregene_core::file_io::gbk::parse_gbk(&test_file)
        .expect("parse original");
    let feat_name;
    {
        let feat = original.features.first_mut().expect("at least one feature");
        feat_name = feat.name.clone();
        let old_ftype = feat.ftype.clone();
        feat.ftype = "CDS".to_string();
        feat.color = "#FF0000".to_string();
        println!("  Modified feature '{}': {} → CDS, color red", feat.name, old_ftype);
    } // drop mutable borrow

    // 3) Write to temp file
    let tmp = std::env::temp_dir().join("libregene_roundtrip_test.gbk");
    libregene_core::file_io::gbk::write_gbk(&original, &tmp)
        .expect("write gbk");
    let original_seq = original.sequence.clone();

    // 4) Parse the written file back
    let reloaded = libregene_core::file_io::gbk::parse_gbk(&tmp)
        .expect("parse written gbk");

    // 5) Verify the modification survived
    let feat2 = reloaded.features.iter().find(|f| f.name == feat_name)
        .expect("feature name survived");
    assert_eq!(feat2.ftype, "CDS", "ftype persisted");
    assert_eq!(feat2.color.to_lowercase(), "#ff0000", "color persisted");
    assert_eq!(reloaded.sequence, original_seq, "sequence unchanged");

    // 6) Verify all features have valid coordinates
    for f in &reloaded.features {
        assert!(f.start >= 0, "feature '{}' start >= 0", f.name);
        assert!(f.end < reloaded.sequence.len() as i64,
            "feature '{}' end {} < seq len {}", f.name, f.end, reloaded.sequence.len());
        assert!(f.start <= f.end, "feature '{}' start <= end", f.name);
        for seg in &f.segments {
            assert!(seg.start >= 0, "segment start >= 0");
            assert!(seg.end < reloaded.sequence.len() as i64, "segment end in range");
            assert!(seg.start <= seg.end, "segment start <= end");
        }
    }

    // 7) Cleanup
    let _ = std::fs::remove_file(&tmp);
    println!("  ✅ Round-trip OK — {} features, {} bp", reloaded.features.len(), reloaded.length);
}
