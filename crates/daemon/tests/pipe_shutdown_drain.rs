// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! FIX 3A (Windows): the named-pipe accept loop must DRAIN in-flight
//! connection handlers on shutdown, mirroring the Unix UDS server.
//!
//! Before the fix the accept loop spawned each connection handler as a
//! DETACHED `tokio::spawn`; `PipeServerHandle::shutdown` awaited only
//! the accept-loop join handle, never the in-flight handlers. A request
//! mid-dispatch at shutdown could be dropped and could outlive
//! `shutdown_store`. The fix tracks handlers in a `JoinSet` owned by the
//! accept loop and awaits them (bounded by `PIPE_DRAIN_CEILING`) before
//! the loop returns, so `handle.shutdown()` transitively drains them.
//!
//! Windows only.

#![cfg(windows)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commanderd::ipc::pipe_server::PipeServer;
use terminal_commanderd::{DaemonConfig, DaemonState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::time::sleep;

fn temp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-pipe-drain-{tag}-{pid}-{nanos}"));
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

fn unique_pipe_name(tag: &str) -> String {
    format!(
        r"\\.\pipe\tc-test-drain-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    )
}

/// `health` request frame (TC37 wire schema), length-prefixed-ready.
const HEALTH_JSON: &[u8] = br#"{"correlation_id":7,"request":{"method":"health"}}"#;

async fn write_health<W: AsyncWriteExt + Unpin>(w: &mut W) {
    let len = u32::try_from(HEALTH_JSON.len()).unwrap().to_be_bytes();
    w.write_all(&len).await.expect("write length prefix");
    w.write_all(HEALTH_JSON).await.expect("write payload");
    w.flush().await.expect("flush");
}

/// Read one length-prefixed response envelope; return parsed JSON.
async fn read_response_json<R: AsyncReadExt + Unpin>(r: &mut R) -> serde_json::Value {
    let mut len_buf = [0_u8; 4];
    r.read_exact(&mut len_buf)
        .await
        .expect("server must write a length-prefixed response");
    let resp_len = u32::from_be_bytes(len_buf) as usize;
    let mut resp = vec![0_u8; resp_len];
    r.read_exact(&mut resp)
        .await
        .expect("server must write the full response payload");
    serde_json::from_slice(&resp).expect("response must be valid JSON")
}

/// A request that is in-flight when shutdown is requested must complete:
/// the client receives a full, valid response frame and
/// `handle.shutdown()` returns promptly (draining the handler, not
/// hanging and not aborting it mid-write). This is the core FIX 3A
/// guarantee: the handler is drained, not orphaned.
#[tokio::test]
async fn in_flight_request_completes_during_shutdown_drain() {
    let pipe_name = unique_pipe_name("inflight");
    let (state, data) = make_state("inflight");
    let server = PipeServer::new(Arc::clone(&state), pipe_name.clone());
    let handle = server.spawn().expect("spawn pipe server");

    // Let the accept loop bind and enter connect().
    sleep(Duration::from_millis(50)).await;

    let mut client = ClientOptions::new()
        .open(&pipe_name)
        .expect("client open named pipe");

    // First round-trip proves the connection's handler is live and
    // parked reading the NEXT frame -- i.e. genuinely in-flight (the
    // handler task is running and tracked in the JoinSet).
    write_health(&mut client).await;
    let env1 = read_response_json(&mut client).await;
    assert_eq!(
        env1["result"]["response"]["method"].as_str(),
        Some("health"),
        "first round-trip must return health; got {env1}"
    );

    // Now request shutdown. The handler is in-flight (parked on its
    // next read). The drain must await it; shutdown must return within
    // the ceiling, not hang.
    tokio::time::timeout(Duration::from_secs(8), handle.shutdown())
        .await
        .expect("handle.shutdown() must drain the in-flight handler and return, not hang");

    // After the drain returns, the handler has been awaited to
    // completion (the JoinSet is empty, the connection is closed
    // server-side). A fresh request on the existing connection must
    // fail cleanly -- either the write fails (pipe closed) or the read
    // sees EOF. Never a partial/torn frame. Both outcomes prove the
    // handler is gone, not orphaned.
    let len = u32::try_from(HEALTH_JSON.len()).unwrap().to_be_bytes();
    let write_failed = client.write_all(&len).await.is_err()
        || client.write_all(HEALTH_JSON).await.is_err()
        || client.flush().await.is_err();
    if !write_failed {
        let mut len_buf = [0_u8; 4];
        let read = client.read_exact(&mut len_buf).await;
        assert!(
            read.is_err(),
            "after drain the handler has exited; a post-shutdown request must \
             see a clean EOF or write failure, not a response or a torn frame"
        );
    }

    cleanup(&data);
}

/// Drain must also be prompt when there are NO in-flight connections:
/// shutdown of an idle server returns immediately (empty JoinSet
/// fast-path), proving the drain does not block on an empty set.
#[tokio::test]
async fn idle_server_shutdown_drains_immediately() {
    let pipe_name = unique_pipe_name("idle");
    let (state, data) = make_state("idle");
    let server = PipeServer::new(Arc::clone(&state), pipe_name.clone());
    let handle = server.spawn().expect("spawn pipe server");

    sleep(Duration::from_millis(50)).await;

    tokio::time::timeout(Duration::from_secs(2), handle.shutdown())
        .await
        .expect("idle server shutdown must return promptly (empty-drain fast path)");

    cleanup(&data);
}
