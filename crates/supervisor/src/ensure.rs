// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Daemon ensure/readiness library entry point.
//
// The MCP adapter calls `ensure_daemon()` before serving rmcp. The
// return value tells the caller whether to forward tool calls, return
// `daemon_unavailable` envelopes, or fail loudly.

use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Endpoint {
    UnixSocket { path: PathBuf },
    WindowsPipe { name: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostics {
    pub endpoint: Endpoint,
    pub log_path: Option<PathBuf>,
    pub last_error: Option<String>,
    pub startup_attempted: bool,
    pub startup_elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EnsureDaemonStatus {
    AlreadyRunning { endpoint: Endpoint, pid: Option<u32> },
    Started {
        endpoint: Endpoint,
        pid: Option<u32>,
        log_path: PathBuf,
    },
    Unavailable {
        reason: DaemonUnavailableReason,
        diagnostics: Diagnostics,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonUnavailableReason {
    SpawnFailed,
    StartupTimeout,
    EndpointBindFailed,
    BinaryNotFound,
}

#[derive(Debug, Error)]
pub enum EnsureError {
    #[error("daemon binary not found at {0}")]
    BinaryNotFound(PathBuf),
}

#[derive(Debug, Clone)]
pub struct EnsureDaemonOptions {
    pub daemon_binary: PathBuf,
    pub state_dir: PathBuf,
    pub log_dir: PathBuf,
    pub endpoint: Endpoint,
    pub startup_timeout: Duration,
    pub allow_spawn: bool,
}

/// Probe the endpoint; if reachable, return `AlreadyRunning`. If not
/// and `allow_spawn` is true, spawn the daemon and wait up to
/// `startup_timeout` for the endpoint to bind. On failure, return
/// `Unavailable { reason, diagnostics }` with the log path included
/// so callers can surface it.
///
/// This function must not panic; it must always return a structured
/// status the caller can render to the operator.
pub async fn ensure_daemon(
    _opts: EnsureDaemonOptions,
) -> EnsureDaemonStatus {
    // Phase 4.1 scaffold: stubbed unavailable result; Task 5 implements
    // the live probe + spawn loop.
    EnsureDaemonStatus::Unavailable {
        reason: DaemonUnavailableReason::SpawnFailed,
        diagnostics: Diagnostics {
            endpoint: Endpoint::WindowsPipe { name: String::new() },
            log_path: None,
            last_error: Some("ensure_daemon not yet implemented".into()),
            startup_attempted: false,
            startup_elapsed_ms: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_returns_unavailable() {
        let opts = EnsureDaemonOptions {
            daemon_binary: PathBuf::from("nonexistent"),
            state_dir: PathBuf::from("."),
            log_dir: PathBuf::from("."),
            endpoint: Endpoint::WindowsPipe { name: r"\\.\pipe\unused".into() },
            startup_timeout: Duration::from_millis(10),
            allow_spawn: false,
        };
        let status = ensure_daemon(opts).await;
        assert!(matches!(status, EnsureDaemonStatus::Unavailable { .. }));
    }
}
