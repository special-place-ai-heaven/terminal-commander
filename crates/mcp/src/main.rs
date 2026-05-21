// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commander-mcp`: thin MCP server adapter.
//!
//! Per `docs/security/PRIVILEGE_MODEL.md` section 3, this crate is
//! a thin adapter. It MUST NOT contain `Command::spawn` or open
//! network sockets. TC29 verifies these invariants.
//!
//! Source-status: scaffold-only at TC04. rmcp wiring lands in TC23;
//! tool surface in TC24.

fn main() -> std::process::ExitCode {
    eprintln!(
        "terminal-commander-mcp: scaffold only (TC04). \
         MCP server wiring lands in TC23-TC24. Refusing to start."
    );
    std::process::ExitCode::from(64)
}
