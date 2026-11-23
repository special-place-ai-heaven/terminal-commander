// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! End-to-end tests for `TC_SURFACE=code` (3-tool Code Mode MCP surface).
//!
//! Verifies:
//! 1. `tools/list` on `Surface::Code` advertises exact 3 tools (`code_search`,
//!    `code_execute`, `status`) with flat object schemas and serialized byte
//!    budget `<= 4,500` B.
//! 2. `Surface::Full` stays untouched at exact 60 tools (neither `code_search`
//!    nor `code_execute` leaks onto `Surface::Full`), and `tool_catalogue()`
//!    stays exact 60 entries.
//! 3. `ServerHandler::get_info().instructions` on `Surface::Code` returns
//!    `CODE_SURFACE_INSTRUCTIONS` (`<= 850` chars), while `Surface::Full` keeps
//!    its default instructions untouched.
//! 4. Admission gate rejects unlisted granular/compact tools on `Surface::Code`
//!    with guidance naming `code_execute` and `TC_SURFACE=full`.
//! 5. Offline `code_search` works without a live daemon; offline `status`
//!    surfaces the `daemon_unavailable` envelope; offline `code_execute` calling
//!    a daemon op surfaces `daemon_unavailable` inside the structured outcome.
//! 6. Live daemon e2e: `status()`, `code_search()`, and multi-step Python
//!    execution (`run_and_watch` + `bucket_events_since` + `command_output_tail`)
//!    over `code_execute()`, plus remote `target` federation routing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ClientServiceExt, ServerHandler, ServiceExt};
use serde_json::{Value, json};

use terminal_commander_mcp::code_mode::CODE_SURFACE_INSTRUCTIONS;
use terminal_commander_mcp::daemon_client::{DaemonStatusHandle, McpDaemonClient};
use terminal_commander_mcp::surface::Surface;
use terminal_commander_mcp::tools::{TerminalCommanderMcpServer, tool_catalogue};
use terminal_commander_supervisor::ensure::{
    DaemonUnavailableReason, Diagnostics, Endpoint, EnsureDaemonStatus,
};
#[cfg(unix)]
use terminal_commanderd::IpcServer;
#[cfg(windows)]
use terminal_commanderd::PipeServer;
use terminal_commanderd::{
    DaemonConfig, DaemonState, PolicyProfile, RemoteTarget, RemoteTransport, TargetsConfig,
};

#[derive(Default, Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

