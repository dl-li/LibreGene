//! MCP server lifecycle (`McpConfig`/`McpServer`) + axum HTTP serving.

// ---------------------------------------------------------------------------
// Server bootstrap + settings (start/stop/restart without app restart)
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session,
};
use tauri::{AppHandle, Runtime};
use tokio::sync::RwLock;

use libregene_core::project::ProjectManager;

use super::auth::{generate_auth_token, load_or_create_token, persist_token, token_eq};
use super::workspace::Workspace;
use super::{LibreGeneMcp, MCP_PORT};

/// Runtime MCP server configuration. The frontend persists the source of truth
/// in localStorage and pushes it here via `set_mcp_config` on startup and on
/// every settings change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McpConfig {
    pub enabled: bool,
    pub port: u16,
    /// When false the bearer-token check is skipped (a token may still be
    /// sent; it is simply ignored). The Host check always applies.
    pub require_auth: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: MCP_PORT,
            require_auth: true,
        }
    }
}

/// Owns the MCP server task. `set_config` stops/restarts the loopback server
/// in place so the settings toggle takes effect without an app restart.
pub struct McpServer<R: Runtime> {
    app_handle: AppHandle<R>,
    pm: Arc<RwLock<ProjectManager>>,
    wp: Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: crate::AgentTabs,
    workspace: Workspace,
    config: Arc<StdMutex<McpConfig>>,
    task: Arc<StdMutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    /// Bearer token required on every MCP request so that other local
    /// processes (or a browser via DNS rebinding) can't drive the MCP tools.
    /// Persisted to `<app_config_dir>/mcp_auth_token` so it survives app
    /// restarts; only regenerated when the user explicitly asks. Exposed to
    /// the trusted frontend via `get_mcp_token` / `regenerate_mcp_token`.
    auth_token: Arc<StdMutex<String>>,
    /// Called when the server's effective state changes outside a
    /// `set_config` call (currently: bind failure) so the caller (tray /
    /// frontend status) can reflect the real state instead of "running".
    status: Arc<dyn Fn(bool, u16) + Send + Sync>,
}

impl<R: Runtime> Clone for McpServer<R> {
    fn clone(&self) -> Self {
        Self {
            app_handle: self.app_handle.clone(),
            pm: self.pm.clone(),
            wp: self.wp.clone(),
            agent_tabs: self.agent_tabs.clone(),
            workspace: self.workspace.clone(),
            config: self.config.clone(),
            task: self.task.clone(),
            auth_token: self.auth_token.clone(),
            status: self.status.clone(),
        }
    }
}

impl<R: Runtime> McpServer<R> {
    pub fn new(
        app_handle: AppHandle<R>,
        pm: Arc<RwLock<ProjectManager>>,
        wp: Arc<RwLock<HashMap<String, String>>>,
        agent_tabs: crate::AgentTabs,
        workspace: Workspace,
        status: impl Fn(bool, u16) + Send + Sync + 'static,
    ) -> Self {
        let auth_token = load_or_create_token(&app_handle);
        Self {
            app_handle,
            pm,
            wp,
            agent_tabs,
            workspace,
            config: Arc::new(StdMutex::new(McpConfig::default())),
            task: Arc::new(StdMutex::new(None)),
            auth_token: Arc::new(StdMutex::new(auth_token)),
            status: Arc::new(status),
        }
    }

    /// The bearer token the trusted frontend must send to use the MCP server.
    pub fn auth_token(&self) -> String {
        self.auth_token.lock().unwrap().clone()
    }

    /// Generate and persist a fresh bearer token. Takes effect immediately for
    /// the running server (the auth middleware reads the shared token per
    /// request), so no restart is needed.
    pub fn regenerate_auth_token(&self) -> String {
        let token = generate_auth_token();
        *self.auth_token.lock().unwrap() = token.clone();
        persist_token(&self.app_handle, &token);
        token
    }

