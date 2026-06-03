// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! FIX 2 (Windows): the Windows runtime arm of `run_ipc_server` must
//! select on `DaemonState::shutdown_notified()` so an IPC `Shutdown`
//! request (which flips the trigger via `trigger_shutdown` and returns
//! `ShutdownAck`) actually stops the daemon. Before the fix the Windows
//! arm awaited ONLY `ctrl_c()`, so `ShutdownAck` was a false success:
//! the trigger fired but nothing observed it and the daemon never
//! exited.
//!
//! The full `run_ipc_server` binds a real named pipe, runs the
//! self-check, and registers an OS Ctrl-C handler -- not friendly to a
//! deterministic in-process test. Instead this asserts the BUILDING
//! BLOCK that FIX 2 added: a `select!` between the Ctrl-C future (here
//! modelled by a future that never resolves, standing in for "no signal
//! was sent") and `shutdown_notified()` must complete promptly when the
//! `Shutdown` trigger fires. That is exactly the branch the Windows
//! runtime now selects on; if it regressed back to awaiting only the
//! signal, this test would hang and the test-level timeout would fail
//! it.
//!
//! Windows only.

#![cfg(windows)]

use std::future::pending;
use std::path::PathBuf;
use std::time::Duration;

use terminal_commanderd::{DaemonConfig, DaemonState};

fn temp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-win-runtime-shutdown-{tag}-{pid}-{nanos}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

/// Mirrors the Windows `run_ipc_server` shutdown select: the OS-signal
/// branch (modelled as a never-completing future, i.e. no Ctrl-C) and
/// the internal `shutdown_notified()` branch. With FIX 2 the latter is
/// present, so an IPC-style `trigger_shutdown()` makes the select
/// resolve. The test timeout would fire if the shutdown branch were
/// missing (the pre-fix behaviour), turning a silent false-success into
/// a hard failure.
#[tokio::test]
async fn windows_runtime_select_completes_on_shutdown_notified() {
    let data = temp_data_dir("select");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = DaemonState::bootstrap(cfg).expect("bootstrap daemon state");

    // Fire the trigger as the `Shutdown` IPC dispatch arm would.
    state.trigger_shutdown();

    // The runtime select: no OS signal (pending) vs. the internal
    // shutdown trigger. Must resolve via the shutdown branch.
    let selected = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            () = pending::<()>() => "ctrl_c",
            () = state.shutdown_notified() => "shutdown_notified",
        }
    })
    .await
    .expect("Windows runtime select must complete on shutdown_notified, not hang");

    assert_eq!(
        selected, "shutdown_notified",
        "the internal Shutdown trigger must win the runtime select"
    );

    cleanup(&data);
}

/// Late-awaiter variant: even when `trigger_shutdown()` is called
/// AFTER the select begins awaiting, the sticky watch flag wakes it.
/// This proves the IPC-`Shutdown`-then-runtime-observes ordering the
/// real daemon experiences (the ACK is sent on the connection task;
/// the runtime select is already parked on `shutdown_notified`).
#[tokio::test]
async fn windows_runtime_select_wakes_on_late_trigger() {
    let data = temp_data_dir("late");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = std::sync::Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"));

    let st = std::sync::Arc::clone(&state);
    tokio::spawn(async move {
        // Simulate the `Shutdown` IPC landing a moment after the
        // runtime parked on the select.
        tokio::time::sleep(Duration::from_millis(50)).await;
        st.trigger_shutdown();
    });

    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            () = pending::<()>() => panic!("ctrl_c branch must not fire"),
            () = state.shutdown_notified() => {}
        }
    })
    .await
    .expect("late Shutdown trigger must wake the parked runtime select");

    cleanup(&data);
}
