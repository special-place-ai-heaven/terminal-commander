// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC-5: the hidden `selfcheck-noop` leaf must exist and exit 0.
//!
//! This is the spawn target the IPC `self_check` probe launches. If a future
//! clap refactor renames/drops the subcommand or its `fn main` short-circuit
//! regresses (e.g. it starts running `resolve_config` and exits nonzero), the
//! self-check would silently turn RED -- this test catches that at the binary
//! boundary. Cross-platform: just spawns the bin and asserts a clean exit.

use std::process::Command;

#[test]
fn selfcheck_noop_subcommand_exits_zero() {
    let exe = env!("CARGO_BIN_EXE_terminal-commanderd");
    let status = Command::new(exe)
        .arg("selfcheck-noop")
        .status()
        .expect("spawn selfcheck-noop");
    assert!(
        status.success(),
        "the hidden selfcheck-noop leaf must exit 0; got {status:?}"
    );
}
