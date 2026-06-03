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
use terminal_commander_ipc::{IpcErrorCode, ProbeKind};
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
