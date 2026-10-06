use super::common::*;
use crate::mcp::*;
use libregene_core::models::ProjectData;

/// 200 bp with two EcoRI sites (G^AATTC at 0-based 40 and 140; cut indices
/// 41 and 141).
fn two_ecori_project(topology: &str) -> ProjectData {
    let mut seq = "ACGT".repeat(50);
    seq.replace_range(40..46, "GAATTC");
    seq.replace_range(140..146, "GAATTC");
    let mut p = ProjectData {
        name: format!("enz2_{}", topology),
        sequence: seq,
        length: 200,
        topology: topology.to_string(),
        molecule_type: "dna".to_string(),
        ..Default::default()
    };
    libregene_core::enzyme::recompute(&mut p);
    assert_eq!(p.enzymes.iter().filter(|e| e.name == "EcoRI").count(), 2);
    p
}

/// 200 bp with one EcoRI site (cut 41) and one BamHI site (cut 101).
fn ecori_bamhi_project(topology: &str) -> ProjectData {
    let mut seq = "ACGT".repeat(50);
    seq.replace_range(40..46, "GAATTC");
    seq.replace_range(100..106, "GGATCC");
    let mut p = ProjectData {
        name: format!("enz_eb_{}", topology),
        sequence: seq,
        length: 200,
        topology: topology.to_string(),
        molecule_type: "dna".to_string(),
        ..Default::default()
    };
    libregene_core::enzyme::recompute(&mut p);
    assert_eq!(p.enzymes.iter().filter(|e| e.name == "EcoRI").count(), 1);
    assert_eq!(p.enzymes.iter().filter(|e| e.name == "BamHI").count(), 1);
    p
}

fn add_req(project_id: &str) -> AddToWorkspaceRequest {
    AddToWorkspaceRequest {
        project_id: project_id.to_string(),
        ..Default::default()
    }
}

#[tokio::test]
async fn add_to_workspace_feature_rebases_and_flips_minus_strand() {
    let seq = synthetic_dna(200, 5);
    let project = ProjectData {
        name: "ws_feat".to_string(),
        sequence: seq.clone(),
        length: 200,
        topology: "linear".to_string(),
        molecule_type: "dna".to_string(),
        features: vec![
            feature("outer", "outer", 10, 89, "-"),
            feature("inner", "tag", 20, 29, "+"),
        ],
        ..Default::default()
    };
    let server = handler_with_project(project).await;
    let mut req = add_req("ws_feat");
    req.feature_id = Some("outer".to_string());
    let v = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v["ok"], true, "{v}");
    let entry = &v["added"][0];
    assert_eq!(v["workspaceCount"], 1, "{v}");
    assert_eq!(entry["length"], 80, "{v}");
    assert_eq!(entry["temporary"], true, "{v}");
    assert_eq!(entry["featureCount"], 2, "{v}");
    assert_hex7(&entry["sequenceHash"]);
    assert_hex7(&entry["revCompHash"]);
    // The minus-strand feature export reverse-complements: fragment content
    // is revcomp(seq[10..=89]); the nested feature lands flipped.
    let expected = libregene_core::utils::reverse_complement(&seq[10..=89]);
    let ws = server.workspace.read().await;
    let item = &ws[0];
    assert_eq!(item.sequence, expected);
    let outer = item.features.iter().find(|f| f.name == "outer").unwrap();
    assert_eq!((outer.start, outer.end, outer.strand.as_str()), (0, 79, "+"));
    let tag = item.features.iter().find(|f| f.name == "tag").unwrap();
    assert_eq!((tag.start, tag.end, tag.strand.as_str()), (60, 69, "-"));
}

#[tokio::test]
async fn add_to_workspace_region_wraps_origin_on_circular() {
    let project = cross_origin_test_project();
    let seq = project.sequence.clone();
    let server = handler_with_project(project).await;
    let mut req = add_req("co_test");
    req.start = Some(191);
    req.end = Some(10);
    let v = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["added"][0]["length"], 20, "{v}");
    let ws = server.workspace.read().await;
    assert_eq!(ws[0].sequence, format!("{}{}", &seq[190..], &seq[..=9]));
    let ori = ws[0].features.iter().find(|f| f.name == "crossOrigin").unwrap();
    assert_eq!((ori.start, ori.end), (0, 19));
}

