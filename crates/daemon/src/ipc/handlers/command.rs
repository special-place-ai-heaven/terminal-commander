// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use terminal_commander_core::RecipeTeachIntent;
use terminal_commander_supervisor::identity::PeerIdentity;

use super::common::{
    attach_recipe_steer, enrich_shell_teach, identity_audit_subject, map_command_error,
};
use crate::command::CommandStartRequest;
use crate::ipc::protocol::{
    CommandOutputTailParams, CommandOutputTailResponse, CommandStartParams, CommandStatusParams,
    CommandStopParams, CommandStopResponse, IpcError, IpcErrorCode, IpcResponse,
    MAX_COMMAND_ENV_ITEMS, MAX_COMMAND_INLINE_RULES, MAX_TAIL_BYTES, MAX_TAIL_LINES,
    ShellExecParams,
};
use crate::shell::ShellExecRequest;
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
        // TC-B1: thread the strip flag end-to-end (default true at the IPC
        // boundary via serde `default_true`).
        strip_ansi: params.strip_ansi,
        // TC-2: thread the client dedup hint end-to-end. Without this
        // explicit assignment the field is silently dropped at this hand-
        // built conversion (amendment #7).
        dedup_nonce: params.dedup_nonce.clone(),
        // TC-2 peer-scoped fallback: pre-hash the dispatching peer so the
        // nonce-less fingerprint window only collapses a SAME-peer retry,
        // never a sibling client guessing another peer's command.
        peer_discriminator: Some(peer_discriminator(peer)),
    };
    let resp = state.command.start_combed(req).map_err(|e| {
        let err = enrich_shell_teach(&state.policy, "command_start_combed", map_command_error(e));
        attach_recipe_steer(state, RecipeTeachIntent::Argv(&params.argv), err)
    })?;
    Ok(IpcResponse::CommandStartCombed(resp))
}

