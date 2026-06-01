// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

use std::process::Command;

const UNAVAILABLE_EXIT: i32 = 69;

fn terminal_commander() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_terminal-commander"));
    // M8: isolate the child from any ambient endpoint. Without this, a dev (or
    // CI agent) with a live daemon reachable via TC_SOCKET/TC_SESSION/TC_DATA
    // flips the "unavailable" (exit 69) assertions below. Force an endpoint that
    // can never resolve to a running daemon.
    cmd.env_remove("TC_SESSION")
        .env_remove("TC_DATA")
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .env(
            "TC_SOCKET",
            if cfg!(windows) {
                r"\\.\pipe\terminal-commander-offline-truth-test-no-daemon"
            } else {
                "/nonexistent/terminal-commander-offline-truth-test.sock"
            },
        );
    cmd
}

#[test]
fn daemon_backed_inspection_commands_do_not_fake_empty_success() {
    // Post-wiring regression guard (P5): every subcommand below is now a LIVE
    // daemon-backed read. `terminal_commander()` points `TC_SOCKET` at a path
    // that can never resolve to a running daemon, so each command runs the real
    // probe-before-IPC handshake, the probe FAILS, and it takes the genuine
    // `CliIpcError::Unavailable` branch -> exit 69, empty stdout, "unavailable"
    // on stderr. This is the proof that wiring the CLI to real IPC preserved the
    // no-fake-success axis: a dead endpoint REFUSES to synthesize data rather
    // than fabricating an empty/not-found result.
    //
    // `buckets show` uses a WELL-FORMED wire id (`bkt_<32-hex>`) so the command
    // reaches the probe-before-IPC path and returns the offline/unavailable
    // contract (exit 69). A malformed id is a usage error (exit 2) handled
    // before any IPC, which is a different, honest failure not under test here.
    for args in [
        &["rules", "list"][..],
        &["rules", "show", "missing.rule"],
        &["buckets", "list"],
        &["buckets", "show", "bkt_00000000000000000000000000000000"],
        &["jobs"],
        &["probes"],
        &["policy"],
        &["audit"],
    ] {
        let output = terminal_commander()
            .args(args)
            .output()
            .expect("terminal-commander test binary should run");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert_eq!(
            output.status.code(),
            Some(UNAVAILABLE_EXIT),
            "{args:?} must fail unavailable, stdout={stdout:?}, stderr={stderr:?}",
        );
        assert!(
            stdout.trim().is_empty(),
            "{args:?} must not emit fake empty data on stdout: {stdout:?}",
        );
        assert!(
            stderr.contains("unavailable"),
            "{args:?} must explain unavailable status on stderr: {stderr:?}",
        );
        assert!(
            !stderr.contains("empty (offline CLI") && !stderr.contains(": 0 (offline CLI"),
            "{args:?} must not preserve fake offline success language: {stderr:?}",
        );
    }
}
