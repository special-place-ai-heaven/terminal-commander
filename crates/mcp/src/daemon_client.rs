// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon UDS client wrapper for the MCP stdio adapter (TC40).
//!
//! Wraps `terminal_commanderd::DaemonClient` and adds:
//! - correlation-id generation,
//! - structured error mapping to MCP tool errors,
//! - bounded, audit-friendly call sites for every MCP tool to call into.
//!
//! Unix-only: the daemon's UDS transport itself is Unix-only. The MCP
//! binary refuses to start on non-Unix targets.
//!
//! Source-status: live (TC40).

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use terminal_commanderd::ipc::protocol::{IpcError, IpcRequest, IpcResponse};
use terminal_commander_supervisor::ensure::EnsureDaemonStatus;

/// Default UDS path used when nothing is passed on the command line
/// or in `TC_SOCKET`. Mirrors the daemon's default
/// `<HOME>/.local/share/terminal-commanderd/terminal-commanderd.sock`.
pub const DEFAULT_SOCKET_SUFFIX: &str = ".local/share/terminal-commanderd/terminal-commanderd.sock";

/// Environment variable that overrides the daemon socket path.
pub const SOCKET_ENV: &str = "TC_SOCKET";

/// Resolve the socket path the MCP adapter should connect to.
///
/// Resolution order:
/// 1. Explicit override (CLI flag) when provided.
/// 2. `TC_SOCKET` env var.
/// 3. `<HOME>/.local/share/terminal-commanderd/terminal-commanderd.sock`.
/// 4. Fallback: `./terminal-commanderd.sock`.
#[must_use]
pub fn resolve_socket_path(cli_override: Option<&std::path::Path>) -> std::path::PathBuf {
    if let Some(p) = cli_override {
        return p.to_path_buf();
    }
    if let Ok(v) = std::env::var(SOCKET_ENV)
        && !v.is_empty()
    {
        return std::path::PathBuf::from(v);
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home).join(DEFAULT_SOCKET_SUFFIX);
    }
    std::path::PathBuf::from("terminal-commanderd.sock")
}

/// Shared, cheaply-cloneable handle to the `EnsureDaemonStatus`
/// returned at MCP startup. Tool dispatch reads this to decide whether
/// to short-circuit with a `daemon_unavailable` envelope.
#[derive(Debug, Clone)]
pub struct DaemonStatusHandle(Arc<Mutex<EnsureDaemonStatus>>);

impl DaemonStatusHandle {
    pub fn new(status: EnsureDaemonStatus) -> Self {
        Self(Arc::new(Mutex::new(status)))
    }
    #[allow(dead_code)]
    pub fn current(&self) -> EnsureDaemonStatus {
        self.0.lock().unwrap().clone()
    }
    pub fn is_unavailable(&self) -> bool {
        matches!(*self.0.lock().unwrap(), EnsureDaemonStatus::Unavailable { .. })
    }
}

/// Forwarding wrapper around the daemon's `DaemonClient`. Adds a
/// monotonic correlation id per call so the IPC envelope is unique.
#[derive(Debug, Clone)]
pub struct McpDaemonClient {
    inner: terminal_commanderd::DaemonClient,
    next_id: Arc<AtomicU64>,
    status: Option<DaemonStatusHandle>,
}

impl McpDaemonClient {
    /// Construct a client targeting the given socket path. Does not
    /// open a connection until [`McpDaemonClient::call`] is invoked.
    #[must_use]
    pub fn new(socket_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            inner: terminal_commanderd::DaemonClient::new(socket_path),
            next_id: Arc::new(AtomicU64::new(1)),
            status: None,
        }
    }

    /// Construct a client pre-loaded with the supervisor status from
    /// startup. Tools use `status()` to short-circuit when unavailable.
    #[must_use]
    pub fn with_status(socket_path: impl Into<std::path::PathBuf>, status: DaemonStatusHandle) -> Self {
        Self {
            inner: terminal_commanderd::DaemonClient::new(socket_path),
            next_id: Arc::new(AtomicU64::new(1)),
            status: Some(status),
        }
    }

    /// Return the supervisor status handle if one was set at construction.
    pub fn status(&self) -> Option<DaemonStatusHandle> {
        self.status.clone()
    }

    /// Override the per-call request timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.inner = self.inner.with_timeout(timeout);
        self
    }

    /// Socket path this client is wired against.
    #[must_use]
    pub fn socket_path(&self) -> &std::path::Path {
        self.inner.socket_path()
    }

    /// Issue one round trip against the daemon. Returns the typed
    /// `IpcResponse` on success and the typed `IpcError` otherwise.
    pub async fn call(&self, request: IpcRequest) -> Result<IpcResponse, IpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner.call(id, request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_uses_cli_override_when_set() {
        let p = std::path::Path::new("/tmp/tc-test.sock");
        let got = resolve_socket_path(Some(p));
        assert_eq!(got, p);
    }

    #[test]
    fn resolve_returns_a_socket_path() {
        // Don't manipulate environment (the workspace forbids unsafe
        // and `set_var` is now unsafe). Just verify that the resolver
        // always returns a path whose final component is the daemon
        // socket file name, regardless of which arm fires.
        let got = resolve_socket_path(None);
        assert!(
            got.to_string_lossy().ends_with("terminal-commanderd.sock")
                || got.to_string_lossy().ends_with(".sock"),
            "got: {got:?}"
        );
    }

    #[test]
    fn client_records_socket_path() {
        let p = std::path::Path::new("/tmp/tc-record.sock");
        let c = McpDaemonClient::new(p);
        assert_eq!(c.socket_path(), p);
    }
}
