// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! US2 (T026) closed-loop e2e for `registry_suggest_from_samples`.
//!
//! Walks the full suggest -> test -> upsert -> activate -> re-run loop
//! ENTIRELY through MCP, proving gate O-05 (SC-002):
//!
//! 1. `registry_suggest_from_samples` over unknown output returns DRAFT
//!    proposals + confidence "heuristic" + the explicit next steps.
//! 2. ASSERT the suggest step activated NOTHING:
//!    `registry_list_active` is empty immediately after suggest.
//! 3. `registry_upsert` persists a rule derived from a proposal.
//! 4. `registry_test` dry-runs it and confirms it fires on the sample.
//! 5. `registry_activate` enables it (the ONLY step that makes it live).
//! 6. `command_start_combed` re-runs a command emitting the same shape;
//!    `bucket_wait` shows the rule-driven signal now appears.
//!
//! The invariant under test (constitution VII / FR-008): a suggestion
//! NEVER auto-activates. Activation requires the explicit
//! test-then-activate sequence.

#![cfg(unix)]

use std::path::PathBuf;
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
    p.push(format!("tc-mcp-suggest-e2e-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn spawn_live_daemon(data: &std::path::Path) -> ServerHandle {
    let cfg = DaemonConfig::defaults_in(data);
    let state = std::sync::Arc::new(DaemonState::bootstrap(cfg).expect("daemon bootstrap"));
    let socket = state.config.socket_path();
    let server = IpcServer::new(std::sync::Arc::clone(&state), socket);
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
        .unwrap_or_else(|e| panic!("call {name}: {e}"))
}

/// A keyword rule json built from the agent's choice, modeling the
/// rule an agent would author after seeing a suggestion. We keep it a
/// keyword rule (simple + deterministic) keyed on a marker that the
/// re-run command emits.
fn marker_rule_json(id: &str, marker: &str, event_kind: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id,
        "version": 1,
        "kind": "keyword",
        "status": "active",
        "severity": "high",
        "event_kind": event_kind,
        "stream": null,
        "description": "us2 suggest-loop e2e",
        "pattern": null,
        "keywords": [marker],
        "captures": [],
        "summary_template": "matched marker",
        "tags": ["test", "us2"],
        "rate_limit_per_min": null,
        "redact": [],
        "context_hint": { "before_lines": 0, "after_lines": 0 },
        "examples": []
    }))
    .expect("rule json")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn suggest_never_activates_then_explicit_loop_makes_signal_appear() {
    let data = tmp_data_dir("loop");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // 0. Baseline: nothing active.
        let baseline =
            first_text(&call_tool(&client, "registry_list_active", serde_json::json!({})).await);
        let baseline_v: serde_json::Value = serde_json::from_str(&baseline).expect("baseline json");
        assert_eq!(
            baseline_v["entries"].as_array().expect("entries").len(),
            0,
            "fresh daemon must have no active rules"
        );

        // 1. SUGGEST from unknown output samples.
        let suggest_payload = first_text(
            &call_tool(
                &client,
                "registry_suggest_from_samples",
                serde_json::json!({
                    "samples": [
                        "error: widget exploded at stage 3",
                        "warning: deprecated flag --foo",
                        "src/widget.rs:88:4: bad thing",
                        "process exited with code 7"
                    ],
                    "intent": "parse the widgettool output",
                    "max_rules": 4
                }),
            )
            .await,
        );
        let suggest_v: serde_json::Value =
            serde_json::from_str(&suggest_payload).expect("suggest json");
        // Confidence + next-step contract.
        assert_eq!(suggest_v["confidence"], "heuristic");
        assert_eq!(
            suggest_v["next_steps"],
            serde_json::json!(["registry_test", "registry_upsert", "registry_activate"])
        );
        let proposals = suggest_v["proposed_rules"]
            .as_array()
            .expect("proposed_rules array");
        assert!(
            !proposals.is_empty(),
            "error/warning/locator/exit samples must yield proposals"
        );
        // Every proposal is DRAFT.
        for p in proposals {
            assert_eq!(
                p["status"], "draft",
                "every proposal must be a DRAFT (suggest never activates)"
            );
        }

        // 2. ASSERT the suggest step changed NO active-rule state.
        let after_suggest =
            first_text(&call_tool(&client, "registry_list_active", serde_json::json!({})).await);
        let after_suggest_v: serde_json::Value =
            serde_json::from_str(&after_suggest).expect("after-suggest json");
        assert_eq!(
            after_suggest_v["entries"]
                .as_array()
                .expect("entries")
                .len(),
            0,
            "registry_suggest_from_samples MUST NOT activate anything (FR-008 / O-05)"
        );

        // 3. UPSERT a rule the agent authored from the proposal. We use
        //    a keyword marker rule keyed on a string the re-run command
        //    will emit, so the loop is deterministic end-to-end.
        let marker = "WIDGET_EXPLODED";
        let upsert = first_text(
            &call_tool(
                &client,
                "registry_upsert",
                serde_json::json!({
                    "definition_json": marker_rule_json("us2-suggest", marker, "widget_failure"),
                }),
            )
            .await,
        );
        let upsert_v: serde_json::Value = serde_json::from_str(&upsert).expect("upsert json");
        assert_eq!(upsert_v["rule_id"], "us2-suggest");

        // Upsert alone must NOT activate.
        let after_upsert =
            first_text(&call_tool(&client, "registry_list_active", serde_json::json!({})).await);
        let after_upsert_v: serde_json::Value =
            serde_json::from_str(&after_upsert).expect("after-upsert json");
        assert_eq!(
            after_upsert_v["entries"].as_array().expect("entries").len(),
            0,
            "upsert persists but must NOT activate"
        );

        // 4. TEST the rule against the sample marker; it must fire.
        let test_payload = first_text(
            &call_tool(
                &client,
                "registry_test",
                serde_json::json!({
                    "rule_id": "us2-suggest",
                    "samples": [
                        {"text": "nothing here"},
                        {"text": "boom WIDGET_EXPLODED at stage 3"}
                    ]
                }),
            )
            .await,
        );
        let test_v: serde_json::Value = serde_json::from_str(&test_payload).expect("test json");
        let matches = test_v["matches"].as_array().expect("matches array");
        assert_eq!(
            matches.len(),
            1,
            "rule should match exactly the marker line"
        );
        assert_eq!(matches[0]["kind"], "widget_failure");

        // 5. ACTIVATE -- the explicit step that finally makes it live.
        let act = first_text(
            &call_tool(
                &client,
                "registry_activate",
                serde_json::json!({"rule_id": "us2-suggest", "scope": {"kind": "global"}}),
            )
            .await,
        );
        let act_v: serde_json::Value = serde_json::from_str(&act).expect("act json");
        assert_eq!(act_v["was_already_active"], false);

        let after_activate =
            first_text(&call_tool(&client, "registry_list_active", serde_json::json!({})).await);
        let after_activate_v: serde_json::Value =
            serde_json::from_str(&after_activate).expect("after-activate json");
        assert_eq!(
            after_activate_v["entries"]
                .as_array()
                .expect("entries")
                .len(),
            1,
            "only AFTER explicit activate is a rule live"
        );

        // 6. RE-RUN a command emitting the marker; the rule-driven
        //    signal now appears in the bucket.
        let start_payload = first_text(
            &call_tool(
                &client,
                "command_start_combed",
                serde_json::json!({
                    "argv": ["echo", "boom WIDGET_EXPLODED at stage 3"],
                    "grace_ms": 2000
                }),
            )
            .await,
        );
        let start_v: serde_json::Value = serde_json::from_str(&start_payload).expect("start json");
        let bucket_id = start_v["bucket_id"].as_str().expect("bucket_id").to_owned();

        // Drain the bucket; assert the rule-driven event landed.
        let mut saw_signal = false;
        for _ in 0..40 {
            let wait_payload = first_text(
                &call_tool(
                    &client,
                    "bucket_wait",
                    serde_json::json!({ "bucket_id": bucket_id, "cursor": 0, "timeout_ms": 200 }),
                )
                .await,
            );
            let wait_v: serde_json::Value = serde_json::from_str(&wait_payload).expect("wait json");
            if let Some(events) = wait_v["events"].as_array()
                && events.iter().any(|e| e["kind"] == "widget_failure")
            {
                saw_signal = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            saw_signal,
            "after explicit activation + re-run, the rule-driven signal must appear (O-05)"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
