use super::common::*;
use crate::mcp::*;

fn operation(action: FeatureAction) -> FeatureOperation {
    FeatureOperation {
        action,
        feature_id: None,
        name: None,
        ftype: None,
        start: None,
        end: None,
        segments: None,
        strand: None,
        color: None,
        notes: None,
    }
}

fn create(name: &str, start: i64, end: i64) -> FeatureOperation {
    FeatureOperation {
        name: Some(name.to_string()),
        ftype: Some("CDS".to_string()),
        start: Some(start),
        end: Some(end),
        ..operation(FeatureAction::Create)
    }
}

async fn update(
    server: &LibreGeneMcp<tauri::test::MockRuntime>,
    operations: Vec<FeatureOperation>,
) -> serde_json::Value {
    server
        .update_features(Parameters(UpdateFeaturesRequest {
            project_id: "feat".to_string(),
            operations,
            include_text: None,
        }))
        .await
        .unwrap()
        .0
}

#[tokio::test]
async fn update_features_create_returns_structured_feature_and_converts_coordinates() {
    let server = handler_with_project(dna_test_project()).await;
    let out = update(&server, vec![create("cds1", 1, 10)]).await;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["operationCount"], 1);
    assert_eq!(out["createdCount"], 1);
    assert!(out.get("text").is_none(), "{out}");
    assert!(out.get("featureId").is_none(), "{out}");
    let result = &out["results"][0];
    assert_eq!(result["index"], 0);
    assert_eq!(result["action"], "create");
    assert_eq!(result["feature"]["start"], 1);
    assert_eq!(result["feature"]["end"], 10);
    let fid = result["featureId"].as_str().unwrap().to_string();
    let pm = server.pm.read().await;
    let f = pm
        .get_project_by_id("feat")
        .unwrap()
        .features
        .iter()
        .find(|f| f.id == fid)
        .unwrap();
    assert_eq!((f.start, f.end), (0, 9));
    assert_eq!(f.strand, "+");
    assert_eq!((f.segments[0].start, f.segments[0].end), (0, 9));
}

#[tokio::test]
async fn update_features_create_segments_and_rejects_bad_spans() {
    let server = handler_with_project(dna_test_project()).await;
    let mut op = create("seg1", 1, 10);
    op.start = None;
    op.end = None;
    op.segments = Some(vec![
        FeatureSegmentSpec { start: 1, end: 10 },
        FeatureSegmentSpec { start: 20, end: 30 },
    ]);
    op.strand = Some("-".to_string());
    let out = update(&server, vec![op]).await;
    assert_eq!(out["ok"], true, "{out}");
    {
        let pm = server.pm.read().await;
        let f = &pm.get_project_by_id("feat").unwrap().features[0];
        assert_eq!((f.start, f.end), (0, 29));
        assert_eq!(f.strand, "-");
        assert_eq!((f.segments[1].start, f.segments[1].end), (19, 29));
    }

    for mut op in [
        create("oob", 91, 101),
        create("zero", 0, 5),
        create("reverse", 9, 5),
    ] {
        if op.name.as_deref() == Some("zero") {
            op.ftype = Some("misc_feature".to_string());
        }
        let out = update(&server, vec![op]).await;
        assert_eq!(out["ok"], false, "{out}");
        assert_eq!(out["failedOperation"]["index"], 0);
    }
    let pm = server.pm.read().await;
    assert_eq!(pm.get_project_by_id("feat").unwrap().features.len(), 1);
}

#[tokio::test]
async fn update_features_update_span_notes_and_strand_semantics() {
    let mut project = dna_test_project();
    project.features = vec![feature("f1", "gene", 0, 9, "-")];
    let server = handler_with_project(project).await;
    let mut op = operation(FeatureAction::Update);
    op.feature_id = Some("f1".to_string());
    op.start = Some(11);
    op.end = Some(20);
    op.notes = Some("domain note".to_string());
    let out = update(&server, vec![op]).await;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["updatedCount"], 1);
    assert_eq!(out["results"][0]["feature"]["notes"], "domain note");
    {
        let pm = server.pm.read().await;
        let f = &pm.get_project_by_id("feat").unwrap().features[0];
        assert_eq!((f.start, f.end), (10, 19));
        assert_eq!(f.strand, "-", "span update must not touch strand");
        assert_eq!(f.notes, "domain note");
    }

    let mut clear = operation(FeatureAction::Update);
    clear.feature_id = Some("f1".to_string());
    clear.notes = Some(String::new());
    let out = update(&server, vec![clear]).await;
    assert_eq!(out["ok"], true, "{out}");
    let pm = server.pm.read().await;
    assert!(pm.get_project_by_id("feat").unwrap().features[0]
        .notes
        .is_empty());
}

