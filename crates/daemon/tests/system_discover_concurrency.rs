// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! `system_discover` runs host probe processes and waits on them, which takes
//! seconds on Windows and longer under load. It ran on an async worker and the
//! daemon answered nothing else until it finished. Every MCP adapter sends one
//! at startup to check version skew and gives up after 750 ms, so the adapter's
//! first tool call (a 5 s `health`) timed out whenever discovery outlasted it.

#![cfg(any(unix, windows))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use terminal_commanderd::{DaemonClient, DaemonConfig, DaemonState, IpcRequest, IpcResponse};

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
            r"\\.\pipe\tc-test-discover-{}-{}",
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

// The server gets its own otherwise idle runtime, as in the daemon process.
// With the client on the same runtime, the client's own activity keeps the
// I/O driver polled and the stall does not show.
#[test]
fn health_is_answered_while_system_discover_runs() {
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

    let (health, health_took, discovered, discover_took) = client_rt.block_on(async move {
        let started = Instant::now();
        let discover = {
            let client = client.clone();
            tokio::spawn(async move { client.call(1, IpcRequest::SystemDiscover).await })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        let health_sent = Instant::now();
        let health = client.call(2, IpcRequest::Health).await;
        let health_took = health_sent.elapsed();
        let discovered = discover.await.unwrap();
        (health, health_took, discovered, started.elapsed())
    });
    assert!(
        matches!(health, Ok(IpcResponse::Health { .. })),
        "{health:?}"
    );
    assert!(
        matches!(discovered, Ok(IpcResponse::SystemDiscover(_))),
        "{discovered:?}"
    );

    // Where the probes finish in well under a second there is no window to
    // observe; Windows hosts take seconds.
    if discover_took < Duration::from_secs(1) {
        eprintln!("discovery took {discover_took:?}; too fast to observe a stall");
        return;
    }
    assert!(
        health_took < discover_took / 2,
        "health waited {health_took:?} behind a {discover_took:?} system_discover"
    );
}
