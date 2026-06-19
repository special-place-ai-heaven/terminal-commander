//! Regression / close-out: a live daemon that LOSES its pidfile re-asserts it.
//!
//! Root cause of the "daemon flaps under use" class (crates/daemon/src/runtime.rs):
//! the daemon wrote its pidfile exactly once at bind and never recovered a lost
//! one, so any event that removed a live daemon's pidfile (a mis-classified
//! reap, a transient fs error, a lost atomic rename, manual cleanup) left it
//! running but pidfile-less. The version-aware replace path then mis-read that
//! as "predates the pidfile feature => stale" and hard-killed it.
//!
//! The 0.1.53 kill-guard (replace_no_pidfile.rs) stops the WRONG KILL by asking
//! the live daemon its version before killing it. This test proves the deeper
//! close: the daemon RE-ASSERTS its own pidfile, so the pidfile-less window
//! never persists at all. That also covers the residuals the kill-guard does
//! not — a legacy daemon that can't report a version, `session list`/`reap`
//! identification, and `update --force` during the gap.
//!
//! This is the END-TO-END proof against the REAL binary: spawn a real daemon,
//! delete its pidfile, and assert it REAPPEARS with the correct pid + version +
//! endpoint within a bounded wait (longer than one self-heal tick). Before the
//! re-assert task existed the pidfile would stay gone for the daemon's lifetime.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use terminal_commander_supervisor::pidfile::{pid_alive, pidfile_path, read_pidfile_raw};

/// Cross-package binary discovery (same approach as replace_no_pidfile.rs): the
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
    p.push(format!("tc-pidfile-reassert-{tag}-{pid}-{nanos}-{n}"));
    p
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_reasserts_a_removed_pidfile() {
    let data = tmp_data_dir("reappear");
    let sock = data.join("tc.sock");
    let daemon_bin = target_bin("terminal-commanderd");
    // The `--workspace` gate builds terminal-commanderd fresh. Running this in
    // isolation needs `cargo build -p terminal-commanderd` first (a stale binary
    // without the re-assert task makes this fail misleadingly — see the note in
    // replace_no_pidfile.rs).
    assert!(
        daemon_bin.exists(),
        "daemon binary not found at {daemon_bin:?}; build terminal-commanderd \
         (the --workspace gate does this) before running this test in isolation"
    );

    // Spawn a REAL daemon bound to an isolated socket.
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

    // Wait for the daemon to bind (its pidfile appears for the first time).
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
    let endpoint = sock.display().to_string();

    // The trigger: remove the pidfile while the daemon stays alive + serving.
    std::fs::remove_file(&pid_path).expect("remove pidfile to simulate the flap trigger");
    assert!(
        !pid_path.exists(),
        "precondition: the pidfile must actually be gone after removal"
    );
    assert!(
        pid_alive(daemon_pid),
        "precondition: the daemon must still be alive after its pidfile is removed"
    );

    // The self-heal task ticks every PIDFILE_REASSERT_TICK_SECS (15s). Wait a
    // bounded window longer than one tick for the pidfile to REAPPEAR.
    let reassert_deadline = Instant::now() + Duration::from_secs(25);
    let mut recovered = None;
    while Instant::now() < reassert_deadline {
        if let Some(rec) = read_pidfile_raw(&data) {
            recovered = Some(rec);
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Clean up the real daemon before asserting, so a failed assert never leaks
    // the process.
    let _ = daemon.kill();
    let _ = daemon.wait();
    let _ = std::fs::remove_dir_all(&data);

    let rec = recovered.expect(
        "the daemon must re-assert its pidfile after it goes missing; \
         none reappeared within the bounded wait (one self-heal tick)",
    );
    assert_eq!(
        rec.pid, daemon_pid,
        "the re-asserted pidfile must record the SAME live daemon's pid"
    );
    assert_eq!(
        rec.version,
        env!("CARGO_PKG_VERSION"),
        "the re-asserted pidfile must record the current daemon version"
    );
    assert_eq!(
        rec.endpoint, endpoint,
        "the re-asserted pidfile must record the bound endpoint"
    );
}
