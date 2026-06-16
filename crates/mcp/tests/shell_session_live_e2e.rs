// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! P1 / TC50 end-to-end smoke for the persistent shell-session MCP surface.
//!
//! Stands up the real `terminal-commanderd` UDS server in a temp dir,
//! mounts the rmcp stdio adapter on a duplex transport pointed at it, and
//! exercises the session flow entirely through MCP:
//!
//! O-02 (cap ON): with `profile = full_access` the `allow_session`
//! capability is preset, so `shell_session_start` is `AllowWithAudit`. The
//! agent then runs `cd /tmp` and `pwd` in the session; the combed signal
//! reports `/tmp` WITHOUT the agent re-passing cwd (sticky cwd). The MCP
//! responses carry bounded combed signals / receipts, never a raw stream.
//!
//! Default-deny (cap OFF): on the default `developer_local` profile the
//! `allow_session` capability defaults false, so `shell_session_start` is
//! denied at the `SessionStart` policy gate and surfaces a denied/policy
//! error through MCP.
//!
//! Mirrors the live-daemon harness in `shell_live_e2e.rs`. The MCP adapter
//! NEVER spawns — every call forwards over IPC to the daemon.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ServiceExt};

use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commanderd::{DaemonConfig, DaemonState, IpcServer, PolicyProfile, ServerHandle};

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
    p.push(format!("tc-mcp-session-e2e-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn bash_available() -> bool {
    std::path::Path::new("/bin/bash").exists()
}

fn spawn_live_daemon(data: &std::path::Path) -> ServerHandle {
    let cfg = DaemonConfig::defaults_in(data);
    spawn_with_config(cfg)
}

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

/// O-02 through MCP: with `allow_session` ON (full_access), the agent
/// starts a session, runs `cd /tmp` then `pwd`, and the combed signal
/// reports `/tmp` WITHOUT re-passing cwd. Responses are combed, never raw.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)] // cohesive end-to-end O-02 flow through MCP
async fn o02_session_cd_then_pwd_reports_tmp_through_mcp() {
    if !bash_available() {
        eprintln!("skipping: /bin/bash not present");
        return;
    }
    let data = tmp_data_dir("o02-cap-on");
    let handle = spawn_live_daemon_full_access(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Start a session with an inline rule firing on lines containing
        // `tmp`, so the combed `pwd` output `/tmp` becomes a signal.
        let start_payload = first_text(
            &call_tool(
                &client,
                "shell_session_start",
                // Default summary_template is the matched line, so a hit on
                // the `pwd` output `/tmp` surfaces `/tmp` in the summary.
                serde_json::json!({
                    "rules": [{
                        "pattern": "tmp",
                        "severity": "info",
                        "event_kind": "cwd"
                    }]
                }),
            )
            .await,
        );
        let start: serde_json::Value =
            serde_json::from_str(&start_payload).expect("start payload is JSON");
        let session_id = start["session_id"]
            .as_str()
            .expect("session_id present")
            .to_owned();
        assert!(
            !session_id.is_empty(),
            "non-empty session id: {start_payload}"
        );
        // Combed, never raw: no stdout/stderr success fields.
        assert!(start.get("stdout").is_none());

        // `cd /tmp` (silent).
        let _ = call_tool(
            &client,
            "shell_session_exec",
            serde_json::json!({ "session_id": session_id, "line": "cd /tmp", "wait_ms": 800 }),
        )
        .await;

        // `pwd` then poll exec on the advancing cursor until `/tmp` appears.
        let mut found = false;
        let mut cursor = 0u64;
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut first = true;
        while std::time::Instant::now() < deadline {
            let line = if first { "pwd" } else { "" };
            first = false;
            let payload = first_text(
                &call_tool(
                    &client,
                    "shell_session_exec",
                    serde_json::json!({
                        "session_id": session_id,
                        "line": line,
                        "cursor": cursor,
                        "wait_ms": 800
                    }),
                )
                .await,
            );
            let v: serde_json::Value =
                serde_json::from_str(&payload).expect("exec payload is JSON");
            // Combed signal shape, never raw stream.
            assert!(v.get("stdout").is_none(), "no raw stdout: {payload}");
            cursor = v["next_cursor"].as_u64().unwrap_or(cursor);
            if v["events"].as_array().is_some_and(|events| {
                events
                    .iter()
                    .any(|e| e["summary"].as_str().is_some_and(|s| s.contains("/tmp")))
            }) {
                found = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            found,
            "combed session signal must report /tmp from `pwd` after `cd /tmp` (O-02, no cwd re-pass)"
        );

        // Status reports the sticky cwd.
        let status_payload = first_text(
            &call_tool(
                &client,
                "shell_session_status",
                serde_json::json!({ "session_id": session_id }),
            )
            .await,
        );
        let status: serde_json::Value =
            serde_json::from_str(&status_payload).expect("status payload is JSON");
        assert_eq!(
            status["cwd"].as_str(),
            Some("/tmp"),
            "status cwd: {status_payload}"
        );

        // Graceful stop.
        let stop_payload = first_text(
            &call_tool(
                &client,
                "shell_session_stop",
                serde_json::json!({ "session_id": session_id }),
            )
            .await,
        );
        let stop: serde_json::Value =
            serde_json::from_str(&stop_payload).expect("stop payload is JSON");
        assert_eq!(
            stop["state"].as_str(),
            Some("exited"),
            "stop state: {stop_payload}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

/// Default-deny: on the default profile `allow_session` is OFF, so
/// `shell_session_start` is denied at the `SessionStart` policy gate and
/// MCP surfaces a denied/policy error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_start_denied_on_default_profile_e2e() {
    let data = tmp_data_dir("default-deny");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        let mut params = CallToolRequestParams::new("shell_session_start");
        params.arguments = serde_json::json!({}).as_object().cloned();
        let err = client
            .call_tool(params)
            .await
            .expect_err("shell_session_start must be denied when allow_session is off");

        let rendered = err.to_string().to_ascii_lowercase();
        assert!(
            rendered.contains("denied") || rendered.contains("policy"),
            "default-profile session denial must surface a denied/policy message; got: {rendered}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
