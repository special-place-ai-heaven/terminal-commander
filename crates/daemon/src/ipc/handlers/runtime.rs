// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use crate::ipc::protocol::{
    IpcError, IpcErrorCode, IpcResponse, Liveness, ProbeKind, ProbeListEntry, ProbeListResponse,
    ProbeStatusParams, ProbeStatusResponse, RuntimeActiveRule, RuntimeBucketSummary,
    RuntimeStateResponse,
};
use crate::state::DaemonState;

pub(in crate::ipc::server) fn collect_probes(state: &Arc<DaemonState>) -> Vec<ProbeListEntry> {
    let mut out: Vec<ProbeListEntry> = Vec::new();

    // CommandRuntime: live_jobs + per-job status for counters.
    for j in state.command.live_jobs() {
        let status = state.command.status(j.job_id).ok();
        // Liveness is the authoritative JobState from the ledger, NOT
        // live-map presence (bindings linger after exit). Missing status
        // (job dropped between live_jobs() and status()) -> Running.
        let liveness = status.as_ref().map_or(Liveness::Running, |s| {
            crate::liveness::command_liveness(s.state, s.exit_code, s.signal.clone())
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

pub(in crate::ipc::server) fn handle_runtime_state(
    state: &Arc<DaemonState>,
    params: &crate::ipc::protocol::ListLimitParams,
) -> IpcResponse {
    let limit = params
        .limit
        .unwrap_or(crate::ipc::protocol::MAX_LIST_LIMIT)
        .min(crate::ipc::protocol::MAX_LIST_LIMIT);
    let probes = collect_probes(state);
    // Counts reflect the TRUE totals (computed before truncation); the
    // returned vecs are bounded independently by `limit`.
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

    // Bound each of the three vecs INDEPENDENTLY (a single cursor cannot
    // page three lists — subscriptions §6).
    let probes_truncated = probes.len() > limit;
    let buckets_truncated = buckets.len() > limit;
    let active_rules_truncated = active_rules.len() > limit;
    let probes: Vec<_> = probes.into_iter().take(limit).collect();
    let buckets: Vec<_> = buckets.into_iter().take(limit).collect();
    let active_rules: Vec<_> = active_rules.into_iter().take(limit).collect();

    IpcResponse::RuntimeState(RuntimeStateResponse {
        command_jobs,
        pty_jobs,
        file_watches,
        bucket_count,
        active_rules_count,
        probes,
        buckets,
        active_rules,
        probes_truncated,
        buckets_truncated,
        active_rules_truncated,
    })
}

pub(in crate::ipc::server) fn handle_probe_list(
    state: &Arc<DaemonState>,
    params: &crate::ipc::protocol::ListLimitParams,
) -> IpcResponse {
    let limit = params
        .limit
        .unwrap_or(crate::ipc::protocol::MAX_LIST_LIMIT)
        .min(crate::ipc::protocol::MAX_LIST_LIMIT);
    let probes = collect_probes(state);
    let truncated = probes.len() > limit;
    let probes: Vec<_> = probes.into_iter().take(limit).collect();
    IpcResponse::ProbeList(ProbeListResponse { probes, truncated })
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
