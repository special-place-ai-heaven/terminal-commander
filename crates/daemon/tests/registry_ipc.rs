// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC42 daemon IPC integration tests for the registry surface.
//!
//! Stands up the real UDS IPC server in a temp dir and exercises the
//! seven `registry_*` methods plus their interaction with the
//! in-memory activation registry.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    ContextHint, RuleDefinition, RuleStatus, RuleType, Severity, SourceStream,
};
use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse, IpcServer,
    MAX_REGISTRY_TEST_SAMPLES, RegistryActivateParams, RegistryDeactivateParams, RegistryGetParams,
    RegistrySearchParams, RegistryTestParams, RegistryTestSample, RegistryUpsertParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-ipc-reg-{tag}-{pid}-{nanos}"));
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

fn build_server(data: &std::path::Path) -> (Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let cfg = DaemonConfig::defaults_in(data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let server = IpcServer::new(Arc::clone(&state), socket);
    let handle = server.spawn().unwrap();
    (state, handle)
}

fn kw_rule(id: &str, keyword: &str, event_kind: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: event_kind.to_owned(),
        stream: None,
        description: Some("test rule".to_owned()),
        pattern: None,
        keywords: Some(vec![keyword.to_owned()]),
        captures: vec![],
        summary_template: "matched keyword".to_owned(),
        tags: vec!["test".to_owned(), "registry-ipc".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

fn regex_rule(id: &str, pattern: &str, event_kind: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Regex,
        status: RuleStatus::Active,
        severity: Severity::High,
        event_kind: event_kind.to_owned(),
        stream: Some(SourceStream::Stdout),
        description: None,
        pattern: Some(pattern.to_owned()),
        keywords: None,
        captures: vec![],
        summary_template: "matched pattern".to_owned(),
        tags: vec!["test".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

#[test]
fn registry_upsert_and_get_round_trip() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("upsert");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let def = kw_rule("kw-test-1", "needle", "needle_match");
        let resp = client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams { definition: def }),
            )
            .await
            .expect("upsert call");
        let v = match resp {
            IpcResponse::RegistryUpsert(r) => r,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(v.rule_id, "kw-test-1");
        assert_eq!(v.version, 1);

        let resp = client
            .call(
                2,
                IpcRequest::RegistryGet(RegistryGetParams {
                    rule_id: "kw-test-1".to_owned(),
                    version: None,
                }),
            )
            .await
            .expect("get call");
        let def_back = match resp {
            IpcResponse::RegistryGet(r) => r.definition,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(def_back.id, "kw-test-1");
        assert_eq!(def_back.keywords.as_deref().unwrap()[0], "needle");

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn registry_get_unknown_rule_returns_typed_error() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("getnf");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let err = client
            .call(
                1,
                IpcRequest::RegistryGet(RegistryGetParams {
                    rule_id: "does-not-exist".to_owned(),
                    version: None,
                }),
            )
            .await
            .expect_err("unknown rule must surface RuleNotFound");
        assert_eq!(err.code, IpcErrorCode::RuleNotFound);
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn registry_upsert_rejects_invalid_regex() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("badrx");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        // Unclosed group => regex compile fail at validation.
        let def = regex_rule("bad-rx", "(unclosed", "x");
        let err = client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams { definition: def }),
            )
            .await
            .expect_err("invalid regex must be rejected");
        assert_eq!(err.code, IpcErrorCode::RuleInvalid);
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn registry_test_evaluates_rule_against_samples() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("test");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());

        let def = kw_rule("kw-test-2", "needle", "needle_match");
        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams { definition: def }),
            )
            .await
            .expect("upsert");

        let resp = client
            .call(
                2,
                IpcRequest::RegistryTest(RegistryTestParams {
                    rule_id: "kw-test-2".to_owned(),
                    version: None,
                    samples: vec![
                        RegistryTestSample {
                            text: "no match here".to_owned(),
                            stream: None,
                        },
                        RegistryTestSample {
                            text: "found a needle in the haystack".to_owned(),
                            stream: None,
                        },
                    ],
                }),
            )
            .await
            .expect("test call");
        let r = match resp {
            IpcResponse::RegistryTest(r) => r,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(r.matches.len(), 1, "exactly one sample should match");
        assert_eq!(r.matches[0].sample_index, 1);
        assert_eq!(r.matches[0].kind, "needle_match");
        assert_eq!(r.matches[0].severity, Severity::Medium);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn registry_test_rejects_oversize_sample_count() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("overspl");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());

        let def = kw_rule("kw-test-3", "x", "x");
        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams { definition: def }),
            )
            .await
            .expect("upsert");

        let too_many: Vec<RegistryTestSample> = (0..=MAX_REGISTRY_TEST_SAMPLES)
            .map(|i| RegistryTestSample {
                text: format!("sample {i}"),
                stream: None,
            })
            .collect();
        let err = client
            .call(
                2,
                IpcRequest::RegistryTest(RegistryTestParams {
                    rule_id: "kw-test-3".to_owned(),
                    version: None,
                    samples: too_many,
                }),
            )
            .await
            .expect_err("over-cap samples must be rejected");
        assert_eq!(err.code, IpcErrorCode::RuleInvalid);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn registry_activate_then_list_then_deactivate() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("act");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());

        let def = kw_rule("kw-act", "needle", "act_match");
        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams { definition: def }),
            )
            .await
            .expect("upsert");

        // Activate.
        let resp = client
            .call(
                2,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-act".to_owned(),
                    version: None,
                }),
            )
            .await
            .expect("activate");
        let act = match resp {
            IpcResponse::RegistryActivate(r) => r,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(act.rule_id, "kw-act");
        assert_eq!(act.version, 1);
        assert!(!act.was_already_active);

        // In-memory state should reflect activation.
        assert!(state.activation.is_active("kw-act", 1));

        // Idempotent re-activate.
        let resp = client
            .call(
                3,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-act".to_owned(),
                    version: Some(1),
                }),
            )
            .await
            .expect("re-activate");
        match resp {
            IpcResponse::RegistryActivate(r) => assert!(r.was_already_active),
            other => panic!("unexpected response: {other:?}"),
        }

        // List active.
        let resp = client
            .call(4, IpcRequest::RegistryListActive)
            .await
            .expect("list active");
        let entries = match resp {
            IpcResponse::RegistryListActive(r) => r.entries,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].rule_id, "kw-act");
        assert_eq!(entries[0].version, 1);

        // Deactivate.
        let resp = client
            .call(
                5,
                IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
                    rule_id: "kw-act".to_owned(),
                    version: 1,
                }),
            )
            .await
            .expect("deactivate");
        let d = match resp {
            IpcResponse::RegistryDeactivate(r) => r,
            other => panic!("unexpected response: {other:?}"),
        };
        assert!(d.was_deactivated);
        assert!(!state.activation.is_active("kw-act", 1));

        // Audit rows must exist for each accepted call.
        let rows = {
            let mut g = state.store.lock();
            g.audit_since(&AuditReadRequest::new(0)).unwrap()
        };
        for action in [
            "ipc_registry_upsert",
            "ipc_registry_activate",
            "ipc_registry_list_active",
            "ipc_registry_deactivate",
        ] {
            assert!(
                rows.iter().any(|r| r.action == action),
                "missing audit row '{action}'; rows: {rows:?}"
            );
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn registry_search_finds_upserted_rule_by_tag() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("search");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());

        let def = kw_rule("kw-search", "needle", "search_kind");
        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams { definition: def }),
            )
            .await
            .expect("upsert");

        let resp = client
            .call(
                2,
                IpcRequest::RegistrySearch(RegistrySearchParams {
                    // FTS5 token search: the tag is stored as
                    // "registry-ipc" but tokenized to {"registry",
                    // "ipc"}, so we query for "registry" to avoid
                    // the FTS5 hyphen-as-NOT operator pitfall.
                    query: "registry".to_owned(),
                    limit: Some(5),
                }),
            )
            .await
            .expect("search");
        let hits = match resp {
            IpcResponse::RegistrySearch(r) => r.hits,
            other => panic!("unexpected response: {other:?}"),
        };
        assert!(
            hits.iter().any(|h| h.rule_id == "kw-search"),
            "expected kw-search in hits, got: {hits:?}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn registry_activations_survive_daemon_restart() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("restart");
        let cfg = DaemonConfig::defaults_in(&data);

        // First boot: upsert + activate.
        {
            let state = Arc::new(DaemonState::bootstrap(cfg.clone()).unwrap());
            let socket = state.config.socket_path();
            let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
            let client = DaemonClient::new(handle.socket_path().to_path_buf());
            let def = kw_rule("kw-restart", "needle", "restart_match");
            client
                .call(
                    1,
                    IpcRequest::RegistryUpsert(RegistryUpsertParams { definition: def }),
                )
                .await
                .expect("upsert");
            client
                .call(
                    2,
                    IpcRequest::RegistryActivate(RegistryActivateParams {
                        rule_id: "kw-restart".to_owned(),
                        version: None,
                    }),
                )
                .await
                .expect("activate");
            handle.shutdown().await;
        }

        // Second boot: the persistent activation row must rehydrate
        // the in-memory registry.
        {
            let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
            assert!(
                state.activation.is_active("kw-restart", 1),
                "activation must survive restart"
            );
        }
        cleanup(&data);
    });
}
