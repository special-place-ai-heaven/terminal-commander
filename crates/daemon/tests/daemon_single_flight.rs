// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// T3 (H6): REAL-subprocess single-flight bring-up.
//
// Two cold-starts against ONE fresh state_dir must converge on exactly
// ONE live daemon: one pidfile, the endpoint answers Health, and any
// extra process has exited. Uses the real `terminal-commanderd` binary
// via CARGO_BIN_EXE so the cross-process lock + the daemon-side guard
// are exercised end to end (an in-process lock would not).
//
// Two scenarios:
//   1. `two_ensure_daemon_cold_starts_*`: a genuine race through the
//      supervisor's `ensure_daemon` single-flight lock (both OSes).
//   2. `second_daemon_does_not_orphan_first` (#[cfg(unix)]): a second raw
//      daemon, launched after the first is up, must NOT `remove_file` +
//      rebind the socket and orphan the first — the Unix-specific bug H6
//      fixes via the daemon-side guard.
//
// Determinism: all waits are tolerance-polled against a bounded deadline,
// never fixed sleeps. The supervisor lock makes scenario 1 deterministic
// (only one spawn wins); scenario 2 gives the first daemon a polled
// head-start so the second deterministically observes a live, matching
// pidfile and exits via the `AlreadyServed` path.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use terminal_commander_supervisor::ensure::{
    Endpoint, EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon,
};
use terminal_commander_supervisor::pidfile;

fn fresh_state_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "tc-sf-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[cfg(unix)]
fn endpoint_for(state_dir: &Path) -> (Endpoint, PathBuf) {
    let sock = state_dir.join("tc.sock");
    (Endpoint::UnixSocket { path: sock.clone() }, sock)
}

#[cfg(windows)]
fn endpoint_for(state_dir: &Path) -> (Endpoint, PathBuf) {
    // A per-test unique pipe name keyed off the state_dir's leaf so
    // concurrent test binaries do not collide on one global pipe.
    let leaf = state_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("tc");
    let name = format!(r"\\.\pipe\terminal-commander-sf-{leaf}");
    (
        Endpoint::WindowsPipe { name: name.clone() },
        PathBuf::from(name),
    )
}

fn make_opts(state_dir: &Path, endpoint: &Endpoint) -> EnsureDaemonOptions {
    EnsureDaemonOptions {
        daemon_binary: PathBuf::from(env!("CARGO_BIN_EXE_terminal-commanderd")),
        state_dir: state_dir.to_path_buf(),
        log_dir: state_dir.join("logs"),
        endpoint: endpoint.clone(),
        startup_timeout: Duration::from_secs(10),
        allow_spawn: true,
    }
}

/// Poll until `f()` is true or the deadline passes. Returns the final
/// value of `f()`.
async fn poll_until(deadline: Instant, mut f: impl FnMut() -> bool) -> bool {
    loop {
        if f() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// True when the endpoint answers a Health handshake (probe-only, no
/// spawn).
async fn endpoint_answers_health(state_dir: &Path, endpoint: &Endpoint) -> bool {
    let opts = EnsureDaemonOptions {
        allow_spawn: false,
        startup_timeout: Duration::from_millis(200),
        ..make_opts(state_dir, endpoint)
    };
    matches!(
        ensure_daemon(opts).await,
        EnsureDaemonStatus::AlreadyRunning { .. }
    )
}

/// Best-effort: kill the daemon recorded in the pidfile and clean up.
async fn teardown(state_dir: &Path) {
    if let Some(rec) = pidfile::read_pidfile(state_dir) {
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &rec.pid.to_string()])
                .status();
        }
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &rec.pid.to_string(), "/F"])
                .output();
        }
    }
    // Give the OS a moment to release handles, then remove the dir.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let _ = std::fs::remove_dir_all(state_dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_ensure_daemon_cold_starts_converge_on_one_live_daemon() {
    let state_dir = fresh_state_dir("race");
    let (endpoint, _ep_path) = endpoint_for(&state_dir);

    let a = tokio::spawn(ensure_daemon(make_opts(&state_dir, &endpoint)));
    let b = tokio::spawn(ensure_daemon(make_opts(&state_dir, &endpoint)));
    let (ra, rb) = tokio::join!(a, b);
    let ra = ra.unwrap();
    let rb = rb.unwrap();

    let unavailable = [&ra, &rb]
        .iter()
        .filter(|s| matches!(s, EnsureDaemonStatus::Unavailable { .. }))
        .count();
    assert_eq!(
        unavailable, 0,
        "neither cold start may be Unavailable; a={ra:?} b={rb:?}"
    );

    // Exactly one pidfile recording one live daemon, and the endpoint
    // answers Health. Poll: the loser's AlreadyRunning may return a hair
    // before the winner finishes writing the pidfile.
    let deadline = Instant::now() + Duration::from_secs(5);
    let have_pidfile = poll_until(deadline, || pidfile::read_pidfile(&state_dir).is_some()).await;
    assert!(
        have_pidfile,
        "expected a live pidfile after cold-start race"
    );

    assert!(
        endpoint_answers_health(&state_dir, &endpoint).await,
        "endpoint should answer Health after single-flight bring-up"
    );

    // The pidfile records exactly one pid; that pid is alive.
    let rec = pidfile::read_pidfile(&state_dir).expect("pidfile present");
    assert!(
        pidfile::pid_alive(rec.pid),
        "pidfile pid {} should be alive",
        rec.pid
    );

    teardown(&state_dir).await;
}