#[tokio::test]
async fn add_to_workspace_single_enzyme_double_cut() {
    // Linear: exactly one middle fragment.
    let project = two_ecori_project("linear");
    let seq = project.sequence.clone();
    let server = handler_with_project(project).await;
    let mut req = add_req("enz2_linear");
    req.enzymes = Some(vec!["EcoRI".to_string()]);
    let v = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["added"].as_array().unwrap().len(), 1, "{v}");
    let ws = server.workspace.read().await;
    assert_eq!(ws[0].sequence, seq[41..=140].to_ascii_uppercase());

    // Circular: both arcs land in the workspace.
    let project = two_ecori_project("circular");
    let seq = project.sequence.clone();
    let server = handler_with_project(project).await;
    let mut req = add_req("enz2_circular");
    req.enzymes = Some(vec!["EcoRI".to_string()]);
    let v = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["added"].as_array().unwrap().len(), 2, "{v}");
    let ws = server.workspace.read().await;
    assert_eq!(ws[0].sequence, seq[41..=140].to_ascii_uppercase());
    let arc2 = format!("{}{}", &seq[141..], &seq[..=40]);
    assert_eq!(ws[1].sequence, arc2);
}

#[tokio::test]
async fn add_to_workspace_two_enzymes_respects_direction() {
    let project = ecori_bamhi_project("linear");
    let seq = project.sequence.clone();
    let server = handler_with_project(project).await;

    // EcoRI (cut 41) → BamHI (cut 101): forward fragment seq[41..=100].
    let mut req = add_req("enz_eb_linear");
    req.enzymes = Some(vec!["EcoRI".to_string(), "BamHI".to_string()]);
    let v = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v["ok"], true, "{v}");
    // BamHI → EcoRI: same span, reverse-complemented (direction honored).
    let mut req = add_req("enz_eb_linear");
    req.enzymes = Some(vec!["BamHI".to_string(), "EcoRI".to_string()]);
    let v2 = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v2["ok"], true, "{v2}");

    let ws = server.workspace.read().await;
    assert_eq!(ws[0].sequence, seq[41..=100].to_ascii_uppercase());
    let expected_rev = libregene_core::utils::reverse_complement(&seq[41..=100]);
    assert_eq!(ws[1].sequence, expected_rev);
}

#[tokio::test]
async fn add_to_workspace_two_enzymes_circular_forward_arc() {
    let project = ecori_bamhi_project("circular");
    let seq = project.sequence.clone();
    let server = handler_with_project(project).await;
    // BamHI (cut 101) → EcoRI (cut 41) wraps the origin.
    let mut req = add_req("enz_eb_circular");
    req.enzymes = Some(vec!["BamHI".to_string(), "EcoRI".to_string()]);
    let v = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v["ok"], true, "{v}");
    let ws = server.workspace.read().await;
    let expected = format!("{}{}", &seq[101..], &seq[..=40]);
    assert_eq!(ws[0].sequence, expected);
}

