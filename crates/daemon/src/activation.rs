// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Runtime activation registry (TC42).
//!
//! `ActivationRegistry` is the daemon's in-memory authority for "which
//! rule versions are currently active". The persistent
//! `rule_activations` table (TC13, V0002 migration) is the durable
//! backing store; this registry is rebuilt from that table at
//! daemon bootstrap and is kept in sync on every activate/deactivate
//! IPC call.
//!
//! Activation scope at TC42 is **global**: a rule that is active
//! applies to every newly-started command. Per-bucket / per-job
//! binding can layer on top later. Already-running probes are NOT
//! hot-rebound; the SifterRuntime captured by `ProcessProbe::spawn`
//! is owned by that probe and cannot be swapped without a new probe
//! API (documented gap in the TC42 report).
//!
//! Source-status: live (TC42) for global pre-spawn activation.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use terminal_commander_core::RuleDefinition;

/// In-memory authority for active rules.
///
/// Keyed by `(rule_id, version)` so the same `rule_id` can be
/// active at multiple historical versions, though in practice a
/// deactivate-then-activate cycle is how operators upgrade.
#[derive(Debug, Default)]
pub struct ActivationRegistry {
    by_key: RwLock<HashMap<(String, u32), RuleDefinition>>,
}

impl ActivationRegistry {
    /// Fresh, empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a registry pre-populated from an arbitrary set of
    /// active rule definitions. Used by `DaemonState::bootstrap`
    /// after `list_active_rule_defs()` returns the persistent set.
    #[must_use]
    pub fn from_defs(defs: Vec<RuleDefinition>) -> Self {
        let me = Self::new();
        for def in defs {
            me.activate(def);
        }
        me
    }

    /// Number of currently-active `(rule_id, version)` entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_key.read().len()
    }

    /// Whether the registry holds zero active rules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_key.read().is_empty()
    }

    /// Insert or replace the active rule for `(rule_id, version)`.
    /// Idempotent: re-activating the same key replaces the stored
    /// definition.
    pub fn activate(&self, def: RuleDefinition) {
        let key = (def.id.clone(), def.version);
        self.by_key.write().insert(key, def);
    }

    /// Remove `(rule_id, version)` from the active set. Returns
    /// whether something was removed so the caller can distinguish
    /// "wasn't active" from a real deactivation.
    pub fn deactivate(&self, rule_id: &str, version: u32) -> bool {
        self.by_key
            .write()
            .remove(&(rule_id.to_owned(), version))
            .is_some()
    }

    /// Whether `(rule_id, version)` is currently active.
    #[must_use]
    pub fn is_active(&self, rule_id: &str, version: u32) -> bool {
        self.by_key
            .read()
            .contains_key(&(rule_id.to_owned(), version))
    }

    /// Snapshot of every active rule definition. Returned cloned so
    /// callers can hold the values across an async boundary without
    /// keeping the registry lock.
    #[must_use]
    pub fn snapshot(&self) -> Vec<RuleDefinition> {
        let g = self.by_key.read();
        let mut out: Vec<RuleDefinition> = g.values().cloned().collect();
        // Deterministic order: by rule_id then version.
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
    }

    #[test]
    fn activate_then_snapshot_returns_rule() {
        let r = ActivationRegistry::new();
        r.activate(rule("a", 1));
        assert!(r.is_active("a", 1));
        assert!(!r.is_active("a", 2));
        let snap = r.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].id, "a");
        assert_eq!(snap[0].version, 1);
    }

    #[test]
    fn deactivate_returns_true_only_when_present() {
        let r = ActivationRegistry::new();
        r.activate(rule("a", 1));
        assert!(r.deactivate("a", 1));
        assert!(!r.deactivate("a", 1));
        assert!(r.is_empty());
    }

    #[test]
    fn from_defs_loads_persistent_state() {
        let r = ActivationRegistry::from_defs(vec![rule("a", 1), rule("b", 1)]);
        assert_eq!(r.len(), 2);
        assert!(r.is_active("a", 1));
        assert!(r.is_active("b", 1));
    }

    #[test]
    fn snapshot_order_is_deterministic() {
        let r = ActivationRegistry::new();
        r.activate(rule("b", 1));
        r.activate(rule("a", 2));
        r.activate(rule("a", 1));
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
        r.activate(def_v1);
        r.activate(def_v1_replaced);
        assert_eq!(r.len(), 1);
        assert_eq!(r.snapshot()[0].summary_template, "second");
    }
}
