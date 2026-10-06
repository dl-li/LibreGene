use super::common::*;
use crate::mcp::*;

    #[tokio::test]
    async fn project_summary_omits_name_prefix_when_nameless() {
        // A nameless project must not render as ": 200 bp linear".
        let mut p = edit_test_project();
        p.name = String::new();
        let server = handler_with_project(p).await;
        let msg = server.project_summary("").await.expect("summary");
        assert_eq!(msg, "200 bp linear", "{msg}");

        let server = handler_with_project(edit_test_project()).await;
        let msg = server.project_summary("edit_test").await.expect("summary");
        assert_eq!(msg, "edit_test: 200 bp linear", "{msg}");
    }

    #[tokio::test]
    async fn dna_only_tools_reject_protein_and_rna_projects() {
        let server = handler_with_project(protein_test_project()).await;

        let err = match server
            .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
                project_id: "prot".to_string(),
                enzymes: None,
            }))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("find_restriction_sites should reject a protein project"),
        };
        assert!(err.message.contains("only supports DNA"), "{}", err.message);

        // RNA projects are gated the same way
        let server = handler_with_project(rna_test_project()).await;
        let err = match server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: "rna".to_string(),
                primers: vec![PrimerInput {
                    name: "p1".to_string(),
                    r#type: "fwd".to_string(),
                    seq: "ACGTACGTAC".to_string(),
                }],
            }))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("check_primer_binding should reject an rna project"),
        };
        assert!(err.message.contains("only supports DNA"), "{}", err.message);
    }

    #[tokio::test]
    async fn convert_sequence_project_mode_rejects_protein_project() {
        let server = handler_with_project(protein_test_project()).await;
        let req = convert_req(vec![ConvertItem {
            project_id: Some("prot".to_string()),
            feature_id: Some("f1".to_string()),
            species: Some("e_coli".to_string()),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        let msg = out.0["results"][0]["message"].as_str().unwrap_or_default();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            msg.contains("inputPath") && msg.contains("reverse-translated"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn edit_sequence_protein_uppercases_and_validates_alphabet() {
        let server = handler_with_project(protein_test_project()).await;
        // lowercase replacement is normalized to uppercase and stored as-is
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "prot".to_string(),
                start: 1,
                end: 4,
                replacement: Some("mvs*".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert!(
            v["message"].as_str().unwrap().contains("aa"),
            "message should use aa units: {}",
            v["message"]
        );
        let pm = server.pm.read().await;
        assert_eq!(&pm.get_project_by_id("prot").unwrap().sequence[0..4], "MVS*");

        // non-amino-acid characters are rejected
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "prot".to_string(),
                start: 6,
                end: 9,
                replacement: Some("MVS1".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("amino-acid"),
            "{}",
            out.0
        );

        // a '*' anywhere but the end is rejected too
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "prot".to_string(),
                start: 6,
                end: 9,
                replacement: Some("M*VS".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        // the failed edits must not have mutated the sequence
        let pm = server.pm.read().await;
        assert_eq!(&pm.get_project_by_id("prot").unwrap().sequence[5..9], "EEDN");
    }

    #[tokio::test]
    async fn get_project_overview_protein_omits_dna_sections() {
        let server = handler_with_project(protein_test_project()).await;
        let out = server
            .get_project_overview(Parameters(OverviewRequest {
                project_id: "prot".to_string(),
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap();
        assert!(text.contains("50 aa"), "overview: {text}");
        assert!(!text.contains("PRIMERS"), "overview: {text}");
        assert!(!text.contains("ENZYMES"), "overview: {text}");
        // Auto-annotation runs on protein projects (aa-level CDS matching);
        // this synthetic 50 aa sequence matches nothing.
        assert!(
            text.contains("DETECTED COMMON FEATURES (auto):\n(none)"),
            "overview: {text}"
        );
    }
