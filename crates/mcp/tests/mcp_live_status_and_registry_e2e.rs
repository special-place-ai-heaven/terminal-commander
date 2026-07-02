// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Live-daemon MCP `call_tool` coverage for the read-only status, registry
//! read, and list surfaces that the happy-path matrix previously left
//! unexercised at the MCP layer:
//!
//! - `policy_status` — active profile name + bounded per-call caps.
//! - `self_check` — bounded text report + failure count.
//! - `registry_get` — fetch a definition after `registry_upsert`.
//! - `registry_search` — FTS hit for an upserted rule.
//! - `file_watch_list` — the started watch appears in the snapshot.
//! - `pty_command_list` — the started PTY job appears in the snapshot.
//!
//! Each test stands up a real `terminal-commanderd` UDS server and mounts
//! the rmcp stdio adapter on a duplex transport pointed at it, reusing the
//! `spawn_live_daemon` / `paired_against_live_daemon` pattern from the other
//! live MCP tests. Every payload is checked against `MAX_RESPONSE_BYTES`.

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
    p.push(format!("tc-mcp-live-statusreg-{tag}-{pid}-{nanos}-{n}"));
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

/// Single-token `event_kind` keeps the FTS5 `rule_search` match
/// unambiguous: the index stores `rule_id`, `event_kind`,
/// `summary_template`, and `tags_text`.
fn keyword_rule_json(id: &str, keyword: &str, event_kind: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id,
        "version": 1,
        "kind": "keyword",
        "status": "active",
        "severity": "medium",
        "event_kind": event_kind,
        "stream": null,
        "description": "live status/registry e2e",
        "pattern": null,
        "keywords": [keyword],
        "captures": [],
        "summary_template": "matched keyword",
        "tags": ["test"],
        "rate_limit_per_min": null,
        "redact": [],
        "context_hint": {
            "before_lines": 0,
            "after_lines": 0
        },
        "examples": []
    }))
    .expect("rule json")
}

