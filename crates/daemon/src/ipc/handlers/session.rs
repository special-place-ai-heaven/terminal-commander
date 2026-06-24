// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! IPC handlers for the persistent shell-session surface (P1 / TC50).
//!
//! `shell_session_start` / `_exec` / `_status` / `_stop` / `_list` and the
//! workspace snapshot create/apply. The session runtime
//! ([`ShellSessionRuntime`](crate::shell_session)) owns the session map,
//! the `max_sessions` cap, and the terminal-state guard; the PTY runtime
//! it builds on performs the `SessionStart` policy gate + the
//! `shell_session_start` audit row BEFORE spawn.
//!
//! Unix-only: sessions are PTY-backed. On non-unix the dispatcher returns
//! `UnsupportedPlatform` via the stub handlers below.

use std::sync::Arc;

use crate::ipc::protocol::{IpcError, IpcResponse};
// `IpcErrorCode` is referenced only by the unix implementation now: the
// non-unix stubs build their `UnsupportedPlatform` error via the typed
// `IpcError::unsupported_platform` constructor (which sets the code), so a
// Windows build that excludes the unix block would otherwise see the import
// as unused. Gate it with the unix block it serves.
#[cfg(unix)]
use crate::ipc::protocol::IpcErrorCode;
#[cfg(unix)]
use crate::ipc::protocol::{
    ShellSessionExecParams, ShellSessionStartParams, ShellSessionStatusParams,
    ShellSessionStopParams, WorkspaceSnapshotApplyParams, WorkspaceSnapshotCreateParams,
};
use crate::state::DaemonState;

#[cfg(not(unix))]
pub(in crate::ipc::server) fn session_ipc_unsupported(tool: &str) -> IpcError {
    IpcError::unsupported_platform(
        tool,
        "persistent shell sessions are not available on this platform (unix-only; Windows session support is a separate slice)",
    )
}

#[cfg(not(unix))]
pub(in crate::ipc::server) fn handle_shell_session_start(
    _state: &Arc<DaemonState>,
    _params: &crate::ipc::protocol::ShellSessionStartParams,
) -> Result<IpcResponse, IpcError> {
    Err(session_ipc_unsupported("shell_session_start"))
}

#[cfg(not(unix))]
#[allow(clippy::unused_async)]
pub(in crate::ipc::server) async fn handle_shell_session_exec(
    _state: &Arc<DaemonState>,
    _params: &crate::ipc::protocol::ShellSessionExecParams,
) -> Result<IpcResponse, IpcError> {
    Err(session_ipc_unsupported("shell_session_exec"))
}

#[cfg(not(unix))]
pub(in crate::ipc::server) fn handle_shell_session_status(
    _state: &Arc<DaemonState>,
    _params: &crate::ipc::protocol::ShellSessionStatusParams,
) -> Result<IpcResponse, IpcError> {
    Err(session_ipc_unsupported("shell_session_status"))
}

#[cfg(not(unix))]
pub(in crate::ipc::server) fn handle_shell_session_stop(
    _state: &Arc<DaemonState>,
    _params: &crate::ipc::protocol::ShellSessionStopParams,
) -> Result<IpcResponse, IpcError> {
    Err(session_ipc_unsupported("shell_session_stop"))
}

#[cfg(not(unix))]
pub(in crate::ipc::server) fn handle_shell_session_list(
    _state: &Arc<DaemonState>,
) -> Result<IpcResponse, IpcError> {
    Err(session_ipc_unsupported("shell_session_list"))
}

#[cfg(not(unix))]
pub(in crate::ipc::server) fn handle_workspace_snapshot_create(
    _state: &Arc<DaemonState>,
    _params: &crate::ipc::protocol::WorkspaceSnapshotCreateParams,
) -> Result<IpcResponse, IpcError> {
    Err(session_ipc_unsupported("workspace_snapshot_create"))
}

#[cfg(not(unix))]
#[allow(clippy::unused_async)]
pub(in crate::ipc::server) async fn handle_workspace_snapshot_apply(
    _state: &Arc<DaemonState>,
    _params: &crate::ipc::protocol::WorkspaceSnapshotApplyParams,
) -> Result<IpcResponse, IpcError> {
    Err(session_ipc_unsupported("workspace_snapshot_apply"))
}

// ---------------------------------------------------------------------
// Unix implementation.
// ---------------------------------------------------------------------

