// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commanderd` library entry point. Exposes the local API
//! router (TC21), the policy engine (TC22), and the persistent audit
//! sink (TC35) so the daemon binary, MCP server (TC23), admin CLI
//! (TC25), and tests can all build against the same in-process API
//! before the UDS / JSON-RPC transport lands.
//!
//! Source-status: live for the in-process API and the audit sink
//! (TC21 + TC22 + TC35). UDS transport is TC37.

pub mod audit;
pub mod policy;
pub mod router;

pub use audit::{AuditSink, InMemoryAudit, PersistentAudit};
pub use policy::{
    COMMANDS_DENY, DEFAULT_DENY_PATH_SUFFIXES, PolicyAction, PolicyDecision, PolicyEngine,
    PolicyProfile, PolicyVerdict,
};
pub use router::Router;