fn next_seq() -> u64 {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

fn unique_socket_path(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let seq = next_seq();

    #[cfg(windows)]
    return PathBuf::from(format!(
        r"\\.\pipe\tc-code-surface-{tag}-{pid}-{nanos}-{seq}"
    ));

    #[cfg(not(windows))]
    std::env::temp_dir().join(format!("tc-code-surface-{tag}-{pid}-{nanos}-{seq}.sock"))
}

fn tmp_data_dir(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seq = next_seq();
    std::env::temp_dir().join(format!("tc-code-surface-live-{tag}-{pid}-{nanos}-{seq}"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

fn make_unavailable_status() -> EnsureDaemonStatus {
    let path = unique_socket_path("down");

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

async fn paired_offline_service(
    surface: Surface,
) -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let socket = unique_socket_path("offline");
    let status = DaemonStatusHandle::new(make_unavailable_status());
    let daemon =
        McpDaemonClient::with_status(socket, status).with_timeout(Duration::from_millis(150));
    let server = TerminalCommanderMcpServer::new(daemon).with_surface(surface);

    let server_handle =
        tokio::spawn(async move { server.serve(server_transport).await.expect("server serve") });
    let client = TestClient
        .serve_with_lifecycle(
            client_transport,
            rmcp::ClientLifecycleMode::Discover {
                preferred_versions: vec![rmcp::model::ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("client serve");
    // rmcp >= 3.5.1: server answers discover, then awaits the first non-discover request.
    client.list_all_tools().await.expect("first real request");
    let server = server_handle.await.expect("server task join");
    (server, client)
}

#[cfg(unix)]
fn start_live_daemon(data_dir: &Path, profile: PolicyProfile) -> (PathBuf, impl Sized) {
    let mut cfg = DaemonConfig::defaults_in(data_dir);
    cfg.policy.profile = profile;
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("daemon bootstrap"));
    let socket = state.config.socket_path();
    let handle = IpcServer::new(state, socket)
        .spawn()
        .expect("ipc server spawn");
    (handle.socket_path().to_path_buf(), handle)
}

#[cfg(windows)]
fn start_live_daemon(data_dir: &Path, profile: PolicyProfile) -> (PathBuf, impl Sized) {
    let mut cfg = DaemonConfig::defaults_in(data_dir);
    cfg.policy.profile = profile;
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("daemon bootstrap"));
    let pipe_path = unique_socket_path("live");
    let pipe = pipe_path.to_string_lossy().into_owned();
    let handle = PipeServer::new(state, pipe)
        .spawn()
        .expect("pipe server spawn");
    (pipe_path, handle)
}

async fn paired_live_service(
    endpoint: PathBuf,
    targets: Option<TargetsConfig>,
    surface: Surface,
) -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let daemon = McpDaemonClient::new(endpoint).with_timeout(Duration::from_secs(5));
    let server = match targets {
        Some(t) => TerminalCommanderMcpServer::with_targets(daemon, t).with_surface(surface),
        None => TerminalCommanderMcpServer::new(daemon).with_surface(surface),
    };

    let server_handle =
        tokio::spawn(async move { server.serve(server_transport).await.expect("server serve") });
    let client = TestClient
        .serve_with_lifecycle(
            client_transport,
            rmcp::ClientLifecycleMode::Discover {
                preferred_versions: vec![rmcp::model::ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("client serve");
    // rmcp >= 3.5.1: server answers discover, then awaits the first non-discover request.
    client.list_all_tools().await.expect("first real request");
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

fn echo_argv(text: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            "cmd".to_owned(),
            "/C".to_owned(),
            "echo".to_owned(),
            text.to_owned(),
        ]
    } else {
        vec!["echo".to_owned(), text.to_owned()]
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_surface_lists_three_slim_tools_within_byte_budget_and_leaves_full_untouched() {
    assert_eq!(
        tool_catalogue().len(),
        60,
        "tool_catalogue() must remain exact 60 tools"
    );

    // 1. Code surface: exact 3 tools, <= 4,500 B serialized, flat schemas.
    let (_code_server, code_client) = paired_offline_service(Surface::Code).await;
    let code_tools = code_client.list_all_tools().await.expect("list code tools");
    let mut code_names: Vec<String> = code_tools.iter().map(|t| t.name.to_string()).collect();
    code_names.sort();
    assert_eq!(code_names, vec!["code_execute", "code_search", "status"]);

    let serialized = serde_json::to_string(&json!({ "tools": code_tools })).expect("serialize");
    assert!(
        serialized.len() <= 4_500,
        "code surface tools/list must stay <= 4,500 B, got {} B",
        serialized.len()
    );

    let status_tool = code_tools
        .iter()
        .find(|t| t.name.as_ref() == "status")
        .expect("status tool present");
    assert_eq!(
        status_tool.input_schema.get("type").and_then(Value::as_str),
        Some("object")
    );
    assert_eq!(
        status_tool
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .map(serde_json::Map::len),
        Some(0),
        "code-surface status must be a flat 0-arg schema"
    );
    assert_eq!(
        status_tool
            .annotations
            .as_ref()
            .and_then(|a| a.read_only_hint),
        Some(true),
        "code-surface status must advertise read_only_hint = true"
    );

    let peer_info = code_client.peer_info().expect("peer info").clone();
    let peer_instr = peer_info
        .instructions
        .as_deref()
        .expect("code surface instructions");
    assert_eq!(peer_instr, CODE_SURFACE_INSTRUCTIONS);
    assert!(
        peer_instr.chars().count() <= 850,
        "code surface instructions must stay <= 850 chars, got {}",
        peer_instr.chars().count()
    );
    let _ = code_client.cancel().await;

    // 2. Full surface: exact 60 tools, no code_search / code_execute / facade leakage.
    let (full_server_raw, full_client) = paired_offline_service(Surface::Full).await;
    let full_tools = full_client.list_all_tools().await.expect("list full tools");
    assert_eq!(
        full_tools.len(),
        60,
        "Surface::Full tools/list must stay exact 60 tools"
    );
    for forbidden in ["code_search", "code_execute", "command", "status"] {
        assert!(
            !full_tools.iter().any(|t| t.name.as_ref() == forbidden),
            "Surface::Full must not advertise '{forbidden}'"
        );
    }
    let full_instr = full_server_raw
        .service()
        .get_info()
        .instructions
        .expect("full instructions");
    assert_ne!(full_instr, CODE_SURFACE_INSTRUCTIONS);
    assert!(full_instr.chars().count() <= 850);
    let _ = full_client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_surface_gate_and_offline_honesty() {
    let (_server, client) = paired_offline_service(Surface::Code).await;

    // Unlisted granular and compact tools are rejected with guidance to code_execute.
    for rejected in ["run_and_watch", "command_status", "command", "health"] {
        let err = client
            .call_tool(CallToolRequestParams::new(rejected))
            .await
            .expect_err("unlisted tool must be rejected on Surface::Code");
        let rendered = err.to_string();
        assert!(
            rendered.contains("not on code surface")
                && rendered.contains("code_execute")
                && rendered.contains("TC_SURFACE=full"),
            "rejection for '{rejected}' must guide to code_execute / TC_SURFACE=full; got: {rendered}"
        );
    }

    // Offline `code_search` succeeds without touching the daemon.
    let stubs_all = client
        .call_tool(CallToolRequestParams::new("code_search"))
        .await
        .expect("offline code_search(empty) must succeed");
    let stubs_all_text = first_text(&stubs_all);
    assert!(stubs_all_text.contains("def run_and_watch(*,"));
    assert!(stubs_all_text.contains("def system_discover(*,"));

    let stubs_filtered = client
        .call_tool(CallToolRequestParams::new("code_search").with_arguments(
            serde_json::from_value(json!({ "query": "run_and_watch", "limit": 2 })).expect("args"),
        ))
        .await
        .expect("offline code_search(query) must succeed");
    let stubs_filtered_text = first_text(&stubs_filtered);
    assert!(stubs_filtered_text.contains("from typing import TypedDict"));
    assert!(stubs_filtered_text.contains("def run_and_watch("));

    // Offline `status` surfaces `daemon_unavailable` (does NOT hit status_facade's action validator).
    let status_err = client
        .call_tool(CallToolRequestParams::new("status"))
        .await
        .expect_err("offline status must fail with daemon_unavailable");
    let status_rendered = status_err.to_string();
    assert!(
        status_rendered.contains("daemon_unavailable"),
        "expected daemon_unavailable from 0-arg code status; got: {status_rendered}"
    );
    assert!(
        !status_rendered.contains("action"),
        "code-surface status must not invoke compact facade_strict; got: {status_rendered}"
    );

    // Offline `code_execute` pure Python works; daemon op surfaces daemon_unavailable inside outcome.
    let pure = client
        .call_tool(
            CallToolRequestParams::new("code_execute").with_arguments(
                serde_json::from_value(json!({
                    "code": "xs = [i * i for i in range(5)]\n{'sum': sum(xs), 'count': len(xs)}"
                }))
                .expect("args"),
            ),
        )
        .await
        .expect("pure python code_execute must succeed");
    let pure_json: Value = serde_json::from_str(&first_text(&pure)).expect("json outcome");
    assert_eq!(pure_json["status"], "ok");
    assert_eq!(pure_json["calls"], 0);
    assert_eq!(pure_json["value"]["sum"], 30);

    let down = client
        .call_tool(
            CallToolRequestParams::new("code_execute").with_arguments(
                serde_json::from_value(json!({
                    "code": "print('before call')\nhealth()"
                }))
                .expect("args"),
            ),
        )
        .await
        .expect("code_execute returns structured error outcome on host failure");
    let down_json: Value = serde_json::from_str(&first_text(&down)).expect("json outcome");
    assert_eq!(down_json["status"], "error");
    assert_eq!(down_json["calls"], 1);
    assert_eq!(down_json["stdout"], "before call\n");
    assert!(
        down_json["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("daemon_unavailable"),
        "expected daemon_unavailable in Python RuntimeError message; got: {down_json}"
    );

    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn code_surface_live_daemon_multi_step_execution_and_status() {
    let local_dir = tmp_data_dir("local");
    let remote_dir = tmp_data_dir("remote");
    let (local_ep, local_guard) = start_live_daemon(&local_dir, PolicyProfile::FullAccess);
    let (remote_ep, remote_guard) = start_live_daemon(&remote_dir, PolicyProfile::FullAccess);

    let targets = TargetsConfig {
        targets: vec![RemoteTarget {
            target_id: "sim-remote".to_owned(),
            transport: RemoteTransport::SshForward,
            host: "sim-remote.local".to_owned(),
            identity_file: None,
            remote_socket: None,
            local_forward_socket: remote_ep,
        }],
    };

    let (_server, client) = paired_live_service(local_ep, Some(targets), Surface::Code).await;

    // 1. Live 0-arg `status()` returns compact health + policy + counts.
    let status_res = client
        .call_tool(CallToolRequestParams::new("status"))
        .await
        .expect("live status() must succeed");
    let status_json: Value = serde_json::from_str(&first_text(&status_res)).expect("status json");
    assert_eq!(status_json["ok"], true);
    assert_eq!(status_json["surface"], "code");
    assert_eq!(status_json["adapter_version"], env!("CARGO_PKG_VERSION"));
    assert!(status_json["uptime_secs"].is_number());
    assert_eq!(status_json["caps"]["allow_remote"], true);
    assert!(status_json["counts"]["command_jobs"].is_number());
    assert!(status_json["counts"]["buckets"].is_number());

    // 2. Multi-step Python execution in 1 turn: run_and_watch + bucket_events_since + command_output_tail + target_list/target_probe.
    let argv_py = serde_json::to_string(&echo_argv("CODEMODE_SIGNAL_OK")).expect("argv json");
    let script = format!(
        r#"
tl = target_list()
tp = target_probe(target_id="sim-remote")
run = run_and_watch(
    argv={argv_py},
    rules=[{{"pattern": "CODEMODE_SIGNAL_OK", "severity": "high"}}],
    wait_ms=10000,
)
ev = bucket_events_since(bucket_id=run["bucket_id"], cursor=0, severity_min="high")
tail = command_output_tail(job_id=run["job_id"], max_lines=10, strip_ansi=True)
{{
    "exit_code": run["exit_code"],
    "signal_count": run["signal_count"],
    "bucket_events": len(ev["events"]),
    "tail_joined": "\n".join(tail["lines"]),
    "targets_len": len(tl["targets"]),
    "remote_reachable": tp["reachable"],
}}
"#
    );

    let exec_res = client
        .call_tool(
            CallToolRequestParams::new("code_execute").with_arguments(
                serde_json::from_value(json!({
                    "code": script,
                    "timeout_ms": 30_000,
                    "max_mutations": 4
                }))
                .expect("exec args"),
            ),
        )
        .await
        .expect("live multi-step code_execute must succeed");
    let outcome: Value = serde_json::from_str(&first_text(&exec_res)).expect("outcome json");
    assert_eq!(outcome["status"], "ok", "unexpected error: {outcome}");
    assert_eq!(outcome["calls"], 5);
    assert_eq!(outcome["mutations"], 1);
    assert_eq!(outcome["value"]["exit_code"], 0);
    assert!(
        outcome["value"]["signal_count"].as_u64().unwrap_or(0) >= 1,
        "expected matched signal: {outcome}"
    );
    assert!(
        outcome["value"]["bucket_events"].as_u64().unwrap_or(0) >= 1,
        "expected bucket event: {outcome}"
    );
    assert!(
        outcome["value"]["tail_joined"]
            .as_str()
            .unwrap_or_default()
            .contains("CODEMODE_SIGNAL_OK"),
        "expected tail output to contain signal token: {outcome}"
    );
    assert_eq!(outcome["value"]["targets_len"], 1);
    assert_eq!(outcome["value"]["remote_reachable"], true);

    // 3. Default `target` parameter on `CodeExecuteInput` routes mutating/status calls to the simulated remote daemon.
    let remote_script = format!(
        r#"
r = run_and_watch(argv={argv_py}, wait_ms=10000)
st = command_status(job_id=r["job_id"])
{{"job_id": r["job_id"], "state": st["state"], "exit_code": st["exit_code"]}}
"#
    );
    let remote_res = client
        .call_tool(
            CallToolRequestParams::new("code_execute").with_arguments(
                serde_json::from_value(json!({
                    "code": remote_script,
                    "target": "sim-remote"
                }))
                .expect("remote args"),
            ),
        )
        .await
        .expect("remote-targeted code_execute must succeed");
    let remote_outcome: Value =
        serde_json::from_str(&first_text(&remote_res)).expect("remote outcome json");
    assert_eq!(
        remote_outcome["status"], "ok",
        "unexpected remote error: {remote_outcome}"
    );
    assert_eq!(remote_outcome["value"]["exit_code"], 0);

    let _ = client.cancel().await;
    drop(local_guard);
    drop(remote_guard);
    cleanup(&local_dir);
    cleanup(&remote_dir);
}
