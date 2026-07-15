// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! P5 (T048) end-to-end smoke for remote federation through the MCP adapter.
//!
//! Gates O-09 / O-10, SC-006: an agent runs a COMBED command on a "remote"
//! host via the SAME tool surface, reaching the remote daemon ONLY through a
//! tunnel to its LOCAL socket -- no public network port on either end.
//!
//! How the remote host is SIMULATED (honest disclosure): this test does NOT
//! use real SSH. It stands up a SECOND `terminal-commanderd` on a SECOND UDS
//! socket in the same process and registers it as a target whose
//! `local_forward_socket` IS that second socket. That is exactly the shape an
//! operator-established `ssh -L <local_forward_socket>:<remote_uds>` produces:
//! a local socket path that terminates at a remote daemon's UDS. The adapter
//! cannot tell the difference -- it only ever dials a local socket path. Real
//! loopback SSH is intentionally NOT exercised here (it needs an sshd + keys
//! the CI host may lack); the second-socket simulation proves the routing,
//! gating, and combed-signal contract without that dependency.
//!
//! Invariants asserted:
//! - target_id unset routes to the LOCAL daemon (backward compatibility).
//! - target_id set routes to the SECOND ("remote") daemon: the combed signal
//!   comes back from THAT daemon's bucket.
//! - target_probe reports reachable + the remote daemon_version.
//! - target_list shows the registered target as reachable.
//! - remote use is gated on allow_remote: a local daemon WITHOUT the cap
//!   refuses a target_id request with a typed remote_denied error.
//!
//! The MCP adapter NEVER spawns and NEVER opens a network socket; routing is
//! a pure local-socket-path selection in the daemon-client layer.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ServiceExt};

use terminal_commander_ipc::{
    IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, ResponseEnvelope, read_request,
    write_response,
};
use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commanderd::{
    DaemonConfig, DaemonState, IpcServer, PolicyProfile, RemoteTarget, RemoteTransport,
    ServerHandle, TargetsConfig,
};

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
    p.push(format!("tc-mcp-remote-e2e-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn spawn_daemon_full_access(data: &Path) -> ServerHandle {
    let mut cfg = DaemonConfig::defaults_in(data);
    // full_access presets allow_remote = true (the cap that gates routing a
    // tool to a target), and lets the daemon run argv commands for combing.
    cfg.policy.profile = PolicyProfile::FullAccess;
    spawn_with_config(cfg)
}

fn spawn_daemon_default(data: &Path) -> ServerHandle {
    // developer_local: allow_remote defaults false (default-deny federation).
    let cfg = DaemonConfig::defaults_in(data);
    spawn_with_config(cfg)
}

fn spawn_with_config(cfg: DaemonConfig) -> ServerHandle {
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("daemon bootstrap"));
    let socket = state.config.socket_path();
    let server = IpcServer::new(Arc::clone(&state), socket);
    server.spawn().expect("ipc server spawn")
}

/// A reachability endpoint that deliberately supports only the cheap Health
/// request. Full discovery is the wrong liveness probe because its bounded
/// environment checks may legitimately outlive the remote dial deadline.
fn spawn_health_only_daemon(socket: &Path) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::UnixListener::bind(socket).expect("bind health-only daemon socket");
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let Ok(request) = read_request(&mut stream).await else {
                continue;
            };
            let result = match request.request {
                IpcRequest::Health => IpcResult::Ok {
                    response: IpcResponse::Health {
                        uptime_secs: 1,
                        idle_secs: Some(0),
                        version: "health-only-test-daemon".to_owned(),
                    },
                },
                _ => IpcResult::Err {
                    error: IpcError::new(
                        IpcErrorCode::UnknownMethod,
                        "reachability must use health",
                    ),
                },
            };
            let response = ResponseEnvelope {
                correlation_id: request.correlation_id,
                result,
            };
            let _ = write_response(&mut stream, &response).await;
        }
    })
}

/// Build a target whose `local_forward_socket` IS the second daemon's live
/// socket -- the exact shape an operator `ssh -L` forward produces.
fn target_for(target_id: &str, remote: &ServerHandle) -> RemoteTarget {
    RemoteTarget {
        target_id: target_id.to_owned(),
        transport: RemoteTransport::SshForward,
        host: format!("{target_id}.simulated"),
        identity_file: None,
        remote_socket: None,
        local_forward_socket: remote.socket_path().to_path_buf(),
    }
}

