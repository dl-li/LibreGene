use super::common::*;
use crate::mcp::*;

    #[tokio::test]
    async fn set_feature_create_converts_1based_to_internal_0based() {
        let server = handler_with_project(dna_test_project()).await;
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("cds1".to_string()),
                ftype: Some("CDS".to_string()),
                start: Some(1),
                end: Some(10),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        // The confirmation message echoes 1-based coordinates.
        assert!(
            out.0["message"].as_str().unwrap().contains("at 1..10"),
            "{}",
            out.0
        );
        let fid = out.0["featureId"].as_str().unwrap().to_string();
        let pm = server.pm.read().await;
        let f = pm
            .get_project_by_id("feat")
            .unwrap()
            .features
            .iter()
            .find(|f| f.id == fid)
            .unwrap()
            .clone();
        assert_eq!((f.start, f.end), (0, 9));
        assert_eq!(f.strand, "+");
        assert_eq!(f.segments.len(), 1);
        assert_eq!((f.segments[0].start, f.segments[0].end), (0, 9));
    }

    #[tokio::test]
    async fn set_feature_create_segments_and_bounds() {
        let server = handler_with_project(dna_test_project()).await;
        // Segmented (join) feature: 1-based interface, 0-based storage
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("seg1".to_string()),
                ftype: Some("CDS".to_string()),
                segments: Some(vec![
                    FeatureSegmentSpec { start: 1, end: 10 },
                    FeatureSegmentSpec { start: 20, end: 30 },
                ]),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        {
            let pm = server.pm.read().await;
            let f = &pm.get_project_by_id("feat").unwrap().features[0];
            assert_eq!((f.start, f.end), (0, 29));
            assert_eq!(f.strand, "-");
            assert_eq!(f.segments.len(), 2);
            assert_eq!((f.segments[1].start, f.segments[1].end), (19, 29));
        }

        // Single point (start == end)
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("pt".to_string()),
                ftype: Some("misc_feature".to_string()),
                start: Some(42),
                end: Some(42),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);

        // Out of range: 1-based end 101 is past the last valid base 100
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("oob".to_string()),
                ftype: Some("CDS".to_string()),
                start: Some(91),
                end: Some(101),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("out of range"),
            "{}",
            out.0
        );

        // Zero/negative start / reversed span / segments+start conflict / start alone
        for req in [
            SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("bad".to_string()),
                ftype: Some("CDS".to_string()),
                start: Some(0),
                end: Some(5),
                ..Default::default()
            },
            SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("bad".to_string()),
                ftype: Some("CDS".to_string()),
                start: Some(9),
                end: Some(5),
                ..Default::default()
            },
            SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("bad".to_string()),
                ftype: Some("CDS".to_string()),
                start: Some(1),
                end: Some(10),
                segments: Some(vec![FeatureSegmentSpec { start: 1, end: 10 }]),
                ..Default::default()
            },
            SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("bad".to_string()),
                ftype: Some("CDS".to_string()),
                start: Some(1),
                ..Default::default()
            },
            SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("bad".to_string()),
                ftype: Some("CDS".to_string()),
                ..Default::default()
            },
        ] {
            assert!(
                server.set_feature(Parameters(req)).await.is_err(),
                "expected invalid_params error"
            );
        }
    }

    #[tokio::test]
    async fn set_feature_update_span_and_segments() {
        let server = handler_with_project(dna_test_project()).await;
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("cds1".to_string()),
                ftype: Some("CDS".to_string()),
                start: Some(1),
                end: Some(10),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let fid = out.0["featureId"].as_str().unwrap().to_string();

        // Move the span; strand must be left untouched.
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                feature_id: Some(fid.clone()),
                start: Some(11),
                end: Some(20),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        {
            let pm = server.pm.read().await;
            let f = &pm.get_project_by_id("feat").unwrap().features[0];
            assert_eq!((f.start, f.end), (10, 19));
            assert_eq!(f.strand, "-", "span update must not touch strand");
        }

        // Replace with segments (join)
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                feature_id: Some(fid.clone()),
                segments: Some(vec![
                    FeatureSegmentSpec { start: 1, end: 10 },
                    FeatureSegmentSpec { start: 91, end: 100 },
                ]),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        {
            let pm = server.pm.read().await;
            let f = &pm.get_project_by_id("feat").unwrap().features[0];
            assert_eq!(f.segments.len(), 2);
            assert_eq!((f.start, f.end), (0, 99));
        }

        // Nothing to update
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                feature_id: Some(fid.clone()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("Nothing to update"),
            "{}",
            out.0
        );

        // Out-of-range span (1-based end 101 > length 100)
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                feature_id: Some(fid.clone()),
                start: Some(96),
                end: Some(101),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("out of range"),
            "{}",
            out.0
        );

        // start without end → invalid_params
        let req = SetFeatureRequest {
            project_id: "feat".to_string(),
            feature_id: Some(fid.clone()),
            start: Some(1),
            ..Default::default()
        };
        assert!(server.set_feature(Parameters(req)).await.is_err());
    }

    #[tokio::test]
    async fn set_feature_wrapping_segments_keep_join_order_bounds() {
        let mut project = dna_test_project();
        project.topology = "circular".to_string();
        let server = handler_with_project(project).await;
        // join(91..100, 1..10): bounds are first-segment start / last-segment
        // end (0-based 90/9, start > end), not the min/max flattening.
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("wrap".to_string()),
                ftype: Some("CDS".to_string()),
                segments: Some(vec![
                    FeatureSegmentSpec { start: 91, end: 100 },
                    FeatureSegmentSpec { start: 1, end: 10 },
                ]),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("at join(91..100,1..10)"),
            "{}",
            out.0
        );
        {
            let pm = server.pm.read().await;
            let f = &pm.get_project_by_id("feat").unwrap().features[0];
            assert_eq!((f.start, f.end), (90, 9));
            assert_eq!(f.segments.len(), 2);
            assert_eq!((f.segments[0].start, f.segments[0].end), (90, 99));
            assert_eq!((f.segments[1].start, f.segments[1].end), (0, 9));
        }

        // A wrap window whose far segment exceeds the length is rejected even
        // though the derived (small) end is in bounds.
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("wrap_oob".to_string()),
                ftype: Some("CDS".to_string()),
                segments: Some(vec![
                    FeatureSegmentSpec { start: 91, end: 120 },
                    FeatureSegmentSpec { start: 1, end: 10 },
                ]),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("out of range"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn set_feature_update_rejects_empty_name_and_notes() {
        let mut p = dna_test_project();
        p.features = vec![feature("f1", "gene", 0, 9, "+")];
        let server = handler_with_project(p).await;
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                feature_id: Some("f1".to_string()),
                name: Some(String::new()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("empty"),
            "{}",
            out.0
        );
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                feature_id: Some("f1".to_string()),
                notes: Some("n".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("not supported"),
            "{}",
            out.0
        );
        // The rejected updates left the feature untouched.
        let pm = server.pm.read().await;
        let f = &pm.get_project_by_id("feat").unwrap().features[0];
        assert_eq!(f.name, "gene");
        assert!(f.notes.is_empty());
    }

    #[tokio::test]
    async fn set_feature_segments_must_be_in_encoding_order() {
        let server = handler_with_project(dna_test_project()).await;
        // Two descending transitions can never be an origin wrap.
        let err = match server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("bad".to_string()),
                ftype: Some("CDS".to_string()),
                segments: Some(vec![
                    FeatureSegmentSpec { start: 30, end: 40 },
                    FeatureSegmentSpec { start: 20, end: 25 },
                    FeatureSegmentSpec { start: 1, end: 10 },
                ]),
                ..Default::default()
            }))
            .await
        {
            Err(e) => e,
            Ok(v) => panic!("expected encoding-order rejection, got {}", v.0),
        };
        assert!(err.message.contains("encoding order"), "{}", err.message);

        // A wrapping feature leads with its tail: accepted on circular.
        let mut p = dna_test_project();
        p.topology = "circular".to_string();
        let server = handler_with_project(p).await;
        let out = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "feat".to_string(),
                name: Some("wrap".to_string()),
                ftype: Some("CDS".to_string()),
                segments: Some(vec![
                    FeatureSegmentSpec { start: 91, end: 100 },
                    FeatureSegmentSpec { start: 1, end: 10 },
                ]),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
    }
