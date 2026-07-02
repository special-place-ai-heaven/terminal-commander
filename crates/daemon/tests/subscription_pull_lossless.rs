// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Task 8 adversarial tests for the multiplexed, lossless `subscription_pull`
//! engine. These are the load-bearing correctness tests:
//!
//! - AC3 no-lost-wakeup: an append after the pull is already enrolled+awaiting
//!   is delivered on the SAME pull (not lost to the timeout). A spurious wake
//!   (a non-matching append) re-enrolls fresh waiters, re-scans, and does NOT
//!   return a premature empty; a matching append in the re-arm window is
//!   delivered on the same pull.
//! - AC4 fairness + cap: a flooding bucket does not starve a quiet one; the
//!   quiet bucket's event appears in the SAME pull; `N > max` returns `<= max`;
//!   `N == 0` does not panic (no ceil(max/0)).
//! - AC12 eviction clamp off-by-one: after FIFO eviction the survivor at the
//!   post-eviction head is delivered EXACTLY once; `lagged` is set.
//! - AC7 unknown sub_id -> UnknownSubscription (never empty).
//!
//! The tests drive buckets + the source side-table + the registry directly
//! (no real subprocesses) so the ordering is deterministic.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use terminal_commander_core::{
    BucketConfig, BucketId, Captures, EventId, EventSource, FrameId, JobId, ProbeId, RuleId,
    RuleRef, Severity, SignalEvent, SourcePointer, SourceStream, SourceType,
};
use terminal_commander_ipc::{IpcErrorCode, Liveness, ProbeKind};
use terminal_commanderd::{
    BucketSource, DaemonConfig, DaemonState, Predicate, SourceSel, subscription_pull,
};
use uuid::Uuid;

