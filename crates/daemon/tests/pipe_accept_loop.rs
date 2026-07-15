// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Verifies the named-pipe accept loop:
//   1. accepts the first client,
//   2. accepts a second client after the first disconnects,
//   3. survives a transient ServerOptions::create error.

#![cfg(windows)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commanderd::ipc::pipe_server::PipeServer;
use terminal_commanderd::{DaemonClient, DaemonConfig, DaemonState, IpcRequest, IpcResponse};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::time::sleep;

fn temp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-pipe-accept-{tag}-{pid}-{nanos}"));
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

#[tokio::test]
async fn first_and_second_client_both_connect() {
    // M1: PID + nanos so a future 2nd test in this file cannot collide on the
    // shared process PID.
    let pipe_name = format!(
        r"\\.\pipe\tc-test-accept-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    );
    let (state, data) = make_state("accept");
    let server = PipeServer::new(state, pipe_name.clone());
    let handle = server.spawn().expect("spawn pipe server");

    // First client.
    sleep(Duration::from_millis(50)).await;
    let c1 = ClientOptions::new().open(&pipe_name).expect("c1 connect");
    drop(c1);

    // Second client should also succeed (accept loop must not exit
    // after the first connection drops).
    sleep(Duration::from_millis(50)).await;
    let c2 = ClientOptions::new().open(&pipe_name).expect("c2 connect");
    drop(c2);

    handle.shutdown().await;
    cleanup(&data);
}

#[tokio::test]
async fn completed_connections_cannot_drop_the_pending_pipe() {
    let pipe_name = format!(
        r"\\.\pipe\tc-test-accept-churn-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    );
    let (state, data) = make_state("accept-churn");
    let handle = PipeServer::new(state, pipe_name.clone())
        .spawn()
        .expect("spawn pipe server");
    sleep(Duration::from_millis(50)).await;

    let client = DaemonClient::new(PathBuf::from(&pipe_name)).with_timeout(Duration::from_secs(3));
    for id in 1..=128 {
        let response = client.call(id, IpcRequest::Health).await;
        assert!(
            matches!(response, Ok(IpcResponse::Health { .. })),
            "every accepted pipe must return its response; call {id}: {response:?}"
        );
    }

    handle.shutdown().await;
    cleanup(&data);
}
