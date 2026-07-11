// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Multiplexed, lossless `subscription_pull` engine (subscriptions §3) —
//! the load-bearing correctness core.
//!
//! THE INVARIANT: within a pull pass, no `events_since` read for a bucket may
//! precede that bucket's waiter ENROLLMENT, because the bucket signals with the
//! permit-less `notify_waiters()` (no stored permit; only ENROLLED waiters
//! wake). In tokio a `Notified` future enrolls into the waiter list only when
//! FIRST POLLED or via `Notified::enable()` — NOT when created or pinned. So we
//! create fresh `notified()` futures each loop iteration, `enable()` them to
//! enroll, and ONLY THEN read offsets. The cursor/seq is the source of truth
//! (lossless); `Notify` is a latency hint only.
//!
//! Fairness (v1): deterministic round-robin, per-bucket share `max(1, max/N)`,
//! draining from `rr_start`, STOPPING the moment the running total reaches
//! `max` (so `N > max` still returns `<= max`). Eviction reconciliation: if a
//! stored offset has fallen behind a bucket's `head_seq` (events FIFO-evicted),
//! clamp to `head_seq.saturating_sub(1)` (NOT `head_seq` — `events_since` reads
//! strictly `seq > cursor`, and `head_seq` is the oldest SURVIVING event's own
//! seq, so clamping to `head_seq` would skip that survivor) and flag `lagged`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use terminal_commander_core::{BucketId, BucketReadRequest, JobId, ProbeId, SignalEvent};
use terminal_commander_ipc::{IpcError, IpcErrorCode, Liveness, MAX_BUCKETS_PER_SUBSCRIPTION};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::state::DaemonState;
use crate::subscriptions::model::{CachedScope, Predicate};
use crate::subscriptions::source::BucketSource;

/// Default hard cap on events returned by one pull when the caller omits a
/// `max` (mirrors the wire `MAX_PULL_EVENTS` that Task 9 will add).
pub const DEFAULT_PULL_MAX: usize = 50;
/// Absolute hard cap on events per pull, independent of the caller's `max`.
pub const MAX_PULL_MAX: usize = 50;

/// Where a delivered event came from. Tags each event so a multiplexed
/// consumer can attribute it without juggling N cursors.
#[derive(Debug, Clone)]
pub struct EventOrigin {
    /// Bucket the event was read from.
    pub bucket_id: BucketId,
    /// Owning job (when the bucket's source recorded one).
    pub job_id: Option<JobId>,
    /// The event's per-bucket sequence number.
    pub seq: u64,
}

/// A delivered event plus its source tag.
#[derive(Debug, Clone)]
pub struct SubEvent {
    /// Provenance of this event.
    pub origin: EventOrigin,
    /// The matched signal event.
    pub event: SignalEvent,
}

/// Per-source liveness entry returned with every pull (including idle pulls).
#[derive(Debug, Clone)]
pub struct SourceLiveness {
    /// The in-scope bucket this entry describes.
    pub bucket_id: BucketId,
    /// Owning job, if recorded.
    pub job_id: Option<JobId>,
    /// Owning probe, if recorded.
    pub probe_id: Option<ProbeId>,
    /// Process / probe liveness (the single authoritative union).
    pub liveness: Liveness,
}

/// The outcome of one pull: bounded events + per-source liveness + flags.
#[derive(Debug, Clone)]
pub struct PullOutcome {
    /// Matched events, tagged by source, capped at `max`.
    pub events: Vec<SubEvent>,
    /// Per in-scope source liveness.
    pub liveness: Vec<SourceLiveness>,
    /// Any in-scope bucket dropped events under us (eviction lag).
    pub lagged: bool,
    /// The routing scan hit [`MAX_BUCKETS_PER_SUBSCRIPTION`] (more in-scope
    /// buckets exist than were scanned this pull).
    pub truncated: bool,
}

impl PullOutcome {
    /// An idle outcome: no events, just liveness + flags.
    const fn idle(liveness: Vec<SourceLiveness>, lagged: bool, truncated: bool) -> Self {
        Self {
            events: Vec::new(),
            liveness,
            lagged,
            truncated,
        }
    }
}

/// One in-scope bucket resolved by the routing rebuild.
struct ScopedBucket {
    bucket_id: BucketId,
    source: BucketSource,
}

