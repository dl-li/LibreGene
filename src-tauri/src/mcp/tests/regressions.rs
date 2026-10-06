use super::common::*;
use crate::mcp::*;
use libregene_core::models::ProjectData;

    #[tokio::test]
    async fn edit_sequence_1based_bounds_and_insertion_message() {
        let server = handler_with_project(edit_test_project()).await;

        // start = 0 is invalid on the 1-based interface.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 0,
                end: 5,
                replacement: Some("ACGT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("1-based inclusive"),
            "{}",
            out.0
        );

        // Wrapping ranges are rejected (start > end+1).
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 50,
                end: 40,
                replacement: Some("ACGT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("start > end+1"),
            "{}",
            out.0
        );

        // Pure insertion before base 61 is start=61, end=60.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement: Some("TT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        assert!(
            out.0["message"]
                .as_str()
                .unwrap()
                .contains("Inserted 2 bp before base 61"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn check_primer_binding_reports_1based_sites() {
        let seq = synthetic_dna(200, 31);
        let project = ProjectData {
            name: "chk".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: "chk".to_string(),
                primers: vec![PrimerInput {
                    name: "p1".to_string(),
                    r#type: "fwd".to_string(),
                    seq: Some(seq[50..70].to_string()),
                    hash: None,
                }],
            }))
            .await
            .unwrap();
        let v = out.0;
        // Internal site [50, 70) → 1-based inclusive 51..70: templateStart
        // shifts by one, templateEnd keeps its value.
        assert_eq!(v["results"][0]["bindingSiteCount"], 1, "{v}");
        assert_eq!(v["results"][0]["site"]["templateStart"], 51, "{v}");
        assert_eq!(v["results"][0]["site"]["templateEnd"], 70, "{v}");
        assert_eq!(v["results"][0]["sites"][0]["templateStart"], 51, "{v}");
        assert_eq!(v["results"][0]["sites"][0]["templateEnd"], 70, "{v}");
    }

    #[tokio::test]
    async fn find_restriction_sites_reports_1based_coordinates() {
        // EcoRI GAATTC placed at internal 0-based 40..45.
        let mut seq = "ACGT".repeat(50);
        seq.replace_range(40..46, "GAATTC");
        let mut project = ProjectData {
            name: "enz".to_string(),
            sequence: seq,
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        libregene_core::enzyme::recompute(&mut project);
        let internal = project
            .enzymes
            .iter()
            .find(|e| e.name == "EcoRI")
            .expect("EcoRI site")
            .clone();
        let server = handler_with_project(project).await;
        let out = server
            .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
                project_id: "enz".to_string(),
                enzymes: Some(vec!["EcoRI".to_string()]),
            }))
            .await
            .unwrap();
        let v = out.0;
        let site = &v["enzymes"][0]["sites"][0];
        // recStart/recEnd shift by one; cut positions keep their value (a cut
        // at internal index C sits between the 1-based bases C and C+1).
        assert_eq!(site["recStart"], internal.rec_start + 1, "{v}");
        assert_eq!(site["recEnd"], internal.rec_end + 1, "{v}");
        assert_eq!(
            site["cuts"][0]["topCutIndex"],
            internal.cut_pairs[0].top_cut_index,
            "{v}"
        );
        assert_eq!(
            site["cuts"][0]["botCutIndex"],
            internal.cut_pairs[0].bot_cut_index,
            "{v}"
        );
    }

    #[test]
    fn from1_saturates_instead_of_overflowing() {
        assert_eq!(from1(1), 0);
        assert_eq!(from1(i64::MIN), i64::MIN);
    }

    #[test]
    fn site_json_to_1based_maps_circular_zero_template_end() {
        // A circular site ending exactly at the last base stores
        // templateEnd 0 (wrapped); the 1-based inclusive end is tlen.
        let mut site = serde_json::json!({"templateStart": 94, "templateEnd": 0});
        site_json_to_1based(&mut site, 100, true);
        assert_eq!(site["templateStart"], 95);
        assert_eq!(site["templateEnd"], 100);
        // Linear sites keep their value (0 never occurs for a real site).
        let mut lin = serde_json::json!({"templateStart": 0, "templateEnd": 20});
        site_json_to_1based(&mut lin, 100, false);
        assert_eq!(lin["templateStart"], 1);
        assert_eq!(lin["templateEnd"], 20);
    }

    #[tokio::test]
    async fn read_sequence_carries_sequence_hashes() {
        let project = edit_test_project();
        let seq = project.sequence.clone();
        let server = handler_with_project(project).await;
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "edit_test".to_string(),
                start: Some(1),
                end: Some(50),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        let h = assert_hex7(&v["sequenceHash"]);
        let rh = assert_hex7(&v["revCompHash"]);
        assert_eq!(h, libregene_core::utils::sequence_hash(&seq));
        assert_eq!(
            rh,
            libregene_core::utils::sequence_hash(&libregene_core::utils::reverse_complement(&seq))
        );
        assert_ne!(h, rh);
    }

    #[tokio::test]
    async fn edit_sequence_response_hash_matches_new_sequence() {
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "edit_test".to_string(),
                start: Some(1),
                end: Some(20),
                ..Default::default()
            }))
            .await
            .unwrap();
        let before = out.0["sequenceHash"].as_str().unwrap().to_string();

        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement: Some("AAACCCGGGTTT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        let after = assert_hex7(&v["sequenceHash"]);
        assert_hex7(&v["revCompHash"]);
        assert_ne!(before, after);
        let pm = server.pm.read().await;
        let live = &pm.get_project_by_id("edit_test").unwrap().sequence;
        assert_eq!(after, libregene_core::utils::sequence_hash(live));
        assert_eq!(live.len(), 212);
    }

    #[tokio::test]
    async fn protein_project_rev_comp_hash_is_null() {
        let server = handler_with_project(protein_test_project()).await;
        let out = server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "prot".to_string(),
                start: Some(1),
                end: Some(10),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_hex7(&v["sequenceHash"]);
        assert!(v.get("revCompHash").is_some(), "{v}");
        assert!(v["revCompHash"].is_null(), "{v}");
    }

    #[tokio::test]
    async fn list_workspace_entries_carry_sequence_hashes() {
        let server = handler_with_project(edit_test_project()).await;
        let out = server.list_workspace().await.unwrap();
        let v = out.0;
        let entry = v["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "edit_test")
            .expect("edit_test listed");
        assert_hex7(&entry["sequenceHash"]);
        assert_hex7(&entry["revCompHash"]);
    }
