// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! The CLI's single live-IPC call site (P2).
//!
//! Every daemon-backed subcommand routes through [`connect_or_unavailable`]:
//! it resolves the endpoint exactly as the daemon binds it (via the shared
//! supervisor resolver), runs the existing probe-before-IPC health handshake
//! through [`ensure_daemon`] with `allow_spawn: false`, and ONLY on
//! `AlreadyRunning` / `Started` constructs a [`DaemonClient`] and issues one
//! `call(id, req)`.
//!
//! Honesty contract (campaign invariant): a subcommand gets REAL daemon data,
//! or a typed [`CliIpcError`] mapped to a non-zero exit. The CLI NEVER
//! fabricates empty/not-found data. The exit-69 "unavailable" path is only
//! reachable AFTER a real probe fails — never as a synthetic success.
//!
//! Platform-agnostic by construction: every transport detail lives inside
//! `terminal_commander_ipc::DaemonClient` (UDS on Unix, named pipe on
//! Windows), so this module names neither.

// The module is `pub(crate)`, so `pub(crate)` on its items is redundant from
// clippy's view; the explicit visibility documents the crate-internal contract.
// Matches the convention in `update_locks.rs`.
#![allow(clippy::redundant_pub_crate)]

use std::time::Duration;

use terminal_commander_ipc::{DaemonClient, IpcError, IpcRequest, IpcResponse};
use terminal_commander_supervisor::ensure::{
    DaemonUnavailableReason, Diagnostics, EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon,
};
use terminal_commander_supervisor::paths::{
    endpoint_from_socket_path, resolve_socket_path, resolve_state_dir,
};

/// Exit code surfaced when a daemon-backed command cannot reach a live daemon.
/// Matches `EX_UNAVAILABLE` (sysexits.h 69) used elsewhere in the CLI.
/// Consumed by [`CliIpcError::exit_code`] on the `Unavailable` path.
pub(crate) const EX_UNAVAILABLE: u8 = 69;

/// Probe budget for the pre-IPC health handshake. The CLI is a one-shot
/// command: it does not spawn the daemon (`allow_spawn: false`), so this only
/// bounds the probe of an ALREADY-running daemon, not a cold start.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Default per-call request timeout for single-shot daemon-backed subcommands.
/// Matches `DaemonClient`'s built-in default; named here so the shared
/// [`connect_or_unavailable_with_timeout`] body sets it explicitly while
/// [`connect_or_unavailable`] preserves the historical 5 s behavior.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// A daemon-backed CLI call failed for one of two HONEST reasons. There is no
/// "synthesized empty" arm by design.
#[derive(Debug)]
pub(crate) enum CliIpcError {
    /// The probe-before-IPC handshake did not find a live daemon. Carries the
    /// supervisor's structured diagnostics so the operator sees WHY. Maps to
    /// exit [`EX_UNAVAILABLE`] (69). Reached only after a real probe failure.
    Unavailable {
        reason: DaemonUnavailableReason,
        diagnostics: Box<Diagnostics>,
    },
    /// The daemon answered with a typed IPC error (policy denied, not found,
    /// internal, etc.). Maps to a non-zero exit (1) and is rendered verbatim.
    Ipc(IpcError),
}

impl CliIpcError {
    /// The non-zero process exit code this error maps to.
    pub(crate) const fn exit_code(&self) -> u8 {
        match self {
            Self::Unavailable { .. } => EX_UNAVAILABLE,
            Self::Ipc(_) => 1,
        }
    }

    /// Render the operator-facing diagnostic line(s) for this error to stderr.
    /// `command` is the human label (e.g. `"rules list"`).
    pub(crate) fn report(&self, command: &str) {
        match self {
            Self::Unavailable {
                reason,
                diagnostics,
            } => {
                eprintln!(
                    "terminal-commander: {command} unavailable: requires live daemon IPC; \
                     refusing to synthesize empty or not-found data."
                );
                eprintln!("  reason   : {reason:?}");
                if let Some(err) = &diagnostics.last_error {
                    eprintln!("  detail   : {err}");
                }
            }
            Self::Ipc(err) => {
                eprintln!(
                    "terminal-commander: {command} failed: {:?}: {}",
                    err.code, err.message
                );
            }
        }
    }
}

/// Resolve the endpoint, run the probe-before-IPC handshake (`allow_spawn:
/// false`), and on a reachable daemon issue exactly ONE `call(id, req)` with the
/// 5 s default `DaemonClient` timeout.
///
/// Returns `Ok(IpcResponse)` with REAL daemon data, or a typed [`CliIpcError`]:
/// `Unavailable` (exit 69) only after the probe fails, `Ipc` (exit 1) when the
/// daemon itself answered with an error. NEVER fabricates a response.
///
/// `correlation_id` is the per-call id echoed back by the daemon; the CLI is
/// single-shot so any stable value (e.g. 1) is fine.
pub(crate) async fn connect_or_unavailable(
    correlation_id: u64,
    request: IpcRequest,
) -> Result<IpcResponse, CliIpcError> {
    connect_or_unavailable_with_timeout(correlation_id, request, DEFAULT_REQUEST_TIMEOUT).await
}

