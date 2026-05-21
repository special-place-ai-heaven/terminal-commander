// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Persistent audit log (TC35).
//!
//! Lives in the same SQLite file as the event store and rule
//! registry. Migration V0003 is applied lazily via
//! [`EventStore::ensure_audit`] using the same pattern as
//! [`EventStore::ensure_registry`].
//!
//! Source-status: live (TC35) for insert + query. The audit log
//! replaces the in-memory `AuditPlaceholder` seam from TC21. The
//! daemon side wires `PersistentAudit` over this API.
//!
//! Bounded metadata: any `metadata_json` payload is rejected if it
//! exceeds [`MAX_AUDIT_METADATA_BYTES`]. Reads are bounded by
//! [`MAX_AUDIT_READ_LIMIT`].

use rusqlite::{OptionalExtension, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::{EventStore, EventStoreError, Result};

/// Embedded V0003 migration. Same manual runner pattern as V0001 / V0002.
const MIGRATION_V0003: &str = include_str!("../migrations/V0003__audit.sql");

/// Hard cap on `metadata_json` bytes per row. Audit must never become
/// a backdoor for raw stream content or large blobs.
pub const MAX_AUDIT_METADATA_BYTES: usize = 4096;

/// Default read limit.
pub const DEFAULT_AUDIT_READ_LIMIT: usize = 200;
/// Hard read limit; mirrors event-store `MAX_READ_LIMIT` posture.
pub const MAX_AUDIT_READ_LIMIT: usize = 10_000;

/// Hard cap on `reason` bytes per row.
pub const MAX_AUDIT_REASON_BYTES: usize = 1024;

/// Hard cap on `subject` bytes per row.
pub const MAX_AUDIT_SUBJECT_BYTES: usize = 1024;

/// Closed-set decision label written to the `decision` column.
///
/// The store layer rejects rows whose decision string is not in this
/// set. The daemon-side audit emitter maps `PolicyDecision`
/// variants to these labels.
pub const ALLOWED_AUDIT_DECISIONS: &[&str] =
    &["allow", "deny", "allow_with_audit", "error", "info"];

/// Inputs for a new audit row. `action` and `subject` are required;
/// the other fields are optional but recorded when present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub action: String,
    pub subject: String,
    pub decision: String,
    pub profile: Option<String>,
    pub reason: Option<String>,
    pub actor: Option<String>,
    pub metadata_json: Option<String>,
}

impl AuditEntry {
    /// Construct a minimal entry with the canonical required fields.
    #[must_use]
    pub fn new(
        action: impl Into<String>,
        subject: impl Into<String>,
        decision: impl Into<String>,
    ) -> Self {
        Self {
            action: action.into(),
            subject: subject.into(),
            decision: decision.into(),
            profile: None,
            reason: None,
            actor: None,
            metadata_json: None,
        }
    }

    /// Attach a profile label (e.g. `"developer_local"`).
    #[must_use]
    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Attach a short reason. Truncated to [`MAX_AUDIT_REASON_BYTES`]
    /// at insert time if longer.
    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Attach an actor label (e.g. `"mcp"`, `"cli"`).
    #[must_use]
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    /// Attach pre-serialized JSON metadata. Rejected at insert time
    /// if it exceeds [`MAX_AUDIT_METADATA_BYTES`].
    #[must_use]
    pub fn with_metadata_json(mut self, metadata: impl Into<String>) -> Self {
        self.metadata_json = Some(metadata.into());
        self
    }
}

/// One audit row as read back from the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    pub audit_id: u64,
    pub timestamp: OffsetDateTime,
    pub action: String,
    pub subject: String,
    pub decision: String,
    pub profile: Option<String>,
    pub reason: Option<String>,
    pub actor: Option<String>,
    pub metadata_json: Option<String>,
}

/// Cursor-based read request.
#[derive(Debug, Clone)]
pub struct AuditReadRequest {
    /// Return rows with `audit_id > cursor`.
    pub cursor: u64,
    /// Optional filter by action.
    pub action_filter: Option<String>,
    /// Optional filter by decision.
    pub decision_filter: Option<String>,
    /// Result count cap; clamped to [`MAX_AUDIT_READ_LIMIT`].
    pub limit: Option<usize>,
}

impl AuditReadRequest {
    /// Construct a minimal request reading from `cursor` with no
    /// filters and the default limit.
    #[must_use]
    pub const fn new(cursor: u64) -> Self {
        Self {
            cursor,
            action_filter: None,
            decision_filter: None,
            limit: None,
        }
    }
}

