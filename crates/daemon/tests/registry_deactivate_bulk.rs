// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! US2 (FR-011) daemon IPC tests for bulk / pack deactivate.
//!
//! Exercises `registry_deactivate_bulk` against the real UDS IPC
//! server: pack-scope deactivation in one call, per-rule outcomes with
//! an unknown rule named, exactly-one-selector validation, and a single
//! post-loop rebind pass.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    ActivationScope, ContextHint, RuleDefinition, RuleStatus, RuleType, Severity,
};
use terminal_commanderd::{
    BulkOutcomeKind, CommandStartParams, DaemonClient, DaemonConfig, DaemonState, IpcErrorCode,
    IpcRequest, IpcResponse, IpcServer, ListLimitParams, RegistryActivateParams,
    RegistryDeactivateBulkParams, RegistryImportPackParams, RegistryUpsertParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-bulk-deact-{tag}-{pid}-{nanos}-{n}"));
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

fn kw_rule(id: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: "bulk_match".to_owned(),
        stream: None,
        description: None,
        pattern: None,
        keywords: Some(vec!["needle".to_owned()]),
        captures: vec![],
        summary_template: "matched".to_owned(),
        tags: vec!["fr011".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

async fn upsert_and_activate(client: &DaemonClient, correlation: u64, rule_id: &str) {
    client
        .call(
            correlation,
            IpcRequest::RegistryUpsert(RegistryUpsertParams {
                definition: kw_rule(rule_id),
            }),
        )
        .await
        .expect("upsert");
    client
        .call(
            correlation + 1,
            IpcRequest::RegistryActivate(RegistryActivateParams {
                rule_id: rule_id.to_owned(),
                version: None,
                scope: Some(ActivationScope::Global),
            }),
        )
        .await
        .expect("activate");
}

async fn list_active_ids(client: &DaemonClient, correlation: u64) -> Vec<String> {
    let resp = client
        .call(
            correlation,
            IpcRequest::RegistryListActive(ListLimitParams::default()),
        )
        .await
        .expect("list active");
    match resp {
        IpcResponse::RegistryListActive(r) => r.entries.into_iter().map(|e| e.rule_id).collect(),
        other => panic!("unexpected response: {other:?}"),
    }
}

/// FR-011 scenario 3: a single pack-level deactivate closes every active
/// member; the response lists each acted-on rule and the active set is
/// empty afterwards.
#[test]
fn deactivate_bulk_pack_scope_deactivates_all_members_in_one_call() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("pack-scope");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        // Import + activate the whole cargo pack (every member active).
        client
            .call(
                1,
                IpcRequest::RegistryImportPack(RegistryImportPackParams {
                    pack: "cargo".to_owned(),
                    activate: true,
                    scope: Some(ActivationScope::Global),
                }),
            )
            .await
            .expect("import+activate cargo");
        let active_before = list_active_ids(&client, 2).await;
        assert!(!active_before.is_empty(), "cargo pack must be active");

        // ONE bulk call deactivates every pack member.
        let resp = client
            .call(
                3,
                IpcRequest::RegistryDeactivateBulk(RegistryDeactivateBulkParams {
                    pack: Some("cargo".to_owned()),
                    rule_ids: None,
                    scope: ActivationScope::Global,
                }),
            )
            .await
            .expect("bulk deactivate pack");
        let r = match resp {
            IpcResponse::RegistryDeactivateBulk(r) => r,
            other => panic!("unexpected response: {other:?}"),
        };
        assert!(!r.outcomes.is_empty(), "pack must have members");
        for o in &r.outcomes {
            assert_eq!(
                o.outcome,
                BulkOutcomeKind::Deactivated,
                "every active pack member must be deactivated, got {o:?}"
            );
            assert!(
                o.version.is_some(),
                "a deactivated rule echoes the version acted on, got {o:?}"
            );
        }
        // The active list is now empty.
        let active_after = list_active_ids(&client, 4).await;
        assert!(
            active_after.is_empty(),
            "no rule may remain active after pack deactivate, got: {active_after:?}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// FR-011 scenario 4: a bulk deactivate naming a known-active rule, a
/// known-inactive rule, and an unknown rule reports one outcome per
/// requested rule, in request order, with the unknown one named.
#[test]
fn deactivate_bulk_reports_per_rule_outcomes_with_unknown_named() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("outcomes");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        // kw-active: upserted AND activated (global).
        upsert_and_activate(&client, 1, "kw-active").await;
        // kw-known: upserted but NEVER activated -> known-but-not-active.
        client
            .call(
                10,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: kw_rule("kw-known"),
                }),
            )
            .await
            .expect("upsert kw-known");

        // rule_ids selector covering all three outcome kinds, order kept.
        let resp = client
            .call(
                20,
                IpcRequest::RegistryDeactivateBulk(RegistryDeactivateBulkParams {
                    pack: None,
                    rule_ids: Some(vec![
                        "kw-active".to_owned(),
                        "kw-known".to_owned(),
                        "ghost.unknown".to_owned(),
                    ]),
                    scope: ActivationScope::Global,
                }),
            )
            .await
            .expect("bulk deactivate mixed");
        let r = match resp {
            IpcResponse::RegistryDeactivateBulk(r) => r,
            other => panic!("unexpected response: {other:?}"),
        };
        // One outcome per requested rule, IN REQUEST ORDER.
        assert_eq!(r.outcomes.len(), 3, "one outcome per requested rule");

        assert_eq!(r.outcomes[0].rule_id, "kw-active");
        assert_eq!(r.outcomes[0].outcome, BulkOutcomeKind::Deactivated);
        assert_eq!(r.outcomes[0].version, Some(1));

        assert_eq!(r.outcomes[1].rule_id, "kw-known");
        assert_eq!(r.outcomes[1].outcome, BulkOutcomeKind::NotActive);
        assert_eq!(r.outcomes[1].version, None);

        assert_eq!(r.outcomes[2].rule_id, "ghost.unknown");
        assert_eq!(r.outcomes[2].outcome, BulkOutcomeKind::UnknownRule);
        assert_eq!(r.outcomes[2].version, None);

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// FR-011: exactly one of `pack` / `rule_ids` must be supplied; zero or
/// both is a teaching error naming both selectors.
#[test]
fn deactivate_bulk_requires_exactly_one_selector() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("selector");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        // Neither selector.
        let err = client
            .call(
                1,
                IpcRequest::RegistryDeactivateBulk(RegistryDeactivateBulkParams {
                    pack: None,
                    rule_ids: None,
                    scope: ActivationScope::Global,
                }),
            )
            .await
            .expect_err("zero selectors must be refused");
        assert_eq!(err.code, IpcErrorCode::RuleInvalid);
        assert!(
            err.message.contains("pack") && err.message.contains("rule_ids"),
            "the error must name BOTH selectors; got: {}",
            err.message
        );

        // Both selectors.
        let err = client
            .call(
                2,
                IpcRequest::RegistryDeactivateBulk(RegistryDeactivateBulkParams {
                    pack: Some("cargo".to_owned()),
                    rule_ids: Some(vec!["a".to_owned()]),
                    scope: ActivationScope::Global,
                }),
            )
            .await
            .expect_err("both selectors must be refused");
        assert_eq!(err.code, IpcErrorCode::RuleInvalid);

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// FR-011: rebinding live jobs runs ONCE after the loop, not per rule.
/// With a single live job and two deactivated rules, `jobs_rebound` is 1
/// (a per-rule rebind would report 2).
#[test]
fn deactivate_bulk_rebinds_live_jobs_once() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("rebind-once");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        // Two rules, both active under global scope.
        upsert_and_activate(&client, 1, "kw-r1").await;
        upsert_and_activate(&client, 3, "kw-r2").await;

        // ONE live command job (global scope reaches it).
        let start = client
            .call(
                5,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: vec!["sleep".to_owned(), "2".to_owned()],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: Some(5_000),
                    tag: None,
                    dedup_nonce: Some("bulk-rebind-once".to_owned()),
                    strip_ansi: true,
                }),
            )
            .await
            .expect("start sleeper");
        assert!(matches!(start, IpcResponse::CommandStartCombed(_)));

        // Bulk-deactivate BOTH rules in one call. Rebinding runs ONCE
        // after the loop, so the single live job is rebound exactly once
        // -- jobs_rebound == 1, NOT 2 (which a per-rule rebind would give).
        let resp = client
            .call(
                6,
                IpcRequest::RegistryDeactivateBulk(RegistryDeactivateBulkParams {
                    pack: None,
                    rule_ids: Some(vec!["kw-r1".to_owned(), "kw-r2".to_owned()]),
                    scope: ActivationScope::Global,
                }),
            )
            .await
            .expect("bulk deactivate");
        let r = match resp {
            IpcResponse::RegistryDeactivateBulk(r) => r,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(r.outcomes.len(), 2);
        assert!(
            r.outcomes
                .iter()
                .all(|o| o.outcome == BulkOutcomeKind::Deactivated),
            "both rules were active and must be deactivated, got: {:?}",
            r.outcomes
        );
        assert_eq!(
            r.jobs_rebound, 1,
            "the single live job must be rebound exactly ONCE after the loop, not per rule"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}
