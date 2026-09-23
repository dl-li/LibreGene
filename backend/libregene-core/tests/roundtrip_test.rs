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

// ---------------------------------------------------------------------------
// RNA / protein support
// ---------------------------------------------------------------------------

fn test_data(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("test_data")
        .join(name)
}

/// The four example files (two formats × RNA/protein) must all parse with the
/// right molecule type, topology, sequence length and key feature annotations.
#[test]
fn parse_rna_and_protein_examples() {
    // RNA: GenBank text + SnapGene binary
    for (path, expect_len) in [
        (test_data("Primary-miR-1.gbk"), 91),
        (test_data("Primary-miR-1.rna"), 91),
    ] {
        let project = libregene_core::file_io::parse_file(&path).expect("parse rna file");
        assert_eq!(project.molecule_type, "rna", "molecule type for {}", path.display());
        assert_eq!(project.topology, "linear", "topology for {}", path.display());
        assert_eq!(project.length, expect_len, "length for {}", path.display());
        let guide = project.features.iter().find(|f| f.name == "Guide strand").expect("Guide strand");
        assert_eq!(guide.color, "#d34035");
        assert_eq!((guide.start, guide.end), (55, 76));
        let pas = project.features.iter().find(|f| f.name == "Passenger strand").expect("Passenger strand");
        assert_eq!(pas.color, "#5c80ba");
        assert_eq!((pas.start, pas.end), (17, 38));
        assert!(project.features.iter().any(|f| f.name == "shRNA (miR-1 scaffold）"), "scaffold feature");
    }

    // Protein: GenBank text + SnapGene binary
    for path in [test_data("mCherry.gpt"), test_data("mCherry.prot")] {
        let project = libregene_core::file_io::parse_file(&path).expect("parse protein file");
        assert_eq!(project.molecule_type, "protein", "molecule type for {}", path.display());
        assert_eq!(project.topology, "linear", "topology for {}", path.display());
        assert_eq!(project.length, 237, "length for {}", path.display());
        assert!(project.sequence.ends_with('*'), "terminal stop for {}", path.display());
        let feat = project.features.iter().find(|f| f.name == "mCherry").expect("mCherry feature");
        assert_eq!(feat.ftype, "Region");
        assert_eq!(feat.color, "#ff0000");
        assert_eq!((feat.start, feat.end), (0, 236));
    }
}

/// RNA .gbk round trip: parse → write .gbk → parse again.
#[test]
fn roundtrip_rna_gbk() {
    let original = libregene_core::file_io::gbk::parse_gbk(&test_data("Primary-miR-1.gbk"))
        .expect("parse rna gbk");
    assert_eq!(original.molecule_type, "rna");

    let tmp = std::env::temp_dir().join("libregene_rna_roundtrip.gbk");
    libregene_core::file_io::gbk::write_gbk(&original, &tmp).expect("write rna gbk");
    let written = std::fs::read_to_string(&tmp).unwrap();
    let reloaded = libregene_core::file_io::gbk::parse_gbk(&tmp).expect("re-parse rna gbk");
    let _ = std::fs::remove_file(&tmp);

    assert!(written.contains("ss-RNA"), "LOCUS keeps ss-RNA molecule type");
    assert_eq!(reloaded.molecule_type, "rna");
    assert_eq!(reloaded.length, 91);
    assert_eq!(reloaded.sequence.to_uppercase(), original.sequence.to_uppercase());
    assert!(reloaded.features.iter().any(|f| f.name == "Guide strand"));
}

/// Protein .gpt round trip: parse → write .gpt → parse again.
#[test]
fn roundtrip_protein_gpt() {
    let original = libregene_core::file_io::gpt::parse_gpt(&test_data("mCherry.gpt"))
        .expect("parse protein gpt");
    assert_eq!(original.molecule_type, "protein");

    let tmp = std::env::temp_dir().join("libregene_protein_roundtrip.gpt");
    libregene_core::file_io::gpt::write_gpt(&original, &tmp).expect("write protein gpt");
    let written = std::fs::read_to_string(&tmp).unwrap();
    let reloaded = libregene_core::file_io::gpt::parse_gpt(&tmp).expect("re-parse protein gpt");
    let _ = std::fs::remove_file(&tmp);

    assert!(written.contains(" aa "), "LOCUS uses aa units");
    assert!(written.contains("ORIGIN"), "ORIGIN section written");
    assert_eq!(reloaded.molecule_type, "protein");
    assert_eq!(reloaded.length, 237);
    assert_eq!(reloaded.sequence, original.sequence);
    let feat = reloaded.features.iter().find(|f| f.name == "mCherry").expect("mCherry feature");
    assert_eq!(feat.color, "#ff0000");
    assert_eq!((feat.start, feat.end), (0, 236));
}

