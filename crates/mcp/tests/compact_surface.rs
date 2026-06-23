// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! End-to-end surface behavior tests for `TC_SURFACE`.
//!
//! Verifies four contracts using the same in-process rmcp duplex transport
//! harness as `daemon_unavailable_envelope.rs`:
//!
//! 1. Default surface (no override): `tools/list` contains legacy names
//!    (`"run_and_watch"`, `"command_status"`) and does NOT contain `"command"`.
//! 2. Compact surface override: `tools/list` contains `"command"` and does NOT
//!    contain `"command_status"` (a legacy name).
//! 3. Compact surface override: `tools/call "command" {action:"run_and_watch"}`
//!    REACHES the `run_and_watch` handler -- proven by a `daemon_unavailable`
//!    envelope in the error (NOT a "tool not found" / gate rejection).
//! 4. Compact surface override: `tools/call "command_status" {...}` is REJECTED
//!    by the surface gate with a message naming the compact surface and
//!    `TC_SURFACE=full`.
//!
//! Harness copied from `crates/mcp/tests/daemon_unavailable_envelope.rs`.
//!
//! The surface is injected via `TerminalCommanderMcpServer::with_surface`
//! (a `#[doc(hidden)]` builder seam, test-only by intent, that bypasses the
//! live `TC_SURFACE` env read). This avoids `std::env::set_var`, which is
//! `unsafe` under edition
//! 2024 and cannot be used because `unsafe_code` is `forbid` workspace-wide.
//! No process-global env mutation; tests may run in any thread count.

use std::path::PathBuf;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ServiceExt};

use terminal_commander_mcp::daemon_client::{DaemonStatusHandle, McpDaemonClient};
use terminal_commander_mcp::surface::Surface;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commander_supervisor::ensure::{
    DaemonUnavailableReason, Diagnostics, Endpoint, EnsureDaemonStatus,
};

// ---------------------------------------------------------------------------
// Harness (copied from daemon_unavailable_envelope.rs, cross-platform)
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

fn unique_socket_path() -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());

    #[cfg(windows)]
    return PathBuf::from(format!(r"\\.\pipe\tc-compact-surface-test-{pid}-{nanos}"));

    #[cfg(not(windows))]
    std::env::temp_dir().join(format!("tc-compact-surface-test-{pid}-{nanos}.sock"))
}

fn make_unavailable_status() -> EnsureDaemonStatus {
    let path = unique_socket_path();

    #[cfg(windows)]
    let endpoint = Endpoint::WindowsPipe {
        name: path.to_string_lossy().into_owned(),
    };

    #[cfg(not(windows))]
    let endpoint = Endpoint::UnixSocket { path };

    EnsureDaemonStatus::Unavailable {
        reason: DaemonUnavailableReason::BinaryNotFound,
        diagnostics: Diagnostics {
            endpoint,
            log_path: None,
            last_error: Some("test: binary not found".into()),
            startup_attempted: false,
            startup_elapsed_ms: 0,
        },
    }
}

/// Build an in-process server/client pair backed by an unavailable daemon,
/// with an explicit surface injected via the test seam.
async fn paired_service(
    surface: Surface,
) -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let socket = unique_socket_path();
    let status = DaemonStatusHandle::new(make_unavailable_status());
    let daemon =
        McpDaemonClient::with_status(socket, status).with_timeout(Duration::from_millis(150));
    let server = TerminalCommanderMcpServer::new(daemon).with_surface(surface);

    let server_handle =
        tokio::spawn(async move { server.serve(server_transport).await.expect("server serve") });
    let client = TestClient
        .serve(client_transport)
        .await
        .expect("client serve");
    let server = server_handle.await.expect("server task join");
    (server, client)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// 1. Full surface: `tools/list` contains legacy names and does NOT contain