#[cfg(unix)]
use crate::ipc::protocol::{
    DEFAULT_BUCKET_READ_LIMIT, MAX_BUCKET_WAIT_MS, ShellSessionExecResponse, ShellSessionListEntry,
    ShellSessionListResponse, ShellSessionStartResponse, ShellSessionStatusResponse,
    ShellSessionStopResponse, WorkspaceSnapshotApplyResponse, WorkspaceSnapshotCreateResponse,
};
#[cfg(unix)]
use crate::shell_session::{SessionError, SessionStartRequest};
#[cfg(unix)]
use terminal_commander_supervisor::identity::PeerIdentity;

/// Default settle window (ms) `shell_session_exec` waits for combed
/// signals after writing the line, when the caller does not set `wait_ms`.
#[cfg(unix)]
const DEFAULT_SESSION_EXEC_WAIT_MS: u64 = 500;

#[cfg(unix)]
fn map_session_error(e: &SessionError) -> IpcError {
    match e {
        SessionError::LimitReached { .. } => {
            IpcError::new(IpcErrorCode::SessionLimitExceeded, e.to_string())
        }
        SessionError::Pty(crate::pty_command::PtyRuntimeError::PolicyDenied(reason)) => {
            IpcError::new(IpcErrorCode::PolicyDenied, reason.clone())
        }
        SessionError::Pty(crate::pty_command::PtyRuntimeError::Sifter(reason)) => {
            IpcError::new(IpcErrorCode::RuleInvalid, reason.clone())
        }
        SessionError::UnknownSession(_) => {
            IpcError::new(IpcErrorCode::UnknownSession, e.to_string())
        }
        SessionError::NotLive(_) => IpcError::new(IpcErrorCode::SessionNotLive, e.to_string()),
        SessionError::OversizedLine => IpcError::new(IpcErrorCode::OversizedRequest, e.to_string()),
        SessionError::SecretInputDenied => {
            IpcError::new(IpcErrorCode::SecretInputDenied, e.to_string())
        }
        SessionError::Pty(other) => {
            IpcError::new(IpcErrorCode::Internal, format!("shell_session: {other}"))
        }
    }
}

#[cfg(unix)]
pub(in crate::ipc::server) fn handle_shell_session_start(
    state: &Arc<DaemonState>,
    params: &ShellSessionStartParams,
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    if params.env.len() > terminal_commander_ipc::MAX_SESSION_ENV_ITEMS {
        return Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            "env exceeds bounded session item cap",
        ));
    }
    let subject = super::common::identity_audit_subject(peer);
    let req = SessionStartRequest {
        shell: params.shell.clone(),
        cwd: params.cwd.clone(),
        env: params.env.clone(),
        rules: params.rules.clone(),
        bucket_config: params.bucket_config.clone(),
        tag: params.tag.clone(),
    };
    match state.sessions.start(req, &subject) {
        Ok(out) => Ok(IpcResponse::ShellSessionStart(ShellSessionStartResponse {
            session_id: out.session_id,
            bucket_id: out.bucket_id,
            state: out.state,
        })),
        Err(e) => Err(map_session_error(&e)),
    }
}

#[cfg(unix)]
pub(in crate::ipc::server) async fn handle_shell_session_exec(
    state: &Arc<DaemonState>,
    params: &ShellSessionExecParams,
) -> Result<IpcResponse, IpcError> {
    use terminal_commander_core::BucketWaitRequest;

    // Write the line (terminal-state + secret guards apply inside).
    let (bucket_id, _ts) = state
        .sessions
        .exec(params.session_id, &params.line)
        .await
        .map_err(|e| map_session_error(&e))?;

    // Read back the combed signals the line produced. Bounded wait via
    // the router; never a raw stream. The cursor is the caller's read
    // position into the session bucket.
    let wait_ms = params
        .wait_ms
        .unwrap_or(DEFAULT_SESSION_EXEC_WAIT_MS)
        .min(MAX_BUCKET_WAIT_MS);
    let req = BucketWaitRequest {
        cursor: params.cursor,
        severity_min: None,
        kind_filter: None,
        limit: Some(DEFAULT_BUCKET_READ_LIMIT),
        timeout: std::time::Duration::from_millis(wait_ms),
    };
    let resp = state
        .router
        .bucket_wait(bucket_id, req)
        .await
        .map_err(super::common::map_bucket_error)?;

    Ok(IpcResponse::ShellSessionExec(ShellSessionExecResponse {
        session_id: params.session_id,
        bucket_id,
        // The probe owns the real byte count; the session lane reports the
        // line + newline it submitted (bounded metadata, not a raw echo).
        bytes_written: (params.line.len() as u64) + 1,
        cursor_in: resp.cursor_in,
        next_cursor: resp.next_cursor,
        has_more: !resp.heartbeat && resp.events.len() >= DEFAULT_BUCKET_READ_LIMIT,
        dropped_count: resp.dropped_count,
        events: resp.events,
    }))
}