// ---------------------------------------------------------------------------
// Multi-segment / cross-origin feature locations
// ---------------------------------------------------------------------------

fn synthetic_project(features: Vec<libregene_core::models::Feature>) -> libregene_core::models::ProjectData {
    libregene_core::models::ProjectData {
        name: "Synthetic".to_string(),
        sequence: "ACGT".repeat(250), // 1000 bp
        length: 1000,
        topology: "circular".to_string(),
        features,
        ..Default::default()
    }
}

fn make_feature(
    name: &str,
    start: i64,
    end: i64,
    segments: Vec<(i64, i64)>,
    strand: &str,
) -> libregene_core::models::Feature {
    libregene_core::models::Feature {
        id: name.to_string(),
        name: name.to_string(),
        start,
        end,
        color: "#60A5FA".to_string(),
        ftype: "misc_feature".to_string(),
        segments: segments
            .into_iter()
            .map(|(s, e)| libregene_core::models::Segment { start: s, end: e, color: None })
            .collect(),
        strand: strand.to_string(),
        notes: String::new(),
        translation: String::new(),
        qualifiers: Vec::new(),
    }
}

fn segments_of(f: &libregene_core::models::Feature) -> Vec<(i64, i64)> {
    f.segments.iter().map(|s| (s.start, s.end)).collect()
}

/// Cross-origin features (both the file-parsed min/max form and the
/// MCP/frontend first/last form) must be written as join() and survive a
/// save→reload with identical segments.
#[test]
fn roundtrip_cross_origin_features() {
    let project = synthetic_project(vec![
        // min/max form (as parsed from files): start=0, end=999
        make_feature("cross_minmax", 0, 999, vec![(900, 999), (0, 49)], "-"),
        // first/last form with join-order segments (MCP/frontend)
        make_feature("cross_firstlast", 900, 49, vec![(900, 999), (0, 49)], "+"),
        // first/last form without segments
        make_feature("cross_noseg", 900, 49, vec![], "+"),
    ]);

    let tmp = std::env::temp_dir().join("libregene_cross_origin_roundtrip.gbk");
    libregene_core::file_io::gbk::write_gbk(&project, &tmp).expect("write gbk");
    let written = std::fs::read_to_string(&tmp).unwrap();
    let reloaded = libregene_core::file_io::gbk::parse_gbk(&tmp).expect("re-parse gbk");
    let _ = std::fs::remove_file(&tmp);

    assert!(written.contains("join(901..1000,1..50)"), "cross-origin join written:\n{written}");
    assert!(!written.contains("901..50"), "no illegal reversed range");

    let expected = vec![(900, 999), (0, 49)];
    let mm = reloaded.features.iter().find(|f| f.name == "cross_minmax").expect("cross_minmax");
    assert_eq!(segments_of(mm), expected);
    assert_eq!(mm.strand, "-", "strand survives complement(join)");
    let fl = reloaded.features.iter().find(|f| f.name == "cross_firstlast").expect("cross_firstlast");
    assert_eq!(segments_of(fl), expected);
    let ns = reloaded.features.iter().find(|f| f.name == "cross_noseg").expect("cross_noseg");
    assert_eq!(segments_of(ns), expected);
}

