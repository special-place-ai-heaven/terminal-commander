// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Phase 2: the `subscription_pull` notification nudge is GUARDED
//! (`TC_MCP_NOTIFY=1`) and BEST-EFFORT. It rides the open stdio pipe as an
//! in-process `notifications/message` send -- no spawn/fs/socket (facade-legal).
//!
//! Proven by DIRECT observation: a recording `ClientHandler` captures
//! `on_logging_message`. The gate is injected per-server via the test seam
//! `TerminalCommanderMcpServer::with_notify` (NOT process-global env, because
//! `std::env::set_var` is `unsafe` under edition 2024 and the crate forbids
//! `unsafe_code`); production reads `TC_MCP_NOTIFY` once in `new`. Two tests:
//!   - flag OFF + non-empty pull  -> NO notification (default-off contract).
//!   - flag ON  + idle/empty pull -> NO notification (never on the idle path),
//!     and flag ON + non-empty pull -> EXACTLY ONE notification whose `count`
//!     mirrors the delivered batch, logger tagged `terminal-commander`.
//! A send error is always ignored -- delivery of events is the pull, never this
//! notification.
//!
//! Linux/WSL only (UDS direct-seed client + the authoritative gate).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, LoggingMessageNotificationParam};
use rmcp::service::NotificationContext;
use rmcp::{ClientHandler, RoleClient, ServiceExt};

use terminal_commander_core::{
    ContextHint, RuleDefinition, RuleStatus, RuleType, Severity, SourceStream,
};
use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commanderd::{
    CommandStartParams, DaemonClient, DaemonConfig, DaemonState, IpcRequest, IpcResponse,
    IpcServer, ServerHandle,
};

/// A client handler that records every `notifications/message` it receives.
#[derive(Clone, Default)]
struct RecordingClient {
    logs: Arc<Mutex<Vec<LoggingMessageNotificationParam>>>,
}

impl ClientHandler for RecordingClient {
    async fn on_logging_message(
        &self,
        params: LoggingMessageNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) {
        self.logs.lock().expect("logs lock").push(params);
    }
}

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-mcp-notify-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn spawn_live_daemon(data: &std::path::Path) -> ServerHandle {
    let cfg = DaemonConfig::defaults_in(data);
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("daemon bootstrap"));
    let socket = state.config.socket_path();
    IpcServer::new(Arc::clone(&state), socket)
        .spawn()
        .expect("ipc server spawn")
}

