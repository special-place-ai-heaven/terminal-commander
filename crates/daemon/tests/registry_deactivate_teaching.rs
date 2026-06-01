// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TB-6 regression: a `registry_deactivate` that matches no active
//! `(rule_id, version, scope)` row must return a typed RuleNotActive
//! teaching error that echoes the actually-active version, NOT a silent
//! `ok:true / was_deactivated:false` no-op.
//!
//! This test drives the daemon's in-process dispatch path
//! (`dispatch_envelope`) so it runs on every platform, including the
//! Windows dev box where the UDS-backed `registry_ipc.rs` suite is
//! `#[cfg(unix)]`-excluded.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use terminal_commander_core::{
    ActivationScope, ContextHint, RuleDefinition, RuleStatus, RuleType, Severity,
};
use terminal_commander_supervisor::identity::PeerIdentity;
use terminal_commanderd::{
    DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse, IpcResult,
    RegistryActivateParams, RegistryDeactivateParams, RegistryUpsertParams, RequestEnvelope,
};

fn temp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-deact-teach-{tag}-{pid}-{nanos}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn make_state(tag: &str) -> (Arc<DaemonState>, PathBuf) {
    let data = temp_data_dir(tag);
    let cfg = DaemonConfig::defaults_in(&data);
    let state = DaemonState::bootstrap(cfg).expect("bootstrap daemon state");
    (Arc::new(state), data)
}

fn kw_rule(id: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: "deact_match".to_owned(),
        stream: None,
        description: Some("test rule".to_owned()),
        pattern: None,
        keywords: Some(vec!["needle".to_owned()]),
        captures: vec![],
        summary_template: "matched keyword".to_owned(),
        tags: vec!["test".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

async fn dispatch(state: &Arc<DaemonState>, correlation_id: u64, request: IpcRequest) -> IpcResult {
    let req = RequestEnvelope {
        correlation_id,
        request,
    };
    let resp = terminal_commanderd::ipc::dispatch_envelope(
        state,
        Instant::now(),
        &req,
        &PeerIdentity::unknown(),
    )
    .await;
    assert_eq!(resp.correlation_id, correlation_id);
    resp.result
}

#[tokio::test]
async fn deactivate_wrong_version_teaches_active_version() {
    let (state, data) = make_state("wrongver");

    // Upsert + activate v1 globally.
    match dispatch(
        &state,
        1,
        IpcRequest::RegistryUpsert(RegistryUpsertParams {
            definition: kw_rule("kw-deact"),
        }),
    )
    .await
    {
        IpcResult::Ok { .. } => {}
        IpcResult::Err { error } => panic!("upsert failed: {error:?}"),
    }
    match dispatch(
        &state,
        2,
        IpcRequest::RegistryActivate(RegistryActivateParams {
            rule_id: "kw-deact".to_owned(),
            version: None,
            scope: Some(ActivationScope::Global),
        }),
    )
    .await
    {
        IpcResult::Ok { .. } => {}
        IpcResult::Err { error } => panic!("activate failed: {error:?}"),
    }

    // Deactivate a version that is NOT active (v99): teaching error.
    match dispatch(
        &state,
        3,
        IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
            rule_id: "kw-deact".to_owned(),
            version: 99,
            scope: Some(ActivationScope::Global),
        }),
    )
    .await
    {
        IpcResult::Err { error } => {
            assert_eq!(error.code, IpcErrorCode::RuleNotActive);
            assert!(
                error.message.contains("kw-deact")
                    && error.message.contains("v99")
                    && error.message.contains("active version is 1"),
                "teaching error must echo the active version; got: {}",
                error.message
            );
        }
        IpcResult::Ok { response } => {
            panic!("deactivating a non-active version must not fake success: {response:?}");
        }
    }

    // The refused deactivate must not mutate the active set.
    assert!(
        state
            .activation
            .is_active("kw-deact", 1, ActivationScope::Global)
    );

    cleanup(&data);
}

#[tokio::test]
async fn deactivate_never_active_rule_teaches_no_active_version() {
    let (state, data) = make_state("never");

    match dispatch(
        &state,
        1,
        IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
            rule_id: "kw-never".to_owned(),
            version: 1,
            scope: Some(ActivationScope::Global),
        }),
    )
    .await
    {
        IpcResult::Err { error } => {
            assert_eq!(error.code, IpcErrorCode::RuleNotActive);
            assert!(
                error.message.contains("kw-never") && error.message.contains("no active version"),
                "teaching error must explain there is no active version; got: {}",
                error.message
            );
        }
        IpcResult::Ok { response } => {
            panic!("deactivating a never-active rule must not fake success: {response:?}");
        }
    }

    cleanup(&data);
}

#[tokio::test]
async fn deactivate_active_row_still_succeeds() {
    // Guard: the teaching error must NOT regress the happy path. A
    // matching (rule_id, version, scope) deactivate still returns ok
    // with was_deactivated=true.
    let (state, data) = make_state("happy");

    dispatch(
        &state,
        1,
        IpcRequest::RegistryUpsert(RegistryUpsertParams {
            definition: kw_rule("kw-happy"),
        }),
    )
    .await;
    dispatch(
        &state,
        2,
        IpcRequest::RegistryActivate(RegistryActivateParams {
            rule_id: "kw-happy".to_owned(),
            version: None,
            scope: Some(ActivationScope::Global),
        }),
    )
    .await;

    match dispatch(
        &state,
        3,
        IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
            rule_id: "kw-happy".to_owned(),
            version: 1,
            scope: Some(ActivationScope::Global),
        }),
    )
    .await
    {
        IpcResult::Ok {
            response: IpcResponse::RegistryDeactivate(r),
        } => {
            assert!(
                r.was_deactivated,
                "matching row must report was_deactivated"
            );
        }
        other => panic!("expected RegistryDeactivate ok, got: {other:?}"),
    }

    assert!(
        !state
            .activation
            .is_active("kw-happy", 1, ActivationScope::Global)
    );

    cleanup(&data);
}
