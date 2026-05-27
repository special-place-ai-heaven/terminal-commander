// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Persistent rule registry (TC13).
//!
//! Lives in the same SQLite file as the event store. Versions are
//! immutable: editing creates a new (rule_id, version+1) row. The
//! latest version is denormalized on the `rules` row.
//!
//! Source-status: live (TC13). MCP `registry_*` tools land in TC24.

#[cfg(test)]
use rusqlite::Connection;
use rusqlite::{OptionalExtension, params};
use serde_json as sj;
use terminal_commander_core::{
    ActivationScope, BucketId, JobId, ProbeId, RuleDefinition, RuleStatus, Severity,
};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::{EventStore, EventStoreError, Result};

/// Default search result limit.
pub const DEFAULT_SEARCH_LIMIT: usize = 50;
/// Hard cap on search results.
pub const MAX_SEARCH_LIMIT: usize = 500;

/// Embedded V0002 migration. Same manual runner pattern as V0001.
const MIGRATION_V0002: &str = include_str!("../migrations/V0002__registry.sql");
/// Embedded V0004 migration: scope columns on rule_activations
/// (TC42c). Runs inside `ensure_registry` so every caller picks up
/// the new schema without a separate bootstrap step.
const MIGRATION_V0004: &str = include_str!("../migrations/V0004__registry_scope.sql");

/// A single search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSearchHit {
    pub rule_id: String,
    pub version: u32,
    pub event_kind: String,
    pub summary_template: String,
    pub tags: Vec<String>,
    pub severity: Severity,
    pub status: RuleStatus,
}

/// Activation record (advisory at MVP). Carries the scope the row
/// was activated under (TC42c). Older rows backfill as `Global`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationRecord {
    pub rule_id: String,
    pub version: u32,
    pub activated_at: OffsetDateTime,
    pub deactivated_at: Option<OffsetDateTime>,
    pub profile: Option<String>,
    pub actor: Option<String>,
    pub scope: ActivationScope,
}

/// One row returned by [`EventStore::list_active_rule_defs_scoped`].
/// Carries the full rule definition + the scope under which it is
/// currently active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRuleDef {
    pub definition: RuleDefinition,
    pub scope: ActivationScope,
}

