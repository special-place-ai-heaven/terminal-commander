// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Subscriptions Phase 1 end-to-end MCP smoke. Drives the full stack
//! (rmcp client -> MCP server -> daemon IPC) for `subscription_open` /
//! `subscription_pull` / `subscription_close`.
//!
//! - AC13 (MCP trust path): an idle `subscription_pull` over the MCP facade
//!   returns SUCCESS empty events + liveness within the timeout, NEVER a
//!   -32603 (the pull tool uses a dedicated long-poll client).
//! - Unknown sub_id -> `invalid_params` with an `unknown_subscription` code.

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
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-mcp-sub-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn spawn_live_daemon(data: &std::path::Path) -> ServerHandle {
    let cfg = DaemonConfig::defaults_in(data);
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("daemon bootstrap"));
    let socket = state.config.socket_path();
    IpcServer::new(Arc::clone(&state), socket)
        .spawn()
        .expect("ipc server spawn")
}

async fn paired_against_live_daemon(
    handle: &ServerHandle,
) -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    // Build with the SAME 5 s normal client the production main.rs uses; the
    // pull tool derives its own 12 s long-poll client inside `new()`. If the
    // pull tool wrongly used this 5 s client, the AC13 idle pull would hit
    // -32603.
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

fn first_text(result: &rmcp::model::CallToolResult) -> String {
    for item in &result.content {
        if let Some(text) = item.as_text() {
            return text.text.clone();
        }
    }
    panic!("tool result had no text content: {result:?}");
}

async fn call_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
    name: &'static str,
    arguments: serde_json::Value,
) -> rmcp::model::CallToolResult {
    let mut params = CallToolRequestParams::new(name);
    if let serde_json::Value::Object(map) = arguments {
        params.arguments = Some(map);
    }
    client
        .call_tool(params)
        .await
        .unwrap_or_else(|e| panic!("call_tool({name}) failed: {e}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac13_idle_pull_over_mcp_is_success_empty_plus_liveness_not_32603() {
    let data = tmp_data_dir("ac13");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Open a subscription over all sources (nothing running -> idle).
        let open_payload = first_text(
            &call_tool(
                &client,
                "subscription_open",
                serde_json::json!({
                    "severity_min": "high",
                    "sources": { "kind": "all" }
                }),
            )
            .await,
        );
        let open_v: serde_json::Value = serde_json::from_str(&open_payload).expect("open json");
        let sub_id = open_v["sub_id"]
            .as_str()
            .expect("sub_id present")
            .to_owned();
        assert!(
            open_v["boot_id"].as_str().is_some(),
            "open must surface a boot_id: {open_payload}"
        );

        // An idle pull with a sub-second timeout must SUCCEED (empty events +
        // liveness). The key AC13 point — that a long (~8 s) idle pull does
        // NOT surface a -32603 — is enforced by the dedicated 12 s long-poll
        // client; here we additionally drive a longer idle pull to prove the
        // client timeout is above the server cap.
        let pull_payload = first_text(
            &call_tool(
                &client,
                "subscription_pull",
                serde_json::json!({ "sub_id": sub_id, "timeout_ms": 200 }),
            )
            .await,
        );
        let pull_v: serde_json::Value = serde_json::from_str(&pull_payload).expect("pull json");
        assert!(
            pull_v["events"]
                .as_array()
                .is_some_and(std::vec::Vec::is_empty),
            "idle pull returns empty events: {pull_payload}"
        );
        // US4 / FR-031: the adapter requests the liveness delta, so on a
        // `sources: all` sub with NO in-scope buckets the section is legitimately
        // OMITTED (an empty delta). When present it is still an array.
        assert!(
            pull_v
                .get("liveness")
                .is_none_or(serde_json::Value::is_array),
            "liveness, when present, is an array: {pull_payload}"
        );

        // A near-server-cap idle pull (close to 8 s) must STILL be SUCCESS,
        // never -32603. (The 12 s client tolerates it.)
        let long_idle = first_text(
            &call_tool(
                &client,
                "subscription_pull",
                serde_json::json!({ "sub_id": sub_id, "timeout_ms": 8000 }),
            )
            .await,
        );
        let long_v: serde_json::Value = serde_json::from_str(&long_idle).expect("long pull json");
        assert!(
            long_v["events"]
                .as_array()
                .is_some_and(std::vec::Vec::is_empty),
            "AC13: ~8 s idle pull is SUCCESS empty, never -32603: {long_idle}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pull_unknown_sub_id_maps_to_invalid_params_unknown_subscription() {
    let data = tmp_data_dir("unknown");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let err = client
            .call_tool({
                let mut p = CallToolRequestParams::new("subscription_pull");
                p.arguments = Some(
                    serde_json::json!({
                        "sub_id": uuid::Uuid::new_v4().to_string(),
                        "timeout_ms": 200
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                );
                p
            })
            .await
            .expect_err("unknown sub_id must error");
        let msg = format!("{err}").to_ascii_lowercase();
        assert!(
            msg.contains("unknown_subscription") || msg.contains("unknown subscription"),
            "expected unknown_subscription error; got: {err}"
        );
        // It must be a caller-fixable invalid_params (-32602), NOT -32603.
        assert!(
            !msg.contains("-32603") && !msg.contains("internal"),
            "unknown sub_id must be invalid_params, not internal: {err}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