    pub fn config(&self) -> McpConfig {
        *self.config.lock().unwrap()
    }

    /// Update the config and restart the server only when enabled/port
    /// changed. `require_auth` is read live by the auth middleware, so
    /// toggling it never restarts the server (and never touches the token).
    pub async fn set_config(
        &self,
        enabled: bool,
        port: u16,
        require_auth: bool,
    ) -> Result<McpConfig, String> {
        if port == 0 {
            return Err(format!("Invalid port: {port} (must be 1-65535)"));
        }
        let changed = {
            let mut c = self.config.lock().unwrap();
            let changed = c.enabled != enabled || c.port != port;
            c.enabled = enabled;
            c.port = port;
            c.require_auth = require_auth;
            changed
        };
        if changed {
            self.apply().await;
        }
        Ok(self.config())
    }

    /// Reconcile the running server with the current config: stop any existing
    /// task, then start one if enabled.
    pub async fn apply(&self) {
        let cfg = self.config();
        let old = self.task.lock().unwrap().take();
        if let Some(handle) = old {
            handle.abort();
        }
        if cfg.enabled {
            let app = self.app_handle.clone();
            let pm = self.pm.clone();
            let wp = self.wp.clone();
            let agent_tabs = self.agent_tabs.clone();
            let workspace = self.workspace.clone();
            let port = cfg.port;
            let token = self.auth_token.clone();
            let config = self.config.clone();
            let status = self.status.clone();
            let serve_config = self.config.clone();
            let handle = tauri::async_runtime::spawn(async move {
                if let Err(e) = serve_mcp(app.clone(), pm, wp, agent_tabs, workspace, port, token, serve_config).await
                {
                    log::error!("MCP server error on port {}: {}", port, e);
                    // Give-up (e.g. the port is held by another app): the
                    // server never came up. Flip the config so get_mcp_config
                    // and the tray stop claiming it is running; guarded so a
                    // concurrent re-enable/relocate via set_config isn't
                    // clobbered — a stale task must neither flip the newer
                    // config nor report its own failure as the tray status.
                    let gave_up = {
                        let mut cfg = config.lock().unwrap();
                        if cfg.enabled && cfg.port == port {
                            cfg.enabled = false;
                            true
                        } else {
                            false
                        }
                    };
                    if gave_up {
                        status(false, port);
                    }
                }
            });
            *self.task.lock().unwrap() = Some(handle);
        }
    }
}

/// Build a JSON-RPC error HTTP response (`Content-Type: application/json`).
/// Used by the request middleware so protocol-level failures (401/406/404)
/// carry a readable, structured body instead of rmcp's bare status text.
fn jsonrpc_error_response(
    status: axum::http::StatusCode,
    id: Option<serde_json::Value>,
    code: i64,
    message: &str,
) -> axum::response::Response {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(serde_json::Value::Null),
        "error": { "code": code, "message": message },
    });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    axum::http::Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(bytes))
        .expect("valid response")
}

/// Extract the JSON-RPC request id from a raw body so error responses can
/// echo it back; None (rendered as `id: null`) when the body isn't parseable
/// or carries no id.
fn jsonrpc_id_from_body(bytes: &[u8]) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    match v {
        serde_json::Value::Object(o) => o.get("id").cloned(),
        serde_json::Value::Array(a) => a.first().and_then(|e| e.get("id").cloned()),
        _ => None,
    }
}

