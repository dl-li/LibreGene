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
async fn render_map_returns_inline_jpg_rejects_svg_and_relocks_agent_tab() {
    let server = handler_with_project(map_project()).await;
    server
        .agent_tabs
        .write()
        .await
        .insert("feat".to_string(), crate::AgentTabMeta { locked: false });
    let out = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["view"], "circular");
    assert_eq!(out["featureCount"], 1);
    assert_eq!(out["format"], "jpg");
    assert_eq!(out["mimeType"], "image/jpeg");
    assert!(out["width"].as_f64().unwrap() > 0.0);
    assert!(out["height"].as_f64().unwrap() > 0.0);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(out["jpg"].as_str().unwrap())
        .unwrap();
    assert!(bytes.starts_with(&[0xff, 0xd8, 0xff]));
    assert!(bytes.ends_with(&[0xff, 0xd9]));
    assert!(out.get("svg").is_none(), "{out}");
    assert!(out.get("path").is_none(), "{out}");
    assert!(server.agent_tabs.read().await["feat"].locked);

    let svg = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            format: Some("svg".to_string()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(svg["ok"], false, "{svg}");
    assert!(svg["message"]
        .as_str()
        .unwrap()
        .contains("only outputs JPG"));
}

#[tokio::test]
async fn render_map_writes_atomically_and_requires_overwrite() {
    let server = handler_with_project(map_project()).await;
    let dir = std::env::temp_dir().join(format!("libregene-mcp-map-{}", std::process::id()));
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
    assert!(out.get("jpg").is_none(), "{out}");
    assert_eq!(
        out["path"],
        crate::normalize_project_path(path.to_str().unwrap()).unwrap()
    );
    let jpg = std::fs::read(&path).unwrap();
    assert_eq!(out["bytesWritten"], jpg.len());
    assert!(jpg.starts_with(&[0xff, 0xd8, 0xff]));
    assert!(jpg.ends_with(&[0xff, 0xd9]));

    let refused = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            output_path: Some(path.to_string_lossy().into_owned()),
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
    assert_eq!(std::fs::read(&path).unwrap(), jpg);

    let overwritten = server
        .render_map(Parameters(RenderMapRequest {
            project_id: "feat".to_string(),
            output_path: Some(path.to_string_lossy().into_owned()),
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
            ..Default::default()
        }))
        .await
        .unwrap()
        .0;
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["view"], "linear");
    assert_eq!(out["format"], "jpg");
    assert!(out["jpg"].as_str().is_some_and(|jpg| !jpg.is_empty()));
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
