// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Persistent workspace snapshots (P1 / TC50, omni spec 001).
//!
//! A workspace snapshot is a saved, restorable `(cwd + bounded env)`
//! captured from a shell session. It lives in the same SQLite file as the
//! event store and registry. The env map is bounded by the daemon BEFORE
//! it reaches this layer (no unredacted host secrets are persisted) and is
//! stored as a single JSON object string.
//!
//! Source-status: live (P1 / TC50).

use rusqlite::{OptionalExtension, params};
use serde_json as sj;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::{EventStore, EventStoreError, Result};

/// Embedded V0006 migration. Same manual runner pattern as the registry.
const MIGRATION_V0006: &str = include_str!("../migrations/V0006__workspace_snapshot.sql");

/// A persisted workspace snapshot row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshotRow {
    pub snapshot_id: String,
    pub name: Option<String>,
    pub source_session_id: Option<String>,
    pub cwd: Option<String>,
    /// Bounded `(key, value)` env overlay. Bounded by the daemon before
    /// persistence.
    pub env: Vec<(String, String)>,
    pub created_at: OffsetDateTime,
}

impl EventStore {
    /// Run the V0006 workspace-snapshot migration. Idempotent.
    pub fn ensure_workspace(&mut self) -> Result<()> {
        let v6: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 6",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if v6 == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(MIGRATION_V0006)
                .map_err(|e| EventStoreError::Migration(e.to_string()))?;
            let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (6, ?1)",
                params![now_s],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Persist a workspace snapshot. The caller supplies the opaque
    /// `snapshot_id` and a pre-bounded env map. Returns the id on success.
    pub fn create_workspace_snapshot(
        &mut self,
        snapshot_id: &str,
        name: Option<&str>,
        source_session_id: Option<&str>,
        cwd: Option<&str>,
        env: &[(String, String)],
    ) -> Result<String> {
        self.ensure_workspace()?;
        let env_json = sj::to_string(env)?;
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        self.conn.execute(
            "INSERT INTO workspace_snapshots
                (snapshot_id, name, source_session_id, cwd, env_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![snapshot_id, name, source_session_id, cwd, env_json, now_s],
        )?;
        Ok(snapshot_id.to_owned())
    }

    /// Fetch a workspace snapshot by id. Returns `None` if unknown.
    pub fn get_workspace_snapshot(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<WorkspaceSnapshotRow>> {
        self.conn
            .query_row(
                "SELECT snapshot_id, name, source_session_id, cwd, env_json, created_at
                 FROM workspace_snapshots WHERE snapshot_id = ?1",
                params![snapshot_id],
                |row| {
                    let snapshot_id: String = row.get(0)?;
                    let name: Option<String> = row.get(1)?;
                    let source_session_id: Option<String> = row.get(2)?;
                    let cwd: Option<String> = row.get(3)?;
                    let env_json: String = row.get(4)?;
                    let created_at: String = row.get(5)?;
                    Ok((
                        snapshot_id,
                        name,
                        source_session_id,
                        cwd,
                        env_json,
                        created_at,
                    ))
                },
            )
            .optional()?
            .map(
                |(snapshot_id, name, source_session_id, cwd, env_json, created_at)| {
                    let env: Vec<(String, String)> = sj::from_str(&env_json)
                        .map_err(|e| EventStoreError::InvalidPayload(e.to_string()))?;
                    let created_at = OffsetDateTime::parse(&created_at, &Rfc3339)?;
                    Ok(WorkspaceSnapshotRow {
                        snapshot_id,
                        name,
                        source_session_id,
                        cwd,
                        env,
                        created_at,
                    })
                },
            )
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use crate::EventStore;

    fn store() -> EventStore {
        let mut s = EventStore::in_memory().expect("in-memory store");
        s.ensure_workspace().expect("ensure workspace");
        s
    }

    #[test]
    fn create_then_get_round_trips() {
        let mut s = store();
        let env = vec![
            ("FOO".to_owned(), "bar".to_owned()),
            ("BAZ".to_owned(), "qux".to_owned()),
        ];
        let id = s
            .create_workspace_snapshot(
                "snap_test1",
                Some("build"),
                Some("ses_abc"),
                Some("/tmp"),
                &env,
            )
            .expect("create");
        assert_eq!(id, "snap_test1");
        let row = s
            .get_workspace_snapshot("snap_test1")
            .expect("get")
            .expect("present");
        assert_eq!(row.cwd.as_deref(), Some("/tmp"));
        assert_eq!(row.name.as_deref(), Some("build"));
        assert_eq!(row.env, env);
    }

    #[test]
    fn get_unknown_is_none() {
        let s = store();
        assert!(
            s.get_workspace_snapshot("snap_nope")
                .expect("get")
                .is_none()
        );
    }

    #[test]
    fn ensure_workspace_is_idempotent() {
        let mut s = store();
        s.ensure_workspace().expect("second ensure");
    }
}
