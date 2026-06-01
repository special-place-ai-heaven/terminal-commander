// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Persistent audit log (TC35).
//!
//! Replaces the in-memory `AuditPlaceholder` introduced at TC21. The
//! `AuditSink` trait abstracts the emission target so the Router can
//! emit audit rows whether the daemon is running against a persistent
//! [`PersistentAudit`] (production) or an in-memory
//! [`InMemoryAudit`] (tests / library smoke).
//!
//! Source-status: live (TC35). Closed-set decision strings are
//! enforced by the store layer
//! ([`terminal_commander_store::ALLOWED_AUDIT_DECISIONS`]). Bounded
//! metadata caps are enforced there as well.

use parking_lot::Mutex;
use terminal_commander_store::{AuditEntry, AuditReadRequest, AuditRow, EventStoreError};

use crate::store_actor::{StoreClient, StoreOp, StoreReply};

/// Emission target for runtime audit rows. Implementations must be
/// thread-safe (`Send + Sync`).
pub trait AuditSink: Send + Sync + std::fmt::Debug {
    /// Emit a single audit row. Implementations MUST NOT panic on a
    /// validation error; they should record the failure (if possible)
    /// and continue. Callers should not block on the return value.
    fn emit(&self, entry: &AuditEntry) -> Result<u64, EventStoreError>;

    /// Optional: read recent rows. Returns an error if the sink does
    /// not support reads (the in-memory sink does; an append-only
    /// sink may not).
    fn read_since(&self, request: &AuditReadRequest) -> Result<Vec<AuditRow>, EventStoreError>;

    /// Optional: number of rows currently recorded.
    fn len(&self) -> usize;

    /// Optional: convenience.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Persistent audit sink backed by the store actor.
pub struct PersistentAudit {
    store: StoreClient,
}

impl std::fmt::Debug for PersistentAudit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentAudit")
            .field("store", &"StoreClient")
            .finish()
    }
}

impl PersistentAudit {
    /// Construct over a [`StoreClient`] handle. The client MUST be
    /// writable; reader-only stores will surface an error on first emit.
    #[must_use]
    pub const fn new(store: StoreClient) -> Self {
        Self { store }
    }

    /// Apply the V0003 audit migration eagerly. Call once at boot if
    /// you want migration cost out of the first emit's critical path.
    pub fn ensure_migration(&self) -> Result<(), EventStoreError> {
        match self.store.call(StoreOp::EnsureAudit)? {
            StoreReply::Unit => Ok(()),
            other => Err(unexpected_reply("EnsureAudit", &other)),
        }
    }
}

impl AuditSink for PersistentAudit {
    fn emit(&self, entry: &AuditEntry) -> Result<u64, EventStoreError> {
        match self.store.call(StoreOp::RecordAudit {
            entry: entry.clone(),
        })? {
            StoreReply::AuditId(id) => Ok(id),
            other => Err(unexpected_reply("RecordAudit", &other)),
        }
    }

    fn read_since(&self, request: &AuditReadRequest) -> Result<Vec<AuditRow>, EventStoreError> {
        match self.store.call(StoreOp::AuditSince {
            request: request.clone(),
        })? {
            StoreReply::AuditRows(rows) => Ok(rows),
            other => Err(unexpected_reply("AuditSince", &other)),
        }
    }

    fn len(&self) -> usize {
        self.store
            .call(StoreOp::AuditCount)
            .ok()
            .and_then(|reply| match reply {
                StoreReply::AuditCount(n) => usize::try_from(n).ok(),
                _ => None,
            })
            .unwrap_or(0)
    }
}

fn unexpected_reply(op: &str, reply: &StoreReply) -> EventStoreError {
    EventStoreError::Unavailable(format!(
        "store actor {op}: unexpected reply variant {reply:?}"
    ))
}

/// In-memory audit sink. Test-only by default; the router also uses
/// it when no persistent store is configured (e.g. unit tests of the
/// router itself).
#[derive(Debug, Default)]
pub struct InMemoryAudit {
    rows: Mutex<Vec<AuditRow>>,
    next_id: Mutex<u64>,
}

