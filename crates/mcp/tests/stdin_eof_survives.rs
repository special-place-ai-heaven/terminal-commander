// SPDX-License-Identifier: Apache-2.0
// End-to-end: launch the daemon, launch MCP pointing at it, close
// MCP stdin, verify MCP exits 0 AND the daemon is still alive.
//
// NOTE: This test is currently #[cfg(unix)] gated because the Windows
// MCP binary exits 64 on startup (Phase 3 IPC wiring for the MCP-side
// named-pipe client lands in a follow-up plan). Daemon-side stdin EOF
// behavior is tested implicitly by crates/mcp/tests/daemon_unavailable_envelope.rs
// which runs in-process and exercises the rmcp service shutdown path.

#[cfg(unix)]
use std::time::Duration;
#[cfg(unix)]
use tempfile::TempDir;

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mcp_stdin_eof_does_not_kill_daemon() {
    let dir = TempDir::new().unwrap();
    let data_dir = dir.path().to_path_buf();

    let daemon_bin = env!("CARGO_BIN_EXE_terminal-commanderd");
    let mcp_bin = env!("CARGO_BIN_EXE_terminal-commander-mcp");

    let mut daemon = std::process::Command::new(daemon_bin)
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("start")
        .arg("--mode")
        .arg("ipc-server")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");

    // Wait briefly for daemon to bind.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let endpoint = data_dir.join("terminal-commanderd.sock");

    let mut mcp = std::process::Command::new(mcp_bin)
        .env("TC_SOCKET", &endpoint)
        .env("TC_SUPERVISOR_ALLOW_SPAWN", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn mcp");

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
