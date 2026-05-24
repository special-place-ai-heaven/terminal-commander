// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC40 integration test: daemon_unavailable envelope (Task 6 / TC40).
//!
//! Verifies that when the MCP adapter is started with a
//! `DaemonStatusHandle` that reports `Unavailable`, every daemon-requiring
//! tool call returns a structured `daemon_unavailable` MCP error envelope
//! rather than a raw transport-level connection error.
//!
//! Daemon-free tools (`health`, `system_discover`, `policy_status`,
//! `self_check`) are NOT expected to short-circuit; they still try the
//! daemon and may return their own typed errors.
//!
//! Uses the same in-process duplex transport as `mcp_stdio.rs` — no
//! real daemon, no process spawn, no unix socket required.
//!
//! API divergence note: `list_all_tools()` is used (not
//! `list_tools(Default::default())`), matching the existing test suite.

#![cfg(unix)]

use std::path::PathBuf;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ServiceExt};

use terminal_commander_mcp::daemon_client::{DaemonStatusHandle, McpDaemonClient};
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commander_supervisor::ensure::{
    DaemonUnavailableReason, Diagnostics, Endpoint, EnsureDaemonStatus,
};

#[derive(Default, Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

fn nonexistent_socket() -> PathBuf {
    std::env::temp_dir().join(format!(
        "tc-mcp-unavail-test-{}-{}",
        std::process::id(),
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ))
}

fn make_unavailable_status() -> EnsureDaemonStatus {
    let socket = nonexistent_socket();
    EnsureDaemonStatus::Unavailable {
        reason: DaemonUnavailableReason::BinaryNotFound,
        diagnostics: Diagnostics {
            endpoint: Endpoint::UnixSocket { path: socket },
            log_path: None,
            last_error: Some("test: binary not found".into()),
            startup_attempted: false,
            startup_elapsed_ms: 0,
        },
    }
}

/// Build a paired server/client where the server is configured with an
/// `Unavailable` daemon status handle.
async fn paired_service_unavailable() -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let socket = nonexistent_socket();
    let status = DaemonStatusHandle::new(make_unavailable_status());
    let daemon = McpDaemonClient::with_status(socket, status)
        .with_timeout(Duration::from_millis(150));
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

// ---------------------------------------------------------------------------
// Test: initialize + tool list still works with unavailable status
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialize_and_list_tools_works_when_daemon_unavailable() {
    let (_server, client) = paired_service_unavailable().await;
    let info = client.peer_info().expect("peer info").clone();
    assert_eq!(
        info.server_info.name, "terminal-commander-mcp",
        "server identity must be correct even when daemon is unavailable"
    );

    // Tool list must be complete — the guard does not remove tools, it
    // returns errors at call time.
    let tools = client.list_all_tools().await.expect("list tools");
    assert!(
        !tools.is_empty(),
        "tools/list must return the full tool set even when daemon is unavailable"
    );
    let _ = client.cancel().await;
}

// ---------------------------------------------------------------------------
// Test: daemon-requiring tool returns daemon_unavailable envelope
// ---------------------------------------------------------------------------

/// Assert a single daemon-requiring tool call returns the
/// `daemon_unavailable` structured error envelope (not a panic, not a raw
/// transport error).
async fn assert_daemon_unavailable_envelope(client: &rmcp::service::RunningService<rmcp::RoleClient, TestClient>, tool: &str) {
    let params = CallToolRequestParams::new(tool.to_owned());
    let err = client
        .call_tool(params)
        .await
        .expect_err(&format!("expected error from {tool} when daemon unavailable"));
    let rendered = err.to_string();
    assert!(
        rendered.contains("daemon_unavailable"),
        "tool `{tool}` must return daemon_unavailable envelope, got: {rendered}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_start_combed_returns_daemon_unavailable() {
    let (_server, client) = paired_service_unavailable().await;
    // command_start_combed requires `argv` — supply minimal valid args so that
    // rmcp's Parameters macro can deserialize them before the guard fires.
    let args: rmcp::model::JsonObject = serde_json::from_value(serde_json::json!({
        "argv": ["ls"]
    }))
    .expect("json args");
    let params = rmcp::model::CallToolRequestParams::new("command_start_combed".to_owned())
        .with_arguments(args);
    let err = client
        .call_tool(params)
        .await
        .expect_err("expected error from command_start_combed when daemon unavailable");
    let rendered = err.to_string();
    assert!(
        rendered.contains("daemon_unavailable"),
        "tool `command_start_combed` must return daemon_unavailable envelope, got: {rendered}"
    );
    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_state_returns_daemon_unavailable() {
    let (_server, client) = paired_service_unavailable().await;
    assert_daemon_unavailable_envelope(&client, "runtime_state").await;
    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_list_returns_daemon_unavailable() {
    let (_server, client) = paired_service_unavailable().await;
    assert_daemon_unavailable_envelope(&client, "probe_list").await;
    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registry_list_active_returns_daemon_unavailable() {
    let (_server, client) = paired_service_unavailable().await;
    assert_daemon_unavailable_envelope(&client, "registry_list_active").await;
    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_watch_list_returns_daemon_unavailable() {
    let (_server, client) = paired_service_unavailable().await;
    assert_daemon_unavailable_envelope(&client, "file_watch_list").await;
    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pty_command_list_returns_daemon_unavailable() {
    let (_server, client) = paired_service_unavailable().await;
    assert_daemon_unavailable_envelope(&client, "pty_command_list").await;
    let _ = client.cancel().await;
}

// ---------------------------------------------------------------------------
// Test: daemon-free tools do NOT short-circuit (they still try daemon)
// ---------------------------------------------------------------------------

/// `system_discover` never calls the daemon; it must succeed even when
/// the status handle reports Unavailable.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn system_discover_succeeds_when_daemon_unavailable() {
    let (_server, client) = paired_service_unavailable().await;
    let params = CallToolRequestParams::new("system_discover");
    let result = client
        .call_tool(params)
        .await
        .expect("system_discover must succeed even when daemon is unavailable");
    assert!(
        !result.content.is_empty(),
        "system_discover must return a non-empty payload"
    );
    let _ = client.cancel().await;
}

/// `health` calls the daemon but is NOT guarded — it returns a daemon
/// connect error (not a daemon_unavailable envelope). Verify it does NOT
/// return a `daemon_unavailable` message (that would mean the guard leaked
/// into a daemon-free tool).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_does_not_return_daemon_unavailable_envelope() {
    let (_server, client) = paired_service_unavailable().await;
    let params = CallToolRequestParams::new("health");
    // health should error (no real daemon), but NOT with daemon_unavailable.
    let err = client
        .call_tool(params)
        .await
        .expect_err("health must error when daemon is unreachable");
    let rendered = err.to_string();
    assert!(
        !rendered.contains("daemon_unavailable"),
        "health must NOT return daemon_unavailable envelope (it is a daemon-free \
         tool that errors via the IPC path), got: {rendered}"
    );
    let _ = client.cancel().await;
}
