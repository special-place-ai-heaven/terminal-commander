// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Data-integrity regressions for rule-activation persistence, driven
//! through the in-process dispatch path (`dispatch_envelope`) so they run
//! on every platform, including the Windows dev box where the UDS-backed
//! `registry_ipc.rs` suite is `#[cfg(unix)]`-excluded.
//!
//! Two bugs are pinned here:
//!
//! BUG 1 (restart resurrection): the persistent activation table must
//! hold AT MOST ONE open row per `(rule_id, version, scope)`. Before the
//! fix, `record_activation_scoped` inserted unconditionally and
//! `deactivate_rule_scoped` closed only ONE open row -- so activate x2 +
//! deactivate x1 left a surviving open row that re-hydrated on bootstrap,
//! resurrecting a deactivated rule. The re-bootstrap test below asserts
//! the rule is NOT active after a simulated restart.
//!
//! BUG 2 (write-ordering divergence): activate/deactivate must persist to
//! the durable store BEFORE mutating the in-memory authority. Before the
//! fix, memory was mutated first; a store-write failure then left memory
//! and store disagreeing. The store-failure tests below inject an
//! `Unavailable` store fault (by shutting the store actor) and assert the
//! in-memory set still matches the durable store (i.e. it was NOT mutated
//! by the failed write).

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
    p.push(format!("tc-act-integrity-{tag}-{pid}-{nanos}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn kw_rule(id: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: "integrity_match".to_owned(),
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

async fn upsert(state: &Arc<DaemonState>, cid: u64, id: &str) {
    match dispatch(
        state,
        cid,
        IpcRequest::RegistryUpsert(RegistryUpsertParams {
            definition: kw_rule(id),
        }),
    )
    .await
    {
        IpcResult::Ok { .. } => {}
        IpcResult::Err { error } => panic!("upsert failed: {error:?}"),
    }
}

async fn activate_global(state: &Arc<DaemonState>, cid: u64, id: &str) {
    match dispatch(
        state,
        cid,
        IpcRequest::RegistryActivate(RegistryActivateParams {
            rule_id: id.to_owned(),
            version: None,
            scope: Some(ActivationScope::Global),
        }),
    )
    .await
    {
        IpcResult::Ok { .. } => {}
        IpcResult::Err { error } => panic!("activate failed: {error:?}"),
    }
}

/// BUG 1 -- the exact resurrection sequence: activate x1, then activate
/// x2 (same key), then deactivate x1, then "restart" (drop + re-bootstrap
/// the DaemonState against the same on-disk store). The rule MUST NOT be
/// active after the reload.
///
/// Before the fix this failed: the second activate inserted a duplicate
/// open row, the single deactivate closed only one (LIMIT 1), and
/// bootstrap re-hydrated the survivor.
#[tokio::test]
async fn double_activate_then_deactivate_does_not_resurrect_on_restart() {
    let data = temp_data_dir("resurrect");
    let cfg = DaemonConfig::defaults_in(&data);

    // First boot: upsert, activate TWICE (same rule/version/scope), then
    // deactivate ONCE.
    {
        let state = Arc::new(DaemonState::bootstrap(cfg.clone()).expect("bootstrap #1"));
        upsert(&state, 1, "kw-resurrect").await;
        activate_global(&state, 2, "kw-resurrect").await;
        activate_global(&state, 3, "kw-resurrect").await; // idempotent re-activate

        // Sanity: active after the activates.
        assert!(
            state
                .activation
                .is_active("kw-resurrect", 1, ActivationScope::Global),
            "rule must be active after activate"
        );

        // Deactivate ONCE.
        match dispatch(
            &state,
            4,
            IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
                rule_id: "kw-resurrect".to_owned(),
                version: 1,
                scope: Some(ActivationScope::Global),
            }),
        )
        .await
        {
            IpcResult::Ok {
                response: IpcResponse::RegistryDeactivate(r),
            } => assert!(r.was_deactivated, "single deactivate must report success"),
            other => panic!("expected RegistryDeactivate ok, got: {other:?}"),
        }

        // Sanity: inactive in memory after deactivate.
        assert!(
            !state
                .activation
                .is_active("kw-resurrect", 1, ActivationScope::Global),
            "rule must be inactive in memory after deactivate"
        );

        // Close the store actor cleanly so the second bootstrap can reopen
        // the same on-disk database deterministically.
        state.store.shutdown().expect("shutdown store actor");
    }

    // Second boot ("restart"): re-hydrate from the durable store. The
    // deactivated rule MUST NOT come back.
    {
        let state = Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap #2"));
        assert!(
            !state
                .activation
                .is_active("kw-resurrect", 1, ActivationScope::Global),
            "a deactivated rule must NOT resurrect after restart (activate x2 + \
             deactivate x1 must leave it INACTIVE)"
        );
        state.store.shutdown().expect("shutdown store actor #2");
    }

    cleanup(&data);
}