/// A HIGH-severity keyword rule so its events pass `severity_min: high`.
fn high_sev_keyword_rule() -> RuleDefinition {
    RuleDefinition {
        id: "notify.needle".to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::High,
        event_kind: "needle_hit".to_owned(),
        stream: Some(SourceStream::Stdout),
        description: None,
        pattern: None,
        keywords: Some(vec!["NEEDLE".to_owned()]),
        captures: vec![],
        summary_template: "needle".to_owned(),
        tags: vec![],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

fn noisy_start_params() -> CommandStartParams {
    CommandStartParams {
        environment: None,
        argv: vec![
            "printf".to_owned(),
            "NEEDLE a\nNEEDLE b\nNEEDLE c\nNEEDLE d\n".to_owned(),
        ],
        cwd: None,
        env: Vec::new(),
        bucket_config: None,
        rules: vec![high_sev_keyword_rule()],
        grace_ms: Some(2_000),
        tag: None,
        dedup_nonce: None,
        strip_ansi: true,
    }
}

async fn paired(
    handle: &ServerHandle,
    recorder: RecordingClient,
    notify: bool,
) -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, RecordingClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let daemon = McpDaemonClient::new(handle.socket_path().to_path_buf())
        .with_timeout(Duration::from_secs(5));
    // `with_notify` bypasses the TC_MCP_NOTIFY env read so the test never
    // mutates process-global env (set_var is `unsafe` under edition 2024 and
    // the crate forbids unsafe_code).
    let server = TerminalCommanderMcpServer::with_notify(daemon, notify);
    let server_handle =
        tokio::spawn(async move { server.serve(server_transport).await.expect("server serve") });
    let client = recorder
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
    client: &rmcp::service::RunningService<rmcp::RoleClient, RecordingClient>,
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

/// Open a fresh subscription over all sources, return its sub_id.
async fn open_sub(
    client: &rmcp::service::RunningService<rmcp::RoleClient, RecordingClient>,
) -> String {
    let open_payload = first_text(
        &call_tool(
            client,
            "subscription_open",
            serde_json::json!({ "severity_min": "high", "sources": { "kind": "all" } }),
        )
        .await,
    );
    let open_v: serde_json::Value = serde_json::from_str(&open_payload).expect("open json");
    open_v["sub_id"].as_str().expect("sub_id").to_owned()
}

/// Start a noisy command via a direct daemon client, then pull `sub_id` until a
/// non-empty batch arrives (within a bounded budget). Returns the batch size.
async fn seed_and_pull_nonempty(
    handle: &ServerHandle,
    client: &rmcp::service::RunningService<rmcp::RoleClient, RecordingClient>,
    sub_id: &str,
) -> usize {
    let direct =
        DaemonClient::new(handle.socket_path().to_path_buf()).with_timeout(Duration::from_secs(10));
    let started = direct
        .call(1, IpcRequest::CommandStartCombed(noisy_start_params()))
        .await
        .expect("command_start_combed");
    assert!(matches!(started, IpcResponse::CommandStartCombed(_)));

    for _ in 0..30 {
        let pull_payload = first_text(
            &call_tool(
                client,
                "subscription_pull",
                serde_json::json!({ "sub_id": sub_id, "max": 50, "timeout_ms": 1500 }),
            )
            .await,
        );
        let v: serde_json::Value = serde_json::from_str(&pull_payload).expect("pull json");
        let n = v["events"].as_array().map_or(0, std::vec::Vec::len);
        if n > 0 {
            return n;
        }
    }
    panic!("never observed a non-empty pull within budget");
}

/// Brief drain so any in-flight `notifications/message` reaches the recorder.
async fn drain() {
    tokio::time::sleep(Duration::from_millis(250)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notify_off_sends_no_message_on_nonempty_pull() {
    // Default OFF: even a non-empty pull must NOT emit a notification.
    let data = tmp_data_dir("off");
    let handle = spawn_live_daemon(&data);
    {
        let recorder = RecordingClient::default();
        let logs = Arc::clone(&recorder.logs);
        let (_server, client) = paired(&handle, recorder, false).await;

        let sub_id = open_sub(&client).await;
        let n = seed_and_pull_nonempty(&handle, &client, &sub_id).await;
        assert!(n > 0, "precondition: pull delivered events");
        drain().await;
        assert_eq!(
            logs.lock().expect("logs lock").len(),
            0,
            "flag OFF: no notifications/message on a non-empty pull"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notify_on_sends_one_message_on_nonempty_only_never_idle() {
    let data = tmp_data_dir("on");
    let handle = spawn_live_daemon(&data);
    {
        let recorder = RecordingClient::default();
        let logs = Arc::clone(&recorder.logs);
        let (_server, client) = paired(&handle, recorder, true).await;

        // Idle pull on a fresh sub (no command) -> NO notification.
        let idle_sub = open_sub(&client).await;
        let _ = first_text(
            &call_tool(
                &client,
                "subscription_pull",
                serde_json::json!({ "sub_id": idle_sub, "timeout_ms": 200 }),
            )
            .await,
        );
        drain().await;
        assert_eq!(
            logs.lock().expect("logs lock").len(),
            0,
            "flag ON: an idle/empty pull MUST NOT notify"
        );

        // Non-empty pull -> exactly one notification, count mirrors the batch.
        let live_sub = open_sub(&client).await;
        let n = seed_and_pull_nonempty(&handle, &client, &live_sub).await;
        assert!(n > 0, "precondition: pull delivered events");
        drain().await;

        // Snapshot under the lock, then drop the guard BEFORE any await so the
        // std Mutex guard never spans an await point (clippy::await_holding_lock).
        let captured: Vec<LoggingMessageNotificationParam> =
            logs.lock().expect("logs lock").clone();
        assert_eq!(
            captured.len(),
            1,
            "flag ON: exactly one notification on the non-empty pull"
        );
        let msg = &captured[0];
        assert_eq!(msg.logger.as_deref(), Some("terminal-commander"));
        assert_eq!(
            msg.data["count"].as_u64(),
            Some(n as u64),
            "notification count mirrors the delivered batch: {:?}",
            msg.data
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}
