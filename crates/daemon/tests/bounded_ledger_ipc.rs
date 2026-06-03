// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Bounded-ledger surface tests (subscriptions §6, Task 12). Each list /
//! snapshot tool clamps to a per-call `limit` and flags `truncated` when more
//! exist than were returned. `runtime_state` bounds its THREE vecs
//! INDEPENDENTLY (a single cursor cannot page three lists).
//!
//! Linux/WSL only (UDS).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    ActivationScope, ContextHint, RuleDefinition, RuleStatus, RuleType, Severity, SourceStream,
};
use terminal_commanderd::{
    CommandStartParams, DaemonClient, DaemonConfig, DaemonState, IpcRequest, IpcResponse,
    IpcServer, ListLimitParams, RegistryActivateParams, RegistryUpsertParams, ServerHandle,
    SubscriptionListParams, SubscriptionOpenParams, SubscriptionPredicate, SubscriptionSourceSel,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-ledger-{tag}-{pid}-{nanos}-{n}"));
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

fn sleeper_params() -> CommandStartParams {
    CommandStartParams {
        environment: None,
        argv: vec!["sleep".to_owned(), "2".to_owned()],
        cwd: None,
        env: Vec::new(),
        bucket_config: None,
        rules: Vec::new(),
        grace_ms: Some(2_000),
    }
}

fn kw_rule(id: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Low,
        event_kind: "hit".to_owned(),
        stream: Some(SourceStream::Stdout),
        description: None,
        pattern: None,
        keywords: Some(vec!["X".to_owned()]),
        captures: vec![],
        summary_template: "x".to_owned(),
        tags: vec![],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

#[test]
fn runtime_state_bounds_its_three_vecs_independently_with_per_vec_truncated() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("rt3");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        // Two command probes -> two probes + two buckets.
        for id in [1u64, 2u64] {
            let _ = client
                .call(id, IpcRequest::CommandStartCombed(sleeper_params()))
                .await
                .expect("command_start_combed");
        }
        // Two active rules.
        for (i, rid) in ["ledger.a", "ledger.b"].iter().enumerate() {
            let id = 10 + i as u64;
            let _ = client
                .call(
                    id,
                    IpcRequest::RegistryUpsert(RegistryUpsertParams {
                        definition: kw_rule(rid),
                    }),
                )
                .await
                .expect("upsert");
            let _ = client
                .call(
                    id + 100,
                    IpcRequest::RegistryActivate(RegistryActivateParams {
                        rule_id: (*rid).to_owned(),
                        version: Some(1),
                        scope: Some(ActivationScope::Global),
                    }),
                )
                .await
                .expect("activate");
        }

        // limit=1 must truncate each of the three vecs to len 1.
        let resp = client
            .call(
                999,
                IpcRequest::RuntimeState(ListLimitParams { limit: Some(1) }),
            )
            .await
            .expect("runtime_state");
        let r = match resp {
            IpcResponse::RuntimeState(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.probes.len(), 1, "probes bounded to limit");
        assert!(r.probes_truncated, "probes_truncated set");
        assert_eq!(r.buckets.len(), 1, "buckets bounded to limit");
        assert!(r.buckets_truncated, "buckets_truncated set");
        assert_eq!(r.active_rules.len(), 1, "active_rules bounded to limit");
        assert!(r.active_rules_truncated, "active_rules_truncated set");
        // Counts reflect the TRUE totals (computed before truncation).
        assert_eq!(r.command_jobs, 2, "command_jobs is the true total");
        assert_eq!(r.active_rules_count, 2, "active_rules_count is true total");

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn probe_list_bounds_and_flags_truncated() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("pl");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        for id in [1u64, 2u64] {
            let _ = client
                .call(id, IpcRequest::CommandStartCombed(sleeper_params()))
                .await
                .expect("command_start_combed");
        }
        let resp = client
            .call(9, IpcRequest::ProbeList(ListLimitParams { limit: Some(1) }))
            .await
            .expect("probe_list");
        match resp {
            IpcResponse::ProbeList(r) => {
                assert_eq!(r.probes.len(), 1);
                assert!(r.truncated, "probe_list truncated flag set");
            }
            other => panic!("unexpected: {other:?}"),
        }

        // No limit -> all returned, not truncated.
        let resp = client
            .call(10, IpcRequest::ProbeList(ListLimitParams { limit: None }))
            .await
            .expect("probe_list");
        match resp {
            IpcResponse::ProbeList(r) => {
                assert_eq!(r.probes.len(), 2);
                assert!(!r.truncated);
            }
            other => panic!("unexpected: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn registry_list_active_bounds_and_flags_truncated() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("rla");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        for (i, rid) in ["la.a", "la.b"].iter().enumerate() {
            let id = 1 + i as u64;
            let _ = client
                .call(
                    id,
                    IpcRequest::RegistryUpsert(RegistryUpsertParams {
                        definition: kw_rule(rid),
                    }),
                )
                .await
                .expect("upsert");
            let _ = client
                .call(
                    id + 100,
                    IpcRequest::RegistryActivate(RegistryActivateParams {
                        rule_id: (*rid).to_owned(),
                        version: Some(1),
                        scope: Some(ActivationScope::Global),
                    }),
                )
                .await
                .expect("activate");
        }
        let resp = client
            .call(
                9,
                IpcRequest::RegistryListActive(ListLimitParams { limit: Some(1) }),
            )
            .await
            .expect("registry_list_active");
        match resp {
            IpcResponse::RegistryListActive(r) => {
                assert_eq!(r.entries.len(), 1);
                assert!(r.truncated, "registry_list_active truncated flag set");
            }
            other => panic!("unexpected: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn subscription_list_bounds_and_flags_truncated() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("sl");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        for id in [1u64, 2u64] {
            let _ = client
                .call(
                    id,
                    IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                        predicate: SubscriptionPredicate {
                            severity_min: None,
                            kind: None,
                            sources: SubscriptionSourceSel::All,
                        },
                    }),
                )
                .await
                .expect("subscription_open");
        }
        let resp = client
            .call(
                9,
                IpcRequest::SubscriptionList(SubscriptionListParams { limit: Some(1) }),
            )
            .await
            .expect("subscription_list");
        match resp {
            IpcResponse::SubscriptionList(r) => {
                assert_eq!(r.subscriptions.len(), 1);
                assert!(r.truncated, "subscription_list truncated flag set");
            }
            other => panic!("unexpected: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}
