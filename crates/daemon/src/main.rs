// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commanderd`: the Terminal Commander daemon.
//!
//! Source-status: scaffold-only at TC04. This entry point exits
//! immediately with a clear "not implemented" message so the binary
//! cannot be mistaken for live behavior. Concrete daemon wiring
//! lands in TC15+ (probes), TC16 (jobs), TC17 (bucket waiter),
//! TC21 (local API), TC22 (policy + audit), TC23/TC24 (MCP).

fn main() -> std::process::ExitCode {
    eprintln!(
        "terminal-commanderd: scaffold only (TC04). \
         Daemon wiring lands in TC15-TC23. Refusing to start."
    );
    std::process::ExitCode::from(64) // EX_USAGE: "not implemented"
}
