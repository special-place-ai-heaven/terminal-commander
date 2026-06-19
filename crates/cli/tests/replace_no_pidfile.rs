//! Regression: a reachable, CURRENT-version daemon with a MISSING pidfile
//! must NOT be hard-killed by `replace_if_stale` (the "daemon flaps under
//! use" bug).
//!
//! Root cause (crates/supervisor/src/replace.rs, no-pidfile branch): the
//! replace path treated "reachable + no pidfile" as unconditionally
//! "predates the pidfile feature => stale" and hard-killed the daemon. But
//! the daemon writes its pidfile exactly once at bind and never re-asserts a
//! lost one, so any event that removes the pidfile of a still-running current
//! daemon (a `session reap` that mis-classified it, a transient fs error, a
//! lost rename, manual cleanup) turned EVERY subsequent adapter startup into
//! a kill-and-respawn: the in-use daemon vanished mid-workflow (orphaning its
//! child + losing job state) and a fresh 0s-uptime daemon came up.
//!
//! The fix asks the live daemon its version over the existing health
//! handshake (the wire `Health` response carries `version`) before killing a
//! pidfile-less daemon; a daemon reporting a non-stale version is left alone.
//!
//! This is the END-TO-END proof against the REAL binary: spawn a real current
//! daemon, delete its pidfile, then run the real `replace_if_stale` and assert
//! the daemon SURVIVES (`UpToDate`, pid still alive, endpoint still served).
//! Before the fix this test fails: the daemon is killed and the outcome is
//! `Replaced { old: "pre-pidfile", .. }`.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use terminal_commander_supervisor::ensure::{Endpoint, EnsureDaemonOptions};
use terminal_commander_supervisor::pidfile::{pid_alive, pidfile_path};
use terminal_commander_supervisor::replace::{ReplaceOutcome, replace_if_stale};

/// Cross-package binary discovery (same approach as session_reap.rs): the
/// daemon is a sibling crate, so derive its path from this test binary's
/// location: `target/<profile>/deps/<test>` -> `target/<profile>/<name>`.
fn target_bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe.parent().and_then(|p| p.parent()).expect("profile dir");
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
    p.push(format!("tc-replace-nopid-{tag}-{pid}-{nanos}-{n}"));
    p
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reachable_current_daemon_with_missing_pidfile_is_not_killed() {
    let data = tmp_data_dir("survive");
    let sock = data.join("tc.sock");
    let daemon_bin = target_bin("terminal-commanderd");
    // This test needs a CURRENT terminal-commanderd — one that reports its
    // `version` over the health handshake (added in 0.1.52). The `--workspace`
    // gate builds it fresh; an ad-hoc `cargo test -p terminal-commander-cli`
    // can pick up a STALE `target/debug` binary (no health.version), which
    // makes the fix's version probe see `None` and this test fail misleadingly.
    // Build terminal-commanderd first if running this in isolation.
    assert!(
        daemon_bin.exists(),
        "daemon binary not found at {daemon_bin:?}; build terminal-commanderd \
         (the --workspace gate does this) before running this test in isolation"
    );

    // Spawn a REAL current-version daemon bound to an isolated socket.
    let mut daemon = Command::new(&daemon_bin)
        .args(["--data-dir"])
        .arg(&data)
        .args(["start", "--mode", "ipc-server"])
        .env("TC_SOCKET", &sock)
        .env_remove("TC_SESSION")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");

    // Wait for the daemon to bind (its pidfile appears).
    let pid_path = pidfile_path(&data);
    let bind_deadline = Instant::now() + Duration::from_secs(10);
    while !pid_path.exists() && Instant::now() < bind_deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if !pid_path.exists() {
        let _ = daemon.kill();
        let _ = daemon.wait();
        let _ = std::fs::remove_dir_all(&data);
        panic!(
            "daemon never bound: pidfile missing at {}",
            pid_path.display()
        );
    }
    let daemon_pid = daemon.id();

    // The trigger: the pidfile is LOST while the daemon stays alive + serving.
    std::fs::remove_file(&pid_path).expect("remove pidfile to simulate the flap trigger");
    assert!(
        pid_alive(daemon_pid),
        "precondition: the daemon must still be alive after its pidfile is removed"
    );

    let endpoint = Endpoint::UnixSocket { path: sock.clone() };
    let opts = EnsureDaemonOptions {
        daemon_binary: daemon_bin.clone(),
        state_dir: data.clone(),
        log_dir: data.join("logs"),
        endpoint: endpoint.clone(),
        startup_timeout: Duration::from_secs(10),
        allow_spawn: true,
    };

    // The decisive call: this is exactly what the adapter runs at startup.
    // BEFORE the fix this returns `Replaced { old: "pre-pidfile", .. }` and
    // hard-kills `daemon_pid`. AFTER the fix it asks the live daemon its
    // version over IPC, sees a current (non-stale) version, and leaves it be.
    let outcome = replace_if_stale(&opts, env!("CARGO_PKG_VERSION"), false).await;

    // Give any (erroneous) kill its full grace + endpoint-drain window before
    // we assert survival, so a flake can't mask a real kill.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let still_alive = pid_alive(daemon_pid);

    // Clean up the real daemon before asserting, so a failed assert never
    // leaks the process.
    let _ = daemon.kill();
    let _ = daemon.wait();
    let _ = std::fs::remove_dir_all(&data);

    assert!(
        matches!(outcome, ReplaceOutcome::UpToDate { .. }),
        "a reachable CURRENT daemon with a missing pidfile must be left UpToDate, \
         not replaced; got {outcome:?}"
    );
    assert!(
        still_alive,
        "the current daemon (pid {daemon_pid}) must NOT be killed merely because \
         its pidfile went missing — that is the 'daemon flaps under use' bug"
    );
}
