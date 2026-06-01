//! `status` end-to-end coverage: the pid column must show the REAL pid of a
//! live daemon, sourced from the pidfile (the Health handshake carries none).
//!
//! Cross-platform: spawns a real `terminal-commanderd` under an isolated
//! TC_DATA + TC_SESSION (so the daemon's state-dir, socket/pipe, and pidfile
//! all co-locate at `<base>/<token>`), then runs `terminal-commander status`
//! with the SAME TC_DATA + TC_SESSION so the CLI probes that exact endpoint and
//! reads that exact pidfile. Asserts stdout shows `pid : <n>` matching the
//! spawned pid and exit 0.
//!
//! Also asserts the OFFLINE posture stays intact: with no daemon and an
//! isolated empty state-dir, `status` exits non-zero and shows `pid : -`
//! (never a fabricated pid).

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use terminal_commander_supervisor::pidfile::pidfile_path;

/// Cross-package binary discovery: `CARGO_BIN_EXE_<name>` is only defined for
/// binaries in the SAME package. The daemon is a sibling crate (a cli
/// dev-dependency, so cargo builds it before these tests), so derive its path
/// from the test binary's location: `target/<profile>/deps/<test>` ->
/// sibling `target/<profile>/<name>[.exe]`.
fn target_bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
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

/// A unique temp base dir per test invocation, so parallel test threads and
/// repeated runs never collide on state-dir / pidfile / socket.
fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-cli-status-pid-{tag}-{pid}-{nanos}-{n}"));
    p
}

/// Extract the value rendered after the `pid           : ` label in
/// `terminal-commander status` stdout. Returns the trimmed value (e.g.
/// `"1234"` or `"-"`), or `None` if the line is absent.
fn parse_pid_line(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let trimmed = line.trim_start();
        trimmed
            .strip_prefix("pid")
            .map(str::trim_start)
            .and_then(|rest| rest.strip_prefix(':'))
            .map(|val| val.trim().to_string())
    })
}

#[test]
fn status_shows_real_pid_of_live_daemon() {
    let base = tmp_data_dir("alive");
    let token = "statustest";
    let state_dir = base.join(token);

    // Spawn a real daemon under TC_DATA + TC_SESSION. With no --data-dir the
    // daemon falls through to the supervisor's resolver, landing its state-dir
    // (and socket/pipe + pidfile) at `<base>/<token>`. TC_SOCKET is removed so
    // the daemon derives its endpoint from the session, matching what the CLI
    // will probe.
    let daemon_bin = target_bin("terminal-commanderd");
    assert!(
        daemon_bin.exists(),
        "daemon binary not found at {daemon_bin:?}; cargo dev-dep should have built it"
    );
    let mut daemon = Command::new(&daemon_bin)
        .args(["start", "--mode", "ipc-server"])
        .env("TC_DATA", &base)
        .env("TC_SESSION", token)
        .env_remove("TC_SOCKET")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn daemon");
    let spawned_pid = daemon.id();

    // Wait up to 5s for the pidfile to appear (= daemon bound IPC).
    let pid_path = pidfile_path(&state_dir);
    let bind_deadline = Instant::now() + Duration::from_secs(5);
    while !pid_path.exists() && Instant::now() < bind_deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if !pid_path.exists() {
        let _ = daemon.kill();
        let _ = daemon.wait();
        let _ = std::fs::remove_dir_all(&base);
        panic!(
            "daemon never bound: pidfile missing at {}",
            pid_path.display()
        );
    }

    // Run `status` with the SAME TC_DATA + TC_SESSION so the CLI probes the
    // daemon's exact endpoint and reads its exact pidfile.
    let out = Command::new(env!("CARGO_BIN_EXE_terminal-commander"))
        .arg("status")
        .env("TC_DATA", &base)
        .env("TC_SESSION", token)
        .env_remove("TC_SOCKET")
        .output()
        .expect("run status");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    // Tear the daemon down before any assertion can unwind the test.
    let _ = daemon.kill();
    let _ = daemon.wait();
    let _ = std::fs::remove_dir_all(&base);

    assert_eq!(
        out.status.code(),
        Some(0),
        "status against a live daemon must exit 0; stdout={stdout}, stderr={stderr}"
    );
    let pid_line = parse_pid_line(&stdout)
        .unwrap_or_else(|| panic!("status output missing a pid line; stdout={stdout}"));
    assert_eq!(
        pid_line,
        spawned_pid.to_string(),
        "status pid must be the live daemon's real pid (from the pidfile), \
         not '-'; stdout={stdout}, stderr={stderr}"
    );
}

#[test]
fn status_offline_shows_dash_and_nonzero_exit() {
    // Isolated empty state-dir with a unique TC_SESSION token: no daemon, no
    // pidfile. `status` must report unavailable (non-zero exit) and pid '-',
    // never a fabricated pid.
    let base = tmp_data_dir("offline");
    let token = "nodaemon";
    // Ensure nothing pre-exists.
    let _ = std::fs::remove_dir_all(&base);

    let out = Command::new(env!("CARGO_BIN_EXE_terminal-commander"))
        .arg("status")
        .env("TC_DATA", &base)
        .env("TC_SESSION", token)
        .env_remove("TC_SOCKET")
        .output()
        .expect("run status");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let _ = std::fs::remove_dir_all(&base);

    assert_ne!(
        out.status.code(),
        Some(0),
        "status with no daemon must exit non-zero; stdout={stdout}, stderr={stderr}"
    );
    let pid_line = parse_pid_line(&stdout)
        .unwrap_or_else(|| panic!("status output missing a pid line; stdout={stdout}"));
    assert_eq!(
        pid_line, "-",
        "offline status must show pid '-', never a fabricated pid; stdout={stdout}"
    );
}
