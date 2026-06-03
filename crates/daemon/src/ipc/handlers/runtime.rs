// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use crate::ipc::protocol::{
    IpcError, IpcErrorCode, IpcResponse, Liveness, ProbeKind, ProbeListEntry, ProbeListResponse,
    ProbeStatusParams, ProbeStatusResponse, RuntimeActiveRule, RuntimeBucketSummary,
    RuntimeStateResponse,
};
use crate::state::DaemonState;

/// Map a command job's ledger state to [`Liveness`]. Reads the
/// authoritative `JobState` (+ exit metadata) from
/// `command.status(job)` rather than live-map presence (bindings
/// linger after exit). See subscriptions spec MUST-ADD #3.
///
/// Cancellation is reported as [`Liveness::Cancelled`] and MUST NOT be
/// folded into `Failed`, even though `cancel()` sets a `"CANCELLED"`
/// signal on the exit info.
fn command_liveness(
    state: terminal_commander_core::JobState,
    exit_code: Option<i32>,
    signal: Option<String>,
) -> Liveness {
    use terminal_commander_core::JobState;
    match state {
        JobState::Starting => Liveness::Starting,
        JobState::Running => Liveness::Running,
        // `finish` only assigns `Exited` for code 0 with no signal.
        JobState::Exited => Liveness::Exited {
            code: exit_code.unwrap_or(0),
        },
        JobState::Failed => Liveness::Failed {
            code: exit_code,
            signal,
        },
        JobState::Cancelled => Liveness::Cancelled,
    }
}
pub(in crate::ipc::server) fn collect_probes(state: &Arc<DaemonState>) -> Vec<ProbeListEntry> {
    let mut out: Vec<ProbeListEntry> = Vec::new();

    // CommandRuntime: live_jobs + per-job status for counters.
    for j in state.command.live_jobs() {
        let status = state.command.status(j.job_id).ok();
        // Liveness is the authoritative JobState from the ledger, NOT
        // live-map presence (bindings linger after exit). Missing status
        // (job dropped between live_jobs() and status()) -> Running.
        let liveness = status.as_ref().map_or(Liveness::Running, |s| {
            command_liveness(s.state, s.exit_code, s.signal.clone())
        });
        out.push(ProbeListEntry {
            kind: ProbeKind::Command,
            job_id: j.job_id,
            bucket_id: j.bucket_id,
            probe_id: j.probe_id,
            frames_total: status.as_ref().map_or(0, |s| s.frames_total),
            events_emitted: status.as_ref().map_or(0, |s| s.events_emitted),
            frames_suppressed: status.as_ref().map_or(0, |s| s.frames_suppressed),
            frames_suppressed_progress: status.as_ref().map_or(0, |s| s.frames_suppressed_progress),
            frames_suppressed_dedupe: status.as_ref().map_or(0, |s| s.frames_suppressed_dedupe),
            secret_prompts_total: 0,
            secret_prompt_active: false,
            path: None,
            liveness,
        });
    }

    // WatchRuntime: list returns (job_id, bucket_id, probe_id, path, metrics).
    // File-watch has no exit-code concept: present -> Running.
    for (wid, bid, pid, path, m) in state.watch.list() {
        out.push(ProbeListEntry {
            kind: ProbeKind::FileWatch,
            job_id: wid,
            bucket_id: bid,
            probe_id: pid,
            frames_total: m.frames_total,
            events_emitted: m.events_emitted,
            frames_suppressed: m.frames_suppressed,
            frames_suppressed_progress: m.frames_suppressed_progress,
            frames_suppressed_dedupe: m.frames_suppressed_dedupe,
            secret_prompts_total: 0,
            secret_prompt_active: false,
            path: Some(path),
            liveness: Liveness::Running,
        });
    }

    // PtyRuntime: list returns (job_id, bucket_id, probe_id, argv, metrics, secret_active).
    // PTY exit is not wired through this list path: present -> Running.
    #[cfg(unix)]
    for (jid, bid, pid, _argv, m, secret) in state.pty.list() {
        out.push(ProbeListEntry {
            kind: ProbeKind::Pty,
            job_id: jid,
            bucket_id: bid,
            probe_id: pid,
            frames_total: m.frames_total,
            events_emitted: m.events_emitted,
            frames_suppressed: m.frames_suppressed,
            frames_suppressed_progress: m.frames_suppressed_progress,
            frames_suppressed_dedupe: m.frames_suppressed_dedupe,
            secret_prompts_total: m.secret_prompts_total,
            secret_prompt_active: secret,
            path: None,
            liveness: Liveness::Running,
        });
    }

    out
}

