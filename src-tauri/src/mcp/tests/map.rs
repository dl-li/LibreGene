use super::common::*;
use crate::mcp::*;
use base64::Engine;

fn map_project() -> ProjectData {
    let mut project = dna_test_project();
    project.features = vec![feature("f1", "AmpR", 90, 9, "+")];
    project.topology = "circular".to_string();
    project
}

#[tokio::test]
async fn render_map_returns_inline_svg_and_relocks_agent_tab() {
    let server = handler_with_project(map_project()).await;
    server
        .agent_tabs
        .write()
        .await
        .insert("feat".to_string(), crate::AgentTabMeta { locked: false });
    let out = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            format: Some("svg".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["view"], "circular");
    assert_eq!(out["featureCount"], 1);
    assert!(out["width"].as_f64().unwrap() > 0.0);
    assert!(out["height"].as_f64().unwrap() > 0.0);
    let svg = out["svg"].as_str().unwrap();
    assert!(svg.starts_with("<svg xmlns="), "{svg}");
    assert!(svg.contains("AmpR"), "{svg}");
    assert!(svg.contains("100 bp"), "{svg}");
    assert!(out.get("path").is_none(), "{out}");
    assert!(server.agent_tabs.read().await["feat"].locked);
}

#[tokio::test]
async fn render_map_writes_atomically_and_requires_overwrite() {
    let server = handler_with_project(map_project()).await;
    let dir = std::env::temp_dir().join(format!("libregene-mcp-map-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("map.svg");
    let out = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            output_path: Some(path.to_string_lossy().into_owned()),
            format: Some("svg".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(out["ok"], true, "{out}");
    assert!(out.get("svg").is_none(), "{out}");
    assert_eq!(
        out["path"],
        crate::normalize_project_path(path.to_str().unwrap()).unwrap()
    );
    let svg = std::fs::read_to_string(&path).unwrap();
    assert_eq!(out["bytesWritten"], svg.len());
    assert!(svg.contains("AmpR"), "{svg}");

    let refused = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            output_path: Some(path.to_string_lossy().into_owned()),
            format: Some("svg".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(refused["message"]
        .as_str()
        .unwrap()
        .contains("overwrite: true"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), svg);

    let overwritten = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            output_path: Some(path.to_string_lossy().into_owned()),
            format: Some("svg".to_string()),
            overwrite: Some(true),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(overwritten["ok"], true, "{overwritten}");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn render_map_linear_can_hide_feature_labels() {
    let server = handler_with_project(map_project()).await;
    let out = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            view: Some("linear".to_string()),
            include_labels: Some(false),
            format: Some("svg".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["view"], "linear");
    let svg = out["svg"].as_str().unwrap();
    assert!(!svg.contains(">AmpR</text>"), "{svg}");
    assert!(svg.contains("feat · 100 bp"), "{svg}");
}

#[tokio::test]
async fn render_map_is_read_only_for_user_projects() {
    let server = handler_with_unbound_project(map_project()).await;
    let out = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["format"], "jpg");
    assert_eq!(out["mimeType"], "image/jpeg");
    assert!(out.get("svg").is_none(), "{out}");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(out["jpg"].as_str().unwrap())
        .unwrap();
    assert!(bytes.starts_with(&[0xff, 0xd8, 0xff]));
    assert!(bytes.ends_with(&[0xff, 0xd9]));
    assert!(!server.agent_tabs.read().await.contains_key("feat"));
}

#[tokio::test]
async fn render_map_defaults_to_jpg_file_output() {
    let server = handler_with_project(map_project()).await;
    let dir = std::env::temp_dir().join(format!("libregene-mcp-map-jpg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("map.jpg");
    let out = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            output_path: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["format"], "jpg");
    assert_eq!(out["mimeType"], "image/jpeg");
    assert!(out.get("svg").is_none(), "{out}");
    assert!(out.get("jpg").is_none(), "{out}");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(out["bytesWritten"], bytes.len());
    assert!(bytes.starts_with(&[0xff, 0xd8, 0xff]));
    assert!(bytes.ends_with(&[0xff, 0xd9]));
    std::fs::remove_dir_all(dir).ok();
}
