// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Verifies that the named-pipe server resolves the real Windows peer
// identity (SID + PID) via Win32 APIs when a client connects
// (Task 8: TC37-Win32-PeerIdentity).

#![cfg(windows)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_supervisor::identity::PeerIdentity;
use terminal_commanderd::ipc::pipe_server::PipeServer;
use terminal_commanderd::{DaemonConfig, DaemonState};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::time::sleep;

fn temp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-pipe-peer-id-{tag}-{pid}-{nanos}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn make_state(tag: &str) -> (Arc<DaemonState>, PathBuf) {
    let data = temp_data_dir(tag);
    let cfg = DaemonConfig::defaults_in(&data);
    let state = DaemonState::bootstrap(cfg).expect("bootstrap daemon state");
    (Arc::new(state), data)
}

/// The server must record a `PeerIdentity::Windows` variant — not
/// `Unknown` — when a local client connects over the named pipe.
/// The SID must be non-empty and the PID must match this process.
#[tokio::test]
async fn pipe_server_records_windows_peer_identity() {
    // M1: PID + nanos so a future 2nd test in this file cannot collide on the
    // shared process PID.
    let pipe_name = format!(
        r"\\.\pipe\tc-test-peer-id-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    );
    let (state, data) = make_state("peer_id");
    let server = PipeServer::new(Arc::clone(&state), pipe_name.clone());
    let handle = server.spawn().expect("spawn pipe server");

    // Give the accept loop time to start and enter connect().
    sleep(Duration::from_millis(50)).await;

    // Connect a client; the server resolves the peer on connect.
    let _client = ClientOptions::new()
        .open(&pipe_name)
        .expect("client open named pipe");

    // Allow the connection handler task to run and record the identity.
    sleep(Duration::from_millis(100)).await;

    let observed = state.test_last_observed_peer_identity();

    handle.shutdown().await;
    cleanup(&data);

    let identity = observed.expect("peer identity must have been recorded");

    match identity {
        PeerIdentity::Windows { sid, pid, .. } => {
            assert!(!sid.is_empty(), "SID must be non-empty");
            // SIDs on Windows start with "S-1-"
            assert!(
                sid.starts_with("S-1-"),
                "SID should start with 'S-1-', got: {sid}"
            );
            let self_pid = std::process::id();
            assert_eq!(
                pid,
                Some(self_pid),
                "PID must match this test process ({self_pid})"
            );
        }
        other => {
            panic!("expected PeerIdentity::Windows, got: {other:?}");
        }
    }
}