fn parse_payload(result: &rmcp::model::CallToolResult) -> serde_json::Value {
    let payload = first_text(result);
    assert!(
        payload.len() <= MAX_RESPONSE_BYTES,
        "payload must respect the IPC response budget"
    );
    serde_json::from_str(&payload).expect("payload is JSON")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_policy_status_reports_profile_and_caps() {
    let data = tmp_data_dir("policy");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let body = parse_payload(&call_tool(&client, "policy_status", serde_json::json!({})).await);
        assert!(
            body["profile"].is_string(),
            "policy_status must report a string profile; got {body}"
        );
        assert!(
            body["file_window_bytes"].is_number(),
            "policy_status must report a numeric file_window_bytes cap; got {body}"
        );
        assert!(
            body["bucket_read_limit"].is_number(),
            "policy_status must report a numeric bucket_read_limit cap; got {body}"
        );
        // W2 / POLICY.md 4.1 guardrail #4: the resolved per-call caps are
        // surfaced so an operator can see the ACTIVE set. The live daemon runs
        // a default base profile, so all four resolve OFF.
        let caps = &body["caps"];
        assert!(
            caps.is_object(),
            "policy_status must surface a caps object; got {body}"
        );
        for key in [
            "allow_shell",
            "allow_session",
            "allow_privileged",
            "allow_remote",
        ] {
            assert_eq!(
                caps[key].as_bool(),
                Some(false),
                "default profile cap {key} must be present and false; got {body}"
            );
        }
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_self_check_reports_report_and_failures() {
    let data = tmp_data_dir("selfcheck");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let body = parse_payload(&call_tool(&client, "self_check", serde_json::json!({})).await);
        assert!(
            body["report"].is_string(),
            "self_check must return a text report; got {body}"
        );
        assert!(
            body["failures"].is_number(),
            "self_check must return a numeric failure count; got {body}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_registry_get_returns_definition_after_upsert() {
    let data = tmp_data_dir("regget");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let _ = call_tool(
            &client,
            "registry_upsert",
            serde_json::json!({
                "definition_json": keyword_rule_json("tcgetrule", "needle", "needle_match"),
            }),
        )
        .await;

        let body = parse_payload(
            &call_tool(
                &client,
                "registry_get",
                serde_json::json!({ "rule_id": "tcgetrule" }),
            )
            .await,
        );
        assert!(
            !body["definition"].is_null(),
            "registry_get must return the stored definition; got {body}"
        );
        assert_eq!(
            body["definition"]["id"], "tcgetrule",
            "registry_get must echo the requested rule id; got {body}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_registry_search_finds_upserted_rule() {
    let data = tmp_data_dir("regsearch");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let _ = call_tool(
            &client,
            "registry_upsert",
            serde_json::json!({
                "definition_json": keyword_rule_json("tcsearchrule", "needle", "uniquesearchkind"),
            }),
        )
        .await;

        let body = parse_payload(
            &call_tool(
                &client,
                "registry_search",
                serde_json::json!({ "query": "uniquesearchkind" }),
            )
            .await,
        );
        let hits = body["hits"]
            .as_array()
            .unwrap_or_else(|| panic!("registry_search must return a hits array; got {body}"));
        assert!(
            !hits.is_empty(),
            "registry_search must find the upserted rule; got {body}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_file_watch_list_contains_started_watch() {
    let data = tmp_data_dir("watchlist");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let watched = data.join("watched.log");
        std::fs::write(&watched, "preexisting\n").expect("seed watch file");

        let start = parse_payload(
            &call_tool(
                &client,
                "file_watch_start",
                serde_json::json!({ "path": watched.to_str().expect("utf8 path") }),
            )
            .await,
        );
        let watch_id = start["watch_id"]
            .as_str()
            .unwrap_or_else(|| panic!("file_watch_start must return a watch_id; got {start}"))
            .to_owned();

        let body =
            parse_payload(&call_tool(&client, "file_watch_list", serde_json::json!({})).await);
        let entries = body["entries"]
            .as_array()
            .unwrap_or_else(|| panic!("file_watch_list must return an entries array; got {body}"));
        assert!(
            entries
                .iter()
                .any(|e| e["watch_id"].as_str() == Some(watch_id.as_str())),
            "file_watch_list must contain the started watch {watch_id}; got {body}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_pty_command_list_contains_started_job() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let data = tmp_data_dir("ptylist");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let start = parse_payload(
            &call_tool(
                &client,
                "pty_command_start",
                serde_json::json!({
                    "argv": ["python3", "-u", "-c", "import time\ntime.sleep(2.0)\n"]
                }),
            )
            .await,
        );
        let job_id = start["job_id"]
            .as_str()
            .unwrap_or_else(|| panic!("pty_command_start must return a job_id; got {start}"))
            .to_owned();

        let body =
            parse_payload(&call_tool(&client, "pty_command_list", serde_json::json!({})).await);
        let entries = body["entries"]
            .as_array()
            .unwrap_or_else(|| panic!("pty_command_list must return an entries array; got {body}"));
        assert!(
            entries
                .iter()
                .any(|e| e["job_id"].as_str() == Some(job_id.as_str())),
            "pty_command_list must contain the started job {job_id}; got {body}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

/// BUG 2: `registry_deactivate` with an OMITTED version must resolve to the
/// LATEST stored version (symmetric with the version-less activate that opened
/// the row) instead of failing with "missing field `version`". Two versions of
/// one rule id are seeded so "latest" (v2) is unambiguously distinct from a
/// hardcoded 1, and the response must echo the version actually acted on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_deactivate_omitted_version_resolves_latest() {
    let data = tmp_data_dir("deactlatest");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Seed two versions of one rule id: the store assigns monotonic
        // versions, so the second upsert is v2 (the latest stored version).
        for _ in 0..2 {
            let _ = call_tool(
                &client,
                "registry_upsert",
                serde_json::json!({
                    "definition_json": keyword_rule_json("tcdeactlatest", "boom", "boom_match"),
                }),
            )
            .await;
        }

        // Activate the latest version (omit version -> daemon resolves v2).
        let activated = parse_payload(
            &call_tool(
                &client,
                "registry_activate",
                serde_json::json!({ "rule_id": "tcdeactlatest", "scope": { "kind": "global" } }),
            )
            .await,
        );
        assert_eq!(
            activated["version"].as_u64(),
            Some(2),
            "activate (omitted version) must resolve to the latest stored version; got {activated}"
        );

        // Deactivate with an OMITTED version: the adapter resolves latest (v2)
        // via a bounded registry_get and echoes the version actually acted on.
        let deactivated = parse_payload(
            &call_tool(
                &client,
                "registry_deactivate",
                serde_json::json!({ "rule_id": "tcdeactlatest", "scope": { "kind": "global" } }),
            )
            .await,
        );
        assert_eq!(
            deactivated["version"].as_u64(),
            Some(2),
            "deactivate (omitted version) must resolve + echo the latest stored version; got {deactivated}"
        );
        assert_eq!(
            deactivated["was_deactivated"].as_bool(),
            Some(true),
            "the resolved-latest deactivate must close the active row; got {deactivated}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
