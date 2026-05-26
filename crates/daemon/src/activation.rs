// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Runtime activation registry (TC42 + TC42c).
//!
//! `ActivationRegistry` is the daemon's in-memory authority for "which
//! rule versions are currently active, and against which scope". The
//! persistent `rule_activations` table (TC13 schema, V0004 columns)
//! is the durable backing store; this registry is rebuilt from that
//! table at daemon bootstrap and is kept in sync on every
//! activate/deactivate IPC call.
//!
//! Scope:
//!
//! - TC42 introduced this registry keyed by `(rule_id, version)` with
//!   an implicit `Global` scope.
//! - TC42b layered live rebind on top: an activation change reaches
//!   every running command's sifter without restarting the probe.
//! - TC42c (this module) layers a scope discriminator on top of the
//!   key: `(rule_id, version, ActivationScope)`. The same rule may be
//!   active under several disjoint scopes simultaneously; the rebind
//!   path consults the scope when computing the per-job rule set.
//!
//! Source-status: live (TC42c).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use terminal_commander_core::{ActivationScope, BucketId, JobId, ProbeId, RuleDefinition};

/// One in-memory activation entry. Carries the active rule
/// definition AND the scope it was activated under.
#[derive(Debug, Clone)]
pub struct ActivationEntry {
    pub definition: RuleDefinition,
    pub scope: ActivationScope,
}

/// In-memory authority for active rules.
///
/// Keyed by `(rule_id, version, scope)` so the same `(rule_id, version)`
/// pair can be active under several disjoint scopes (e.g. global +
/// one specific bucket). The same `(rule_id, version, scope)` tuple
/// is idempotent: re-activating replaces the stored definition.
#[derive(Debug, Default)]
pub struct ActivationRegistry {
    by_key: RwLock<HashMap<(String, u32, ActivationScope), RuleDefinition>>,
}

impl ActivationRegistry {
    /// Fresh, empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a registry pre-populated from an arbitrary set of
    /// `(definition, scope)` entries. Used by `DaemonState::bootstrap`
    /// after `list_active_rule_defs_scoped()` returns the persistent
    /// set.
    #[must_use]
    pub fn from_entries(entries: Vec<ActivationEntry>) -> Self {
        let me = Self::new();
        for e in entries {
            // Defense in depth against the draft-poison footgun: the IPC
            // activate handler refuses non-Active rules, but a row
            // persisted before that gate existed (or written by any
            // future non-IPC path) must not silently rehydrate into the
            // live set on restart and re-block every command in scope.
            // Skip anything not runtime-eligible.
            if !e.definition.status.is_runtime_eligible() {
                continue;
            }
            me.activate(e.definition, e.scope);
        }
        me
    }

