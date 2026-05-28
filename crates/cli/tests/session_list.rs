//! `session list` enumerates the default + seeded sessions over a fabricated
//! base state dir (pointed at via TC_DATA), proving the filesystem-as-registry
//! contract on the CLI side. Pure stdout assertions — no real daemon.

use std::process::Command;
use terminal_commander_supervisor::pidfile::{write_pidfile, RunningDaemon};

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-cli-sess-list-{tag}-{pid}-{nanos}-{n}"));
    p
}

#[test]
fn session_list_shows_default_and_seeded_with_state_columns() {
    let base = tmp_dir("default-and-seeded");
    write_pidfile(
        &base,
        &RunningDaemon {
            pid: 999_999_999,
            version: "0.1.0".into(),
            endpoint: "base.sock".into(),
        },
    )
    .unwrap();
    write_pidfile(
        &base.join("agent-1"),
        &RunningDaemon {
            pid: 999_999_998,
            version: "0.1.1".into(),
            endpoint: "agent.sock".into(),
        },
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_terminal-commander"))
        .args(["session", "list"])
        .env("TC_DATA", &base)
        .env_remove("TC_SOCKET")
        .env_remove("TC_SESSION")
        .output()
        .expect("run terminal-commander session list");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "session list must exit 0; stderr={stderr}, stdout={stdout}"
    );
    assert!(stdout.contains("SESSION"), "header row missing: {stdout}");
    assert!(
        stdout.contains("default"),
        "must list the default session: {stdout}"
    );
    assert!(
        stdout.contains("agent-1"),
        "must list the seeded session: {stdout}"
    );
    assert!(
        stdout.contains("stale"),
        "dead-pid entries must show state=stale: {stdout}"
    );
    let _ = std::fs::remove_dir_all(&base);
}
