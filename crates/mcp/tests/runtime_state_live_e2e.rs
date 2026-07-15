// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC45 end-to-end MCP smoke for the aggregate runtime view.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
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
    p.push(format!("tc-mcp-rt-{tag}-{pid}-{nanos}-{n}"));
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::similar_names)]
async fn runtime_state_aggregates_two_sources_through_mcp() {
    let data = tmp_data_dir("agg");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Source 1: a command_start_combed sleeper.
        let _ = call_tool(
            &client,
            "command_start_combed",
            serde_json::json!({
                "argv": ["sleep", "1"],
                "grace_ms": 2000
            }),
        )
        .await;

        // Source 2: a file_watch over a freshly created file.
        std::fs::create_dir_all(&data).unwrap();
        let log = data.join("watch.log");
        std::fs::write(&log, "preexisting\n").unwrap();
        let watch_payload = first_text(
            &call_tool(
                &client,
                "file_watch_start",
                serde_json::json!({"path": log.to_string_lossy()}),
            )
            .await,
        );
        let watch_v: serde_json::Value = serde_json::from_str(&watch_payload).expect("watch json");
        let watch_probe_id = watch_v["probe_id"].as_str().unwrap().to_owned();

        // runtime_state must show both.
        let payload = first_text(&call_tool(&client, "runtime_state", serde_json::json!({})).await);
        let v: serde_json::Value = serde_json::from_str(&payload).expect("runtime_state json");
        assert!(
            v["command_jobs"].as_u64().unwrap_or(0) >= 1,
            "expected >=1 command job; payload: {payload}"
        );
        assert!(
            v["file_watches"].as_u64().unwrap_or(0) >= 1,
            "expected >=1 file watch; payload: {payload}"
        );
        let probes = v["probes"].as_array().expect("probes array");
        assert!(probes.len() >= 2, "probes: {probes:?}");

        // probe_list returns the same set.
        let pl = first_text(&call_tool(&client, "probe_list", serde_json::json!({})).await);
        let pl_v: serde_json::Value = serde_json::from_str(&pl).expect("probe_list json");
        assert!(pl_v["probes"].as_array().unwrap().len() >= 2);

        // probe_status by id resolves the file-watch probe.
        let ps = first_text(
            &call_tool(
                &client,
                "probe_status",
                serde_json::json!({"probe_id": watch_probe_id}),
            )
            .await,
        );
        let ps_v: serde_json::Value = serde_json::from_str(&ps).expect("probe_status json");
        assert_eq!(ps_v["probe"]["kind"].as_str(), Some("file_watch"));

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_and_watch_threads_tag_and_redacts_argv_head_through_mcp() {
    let data = tmp_data_dir("rw-tag");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Keep the command live while runtime_state observes its metadata.
        let helper = data.join("slow-command.py");
        std::fs::write(
            &helper,
            "#!/usr/bin/env python3\nimport time\ntime.sleep(5)\n",
        )
        .expect("write helper");
        let mut permissions = std::fs::metadata(&helper)
            .expect("helper metadata")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&helper, permissions).expect("make helper executable");
        let start_payload = first_text(
            &call_tool(
                &client,
                "run_and_watch",
                serde_json::json!({
                    "argv": [helper.to_string_lossy(), "--password", "E2E-SECRET-XYZ"],
                    "tag": "e2e-4b-tag",
                    "wait_ms": 100
                }),
            )
            .await,
        );
        let start: serde_json::Value =
            serde_json::from_str(&start_payload).expect("run_and_watch json");
        let job_id = start["job_id"].as_str().expect("run_and_watch job_id");

        // Poll runtime_state until a command probe registers (avoid any
        // registration race between the spawn and the live-map insert).
        let mut command_probe: Option<serde_json::Value> = None;
        for _ in 0..10 {
            let payload =
                first_text(&call_tool(&client, "runtime_state", serde_json::json!({})).await);
            let v: serde_json::Value = serde_json::from_str(&payload).expect("runtime_state json");
            if let Some(probes) = v["probes"].as_array()
                && let Some(p) = probes
                    .iter()
                    .find(|p| p["kind"].as_str() == Some("command"))
            {
                command_probe = Some(p.clone());
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let probe = command_probe.expect("a command probe must appear in runtime_state");

        // The tag threads end-to-end through run_and_watch -> command_start.
        assert_eq!(
            probe["tag"].as_str(),
            Some("e2e-4b-tag"),
            "tag must thread through run_and_watch; probe: {probe}"
        );

        // The argv head is bounded + redacted: the secret value is masked, the
        // program and the flag NAME stay visible, and the raw secret never
        // appears.
        let argv_head = probe["argv_head"]
            .as_array()
            .expect("command probe must carry an argv_head")
            .iter()
            .map(|s| s.as_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !argv_head.contains("E2E-SECRET-XYZ"),
            "raw secret leaked into argv_head: {argv_head}"
        );
        assert!(
            argv_head.contains("<redacted>"),
            "argv_head must mask the secret span: {argv_head}"
        );
        assert!(
            argv_head.contains("slow-command.py"),
            "argv_head must keep the program name: {argv_head}"
        );
        assert!(
            argv_head.contains("--password"),
            "argv_head must keep the flag name: {argv_head}"
        );

        let _ = call_tool(
            &client,
            "command_stop",
            serde_json::json!({"job_id": job_id}),
        )
        .await;

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_status_unknown_probe_returns_error_through_mcp() {
    let data = tmp_data_dir("unknown");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let err = client
            .call_tool({
                let mut p = CallToolRequestParams::new("probe_status");
                p.arguments = Some(
                    serde_json::json!({
                        "probe_id": terminal_commander_core::ProbeId::new().to_wire_string(),
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                );
                p
            })
            .await
            .expect_err("unknown probe must error");
        let msg = format!("{err}").to_ascii_lowercase();
        assert!(
            msg.contains("unknown_probe") || msg.contains("not live"),
            "expected unknown-probe error; got: {err}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