pub(crate) async fn serve_mcp<R: Runtime>(
    app_handle: AppHandle<R>,
    pm: Arc<RwLock<ProjectManager>>,
    wp: Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: crate::AgentTabs,
    workspace: Workspace,
    port: u16,
    auth_token: Arc<StdMutex<String>>,
    config: Arc<StdMutex<McpConfig>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let expected_host = format!("127.0.0.1:{}", port);

    // Session durability: rmcp's default SessionConfig.keep_alive closes a
    // session after 5 minutes of inactivity. An LLM agent can pause longer
    // than that between tool calls (the 2026-08 MCP test lost sessions this
    // way through a 4 s keep-alive local proxy). Sessions are keyed in
    // memory, not bound to a TCP connection — a dropped connection does not
    // close them; only explicit DELETE, the idle timeout, or app exit does.
    // Extend the idle timeout to 24 h. Trade-off: abandoned sessions linger
    // until the app exits (bounded in practice — a desktop app holds a
    // handful), which is why rmcp's own docs advise against disabling the
    // timeout entirely on long-running public servers.
    let mut session_manager = session::local::LocalSessionManager::default();
    session_manager.session_config.keep_alive = Some(Duration::from_secs(24 * 60 * 60));

    // SSE keep-alive pings every 3 s (rmcp default is 15 s): long-lived SSE
    // streams (GET notification channels) stay busy enough that aggressive
    // local proxies with short idle timeouts (e.g. 4 s on 127.0.0.1:7890)
    // don't drop them mid-stream.
    let mut server_config = StreamableHttpServerConfig::default();
    server_config.sse_keep_alive = Some(Duration::from_secs(3));

    let service = StreamableHttpService::new(
        move || {
            Ok(LibreGeneMcp::new(
                app_handle.clone(),
                pm.clone(),
                wp.clone(),
                agent_tabs.clone(),
                workspace.clone(),
            ))
        },
        Arc::new(session_manager),
        server_config,
    );

    // Middleware: (1) require a local bearer token AND a matching Host header
    // (the token stops other local processes / a browser page via DNS
    // rebinding from driving the MCP tools; the Host check blocks
    // cross-origin/rebinding requests that don't target 127.0.0.1:<port>);
    // (2) reject missing/wrong Accept headers with a JSON-RPC error body
    // instead of rmcp's bare 406; (3) rewrite rmcp's plain-text 404
    // "Session not found" into a structured JSON-RPC error (code -32001) so
    // clients can tell the session expired and must re-initialize. The
    // request body is buffered (bounded, same 4 MiB limit as rmcp) only to
    // echo the JSON-RPC id back in error bodies; success responses are
    // passed through untouched (their SSE bodies must never be consumed).
    const MCP_BODY_LIMIT: usize = 4 * 1024 * 1024;
    let auth_token_for_layer = auth_token.clone();
    let expected_host_for_layer = expected_host.clone();
    let config_for_layer = config.clone();
    let auth_layer = axum::middleware::from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| {
            let auth_token = auth_token_for_layer.clone();
            let expected_host = expected_host_for_layer.clone();
            let config = config_for_layer.clone();
            async move {
                let host_ok = req
                    .headers()
                    .get(axum::http::header::HOST)
                    .and_then(|h| h.to_str().ok())
                    .map(|h| h == expected_host.as_str())
                    .unwrap_or(false);
                // Read the shared config per request so toggling token
                // verification takes effect without restarting the server.
                let auth_required = config
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .require_auth;
                let bearer_ok = !auth_required
                    || req
                        .headers()
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|h| h.to_str().ok())
                        // Read the shared token per request so a user-triggered
                        // regeneration takes effect without restarting the server.
                        // Compared in constant time; the lock tolerates poisoning
                        // (a panicking request handler must not lock out auth).
                        .map(|h| {
                            h.strip_prefix("Bearer ")
                                .map(|t| {
                                    token_eq(t, &auth_token.lock().unwrap_or_else(|e| e.into_inner()))
                                })
                                .unwrap_or(false)
                        })
                        .unwrap_or(false);
                if !(host_ok && bearer_ok) {
                    let message = if auth_required {
                        "Unauthorized: every MCP request must include 'Authorization: Bearer <token>' and 'Host: 127.0.0.1:<port>'"
                    } else {
                        "Unauthorized: every MCP request must use 'Host: 127.0.0.1:<port>'"
                    };
                    return jsonrpc_error_response(
                        axum::http::StatusCode::UNAUTHORIZED,
                        None,
                        -32000,
                        message,
                    );
                }

                let (parts, body) = req.into_parts();
                let bytes = match axum::body::to_bytes(body, MCP_BODY_LIMIT).await {
                    Ok(b) => b,
                    Err(_) => {
                        return jsonrpc_error_response(
                            axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                            None,
                            -32000,
                            "Request body too large",
                        );
                    }
                };
                let req_id = jsonrpc_id_from_body(&bytes);

                // Mirror rmcp's Accept requirement (it otherwise answers with
                // a bare 406 and no readable body).
                let accept_ok = if parts.method == axum::http::Method::GET {
                    parts
                        .headers
                        .get(axum::http::header::ACCEPT)
                        .and_then(|h| h.to_str().ok())
                        .is_some_and(|h| h.contains("text/event-stream"))
                } else if parts.method == axum::http::Method::POST {
                    parts
                        .headers
                        .get(axum::http::header::ACCEPT)
                        .and_then(|h| h.to_str().ok())
                        .is_some_and(|h| h.contains("application/json") && h.contains("text/event-stream"))
                } else {
                    true
                };
                if !accept_ok {
                    return jsonrpc_error_response(
                        axum::http::StatusCode::NOT_ACCEPTABLE,
                        req_id,
                        -32600,
                        "Not Acceptable: MCP Streamable HTTP requires an Accept header — POST /mcp needs 'Accept: application/json, text/event-stream', GET needs 'Accept: text/event-stream'",
                    );
                }

                let req = axum::http::Request::from_parts(parts, axum::body::Body::from(bytes));
                let resp = next.run(req).await;
                if resp.status() == axum::http::StatusCode::NOT_FOUND {
                    let (rparts, rbody) = resp.into_parts();
                    match axum::body::to_bytes(rbody, 64 * 1024).await {
                        Ok(rbytes) => {
                            let text = String::from_utf8_lossy(&rbytes);
                            if text.contains("Session not found") {
                                return jsonrpc_error_response(
                                    axum::http::StatusCode::NOT_FOUND,
                                    req_id,
                                    -32001,
                                    "Session not found: the MCP session has expired or was closed (e.g. the connection was dropped by a proxy or an idle timeout). Call initialize again to create a new session.",
                                );
                            }
                            return axum::http::Response::from_parts(
                                rparts,
                                axum::body::Body::from(rbytes),
                            );
                        }
                        Err(_) => {
                            return axum::http::Response::from_parts(rparts, axum::body::Body::empty())
                        }
                    }
                }
                resp
            }
        },
    );

    let router = axum::Router::new()
        .route("/mcp", axum::routing::any_service(service.clone()))
        .fallback_service(service)
        .layer(auth_layer);

    // Retry briefly on AddrInUse so a restart that races the previous
    // instance's socket release still binds; give up (with a log trail) after
    // ~2 s so a port held by another app doesn't spin forever.
    const MAX_BIND_ATTEMPTS: u32 = 40;
    let mut attempt = 0u32;
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                let auth = if config.lock().map(|c| c.require_auth).unwrap_or(true) {
                    "auth enabled"
                } else {
                    "auth disabled"
                };
                log::info!("MCP server listening on http://{addr}/mcp ({auth})");
                return axum::serve(listener, router).await.map_err(Into::into);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                attempt += 1;
                if attempt >= MAX_BIND_ATTEMPTS {
                    log::error!(
                        "MCP server: {addr} still in use after {MAX_BIND_ATTEMPTS} bind attempts; giving up"
                    );
                    return Err(Box::new(e));
                }
                log::warn!("MCP server: {addr} in use, retrying ({attempt}/{MAX_BIND_ATTEMPTS})");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(e) => return Err(Box::new(e)),
        }
    }
}
