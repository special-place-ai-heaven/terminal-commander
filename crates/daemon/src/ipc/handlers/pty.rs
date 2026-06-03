// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use crate::ipc::protocol::{
    IpcError, IpcErrorCode, IpcResponse, IpcResult, PtyCommandStartParams, PtyCommandStopParams,
    PtyCommandWriteStdinParams,
};
#[cfg(unix)]
use crate::ipc::protocol::{
    MAX_COMMAND_ENV_ITEMS, MAX_COMMAND_INLINE_RULES, MAX_PTY_ARGV_ITEMS, MAX_PTY_STDIN_BYTES,
    PtyCommandListEntry, PtyCommandListResponse, PtyCommandStartResponse, PtyCommandStopResponse,
    PtyCommandWriteStdinResponse,
};
use crate::state::DaemonState;

#[cfg(not(unix))]
pub(in crate::ipc::server) fn pty_ipc_unsupported() -> IpcError {
    IpcError::new(
        IpcErrorCode::UnsupportedPlatform,
        "PTY command runtime is not available on this platform yet (ConPTY support pending)",
    )
}

#[cfg(not(unix))]
pub(in crate::ipc::server) fn handle_pty_command_start(
    _state: &Arc<DaemonState>,
    _params: &PtyCommandStartParams,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(unix))]
#[allow(clippy::unused_async)] // async matches the unix signature; removed when unix impl lands
pub(in crate::ipc::server) async fn handle_pty_command_write_stdin(
    _state: &Arc<DaemonState>,
    _params: &PtyCommandWriteStdinParams,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(unix))]
pub(in crate::ipc::server) fn handle_pty_command_stop(
    _state: &Arc<DaemonState>,
    _params: &PtyCommandStopParams,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(unix))]
pub(in crate::ipc::server) fn handle_pty_command_list(
    _state: &Arc<DaemonState>,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(unix))]
pub(in crate::ipc::server) fn dispatch_pty_command_list(
    state: &Arc<DaemonState>,
) -> (&'static str, IpcResult) {
    match handle_pty_command_list(state) {
        Ok(r) => ("pty_command_list", IpcResult::Ok { response: r }),
        Err(e) => ("pty_command_list", IpcResult::Err { error: e }),
    }
}

#[cfg(unix)]
pub(in crate::ipc::server) fn handle_pty_command_start(
    state: &Arc<DaemonState>,
    params: &PtyCommandStartParams,
) -> Result<IpcResponse, IpcError> {
    if params.argv.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            "argv must not be empty",
        ));
    }
    if params.argv.len() > MAX_PTY_ARGV_ITEMS {
        return Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            format!(
                "argv has {} items; cap is {MAX_PTY_ARGV_ITEMS}",
                params.argv.len()
            ),
        ));
    }
    if params.env.len() > MAX_COMMAND_ENV_ITEMS {
        return Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            "env exceeds bounded item cap",
        ));
    }
    if params.rules.len() > MAX_COMMAND_INLINE_RULES {
        return Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            "rules exceeds bounded item cap",
        ));
    }
    let env_os: Vec<(std::ffi::OsString, std::ffi::OsString)> = params
        .env
        .iter()
        .map(|(k, v)| (std::ffi::OsString::from(k), std::ffi::OsString::from(v)))
        .collect();
    let req = crate::pty_command::PtyStartRequest {
        argv: params.argv.clone(),
        cwd: params.cwd.clone(),
        env: env_os,
        bucket_config: params.bucket_config.clone(),
        rules: params.rules.clone(),
        rows: params.rows,
        cols: params.cols,
        tag: params.tag.clone(),
    };
    match state.pty.start(req) {
        Ok(r) => Ok(IpcResponse::PtyCommandStart(PtyCommandStartResponse {
            job_id: r.job_id,
            bucket_id: r.bucket_id,
            probe_id: r.probe_id,
            cursor: 0,
        })),
        Err(crate::pty_command::PtyRuntimeError::PolicyDenied(reason)) => {
            Err(IpcError::new(IpcErrorCode::PolicyDenied, reason))
        }
        Err(crate::pty_command::PtyRuntimeError::ShellInterpreterDenied(shell)) => {
            Err(IpcError::new(IpcErrorCode::ShellInterpreterDenied, shell))
        }
        Err(crate::pty_command::PtyRuntimeError::EmptyArgv) => Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            "argv must not be empty",
        )),
        Err(crate::pty_command::PtyRuntimeError::Sifter(reason)) => {
            Err(IpcError::new(IpcErrorCode::RuleInvalid, reason))
        }
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("pty_command_start: {other}"),
        )),
    }
}

