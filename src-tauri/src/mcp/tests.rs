    use super::*;
    use std::time::Duration;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Raw HTTP POST /mcp initialize; true when the MCP handshake succeeds.
    async fn handshake_ok(port: u16, token: &str) -> bool {
        let addr = format!("127.0.0.1:{}", port);
        let Ok(mut stream) = tokio::net::TcpStream::connect(&addr).await else {
            return false;
        };
        let body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"cfg-test\",\"version\":\"0\"}}}";
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        if stream.write_all(req.as_bytes()).await.is_err() {
            return false;
        }
        let mut buf = vec![0u8; 4096];
        match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => {
                let text = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                text.contains("200 ok") && text.contains("mcp-session-id")
            }
            _ => false,
        }
    }

    /// Raw HTTP POST /mcp returning the full response (status line + body).
    /// `extra_headers` must be pre-formatted header lines each ending with
    /// `\r\n` (e.g. Accept, Mcp-Session-Id); Content-Type,
    /// Content-Length and `Connection: close` are added automatically.
    async fn raw_post(port: u16, token: &str, extra_headers: &str, body: &str) -> String {
        let addr = format!("127.0.0.1:{}", port);
        let mut stream = tokio::net::TcpStream::connect(&addr)
            .await
            .expect("connect");
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).await.expect("write request");
        let mut buf = Vec::new();
        let mut tmp = [0u8; 8192];
        loop {
            match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut tmp)).await {
                Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&tmp[..n]),
                _ => break,
            }
        }
        String::from_utf8_lossy(&buf).to_string()
    }

    async fn wait_up(port: u16, token: &str) -> bool {
        for _ in 0..40 {
            if handshake_ok(port, token).await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    fn test_server() -> McpServer<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        McpServer::new(
            app.handle().clone(),
            Arc::new(RwLock::new(ProjectManager::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            |_, _| {},
        )
    }

    #[tokio::test]
    async fn config_defaults_to_enabled_on_mcp_port() {
        let server = test_server();
        let cfg = server.config();
        assert!(cfg.enabled);
        assert_eq!(cfg.port, MCP_PORT);
    }

    #[tokio::test]
    async fn set_config_rejects_bad_ports() {
        let server = test_server();
        assert!(server.set_config(true, 0).await.is_err());
        // rejected change must not alter the stored config
        assert_eq!(server.config().port, MCP_PORT);
    }

    #[tokio::test]
    async fn server_starts_stops_and_restarts_on_port_change() {
        let server = test_server();
        let token = server.auth_token();

        // start on a fresh port
        server.set_config(true, 19999).await.unwrap();
        assert!(wait_up(19999, &token).await, "server should be up on 19999");

        // disable → port released
        server.set_config(false, 19999).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!handshake_ok(19999, &token).await, "server should be down after disable");

        // re-enable on a new port without an app restart
        server.set_config(true, 20001).await.unwrap();
        assert!(wait_up(20001, &token).await, "server should be up on 20001 after restart");
        assert!(!handshake_ok(19999, &token).await, "old port must stay free");

        // unchanged config → no restart churn
        server.set_config(true, 20001).await.unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(handshake_ok(20001, &token).await, "server must survive a no-op set_config");

        // clean up
        server.set_config(false, 20001).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!handshake_ok(20001, &token).await);
    }

    #[tokio::test]
    async fn bind_failure_marks_server_disabled() {
        // Occupy a port so serve_mcp's bind retries exhaust and give up; the
        // config must then report disabled (get_mcp_config/tray reflect the
        // real state) instead of "running".
        let _occupied = std::net::TcpListener::bind(("127.0.0.1", 21007)).unwrap();
        let server = test_server();
        server.set_config(true, 21007).await.unwrap();
        // 40 retries × 50ms + slack; the give-up is asynchronous.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while server.config().enabled {
            assert!(
                tokio::time::Instant::now() < deadline,
                "config still enabled after bind give-up"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    #[tokio::test]
    async fn regenerated_token_takes_effect_without_restart() {
        let server = test_server();
        server.set_config(true, 20003).await.unwrap();
        let old = server.auth_token();
        assert!(wait_up(20003, &old).await, "server should be up with the initial token");

        let new = server.regenerate_auth_token();
        assert_ne!(old, new);
        // the running server must accept the new token and reject the old one
        assert!(handshake_ok(20003, &new).await, "new token should be accepted");
        assert!(!handshake_ok(20003, &old).await, "old token should be rejected");

        server.set_config(false, 20003).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn missing_accept_header_returns_structured_406() {
        let server = test_server();
        let token = server.auth_token();
        server.set_config(true, 20005).await.unwrap();
        assert!(wait_up(20005, &token).await);

        // No Accept header at all: rmcp would answer a bare 406 with no
        // readable body; the middleware must return a JSON-RPC error body
        // naming the required Accept header and echoing the request id.
        let resp = raw_post(
            20005,
            &token,
            "",
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/list\"}",
        )
        .await;
        assert!(resp.contains("406"), "expected 406, got: {resp}");
        assert!(resp.contains("application/json"), "{resp}");
        assert!(resp.contains("text/event-stream"), "{resp}");
        assert!(resp.contains("-32600"), "{resp}");
        assert!(resp.contains("\"id\":7"), "{resp}");

        server.set_config(false, 20005).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn unknown_session_returns_structured_404() {
        let server = test_server();
        let token = server.auth_token();
        server.set_config(true, 20006).await.unwrap();
        assert!(wait_up(20006, &token).await);

        // A request for a session that never existed: rmcp answers 404 with
        // plain text; the middleware must rewrite it into a JSON-RPC error
        // with code -32001 so clients know the session is gone and must
        // re-initialize.
        let resp = raw_post(
            20006,
            &token,
            "Accept: application/json, text/event-stream\r\nMcp-Session-Id: does-not-exist\r\n",
            "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"tools/list\"}",
        )
        .await;
        assert!(resp.contains("404"), "expected 404, got: {resp}");
        assert!(resp.contains("-32001"), "{resp}");
        assert!(resp.contains("jsonrpc"), "{resp}");
        assert!(resp.contains("\"id\":9"), "{resp}");

        server.set_config(false, 20006).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // ------------------------------------------------------------------
    // convert_sequence: input resolution / sequence cleaning / file output
    // ------------------------------------------------------------------

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

    fn test_handler() -> LibreGeneMcp<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        LibreGeneMcp::new(
            app.handle().clone(),
            Arc::new(RwLock::new(ProjectManager::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
        )
    }

    fn convert_req(items: Vec<ConvertItem>) -> ConvertSequenceRequest {
        ConvertSequenceRequest { items: Some(items), ..Default::default() }
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
        let err = match server.convert_sequence(Parameters(req)).await {
            Err(e) => e,
            Ok(_) => panic!("expected apply validation error"),
        };
        assert!(err.message.contains("apply=true"), "{}", err.message);
        assert!(err.message.contains("output_path"), "{}", err.message);

        // sequence + input_path conflict
        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            sequence: Some("ATG".to_string()),
            input_path: Some("x.gbk".to_string()),
            ..Default::default()
        }]);
        assert!(server.convert_sequence(Parameters(req)).await.is_err());

        // project mode without feature_id → clear error
        let req = convert_req(vec![ConvertItem {
            project_id: Some("p1".to_string()),
            species: Some("e_coli".to_string()),
            ..Default::default()
        }]);
        let err = match server.convert_sequence(Parameters(req)).await {
            Err(e) => e,
            Ok(_) => panic!("expected missing-feature_id error"),
        };
        assert!(err.message.contains("feature_id"), "{}", err.message);

        // no input at all → clear error
        let req = convert_req(vec![ConvertItem {
            species: Some("e_coli".to_string()),
            ..Default::default()
        }]);
        let err = match server.convert_sequence(Parameters(req)).await {
            Err(e) => e,
            Ok(_) => panic!("expected missing-project_id error"),
        };
        assert!(err.message.contains("project_id"), "{}", err.message);

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
        let gpt = include_str!("../../../backend/test_data/mCherry.gpt");
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
        assert!(r[1]["error"].as_str().unwrap().contains("protein"), "{}", r[1]);
        assert_eq!(r[2]["ok"], false);
        assert!(r[2]["error"].as_str().unwrap().contains("revComp"), "{}", r[2]);
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
        let err = match server.convert_sequence(Parameters(req)).await {
            Err(e) => e,
            Ok(v) => panic!("expected all-failed error, got {}", v.0),
        };
        assert!(err.message.contains("all 2 item(s) failed"), "{}", err.message);
    }

    // ------------------------------------------------------------------
    // save_file region mode (subsequence export)
    // ------------------------------------------------------------------

    /// Deterministic pseudo-random ACGT sequence (unique long substrings).
    fn synthetic_dna(length: usize, mut seed: u64) -> String {
        let mut out = String::with_capacity(length);
        for _ in 0..length {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            out.push(b"ACGT"[(seed >> 33) as usize & 3] as char);
        }
        out
    }

    fn feature(id: &str, name: &str, start: i64, end: i64, strand: &str) -> Feature {
        Feature {
            id: id.to_string(),
            name: name.to_string(),
            start,
            end,
            color: "#60A5FA".to_string(),
            ftype: "CDS".to_string(),
            segments: Vec::new(),
            strand: strand.to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        }
    }

    /// Handler whose project manager holds one project (id = name, active).
    /// The project is bound as an MCP agent tab so mutating tools pass the
    /// agent-tab gate.
    async fn handler_with_project(project: ProjectData) -> LibreGeneMcp<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let id = project.name.clone();
        pm.write().await.load(&id, project).unwrap();
        let agent_tabs: crate::AgentTabs = Arc::new(RwLock::new(HashMap::new()));
        agent_tabs.write().await.insert(id.clone(), crate::AgentTabMeta { locked: true });
        LibreGeneMcp::new(
            app.handle().clone(),
            pm,
            Arc::new(RwLock::new(HashMap::new())),
            agent_tabs,
        )
    }

    /// Same as handler_with_project but WITHOUT an agent tab — for testing
    /// that mutating tools refuse projects not bound as an agent tab.
    async fn handler_with_unbound_project(project: ProjectData) -> LibreGeneMcp<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let id = project.name.clone();
        pm.write().await.load(&id, project).unwrap();
        LibreGeneMcp::new(
            app.handle().clone(),
            pm,
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
        )
    }

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
        assert_eq!(v["outputPath"], out_path.to_str().unwrap());
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
    /// produced a regionView bbox with start > end before the fix, which
    /// either dropped the regionView digest (linear) or showed a wrong
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
        // The headline assertion: regionView must be present (non-null) and
        // describe a span within the feature's real coordinates [10, 60].
        // Before the fix, bbox was (40, 30) → start > end → regionView dropped.
        let region = v["regionView"].as_str().unwrap_or("");
        assert!(
            !region.is_empty(),
            "regionView must not be empty for a multi-segment minus-strand feature (was dropped by bbox bug)"
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
        let region = v["regionView"].as_str().unwrap_or("");
        assert!(
            region.contains("REGION: 91..21"),
            "regionView should show the 1-based wrap window 91..21, got: {}",
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
    async fn save_file_region_enzyme_fragment_and_explicit_cuts() {
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
        assert_eq!(enzyme_cut_index(&project, "EcoRI", 0).unwrap(), 41);
        assert_eq!(enzyme_cut_index(&project, "BamHI", 0).unwrap(), 101);

        let server = handler_with_project(project.clone()).await;
        let dir = std::env::temp_dir().join(format!("libregene-mcp-export-enz-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let out_path = dir.join("frag.gbk");
        let req = SaveFileRequest {
            project_id: "enz_test".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                enzyme1: Some("EcoRI".to_string()),
                enzyme2: Some("BamHI".to_string()),
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

        // explicit cut indices mode (cuts may be given in either order)
        let out_path2 = dir.join("cuts.gbk");
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
        let v = out.0;
        assert_eq!(v["length"], 40, "[30, 69] = 40 bp");
        let parsed = libregene_core::file_io::parse_file(&out_path2).unwrap();
        assert_eq!(parsed.sequence, seq[30..=69].to_string());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn save_file_region_primer_amplicon() {
        let seq = synthetic_dna(200, 19);
        let project = ProjectData {
            name: "amp_test".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let dir = std::env::temp_dir().join(format!("libregene-mcp-export-amp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // raw primer sequences
        let out_path = dir.join("amp.gbk");
        let req = SaveFileRequest {
            project_id: "amp_test".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                fwd_primer: Some(seq[50..70].to_string()),
                rev_primer: Some(libregene_core::utils::reverse_complement(&seq[100..120])),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 70, "amplicon [50, 119]");
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        assert_eq!(parsed.sequence, seq[50..=119].to_string());

        // same amplicon via stored project primers (name lookup path)
        let fwd = Primer {
            id: "F1".to_string(),
            name: "F1".to_string(),
            r#type: "fwd".to_string(),
            primer_seq: seq[50..70].to_string(),
            binding_sites: Vec::new(),
        };
        let rev = Primer {
            id: "R1".to_string(),
            name: "R1".to_string(),
            r#type: "rev".to_string(),
            primer_seq: libregene_core::utils::reverse_complement(&seq[100..120]),
            binding_sites: Vec::new(),
        };
        let mut project = ProjectData {
            name: "amp_name_test".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            primers: vec![fwd, rev],
            ..Default::default()
        };
        libregene_core::primer::recompute(&mut project);
        let server = handler_with_project(project).await;
        let out_path2 = dir.join("amp-name.gbk");
        let req = SaveFileRequest {
            project_id: "amp_name_test".to_string(),
            path: out_path2.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                fwd_primer: Some("F1".to_string()),
                rev_primer: Some("R1".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        assert_eq!(out.0["length"], 70);
        let parsed = libregene_core::file_io::parse_file(&out_path2).unwrap();
        assert_eq!(parsed.sequence, seq[50..=119].to_string());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn save_file_region_circular_primer_amplicon_wraps_origin() {
        let seq = synthetic_dna(200, 23);
        let project = ProjectData {
            name: "amp_circ".to_string(),
            sequence: seq.clone(),
            length: 200,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let dir = std::env::temp_dir().join(format!("libregene-mcp-export-ampc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("amp.gbk");
        // fwd primer sits at the very end (188..200, its site wraps the origin),
        // rev primer at 30..45: the amplicon wraps 188..199 + 0..44.
        let req = SaveFileRequest {
            project_id: "amp_circ".to_string(),
            path: out_path.to_string_lossy().into_owned(),
            region: Some(RegionSpec {
                fwd_primer: Some(seq[188..200].to_string()),
                rev_primer: Some(libregene_core::utils::reverse_complement(&seq[30..45])),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = server.save_file(Parameters(req)).await.unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true);
        assert_eq!(v["length"], 57, "12 bp (188..199) + 45 bp (0..44)");
        let parsed = libregene_core::file_io::parse_file(&out_path).unwrap();
        let expected = format!("{}{}", &seq[188..], &seq[..=44]);
        assert_eq!(parsed.sequence, expected);
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

        // unknown enzyme
        let msg = expect_err(bad_req(RegionSpec {
            enzyme1: Some("EcoRI".to_string()),
            enzyme2: Some("NotARealEnzyme".to_string()),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("Unknown enzyme"), "{}", msg);

        // mixed fragment selectors
        let msg = expect_err(bad_req(RegionSpec {
            enzyme1: Some("EcoRI".to_string()),
            cut1: Some(10),
            cut2: Some(20),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("not a mix"), "{}", msg);

        // equal cuts on a linear sequence
        let msg = expect_err(bad_req(RegionSpec {
            cut1: Some(10),
            cut2: Some(10),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("equal"), "{}", msg);

        // primer that is neither a name nor a sequence
        let msg = expect_err(bad_req(RegionSpec {
            fwd_primer: Some("!!!".to_string()),
            rev_primer: Some(seq[20..40].to_string()),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("neither a primer name"), "{}", msg);

        // primer that only binds the reverse strand in the fwd role
        let msg = expect_err(bad_req(RegionSpec {
            fwd_primer: Some(libregene_core::utils::reverse_complement(&seq[20..40])),
            rev_primer: Some(libregene_core::utils::reverse_complement(&seq[50..70])),
            ..Default::default()
        }))
        .await;
        assert!(msg.contains("does not bind the forward strand"), "{}", msg);

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

    // ------------------------------------------------------------------
    // edit_sequence: replacement from file
    // ------------------------------------------------------------------

    fn edit_test_project() -> ProjectData {
        let seq = synthetic_dna(200, 42);
        ProjectData {
            name: "edit_test".to_string(),
            sequence: seq,
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("f1", "gene", 50, 100, "+")],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn edit_sequence_replacement_from_fasta_file() {
        let server = handler_with_project(edit_test_project()).await;
        let insert = "AAACCCGGGTTT";
        let fasta = std::env::temp_dir()
            .join(format!("libregene-mcp-edit-ins-{}.fasta", std::process::id()));
        std::fs::write(&fasta, format!(">insert\n{}\n", insert)).unwrap();

        // pure insertion before base 61 (1-based; internal position 60) via
        // replacement_path
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement_path: Some(fasta.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["newLength"], 212);

        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(p.sequence.len(), 212);
        assert_eq!(&p.sequence[60..72], insert);
        // feature 50..100 spans the insertion point → end shifted by 12
        let f = p.features.iter().find(|f| f.name == "gene").unwrap();
        assert_eq!((f.start, f.end), (50, 112));
        drop(pm);
        std::fs::remove_file(&fasta).ok();
    }

    /// Write a 60 bp fragment carrying a feature (10..29, "+"), a feature
    /// named "gene" (0..5, clashes with the target's "gene") and a primer
    /// binding 30..49; returns (dir, path).
    fn write_annotated_insert() -> (std::path::PathBuf, std::path::PathBuf) {
        let src_seq = synthetic_dna(60, 99);
        let mut src = ProjectData {
            name: "ann_src".to_string(),
            sequence: src_seq.clone(),
            length: 60,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![
                feature("f", "ins_feat", 10, 29, "-"),
                feature("f2", "gene", 0, 5, "+"),
            ],
            primers: vec![Primer {
                id: "ins_primer".to_string(),
                name: "ins_primer".to_string(),
                r#type: "fwd".to_string(),
                primer_seq: src_seq[30..50].to_string(),
                binding_sites: Vec::new(),
            }],
            ..Default::default()
        };
        libregene_core::primer::recompute(&mut src);
        let dir =
            std::env::temp_dir().join(format!("libregene-mcp-edit-ann-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gbk = dir.join("insert.gbk");
        libregene_core::file_io::gbk::write_gbk(&src, &gbk).unwrap();
        (dir, gbk)
    }

    #[tokio::test]
    async fn edit_sequence_transfers_annotations_from_gbk() {
        let (dir, gbk) = write_annotated_insert();
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement_path: Some(gbk.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(
            v["transferredFeatures"],
            serde_json::json!(["ins_feat", "gene (2)"])
        );
        assert_eq!(v["transferredPrimers"], serde_json::json!(["ins_primer"]));

        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(p.sequence.len(), 260);
        let f = p.features.iter().find(|f| f.name == "ins_feat").unwrap();
        assert_eq!((f.start, f.end), (70, 89));
        // name clash with the target's "gene" → renamed, rebased to 60..65
        let renamed = p.features.iter().find(|f| f.name == "gene (2)").unwrap();
        assert_eq!((renamed.start, renamed.end), (60, 65));
        let pr = p.primers.iter().find(|x| x.name == "ins_primer").unwrap();
        assert!(!pr.binding_sites.is_empty(), "primer site recomputed");
        let bs = &pr.binding_sites[0];
        assert_eq!((bs.template_start, bs.template_end), (90, 110));
        drop(pm);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn edit_sequence_transfers_annotations_reverse_complemented() {
        let (dir, gbk) = write_annotated_insert();
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement_path: Some(gbk.to_string_lossy().into_owned()),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);

        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        // local [10..29] in a 60 bp insert mirrors to [30..49] → +60 offset,
        // and the "-" strand survives the gbk round trip and flips to "+"
        let f = p.features.iter().find(|f| f.name == "ins_feat").unwrap();
        assert_eq!((f.start, f.end), (90, 109));
        assert_eq!(f.strand, "+");
        // the primer still binds (on the opposite strand) inside the insert
        let pr = p.primers.iter().find(|x| x.name == "ins_primer").unwrap();
        let bs = &pr.binding_sites[0];
        assert_eq!(bs.strand, -1);
        assert!(bs.template_start >= 60 && bs.template_end <= 120);
        drop(pm);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn edit_sequence_replacement_input_validation() {
        let server = handler_with_project(edit_test_project()).await;

        // both replacement and replacement_path → error
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 21,
                replacement: Some("ACGT".to_string()),
                replacement_path: Some("x.fasta".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(out.0["message"].as_str().unwrap().contains("exactly one"), "{}", out.0);

        // neither → error
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 21,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(out.0["message"].as_str().unwrap().contains("exactly one"), "{}", out.0);

        // bad extension → hard error from validate_user_path
        let res = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 21,
                replacement_path: Some("notes.txt".to_string()),
                ..Default::default()
            }))
            .await;
        assert!(res.is_err(), "txt path must be rejected");

        // unreadable/missing file → fail envelope
        let missing = std::env::temp_dir()
            .join(format!("libregene-mcp-edit-missing-{}.fasta", std::process::id()));
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 21,
                replacement_path: Some(missing.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("Failed to read replacement"),
            "{}",
            out.0
        );

        // sequence must be untouched after all these failures
        let pm = server.pm.read().await;
        assert_eq!(pm.get_project_by_id("edit_test").unwrap().sequence.len(), 200);
    }

    #[tokio::test]
    async fn edit_sequence_strand_minus_inserts_reverse_complement() {
        let server = handler_with_project(edit_test_project()).await;

        // pure insertion before base 61 (1-based) with strand "-" → revcomp inserted
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement: Some("AAACCCGGGTTG".to_string()),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert!(
            v["message"].as_str().unwrap().contains("reverse-complemented"),
            "{}",
            v
        );
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(p.sequence.len(), 212);
        assert_eq!(&p.sequence[60..72], "CAACCCGGGTTT");
        // feature 50..100 spans the insertion point → end shifted by 12
        let f = p.features.iter().find(|f| f.name == "gene").unwrap();
        assert_eq!((f.start, f.end), (50, 112));
    }

    #[tokio::test]
    async fn edit_sequence_strand_validation() {
        // invalid strand value → fail envelope
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 60,
                replacement: Some("ACGT".to_string()),
                strand: Some("x".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("Invalid strand"),
            "{}",
            out.0
        );

        // strand "-" rejected on protein projects
        let server = handler_with_project(protein_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "prot".to_string(),
                start: 11,
                end: 11,
                replacement: Some("AA".to_string()),
                strand: Some("-".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("only supported on DNA"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn edit_sequence_string_replacement_still_works() {
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 20,
                replacement: Some("TT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["newLength"], 192);
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(&p.sequence[10..12], "TT");
    }

    #[tokio::test]
    async fn edit_sequence_rechecks_bounds_against_live_sequence() {
        // A stale `length` field simulates a live sequence that shrank after
        // the resolve-time snapshot: the write-lock re-check must reject the
        // edit instead of panicking on the slice.
        let mut project = edit_test_project();
        project.length = project.sequence.len() as i64 + 50;
        let stale_len = project.length;
        let server = handler_with_project(project).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: stale_len - 10,
                end: stale_len,
                replacement: Some("AA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false);
        assert!(
            out.0["message"].as_str().unwrap().contains("out of bounds"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn edit_sequence_uppercases_dna_replacement() {
        // Lowercase replacement bases must be normalized to uppercase (as
        // update_sequence does); otherwise the case-sensitive enzyme recompute
        // loses sites spanning the edit boundary and gbk output mixes case.
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 20,
                replacement: Some("gaattcGGTT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(&p.sequence[10..20], "GAATTCGGTT");
        assert!(
            !p.sequence.chars().any(|c| c.is_ascii_lowercase()),
            "no lowercase bases left in the stored sequence"
        );
        drop(pm);

        // Same normalization applies to the replacement_path input (GenBank
        // files conventionally store lowercase sequence).
        let server = handler_with_project(edit_test_project()).await;
        let gbk = std::env::temp_dir()
            .join(format!("libregene-mcp-edit-lower-{}.gbk", std::process::id()));
        std::fs::write(
            &gbk,
            "LOCUS       ins                       10 bp    DNA     linear   UNA 01-JAN-1980\n\
             FEATURES             Location/Qualifiers\n\
             ORIGIN\n\
             1 gaattcggtt\n\
             //\n",
        )
        .unwrap();
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 11,
                end: 20,
                replacement_path: Some(gbk.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert_eq!(&p.sequence[10..20], "GAATTCGGTT");
        drop(pm);
        std::fs::remove_file(&gbk).ok();
    }

    #[tokio::test]
    async fn edit_sequence_equal_length_replacement_keeps_covered_features() {
        // Equal-length replacement over feature f1 (internal 50..100): the
        // feature stays at its coordinates and removedFeatures is empty.
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 61,
                end: 70,
                replacement: Some("TTTTTTTTTT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["removedFeatures"], serde_json::json!([]), "{v}");
        assert_eq!(v["clippedFeatures"], serde_json::json!([]), "{v}");
        // Content differs beyond case → the covered feature is surfaced in
        // contentChangedFeatures so the agent knows the annotation now
        // describes different bases.
        assert_eq!(
            v["contentChangedFeatures"],
            serde_json::json!(["gene"]),
            "equal-length replacement with different content must flag covered features: {v}"
        );
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        let f = p.features.iter().find(|f| f.name == "gene").unwrap();
        assert_eq!((f.start, f.end), (50, 100), "feature untouched");
        drop(pm);

        // Case-only change (the uppercase normalization path): content is
        // equivalent ignoring case → no contentChangedFeatures signal.
        let server = handler_with_project(edit_test_project()).await;
        let original = {
            let pm = server.pm.read().await;
            let p = pm.get_project_by_id("edit_test").unwrap();
            p.sequence[60..70].to_string() // internal 60..=69 = 1-based 61..=70
        };
        let lower: String = original.to_ascii_lowercase();
        if lower != original {
            let out = server
                .edit_sequence(Parameters(EditSequenceRequest {
                    project_id: "edit_test".to_string(),
                    start: 61,
                    end: 70,
                    replacement: Some(lower),
                    ..Default::default()
                }))
                .await
                .unwrap();
            let v = out.0;
            assert_eq!(v["ok"], true, "{}", v);
            assert!(
                v.get("contentChangedFeatures").is_none(),
                "case-only normalization must not flag features: {v}"
            );
        }

        // A length-changing replacement fully covering the feature still
        // removes it (reported in removedFeatures).
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 41,
                end: 120,
                replacement: Some("GG".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["removedFeatures"][0]["name"], "gene", "{v}");
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("edit_test").unwrap();
        assert!(p.features.iter().all(|f| f.name != "gene"));
    }

    fn cross_origin_test_project() -> ProjectData {
        // 200 bp circle with a cross-origin feature: segments (190,199)+(0,9),
        // start/end in first/last join form (start > end).
        let mut f = feature("co", "crossOrigin", 190, 9, "+");
        f.segments = vec![
            Segment { start: 190, end: 199, color: None },
            Segment { start: 0, end: 9, color: None },
        ];
        ProjectData {
            name: "co_test".to_string(),
            sequence: synthetic_dna(200, 7),
            length: 200,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![f],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn edit_sequence_pure_shift_of_cross_origin_feature_is_not_clipped() {
        // Delete 10 bp at 1-based 51..60 (internal 50..59): both segments keep
        // their base content (the first merely translates), so the feature
        // must show up in neither removedFeatures nor clippedFeatures.
        let server = handler_with_project(cross_origin_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "co_test".to_string(),
                start: 51,
                end: 60,
                replacement: Some(String::new()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["removedFeatures"], serde_json::json!([]), "{v}");
        assert_eq!(v["clippedFeatures"], serde_json::json!([]), "{v}");
        let pm = server.pm.read().await;
        let p = pm.get_project_by_id("co_test").unwrap();
        let f = p.features.iter().find(|f| f.name == "crossOrigin").unwrap();
        let spans: Vec<(i64, i64)> = f.segments.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(spans, vec![(180, 189), (0, 9)], "segments shifted, content intact");
    }

    #[tokio::test]
    async fn edit_sequence_clipped_cross_origin_feature_reports_segments() {
        // Delete the last 5 bp (1-based 196..200), clipping the tail of the
        // first segment: before/after are bounding spans on one basis and the
        // per-segment 1-based ranges ride along.
        let server = handler_with_project(cross_origin_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "co_test".to_string(),
                start: 196,
                end: 200,
                replacement: Some(String::new()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["removedFeatures"], serde_json::json!([]), "{v}");
        let c = &v["clippedFeatures"][0];
        assert_eq!(c["name"], "crossOrigin", "{v}");
        assert_eq!(c["before"], serde_json::json!({"start": 1, "end": 200}), "{v}");
        assert_eq!(c["after"], serde_json::json!({"start": 1, "end": 195}), "{v}");
        assert_eq!(
            c["beforeSegments"],
            serde_json::json!([{"start": 191, "end": 200}, {"start": 1, "end": 10}]),
            "{v}"
        );
        assert_eq!(
            c["afterSegments"],
            serde_json::json!([{"start": 191, "end": 195}, {"start": 1, "end": 10}]),
            "{v}"
        );
    }

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

    // ------------------------------------------------------------------
    // Molecule-type gates
    // ------------------------------------------------------------------

    fn protein_test_project() -> ProjectData {
        ProjectData {
            name: "prot".to_string(),
            sequence: "MVSKGEEDNM".repeat(5),
            length: 50,
            topology: "linear".to_string(),
            molecule_type: "protein".to_string(),
            features: vec![feature("f1", "mCherry", 0, 49, "+")],
            ..Default::default()
        }
    }

    fn rna_test_project() -> ProjectData {
        ProjectData {
            name: "rna".to_string(),
            sequence: "ACGU".repeat(25),
            length: 100,
            topology: "linear".to_string(),
            molecule_type: "rna".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn dna_only_tools_reject_protein_and_rna_projects() {
        let server = handler_with_project(protein_test_project()).await;

        let err = match server
            .search_sequence(Parameters(SearchRequest {
                project_id: "prot".to_string(),
                query: "ACG".to_string(),
            }))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("search_sequence should reject a protein project"),
        };
        assert!(err.message.contains("only supports DNA"), "{}", err.message);

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
        let err = match server.convert_sequence(Parameters(req)).await {
            Err(e) => e,
            Ok(_) => panic!("expected protein project-mode rejection"),
        };
        assert!(
            err.message.contains("input_path") && err.message.contains("reverse-translated"),
            "{}",
            err.message
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

    // ------------------------------------------------------------------
    // set_feature (create / update; 1-based inclusive interface params)
    // ------------------------------------------------------------------

    fn dna_test_project() -> ProjectData {
        ProjectData {
            name: "feat".to_string(),
            sequence: synthetic_dna(100, 3),
            length: 100,
            topology: "linear".to_string(),
            ..Default::default()
        }
    }

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

    // ------------------------------------------------------------------
    // add_alignment: orientedSequence / coverage + region-view diff section
    // ------------------------------------------------------------------

    fn alignment_test_project(topology: &str) -> ProjectData {
        ProjectData {
            name: "aln_test".to_string(),
            sequence: synthetic_dna(200, 7),
            length: 200,
            topology: topology.to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn add_alignment_returns_oriented_sequence_and_coverage() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        // Read = template[50..150] with one base flipped at index 60 (pos 110).
        let mut read = template[50..150].to_string();
        let i = 60;
        let orig = read.as_bytes()[i];
        let flipped = if orig == b'A' { b'C' } else { b'A' };
        read.replace_range(i..i + 1, &(flipped as char).to_string());

        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "read1".to_string(),
                bases: Some(read.clone()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["strand"], "+");
        assert_eq!(v["orientedSequence"], read, "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 51, "end": 150 }]),
            "{v}"
        );
        assert_eq!(
            v["mismatchDetails"],
            serde_json::json!([{
                "pos": 111,
                "templateBase": (orig as char).to_string(),
                "readBase": (flipped as char).to_string(),
            }]),
            "{v}"
        );
        // The alignments array carries the same new fields per entry.
        let entry = &v["alignments"][0];
        assert_eq!(entry["orientedSequence"], read, "{entry}");
        assert_eq!(
            entry["coverage"],
            serde_json::json!([{ "start": 51, "end": 150 }]),
            "{entry}"
        );

        // Region view over the mismatch lists it in the diff section.
        let out = server
            .get_region_view(Parameters(RegionRequest {
                project_id: "aln_test".to_string(),
                start: 101,
                end: 121,
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap().to_string();
        assert!(text.contains("ALIGNMENT DIFFS IN REGION"), "{text}");
        assert!(
            text.contains(&format!("mismatch at 111: {} > {}", orig as char, flipped as char)),
            "{text}"
        );

        // Window overlapping the read but not the diff.
        let out = server
            .get_region_view(Parameters(RegionRequest {
                project_id: "aln_test".to_string(),
                start: 51,
                end: 61,
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap().to_string();
        assert!(text.contains("no differences in window"), "{text}");
        assert!(!text.contains("mismatch at 111"), "{text}");

        // Window outside the read: no diff section at all.
        let out = server
            .get_region_view(Parameters(RegionRequest {
                project_id: "aln_test".to_string(),
                start: 1,
                end: 41,
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap().to_string();
        assert!(!text.contains("ALIGNMENT DIFFS IN REGION"), "{text}");
    }

    #[tokio::test]
    async fn add_alignment_focus_region_filters_diffs_and_omits_oriented_sequence() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        // Read = template[50..150] with mismatches at pos 61 (outside the
        // focus window) and pos 111 (inside).
        let mut read = template[50..150].to_string();
        for i in [10usize, 60] {
            let orig = read.as_bytes()[i];
            let flipped = if orig == b'A' { b'C' } else { b'A' };
            read.replace_range(i..i + 1, &(flipped as char).to_string());
        }

        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "focused".to_string(),
                bases: Some(read),
                path: None,
                region: Some(SegParam { start: 100, end: 120 }),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        // Only the in-window mismatch is detailed; totals stay global.
        let details = v["mismatchDetails"].as_array().unwrap();
        assert_eq!(details.len(), 1, "{v}");
        assert_eq!(details[0]["pos"], 111, "{v}");
        assert_eq!(v["mismatches"], 2, "{v}");
        // Full read omitted; focused regionView + focus echo present.
        assert!(v.get("orientedSequence").is_none(), "{v}");
        assert!(v.get("regionView").is_some(), "{v}");
        assert_eq!(v["focus"]["start"], 100, "{v}");
        assert_eq!(v["focus"]["end"], 120, "{v}");
        // The out-of-window mismatch is counted in outsideWindow.
        assert_eq!(
            v["outsideWindow"],
            serde_json::json!({"mismatches": 1, "deletions": 0, "insertions": 0}),
            "{v}"
        );
        // The alignments entry mirrors the filtering.
        let entry = &v["alignments"][0];
        assert!(entry.get("orientedSequence").is_none(), "{entry}");
        assert_eq!(entry["mismatchDetails"].as_array().unwrap().len(), 1, "{entry}");
        assert_eq!(entry["outsideWindow"]["mismatches"], 1, "{entry}");
    }

    #[tokio::test]
    async fn add_alignment_focus_feature_id_and_validation() {
        let mut project = alignment_test_project("linear");
        // 0-based 100..110 -> 1-based 101..111.
        project.features = vec![feature("f1", "site", 100, 110, "+")];
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let mut read = template[50..150].to_string();
        let i = 60; // mismatch at 1-based pos 111, inside the feature span
        let orig = read.as_bytes()[i];
        let flipped = if orig == b'A' { b'C' } else { b'A' };
        read.replace_range(i..i + 1, &(flipped as char).to_string());

        // feature_id focus with flank 5 -> window 96..116.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "ffocus".to_string(),
                bases: Some(read.clone()),
                path: None,
                feature_id: Some("f1".to_string()),
                flank: Some(5),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["focus"]["start"], 96, "{v}");
        assert_eq!(v["focus"]["end"], 116, "{v}");
        assert_eq!(v["focus"]["featureId"], "f1", "{v}");
        assert_eq!(v["mismatchDetails"].as_array().unwrap().len(), 1, "{v}");
        // The only mismatch is inside the window: nothing outside.
        assert_eq!(
            v["outsideWindow"],
            serde_json::json!({"mismatches": 0, "deletions": 0, "insertions": 0}),
            "{v}"
        );

        // Unknown feature id is rejected before aligning.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "bad".to_string(),
                bases: Some(read.clone()),
                path: None,
                feature_id: Some("nope".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("not found"),
            "{}",
            out.0
        );

        // region + feature_id together are rejected.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "both".to_string(),
                bases: Some(read),
                path: None,
                region: Some(SegParam { start: 1, end: 10 }),
                feature_id: Some("f1".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"]
                .as_str()
                .unwrap()
                .contains("mutually exclusive"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn add_alignment_reverse_strand_oriented_sequence_is_revcomp() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let read = libregene_core::utils::reverse_complement(&template[50..120]);
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "rev_read".to_string(),
                bases: Some(read),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["strand"], "-", "{v}");
        // Oriented to the template: the rev-comp of the raw read.
        assert_eq!(v["orientedSequence"], template[50..120], "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 51, "end": 120 }]),
            "{v}"
        );
    }

    #[tokio::test]
    async fn add_alignment_circular_coverage_splits_at_origin() {
        let project = alignment_test_project("circular");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let read = format!("{}{}", &template[170..200], &template[0..25]);
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "wrap_read".to_string(),
                bases: Some(read.clone()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["orientedSequence"], read, "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 171, "end": 200 }, { "start": 1, "end": 25 }]),
            "{v}"
        );
    }

    #[tokio::test]
    async fn add_alignment_compact_omits_oriented_sequence_and_region_view() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let read = template[50..150].to_string();

        // compact=true drops orientedSequence and regionView, keeps coverage.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "compact_read".to_string(),
                bases: Some(read.clone()),
                path: None,
                compact: Some(true),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert!(v.get("orientedSequence").is_none(), "{v}");
        assert!(v.get("regionView").is_none(), "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 51, "end": 150 }]),
            "{v}"
        );
        let entry = &v["alignments"][0];
        assert!(entry.get("orientedSequence").is_none(), "{entry}");
        assert!(entry.get("coverage").is_some(), "{entry}");

        // compact=false / omitted keeps orientedSequence and regionView.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "full_read".to_string(),
                bases: Some(read),
                path: None,
                compact: Some(false),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(v.get("orientedSequence").is_some(), "{v}");
        assert!(v.get("regionView").is_some(), "{v}");
        // alignments[1] is the newly added read → full detail; alignments[0]
        // is the earlier compact read → stats-only (no orientedSequence,
        // no per-column details).
        assert!(v["alignments"][1].get("orientedSequence").is_some(), "{v}");
        assert!(v["alignments"][0].get("orientedSequence").is_none(), "{v}");
        assert!(v["alignments"][0].get("mismatchDetails").is_none(), "{v}");
        assert!(v["alignments"][0].get("coverage").is_some(), "{v}");
    }

    #[tokio::test]
    async fn add_alignment_slims_existing_alignments_in_full_responses() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let mut read1 = template[50..150].to_string();
        let i = 60;
        let orig = read1.as_bytes()[i];
        let flipped = if orig == b'A' { b'C' } else { b'A' };
        read1.replace_range(i..i + 1, &(flipped as char).to_string());
        server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "read1".to_string(),
                bases: Some(read1.clone()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();

        let read2 = template[60..130].to_string();
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "read2".to_string(),
                bases: Some(read2),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["alignments"].as_array().unwrap().len(), 2, "{v}");
        // The new read is fully expanded.
        assert_eq!(v["alignments"][1]["name"], "read2", "{v}");
        assert!(v["alignments"][1].get("orientedSequence").is_some(), "{v}");
        // The previously stored read is stats-only.
        assert_eq!(v["alignments"][0]["name"], "read1", "{v}");
        assert!(v["alignments"][0].get("orientedSequence").is_none(), "{v}");
        assert!(v["alignments"][0].get("mismatchDetails").is_none(), "{v}");
        assert!(v["alignments"][0].get("deletionDetails").is_none(), "{v}");
        assert!(v["alignments"][0].get("insertionDetails").is_none(), "{v}");
        for key in [
            "alignmentId",
            "name",
            "identity",
            "strand",
            "segmentCount",
            "alignedLength",
            "mismatches",
            "insertions",
            "deletions",
            "coverage",
        ] {
            assert!(v["alignments"][0].get(key).is_some(), "missing {key}: {v}");
        }
        // No coverage gaps for a single-segment read: no coverageNote.
        assert!(v.get("coverageNote").is_none(), "{v}");
    }

    #[test]
    fn uncovered_between_segments_measures_gaps_only() {
        use libregene_core::models::{AlignSegment, Alignment};
        let aln = |segs: Vec<(usize, usize, &str)>| Alignment {
            id: String::new(),
            name: String::new(),
            length: 0,
            strand: "+".into(),
            identity: 1.0,
            segments: segs
                .into_iter()
                .map(|(s, e, c)| AlignSegment {
                    start: s,
                    end: e,
                    chars: c.to_string(),
                })
                .collect(),
            insertions: Vec::new(),
            seq: String::new(),
            trace_path: None,
        };
        // Single segment: no gap.
        assert_eq!(uncovered_between_segments(&aln(vec![(10, 29, "x")]), 60, false), 0);
        // Two segments with a 10 bp hole between them (linear).
        assert_eq!(
            uncovered_between_segments(&aln(vec![(10, 29, "x"), (40, 49, "y")]), 60, false),
            10
        );
        // Origin-spanning circular read: segments are adjacent at the wrap.
        assert_eq!(
            uncovered_between_segments(&aln(vec![(50, 59, "x"), (0, 9, "y")]), 60, true),
            0
        );
        // Circular segments with a real hole.
        assert_eq!(
            uncovered_between_segments(&aln(vec![(10, 29, "x"), (40, 49, "y")]), 60, true),
            10
        );
    }

    #[tokio::test]
    async fn design_primers_amplify_reports_orientation_and_cds_strand() {
        // Minus-strand CDS overlapping the seg: the response must spell out
        // the product orientation and the CDS strand so Fwd/Rev are not
        // misread as coding-direction names.
        let project = ProjectData {
            name: "amp_test".to_string(),
            sequence: synthetic_dna(200, 5),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "mEGFP", 60, 120, "-")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "amp_test".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 51, end: 151 }),
                name: Some("Amp".to_string()),
                target_tm: 55.0,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(v["groups"].as_array().is_some_and(|g| g.len() == 2), "{v}");
        let orientation = v["orientation"].as_str().expect("orientation note");
        assert!(orientation.contains("seg 51..151"), "{orientation}");
        assert!(orientation.contains("template top strand"), "{orientation}");
        let overlaps = v["cdsOverlaps"].as_array().expect("cdsOverlaps");
        assert_eq!(overlaps.len(), 1, "{v}");
        assert_eq!(overlaps[0]["name"], "mEGFP");
        assert_eq!(overlaps[0]["strand"], "-");
        assert!(
            overlaps[0]["note"].as_str().unwrap().contains("MINUS strand"),
            "{}",
            overlaps[0]["note"]
        );
        assert!(v["internalSites"].as_array().unwrap().is_empty(), "{v}");
    }

    #[tokio::test]
    async fn design_primers_mutagenesis_whole_codon_skips_warning() {
        // Codon-aligned full replacement inside a CDS: no warning. The same
        // full replacement OUTSIDE any CDS keeps the warning.
        let mut bytes = vec![b'A'; 90];
        bytes[60] = b'C';
        bytes[61] = b'G';
        bytes[62] = b'C';
        let project = ProjectData {
            name: "mut_test".to_string(),
            sequence: String::from_utf8(bytes).unwrap(),
            length: 90,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "orf", 30, 89, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;

        // Whole-codon swap CGC -> AAA (Arg -> Lys): expected operation.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "mut_test".to_string(),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 61, end: 63 }),
                site_name: Some("A11K".to_string()),
                target_tm: 55.0,
                mut_seq: Some("AAA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(
            v["mutation"].get("warning").is_none(),
            "whole-codon swap must not warn: {v}"
        );
        // The mutation block is 1-based: seg 61..63, codonIndex ==
        // aaPosition1Based (aa numbering conventions are untouched).
        assert_eq!(v["mutation"]["segStart"], 61, "{v}");
        assert_eq!(v["mutation"]["segEnd"], 63, "{v}");
        assert_eq!(v["mutation"]["cds"]["codonIndex"], 11, "{v}");
        assert_eq!(v["mutation"]["cds"]["aaBefore"], "Arg", "{v}");
        assert_eq!(v["mutation"]["cds"]["aaAfter"], "Lys", "{v}");
        assert_eq!(v["mutation"]["cds"]["aaPosition1Based"], 11, "{v}");
        assert_eq!(v["mutation"]["cds"]["aaPositionExcludingMet"], 10, "{v}");

        // Same 3-base full replacement outside any CDS: warning kept.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "mut_test".to_string(),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 11, end: 13 }),
                site_name: Some("M1".to_string()),
                target_tm: 55.0,
                mut_seq: Some("CCC".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        let w = v["mutation"]["warning"]
            .as_str()
            .expect("non-CDS full replacement must warn");
        assert!(w.contains("PLUS-strand"), "{w}");
    }

    #[tokio::test]
    async fn design_primers_mutagenesis_orientation_hint_minus_strand() {
        // Minus-strand CDS: mut_seq is PLUS-strand content, so the coding
        // effect is its reverse complement; the orientationHint must state
        // the CDS strand and the actual codon/amino-acid outcome.
        let mut bytes = vec![b'A'; 90];
        // Plus-strand CGC at 60..62 (0-based) -> coding (minus) GCG = Ala.
        bytes[60] = b'C';
        bytes[61] = b'G';
        bytes[62] = b'C';
        let project = ProjectData {
            name: "mut_hint".to_string(),
            sequence: String::from_utf8(bytes).unwrap(),
            length: 90,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "orf", 30, 89, "-")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;

        // Coding GCG->AAG (Ala->Lys) on a minus-strand CDS is plus-strand
        // mut_seq = rev-comp(AAG) = CTT.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "mut_hint".to_string(),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 61, end: 63 }),
                site_name: Some("A11K".to_string()),
                target_tm: 55.0,
                mut_seq: Some("CTT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["mutation"]["cds"]["aaAfter"], "Lys", "{v}");
        let hint = v["mutation"]["orientationHint"]
            .as_str()
            .expect("orientationHint present");
        assert!(hint.contains("MINUS strand"), "{hint}");
        assert!(hint.contains("AAG") && hint.contains("Lys"), "{hint}");
        assert!(hint.contains("reverse-complement"), "{hint}");

        // Plus-strand CDS: the hint confirms the direct read-out instead.
        let project = ProjectData {
            name: "mut_hint2".to_string(),
            sequence: {
                let mut b = vec![b'A'; 90];
                b[60] = b'C';
                b[61] = b'G';
                b[62] = b'C';
                String::from_utf8(b).unwrap()
            },
            length: 90,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "orf", 30, 89, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "mut_hint2".to_string(),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 61, end: 63 }),
                site_name: Some("A11K".to_string()),
                target_tm: 55.0,
                mut_seq: Some("AAA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let hint = out.0["mutation"]["orientationHint"]
            .as_str()
            .expect("orientationHint present");
        assert!(hint.contains("PLUS strand"), "{hint}");
        assert!(hint.contains("AAA") && hint.contains("Lys"), "{hint}");
    }

    #[tokio::test]
    async fn design_primers_unified_tm_matches_check_primer_binding() {
        // Construct a template where the fwd enzyme tail's 3' side accidentally
        // pairs with the template upstream of the anneal core. The unified
        // annealLen/Tm must match a separate check_primer_binding call.
        let mut seq = synthetic_dna(120, 42);
        // BamHI site (GGATCC) is the 3'-most 6 bases of the default fwd tail
        // GCG + GGATCC. Place it immediately 5' of the fwd anneal core.
        seq.replace_range(30..36, "GGATCC");
        let project = ProjectData {
            name: "tail_test".to_string(),
            sequence: seq.clone(),
            length: 120,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "tail_test".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 37, end: 77 }),
                name: Some("Amp".to_string()),
                target_tm: 55.0,
                fwd_enzyme: Some("BamHI".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(v.get("tmBasis").is_some(), "{v}");
        let fwd_group = v["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["type"] == "fwd")
            .cloned()
            .expect("fwd group");
        let default_idx = fwd_group["defaultIndex"].as_u64().map(|n| n as usize).unwrap_or(0);
        let cand = &fwd_group["candidates"][default_idx];
        let primer_seq = cand["seq"].as_str().unwrap().to_string();
        let designed_len = cand["designedAnnealLen"].as_u64().unwrap() as usize;
        let unified_len = cand["annealLen"].as_u64().unwrap() as usize;
        assert!(
            unified_len > designed_len,
            "tail should extend anneal_len: designed={designed_len}, unified={unified_len}"
        );

        // Verify the same values come out of check_primer_binding.
        let chk = server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: "tail_test".to_string(),
                primers: vec![PrimerInput {
                    name: "cand".to_string(),
                    r#type: "fwd".to_string(),
                    seq: primer_seq,
                }],
            }))
            .await
            .unwrap();
        let site = &chk.0["results"][0]["site"];
        assert_eq!(
            site["annealLen"].as_u64().unwrap() as usize,
            unified_len,
            "annealLen mismatch"
        );
        assert!(
            (site["tm"].as_f64().unwrap() - cand["tm"].as_f64().unwrap()).abs() < 0.05,
            "tm mismatch: check={} design={}",
            site["tm"],
            cand["tm"]
        );
    }

    #[tokio::test]
    async fn design_primers_unified_tm_matches_check_primer_binding_rev() {
        // Rev enzyme tail whose 3' side accidentally pairs with the template
        // downstream of the rev anneal core. The unified Tm must match what
        // check_primer_binding reports (the engine reverses the matched bases
        // for rev primers before computing Tm).
        let mut seq = synthetic_dna(120, 42);
        // HindIII tail = protect GCG + AAGCTT. Place AAGCTT immediately 3' of
        // the rev anneal core (which ends at seg.end = 77 0-based) so the
        // tail's 3'-most 6 bases pair.
        seq.replace_range(77..83, "AAGCTT");
        let project = ProjectData {
            name: "tail_rev_test".to_string(),
            sequence: seq.clone(),
            length: 120,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "tail_rev_test".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 37, end: 77 }),
                name: Some("Amp".to_string()),
                target_tm: 55.0,
                rev_enzyme: Some("HindIII".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        let rev_group = v["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["type"] == "rev")
            .cloned()
            .expect("rev group");
        let default_idx = rev_group["defaultIndex"].as_u64().map(|n| n as usize).unwrap_or(0);
        let cand = &rev_group["candidates"][default_idx];
        let primer_seq = cand["seq"].as_str().unwrap().to_string();
        let designed_len = cand["designedAnnealLen"].as_u64().unwrap() as usize;
        let unified_len = cand["annealLen"].as_u64().unwrap() as usize;
        assert!(
            unified_len > designed_len,
            "rev tail should extend anneal_len: designed={designed_len}, unified={unified_len}"
        );

        let chk = server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: "tail_rev_test".to_string(),
                primers: vec![PrimerInput {
                    name: "cand".to_string(),
                    r#type: "rev".to_string(),
                    seq: primer_seq,
                }],
            }))
            .await
            .unwrap();
        let site = &chk.0["results"][0]["site"];
        assert_eq!(
            site["annealLen"].as_u64().unwrap() as usize,
            unified_len,
            "annealLen mismatch"
        );
        assert!(
            (site["tm"].as_f64().unwrap() - cand["tm"].as_f64().unwrap()).abs() < 0.05,
            "tm mismatch: check={} design={}",
            site["tm"],
            cand["tm"]
        );
    }

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
        assert_eq!(v["input"]["kind"], "template", "{v}");
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
        assert_eq!(v["input"]["kind"], "featureOffset", "{v}");
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
        assert_eq!(v["input"]["kind"], "aminoAcid", "{v}");
        assert_eq!(v["input"]["aaPosition"], 2, "{v}");
        assert_eq!(v["position"], 4, "{v}");
        assert_eq!(v["base"], "G", "{v}");
        assert_eq!(v["codonPositions"], serde_json::json!([4, 5, 6]), "{v}");
        assert_eq!(v["translations"][0]["codonIndex"], 2, "{v}");
        assert_eq!(v["translations"][0]["aaPositionExcludingMet"], 1, "{v}");
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
                    seq: seq[50..70].to_string(),
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

    fn enzyme_test_project() -> ProjectData {
        // EcoRI GAATTC at internal 0-based 40..45; no AgeI (ACCGGT) site.
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
        assert!(project.enzymes.iter().any(|e| e.name == "EcoRI"));
        assert!(!project.enzymes.iter().any(|e| e.name == "AgeI"));
        project
    }

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

    #[test]
    fn project_id_is_required_on_project_tools() {
        assert!(serde_json::from_value::<EditSequenceRequest>(serde_json::json!({
            "start": 1,
            "end": 2,
            "replacement": "AA"
        }))
        .is_err());
        assert!(serde_json::from_value::<SequenceRequest>(serde_json::json!({
            "start": 1,
            "end": 10
        }))
        .is_err());
        assert!(serde_json::from_value::<OverviewRequest>(serde_json::json!({})).is_err());
    }

    #[tokio::test]
    async fn mutating_tools_require_agent_tab() {
        let server = handler_with_unbound_project(edit_test_project()).await;
        let err = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 1,
                end: 2,
                replacement: Some("AA".to_string()),
                ..Default::default()
            }))
            .await
            .err()
            .expect("expected agent-tab gate error");
        assert!(err.message.contains("not bound"), "{err}");
        assert!(err.message.contains("open_project"), "{err}");
        assert!(err.message.contains("cp"), "{err}");
        let err = server
            .set_feature(Parameters(SetFeatureRequest {
                project_id: "edit_test".to_string(),
                name: Some("x".to_string()),
                ftype: Some("misc_feature".to_string()),
                start: Some(1),
                end: Some(5),
                ..Default::default()
            }))
            .await
            .err()
            .expect("expected agent-tab gate error");
        assert!(err.message.contains("not bound"), "{err}");
        // Read-only tools stay usable without an agent tab.
        server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "edit_test".to_string(),
                start: Some(1),
                end: Some(10),
                ..Default::default()
            }))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn tool_call_relocks_unlocked_agent_tab() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let project = edit_test_project();
        let id = project.name.clone();
        pm.write().await.load(&id, project).unwrap();
        let agent_tabs: crate::AgentTabs = Arc::new(RwLock::new(HashMap::new()));
        // The user has unlocked the tab.
        agent_tabs.write().await.insert(id.clone(), crate::AgentTabMeta { locked: false });
        let server = LibreGeneMcp::new(
            app.handle().clone(),
            pm,
            Arc::new(RwLock::new(HashMap::new())),
            agent_tabs.clone(),
        );
        // Any tool call that resolves the project re-locks the tab.
        server
            .read_sequence(Parameters(SequenceRequest {
                project_id: id.clone(),
                start: Some(1),
                end: Some(10),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert!(agent_tabs.read().await[&id].locked);
    }

    #[test]
    fn sanitize_window_label_keeps_only_alnum_dash_underscore() {
        assert_eq!(
            sanitize_window_label("/tmp/my project (v2).gbk"),
            "_tmp_my_project__v2__gbk"
        );
        assert_eq!(sanitize_window_label("plain_path-1_2.gbk"), "plain_path-1_2_gbk");
        assert_eq!(sanitize_window_label("ABC-def_123"), "ABC-def_123");
    }

    // ------------------------------------------------------------------
    // open_project / close_project / save_file guards
    // ------------------------------------------------------------------

    fn write_temp_gbk(dir_name: &str, file_name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let project = ProjectData {
            name: "tmp".to_string(),
            sequence: synthetic_dna(120, 77),
            length: 120,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("f1", "gene", 10, 49, "+")],
            ..Default::default()
        };
        let dir = std::env::temp_dir().join(format!("libregene-mcp-{}-{}", dir_name, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(file_name);
        libregene_core::file_io::gbk::write_gbk(&project, &path).unwrap();
        (dir, path)
    }

    #[tokio::test]
    async fn open_project_binds_fresh_file_as_agent_tab() {
        let (dir, path) = write_temp_gbk("open", "fresh.gbk");
        let server = test_handler();
        let out = server
            .open_project(Parameters(OpenProjectRequest {
                path: path.to_string_lossy().into_owned(),
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["projectId"], path.to_str().unwrap(), "{v}");
        assert!(v["regionView"].is_string(), "{v}");
        let id = path.to_string_lossy().into_owned();
        {
            let pm = server.pm.read().await;
            assert!(pm.get_project_by_id(&id).is_some(), "project loaded");
        }
        assert!(
            server.agent_tabs.read().await[&id].locked,
            "fresh open binds a locked agent tab"
        );
        // The bound project accepts mutations.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: id.clone(),
                start: 1,
                end: 2,
                replacement: Some("AA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn open_project_reuses_existing_agent_tab() {
        // handler_with_project pre-loads AND pre-binds the project; the id
        // doubles as the "path" key here.
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .open_project(Parameters(OpenProjectRequest {
                path: "edit_test".to_string(),
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["reused"], true, "{v}");
        assert_eq!(v["locked"], true, "{v}");
    }

    #[tokio::test]
    async fn open_project_rejects_user_opened_project() {
        // Loaded but not bound = opened by the user; open_project refuses and
        // points at the bash-cp-copy workflow.
        let server = handler_with_unbound_project(edit_test_project()).await;
        let err = server
            .open_project(Parameters(OpenProjectRequest {
                path: "edit_test".to_string(),
            }))
            .await
            .err()
            .expect("expected refusal for a user-opened project");
        assert!(err.message.contains("cp"), "{err}");
        assert!(err.message.contains("open_project"), "{err}");
    }

    #[tokio::test]
    async fn open_project_binds_path_with_parens_and_spaces() {
        // Agent tabs are keyed by the raw project id (= path), so paths with
        // parens/spaces bind without sanitization and never touch
        // window_projects.
        let (dir, path) = write_temp_gbk("open-parens", "my project (v2).gbk");
        let server = test_handler();
        let out = server
            .open_project(Parameters(OpenProjectRequest {
                path: path.to_string_lossy().into_owned(),
            }))
            .await
            .unwrap();
        let v = out.0;
        let id = path.to_string_lossy().into_owned();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["projectId"], id, "{v}");
        assert!(server.agent_tabs.read().await.contains_key(&id));
        assert!(
            server.wp.read().await.is_empty(),
            "no window must be created"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn close_project_guards_unbound_and_dirty() {
        // Unbound project → refused by the agent-tab gate.
        let server = handler_with_unbound_project(edit_test_project()).await;
        let err = server
            .close_project(Parameters(CloseProjectRequest {
                project_id: "edit_test".to_string(),
                ..Default::default()
            }))
            .await
            .err()
            .expect("unbound close must fail");
        assert!(err.message.contains("not bound"), "{err}");

        // Bound but dirty → needs force.
        let server = handler_with_project(edit_test_project()).await;
        server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 1,
                end: 2,
                replacement: Some("AA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let out = server
            .close_project(Parameters(CloseProjectRequest {
                project_id: "edit_test".to_string(),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(out.0["message"].as_str().unwrap().contains("force"), "{}", out.0);
        assert!(
            server.pm.read().await.get_project_by_id("edit_test").is_some(),
            "project survives a refused close"
        );

        // force: true discards the dirty project.
        let out = server
            .close_project(Parameters(CloseProjectRequest {
                project_id: "edit_test".to_string(),
                force: Some(true),
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        assert!(server.pm.read().await.get_project_by_id("edit_test").is_none());
        assert!(
            !server.agent_tabs.read().await.contains_key("edit_test"),
            "agent tab is cleaned up"
        );
    }

    #[tokio::test]
    async fn close_project_clean_project_closes() {
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .close_project(Parameters(CloseProjectRequest {
                project_id: "edit_test".to_string(),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        assert!(server.pm.read().await.get_project_by_id("edit_test").is_none());
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

    /// DNA/RNA replacements must be IUPAC nucleotide bases; a protein file as
    /// replacement_path for a DNA project is rejected (and vice versa).
    #[tokio::test]
    async fn edit_sequence_validates_replacement_alphabet_and_file_type() {
        let server = handler_with_project(edit_test_project()).await;

        // Non-IUPAC letters rejected on a DNA project.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 10,
                end: 12,
                replacement: Some("AXZ".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(out.0["message"].as_str().unwrap().contains("IUPAC"), "{}", out.0);

        // Degenerate IUPAC codes are accepted.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 10,
                end: 12,
                replacement: Some("NWR".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);

        // A protein .gpt as replacement_path for a DNA project is refused.
        let gpt_path = std::env::temp_dir()
            .join(format!("libregene-mcp-repl-{}.gpt", std::process::id()));
        write_convert_output(gpt_path.to_str().unwrap(), "MVS*", "protein", None, None).unwrap();
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 10,
                end: 12,
                replacement_path: Some(gpt_path.to_string_lossy().into_owned()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(out.0["message"].as_str().unwrap().contains("protein"), "{}", out.0);
        std::fs::remove_file(&gpt_path).ok();
    }

    /// A U-bearing replacement inserted into a DNA project is normalized to T
    /// (the note reports the conversion); a no-cross insertion carries no note.
    #[tokio::test]
    async fn edit_sequence_normalizes_u_for_dna_project_and_notes_conversion() {
        let server = handler_with_project(edit_test_project()).await;

        // Insert "AUG" (has U) into the DNA project: normalization rewrites it
        // to "ATG" and the response must tell the caller a conversion happened.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 10,
                end: 9,
                replacement: Some("AUG".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        let note = out.0["note"].as_str().unwrap_or_default();
        assert!(note.contains("Converted 1 U→T"), "{}", out.0);
        // No U left in the sequence after normalization.
        assert!(server
            .read_sequence(Parameters(SequenceRequest {
                project_id: "edit_test".to_string(),
                start: Some(10),
                end: Some(20),
                ..Default::default()
            }))
            .await
            .unwrap()
            .0["sequence"]
            .as_str()
            .unwrap()
            .matches('U')
            .next()
            .is_none());

        // Same-alphabet insertion → no note.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 20,
                end: 19,
                replacement: Some("ATG".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        assert!(
            !out.0["note"].as_str().unwrap_or_default().contains("Converted"),
            "{}",
            out.0
        );
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
        let err = server
            .convert_sequence(Parameters(req))
            .await
            .err()
            .expect("existing output_path without overwrite must fail");
        assert!(err.message.contains("overwrite"), "{err}");

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

    // ------------------------------------------------------------------
    // Regression tests: audit fixes
    // ------------------------------------------------------------------

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

    #[test]
    fn focus_filter_counts_partial_deletion_overlap() {
        // A deletion kept by a partial overlap must count only its in-window
        // bases against the window, not its full length.
        let mut v = serde_json::json!({
            "mismatches": 0, "insertions": 0, "deletions": 10,
            "mismatchDetails": [],
            "insertionDetails": [],
            "deletionDetails": [{"pos": 8, "length": 10, "bases": "XXXXXXXXXX"}],
        });
        filter_alignment_json_focus(&mut v, 1, 10, 100, false);
        assert_eq!(v["deletionDetails"].as_array().unwrap().len(), 1, "{v}");
        assert_eq!(
            v["outsideWindow"],
            serde_json::json!({"mismatches": 0, "deletions": 7, "insertions": 0}),
            "{v}"
        );
    }

    #[test]
    fn focus_filter_wraps_origin_merged_deletion() {
        // Circular tlen=20: a merged deletion at 1-based pos 18 length 5
        // covers bases 18,19,20,1,2 (coordinates past tlen wrap back).
        let mk = || serde_json::json!({
            "mismatches": 0, "insertions": 0, "deletions": 5,
            "mismatchDetails": [],
            "insertionDetails": [],
            "deletionDetails": [{"pos": 18, "length": 5, "bases": "XXXXX"}],
        });
        let mut v = mk();
        filter_alignment_json_focus(&mut v, 1, 3, 20, true);
        assert_eq!(v["deletionDetails"].as_array().unwrap().len(), 1, "{v}");
        assert_eq!(v["outsideWindow"]["deletions"], 3, "{v}");
        let mut v = mk();
        filter_alignment_json_focus(&mut v, 18, 20, 20, true);
        assert_eq!(v["outsideWindow"]["deletions"], 2, "{v}");
        // A window touching neither arc drops the entry entirely.
        let mut v = mk();
        filter_alignment_json_focus(&mut v, 5, 10, 20, true);
        assert_eq!(v["deletionDetails"].as_array().unwrap().len(), 0, "{v}");
        assert_eq!(v["outsideWindow"]["deletions"], 5, "{v}");
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
        let spec = RegionSpec {
            enzyme1: Some("GoodCutter".to_string()),
            enzyme2: Some("BbsI".to_string()),
            ..Default::default()
        };
        let err = resolve_export_region(&project, &spec).expect_err("must reject");
        assert!(
            err.contains("BbsI") && err.contains("falls outside the linear molecule"),
            "{err}"
        );
    }

    #[test]
    fn amplicon_rev_site_wrapping_origin_exports_wrap_arc() {
        // Circular len=100: rev site covers 95..=99,0..=4 (stored wrapped,
        // template_end < template_start); fwd 5' end at 3 (1-based). The
        // amplicon runs from the fwd 5' end ACROSS the origin to the rev 5'
        // end — previously f_start <= r_end picked the short wrong arc.
        let seq = synthetic_dna(100, 13);
        let site = |strand: i8, start: i64, end: i64, id: &str| libregene_core::models::PrimerBindingSite {
            primer_id: id.to_string(),
            strand,
            template_start: start,
            template_end: end,
            tm: 60.0,
            gc_content: 0.5,
            match_score: 20,
            has_3_prime_mismatch: false,
            five_prime_tail: String::new(),
            three_prime_tail: String::new(),
            alignment: Default::default(),
        };
        let project = ProjectData {
            name: "ampwrap".to_string(),
            sequence: seq,
            length: 100,
            topology: "circular".to_string(),
            molecule_type: "dna".to_string(),
            primers: vec![
                Primer {
                    id: "fp".to_string(),
                    name: "fp".to_string(),
                    r#type: "fwd".to_string(),
                    primer_seq: "AAAAAAAAAAAAAAAAAAAA".to_string(),
                    binding_sites: vec![site(1, 2, 22, "fp")],
                },
                Primer {
                    id: "rp".to_string(),
                    name: "rp".to_string(),
                    r#type: "rev".to_string(),
                    primer_seq: "TTTTTTTTTTTTTTTTTTTT".to_string(),
                    binding_sites: vec![site(-1, 95, 5, "rp")],
                },
            ],
            ..Default::default()
        };
        let spec = RegionSpec {
            fwd_primer: Some("fp".to_string()),
            rev_primer: Some("rp".to_string()),
            ..Default::default()
        };
        let (pieces, _, _) = resolve_export_region(&project, &spec).expect("resolves");
        assert_eq!(pieces, vec![(2, 99), (0, 4)], "amplicon must wrap the origin");
    }

    #[tokio::test]
    async fn add_alignment_compact_slims_history_and_focuses_new_diffs() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "r1".to_string(),
                bases: Some(template[50..150].to_string()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);

        // Second read with mismatches at 1-based 61 (outside focus) and 111
        // (inside), added with compact + focus: the history entry must be
        // stats-only (previously compact kept FULL details for history) and
        // the new entry must be focus-filtered with outsideWindow
        // (previously compact skipped the focus filter entirely).
        let mut read2 = template[50..150].to_string();
        for i in [10usize, 60] {
            let orig = read2.as_bytes()[i];
            let flipped = if orig == b'A' { b'C' } else { b'A' };
            read2.replace_range(i..i + 1, &(flipped as char).to_string());
        }
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "r2".to_string(),
                bases: Some(read2),
                path: None,
                compact: Some(true),
                region: Some(SegParam { start: 100, end: 120 }),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        let hist = &v["alignments"][0];
        assert!(hist.get("mismatchDetails").is_none(), "history stats-only: {hist}");
        let new = &v["alignments"][1];
        let det = new["mismatchDetails"].as_array().unwrap();
        assert_eq!(det.len(), 1, "{new}");
        assert_eq!(det[0]["pos"], 111, "{new}");
        assert_eq!(new["outsideWindow"]["mismatches"], 1, "{new}");
        assert!(new.get("orientedSequence").is_none(), "{new}");
        assert_eq!(v["focus"]["start"], 100, "{v}");
        assert!(v.get("regionView").is_none(), "compact suppresses regionView: {v}");
    }

    #[tokio::test]
    async fn edit_sequence_expected_old_checked_against_live_state() {
        let original = edit_test_project().sequence[..2].to_string();
        let server = handler_with_project(edit_test_project()).await;
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 1,
                end: 2,
                replacement: Some("TT".to_string()),
                expected_old: Some(original.clone()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);
        // A retry with the now-stale expected_old must fail, and
        // currentContent must reflect the LIVE sequence (TT), proving the
        // check re-reads the project instead of a stale snapshot.
        let out = server
            .edit_sequence(Parameters(EditSequenceRequest {
                project_id: "edit_test".to_string(),
                start: 1,
                end: 2,
                replacement: Some("GG".to_string()),
                expected_old: Some(original),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert_eq!(out.0["currentContent"], "TT", "{}", out.0);
        let pm = server.pm.read().await;
        assert_eq!(
            &pm.get_project_by_id("edit_test").unwrap().sequence[..2],
            "TT",
            "failed edit must not mutate"
        );
    }

    #[tokio::test]
    async fn convert_sequence_project_mode_rejects_output_path_and_rna() {
        let mut p = dna_test_project();
        p.features = vec![feature("f1", "cds", 10, 60, "+")];
        let server = handler_with_project(p).await;
        let err = match server
            .convert_sequence(Parameters(convert_req(vec![ConvertItem {
                project_id: Some("feat".to_string()),
                feature_id: Some("f1".to_string()),
                species: Some("e_coli".to_string()),
                output_path: Some("/tmp/libregene-should-not-write.gbk".to_string()),
                ..Default::default()
            }])))
            .await
        {
            Err(e) => e,
            Ok(v) => panic!("expected output_path rejection, got {}", v.0),
        };
        assert!(err.message.contains("output_path"), "{}", err.message);
        assert!(!std::path::Path::new("/tmp/libregene-should-not-write.gbk").exists());

        // RNA projects have no coding DNA to re-encode either.
        let server = handler_with_project(rna_test_project()).await;
        let err = match server
            .convert_sequence(Parameters(convert_req(vec![ConvertItem {
                project_id: Some("rna".to_string()),
                feature_id: Some("f1".to_string()),
                species: Some("e_coli".to_string()),
                ..Default::default()
            }])))
            .await
        {
            Err(e) => e,
            Ok(v) => panic!("expected rna rejection, got {}", v.0),
        };
        assert!(err.message.contains("rna"), "{}", err.message);
    }

    #[tokio::test]
    async fn design_primers_validates_segment_bounds() {
        let server = handler_with_project(dna_test_project()).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "feat".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 90, end: 120 }),
                target_tm: 55.0,
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
        // start > end on a LINEAR template is rejected.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "feat".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 90, end: 10 }),
                target_tm: 55.0,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("wraps the origin"),
            "{}",
            out.0
        );
        // start > end on a CIRCULAR template wraps and designs fine.
        let mut p = dna_test_project();
        p.topology = "circular".to_string();
        let server = handler_with_project(p).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "feat".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 90, end: 10 }),
                target_tm: 55.0,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert!(
            out.0["groups"].as_array().is_some_and(|g| g.len() == 2),
            "{}",
            out.0
        );
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

    #[tokio::test]
    async fn primer_tools_validate_seq_and_type() {
        let server = handler_with_project(dna_test_project()).await;
        let out = server
            .add_primer(Parameters(AddPrimerRequest {
                project_id: "feat".to_string(),
                name: "p1".to_string(),
                r#type: "fwd".to_string(),
                seq: "123 ---".to_string(),
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
            .add_primer(Parameters(AddPrimerRequest {
                project_id: "feat".to_string(),
                name: "p1".to_string(),
                r#type: "sideways".to_string(),
                seq: "ACGTACGT".to_string(),
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("fwd"),
            "{}",
            out.0
        );
        // check_primer_binding shares the validation.
        let out = server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: "feat".to_string(),
                primers: vec![PrimerInput {
                    name: "x".to_string(),
                    r#type: "bad".to_string(),
                    seq: "ACGTACGT".to_string(),
                }],
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        // Rejected primers were not persisted.
        let pm = server.pm.read().await;
        assert!(pm.get_project_by_id("feat").unwrap().primers.is_empty());
    }

    fn assert_hex7(v: &serde_json::Value) -> String {
        let s = v.as_str().expect("hash is a string");
        assert_eq!(s.len(), 7, "{s}");
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()), "{s}");
        s.to_string()
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
    async fn list_projects_entries_carry_sequence_hashes() {
        let server = handler_with_project(edit_test_project()).await;
        let out = server.list_projects().await.unwrap();
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
