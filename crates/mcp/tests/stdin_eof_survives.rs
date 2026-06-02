// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// End-to-end: launch the daemon, launch MCP pointing at it, close
// MCP stdin, verify MCP exits 0 AND the daemon is still alive.
//
// Previously this test was #[cfg(unix)] gated because the Windows MCP
// binary exited 64 on startup. Task 3.5.5 wired the Windows named-pipe
// IPC transport into the MCP adapter, so the gate is removed and the
// test runs on both platforms.

use std::path::PathBuf;
use std::time::Duration;

use tempfile::TempDir;

/// Resolve a sibling binary in the same `target/<profile>/` directory
/// as the current test executable. This works for any crate in the
/// workspace because Cargo places all binaries under the same
/// `target/<profile>/` tree.
///
/// `CARGO_BIN_EXE_<name>` is only available for binaries in the same
/// package; for cross-package binaries we derive the path from the
/// current executable's location.
fn target_bin(name: &str) -> PathBuf {
    // The test binary lives at target/<profile>/deps/<test_binary>.
    // Sibling binaries live at target/<profile>/<name>[.exe].
    let exe = std::env::current_exe().expect("current_exe");
    // Go up from deps/ to profile/
    let profile_dir = exe
        .parent() // deps/
        .and_then(|p| p.parent()) // <profile>/
        .expect("profile dir");
    let mut bin = profile_dir.join(name);
    if cfg!(windows) {
        bin.set_extension("exe");
    }
    bin
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mcp_stdin_eof_does_not_kill_daemon() {
    let dir = TempDir::new().unwrap();
    let data_dir = dir.path().to_path_buf();

    let daemon_bin = target_bin("terminal-commanderd");
    let mcp_bin = target_bin("terminal-commander-mcp");

    assert!(
        daemon_bin.exists(),
        "daemon binary not found at {daemon_bin:?}; run `cargo build` first"
    );
    assert!(
        mcp_bin.exists(),
        "mcp binary not found at {mcp_bin:?}; run `cargo build` first"
    );

    // Resolve the endpoint path the daemon binds to — must be set BEFORE
    // spawning the daemon so we can pass it via TC_SOCKET on both platforms.
    //
    // Unix: <data_dir>/terminal-commanderd.sock (set via TC_SOCKET).
    // Windows: unique named pipe per test run, passed to both the daemon
    // and the MCP child via TC_SOCKET so the test never touches the
    // per-user global pipe that another daemon or test may already own.
    #[cfg(unix)]
    let tc_socket_env: Option<std::path::PathBuf> = Some(data_dir.join("terminal-commanderd.sock"));
    #[cfg(windows)]
    let tc_socket_env: Option<std::path::PathBuf> = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        Some(std::path::PathBuf::from(format!(
            r"\\.\pipe\tc-test-stdin-eof-{pid}-{nanos}"
        )))
    };

    let mut daemon_cmd = std::process::Command::new(&daemon_bin);
    daemon_cmd
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("start")
        .arg("--mode")
        .arg("ipc-server")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // On Windows, bind the daemon to the unique test pipe so it does not
    // claim the user's default global pipe.
    if let Some(ref sock) = tc_socket_env {
        daemon_cmd.env("TC_SOCKET", sock);
    }
    let mut daemon = daemon_cmd.spawn().expect("spawn daemon");

    // Wait briefly for daemon to bind.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut mcp_cmd = std::process::Command::new(&mcp_bin);
    mcp_cmd
        .env("TC_SUPERVISOR_ALLOW_SPAWN", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(ref sock) = tc_socket_env {
        mcp_cmd.env("TC_SOCKET", sock);
    }
    let mut mcp = mcp_cmd.spawn().expect("spawn mcp");

    // Close MCP stdin.
    drop(mcp.stdin.take());
    let status = mcp.wait().expect("mcp wait");
    assert_eq!(status.code(), Some(0), "MCP should exit 0 on stdin EOF");

    // Verify daemon still alive.
    assert!(
        daemon.try_wait().expect("try_wait").is_none(),
        "daemon should still be running"
    );

    daemon.kill().ok();
    let _ = daemon.wait();
}
