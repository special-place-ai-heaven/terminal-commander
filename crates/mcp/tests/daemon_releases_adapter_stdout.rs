// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// When the adapter starts a daemon and then exits, whoever launched the
// adapter must see end-of-output on its stdout pipe. On Windows the daemon
// used to inherit the adapter's stdio pipe handles, so a launcher reading
// stdout to the end blocked forever (the v0.3.3 Windows verify job hung
// exactly this way).

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use tempfile::TempDir;

#[path = "../../test_support/isolated_env.rs"]
mod isolated_env;

fn target_bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe.parent().and_then(|p| p.parent()).expect("profile dir");
    let mut bin = profile_dir.join(name);
    if cfg!(windows) {
        bin.set_extension("exe");
    }
    bin
}

fn kill_pid(pid: u32) {
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(unix)]
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}

#[test]
fn adapter_stdout_reaches_eof_after_it_starts_a_daemon_and_exits() {
    let dir = TempDir::new().unwrap();
    let state_dir = dir.path().join("state");
    let mcp_bin = target_bin("terminal-commander-mcp");
    assert!(
        target_bin("terminal-commanderd").exists() && mcp_bin.exists(),
        "adapter/daemon binaries missing; run `cargo build` first"
    );

    #[cfg(unix)]
    let socket = dir.path().join("tcd.sock").display().to_string();
    #[cfg(windows)]
    let socket = format!(r"\\.\pipe\tc-test-stdout-eof-{}", std::process::id());

    let mut mcp = isolated_env::isolate(&mut Command::new(&mcp_bin), dir.path())
        .arg("--state-dir")
        .arg(&state_dir)
        .env("TC_SOCKET", &socket)
        .env("TC_DATA", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn adapter");
    let mut stdout = mcp.stdout.take().expect("stdout pipe");
    // Closing stdin makes the adapter exit once it has ensured the daemon.
    drop(mcp.stdin.take());

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut sink = Vec::new();
        let _ = tx.send(stdout.read_to_end(&mut sink).map(|_| ()));
    });
    let status = mcp.wait().expect("adapter wait");
    let eof = rx.recv_timeout(Duration::from_secs(15));

    let daemon = terminal_commander_supervisor::pidfile::read_pidfile_raw(&state_dir);
    if let Some(d) = &daemon {
        kill_pid(d.pid);
    }

    assert!(status.success(), "adapter exit: {status:?}");
    assert!(
        daemon.is_some(),
        "adapter did not start a daemon, so the test proved nothing"
    );
    assert!(
        matches!(eof, Ok(Ok(()))),
        "adapter stdout never reached EOF after the adapter exited: the daemon \
         it started still holds the pipe ({eof:?})"
    );
}
