// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use terminal_commander_supervisor::identity::PeerIdentity;

use super::common::{identity_audit_subject, map_command_error};
use crate::command::CommandStartRequest;
use crate::ipc::protocol::{
    CommandOutputTailParams, CommandOutputTailResponse, CommandStartParams, CommandStatusParams,
    CommandStopParams, CommandStopResponse, IpcError, IpcErrorCode, IpcResponse,
    MAX_COMMAND_ENV_ITEMS, MAX_COMMAND_INLINE_RULES, MAX_TAIL_BYTES, MAX_TAIL_LINES,
};
use crate::state::DaemonState;

pub(in crate::ipc::server) fn handle_command_start_combed(
    state: &Arc<DaemonState>,
    params: &CommandStartParams,
    peer: &PeerIdentity,
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
        // TC-2: thread the client dedup hint end-to-end. Without this
        // explicit assignment the field is silently dropped at this hand-
        // built conversion (amendment #7).
        dedup_nonce: params.dedup_nonce.clone(),
        // TC-2 peer-scoped fallback: pre-hash the dispatching peer so the
        // nonce-less fingerprint window only collapses a SAME-peer retry,
        // never a sibling client guessing another peer's command.
        peer_discriminator: Some(peer_discriminator(peer)),
    };
    let resp = state.command.start_combed(req).map_err(map_command_error)?;
    Ok(IpcResponse::CommandStartCombed(resp))
}

/// TC-3 `command_stop` handler: force-kill a running combed command.
///
/// Mirrors [`handle_command_start_combed`]'s convention: returns
/// `Result<IpcResponse, IpcError>` and maps the runtime error via
/// [`map_command_error`] (so `PolicyDenied -> PolicyDenied` and
/// `UnknownJob -> UnknownJob` reach the wire with the right codes).
///
/// The peer is rendered to an audit subject via the SHARED
/// [`identity_audit_subject`] helper and passed to `stop` so a
/// policy-denied caller's deny audit row names the PEER, never the
/// `job_id` -- the deny path inside `stop` never touches the live map.
pub(in crate::ipc::server) fn handle_command_stop(
    state: &Arc<DaemonState>,
    params: &CommandStopParams,
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    let peer_subject = identity_audit_subject(peer);
    match state.command.stop(params.job_id, &peer_subject) {
        Ok((bucket_id, m)) => Ok(IpcResponse::CommandStop(CommandStopResponse {
            job_id: params.job_id,
            bucket_id,
            frames_total: m.frames_total,
            events_emitted: m.events_emitted,
            bytes_total: m.bytes_total,
        })),
        Err(e) => Err(map_command_error(e)),
    }
}

/// Stable per-peer discriminator for the TC-2 nonce-less dedup fallback.
///
/// A `DefaultHasher` digest of the peer's stable identity field (uid for
/// Unix, sid for Windows). The pid is deliberately EXCLUDED so two
/// connections from the same principal still dedup a retry. An unknown
/// peer hashes to a single shared bucket -- conservative: it can only
/// collapse with another equally-unknown peer's identical signature
/// inside the short TTL.
fn peer_discriminator(peer: &PeerIdentity) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match peer {
        PeerIdentity::Unix { uid, .. } => {
            0u8.hash(&mut h);
            uid.hash(&mut h);
        }
        PeerIdentity::Windows { sid, .. } => {
            1u8.hash(&mut h);
            sid.hash(&mut h);
        }
        PeerIdentity::Unknown { .. } => {
            2u8.hash(&mut h);
        }
    }
    h.finish()
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