fn tmp_data_dir(tag: &str) -> std::path::PathBuf {
    static C: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = C.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-subpull-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn boot(tag: &str) -> (std::path::PathBuf, Arc<DaemonState>) {
    let data = tmp_data_dir(tag);
    let cfg = DaemonConfig::defaults_in(&data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    (data, state)
}

/// Create a bucket with the given config and record a Command source for it so
/// a `sources: all` predicate routes to it. Returns the bucket id.
fn make_bucket(state: &Arc<DaemonState>, cfg: BucketConfig) -> (BucketId, JobId) {
    let bucket = BucketId::new();
    let job = JobId::new();
    state.buckets.create_bucket(bucket, cfg).unwrap();
    state.sources.record(
        bucket,
        BucketSource {
            kind: ProbeKind::Command,
            job_id: Some(job),
            probe_id: Some(ProbeId::new()),
            path: None,
            tag: None,
        },
    );
    (bucket, job)
}

/// Build a `SignalEvent` for a bucket (seq auto-assigned by `append`).
fn ev(bucket: BucketId, severity: Severity, kind: &str) -> SignalEvent {
    SignalEvent {
        event_id: EventId::new(),
        bucket_id: bucket,
        seq: 0,
        timestamp: time::OffsetDateTime::now_utc(),
        severity,
        kind: kind.to_owned(),
        summary: format!("{kind} summary"),
        rule: Some(RuleRef {
            id: RuleId::new(),
            version: 1,
        }),
        source: EventSource {
            probe_id: ProbeId::new(),
            source_type: SourceType::Process,
            stream: SourceStream::Stderr,
            job_id: None,
        },
        captures: Some(Captures::new()),
        // Pointer present so the TC02 invariant holds for severity >= Medium.
        pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
        pointer_unavailable_reason: None,
        tags: None,
        count: 1,
        first_seen: None,
        last_seen: None,
        suppressed: false,
    }
}

fn append(state: &Arc<DaemonState>, bucket: BucketId, severity: Severity, kind: &str) -> u64 {
    state
        .buckets
        .append(bucket, ev(bucket, severity, kind))
        .unwrap()
}

/// Open a `sources: all` subscription with the given severity floor. Offsets
/// start empty (drain initializes them to 0, the pre-first-append cursor).
fn open_all(state: &Arc<DaemonState>, severity_min: Option<Severity>) -> Uuid {
    let predicate = Predicate {
        severity_min,
        kind: None,
        sources: SourceSel::All,
        tag: None,
    };
    state.subscriptions.open(predicate, HashMap::new()).unwrap()
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

// ---------------------------------------------------------------------------
// AC3: no lost wakeup (enroll-before-recheck) + spurious-wake re-enroll.
// ---------------------------------------------------------------------------

#[test]
fn pull_fast_path_returns_already_present_event() {
    let (data, state) = boot("fastpath");
    rt().block_on(async {
        let (bucket, _job) = make_bucket(&state, BucketConfig::default());
        let sub = open_all(&state, Some(Severity::High));
        // Event present BEFORE the pull -> fast-path returns it immediately.
        append(&state, bucket, Severity::High, "error");
        let out = subscription_pull(&state, sub, 10, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(out.events.len(), 1, "fast-path delivers the present event");
        assert_eq!(out.events[0].origin.bucket_id, bucket);
        assert_eq!(out.events[0].event.severity, Severity::High);
    });
    cleanup(&data);
}

#[test]
fn pull_wakes_on_append_after_enrollment_not_timeout() {
    // AC3 core: the pull enrolls on an empty in-scope bucket, blocks in the
    // select, and an append landing AFTER enrollment is delivered on the same
    // pull WELL before the (long) timeout. If enroll-before-recheck were
    // broken (read before enable, or create+pin without enable), the wake on
    // the permit-less notify_waiters() would be lost and the pull would only
    // return at the 5s timeout with empty events.
    let (data, state) = boot("wake");
    rt().block_on(async {
        let (bucket, _job) = make_bucket(&state, BucketConfig::default());
        let sub = open_all(&state, Some(Severity::High));

        let st = Arc::clone(&state);
        let started = Instant::now();
        let handle =
            tokio::spawn(
                async move { subscription_pull(&st, sub, 10, Duration::from_secs(5)).await },
            );

        // Give the pull time to reach its enroll + await. The fast-path read
        // finds nothing (empty bucket), so it parks in the select.
        tokio::time::sleep(Duration::from_millis(150)).await;
        // Append AFTER the pull is enrolled and awaiting.
        append(&state, bucket, Severity::High, "panic");

        let out = handle.await.unwrap().unwrap();
        let elapsed = started.elapsed();
        assert_eq!(out.events.len(), 1, "appended event delivered on same pull");
        assert_eq!(out.events[0].event.kind, "panic");
        assert!(
            elapsed < Duration::from_secs(4),
            "woke via notify, not the 5s timeout (elapsed {elapsed:?})"
        );
    });
    cleanup(&data);
}

#[test]
fn pull_spurious_wake_reenrolls_and_delivers_later_match() {
    // AC3 second half: a non-matching append (below the severity floor) fires
    // notify_waiters() and wakes the pull. The pull re-scans, finds NO in-scope
    // match, and MUST re-enter the loop (re-enroll fresh waiters) rather than
    // return a premature empty. A subsequent HIGH-sev append is then delivered
    // on the SAME pull. A buggy engine that returns empty on the spurious wake
    // would complete the spawned task before the high-sev append and fail the
    // non-empty assertion.
    let (data, state) = boot("spurious");
    rt().block_on(async {
        let (bucket, _job) = make_bucket(&state, BucketConfig::default());
        let sub = open_all(&state, Some(Severity::High));

        let st = Arc::clone(&state);
        let handle =
            tokio::spawn(
                async move { subscription_pull(&st, sub, 10, Duration::from_secs(5)).await },
            );

        tokio::time::sleep(Duration::from_millis(150)).await;
        // Spurious wake: a low-severity event that the predicate rejects.
        append(&state, bucket, Severity::Low, "progress");
        // Let the pull process the spurious wake and re-enroll.
        tokio::time::sleep(Duration::from_millis(150)).await;
        // The real match.
        append(&state, bucket, Severity::High, "error");

        let out = handle.await.unwrap().unwrap();
        assert_eq!(
            out.events.len(),
            1,
            "spurious wake did not return a premature empty; high-sev delivered"
        );
        assert_eq!(out.events[0].event.severity, Severity::High);
        assert_eq!(out.events[0].event.kind, "error");
    });
    cleanup(&data);
}

// ---------------------------------------------------------------------------
// AC4: fairness + hard cap.
// ---------------------------------------------------------------------------

#[test]
fn pull_flood_does_not_starve_quiet_bucket() {
    // Two in-scope buckets, max=4. One floods (10 events), the other has a
    // single event. The quiet bucket's lone event MUST appear in the SAME pull
    // (not starved by the flood). The flood fills the remaining capacity but
    // the total is capped at max.
    let (data, state) = boot("fair");
    rt().block_on(async {
        let (flood, _j1) = make_bucket(&state, BucketConfig::default());
        let (quiet, _j2) = make_bucket(&state, BucketConfig::default());
        let sub = open_all(&state, Some(Severity::High));

        for _ in 0..10 {
            append(&state, flood, Severity::High, "error");
        }
        append(&state, quiet, Severity::High, "error");

        let out = subscription_pull(&state, sub, 4, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(out.events.len() <= 4, "hard cap honored (<= max)");
        let from_quiet = out
            .events
            .iter()
            .filter(|e| e.origin.bucket_id == quiet)
            .count();
        let from_flood = out
            .events
            .iter()
            .filter(|e| e.origin.bucket_id == flood)
            .count();
        assert_eq!(from_quiet, 1, "quiet bucket not starved by the flood");
        // Flood is bounded by the cap minus the quiet bucket's contribution; it
        // never consumes ALL the capacity to starve the quiet bucket.
        assert!(from_flood <= 3, "flood bounded by cap (got {from_flood})");
        assert!(from_flood >= 1, "flood also delivers within the cap");
    });
    cleanup(&data);
}

#[test]
fn pull_two_floods_split_fairly_per_share() {
    // Two flooding buckets, max=4 -> per-bucket per-pass share = max(1, 4/2)=2.
    // Each flood has 10 matching events. A single pull returns exactly 2 from
    // EACH (the share), proving neither monopolizes the cap within one pull.
    let (data, state) = boot("twoflood");
    rt().block_on(async {
        let (a, _ja) = make_bucket(&state, BucketConfig::default());
        let (b, _jb) = make_bucket(&state, BucketConfig::default());
        let sub = open_all(&state, Some(Severity::High));
        for _ in 0..10 {
            append(&state, a, Severity::High, "error");
            append(&state, b, Severity::High, "error");
        }

        let out = subscription_pull(&state, sub, 4, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(out.events.len(), 4, "cap honored exactly");
        let from_a = out
            .events
            .iter()
            .filter(|e| e.origin.bucket_id == a)
            .count();
        let from_b = out
            .events
            .iter()
            .filter(|e| e.origin.bucket_id == b)
            .count();
        assert_eq!(from_a, 2, "bucket A gets its fair share (no monopoly)");
        assert_eq!(from_b, 2, "bucket B gets its fair share (no monopoly)");
    });
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread")]
async fn pull_proportional_share_favors_high_backlog_bucket() {
    // Phase 3 lag-weighted fairness: A has a LARGE backlog (100 unread), B a
    // SMALLER one (20 unread). Both exceed the FLAT per-bucket share
    // (max/n = 10), so the OLD flat policy would split the cap EQUALLY (10/10)
    // and fail the strict inequality below. Lag-weighting instead gives A a
    // strictly larger share. One pull with max=20:
    //   - A receives strictly MORE than B (proportional to backlog),
    //   - total <= 20 (the hard cap still bounds the pull),
    //   - B is NOT starved (>= 1 event).
    let (data, state) = boot("proportional");
    {
        let (big, _ja) = make_bucket(&state, BucketConfig::default());
        let (small, _jb) = make_bucket(&state, BucketConfig::default());
        let sub = open_all(&state, Some(Severity::High));
        for _ in 0..100 {
            append(&state, big, Severity::High, "error");
        }
        for _ in 0..20 {
            append(&state, small, Severity::High, "error");
        }

        let out = subscription_pull(&state, sub, 20, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(out.events.len() <= 20, "hard cap honored (<= max)");
        let from_big = out
            .events
            .iter()
            .filter(|e| e.origin.bucket_id == big)
            .count();
        let from_small = out
            .events
            .iter()
            .filter(|e| e.origin.bucket_id == small)
            .count();
        assert!(
            from_big > from_small,
            "high-backlog bucket gets a strictly larger share (big={from_big}, small={from_small})"
        );
        assert!(
            from_small >= 1,
            "low-backlog bucket is never starved (small={from_small})"
        );
    }
    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread")]
async fn pull_equal_backlogs_behave_like_round_robin() {
    // Equal backlogs -> roughly equal shares (the AC4 round-robin behavior is
    // preserved). Two buckets with equal backlog, max=4 -> 2 and 2.
    let (data, state) = boot("equalbacklog");
    {
        let (a, _ja) = make_bucket(&state, BucketConfig::default());
        let (b, _jb) = make_bucket(&state, BucketConfig::default());
        let sub = open_all(&state, Some(Severity::High));
        for _ in 0..10 {
            append(&state, a, Severity::High, "error");
            append(&state, b, Severity::High, "error");
        }

        let out = subscription_pull(&state, sub, 4, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(out.events.len(), 4, "cap honored exactly");
        let from_a = out
            .events
            .iter()
            .filter(|e| e.origin.bucket_id == a)
            .count();
        let from_b = out
            .events
            .iter()
            .filter(|e| e.origin.bucket_id == b)
            .count();
        let diff = from_a.abs_diff(from_b);
        assert!(
            diff <= 1,
            "equal backlogs -> roughly equal shares (a={from_a}, b={from_b})"
        );
        assert!(from_a >= 1 && from_b >= 1, "neither bucket starved");
    }
    cleanup(&data);
}

#[test]
fn pull_n_greater_than_max_returns_at_most_max() {
    // N (in-scope buckets) > max. Each bucket has one matching event. A single
    // pull returns <= max events (per-bucket share = max(1, max/N) = 1, stop at
    // the running total). No panic, no ceil(max/0).
    let (data, state) = boot("ngtmax");
    rt().block_on(async {
        let max = 5usize;
        let n = 12usize;
        for _ in 0..n {
            let (b, _j) = make_bucket(&state, BucketConfig::default());
            append(&state, b, Severity::High, "error");
        }
        let sub = open_all(&state, Some(Severity::High));

        let out = subscription_pull(&state, sub, max, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(
            out.events.len() <= max,
            "N>max returns <= max (got {})",
            out.events.len()
        );
        assert!(!out.events.is_empty(), "some events delivered");
    });
    cleanup(&data);
}

#[test]
fn pull_n_zero_returns_idle_without_panic() {
    // No in-scope buckets (no sources recorded) -> idle empty + liveness, no
    // division-by-zero. Use a short timeout so the test is fast.
    let (data, state) = boot("nzero");
    rt().block_on(async {
        let sub = open_all(&state, Some(Severity::High));
        let out = subscription_pull(&state, sub, 10, Duration::from_millis(200))
            .await
            .unwrap();
        assert!(out.events.is_empty(), "no buckets -> no events");
        assert!(out.liveness.is_empty(), "no in-scope sources");
    });
    cleanup(&data);
}

// ---------------------------------------------------------------------------
// AC12: eviction clamp off-by-one.
// ---------------------------------------------------------------------------

#[test]
fn pull_eviction_clamp_delivers_survivor_exactly_once_and_flags_lagged() {
    // A small ring (max_events=2). Append 5 high-sev events: seqs 1..=5, but
    // only the last 2 survive (head_seq=4, tail_seq=5). The subscription's
    // offset is still 0 (never pulled). The clamp sets offset to head_seq-1=3,
    // so events_since (strict seq>cursor) delivers seq 4 and 5 EXACTLY once.
    // Clamping to head_seq (4) would skip the survivor at seq 4.
    let (data, state) = boot("evict");
    rt().block_on(async {
        let cfg = BucketConfig {
            max_events: 2,
            ttl: BucketConfig::default().ttl,
        };
        let (bucket, _job) = make_bucket(&state, cfg);
        let sub = open_all(&state, Some(Severity::High));

        for _ in 0..5 {
            append(&state, bucket, Severity::High, "error");
        }
        let st = state.buckets.state(bucket).unwrap();
        assert_eq!(st.head_seq, 4, "oldest surviving seq is 4");
        assert_eq!(st.tail_seq, 5, "newest seq is 5");
        assert!(st.dropped_count >= 3, "3 events evicted");

        let out = subscription_pull(&state, sub, 50, Duration::from_secs(5))
            .await
            .unwrap();
        let seqs: Vec<u64> = out.events.iter().map(|e| e.origin.seq).collect();
        assert_eq!(
            seqs,
            vec![4, 5],
            "survivors delivered exactly once, in order"
        );
        assert!(out.lagged, "eviction past the offset flags lagged");

        // A second pull after committing offset=5 finds nothing new.
        let out2 = subscription_pull(&state, sub, 50, Duration::from_millis(200))
            .await
            .unwrap();
        assert!(out2.events.is_empty(), "no double-delivery of the survivor");
    });
    cleanup(&data);
}

// ---------------------------------------------------------------------------
// AC7: unknown sub_id -> typed error, never empty.
// ---------------------------------------------------------------------------

#[test]
fn pull_unknown_sub_is_typed_error_never_empty() {
    let (data, state) = boot("unknown");
    rt().block_on(async {
        let bogus = Uuid::new_v4();
        let err = subscription_pull(&state, bogus, 10, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert_eq!(
            err.code,
            IpcErrorCode::UnknownSubscription,
            "unknown sub_id is a typed error, not empty+liveness"
        );
    });
    cleanup(&data);
}

#[test]
fn pull_closed_sub_mid_session_is_unknown() {
    let (data, state) = boot("closed");
    rt().block_on(async {
        let sub = open_all(&state, Some(Severity::High));
        assert!(state.subscriptions.close(sub), "sub closed");
        let err = subscription_pull(&state, sub, 10, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert_eq!(err.code, IpcErrorCode::UnknownSubscription);
    });
    cleanup(&data);
}

// ---------------------------------------------------------------------------
// Consumer isolation through the pull path (AC8 at the engine level).
// ---------------------------------------------------------------------------

#[test]
fn two_subs_same_predicate_have_independent_pull_offsets() {
    let (data, state) = boot("isolation");
    rt().block_on(async {
        let (bucket, _job) = make_bucket(&state, BucketConfig::default());
        let a = open_all(&state, Some(Severity::High));
        let b = open_all(&state, Some(Severity::High));
        append(&state, bucket, Severity::High, "error");

        // A drains the event.
        let out_a = subscription_pull(&state, a, 10, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(out_a.events.len(), 1, "A sees the event");

        // B's offsets are independent: B still sees the same event.
        let out_b = subscription_pull(&state, b, 10, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(out_b.events.len(), 1, "B independently sees the same event");
    });
    cleanup(&data);
}

#[test]
fn pull_auto_joins_future_bucket() {
    // AC2: a bucket created AFTER subscription_open is auto-joined on the next
    // pull's routing rebuild (sources: all re-reads the side-table), and its
    // events are delivered with no full-ring replay.
    let (data, state) = boot("autojoin");
    rt().block_on(async {
        let sub = open_all(&state, Some(Severity::High));
        // No buckets at open time. Now create one and append.
        let (bucket, _job) = make_bucket(&state, BucketConfig::default());
        append(&state, bucket, Severity::High, "error");

        let out = subscription_pull(&state, sub, 10, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            out.events.len(),
            1,
            "future bucket auto-joined and delivered"
        );
        assert_eq!(out.events[0].origin.bucket_id, bucket);
    });
    cleanup(&data);
}

// ---------------------------------------------------------------------------
// Dirty-epoch scope cache (subscriptions §1, LOAD-BEARING). The cache only
// skips a routing rebuild when the source side-table's dirty epoch is
// unchanged; a new bucket bumps the epoch and forces a rebuild (auto-join).
// ---------------------------------------------------------------------------

#[test]
fn pull_caches_scope_and_reuses_when_dirty_epoch_unchanged() {
    // (b) Cache reuse: after the first pull, the subscription holds a cached
    // scope keyed to the live dirty_epoch. With no new buckets created, the
    // epoch is unchanged, so a repeat pull reuses the SAME cache (same epoch,
    // same bucket set) and returns the same scope behaviorally.
    let (data, state) = boot("cache-reuse");
    rt().block_on(async {
        let (b1, _j1) = make_bucket(&state, BucketConfig::default());
        let (b2, _j2) = make_bucket(&state, BucketConfig::default());
        let sub = open_all(&state, Some(Severity::High));

        // No cache yet (never pulled).
        let pre = state
            .subscriptions
            .with_sub(sub, |s| s.cached_scope.clone())
            .unwrap();
        assert!(pre.is_none(), "no cached scope before the first pull");

        // First pull populates the cache (no events present -> short timeout).
        let epoch_before = state.sources.dirty_epoch();
        let out1 = subscription_pull(&state, sub, 10, Duration::from_millis(150))
            .await
            .unwrap();
        assert!(out1.events.is_empty(), "no events appended yet");

        let cached = state
            .subscriptions
            .with_sub(sub, |s| s.cached_scope.clone())
            .unwrap()
            .expect("first pull populates the cache");
        assert_eq!(
            cached.dirty_epoch, epoch_before,
            "cache is keyed to the dirty epoch it was built from"
        );
        let mut cached_ids = cached.buckets.clone();
        cached_ids.sort_by_key(ToString::to_string);
        let mut want_ids = vec![b1, b2];
        want_ids.sort_by_key(ToString::to_string);
        assert_eq!(cached_ids, want_ids, "both in-scope buckets cached");
        assert!(!cached.truncated, "well under the bucket cap");

        // Second pull with NO new buckets: epoch unchanged -> cache reused.
        assert_eq!(
            state.sources.dirty_epoch(),
            epoch_before,
            "no record() between pulls, so the epoch is unchanged"
        );
        let out2 = subscription_pull(&state, sub, 10, Duration::from_millis(150))
            .await
            .unwrap();
        assert!(out2.events.is_empty(), "still no events");
        // Liveness still covers both in-scope buckets via the reused cache.
        assert_eq!(
            out2.liveness.len(),
            2,
            "reused cache still resolves both in-scope sources"
        );
        let cached2 = state
            .subscriptions
            .with_sub(sub, |s| s.cached_scope.clone())
            .unwrap()
            .expect("cache still present");
        assert_eq!(
            cached2.dirty_epoch, epoch_before,
            "reused cache keeps the same epoch (no rebuild happened)"
        );

        // Now drain an event from a cached bucket to prove the reused scope is
        // behaviorally identical to a fresh rebuild.
        append(&state, b1, Severity::High, "error");
        let out3 = subscription_pull(&state, sub, 10, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(out3.events.len(), 1, "reused-cache scope still delivers");
        assert_eq!(out3.events[0].origin.bucket_id, b1);
    });
    cleanup(&data);
}

#[test]
fn pull_cache_invalidates_on_new_bucket_and_auto_joins() {
    // (a) AC2 preserved WITH the cache: a matching bucket created AFTER the
    // first pull (which populated the cache) bumps the side-table's dirty
    // epoch, invalidating the cache. The next pull rebuilds and routes to the
    // new bucket, delivering its event (auto-join survives the cache).
    let (data, state) = boot("cache-invalidate");
    rt().block_on(async {
        let (b1, _j1) = make_bucket(&state, BucketConfig::default());
        let sub = open_all(&state, Some(Severity::High));

        // First pull populates the cache against the current epoch (1 bucket).
        let out1 = subscription_pull(&state, sub, 10, Duration::from_millis(150))
            .await
            .unwrap();
        assert!(out1.events.is_empty());
        let cached = state
            .subscriptions
            .with_sub(sub, |s| s.cached_scope.clone())
            .unwrap()
            .expect("cache populated");
        assert_eq!(cached.buckets, vec![b1], "only b1 cached at first pull");
        let epoch_after_first = cached.dirty_epoch;

        // Create a NEW matching bucket: record() bumps the dirty epoch, so the
        // cache (keyed to the old epoch) is now stale.
        let (b2, _j2) = make_bucket(&state, BucketConfig::default());
        assert!(
            state.sources.dirty_epoch() > epoch_after_first,
            "new bucket bumped the dirty epoch -> cache stale"
        );
        append(&state, b2, Severity::High, "error");

        // Next pull MUST rebuild (epoch changed) and auto-join b2.
        let out2 = subscription_pull(&state, sub, 10, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            out2.events.len(),
            1,
            "new bucket auto-joined with the cache"
        );
        assert_eq!(out2.events[0].origin.bucket_id, b2);

        // The cache was refreshed to the new epoch and now holds both buckets.
        let refreshed = state
            .subscriptions
            .with_sub(sub, |s| s.cached_scope.clone())
            .unwrap()
            .expect("cache refreshed");
        assert_eq!(
            refreshed.dirty_epoch,
            state.sources.dirty_epoch(),
            "rebuild re-keyed the cache to the live epoch"
        );
        let mut got = refreshed.buckets;
        got.sort_by_key(ToString::to_string);
        let mut want = vec![b1, b2];
        want.sort_by_key(ToString::to_string);
        assert_eq!(got, want, "refreshed cache holds both buckets");
    });
    cleanup(&data);
}

// ---------------------------------------------------------------------------
// Dogfood regression (2026-07-02): a Pty source's liveness must come from the
// PTY runtime's job ledger, never a hardcoded Running. A job the runtime does
// not know reports Stopped (the `PtyRuntime::liveness` contract for a
// dropped/forgotten record); the pre-fix arm reported Running unconditionally,
// so a stopped PTY looked alive in every subsequent pull.
// ---------------------------------------------------------------------------

#[cfg(any(unix, windows))]
#[test]
fn pull_liveness_pty_source_not_hardcoded_running() {
    let (data, state) = boot("pty-liveness");
    rt().block_on(async {
        let bucket = BucketId::new();
        let job = JobId::new(); // never started by the PTY runtime
        state
            .buckets
            .create_bucket(bucket, BucketConfig::default())
            .unwrap();
        state.sources.record(
            bucket,
            BucketSource {
                kind: ProbeKind::Pty,
                job_id: Some(job),
                probe_id: Some(ProbeId::new()),
                path: None,
                tag: None,
            },
        );
        let sub = open_all(&state, Some(Severity::High));
        append(&state, bucket, Severity::High, "error");
        let out = subscription_pull(&state, sub, 10, Duration::from_secs(5))
            .await
            .unwrap();
        let src = out
            .liveness
            .iter()
            .find(|s| s.job_id == Some(job))
            .expect("pty source liveness present");
        assert!(
            matches!(src.liveness, Liveness::Stopped),
            "unknown pty job must report the runtime's answer (Stopped), not a hardcoded Running"
        );
    });
    cleanup(&data);
}