/// A join(a,b) feature with a gap keeps both segments across a round trip.
#[test]
fn roundtrip_join_feature_with_gap() {
    let project = synthetic_project(vec![
        make_feature("gapped", 99, 399, vec![(99, 199), (299, 399)], "+"),
    ]);

    let tmp = std::env::temp_dir().join("libregene_gap_join_roundtrip.gbk");
    libregene_core::file_io::gbk::write_gbk(&project, &tmp).expect("write gbk");
    let written = std::fs::read_to_string(&tmp).unwrap();
    let reloaded = libregene_core::file_io::gbk::parse_gbk(&tmp).expect("re-parse gbk");
    let _ = std::fs::remove_file(&tmp);

    assert!(written.contains("join(100..200,300..400)"), "gap join written:\n{written}");
    let f = reloaded.features.iter().find(|f| f.name == "gapped").expect("gapped");
    assert_eq!(segments_of(f), vec![(99, 199), (299, 399)]);
}

/// A plain single-range feature roundtrips unchanged (no join emitted).
#[test]
fn roundtrip_single_range_feature() {
    let project = synthetic_project(vec![
        make_feature("plain", 99, 199, vec![(99, 199)], "+"),
    ]);

    let tmp = std::env::temp_dir().join("libregene_single_range_roundtrip.gbk");
    libregene_core::file_io::gbk::write_gbk(&project, &tmp).expect("write gbk");
    let written = std::fs::read_to_string(&tmp).unwrap();
    let reloaded = libregene_core::file_io::gbk::parse_gbk(&tmp).expect("re-parse gbk");
    let _ = std::fs::remove_file(&tmp);

    assert!(written.contains("100..200"), "plain range written:\n{written}");
    assert!(!written.contains("join("), "no join for single range:\n{written}");
    let f = reloaded.features.iter().find(|f| f.name == "plain").expect("plain");
    assert_eq!((f.start, f.end), (99, 199));
    assert_eq!(segments_of(f), vec![(99, 199)]);
}

/// An order(100..150,200..250) location must load with real bounds instead of
/// collapsing to a point, and re-saving must not produce 1..1.
#[test]
fn roundtrip_order_location() {
    let seq = "acgt".repeat(125); // 500 bp
    let mut origin = String::from("ORIGIN\n");
    for (i, chunk) in seq.as_bytes().chunks(60).enumerate() {
        let line: Vec<String> = chunk
            .chunks(10)
            .map(|c| std::str::from_utf8(c).unwrap().to_string())
            .collect();
        origin.push_str(&format!("{:>9} {}\n", i * 60 + 1, line.join(" ")));
    }
    let gbk = format!(
        "LOCUS       TestOrder              500 bp    DNA     circular SYN 23-SEP-2026\n\
         DEFINITION  .\n\
         ACCESSION   .\n\
         VERSION     .\n\
         KEYWORDS    .\n\
         SOURCE      synthetic DNA construct\n\
         \x20 ORGANISM  synthetic DNA construct\n\
         FEATURES             Location/Qualifiers\n\
         \x20    misc_feature    order(100..150,200..250)\n\
         \x20                    /label=\"ordered\"\n\
         {origin}//\n"
    );

    let tmp = std::env::temp_dir().join("libregene_order_location.gbk");
    std::fs::write(&tmp, gbk).unwrap();
    let project = libregene_core::file_io::gbk::parse_gbk(&tmp).expect("parse order() gbk");

    let f = project.features.iter().find(|f| f.name == "ordered").expect("ordered feature");
    assert_eq!((f.start, f.end), (99, 249), "order() bounds");
    assert_eq!(segments_of(f), vec![(99, 149), (199, 249)], "order() segments");

    let tmp2 = std::env::temp_dir().join("libregene_order_location_out.gbk");
    libregene_core::file_io::gbk::write_gbk(&project, &tmp2).expect("write gbk");
    let written = std::fs::read_to_string(&tmp2).unwrap();
    let reloaded = libregene_core::file_io::gbk::parse_gbk(&tmp2).expect("re-parse gbk");
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&tmp2);

    assert!(!written.contains("1..1"), "order() must not collapse to 1..1:\n{written}");
    let f2 = reloaded.features.iter().find(|f| f.name == "ordered").expect("ordered feature");
    assert_eq!((f2.start, f2.end), (99, 249), "order() bounds survive re-save");
    assert_eq!(segments_of(f2), vec![(99, 149), (199, 249)]);
}
