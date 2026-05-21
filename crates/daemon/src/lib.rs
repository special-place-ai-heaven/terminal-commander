// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commanderd` library entry point. Exposes the local API
//! router (TC21) so the daemon binary, MCP server (TC23), admin CLI
//! (TC25), and tests can all build against the same in-process API
//! before the UDS / JSON-RPC transport lands.
//!
//! Source-status: live (TC21).

pub mod router;

pub use router::{AuditPlaceholder, AuditRecord, Router};
