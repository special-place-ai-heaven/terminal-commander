//! `session reap` end-to-end coverage (unix only; spawns a real daemon).
//!
//! Two scenarios:
//! 1. `reap <token>` against a live per-session daemon: graceful Shutdown-IPC,
//!    daemon drains and exits, pidfile is removed.
//! 2. `reap <token>` against a STALE pidfile (dead pid, no daemon): the CLI
//!    must compare-before-delete via `sessions::cleanup_stale` and remove the
//!    stale pidfile. No real daemon is spawned for this case.
//!
//! Both spawns isolate via a fabricated TC_DATA + TC_SESSION; the daemon's
//! default state-dir resolution honors those (paths::resolve_state_dir reads
//! TC_DATA + TC_SESSION token), so its state_dir lands at `<base>/<token>`.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use terminal_commander_supervisor::pidfile::{
    RunningDaemon, pidfile_path, read_pidfile_raw, write_pidfile,
};

/// Cross-package binary discovery: `CARGO_BIN_EXE_<name>` is only available
/// for binaries in the same package. For the sibling `terminal-commanderd`
/// crate we derive the path from the test binary's location at
/// `target/<profile>/deps/<test>` -> sibling `target/<profile>/<name>[.exe]`.
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

fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-cli-sess-reap-{tag}-{pid}-{nanos}-{n}"));
    p
}

#[test]
fn session_reap_token_shuts_down_the_daemon() {
    let base = tmp_data_dir("alive");
    let token = "reaptest";
    let state_dir = base.join(token);

    // Spawn a real daemon under TC_DATA + TC_SESSION; no --data-dir means the
    // daemon falls through to `default_state_dir` -> supervisor's resolver,
    // which produces `<TC_DATA>/<TC_SESSION>` for the seeded session.
    //
    // `CARGO_BIN_EXE_<name>` is only available for binaries in the same
    // package. The daemon is a sibling crate, so we derive its path from the
    // test binary's location (target/<profile>/deps/<test> -> sibling
    // target/<profile>/terminal-commanderd[.exe]). The cli crate has the
    // daemon in [dev-dependencies] so cargo builds the daemon before tests.
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
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");

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

    // Reap.
    let out = Command::new(env!("CARGO_BIN_EXE_terminal-commander"))
        .args(["session", "reap", token])
        .env("TC_DATA", &base)
        .env_remove("TC_SOCKET")
        .env_remove("TC_SESSION")
        .output()
        .expect("run session reap");
    let cli_stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let cli_stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert_eq!(
        out.status.code(),
        Some(0),
        "reap must exit 0; stdout={cli_stdout}, stderr={cli_stderr}"
    );

    // After a successful graceful reap the daemon drains, removes its pidfile,
    // and exits. Bounded wait for pidfile to disappear.
    let down_deadline = Instant::now() + Duration::from_secs(5);
    let mut down = false;
    while Instant::now() < down_deadline {
        if !pid_path.exists() {
            down = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !down {
        let _ = daemon.kill();
    }
    let _ = daemon.wait();
    let _ = std::fs::remove_dir_all(&base);
    assert!(
        down,
        "daemon must be reaped (pidfile removed) within deadline; \
         cli stdout={cli_stdout}, cli stderr={cli_stderr}"
    );
}

#[test]
fn session_reap_token_cleans_stale_pidfile() {
    let base = tmp_data_dir("stale");
    let token = "deadguy";
    // Fabricate a stale pidfile naming a pid that cannot exist.
    write_pidfile(
        &base.join(token),
        &RunningDaemon {
            pid: 999_999_999,
            version: "0".into(),
            endpoint: "x".into(),
        },
    )
    .expect("write stale pidfile");

    let out = Command::new(env!("CARGO_BIN_EXE_terminal-commander"))
        .args(["session", "reap", token])
        .env("TC_DATA", &base)
        .env_remove("TC_SOCKET")
        .env_remove("TC_SESSION")
        .output()
        .expect("run session reap");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stale reap must exit 0; stdout={stdout}, stderr={stderr}"
    );
    assert!(
        read_pidfile_raw(&base.join(token)).is_none(),
        "stale pidfile must be removed; stdout={stdout}, stderr={stderr}"
    );
    let _ = std::fs::remove_dir_all(&base);
}