#[tokio::test]
async fn update_features_wrapping_segments_keep_join_order_bounds() {
    let mut project = dna_test_project();
    project.topology = "circular".to_string();
    let server = handler_with_project(project).await;
    let mut op = create("wrap", 1, 10);
    op.start = None;
    op.end = None;
    op.segments = Some(vec![
        FeatureSegmentSpec {
            start: 91,
            end: 100,
        },
        FeatureSegmentSpec { start: 1, end: 10 },
    ]);
    let out = update(&server, vec![op]).await;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["results"][0]["feature"]["start"], 91);
    assert_eq!(out["results"][0]["feature"]["end"], 10);
    let pm = server.pm.read().await;
    let f = &pm.get_project_by_id("feat").unwrap().features[0];
    assert_eq!((f.start, f.end), (90, 9));
    assert_eq!((f.segments[0].start, f.segments[0].end), (90, 99));
    assert_eq!((f.segments[1].start, f.segments[1].end), (0, 9));
}

#[tokio::test]
async fn update_features_mixed_batch_is_one_structured_response() {
    let mut project = dna_test_project();
    project.features = vec![
        feature("old", "obsolete", 30, 39, "+"),
        feature("move", "before", 50, 59, "+"),
    ];
    let server = handler_with_project(project).await;
    let mut rename = operation(FeatureAction::Update);
    rename.feature_id = Some("move".to_string());
    rename.name = Some("after".to_string());
    let mut remove = operation(FeatureAction::Delete);
    remove.feature_id = Some("old".to_string());
    let out = update(&server, vec![create("new", 1, 10), rename, remove]).await;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["operationCount"], 3);
    assert_eq!(out["createdCount"], 1);
    assert_eq!(out["updatedCount"], 1);
    assert_eq!(out["deletedCount"], 1);
    assert_eq!(out["results"].as_array().unwrap().len(), 3);
    assert_eq!(out["results"][2]["feature"]["name"], "obsolete");
    let pm = server.pm.read().await;
    let features = &pm.get_project_by_id("feat").unwrap().features;
    assert_eq!(features.len(), 2);
    assert!(features.iter().any(|f| f.name == "new"));
    assert!(features.iter().any(|f| f.name == "after"));
    assert!(!features.iter().any(|f| f.id == "old"));
}

#[tokio::test]
async fn update_features_any_failure_rolls_back_the_whole_batch() {
    let mut project = dna_test_project();
    project.features = vec![feature("keep", "original", 30, 39, "+")];
    let server = handler_with_project(project).await;
    let mut rename = operation(FeatureAction::Update);
    rename.feature_id = Some("keep".to_string());
    rename.name = Some("must not persist".to_string());
    let mut missing = operation(FeatureAction::Delete);
    missing.feature_id = Some("missing".to_string());
    let out = update(&server, vec![create("transient", 1, 10), rename, missing]).await;
    assert_eq!(out["ok"], false, "{out}");
    assert_eq!(out["failedOperation"]["index"], 2);
    assert_eq!(out["failedOperation"]["action"], "delete");
    assert_eq!(out["failedOperation"]["featureId"], "missing");
    let pm = server.pm.read().await;
    let features = &pm.get_project_by_id("feat").unwrap().features;
    assert_eq!(features.len(), 1);
    assert_eq!(features[0].id, "keep");
    assert_eq!(features[0].name, "original");
}

