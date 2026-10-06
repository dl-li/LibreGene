use super::common::*;
use crate::mcp::*;

/// Assert the unified envelope keys of a success response.
fn assert_envelope(v: &serde_json::Value, project_id: Option<&str>) {
    assert_eq!(v["ok"], true, "ok missing/false: {v}");
    assert!(v["message"].as_str().is_some_and(|m| !m.is_empty()), "message: {v}");
    if let Some(id) = project_id {
        assert_eq!(v["projectId"], id, "{v}");
    }
}

/// Sorted top-level keys of a JSON object, for shape comparisons.
fn keys(v: &serde_json::Value) -> Vec<String> {
    let mut k: Vec<String> = v
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    k.sort();
    k
}

#[test]
fn wire_params_are_camel_case() {
    // The camelCase spellings deserialize...
    let req: EditSequenceRequest = serde_json::from_value(serde_json::json!({
        "projectId": "p",
        "start": 1,
        "end": 2,
        "replacementPath": "/tmp/x.gbk",
        "expectedOld": "AC",
        "strand": "+",
    }))
    .expect("camelCase edit_sequence request");
    assert_eq!(req.project_id, "p");
    assert_eq!(req.replacement_path.as_deref(), Some("/tmp/x.gbk"));
    assert_eq!(req.expected_old.as_deref(), Some("AC"));

    // ...and the old snake_case spelling no longer addresses them.
    let stale = serde_json::from_value::<EditSequenceRequest>(serde_json::json!({
        "project_id": "p",
        "start": 1,
        "end": 2,
        "replacement": "AC",
    }));
    assert!(stale.is_err(), "snake_case params must not deserialize");

    let seq: SequenceRequest = serde_json::from_value(serde_json::json!({
        "projectId": "p",
        "featureId": "f1",
        "featureOffset": 3,
        "flank": 5,
    }))
    .expect("camelCase read_sequence request");
    assert_eq!(seq.feature_offset, Some(3));

    let convert: ConvertItem = serde_json::from_value(serde_json::json!({
        "inputPath": "/tmp/in.gbk",
        "outputPath": "/tmp/out.gbk",
        "revComp": true,
        "avoidEnzymeSites": ["GAATTC"],
    }))
    .expect("camelCase convert item");
    assert_eq!(convert.rev_comp, Some(true));
    assert_eq!(convert.output_path.as_deref(), Some("/tmp/out.gbk"));
}

