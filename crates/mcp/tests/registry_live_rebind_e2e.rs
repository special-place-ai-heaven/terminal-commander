// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC42b end-to-end smoke through MCP: activate a rule WHILE a
//! command is already running and prove the activation reaches
//! future frames from that same running command. Then deactivate
//! and prove future matches stop.
//!
//! Uses `python3 -u -c '...'` as the slow line emitter so the test
//! has measurable temporal windows around the mid-run activation.
//! `python3` is not in the shell-bridge deny list; it is exec'd
//! directly as an argv program, not via a shell.

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
    p.push(format!("tc-mcp-rebind-{tag}-{pid}-{nanos}-{n}"));
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
    // Path-based availability check so this test file does not
    // contain a `Command::new(...)` (the daemon-level grep that
    // proves the MCP crate never spawns directly should not flag
    // a test fixture).
    for candidate in ["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"] {
        if std::path::Path::new(candidate).exists() {
            return true;
        }
    }
    false
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
        "description": "tc42b live rebind",
        "pattern": null,
        "keywords": [keyword],
        "captures": [],
        "summary_template": "matched keyword",
        "tags": ["tc42b"],
        "rate_limit_per_min": null,
        "redact": [],
        "context_hint": {"before_lines": 0, "after_lines": 0},
        "examples": []
    }))
    .expect("rule json")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn activate_while_command_runs_drives_signal_then_deactivate_silences_it() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let data = tmp_data_dir("midrun");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Upsert the rule but leave it inactive.
        let _ = call_tool(
            &client,
            "registry_upsert",
            serde_json::json!({
                "definition_json": keyword_rule_json("tc42b-kw", "midrun-token", "midrun_match"),
            }),
        )
        .await;

        // Long-running emitter: prints "midrun-token" eight times,
        // 250 ms apart. Total ~2 seconds. Plenty of window for a
        // mid-run activation + deactivation cycle.
        let py = r#"
import sys, time
for i in range(8):
    print("midrun-token", flush=True)
    time.sleep(0.25)
"#;
        let start = first_text(
            &call_tool(
                &client,
                "command_start_combed",
                serde_json::json!({
                    "argv": ["python3", "-u", "-c", py],
                    "grace_ms": 5000
                }),
            )
            .await,
        );
        let start_v: serde_json::Value = serde_json::from_str(&start).expect("start json");
        let bucket_id = start_v["bucket_id"].as_str().unwrap().to_owned();

        // Phase 1: drain bucket for 500 ms WITHOUT the rule active.
        // No "midrun_match" event may appear.
        let pre = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_id,
                    "cursor": 0,
                    "timeout_ms": 500
                }),
            )
            .await,
        );
        let pre_v: serde_json::Value = serde_json::from_str(&pre).expect("pre json");
        let pre_events = pre_v["events"].as_array().cloned().unwrap_or_default();
        for e in &pre_events {
            assert_ne!(
                e["kind"].as_str(),
                Some("midrun_match"),
                "pre-activation must not produce midrun_match; got: {pre}"
            );
        }
        let pre_cursor = pre_v["next_cursor"].as_u64().unwrap_or(0);

        // Phase 2: activate the rule while the command is still
        // running. The TC42b path triggers rebind_all_jobs on the
        // activate IPC handler, which swaps the running job's
        // sifter in place.
        let _ = call_tool(
            &client,
            "registry_activate",
            serde_json::json!({"rule_id": "tc42b-kw", "scope": {"kind": "global"}}),
        )
        .await;

        // Phase 3: drain bucket for ~900 ms after activation. The
        // running command should fire `midrun_match` events on
        // subsequent emitted lines.
        let post = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_id,
                    "cursor": pre_cursor,
                    "timeout_ms": 900
                }),
            )
            .await,
        );
        let post_v: serde_json::Value = serde_json::from_str(&post).expect("post json");
        let post_events = post_v["events"].as_array().cloned().unwrap_or_default();
        assert!(
            post_events
                .iter()
                .any(|e| e["kind"].as_str() == Some("midrun_match")),
            "mid-run activation must produce a midrun_match event from the running command; payload: {post}"
        );
        let after_active_cursor = post_v["next_cursor"].as_u64().unwrap_or(pre_cursor);

        // Phase 4: deactivate while still running.
        let _ = call_tool(
            &client,
            "registry_deactivate",
            serde_json::json!({"rule_id": "tc42b-kw", "version": 1, "scope": {"kind": "global"}}),
        )
        .await;

        // Phase 5: drain another window. No new midrun_match events
        // should appear. We accept that events fired between the
        // deactivate IPC arriving and the next stdout line may
        // still be in flight; we therefore wait a small grace
        // window to let any racing line through, then require the
        // FINAL bucket_wait window to be midrun_match-free.
        // (Lifecycle events are fine; the command may still exit
        // here.)
        tokio::time::sleep(Duration::from_millis(150)).await;
        let drain = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_id,
                    "cursor": after_active_cursor,
                    "timeout_ms": 400
                }),
            )
            .await,
        );
        let drain_v: serde_json::Value = serde_json::from_str(&drain).expect("drain json");
        let drain_cursor = drain_v["next_cursor"]
            .as_u64()
            .unwrap_or(after_active_cursor);

        // Final read window after the grace + drain pass. This is
        // the strict assertion: at this point the deactivate has
        // had time to propagate; no further midrun_match events.
        let final_payload = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_id,
                    "cursor": drain_cursor,
                    "timeout_ms": 800
                }),
            )
            .await,
        );
        let final_v: serde_json::Value = serde_json::from_str(&final_payload).expect("final json");
        let final_events = final_v["events"].as_array().cloned().unwrap_or_default();
        for e in &final_events {
            assert_ne!(
                e["kind"].as_str(),
                Some("midrun_match"),
                "post-deactivation final window must not produce midrun_match; payload: {final_payload}"
            );
        }

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
