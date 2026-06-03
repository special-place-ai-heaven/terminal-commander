// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use super::common::map_command_error;
use crate::command::CommandStartRequest;
use crate::ipc::protocol::{
    CommandOutputTailParams, CommandOutputTailResponse, CommandStartParams, CommandStatusParams,
    IpcError, IpcErrorCode, IpcResponse, MAX_COMMAND_ENV_ITEMS, MAX_COMMAND_INLINE_RULES,
    MAX_TAIL_BYTES, MAX_TAIL_LINES,
};
use crate::state::DaemonState;

pub(in crate::ipc::server) fn handle_command_start_combed(
    state: &Arc<DaemonState>,
    params: &CommandStartParams,
) -> Result<IpcResponse, IpcError> {
    if params.env.len() > MAX_COMMAND_ENV_ITEMS {
        return Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            format!("env entries {} exceed cap", params.env.len()),
        ));
    }
    if params.rules.len() > MAX_COMMAND_INLINE_RULES {
        return Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            format!("inline rules {} exceed cap", params.rules.len()),
        ));
    }
    let req = CommandStartRequest {
        argv: params.argv.clone(),
        cwd: params.cwd.clone(),
        env: params.env.clone(),
        bucket_config: params.bucket_config.clone(),
        rules: params.rules.clone(),
        grace: params.grace(),
        tag: params.tag.clone(),
    };
    let resp = state.command.start_combed(req).map_err(map_command_error)?;
    Ok(IpcResponse::CommandStartCombed(resp))
}

pub(in crate::ipc::server) fn handle_command_status(
    state: &Arc<DaemonState>,
    params: &CommandStatusParams,
) -> Result<IpcResponse, IpcError> {
    let resp = state
        .command
        .status(params.job_id)
        .map_err(map_command_error)?;
    Ok(IpcResponse::CommandStatus(resp))
}

pub(in crate::ipc::server) fn handle_command_output_tail(
    state: &Arc<DaemonState>,
    params: &CommandOutputTailParams,
) -> Result<IpcResponse, IpcError> {
    let rec = state.jobs.get(params.job_id).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::UnknownJob,
            format!("unknown job: {}", params.job_id),
        )
    })?;
    let probe_id = rec.config.probe_id;
    let max_lines = (params.max_lines as usize).min(MAX_TAIL_LINES);
    let max_bytes = (params.max_bytes as usize).min(MAX_TAIL_BYTES);
    // NotFound = ring absent (job had no ring yet); treat as empty tail
    let tail = match state.rings.tail_frames(probe_id, max_lines, max_bytes) {
        Ok(t) => t,
        Err(terminal_commander_core::ContextError::NotFound(_)) => {
            terminal_commander_core::RingTail {
                lines: vec![],
                evicted_frames: 0,
                truncated: false,
            }
        }
        Err(e) => return Err(IpcError::new(IpcErrorCode::Internal, e.to_string())),
    };
    let frame_count = state.rings.frame_count(probe_id);
    // Safe: tail.lines.len() is bounded by MAX_TAIL_LINES (200), fits u32.
    #[allow(clippy::cast_possible_truncation)]
    let returned_lines = tail.lines.len() as u32;
    let truncated_lines = frame_count > tail.lines.len();
    let truncated_bytes = tail.truncated;
    Ok(IpcResponse::CommandOutputTail(CommandOutputTailResponse {
        job_id: params.job_id,
        lines: tail.lines,
        returned_lines,
        truncated_lines,
        truncated_bytes,
        evicted_frames: tail.evicted_frames,
    }))
}
