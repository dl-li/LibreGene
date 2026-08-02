//! Round-trip test: parse a .gbk → modify → write → parse again.
//! Verifies that edits produce valid GenBank output.
use std::path::Path;

#[test]
fn roundtrip_feature_edit() {
    let test_file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("examples")
        .join("pGGA-mCherry.gbk");

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

/// Enhanced-GenBank round trip: parse the LibreGene-exported pUC19 reference
/// (colors, segments and primer annotations), write it back out, parse again,
/// and verify the annotations survive.
#[test]
fn roundtrip_puc19_annotated() {
    let test_file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("examples")
        .join("pUC19 Annotated.gbk");

    let mut original = libregene_core::file_io::gbk::parse_gbk(&test_file)
        .expect("parse pUC19");

    // Header fields
    assert_eq!(original.name, "pUC19_Annotated");
    assert_eq!(original.lab_host, "Escherichia coli");

    let feat_count = original.features.len();
    let primer_count = original.primers.len();
    assert!(feat_count > 0);
    assert_eq!(primer_count, 2, "M13 fwd + M13 rev");

    // Segmented AmpR restored from the "This feature has N segments" note
    let ampr = original.features.iter().find(|f| f.name == "AmpR")
        .expect("AmpR parsed");
    assert_eq!(ampr.segments.len(), 2);
    assert_eq!((ampr.segments[0].start, ampr.segments[0].end), (1625, 2416));
    assert_eq!((ampr.segments[1].start, ampr.segments[1].end), (2417, 2485));
    assert_eq!(ampr.segments[0].color.as_deref(), Some("#ccffcc"));

    // Write out and parse again
    libregene_core::primer::recompute(&mut original);
    let tmp = std::env::temp_dir().join("libregene_puc19_roundtrip.gbk");
    libregene_core::file_io::gbk::write_gbk(&original, &tmp).expect("write gbk");
    let written = std::fs::read_to_string(&tmp).unwrap();
    let reloaded = libregene_core::file_io::gbk::parse_gbk(&tmp).expect("re-parse written gbk");
    let _ = std::fs::remove_file(&tmp);

    assert_eq!(reloaded.name, "pUC19_Annotated");
    assert_eq!(reloaded.lab_host, "Escherichia coli");
    assert_eq!(reloaded.features.len(), feat_count);
    assert_eq!(reloaded.primers.len(), primer_count);
    assert_eq!(reloaded.sequence, original.sequence);
    assert!(written.contains("ORIGIN"));

    // Segments survive the round trip
    let ampr2 = reloaded.features.iter().find(|f| f.name == "AmpR").unwrap();
    assert_eq!(ampr2.segments.len(), 2);
    assert_eq!((ampr2.segments[0].start, ampr2.segments[0].end), (1625, 2416));
    assert_eq!((ampr2.segments[1].start, ampr2.segments[1].end), (2417, 2485));

    // Primer direction note (no color — primers have no static color)
    let fwd = reloaded.primers.iter().find(|p| p.name == "M13 fwd").expect("M13 fwd");
    let rev = reloaded.primers.iter().find(|p| p.name == "M13 rev").expect("M13 rev");
    assert_eq!(fwd.r#type, "fwd");
    assert_eq!(rev.r#type, "rev");
    assert!(written.contains("direction: RIGHT; sequence: "), "fwd primer note");
    assert!(written.contains("direction: LEFT; sequence: "), "rev primer note");

    // Color note format: merged direction for reverse features, plain for forward
    assert!(written.contains("; direction: LEFT\""),
        "reverse feature merged color note");
    assert!(written.contains("/note=\"color: #99ccff\""),
        "forward feature plain color note");
    assert!(!written.contains("direction: RIGHT\"\n"), "no separate forward direction note");

    // Segments note written in enhanced-GenBank style
    assert!(written.contains("This feature has 2 segments:"), "segments note written");
}
