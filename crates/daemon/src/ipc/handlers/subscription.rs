// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! IPC handlers for the predicate-routed subscription surface
//! (`subscription_open/pull/list/close`). Thin glue between the wire
//! protocol and the daemon-internal subscription core (`crate::subscriptions`):
//! parse the wire predicate, compute from-now initial offsets, drive the
//! lossless multiplexed `pull`, and project the outcome back onto the wire.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::ipc::protocol::{
    DEFAULT_PULL_TIMEOUT_MS, IpcError, IpcErrorCode, IpcResponse, MAX_PULL_EVENTS,
    MAX_PULL_TIMEOUT_MS, MAX_SUBSCRIPTIONS, SourceLiveness, SubscriptionCloseParams,
    SubscriptionCloseResponse, SubscriptionEvent, SubscriptionListParams, SubscriptionListResponse,
    SubscriptionOpenParams, SubscriptionOpenResponse, SubscriptionPredicate,
    SubscriptionPullParams, SubscriptionPullResponse, SubscriptionSeekParams,
    SubscriptionSeekResponse, SubscriptionSourceSel, SubscriptionSummary,
};
use crate::state::DaemonState;
use crate::subscriptions::model::{Predicate, SourceSel};
use crate::subscriptions::pull::{self, PullOutcome};
use terminal_commander_core::BucketId;
use uuid::Uuid;

/// Convert the wire predicate into the daemon-internal [`Predicate`].
fn predicate_from_wire(wire: SubscriptionPredicate) -> Predicate {
    let sources = match wire.sources {
        SubscriptionSourceSel::All => SourceSel::All,
        SubscriptionSourceSel::Jobs { jobs } => SourceSel::Jobs(jobs),
        SubscriptionSourceSel::Buckets { buckets } => SourceSel::Buckets(buckets),
        SubscriptionSourceSel::Probes { probes } => SourceSel::Probes(probes),
    };
    Predicate {
        severity_min: wire.severity_min,
        kind: wire.kind,
        sources,
        tag: wire.tag,
    }
}

/// Compute from-now initial offsets for every already-in-scope bucket: a
/// late open starts at each bucket's CURRENT tail (`tail_seq`), so it never
/// replays the ring. Buckets born after the open start at offset 0 and are
/// auto-joined by the next pull's routing rebuild. Returns the offset map
/// plus the matched-source count.
fn initial_offsets(
    state: &Arc<DaemonState>,
    predicate: &Predicate,
) -> (HashMap<BucketId, u64>, u32) {
    let mut offsets: HashMap<BucketId, u64> = HashMap::new();
    for id in state.buckets.list_bucket_ids() {
        let Some(source) = state.sources.get(id) else {
            continue;
        };
        if predicate.bucket_in_scope(id, &source) {
            // From-now: events_since reads strictly `seq > cursor`, so the
            // current `tail_seq` means "deliver only events after the tip".
            let tail = state.buckets.state(id).map_or(0, |s| s.tail_seq);
            offsets.insert(id, tail);
        }
    }
    let matched = u32::try_from(offsets.len()).unwrap_or(u32::MAX);
    (offsets, matched)
}

/// Milliseconds since the Unix epoch right now (wall clock for wire stamps).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

pub(in crate::ipc::server) fn handle_subscription_open(
    state: &Arc<DaemonState>,
    params: &SubscriptionOpenParams,
) -> Result<IpcResponse, IpcError> {
    let predicate = predicate_from_wire(params.predicate.clone());
    let predicate_hash = predicate.normalized_hash();
    let (offsets, matched_sources) = initial_offsets(state, &predicate);
    let sub_id = state.subscriptions.open(predicate, offsets)?;
    Ok(IpcResponse::SubscriptionOpen(SubscriptionOpenResponse {
        sub_id: sub_id.to_string(),
        boot_id: state.boot_id.to_string(),
        predicate_hash: predicate_hash.to_string(),
        created_at_ms: now_ms(),
        matched_sources,
    }))
}

/// Parse a wire `sub_id` string into a [`Uuid`]. A parse failure is treated
/// as [`IpcErrorCode::UnknownSubscription`] (a malformed handle is, by
/// definition, not a known subscription).
fn parse_sub_id(s: &str) -> Result<Uuid, IpcError> {
    Uuid::parse_str(s).map_err(|_| {
        IpcError::new(
            IpcErrorCode::UnknownSubscription,
            format!("unknown subscription: {s}"),
        )
    })
}

pub(in crate::ipc::server) async fn handle_subscription_pull(
    state: &Arc<DaemonState>,
    params: &SubscriptionPullParams,
) -> Result<IpcResponse, IpcError> {
    let sub_id = parse_sub_id(&params.sub_id)?;
    let max = params.max.unwrap_or(MAX_PULL_EVENTS).min(MAX_PULL_EVENTS);
    let timeout_ms = params
        .timeout_ms
        .unwrap_or(DEFAULT_PULL_TIMEOUT_MS)
        .clamp(1, MAX_PULL_TIMEOUT_MS);
    let outcome = pull::pull(state, sub_id, max, Duration::from_millis(timeout_ms)).await?;
    Ok(IpcResponse::SubscriptionPull(pull_outcome_to_wire(outcome)))
}

