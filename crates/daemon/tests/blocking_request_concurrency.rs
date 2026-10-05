// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! A request whose handler blocks (child processes, directory walks, large
//! reads) must not hold up other requests. These handlers ran inline on an
//! async worker, and the daemon answered nothing else, Health included, until
//! they returned. `system_discover` was the visible case: every MCP adapter
//! sends one at startup to check version skew and gives up after 750 ms, so
//! the adapter's first tool call (a 5 s `health`) timed out whenever discovery
//! outlasted it.

#![cfg(any(unix, windows))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcError, IpcRequest, IpcResponse,
};

#[cfg(unix)]
type ServerHandle = terminal_commanderd::ServerHandle;
#[cfg(windows)]
type ServerHandle = terminal_commanderd::PipeServerHandle;

fn serve(state: &Arc<DaemonState>) -> (PathBuf, ServerHandle) {
    #[cfg(unix)]
    {
        let handle =
            terminal_commanderd::IpcServer::new(Arc::clone(state), state.config.socket_path())
                .spawn()
                .unwrap();
        (handle.socket_path().to_path_buf(), handle)
    }
    #[cfg(windows)]
    {
        let name = format!(
            r"\\.\pipe\tc-test-blocking-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        let handle = terminal_commanderd::PipeServer::new(Arc::clone(state), name.clone())
            .spawn()
            .unwrap();
        (PathBuf::from(name), handle)
    }
}

fn request(method: &str, params: &Value) -> IpcRequest {
    serde_json::from_value(json!({"method": method, "params": params})).unwrap()
}

/// Send `request`, then Health 50 ms later, and assert Health did not wait
/// for the request. Returns the request's result.
///
/// The server gets its own otherwise idle runtime, as in the daemon process.
/// With the client on the same runtime, the client's own activity keeps the
/// I/O driver polled and the stall does not show.
fn assert_health_not_blocked_by(request: IpcRequest) -> Result<IpcResponse, IpcError> {
    let data = tempfile::tempdir().unwrap();
    let state = Arc::new(DaemonState::bootstrap(DaemonConfig::defaults_in(data.path())).unwrap());
    let server_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (endpoint, _handle) = server_rt.block_on(async { serve(&state) });
    let client_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let client = DaemonClient::new(endpoint).with_timeout(Duration::from_mins(1));

    let label = format!("{request:?}").chars().take(80).collect::<String>();
    let (result, health, health_took, request_took) = client_rt.block_on(async move {
        let started = Instant::now();
        let pending = {
            let client = client.clone();
            tokio::spawn(async move { client.call(1, request).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let health_sent = Instant::now();
        let health = client.call(2, IpcRequest::Health).await;
        let health_took = health_sent.elapsed();
        let result = pending.await.unwrap();
        (result, health, health_took, started.elapsed())
    });
    assert!(
        matches!(health, Ok(IpcResponse::Health { .. })),
        "{health:?}"
    );

    // A request that finishes this fast leaves no window to observe a stall.
    if request_took < Duration::from_millis(250) {
        eprintln!("{label} took {request_took:?}; too fast to observe a stall");
        return result;
    }
    assert!(
        health_took < request_took / 2,
        "health waited {health_took:?} behind a {request_took:?} {label}"
    );
    result
}

fn many_files(dir: &Path, n: usize) {
    for i in 0..n {
        std::fs::write(dir.join(format!("f{i:05}.txt")), b"hello\n").unwrap();
    }
}

#[test]
fn health_is_answered_while_system_discover_runs() {
    let r = assert_health_not_blocked_by(IpcRequest::SystemDiscover);
    assert!(matches!(r, Ok(IpcResponse::SystemDiscover(_))), "{r:?}");
}

/// Without `shell`, the default shell is resolved by running probe processes.
#[test]
fn health_is_answered_while_shell_exec_resolves_the_default_shell() {
    let r = assert_health_not_blocked_by(request("shell_exec", &json!({"shell_line": "echo hi"})));
    assert!(matches!(r, Ok(IpcResponse::CommandStartCombed(_))), "{r:?}");
}

/// `start_line` is not capped, so every earlier line is read and skipped.
#[test]
fn health_is_answered_while_file_read_window_skips_to_a_far_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lines.txt");
    std::fs::write(&path, "x\n".repeat(10_000_000)).unwrap();
    let r = assert_health_not_blocked_by(request(
        "file_read_window",
        &json!({"path": path, "start_line": 10_000_000u64}),
    ));
    assert!(matches!(r, Ok(IpcResponse::FileReadWindow(_))), "{r:?}");
}

#[test]
fn health_is_answered_while_file_search_walks_a_tree() {
    let dir = tempfile::tempdir().unwrap();
    many_files(dir.path(), 3_000);
    let r = assert_health_not_blocked_by(request(
        "file_search",
        &json!({"path": dir.path(), "query": "no-such-text"}),
    ));
    assert!(matches!(r, Ok(IpcResponse::FileSearch(_))), "{r:?}");
}

/// Every entry is read and stat'ed before the listing is capped.
#[test]
fn health_is_answered_while_file_list_dir_reads_a_large_directory() {
    let dir = tempfile::tempdir().unwrap();
    many_files(dir.path(), 20_000);
    let r = assert_health_not_blocked_by(request(
        "file_list_dir",
        &json!({"path": dir.path().display().to_string()}),
    ));
    assert!(matches!(r, Ok(IpcResponse::FileListDir(_))), "{r:?}");
}
