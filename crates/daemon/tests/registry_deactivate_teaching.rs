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

/// Upsert `kw_rule_v(id, def_version)` and activate it under Global,
/// asserting the activate response binds the expected `def.version`. The
/// verify-as-you-go assertion is the load-bearing guard: if the handler
/// keyed on something other than `def_version`, two versions would not
/// coexist and the caller's plural-branch premise would be a lie.
async fn upsert_and_activate_global(
    state: &Arc<DaemonState>,
    base_cid: u64,
    id: &str,
    def_version: u32,
) {
    match dispatch(
        state,
        base_cid,
        IpcRequest::RegistryUpsert(RegistryUpsertParams {
            definition: kw_rule_v(id, def_version),
        }),
    )
    .await
    {
        IpcResult::Ok { .. } => {}
        IpcResult::Err { error } => panic!("upsert v{def_version} failed: {error:?}"),
    }
    match dispatch(
        state,
        base_cid + 1,
        IpcRequest::RegistryActivate(RegistryActivateParams {
            rule_id: id.to_owned(),
            version: Some(def_version),
            scope: Some(ActivationScope::Global),
        }),
    )
    .await
    {
        IpcResult::Ok {
            response: IpcResponse::RegistryActivate(r),
        } => assert_eq!(
            r.version, def_version,
            "activate(version={def_version}) must bind def.version {def_version}; got {}",
            r.version
        ),
        other => panic!("expected RegistryActivate ok for v{def_version}, got: {other:?}"),
    }
}

#[tokio::test]
async fn deactivate_unknown_version_with_two_active_teaches_plural() {
    // Cover the `else` (plural) branch of the deactivate teaching hint:
    // when TWO versions of one rule are active under one scope, a
    // deactivate of a third version must render "active versions are
    // [1, 2]" (built from active_versions_for_scope: sort/dedup/join).
    //
    // The activation set is keyed by the *definition's* `.version` field
    // (handle_registry_activate: `let version = def.version`), so the only
    // way two versions of one rule coexist under one scope via this
    // dispatch path is to upsert two defs with DISTINCT `.version` values.
    // `kw_rule_v` does exactly that. The store row version and the
    // definition version happen to coincide here (upsert #1 -> row 1 with
    // def.version 1; upsert #2 -> row 2 with def.version 2), so activating
    // version:Some(1) then version:Some(2) yields two distinct keys.
    let (state, data) = make_state("plural");

    upsert_and_activate_global(&state, 1, "kw-multi", 1).await;
    upsert_and_activate_global(&state, 3, "kw-multi", 2).await;

    // Sanity: BOTH versions must be simultaneously active under Global.
    // If either fails, the second activate collapsed onto the first key
    // and the plural branch is unreachable -- the test would be a lie.
    assert!(
        state
            .activation
            .is_active("kw-multi", 1, ActivationScope::Global),
        "v1 must remain active after activating v2 (distinct keys)"
    );
    assert!(
        state
            .activation
            .is_active("kw-multi", 2, ActivationScope::Global),
        "v2 must be active alongside v1 (distinct keys)"
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
