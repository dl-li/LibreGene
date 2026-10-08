use super::common::*;
use crate::mcp::*;
use std::time::Duration;

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
        assert!(server.set_config(true, 0, true).await.is_err());
        // rejected change must not alter the stored config
        assert_eq!(server.config().port, MCP_PORT);
    }

    #[tokio::test]
    async fn server_starts_stops_and_restarts_on_port_change() {
        let server = test_server();
        let token = server.auth_token();

        // start on a fresh port
        server.set_config(true, 19999, true).await.unwrap();
        assert!(wait_up(19999, &token).await, "server should be up on 19999");

        // disable → port released
        server.set_config(false, 19999, true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!handshake_ok(19999, &token).await, "server should be down after disable");

        // re-enable on a new port without an app restart
        server.set_config(true, 20001, true).await.unwrap();
        assert!(wait_up(20001, &token).await, "server should be up on 20001 after restart");
        assert!(!handshake_ok(19999, &token).await, "old port must stay free");

        // unchanged config → no restart churn
        server.set_config(true, 20001, true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(handshake_ok(20001, &token).await, "server must survive a no-op set_config");

        // clean up
        server.set_config(false, 20001, true).await.unwrap();
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
        server.set_config(true, 21007, true).await.unwrap();
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
    async fn connections_beyond_the_cap_are_dropped() {
        let server = test_server();
        let token = server.auth_token();
        server.set_config(true, 20011, false).await.unwrap();
        assert!(wait_up(20011, &token).await, "server should be up");

        // Hold every slot with idle slowloris-style sockets (connected, no
        // bytes sent). handshake_ok used one and released it, so reopen all.
        let mut idle = Vec::new();
        for _ in 0..MAX_MCP_CONNECTIONS {
            if let Ok(s) = tokio::net::TcpStream::connect(("127.0.0.1", 20011)).await {
                idle.push(s);
            }
        }
        // Give the accept loop a moment to take all the slots.
        tokio::time::sleep(Duration::from_millis(300)).await;

        // One connection past the cap must be closed by the server promptly:
        // the read resolves (rather than timing out) with an error/EOF.
        let mut extra = tokio::net::TcpStream::connect(("127.0.0.1", 20011)).await.unwrap();
        use tokio::io::AsyncReadExt as _;
        let n = tokio::time::timeout(Duration::from_secs(3), extra.read_u8()).await;
        assert!(n.is_ok(), "over-cap connection was left hanging");
        assert!(n.unwrap().is_err(), "expected EOF/error on the over-cap connection");

        // Releasing the idle sockets frees slots again.
        drop(idle);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            handshake_ok(20011, &token).await,
            "server must serve again once idle connections are gone"
        );
        server.set_config(false, 20011, false).await.unwrap();
    }

    #[tokio::test]
    async fn regenerated_token_takes_effect_without_restart() {
        let server = test_server();
        server.set_config(true, 20003, true).await.unwrap();
        let old = server.auth_token();
        assert!(wait_up(20003, &old).await, "server should be up with the initial token");

        let new = server.regenerate_auth_token();
        assert_ne!(old, new);
        // the running server must accept the new token and reject the old one
        assert!(handshake_ok(20003, &new).await, "new token should be accepted");
        assert!(!handshake_ok(20003, &old).await, "old token should be rejected");

        server.set_config(false, 20003, true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn auth_toggle_applies_live_and_ignores_tokens_when_off() {
        let server = test_server();
        let token = server.auth_token();
        server.set_config(true, 20008, true).await.unwrap();
        assert!(wait_up(20008, &token).await, "server should be up with auth on");
        assert!(!handshake_anon(20008).await, "no-token request must be rejected while auth is on");
        assert!(!handshake_ok(20008, "wrong-token").await, "wrong token must be rejected while auth is on");

        // Toggling auth off must not restart the server nor rotate the token.
        server.set_config(true, 20008, false).await.unwrap();
        assert_eq!(server.auth_token(), token, "toggle must not regenerate the token");
        assert!(handshake_anon(20008).await, "no-token request must be accepted while auth is off");
        assert!(
            handshake_ok(20008, "wrong-token").await,
            "a bogus bearer token must be ignored while auth is off"
        );
        assert!(handshake_ok(20008, &token).await, "the real token must still work while auth is off");

        // Back on: the token gate applies again without a restart.
        server.set_config(true, 20008, true).await.unwrap();
        assert!(!handshake_anon(20008).await, "no-token request must be rejected again");
        assert!(handshake_ok(20008, &token).await, "server must still be up with the same token");

        server.set_config(false, 20008, true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn missing_accept_header_returns_structured_406() {
        let server = test_server();
        let token = server.auth_token();
        server.set_config(true, 20005, true).await.unwrap();
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

        server.set_config(false, 20005, true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn unknown_session_returns_structured_404() {
        let server = test_server();
        let token = server.auth_token();
        server.set_config(true, 20006, true).await.unwrap();
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

        server.set_config(false, 20006, true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