#[tokio::test]
async fn add_to_workspace_error_paths() {
    let server = handler_with_project(enzyme_test_project()).await;

    // No selector.
    let v = server.add_to_workspace(Parameters(add_req("enz"))).await.unwrap().0;
    assert_eq!(v["ok"], false, "{v}");

    // Several selectors.
    let mut req = add_req("enz");
    req.feature_id = Some("nope".to_string());
    req.start = Some(1);
    req.end = Some(10);
    let v = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v["ok"], false, "{v}");

    // Single-enzyme mode needs exactly two cuts; EcoRI has one here.
    let mut req = add_req("enz");
    req.enzymes = Some(vec!["EcoRI".to_string()]);
    let v = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v["ok"], false, "{v}");
    assert!(v["message"].as_str().unwrap().contains("1 site"), "{v}");

    // Unknown enzyme: ok:false with unknownEnzymes + similar suggestions.
    let mut req = add_req("enz");
    req.enzymes = Some(vec!["eco".to_string()]);
    let v = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v["ok"], false, "{v}");
    let unknown = &v["unknownEnzymes"][0];
    assert_eq!(unknown["name"], "eco", "{v}");
    assert!(
        unknown["similar"].as_array().unwrap().iter().any(|s| s == "EcoRI"),
        "{v}"
    );

    // Known enzyme without a site on this sequence.
    let mut req = add_req("enz");
    req.enzymes = Some(vec!["AgeI".to_string()]);
    let v = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    assert_eq!(v["ok"], false, "{v}");
    assert!(v["message"].as_str().unwrap().contains("no recognition site"), "{v}");

    // Enzyme mode is DNA-gated (MCP protocol error, not an envelope).
    let server = handler_with_project(rna_test_project()).await;
    let mut req = add_req("rna");
    req.enzymes = Some(vec!["EcoRI".to_string()]);
    assert!(server.add_to_workspace(Parameters(req)).await.is_err());
}

#[tokio::test]
async fn list_workspace_merges_projects_and_fragments() {
    let server = handler_with_project(edit_test_project()).await;
    let mut req = add_req("edit_test");
    req.feature_id = Some("f1".to_string());
    server.add_to_workspace(Parameters(req)).await.unwrap();

    let v = server.list_workspace().await.unwrap().0;
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["projects"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v["projects"][0]["id"], "edit_test", "{v}");
    // Open projects are implicit (non-temporary) members: the projects array
    // keeps its old shape with no `temporary` field.
    assert!(v["projects"][0].get("temporary").is_none(), "{v}");
    let ws = v["workspace"].as_array().unwrap();
    assert_eq!(ws.len(), 1, "{v}");
    assert_eq!(ws[0]["temporary"], true, "{v}");
    assert_eq!(ws[0]["length"], 51, "{v}");
    assert_eq!(ws[0]["moleculeType"], "dna", "{v}");
    assert_eq!(ws[0]["featureCount"], 1, "{v}");
    assert_hex7(&ws[0]["sequenceHash"]);
    assert_hex7(&ws[0]["revCompHash"]);
    assert!(v["message"].as_str().unwrap().contains("1 workspace fragment"), "{v}");
}