#[tokio::test]
async fn read_tools_share_the_success_envelope() {
    let server = handler_with_project(enzyme_test_project()).await;

    let overview = server
        .get_project_overview(Parameters(OverviewRequest {
            project_id: "enz".to_string(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&overview, Some("enz"));
    assert_eq!(overview["unit"], "bp", "{overview}");
    assert!(overview["text"].is_string(), "{overview}");
    assert!(overview.get("regionView").is_none(), "{overview}");

    let region = server
        .get_region_view(Parameters(RegionRequest {
            project_id: "enz".to_string(),
            start: 1,
            end: 60,
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&region, Some("enz"));
    assert_eq!(region["region"], serde_json::json!({"start": 1, "end": 60}), "{region}");

    let read = server
        .read_sequence(Parameters(SequenceRequest {
            project_id: "enz".to_string(),
            start: Some(5),
            end: Some(25),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&read, Some("enz"));
    assert_eq!(read["start"], 5, "{read}");
    assert_eq!(read["end"], 25, "{read}");
    assert_eq!(read["length"], 21, "{read}");
    assert_eq!(read["sequence"].as_str().unwrap().len(), 21, "{read}");

    let catalog = server
        .list_enzymes(Parameters(EnzymeListRequest {
            query: Some("GAATTC".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&catalog, None);
    assert!(
        catalog["enzymes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "EcoRI"),
        "{catalog}"
    );
    assert_eq!(catalog["count"], catalog["total"], "{catalog}");

    let sites = server
        .find_restriction_sites(Parameters(FindRestrictionSitesRequest {
            project_id: "enz".to_string(),
            enzymes: Some(vec!["EcoRI".to_string()]),
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&sites, Some("enz"));
    let site = &sites["enzymes"][0]["sites"][0];
    let site_keys = keys(site);
    for expected in [
        "recStart",
        "recEnd",
        "recSeq",
        "strand",
        "cuts",
        "unique",
        "methylationBlocked",
        "hasCutsOutsideRecognitionSite",
    ] {
        assert!(site_keys.contains(&expected.to_string()), "{site}");
    }
    assert_eq!(sites["enzymes"][0]["siteCount"], 1, "{sites}");

    let primers = server
        .list_primers(Parameters(ListPrimersRequest {
            project_id: "enz".to_string(),
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&primers, Some("enz"));
    assert_eq!(primers["unit"], "bp", "{primers}");
}

#[tokio::test]
async fn list_projects_uses_the_envelope_without_a_project_id() {
    let server = handler_with_project(dna_test_project()).await;
    let v = server.list_projects().await.unwrap().0;
    assert_envelope(&v, None);
    assert!(v.get("projectId").is_none(), "{v}");
    assert_eq!(v["projects"][0]["unit"], "bp", "{v}");
    assert!(v["projects"][0]["sequenceHash"].is_string(), "{v}");
}

#[tokio::test]
async fn mutation_and_edit_tools_share_the_envelope() {
    let server = handler_with_project(edit_test_project()).await;

    let feature = server
        .set_feature(Parameters(SetFeatureRequest {
            project_id: "edit_test".to_string(),
            name: Some("tag".to_string()),
            ftype: Some("CDS".to_string()),
            start: Some(10),
            end: Some(30),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&feature, Some("edit_test"));
    assert_eq!(feature["unit"], "bp", "{feature}");
    assert!(feature["featureId"].is_string(), "{feature}");
    assert!(feature["text"].is_string(), "{feature}");

    let edited = server
        .edit_sequence(Parameters(EditSequenceRequest {
            project_id: "edit_test".to_string(),
            start: 10,
            end: 12,
            replacement: Some("AAA".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&edited, Some("edit_test"));
    assert_eq!(edited["unit"], "bp", "{edited}");
    assert!(edited.get("regionView").is_none(), "{edited}");
    assert!(edited.get("regionViewBefore").is_none(), "{edited}");
    assert!(edited["contextBefore"]["start"].as_i64().is_some(), "{edited}");
    assert!(edited["contextBefore"]["sequence"].as_str().is_some(), "{edited}");
}

#[tokio::test]
async fn failure_envelopes_keep_the_project_id() {
    let server = handler_with_project(edit_test_project()).await;
    let bad_range = server
        .edit_sequence(Parameters(EditSequenceRequest {
            project_id: "edit_test".to_string(),
            start: 500,
            end: 510,
            replacement: Some("A".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(bad_range["ok"], false, "{bad_range}");
    assert_eq!(bad_range["projectId"], "edit_test", "{bad_range}");
    assert!(
        bad_range["message"].as_str().unwrap().contains("out of bounds"),
        "{bad_range}"
    );
}

#[tokio::test]
async fn primer_site_shape_is_shared_by_all_three_primer_tools() {
    let project = dna_test_project();
    let template = project.sequence.clone();
    let server = handler_with_project(project).await;

    // A 20 nt oligo taken from the template binds it once.
    let seq = template[40..60].to_string();

    let added = server
        .add_primer(Parameters(AddPrimerRequest {
            project_id: "feat".to_string(),
            name: "p1".to_string(),
            r#type: "fwd".to_string(),
            seq: seq.clone(),
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&added, Some("feat"));
    assert!(added["primerId"].is_string(), "{added}");
    assert_eq!(added["length"], 20, "{added}");
    assert_eq!(added["bindingSiteCount"], 1, "{added}");
    let added_keys = keys(&added["sites"][0]);

    let listed = server
        .list_primers(Parameters(ListPrimersRequest {
            project_id: "feat".to_string(),
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(listed["primerCount"], 1, "{listed}");
    assert_eq!(keys(&listed["primers"][0]["sites"][0]), added_keys, "{listed}");

    let checked = server
        .check_primer_binding(Parameters(CheckPrimerBindingRequest {
            project_id: "feat".to_string(),
            primers: vec![PrimerInput {
                name: "p1".to_string(),
                r#type: "fwd".to_string(),
                seq: seq.clone(),
            }],
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&checked, Some("feat"));
    assert_eq!(keys(&checked["results"][0]["sites"][0]), added_keys, "{checked}");
    assert_eq!(checked["results"][0]["name"], "p1", "{checked}");
    assert_eq!(checked["results"][0]["type"], "fwd", "{checked}");
    assert_eq!(checked["results"][0]["primerLength"], 20, "{checked}");
    // add_primer and check_primer_binding must agree on the site.
    assert_eq!(
        added["sites"][0]["templateStart"],
        checked["results"][0]["sites"][0]["templateStart"],
        "{checked}"
    );
    assert_eq!(
        added["sites"][0]["annealLength"],
        checked["results"][0]["sites"][0]["annealLength"],
        "{checked}"
    );
}

#[tokio::test]
async fn check_primer_binding_reports_the_amplicon_size() {
    let project = dna_test_project();
    let template = project.sequence.clone();
    let server = handler_with_project(project).await;

    // Fwd binds 11..30 on the plus strand; rev binds 81..100 on the minus
    // strand → the product spans 11..100 = 90 bp.
    let fwd = template[10..30].to_string();
    let rev = libregene_core::utils::reverse_complement(&template[80..100]);

    let checked = server
        .check_primer_binding(Parameters(CheckPrimerBindingRequest {
            project_id: "feat".to_string(),
            primers: vec![
                PrimerInput { name: "F".to_string(), r#type: "fwd".to_string(), seq: fwd },
                PrimerInput { name: "R".to_string(), r#type: "rev".to_string(), seq: rev },
            ],
        }))
        .await
        .unwrap()
        .0;
    let amp = &checked["amplicon"];
    assert_eq!(amp["forwardStart"], 11, "{checked}");
    assert_eq!(amp["reverseEnd"], 100, "{checked}");
    assert_eq!(amp["length"], 90, "{checked}");
    assert!(amp["note"].as_str().is_some(), "{checked}");

    // A single primer cannot define an amplicon.
    let single = server
        .check_primer_binding(Parameters(CheckPrimerBindingRequest {
            project_id: "feat".to_string(),
            primers: vec![PrimerInput {
                name: "F".to_string(),
                r#type: "fwd".to_string(),
                seq: template[10..30].to_string(),
            }],
        }))
        .await
        .unwrap()
        .0;
    assert!(single.get("amplicon").is_none(), "{single}");
}

#[tokio::test]
async fn design_primers_candidates_are_labelled_and_recommended() {
    let server = handler_with_project(alignment_test_project("linear")).await;
    let v = server
        .design_primers(Parameters(DesignPrimersRequest {
            project_id: "aln_test".to_string(),
            mode: "amplify".to_string(),
            seg: Some(SegParam { start: 21, end: 120 }),
            target_tm: 60.0,
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&v, Some("aln_test"));
    assert_eq!(v["mode"], "amplify", "{v}");
    let groups = v["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2, "{v}");
    for group in groups {
        let ty = group["type"].as_str().unwrap();
        let idx = group["recommendedIndex"].as_u64().unwrap() as usize;
        let cands = group["candidates"].as_array().unwrap();
        assert!(idx < cands.len(), "{group}");
        for (i, c) in cands.iter().enumerate() {
            assert_eq!(c["id"], format!("{}-{}", ty, i + 1), "{c}");
            assert_eq!(c["recommended"], i == idx, "{c}");
            for key in ["seq", "tail", "tailLength", "annealLength", "tm", "gcPercent"] {
                assert!(c.get(key).is_some(), "candidate missing {key}: {c}");
            }
            assert!(c.get("annealLen").is_none(), "{c}");
            assert!(c.get("gc").is_none(), "{c}");
        }
        assert_eq!(cands.iter().filter(|c| c["recommended"] == true).count(), 1, "{group}");
    }
}

#[tokio::test]
async fn design_primers_reports_mode_mismatched_parameters() {
    let server = handler_with_project(alignment_test_project("linear")).await;
    let v = server
        .design_primers(Parameters(DesignPrimersRequest {
            project_id: "aln_test".to_string(),
            mode: "amplify".to_string(),
            seg: Some(SegParam { start: 21, end: 120 }),
            target_tm: 60.0,
            // oepcr-only parameters on an amplify call
            name1: Some("left".to_string()),
            seg2: Some(SegParam { start: 130, end: 180 }),
            // mutagenesis-only parameter
            mut_seq: Some("AAA".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&v, Some("aln_test"));
    let notes = v["notes"].as_array().expect("notes array");
    let joined = notes
        .iter()
        .filter_map(|n| n.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(joined.contains("oepcr"), "{v}");
    assert!(joined.contains("mutagenesis"), "{v}");
    assert_eq!(notes.len(), 2, "{v}");
}

#[tokio::test]
async fn reverse_translation_without_a_stop_codon_is_noted() {
    let server = test_handler();
    let out = server
        .convert_sequence(Parameters(convert_req(vec![ConvertItem {
            sequence: Some("MVS".to_string()),
            from: Some("protein".to_string()),
            to: Some("dna".to_string()),
            species: Some("e_coli".to_string()),
            ..Default::default()
        }])))
        .await
        .unwrap()
        .0;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["okCount"], 1, "{out}");
    let item = &out["results"][0];
    assert!(
        item["notes"][0].as_str().unwrap().contains("stop codon"),
        "{item}"
    );
    // gcPercent* are percentages (0-100), not fractions.
    let with_stop = server
        .convert_sequence(Parameters(convert_req(vec![ConvertItem {
            sequence: Some("MVS*".to_string()),
            from: Some("protein".to_string()),
            to: Some("dna".to_string()),
            species: Some("e_coli".to_string()),
            ..Default::default()
        }])))
        .await
        .unwrap()
        .0;
    let item = &with_stop["results"][0];
    assert!(item.get("notes").is_none(), "{item}");
    let gc = item["gcPercentAfter"].as_f64().unwrap();
    assert!((0.0..=100.0).contains(&gc), "{item}");
    assert_eq!(item["gcPercentAfter"], item["gcPercentBefore"], "{item}");
}

#[test]
fn tool_descriptions_are_concise_and_state_their_response() {
    let tools = LibreGeneMcp::<tauri::test::MockRuntime>::tool_router().list_all();
    assert_eq!(tools.len(), 17, "unexpected tool count");
    for t in tools {
        assert_ne!(t.name, "list_species", "species keys live in the convert_sequence description");
        assert_ne!(t.name, "close_project", "close_project was removed");
        assert_ne!(t.name, "search_sequence", "search_sequence is a bash-replaceable string scan");
        let d = t.description.as_deref().unwrap_or_default();
        assert!(!d.is_empty(), "{} has no description", t.name);
        assert!(
            d.to_lowercase().contains("returns"),
            "{} does not state its response shape",
            t.name
        );
        // Concise enough to stay readable in an agent's context: the longest
        // tool (convert_sequence) sits well under this bound.
        assert!(
            d.len() <= 3000,
            "{} description is {} chars — condense it",
            t.name,
            d.len()
        );
    }
}

#[tokio::test]
async fn convert_sequence_description_lists_the_builtin_species() {
    // The species keys have no tool of their own: `list_tools` splices the
    // authoritative core table into the convert_sequence description.
    let mut tools = LibreGeneMcp::<tauri::test::MockRuntime>::tool_router().list_all();
    crate::mcp::splice_species_keys(&mut tools);
    let convert = tools
        .iter()
        .find(|t| t.name == "convert_sequence")
        .expect("convert_sequence tool");
    let desc = convert.description.as_deref().unwrap_or_default();
    assert!(!desc.contains("{species}"), "placeholder left unreplaced: {desc}");
    let keys = libregene_core::codon::list_species();
    assert!(!keys.is_empty(), "no built-in species");
    for key in keys {
        assert!(desc.contains(key), "missing species '{key}' in: {desc}");
    }
    for t in &tools {
        let d = t.description.as_deref().unwrap_or_default();
        assert!(!d.contains("{species}"), "{} left a placeholder", t.name);
    }
}

#[tokio::test]
async fn list_enzymes_filters_by_name_and_site_and_reports_the_full_total() {
    let server = test_handler();

    let by_name = server
        .list_enzymes(Parameters(EnzymeListRequest {
            query: Some("eco".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_envelope(&by_name, None);
    assert!(by_name["total"].as_u64().unwrap() >= 1, "{by_name}");
    assert!(
        by_name["enzymes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["name"].as_str().unwrap().to_lowercase().contains("eco")
                || e["site"].as_str().unwrap().to_lowercase().contains("eco")),
        "{by_name}"
    );

    // A recognition-site query finds the enzyme(s) that cut it.
    let by_site = server
        .list_enzymes(Parameters(EnzymeListRequest {
            query: Some("GAATTC".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert!(
        by_site["enzymes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "EcoRI"),
        "{by_site}"
    );

    // limit caps the page but total reports every match.
    let paged = server
        .list_enzymes(Parameters(EnzymeListRequest {
            limit: Some(2),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(paged["count"], 2, "{paged}");
    assert!(paged["total"].as_u64().unwrap() > 2, "{paged}");
}

#[tokio::test]
async fn region_view_alignment_columns_are_opt_in() {
    use libregene_core::models::{AlignSegment, Alignment};

    let mut project = alignment_test_project("linear");
    let read = project.sequence[10..=29].to_string();
    project.alignments = vec![Alignment {
        id: "aln-1".into(),
        name: "read1".into(),
        length: read.len(),
        strand: "+".into(),
        identity: 1.0,
        segments: vec![AlignSegment {
            start: 10,
            end: 29,
            chars: read.clone(),
        }],
        insertions: Vec::new(),
        seq: read,
        trace_path: None,
    }];
    let server = handler_with_project(project).await;

    let off = server
        .get_region_view(Parameters(RegionRequest {
            project_id: "aln_test".to_string(),
            start: 1,
            end: 40,
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert!(off["text"].as_str().unwrap().contains("ALIGNMENT DIFFS"), "{off}");
    assert!(!off["text"].as_str().unwrap().contains("ALIGNMENT VIEW"), "{off}");

    let on = server
        .get_region_view(Parameters(RegionRequest {
            project_id: "aln_test".to_string(),
            start: 1,
            end: 40,
            show_alignment_columns: Some(true),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert!(on["text"].as_str().unwrap().contains("ALIGNMENT VIEW IN REGION"), "{on}");
}
