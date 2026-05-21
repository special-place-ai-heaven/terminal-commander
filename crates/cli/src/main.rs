// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commander`: operator CLI.
//!
//! Source-status: scaffold-only at TC04. Subcommands (status, doctor,
//! rules, buckets, jobs, probes, policy, audit) land in TC25.

fn main() -> std::process::ExitCode {
    eprintln!(
        "terminal-commander: scaffold only (TC04). \
         CLI subcommands land in TC25. Refusing to run."
    );
    std::process::ExitCode::from(64)
}
