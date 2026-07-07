// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC49 end-to-end smoke for the gated `shell_exec` MCP tool surface.
//!
//! Stands up the real `terminal-commanderd` UDS server in a temp dir,
//! mounts the rmcp stdio adapter on a duplex transport pointed at it,
//! and exercises the shell lane entirely through MCP:
//!
//! O-01 (cap ON): with `profile = full_access` the `allow_shell`
//! capability is preset, so `shell_exec { shell_line: "echo a | wc -c" }`
//! is `AllowWithAudit` and the daemon spawns `[shell,"-lc",shell_line]`
//! through the same comb/bucket pipeline. The response carries bounded
//! start metadata (job_id/bucket_id/probe_id/cursor) — a combed signal /
//! receipt — and NEVER a raw stdout dump of the pipeline output.
//!
//! Default profile: `developer_local` grants one-shot shell by default, so
//! the same shell lane works out of the box while still skipping raw stdout
//! dumps and routing output through the comb/bucket pipeline.
//!
//! Mirrors the live-daemon harness in `mcp_live_command_e2e.rs`.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ServiceExt};

use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commanderd::{
    DaemonConfig, DaemonState, IpcServer, MAX_RESPONSE_BYTES, PolicyProfile, ServerHandle,
};

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
    p.push(format!("tc-mcp-shell-e2e-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

/// Live daemon on the DEFAULT profile (`developer_local`): one-shot shell is
/// enabled by default, so the shell lane should run through the comb pipeline.
fn spawn_live_daemon(data: &std::path::Path) -> ServerHandle {
    let cfg = DaemonConfig::defaults_in(data);
    spawn_with_config(cfg)
}

/// Live daemon on the `full_access` profile: the loader preset flips
/// every cap (including `allow_shell`) ON, so the shell lane is allowed
/// with audit. Caps are config/TOML only — never an MCP/IPC flag.
fn spawn_live_daemon_full_access(data: &std::path::Path) -> ServerHandle {
    let mut cfg = DaemonConfig::defaults_in(data);
    cfg.policy.profile = PolicyProfile::FullAccess;
    spawn_with_config(cfg)
}

fn spawn_with_config(cfg: DaemonConfig) -> ServerHandle {
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

/// O-01: with the `allow_shell` capability ON (via `full_access`), a
/// pipeline runs through the shell lane and MCP returns a combed signal
/// / bounded receipt — NOT a raw stdout dump of the pipeline output.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn o01_pipeline_returns_signal_when_cap_on() {
    let data = tmp_data_dir("o01-cap-on");
    let handle = spawn_live_daemon_full_access(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // `echo a | wc -c` prints "2" on stdout. The shell lane must
        // return only bounded start metadata; the literal pipeline
        // output must NOT appear in the MCP response text.
        let payload = first_text(
            &call_tool(
                &client,
                "shell_exec",
                serde_json::json!({
                    "shell_line": "echo a | wc -c",
                    "wait_ms": 2000,
                }),
            )
            .await,
        );
        assert!(
            payload.len() <= MAX_RESPONSE_BYTES,
            "shell_exec payload must respect IPC response budget"
        );

        let v: serde_json::Value =
            serde_json::from_str(&payload).expect("shell_exec payload is JSON");

        // Combed signal / receipt present: the daemon allocated a job
        // and bucket for the spawned pipeline.
        assert!(
            v.get("job_id").is_some() || v.get("receipt").is_some(),
            "shell_exec must return a combed signal (job_id) or receipt; got: {payload}"
        );
        assert!(
            v["job_id"].as_str().is_some_and(|s| !s.is_empty()),
            "job_id must be a non-empty opaque id; got: {payload}"
        );

        // Combed, never raw: no stdout/stderr success fields, and the
        // raw pipeline output is not dumped into the response text.
        assert!(
            v.get("stdout").is_none(),
            "shell_exec response must not carry a stdout field; got: {payload}"
        );
        assert!(
            v.get("stderr").is_none(),
            "shell_exec response must not carry a stderr field; got: {payload}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

/// Default profile: `developer_local` grants one-shot shell, so the shell lane
/// runs through policy and returns a combed bounded response out of the box.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_exec_allowed_on_default_profile_e2e() {
    let data = tmp_data_dir("default-allow");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        let payload = first_text(
            &call_tool(
                &client,
                "shell_exec",
                serde_json::json!({
                    "shell_line": "echo hi | wc -c",
                    "wait_ms": 2000,
                }),
            )
            .await,
        );
        assert!(
            payload.len() <= MAX_RESPONSE_BYTES,
            "default shell_exec payload must respect IPC response budget"
        );

        let v: serde_json::Value =
            serde_json::from_str(&payload).expect("shell_exec payload is JSON");
        assert!(
            v["job_id"].as_str().is_some_and(|s| !s.is_empty()),
            "default shell_exec must return a non-empty job_id; got: {payload}"
        );
        assert!(
            v.get("stdout").is_none(),
            "shell_exec response must not carry a stdout field; got: {payload}"
        );
        assert!(
            v.get("stderr").is_none(),
            "shell_exec response must not carry a stderr field; got: {payload}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
