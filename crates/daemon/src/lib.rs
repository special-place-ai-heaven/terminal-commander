// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commanderd` library entry point. Exposes the local API
//! router (TC21), the policy engine (TC22), the persistent audit
//! sink (TC35), the daemon runtime bootstrap (TC36), and the local
//! UDS IPC transport (TC37) so the daemon binary, future MCP
//! adapter, admin CLI, and tests can build against the same
//! in-process / cross-process API.
//!
//! Source-status:
//! - Router + Policy + Audit: live.
//! - Daemon runtime bootstrap: live (TC36).
//! - UDS IPC transport: live (TC37) on Unix; unsupported on Windows
//!   (use WSL2). rmcp transport adapter remains deferred to TC40.

pub mod audit;
pub mod command;
pub mod config;
pub mod ipc;
pub mod policy;
pub mod router;
pub mod runtime;
pub mod state;

pub use audit::{AuditSink, InMemoryAudit, PersistentAudit};
pub use command::{
    CommandError, CommandRuntime, CommandStartRequest, CommandStartResponse, CommandStatusResponse,
    DEFAULT_COMMAND_SEVERITY_MIN, MAX_ARGV_ITEM_BYTES, MAX_ARGV_ITEMS, SHELL_INTERPRETERS_DENY,
};
pub use config::{
    ConfigError, DaemonConfig, DaemonSection, LimitsSection, PolicySection, RetentionSection,
    RuntimeMode,
};
#[cfg(unix)]
pub use ipc::{DaemonClient, IpcServer, PeerCred, ServerHandle};
pub use ipc::{
    DiscoverResponse, IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, MAX_FRAME_BYTES,
    MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, PolicyStatusResponse, RequestEnvelope, ResponseEnvelope,
    SelfCheckResponse,
};
pub use policy::{
    COMMANDS_DENY, DEFAULT_DENY_PATH_SUFFIXES, PolicyAction, PolicyDecision, PolicyEngine,
    PolicyProfile, PolicyVerdict,
};
pub use router::Router;
pub use runtime::{
    RuntimeError, SelfCheckReport, run_foreground_idle, run_ipc_server, run_self_check,
};
pub use state::{BootstrapError, DaemonState};