pub(in crate::ipc::server) fn handle_runtime_state(state: &Arc<DaemonState>) -> IpcResponse {
    let probes = collect_probes(state);
    let command_jobs = u32::try_from(
        probes
            .iter()
            .filter(|p| matches!(p.kind, ProbeKind::Command))
            .count(),
    )
    .unwrap_or(u32::MAX);
    let pty_jobs = u32::try_from(
        probes
            .iter()
            .filter(|p| matches!(p.kind, ProbeKind::Pty))
            .count(),
    )
    .unwrap_or(u32::MAX);
    let file_watches = u32::try_from(
        probes
            .iter()
            .filter(|p| matches!(p.kind, ProbeKind::FileWatch))
            .count(),
    )
    .unwrap_or(u32::MAX);

    // Bucket counters: walk every live bucket via the new
    // `BucketManager::list_bucket_ids`. `summary` runs TTL eviction
    // and returns the bounded counters we surface.
    let mut buckets: Vec<RuntimeBucketSummary> = Vec::new();
    for bid in state.buckets.list_bucket_ids() {
        if let Ok(s) = state.buckets.summary(bid) {
            buckets.push(RuntimeBucketSummary {
                bucket_id: bid,
                head_seq: s.head_seq,
                tail_seq: s.tail_seq,
                event_count: s.event_count,
                dropped_count: s.dropped_count,
            });
        }
    }
    let bucket_count = u32::try_from(buckets.len()).unwrap_or(u32::MAX);

    // Scoped activation snapshot.
    let active_entries = state.activation.snapshot_entries();
    let active_rules: Vec<RuntimeActiveRule> = active_entries
        .into_iter()
        .map(|e| RuntimeActiveRule {
            rule_id: e.definition.id,
            version: e.definition.version,
            event_kind: e.definition.event_kind,
            scope: e.scope,
        })
        .collect();
    let active_rules_count = u32::try_from(active_rules.len()).unwrap_or(u32::MAX);

    IpcResponse::RuntimeState(RuntimeStateResponse {
        command_jobs,
        pty_jobs,
        file_watches,
        bucket_count,
        active_rules_count,
        probes,
        buckets,
        active_rules,
    })
}

pub(in crate::ipc::server) fn handle_probe_list(state: &Arc<DaemonState>) -> IpcResponse {
    IpcResponse::ProbeList(ProbeListResponse {
        probes: collect_probes(state),
    })
}

#[allow(clippy::option_if_let_else)]
pub(in crate::ipc::server) fn handle_probe_status(
    state: &Arc<DaemonState>,
    params: &ProbeStatusParams,
) -> Result<IpcResponse, IpcError> {
    let probes = collect_probes(state);
    match probes.into_iter().find(|p| p.probe_id == params.probe_id) {
        Some(p) => Ok(IpcResponse::ProbeStatus(ProbeStatusResponse { probe: p })),
        None => Err(IpcError::new(
            IpcErrorCode::UnknownProbe,
            format!(
                "probe '{}' is not live in any runtime",
                params.probe_id.to_wire_string()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::JobState;

    // The load-bearing invariant (spec MUST-ADD #3): a cancelled job
    // reports `Cancelled`, NOT `Failed`, even though `cancel()` stamps a
    // `"CANCELLED"` signal on the exit info. An exited job reports
    // `Exited{code}`.
    #[test]
    fn command_liveness_maps_jobstate_without_folding_cancel_into_failed() {
        assert_eq!(
            command_liveness(JobState::Starting, None, None),
            Liveness::Starting
        );
        assert_eq!(
            command_liveness(JobState::Running, None, None),
            Liveness::Running
        );
        // Clean exit -> Exited{code}. `finish` only sets Exited for code 0.
        assert_eq!(
            command_liveness(JobState::Exited, Some(0), None),
            Liveness::Exited { code: 0 }
        );
        // Missing exit code on an Exited state defaults to 0 (clean).
        assert_eq!(
            command_liveness(JobState::Exited, None, None),
            Liveness::Exited { code: 0 }
        );
        // Non-zero / signalled exit -> Failed{code,signal}.
        assert_eq!(
            command_liveness(JobState::Failed, Some(2), None),
            Liveness::Failed {
                code: Some(2),
                signal: None
            }
        );
        assert_eq!(
            command_liveness(JobState::Failed, None, Some("SIGTERM".to_owned())),
            Liveness::Failed {
                code: None,
                signal: Some("SIGTERM".to_owned())
            }
        );
        // Cancellation -> Cancelled, NOT Failed (even with a CANCELLED
        // signal present on the exit info).
        assert_eq!(
            command_liveness(JobState::Cancelled, None, Some("CANCELLED".to_owned())),
            Liveness::Cancelled
        );
    }
}
