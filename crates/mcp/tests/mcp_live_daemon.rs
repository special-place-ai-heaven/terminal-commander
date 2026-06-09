// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC40 amended smoke (live daemon path).
//!
//! Stands up a real `terminal-commanderd` UDS IPC server in a temp
//! directory, mounts the rmcp stdio adapter on a duplex transport
//! pointed at it, and verifies the live tool round trip:
//!
//! - MCP `initialize` succeeds.
//! - `list_tools` returns the full tool set (38 live tools: TC45 + registry_import_pack + command_stop).
//! - `health` forwards through UDS and returns a payload that decodes
//!   to a real `uptime_secs` field (i.e. the daemon answered).
//! - `system_discover` forwards through UDS, the `daemon` field is
//!   populated (daemon reachable), and the response stays under the
//!   IPC response budget.
//!
//! Bounded-output invariant: every assertion uses the existing
//! `MAX_RESPONSE_BYTES` cap exposed from `terminal_commanderd`.
//!
//! No command spawn, no network listener, no raw stream — the daemon
//! runtime does not wire those at TC40 and this test does not try to
//! introduce them.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ServiceExt};

use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commanderd::{DaemonConfig, DaemonState, IpcServer, MAX_RESPONSE_BYTES, ServerHandle};

#[derive(Default, Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-mcp-live-{tag}-{pid}-{nanos}-{n}"));
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
        .with_timeout(Duration::from_secs(2));
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
async fn live_health_roundtrip_through_uds() {
    let data = tmp_data_dir("health");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

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
                "command_stop".to_owned(),
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
                "subscription_close".to_owned(),
                "subscription_list".to_owned(),
                "subscription_open".to_owned(),
                "subscription_pull".to_owned(),
                "subscription_seek".to_owned(),
                "system_discover".to_owned(),
            ],
            "live daemon must expose the full TC45 tool set + subscriptions"
        );

        let result = client
            .call_tool(CallToolRequestParams::new("health"))
            .await
            .expect("health call should succeed against a live daemon");
        let payload = first_text_content(&result);
        assert!(
            payload.len() <= MAX_RESPONSE_BYTES,
            "health payload must stay within IPC response budget"
        );
        let body: serde_json::Value =
            serde_json::from_str(&payload).expect("health payload is JSON");
        assert_eq!(body["ok"], serde_json::Value::Bool(true));
        assert!(
            body["uptime_secs"].is_number(),
            "health payload must include numeric uptime_secs; got {body}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_system_discover_roundtrip_reports_daemon() {
    let data = tmp_data_dir("discover");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        let result = client
            .call_tool(CallToolRequestParams::new("system_discover"))
            .await
            .expect("system_discover should succeed");
        let payload = first_text_content(&result);
        assert!(
            payload.len() <= MAX_RESPONSE_BYTES,
            "system_discover payload must stay within IPC response budget"
        );

        let body: serde_json::Value =
            serde_json::from_str(&payload).expect("system_discover payload is JSON");
        assert!(
            body.get("daemon").is_some_and(|d| !d.is_null()),
            "daemon field must be populated when daemon is reachable; got {body}"
        );
        assert!(
            body.get("daemon_error")
                .is_none_or(serde_json::Value::is_null),
            "daemon_error must be null when daemon is reachable; got {body}"
        );
        let methods = body["daemon"]["methods"]
            .as_array()
            .expect("daemon.methods must be an array");
        let method_names: Vec<String> = methods
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        for required in ["system_discover", "health", "policy_status", "self_check"] {
            assert!(
                method_names.iter().any(|m| m == required),
                "daemon.methods must advertise {required}; got {method_names:?}"
            );
        }
        let tools = body["tools"].as_array().expect("tools array");
        let live_count = tools
            .iter()
            .filter(|t| t["status"].as_str() == Some("live"))
            .count();
        assert_eq!(
            live_count, 38,
            "tool catalogue must list exactly 38 live tools (33 + subscription_open/pull/list/close/seek)"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

fn first_text_content(result: &rmcp::model::CallToolResult) -> String {
    for item in &result.content {
        if let Some(text) = item.as_text() {
            return text.text.clone();
        }
    }
    panic!("tool result had no text content: {result:?}");
}
