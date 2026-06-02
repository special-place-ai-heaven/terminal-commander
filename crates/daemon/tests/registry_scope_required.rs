// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC42d daemon-level tests: registry_activate / registry_deactivate
//! REQUIRE an explicit `scope` field. A missing scope must be
//! rejected with `IpcErrorCode::ScopeInvalid` and durably audited
//! through the standard dispatcher path. An explicit `Global` scope
//! continues to behave as TC42b global activation.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    ActivationScope, ContextHint, RuleDefinition, RuleStatus, RuleType, Severity,
};
use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse, IpcServer,
    RegistryActivateParams, RegistryDeactivateParams, RegistryUpsertParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-scope-req-{tag}-{pid}-{nanos}-{n}"));
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

fn kw_rule(id: &str, keyword: &str, event_kind: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: event_kind.to_owned(),
        stream: None,
        description: None,
        pattern: None,
        keywords: Some(vec![keyword.to_owned()]),
        captures: vec![],
        summary_template: "matched".to_owned(),
        tags: vec!["tc42d".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

#[test]
fn activate_without_scope_is_rejected_and_audited() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("act-missing");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: kw_rule("kw-no-scope", "needle", "needle_match"),
                }),
            )
            .await
            .expect("upsert");

        let err = client
            .call(
                2,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-no-scope".to_owned(),
                    version: None,
                    scope: None,
                }),
            )
            .await
            .expect_err("missing scope must be rejected");
        assert_eq!(err.code, IpcErrorCode::ScopeInvalid);
        assert!(
            err.message.contains("scope is required"),
            "expected helpful reason; got: {}",
            err.message
        );

        // Registry must stay empty for this rule.
        assert!(!state.activation.is_active("kw-no-scope", 1, ActivationScope::Global));

        // Audit row landed via the dispatcher (decision = error).
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|r| r.action == "ipc_registry_activate" && r.decision == "error"),
            "missing-scope rejection must land an ipc_registry_activate audit row with decision=error; rows: {rows:?}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn deactivate_without_scope_is_rejected_and_audited() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("deact-missing");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
                    rule_id: "anything".to_owned(),
                    version: 1,
                    scope: None,
                }),
            )
            .await
            .expect_err("missing scope must be rejected");
        assert_eq!(err.code, IpcErrorCode::ScopeInvalid);

        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|r| r.action == "ipc_registry_deactivate" && r.decision == "error"),
            "missing-scope rejection must land an ipc_registry_deactivate audit row; rows: {rows:?}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn explicit_global_scope_activates_normally() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("explicit-global");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: kw_rule("kw-explicit-global", "needle", "needle_match"),
                }),
            )
            .await
            .expect("upsert");

        let resp = client
            .call(
                2,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-explicit-global".to_owned(),
                    version: None,
                    scope: Some(ActivationScope::Global),
                }),
            )
            .await
            .expect("activate explicit global");
        match resp {
            IpcResponse::RegistryActivate(r) => {
                assert!(matches!(r.scope, ActivationScope::Global));
                assert!(!r.was_already_active);
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(
            state
                .activation
                .is_active("kw-explicit-global", 1, ActivationScope::Global)
        );

        // Symmetric explicit-global deactivate.
        let resp = client
            .call(
                3,
                IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
                    rule_id: "kw-explicit-global".to_owned(),
                    version: 1,
                    scope: Some(ActivationScope::Global),
                }),
            )
            .await
            .expect("deactivate explicit global");
        match resp {
            IpcResponse::RegistryDeactivate(r) => {
                assert!(r.was_deactivated);
                assert!(matches!(r.scope, ActivationScope::Global));
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(
            !state
                .activation
                .is_active("kw-explicit-global", 1, ActivationScope::Global)
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}