impl EventStore {
    /// Apply the V0003 audit migration. Idempotent. Safe to call on
    /// every store-using boot.
    pub fn ensure_audit(&mut self) -> Result<()> {
        let already: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 3",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if already == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(MIGRATION_V0003)
                .map_err(|e| EventStoreError::Migration(e.to_string()))?;
            let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (3, ?1)",
                params![now_s],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Insert a new audit row. Validates bounded sizes and the closed
    /// decision set. Returns the assigned `audit_id`.
    pub fn record_audit(&mut self, entry: &AuditEntry) -> Result<u64> {
        self.ensure_audit()?;

        // Closed-set decision validation. Reject silent drift.
        if !ALLOWED_AUDIT_DECISIONS.contains(&entry.decision.as_str()) {
            return Err(EventStoreError::InvalidPayload(format!(
                "audit decision '{}' is not in the closed set",
                entry.decision
            )));
        }
        if entry.action.is_empty() {
            return Err(EventStoreError::InvalidPayload(
                "audit action must not be empty".to_owned(),
            ));
        }
        if entry.subject.len() > MAX_AUDIT_SUBJECT_BYTES {
            return Err(EventStoreError::InvalidPayload(format!(
                "audit subject exceeds MAX_AUDIT_SUBJECT_BYTES={MAX_AUDIT_SUBJECT_BYTES}"
            )));
        }
        if let Some(m) = entry.metadata_json.as_ref()
            && m.len() > MAX_AUDIT_METADATA_BYTES
        {
            return Err(EventStoreError::InvalidPayload(format!(
                "audit metadata exceeds MAX_AUDIT_METADATA_BYTES={MAX_AUDIT_METADATA_BYTES}"
            )));
        }

        // Reason is truncated rather than rejected to keep audit
        // emission infallible at typical call sites.
        let reason_trimmed: Option<String> = entry.reason.as_ref().map(|r| {
            if r.len() <= MAX_AUDIT_REASON_BYTES {
                r.clone()
            } else {
                let mut end = MAX_AUDIT_REASON_BYTES;
                while !r.is_char_boundary(end) && end > 0 {
                    end -= 1;
                }
                r[..end].to_owned()
            }
        });

        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        self.conn.execute(
            "INSERT INTO audit_records
              (timestamp, action, subject, decision, profile, reason, actor, metadata_json)
             VALUES
              (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                now_s,
                entry.action,
                entry.subject,
                entry.decision,
                entry.profile,
                reason_trimmed,
                entry.actor,
                entry.metadata_json,
            ],
        )?;
        let id_i: i64 = self.conn.last_insert_rowid();
        Ok(u64::try_from(id_i).unwrap_or(0))
    }

    /// Read audit rows strictly after `cursor`, ordered by `audit_id`.
    pub fn audit_since(&mut self, request: &AuditReadRequest) -> Result<Vec<AuditRow>> {
        self.ensure_audit()?;
        let limit = request
            .limit
            .unwrap_or(DEFAULT_AUDIT_READ_LIMIT)
            .clamp(1, MAX_AUDIT_READ_LIMIT);
        let cursor_i = i64::try_from(request.cursor).unwrap_or(i64::MAX);
        let action_pat = request.action_filter.as_deref();
        let decision_pat = request.decision_filter.as_deref();
        let mut stmt = self.conn.prepare(
            "SELECT audit_id, timestamp, action, subject, decision,
                    profile, reason, actor, metadata_json
             FROM audit_records
             WHERE audit_id > ?1
               AND (?2 IS NULL OR action = ?2)
               AND (?3 IS NULL OR decision = ?3)
             ORDER BY audit_id ASC LIMIT ?4",
        )?;
        let mut rows = stmt.query(params![
            cursor_i,
            action_pat,
            decision_pat,
            i64::try_from(limit).unwrap_or(i64::MAX),
        ])?;
        let mut out = Vec::with_capacity(limit);
        while let Some(row) = rows.next()? {
            let audit_id_i: i64 = row.get(0)?;
            let ts_s: String = row.get(1)?;
            let timestamp = OffsetDateTime::parse(&ts_s, &Rfc3339)?;
            out.push(AuditRow {
                audit_id: u64::try_from(audit_id_i).unwrap_or(0),
                timestamp,
                action: row.get(2)?,
                subject: row.get(3)?,
                decision: row.get(4)?,
                profile: row.get(5)?,
                reason: row.get(6)?,
                actor: row.get(7)?,
                metadata_json: row.get(8)?,
            });
        }
        Ok(out)
    }

    /// Count audit rows. Operator-side metric; not exposed via MCP.
    pub fn audit_count(&mut self) -> Result<u64> {
        self.ensure_audit()?;
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM audit_records", [], |row| row.get(0))
            .optional()?
            .unwrap_or(0);
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Returns the `(name, type)` of each column on the `audit_records`
    /// table. Used by structural schema tests to prove no BLOB / raw
    /// columns are added without an explicit doctrine change.
    pub fn audit_table_columns(&mut self) -> Result<Vec<(String, String)>> {
        self.ensure_audit()?;
        let mut stmt = self
            .conn
            .prepare("SELECT name, type FROM pragma_table_info('audit_records')")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
