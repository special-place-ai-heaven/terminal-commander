// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC40 integration smoke: exercise the rmcp stdio adapter through a
//! duplex transport (no real stdin/stdout, no real daemon). Verifies:
//!
//! - initialize handshake completes,
//! - tool list contains all live tools at the current TC level,
//! - calling a tool against an unreachable daemon returns a typed
//!   error rather than panicking or producing raw output.
//!
//! This avoids spawning the daemon process; it points the adapter at
//! a UDS path that does not exist and lets the IPC client return its
//! typed connect-failure error. The MCP layer's job at TC40 is to
//! surface that error structurally — that's what we assert.

#![cfg(unix)]

use std::path::PathBuf;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ServiceExt};

use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;

#[derive(Default, Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

fn nonexistent_socket() -> PathBuf {
    std::env::temp_dir().join(format!(
        "tc-mcp-test-no-such-socket-{}-{}",
        std::process::id(),
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ))
}

async fn paired_service() -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let socket = nonexistent_socket();
    let daemon = McpDaemonClient::new(socket).with_timeout(Duration::from_millis(150));
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialize_and_list_tools_returns_full_live_set() {
    let (_server, client) = paired_service().await;
    let info = client.peer_info().expect("peer info").clone();
    assert_eq!(
        info.server_info.name, "terminal-commander-mcp",
        "server identity"
    );

    let tools = client.list_all_tools().await.expect("list tools");
    let mut names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "bucket_events_since".to_owned(),
            "bucket_summary".to_owned(),
            "bucket_wait".to_owned(),
            "command_output_tail".to_owned(),
            "command_start_combed".to_owned(),
            "command_status".to_owned(),
            "event_context".to_owned(),
            "file_read_window".to_owned(),
            "file_search".to_owned(),
            "file_watch_list".to_owned(),
            "file_watch_start".to_owned(),
            "file_watch_stop".to_owned(),
            "health".to_owned(),
            "policy_status".to_owned(),
            "probe_list".to_owned(),
            "probe_status".to_owned(),
            "pty_command_list".to_owned(),
            "pty_command_start".to_owned(),
            "pty_command_stop".to_owned(),
            "pty_command_write_stdin".to_owned(),
            "registry_activate".to_owned(),
            "registry_deactivate".to_owned(),
            "registry_get".to_owned(),
            "registry_import_pack".to_owned(),
            "registry_list_active".to_owned(),
            "registry_search".to_owned(),
            "registry_test".to_owned(),
            "registry_upsert".to_owned(),
            "run_and_watch".to_owned(),
            "runtime_state".to_owned(),
            "self_check".to_owned(),
            "system_discover".to_owned(),
        ],
        "TC45 must advertise the full TC44 set plus aggregate runtime view"
    );

    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_against_missing_daemon_returns_typed_error_not_panic() {
    let (_server, client) = paired_service().await;
    let params = CallToolRequestParams::new("health");
    let err = client
        .call_tool(params)
        .await
        .expect_err("expected typed error when daemon is unreachable");
    let rendered = err.to_string();
    assert!(
        rendered.contains("daemon ipc error") || rendered.contains("connect"),
        "error should surface the daemon connect failure, got: {rendered}"
    );
    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn system_discover_returns_payload_even_when_daemon_is_unreachable() {
    let (_server, client) = paired_service().await;
    let params = CallToolRequestParams::new("system_discover");
    let result = client
        .call_tool(params)
        .await
        .expect("system_discover call");
    assert!(
        !result.content.is_empty(),
        "system_discover should always return a payload"
    );
    let _ = client.cancel().await;
}