/// Like [`connect_or_unavailable`] but with a caller-chosen per-call timeout.
///
/// Used by `subscription-stream`, whose blocking pulls need a client timeout
/// ABOVE the server's `MAX_PULL_TIMEOUT_MS` (8 s) so an idle ~8 s server pull
/// returns SUCCESS, not a premature client timeout. Shares the SINGLE
/// probe-before-IPC path: it differs only in the constructed `DaemonClient`'s
/// `.with_timeout(timeout)`.
pub(crate) async fn connect_or_unavailable_with_timeout(
    correlation_id: u64,
    request: IpcRequest,
    timeout: Duration,
) -> Result<IpcResponse, CliIpcError> {
    let state_dir = resolve_state_dir();
    let endpoint_path = resolve_socket_path();
    let endpoint = endpoint_from_socket_path(&endpoint_path);

    // Probe-before-IPC: reach the daemon only via the existing health
    // handshake first. `allow_spawn: false` means we NEVER cold-start the
    // daemon from a read-only inspection command; an unreachable endpoint is
    // surfaced as Unavailable, not silently spawned.
    let opts = EnsureDaemonOptions {
        // daemon_binary is unused when allow_spawn is false, but the struct
        // requires it; the bare name never resolves because we never spawn.
        daemon_binary: std::path::PathBuf::from("terminal-commanderd"),
        state_dir: state_dir.clone(),
        log_dir: state_dir.join("logs"),
        endpoint: endpoint.clone(),
        startup_timeout: PROBE_TIMEOUT,
        allow_spawn: false,
    };

    match ensure_daemon(opts).await {
        // Reachable: construct the transport ONLY now and issue one call.
        EnsureDaemonStatus::AlreadyRunning { endpoint, .. }
        | EnsureDaemonStatus::Started { endpoint, .. } => {
            let client = DaemonClient::new(endpoint_string(&endpoint)).with_timeout(timeout);
            client
                .call(correlation_id, request)
                .await
                .map_err(CliIpcError::Ipc)
        }
        // The probe failed: honest unavailable, never synthesized data.
        EnsureDaemonStatus::Unavailable {
            reason,
            diagnostics,
        } => Err(CliIpcError::Unavailable {
            reason,
            diagnostics: Box::new(diagnostics),
        }),
    }
}

/// Recover the endpoint path/pipe string `DaemonClient::new` wants from the
/// supervisor's `Endpoint` enum. UDS carries a path; the Windows pipe carries
/// its `\\.\pipe\...` name. Platform-agnostic: both arms compile everywhere
/// (the enum is not cfg-gated) and `DaemonClient::new` takes the same string.
fn endpoint_string(endpoint: &terminal_commander_supervisor::ensure::Endpoint) -> String {
    match endpoint {
        terminal_commander_supervisor::ensure::Endpoint::UnixSocket { path } => {
            path.to_string_lossy().into_owned()
        }
        terminal_commander_supervisor::ensure::Endpoint::WindowsPipe { name } => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_ipc::IpcErrorCode;

    #[test]
    fn unavailable_maps_to_exit_69() {
        let err = CliIpcError::Unavailable {
            reason: DaemonUnavailableReason::EndpointBindFailed,
            diagnostics: Box::new(Diagnostics {
                endpoint: terminal_commander_supervisor::ensure::Endpoint::WindowsPipe {
                    name: r"\\.\pipe\unused".into(),
                },
                log_path: None,
                last_error: Some("endpoint unreachable; spawn disabled".into()),
                startup_attempted: false,
                startup_elapsed_ms: 0,
            }),
        };
        assert_eq!(err.exit_code(), EX_UNAVAILABLE);
    }

    #[test]
    fn ipc_error_maps_to_exit_1() {
        let err = CliIpcError::Ipc(IpcError::new(IpcErrorCode::Internal, "boom"));
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn endpoint_string_roundtrips_both_variants() {
        use terminal_commander_supervisor::ensure::Endpoint;
        assert_eq!(
            endpoint_string(&Endpoint::UnixSocket {
                path: std::path::PathBuf::from("/tmp/x.sock"),
            }),
            "/tmp/x.sock"
        );
        assert_eq!(
            endpoint_string(&Endpoint::WindowsPipe {
                name: r"\\.\pipe\terminal-commander-x".into(),
            }),
            r"\\.\pipe\terminal-commander-x"
        );
    }
}
