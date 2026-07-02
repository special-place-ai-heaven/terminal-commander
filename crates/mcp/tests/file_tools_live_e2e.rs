// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-mcp-file-{tag}-{pid}-{nanos}-{n}"));
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

/// TC22 A3 end-to-end through the real rmcp stdio adapter: file_write creates
/// a file with the exact content (response reports the canonical path + byte
/// count), and file_read_window reads the same bytes back. Proves the full
/// MCP -> daemon -> filesystem write path works through the production surface.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_write_then_read_back_through_mcp() {
    let data = tmp_data_dir("write");
    let handle = spawn_live_daemon(&data);
    std::fs::create_dir_all(&data).unwrap();
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        let target = data.join("nested/out.txt");
        let content = "first line\nsecond line\n";

        // Write through the MCP tool (create_dirs builds the nested parent).
        let payload = first_text(
            &call_tool(
                &client,
                "file_write",
                serde_json::json!({
                    "path": target.to_string_lossy(),
                    "content": content,
                    "create_dirs": true,
                }),
            )
            .await,
        );
        let v: serde_json::Value = serde_json::from_str(&payload).expect("write json");
        assert_eq!(
            v["bytes_written"].as_u64(),
            Some(content.len() as u64),
            "write response must report the byte count; payload: {payload}"
        );

        // The file exists on disk with the EXACT content.
        let on_disk = std::fs::read_to_string(&target).expect("written file must exist");
        assert_eq!(on_disk, content, "written content must be exact");

        // Read it back through file_read_window to prove the round-trip.
        let payload = first_text(
            &call_tool(
                &client,
                "file_read_window",
                serde_json::json!({"path": target.to_string_lossy()}),
            )
            .await,
        );
        let v: serde_json::Value = serde_json::from_str(&payload).expect("read json");
        let lines = v["lines"].as_array().expect("lines");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["text"], "first line");
        assert_eq!(lines[1]["text"], "second line");

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

/// US3 (FR-020/021) end-to-end: on the default-deny developer_local profile
/// (no shell, no session caps), the `files` facade `list` action enumerates a
/// project directory through TC alone — the exact operation that was impossible
/// in the dogfood round. Dirs sort before files; a file entry carries a size.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_list_dir_through_mcp_on_default_deny_profile() {
    let data = tmp_data_dir("list");
    let handle = spawn_live_daemon(&data);
    std::fs::create_dir_all(&data).unwrap();
    let dir = data.join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir(dir.join("src")).unwrap();
    {
        let mut f = std::fs::File::create(dir.join("Cargo.toml")).unwrap();
        writeln!(f, "[package]").unwrap();
    }
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // The default-deny profile grants no shell/session caps; directory
        // listing is gated only by the read-path policy, so a readable project
        // directory enumerates in ONE call.
        let payload = first_text(
            &call_tool(
                &client,
                "files",
                serde_json::json!({"action": "list", "path": dir.to_string_lossy()}),
            )
            .await,
        );
        let v: serde_json::Value = serde_json::from_str(&payload).expect("list json");
        let entries = v["entries"].as_array().expect("entries array");
        let names: Vec<&str> = entries.iter().filter_map(|e| e["name"].as_str()).collect();
        assert!(
            names.contains(&"src"),
            "listing must include the src dir; payload: {payload}"
        );
        assert!(
            names.contains(&"Cargo.toml"),
            "listing must include Cargo.toml; payload: {payload}"
        );
        // Dirs sort before files: src (dir) precedes Cargo.toml (file).
        let src_idx = names.iter().position(|n| *n == "src").unwrap();
        let cargo_idx = names.iter().position(|n| *n == "Cargo.toml").unwrap();
        assert!(
            src_idx < cargo_idx,
            "dirs must sort before files; got {names:?}"
        );
        let src = entries.iter().find(|e| e["name"] == "src").unwrap();
        assert_eq!(src["kind"], "dir");
        let cargo = entries.iter().find(|e| e["name"] == "Cargo.toml").unwrap();
        assert_eq!(cargo["kind"], "file");
        assert!(
            cargo["size_bytes"].as_u64().is_some(),
            "a file entry must carry size_bytes; payload: {payload}"
        );
        assert_eq!(v["truncated"], false);
        assert!(v["total_entries"].as_u64().unwrap_or(0) >= 2);

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

/// TC22 A3 end-to-end: file_write to a default-deny sensitive path is denied
/// through the MCP adapter and creates no file.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_write_denies_sensitive_path_through_mcp() {
    let data = tmp_data_dir("write-deny");
    let handle = spawn_live_daemon(&data);
    std::fs::create_dir_all(&data).unwrap();
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;
        // Build the parent so the deny is purely the policy verdict, not a
        // missing-parent error. `.ssh/id_rsa` matches a default-deny suffix.
        let ssh_dir = data.join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();
        let secret = ssh_dir.join("id_rsa");

        let mut params = CallToolRequestParams::new("file_write");
        params.arguments = Some(
            serde_json::json!({
                "path": secret.to_string_lossy(),
                "content": "BEGIN OPENSSH PRIVATE KEY\n",
            })
            .as_object()
            .cloned()
            .unwrap(),
        );
        let err = client
            .call_tool(params)
            .await
            .expect_err("sensitive write target must be rejected");
        let msg = format!("{err}").to_ascii_lowercase();
        assert!(
            msg.contains("path_denied") || msg.contains("default-deny"),
            "expected path-denied error; got: {err}"
        );
        assert!(!secret.exists(), "denied write must not create the file");

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