#[tokio::test]
async fn edit_sequence_replacement_hash_forward_and_flipped() {
    let project = edit_test_project();
    let seq = project.sequence.clone();
    let server = handler_with_project(project).await;
    let mut req = add_req("edit_test");
    req.feature_id = Some("f1".to_string());
    let added = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    let fwd = added["added"][0]["sequenceHash"].as_str().unwrap().to_string();
    let rev = added["added"][0]["revCompHash"].as_str().unwrap().to_string();
    let frag = seq[50..=100].to_ascii_uppercase();

    // Forward: pure insertion before base 1; annotations travel along.
    let v = server
        .edit_sequence(Parameters(EditSequenceRequest {
            project_id: "edit_test".to_string(),
            start: 1,
            end: 0,
            replacement_hash: Some(format!("{}/{}", fwd, rev)),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["newLength"], 251, "{v}");
    assert_eq!(v["transferredFeatures"].as_array().unwrap().len(), 1, "{v}");
    {
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(p.sequence, format!("{}{}", frag, seq));
        // 'gene' clashes with the existing feature and is uniquified; the
        // transferred copy covers the inserted span on the + strand.
        let t = p.features.iter().find(|f| f.start == 0 && f.end == 50).unwrap();
        assert_eq!(t.strand, "+");
    }

    // Flipped: swapped pair inserts the reverse complement.
    let v = server
        .edit_sequence(Parameters(EditSequenceRequest {
            project_id: "edit_test".to_string(),
            start: 1,
            end: 0,
            replacement_hash: Some(format!("{}/{}", rev, fwd)),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(v["ok"], true, "{v}");
    assert!(
        v["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("reverse complement")),
        "{v}"
    );
    let flipped_frag = libregene_core::utils::reverse_complement(&frag);
    let pm = server.pm.read().await;
    let p = pm.get_project_by_id("edit_test").unwrap();
    assert_eq!(p.sequence, format!("{}{}{}", flipped_frag, frag, seq));
    // The transferred feature of the flipped insert is strand-flipped.
    let t = p.features.iter().find(|f| f.start == 0 && f.end == 50).unwrap();
    assert_eq!(t.strand, "-");
}

#[tokio::test]
async fn hash_inputs_resolve_open_projects_and_fail_cleanly() {
    let project = alignment_test_project("linear");
    let seq = project.sequence.clone();
    let server = handler_with_project(project).await;

    // An open project is an implicit workspace member: its own hash pair is a
    // valid `hash` input (here: align the template against itself).
    let list = server.list_workspace().await.unwrap().0;
    let fwd = list["projects"][0]["sequenceHash"].as_str().unwrap().to_string();
    let rev = list["projects"][0]["revCompHash"].as_str().unwrap().to_string();
    let v = server
        .add_alignment(Parameters(AddAlignmentRequest {
            project_id: "aln_test".to_string(),
            name: "self".to_string(),
            hash: Some(format!("{}/{}", fwd, rev)),
            compact: Some(true),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["significant"], true, "{v}");
    assert_eq!(v["readLength"], 200, "{v}");
    assert_eq!(seq.len(), 200);

    // Unknown hash → ok:false, not a protocol error.
    let v = server
        .edit_sequence(Parameters(EditSequenceRequest {
            project_id: "aln_test".to_string(),
            start: 1,
            end: 0,
            replacement_hash: Some("0000000/1111111".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(v["ok"], false, "{v}");
    assert!(v["message"].as_str().unwrap().contains("list_workspace"), "{v}");
}

#[tokio::test]
async fn convert_sequence_hash_mode_defaults_from_entry_type() {
    let server = handler_with_project(edit_test_project()).await;
    let mut req = add_req("edit_test");
    req.start = Some(51);
    req.end = Some(101);
    server.add_to_workspace(Parameters(req)).await.unwrap();
    let list = server.list_workspace().await.unwrap().0;
    let hash = list["workspace"][0]["sequenceHash"].as_str().unwrap().to_string();

    let v = server
        .convert_sequence(Parameters(convert_req(vec![ConvertItem {
            hash: Some(hash),
            to: Some("rna".to_string()),
            ..Default::default()
        }])))
        .await
        .unwrap()
        .0;
    assert_eq!(v["ok"], true, "{v}");
    let item = &v["results"][0];
    assert_eq!(item["ok"], true, "{v}");
    assert_eq!(item["from"], "dna", "{v}");
    assert_eq!(item["to"], "rna", "{v}");
    let out = item["sequence"].as_str().unwrap();
    assert_eq!(out.len(), 51, "{v}");
    assert!(out.contains('U') && !out.contains('T'), "{v}");
}

#[tokio::test]
async fn add_primer_accepts_workspace_hash() {
    let project = dna_test_project();
    let seq = project.sequence.clone();
    let server = handler_with_project(project).await;
    let mut req = add_req("feat");
    req.start = Some(11);
    req.end = Some(30);
    let added = server.add_to_workspace(Parameters(req)).await.unwrap().0;
    let fwd = added["added"][0]["sequenceHash"].as_str().unwrap().to_string();
    let rev = added["added"][0]["revCompHash"].as_str().unwrap().to_string();

    let v = server
        .add_primer(Parameters(AddPrimerRequest {
            project_id: "feat".to_string(),
            name: "hp1".to_string(),
            r#type: "fwd".to_string(),
            seq: None,
            hash: Some(format!("{}/{}", fwd, rev)),
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["seq"], seq[10..30].to_ascii_uppercase(), "{v}");
    assert_eq!(v["bindingSiteCount"], 1, "{v}");
}