#[cfg(unix)]
pub(in crate::ipc::server) async fn handle_pty_command_write_stdin(
    state: &Arc<DaemonState>,
    params: &PtyCommandWriteStdinParams,
) -> Result<IpcResponse, IpcError> {
    let bytes = params.bytes.as_bytes();
    if bytes.len() > MAX_PTY_STDIN_BYTES {
        return Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            format!("stdin payload {} > cap {MAX_PTY_STDIN_BYTES}", bytes.len()),
        ));
    }
    match state.pty.write_stdin(params.job_id, bytes).await {
        Ok(r) => Ok(IpcResponse::PtyCommandWriteStdin(
            PtyCommandWriteStdinResponse {
                job_id: params.job_id,
                bytes_written: r.bytes_written,
                secret_prompt_active: r.secret_prompt_active,
            },
        )),
        Err(crate::pty_command::PtyRuntimeError::SecretInputDenied) => Err(IpcError::new(
            IpcErrorCode::SecretInputDenied,
            "secret prompt active; LLM-supplied input denied",
        )),
        Err(crate::pty_command::PtyRuntimeError::OversizedStdin) => Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            "stdin exceeds bounded cap",
        )),
        Err(crate::pty_command::PtyRuntimeError::UnknownJob(id)) => Err(IpcError::new(
            IpcErrorCode::UnknownJob,
            format!("pty job '{}' is not live", id.to_wire_string()),
        )),
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("pty_command_write_stdin: {other}"),
        )),
    }
}

#[cfg(unix)]
pub(in crate::ipc::server) fn handle_pty_command_stop(
    state: &Arc<DaemonState>,
    params: &PtyCommandStopParams,
) -> Result<IpcResponse, IpcError> {
    match state.pty.stop(params.job_id) {
        Ok((bucket_id, m)) => Ok(IpcResponse::PtyCommandStop(PtyCommandStopResponse {
            job_id: params.job_id,
            bucket_id,
            frames_total: m.frames_total,
            events_emitted: m.events_emitted,
            bytes_total: m.bytes_total,
            stdin_bytes_written: m.stdin_bytes_written,
            secret_prompts_total: m.secret_prompts_total,
        })),
        Err(crate::pty_command::PtyRuntimeError::UnknownJob(id)) => Err(IpcError::new(
            IpcErrorCode::UnknownJob,
            format!("pty job '{}' is not live", id.to_wire_string()),
        )),
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("pty_command_stop: {other}"),
        )),
    }
}

#[cfg(unix)]
pub(in crate::ipc::server) fn handle_pty_command_list(state: &Arc<DaemonState>) -> IpcResponse {
    let entries: Vec<PtyCommandListEntry> = state
        .pty
        .list()
        .into_iter()
        // The binding lingers after exit so `collect_probes` can report a
        // terminal liveness; the operator-facing live list excludes terminal
        // jobs (Exited/Failed/Cancelled) so it shows only currently-live PTYs.
        .filter(|(job_id, ..)| {
            !matches!(
                state.pty.liveness(*job_id),
                terminal_commander_ipc::Liveness::Exited { .. }
                    | terminal_commander_ipc::Liveness::Failed { .. }
                    | terminal_commander_ipc::Liveness::Cancelled
                    | terminal_commander_ipc::Liveness::Stopped
            )
        })
        .map(
            |(job_id, bucket_id, probe_id, argv, m, secret_prompt_active)| PtyCommandListEntry {
                job_id,
                bucket_id,
                probe_id,
                argv,
                frames_total: m.frames_total,
                events_emitted: m.events_emitted,
                bytes_total: m.bytes_total,
                stdin_bytes_written: m.stdin_bytes_written,
                secret_prompts_total: m.secret_prompts_total,
                secret_prompt_active,
            },
        )
        .collect();
    IpcResponse::PtyCommandList(PtyCommandListResponse { entries })
}

#[cfg(unix)]
pub(in crate::ipc::server) fn dispatch_pty_command_list(
    state: &Arc<DaemonState>,
) -> (&'static str, IpcResult) {
    let r = handle_pty_command_list(state);
    ("pty_command_list", IpcResult::Ok { response: r })
}
