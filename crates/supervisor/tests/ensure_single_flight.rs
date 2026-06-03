// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// T2 (H6): two concurrent `ensure_daemon` calls against a shared
// state_dir must single-flight the bring-up. Exactly one call "owns"
// the spawn (returns `Started`), the other observes the freshly-bound
// endpoint and returns `AlreadyRunning`. Neither returns `Unavailable`.
//
// In-process model: the "daemon binary" is a no-op (`/bin/true`) that
// exits immediately and never binds; a fake health-answering listener
// (started by the test after a short delay) provides the bind that the
// winner's wait-for-bind and the loser's re-probe both observe. This
// exercises the cross-process lock + the Contended re-probe path without
// the real daemon binary (covered by the daemon-crate T3).

#![cfg(unix)]

use std::path::PathBuf;
use std::time::Duration;

use tempfile::TempDir;
use terminal_commander_supervisor::ensure::{
    Endpoint, EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon,
};

/// Spawn a minimal UDS listener that binds `sock` after `delay` and
/// replies to every connection with one length-prefixed Health frame,
/// matching the TC37 wire format `probe_endpoint` requires.
fn spawn_fake_daemon(sock: PathBuf, delay: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let listener = match tokio::net::UnixListener::bind(&sock) {
            Ok(l) => l,
            Err(_) => return,
        };
        loop {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_concurrent_ensure_single_flight_one_owner_one_already_running() {
    let dir = TempDir::new().unwrap();
    let state_dir: PathBuf = dir.path().into();
    let sock = state_dir.join("tc.sock");

    // The fake endpoint binds shortly after both ensure_daemon calls have
    // started (so both observe an initial probe-miss), but well within the
    // generous startup_timeout.
    spawn_fake_daemon(sock.clone(), Duration::from_millis(250));

    let make_opts = || EnsureDaemonOptions {
        // A no-op binary: spawn succeeds, process exits immediately and
        // never binds. The fake listener supplies the bind.
        daemon_binary: PathBuf::from("/bin/true"),
        state_dir: state_dir.clone(),
        log_dir: state_dir.clone(),
        endpoint: Endpoint::UnixSocket { path: sock.clone() },
        startup_timeout: Duration::from_secs(5),
        allow_spawn: true,
    };

    let a = tokio::spawn(ensure_daemon(make_opts()));
    let b = tokio::spawn(ensure_daemon(make_opts()));
    let (ra, rb) = tokio::join!(a, b);
    let ra = ra.unwrap();
    let rb = rb.unwrap();

    let n_started = [&ra, &rb]
        .iter()
        .filter(|s| matches!(s, EnsureDaemonStatus::Started { .. }))
        .count();
    let n_already = [&ra, &rb]
        .iter()
        .filter(|s| matches!(s, EnsureDaemonStatus::AlreadyRunning { .. }))
        .count();
    let n_unavailable = [&ra, &rb]
        .iter()
        .filter(|s| matches!(s, EnsureDaemonStatus::Unavailable { .. }))
        .count();

    assert_eq!(
        n_unavailable, 0,
        "neither call may be Unavailable; got a={ra:?} b={rb:?}"
    );
    assert_eq!(
        n_started, 1,
        "exactly one call should own the spawn (Started); got a={ra:?} b={rb:?}"
    );
    assert_eq!(
        n_already, 1,
        "exactly one call should see AlreadyRunning; got a={ra:?} b={rb:?}"
    );

    // The lock file persists (we never delete it) and is a sibling of the
    // pidfile under state_dir.
    assert!(
        state_dir.join("terminal-commanderd.lock").exists(),
        "bring-up lock file should exist under state_dir"
    );
}
