// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Persistent job/bucket receipts (P1 / TC-B3, omni spec 001 FR-027).
//!
//! A job receipt is a compact, durable record of a command's terminal
//! transition. It lets a `command_status` poll AFTER a daemon restart --
//! when the in-memory job map is gone -- return a known terminal /
//! restart-marked result instead of a bare `UnknownJob` error (constitution
//! VII: honest degradation). Written on every terminal transition; read by
//! the status handler's fallback path.
//!
//! Lives in the same SQLite file as the event store, registry, and
//! workspace snapshots. `final_signal_counts` is a small JSON object of
//! rule-driven event counts (bounded by the daemon before persistence).
//!
//! Source-status: live (P1 / TC-B3).

use rusqlite::{OptionalExtension, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::{EventStore, EventStoreError, Result};

/// Embedded V0007 migration. Same manual runner pattern as the registry
/// and workspace snapshots.
const MIGRATION_V0007: &str = include_str!("../migrations/V0007__job_receipt.sql");

/// A persisted job/bucket receipt row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobReceiptRow {
    pub job_id: String,
    pub bucket_id: String,
    /// Terminal job state as a lowercase string (`exited` / `cancelled` /
    /// `failed`). Stored as text so the row is human-readable and
    /// decode-tolerant.
    pub terminal_state: String,
    pub exit_code: Option<i32>,
    /// Bounded JSON object of rule-driven signal counts, e.g.
    /// `{"events_emitted":3}`. Opaque to this layer.
    pub final_signal_counts: String,
    /// `Some` once a post-restart read stamped this receipt; `None` while
    /// the originating daemon process is still the one that wrote it.
    pub restarted_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

impl EventStore {
    /// Run the V0007 job-receipt migration. Idempotent.
    pub fn ensure_job_receipts(&mut self) -> Result<()> {
        let v7: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 7",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if v7 == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(MIGRATION_V0007)
                .map_err(|e| EventStoreError::Migration(e.to_string()))?;
            let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (7, ?1)",
                params![now_s],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Persist (or replace) a job receipt on a terminal transition. The
    /// caller supplies the opaque ids, the terminal state string, the exit
    /// code, and the pre-bounded `final_signal_counts` JSON. `INSERT OR
    /// REPLACE` keeps the write idempotent if a transition is recorded
    /// twice (the terminal state never changes after the first write).
    pub fn record_job_receipt(
        &mut self,
        job_id: &str,
        bucket_id: &str,
        terminal_state: &str,
        exit_code: Option<i32>,
        final_signal_counts: &str,
    ) -> Result<()> {
        self.ensure_job_receipts()?;
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO job_receipts
                (job_id, bucket_id, terminal_state, exit_code,
                 final_signal_counts, restarted_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
            params![
                job_id,
                bucket_id,
                terminal_state,
                exit_code,
                final_signal_counts,
                now_s
            ],
        )?;
        Ok(())
    }

    /// Fetch a job receipt by id. Returns `None` if unknown.
    ///
    /// This is the post-restart fallback read: the in-memory job is gone,
    /// so the status handler reads the durable receipt and returns a
    /// restart-marked terminal result rather than a bare error.
    pub fn get_job_receipt(&self, job_id: &str) -> Result<Option<JobReceiptRow>> {
        self.conn
            .query_row(
                "SELECT job_id, bucket_id, terminal_state, exit_code,
                        final_signal_counts, restarted_at, created_at
                 FROM job_receipts WHERE job_id = ?1",
                params![job_id],
                |row| {
                    let job_id: String = row.get(0)?;
                    let bucket_id: String = row.get(1)?;
                    let terminal_state: String = row.get(2)?;
                    let exit_code: Option<i32> = row.get(3)?;
                    let final_signal_counts: String = row.get(4)?;
                    let restarted_at: Option<String> = row.get(5)?;
                    let created_at: String = row.get(6)?;
                    Ok((
                        job_id,
                        bucket_id,
                        terminal_state,
                        exit_code,
                        final_signal_counts,
                        restarted_at,
                        created_at,
                    ))
                },
            )
            .optional()?
            .map(
                |(
                    job_id,
                    bucket_id,
                    terminal_state,
                    exit_code,
                    final_signal_counts,
                    restarted_at,
                    created_at,
                )| {
                    let created_at = OffsetDateTime::parse(&created_at, &Rfc3339)
                        .map_err(|e| EventStoreError::Migration(e.to_string()))?;
                    let restarted_at = restarted_at
                        .map(|s| OffsetDateTime::parse(&s, &Rfc3339))
                        .transpose()
                        .map_err(|e| EventStoreError::Migration(e.to_string()))?;
                    Ok::<JobReceiptRow, EventStoreError>(JobReceiptRow {
                        job_id,
                        bucket_id,
                        terminal_state,
                        exit_code,
                        final_signal_counts,
                        restarted_at,
                        created_at,
                    })
                },
            )
            .transpose()
    }

    /// Stamp a receipt's `restarted_at` to mark that it was read after the
    /// originating daemon process is gone. Best-effort: a read-only handle
    /// or a missing row is a silent no-op (the returned receipt already
    /// carries the restart marker the caller surfaces to the agent).
    pub fn mark_job_receipt_restarted(&mut self, job_id: &str) -> Result<()> {
        if self.is_read_only() {
            return Ok(());
        }
        self.ensure_job_receipts()?;
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        self.conn.execute(
            "UPDATE job_receipts SET restarted_at = ?2
             WHERE job_id = ?1 AND restarted_at IS NULL",
            params![job_id, now_s],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> EventStore {
        let mut s = EventStore::in_memory().expect("open in-memory store");
        s.ensure_job_receipts().expect("migrate job_receipts");
        s
    }

    #[test]
    fn record_then_get_round_trips() {
        let mut s = store();
        s.record_job_receipt(
            "job_abc",
            "bkt_1",
            "exited",
            Some(0),
            r#"{"events_emitted":2}"#,
        )
        .expect("record");
        let r = s.get_job_receipt("job_abc").expect("get").expect("present");
        assert_eq!(r.job_id, "job_abc");
        assert_eq!(r.bucket_id, "bkt_1");
        assert_eq!(r.terminal_state, "exited");
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.final_signal_counts, r#"{"events_emitted":2}"#);
        assert!(r.restarted_at.is_none());
    }

    #[test]
    fn get_unknown_is_none() {
        let s = store();
        assert!(s.get_job_receipt("job_missing").expect("get").is_none());
    }

    #[test]
    fn record_is_idempotent_on_replace() {
        let mut s = store();
        s.record_job_receipt("job_x", "bkt_1", "exited", Some(1), "{}")
            .expect("first");
        s.record_job_receipt("job_x", "bkt_1", "exited", Some(1), "{}")
            .expect("replace");
        let r = s.get_job_receipt("job_x").expect("get").expect("present");
        assert_eq!(r.exit_code, Some(1));
    }

    #[test]
    fn ensure_is_idempotent() {
        let mut s = store();
        s.ensure_job_receipts().expect("second ensure is a no-op");
    }

    #[test]
    fn mark_restarted_stamps_once() {
        let mut s = store();
        s.record_job_receipt("job_r", "bkt_1", "exited", Some(0), "{}")
            .expect("record");
        s.mark_job_receipt_restarted("job_r").expect("mark");
        let r = s.get_job_receipt("job_r").expect("get").expect("present");
        assert!(r.restarted_at.is_some(), "restarted_at must be stamped");
    }

    #[test]
    fn cancelled_state_round_trips_with_null_exit_code() {
        let mut s = store();
        s.record_job_receipt("job_c", "bkt_2", "cancelled", None, "{}")
            .expect("record");
        let r = s.get_job_receipt("job_c").expect("get").expect("present");
        assert_eq!(r.terminal_state, "cancelled");
        assert_eq!(r.exit_code, None);
    }
}