impl InMemoryAudit {
    /// Construct an empty in-memory audit sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the current rows. Useful for tests.
    #[must_use]
    pub fn snapshot(&self) -> Vec<AuditRow> {
        self.rows.lock().clone()
    }
}

impl AuditSink for InMemoryAudit {
    fn emit(&self, entry: &AuditEntry) -> Result<u64, EventStoreError> {
        // Mirror the store's closed-set check so tests catch drift.
        if !terminal_commander_store::ALLOWED_AUDIT_DECISIONS.contains(&entry.decision.as_str()) {
            return Err(EventStoreError::InvalidPayload(format!(
                "audit decision '{}' is not in the closed set",
                entry.decision
            )));
        }
        let mut id_g = self.next_id.lock();
        *id_g += 1;
        let id = *id_g;
        drop(id_g);
        let row = AuditRow {
            audit_id: id,
            timestamp: time::OffsetDateTime::now_utc(),
            action: entry.action.clone(),
            subject: entry.subject.clone(),
            decision: entry.decision.clone(),
            profile: entry.profile.clone(),
            reason: entry.reason.clone(),
            actor: entry.actor.clone(),
            metadata_json: entry.metadata_json.clone(),
        };
        self.rows.lock().push(row);
        Ok(id)
    }

    fn read_since(&self, request: &AuditReadRequest) -> Result<Vec<AuditRow>, EventStoreError> {
        let g = self.rows.lock();
        let limit = request
            .limit
            .unwrap_or(terminal_commander_store::DEFAULT_AUDIT_READ_LIMIT)
            .clamp(1, terminal_commander_store::MAX_AUDIT_READ_LIMIT);
        let action_f = request.action_filter.as_deref();
        let decision_f = request.decision_filter.as_deref();
        let out: Vec<AuditRow> = g
            .iter()
            .filter(|r| r.audit_id > request.cursor)
            .filter(|r| action_f.is_none_or(|a| r.action == a))
            .filter(|r| decision_f.is_none_or(|d| r.decision == d))
            .take(limit)
            .cloned()
            .collect();
        Ok(out)
    }

    fn len(&self) -> usize {
        self.rows.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inmem_emit_and_read() {
        let s = InMemoryAudit::new();
        let id = s
            .emit(
                &AuditEntry::new("bucket_create", "bkt_x", "info")
                    .with_actor("router")
                    .with_profile("developer_local"),
            )
            .unwrap();
        assert!(id >= 1);
        assert_eq!(s.len(), 1);
        let rows = s.read_since(&AuditReadRequest::new(0)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, "bucket_create");
        assert_eq!(rows[0].decision, "info");
    }

    #[test]
    fn inmem_rejects_unknown_decision() {
        let s = InMemoryAudit::new();
        let err = s
            .emit(&AuditEntry::new("anything", "x", "totally_bogus"))
            .unwrap_err();
        assert!(format!("{err}").contains("closed set"));
    }

    #[test]
    fn inmem_cursor_and_filters() {
        let s = InMemoryAudit::new();
        s.emit(&AuditEntry::new("bucket_create", "bkt_x", "info"))
            .unwrap();
        s.emit(&AuditEntry::new("bucket_append", "bkt_x", "info"))
            .unwrap();
        s.emit(
            &AuditEntry::new("registry_activate", "rule_y", "allow_with_audit")
                .with_profile("admin_debug"),
        )
        .unwrap();
        let r = s
            .read_since(&AuditReadRequest {
                cursor: 0,
                action_filter: Some("bucket_append".to_owned()),
                decision_filter: None,
                limit: None,
            })
            .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].action, "bucket_append");
        let r2 = s
            .read_since(&AuditReadRequest {
                cursor: 0,
                action_filter: None,
                decision_filter: Some("allow_with_audit".to_owned()),
                limit: None,
            })
            .unwrap();
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].action, "registry_activate");
    }
}