/// The result of a routing rebuild for one pull.
struct Scope {
    /// In-scope buckets (capped at [`MAX_BUCKETS_PER_SUBSCRIPTION`]).
    buckets: Vec<ScopedBucket>,
    /// The subscription's per-bucket offsets (a working copy, committed back
    /// only when events are delivered).
    offsets: HashMap<BucketId, u64>,
    /// Round-robin rotation cursor copied from the subscription.
    rr_start: usize,
    /// More buckets matched than the cap allowed.
    truncated: bool,
}

/// Resolve the in-scope bucket set for a pull, gated by the source side-table's
/// dirty epoch (subscriptions §1, LOAD-BEARING).
///
/// Fast path: if the subscription's cached scope was resolved against the live
/// [`crate::subscriptions::source::BucketSourceTable::dirty_epoch`], no bucket
/// was created since (buckets are immortal and every `bucket_create` bumps the
/// epoch), so the in-scope set is unchanged. We re-fetch each cached bucket's
/// source (O(in-scope), not O(all-ever-created)) and skip the
/// `list_bucket_ids()` ∩ predicate scan entirely.
///
/// Slow path: on a cache miss (first pull, or a new bucket bumped the epoch),
/// rebuild via `list_bucket_ids()` ∩ side-table predicate match, capped, and
/// store `(epoch, bucket_ids, truncated)` back. A new MATCHING bucket bumps the
/// epoch, forcing a rebuild that routes to it — AC2 auto-join preserved.
///
/// Returns the subscription's working offsets + rr cursor either way.
///
/// # Errors
/// [`IpcErrorCode::UnknownSubscription`] if `sub_id` is not in the registry.
fn scope_snapshot(state: &Arc<DaemonState>, sub_id: Uuid) -> Result<Scope, IpcError> {
    // Copy the subscription's predicate, offsets, rr cursor, and cached scope
    // under the lock.
    #[allow(clippy::type_complexity)]
    let (predicate, offsets, rr_start, cached): (
        Predicate,
        HashMap<BucketId, u64>,
        usize,
        Option<CachedScope>,
    ) = state.subscriptions.with_sub(sub_id, |s| {
        (
            s.predicate.clone(),
            s.offsets.clone(),
            s.rr_start,
            s.cached_scope.clone(),
        )
    })?;

    let epoch = state.sources.dirty_epoch();

    // Fast path: the cache is valid for the current epoch. Nothing was created
    // since it was built, so the in-scope set is identical; just re-resolve each
    // bucket's source (immortal side-table) and reuse the cached `truncated`.
    if let Some(c) = &cached
        && c.dirty_epoch == epoch
    {
        let mut buckets: Vec<ScopedBucket> = Vec::with_capacity(c.buckets.len());
        for &id in &c.buckets {
            // The side-table is immortal, so a cached id resolves; if it somehow
            // does not, skip it (matches the slow-path `continue`).
            if let Some(source) = state.sources.get(id) {
                buckets.push(ScopedBucket {
                    bucket_id: id,
                    source,
                });
            }
        }
        return Ok(Scope {
            buckets,
            offsets,
            rr_start,
            truncated: c.truncated,
        });
    }

    // Slow path: full rebuild — `list_bucket_ids()` ∩ predicate match, capped.
    let mut buckets: Vec<ScopedBucket> = Vec::new();
    let mut truncated = false;
    for id in state.buckets.list_bucket_ids() {
        let Some(source) = state.sources.get(id) else {
            continue;
        };
        if predicate.bucket_in_scope(id, &source) {
            if buckets.len() >= MAX_BUCKETS_PER_SUBSCRIPTION {
                truncated = true;
                break;
            }
            buckets.push(ScopedBucket {
                bucket_id: id,
                source,
            });
        }
    }
    // Deterministic order so rr rotation is stable across pulls.
    buckets.sort_by_key(|b| b.bucket_id.to_string());

    // Cache the resolved scope against the epoch it was built from. The next
    // pull reuses it while no bucket is created (epoch unchanged).
    let cached_ids: Vec<BucketId> = buckets.iter().map(|b| b.bucket_id).collect();
    let _ = state.subscriptions.with_sub_mut(sub_id, |s| {
        s.cached_scope = Some(CachedScope {
            dirty_epoch: epoch,
            buckets: cached_ids,
            truncated,
        });
    });

    Ok(Scope {
        buckets,
        offsets,
        rr_start,
        truncated,
    })
}

