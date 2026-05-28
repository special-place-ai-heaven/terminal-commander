// SPDX-License-Identifier: Apache-2.0
// Smoke test for ensure_daemon. Uses an already-running listener so
// the test does not depend on the real daemon binary; that exercise
// is covered by crates/daemon tests.

use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use terminal_commander_supervisor::ensure::{
    DaemonUnavailableReason, Endpoint, EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon,
};

#[cfg(unix)]
#[tokio::test]
async fn already_running_uds_endpoint_returns_already_running() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;
    let dir = TempDir::new().unwrap();
    let sock = dir.path().join("test.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    // A real daemon answers the health handshake; probe_endpoint now
    // requires a well-formed IpcResponse::Health (a bare connectable
    // socket is no longer accepted). Stand in for the daemon with a
    // minimal accept loop that replies with one length-prefixed Health
    // frame per connection, matching the TC37 wire format.
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            // Drain the request frame (4-byte len + payload), then reply.
            let mut len_buf = [0_u8; 4];
            if stream.read_exact(&mut len_buf).await.is_err() {
                continue;
            }
            let req_len = u32::from_be_bytes(len_buf) as usize;
            let mut req = vec![0_u8; req_len];
            if stream.read_exact(&mut req).await.is_err() {
                continue;
            }
            let resp = br#"{"correlation_id":0,"result":{"kind":"ok","response":{"method":"health","uptime_secs":1}}}"#;
            let resp_len = u32::try_from(resp.len()).unwrap().to_be_bytes();
            let _ = stream.write_all(&resp_len).await;
            let _ = stream.write_all(resp).await;
            let _ = stream.flush().await;
        }
    });
    let opts = EnsureDaemonOptions {
        daemon_binary: PathBuf::from("/bin/true"),
        state_dir: dir.path().into(),
        log_dir: dir.path().into(),
        endpoint: Endpoint::UnixSocket { path: sock },
        startup_timeout: Duration::from_millis(500),
        allow_spawn: false,
    };
    let status = ensure_daemon(opts).await;
    assert!(matches!(status, EnsureDaemonStatus::AlreadyRunning { .. }));
}

#[tokio::test]
async fn no_listener_no_spawn_returns_unavailable() {
    let dir = TempDir::new().unwrap();
    let opts = EnsureDaemonOptions {
        daemon_binary: PathBuf::from("nonexistent"),
        state_dir: dir.path().into(),
        log_dir: dir.path().into(),
        #[cfg(windows)]
        endpoint: Endpoint::WindowsPipe {
            name: r"\\.\pipe\terminal-commander-test-never-bound".into(),
        },
        #[cfg(unix)]
        endpoint: Endpoint::UnixSocket {
            path: dir.path().join("never.sock"),
        },
        startup_timeout: Duration::from_millis(50),
        allow_spawn: false,
    };
    let status = ensure_daemon(opts).await;
    match status {
        EnsureDaemonStatus::Unavailable {
            reason,
            diagnostics,
        } => {
            assert!(
                matches!(reason, DaemonUnavailableReason::EndpointBindFailed),
                "expected EndpointBindFailed, got {reason:?}"
            );
            assert!(!diagnostics.startup_attempted);
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}
