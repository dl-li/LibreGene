use super::common::*;
use crate::mcp::*;
use libregene_core::models::ProjectData;

    #[tokio::test]
    async fn find_restriction_sites_known_enzyme_without_site_returns_empty_sites() {
        // AgeI is in the enzyme database but does not cut this sequence:
        // report it with empty sites and a note instead of an "Unknown
        // enzyme" error.
        let server = handler_with_project(enzyme_test_project()).await;
        let out = server
            .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
                project_id: "enz".to_string(),
                enzymes: Some(vec!["agei".to_string()]),
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(v.get("ok").is_none(), "{v}");
        let entry = &v["enzymes"][0];
        assert_eq!(entry["name"], "AgeI", "{v}");
        assert_eq!(entry["sites"], serde_json::json!([]), "{v}");
        assert!(
            entry["note"].as_str().unwrap_or("").contains("no recognition site"),
            "{v}"
        );
        assert!(v.get("unknownEnzymes").is_none(), "{v}");
    }

    #[tokio::test]
    async fn find_restriction_sites_batch_degrades_partially() {
        // Mixed batch: EcoRI cuts, AgeI is known but has no site,
        // NotARealEnzyme is unknown — known names succeed, the unknown one
        // is listed under unknownEnzymes without failing the whole call.
        let server = handler_with_project(enzyme_test_project()).await;
        let out = server
            .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
                project_id: "enz".to_string(),
                enzymes: Some(vec![
                    "EcoRI".to_string(),
                    "AgeI".to_string(),
                    "NotARealEnzyme".to_string(),
                ]),
            }))
            .await
            .unwrap();
        let v = out.0;
        let names: Vec<&str> = v["enzymes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["AgeI", "EcoRI"], "{v}");
        assert_eq!(v["enzymes"][1]["sites"].as_array().unwrap().len(), 1, "{v}");
        assert_eq!(
            v["unknownEnzymes"][0]["name"], "NotARealEnzyme",
            "{v}"
        );
        assert!(
            v["unknownEnzymes"][0]["error"]
                .as_str()
                .unwrap_or("")
                .contains("not in the enzyme database"),
            "{v}"
        );
    }

    #[tokio::test]
    async fn find_restriction_sites_all_unknown_still_fails_with_suggestions() {
        // Every requested name unknown → keep the probe error.
        let server = handler_with_project(enzyme_test_project()).await;
        let out = server
            .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
                project_id: "enz".to_string(),
                enzymes: Some(vec!["EcoR".to_string()]),
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], false, "{v}");
        let msg = v["message"].as_str().unwrap_or("");
        assert!(msg.contains("Unknown enzyme 'EcoR'"), "{msg}");
        assert!(msg.contains("EcoRI"), "{msg}");
    }

    #[tokio::test]
    async fn find_restriction_sites_flags_type_iis_cuts_outside_recognition() {
        // BbsI (GAAGAC, cuts 2/6 nt downstream) at internal 0-based 40..45.
        let mut seq = "ACGT".repeat(50);
        seq.replace_range(40..46, "GAAGAC");
        let mut project = ProjectData {
            name: "iis".to_string(),
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
            .find(|e| e.name == "BbsI")
            .expect("BbsI site")
            .clone();
        let server = handler_with_project(project).await;
        let out = server
            .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
                project_id: "iis".to_string(),
                enzymes: Some(vec!["BbsI".to_string()]),
            }))
            .await
            .unwrap();
        let v = out.0;
        let site = &v["enzymes"][0]["sites"][0];
        let top = site["cuts"][0]["topCutIndex"].as_i64().unwrap();
        assert!(top > internal.rec_end + 1, "BbsI cuts downstream: {v}");
        assert_eq!(site["cutsOutsideRecognitionSite"], true, "{v}");
        assert!(
            site["note"].as_str().unwrap_or("").contains("type IIS"),
            "{v}"
        );
    }

    #[tokio::test]
    async fn find_restriction_sites_circular_origin_site_stays_in_range() {
        // 60 bp circle with a BbsI (GAAGAC, non-palindromic) bottom-strand
        // site GTCTTC at internal 0-based 0..5. Its cuts fall upstream
        // (negative), so the engine shifts the display frame and stores
        // rec_start/rec_end in [len, 2*len); MCP must wrap them back into
        // 1..=len.
        let mut seq = "ACGT".repeat(15);
        seq.replace_range(0..6, "GTCTTC");
        let mut project = ProjectData {
            name: "circ".to_string(),
            sequence: seq,
            length: 60,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        libregene_core::enzyme::recompute(&mut project);
        let internal = project
            .enzymes
            .iter()
            .find(|e| e.name == "BbsI")
            .expect("BbsI site")
            .clone();
        assert_eq!(internal.recognition_strand, "bottom");
        // Sanity: the engine really does store the shifted frame here.
        assert!(internal.rec_start >= 60, "engine frame shifted: {internal:?}");
        let server = handler_with_project(project).await;
        let out = server
            .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
                project_id: "circ".to_string(),
                enzymes: Some(vec!["BbsI".to_string()]),
            }))
            .await
            .unwrap();
        let v = out.0;
        let site = &v["enzymes"][0]["sites"][0];
        assert_eq!(site["recStart"], 1, "{v}");
        assert_eq!(site["recEnd"], 6, "{v}");
        for c in site["cuts"].as_array().unwrap() {
            let t = c["topCutIndex"].as_i64().unwrap();
            let b = c["botCutIndex"].as_i64().unwrap();
            assert!((1..=60).contains(&t), "topCutIndex in range: {v}");
            assert!((1..=60).contains(&b), "botCutIndex in range: {v}");
        }
        // BbsI genuinely cuts outside its recognition sequence (upstream of a
        // bottom-strand site), so the flag must survive the frame wrap.
        assert_eq!(site["cutsOutsideRecognitionSite"], true, "{v}");
    }

    #[tokio::test]
    async fn find_restriction_sites_circular_palindrome_inside_no_false_note() {
        // HindIII AAGCTT at internal 0-based 55..60 — the recognition spans
        // the origin (55..59,0) and its cuts stay inside the site. The
        // wrapped comparison must not flag a false cutsOutsideRecognitionSite.
        let mut seq = "ACGT".repeat(15);
        seq.replace_range(55..60, "AAGCT");
        seq.replace_range(0..1, "T");
        let mut project = ProjectData {
            name: "circ2".to_string(),
            sequence: seq,
            length: 60,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        libregene_core::enzyme::recompute(&mut project);
        assert!(project.enzymes.iter().any(|e| e.name == "HindIII"));
        let server = handler_with_project(project).await;
        let out = server
            .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
                project_id: "circ2".to_string(),
                enzymes: Some(vec!["HindIII".to_string()]),
            }))
            .await
            .unwrap();
        let v = out.0;
        let site = &v["enzymes"][0]["sites"][0];
        let rec_start = site["recStart"].as_i64().unwrap();
        let rec_end = site["recEnd"].as_i64().unwrap();
        assert!((1..=60).contains(&rec_start), "{v}");
        assert!((1..=60).contains(&rec_end), "{v}");
        // Origin-spanning recognition reads recStart > recEnd after wrapping.
        assert_eq!((rec_start, rec_end), (56, 1), "{v}");
        assert_eq!(site["cutsOutsideRecognitionSite"], false, "{v}");
    }
