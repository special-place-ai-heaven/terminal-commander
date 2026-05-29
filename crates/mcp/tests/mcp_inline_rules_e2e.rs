// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Live-daemon proof for Task 7 (Option B — expose `bucket_config` + inline
//! `rules` on the MCP `*_start` surfaces).
//!
//! `command_start_combed` and `file_watch_start` upsert nothing and activate
//! nothing: they pass a one-rule `rules_json` array directly to the start
//! tool, drive a workload that emits the rule's keyword, and assert the
//! rule-driven event surfaces in the job's bucket — i.e. the inline rule
//! takes effect with no prior `registry_activate`.
//!
//! `pty_command_start` asserts the same MCP -> IPC parity at the start
//! surface (inline `rules` + a per-job `bucket_config` map through and the
//! job starts); PTY frame-to-event sifting itself is covered by the daemon
//! pty tests. It skips when `python3` is unavailable.

#![cfg(unix)]

use std::io::Write as _;
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
    p.push(format!("tc-mcp-inline-rules-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn python3_available() -> bool {
    ["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"]
        .iter()
        .any(|c| std::path::Path::new(c).exists())
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
        .with_timeout(Duration::from_secs(3));
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

fn json(result: &rmcp::model::CallToolResult) -> serde_json::Value {
    serde_json::from_str(&first_text(result)).expect("tool payload is JSON")
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

/// A one-element `rules_json` array carrying a single keyword rule.
fn inline_rules_json(id: &str, keyword: &str, event_kind: &str) -> String {
    serde_json::to_string(&serde_json::json!([{
        "id": id,
        "version": 1,
        "kind": "keyword",
        "status": "active",
        "severity": "medium",
        "event_kind": event_kind,
        "stream": null,
        "description": "inline rule e2e",
        "pattern": null,
        "keywords": [keyword],
        "captures": [],
        "summary_template": "matched keyword",
        "tags": ["test", "inline"],
        "rate_limit_per_min": null,
        "redact": [],
        "context_hint": { "before_lines": 0, "after_lines": 0 },
        "examples": []
    }]))
    .expect("rules json")
}

fn has_needle_match(wait: &serde_json::Value) -> bool {
    wait["events"].as_array().is_some_and(|events| {
        events
            .iter()
            .any(|e| e["kind"].as_str() == Some("needle_match"))
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_start_combed_inline_rule_drives_signal_without_activation() {
    let data = tmp_data_dir("cmd");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let start = json(
            &call_tool(
                &client,
                "command_start_combed",
                serde_json::json!({
                    "argv": ["echo", "found needle in haystack"],
                    "grace_ms": 2000,
                    "rules_json": inline_rules_json("inline-cmd", "needle", "needle_match")
                }),
            )
            .await,
        );
        let bucket_id = start["bucket_id"].as_str().expect("bucket_id").to_owned();

        let wait = json(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({ "bucket_id": bucket_id, "cursor": 0, "timeout_ms": 2000 }),
            )
            .await,
        );
        assert!(
            has_needle_match(&wait),
            "inline rule on command_start_combed must drive a needle_match without registry_activate; got {wait}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_watch_start_inline_rule_drives_signal_without_activation() {
    let data = tmp_data_dir("watch");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let watched = data.join("inline-watch.log");
        std::fs::write(&watched, "preexisting\n").expect("seed watch file");

        let start = json(
            &call_tool(
                &client,
                "file_watch_start",
                serde_json::json!({
                    "path": watched.to_string_lossy(),
                    "follow_from_beginning": false,
                    "rules_json": inline_rules_json("inline-watch", "needle", "needle_match")
                }),
            )
            .await,
        );
        let bucket_id = start["bucket_id"].as_str().expect("bucket_id").to_owned();

        // Append matching content after the watch is live.
        tokio::time::sleep(Duration::from_millis(250)).await;
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&watched)
                .expect("open watch file");
            writeln!(f, "needle appears here").expect("append");
        }

        let wait = json(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({ "bucket_id": bucket_id, "cursor": 0, "timeout_ms": 3000 }),
            )
            .await,
        );
        assert!(
            has_needle_match(&wait),
            "inline rule on file_watch_start must drive a needle_match without registry_activate; got {wait}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pty_command_start_accepts_inline_rules_and_bucket_config() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let data = tmp_data_dir("pty");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        // command_start_combed / file_watch_start above prove inline rules
        // actually fire; PTY frame->event sifting is a daemon-internal concern
        // covered by the daemon pty tests. Here we assert the PTY *start
        // surface* honors the same MCP -> IPC parity: inline rules + a per-job
        // bucket_config map through without a schema or IPC rejection and the
        // job starts.
        let start = json(
            &call_tool(
                &client,
                "pty_command_start",
                serde_json::json!({
                    "argv": ["python3", "-u", "-c", "print('hi', flush=True)\n"],
                    "bucket_config_json": "{\"max_events\": 256, \"ttl\": 120}",
                    "rules_json": inline_rules_json("inline-pty", "needle", "needle_match")
                }),
            )
            .await,
        );
        let job_id = start["job_id"]
            .as_str()
            .unwrap_or_else(|| {
                panic!(
                    "pty_command_start must accept inline rules + bucket_config and return a job_id; got {start}"
                )
            })
            .to_owned();
        assert!(
            start["bucket_id"].as_str().is_some(),
            "pty_command_start must return a bucket_id; got {start}"
        );

        let _ = call_tool(
            &client,
            "pty_command_stop",
            serde_json::json!({ "job_id": job_id }),
        )
        .await;
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

// --- TC ergonomics Phase 2 (P4): run_and_watch one-shot ---

/// One call: typed shorthand `rules` + a noisy command returns the
/// matching signal AND the exit code, with no separate wait/status call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_and_watch_returns_signal_and_exit_in_one_call() {
    let data = tmp_data_dir("raw-signal");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let out = json(
            &call_tool(
                &client,
                "run_and_watch",
                serde_json::json!({
                    "argv": ["echo", "found needle in haystack"],
                    "grace_ms": 2000,
                    "wait_ms": 3000,
                    "rules": [{ "keywords": ["needle"], "event_kind": "needle_match" }]
                }),
            )
            .await,
        );
        let signals = out["signals"].as_array().expect("signals array");
        assert!(
            signals
                .iter()
                .any(|e| e["kind"].as_str() == Some("needle_match")),
            "run_and_watch must return the needle_match signal in one call; got {out}"
        );
        assert_eq!(
            out["exit_code"].as_i64(),
            Some(0),
            "run_and_watch must report the exit code; got {out}"
        );
        assert!(
            out["receipt"].is_null(),
            "matched run must omit receipt; got {out}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

/// Council HARD constraint: a quiet command (no rule matches) must NOT
/// error — run_and_watch returns the bounded exit receipt instead, so TC
/// never bounces the agent to the shell for running a small command.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_and_watch_quiet_command_returns_receipt_not_error() {
    let data = tmp_data_dir("raw-quiet");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let out = json(
            &call_tool(
                &client,
                "run_and_watch",
                serde_json::json!({
                    "argv": ["echo", "nothing interesting here"],
                    "grace_ms": 2000,
                    "wait_ms": 3000,
                    "rules": [{ "keywords": ["zzz-no-match-zzz"], "event_kind": "never" }]
                }),
            )
            .await,
        );
        let signals = out["signals"].as_array().expect("signals array");
        assert!(
            signals.is_empty(),
            "quiet command must have no signals; got {out}"
        );
        assert_eq!(
            out["exit_code"].as_i64(),
            Some(0),
            "exit code present; got {out}"
        );
        assert!(
            !out["receipt"].is_null(),
            "quiet command must return a receipt (no-silence rule), not an error; got {out}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
