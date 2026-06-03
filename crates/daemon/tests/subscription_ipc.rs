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
    ContextHint, RuleDefinition, RuleStatus, RuleType, Severity, SourceStream,
};
use terminal_commanderd::{
    CommandStartParams, DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest,
    IpcResponse, IpcServer, ServerHandle, SubscriptionListParams, SubscriptionOpenParams,
    SubscriptionPredicate, SubscriptionPullParams, SubscriptionSourceSel,
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
    }
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