/// Map a `BucketError` to a typed `IpcError`.
fn map_bucket(e: terminal_commander_core::BucketError) -> IpcError {
    use terminal_commander_core::BucketError;
    match e {
        BucketError::NotFound(_) => IpcError::new(IpcErrorCode::BucketNotFound, e.to_string()),
        other => IpcError::new(IpcErrorCode::Internal, other.to_string()),
    }
}

/// The single-kind fast path uses `events_since`'s native `kind_filter`; the
/// multi-kind / no-kind path reads severity-only and post-filters by the kind
/// allowlist (the bucket read API takes ONE kind, but a predicate's `kind` is
/// an allowlist). `None` kind = any kind.
fn kind_filter_for(predicate: &Predicate) -> (Option<String>, Option<&[String]>) {
    match &predicate.kind {
        None => (None, None),
        Some(kinds) if kinds.len() == 1 => (Some(kinds[0].clone()), None),
        Some(kinds) => (None, Some(kinds.as_slice())),
    }
}

/// The fair, capped, eviction-clamped drain across the in-scope buckets.
struct Drained {
    events: Vec<SubEvent>,
    lagged: bool,
    /// New round-robin start (rotated by one for next pull).
    next_rr: usize,
}

/// Compute the lag-weighted per-bucket pull share for `drain_fair`
/// (subscriptions §3 step 6, Phase 3).
///
/// `backlog_i = tail_seq - eff_off` is each bucket's unread depth, measured at
/// the SAME clamp floor (`head_seq - 1`) the drain loop uses, so the two can
/// never disagree. The share is `per_i = max(1, cap * backlog_i / sum(backlog))`
/// — proportional to lag, but never zero (no starvation). When every backlog is
/// equal or zero this collapses to the flat round-robin share `max(1, cap / n)`,
/// preserving the AC4 behavior. The returned vector is indexed by the bucket's
/// position in `scope.buckets`.
fn lag_weighted_shares(
    state: &Arc<DaemonState>,
    scope: &Scope,
    offsets: &HashMap<BucketId, u64>,
    cap: usize,
) -> Result<Vec<usize>, IpcError> {
    let n = scope.buckets.len();
    let mut backlog: Vec<u64> = Vec::with_capacity(n);
    for scoped in &scope.buckets {
        let st = state.buckets.state(scoped.bucket_id).map_err(map_bucket)?;
        let off = offsets.get(&scoped.bucket_id).copied().unwrap_or(0);
        let floor = st.head_seq.saturating_sub(1);
        let eff_off = off.max(floor);
        backlog.push(st.tail_seq.saturating_sub(eff_off));
    }
    let total_backlog: u64 = backlog.iter().copied().sum();
    if total_backlog == 0 {
        // No backlog anywhere -> flat fallback (round-robin share).
        return Ok(vec![(cap / n).max(1); n]);
    }
    Ok(backlog
        .iter()
        .map(|&b| {
            let share = (cap as u128 * u128::from(b) / u128::from(total_backlog)) as usize;
            share.max(1)
        })
        .collect())
}
/// Drain matched events fair + capped (subscriptions §3 step 6 + AC4/AC12).
///
/// `offsets` is advanced in place to the last DELIVERED seq per bucket (the
/// `next_cursor` from `events_since`, which only counts events that passed
/// severity + the native single-kind filter). Trailing non-matching events are
/// re-scanned on the next pull but never re-delivered — lossless, not loss.
fn drain_fair(
    state: &Arc<DaemonState>,
    scope: &Scope,
    offsets: &mut HashMap<BucketId, u64>,
    predicate: &Predicate,
    cap: usize,
) -> Result<Drained, IpcError> {
    let n = scope.buckets.len();
    let mut events: Vec<SubEvent> = Vec::new();
    let mut lagged = false;
    if n == 0 || cap == 0 {
        return Ok(Drained {
            events,
            lagged,
            next_rr: 0,
        });
    }
    // Lag-weighted per-bucket share (Phase 3): a high-backlog bucket drains
    // more within one pull, while a quiet bucket is never starved (>= 1). The
    // cap remains a hard stop and the rr_start rotation breaks ties.
    let per_i = lag_weighted_shares(state, scope, offsets, cap)?;
    let (kind_native, kind_allow) = kind_filter_for(predicate);

    // Visit order rotated by rr_start so a flooding bucket cannot starve a
    // quiet one across pulls.
    let order: Vec<usize> = (0..n).map(|i| (scope.rr_start + i) % n).collect();

    'outer: loop {
        let mut progressed = false;
        for &i in &order {
            if events.len() >= cap {
                break 'outer;
            }
            let scoped = &scope.buckets[i];
            let bid = scoped.bucket_id;
            let st = state.buckets.state(bid).map_err(map_bucket)?;
            let off = offsets.entry(bid).or_insert(0);

            // AC12 eviction clamp: clamp to head_seq-1 (NOT head_seq), so the
            // survivor AT head_seq is still delivered. Dropped delta counts
            // only truly-lost events (stored_offset < seq < head_seq).
            let clamp_floor = st.head_seq.saturating_sub(1);
            if *off < clamp_floor {
                *off = clamp_floor;
                lagged = true;
            }
            // Bucket-level drop (eviction) is also a lag signal.
            if st.dropped_count > 0 {
                lagged = true;
            }

            let want = per_i[i].min(cap - events.len());
            if want == 0 {
                continue;
            }
            let resp = state
                .buckets
                .events_since(
                    bid,
                    &BucketReadRequest {
                        cursor: *off,
                        severity_min: predicate.severity_min,
                        kind_filter: kind_native.clone(),
                        limit: Some(want),
                    },
                )
                .map_err(map_bucket)?;

            // Advance to the last DELIVERED seq (`events_since` sets
            // `next_cursor` to the seq of the last event it actually returned
            // — one that passed severity + the native single-kind filter).
            // Events that FAIL those filters do NOT move the cursor, so any
            // trailing non-matching events are RE-SCANNED on the next read.
            // That is lossless, not loss: a re-scanned non-match is re-filtered
            // out (never re-delivered), and a multi-kind allowlist drop below
            // is likewise re-scanned but never re-delivered. The cursor only
            // ever advances past events we returned to the caller.
            if resp.next_cursor > *off {
                *off = resp.next_cursor;
            }

            let job_id = scoped.source.job_id;
            for ev in resp.events {
                // Multi-kind allowlist post-filter (native single-kind already
                // applied by events_since).
                if let Some(allow) = kind_allow
                    && !allow.contains(&ev.kind)
                {
                    continue;
                }
                if events.len() >= cap {
                    break 'outer;
                }
                let seq = ev.seq;
                events.push(SubEvent {
                    origin: EventOrigin {
                        bucket_id: bid,
                        job_id,
                        seq,
                    },
                    event: ev,
                });
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }

    Ok(Drained {
        events,
        lagged,
        next_rr: (scope.rr_start + 1) % n,
    })
}

/// Derive per-source liveness for every in-scope bucket (subscriptions §3
/// step 7 + MUST-ADD #3). Command and PTY sources read their runtime's
/// authoritative job state; file-watch present -> Running.
fn liveness_for(state: &Arc<DaemonState>, scope: &Scope) -> Vec<SourceLiveness> {
    use terminal_commander_ipc::ProbeKind;
    let mut out = Vec::with_capacity(scope.buckets.len());
    for scoped in &scope.buckets {
        let source = &scoped.source;
        let liveness = match source.kind {
            ProbeKind::Command => source.job_id.map_or(Liveness::Stopped, |job| {
                state.command.status(job).map_or(Liveness::Stopped, |s| {
                    crate::liveness::command_liveness(s.state, s.exit_code, s.signal)
                })
            }),
            // Source records intentionally outlive stopped watches. Consult
            // the runtime's live-handle registry so a retained subscription
            // observes the watch's transition to Stopped.
            ProbeKind::FileWatch => source.job_id.map_or(Liveness::Stopped, |watch_id| {
                if state.watch.is_live(watch_id) {
                    Liveness::Running
                } else {
                    Liveness::Stopped
                }
            }),
            // PTY bindings LINGER in the runtime's job ledger after exit
            // (like command's), so the authoritative terminal state is one
            // lookup away — mirror the command arm instead of reporting a
            // stopped PTY as Running.
            #[cfg(any(unix, windows))]
            ProbeKind::Pty => source
                .job_id
                .map_or(Liveness::Stopped, |job| state.pty.liveness(job)),
            // No PTY backend on this platform: a Pty source cannot exist,
            // but the match must stay exhaustive.
            #[cfg(not(any(unix, windows)))]
            ProbeKind::Pty => Liveness::Stopped,
        };
        out.push(SourceLiveness {
            bucket_id: scoped.bucket_id,
            job_id: source.job_id,
            probe_id: source.probe_id,
            liveness,
        });
    }
    out
}

/// Poll N pinned single-use `Notified` futures, completing when ANY fires.
/// Avoids a `futures`-crate dependency for `select_all`.
///
/// Correctness: each `Notified` is already ENROLLED (via `enable()`) before
/// this is polled, so a wake landing between enroll and the first poll is NOT
/// lost — `Notified` latches the notification and reports `Ready` on the next
/// poll.
fn poll_any_notified<N>(futs: &mut [Pin<Box<N>>], cx: &mut Context<'_>) -> Poll<()>
where
    N: Future<Output = ()>,
{
    for fut in futs.iter_mut() {
        if fut.as_mut().poll(cx).is_ready() {
            return Poll::Ready(());
        }
    }
    Poll::Pending
}

/// Multiplexed, lossless `subscription_pull`.
///
/// See the module docs for the enroll-before-recheck invariant. `timeout` is
/// honored as passed; the CALLER (Task 10) clamps it strictly below
/// `DRAIN_CEILING`.
///
/// # Errors
/// [`IpcErrorCode::UnknownSubscription`] if `sub_id` is unknown/expired (NEVER
/// returned as empty — registry-loss must not be mistaken for no-events).
pub async fn pull(
    state: &Arc<DaemonState>,
    sub_id: Uuid,
    max: usize,
    timeout: Duration,
    liveness_delta: bool,
) -> Result<PullOutcome, IpcError> {
    let cap = if max == 0 {
        DEFAULT_PULL_MAX
    } else {
        max.min(MAX_PULL_MAX)
    };
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        // (1) resolve + (2) snapshot scope. UnknownSubscription if gone.
        let mut scope = scope_snapshot(state, sub_id)?;
        let predicate = state
            .subscriptions
            .with_sub(sub_id, |s| s.predicate.clone())?;
        // (3) ENROLL: clone each in-scope bucket's Notify, keeping the waiter
        // set and the drain set (`scope.buckets`) in LOCKSTEP. If
        // `bucket_notify` fails for a bucket (only `NotFound`, near-impossible
        // since buckets are immortal), drop that bucket from BOTH sets so they
        // cannot desync — a bucket we cannot enroll must not be drained (a wake
        // on it would be lost). The drop is logged so it is observable rather
        // than silently swallowed.
        let mut notifies: Vec<Arc<Notify>> = Vec::with_capacity(scope.buckets.len());
        scope
            .buckets
            .retain(|b| match state.buckets.bucket_notify(b.bucket_id) {
                Ok(notify) => {
                    notifies.push(notify);
                    true
                }
                Err(e) => {
                    tracing::warn!(
                        bucket_id = %b.bucket_id,
                        sub_id = %sub_id,
                        "subscription_pull: skipping in-scope bucket — bucket_notify failed ({e}); \
                         dropped from both waiter and drain sets to avoid desync"
                    );
                    false
                }
            });
        let n = scope.buckets.len();

        // N == 0: nothing to enroll/drain. Sleep out the remaining time and
        // return idle + liveness (no ceil(max/0)).
        if n == 0 {
            tokio::time::sleep_until(deadline).await;
            let liveness = resolve_liveness_delta(state, sub_id, Vec::new(), liveness_delta);
            mark_pulled(state, sub_id);
            return Ok(PullOutcome::idle(liveness, false, scope.truncated));
        }

        // Build FRESH notified() futures and enable() them to enroll the
        // waiters BEFORE any read (the load-bearing enroll-before-recheck).
        let mut futs: Vec<Pin<Box<tokio::sync::futures::Notified<'_>>>> =
            notifies.iter().map(|n| Box::pin(n.notified())).collect();
        for f in &mut futs {
            f.as_mut().enable();
        }

        // (4) fast-path recheck (AFTER enroll): scan ALL in-scope offsets.
        let mut offsets = scope.offsets.clone();
        let drained = drain_fair(state, &scope, &mut offsets, &predicate, cap)?;
        if !drained.events.is_empty() {
            let liveness = liveness_for(state, &scope);
            let liveness = resolve_liveness_delta(state, sub_id, liveness, liveness_delta);
            commit(state, sub_id, &offsets, drained.next_rr);
            return Ok(PullOutcome {
                events: drained.events,
                liveness,
                lagged: drained.lagged,
                truncated: scope.truncated,
            });
        }

        // (5) slow path: race the enrolled futures against the remaining time.
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            let liveness = liveness_for(state, &scope);
            let liveness = resolve_liveness_delta(state, sub_id, liveness, liveness_delta);
            // No new events, but a clamp may have flagged lag.
            commit(state, sub_id, &offsets, scope.rr_start);
            mark_pulled(state, sub_id);
            return Ok(PullOutcome::idle(liveness, drained.lagged, scope.truncated));
        }
        let any = std::future::poll_fn(|cx| poll_any_notified(&mut futs, cx));
        if tokio::time::timeout(remaining, any).await.is_err() {
            // (6) timeout -> idle + liveness.
            let liveness = liveness_for(state, &scope);
            let liveness = resolve_liveness_delta(state, sub_id, liveness, liveness_delta);
            commit(state, sub_id, &offsets, scope.rr_start);
            mark_pulled(state, sub_id);
            return Ok(PullOutcome::idle(liveness, drained.lagged, scope.truncated));
        }

        // Woken (or spurious): drop the enrolled futures and loop. The next
        // iteration re-enrolls FRESH waiters before re-scanning, so an append
        // in the re-arm window is not lost. A spurious wake (no in-scope event)
        // re-enters and does NOT return a premature empty.
        //
        // No offsets are carried across iterations: the working `offsets` were a
        // clone of `scope.offsets`, and the only in-loop mutation is the
        // eviction clamp, which is IDEMPOTENT — `drain_fair` recomputes it from
        // each bucket's live `head_seq` every pass. The next iteration re-reads
        // committed offsets from the registry via `scope_snapshot`, so there is
        // nothing to persist here.
        drop(futs);
    }
}

