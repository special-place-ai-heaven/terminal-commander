// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC43 end-to-end MCP smoke for file tools.
//!
//! 1. file_search returns bounded matches with pointers.
//! 2. file_read_window returns a bounded line window.
//! 3. file_watch_start creates a daemon-owned watch + bucket.
//! 4. registry_upsert + registry_activate with bucket scope binds a
//!    rule to that watch's bucket only.
//! 5. Append matching content -> bucket_wait sees the signal.
//! 6. Negative: file_watch_start a SECOND file in a different bucket;
//!    a bucket-scoped activation on bucket A must not produce signal
//!    in bucket B even when the second file gets matching content.

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
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-mcp-file-{tag}-{pid}-{nanos}"));
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

fn keyword_rule_json(id: &str, keyword: &str, event_kind: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id,
        "version": 1,
        "kind": "keyword",
        "status": "active",
        "severity": "medium",
        "event_kind": event_kind,
        "stream": null,
        "description": "tc43 file watch",
        "pattern": null,
        "keywords": [keyword],
        "captures": [],
        "summary_template": "matched keyword",
        "tags": ["tc43"],
        "rate_limit_per_min": null,
        "redact": [],
        "context_hint": {"before_lines": 0, "after_lines": 0},
        "examples": []
    }))
    .expect("rule json")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines, clippy::similar_names)]
async fn file_tools_full_lifecycle_through_mcp() {
    let data = tmp_data_dir("full");
    let handle = spawn_live_daemon(&data);
    // File A: target of the scoped activation.
    let file_a = data.join("a.log");
    std::fs::create_dir_all(&data).unwrap();
    {
        let mut f = std::fs::File::create(&file_a).unwrap();
        writeln!(f, "alpha").unwrap();
        writeln!(f, "needle pre").unwrap();
        writeln!(f, "beta").unwrap();
    }
    // File B: negative — must NOT emit signal under bucket-A scope.
    let file_b = data.join("b.log");
    {
        let mut f = std::fs::File::create(&file_b).unwrap();
        writeln!(f, "zero").unwrap();
    }

    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // 1. file_search finds the preexisting "needle" line.
        let payload = first_text(
            &call_tool(
                &client,
                "file_search",
                serde_json::json!({
                    "path": file_a.to_string_lossy(),
                    "query": "needle",
                }),
            )
            .await,
        );
        let v: serde_json::Value = serde_json::from_str(&payload).expect("search json");
        let matches = v["matches"].as_array().expect("matches array");
        assert!(!matches.is_empty(), "search must find preexisting needle");
        assert_eq!(matches[0]["line"].as_u64(), Some(2));

        // 2. file_read_window returns a bounded window.
        let payload = first_text(
            &call_tool(
                &client,
                "file_read_window",
                serde_json::json!({
                    "path": file_a.to_string_lossy(),
                    "start_line": 1,
                    "max_lines": 2,
                }),
            )
            .await,
        );
        let v: serde_json::Value = serde_json::from_str(&payload).expect("read json");
        let lines = v["lines"].as_array().expect("lines");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["text"], "alpha");
        assert!(v["truncated"].as_bool().unwrap_or(false));

        // 3. Upsert rule, file_watch_start on both files.
        let _ = call_tool(
            &client,
            "registry_upsert",
            serde_json::json!({
                "definition_json": keyword_rule_json("tc43-kw", "needle", "needle_match"),
            }),
        )
        .await;
        let payload = first_text(
            &call_tool(
                &client,
                "file_watch_start",
                serde_json::json!({"path": file_a.to_string_lossy()}),
            )
            .await,
        );
        let v_a: serde_json::Value = serde_json::from_str(&payload).expect("watch a json");
        let bucket_a = v_a["bucket_id"].as_str().unwrap().to_owned();
        let watch_a = v_a["watch_id"].as_str().unwrap().to_owned();

        let payload = first_text(
            &call_tool(
                &client,
                "file_watch_start",
                serde_json::json!({"path": file_b.to_string_lossy()}),
            )
            .await,
        );
        let v_b: serde_json::Value = serde_json::from_str(&payload).expect("watch b json");
        let bucket_b = v_b["bucket_id"].as_str().unwrap().to_owned();

        // 4. Activate the rule with bucket scope = bucket A.
        let _ = call_tool(
            &client,
            "registry_activate",
            serde_json::json!({
                "rule_id": "tc43-kw",
                "scope": {"kind": "bucket", "bucket_id": bucket_a},
            }),
        )
        .await;

        // 5. Append matching content to BOTH files. Only bucket A
        // should produce signal.
        tokio::time::sleep(Duration::from_millis(200)).await;
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&file_a)
                .unwrap();
            writeln!(f, "needle inside A").unwrap();
        }
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&file_b)
                .unwrap();
            writeln!(f, "needle inside B").unwrap();
        }
        tokio::time::sleep(Duration::from_millis(900)).await;

        let wait_a = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_a,
                    "cursor": 0,
                    "timeout_ms": 600,
                }),
            )
            .await,
        );
        let wv_a: serde_json::Value = serde_json::from_str(&wait_a).expect("wait a");
        let events_a = wv_a["events"].as_array().cloned().unwrap_or_default();
        assert!(
            events_a
                .iter()
                .any(|e| e["kind"].as_str() == Some("needle_match")),
            "bucket A must emit needle_match; payload: {wait_a}"
        );

        let wait_b = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_b,
                    "cursor": 0,
                    "timeout_ms": 600,
                }),
            )
            .await,
        );
        let wv_b: serde_json::Value = serde_json::from_str(&wait_b).expect("wait b");
        for e in wv_b["events"].as_array().cloned().unwrap_or_default() {
            assert_ne!(
                e["kind"].as_str(),
                Some("needle_match"),
                "bucket B must NOT emit needle_match under bucket-A scope; payload: {wait_b}"
            );
        }

        // 6. Stop watch A.
        let _ = call_tool(
            &client,
            "file_watch_stop",
            serde_json::json!({"watch_id": watch_a}),
        )
        .await;

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_read_window_denies_sensitive_path_through_mcp() {
    let data = tmp_data_dir("deny");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        let mut params = CallToolRequestParams::new("file_read_window");
        params.arguments = Some(
            serde_json::json!({"path": "/etc/shadow"})
                .as_object()
                .cloned()
                .unwrap(),
        );
        let err = client
            .call_tool(params)
            .await
            .expect_err("sensitive path must be rejected");
        let msg = format!("{err}").to_ascii_lowercase();
        assert!(
            msg.contains("path_denied") || msg.contains("default-deny"),
            "expected path-denied error; got: {err}"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
