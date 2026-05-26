// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Integration test: `daemon_unavailable` envelope coverage.
//!
//! Verifies that when the MCP adapter is started with a
//! `DaemonStatusHandle` that reports `Unavailable`, **every** daemon-backed
//! tool call returns a structured `daemon_unavailable` MCP error envelope
//! rather than a raw transport-level connection error.
//!
//! Coverage is table-driven: the test iterates the live `tool_catalogue()`
//! and exercises all 28 daemon-backed tools (every tool except
//! `system_discover`). `minimal_tool_args` supplies the minimal required
//! arguments per tool so rmcp's `Parameters` deserialization succeeds and the
//! availability guard — which fires before any argument parsing — is the code
//! path under test. Driving off the catalogue means new tools are covered
//! automatically and cannot drift out of this guarantee.
//!
//! `system_discover` is the only daemon-independent tool; it has its own test
//! asserting it stays callable and labels daemon-backed tools honestly.
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
use terminal_commander_mcp::tools::{TerminalCommanderMcpServer, tool_catalogue};
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
    let daemon =
        McpDaemonClient::with_status(socket, status).with_timeout(Duration::from_millis(150));
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

fn first_text(result: &rmcp::model::CallToolResult) -> String {
    for item in &result.content {
        if let Some(text) = item.as_text() {
            return text.text.clone();
        }
    }
    panic!("expected text content in call result");
}

/// Minimal valid arguments for each daemon-backed tool, sufficient to pass
/// rmcp `Parameters` deserialization so the daemon-status guard — not a
/// schema-validation error — is what fires. IDs need not be semantically
/// valid: every handler checks `daemon.status().is_unavailable()` before it
/// parses any typed id.
fn minimal_tool_args(tool: &str) -> serde_json::Value {
    match tool {
        "command_start_combed" | "pty_command_start" => serde_json::json!({ "argv": ["ls"] }),
        "command_status" | "pty_command_stop" => serde_json::json!({ "job_id": "job_x" }),
        "pty_command_write_stdin" => serde_json::json!({ "job_id": "job_x", "bytes": "x" }),
        "bucket_events_since" | "bucket_wait" => {
            serde_json::json!({ "bucket_id": "bkt_x", "cursor": 0 })
        }
        "bucket_summary" => serde_json::json!({ "bucket_id": "bkt_x" }),
        "event_context" => serde_json::json!({ "bucket_id": "bkt_x", "event_id": "evt_x" }),
        "registry_search" => serde_json::json!({ "query": "x" }),
        "registry_get" | "registry_activate" => serde_json::json!({ "rule_id": "rule_x" }),
        "registry_upsert" => serde_json::json!({ "definition_json": "{}" }),
        "registry_test" => serde_json::json!({ "rule_id": "rule_x", "samples": [] }),
        "registry_deactivate" => serde_json::json!({ "rule_id": "rule_x", "version": 1 }),
        "file_read_window" | "file_watch_start" => {
            serde_json::json!({ "path": "/tmp/tc-unavail" })
        }
        "file_search" => serde_json::json!({ "path": "/tmp/tc-unavail", "query": "q" }),
        "file_watch_stop" => serde_json::json!({ "watch_id": "job_x" }),
        "probe_status" => serde_json::json!({ "probe_id": "prb_x" }),
        // health, policy_status, self_check, *_list, runtime_state, probe_list,
        // registry_list_active take no required arguments.
        _ => serde_json::json!({}),
    }
}

/// Table-driven contract: every daemon-backed tool (all catalogue entries
/// except `system_discover`) must return the structured `daemon_unavailable`
/// envelope when the adapter starts with an `Unavailable` daemon status —
/// not a raw transport error and not a schema-validation error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_daemon_backed_tools_return_daemon_unavailable() {
    let (_server, client) = paired_service_unavailable().await;

    let mut offenders: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for entry in tool_catalogue() {
        let tool = entry.name;
        if tool == "system_discover" {
            continue;
        }
        checked += 1;

        let args: rmcp::model::JsonObject =
            serde_json::from_value(minimal_tool_args(tool)).expect("minimal args object");
        let params = CallToolRequestParams::new(tool.to_owned()).with_arguments(args);

        let Err(err) = client.call_tool(params).await else {
            offenders.push(format!("{tool}: call unexpectedly succeeded"));
            continue;
        };
        let rendered = err.to_string();
        if !rendered.contains("daemon_unavailable") {
            offenders.push(format!("{tool}: {rendered}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "tools that did not return a daemon_unavailable envelope: {offenders:#?}"
    );
    assert_eq!(
        checked, 28,
        "expected 28 daemon-backed tools (29 catalogue entries minus system_discover)"
    );

    let _ = client.cancel().await;
}

// ---------------------------------------------------------------------------
// Test: discovery stays callable and labels daemon-backed tools honestly
// ---------------------------------------------------------------------------

/// `system_discover` must succeed even if daemon IPC fails and report the
/// daemon as unavailable in its payload.
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
    let payload: serde_json::Value =
        serde_json::from_str(&first_text(&result)).expect("system_discover json payload");
    assert_eq!(
        payload["daemon_available"], false,
        "system_discover must make daemon availability explicit"
    );
    let tools = payload["tools"]
        .as_array()
        .expect("system_discover tools array");
    assert!(
        !tools.is_empty(),
        "system_discover must return the advertised tool catalogue"
    );

    for tool in tools {
        let name = tool["name"]
            .as_str()
            .expect("tool entry should include a name");
        let requires_daemon = name != "system_discover";
        assert_eq!(
            tool["requires_daemon"], requires_daemon,
            "{name} requires_daemon mismatch"
        );
        assert_eq!(
            tool["available"], !requires_daemon,
            "{name} availability mismatch when daemon is unavailable"
        );
        if requires_daemon {
            assert_eq!(
                tool["unavailable_reason"], "daemon_unavailable",
                "{name} should explain daemon unavailability"
            );
        } else {
            assert!(
                tool["unavailable_reason"].is_null(),
                "{name} should not have an unavailable reason"
            );
        }
    }
    let _ = client.cancel().await;
}
