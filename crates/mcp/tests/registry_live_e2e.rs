// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC42 live-daemon e2e for the MCP registry surface.
//!
//! Walks the LLM-shaped activation flow entirely through MCP:
//!
//! 1. `registry_upsert` creates a keyword rule.
//! 2. `registry_test` dry-runs it against sample text and verifies the
//!    rule fires.
//! 3. `registry_activate` enables the rule for newly-started commands.
//! 4. `command_start_combed` launches `echo needle` (or `printf`) via
//!    MCP, producing a bucket whose sifter is built from the active
//!    registry snapshot.
//! 5. `bucket_wait` observes the rule-driven event before the
//!    lifecycle event arrives.
//! 6. `registry_deactivate` removes the rule. A subsequent
//!    `command_start_combed` against the same argv yields only the
//!    lifecycle event — no rule-driven match.
//!
//! Plus a negative path: `registry_upsert` with an unclosed regex
//! group is rejected with a typed error through MCP.

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
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-mcp-registry-e2e-{tag}-{pid}-{nanos}"));
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

fn keyword_rule_json(id: &str, keyword: &str, event_kind: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id,
        "version": 1,
        "kind": "keyword",
        "status": "active",
        "severity": "medium",
        "event_kind": event_kind,
        "stream": null,
        "description": "tc42 e2e",
        "pattern": null,
        "keywords": [keyword],
        "captures": [],
        "summary_template": "matched keyword",
        "tags": ["test", "tc42"],
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines, clippy::similar_names)]
async fn activated_rule_drives_signal_then_deactivated_rule_does_not() {
    let data = tmp_data_dir("full");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // 1. Upsert.
        let upsert = first_text(
            &call_tool(
                &client,
                "registry_upsert",
                serde_json::json!({
                    "definition_json": keyword_rule_json("tc42-kw", "needle", "needle_match"),
                }),
            )
            .await,
        );
        let upsert_v: serde_json::Value = serde_json::from_str(&upsert).expect("upsert json");
        assert_eq!(upsert_v["rule_id"], "tc42-kw");
        assert_eq!(upsert_v["version"], 1);

        // 2. Test against bounded samples; the second one matches.
        let test_payload = first_text(
            &call_tool(
                &client,
                "registry_test",
                serde_json::json!({
                    "rule_id": "tc42-kw",
                    "samples": [
                        {"text": "no match here"},
                        {"text": "found needle in haystack"}
                    ]
                }),
            )
            .await,
        );
        assert!(
            test_payload.len() <= MAX_RESPONSE_BYTES,
            "registry_test payload must respect IPC response budget"
        );
        let test_v: serde_json::Value = serde_json::from_str(&test_payload).expect("test json");
        let matches = test_v["matches"].as_array().expect("matches array");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["sample_index"], 1);
        assert_eq!(matches[0]["kind"], "needle_match");

        // 3. Activate.
        let act_payload = first_text(
            &call_tool(
                &client,
                "registry_activate",
                serde_json::json!({"rule_id": "tc42-kw"}),
            )
            .await,
        );
        let act_v: serde_json::Value = serde_json::from_str(&act_payload).expect("act json");
        assert_eq!(act_v["rule_id"], "tc42-kw");
        assert_eq!(act_v["was_already_active"], false);

        // 3b. list_active confirms one entry.
        let list_payload =
            first_text(&call_tool(&client, "registry_list_active", serde_json::json!({})).await);
        let list_v: serde_json::Value = serde_json::from_str(&list_payload).expect("list json");
        let entries = list_v["entries"].as_array().expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["rule_id"], "tc42-kw");

        // 4. Start a command whose stdout contains the keyword.
        let start_payload = first_text(
            &call_tool(
                &client,
                "command_start_combed",
                serde_json::json!({
                    "argv": ["echo", "found needle in haystack"],
                    "grace_ms": 2000
                }),
            )
            .await,
        );
        let start_v: serde_json::Value = serde_json::from_str(&start_payload).expect("start json");
        let bucket_id = start_v["bucket_id"].as_str().unwrap().to_owned();

        // 5. bucket_wait should surface the rule-driven match. We
        // accept any number of events as long as one of them is the
        // needle_match kind produced by the active rule.
        let wait_payload = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_id,
                    "cursor": 0,
                    "timeout_ms": 2000
                }),
            )
            .await,
        );
        let wait_v: serde_json::Value = serde_json::from_str(&wait_payload).expect("wait json");
        let events = wait_v["events"].as_array().expect("events");
        assert!(
            events
                .iter()
                .any(|e| e["kind"].as_str() == Some("needle_match")),
            "active rule must drive at least one needle_match event; payload: {wait_payload}"
        );

        // 6. Deactivate.
        let deact_payload = first_text(
            &call_tool(
                &client,
                "registry_deactivate",
                serde_json::json!({"rule_id": "tc42-kw", "version": 1}),
            )
            .await,
        );
        let deact_v: serde_json::Value = serde_json::from_str(&deact_payload).expect("deact json");
        assert_eq!(deact_v["was_deactivated"], true);

        // 7. New command after deactivation: only lifecycle events.
        let start2_payload = first_text(
            &call_tool(
                &client,
                "command_start_combed",
                serde_json::json!({
                    "argv": ["echo", "found needle in haystack"],
                    "grace_ms": 2000
                }),
            )
            .await,
        );
        let start2_v: serde_json::Value =
            serde_json::from_str(&start2_payload).expect("start2 json");
        let bucket_id2 = start2_v["bucket_id"].as_str().unwrap().to_owned();

        let wait2_payload = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_id2,
                    "cursor": 0,
                    "timeout_ms": 2000
                }),
            )
            .await,
        );
        let wait2_v: serde_json::Value = serde_json::from_str(&wait2_payload).expect("wait2 json");
        let events2 = wait2_v["events"].as_array().expect("events2");
        assert!(
            !events2
                .iter()
                .any(|e| e["kind"].as_str() == Some("needle_match")),
            "deactivated rule must NOT drive any needle_match event; payload: {wait2_payload}"
        );
        assert!(
            events2.iter().all(|e| e["kind"]
                .as_str()
                .is_some_and(|k| k.starts_with("command_"))),
            "without active rules only lifecycle events are expected; payload: {wait2_payload}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registry_upsert_rejects_invalid_regex_through_mcp() {
    let data = tmp_data_dir("badrx");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Build a definition with an unclosed regex group.
        let bad = serde_json::json!({
            "id": "tc42-bad-rx",
            "version": 1,
            "kind": "regex",
            "status": "active",
            "severity": "high",
            "event_kind": "x",
            "stream": "stdout",
            "description": null,
            "pattern": "(unclosed",
            "keywords": null,
            "captures": [],
            "summary_template": "x",
            "tags": [],
            "rate_limit_per_min": null,
            "redact": [],
            "context_hint": { "before_lines": 0, "after_lines": 0 },
            "examples": []
        });
        let mut params = CallToolRequestParams::new("registry_upsert");
        params.arguments = Some(
            serde_json::json!({"definition_json": bad.to_string()})
                .as_object()
                .unwrap()
                .clone(),
        );
        let err = client
            .call_tool(params)
            .await
            .expect_err("invalid regex must be rejected through MCP");
        let msg = err.to_string();
        assert!(
            msg.contains("regex") || msg.contains("RuleInvalid"),
            "denial must surface the invalid-rule reason; got: {msg}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
