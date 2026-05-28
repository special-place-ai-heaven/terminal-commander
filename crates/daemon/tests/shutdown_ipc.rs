// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! E2 graceful-shutdown IPC tests.
//!
//! Exercises the `Shutdown` request path on the real UDS IPC server:
//!
//! - `Shutdown` returns a real `ShutdownAck { draining: true }` (proves the
//!   dispatch arm fires `trigger_shutdown` and ACKs, not the E1 placeholder
//!   that returned a typed `Internal` error).
//! - After the ACK, `ServerHandle::shutdown()` completes promptly: the
//!   in-flight connection that served the `Shutdown` request has flushed its
//!   ACK and the drain joins all connection tasks without hanging.
//!
//! The runtime's OS-signal-vs-internal `select!` lives in the daemon BINARY
//! (`run_ipc_server`), not in this in-process server, so this file asserts the
//! SERVER-side guarantees: the ACK and a clean, prompt drain.
//!
//! Linux/WSL only (UDS).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcRequest, IpcResponse, IpcServer,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-ipc-shutdown-{tag}-{pid}-{nanos}-{n}"));
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
fn shutdown_request_acks_then_daemon_exits_and_endpoint_unreachable() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("shutdown");
        let (_state, handle) = build_server(&data);
        let sock = handle.socket_path().to_path_buf();
        let client = DaemonClient::new(sock.clone()).with_timeout(Duration::from_secs(3));

        let resp = client
            .call(1, IpcRequest::Shutdown)
            .await
            .expect("shutdown call");
        match resp {
            IpcResponse::ShutdownAck { draining } => {
                assert!(draining, "ack must report draining");
            }
            other => panic!("unexpected: {other:?}"),
        }

        // After the ACK, drive the drain explicitly and assert it returns
        // promptly (in-flight connections drained, not hung). A 10s ceiling
        // lives in the drain itself; wrap the call in a tighter test timeout
        // so a deadlock fails the test instead of hanging the suite.
        tokio::time::timeout(Duration::from_secs(8), handle.shutdown())
            .await
            .expect("handle.shutdown() must drain and return promptly, not hang");

        // Socket file is removed by shutdown; the endpoint is unreachable.
        assert!(
            !sock.exists(),
            "socket file must be removed after shutdown drain"
        );
        cleanup(&data);
    });
}

/// After IPC drain, [`StoreClient::shutdown`] (mirroring
/// `run_ipc_server`'s `shutdown_store`) must join the writer thread
/// promptly — no hang waiting on WAL checkpoint.
#[test]
fn shutdown_drain_then_store_shutdown_completes_promptly() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("store-shutdown");
        let (state, handle) = build_server(&data);
        let sock = handle.socket_path().to_path_buf();
        let client = DaemonClient::new(sock).with_timeout(Duration::from_secs(3));

        let resp = client
            .call(1, IpcRequest::Shutdown)
            .await
            .expect("shutdown call");
        match resp {
            IpcResponse::ShutdownAck { draining } => {
                assert!(draining, "ack must report draining");
            }
            other => panic!("unexpected: {other:?}"),
        }

        tokio::time::timeout(Duration::from_secs(8), handle.shutdown())
            .await
            .expect("IPC drain must complete promptly");

        let store = state.store.clone();
        tokio::time::timeout(Duration::from_secs(8), async move {
            tokio::task::spawn_blocking(move || store.shutdown())
                .await
                .expect("join spawn_blocking")
                .expect("store shutdown ok");
        })
        .await
        .expect("store.shutdown() must join writer thread promptly");

        cleanup(&data);
    });
}

/// `trigger_shutdown` must satisfy a subsequent `shutdown_notified()` even
/// when triggered BEFORE anyone awaits (the late-awaiter race). The sticky
/// watch flag guarantees the late awaiter observes the request.
#[test]
fn trigger_shutdown_is_observed_by_a_late_awaiter() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("late-await");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // Trigger first, then await: the late awaiter must still wake.
        state.trigger_shutdown();
        tokio::time::timeout(Duration::from_secs(2), state.shutdown_notified())
            .await
            .expect("late awaiter must observe an already-fired shutdown trigger");

        cleanup(&data);
    });
}

#[test]
fn daemon_self_reaps_after_idle_ttl_via_trigger_shutdown() {
    // We do NOT run the full `run_ipc_server` here; in-process tests don't
    // exercise the runtime's select! (that's the binary). Instead we assert
    // the building block: an idle-timer task observes idle_secs >= ttl and
    // calls trigger_shutdown, which the late-awaiter-safe shutdown_notified
    // resolves on.
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("self-reap");
        let (state, _handle) = build_server(&data);

        // Spawn the idle-timer task exactly as run_ipc_server will, with a
        // 1-second ttl. The task ticks fast enough that this completes within
        // a few seconds even on a loaded WSL host.
        let ttl: u64 = 1;
        // Equivalent to `max(1, min(60, ttl/2)).max(1)`; clippy prefers
        // clamp. Production wiring uses the same formula in
        // `runtime::spawn_idle_reaper`.
        let tick = (ttl / 2).clamp(1, 60);
        let st = Arc::clone(&state);
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(std::time::Duration::from_secs(tick));
            iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                iv.tick().await;
                if st.idle_secs() >= ttl {
                    st.trigger_shutdown();
                    break;
                }
            }
        });

        // No IPC at all -> idle grows -> within a few seconds trigger fires ->
        // shutdown_notified resolves.
        tokio::time::timeout(std::time::Duration::from_secs(6), state.shutdown_notified())
            .await
            .expect("idle timer did not trigger shutdown within 6s (ttl=1s)");
        cleanup(&data);
    });
}
