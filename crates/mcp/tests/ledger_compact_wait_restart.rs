// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Folded field-ledger fixes, through-the-daemon (omni spec 001, US1):
//!
//! * T011 / TC-E1: `run_and_watch` `compact:true` projects each signal to
//!   `{summary, stream, seq, severity}` ONLY -- no id plumbing.
//! * T011 / TC-E4: a simple-pattern signal carries ONE canonical matched-text
//!   capture field (`match`) plus named captures -- not the historical
//!   `0`/`line`/`match` triple echoing identical bytes.
//! * T012 / TC-E2: `wait_until:"exit"` wall-time is bounded by the advertised
//!   server cap (never exceeds it).
//! * T012 / TC-B3: after a daemon restart (in-memory job gone), `command_status`
//!   for that job returns a RESTART-MARKED terminal result read from the
//!   persisted receipt -- not a bare error.
//!
//! Each test stands up a real `terminal-commanderd` UDS server in a temp dir
//! and drives it through the rmcp stdio adapter, so these are live
//! through-the-daemon integration tests (constitution VI), not unit shims.
//!
//! Source-status: live.

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
    p.push(format!("tc-mcp-ledger-{tag}-{pid}-{nanos}-{n}"));
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

fn first_text_content(result: &rmcp::model::CallToolResult) -> String {
    for item in &result.content {
        if let Some(text) = item.as_text() {
            return text.text.clone();
        }
    }
    panic!("tool result had no text content: {result:?}");
}

async fn call_json(
    client: &rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
    tool: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let obj: rmcp::model::JsonObject = serde_json::from_value(args).expect("args object");
    let params = CallToolRequestParams::new(tool.to_owned()).with_arguments(obj);
    let result = client
        .call_tool(params)
        .await
        .unwrap_or_else(|e| panic!("{tool} call failed: {e}"));
    serde_json::from_str(&first_text_content(&result)).expect("payload is JSON")
}

/// `printf` (NOT a shell) emits a single line that an inline regex rule with a
/// named capture matches. The argv lane is allowed by the default profile;
/// only shell-string passthrough is denied.
fn printf_error_argv() -> serde_json::Value {
    serde_json::json!(["printf", "ERROR boom happened\n"])
}

