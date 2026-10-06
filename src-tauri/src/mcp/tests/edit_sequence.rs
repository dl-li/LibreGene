use super::common::*;
use crate::mcp::*;

    #[tokio::test]
    async fn edit_sequence_replacement_from_fasta_file() {
        let server = handler_with_project(edit_test_project()).await;
        let insert = "AAACCCGGGTTT";
        let fasta = std::env::temp_dir()
            .join(format!("libregene-mcp-edit-ins-{}.fasta", std::process::id()));
        std::fs::write(&fasta, format!(">insert\n{}\n", insert)).unwrap();

        // pure insertion before base 61 (1-based; internal position 60) via
        // replacement_path
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement_path: Some(fasta.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["newLength"], 212);

        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(p.sequence.len(), 212);
        assert_eq!(&p.sequence[60..72], insert);
        // feature 50..100 spans the insertion point → end shifted by 12
        let f = p.features.iter().find(|f| f.name == "gene").unwrap();
        assert_eq!((f.start, f.end), (50, 112));
        drop(pm);
        std::fs::remove_file(&fasta).ok();
    }

    #[tokio::test]
    async fn edit_sequence_transfers_annotations_from_gbk() {
        let (dir, gbk) = write_annotated_insert();
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement_path: Some(gbk.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(
            v["transferredFeatures"],
            serde_json::json!(["ins_feat", "gene (2)"])
        );
        assert_eq!(v["transferredPrimers"], serde_json::json!(["ins_primer"]));

        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(p.sequence.len(), 260);
        let f = p.features.iter().find(|f| f.name == "ins_feat").unwrap();
        assert_eq!((f.start, f.end), (70, 89));
        // name clash with the target's "gene" → renamed, rebased to 60..65
        let renamed = p.features.iter().find(|f| f.name == "gene (2)").unwrap();
        assert_eq!((renamed.start, renamed.end), (60, 65));
        let pr = p.primers.iter().find(|x| x.name == "ins_primer").unwrap();
        assert!(!pr.binding_sites.is_empty(), "primer site recomputed");
        let bs = &pr.binding_sites[0];
        assert_eq!((bs.template_start, bs.template_end), (90, 110));
        drop(pm);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn edit_sequence_transfers_annotations_reverse_complemented() {
        let (dir, gbk) = write_annotated_insert();
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement_path: Some(gbk.to_string_lossy().into_owned()),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);

        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        // local [10..29] in a 60 bp insert mirrors to [30..49] → +60 offset,
        // and the "-" strand survives the gbk round trip and flips to "+"
        let f = p.features.iter().find(|f| f.name == "ins_feat").unwrap();
        assert_eq!((f.start, f.end), (90, 109));
        assert_eq!(f.strand, "+");
        // the primer still binds (on the opposite strand) inside the insert
        let pr = p.primers.iter().find(|x| x.name == "ins_primer").unwrap();
        let bs = &pr.binding_sites[0];
        assert_eq!(bs.strand, -1);
        assert!(bs.template_start >= 60 && bs.template_end <= 120);
        drop(pm);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn edit_sequence_replacement_input_validation() {
        let server = handler_with_project(edit_test_project()).await;

        // both replacement and replacement_path → error
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 21,
                replacement: Some("ACGT".to_string()),
                replacement_path: Some("x.fasta".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(out.0["message"].as_str().unwrap().contains("exactly one"), "{}", out.0);

        // neither → error
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 21,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(out.0["message"].as_str().unwrap().contains("exactly one"), "{}", out.0);

        // bad extension → hard error from validate_user_path
        let res = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 21,
                replacement_path: Some("notes.txt".to_string()),
                ..Default::default()
            }))
            .await;
        assert!(res.is_err(), "txt path must be rejected");

        // unreadable/missing file → fail envelope
        let missing = std::env::temp_dir()
            .join(format!("libregene-mcp-edit-missing-{}.fasta", std::process::id()));
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 21,
                replacement_path: Some(missing.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("Failed to read replacement"),
            "{}",
            out.0
        );

        // sequence must be untouched after all these failures
        let pm = server.pm.read().await;
        assert_eq!(pm.get_project_by_id("edit_test").unwrap().sequence.len(), 200);
    }

    #[tokio::test]
    async fn edit_sequence_strand_minus_inserts_reverse_complement() {
        let server = handler_with_project(edit_test_project()).await;

        // pure insertion before base 61 (1-based) with strand "-" → revcomp inserted
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement: Some("AAACCCGGGTTG".to_string()),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert!(
            v["message"].as_str().unwrap().contains("reverse-complemented"),
            "{}",
            v
        );
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(p.sequence.len(), 212);
        assert_eq!(&p.sequence[60..72], "CAACCCGGGTTT");
        // feature 50..100 spans the insertion point → end shifted by 12
        let f = p.features.iter().find(|f| f.name == "gene").unwrap();
        assert_eq!((f.start, f.end), (50, 112));
    }

    #[tokio::test]
    async fn edit_sequence_strand_validation() {
        // invalid strand value → fail envelope
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement: Some("ACGT".to_string()),
                strand: Some("x".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("Invalid strand"),
            "{}",
            out.0
        );

        // strand "-" rejected on protein projects
        let server = handler_with_project(protein_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "prot".to_string(),
                start: 11,
                end: 11,
                replacement: Some("AA".to_string()),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("only supported on DNA"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn edit_sequence_string_replacement_still_works() {
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 20,
                replacement: Some("TT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["newLength"], 192);
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(&p.sequence[10..12], "TT");
    }

    #[tokio::test]
    async fn edit_sequence_rechecks_bounds_against_live_sequence() {
        // A stale `length` field simulates a live sequence that shrank after
        // the resolve-time snapshot: the write-lock re-check must reject the
        // edit instead of panicking on the slice.
        let mut project = edit_test_project();
        project.length = project.sequence.len() as i64 + 50;
        let stale_len = project.length;
        let server = handler_with_project(project).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: stale_len - 10,
                end: stale_len,
                replacement: Some("AA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("out of bounds"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn edit_sequence_uppercases_dna_replacement() {
        // Lowercase replacement bases must be normalized to uppercase (as
        // update_sequence does); otherwise the case-sensitive enzyme recompute
        // loses sites spanning the edit boundary and gbk output mixes case.
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 20,
                replacement: Some("gaattcGGTT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(&p.sequence[10..20], "GAATTCGGTT");
        assert!(
            !p.sequence.chars().any(|c| c.is_ascii_lowercase()),
            "no lowercase bases left in the stored sequence"
        );
        drop(pm);

        // Same normalization applies to the replacement_path input (GenBank
        // files conventionally store lowercase sequence).
        let server = handler_with_project(edit_test_project()).await;
        let gbk = std::env::temp_dir()
            .join(format!("libregene-mcp-edit-lower-{}.gbk", std::process::id()));
        std::fs::write(
            &gbk,
            "LOCUS       ins                       10 bp    DNA     linear   UNA 01-JAN-1980\n\
             FEATURES             Location/Qualifiers\n\
             ORIGIN\n\
             1 gaattcggtt\n\
             //\n",
        )
        .unwrap();
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 20,
                replacement_path: Some(gbk.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(&p.sequence[10..20], "GAATTCGGTT");
        drop(pm);
        std::fs::remove_file(&gbk).ok();
    }

    #[tokio::test]
    async fn edit_sequence_equal_length_replacement_keeps_covered_features() {
        // Equal-length replacement over feature f1 (internal 50..100): the
        // feature stays at its coordinates and removedFeatures is empty.
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 70,
                replacement: Some("TTTTTTTTTT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["removedFeatures"], serde_json::json!([]), "{v}");
        assert_eq!(v["clippedFeatures"], serde_json::json!([]), "{v}");
        // Content differs beyond case → the covered feature is surfaced in
        // contentChangedFeatures so the agent knows the annotation now
        // describes different bases.
        assert_eq!(
            v["contentChangedFeatures"],
            serde_json::json!(["gene"]),
            "equal-length replacement with different content must flag covered features: {v}"
        );
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        let f = p.features.iter().find(|f| f.name == "gene").unwrap();
        assert_eq!((f.start, f.end), (50, 100), "feature untouched");
        drop(pm);

        // Case-only change (the uppercase normalization path): content is
        // equivalent ignoring case → no contentChangedFeatures signal.
        let server = handler_with_project(edit_test_project()).await;
        let original = {
            let pm = server.pm.read().await;
            let p = pm.get_project_by_id("edit_test").unwrap();
            p.sequence[60..70].to_string() // internal 60..=69 = 1-based 61..=70
        };
        let lower: String = original.to_ascii_lowercase();
        if lower != original {
            let out = server
                .edit_sequence(Parameters(EditSequenceRequest {
                    project_id: "edit_test".to_string(),
                    start: 61,
                    end: 70,
                    replacement: Some(lower),
                    ..Default::default()
                }))
                .await
                .unwrap();
            let v = out.0;
            assert_eq!(v["ok"], true, "{}", v);
            assert!(
                v.get("contentChangedFeatures").is_none(),
                "case-only normalization must not flag features: {v}"
            );
        }

        // A length-changing replacement fully covering the feature still
        // removes it (reported in removedFeatures).
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 41,
                end: 120,
                replacement: Some("GG".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["removedFeatures"][0]["name"], "gene", "{v}");
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert!(p.features.iter().all(|f| f.name != "gene"));
    }

    #[tokio::test]
    async fn edit_sequence_pure_shift_of_cross_origin_feature_is_not_clipped() {
        // Delete 10 bp at 1-based 51..60 (internal 50..59): both segments keep
        // their base content (the first merely translates), so the feature
        // must show up in neither removedFeatures nor clippedFeatures.
        let server = handler_with_project(cross_origin_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "co_test".to_string(),
                start: 51,
                end: 60,
                replacement: Some(String::new()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["removedFeatures"], serde_json::json!([]), "{v}");
        assert_eq!(v["clippedFeatures"], serde_json::json!([]), "{v}");
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("co_test").unwrap();
        let f = p.features.iter().find(|f| f.name == "crossOrigin").unwrap();
        let spans: Vec<(i64, i64)> = f.segments.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(spans, vec![(180, 189), (0, 9)], "segments shifted, content intact");
    }

    #[tokio::test]
    async fn edit_sequence_clipped_cross_origin_feature_reports_segments() {
        // Delete the last 5 bp (1-based 196..200), clipping the tail of the
        // first segment: before/after are bounding spans on one basis and the
        // per-segment 1-based ranges ride along.
        let server = handler_with_project(cross_origin_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "co_test".to_string(),
                start: 196,
                end: 200,
                replacement: Some(String::new()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["removedFeatures"], serde_json::json!([]), "{v}");
        let c = &v["clippedFeatures"][0];
        assert_eq!(c["name"], "crossOrigin", "{v}");
        assert_eq!(c["before"], serde_json::json!({"start": 1, "end": 200}), "{v}");
        assert_eq!(c["after"], serde_json::json!({"start": 1, "end": 195}), "{v}");
        assert_eq!(
            c["beforeSegments"],
            serde_json::json!([{"start": 191, "end": 200}, {"start": 1, "end": 10}]),
            "{v}"
        );
        assert_eq!(
            c["afterSegments"],
            serde_json::json!([{"start": 191, "end": 195}, {"start": 1, "end": 10}]),
            "{v}"
        );
    }

    /// DNA/RNA replacements must be IUPAC nucleotide bases; a protein file as
    /// replacement_path for a DNA project is rejected (and vice versa).
    #[tokio::test]
    async fn edit_sequence_validates_replacement_alphabet_and_file_type() {
        let server = handler_with_project(edit_test_project()).await;

        // Non-IUPAC letters rejected on a DNA project.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 10,
                end: 12,
                replacement: Some("AXZ".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(out.0["message"].as_str().unwrap().contains("IUPAC"), "{}", out.0);

        // Degenerate IUPAC codes are accepted.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 10,
                end: 12,
                replacement: Some("NWR".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);

        // A protein .gpt as replacement_path for a DNA project is refused.
        let gpt_path = std::env::temp_dir()
            .join(format!("libregene-mcp-repl-{}.gpt", std::process::id()));
        write_convert_output(gpt_path.to_str().unwrap(), "MVS*", "protein", None, None).unwrap();
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 10,
                end: 12,
                replacement_path: Some(gpt_path.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(out.0["message"].as_str().unwrap().contains("protein"), "{}", out.0);
        std::fs::remove_file(&gpt_path).ok();
    }

    /// A U-bearing replacement inserted into a DNA project is normalized to T
    /// (the note reports the conversion); a no-cross insertion carries no note.
    #[tokio::test]
    async fn edit_sequence_normalizes_u_for_dna_project_and_notes_conversion() {
        let server = handler_with_project(edit_test_project()).await;

        // Insert "AUG" (has U) into the DNA project: normalization rewrites it
        // to "ATG" and the response must tell the caller a conversion happened.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 10,
                end: 9,
                replacement: Some("AUG".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        let note = out.0["notes"][0].as_str().unwrap_or_default();
        assert!(note.contains("Converted 1 U→T"), "{}", out.0);
        // No U left in the sequence after normalization.
        assert!(server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "edit_test".to_string(),
                start: Some(10),
                end: Some(20),
                ..Default::default()
            }))
            .await
            .unwrap()
            .0["sequence"]
            .as_str()
            .unwrap()
            .matches('U')
            .next()
            .is_none());

        // Same-alphabet insertion → no note.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 20,
                end: 19,
                replacement: Some("ATG".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        assert!(
            !out.0["notes"][0].as_str().unwrap_or_default().contains("Converted"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn edit_sequence_expected_old_checked_against_live_state() {
        let original = edit_test_project().sequence[..2].to_string();
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 1,
                end: 2,
                replacement: Some("TT".to_string()),
                expected_old: Some(original.clone()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        // A retry with the now-stale expected_old must fail, and
        // currentContent must reflect the LIVE sequence (TT), proving the
        // check re-reads the project instead of a stale snapshot.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 1,
                end: 2,
                replacement: Some("GG".to_string()),
                expected_old: Some(original),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert_eq!(out.0["currentContent"], "TT", "{}", out.0);
        let pm = server.pm.read().await;
        assert_eq!(
            &pm.get_project_by_id("edit_test").unwrap().sequence[..2],
            "TT",
            "failed edit must not mutate"
        );
    }
