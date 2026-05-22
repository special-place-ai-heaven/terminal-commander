// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC42c end-to-end smoke through MCP: start two long-running
//! commands emitting the same matchable token. Activate the rule
//! with scope = bucket A. Prove bucket A produces matching signal,
//! bucket B does NOT. Then deactivate the same scope and prove
//! bucket A stops producing matching signal too.
//!
//! Uses `python3 -u -c '...'` as the slow line emitter — same
//! pattern as `registry_live_rebind_e2e`. `python3` is not in the
//! shell-bridge deny list; it is exec'd directly as an argv program.

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
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-mcp-scope-{tag}-{pid}-{nanos}"));
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

fn keyword_rule_json(id: &str, keyword: &str, event_kind: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id,
        "version": 1,
        "kind": "keyword",
        "status": "active",
        "severity": "medium",
        "event_kind": event_kind,
        "stream": null,
        "description": "tc42c scoped rebind",
        "pattern": null,
        "keywords": [keyword],
        "captures": [],
        "summary_template": "matched keyword",
        "tags": ["tc42c"],
        "rate_limit_per_min": null,
        "redact": [],
        "context_hint": {"before_lines": 0, "after_lines": 0},
        "examples": []
    }))
    .expect("rule json")
}

async fn start_emitter(
    client: &rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) -> String {
    let py = r#"
import sys, time
for i in range(8):
    print("scope-token", flush=True)
    time.sleep(0.25)
"#;
    let start = first_text(
        &call_tool(
            client,
            "command_start_combed",
            serde_json::json!({
                "argv": ["python3", "-u", "-c", py],
                "grace_ms": 5000
            }),
        )
        .await,
    );
    let v: serde_json::Value = serde_json::from_str(&start).expect("start json");
    v["bucket_id"]
        .as_str()
        .expect("bucket_id string")
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines, clippy::similar_names)]
async fn bucket_scoped_activation_emits_only_in_matching_bucket() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let data = tmp_data_dir("scope-bucket");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Upsert the rule but leave it inactive.
        let _ = call_tool(
            &client,
            "registry_upsert",
            serde_json::json!({
                "definition_json": keyword_rule_json("tc42c-kw", "scope-token", "scope_match"),
            }),
        )
        .await;

        // Two emitters, one bucket each. Both print "scope-token".
        let bucket_a = start_emitter(&client).await;
        let bucket_b = start_emitter(&client).await;

        // Phase 1: drain both buckets for 400 ms WITHOUT the rule
        // active. No `scope_match` event may appear in either.
        let pre_a = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({"bucket_id": bucket_a, "cursor": 0, "timeout_ms": 400}),
            )
            .await,
        );
        let pre_a_v: serde_json::Value = serde_json::from_str(&pre_a).expect("pre A json");
        for e in pre_a_v["events"].as_array().cloned().unwrap_or_default() {
            assert_ne!(
                e["kind"].as_str(),
                Some("scope_match"),
                "pre-activation A must not produce scope_match; payload: {pre_a}"
            );
        }
        let cursor_a = pre_a_v["next_cursor"].as_u64().unwrap_or(0);

        let pre_b = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({"bucket_id": bucket_b, "cursor": 0, "timeout_ms": 400}),
            )
            .await,
        );
        let pre_b_v: serde_json::Value = serde_json::from_str(&pre_b).expect("pre B json");
        for e in pre_b_v["events"].as_array().cloned().unwrap_or_default() {
            assert_ne!(
                e["kind"].as_str(),
                Some("scope_match"),
                "pre-activation B must not produce scope_match; payload: {pre_b}"
            );
        }
        let cursor_b = pre_b_v["next_cursor"].as_u64().unwrap_or(0);

        // Phase 2: scoped activate — bucket A only.
        let activate_payload = first_text(
            &call_tool(
                &client,
                "registry_activate",
                serde_json::json!({
                    "rule_id": "tc42c-kw",
                    "scope": {"kind": "bucket", "bucket_id": bucket_a},
                }),
            )
            .await,
        );
        let act_v: serde_json::Value =
            serde_json::from_str(&activate_payload).expect("activate json");
        assert_eq!(act_v["jobs_rebound"].as_u64().unwrap_or(0), 1);
        assert_eq!(act_v["scope"]["kind"].as_str(), Some("bucket"));

        // Phase 3: drain bucket A — MUST see a scope_match event.
        let post_a = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({"bucket_id": bucket_a, "cursor": cursor_a, "timeout_ms": 900}),
            )
            .await,
        );
        let post_a_v: serde_json::Value = serde_json::from_str(&post_a).expect("post A json");
        let post_a_events = post_a_v["events"].as_array().cloned().unwrap_or_default();
        assert!(
            post_a_events
                .iter()
                .any(|e| e["kind"].as_str() == Some("scope_match")),
            "bucket A must emit scope_match after scoped activation; payload: {post_a}"
        );

        // Phase 4: drain bucket B — MUST NOT see any scope_match.
        let post_b = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({"bucket_id": bucket_b, "cursor": cursor_b, "timeout_ms": 900}),
            )
            .await,
        );
        let post_b_v: serde_json::Value = serde_json::from_str(&post_b).expect("post B json");
        for e in post_b_v["events"].as_array().cloned().unwrap_or_default() {
            assert_ne!(
                e["kind"].as_str(),
                Some("scope_match"),
                "bucket B must NOT emit scope_match under bucket-A scope; payload: {post_b}"
            );
        }
        let after_active_cursor_a = post_a_v["next_cursor"].as_u64().unwrap_or(cursor_a);

        // Phase 5: scoped deactivate — bucket A.
        let _ = call_tool(
            &client,
            "registry_deactivate",
            serde_json::json!({
                "rule_id": "tc42c-kw",
                "version": 1,
                "scope": {"kind": "bucket", "bucket_id": bucket_a},
            }),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Phase 6: drain bucket A again. Lifecycle events are fine;
        // no further scope_match should appear once the deactivate
        // has propagated.
        let drain = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_a,
                    "cursor": after_active_cursor_a,
                    "timeout_ms": 400,
                }),
            )
            .await,
        );
        let drain_v: serde_json::Value = serde_json::from_str(&drain).expect("drain json");
        let drain_cursor = drain_v["next_cursor"]
            .as_u64()
            .unwrap_or(after_active_cursor_a);

        let final_payload = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_a,
                    "cursor": drain_cursor,
                    "timeout_ms": 800,
                }),
            )
            .await,
        );
        let final_v: serde_json::Value = serde_json::from_str(&final_payload).expect("final json");
        for e in final_v["events"].as_array().cloned().unwrap_or_default() {
            assert_ne!(
                e["kind"].as_str(),
                Some("scope_match"),
                "post-deactivation bucket A must not emit scope_match; payload: {final_payload}"
            );
        }

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
