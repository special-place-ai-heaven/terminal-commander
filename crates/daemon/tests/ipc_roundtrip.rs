// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! UDS IPC (TC37) integration tests.
//!
//! Unix-only. On Windows the file compiles to an empty module so
//! the workspace still builds.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcRequest, IpcResponse, IpcServer, MAX_FRAME_BYTES,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-ipc-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn build_server(data: &std::path::Path) -> (Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let cfg = DaemonConfig::defaults_in(data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let server = IpcServer::new(Arc::clone(&state), socket);
    let handle = server.spawn().unwrap();
    (state, handle)
}

#[test]
fn system_discover_round_trip() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("discover");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));
        let resp = client.call(1, IpcRequest::SystemDiscover).await.unwrap();
        match resp {
            IpcResponse::SystemDiscover(d) => {
                assert_eq!(d.mcp_spec, "2025-11-25");
                assert!(d.methods.iter().any(|m| m == "system_discover"));
                assert!(d.methods.iter().any(|m| m == "health"));
                assert!(d.methods.iter().any(|m| m == "policy_status"));
                assert!(d.methods.iter().any(|m| m == "self_check"));
            }
            other => panic!("unexpected response: {other:?}"),
        }
        // Audit row should have landed.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(rows.iter().any(|r| r.action == "ipc_system_discover"));
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn health_round_trip() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("health");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let resp = client.call(2, IpcRequest::Health).await.unwrap();
        match resp {
            IpcResponse::Health { uptime_secs: _, .. } => {}
            other => panic!("unexpected response: {other:?}"),
        }
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn health_returns_idle_secs_and_does_not_bump_or_audit() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("health-idle");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());

        let _ = client.call(1, IpcRequest::Health).await.expect("h1");
        let h2 = client.call(2, IpcRequest::Health).await.expect("h2");
        match h2 {
            IpcResponse::Health { idle_secs, .. } => {
                assert!(idle_secs.is_some(), "Health must report idle_secs");
            }
            other => panic!("unexpected: {other:?}"),
        }
        // Health is a peek: it must NOT write a persistent audit row.
        // emit_audit prefixes the method, so a health row would be
        // recorded as "ipc_health".
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            !rows.iter().any(|r| r.action == "ipc_health"),
            "Health is a peek: it must NOT write an audit row; got {rows:?}"
        );
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn policy_status_reports_active_caps() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("policy");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let resp = client.call(3, IpcRequest::PolicyStatus).await.unwrap();
        match resp {
            IpcResponse::PolicyStatus(p) => {
                assert_eq!(p.file_window_bytes, state.config.limits.file_window_bytes);
                assert_eq!(p.bucket_read_limit, state.config.limits.bucket_read_limit);
                assert!(p.commands_deny_count > 0);
                assert!(p.default_deny_path_suffix_count > 0);
            }
            other => panic!("unexpected response: {other:?}"),
        }
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn self_check_method_returns_report() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("sc");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let resp = client.call(4, IpcRequest::SelfCheck).await.unwrap();
        match resp {
            IpcResponse::SelfCheck(sc) => {
                assert_eq!(sc.failures, 0);
                assert!(sc.report.contains("policy_profile"));
                assert!(sc.report.contains("audit"));
            }
            other => panic!("unexpected response: {other:?}"),
        }
        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Malformed JSON in the payload must yield a typed error and close
/// the connection without panic.
#[test]
fn malformed_json_returns_typed_error_and_closes() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("badjson");
        let (_state, handle) = build_server(&data);
        let socket = handle.socket_path().to_path_buf();
        let mut stream = UnixStream::connect(&socket).await.unwrap();
        let payload = b"{not valid json at all";
        let len = u32::try_from(payload.len()).unwrap().to_be_bytes();
        stream.write_all(&len).await.unwrap();
        stream.write_all(payload).await.unwrap();
        // Read response.
        let mut len_buf = [0_u8; 4];
        stream.read_exact(&mut len_buf).await.unwrap();
        let resp_len = u32::from_be_bytes(len_buf) as usize;
        let mut resp = vec![0_u8; resp_len];
        stream.read_exact(&mut resp).await.unwrap();
        let env: serde_json::Value = serde_json::from_slice(&resp).unwrap();
        let code = env["result"]["error"]["code"].as_str().unwrap();
        assert!(
            code == "malformed_json" || code == "schema_mismatch",
            "expected malformed_json/schema_mismatch, got {code}"
        );
        handle.shutdown().await;
        cleanup(&data);
    });
}

/// A length prefix above MAX_FRAME_BYTES must be rejected.
#[test]
fn oversized_frame_rejected() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("toobig");
        let (_state, handle) = build_server(&data);
        let socket = handle.socket_path().to_path_buf();
        let mut stream = UnixStream::connect(&socket).await.unwrap();
        let bogus_len = u32::try_from(MAX_FRAME_BYTES + 1024).unwrap().to_be_bytes();
        stream.write_all(&bogus_len).await.unwrap();
        // Read response (we don't have to actually send the payload).
        let mut resp_len_buf = [0_u8; 4];
        stream.read_exact(&mut resp_len_buf).await.unwrap();
        let resp_len = u32::from_be_bytes(resp_len_buf) as usize;
        let mut resp = vec![0_u8; resp_len];
        stream.read_exact(&mut resp).await.unwrap();
        let env: serde_json::Value = serde_json::from_slice(&resp).unwrap();
        assert_eq!(
            env["result"]["error"]["code"].as_str().unwrap(),
            "frame_too_large"
        );
        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Peer credentials must be captured and audit-visible on Linux/WSL.
/// On macOS/BSD `pid` may be None but uid/gid are still captured.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn peer_credentials_recorded_in_audit_metadata() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("peer");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        // SystemDiscover (not Health) is used here: Health is now an
        // audit-free peek, so it writes no audit row. Any audited
        // method carries the same peer metadata we assert on.
        let _ = client.call(5, IpcRequest::SystemDiscover).await.unwrap();
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let row = rows
            .iter()
            .find(|r| r.action == "ipc_system_discover")
            .expect("system_discover audit row must exist");
        // metadata_json should look like {"uid":N,"gid":N,"pid":N}
        let meta = row
            .metadata_json
            .as_deref()
            .expect("peer credentials should be recorded in metadata_json");
        assert!(meta.contains("\"uid\""));
        assert!(meta.contains("\"gid\""));
        assert!(meta.contains("\"pid\""));
        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Socket file is removed when the server shuts down.
#[test]
fn shutdown_removes_socket_file() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("cleanup");
        let (_state, handle) = build_server(&data);
        let socket = handle.socket_path().to_path_buf();
        assert!(socket.exists(), "socket should exist while server runs");
        handle.shutdown().await;
        assert!(!socket.exists(), "socket should be removed after shutdown");
        cleanup(&data);
    });
}