/// A second daemon launched while a first is already up + has written its
/// pidfile must detect it (lock contended + live matching pidfile) and
/// exit WITHOUT rebinding the socket — so the first daemon is not
/// orphaned. This is the Unix-specific orphaning H6 fixes.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn second_daemon_does_not_orphan_first() {
    use std::os::unix::fs::MetadataExt;

    let state_dir = fresh_state_dir("orphan");
    let (endpoint, sock) = endpoint_for(&state_dir);
    let Endpoint::UnixSocket { path: sock_path } = &endpoint else {
        unreachable!()
    };

    let bin = env!("CARGO_BIN_EXE_terminal-commanderd");
    let spawn_daemon = || {
        std::process::Command::new(bin)
            .arg("--data-dir")
            .arg(&state_dir)
            .arg("start")
            .arg("--mode")
            .arg("ipc-server")
            .env("TC_SOCKET", sock_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn daemon")
    };

    // 1. First daemon: spawn and wait (polled) until it is up AND has
    //    written its pidfile.
    let mut first = spawn_daemon();
    let first_pid = first.id();
    let up_deadline = Instant::now() + Duration::from_secs(10);
    let first_up = poll_until(up_deadline, || {
        pidfile::read_pidfile(&state_dir).is_some_and(|r| r.pid == first_pid)
    })
    .await;
    assert!(
        first_up,
        "first daemon did not write its pidfile within 10s"
    );
    assert!(
        endpoint_answers_health(&state_dir, &endpoint).await,
        "first daemon should answer Health before the second is launched"
    );
    // Capture the socket inode so we can detect an orphaning rebind
    // (remove_file + bind creates a NEW inode).
    let ino_before = std::fs::metadata(&sock).unwrap().ino();

    // 2. Second daemon: it should see the lock contended + a live,
    //    endpoint-matching pidfile and exit gracefully WITHOUT rebinding.
    let mut second = spawn_daemon();
    let second_pid = second.id();

    // The second process should exit on its own (bounded poll, ~5s).
    let exit_deadline = Instant::now() + Duration::from_secs(5);
    let second_exited =
        poll_until(exit_deadline, || matches!(second.try_wait(), Ok(Some(_)))).await;

    // 3. Assertions: the first daemon is untouched.
    assert!(
        pidfile::pid_alive(first_pid),
        "first daemon (pid {first_pid}) must still be alive — it was orphaned"
    );
    let rec_after = pidfile::read_pidfile(&state_dir).expect("pidfile still present");
    assert_eq!(
        rec_after.pid, first_pid,
        "pidfile must still record the FIRST daemon, not the second"
    );
    let ino_after = std::fs::metadata(&sock).unwrap().ino();
    assert_eq!(
        ino_before, ino_after,
        "socket inode changed — the second daemon rebound and orphaned the first"
    );
    assert!(
        endpoint_answers_health(&state_dir, &endpoint).await,
        "the (first) daemon should still answer Health"
    );
    assert!(
        second_exited,
        "second daemon should have exited on its own (AlreadyServed)"
    );
    assert_ne!(
        first_pid, second_pid,
        "the two spawns share a pid (impossible)"
    );

    // Cleanup.
    let _ = first.kill();
    let _ = first.wait();
    let _ = second.kill();
    let _ = second.wait();
    teardown(&state_dir).await;
}
