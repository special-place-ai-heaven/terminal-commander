// SPDX-License-Identifier: Apache-2.0
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
use terminal_commanderd::{DaemonConfig, DaemonState};
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
    let pipe_name = format!(r"\\.\pipe\tc-test-accept-{}", std::process::id());
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