/// Commit advanced offsets + the new rr cursor back into the subscription, and
/// stamp `last_pull_at`. Silently no-ops if the sub was closed mid-pull.
fn commit(
    state: &Arc<DaemonState>,
    sub_id: Uuid,
    offsets: &HashMap<BucketId, u64>,
    next_rr: usize,
) {
    let _ = state.subscriptions.with_sub_mut(sub_id, |s| {
        for (bid, off) in offsets {
            s.offsets.insert(*bid, *off);
        }
        s.rr_start = next_rr;
        s.last_pull_at = Some(std::time::SystemTime::now());
    });
}

/// Stamp `last_pull_at` without touching offsets. Silently no-ops if closed.
fn mark_pulled(state: &Arc<DaemonState>, sub_id: Uuid) {
    let _ = state.subscriptions.with_sub_mut(sub_id, |s| {
        s.last_pull_at = Some(std::time::SystemTime::now());
    });
}

/// Resolve the liveness a pull should report (US4 / FR-031).
///
/// With `delta` false, return the full snapshot unchanged (byte-identical
/// legacy behavior). With `delta` true, atomically — under one `with_sub_mut`,
/// the same lock the offset commit uses — diff `full` against the
/// subscription's stored `last_liveness`, overwrite the map with `full`, and
/// return ONLY the changed entries. An empty stored map (a subscription's first
/// pull, or the pull right after a seek clears it) is the baseline case: every
/// entry counts as changed, so the full snapshot is returned. The read-diff-
/// write is a single critical section, so a transition observed by this pull is
/// recorded before the next pull runs — no transition is skippable, and a
/// delivered transition is not repeated.
///
/// If the subscription was closed mid-pull the map cannot be recorded; the full
/// snapshot is returned rather than hiding liveness.
fn resolve_liveness_delta(
    state: &Arc<DaemonState>,
    sub_id: Uuid,
    full: Vec<SourceLiveness>,
    delta: bool,
) -> Vec<SourceLiveness> {
    if !delta {
        return full;
    }
    state
        .subscriptions
        .with_sub_mut(sub_id, |s| {
            let baseline = s.last_liveness.is_empty();
            let changed: Vec<SourceLiveness> = full
                .iter()
                .filter(|e| baseline || s.last_liveness.get(&e.bucket_id) != Some(&e.liveness))
                .cloned()
                .collect();
            s.last_liveness = full
                .iter()
                .map(|e| (e.bucket_id, e.liveness.clone()))
                .collect();
            changed
        })
        .unwrap_or(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DaemonConfig;
    use std::sync::atomic::{AtomicU64, Ordering};
    use terminal_commander_core::BucketConfig;
    use terminal_commander_ipc::ProbeKind;

    fn temp_data_dir() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "tc-pull-file-liveness-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn stopped_file_watch_reports_stopped_liveness() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime");
        runtime.block_on(async {
            let data = temp_data_dir();
            std::fs::create_dir_all(&data).expect("create test data dir");
            let watched = data.join("watch.log");
            std::fs::write(&watched, "ready\n").expect("create watched file");

            let state = Arc::new(
                DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).expect("bootstrap daemon"),
            );
            let (watch_id, bucket_id, probe_id) = state
                .watch
                .start(
                    watched.clone(),
                    BucketConfig::default(),
                    Vec::new(),
                    false,
                    None,
                )
                .expect("start file watch");
            let scope = Scope {
                buckets: vec![ScopedBucket {
                    bucket_id,
                    source: BucketSource {
                        kind: ProbeKind::FileWatch,
                        job_id: Some(watch_id),
                        probe_id: Some(probe_id),
                        path: Some(watched),
                        tag: None,
                    },
                }],
                offsets: HashMap::new(),
                rr_start: 0,
                truncated: false,
            };

            assert_eq!(liveness_for(&state, &scope)[0].liveness, Liveness::Running);
            state.watch.stop(watch_id).expect("stop file watch");
            assert_eq!(liveness_for(&state, &scope)[0].liveness, Liveness::Stopped);

            drop(state);
            let _ = std::fs::remove_dir_all(data);
        });
    }

    #[test]
    fn naturally_terminated_file_watch_reports_stopped_liveness() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime");
        runtime.block_on(async {
            let data = temp_data_dir();
            std::fs::create_dir_all(&data).expect("create test data dir");
            let watched = data.join("watch.log");
            std::fs::write(&watched, "ready\n").expect("create watched file");

            let state = Arc::new(
                DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).expect("bootstrap daemon"),
            );
            let (watch_id, bucket_id, probe_id) = state
                .watch
                .start(
                    watched.clone(),
                    BucketConfig::default(),
                    Vec::new(),
                    false,
                    None,
                )
                .expect("start file watch");
            let scope = Scope {
                buckets: vec![ScopedBucket {
                    bucket_id,
                    source: BucketSource {
                        kind: ProbeKind::FileWatch,
                        job_id: Some(watch_id),
                        probe_id: Some(probe_id),
                        path: Some(watched.clone()),
                        tag: None,
                    },
                }],
                offsets: HashMap::new(),
                rr_start: 0,
                truncated: false,
            };

            std::fs::remove_file(&watched).expect("remove watched file");
            std::fs::create_dir(&watched).expect("replace watched file with directory");

            let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
            while tokio::time::Instant::now() < deadline
                && liveness_for(&state, &scope)[0].liveness == Liveness::Running
            {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            assert_eq!(liveness_for(&state, &scope)[0].liveness, Liveness::Stopped);

            drop(state);
            let _ = std::fs::remove_dir_all(data);
        });
    }

    #[test]
    fn source_without_job_id_fails_safe_stopped() {
        let data = temp_data_dir();
        std::fs::create_dir_all(&data).expect("create test data dir");
        let state = Arc::new(
            DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).expect("bootstrap daemon"),
        );
        let scope = Scope {
            buckets: vec![ScopedBucket {
                bucket_id: BucketId::new(),
                source: BucketSource {
                    kind: ProbeKind::FileWatch,
                    job_id: None,
                    probe_id: Some(ProbeId::new()),
                    path: None,
                    tag: None,
                },
            }],
            offsets: HashMap::new(),
            rr_start: 0,
            truncated: false,
        };

        assert_eq!(liveness_for(&state, &scope)[0].liveness, Liveness::Stopped);

        drop(state);
        let _ = std::fs::remove_dir_all(data);
    }
}