/// Inline rule: regex with ONE named capture, summary echoes the full match.
fn error_rule() -> serde_json::Value {
    serde_json::json!([{
        "pattern": "ERROR (?P<what>.+)",
        "severity": "high",
        "summary_template": "saw: ${what}",
        "captures": ["what"]
    }])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compact_projection_returns_only_load_bearing_fields() {
    let data = tmp_data_dir("compact");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let body = call_json(
            &client,
            "run_and_watch",
            serde_json::json!({
                "argv": printf_error_argv(),
                "rules": error_rule(),
                "wait_ms": 4000,
                "compact": true
            }),
        )
        .await;

        assert_eq!(body["compact"], serde_json::json!(true), "body: {body}");
        let signals = body["signals"].as_array().expect("signals array");
        assert!(!signals.is_empty(), "expected >=1 signal; body: {body}");
        // TC-E1: each compact signal carries EXACTLY the load-bearing keys.
        let allowed: std::collections::BTreeSet<&str> = ["summary", "stream", "seq", "severity"]
            .into_iter()
            .collect();
        for sig in signals {
            let obj = sig.as_object().expect("signal object");
            let keys: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
            assert_eq!(
                keys, allowed,
                "compact signal must carry ONLY {{summary,stream,seq,severity}}; got {keys:?}"
            );
        }
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_signal_has_one_canonical_capture_not_triple() {
    let data = tmp_data_dir("capture");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        // compact omitted/false -> full signal records, including captures.
        let body = call_json(
            &client,
            "run_and_watch",
            serde_json::json!({
                "argv": printf_error_argv(),
                "rules": error_rule(),
                "wait_ms": 4000
            }),
        )
        .await;

        let signals = body["signals"].as_array().expect("signals array");
        assert!(!signals.is_empty(), "expected >=1 signal; body: {body}");
        let caps = signals[0]["captures"]
            .as_object()
            .expect("full signal carries captures");
        // TC-E4: the named capture survives.
        assert_eq!(
            caps.get("what").and_then(serde_json::Value::as_str),
            Some("boom happened"),
            "named capture must be present; caps: {caps:?}"
        );
        // The canonical matched-text key is present...
        assert!(
            caps.contains_key("match"),
            "canonical `match` capture must be present; caps: {caps:?}"
        );
        // ...and the redundant synonyms are collapsed away (no triple echo).
        assert!(
            !caps.contains_key("line"),
            "redundant `line` synonym must be collapsed; caps: {caps:?}"
        );
        assert!(
            !caps.contains_key("0"),
            "redundant `0` synonym must be collapsed; caps: {caps:?}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wait_until_exit_is_bounded_by_advertised_cap() {
    let data = tmp_data_dir("waituntil");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        // A sleeper that outlives the wait budget: `wait_until:"exit"` must NOT
        // wait past the advertised cap. wait_ms is the honest bound; sleep 30s
        // far exceeds it. Wall time must stay within wait_ms + slop.
        let started = std::time::Instant::now();
        let body = call_json(
            &client,
            "run_and_watch",
            serde_json::json!({
                "argv": ["sleep", "30"],
                "wait_ms": 1500,
                "wait_until": "exit"
            }),
        )
        .await;
        let elapsed = started.elapsed();

        // Honest cap (constitution VII): never exceed the advertised wait. Allow
        // generous slop for one in-flight slice + round-trips, but it MUST be
        // far under the 30s sleep.
        assert!(
            elapsed < Duration::from_secs(8),
            "wait_until:exit must be bounded by the wait_ms cap, not the 30s sleep; \
             elapsed={elapsed:?}"
        );
        // The job is still running (not complete) and advertises a poll hint.
        assert_eq!(body["complete"], serde_json::json!(false), "body: {body}");
        assert!(
            body["poll_hint_ms"].as_u64().is_some(),
            "a running result must advertise poll_hint_ms; body: {body}"
        );
        assert!(
            body["wait_cap_ms"].as_u64().is_some(),
            "the advertised cap must be present; body: {body}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_status_after_restart_returns_restart_marked_terminal() {
    // TC-B3: run a command to completion, drop the daemon (simulating a
    // restart), re-bootstrap on the SAME data dir, then poll command_status for
    // the now-forgotten job. The persisted receipt must yield a restart-marked
    // TERMINAL result, never a bare error.
    let data = tmp_data_dir("restart");

    // --- Phase 1: run a command to exit, capture its job_id. ---
    let job_id = {
        let handle = spawn_live_daemon(&data);
        let job_id = {
            let (_server, client) = paired_against_live_daemon(&handle).await;
            let body = call_json(
                &client,
                "run_and_watch",
                serde_json::json!({
                    "argv": ["printf", "quiet output\n"],
                    "wait_ms": 4000,
                    "wait_until": "exit"
                }),
            )
            .await;
            assert_eq!(
                body["complete"],
                serde_json::json!(true),
                "command should have exited within the budget; body: {body}"
            );
            let job_id = body["job_id"].as_str().expect("job_id present").to_owned();
            let _ = client.cancel().await;
            job_id
        };
        // Drop the daemon: its in-memory job map is gone, the receipt is on
        // disk (the receipt write is synchronous through the store actor on the
        // terminal transition, which completed before `complete:true` above).
        handle.shutdown().await;
        job_id
    };

    // --- Phase 2: re-bootstrap on the SAME data dir and poll status. ---
    {
        let handle = spawn_live_daemon(&data);
        {
            let (_server, client) = paired_against_live_daemon(&handle).await;
            let body = call_json(
                &client,
                "command_status",
                serde_json::json!({ "job_id": job_id }),
            )
            .await;
            // TC-B3: a known terminal result reconstructed from the receipt,
            // NOT a bare error and NOT a silent running state.
            assert_eq!(
                body["restarted"],
                serde_json::json!(true),
                "post-restart status must be restart-marked; body: {body}"
            );
            let state = body["state"].as_str().unwrap_or("");
            assert_eq!(
                state, "exited",
                "post-restart terminal state must be exited; body: {body}"
            );
            let _ = client.cancel().await;
        }
        handle.shutdown().await;
    }
    cleanup(&data);
}
