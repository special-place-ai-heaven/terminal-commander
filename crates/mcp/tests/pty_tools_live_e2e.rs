// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC44 end-to-end MCP smoke for PTY tools.

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
    p.push(format!("tc-mcp-pty-{tag}-{pid}-{nanos}-{n}"));
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

fn python3_available() -> bool {
    for candidate in ["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"] {
        if std::path::Path::new(candidate).exists() {
            return true;
        }
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pty_command_full_lifecycle_through_mcp() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let data = tmp_data_dir("full");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Start a simple interactive python script that echoes any
        // line we send to stdin.
        let py = r#"
import sys
for line in sys.stdin:
    sys.stdout.write("echoed: " + line)
    sys.stdout.flush()
"#;
        let start = first_text(
            &call_tool(
                &client,
                "pty_command_start",
                serde_json::json!({
                    "argv": ["python3", "-u", "-c", py],
                }),
            )
            .await,
        );
        let start_v: serde_json::Value = serde_json::from_str(&start).expect("start json");
        let job_id = start_v["job_id"].as_str().unwrap().to_owned();

        // Non-secret stdin works.
        let _ = call_tool(
            &client,
            "pty_command_write_stdin",
            serde_json::json!({"job_id": job_id, "bytes": "hello\n"}),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(400)).await;

        // Stop and capture metrics.
        let stop_payload = first_text(
            &call_tool(
                &client,
                "pty_command_stop",
                serde_json::json!({"job_id": job_id}),
            )
            .await,
        );
        let stop_v: serde_json::Value = serde_json::from_str(&stop_payload).expect("stop json");
        assert_eq!(stop_v["job_id"].as_str().unwrap(), job_id);
        assert!(
            stop_v["stdin_bytes_written"].as_u64().unwrap_or(0) > 0,
            "stdin bytes must be recorded; got {stop_payload}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pty_secret_prompt_denies_stdin_through_mcp() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let data = tmp_data_dir("secret");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        let py = r#"
import sys, time
sys.stdout.write("[sudo] password for dev: ")
sys.stdout.flush()
time.sleep(2)
"#;
        let start = first_text(
            &call_tool(
                &client,
                "pty_command_start",
                serde_json::json!({
                    "argv": ["python3", "-u", "-c", py],
                }),
            )
            .await,
        );
        let start_v: serde_json::Value = serde_json::from_str(&start).expect("start json");
        let job_id = start_v["job_id"].as_str().unwrap().to_owned();

        // Wait for the prompt to land.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Attempt to write the password. MUST be rejected.
        let err = client
            .call_tool({
                let mut p = CallToolRequestParams::new("pty_command_write_stdin");
                p.arguments = Some(
                    serde_json::json!({
                        "job_id": job_id,
                        "bytes": "super-secret-password\n",
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                );
                p
            })
            .await
            .expect_err("secret prompt must reject stdin");
        let msg = format!("{err}").to_ascii_lowercase();
        assert!(
            msg.contains("secret") || msg.contains("secret_input_denied"),
            "expected secret-input-denied error; got: {err}"
        );

        let _ = call_tool(
            &client,
            "pty_command_stop",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pty_shell_interpreter_denied_through_mcp() {
    let data = tmp_data_dir("shell-deny");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let mut params = CallToolRequestParams::new("pty_command_start");
        params.arguments = Some(
            serde_json::json!({"argv": ["bash"]})
                .as_object()
                .cloned()
                .unwrap(),
        );
        let err = client
            .call_tool(params)
            .await
            .expect_err("bash must be rejected");
        let msg = format!("{err}").to_ascii_lowercase();
        assert!(
            msg.contains("shell") || msg.contains("shell_interpreter_denied"),
            "expected shell-interpreter-denied; got: {err}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