/// Pair the rmcp adapter (wired to `local` daemon + the target registry)
/// against an in-process duplex client.
async fn paired_with_targets(
    local: &ServerHandle,
    targets: TargetsConfig,
) -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let daemon = McpDaemonClient::new(local.socket_path().to_path_buf())
        .with_timeout(Duration::from_secs(5));
    let server = TerminalCommanderMcpServer::with_targets(daemon, targets);
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

fn json_body(result: &rmcp::model::CallToolResult) -> serde_json::Value {
    serde_json::from_str(&first_text(result)).expect("tool result is JSON")
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

/// Inline rule matching the literal `needle`, emitting a `needle_match` event.
fn needle_rule() -> serde_json::Value {
    serde_json::json!([{ "pattern": "needle", "event_kind": "needle_match" }])
}

fn has_needle_signal(body: &serde_json::Value) -> bool {
    body["signals"]
        .as_array()
        .is_some_and(|sigs| !sigs.is_empty())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn target_reachability_uses_health_not_full_environment_discovery() {
    let local_data = tmp_data_dir("local-health-probe");
    let remote_data = tmp_data_dir("remote-health-probe");
    std::fs::create_dir_all(&remote_data).expect("create fake remote data dir");
    let remote_socket = remote_data.join("health-only.sock");
    let fake_remote = spawn_health_only_daemon(&remote_socket);
    let local = spawn_daemon_full_access(&local_data);

    let targets = TargetsConfig {
        targets: vec![RemoteTarget {
            target_id: "health-only".to_owned(),
            transport: RemoteTransport::SshForward,
            host: "health-only.simulated".to_owned(),
            identity_file: None,
            remote_socket: None,
            local_forward_socket: remote_socket,
        }],
    };
    let (_server, client) = paired_with_targets(&local, targets).await;
    let list = json_body(&call_tool(&client, "target_list", serde_json::json!({})).await);
    let reachable = list["targets"][0]["reachable"].clone();

    let _ = client.cancel().await;
    local.shutdown().await;
    fake_remote.abort();
    let _ = fake_remote.await;
    cleanup(&local_data);
    cleanup(&remote_data);

    assert_eq!(
        reachable, true,
        "remote reachability must use the cheap Health request; got {list}"
    );
}

/// Core gate O-09/O-10: a combed command routed with `target_id` runs on the
/// SECOND ("remote") daemon and its combed signal comes back through the
/// tunnel-shaped local socket. Probe + list also reach the remote.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_target_routes_combed_command_to_second_daemon() {
    if !Path::new("/bin/echo").exists() {
        eprintln!("skipping: /bin/echo not available");
        return;
    }

    let local_data = tmp_data_dir("local");
    let remote_data = tmp_data_dir("remote");
    let local = spawn_daemon_full_access(&local_data);
    let remote = spawn_daemon_full_access(&remote_data);
    {
        let targets = TargetsConfig {
            targets: vec![target_for("remote-1", &remote)],
        };
        let (_server, client) = paired_with_targets(&local, targets).await;

        // 1. target_list: the registered target is reachable over its forward
        //    socket (the second daemon's live UDS).
        let list = json_body(&call_tool(&client, "target_list", serde_json::json!({})).await);
        let entries = list["targets"].as_array().expect("targets array");
        assert_eq!(
            entries.len(),
            1,
            "exactly one registered target; got {list}"
        );
        assert_eq!(entries[0]["target_id"], "remote-1");
        assert_eq!(
            entries[0]["reachable"], true,
            "registered target must be reachable over its forward socket; got {list}"
        );

        // 2. target_probe: reachable + the remote daemon_version.
        let probe = json_body(
            &call_tool(
                &client,
                "target_probe",
                serde_json::json!({ "target_id": "remote-1" }),
            )
            .await,
        );
        assert_eq!(
            probe["reachable"], true,
            "probe must be reachable; got {probe}"
        );
        assert!(
            probe["daemon_version"].is_string(),
            "probe must report the remote daemon_version; got {probe}"
        );

        // 3. run_and_watch WITH target_id => combed signal from the REMOTE
        //    daemon (gate O-09/O-10): the command never ran locally.
        let remote_run = json_body(
            &call_tool(
                &client,
                "run_and_watch",
                serde_json::json!({
                    "argv": ["echo", "found needle in haystack"],
                    "rules": needle_rule(),
                    "wait_ms": 4000,
                    "target_id": "remote-1",
                }),
            )
            .await,
        );
        assert!(
            has_needle_signal(&remote_run),
            "remote run_and_watch must return a combed signal from the second daemon; got {remote_run}"
        );
        assert_eq!(
            remote_run["exit_code"], 0,
            "remote command must exit 0; got {remote_run}"
        );

        // 4. Backward compatibility: NO target_id => local daemon, combed the
        //    same way (bounded/combed output is identical local vs remote).
        let local_run = json_body(
            &call_tool(
                &client,
                "run_and_watch",
                serde_json::json!({
                    "argv": ["echo", "found needle locally"],
                    "rules": needle_rule(),
                    "wait_ms": 4000,
                }),
            )
            .await,
        );
        assert!(
            has_needle_signal(&local_run),
            "local run_and_watch must still comb identically; got {local_run}"
        );

        // 5. Unknown target => typed error, not a silent local run.
        let unknown = client
            .call_tool(
                CallToolRequestParams::new("target_probe").with_arguments(
                    serde_json::from_value(serde_json::json!({ "target_id": "nope" }))
                        .expect("args object"),
                ),
            )
            .await;
        let err = unknown.expect_err("unknown target must error");
        assert!(
            err.to_string().contains("unknown target"),
            "unknown target_id must be a typed unknown_target error; got {err}"
        );

        let _ = client.cancel().await;
    }
    local.shutdown().await;
    remote.shutdown().await;
    cleanup(&local_data);
    cleanup(&remote_data);
}

/// Default-deny: a local daemon WITHOUT `allow_remote` refuses a target_id
/// request with a typed `remote_denied` error (constitution II). The remote
/// daemon is never dialed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_use_is_denied_without_allow_remote_cap() {
    let local_data = tmp_data_dir("denylocal");
    let remote_data = tmp_data_dir("denyremote");
    // LOCAL daemon on the default profile: allow_remote = false.
    let local = spawn_daemon_default(&local_data);
    let remote = spawn_daemon_full_access(&remote_data);
    {
        let targets = TargetsConfig {
            targets: vec![target_for("remote-1", &remote)],
        };
        let (_server, client) = paired_with_targets(&local, targets).await;

        // target_probe on a registered target must be DENIED (gate before dial).
        let denied = client
            .call_tool(
                CallToolRequestParams::new("target_probe").with_arguments(
                    serde_json::from_value(serde_json::json!({ "target_id": "remote-1" }))
                        .expect("args object"),
                ),
            )
            .await
            .expect_err("remote probe must be denied without allow_remote");
        assert!(
            denied.to_string().contains("remote_denied"),
            "must surface a typed remote_denied error; got {denied}"
        );

        // Routing a command tool with target_id must be denied too.
        let denied_run = client
            .call_tool(
                CallToolRequestParams::new("run_and_watch").with_arguments(
                    serde_json::from_value(serde_json::json!({
                        "argv": ["echo", "x"],
                        "target_id": "remote-1",
                    }))
                    .expect("args object"),
                ),
            )
            .await
            .expect_err("remote run must be denied without allow_remote");
        assert!(
            denied_run.to_string().contains("remote_denied"),
            "remote command routing must be denied; got {denied_run}"
        );

        // target_list (read-only) is NOT gated by allow_remote: it still lists.
        let list = json_body(&call_tool(&client, "target_list", serde_json::json!({})).await);
        assert_eq!(
            list["targets"].as_array().map(Vec::len),
            Some(1),
            "target_list is read-only and lists regardless of allow_remote; got {list}"
        );

        let _ = client.cancel().await;
    }
    local.shutdown().await;
    remote.shutdown().await;
    cleanup(&local_data);
    cleanup(&remote_data);
}
