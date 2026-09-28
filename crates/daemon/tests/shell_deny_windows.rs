// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! FCR-001: Windows argv-lane spawn attempt must be denied before CreateProcess.
//!
//! The guard is the shared `shell_argv_denied` predicate. This file does not
//! run on Linux CI; `scripts/windows-gate.ps1` executes it on Windows.

#![cfg(windows)]

use std::path::PathBuf;

use terminal_commanderd::{CommandError, CommandStartRequest, DaemonConfig, DaemonState};

fn tmp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!(
        "tc-shell-deny-win-{tag}-{}-{nanos}",
        std::process::id()
    ));
    p
}

#[test]
fn windows_argv_lane_denies_shell_before_spawn() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("fcr001");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        assert!(!state.policy.caps_allow_shell());

        let cases: &[(&[&str], &str)] = &[
            (&["bash.exe", "-c", "whoami"], "bash"),
            (&["sh.exe", "-c", "id"], "sh"),
            (&["CMD", "/c", "dir"], "cmd"),
            (&["PowerShell", "-EncodedCommand", "QQ=="], "powershell"),
            (&["env", "bash", "-ec", "id"], "bash"),
            (&["env", "cmd.exe", "/k", "dir"], "cmd.exe"),
        ];
        for (argv, expected) in cases {
            let jobs_before = state.jobs.list().len();
            let err = state
                .command
                .start_combed(CommandStartRequest {
                    argv: argv.iter().map(|arg| (*arg).to_owned()).collect(),
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace: None,
                    tag: None,
                    dedup_nonce: None,
                    strip_ansi: true,
                    peer_discriminator: None,
                })
                .unwrap_err();
            match err {
                CommandError::ShellInterpreterDenied(ref shell) => {
                    assert_eq!(shell, expected, "argv={argv:?}");
                }
                other => panic!("argv={argv:?} expected ShellInterpreterDenied, got {other:?}"),
            }
            assert_eq!(
                state.jobs.list().len(),
                jobs_before,
                "argv={argv:?} must not spawn"
            );
        }
        let _ = std::fs::remove_dir_all(&data);
    });
}
