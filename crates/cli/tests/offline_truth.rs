// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

use std::process::Command;

const UNAVAILABLE_EXIT: i32 = 69;

fn terminal_commander() -> Command {
    Command::new(env!("CARGO_BIN_EXE_terminal-commander"))
}

#[test]
fn daemon_backed_inspection_commands_do_not_fake_empty_success() {
    for args in [
        &["rules", "list"][..],
        &["rules", "show", "missing.rule"],
        &["buckets", "list"],
        &["buckets", "show", "bucket-123"],
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
