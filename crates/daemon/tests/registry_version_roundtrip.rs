// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Portable (transport-agnostic) regression test for the registry
//! version round-trip trust bug.
//!
//! The bug: `create_rule_version` assigned a monotonic row version
//! (latest+1) and returned it, but persisted the caller's
//! RuleDefinition VERBATIM -- so the blob's `version` kept the value
//! the caller sent. Every other registry API (activate / deactivate /
//! list_active) keys on the blob's `definition.version`, so a second
//! upsert returned version 2 while activate/deactivate would only
//! accept the caller's (unchanged) version 1. `registry_upsert` and
//! the rest of the registry surface disagreed.
//!
//! This test drives the public dispatch path (`dispatch_envelope`,
//! the same entry the Windows named-pipe transport uses) in-process,
//! so it runs on every host -- no UDS required. It simulates a naive
//! operator who upserts the same definition twice WITHOUT bumping its
//! `version`, then activates / lists / deactivates the version the
//! second upsert reported. After the fix that version must be accepted
//! by every API.

use std::sync::Arc;
use std::time::Instant;

use terminal_commander_core::{
    ActivationScope, ContextHint, RuleDefinition, RuleStatus, RuleType, Severity,
};
use terminal_commander_supervisor::identity::PeerIdentity;
use terminal_commanderd::ipc::dispatch_envelope;
use terminal_commanderd::{
    DaemonConfig, DaemonState, IpcRequest, IpcResponse, IpcResult, RegistryActivateParams,
    RegistryDeactivateParams, RegistryUpsertParams, RequestEnvelope,
};

fn temp_data_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-reg-vrt-{tag}-{pid}-{nanos}-{n}"));
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

fn test_peer() -> PeerIdentity {
    PeerIdentity::unknown_because("in-process dispatch test")
}

/// A keyword rule whose `version` is left at 1 by the caller and never
/// changed between upserts (the exact naive-operator behavior that
/// surfaced the bug).
fn kw_rule(id: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: "vrt_match".to_owned(),
        stream: None,
        description: Some("version round-trip test rule".to_owned()),
        pattern: None,
        keywords: Some(vec!["needle".to_owned()]),
        captures: vec![],
        summary_template: "matched needle".to_owned(),
        tags: vec!["test".to_owned(), "version-roundtrip".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

async fn call(
    state: &Arc<DaemonState>,
    boot: Instant,
    peer: &PeerIdentity,
    correlation_id: u64,
    request: IpcRequest,
) -> IpcResponse {
    let env = RequestEnvelope {
        correlation_id,
        request,
    };
    let resp = dispatch_envelope(state, boot, &env, peer).await;
    match resp.result {
        IpcResult::Ok { response } => response,
        IpcResult::Err { error } => panic!("dispatch returned error: {error:?}"),
    }
}

/// Upsert `def` and return the store-assigned version.
async fn upsert(
    state: &Arc<DaemonState>,
    boot: Instant,
    peer: &PeerIdentity,
    correlation_id: u64,
    def: &RuleDefinition,
) -> u32 {
    let resp = call(
        state,
        boot,
        peer,
        correlation_id,
        IpcRequest::RegistryUpsert(RegistryUpsertParams {
            definition: def.clone(),
        }),
    )
    .await;
    match resp {
        IpcResponse::RegistryUpsert(r) => r.version,
        other => panic!("unexpected upsert response: {other:?}"),
    }
}

#[test]
fn registry_upsert_returned_version_round_trips_through_activate_deactivate() {
    let runtime = rt();
    runtime.block_on(async {
        let data = temp_data_dir("rt");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let boot = Instant::now();
        let peer = test_peer();

        let def = kw_rule("kw-vrt");

        // First upsert -> v1.
        let v1 = upsert(&state, boot, &peer, 1, &def).await;
        assert_eq!(v1, 1);

        // Second upsert of the SAME definition (version still 1; the
        // operator did not bump it) -> the store assigns v2.
        let v2 = upsert(&state, boot, &peer, 2, &def).await;
        assert_eq!(
            v2, 2,
            "second upsert of the same def must report the assigned version 2"
        );

        // Activate the version the upsert reported (2). Pre-fix the
        // blob still said 1, so the daemon's activate path (which keys
        // on definition.version) would reject 2 -- the trust bug.
        let resp = call(
            &state,
            boot,
            &peer,
            3,
            IpcRequest::RegistryActivate(RegistryActivateParams {
                rule_id: "kw-vrt".to_owned(),
                version: Some(2),
                scope: Some(ActivationScope::Global),
            }),
        )
        .await;
        let act = match resp {
            IpcResponse::RegistryActivate(r) => r,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(
            act.version, 2,
            "activate must accept and echo the upsert-returned version"
        );
        assert!(!act.was_already_active);

        // In-memory activation state must reflect v2 (not v1).
        assert!(
            state
                .activation
                .is_active("kw-vrt", 2, ActivationScope::Global),
            "v2 must be active in the in-memory registry"
        );

        // list_active must surface version 2.
        let resp = call(&state, boot, &peer, 4, IpcRequest::RegistryListActive).await;
        let entries = match resp {
            IpcResponse::RegistryListActive(r) => r.entries,
            other => panic!("unexpected response: {other:?}"),
        };
        let entry = entries
            .iter()
            .find(|e| e.rule_id == "kw-vrt")
            .expect("kw-vrt must appear in list_active");
        assert_eq!(
            entry.version, 2,
            "list_active must report the upsert-returned version"
        );

        // Deactivate version 2 -> ok / was_deactivated true.
        let resp = call(
            &state,
            boot,
            &peer,
            5,
            IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
                rule_id: "kw-vrt".to_owned(),
                version: 2,
                scope: Some(ActivationScope::Global),
            }),
        )
        .await;
        let deact = match resp {
            IpcResponse::RegistryDeactivate(r) => r,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(deact.version, 2);
        assert!(
            deact.was_deactivated,
            "deactivate of the upsert-returned version must report it was active"
        );
        assert!(
            !state
                .activation
                .is_active("kw-vrt", 2, ActivationScope::Global),
            "v2 must be inactive after deactivate"
        );

        cleanup(&data);
    });
}