    /// Number of currently-active `(rule_id, version, scope)` entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_key.read().len()
    }

    /// Whether the registry holds zero active entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_key.read().is_empty()
    }

    /// Insert or replace the active entry for `(rule_id, version, scope)`.
    /// Idempotent: re-activating the same key replaces the stored
    /// definition.
    pub fn activate(&self, def: RuleDefinition, scope: ActivationScope) {
        // Defense in depth: never let a non-runtime-eligible rule enter
        // the live activation set, regardless of caller. The IPC handler
        // already rejects Draft activations up front with a typed error;
        // this guard means any other present-or-future caller cannot
        // reintroduce the draft-poison footgun by binding a Draft /
        // Deprecated / Tombstoned definition. A blocked rule simply is
        // not stored, so command starts in scope are never poisoned.
        if !def.status.is_runtime_eligible() {
            debug_assert!(
                false,
                "activate() called with non-eligible rule '{}' v{} (status {:?}); \
                 callers must gate on is_runtime_eligible first",
                def.id, def.version, def.status
            );
            return;
        }
        let key = (def.id.clone(), def.version, scope);
        self.by_key.write().insert(key, def);
    }

    /// Remove `(rule_id, version, scope)` from the active set. Returns
    /// whether something was removed so the caller can distinguish
    /// "wasn't active" from a real deactivation.
    pub fn deactivate(&self, rule_id: &str, version: u32, scope: ActivationScope) -> bool {
        self.by_key
            .write()
            .remove(&(rule_id.to_owned(), version, scope))
            .is_some()
    }

    /// Whether `(rule_id, version, scope)` is currently active.
    #[must_use]
    pub fn is_active(&self, rule_id: &str, version: u32, scope: ActivationScope) -> bool {
        self.by_key
            .read()
            .contains_key(&(rule_id.to_owned(), version, scope))
    }

    /// Snapshot of every active entry, including its scope. Used by
    /// `registry_list_active` so the caller can see exactly which
    /// scopes a rule is bound to.
    #[must_use]
    pub fn snapshot_entries(&self) -> Vec<ActivationEntry> {
        let g = self.by_key.read();
        let mut out: Vec<ActivationEntry> = g
            .iter()
            .map(|((_id, _ver, scope), def)| ActivationEntry {
                definition: def.clone(),
                scope: *scope,
            })
            .collect();
        // Deterministic order: rule_id, version, scope label, scope value.
        out.sort_by(|a, b| {
            a.definition
                .id
                .cmp(&b.definition.id)
                .then(a.definition.version.cmp(&b.definition.version))
                .then(a.scope.kind_label().cmp(b.scope.kind_label()))
                .then(a.scope.value_wire().cmp(&b.scope.value_wire()))
        });
        out
    }

    /// Snapshot of every active rule definition, dropping the scope.
    /// Deduplicates by `(rule_id, version)` so a rule that is active
    /// under both `Global` and a bucket scope appears once. Useful
    /// for backwards-compatible callers that still want a flat list.
    #[must_use]
    pub fn snapshot(&self) -> Vec<RuleDefinition> {
        let g = self.by_key.read();
        let mut seen = std::collections::HashSet::<(String, u32)>::new();
        let mut out: Vec<RuleDefinition> = Vec::with_capacity(g.len());
        for ((id, ver, _scope), def) in g.iter() {
            if seen.insert((id.clone(), *ver)) {
                out.push(def.clone());
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id).then(a.version.cmp(&b.version)));
        out
    }

    /// Resolve every rule definition whose scope matches the given
    /// live job triple. Deduplicates by `(rule_id, version)` so a
    /// rule active under both `Global` and a matching `Bucket` scope
    /// merges to one entry. Order is deterministic by rule id +
    /// version.
    #[must_use]
    pub fn snapshot_for_job(
        &self,
        bucket_id: BucketId,
        job_id: JobId,
        probe_id: ProbeId,
    ) -> Vec<RuleDefinition> {
        let g = self.by_key.read();
        let mut seen = std::collections::HashSet::<(String, u32)>::new();
        let mut out: Vec<RuleDefinition> = Vec::with_capacity(g.len());
        for ((id, ver, scope), def) in g.iter() {
            if scope.matches(bucket_id, job_id, probe_id) && seen.insert((id.clone(), *ver)) {
                out.push(def.clone());
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id).then(a.version.cmp(&b.version)));
        out
    }
}

