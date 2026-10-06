use super::common::*;
use crate::mcp::*;
use libregene_core::models::ProjectData;
use libregene_core::models::Feature;

/// A single failing item no longer aborts the call: the response is an
/// `ok: false` envelope whose first result slot carries the message.
fn item_failure(v: &serde_json::Value) -> String {
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["results"][0]["ok"], false, "{v}");
    v["results"][0]["message"].as_str().unwrap_or_default().to_string()
}

    #[test]
    fn clean_coding_sequence_strips_junk_and_validates() {
        assert_eq!(
            clean_coding_sequence("atg gtg agc\n1 2 3\ntaa").unwrap(),
            "ATGGTGAGCTAA"
        );
        assert_eq!(clean_coding_sequence("ATG").unwrap(), "ATG");
        assert_eq!(clean_coding_sequence("1atg2").unwrap(), "ATG");
        assert!(clean_coding_sequence("ATGN").is_err()); // ambiguous base
        assert!(clean_coding_sequence("ATGGT").is_err()); // length not %3
        assert!(clean_coding_sequence("").is_err()); // empty
        assert!(clean_coding_sequence("   \n\t ").is_err()); // only junk
    }

    #[test]
    fn resolve_optimize_input_requires_exactly_one_mode() {
        // project mode: no sequence/input_path → project_id + feature_id required
        assert!(matches!(
            resolve_optimize_input(Some("p1"), Some("f1"), None, None),
            Ok(OptimizeInput::Project { project_id, feature_id }) if project_id == "p1" && feature_id == "f1"
        ));
        assert!(resolve_optimize_input(None, Some("f1"), None, None).is_err());
        assert!(resolve_optimize_input(Some("p1"), None, None, None).is_err());
        assert!(resolve_optimize_input(None, None, None, None).is_err());
        // sequence mode
        assert!(matches!(
            resolve_optimize_input(None, None, Some("ATG"), None),
            Ok(OptimizeInput::Sequence(_))
        ));
        // file mode with optional feature_id
        assert!(matches!(
            resolve_optimize_input(None, Some("f1"), None, Some("x.gbk")),
            Ok(OptimizeInput::File { feature_id: Some(_), .. })
        ));
        assert!(matches!(
            resolve_optimize_input(None, None, None, Some("x.gbk")),
            Ok(OptimizeInput::File { feature_id: None, .. })
        ));
        // conflicts must error
        assert!(resolve_optimize_input(None, None, Some("ATG"), Some("x.gbk")).is_err());
        assert!(resolve_optimize_input(Some("p1"), None, Some("ATG"), None).is_err());
        assert!(resolve_optimize_input(Some("p1"), None, None, Some("x.gbk")).is_err());
        assert!(resolve_optimize_input(None, Some("f1"), Some("ATG"), None).is_err());
    }

    #[test]
    fn write_convert_output_rejects_unknown_extension() {
        assert!(write_convert_output("out.ab1", "ATG", "dna", None, None).is_err());
        assert!(write_convert_output("../esc.gbk", "ATG", "dna", None, None).is_err());
        // .gpt is protein-only; a DNA output cannot be written as .gpt.
        assert!(write_convert_output("out.gpt", "ATG", "dna", None, None).is_err());
    }

    #[test]
    fn tool_input_schemas_avoid_nonstandard_int_formats() {
        // schemars maps usize/isize to format "uint"/"int", which strict MCP
        // clients (e.g. kimi-code) reject as unknown JSON Schema formats;
        // unsigned fields must use #[schemars(with = "Option<i64>")] instead.
        for tool in LibreGeneMcp::<tauri::Wry>::tool_router().list_all() {
            let schema = serde_json::to_string(&tool.input_schema).unwrap();
            assert!(
                !schema.contains("\"format\":\"uint\"") && !schema.contains("\"format\":\"int\""),
                "tool {} emits a non-standard integer format: {}",
                tool.name,
                schema
            );
        }
    }

    #[test]
    fn write_convert_output_roundtrips_gbk_gpt_rna_and_text() {
        let dir = std::env::temp_dir().join(format!("libregene-codon-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let gbk_path = dir.join("out.gbk");
        let written =
            write_convert_output(gbk_path.to_str().unwrap(), "ATGGTGAGCTAA", "dna", None, Some("CAR"))
                .unwrap();
        assert_eq!(written, gbk_path.to_str().unwrap());
        let parsed = libregene_core::file_io::parse_file(&gbk_path).unwrap();
        assert_eq!(parsed.sequence, "ATGGTGAGCTAA");
        assert_eq!(parsed.molecule_type, "dna");
        // The whole-length CDS is labeled after the source name when given.
        assert!(parsed.features.iter().any(|f| f.ftype == "CDS" && f.name == "CAR"));

        let gpt_path = dir.join("out.gpt");
        write_convert_output(gpt_path.to_str().unwrap(), "MVS*", "protein", None, None).unwrap();
        let parsed = libregene_core::file_io::parse_file(&gpt_path).unwrap();
        assert_eq!(parsed.molecule_type, "protein");
        assert_eq!(parsed.sequence, "mvs*"); // the gpt writer lower-cases

        let rna_path = dir.join("out_rna.gbk");
        write_convert_output(rna_path.to_str().unwrap(), "AUGGUGAGCUAA", "rna", None, None).unwrap();
        let parsed = libregene_core::file_io::parse_file(&rna_path).unwrap();
        assert_eq!(parsed.molecule_type, "rna");
        assert_eq!(parsed.sequence.to_ascii_uppercase(), "AUGGUGAGCUAA");

        let txt_path = dir.join("out.txt");
        write_convert_output(txt_path.to_str().unwrap(), "ATGGTGAGCTAA", "dna", None, None).unwrap();
        assert_eq!(std::fs::read_to_string(&txt_path).unwrap(), "ATGGTGAGCTAA\n");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn codon_preview_json_reports_1based_repairs_and_unresolved() {
        let result = libregene_core::codon::OptimizeResult {
            new_codons: vec!["GAA".to_string()],
            cai_before: 0.5,
            cai_after: 0.9,
            gc_before: 40.0,
            gc_after: 50.0,
            repairs: vec![
                libregene_core::codon::Repair {
                    codon_index: 5,
                    old: "GAG".to_string(),
                    new: "GAA".to_string(),
                    reason: "homopolymer".to_string(),
                },
                libregene_core::codon::Repair {
                    codon_index: 0,
                    old: "TTT".to_string(),
                    new: "TTC".to_string(),
                    reason: "repeat".to_string(),
                },
            ],
            unresolved: vec![
                "repeat 10..17".to_string(),
                "gc_window 0..29".to_string(),
                "unparseable entry".to_string(),
            ],
        };
        let v = codon_preview_json(&result, "E*", 2, "best", "e_coli");
        assert_eq!(v["repairs"][0]["codonIndex"], 6, "{v}");
        assert_eq!(v["repairs"][0]["old"], "GAG", "{v}");
        assert_eq!(v["repairs"][1]["codonIndex"], 1, "{v}");
        assert_eq!(v["repairCount"], 2, "{v}");
        assert_eq!(v["unresolved"][0], "repeat 11..18", "{v}");
        assert_eq!(v["unresolved"][1], "gc_window 1..30", "{v}");
        // Entries that don't match the "<reason> <s>..<e>" shape pass through.
        assert_eq!(v["unresolved"][2], "unparseable entry", "{v}");
    }

    #[tokio::test]
    async fn convert_sequence_sequence_preview_and_validation() {
        let server = test_handler();
        // happy path: sequence input → converted sequence, no projectId
        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            sequence: Some("GAG GAG GAG\nTAA".to_string()),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        assert_eq!(out.0["ok"], true);
        let v = &out.0["results"][0];
        assert_eq!(v["ok"], true);
        assert_eq!(v["from"], "dna");
        assert_eq!(v["to"], "dna");
        assert_eq!(v["aa"], "EEE*");
        assert_eq!(v["codonCount"], 4);
        assert_eq!(v["sequence"], "GAAGAAGAATAA"); // E→GAA, stop→TAA (e_coli best)
        assert!(v.get("projectId").is_none());

        // apply=true without output_path in sequence mode → clear error
        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            sequence: Some("ATGGTGAGCTAA".to_string()),
            apply: Some(true),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        let msg = item_failure(&out.0);
        assert!(msg.contains("apply=true"), "{msg}");
        assert!(msg.contains("outputPath"), "{msg}");

        // sequence + input_path conflict
        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            sequence: Some("ATG".to_string()),
            input_path: Some("x.gbk".to_string()),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        assert!(
            item_failure(&out.0).contains("exactly one input"),
            "{}",
            out.0
        );

        // project mode without feature_id → clear error
        let req = convert_req(vec![ConvertItem {
            project_id: Some("p1".to_string()),
            species: Some("e_coli".to_string()),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        assert!(item_failure(&out.0).contains("featureId"), "{}", out.0);

        // no input at all → clear error
        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        assert!(item_failure(&out.0).contains("projectId"), "{}", out.0);

        // an explicit empty batch → clear error
        let err = match server
            .convert_sequence(Parameters(ConvertSequenceRequest {
                items: Some(vec![]),
                ..Default::default()
            }))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("expected empty-items error"),
        };
        assert!(err.message.contains("empty"), "{}", err.message);

        // no items and no single-item fields → clear error
        assert!(server
            .convert_sequence(Parameters(ConvertSequenceRequest::default()))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn convert_sequence_single_item_top_level_compat() {
        let server = test_handler();
        // Without `items`, the top-level fields act as a single item.
        let req = ConvertSequenceRequest {
            single: ConvertItem {
                sequence: Some("ATGTAAC".to_string()),
                from: Some("dna".to_string()),
                to: Some("rna".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        let v = &out.0["results"][0];
        assert_eq!(v["ok"], true);
        assert_eq!(v["sequence"], "AUGUAAC");
        assert_eq!(v["length"], 7);
    }

    #[tokio::test]
    async fn convert_sequence_file_reverse_translates_protein_gpt() {
        let gpt = include_str!("../../../../backend/test_data/mCherry.gpt");
        let path = std::env::temp_dir().join(format!("libregene-mcp-revtest-{}.gpt", std::process::id()));
        std::fs::write(&path, gpt).unwrap();
        let server = test_handler();
        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            input_path: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        let v = &out.0["results"][0];
        assert_eq!(v["ok"], true);
        assert_eq!(v["from"], "protein");
        assert_eq!(v["to"], "dna");
        let aa = v["aa"].as_str().unwrap();
        assert!(aa.starts_with("MVSKGEEDNM"), "aa: {}", aa);
        assert!(aa.ends_with('*'), "aa: {}", aa);
        let dna = v["sequence"].as_str().unwrap();
        assert_eq!(dna.len(), aa.chars().count() * 3);
        assert!(dna.bytes().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T')));
        assert!(v["message"].as_str().unwrap().contains("Reverse translation"));
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn convert_sequence_sequence_writes_output_file() {
        let out_path = std::env::temp_dir().join(format!("libregene-mcp-outtest-{}.gbk", std::process::id()));
        let server = test_handler();
        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            sequence: Some("ATGGTGAGCTAA".to_string()),
            output_path: Some(out_path.to_string_lossy().into_owned()),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        let v = &out.0["results"][0];
        assert_eq!(v["path"], out_path.to_str().unwrap());
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, "ATGGTGAGCTAA");
        assert!(parsed.features.iter().any(|f| f.ftype == "CDS"));
        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn convert_sequence_file_with_feature_writes_optimized_gbk() {
        let dir = std::env::temp_dir().join(format!("libregene-mcp-feattest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.gbk");
        // GAG GAG GAG TAA = EEE*; E's best codon is GAA, so the optimized
        // whole-file sequence (CDS replaced in place) must be GAAGAAGAATAA.
        let project = ProjectData {
            name: "test".to_string(),
            sequence: "GAGGAGGAGTAA".to_string(),
            length: 12,
            topology: "linear".to_string(),
            features: vec![Feature {
                id: "cds".to_string(),
                name: "cds".to_string(),
                start: 0,
                end: 11,
                color: "#60A5FA".to_string(),
                ftype: "CDS".to_string(),
                segments: Vec::new(),
                strand: "+".to_string(),
                notes: String::new(),
                translation: String::new(),
                qualifiers: Vec::new(),
            }],
            ..Default::default()
        };
        libregene_core::file_io::gbk::write_gbk(&project, &src).unwrap();

        let out_path = dir.join("out.gbk");
        let server = test_handler();
        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            input_path: Some(src.to_string_lossy().into_owned()),
            feature_id: Some("cds_0".to_string()), // id rebuilt as {label}_{start} on parse
            output_path: Some(out_path.to_string_lossy().into_owned()),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        let v = &out.0["results"][0];
        assert_eq!(v["ok"], true);
        assert_eq!(v["aa"], "EEE*");
        assert_eq!(v["sequence"], "GAAGAAGAATAA");
        assert_eq!(v["path"], out_path.to_str().unwrap());
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, "GAAGAAGAATAA");
        assert!(parsed.features.iter().any(|f| f.ftype == "CDS"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn convert_sequence_nucleotide_conversions() {
        let server = test_handler();
        let req = convert_req(vec![
            // dna→rna (no %3 constraint outside codon optimization)
            ConvertItem {
                sequence: Some("ATGTAAC".to_string()),
                from: Some("dna".to_string()),
                to: Some("rna".to_string()),
                ..Default::default()
            },
            // rna→dna
            ConvertItem {
                sequence: Some("AUGUAA".to_string()),
                from: Some("rna".to_string()),
                to: Some("dna".to_string()),
                ..Default::default()
            },
            // dna→dna revComp (no species → no optimization)
            ConvertItem {
                sequence: Some("ATGC".to_string()),
                rev_comp: Some(true),
                ..Default::default()
            },
            // dna→rna with revComp: ATGC → GCAT → GCAU
            ConvertItem {
                sequence: Some("ATGC".to_string()),
                from: Some("dna".to_string()),
                to: Some("rna".to_string()),
                rev_comp: Some(true),
                ..Default::default()
            },
            // rna→dna with revComp: AUGC → ATGC → GCAT
            ConvertItem {
                sequence: Some("AUGC".to_string()),
                from: Some("rna".to_string()),
                to: Some("dna".to_string()),
                rev_comp: Some(true),
                ..Default::default()
            },
        ]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        assert_eq!(out.0["ok"], true);
        let r = &out.0["results"];
        assert_eq!(r[0]["sequence"], "AUGUAAC");
        assert_eq!(r[1]["sequence"], "ATGTAA");
        assert_eq!(r[2]["sequence"], "GCAT");
        assert_eq!(r[3]["sequence"], "GCAU");
        assert_eq!(r[4]["sequence"], "GCAT");
        // plain conversions carry no optimizer fields
        assert!(r[0].get("aa").is_none());
    }

    #[tokio::test]
    async fn convert_sequence_translation_conversions() {
        let server = test_handler();
        let req = convert_req(vec![
            // dna→protein
            ConvertItem {
                sequence: Some("ATGGTGAGCTAA".to_string()),
                from: Some("dna".to_string()),
                to: Some("protein".to_string()),
                ..Default::default()
            },
            // rna→protein
            ConvertItem {
                sequence: Some("AUGGUGAGCUAA".to_string()),
                from: Some("rna".to_string()),
                to: Some("protein".to_string()),
                ..Default::default()
            },
            // protein→dna (reverse translation, e_coli best codons)
            ConvertItem {
                sequence: Some("MVS*".to_string()),
                from: Some("protein".to_string()),
                to: Some("dna".to_string()),
                species: Some("e_coli".to_string()),
                ..Default::default()
            },
            // protein→rna
            ConvertItem {
                sequence: Some("MVS*".to_string()),
                from: Some("protein".to_string()),
                to: Some("rna".to_string()),
                species: Some("e_coli".to_string()),
                ..Default::default()
            },
        ]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        let r = &out.0["results"];
        assert_eq!(r[0]["sequence"], "MVS*");
        assert_eq!(r[0]["length"], 4);
        assert_eq!(r[1]["sequence"], "MVS*");
        assert_eq!(r[2]["sequence"], "ATGGTGAGCTAA");
        assert_eq!(r[2]["aa"], "MVS*");
        assert_eq!(r[2]["codonCount"], 4);
        assert_eq!(r[3]["sequence"], "AUGGUGAGCUAA");
    }

    #[tokio::test]
    async fn convert_sequence_rna_output_writes_fasta_and_gbk() {
        let dir = std::env::temp_dir().join(format!("libregene-mcp-rnaout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fa = dir.join("out.fa");
        let gbk = dir.join("out.gbk");
        let server = test_handler();
        let req = convert_req(vec![
            ConvertItem {
                sequence: Some("ATGTAAC".to_string()),
                from: Some("dna".to_string()),
                to: Some("rna".to_string()),
                output_path: Some(fa.to_string_lossy().into_owned()),
                ..Default::default()
            },
            ConvertItem {
                sequence: Some("ATGTAAC".to_string()),
                from: Some("dna".to_string()),
                to: Some("rna".to_string()),
                output_path: Some(gbk.to_string_lossy().into_owned()),
                ..Default::default()
            },
        ]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        let r = &out.0["results"];
        assert_eq!(r[0]["path"], fa.to_str().unwrap());
        assert_eq!(std::fs::read_to_string(&fa).unwrap(), "AUGUAAC\n");
        let parsed = libregene_core::file_io::parse_file(&gbk).unwrap();
        assert_eq!(parsed.molecule_type, "rna");
        assert_eq!(parsed.sequence.to_ascii_uppercase(), "AUGUAAC");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn convert_sequence_batch_isolates_item_errors() {
        let server = test_handler();
        let req = convert_req(vec![
            ConvertItem {
                sequence: Some("ATGTAA".to_string()),
                from: Some("dna".to_string()),
                to: Some("rna".to_string()),
                ..Default::default()
            },
            // protein→protein is not supported
            ConvertItem {
                sequence: Some("MVS".to_string()),
                from: Some("protein".to_string()),
                to: Some("protein".to_string()),
                ..Default::default()
            },
            // revComp with a protein output is rejected
            ConvertItem {
                sequence: Some("ATGTAA".to_string()),
                from: Some("dna".to_string()),
                to: Some("protein".to_string()),
                rev_comp: Some(true),
                ..Default::default()
            },
            ConvertItem {
                sequence: Some("ATGC".to_string()),
                rev_comp: Some(true),
                ..Default::default()
            },
        ]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        assert_eq!(out.0["ok"], true);
        let r = &out.0["results"];
        assert_eq!(r[0]["ok"], true);
        assert_eq!(r[0]["sequence"], "AUGUAA");
        assert_eq!(r[1]["ok"], false);
        assert!(r[1]["message"].as_str().unwrap().contains("protein"), "{}", r[1]);
        assert_eq!(r[2]["ok"], false);
        assert!(r[2]["message"].as_str().unwrap().contains("revComp"), "{}", r[2]);
        assert_eq!(r[3]["ok"], true);
        assert_eq!(r[3]["sequence"], "GCAT");
    }

    #[tokio::test]
    async fn convert_sequence_all_items_failed_is_error() {
        let server = test_handler();
        let req = convert_req(vec![
            ConvertItem {
                sequence: Some("MVS".to_string()),
                from: Some("protein".to_string()),
                to: Some("protein".to_string()),
                ..Default::default()
            },
            ConvertItem {
                sequence: Some("ATGN".to_string()),
                from: Some("dna".to_string()),
                to: Some("rna".to_string()),
                ..Default::default()
            },
        ]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], false, "{v}");
        assert_eq!(v["resultCount"], 2, "{v}");
        assert_eq!(v["okCount"], 0, "{v}");
        assert_eq!(v["failedCount"], 2, "{v}");
        assert!(
            v["message"].as_str().unwrap().contains("All 2 item(s) failed"),
            "{v}"
        );
        assert_eq!(v["results"].as_array().unwrap().len(), 2, "{v}");
    }

    /// convert_sequence's output_path follows the save_file overwrite rule:
    /// an existing target needs an explicit overwrite flag.
    #[tokio::test]
    async fn convert_sequence_output_path_requires_overwrite() {
        let server = test_handler();
        let out_path = std::env::temp_dir()
            .join(format!("libregene-mcp-opt-overwrite-{}.gbk", std::process::id()));
        std::fs::write(&out_path, "placeholder").unwrap();

        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            sequence: Some("ATGGTGAGCTAA".to_string()),
            output_path: Some(out_path.to_string_lossy().into_owned()),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        assert!(
            item_failure(&out.0).contains("overwrite"),
            "{}",
            out.0
        );

        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            sequence: Some("ATGGTGAGCTAA".to_string()),
            output_path: Some(out_path.to_string_lossy().into_owned()),
            overwrite: Some(true),
            ..Default::default()
        }]);
        let out = server.convert_sequence(Parameters(req)).await.unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        let v = &out.0["results"][0];
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["path"], out_path.to_str().unwrap(), "{}", v);
        std::fs::remove_file(&out_path).ok();
    }

    #[tokio::test]
    async fn convert_sequence_project_mode_rejects_output_path_and_rna() {
        let mut p = dna_test_project();
        p.features = vec![feature("f1", "cds", 10, 60, "+")];
        let server = handler_with_project(p).await;
        let out = server
            .convert_sequence(Parameters(convert_req(vec![ConvertItem {
                project_id: Some("feat".to_string()),
                feature_id: Some("f1".to_string()),
                species: Some("e_coli".to_string()),
                output_path: Some("/tmp/libregene-should-not-write.gbk".to_string()),
                ..Default::default()
            }])))
            .await
            .unwrap();
        assert!(item_failure(&out.0).contains("outputPath"), "{}", out.0);
        assert!(!std::path::Path::new("/tmp/libregene-should-not-write.gbk").exists());

        // RNA projects have no coding DNA to re-encode either.
        let server = handler_with_project(rna_test_project()).await;
        let out = server
            .convert_sequence(Parameters(convert_req(vec![ConvertItem {
                project_id: Some("rna".to_string()),
                feature_id: Some("f1".to_string()),
                species: Some("e_coli".to_string()),
                ..Default::default()
            }])))
            .await
            .unwrap();
        assert!(item_failure(&out.0).contains("rna"), "{}", out.0);
    }
