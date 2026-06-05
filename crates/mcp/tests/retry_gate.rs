// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Integration test: the MCP daemon-client retry gate (TC-1a).
//!
//! `McpDaemonClient::call` must NEVER blindly re-send a MUTATING RPC after a
//! mid-call transport failure: a client-side timeout / dropped pipe cannot
//! prove the daemon did not already perform the side effect, so a re-send
//! risks a silent double-effect (the HEAD double-spawn regression). Idempotent
//! reads, by contrast, are safe to retry once.
//!
//! Harness: a counting fake daemon over a temp unix socket (the same minimal
//! `UnixListener` accept-loop pattern as
//! `crates/supervisor/tests/ensure_single_flight.rs`). The fake accepts a
//! connection, reads the request frame, bumps a connection counter, then drops
//! the stream WITHOUT writing a response. The client's response read then hits
//! EOF and surfaces an `IpcError::transport`. Counting connections counts
//! re-sends: a non-idempotent request must produce exactly ONE connection, an
//! idempotent request exactly TWO (the original plus the single retry).
//!
//! Source-status: test-only/mock (no real daemon; the fake speaks only the
//! TC37 length-prefixed wire framing far enough to receive a request).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use terminal_commander_ipc::{
    CommandStartParams, IpcRequest, PtyCommandStartParams, SubscriptionPullParams,
};
use terminal_commander_mcp::daemon_client::McpDaemonClient;

/// Spawn a fake daemon that counts inbound request connections and then drops
/// each one without replying, forcing a transport error on the client. Returns
/// the shared counter. Binds synchronously (tokio `UnixListener::bind` is not
/// async) before returning so the client can connect immediately. Must be
/// called from within a tokio runtime (the `#[tokio::test]` provides one).
fn spawn_counting_drop_daemon(sock: &std::path::Path) -> Arc<AtomicU64> {
    let count = Arc::new(AtomicU64::new(0));
    let listener = tokio::net::UnixListener::bind(sock).expect("bind fake daemon socket");
    let task_count = Arc::clone(&count);
    tokio::spawn(async move {
        loop {
            use tokio::io::AsyncReadExt;
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            // Read the length prefix + request body so the connection counts as
            // a real delivered request, then drop the stream (no response).
            let mut len_buf = [0_u8; 4];
            if stream.read_exact(&mut len_buf).await.is_err() {
                // Count even a header-only connection: the client did reach us.
                task_count.fetch_add(1, Ordering::SeqCst);
                continue;
            }
            let req_len = u32::from_be_bytes(len_buf) as usize;
            let mut req = vec![0_u8; req_len];
            let _ = stream.read_exact(&mut req).await;
            task_count.fetch_add(1, Ordering::SeqCst);
            // Drop `stream` here: the client's response read sees EOF and maps
            // it to a transport error.
        }
    });
    count
}

fn unique_sock(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tc-mcp-retry-gate-{label}-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ))
}

/// Wait until the connection counter reaches at least `want`, or the budget
/// elapses. Returns the observed count. Bounds the inherent accept-loop lag
/// between the client's `call` returning and the server task counting.
async fn wait_for_count(count: &AtomicU64, want: u64) -> u64 {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let now = count.load(Ordering::SeqCst);
        if now >= want || std::time::Instant::now() >= deadline {
            return now;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn command_start() -> IpcRequest {
    IpcRequest::CommandStartCombed(CommandStartParams {
        environment: None,
        argv: vec!["sleep".to_owned(), "10".to_owned()],
        cwd: None,
        env: vec![],
        bucket_config: None,
        rules: vec![],
        grace_ms: None,
        tag: None,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mutating_request_is_not_resent_on_transport_failure() {
    let sock = unique_sock("mutating");
    let count = spawn_counting_drop_daemon(&sock);
    let client = McpDaemonClient::new(&sock).with_timeout(Duration::from_millis(500));

    let result = client.call(command_start()).await;
    assert!(
        result.is_err(),
        "the drop-without-reply daemon must surface a transport error"
    );

    // Give the server task a chance to count beyond 1 if a (forbidden) re-send
    // had occurred, then assert exactly one connection was made.
    let observed = wait_for_count(&count, 2).await;
    assert_eq!(
        observed, 1,
        "a MUTATING CommandStartCombed must NOT be re-sent after a transport \
         failure (got {observed} connections; >1 means a double-spawn risk)"
    );

    let _ = std::fs::remove_file(&sock);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pty_start_is_not_resent_on_transport_failure() {
    // PtyCommandStart is also is_idempotent()==false; lock in that the gate
    // covers the PTY start path, not only command_start_combed.
    let sock = unique_sock("pty");
    let count = spawn_counting_drop_daemon(&sock);
    let client = McpDaemonClient::new(&sock).with_timeout(Duration::from_millis(500));

    let req = IpcRequest::PtyCommandStart(PtyCommandStartParams {
        environment: None,
        argv: vec!["sleep".to_owned(), "10".to_owned()],
        cwd: None,
        env: vec![],
        bucket_config: None,
        rules: vec![],
        rows: None,
        cols: None,
        tag: None,
    });
    let result = client.call(req).await;
    assert!(result.is_err(), "transport error expected");

    let observed = wait_for_count(&count, 2).await;
    assert_eq!(
        observed, 1,
        "a MUTATING PtyCommandStart must NOT be re-sent (got {observed})"
    );

    let _ = std::fs::remove_file(&sock);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscription_pull_is_not_resent_on_transport_failure() {
    // SubscriptionPull commits per-consumer offsets server-side inside the
    // pull, so a blind retry silently drops already-drained events. It is
    // classified non-idempotent; assert the gate does not re-send it.
    let sock = unique_sock("subpull");
    let count = spawn_counting_drop_daemon(&sock);
    let client = McpDaemonClient::new(&sock).with_timeout(Duration::from_millis(500));

    let req = IpcRequest::SubscriptionPull(SubscriptionPullParams {
        sub_id: "sub-1".to_owned(),
        max: None,
        timeout_ms: None,
    });
    let result = client.call(req).await;
    assert!(result.is_err(), "transport error expected");

    let observed = wait_for_count(&count, 2).await;
    assert_eq!(
        observed, 1,
        "a MUTATING SubscriptionPull must NOT be re-sent (got {observed})"
    );

    let _ = std::fs::remove_file(&sock);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idempotent_request_is_resent_once_on_transport_failure() {
    let sock = unique_sock("idempotent");
    let count = spawn_counting_drop_daemon(&sock);
    let client = McpDaemonClient::new(&sock).with_timeout(Duration::from_millis(500));

    // Health is is_idempotent()==true: safe to retry once. With no status
    // handle, try_self_heal is a no-op, so exactly the original + one retry
    // reach the fake daemon.
    let result = client.call(IpcRequest::Health).await;
    assert!(
        result.is_err(),
        "the drop-without-reply daemon must surface a transport error"
    );

    let observed = wait_for_count(&count, 2).await;
    assert_eq!(
        observed, 2,
        "an IDEMPOTENT Health request IS re-sent exactly once after a \
         transport failure (expected 2 connections, got {observed})"
    );

    let _ = std::fs::remove_file(&sock);
}
