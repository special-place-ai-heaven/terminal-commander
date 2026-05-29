// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Platform-specific helpers (no I/O beyond what callers perform).

/// Windows only: suppress console allocation for daemon-initiated payload children.
///
/// The GUI-subsystem daemon has no parent console; console-subsystem children would
/// otherwise allocate a visible window (outward-filter leakage). This does **not**
/// apply to the JS bridge (`packages/terminal-commander/lib/wsl/spawn.js`), which
/// must remain visible for WWS04 / EDR legitimacy — see
/// `docs/release/windows-wsl-bridge-contract.md` §4.4.
#[cfg(windows)]
pub fn windows_silent(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW)
}
