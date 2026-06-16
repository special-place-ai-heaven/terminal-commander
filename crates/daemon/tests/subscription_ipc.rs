// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Subscriptions Phase 1 end-to-end daemon IPC tests
//! (`subscription_open/pull/list/close`). Stands up the real UDS IPC server
//! and drives the multiplexed pull through `DaemonClient`.
//!
//! - AC1: open `{severity_min: high, sources: all}`, start two noisy
//!   commands, pull returns high-sev events from BOTH, tagged + bounded.
//! - AC5: an idle pull returns SUCCESS empty events + liveness, never error.
//! - AC7: pull an unknown sub_id returns `UnknownSubscription`.
//! - AC8: two opens with the same predicate get DISTINCT sub_ids with
//!   independent offsets.
//!
//! Linux/WSL only (UDS).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    BucketConfig, BucketId, Captures, ContextHint, EventId, EventSource, FrameId, ProbeId,
    RuleDefinition, RuleId, RuleRef, RuleStatus, RuleType, Severity, SignalEvent, SourcePointer,
    SourceStream, SourceType,
};
use terminal_commander_ipc::ProbeKind;
use terminal_commanderd::{
    BucketSource, CommandStartParams, DaemonClient, DaemonConfig, DaemonState, IpcErrorCode,
    IpcRequest, IpcResponse, IpcServer, ServerHandle, SubscriptionListParams,
    SubscriptionOpenParams, SubscriptionPredicate, SubscriptionPullParams, SubscriptionSeekParams,
    SubscriptionSourceSel,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-sub-ipc-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn build_server(data: &std::path::Path) -> (Arc<DaemonState>, ServerHandle) {
    let cfg = DaemonConfig::defaults_in(data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    (state, handle)
}

/// A HIGH-severity keyword rule so its events pass `severity_min: high`
/// while clean-exit lifecycle markers (`command_exited`, Low) are filtered.
fn high_sev_keyword_rule() -> RuleDefinition {
    RuleDefinition {
        id: "sub.needle".to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::High,
        event_kind: "needle_hit".to_owned(),
        stream: Some(SourceStream::Stdout),
        description: None,
        pattern: None,
        keywords: Some(vec!["NEEDLE".to_owned()]),
        captures: vec![],
        summary_template: "needle".to_owned(),
        tags: vec![],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

/// A command that prints the needle keyword a few times then exits.
fn noisy_start_params() -> CommandStartParams {
    // TC-2: each call is a DISTINCT logical start, so it carries a fresh
    // dedup_nonce (mirroring the real adapter). Without it, two identical
    // nonce-less same-peer starts within the 3s fallback window would
    // collapse to ONE job; tests here start two and expect two buckets.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    CommandStartParams {
        environment: None,
        // `printf` (NOT a shell) emits several distinct NEEDLE lines so the
        // keyword rule produces multiple high-sev events without dedupe
        // collapsing them. The shell-bridge guard denies sh/bash/etc. by
        // basename (even absolute paths), so a real shell is not an option;
        // `printf` is a plain coreutil and resolves via PATH.
        argv: vec![
            "printf".to_owned(),
            "NEEDLE a\nNEEDLE b\nNEEDLE c\nNEEDLE d\n".to_owned(),
        ],
        cwd: None,
        env: Vec::new(),
        bucket_config: None,
        rules: vec![high_sev_keyword_rule()],
        grace_ms: Some(2_000),
        tag: None,
        dedup_nonce: Some(format!("sub-ipc-noisy-{n}")),
        strip_ansi: true,
    }
}

/// Same noisy command as [`noisy_start_params`], but stamped with a per-bucket
/// `tag` so a tag predicate can AND-filter to (or away from) it.
fn noisy_start_params_with_tag(tag: Option<&str>) -> CommandStartParams {
    CommandStartParams {
        tag: tag.map(str::to_owned),
        ..noisy_start_params()
    }
}

/// Build a minimal high-sev `SignalEvent` for direct bucket appends in the
/// seek tests (seq is auto-assigned by the store). Mirrors the lossless
/// engine's builder so the seek tests can control head/tail deterministically.
fn seek_test_event(bucket: BucketId) -> SignalEvent {
    SignalEvent {
        event_id: EventId::new(),
        bucket_id: bucket,
        seq: 0,
        timestamp: time::OffsetDateTime::now_utc(),
        severity: Severity::High,
        kind: "seek_probe".to_owned(),
        summary: "seek probe summary".to_owned(),
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
        pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
        pointer_unavailable_reason: None,
        tags: None,
        count: 1,
        first_seen: None,
        last_seen: None,
        suppressed: false,
    }
}

/// Create a bucket with the given config, record a Command source so a
/// `sources: all` predicate routes to it, and append `n` high-sev events.
/// Returns `(bucket_id, last_seq)`.
fn seed_bucket(state: &Arc<DaemonState>, cfg: BucketConfig, n: u64) -> (BucketId, u64) {
    let bucket = BucketId::new();
    state.buckets.create_bucket(bucket, cfg).unwrap();
    state.sources.record(
        bucket,
        BucketSource {
            kind: ProbeKind::Command,
            job_id: Some(terminal_commander_core::JobId::new()),
            probe_id: Some(ProbeId::new()),
            path: None,
            tag: None,
        },
    );
    let mut last = 0;
    for _ in 0..n {
        last = state
            .buckets
            .append(bucket, seek_test_event(bucket))
            .unwrap();
    }
    (bucket, last)
}

/// Pull repeatedly within a bounded retry budget, collecting delivered event
/// seqs in arrival order. Stops as soon as a pull returns no events (the
/// engine drained everything in scope for this offset).
async fn drain_seqs(client: &DaemonClient, sub_id: &str, base_correlation: u64) -> Vec<u64> {
    let mut seqs = Vec::new();
    for i in 0..20u64 {
        let pull = client
            .call(
                base_correlation + i,
                IpcRequest::SubscriptionPull(SubscriptionPullParams {
                    sub_id: sub_id.to_owned(),
                    max: Some(50),
                    timeout_ms: Some(500),
                }),
            )
            .await
            .expect("subscription_pull");
        let r = match pull {
            IpcResponse::SubscriptionPull(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        if r.events.is_empty() {
            break;
        }
        for ev in &r.events {
            seqs.push(ev.event.seq);
        }
    }
    seqs
}

#[test]
fn ac1_pull_returns_high_sev_events_from_both_noisy_commands_tagged_and_bounded() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("ac1");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(10));

        // Open: severity_min high, sources all.
        let open = client
            .call(
                1,
                IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                    predicate: SubscriptionPredicate {
                        severity_min: Some(Severity::High),
                        kind: None,
                        sources: SubscriptionSourceSel::All,
                        tag: None,
                    },
                }),
            )
            .await
            .expect("subscription_open");
        let sub_id = match open {
            IpcResponse::SubscriptionOpen(r) => r.sub_id,
            other => panic!("unexpected: {other:?}"),
        };

        // Start two noisy commands (each emits NEEDLE -> high-sev events).
        for id in [2u64, 3u64] {
            let resp = client
                .call(id, IpcRequest::CommandStartCombed(noisy_start_params()))
                .await
                .expect("command_start_combed");
            assert!(matches!(resp, IpcResponse::CommandStartCombed(_)));
        }

        // Pull, retrying within a bounded budget until both buckets show.
        let mut buckets_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut total_events = 0usize;
        for id in 4u64..40 {
            let pull = client
                .call(
                    id,
                    IpcRequest::SubscriptionPull(SubscriptionPullParams {
                        sub_id: sub_id.clone(),
                        max: Some(50),
                        timeout_ms: Some(1_500),
                    }),
                )
                .await
                .expect("subscription_pull");
            let r = match pull {
                IpcResponse::SubscriptionPull(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            // Bounded by max.
            assert!(r.events.len() <= 50, "pull must be bounded by max");
            for ev in &r.events {
                // Every delivered event is high-sev and tagged by source.
                assert!(
                    ev.event.severity >= Severity::High,
                    "severity_min must hold: {:?}",
                    ev.event.severity
                );
                assert_eq!(ev.bucket_id, ev.event.bucket_id, "origin tag matches event");
                buckets_seen.insert(ev.bucket_id.to_string());
                total_events += 1;
            }
            if buckets_seen.len() >= 2 {
                break;
            }
        }

        assert!(total_events > 0, "must deliver some high-sev needle events");
        assert!(
            buckets_seen.len() >= 2,
            "AC1: high-sev events must arrive from BOTH command buckets; saw {}",
            buckets_seen.len()
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Phase 3 tags: a subscription opened with `tag: Some("deploy")` receives
/// events ONLY from a probe started with that tag; an untagged probe's events
/// are excluded. Auto-join still applies (the tagged probe may start after the
/// open and is picked up on the next pull because `record` bumps the dirty
/// epoch and the routing rebuild re-evaluates the tag).
#[test]
fn tagged_probe_matched_only_by_matching_tag_predicate() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("tagpred");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(10));

        // Open a tag-filtered subscription BEFORE any probe exists, proving
        // auto-join routes the future tagged probe.
        let open = client
            .call(
                1,
                IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                    predicate: SubscriptionPredicate {
                        severity_min: Some(Severity::High),
                        kind: None,
                        sources: SubscriptionSourceSel::All,
                        tag: Some("deploy".to_owned()),
                    },
                }),
            )
            .await
            .expect("subscription_open");
        let sub_id = match open {
            IpcResponse::SubscriptionOpen(r) => r.sub_id,
            other => panic!("unexpected: {other:?}"),
        };

        // Start a tagged probe (matches the predicate) and an untagged one
        // (excluded). Capture each bucket_id so we can assert the routing.
        let tagged = match client
            .call(
                2,
                IpcRequest::CommandStartCombed(noisy_start_params_with_tag(Some("deploy"))),
            )
            .await
            .expect("tagged command_start")
        {
            IpcResponse::CommandStartCombed(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        let untagged = match client
            .call(
                3,
                IpcRequest::CommandStartCombed(noisy_start_params_with_tag(None)),
            )
            .await
            .expect("untagged command_start")
        {
            IpcResponse::CommandStartCombed(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_ne!(
            tagged.bucket_id, untagged.bucket_id,
            "the two probes must own distinct buckets"
        );

        // Pull within a bounded budget. Every delivered event MUST originate
        // from the tagged bucket; the untagged bucket must never appear.
        let mut tagged_events = 0usize;
        for id in 4u64..40 {
            let pull = client
                .call(
                    id,
                    IpcRequest::SubscriptionPull(SubscriptionPullParams {
                        sub_id: sub_id.clone(),
                        max: Some(50),
                        timeout_ms: Some(1_500),
                    }),
                )
                .await
                .expect("subscription_pull");
            let r = match pull {
                IpcResponse::SubscriptionPull(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            for ev in &r.events {
                assert_eq!(
                    ev.bucket_id, tagged.bucket_id,
                    "tag predicate must route ONLY to the tagged bucket"
                );
                assert_ne!(
                    ev.bucket_id, untagged.bucket_id,
                    "untagged bucket must be excluded by the tag predicate"
                );
                tagged_events += 1;
            }
            if tagged_events > 0 {
                break;
            }
        }

        assert!(
            tagged_events > 0,
            "the tagged probe's high-sev events must be delivered to the tag subscription"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn ac5_idle_pull_returns_empty_events_plus_liveness_not_error() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("ac5");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(10));

        // Open over all sources, but start NOTHING -> no in-scope buckets.
        let open = client
            .call(
                1,
                IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                    predicate: SubscriptionPredicate {
                        severity_min: Some(Severity::High),
                        kind: None,
                        sources: SubscriptionSourceSel::All,
                        tag: None,
                    },
                }),
            )
            .await
            .expect("subscription_open");
        let sub_id = match open {
            IpcResponse::SubscriptionOpen(r) => r.sub_id,
            other => panic!("unexpected: {other:?}"),
        };

        // A short idle pull must SUCCEED with empty events (never an error).
        let pull = client
            .call(
                2,
                IpcRequest::SubscriptionPull(SubscriptionPullParams {
                    sub_id,
                    max: Some(50),
                    timeout_ms: Some(200),
                }),
            )
            .await
            .expect("idle pull must be SUCCESS, not an error");
        let r = match pull {
            IpcResponse::SubscriptionPull(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(r.events.is_empty(), "idle pull returns no events");
        assert!(!r.lagged, "no lag on a fresh idle pull");

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn ac7_pull_unknown_sub_id_returns_unknown_subscription() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("ac7");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        // A well-formed-but-unknown uuid.
        let err = client
            .call(
                1,
                IpcRequest::SubscriptionPull(SubscriptionPullParams {
                    sub_id: uuid::Uuid::new_v4().to_string(),
                    max: Some(10),
                    timeout_ms: Some(200),
                }),
            )
            .await
            .expect_err("unknown sub_id must error");
        assert_eq!(err.code, IpcErrorCode::UnknownSubscription);

        // A malformed sub_id is ALSO UnknownSubscription (not malformed json
        // — the daemon parses the string after deserialization).
        let err2 = client
            .call(
                2,
                IpcRequest::SubscriptionPull(SubscriptionPullParams {
                    sub_id: "not-a-uuid".to_owned(),
                    max: Some(10),
                    timeout_ms: Some(200),
                }),
            )
            .await
            .expect_err("malformed sub_id must error");
        assert_eq!(err2.code, IpcErrorCode::UnknownSubscription);

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Phase 3 seek: repositioning within the live range moves the offset so the
/// NEXT pull re-delivers from there. Clamp is a no-op; `lagged` is false.
#[test]
fn seek_within_range_repositions_offset() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("seek-range");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        // Open BEFORE seeding so the from-now offset starts at 0 and the first
        // pull delivers every seeded event.
        let open = client
            .call(
                1,
                IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                    predicate: SubscriptionPredicate {
                        severity_min: Some(Severity::High),
                        kind: None,
                        sources: SubscriptionSourceSel::All,
                        tag: None,
                    },
                }),
            )
            .await
            .expect("subscription_open");
        let sub_id = match open {
            IpcResponse::SubscriptionOpen(r) => r.sub_id,
            other => panic!("unexpected: {other:?}"),
        };

        // Seed five high-sev events (seq 1..=5) into one bucket.
        let (bucket, tail) = seed_bucket(&state, BucketConfig::default(), 5);
        assert_eq!(tail, 5, "five appends -> tail_seq 5");

        // Drain them; the offset advances to the tail.
        let first: Vec<u64> = drain_seqs(&client, &sub_id, 2).await;
        assert_eq!(first, vec![1, 2, 3, 4, 5], "first pull delivers all five");

        // Seek back to seq 2 (within [head-1, tail] = [0, 5]); never an error.
        let seek = client
            .call(
                10,
                IpcRequest::SubscriptionSeek(SubscriptionSeekParams {
                    sub_id: sub_id.clone(),
                    bucket_id: bucket,
                    seq: 2,
                }),
            )
            .await
            .expect("subscription_seek");
        match seek {
            IpcResponse::SubscriptionSeek(r) => {
                assert_eq!(r.clamped_seq, 2, "within-range seek is a no-op clamp");
                assert!(!r.lagged, "seq 2 is above the surviving head -> not lagged");
            }
            other => panic!("unexpected: {other:?}"),
        }

        // The next pull re-delivers strictly from seq 3.
        let after: Vec<u64> = drain_seqs(&client, &sub_id, 20).await;
        assert_eq!(after, vec![3, 4, 5], "next pull re-delivers from offset+1");

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Phase 3 seek: a request below the surviving head is CLAMPED to `head_seq-1`
/// (never an error) and flags `lagged` so the consumer knows events were lost.
#[test]
fn seek_into_evicted_territory_clamps_and_sets_lagged() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("seek-evict");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        let open = client
            .call(
                1,
                IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                    predicate: SubscriptionPredicate {
                        severity_min: Some(Severity::High),
                        kind: None,
                        sources: SubscriptionSourceSel::All,
                        tag: None,
                    },
                }),
            )
            .await
            .expect("subscription_open");
        let sub_id = match open {
            IpcResponse::SubscriptionOpen(r) => r.sub_id,
            other => panic!("unexpected: {other:?}"),
        };

        // max_events=3 + 5 appends -> FIFO evicts seq 1,2; survivors 3,4,5.
        // head_seq=3, tail_seq=5, so the seek floor is head_seq-1 = 2.
        let cfg = BucketConfig {
            max_events: 3,
            ..BucketConfig::default()
        };
        let (bucket, tail) = seed_bucket(&state, cfg, 5);
        assert_eq!(
            tail, 5,
            "tail tracks the latest append regardless of eviction"
        );

        // Seek to seq 0 (below the surviving head): clamp to head_seq-1 = 2,
        // lagged = true. NOT an error.
        let seek = client
            .call(
                10,
                IpcRequest::SubscriptionSeek(SubscriptionSeekParams {
                    sub_id: sub_id.clone(),
                    bucket_id: bucket,
                    seq: 0,
                }),
            )
            .await
            .expect("subscription_seek clamps, never errors");
        match seek {
            IpcResponse::SubscriptionSeek(r) => {
                assert_eq!(r.clamped_seq, 2, "clamped to head_seq-1 after eviction");
                assert!(r.lagged, "a request below the surviving head is lagged");
            }
            other => panic!("unexpected: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Phase 3 seek: an unknown (or malformed) sub_id is `UnknownSubscription`,
/// the existing closed-set code — Phase 3 adds NO new error code.
#[test]
fn seek_unknown_sub_is_unknown_subscription() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("seek-unknown");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        // A real bucket exists so the failure is the SUB miss, not a bucket miss.
        let (bucket, _tail) = seed_bucket(&state, BucketConfig::default(), 1);

        let err = client
            .call(
                1,
                IpcRequest::SubscriptionSeek(SubscriptionSeekParams {
                    sub_id: uuid::Uuid::new_v4().to_string(),
                    bucket_id: bucket,
                    seq: 0,
                }),
            )
            .await
            .expect_err("unknown sub_id must error");
        assert_eq!(err.code, IpcErrorCode::UnknownSubscription);

        // A malformed sub_id is ALSO UnknownSubscription.
        let err2 = client
            .call(
                2,
                IpcRequest::SubscriptionSeek(SubscriptionSeekParams {
                    sub_id: "not-a-uuid".to_owned(),
                    bucket_id: bucket,
                    seq: 0,
                }),
            )
            .await
            .expect_err("malformed sub_id must error");
        assert_eq!(err2.code, IpcErrorCode::UnknownSubscription);

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Phase 3 seek scope-guard: seeking a bucket OUTSIDE the subscription's
/// predicate scope is a NO-OP -- it must NOT create a dangling offset that no
/// pull would ever read or advance. An IN-scope seek, by contrast, DOES write
/// the offset. Proves the fix leaves no dead state for out-of-scope buckets.
#[test]
fn seek_out_of_scope_bucket_creates_no_dead_state() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("seek-scope");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        // Seed two real buckets so both bucket-store lookups succeed (the seek
        // floor/tail clamp needs a known bucket; the failure under test is the
        // SCOPE miss, not a bucket miss).
        let (in_scope_bucket, _t1) = seed_bucket(&state, BucketConfig::default(), 3);
        let (out_of_scope_bucket, _t2) = seed_bucket(&state, BucketConfig::default(), 3);

        // Open a subscription scoped to ONLY the in-scope bucket. The
        // out-of-scope bucket is a real bucket the sub never routes to.
        let open = client
            .call(
                1,
                IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                    predicate: SubscriptionPredicate {
                        severity_min: Some(Severity::High),
                        kind: None,
                        sources: SubscriptionSourceSel::Buckets {
                            buckets: vec![in_scope_bucket],
                        },
                        tag: None,
                    },
                }),
            )
            .await
            .expect("subscription_open");
        let sub_id = match open {
            IpcResponse::SubscriptionOpen(r) => r.sub_id,
            other => panic!("unexpected: {other:?}"),
        };
        let sub_uuid: uuid::Uuid = sub_id.parse().expect("sub_id is a uuid");

        // Seek the OUT-OF-SCOPE bucket: clamps + responds (never an error) but
        // must NOT persist an offset for a bucket the sub does not route to.
        let seek_out = client
            .call(
                2,
                IpcRequest::SubscriptionSeek(SubscriptionSeekParams {
                    sub_id: sub_id.clone(),
                    bucket_id: out_of_scope_bucket,
                    seq: 1,
                }),
            )
            .await
            .expect("out-of-scope seek still responds, never errors");
        assert!(
            matches!(seek_out, IpcResponse::SubscriptionSeek(_)),
            "out-of-scope seek returns the typed seek response: {seek_out:?}"
        );

        // The dead-state assertion: NO offset was created for the out-of-scope
        // bucket.
        let has_dead_offset = state
            .subscriptions
            .with_sub(sub_uuid, |s| s.offsets.contains_key(&out_of_scope_bucket))
            .expect("sub present");
        assert!(
            !has_dead_offset,
            "out-of-scope seek must NOT create a dangling offset"
        );

        // Control: an IN-scope seek DOES write the offset (the guard is not
        // over-broad).
        let seek_in = client
            .call(
                3,
                IpcRequest::SubscriptionSeek(SubscriptionSeekParams {
                    sub_id: sub_id.clone(),
                    bucket_id: in_scope_bucket,
                    seq: 1,
                }),
            )
            .await
            .expect("in-scope seek responds");
        assert!(matches!(seek_in, IpcResponse::SubscriptionSeek(_)));
        let in_scope_offset = state
            .subscriptions
            .with_sub(sub_uuid, |s| s.offsets.get(&in_scope_bucket).copied())
            .expect("sub present");
        assert_eq!(
            in_scope_offset,
            Some(1),
            "in-scope seek writes the clamped offset"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
#[allow(clippy::too_many_lines)]
fn ac8_two_opens_same_predicate_get_distinct_ids_and_independent_offsets() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("ac8");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(10));

        let predicate = SubscriptionPredicate {
            severity_min: Some(Severity::High),
            kind: None,
            sources: SubscriptionSourceSel::All,
            tag: None,
        };

        let open_a = client
            .call(
                1,
                IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                    predicate: predicate.clone(),
                }),
            )
            .await
            .expect("open A");
        let open_b = client
            .call(
                2,
                IpcRequest::SubscriptionOpen(SubscriptionOpenParams { predicate }),
            )
            .await
            .expect("open B");
        let (a, hash_a) = match open_a {
            IpcResponse::SubscriptionOpen(r) => (r.sub_id, r.predicate_hash),
            other => panic!("unexpected: {other:?}"),
        };
        let (b, hash_b) = match open_b {
            IpcResponse::SubscriptionOpen(r) => (r.sub_id, r.predicate_hash),
            other => panic!("unexpected: {other:?}"),
        };
        assert_ne!(a, b, "AC8: distinct opaque sub_ids");
        assert_eq!(hash_a, hash_b, "identical predicates share predicate_hash");

        // Start one noisy command (both subscriptions are sources:all).
        let _ = client
            .call(3, IpcRequest::CommandStartCombed(noisy_start_params()))
            .await
            .expect("command_start_combed");

        // A drains its events. B is independent: it MUST still receive the
        // same events on its own pull (A's pull never advanced B's offsets).
        let mut a_count = 0usize;
        for id in 4u64..30 {
            let pull = client
                .call(
                    id,
                    IpcRequest::SubscriptionPull(SubscriptionPullParams {
                        sub_id: a.clone(),
                        max: Some(50),
                        timeout_ms: Some(1_000),
                    }),
                )
                .await
                .expect("pull A");
            if let IpcResponse::SubscriptionPull(r) = pull {
                a_count += r.events.len();
                if a_count > 0 {
                    break;
                }
            }
        }
        assert!(a_count > 0, "A must receive needle events");

        let mut b_count = 0usize;
        for id in 30u64..60 {
            let pull = client
                .call(
                    id,
                    IpcRequest::SubscriptionPull(SubscriptionPullParams {
                        sub_id: b.clone(),
                        max: Some(50),
                        timeout_ms: Some(1_000),
                    }),
                )
                .await
                .expect("pull B");
            if let IpcResponse::SubscriptionPull(r) = pull {
                b_count += r.events.len();
                if b_count > 0 {
                    break;
                }
            }
        }
        assert!(
            b_count > 0,
            "AC8: B's offsets are independent of A — B still gets the events"
        );

        // subscription_list shows both; close removes them.
        let listed = client
            .call(
                60,
                IpcRequest::SubscriptionList(SubscriptionListParams { limit: None }),
            )
            .await
            .expect("list");
        match listed {
            IpcResponse::SubscriptionList(r) => {
                assert_eq!(r.subscriptions.len(), 2, "both subs listed");
                assert!(!r.truncated);
            }
            other => panic!("unexpected: {other:?}"),
        }

        for (id, sub) in [(61u64, a), (62u64, b)] {
            let closed = client
                .call(
                    id,
                    IpcRequest::SubscriptionClose(terminal_commanderd::SubscriptionCloseParams {
                        sub_id: sub,
                    }),
                )
                .await
                .expect("close");
            match closed {
                IpcResponse::SubscriptionClose(r) => assert!(r.closed),
                other => panic!("unexpected: {other:?}"),
            }
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

// Item #3: `created_at_ms` / `last_pull_at_ms` are TRUE wall-clock stamps.
// Regression guard for the old presence-based hack that returned `now_ms()`
// at LIST time: `created_at_ms` must be stable across two list calls (it is
// the open time, not the list time), and `last_pull_at_ms` must be absent
// until the first pull, then reflect the pull's wall-clock.
#[test]
fn subscription_list_reports_true_wall_clock_stamps() {
    let data = tmp_data_dir("wallclock");
    let runtime = rt();
    runtime.block_on(async {
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        let open = client
            .call(
                1,
                IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                    predicate: SubscriptionPredicate {
                        severity_min: None,
                        kind: None,
                        sources: SubscriptionSourceSel::All,
                        tag: None,
                    },
                }),
            )
            .await
            .expect("subscription_open");
        let sub_id = match open {
            IpcResponse::SubscriptionOpen(r) => r.sub_id,
            other => panic!("unexpected: {other:?}"),
        };

        let list = |id: u64| {
            let client = &client;
            async move {
                match client
                    .call(
                        id,
                        IpcRequest::SubscriptionList(SubscriptionListParams { limit: None }),
                    )
                    .await
                    .expect("list")
                {
                    IpcResponse::SubscriptionList(r) => r,
                    other => panic!("unexpected: {other:?}"),
                }
            }
        };

        // First list: created_at_ms present, last_pull_at_ms absent (no pull).
        let first = list(2).await;
        let s1 = &first.subscriptions[0];
        let created_first = s1.created_at_ms;
        assert!(
            created_first > 0,
            "created_at_ms must be a real epoch stamp"
        );
        assert!(
            s1.last_pull_at_ms.is_none(),
            "last_pull_at_ms must be absent before the first pull"
        );

        // A second list a beat later: created_at_ms is STABLE (open time, not
        // list time). The old hack returned now_ms() here, so it would drift.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let second = list(3).await;
        let s2 = &second.subscriptions[0];
        assert_eq!(
            s2.created_at_ms, created_first,
            "created_at_ms must be the stable open time, not the list time"
        );

        // Pull once (idle pull is a success), then list: last_pull_at_ms is now
        // present and >= created_at_ms (the pull happened after the open).
        let _ = client
            .call(
                4,
                IpcRequest::SubscriptionPull(SubscriptionPullParams {
                    sub_id: sub_id.clone(),
                    max: Some(10),
                    timeout_ms: Some(100),
                }),
            )
            .await
            .expect("pull");
        let third = list(5).await;
        let s3 = &third.subscriptions[0];
        let pulled = s3
            .last_pull_at_ms
            .expect("last_pull_at_ms must be present after a pull");
        assert!(
            pulled >= created_first,
            "last_pull_at_ms ({pulled}) must be at or after created_at_ms ({created_first})"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}