/// Convenience alias so call sites can write
/// `Arc<ActivationRegistryHandle>` without paying the verbose
/// `Arc<ActivationRegistry>` everywhere.
pub type ActivationRegistryHandle = Arc<ActivationRegistry>;

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{ContextHint, RuleStatus, RuleType, Severity};

    fn rule(id: &str, version: u32) -> RuleDefinition {
        RuleDefinition {
            id: id.to_owned(),
            version,
            kind: RuleType::Keyword,
            status: RuleStatus::Active,
            severity: Severity::Medium,
            event_kind: "kw".to_owned(),
            stream: None,
            description: None,
            pattern: None,
            keywords: Some(vec!["needle".to_owned()]),
            captures: vec![],
            summary_template: "matched".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    #[test]
    fn empty_registry_has_zero_entries() {
        let r = ActivationRegistry::new();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
        assert!(r.snapshot().is_empty());
        assert!(r.snapshot_entries().is_empty());
    }

    #[test]
    fn activate_global_then_snapshot_returns_rule() {
        let r = ActivationRegistry::new();
        r.activate(rule("a", 1), ActivationScope::Global);
        assert!(r.is_active("a", 1, ActivationScope::Global));
        assert!(!r.is_active("a", 2, ActivationScope::Global));
        let snap = r.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].id, "a");
        assert_eq!(snap[0].version, 1);
    }

    #[test]
    fn deactivate_returns_true_only_when_present() {
        let r = ActivationRegistry::new();
        r.activate(rule("a", 1), ActivationScope::Global);
        assert!(r.deactivate("a", 1, ActivationScope::Global));
        assert!(!r.deactivate("a", 1, ActivationScope::Global));
        assert!(r.is_empty());
    }

    #[test]
    fn from_entries_loads_persistent_state() {
        let r = ActivationRegistry::from_entries(vec![
            ActivationEntry {
                definition: rule("a", 1),
                scope: ActivationScope::Global,
            },
            ActivationEntry {
                definition: rule("b", 1),
                scope: ActivationScope::Global,
            },
        ]);
        assert_eq!(r.len(), 2);
        assert!(r.is_active("a", 1, ActivationScope::Global));
        assert!(r.is_active("b", 1, ActivationScope::Global));
    }

    #[test]
    fn snapshot_order_is_deterministic() {
        let r = ActivationRegistry::new();
        r.activate(rule("b", 1), ActivationScope::Global);
        r.activate(rule("a", 2), ActivationScope::Global);
        r.activate(rule("a", 1), ActivationScope::Global);
        let names: Vec<(String, u32)> = r
            .snapshot()
            .into_iter()
            .map(|d| (d.id, d.version))
            .collect();
        assert_eq!(
            names,
            vec![
                ("a".to_owned(), 1),
                ("a".to_owned(), 2),
                ("b".to_owned(), 1)
            ]
        );
    }

    #[test]
    fn re_activating_same_key_replaces_definition() {
        let r = ActivationRegistry::new();
        let mut def_v1 = rule("a", 1);
        def_v1.summary_template = "first".to_owned();
        let mut def_v1_replaced = rule("a", 1);
        def_v1_replaced.summary_template = "second".to_owned();
        r.activate(def_v1, ActivationScope::Global);
        r.activate(def_v1_replaced, ActivationScope::Global);
        assert_eq!(r.len(), 1);
        assert_eq!(r.snapshot()[0].summary_template, "second");
    }

    #[test]
    fn same_rule_can_be_active_under_multiple_scopes() {
        let r = ActivationRegistry::new();
        let bid = BucketId::new();
        r.activate(rule("a", 1), ActivationScope::Global);
        r.activate(rule("a", 1), ActivationScope::Bucket { bucket_id: bid });
        assert_eq!(r.len(), 2);
        assert!(r.is_active("a", 1, ActivationScope::Global));
        assert!(r.is_active("a", 1, ActivationScope::Bucket { bucket_id: bid }));
    }

    #[test]
    fn snapshot_for_job_returns_only_matching_scopes() {
        let r = ActivationRegistry::new();
        let bid_a = BucketId::new();
        let bid_b = BucketId::new();
        let jid = JobId::new();
        let pid = ProbeId::new();
        // Global rule applies to both.
        r.activate(rule("global", 1), ActivationScope::Global);
        // Bucket A only.
        r.activate(
            rule("a_only", 1),
            ActivationScope::Bucket { bucket_id: bid_a },
        );
        // Bucket B only.
        r.activate(
            rule("b_only", 1),
            ActivationScope::Bucket { bucket_id: bid_b },
        );
        let snap_a = r.snapshot_for_job(bid_a, jid, pid);
        let ids_a: Vec<&str> = snap_a.iter().map(|d| d.id.as_str()).collect();
        assert!(ids_a.contains(&"global"));
        assert!(ids_a.contains(&"a_only"));
        assert!(!ids_a.contains(&"b_only"));
    }

    #[test]
    fn snapshot_dedupes_same_rule_across_scopes() {
        let r = ActivationRegistry::new();
        let bid = BucketId::new();
        let jid = JobId::new();
        let pid = ProbeId::new();
        r.activate(rule("dup", 1), ActivationScope::Global);
        r.activate(rule("dup", 1), ActivationScope::Bucket { bucket_id: bid });
        // snapshot() dedupes by (rule_id, version).
        assert_eq!(r.snapshot().len(), 1);
        // snapshot_for_job dedupes too even if both scopes match.
        let merged = r.snapshot_for_job(bid, jid, pid);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "dup");
    }

    #[test]
    fn snapshot_entries_keeps_all_scopes() {
        let r = ActivationRegistry::new();
        let bid = BucketId::new();
        r.activate(rule("a", 1), ActivationScope::Global);
        r.activate(rule("a", 1), ActivationScope::Bucket { bucket_id: bid });
        let entries = r.snapshot_entries();
        assert_eq!(entries.len(), 2);
        let kinds: Vec<&str> = entries.iter().map(|e| e.scope.kind_label()).collect();
        assert!(kinds.contains(&"global"));
        assert!(kinds.contains(&"bucket"));
    }
}
