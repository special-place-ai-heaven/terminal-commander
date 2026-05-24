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
    opts: EnsureDaemonOptions,
) -> EnsureDaemonStatus {
    let start = std::time::Instant::now();

    // 1. Probe endpoint first.
    if probe_endpoint(&opts.endpoint).await {
        return EnsureDaemonStatus::AlreadyRunning {
            endpoint: opts.endpoint,
            pid: None,
        };
    }

    if !opts.allow_spawn {
        return EnsureDaemonStatus::Unavailable {
            reason: DaemonUnavailableReason::EndpointBindFailed,
            diagnostics: Diagnostics {
                endpoint: opts.endpoint,
                log_path: None,
                last_error: Some("endpoint unreachable; spawn disabled".into()),
                startup_attempted: false,
                startup_elapsed_ms: start.elapsed().as_millis() as u64,
            },
        };
    }

    // 2. Spawn daemon (binary path required to exist).
    //
    // Note: this branch uses blocking std::fs and std::process::Command
    // inside an async fn. Under tokio's multi-threaded runtime this
    // starves a single worker thread per call, not the whole runtime.
    // Spawn is rare and fast on Windows/Linux so the tradeoff is
    // acceptable for Phase 3. If diagnostics fidelity ever requires
    // capturing per-syscall latency or this is called from a hot
    // path, wrap the blocking section in `tokio::task::spawn_blocking`.
    if !opts.daemon_binary.exists() {
        return EnsureDaemonStatus::Unavailable {
            reason: DaemonUnavailableReason::BinaryNotFound,
            diagnostics: Diagnostics {
                endpoint: opts.endpoint,
                log_path: None,
                last_error: Some(format!(
                    "daemon binary not found: {}",
                    opts.daemon_binary.display()
                )),
                startup_attempted: false,
                startup_elapsed_ms: start.elapsed().as_millis() as u64,
            },
        };
    }
    let _ = std::fs::create_dir_all(&opts.log_dir);
    let log_path = opts.log_dir.join("terminal-commanderd.log");
    let log_file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(e) => {
            return EnsureDaemonStatus::Unavailable {
                reason: DaemonUnavailableReason::SpawnFailed,
                diagnostics: Diagnostics {
                    endpoint: opts.endpoint,
                    log_path: Some(log_path),
                    last_error: Some(format!("open log: {e}")),
                    startup_attempted: false,
                    startup_elapsed_ms: start.elapsed().as_millis() as u64,
                },
            };
        }
    };
    let log_file_err = match log_file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            return EnsureDaemonStatus::Unavailable {
                reason: DaemonUnavailableReason::SpawnFailed,
                diagnostics: Diagnostics {
                    endpoint: opts.endpoint,
                    log_path: Some(log_path),
                    last_error: Some(format!("clone log fd: {e}")),
                    startup_attempted: false,
                    startup_elapsed_ms: start.elapsed().as_millis() as u64,
                },
            };
        }
    };
    let mut cmd = std::process::Command::new(&opts.daemon_binary);
    cmd.arg("--data-dir")
        .arg(&opts.state_dir)
        .arg("start")
        .arg("--mode")
        .arg("ipc-server")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(log_file_err));
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return EnsureDaemonStatus::Unavailable {
                reason: DaemonUnavailableReason::SpawnFailed,
                diagnostics: Diagnostics {
                    endpoint: opts.endpoint,
                    log_path: Some(log_path),
                    last_error: Some(format!("spawn: {e}")),
                    startup_attempted: true,
                    startup_elapsed_ms: start.elapsed().as_millis() as u64,
                },
            };
        }
    };
    let pid = Some(child.id());
    // `child` is dropped at the end of this function. On both Unix
    // and Windows, dropping std::process::Child does NOT terminate
    // the underlying process — it only releases the handle. That is
    // the intended daemon semantics here: the spawned terminal-
    // commanderd outlives the supervisor call.
    drop(child);

    // 3. Wait for endpoint bind up to startup_timeout.
    let deadline = std::time::Instant::now() + opts.startup_timeout;
    while std::time::Instant::now() < deadline {
        if probe_endpoint(&opts.endpoint).await {
            return EnsureDaemonStatus::Started {
                endpoint: opts.endpoint,
                pid,
                log_path,
            };
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    EnsureDaemonStatus::Unavailable {
        reason: DaemonUnavailableReason::StartupTimeout,
        diagnostics: Diagnostics {
            endpoint: opts.endpoint,
            log_path: Some(log_path),
            last_error: Some(format!(
                "endpoint did not bind within {}ms",
                opts.startup_timeout.as_millis()
            )),
            startup_attempted: true,
            startup_elapsed_ms: start.elapsed().as_millis() as u64,
        },
    }
}

async fn probe_endpoint(endpoint: &Endpoint) -> bool {
    match endpoint {
        #[cfg(unix)]
        Endpoint::UnixSocket { path } => {
            tokio::net::UnixStream::connect(path).await.is_ok()
        }
        #[cfg(not(unix))]
        Endpoint::UnixSocket { .. } => false,
        #[cfg(windows)]
        Endpoint::WindowsPipe { name } => {
            // ClientOptions::new().open is synchronous; same tokio
            // contract caveat as the blocking I/O in ensure_daemon
            // step 2 (acceptable for Phase 3, revisit if probed in a
            // hot path).
            use tokio::net::windows::named_pipe::ClientOptions;
            ClientOptions::new().open(name.as_str()).is_ok()
        }
        #[cfg(not(windows))]
        Endpoint::WindowsPipe { .. } => false,
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