///    the compact facade name `"command"`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_surface_lists_legacy_not_command() {
    let (_server, client) = paired_service(Surface::Full).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

    // Must contain at least two well-known legacy names.
    assert!(
        names.contains(&"run_and_watch".to_owned()),
        "full surface must expose 'run_and_watch'; got: {names:?}"
    );
    assert!(
        names.contains(&"command_status".to_owned()),
        "full surface must expose 'command_status'; got: {names:?}"
    );

    // The compact facade MUST NOT be visible on the full surface.
    assert!(
        !names.contains(&"command".to_owned()),
        "full surface must NOT expose 'command' (compact facade); got: {names:?}"
    );

    let _ = client.cancel().await;
}

/// 2. Compact surface: `tools/list` contains `"command"` and does NOT contain
///    `"command_status"` (a legacy name).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compact_surface_lists_command_not_legacy() {
    let (_server, client) = paired_service(Surface::Compact).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

    // The facade MUST be visible.
    assert!(
        names.contains(&"command".to_owned()),
        "compact surface must expose 'command'; got: {names:?}"
    );

    // Legacy granular names MUST NOT be visible.
    assert!(
        !names.contains(&"command_status".to_owned()),
        "compact surface must NOT expose 'command_status'; got: {names:?}"
    );

    let _ = client.cancel().await;
}

/// 3. Compact surface: `tools/call "command" {action:"run_and_watch",...}`
///    REACHES the `run_and_watch` handler. With no daemon the handler returns
///    a `daemon_unavailable` envelope -- proving the gate LET IT THROUGH (not
///    a "tool not found" rejection and not a gate rejection).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compact_surface_command_run_and_watch_reaches_handler() {
    let (_server, client) = paired_service(Surface::Compact).await;

    let args: rmcp::model::JsonObject = serde_json::from_value(serde_json::json!({
        "action": "run_and_watch",
        "argv": ["echo", "hello"]
    }))
    .expect("args object");
    let params = CallToolRequestParams::new("command").with_arguments(args);

    let result = client.call_tool(params).await;

    // The gate admitted "command" under compact surface. With no daemon the
    // handler returns an error envelope whose rendered form contains
    // "daemon_unavailable" -- NOT a surface gate rejection or tool-not-found.
    match result {
        Err(err) => {
            let rendered = err.to_string();
            assert!(
                rendered.contains("daemon_unavailable"),
                "expected daemon_unavailable envelope from run_and_watch handler; got: {rendered}"
            );
            assert!(
                !rendered.contains("compact surface"),
                "must NOT be a surface gate rejection; got: {rendered}"
            );
            assert!(
                !rendered.contains("tool not found"),
                "must NOT be a tool-not-found error; got: {rendered}"
            );
        }
        Ok(result) => {
            // A real daemon answered; any non-empty result also proves the
            // handler was reached (not rejected at the gate).
            assert!(
                !result.content.is_empty(),
                "run_and_watch handler must return non-empty content"
            );
        }
    }

    let _ = client.cancel().await;
}

/// 4. Compact surface: `tools/call "command_status" {...}` is REJECTED by the
///    surface gate (invalid_request) with a message naming the compact surface
///    and instructing the caller to set `TC_SURFACE=full`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compact_surface_rejects_legacy_tool_at_gate() {
    let (_server, client) = paired_service(Surface::Compact).await;

    let args: rmcp::model::JsonObject =
        serde_json::from_value(serde_json::json!({ "job_id": "job_test_x" })).expect("args object");
    let params = CallToolRequestParams::new("command_status").with_arguments(args);

    let err = client
        .call_tool(params)
        .await
        .expect_err("legacy tool must be rejected on compact surface");

    let rendered = err.to_string();

    // Must mention the surface boundary.
    assert!(
        rendered.contains("compact surface"),
        "gate rejection must mention compact surface; got: {rendered}"
    );

    // Must tell the caller how to escape.
    assert!(
        rendered.contains("TC_SURFACE=full"),
        "gate rejection must include TC_SURFACE=full hint; got: {rendered}"
    );

    let _ = client.cancel().await;
}