/// Handle a `shell_exec` IPC request (TC49). Mirrors
/// [`handle_command_start_combed`] but routes through the gated shell
/// lane: it builds a [`ShellExecRequest`] from the wire params and calls
/// the SYNC [`ShellRuntime::exec`](crate::shell::ShellRuntime::exec),
/// which gates on `PolicyAction::CommandShellStart` (`allow_shell`).
///
/// The shell lane SKIPS the `SHELL_INTERPRETERS_DENY` guard, so it can
/// NEVER produce [`CommandError::ShellInterpreterDenied`]; its denials are
/// [`CommandError::PolicyDenied`], which [`map_command_error`] maps to
/// [`IpcErrorCode::PolicyDenied`]. The reply reuses
/// [`IpcResponse::CommandStartCombed`] — the shell lane returns the same
/// bounded [`CommandStartResponse`](crate::ipc::protocol::CommandStartResponse)
/// shape and never raw stdout/stderr.
///
/// SYNC: `exec` never awaits, so no `.await` here — the async dispatcher
/// calls this inline.
pub(in crate::ipc::server) fn handle_shell_exec(
    state: &Arc<DaemonState>,
    params: &ShellExecParams,
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
    let req = ShellExecRequest {
        shell_line: params.shell_line.clone(),
        shell: params.shell.clone(),
        cwd: params.cwd.clone(),
        env: params.env.clone(),
        rules: params.rules.clone(),
        bucket_config: params.bucket_config.clone(),
        tag: params.tag.clone(),
    };
    let resp = state.shell.exec(req).map_err(|e| {
        let err = enrich_shell_teach(&state.policy, "shell_exec", map_command_error(e));
        attach_recipe_steer(state, RecipeTeachIntent::ShellLine(&params.shell_line), err)
    })?;
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
    // 1. Ask the lane that OWNS this job. The ledger is shared across lanes, so
    //    the combed runtime declines jobs whose metrics live elsewhere; a PTY or
    //    watch job answers here with REAL counters. Constitution VII forbids
    //    discarding a live job behind a bare error, and `command_status` is
    //    today the only surface exposing a finished PTY job's exit code at all.
    if let Some(resp) = state.live_lane_status(params.job_id) {
        return Ok(IpcResponse::CommandStatus(resp));
    }
    let job_id = params.job_id;

    // 2. No live owner. Consult the persisted receipt: a known terminal outcome
    //    from disk beats a bare "unknown job" (constitution VII honest
    //    degradation).
    if let Some(resp) = state.command.reconstructed_status(job_id) {
        return Ok(IpcResponse::CommandStatus(resp));
    }

    // 3. No receipt exists. Did the daemon durably record this job STARTING? If
    //    so it is LOST: it began and never reached a recorded terminal
    //    transition, which is a diagnosis rather than the "never heard of it"
    //    that `UnknownJob` implies.
    //
    //    Fails safe (spec FR-011). Audit emits are dropped on failure at the
    //    call site, so absence is not proof; an errored or empty lookup degrades
    //    to `UnknownJob` and NEVER to a terminal outcome. The failure direction
    //    is one-way by construction.
    let wire = job_id.to_wire_string();
    if state.store.job_start_recorded(&wire).unwrap_or(false) {
        return Err(IpcError::new(
            IpcErrorCode::JobLost,
            format!("job {wire} started but never recorded a terminal transition"),
        ));
    }
    Err(map_command_error(crate::command::CommandError::UnknownJob(
        job_id,
    )))
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
    let mut tail = match state.rings.tail_frames(probe_id, max_lines, max_bytes) {
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
    if params.strip_ansi {
        tail.lines = tail
            .lines
            .into_iter()
            .map(|line| terminal_commander_probes::strip_ansi(&line))
            .collect();
    }
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

#[cfg(test)]
mod tests {
    use terminal_commander_core::JobId;
    use terminal_commander_store::AuditEntry;

    use super::{CommandStatusParams, IpcErrorCode, handle_command_status};
    use crate::audit::AuditSink;
    use crate::config::DaemonConfig;
    use crate::state::DaemonState;

    fn tmp_data_dir(tag: &str) -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        p.push(format!(
            "tc-joblost-{tag}-{}-{nanos}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        p
    }

    /// spec 004 FR-009: a job the daemon durably recorded STARTING, with no
    /// receipt, is LOST -- a diagnosis, not "never heard of it".
    ///
    /// This is the wire-level half of the guarantee; the discrimination logic
    /// itself is covered in `tests/status_lost_detection.rs`.
    #[test]
    fn a_started_job_with_no_receipt_reports_job_lost() {
        let data = tmp_data_dir("lost");
        let state = std::sync::Arc::new(
            DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).expect("bootstrap"),
        );

        let job_id = JobId::new();
        state
            .audit
            .emit(&AuditEntry::new(
                "command_start",
                job_id.to_wire_string(),
                "allow",
            ))
            .expect("audit emit");

        let err = handle_command_status(&state, &CommandStatusParams { job_id })
            .expect_err("a lost job must not return a status");
        assert_eq!(
            err.code,
            IpcErrorCode::JobLost,
            "a started-but-unfinished job is LOST, not unknown"
        );

        let _ = std::fs::remove_dir_all(&data);
    }

    /// An identifier with no durable trace at all stays `UnknownJob`. The lost
    /// case must NARROW that error, not swallow it.
    #[test]
    fn an_identifier_with_no_trace_reports_unknown_job() {
        let data = tmp_data_dir("unknown");
        let state = std::sync::Arc::new(
            DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).expect("bootstrap"),
        );

        let err = handle_command_status(
            &state,
            &CommandStatusParams {
                job_id: JobId::new(),
            },
        )
        .expect_err("an unknown id must not return a status");
        assert_eq!(err.code, IpcErrorCode::UnknownJob);

        let _ = std::fs::remove_dir_all(&data);
    }
}
