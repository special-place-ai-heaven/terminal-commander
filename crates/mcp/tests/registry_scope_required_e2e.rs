// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC42d MCP-level rejection test: registry_activate / registry_deactivate
//! WITHOUT a `scope` field must surface as an MCP error (mapped from
//! the daemon's `ScopeInvalid`). The LLM never gets a silent
//! widen-to-Global.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ServiceExt};

use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commanderd::{DaemonConfig, DaemonState, IpcServer, ServerHandle};

#[derive(Default, Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

fn tmp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-mcp-scope-req-{tag}-{pid}-{nanos}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn spawn_live_daemon(data: &std::path::Path) -> ServerHandle {
    let cfg = DaemonConfig::defaults_in(data);
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("daemon bootstrap"));
    let socket = state.config.socket_path();
    let server = IpcServer::new(Arc::clone(&state), socket);
    server.spawn().expect("ipc server spawn")
}

async fn paired_against_live_daemon(
    handle: &ServerHandle,
) -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let daemon = McpDaemonClient::new(handle.socket_path().to_path_buf())
        .with_timeout(Duration::from_secs(5));
    let server = TerminalCommanderMcpServer::new(daemon);
    let server_handle =
        tokio::spawn(async move { server.serve(server_transport).await.expect("server serve") });
    let client = TestClient
        .serve(client_transport)
        .await
        .expect("client serve");
    let server = server_handle.await.expect("server task join");
    (server, client)
}

fn keyword_rule_json(id: &str, keyword: &str, event_kind: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id,
        "version": 1,
        "kind": "keyword",
        "status": "active",
        "severity": "medium",
        "event_kind": event_kind,
        "stream": null,
        "description": "tc42d scope required",
        "pattern": null,
        "keywords": [keyword],
        "captures": [],
        "summary_template": "matched keyword",
        "tags": ["tc42d"],
        "rate_limit_per_min": null,
        "redact": [],
        "context_hint": {"before_lines": 0, "after_lines": 0},
        "examples": []
    }))
    .expect("rule json")
}

async fn call_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
    name: &'static str,
    arguments: serde_json::Value,
) -> Result<rmcp::model::CallToolResult, rmcp::ServiceError> {
    let mut params = CallToolRequestParams::new(name);
    if let serde_json::Value::Object(map) = arguments {
        params.arguments = Some(map);
    }
    client.call_tool(params).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_activate_without_scope_returns_error() {
    let data = tmp_data_dir("act");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Upsert is a prerequisite for the rule_id lookup inside the
        // activate handler. Use a non-error path here so the test is
        // unambiguous about WHY the activate fails.
        let _ = call_tool(
            &client,
            "registry_upsert",
            serde_json::json!({
                "definition_json": keyword_rule_json("tc42d-kw", "needle", "kw_match"),
            }),
        )
        .await
        .expect("upsert ok");

        // Omit scope. Daemon must reject with ScopeInvalid -> MCP
        // invalid_params.
        let err = call_tool(
            &client,
            "registry_activate",
            serde_json::json!({"rule_id": "tc42d-kw"}),
        )
        .await
        .expect_err("missing scope must error");
        let msg = format!("{err}");
        let lower = msg.to_ascii_lowercase();
        assert!(
            lower.contains("scope")
                && (lower.contains("required") || lower.contains("scope_invalid")),
            "expected scope-required style error; got: {msg}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_deactivate_without_scope_returns_error() {
    let data = tmp_data_dir("deact");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        let err = call_tool(
            &client,
            "registry_deactivate",
            serde_json::json!({"rule_id": "anything", "version": 1}),
        )
        .await
        .expect_err("missing scope must error");
        let msg = format!("{err}");
        let lower = msg.to_ascii_lowercase();
        assert!(
            lower.contains("scope")
                && (lower.contains("required") || lower.contains("scope_invalid")),
            "expected scope-required style error; got: {msg}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