/// Project a daemon-internal [`PullOutcome`] onto the wire response.
fn pull_outcome_to_wire(outcome: PullOutcome) -> SubscriptionPullResponse {
    let events = outcome
        .events
        .into_iter()
        .map(|e| SubscriptionEvent {
            bucket_id: e.origin.bucket_id,
            job_id: e.origin.job_id,
            seq: e.origin.seq,
            event: e.event,
        })
        .collect();
    let liveness = outcome
        .liveness
        .into_iter()
        .map(|l| SourceLiveness {
            bucket_id: l.bucket_id,
            job_id: l.job_id,
            probe_id: l.probe_id,
            liveness: l.liveness,
        })
        .collect();
    SubscriptionPullResponse {
        events,
        liveness,
        lagged: outcome.lagged,
        truncated: outcome.truncated,
    }
}

pub(in crate::ipc::server) fn handle_subscription_list(
    state: &Arc<DaemonState>,
    params: &SubscriptionListParams,
) -> IpcResponse {
    let limit = params
        .limit
        .unwrap_or(MAX_SUBSCRIPTIONS)
        .min(MAX_SUBSCRIPTIONS);
    let mut all = state.subscriptions.list();
    // Deterministic order so pagination/truncation is stable across calls.
    all.sort_by_key(|s| s.sub_id);
    let truncated = all.len() > limit;
    let subscriptions: Vec<SubscriptionSummary> = all
        .into_iter()
        .take(limit)
        .map(|s| SubscriptionSummary {
            sub_id: s.sub_id.to_string(),
            predicate_hash: s.predicate_hash.to_string(),
            source_count: u32::try_from(s.source_count).unwrap_or(u32::MAX),
            created_at_ms: now_ms(),
            // `Instant` is not wall-clock convertible; surface the last-pull
            // recency only as presence (a wall-clock stamp would require
            // storing a `SystemTime` per pull, out of Phase 1 scope).
            last_pull_at_ms: s.last_pull_at.map(|_| now_ms()),
        })
        .collect();
    IpcResponse::SubscriptionList(SubscriptionListResponse {
        subscriptions,
        truncated,
    })
}

pub(in crate::ipc::server) fn handle_subscription_close(
    state: &Arc<DaemonState>,
    params: &SubscriptionCloseParams,
) -> IpcResponse {
    // A malformed id is simply "not present": idempotent close, never an
    // error (matches the registry's `close -> bool` contract).
    let closed = Uuid::parse_str(&params.sub_id).is_ok_and(|id| state.subscriptions.close(id));
    IpcResponse::SubscriptionClose(SubscriptionCloseResponse { closed })
}

/// Map a bucket-store error onto the closed IPC error set. A missing bucket is
/// the existing [`IpcErrorCode::BucketNotFound`]; anything else is internal.
/// Phase 3 adds NO new error code.
fn map_bucket(e: terminal_commander_core::BucketError) -> IpcError {
    use terminal_commander_core::BucketError;
    match e {
        BucketError::NotFound(_) => IpcError::new(IpcErrorCode::BucketNotFound, e.to_string()),
        other => IpcError::new(IpcErrorCode::Internal, other.to_string()),
    }
}

/// `subscription_seek` -> reposition this consumer's offset for ONE bucket.
///
/// The requested seq is CLAMPED to `[head_seq.saturating_sub(1), tail_seq]`
/// (never an error); `lagged` flags a request below the surviving head (the
/// requested events were already evicted). Phase 3 adds NO new error code.
///
/// Scope guard: an offset is written ONLY for a bucket the subscription
/// actually routes to -- one currently in the sub's predicate scope, or one
/// already tracked in `offsets`. Seeking a bucket OUTSIDE the predicate scope
/// is a NO-OP (it does not create a dangling offset that no pull would ever
/// read or advance); the response still reports the clamp so the caller sees
/// where it would park, but no dead state is left behind.
///
/// # Errors
/// - [`IpcErrorCode::UnknownSubscription`] if `sub_id` is malformed or the sub
///   is not present (via `parse_sub_id` and `with_sub_mut`'s miss path).
/// - [`IpcErrorCode::BucketNotFound`] if the daemon does not know the bucket.
pub(in crate::ipc::server) fn handle_subscription_seek(
    state: &Arc<DaemonState>,
    params: &SubscriptionSeekParams,
) -> Result<IpcResponse, IpcError> {
    let sub_id = parse_sub_id(&params.sub_id)?;
    let st = state.buckets.state(params.bucket_id).map_err(map_bucket)?;
    // The smallest position a consumer can be parked at is `head_seq - 1` (the
    // pre-first-readable cursor); the largest is `tail_seq`. A request below
    // the floor means the events it wanted were evicted -> lagged.
    let floor = st.head_seq.saturating_sub(1);
    let lagged = params.seq < floor;
    let clamped = params.seq.clamp(floor, st.tail_seq);

    // Resolve the bucket's source (immortal side-table) OUTSIDE the sub lock so
    // the in-scope check inside `with_sub_mut` uses the SAME predicate the pull
    // routing uses. An unknown source -> never in scope (only an already-tracked
    // offset keeps it routable).
    let source = state.sources.get(params.bucket_id);
    state.subscriptions.with_sub_mut(sub_id, |s| {
        // Write the offset only for a bucket this sub actually routes to:
        // in-scope per the live predicate, or already tracked. Seeking an
        // out-of-scope bucket is a no-op -- no dangling offset is created.
        let in_scope = source
            .as_ref()
            .is_some_and(|src| s.predicate.bucket_in_scope(params.bucket_id, src));
        if in_scope || s.offsets.contains_key(&params.bucket_id) {
            s.offsets.insert(params.bucket_id, clamped);
        }
    })?;
    Ok(IpcResponse::SubscriptionSeek(SubscriptionSeekResponse {
        clamped_seq: clamped,
        lagged,
    }))
}
