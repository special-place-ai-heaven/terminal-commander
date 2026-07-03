// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! US4 / FR-030: opt-in `compact` projection on `bucket_wait` and
//! `bucket_events_since`, through the daemon.
//!
//! * `compact:true` projects each returned signal to the EXACT load-bearing
//!   field set `run_and_watch` already established (`{summary, stream, seq,
//!   severity}`) and echoes `"compact": true` in the payload.
//! * The event store keeps the FULL record: re-issuing the same cursor read
//!   with `compact` omitted returns the full-shape signals.
//! * `compact` is a PRESENTATION concern only: it never changes which events
//!   match (severity/kind filters compose untouched).
//!
//! Live through-the-daemon integration tests (constitution VI): each stands up
//! a real `terminal-commanderd` UDS server and drives it through the rmcp
//! stdio adapter.
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
    p.push(format!("tc-mcp-compact-{tag}-{pid}-{nanos}-{n}"));
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

/// `printf` (NOT a shell) emits three lines: two ERROR (high) and one INFO
/// (info). The argv lane is allowed by the default profile.
fn mixed_severity_argv() -> serde_json::Value {
    serde_json::json!(["printf", "ERROR alpha\nINFO beta\nERROR gamma\n"])
}

/// Two inline regex rules: ERROR -> high, INFO -> info. So three signals of
/// two severities land in the bucket.
fn mixed_rules() -> serde_json::Value {
    serde_json::json!([
        {
            "pattern": "ERROR (?P<what>.+)",
            "severity": "high",
            "summary_template": "err: ${what}",
            "captures": ["what"]
        },
        {
            "pattern": "INFO (?P<what>.+)",
            "severity": "info",
            "summary_template": "info: ${what}",
            "captures": ["what"]
        }
    ])
}

/// Run the mixed-severity command to completion and return its bucket id. The
/// full SignalEvent records are retained in the bucket store, re-readable from
/// cursor 0.
async fn run_and_bucket(
    client: &rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) -> String {
    let body = call_json(
        client,
        "run_and_watch",
        serde_json::json!({
            "argv": mixed_severity_argv(),
            "rules": mixed_rules(),
            "wait_ms": 4000,
            "wait_until": "exit"
        }),
    )
    .await;
    assert_eq!(
        body["complete"],
        serde_json::json!(true),
        "command should exit within budget; body: {body}"
    );
    body["bucket_id"]
        .as_str()
        .expect("bucket_id present")
        .to_owned()
}

const COMPACT_KEYS: [&str; 4] = ["summary", "stream", "seq", "severity"];

fn assert_compact_signal(sig: &serde_json::Value) {
    let obj = sig.as_object().expect("signal object");
    let keys: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let allowed: std::collections::BTreeSet<&str> = COMPACT_KEYS.into_iter().collect();
    assert_eq!(
        keys, allowed,
        "compact signal must carry ONLY {{summary,stream,seq,severity}}; got {keys:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bucket_wait_compact_projects_only_load_bearing_fields() {
    let data = tmp_data_dir("wait");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let bucket_id = run_and_bucket(&client).await;

        let body = call_json(
            &client,
            "bucket_wait",
            serde_json::json!({
                "bucket_id": bucket_id,
                "cursor": 0,
                "compact": true,
                "timeout_ms": 3000
            }),
        )
        .await;

        assert_eq!(
            body["compact"],
            serde_json::json!(true),
            "compact wait echoes compact:true; body: {body}"
        );
        let events = body["events"].as_array().expect("events array");
        assert!(!events.is_empty(), "expected >=1 signal; body: {body}");
        for sig in events {
            assert_compact_signal(sig);
        }
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bucket_events_compact_full_records_refetchable_by_cursor() {
    let data = tmp_data_dir("events");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let bucket_id = run_and_bucket(&client).await;

        // Compact read: load-bearing fields only.
        let compact = call_json(
            &client,
            "bucket_events_since",
            serde_json::json!({
                "bucket_id": bucket_id,
                "cursor": 0,
                "compact": true
            }),
        )
        .await;
        assert_eq!(
            compact["compact"],
            serde_json::json!(true),
            "compact events echoes compact:true; body: {compact}"
        );
        let compact_events = compact["events"].as_array().expect("events array");
        assert!(!compact_events.is_empty(), "expected >=1 signal; {compact}");
        for sig in compact_events {
            assert_compact_signal(sig);
        }

        // Same cursor, compact OMITTED: the full record is still there.
        let full = call_json(
            &client,
            "bucket_events_since",
            serde_json::json!({
                "bucket_id": bucket_id,
                "cursor": 0
            }),
        )
        .await;
        // No compact echo on a full read (byte-identical default).
        assert!(
            full.get("compact").is_none() || full["compact"] == serde_json::json!(false),
            "full read must not claim compact; body: {full}"
        );
        let full_events = full["events"].as_array().expect("events array");
        assert_eq!(
            full_events.len(),
            compact_events.len(),
            "same cursor read returns the same set; full: {full}"
        );
        // The full record carries the id plumbing that compact drops.
        let first = &full_events[0];
        assert!(
            first.get("event_id").is_some(),
            "full record retains event_id; body: {full}"
        );
        assert!(
            first.get("bucket_id").is_some(),
            "full record retains bucket_id; body: {full}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compact_never_changes_which_events_match() {
    let data = tmp_data_dir("filter");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let bucket_id = run_and_bucket(&client).await;

        // A severity floor that drops the INFO signal but keeps the two ERRORs.
        let filtered_full = call_json(
            &client,
            "bucket_events_since",
            serde_json::json!({
                "bucket_id": bucket_id,
                "cursor": 0,
                "severity_min": "high"
            }),
        )
        .await;
        let filtered_compact = call_json(
            &client,
            "bucket_events_since",
            serde_json::json!({
                "bucket_id": bucket_id,
                "cursor": 0,
                "severity_min": "high",
                "compact": true
            }),
        )
        .await;

        let full_seqs: Vec<u64> = filtered_full["events"]
            .as_array()
            .expect("events array")
            .iter()
            .map(|e| e["seq"].as_u64().expect("seq"))
            .collect();
        let compact_seqs: Vec<u64> = filtered_compact["events"]
            .as_array()
            .expect("events array")
            .iter()
            .map(|e| e["seq"].as_u64().expect("seq"))
            .collect();

        assert!(
            !full_seqs.is_empty(),
            "the high-severity filter should keep the ERROR signals; full: {filtered_full}"
        );
        assert_eq!(
            full_seqs, compact_seqs,
            "compact must NOT change which events match: full={full_seqs:?} compact={compact_seqs:?}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