/// Control: the legitimate restart-survival path is preserved. A single
/// activate (no deactivate) must STILL re-hydrate as active after restart.
/// Guards against the resurrection fix over-correcting into "activations
/// never survive".
#[tokio::test]
async fn single_activate_survives_restart() {
    let data = temp_data_dir("survive");
    let cfg = DaemonConfig::defaults_in(&data);

    {
        let state = Arc::new(DaemonState::bootstrap(cfg.clone()).expect("bootstrap #1"));
        upsert(&state, 1, "kw-survive").await;
        activate_global(&state, 2, "kw-survive").await;
        state.store.shutdown().expect("shutdown store actor");
    }
    {
        let state = Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap #2"));
        assert!(
            state
                .activation
                .is_active("kw-survive", 1, ActivationScope::Global),
            "a single activation (never deactivated) must survive restart"
        );
        state.store.shutdown().expect("shutdown store actor #2");
    }

    cleanup(&data);
}

/// BUG 2 (activate ordering): if the durable store write fails, the
/// in-memory authority must remain UNCHANGED -- memory and store stay
/// consistent. We inject the failure by shutting the store actor so every
/// subsequent write returns `Unavailable` (mapped to `Internal`).
#[tokio::test]
async fn activate_store_failure_does_not_mutate_memory() {
    let data = temp_data_dir("act-fail");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"));

    // Upsert succeeds while the store is alive.
    upsert(&state, 1, "kw-actfail").await;

    // Kill the store actor: subsequent writes fail.
    state.store.shutdown().expect("shutdown store actor");

    // Activate now: the durable write fails. Persist-first ordering means
    // memory was NOT touched.
    match dispatch(
        &state,
        2,
        IpcRequest::RegistryActivate(RegistryActivateParams {
            rule_id: "kw-actfail".to_owned(),
            version: Some(1),
            scope: Some(ActivationScope::Global),
        }),
    )
    .await
    {
        IpcResult::Err { error } => {
            assert_eq!(
                error.code,
                IpcErrorCode::Internal,
                "a store backend fault must surface as Internal, not a \
                 caller-fixable code; got {:?}: {}",
                error.code,
                error.message
            );
        }
        IpcResult::Ok { response } => {
            panic!("activate must fail when the store write fails, not fake success: {response:?}");
        }
    }

    // The invariant: memory must agree with the (unchanged) store. The
    // failed activate must NOT have left the rule active in memory.
    assert!(
        !state
            .activation
            .is_active("kw-actfail", 1, ActivationScope::Global),
        "a failed activate store-write must NOT mutate the in-memory active \
         set (memory/store divergence)"
    );

    cleanup(&data);
}

/// BUG 2 (deactivate ordering): if the durable store write fails, the
/// in-memory authority must remain UNCHANGED. A rule that was active
/// before the failed deactivate must STILL be active in memory afterward,
/// matching the still-open durable row.
#[tokio::test]
async fn deactivate_store_failure_does_not_mutate_memory() {
    let data = temp_data_dir("deact-fail");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"));

    // Upsert + activate while the store is alive: durable open row exists,
    // memory has the rule active.
    upsert(&state, 1, "kw-deactfail").await;
    activate_global(&state, 2, "kw-deactfail").await;
    assert!(
        state
            .activation
            .is_active("kw-deactfail", 1, ActivationScope::Global),
        "precondition: rule must be active before the failed deactivate"
    );

    // Kill the store actor: the deactivate's durable write will fail.
    state.store.shutdown().expect("shutdown store actor");

    match dispatch(
        &state,
        3,
        IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
            rule_id: "kw-deactfail".to_owned(),
            version: 1,
            scope: Some(ActivationScope::Global),
        }),
    )
    .await
    {
        IpcResult::Err { error } => {
            assert_eq!(
                error.code,
                IpcErrorCode::Internal,
                "a store backend fault must surface as Internal; got {:?}: {}",
                error.code,
                error.message
            );
        }
        IpcResult::Ok { response } => {
            panic!(
                "deactivate must fail when the store write fails, not fake success: {response:?}"
            );
        }
    }

    // The invariant: the failed deactivate must NOT have removed the rule
    // from memory. Memory still matches the still-open durable row.
    assert!(
        state
            .activation
            .is_active("kw-deactfail", 1, ActivationScope::Global),
        "a failed deactivate store-write must NOT mutate the in-memory active \
         set (memory/store divergence)"
    );

    cleanup(&data);
}
