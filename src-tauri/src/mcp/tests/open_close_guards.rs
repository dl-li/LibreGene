use super::common::*;
use crate::mcp::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use tauri::test::{mock_builder, mock_context, noop_assets};
use libregene_core::project::ProjectManager;

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
