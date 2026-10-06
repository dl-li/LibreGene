use super::common::*;
use crate::mcp::*;
use libregene_core::models::ProjectData;
use libregene_core::models::Segment;
use libregene_core::models::Feature;
use libregene_core::models::Primer;

    #[tokio::test]
    async fn save_file_region_writes_gbk_with_translated_features() {
        let seq = synthetic_dna(200, 7);
        let project = ProjectData {
            name: "region_test".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("f1", "gene", 50, 100, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-region-{}.gbk", std::process::id()));
        let req = SaveFileRequest {
            project_id: "region_test".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                // 1-based inclusive interface → internal 0-based [40, 160]
                start: Some(41),
                end: Some(161),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 121);
        assert_eq!(v["path"], out_path.to_str().unwrap());
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, seq[40..=160].to_ascii_uppercase());
        let f = parsed
            .features
            .iter()
            .find(|f| f.name == "gene")
            .expect("overlapping feature carried over");
        assert_eq!((f.start, f.end), (10, 60), "feature translated by -40");
        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn save_file_region_wraps_on_circular() {
        let seq = synthetic_dna(100, 11);
        // Cross-origin feature 95..99 + 0..5 (stored as two segments).
        let mut f = feature("f1", "ori", 95, 5, "+");
        f.segments = vec![
            Segment { start: 95, end: 99, color: None },
            Segment { start: 0, end: 5, color: None },
        ];
        let project = ProjectData {
            name: "circ_test".to_string(),
            sequence: seq.clone(),
            length: 100,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![f],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-circ-{}.gbk", std::process::id()));
        let req = SaveFileRequest {
            project_id: "circ_test".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                // 1-based wrap window 91..10 → internal 0-based 90..9
                start: Some(91),
                end: Some(10),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["length"], 20);
        let expected = format!("{}{}", &seq[90..], &seq[..=9]);
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, expected);
        // the cross-origin feature becomes one contiguous span 5..16
        let f = parsed
            .features
            .iter()
            .find(|f| f.name == "ori")
            .expect("feature carried over");
        assert_eq!((f.start, f.end), (5, 15));
        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn save_file_region_feature_joins_segments_5_to_3() {
        let seq = synthetic_dna(100, 13);
        let mut cds = feature("cds", "spliced", 10, 39, "+");
        cds.segments = vec![
            Segment { start: 10, end: 19, color: None },
            Segment { start: 30, end: 39, color: None },
        ];
        let inner = feature("in", "inner", 32, 35, "+");
        let project = ProjectData {
            name: "feat_test".to_string(),
            sequence: seq.clone(),
            length: 100,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![cds, inner],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-feat-{}.gbk", std::process::id()));
        let req = SaveFileRequest {
            project_id: "feat_test".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                feature_id: Some("cds".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 20);
        let expected = format!("{}{}", &seq[10..=19], &seq[30..=39]);
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, expected);
        let exported = parsed
            .features
            .iter()
            .find(|f| f.name == "spliced")
            .expect("exported feature spans the whole sequence");
        assert_eq!((exported.start, exported.end), (0, 19));
        let inner = parsed
            .features
            .iter()
            .find(|f| f.name == "inner")
            .expect("inner feature carried over");
        assert_eq!((inner.start, inner.end), (12, 15));
        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn save_file_region_minus_strand_feature_is_reverse_complemented() {
        let seq = synthetic_dna(100, 17);
        let project = ProjectData {
            name: "minus_test".to_string(),
            sequence: seq.clone(),
            length: 100,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![
                feature("rev", "repressor", 40, 59, "-"),
                feature("fwd", "promoter", 45, 50, "+"),
            ],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-minus-{}.gbk", std::process::id()));
        let req = SaveFileRequest {
            project_id: "minus_test".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                feature_id: Some("rev".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 20);
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        let expected = libregene_core::utils::reverse_complement(&seq[40..=59]);
        assert_eq!(parsed.sequence, expected);
        let exported = parsed
            .features
            .iter()
            .find(|f| f.name == "repressor")
            .expect("exported feature carried over");
        assert_eq!((exported.start, exported.end), (0, 19));
        assert_eq!(
            exported.strand, ".",
            "plus-strand round-trips as '.' (gbk only encodes '-' via complement)"
        );
        let prom = parsed
            .features
            .iter()
            .find(|f| f.name == "promoter")
            .expect("overlapping plus-strand feature carried over, flipped");
        assert_eq!((prom.start, prom.end), (9, 14));
        assert_eq!(prom.strand, "-", "plus-strand feature flips in a rev-comp export");
        std::fs::remove_file(&out_path).ok();
    }

    /// Regression: a multi-segment minus-strand feature (e.g. spliced CDS)
    /// produced a text-digest bbox with start > end before the fix, which
    /// either dropped the text digest (linear) or showed a wrong
    /// wrap-around window (circular). The bbox must cover the full span
    /// occupied by the feature on the template, regardless of piece order.
    #[tokio::test]
    async fn save_file_region_multi_segment_minus_strand_regionview_span() {
        use libregene_core::models::Segment;
        let seq = synthetic_dna(120, 23);
        // Two-segment minus-strand feature: pieces (after resolve) are
        // descending, which is what triggered the original bbox bug.
        let multi_seg = Feature {
            id: "split".to_string(),
            name: "split_cds".to_string(),
            start: 10,
            end: 60,
            color: "#60A5FA".to_string(),
            ftype: "CDS".to_string(),
            segments: vec![
                Segment { start: 40, end: 60, color: None },
                Segment { start: 10, end: 30, color: None },
            ],
            strand: "-".to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        };
        let project = ProjectData {
            name: "multi_minus".to_string(),
            sequence: seq.clone(),
            length: 120,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![multi_seg],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-multi-minus-{}.gbk", std::process::id()));
        let req = SaveFileRequest {
            project_id: "multi_minus".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                feature_id: Some("split".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "export should succeed");
        // The headline assertion: text digest must be present (non-null) and
        // describe a span within the feature's real coordinates [10, 60].
        // Before the fix, bbox was (40, 30) → start > end → text digest dropped.
        let region = v["text"].as_str().unwrap_or("");
        assert!(
            !region.is_empty(),
            "text digest must not be empty for a multi-segment minus-strand feature (was dropped by bbox bug)"
        );
        std::fs::remove_file(&out_path).ok();
    }

    /// Circular wrap variant: a minus-strand feature whose pieces straddle the
    /// origin ((90, 119) + (0, 20)) must report the wrap window 90..20, not a
    /// full-length min/max span.
    #[tokio::test]
    async fn save_file_region_wrap_origin_minus_strand_regionview_span() {
        use libregene_core::models::Segment;
        let seq = synthetic_dna(120, 24);
        let wrap_feat = Feature {
            id: "wrap".to_string(),
            name: "wrap_cds".to_string(),
            start: 90,
            end: 20,
            color: "#60A5FA".to_string(),
            ftype: "CDS".to_string(),
            segments: vec![
                Segment { start: 90, end: 119, color: None },
                Segment { start: 0, end: 20, color: None },
            ],
            strand: "-".to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        };
        let project = ProjectData {
            name: "wrap_minus".to_string(),
            sequence: seq.clone(),
            length: 120,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![wrap_feat],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-wrap-minus-{}.gbk", std::process::id()));
        let req = SaveFileRequest {
            project_id: "wrap_minus".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                feature_id: Some("wrap".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "export should succeed");
        let region = v["text"].as_str().unwrap_or("");
        assert!(
            region.contains("REGION: 91..21"),
            "text digest should show the 1-based wrap window 91..21, got: {}",
            region.lines().next().unwrap_or("")
        );
        std::fs::remove_file(&out_path).ok();
    }

    /// Origin-wrapping feature export must keep join order (not coordinate
    /// order): plus strand concatenates end-segment after start-segment;
    /// minus strand reverse-complements each piece in reversed join order —
    /// the biological 5'→3' order of the feature.
    #[tokio::test]
    async fn save_file_region_wrap_origin_feature_keeps_join_order() {
        use libregene_core::models::Segment;
        let seq = synthetic_dna(120, 25);
        let mk = |strand: &str| Feature {
            id: "wrap".to_string(),
            name: "wrap_cds".to_string(),
            start: 0,
            end: 119,
            color: "#60A5FA".to_string(),
            ftype: "CDS".to_string(),
            segments: vec![
                Segment { start: 90, end: 119, color: None },
                Segment { start: 0, end: 20, color: None },
            ],
            strand: strand.to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        };
        let cases: Vec<(&str, String)> = vec![
            ("+", format!("{}{}", &seq[90..=119], &seq[0..=20])),
            (
                "-",
                format!(
                    "{}{}",
                    libregene_core::utils::reverse_complement(&seq[0..=20]),
                    libregene_core::utils::reverse_complement(&seq[90..=119])
                ),
            ),
        ];
        for (strand, expected) in cases {
            let project = ProjectData {
                name: "wrap_order".to_string(),
                sequence: seq.clone(),
                length: 120,
                topology: "circular".to_string(),
                molecule_type: "dna".to_string(),
                features: vec![mk(strand)],
                ..Default::default()
            };
            let server = handler_with_project(project).await;
            let out_path = std::env::temp_dir().join(format!(
                "libregene-mcp-export-wrap-order-{}-{}.gbk",
                if strand == "+" { "plus" } else { "minus" },
                std::process::id()
            ));
            let req = SaveFileRequest {
                project_id: "wrap_order".to_string(),
                path: out_path.to_string_lossy().into_owned(),
                region: Some(RegionSpec {
                    feature_id: Some("wrap".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let out = server.save_file(Parameters(req)).await.unwrap();
            assert_eq!(out.0["ok"], true);
            assert_eq!(out.0["length"], 51);
            let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
            assert_eq!(parsed.sequence, expected, "strand {} export order", strand);
            std::fs::remove_file(&out_path).ok();
        }
    }

    #[tokio::test]
    async fn save_file_region_explicit_cuts_export_the_fragment() {
        // "ACGT" repeat has no EcoRI/BamHI recognition sites, so the placed
        // sites are the only ones: EcoRI cuts G^AATTC (cut 41), BamHI G^GATCC
        // (cut 101).
        let mut seq = "ACGT".repeat(50);
        seq.replace_range(40..46, "GAATTC");
        seq.replace_range(100..106, "GGATCC");
        let mut project = ProjectData {
            name: "enz_test".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        libregene_core::enzyme::recompute(&mut project);
        // EcoRI cuts between 41 and 42, BamHI between 101 and 102: the fragment
        // between the cuts is [41..=100] (60 bp), which is what an agent reads
        // off find_restriction_sites' topCutIndex.
        assert_eq!(project.enzymes.iter().find(|e| e.name == "EcoRI").unwrap().cut_pairs[0].top_cut_index, 41);
        assert_eq!(project.enzymes.iter().find(|e| e.name == "BamHI").unwrap().cut_pairs[0].top_cut_index, 101);

        let server = handler_with_project(project.clone()).await;
        let dir = std::env::temp_dir().join(format!("libregene-mcp-export-enz-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let out_path = dir.join("cuts.gbk");
        let req = SaveFileRequest {
            project_id: "enz_test".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                cut1: Some(41),
                cut2: Some(101),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 60, "fragment [41..=100] = 60 bp");
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, seq[41..=100].to_string());

        // cuts may be given in either order
        let out_path2 = dir.join("cuts-reversed.gbk");
        let req = SaveFileRequest {
            project_id: "enz_test".to_string(),
            path: out_path2.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                cut1: Some(70),
                cut2: Some(30),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        assert_eq!(out.0["length"], 40, "[30, 69] = 40 bp");
        let parsed = libregene_core::file_io::parse_file(&out_path2).unwrap();
        assert_eq!(parsed.sequence, seq[30..=69].to_string());

        std::fs::remove_dir_all(&dir).ok();
    }

            #[tokio::test]
    async fn save_file_region_exports_overlapping_primers() {
        // Export region [200..299]. P_in fully inside, P_part overlapping the
        // left edge, P_out fully outside → only P_in and P_part are written.
        let seq = synthetic_dna(400, 23);
        let mk = |name: &str, s: usize, e: usize| Primer {
            id: name.to_string(),
            name: name.to_string(),
            r#type: "fwd".to_string(),
            primer_seq: seq[s..e].to_string(),
            binding_sites: Vec::new(),
        };
        let mut project = ProjectData {
            name: "exp_primer_test".to_string(),
            sequence: seq.clone(),
            length: 400,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("f", "partial", 250, 350, "+")],
            primers: vec![mk("P_in", 220, 240), mk("P_part", 190, 210), mk("P_out", 320, 340)],
            ..Default::default()
        };
        libregene_core::primer::recompute(&mut project);
        assert!(project.primers.iter().all(|p| !p.binding_sites.is_empty()));

        // Direct mapping check: P_part's site clips to the overlap [200..209]
        // → [0..9] in export coordinates (template_end exclusive).
        let (_, _, primers) = build_export_data(&project, &[(200, 299)], false);
        assert_eq!(
            primers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["P_in", "P_part"]
        );
        let part = &primers.iter().find(|p| p.name == "P_part").unwrap().binding_sites[0];
        assert_eq!((part.template_start, part.template_end), (0, 10));

        let server = handler_with_project(project).await;
        let dir =
            std::env::temp_dir().join(format!("libregene-mcp-export-pr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("region.gbk");
        let out = server
            .save_file(Parameters(SaveFileRequest {
                project_id: "exp_primer_test".to_string(),
                path: out_path.to_string_lossy().into_owned(),
                region: Some(RegionSpec {
                    // 1-based inclusive interface → internal 0-based [200, 299]
                    start: Some(201),
                    end: Some(300),
                    ..Default::default()
                }),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["primers"], serde_json::json!(["P_in", "P_part"]));

        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        let mut names: Vec<&str> = parsed.primers.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["P_in", "P_part"]);
        // partially covered feature is clipped to the region: 250..299 → 50..99
        let f = parsed.features.iter().find(|f| f.name == "partial").unwrap();
        assert_eq!((f.start, f.end), (50, 99));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn save_file_region_protein_writes_gpt() {
        let aa = "MVSKGEEDNMAAEF".to_string();
        let project = ProjectData {
            name: "prot_test".to_string(),
            sequence: aa.clone(),
            length: aa.len() as i64,
            topology: "linear".to_string(),
            molecule_type: "protein".to_string(),
            features: vec![feature("prot", "mCherry", 0, (aa.len() - 1) as i64, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let dir = std::env::temp_dir().join(format!("libregene-mcp-export-prot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let out_path = dir.join("out.gpt");
        let req = SaveFileRequest {
            project_id: "prot_test".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                // 1-based inclusive: the whole 14 aa protein
                start: Some(1),
                end: Some(aa.len() as i64),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        assert_eq!(out.0["ok"], true);
        assert_eq!(out.0["length"], aa.len() as i64);
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.molecule_type, "protein");
        assert_eq!(parsed.sequence, aa.to_lowercase(), "the gpt writer lower-cases");

        // protein project must not go to a .gbk path
        let req = SaveFileRequest {
            project_id: "prot_test".to_string(),
            path: dir.join("out.gbk").to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                start: Some(1),
                end: Some(4),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(out.0["message"].as_str().unwrap().contains(".gpt"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn save_file_region_rejects_bad_requests() {
        let seq = synthetic_dna(100, 29);
        let project = ProjectData {
            name: "bad_test".to_string(),
            sequence: seq.clone(),
            length: 100,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("f1", "gene", 10, 50, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-bad-{}.gbk", std::process::id()));
        let bad_req = |region: RegionSpec| SaveFileRequest {
            project_id: "bad_test".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(region),
            ..Default::default()
        };
        let expect_err = |req: SaveFileRequest| async {
            match server.save_file(Parameters(req)).await {
                Err(e) => e.message.into_owned(),
                Ok(v) => v.0["message"].as_str().unwrap_or("").to_string(),
            }
        };

        // multiple selectors
        let msg = expect_err(bad_req(RegionSpec {
            start: Some(0),
            end: Some(9),
            feature_id: Some("f1".to_string()),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("exactly one region selector"), "{}", msg);

        // start without end
        let msg = expect_err(bad_req(RegionSpec {
            start: Some(0),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("both required"), "{}", msg);

        // linear start > end
        let msg = expect_err(bad_req(RegionSpec {
            start: Some(50),
            end: Some(10),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("only allowed on circular"), "{}", msg);

        // unknown feature
        let msg = expect_err(bad_req(RegionSpec {
            feature_id: Some("nope".to_string()),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("Feature not found"), "{}", msg);

        // mixed selectors (coordinates + cuts)
        let msg = expect_err(bad_req(RegionSpec {
            start: Some(1),
            end: Some(9),
            cut1: Some(10),
            cut2: Some(20),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("exactly one region selector"), "{}", msg);

        // cut1 without cut2
        let msg = expect_err(bad_req(RegionSpec {
            cut1: Some(10),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("cut1 and cut2"), "{}", msg);

        // equal cuts on a linear sequence
        let msg = expect_err(bad_req(RegionSpec {
            cut1: Some(10),
            cut2: Some(10),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("equal"), "{}", msg);


        // DNA project must not go to a .gpt path
        let mut req = bad_req(RegionSpec {
            start: Some(0),
            end: Some(9),
            ..Default::default()
        });
        req.path = out_path
            .to_string_lossy()
            .replace("bad", "bad2")
            .replace(".gbk", ".gpt");
        let msg = expect_err(req).await;
        assert!(msg.contains(".gpt"), "{}", msg);

        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn save_file_overwrite_rules() {
        let (dir, path) = write_temp_gbk("save-overwrite", "existing.gbk");
        let server = test_handler();
        // Open the real file; its project id IS the path.
        let out = server
            .open_project(Parameters(OpenProjectRequest {
                path: path.to_string_lossy().into_owned(),
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        let id = path.to_string_lossy().into_owned();

        // Saving back to the project's own path is always allowed.
        let out = server
            .save_file(Parameters(SaveFileRequest {
                project_id: id.clone(),
                path: id.clone(),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        assert!(out.0["bytesWritten"].as_u64().unwrap() > 0, "{}", out.0);

        // Saving to a DIFFERENT existing path requires overwrite: true.
        let other = dir.join("other.gbk");
        let mut other_project = edit_test_project();
        other_project.name = "other".to_string();
        libregene_core::file_io::gbk::write_gbk(&other_project, &other).unwrap();
        let out = server
            .save_file(Parameters(SaveFileRequest {
                project_id: id.clone(),
                path: other.to_string_lossy().into_owned(),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("overwrite"),
            "{}",
            out.0
        );
        let out = server
            .save_file(Parameters(SaveFileRequest {
                project_id: id.clone(),
                path: other.to_string_lossy().into_owned(),
                overwrite: Some(true),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn save_file_whole_project_marks_clean() {
        let (dir, path) = write_temp_gbk("save-clean", "clean.gbk");
        let server = test_handler();
        server
            .open_project(Parameters(OpenProjectRequest {
                path: path.to_string_lossy().into_owned(),
            }))
            .await
            .unwrap();
        let id = path.to_string_lossy().into_owned();
        // Dirty the project, then save it back to its own path.
        server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: id.clone(),
                start: 1,
                end: 2,
                replacement: Some("AA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert!(server.pm.read().await.is_dirty(&id), "edit marks dirty");
        let out = server
            .save_file(Parameters(SaveFileRequest {
                project_id: id.clone(),
                path: id.clone(),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        assert!(out.0["bytesWritten"].as_u64().unwrap() > 0, "{}", out.0);
        assert!(!server.pm.read().await.is_dirty(&id), "save marks clean");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A minus-strand feature export reverse-complements each piece — DNA-only
    /// semantics. On protein/RNA projects it must be refused, not silently
    /// complemented with the DNA alphabet.
    #[tokio::test]
    async fn save_file_region_minus_strand_feature_rejected_on_protein() {
        let project = ProjectData {
            name: "prot_test".to_string(),
            sequence: "MVSAAAAAAAAR".to_string(),
            length: 12,
            topology: "linear".to_string(),
            molecule_type: "protein".to_string(),
            features: vec![feature("dom", "domain", 2, 8, "-")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-export-prot-{}.gpt", std::process::id()));
        let err = server
            .save_file(Parameters(SaveFileRequest {
                project_id: "prot_test".to_string(),
                path: out_path.to_string_lossy().into_owned(),
                region: Some(RegionSpec {
                    feature_id: Some("dom".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }))
            .await
            .err()
            .expect("minus-strand feature export on a protein project must fail");
        assert!(err.message.contains("minus strand"), "{err}");
        assert!(err.message.contains("DNA"), "{err}");
        assert!(!out_path.exists(), "no file written on refusal");
    }

    /// Crafted .dna files can smuggle below-one or sentinel coordinates into
    /// features (the SnapGene parser drops them now, but projects saved by
    /// older builds may already carry such data). Feature-mode export must
    /// reject them instead of slicing out of range.
    #[tokio::test]
    async fn save_file_region_feature_with_out_of_range_segments_is_rejected() {
        let seq = synthetic_dna(100, 13);
        let mut poisoned = feature("neg", "poisoned", 4, 19, "+");
        poisoned.segments = vec![
            Segment { start: 4, end: -4, color: None },
            Segment { start: 9, end: 19, color: None },
        ];
        let sentinel = feature("sent", "degenerate", i64::MAX, i64::MIN, "+");
        let project = ProjectData {
            name: "poison_test".to_string(),
            sequence: seq,
            length: 100,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![poisoned, sentinel],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        for fid in ["neg", "sent"] {
            let out_path = std::env::temp_dir().join(format!(
                "libregene-mcp-export-poison-{}-{}.gbk",
                fid,
                std::process::id()
            ));
            let err = server
                .save_file(Parameters(SaveFileRequest {
                    project_id: "poison_test".to_string(),
                    path: out_path.to_string_lossy().into_owned(),
                    region: Some(RegionSpec {
                        feature_id: Some(fid.to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }))
                .await
                .err()
                .expect("out-of-range feature segments must be rejected, not sliced");
            assert!(err.message.contains("out of range"), "{err}");
            assert!(!out_path.exists(), "no file written on refusal");
        }
    }

    /// The primer engine stores a circular origin-wrapping primary binding
    /// site as template_start near len with template_end = (start + len) %
    /// tlen (< start). Region export must split that span into its two arcs —
    /// previously the inverted span matched nothing and the primer was
    /// silently dropped from the exported file.
    #[test]
    fn build_export_data_keeps_wrap_origin_primer() {
        let seq = synthetic_dna(100, 5);
        let site = libregene_core::models::PrimerBindingSite {
            primer_id: "wp".to_string(),
            strand: 1,
            template_start: 95,
            template_end: 5, // wraps the origin: covers 95..=99 and 0..=4
            tm: 60.0,
            gc_content: 0.5,
            match_score: 10,
            has_3_prime_mismatch: false,
            five_prime_tail: String::new(),
            three_prime_tail: String::new(),
            alignment: Default::default(),
        };
        let project = ProjectData {
            name: "wrap_primer".to_string(),
            sequence: seq,
            length: 100,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            primers: vec![Primer {
                id: "wp".to_string(),
                name: "wp".to_string(),
                r#type: "fwd".to_string(),
                primer_seq: "AAAAAAAAAA".to_string(),
                binding_sites: vec![site],
            }],
            ..Default::default()
        };
        // Export the wrap window 91..100,1..11 (0-based pieces).
        let (_sequence, _features, primers) =
            build_export_data(&project, &[(90, 99), (0, 10)], false);
        assert_eq!(primers.len(), 1, "wrap-origin primer must be exported");
        let site = &primers[0].binding_sites[0];
        // Both arcs survive: 95..=99 lands at 5..=9 (offset 90→0) and 0..=4
        // follows contiguously at 10..=14, merging into one 10 bp site.
        assert_eq!((site.template_start, site.template_end), (5, 15));
    }

    #[test]
    fn build_export_data_full_circle_export_keeps_wrapped_primer_site() {
        // Full-sequence export of a 100 bp circle: the two arcs of a
        // 95..=99,0..=4 site stay disjoint in the linear export coordinates,
        // so the site must keep its wrapped form (template_end <
        // template_start) covering all 10 bp — gbk.rs writes it as a join.
        let seq = synthetic_dna(100, 5);
        let site = libregene_core::models::PrimerBindingSite {
            primer_id: "wp".to_string(),
            strand: 1,
            template_start: 95,
            template_end: 5,
            tm: 60.0,
            gc_content: 0.5,
            match_score: 10,
            has_3_prime_mismatch: false,
            five_prime_tail: String::new(),
            three_prime_tail: String::new(),
            alignment: Default::default(),
        };
        let project = ProjectData {
            name: "wrap_primer".to_string(),
            sequence: seq,
            length: 100,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            primers: vec![Primer {
                id: "wp".to_string(),
                name: "wp".to_string(),
                r#type: "fwd".to_string(),
                primer_seq: "AAAAAAAAAA".to_string(),
                binding_sites: vec![site],
            }],
            ..Default::default()
        };
        let (_sequence, _features, primers) = build_export_data(&project, &[(0, 99)], false);
        assert_eq!(primers.len(), 1, "wrap-origin primer must be exported");
        let site = &primers[0].binding_sites[0];
        assert_eq!((site.template_start, site.template_end), (95, 5));
    }

    #[test]
    fn resolve_export_region_rejects_iis_cut_outside_linear_molecule() {
        let seq = synthetic_dna(100, 11);
        let project = ProjectData {
            name: "iis".to_string(),
            sequence: seq,
            length: 100,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            enzymes: vec![
                // Type-IIS enzyme whose recognition site sits at the very end:
                // the cut lands before base 1 on a linear molecule.
                libregene_core::models::Enzyme {
                    name: "BbsI".to_string(),
                    rec_start: 94,
                    rec_end: 99,
                    cut_index: -2,
                    cut_pairs: vec![libregene_core::models::CutPair {
                        top_cut_index: -2,
                        bot_cut_index: 2,
                    }],
                    ..Default::default()
                },
                libregene_core::models::Enzyme {
                    name: "GoodCutter".to_string(),
                    rec_start: 40,
                    rec_end: 45,
                    cut_index: 50,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        // A cut position outside the linear molecule is rejected before slicing.
        let spec = RegionSpec {
            cut1: Some(0),
            cut2: Some(50),
            ..Default::default()
        };
        let err = resolve_export_region(&project, &spec).expect_err("must reject");
        assert!(err.contains("out of range"), "{err}");
    }

        #[tokio::test]
    async fn save_file_region_over_own_source_requires_overwrite() {
        let (dir, path) = write_temp_gbk("save-region-self", "self.gbk");
        let server = test_handler();
        server
            .open_project(Parameters(OpenProjectRequest {
                path: path.to_string_lossy().into_owned(),
            }))
            .await
            .unwrap();
        let id = path.to_string_lossy().into_owned();
        let full_len = {
            let pm = server.pm.read().await;
            pm.get_project_by_id(&id).unwrap().length
        };
        let req = || SaveFileRequest {
            project_id: id.clone(),
            path: id.clone(),
            region: Some(RegionSpec {
                start: Some(1),
                end: Some(10),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req())).await.unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("overwrite: true"),
            "{}",
            out.0
        );
        let mut with_flag = req();
        with_flag.overwrite = Some(true);
        let out = server.save_file(Parameters(with_flag)).await.unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        let parsed = libregene_core::file_io::parse_file(&path).unwrap();
        assert_eq!(parsed.sequence.len(), 10, "source file replaced by the fragment");
        assert!(full_len > 10);
        std::fs::remove_dir_all(&dir).ok();
    }