impl EventStore {
    /// Run the V0002 + V0004 registry migrations. Idempotent.
    pub fn ensure_registry(&mut self) -> Result<()> {
        let v2: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 2",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if v2 == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(MIGRATION_V0002)
                .map_err(|e| EventStoreError::Migration(e.to_string()))?;
            let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (2, ?1)",
                params![now_s],
            )?;
            tx.commit()?;
        }
        // V0004: scoped activation columns. Additive to V0002; safe
        // to run on freshly-migrated databases and on TC42b databases.
        let v4: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 4",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if v4 == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(MIGRATION_V0004)
                .map_err(|e| EventStoreError::Migration(e.to_string()))?;
            let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (4, ?1)",
                params![now_s],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Insert a new version of a rule. Returns the assigned version.
    pub fn create_rule_version(&mut self, def: &RuleDefinition) -> Result<u32> {
        self.ensure_registry()?;
        def.validate()
            .map_err(|e| EventStoreError::InvalidPayload(e.to_string()))?;
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;

        let tx = self.conn.transaction()?;

        // Check tombstoned.
        let tombstoned: i64 = tx
            .query_row(
                "SELECT tombstoned FROM rules WHERE rule_id = ?1",
                params![&def.id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if tombstoned == 1 {
            return Err(EventStoreError::InvalidPayload(format!(
                "rule '{}' is tombstoned; cannot add a new version",
                def.id
            )));
        }

        // Insert or update parent row.
        let latest: i64 = tx
            .query_row(
                "SELECT latest_version FROM rules WHERE rule_id = ?1",
                params![&def.id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let next_version_u = u32::try_from(latest).unwrap_or(0).saturating_add(1);
        let next_version_i = i64::from(next_version_u);

        if latest == 0 {
            tx.execute(
                "INSERT INTO rules (rule_id, latest_version, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
                params![&def.id, next_version_i, &now_s],
            )?;
        } else {
            tx.execute(
                "UPDATE rules SET latest_version = ?1, updated_at = ?2 WHERE rule_id = ?3",
                params![next_version_i, &now_s, &def.id],
            )?;
        }

        let def_json = sj::to_string(def)?;
        let status_s = match def.status {
            RuleStatus::Draft => "draft",
            RuleStatus::Active => "active",
            RuleStatus::Disabled => "disabled",
            RuleStatus::Deprecated => "deprecated",
            RuleStatus::Tombstoned => "tombstoned",
        };
        let kind_s = serde_json::to_string(&def.kind)?;
        let kind_s = kind_s.trim_matches('"').to_owned();

        tx.execute(
            "INSERT INTO rule_versions
              (rule_id, version, status, severity, kind, event_kind, definition, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &def.id,
                next_version_i,
                status_s,
                def.severity.as_str(),
                kind_s,
                &def.event_kind,
                &def_json,
                &now_s,
            ],
        )?;

        // Tags.
        for tag in &def.tags {
            tx.execute(
                "INSERT INTO rule_tags (rule_id, version, tag) VALUES (?1, ?2, ?3)",
                params![&def.id, next_version_i, tag],
            )?;
        }

        // FTS5 row.
        let tags_text = def.tags.join(" ");
        tx.execute(
            "INSERT INTO rule_search (rule_id, event_kind, summary_template, tags_text)
             VALUES (?1, ?2, ?3, ?4)",
            params![&def.id, &def.event_kind, &def.summary_template, &tags_text],
        )?;

        tx.commit()?;
        Ok(next_version_u)
    }

    /// Fetch the latest version of a rule.
    pub fn get_latest_rule(&self, rule_id: &str) -> Result<Option<RuleDefinition>> {
        let latest_opt: Option<i64> = self
            .conn
            .query_row(
                "SELECT latest_version FROM rules WHERE rule_id = ?1",
                params![rule_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(latest) = latest_opt else {
            return Ok(None);
        };
        if latest == 0 {
            return Ok(None);
        }
        self.get_rule_version(rule_id, u32::try_from(latest).unwrap_or(0))
    }

    /// Fetch a specific version of a rule.
    pub fn get_rule_version(&self, rule_id: &str, version: u32) -> Result<Option<RuleDefinition>> {
        let def_json: Option<String> = self
            .conn
            .query_row(
                "SELECT definition FROM rule_versions WHERE rule_id = ?1 AND version = ?2",
                params![rule_id, i64::from(version)],
                |row| row.get(0),
            )
            .optional()?;
        match def_json {
            Some(s) => Ok(Some(sj::from_str(&s)?)),
            None => Ok(None),
        }
    }

    /// List `(version, created_at)` pairs for a rule, oldest first.
    pub fn list_rule_versions(&self, rule_id: &str) -> Result<Vec<(u32, OffsetDateTime)>> {
        let mut stmt = self.conn.prepare(
            "SELECT version, created_at FROM rule_versions WHERE rule_id = ?1 ORDER BY version ASC",
        )?;
        let mut rows = stmt.query(params![rule_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let v: i64 = row.get(0)?;
            let ts_s: String = row.get(1)?;
            let ts = OffsetDateTime::parse(&ts_s, &Rfc3339)?;
            out.push((u32::try_from(v).unwrap_or(0), ts));
        }
        Ok(out)
    }

    /// Bounded text search over rule_versions FTS5.
    pub fn search_rules(&self, query: &str, limit: Option<usize>) -> Result<Vec<RuleSearchHit>> {
        let lim = limit
            .unwrap_or(DEFAULT_SEARCH_LIMIT)
            .clamp(1, MAX_SEARCH_LIMIT);
        let lim_i = i64::try_from(lim).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(
            "SELECT rv.rule_id, rv.version, rv.event_kind, rv.status, rv.severity, rv.definition
             FROM rule_search rs
             JOIN rule_versions rv ON rv.rowid = rs.rowid
             WHERE rule_search MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![query, lim_i])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let rule_id: String = row.get(0)?;
            let version_i: i64 = row.get(1)?;
            let event_kind: String = row.get(2)?;
            let status_s: String = row.get(3)?;
            let severity_s: String = row.get(4)?;
            let definition_s: String = row.get(5)?;
            let def: RuleDefinition = sj::from_str(&definition_s)?;
            let status = parse_status(&status_s)?;
            let severity = Severity::parse(&severity_s)
                .map_err(|e| EventStoreError::InvalidPayload(format!("severity parse: {e}")))?;
            out.push(RuleSearchHit {
                rule_id,
                version: u32::try_from(version_i).unwrap_or(0),
                event_kind,
                summary_template: def.summary_template.clone(),
                tags: def.tags.clone(),
                severity,
                status,
            });
        }
        Ok(out)
    }

    /// Record a globally-scoped activation. Compatibility wrapper
    /// around [`Self::record_activation_scoped`] for older callers.
    pub fn record_activation(
        &mut self,
        rule_id: &str,
        version: u32,
        profile: Option<&str>,
        actor: Option<&str>,
    ) -> Result<()> {
        self.record_activation_scoped(rule_id, version, ActivationScope::Global, profile, actor)
    }

    /// Record a scoped activation (TC42c). Advisory at MVP.
    pub fn record_activation_scoped(
        &mut self,
        rule_id: &str,
        version: u32,
        scope: ActivationScope,
        profile: Option<&str>,
        actor: Option<&str>,
    ) -> Result<()> {
        self.ensure_registry()?;
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        let scope_kind = scope.kind_label();
        let scope_value = scope.value_wire();
        self.conn.execute(
            "INSERT INTO rule_activations
                (rule_id, version, activated_at, profile, actor, scope_kind, scope_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                rule_id,
                i64::from(version),
                now_s,
                profile,
                actor,
                scope_kind,
                scope_value
            ],
        )?;
        Ok(())
    }

    /// List activations for a rule (most recent first). Each row
    /// carries its scope (rows pre-TC42c surface as `Global`).
    pub fn list_activations(&self, rule_id: &str) -> Result<Vec<ActivationRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT rule_id, version, activated_at, deactivated_at, profile, actor, scope_kind, scope_value
             FROM rule_activations WHERE rule_id = ?1 ORDER BY activated_at DESC",
        )?;
        let mut rows = stmt.query(params![rule_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let rule_id: String = row.get(0)?;
            let version_i: i64 = row.get(1)?;
            let act_s: String = row.get(2)?;
            let deact_s: Option<String> = row.get(3)?;
            let profile: Option<String> = row.get(4)?;
            let actor: Option<String> = row.get(5)?;
            let scope_kind: Option<String> = row.get(6).ok();
            let scope_value: Option<String> = row.get(7).ok();
            let act = OffsetDateTime::parse(&act_s, &Rfc3339)?;
            let deact = match deact_s {
                Some(s) => Some(OffsetDateTime::parse(&s, &Rfc3339)?),
                None => None,
            };
            let scope = parse_scope(scope_kind.as_deref(), scope_value.as_deref())?;
            out.push(ActivationRecord {
                rule_id,
                version: u32::try_from(version_i).unwrap_or(0),
                activated_at: act,
                deactivated_at: deact,
                profile,
                actor,
                scope,
            });
        }
        Ok(out)
    }

    /// Close the most recent open globally-scoped activation for
    /// `(rule_id, version)`. Compatibility wrapper around
    /// [`Self::deactivate_rule_scoped`].
    pub fn deactivate_rule(&mut self, rule_id: &str, version: u32) -> Result<bool> {
        self.deactivate_rule_scoped(rule_id, version, ActivationScope::Global)
    }

    /// Close the most recent open activation for
    /// `(rule_id, version, scope)`. "Open" means
    /// `deactivated_at IS NULL`. Returns whether a row was actually
    /// closed (so the caller can distinguish "wasn't active" from a
    /// real deactivation).
    pub fn deactivate_rule_scoped(
        &mut self,
        rule_id: &str,
        version: u32,
        scope: ActivationScope,
    ) -> Result<bool> {
        self.ensure_registry()?;
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        let scope_kind = scope.kind_label();
        let scope_value = scope.value_wire();
        // Match scope_kind exactly. scope_value is matched with NULL
        // semantics so `Global` rows (where scope_value IS NULL)
        // still close.
        let changed = self.conn.execute(
            "UPDATE rule_activations
                SET deactivated_at = ?1
              WHERE rowid IN (
                  SELECT rowid FROM rule_activations
                   WHERE rule_id = ?2
                     AND version = ?3
                     AND scope_kind = ?4
                     AND (
                         (?5 IS NULL AND scope_value IS NULL)
                         OR scope_value = ?5
                     )
                     AND deactivated_at IS NULL
                   ORDER BY activated_at DESC, rowid DESC
                   LIMIT 1
              )",
            params![now_s, rule_id, i64::from(version), scope_kind, scope_value],
        )?;
        Ok(changed > 0)
    }

    /// Load every currently-active rule (one row per open activation,
    /// scope included). Definitions are read from
    /// `rule_versions.definition`. Used by the daemon at bootstrap to
    /// rebuild the in-memory activation registry without trusting the
    /// in-memory state alone.
    pub fn list_active_rule_defs_scoped(&self) -> Result<Vec<ActiveRuleDef>> {
        let mut stmt = self.conn.prepare(
            "SELECT rv.definition, ra.scope_kind, ra.scope_value
               FROM rule_activations ra
               JOIN rule_versions rv
                 ON rv.rule_id = ra.rule_id
                AND rv.version = ra.version
              WHERE ra.deactivated_at IS NULL
              ORDER BY ra.activated_at ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let def_s: String = row.get(0)?;
            let scope_kind: Option<String> = row.get(1).ok();
            let scope_value: Option<String> = row.get(2).ok();
            let def: RuleDefinition = sj::from_str(&def_s)?;
            let scope = parse_scope(scope_kind.as_deref(), scope_value.as_deref())?;
            out.push(ActiveRuleDef {
                definition: def,
                scope,
            });
        }
        Ok(out)
    }

    /// Compatibility wrapper: returns the de-duplicated flat list of
    /// active rule definitions (dropping scope). Older callers that
    /// pre-date TC42c keep working.
    pub fn list_active_rule_defs(&self) -> Result<Vec<RuleDefinition>> {
        let mut seen = std::collections::HashSet::<(String, u32)>::new();
        let scoped = self.list_active_rule_defs_scoped()?;
        let mut out = Vec::with_capacity(scoped.len());
        for ActiveRuleDef { definition, .. } in scoped {
            if seen.insert((definition.id.clone(), definition.version)) {
                out.push(definition);
            }
        }
        Ok(out)
    }

    /// Tombstone a rule. Existing versions remain queryable; new
    /// versions cannot be inserted while tombstoned.
    pub fn tombstone_rule(&mut self, rule_id: &str) -> Result<()> {
        self.ensure_registry()?;
        self.conn.execute(
            "UPDATE rules SET tombstoned = 1, updated_at = ?1 WHERE rule_id = ?2",
            params![OffsetDateTime::now_utc().format(&Rfc3339)?, rule_id],
        )?;
        Ok(())
    }
}

fn parse_status(s: &str) -> Result<RuleStatus> {
    match s {
        "draft" => Ok(RuleStatus::Draft),
        "active" => Ok(RuleStatus::Active),
        "disabled" => Ok(RuleStatus::Disabled),
        "deprecated" => Ok(RuleStatus::Deprecated),
        "tombstoned" => Ok(RuleStatus::Tombstoned),
        // Previously an unknown/corrupt status silently became Draft, which
        // silently deactivates a rule. Error like the sibling parse_scope so a
        // corrupt column is surfaced, not swallowed.
        other => Err(EventStoreError::InvalidPayload(format!(
            "unknown rule status {other:?}"
        ))),
    }
}

/// Reconstruct an [`ActivationScope`] from the persistent
/// `(scope_kind, scope_value)` pair. Rows that pre-date the V0004
/// migration arrive with `scope_kind = "global"` (column default)
/// and `scope_value = NULL`. Unknown / malformed values surface as
/// an [`EventStoreError::InvalidPayload`] so the caller can decide
/// to fail-fast or skip the row.
fn parse_scope(kind: Option<&str>, value: Option<&str>) -> Result<ActivationScope> {
    let kind = kind.unwrap_or("global");
    match kind {
        "global" => Ok(ActivationScope::Global),
        "bucket" => {
            let s = value.ok_or_else(|| {
                EventStoreError::InvalidPayload(
                    "scope_kind=bucket but scope_value is NULL".to_owned(),
                )
            })?;
            let bucket_id = BucketId::parse_wire(s)
                .map_err(|e| EventStoreError::InvalidPayload(format!("bucket scope: {e}")))?;
            Ok(ActivationScope::Bucket { bucket_id })
        }
        "job" => {
            let s = value.ok_or_else(|| {
                EventStoreError::InvalidPayload("scope_kind=job but scope_value is NULL".to_owned())
            })?;
            let job_id = JobId::parse_wire(s)
                .map_err(|e| EventStoreError::InvalidPayload(format!("job scope: {e}")))?;
            Ok(ActivationScope::Job { job_id })
        }
        "probe" => {
            let s = value.ok_or_else(|| {
                EventStoreError::InvalidPayload(
                    "scope_kind=probe but scope_value is NULL".to_owned(),
                )
            })?;
            let probe_id = ProbeId::parse_wire(s)
                .map_err(|e| EventStoreError::InvalidPayload(format!("probe scope: {e}")))?;
            Ok(ActivationScope::Probe { probe_id })
        }
        other => Err(EventStoreError::InvalidPayload(format!(
            "unknown scope_kind '{other}'"
        ))),
    }
}

/// Test helper for in-memory connections: expose the underlying
/// Connection (limited to this crate). Used by registry-specific
/// tests that want to peek schema_migrations.
impl EventStore {
    #[cfg(test)]
    pub(crate) const fn conn(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{ContextHint, RuleStatus, RuleType, SourceStream};

    fn k(id: &str) -> RuleDefinition {
        RuleDefinition {
            id: id.to_owned(),
            version: 1,
            kind: RuleType::Keyword,
            status: RuleStatus::Draft,
            severity: Severity::Medium,
            event_kind: "kw_match".to_owned(),
            stream: None,
            description: None,
            pattern: None,
            keywords: Some(vec!["needle".to_owned()]),
            captures: vec![],
            summary_template: "found needle".to_owned(),
            tags: vec!["test".to_owned()],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    fn r(id: &str, tags: &[&str]) -> RuleDefinition {
        RuleDefinition {
            id: id.to_owned(),
            version: 1,
            kind: RuleType::Regex,
            status: RuleStatus::Draft,
            severity: Severity::High,
            event_kind: "missing_package".to_owned(),
            stream: Some(SourceStream::Stderr),
            description: None,
            pattern: Some(
                r"^E: Unable to locate package (?P<package>[A-Za-z0-9._+-]+)$".to_owned(),
            ),
            keywords: None,
            captures: vec!["package".to_owned()],
            summary_template: "missing ${package}".to_owned(),
            tags: tags.iter().map(|s| (*s).to_owned()).collect(),
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    #[test]
    fn registry_create_version_assigns_v1_then_v2() {
        let mut s = EventStore::in_memory().unwrap();
        s.ensure_registry().unwrap();
        assert_eq!(s.create_rule_version(&k("kw1")).unwrap(), 1);
        assert_eq!(s.create_rule_version(&k("kw1")).unwrap(), 2);
        assert_eq!(s.create_rule_version(&k("kw1")).unwrap(), 3);
    }

    #[test]
    fn registry_get_latest_returns_highest_version() {
        let mut s = EventStore::in_memory().unwrap();
        s.create_rule_version(&k("kw1")).unwrap();
        let mut def2 = k("kw1");
        def2.summary_template = "v2 template".to_owned();
        s.create_rule_version(&def2).unwrap();
        let got = s.get_latest_rule("kw1").unwrap().unwrap();
        assert_eq!(got.summary_template, "v2 template");
    }

    #[test]
    fn registry_get_specific_version() {
        let mut s = EventStore::in_memory().unwrap();
        s.create_rule_version(&k("kw1")).unwrap();
        let mut def2 = k("kw1");
        def2.summary_template = "v2".to_owned();
        s.create_rule_version(&def2).unwrap();
        let v1 = s.get_rule_version("kw1", 1).unwrap().unwrap();
        let v2 = s.get_rule_version("kw1", 2).unwrap().unwrap();
        assert_eq!(v1.summary_template, "found needle");
        assert_eq!(v2.summary_template, "v2");
    }

    #[test]
    fn registry_invalid_rule_rejected() {
        let mut s = EventStore::in_memory().unwrap();
        let mut bad = k("");
        bad.id = String::new(); // empty id
        let err = s.create_rule_version(&bad).unwrap_err();
        assert!(matches!(err, EventStoreError::InvalidPayload(_)));
    }

    #[test]
    fn registry_list_versions_ascending() {
        let mut s = EventStore::in_memory().unwrap();
        s.create_rule_version(&k("kw1")).unwrap();
        s.create_rule_version(&k("kw1")).unwrap();
        s.create_rule_version(&k("kw1")).unwrap();
        let v = s.list_rule_versions("kw1").unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].0, 1);
        assert_eq!(v[2].0, 3);
    }

    #[test]
    fn registry_tag_search_finds_rule() {
        let mut s = EventStore::in_memory().unwrap();
        s.create_rule_version(&r("apt-missing", &["apt", "packaging"]))
            .unwrap();
        s.create_rule_version(&r("cargo-error", &["cargo", "rust"]))
            .unwrap();
        let hits = s.search_rules("apt", None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rule_id, "apt-missing");
    }

    #[test]
    fn registry_search_limit_clamps_to_default() {
        let mut s = EventStore::in_memory().unwrap();
        s.create_rule_version(&r("a", &["apt"])).unwrap();
        s.create_rule_version(&r("b", &["apt"])).unwrap();
        let hits = s.search_rules("apt", Some(1)).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn registry_tombstone_blocks_new_versions() {
        let mut s = EventStore::in_memory().unwrap();
        s.create_rule_version(&k("kw1")).unwrap();
        s.tombstone_rule("kw1").unwrap();
        let err = s.create_rule_version(&k("kw1")).unwrap_err();
        assert!(matches!(err, EventStoreError::InvalidPayload(_)));
    }

    #[test]
    fn registry_record_and_list_activation() {
        let mut s = EventStore::in_memory().unwrap();
        s.create_rule_version(&k("kw1")).unwrap();
        s.record_activation("kw1", 1, Some("developer_local"), Some("operator"))
            .unwrap();
        let acts = s.list_activations("kw1").unwrap();
        assert_eq!(acts.len(), 1);
        assert_eq!(acts[0].profile.as_deref(), Some("developer_local"));
        assert_eq!(acts[0].actor.as_deref(), Some("operator"));
    }

    #[test]
    fn registry_v2_migration_applied_once() {
        let mut s = EventStore::in_memory().unwrap();
        s.ensure_registry().unwrap();
        s.ensure_registry().unwrap(); // idempotent
        let n: i64 = s
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn registry_immutable_versions_no_mutation() {
        // Inserting another version with the same content must
        // still produce a new (rule_id, version+1) row; the v1 row
        // remains queryable with its ORIGINAL definition.
        let mut s = EventStore::in_memory().unwrap();
        s.create_rule_version(&k("kw1")).unwrap();
        let mut v2 = k("kw1");
        v2.summary_template = "edited".to_owned();
        s.create_rule_version(&v2).unwrap();
        let original = s.get_rule_version("kw1", 1).unwrap().unwrap();
        assert_eq!(original.summary_template, "found needle"); // unchanged
    }
}