#[tokio::test]
async fn update_features_validates_operation_shapes() {
    let server = handler_with_project(dna_test_project()).await;
    let out = update(&server, vec![]).await;
    assert_eq!(out["ok"], false);

    let mut create_with_id = create("bad", 1, 10);
    create_with_id.feature_id = Some("caller-id".to_string());
    let out = update(&server, vec![create_with_id]).await;
    assert_eq!(out["ok"], false, "{out}");

    let created = update(&server, vec![create("good", 1, 10)]).await;
    let fid = created["results"][0]["featureId"].as_str().unwrap();
    let mut noop = operation(FeatureAction::Update);
    noop.feature_id = Some(fid.to_string());
    let out = update(&server, vec![noop]).await;
    assert_eq!(out["ok"], false, "{out}");
    assert!(out["message"]
        .as_str()
        .unwrap()
        .contains("Nothing to update"));

    let mut bad_delete = operation(FeatureAction::Delete);
    bad_delete.feature_id = Some(fid.to_string());
    bad_delete.name = Some("not allowed".to_string());
    let out = update(&server, vec![bad_delete]).await;
    assert_eq!(out["ok"], false, "{out}");
    let pm = server.pm.read().await;
    assert_eq!(pm.get_project_by_id("feat").unwrap().features.len(), 1);
}

#[tokio::test]
async fn update_features_optional_text_is_explicit() {
    let server = handler_with_project(dna_test_project()).await;
    let out = server
        .update_features(Parameters(UpdateFeaturesRequest {
            project_id: "feat".to_string(),
            operations: vec![create("cds1", 1, 10)],
            include_text: Some(true),
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(out["ok"], true, "{out}");
    assert!(out["text"].as_str().unwrap().contains("FEATURES"), "{out}");
}

fn translation_project() -> libregene_core::models::ProjectData {
    libregene_core::models::ProjectData {
        name: "feat".to_string(),
        sequence: "ATGGTGAGCTAA".to_string(),
        length: 12,
        topology: "linear".to_string(),
        molecule_type: "dna".to_string(),
        ..Default::default()
    }
}

#[tokio::test]
async fn update_features_refreshes_translation_on_create_span_strand_and_recreate() {
    let server = handler_with_project(translation_project()).await;
    let created = update(&server, vec![create("CAR", 1, 9)]).await;
    assert_eq!(created["ok"], true, "{created}");
    assert_eq!(created["results"][0]["feature"]["translation"], "MVS");
    let fid = created["results"][0]["featureId"]
        .as_str()
        .unwrap()
        .to_string();

    let mut expand = operation(FeatureAction::Update);
    expand.feature_id = Some(fid.clone());
    expand.start = Some(1);
    expand.end = Some(12);
    let expanded = update(&server, vec![expand]).await;
    assert_eq!(expanded["ok"], true, "{expanded}");
    assert_eq!(expanded["results"][0]["feature"]["translation"], "MVS*");

    let mut reverse = operation(FeatureAction::Update);
    reverse.feature_id = Some(fid.clone());
    reverse.strand = Some("-".to_string());
    let reversed = update(&server, vec![reverse]).await;
    assert_eq!(reversed["ok"], true, "{reversed}");
    assert_eq!(reversed["results"][0]["feature"]["translation"], "LAHH");

    let mut remove = operation(FeatureAction::Delete);
    remove.feature_id = Some(fid);
    assert_eq!(update(&server, vec![remove]).await["ok"], true);
    let recreated = update(&server, vec![create("CAR", 1, 12)]).await;
    assert_eq!(recreated["ok"], true, "{recreated}");
    assert_eq!(recreated["results"][0]["feature"]["translation"], "MVS*");

    let pm = server.pm.read().await;
    let features = &pm.get_project_by_id("feat").unwrap().features;
    assert_eq!(features.len(), 1);
    assert_eq!(features[0].translation, "MVS*");
}

#[tokio::test]
async fn overview_distinguishes_derived_and_imported_translations() {
    let mut project = translation_project();
    let mut cds = feature("cds", "CAR", 0, 11, "+");
    cds.translation = "STALE".to_string();
    cds.qualifiers = vec![("translation".to_string(), "STALE".to_string())];
    project.features = vec![cds];
    libregene_core::translate::refresh_feature_translations(&mut project);
    let server = handler_with_project(project).await;
    let out = server
        .get_project_overview(Parameters(OverviewRequest {
            project_id: "feat".to_string(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    let feature = &out["features"][0];
    assert_eq!(feature["translation"], "MVS*", "{feature}");
    assert_eq!(feature["sourceTranslation"], "STALE", "{feature}");
    assert_eq!(feature["translationBasis"], "libregene-dna-derived");
    assert!(feature["qualifiers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|qualifier| qualifier[0] != "translation"));
}
