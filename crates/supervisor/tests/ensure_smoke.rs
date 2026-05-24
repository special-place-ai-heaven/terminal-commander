// SPDX-License-Identifier: Apache-2.0
// Smoke test for ensure_daemon. Uses an already-running listener so
// the test does not depend on the real daemon binary; that exercise
// is covered by crates/daemon tests.

use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use terminal_commander_supervisor::ensure::{
    Endpoint, EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon,
};

#[cfg(unix)]
#[tokio::test]
async fn already_running_uds_endpoint_returns_already_running() {
    use tokio::net::UnixListener;
    let dir = TempDir::new().unwrap();
    let sock = dir.path().join("test.sock");
    let _listener = UnixListener::bind(&sock).unwrap();
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
        endpoint: Endpoint::WindowsPipe { name: r"\\.\pipe\terminal-commander-test-never-bound".into() },
        #[cfg(unix)]
        endpoint: Endpoint::UnixSocket { path: dir.path().join("never.sock") },
        startup_timeout: Duration::from_millis(50),
        allow_spawn: false,
    };
    let status = ensure_daemon(opts).await;
    match status {
        EnsureDaemonStatus::Unavailable { diagnostics, .. } => {
            assert!(!diagnostics.startup_attempted);
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}
