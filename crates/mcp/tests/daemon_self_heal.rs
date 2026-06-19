// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Integration test: daemon availability self-heal (audit H1, FIX D).
//!
//! `EnsureDaemonStatus` is sampled once at MCP startup. A daemon that was
//! slow to bind (a transient `StartupTimeout`) would otherwise pin every
//! daemon-backed tool to `daemon_unavailable` for the whole process life,
//! even after the socket goes live — the agent cannot restart the MCP
//! process, so it permanently falls back to raw shell.
//!
//! This test stands up a REAL `terminal-commanderd` UDS IPC server, then
//! builds the adapter with a status handle that reports `Unavailable`
//! (simulating the slow-bind startup sample) while the socket actually
//! points at the live daemon. A daemon-backed tool call must:
//!   1. trigger a bounded, single-flight `Health` re-probe,
//!   2. observe the daemon is live, flip the cached status to available,
//!   3. proceed and return a real payload.
//!
//! Single-flight is asserted directly: many concurrent tool calls against
//! a freshly-healed handle fire AT MOST ONE `Health` probe (the rest
//! coalesce on the probe guard and see the already-healed status).
//!
//! The complementary "still-down daemon keeps returning the envelope"
//! guarantee is covered by `daemon_unavailable_envelope.rs` and by the
//! `try_self_heal_keeps_status_unavailable_when_daemon_down` unit test.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ServiceExt};

use terminal_commander_mcp::daemon_client::{DaemonStatusHandle, McpDaemonClient};
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commander_supervisor::ensure::{
    DaemonUnavailableReason, Diagnostics, Endpoint, EnsureDaemonStatus,
};
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
    p.push(format!("tc-mcp-self-heal-{tag}-{pid}-{nanos}-{n}"));
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

/// An `Unavailable` startup status whose recorded endpoint points at
/// `socket` (the live daemon). This is the slow-bind sample: the daemon
/// became reachable AFTER the one-shot startup probe gave up.
fn stale_unavailable_status(socket: &std::path::Path) -> EnsureDaemonStatus {
    EnsureDaemonStatus::Unavailable {
        reason: DaemonUnavailableReason::StartupTimeout,
        diagnostics: Diagnostics {
            endpoint: Endpoint::UnixSocket {
                path: socket.to_path_buf(),
            },
            log_path: None,
            last_error: Some("test: slow-bind startup timeout".into()),
            startup_attempted: true,
            startup_elapsed_ms: 10_000,
        },
    }
}

/// Pair an adapter (configured with the given status handle, socket
/// pointed at the live daemon) with an in-process rmcp client.
async fn paired_with_status(
    handle: &ServerHandle,
    status: DaemonStatusHandle,
) -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let daemon = McpDaemonClient::with_status(handle.socket_path().to_path_buf(), status)
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
    panic!("expected text content in call result");
}

/// A stale-unavailable status pointed at a NOW-LIVE daemon must self-heal:
/// the tool call re-probes, observes the daemon, flips the flag, and
/// returns a real payload instead of the daemon_unavailable envelope.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn self_heal_flips_unavailable_to_available_against_live_daemon() {
    let data = tmp_data_dir("flip");
    let handle = spawn_live_daemon(&data);
    {
        let status = DaemonStatusHandle::new(stale_unavailable_status(handle.socket_path()));
        assert!(
            status.is_unavailable(),
            "precondition: status starts unavailable"
        );
        let (_server, client) = paired_with_status(&handle, status.clone()).await;

        // health requires the daemon; with a stale-unavailable status it
        // would return the envelope WITHOUT self-heal. With self-heal it
        // must succeed against the live daemon.
        let result = client
            .call_tool(CallToolRequestParams::new("health"))
            .await
            .expect("health must succeed after self-heal flips status to available");
        let body: serde_json::Value =
            serde_json::from_str(&first_text(&result)).expect("health payload is JSON");
        assert_eq!(body["ok"], serde_json::Value::Bool(true));
        assert!(
            body["uptime_secs"].is_number(),
            "healed health payload must include uptime_secs; got {body}"
        );
        // The live daemon populates its own crate version; the MCP tool
        // surfaces it so a client can assert WHICH build is running.
        assert_eq!(
            body["version"].as_str(),
            Some(env!("CARGO_PKG_VERSION")),
            "healed health payload must carry the live daemon's version; got {body}"
        );

        // The cached status must now be available (the flag was cleared),
        // and exactly one probe was fired to observe the live daemon.
        assert!(
            !status.is_unavailable(),
            "status must be flipped to available after a successful re-probe"
        );
        assert_eq!(
            status.probe_count(),
            1,
            "exactly one self-heal probe should have flipped the status"
        );

        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    cleanup(&data);
}

/// Single-flight: many concurrent tool calls arriving while the status is
/// still unavailable must coalesce into AT MOST ONE `Health` re-probe; the
/// losers re-check under the guard and see the already-healed status. All
/// calls must succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn self_heal_is_single_flight_under_concurrency() {
    let data = tmp_data_dir("singleflight");
    let handle = spawn_live_daemon(&data);
    {
        let status = DaemonStatusHandle::new(stale_unavailable_status(handle.socket_path()));
        let (_server, client) = paired_with_status(&handle, status.clone()).await;
        let client = Arc::new(client);

        let mut tasks = Vec::new();
        for _ in 0..24 {
            let c = Arc::clone(&client);
            tasks.push(tokio::spawn(async move {
                c.call_tool(CallToolRequestParams::new("health")).await
            }));
        }
        for t in tasks {
            let result = t
                .await
                .expect("join health task")
                .expect("every concurrent health call must succeed after self-heal");
            let body: serde_json::Value =
                serde_json::from_str(&first_text(&result)).expect("health payload is JSON");
            assert_eq!(body["ok"], serde_json::Value::Bool(true));
        }

        assert!(!status.is_unavailable(), "status healed");
        assert_eq!(
            status.probe_count(),
            1,
            "single-flight: concurrent self-heal attempts must coalesce into ONE probe"
        );

        if let Ok(inner) = Arc::try_unwrap(client) {
            let _ = inner.cancel().await;
        }
    }
    handle.shutdown().await;
    cleanup(&data);
}