#[cfg(unix)]
pub(in crate::ipc::server) fn handle_shell_session_status(
    state: &Arc<DaemonState>,
    params: &ShellSessionStatusParams,
) -> Result<IpcResponse, IpcError> {
    match state.sessions.status(params.session_id) {
        Ok(s) => Ok(IpcResponse::ShellSessionStatus(
            ShellSessionStatusResponse {
                session_id: s.session_id,
                bucket_id: s.bucket_id,
                state: s.state,
                cwd: s.cwd,
                env_snapshot: s.env_snapshot,
                last_active_at: s.last_active_at,
            },
        )),
        Err(e) => Err(map_session_error(&e)),
    }
}

#[cfg(unix)]
// stop is idempotent (never errors), but the signature stays `Result` to
// match the `#[cfg(not(unix))]` stub that returns `UnsupportedPlatform` so
// the dispatcher arm is cfg-uniform.
#[allow(clippy::unnecessary_wraps)]
pub(in crate::ipc::server) fn handle_shell_session_stop(
    state: &Arc<DaemonState>,
    params: &ShellSessionStopParams,
) -> Result<IpcResponse, IpcError> {
    let (session_state, terminal_reason) = state.sessions.stop(params.session_id);
    Ok(IpcResponse::ShellSessionStop(ShellSessionStopResponse {
        session_id: params.session_id,
        state: session_state,
        terminal_reason,
    }))
}

#[cfg(unix)]
// list never errors, but the signature stays `Result` to match the
// `#[cfg(not(unix))]` stub (UnsupportedPlatform) so the dispatcher arm is
// cfg-uniform.
#[allow(clippy::unnecessary_wraps)]
pub(in crate::ipc::server) fn handle_shell_session_list(
    state: &Arc<DaemonState>,
) -> Result<IpcResponse, IpcError> {
    let sessions: Vec<ShellSessionListEntry> = state
        .sessions
        .list()
        .into_iter()
        .map(|e| ShellSessionListEntry {
            session_id: e.session_id,
            bucket_id: e.bucket_id,
            state: e.state,
            cwd: e.cwd,
            last_active_at: e.last_active_at,
        })
        .collect();
    Ok(IpcResponse::ShellSessionList(ShellSessionListResponse {
        sessions,
    }))
}

#[cfg(unix)]
pub(in crate::ipc::server) fn handle_workspace_snapshot_create(
    state: &Arc<DaemonState>,
    params: &WorkspaceSnapshotCreateParams,
) -> Result<IpcResponse, IpcError> {
    let Some((cwd, env)) = state.sessions.workspace_of(params.session_id) else {
        return Err(IpcError::new(
            IpcErrorCode::UnknownSession,
            format!(
                "shell session '{}' is not known",
                params.session_id.to_wire_string()
            ),
        ));
    };
    let snapshot_id = format!("snap_{}", uuid::Uuid::new_v4().simple());
    let session_wire = params.session_id.to_wire_string();
    match state.store.create_workspace_snapshot(
        &snapshot_id,
        params.name.as_deref(),
        Some(&session_wire),
        cwd.as_deref(),
        &env,
    ) {
        Ok(id) => Ok(IpcResponse::WorkspaceSnapshotCreate(
            WorkspaceSnapshotCreateResponse { snapshot_id: id },
        )),
        Err(e) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("workspace_snapshot_create: {e}"),
        )),
    }
}

#[cfg(unix)]
pub(in crate::ipc::server) async fn handle_workspace_snapshot_apply(
    state: &Arc<DaemonState>,
    params: &WorkspaceSnapshotApplyParams,
) -> Result<IpcResponse, IpcError> {
    let row = state
        .store
        .get_workspace_snapshot(&params.snapshot_id)
        .map_err(|e| {
            IpcError::new(
                IpcErrorCode::Internal,
                format!("workspace_snapshot_apply: {e}"),
            )
        })?;
    let Some(row) = row else {
        return Err(IpcError::new(
            IpcErrorCode::FileNotFound,
            format!("workspace snapshot '{}' is not known", params.snapshot_id),
        ));
    };
    let applied_cwd = state
        .sessions
        .apply_workspace(params.session_id, row.cwd.clone(), row.env)
        .await
        .map_err(|e| map_session_error(&e))?;
    Ok(IpcResponse::WorkspaceSnapshotApply(
        WorkspaceSnapshotApplyResponse {
            applied: true,
            session_id: params.session_id,
            cwd: applied_cwd,
        },
    ))
}
