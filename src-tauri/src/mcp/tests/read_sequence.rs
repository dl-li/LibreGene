use super::common::*;
use crate::mcp::*;
use libregene_core::models::ProjectData;
use libregene_core::models::Segment;

    #[tokio::test]
    async fn read_sequence_coordinate_input_forms() {
        // "ATGGTATAA" -> M V *; CDS on plus strand covers positions 1..9.
        let seq = "ATGGTATAA".to_string();
        let project = ProjectData {
            name: "coord_test".to_string(),
            sequence: seq.clone(),
            length: 9,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "orf", 0, 8, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;

        // 1. Template position input.
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "coord_test".to_string(),
                position: Some(2),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["projectId"], "coord_test", "{v}");
        assert_eq!(v["input"]["mode"], "position", "{v}");
        assert_eq!(v["input"]["position"], 2, "{v}");
        assert_eq!(v["position"], 2, "{v}");
        assert_eq!(v["base"], "T", "{v}");
        assert_eq!(v["features"][0]["featureOffset"], 2, "{v}");
        assert_eq!(v["translations"][0]["codonIndex"], 1, "{v}");
        assert_eq!(v["translations"][0]["codonBaseIndex"], 2, "{v}");

        // 2. Feature offset input -> same position.
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "coord_test".to_string(),
                feature_id: Some("cds1".to_string()),
                feature_offset: Some(2),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["input"]["mode"], "featureOffset", "{v}");
        assert_eq!(v["position"], 2, "{v}");
        assert_eq!(v["features"][0]["featureOffset"], 2, "{v}");

        // 3. Amino-acid position input -> codon positions and translation.
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "coord_test".to_string(),
                feature_id: Some("cds1".to_string()),
                aa_position: Some(2),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["input"]["mode"], "aminoAcid", "{v}");
        assert_eq!(v["input"]["aaPosition"], 2, "{v}");
        assert_eq!(v["position"], 4, "{v}");
        assert_eq!(v["base"], "G", "{v}");
        assert_eq!(v["codonPositions"], serde_json::json!([4, 5, 6]), "{v}");
        assert_eq!(v["translations"][0]["codonIndex"], 2, "{v}");
        assert!(
            v["translations"][0].get("aaPositionExcludingMet").is_none(),
            "only one amino-acid numbering is reported: {v}"
        );
        assert_eq!(v["translations"][0]["codon"], "GTA", "{v}");
        assert_eq!(v["translations"][0]["aminoAcid"], "V", "{v}");

        // Mutual-exclusion error.
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "coord_test".to_string(),
                position: Some(2),
                feature_id: Some("cds1".to_string()),
                feature_offset: Some(2),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("exactly one of"),
            "{}",
            out.0
        );

        // Out-of-bounds error.
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "coord_test".to_string(),
                position: Some(100),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("out of bounds"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn read_sequence_coordinate_minus_strand_segmented_round_trip() {
        // Same minus-strand segmented CDS used in coords.rs tests: translates to FH.
        let seq = "ATGAAATTTAAA".to_string();
        let mut f = feature("mEGFP", "mEGFP", 0, 5, "-");
        f.segments = vec![
            Segment {
                start: 0,
                end: 2,
                color: None,
            },
            Segment {
                start: 3,
                end: 5,
                color: None,
            },
        ];
        let project = ProjectData {
            name: "coord_minus_test".to_string(),
            sequence: seq.clone(),
            length: 12,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![f],
            ..Default::default()
        };
        let server = handler_with_project(project).await;

        // aaPosition=2 (H) on the minus strand: 5'→3' codon order runs from
        // the higher template coordinate to the lower one (positions 3,2,1 in
        // 1-based), because the CDS's 5' end is at the right-hand segment.
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "coord_minus_test".to_string(),
                feature_id: Some("mEGFP".to_string()),
                aa_position: Some(2),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["codonPositions"], serde_json::json!([3, 2, 1]), "{v}");
        assert_eq!(v["translations"][0]["codon"], "CAT", "{v}");
        assert_eq!(v["translations"][0]["aminoAcid"], "H", "{v}");

        // Convert the first codon position (5' end, 1-based 3) back via template input.
        let pos = v["codonPositions"][0].as_i64().unwrap();
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "coord_minus_test".to_string(),
                position: Some(pos),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["translations"][0]["codonIndex"], 2, "{v}");
        assert_eq!(v["translations"][0]["codonBaseIndex"], 1, "{v}");
    }

    #[tokio::test]
    async fn read_sequence_window_is_1based() {
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "edit_test".to_string(),
                start: Some(1),
                end: Some(10),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        let text = v["text"].as_str().unwrap();
        assert!(text.contains("Window 1..10 (10 bp)"), "{text}");
        let seq = synthetic_dna(200, 42);
        assert_eq!(
            v["sequence"].as_str().unwrap(),
            seq[0..10].to_ascii_uppercase(),
            "1-based 1..10 reads internal bases 0..=9"
        );
    }

    #[tokio::test]
    async fn read_sequence_window_reports_endpoint_contexts() {
        // CDS at internal 20..49 (1-based 21..50): a 21..50 window starts and
        // ends exactly on the feature boundaries.
        let mut project = edit_test_project();
        project.features = vec![feature("cds1", "mEGFP", 20, 49, "+")];
        let server = handler_with_project(project).await;
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "edit_test".to_string(),
                start: Some(21),
                end: Some(50),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        let start_ctx = &v["startContext"];
        assert_eq!(start_ctx["position"], 21, "{v}");
        assert_eq!(start_ctx["features"][0]["name"], "mEGFP", "{v}");
        assert_eq!(start_ctx["features"][0]["featureOffset"], 1, "{v}");
        let end_ctx = &v["endContext"];
        assert_eq!(end_ctx["position"], 50, "{v}");
        assert_eq!(end_ctx["features"][0]["featureOffset"], 30, "{v}");
    }
