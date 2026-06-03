// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Data-integrity regression: the persistent activation table must hold
//! AT MOST ONE open row per `(rule_id, version, scope)`, mirroring the
//! daemon's in-memory `ActivationRegistry` (a single-entry upsert keyed
//! on that tuple).
//!
//! The bug this guards against: `record_activation_scoped` used to
//! INSERT unconditionally on every activate, and `deactivate_rule_scoped`
//! closed only ONE open row (`LIMIT 1`). So two activates left two open
//! rows; one deactivate closed one; the surviving open row re-hydrated on
//! bootstrap, resurrecting a rule the operator deactivated. These tests
//! pin the durable store to the idempotent in-memory model.

use std::path::PathBuf;

use terminal_commander_core::{
    ActivationScope, ContextHint, RuleDefinition, RuleStatus, RuleType, Severity,
};
use terminal_commander_store::EventStore;

fn tmp_db_path(suffix: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-reg-openrow-{pid}-{nanos}-{suffix}.db"));
    p
}

fn cleanup(p: &PathBuf) {
    let _ = std::fs::remove_file(p);
    let _ = std::fs::remove_file(p.with_extension("db-wal"));
    let _ = std::fs::remove_file(p.with_extension("db-shm"));
}

fn kw_rule(id: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: "openrow_match".to_owned(),
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

/// Activating the same `(rule_id, version, scope)` twice must leave
/// exactly ONE open row, and a single deactivate must then close it,
/// leaving the rule inactive. This is the precise resurrection sequence:
/// activate x2 -> deactivate x1 -> (would-be) restart.
#[test]
fn double_activate_then_single_deactivate_leaves_no_open_row() {
    let p = tmp_db_path("double-then-deact");
    let mut s = EventStore::with_writer(&p).unwrap();
    let version = s.create_rule_version(&kw_rule("kw-openrow")).unwrap();

    // Activate the same key twice. The second is an idempotent no-op at
    // the store layer (open-row guard), so only one open row exists.
    s.record_activation_scoped(
        "kw-openrow",
        version,
        ActivationScope::Global,
        Some("developer_local"),
        Some("test"),
    )
    .unwrap();
    s.record_activation_scoped(
        "kw-openrow",
        version,
        ActivationScope::Global,
        Some("developer_local"),
        Some("test"),
    )
    .unwrap();

    // After two activates, exactly one activation is open.
    let active = s.list_active_rule_defs_scoped().unwrap();
    assert_eq!(
        active.len(),
        1,
        "two activates of one key must leave exactly one open row, got {}",
        active.len()
    );

    // A single deactivate must close it.
    let closed = s
        .deactivate_rule_scoped("kw-openrow", version, ActivationScope::Global)
        .unwrap();
    assert!(closed, "deactivate must report it closed a row");

    // The rule is now inactive in the durable store -- a restart that
    // re-hydrates from `list_active_rule_defs_scoped` sees nothing. This
    // is the anti-resurrection invariant.
    let active_after = s.list_active_rule_defs_scoped().unwrap();
    assert!(
        active_after.is_empty(),
        "after activate x2 then deactivate x1, NO open row may survive \
         (the rule must NOT resurrect on restart); got {} open row(s)",
        active_after.len()
    );

    drop(s);
    cleanup(&p);
}

/// Re-activation history: activate -> deactivate -> activate produces one
/// CLOSED row (audit trail) and one OPEN row. A final deactivate must
/// leave ZERO open rows. This proves (a) the history trail is preserved
/// across a deactivate/re-activate cycle and (b) the final deactivate
/// closes the surviving open row so nothing resurrects on restart.
#[test]
fn deactivate_closes_all_open_rows_for_key() {
    let p = tmp_db_path("close-all");
    let mut s = EventStore::with_writer(&p).unwrap();
    let version = s.create_rule_version(&kw_rule("kw-closeall")).unwrap();

    // activate -> deactivate -> activate: a legitimate history that
    // produces one CLOSED row and one OPEN row.
    s.record_activation_scoped("kw-closeall", version, ActivationScope::Global, None, None)
        .unwrap();
    assert!(
        s.deactivate_rule_scoped("kw-closeall", version, ActivationScope::Global)
            .unwrap()
    );
    s.record_activation_scoped("kw-closeall", version, ActivationScope::Global, None, None)
        .unwrap();

    // Exactly one open row right now (the re-activation). History row is
    // closed.
    assert_eq!(
        s.list_active_rule_defs_scoped().unwrap().len(),
        1,
        "re-activation after deactivate must produce exactly one open row"
    );

    // Final deactivate clears it; no open row may survive.
    assert!(
        s.deactivate_rule_scoped("kw-closeall", version, ActivationScope::Global)
            .unwrap()
    );
    assert!(
        s.list_active_rule_defs_scoped().unwrap().is_empty(),
        "deactivate must leave zero open rows for the key"
    );

    drop(s);
    cleanup(&p);
}

/// Scope isolation: the open-row guard and "close all" semantics are
/// per-scope. Activating one rule under Global must not be closed by a
/// deactivate under a Bucket scope, and vice versa.
#[test]
fn open_row_guard_and_close_all_are_scoped() {
    let p = tmp_db_path("scoped");
    let mut s = EventStore::with_writer(&p).unwrap();
    let version = s.create_rule_version(&kw_rule("kw-scoped")).unwrap();
    let bucket = terminal_commander_core::BucketId::new();

    s.record_activation_scoped("kw-scoped", version, ActivationScope::Global, None, None)
        .unwrap();
    s.record_activation_scoped(
        "kw-scoped",
        version,
        ActivationScope::Bucket { bucket_id: bucket },
        None,
        None,
    )
    .unwrap();

    // Two distinct scopes -> two open rows.
    assert_eq!(
        s.list_active_rule_defs_scoped().unwrap().len(),
        2,
        "the same rule under two disjoint scopes must yield two open rows"
    );

    // Deactivating Global must leave the Bucket row open.
    assert!(
        s.deactivate_rule_scoped("kw-scoped", version, ActivationScope::Global)
            .unwrap()
    );
    let remaining = s.list_active_rule_defs_scoped().unwrap();
    assert_eq!(
        remaining.len(),
        1,
        "deactivating one scope must not close another scope's open row"
    );
    assert_eq!(
        remaining[0].scope,
        ActivationScope::Bucket { bucket_id: bucket },
        "the surviving open row must be the Bucket-scoped one"
    );

    drop(s);
    cleanup(&p);
}
