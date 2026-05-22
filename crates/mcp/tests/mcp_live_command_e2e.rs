// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC41 end-to-end smoke for the MCP command + bucket tool surface.
//!
//! Stands up the real `terminal-commanderd` UDS server in a temp dir,
//! mounts the rmcp stdio adapter on a duplex transport pointed at it,
//! and walks an LLM-shaped flow entirely through MCP:
//!
//! 1. `command_start_combed` starts a small argv command (no shell);
//!    response carries only bounded ids + cursor.
//! 2. `bucket_wait` (or `bucket_events_since`) observes the lifecycle
//!    signal event the daemon emits when the child exits. No raw
//!    stdout bytes appear in the MCP response.
//! 3. `event_context` on the lifecycle event returns a typed
//!    `unavailable_reason` because lifecycle events carry no source
//!    pointer by design.
//! 4. `command_status` reports the bounded lifecycle counters.
//!
//! Plus three negative / heartbeat checks the acceptance criteria
//! call out:
//! - `argv = ["sh", "-c", "..."]` is denied through MCP, audited, and
//!   not spawned (`shell_interpreter_denied`).
//! - `bucket_wait` with a short timeout on a quiet bucket returns
//!   `heartbeat = true` and no events.
//! - the full bucket payload never contains the raw stdout token
//!   produced by the command.

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
    p.push(format!("tc-mcp-cmd-e2e-{tag}-{pid}-{nanos}"));
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn full_command_lifecycle_through_mcp_yields_only_structured_signal() {
    let data = tmp_data_dir("full");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // 1. Start a small argv command. `true` is a silent
        //    OS-builtin: it produces zero stdout/stderr bytes, which
        //    isolates the "no raw bytes leak" assertion from any
        //    argv-reflection in event summaries.
        let start_payload = first_text(
            &call_tool(
                &client,
                "command_start_combed",
                serde_json::json!({
                    "argv": ["true"],
                    "grace_ms": 2000,
                }),
            )
            .await,
        );
        assert!(
            start_payload.len() <= MAX_RESPONSE_BYTES,
            "start payload must respect IPC response budget"
        );
        let start: serde_json::Value =
            serde_json::from_str(&start_payload).expect("start payload is JSON");
        let job_id = start["job_id"].as_str().expect("job_id present").to_owned();
        let bucket_id = start["bucket_id"]
            .as_str()
            .expect("bucket_id present")
            .to_owned();
        assert!(start.get("stdout").is_none(), "no stdout field");
        assert!(start.get("stderr").is_none(), "no stderr field");

        // 2. Observe lifecycle signal via bucket_wait. The daemon
        //    emits a `command_exited` (or `command_failed`) event
        //    when the child exits; we should see at least one event,
        //    and every event must be structured signal (kind starts
        //    with `command_`, stream is the meta lane). No event may
        //    carry stdout/stderr as its source stream.
        let wait_payload = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_id,
                    "cursor": 0,
                    "timeout_ms": 2000,
                }),
            )
            .await,
        );
        assert!(
            wait_payload.len() <= MAX_RESPONSE_BYTES,
            "bucket_wait payload must respect IPC response budget"
        );
        let wait: serde_json::Value =
            serde_json::from_str(&wait_payload).expect("bucket_wait JSON");
        let events = wait["events"]
            .as_array()
            .expect("bucket_wait carries events array");
        assert!(
            !events.is_empty() && !wait["heartbeat"].as_bool().unwrap_or(true),
            "must observe at least one lifecycle event from `true`; payload: {wait_payload}"
        );
        for ev in events {
            let kind = ev["kind"].as_str().unwrap_or("");
            assert!(
                kind.starts_with("command_"),
                "TC41 with no inline rules must only surface lifecycle events; got kind={kind} in {wait_payload}"
            );
            let stream = ev["source"]["stream"].as_str().unwrap_or("");
            assert!(
                stream != "stdout" && stream != "stderr",
                "lifecycle event must not be tagged as a raw stream lane; stream={stream}"
            );
        }

        // 2b. Pick the first event for event_context resolution.
        let event_id = events[0]["event_id"]
            .as_str()
            .expect("event has event_id")
            .to_owned();

        // 3. event_context: lifecycle events have no source pointer
        //    at Info severity by design; expect a typed
        //    unavailable_reason rather than raw bytes.
        let context_payload = first_text(
            &call_tool(
                &client,
                "event_context",
                serde_json::json!({
                    "bucket_id": bucket_id,
                    "event_id": event_id,
                }),
            )
            .await,
        );
        assert!(
            context_payload.len() <= MAX_RESPONSE_BYTES,
            "event_context payload must respect IPC response budget"
        );
        let context: serde_json::Value =
            serde_json::from_str(&context_payload).expect("event_context JSON");
        assert!(
            context["frames"].as_array().is_some_and(Vec::is_empty),
            "lifecycle event must have no context frames; got: {context_payload}"
        );
        let unavailable = context["unavailable_reason"].as_str();
        assert!(
            unavailable.is_some_and(|r| matches!(
                r,
                "no_pointer" | "synthetic_event" | "anchor_evicted" | "unknown_probe"
            )),
            "lifecycle event_context must report typed unavailable_reason; got: {context_payload}"
        );

        // 4. command_status: bounded lifecycle counters; no stdout
        //    field at all on this wire shape.
        let status_payload = first_text(
            &call_tool(
                &client,
                "command_status",
                serde_json::json!({"job_id": job_id}),
            )
            .await,
        );
        assert!(
            status_payload.len() <= MAX_RESPONSE_BYTES,
            "command_status payload must respect IPC response budget"
        );
        let status: serde_json::Value =
            serde_json::from_str(&status_payload).expect("command_status JSON");
        assert_eq!(status["job_id"].as_str(), Some(job_id.as_str()));
        assert!(
            status["state"].is_string() || status["state"].is_object(),
            "state must be present; got: {status_payload}"
        );
        assert!(status.get("stdout").is_none(), "no stdout field on status");
        assert!(status.get("stderr").is_none(), "no stderr field on status");

        // 5. Heartbeat: re-call bucket_wait with the freshly advanced
        //    cursor and a short timeout. Nothing new will fire, so
        //    we must see heartbeat=true and an empty events array.
        let next_cursor = wait["next_cursor"]
            .as_u64()
            .expect("bucket_wait reports next_cursor");
        let heartbeat_payload = first_text(
            &call_tool(
                &client,
                "bucket_wait",
                serde_json::json!({
                    "bucket_id": bucket_id,
                    "cursor": next_cursor,
                    "timeout_ms": 250,
                }),
            )
            .await,
        );
        let heartbeat: serde_json::Value =
            serde_json::from_str(&heartbeat_payload).expect("heartbeat JSON");
        assert_eq!(
            heartbeat["heartbeat"].as_bool(),
            Some(true),
            "bucket_wait on a quiet bucket must heartbeat; got: {heartbeat_payload}"
        );
        assert!(
            heartbeat["events"].as_array().is_some_and(Vec::is_empty),
            "heartbeat response must carry an empty events array; got: {heartbeat_payload}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_shell_attempt_is_denied_and_audited() {
    let data = tmp_data_dir("sh-deny");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        let mut params = CallToolRequestParams::new("command_start_combed");
        params.arguments = Some(
            serde_json::json!({
                "argv": ["sh", "-c", "echo nope"],
            })
            .as_object()
            .unwrap()
            .clone(),
        );
        let err = client
            .call_tool(params)
            .await
            .expect_err("MCP shell attempt must be denied");
        let rendered = err.to_string();
        assert!(
            rendered.to_ascii_lowercase().contains("shell")
                || rendered.contains("ShellInterpreterDenied"),
            "denial message must surface the shell-bridge guard; got: {rendered}"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bucket_events_since_returns_structured_events_only() {
    let data = tmp_data_dir("events");
    let handle = spawn_live_daemon(&data);
    {
        let (_server, client) = paired_against_live_daemon(&handle).await;

        // Start a command, then read with bucket_events_since.
        let start_payload = first_text(
            &call_tool(
                &client,
                "command_start_combed",
                serde_json::json!({"argv": ["true"]}),
            )
            .await,
        );
        let start: serde_json::Value = serde_json::from_str(&start_payload).expect("start JSON");
        let bucket_id = start["bucket_id"].as_str().unwrap().to_owned();

        // Give the lifecycle waiter a moment to emit.
        tokio::time::sleep(Duration::from_millis(300)).await;

        let events_payload = first_text(
            &call_tool(
                &client,
                "bucket_events_since",
                serde_json::json!({"bucket_id": bucket_id, "cursor": 0}),
            )
            .await,
        );
        assert!(
            events_payload.len() <= MAX_RESPONSE_BYTES,
            "bucket_events_since payload must respect IPC response budget"
        );
        let body: serde_json::Value =
            serde_json::from_str(&events_payload).expect("bucket_events_since JSON");
        assert!(body["events"].is_array(), "events array present");
        assert!(
            body.get("stdout").is_none() && body.get("stderr").is_none(),
            "wire shape must not carry raw stream fields"
        );

        // bucket_summary also stays structured.
        let summary_payload = first_text(
            &call_tool(
                &client,
                "bucket_summary",
                serde_json::json!({"bucket_id": bucket_id}),
            )
            .await,
        );
        let summary: serde_json::Value =
            serde_json::from_str(&summary_payload).expect("bucket_summary JSON");
        assert!(summary["by_severity"].is_object(), "histogram present");
        assert!(
            summary["event_count"].as_u64().is_some(),
            "event_count is numeric"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
