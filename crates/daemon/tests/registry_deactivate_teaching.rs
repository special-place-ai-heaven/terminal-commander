// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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
    DaemonConfig, DaemonState, FileWatchStartParams, FileWatchStartResponse, IpcErrorCode,
    IpcRequest, IpcResponse, IpcResult, RegistryActivateParams, RegistryDeactivateParams,
    RegistryUpsertParams, RequestEnvelope,
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

/// `kw_rule` with an explicit `definition.version`. The activation set is
/// keyed by the *definition's* `.version` field (see
/// `handle_registry_activate`: `let version = def.version`), so distinct
/// `.version` values are the only way to make two versions of one rule
/// coexist under a single scope via the dispatch path.
fn kw_rule_v(id: &str, version: u32) -> RuleDefinition {
    RuleDefinition {
        version,
        ..kw_rule(id)
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

/// Create a unique existing regular file under the temp dir and return its
/// path. A live file-watch over it makes a `Bucket` scope resolve to a live
/// entity (so `validate_scope_against_live_jobs` passes), which is the
/// precondition for reaching the teaching-error branch with a Bucket scope.
fn make_temp_file(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-deact-teach-watch-{tag}-{pid}-{nanos}.log"));
    std::fs::write(&p, b"seed\n").expect("write temp watch file");
    p
}

#[tokio::test]
async fn deactivate_wrong_scope_teaches_cross_scope_and_preserves_row() {
    // A rule active under Global, deactivated under a (live, valid) Bucket
    // scope, must (1) pass live-job scope validation, (2) hit the typed
    // RuleNotActive teaching branch, (3) render the non-Global scope_desc
    // ("bucket=..."), (4) emit the cross-scope hint, and (5) leave the real
    // Global row untouched (a refused wrong-scope op must not mutate state).
    let (state, data) = make_state("xscope");
    let watch_file = make_temp_file("xscope");

    // Start a live file-watch so the bucket scope resolves to a live entity.
    let bucket_id = match dispatch(
        &state,
        1,
        IpcRequest::FileWatchStart(FileWatchStartParams {
            path: watch_file.clone(),
            bucket_config: None,
            rules: vec![],
            follow_from_beginning: None,
            tag: None,
        }),
    )
    .await
    {
        IpcResult::Ok {
            response: IpcResponse::FileWatchStart(r),
        } => {
            let r: FileWatchStartResponse = r;
            r.bucket_id
        }
        other => panic!("expected FileWatchStart ok, got: {other:?}"),
    };

    // Upsert + activate the rule under Global scope.
    match dispatch(
        &state,
        2,
        IpcRequest::RegistryUpsert(RegistryUpsertParams {
            definition: kw_rule("kw-xscope"),
        }),
    )
    .await
    {
        IpcResult::Ok { .. } => {}
        IpcResult::Err { error } => panic!("upsert failed: {error:?}"),
    }
    match dispatch(
        &state,
        3,
        IpcRequest::RegistryActivate(RegistryActivateParams {
            rule_id: "kw-xscope".to_owned(),
            version: None,
            scope: Some(ActivationScope::Global),
        }),
    )
    .await
    {
        IpcResult::Ok { .. } => {}
        IpcResult::Err { error } => panic!("activate failed: {error:?}"),
    }

    // Deactivate under the (valid, live) Bucket scope -> wrong scope.
    match dispatch(
        &state,
        4,
        IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
            rule_id: "kw-xscope".to_owned(),
            version: 1,
            scope: Some(ActivationScope::Bucket { bucket_id }),
        }),
    )
    .await
    {
        IpcResult::Err { error } => {
            assert_eq!(error.code, IpcErrorCode::RuleNotActive);
            // Non-Global scope_desc renders as "bucket=<wire>"; do not pin
            // the exact wire string, only the kind prefix.
            assert!(
                error.message.contains("bucket="),
                "non-Global scope must render scope_desc as bucket=...; got: {}",
                error.message
            );
            // Cross-scope hint: no active version under THIS scope.
            assert!(
                error.message.contains("different scope"),
                "cross-scope hint must mention a different scope; got: {}",
                error.message
            );
        }
        IpcResult::Ok { response } => {
            panic!("wrong-scope deactivate must not fake success: {response:?}");
        }
    }

    // The real Global row must survive the refused wrong-scope op.
    assert!(
        state
            .activation
            .is_active("kw-xscope", 1, ActivationScope::Global),
        "refused wrong-scope deactivate must not mutate the Global row"
    );

    cleanup(&data);
    let _ = std::fs::remove_file(&watch_file);
}

#[tokio::test]
async fn deactivate_unknown_version_with_two_active_teaches_plural() {
    // Cover the `else` (plural) branch of the deactivate teaching hint:
    // when TWO versions of one rule are active under one scope, a
    // deactivate of a third version must render "active versions are
    // [1, 2]" (built from active_versions_for_scope: sort/dedup/join).
    //
    // Since S5 (activate-supersedes), the activate DISPATCH path can no
    // longer create this state: activating v2 closes v1 under the same
    // scope. The state still exists in the wild — daemons that persisted
    // multi-version activations before S5 re-hydrate BOTH rows at
    // bootstrap (`ActivationRegistry::from_entries`) — so the plural
    // teaching branch stays live code. Simulate exactly that legacy
    // re-hydration by binding both versions directly into the in-memory
    // authority, which is what bootstrap does with persisted rows.
    let (state, data) = make_state("plural");

    state
        .activation
        .activate(kw_rule_v("kw-multi", 1), ActivationScope::Global);
    state
        .activation
        .activate(kw_rule_v("kw-multi", 2), ActivationScope::Global);

    // Sanity: BOTH versions must be simultaneously active under Global,
    // mirroring a pre-S5 persisted state after re-hydration.
    assert!(
        state
            .activation
            .is_active("kw-multi", 1, ActivationScope::Global),
        "legacy v1 must be active after direct rehydration"
    );
    assert!(
        state
            .activation
            .is_active("kw-multi", 2, ActivationScope::Global),
        "legacy v2 must be active alongside v1"
    );

    // Deactivate a version that is NOT active (v3): plural teaching error
    // listing both active versions.
    match dispatch(
        &state,
        5,
        IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
            rule_id: "kw-multi".to_owned(),
            version: 3,
            scope: Some(ActivationScope::Global),
        }),
    )
    .await
    {
        IpcResult::Err { error } => {
            assert_eq!(error.code, IpcErrorCode::RuleNotActive);
            assert!(
                error.message.contains("kw-multi")
                    && error.message.contains("v3")
                    && error.message.contains("active versions are [1, 2]"),
                "plural teaching error must list both active versions; got: {}",
                error.message
            );
        }
        IpcResult::Ok { response } => {
            panic!("deactivating a non-active version must not fake success: {response:?}");
        }
    }

    // The refused deactivate must not mutate either active row.
    assert!(
        state
            .activation
            .is_active("kw-multi", 1, ActivationScope::Global)
    );
    assert!(
        state
            .activation
            .is_active("kw-multi", 2, ActivationScope::Global)
    );

    cleanup(&data);
}
