use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use libregene_core::project::ProjectManager;
use libregene_core::models::ProjectData;
use libregene_core::models::Segment;
use libregene_core::models::Feature;
use libregene_core::models::Primer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::mcp::*;

    /// Raw HTTP POST /mcp initialize; true when the MCP handshake succeeds.
    pub(crate) async fn handshake_ok(port: u16, token: &str) -> bool {
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

    /// Like handshake_ok but sends no Authorization header at all.
    pub(crate) async fn handshake_anon(port: u16) -> bool {
        let addr = format!("127.0.0.1:{}", port);
        let Ok(mut stream) = tokio::net::TcpStream::connect(&addr).await else {
            return false;
        };
        let body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"cfg-test\",\"version\":\"0\"}}}";
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
    pub(crate) async fn raw_post(port: u16, token: &str, extra_headers: &str, body: &str) -> String {
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

    pub(crate) async fn wait_up(port: u16, token: &str) -> bool {
        for _ in 0..40 {
            if handshake_ok(port, token).await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    pub(crate) fn test_server() -> McpServer<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        McpServer::new(
            app.handle().clone(),
            Arc::new(RwLock::new(ProjectManager::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(Vec::new())),
            |_, _| {},
        )
    }

    pub(crate) fn test_handler() -> LibreGeneMcp<MockRuntime> {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        LibreGeneMcp::new(
            app.handle().clone(),
            Arc::new(RwLock::new(ProjectManager::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(Vec::new())),
        )
    }

    pub(crate) fn convert_req(items: Vec<ConvertItem>) -> ConvertSequenceRequest {
        ConvertSequenceRequest { items: Some(items), ..Default::default() }
    }

    /// Deterministic pseudo-random ACGT sequence (unique long substrings).
    pub(crate) fn synthetic_dna(length: usize, mut seed: u64) -> String {
        let mut out = String::with_capacity(length);
        for _ in 0..length {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            out.push(b"ACGT"[(seed >> 33) as usize & 3] as char);
        }
        out
    }

    pub(crate) fn feature(id: &str, name: &str, start: i64, end: i64, strand: &str) -> Feature {
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
    pub(crate) async fn handler_with_project(project: ProjectData) -> LibreGeneMcp<MockRuntime> {
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
            Arc::new(RwLock::new(Vec::new())),
        )
    }

    /// Same as handler_with_project but WITHOUT an agent tab — for testing
    /// that mutating tools refuse projects not bound as an agent tab.
    pub(crate) async fn handler_with_unbound_project(project: ProjectData) -> LibreGeneMcp<MockRuntime> {
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
            Arc::new(RwLock::new(Vec::new())),
        )
    }

    pub(crate) fn edit_test_project() -> ProjectData {
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

    /// Write a 60 bp fragment carrying a feature (10..29, "+"), a feature
    /// named "gene" (0..5, clashes with the target's "gene") and a primer
    /// binding 30..49; returns (dir, path).
    pub(crate) fn write_annotated_insert() -> (std::path::PathBuf, std::path::PathBuf) {
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
        // Unique per call: two tests call this helper concurrently, and a
        // shared dir would let one test's remove_dir_all delete the other's
        // insert.gbk mid-write (flaky NotFound).
        static ANN_DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "libregene-mcp-edit-ann-{}-{}",
            std::process::id(),
            ANN_DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let gbk = dir.join("insert.gbk");
        libregene_core::file_io::gbk::write_gbk(&src, &gbk).unwrap();
        (dir, gbk)
    }

    pub(crate) fn cross_origin_test_project() -> ProjectData {
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

    pub(crate) fn protein_test_project() -> ProjectData {
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

    pub(crate) fn rna_test_project() -> ProjectData {
        ProjectData {
            name: "rna".to_string(),
            sequence: "ACGU".repeat(25),
            length: 100,
            topology: "linear".to_string(),
            molecule_type: "rna".to_string(),
            ..Default::default()
        }
    }

    pub(crate) fn dna_test_project() -> ProjectData {
        ProjectData {
            name: "feat".to_string(),
            sequence: synthetic_dna(100, 3),
            length: 100,
            topology: "linear".to_string(),
            ..Default::default()
        }
    }

    pub(crate) fn alignment_test_project(topology: &str) -> ProjectData {
        ProjectData {
            name: "aln_test".to_string(),
            sequence: synthetic_dna(200, 7),
            length: 200,
            topology: topology.to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        }
    }

    pub(crate) fn enzyme_test_project() -> ProjectData {
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

    pub(crate) fn write_temp_gbk(dir_name: &str, file_name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
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

    pub(crate) fn assert_hex7(v: &serde_json::Value) -> String {
        let s = v.as_str().expect("hash is a string");
        assert_eq!(s.len(), 7, "{s}");
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()), "{s}");
        s.to_string()
    }
