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

/// Embedded V0008 migration: outcome evidence (`metrics_json`, `end_cause`)
/// plus the `(action, subject)` audit index that lost-detection needs.
const MIGRATION_V0008: &str = include_str!("../migrations/V0008__outcome_evidence.sql");

/// The one `end_cause` value that is NOT a real terminal outcome: the job was
/// ended by daemon shutdown or stale replacement rather than reaching its own
/// conclusion.
///
/// Load-bearing beyond labelling: `record_job_receipt` keys its asymmetric
/// write discipline off this exact value, so an abandonment can never clobber
/// a genuine receipt. The SQL `WHERE job_receipts.end_cause = 'abandoned'`
/// clause must stay in step with it -- pinned by
/// `abandoned_constant_matches_the_sql_literal`.
pub const ABANDONED_END_CAUSE: &str = "abandoned";

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
    /// Bounded JSON object of NUMERIC AND IDENTIFIER evidence captured at the
    /// terminal transition -- frames, bytes, suppression counts, duration,
    /// probe id. `None` for rows written before V0008; such a row is reported
    /// honestly rather than as zeroes-presented-as-observations.
    ///
    /// Carries NO frame text. Constitution III bars raw frames from persistent
    /// output, which is why the no-silence tail is not persisted (decision D2).
    pub metrics_json: Option<String>,
    /// Why the job ended, when that is not self-evident from `terminal_state`.
    /// Today the only value is `abandoned` (ended by daemon shutdown or stale
    /// replacement). Abandonment is deliberately NOT a `JobState` variant
    /// (decision D1): the lifecycle state stays the truthful `cancelled`.
    pub end_cause: Option<String>,
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

    /// Run the V0008 outcome-evidence migration. Idempotent.
    ///
    /// Depends on BOTH prior tables existing: it alters `job_receipts` (V0007)
    /// and indexes `audit_records` (V0003). Both `ensure_*` calls below are
    /// themselves idempotent, so this is safe to call on every boot and in any
    /// order relative to other `ensure_*` functions.
    pub fn ensure_outcome_evidence(&mut self) -> Result<()> {
        self.ensure_job_receipts()?;
        self.ensure_audit()?;
        let v8: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 8",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if v8 == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(MIGRATION_V0008)
                .map_err(|e| EventStoreError::Migration(e.to_string()))?;
            let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (8, ?1)",
                params![now_s],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Persist a job receipt on a terminal transition.
    ///
    /// ## Write discipline is ASYMMETRIC, and that is load-bearing
    ///
    /// spec 004 / review finding (kimi-k3 HIGH-2). `record_abandoned_jobs`
    /// scans the ledger and then writes, so its decision is already stale by
    /// the time it lands; PTY waiters are not drained at all, and the
    /// `QuiesceForReplace` path runs on a live, busy, undrained daemon. With a
    /// symmetric `INSERT OR REPLACE` a job that exits DURING that window has
    /// its real receipt (`exited`, `0`, full evidence) overwritten by
    /// `cancelled / abandoned / exit_code NULL` -- destroying a genuine pass
    /// and reintroducing the precise harm this feature exists to remove.
    ///
    /// So:
    /// - the first REAL terminal outcome wins, and heals a stray abandonment;
    ///   a later observation may refresh evidence for that same outcome, but
    ///   cannot replace it with a conflicting state or exit code;
    /// - an ABANDONMENT inserts only when no row exists, and may overwrite
    ///   only another abandonment (keeping a double quiesce idempotent). It
    ///   can never clobber a real outcome.
    ///
    /// This closes the race deterministically at the storage layer, so it
    /// holds regardless of scheduling, drain coverage, or call ordering --
    /// none of which the writer can guarantee.
    ///
    /// `metrics_json` MUST contain only numeric and identifier evidence. Frame
    /// text is forbidden here by constitution III (no raw frames in persistent
    /// output); the caller is responsible for honouring that.
    // Eight positional parameters, one over the clippy default. Grouping them
    // into a params struct was considered and rejected: the receipt fields are
    // a flat, stable row shape, and the two adjacent `Option<&str>` arguments
    // are pinned by round-trip tests below (a swap fails
    // `abandoned_end_cause_round_trips_without_an_exit_code`). Matches the
    // existing repo convention for wide-but-flat signatures.
    #[allow(clippy::too_many_arguments)]
    pub fn record_job_receipt(
        &mut self,
        job_id: &str,
        bucket_id: &str,
        terminal_state: &str,
        exit_code: Option<i32>,
        final_signal_counts: &str,
        metrics_json: Option<&str>,
        end_cause: Option<&str>,
    ) -> Result<()> {
        self.ensure_outcome_evidence()?;
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        let sql = if end_cause == Some(ABANDONED_END_CAUSE) {
            // Insert when absent; overwrite ONLY a prior abandonment. A real
            // receipt already on the row is left untouched.
            "INSERT INTO job_receipts
                (job_id, bucket_id, terminal_state, exit_code,
                 final_signal_counts, restarted_at, created_at,
                 metrics_json, end_cause)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8)
             ON CONFLICT(job_id) DO UPDATE SET
                bucket_id = excluded.bucket_id,
                terminal_state = excluded.terminal_state,
                exit_code = excluded.exit_code,
                final_signal_counts = excluded.final_signal_counts,
                restarted_at = NULL,
                created_at = excluded.created_at,
                metrics_json = excluded.metrics_json,
                end_cause = excluded.end_cause
             WHERE job_receipts.end_cause = 'abandoned'"
        } else {
            // A real terminal transition heals a stray abandonment. Repeated
            // observations of the same outcome may add final drain evidence;
            // a conflicting late callback cannot rewrite an observed outcome.
            "INSERT INTO job_receipts
                (job_id, bucket_id, terminal_state, exit_code,
                 final_signal_counts, restarted_at, created_at,
                 metrics_json, end_cause)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8)
             ON CONFLICT(job_id) DO UPDATE SET
                bucket_id = excluded.bucket_id,
                terminal_state = excluded.terminal_state,
                exit_code = excluded.exit_code,
                final_signal_counts = excluded.final_signal_counts,
                restarted_at = NULL,
                created_at = excluded.created_at,
                metrics_json = excluded.metrics_json,
                end_cause = excluded.end_cause
             WHERE job_receipts.end_cause = 'abandoned'
                OR (job_receipts.terminal_state = excluded.terminal_state
                    AND job_receipts.exit_code IS excluded.exit_code)"
        };
        self.conn.execute(
            sql,
            params![
                job_id,
                bucket_id,
                terminal_state,
                exit_code,
                final_signal_counts,
                now_s,
                metrics_json,
                end_cause
            ],
        )?;
        Ok(())
    }

    /// Fetch a job receipt by id. Returns `None` if unknown.
    ///
    /// This is the post-restart fallback read: the in-memory job is gone,
    /// so the status handler reads the durable receipt and returns a
    /// restart-marked terminal result rather than a bare error.
    ///
    /// `metrics_json` is `None` for rows written before V0008. Callers MUST
    /// report that absence honestly and MUST NOT substitute zeroes, which is
    /// the exact defect this feature exists to remove.
    pub fn get_job_receipt(&self, job_id: &str) -> Result<Option<JobReceiptRow>> {
        self.conn
            .query_row(
                "SELECT job_id, bucket_id, terminal_state, exit_code,
                        final_signal_counts, restarted_at, created_at,
                        metrics_json, end_cause
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
                    let metrics_json: Option<String> = row.get(7)?;
                    let end_cause: Option<String> = row.get(8)?;
                    Ok((
                        job_id,
                        bucket_id,
                        terminal_state,
                        exit_code,
                        final_signal_counts,
                        restarted_at,
                        created_at,
                        metrics_json,
                        end_cause,
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
                    metrics_json,
                    end_cause,
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
                        metrics_json,
                        end_cause,
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
        s.ensure_outcome_evidence()
            .expect("migrate job_receipts + outcome evidence");
        s
    }

    const EVIDENCE: &str = r#"{"frames_total":4586,"bytes_total":334421,"duration_ms":641248}"#;

    #[test]
    fn record_then_get_round_trips() {
        let mut s = store();
        s.record_job_receipt(
            "job_abc",
            "bkt_1",
            "exited",
            Some(0),
            r#"{"events_emitted":2}"#,
            Some(EVIDENCE),
            None,
        )
        .expect("record");
        let r = s.get_job_receipt("job_abc").expect("get").expect("present");
        assert_eq!(r.job_id, "job_abc");
        assert_eq!(r.bucket_id, "bkt_1");
        assert_eq!(r.terminal_state, "exited");
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.final_signal_counts, r#"{"events_emitted":2}"#);
        assert!(r.restarted_at.is_none());
        assert_eq!(r.metrics_json.as_deref(), Some(EVIDENCE));
        assert_eq!(r.end_cause, None);
    }

    #[test]
    fn get_unknown_is_none() {
        let s = store();
        assert!(s.get_job_receipt("job_missing").expect("get").is_none());
    }

    #[test]
    fn record_is_idempotent_on_replace() {
        let mut s = store();
        s.record_job_receipt("job_x", "bkt_1", "exited", Some(1), "{}", None, None)
            .expect("first");
        s.record_job_receipt("job_x", "bkt_1", "exited", Some(1), "{}", None, None)
            .expect("replace");
        let r = s.get_job_receipt("job_x").expect("get").expect("present");
        assert_eq!(r.exit_code, Some(1));
    }

    #[test]
    fn conflicting_real_terminal_outcome_cannot_replace_the_first_receipt() {
        for (state, exit_code, late_state, late_code) in [
            ("failed", Some(7), "exited", Some(0)),
            ("cancelled", None, "exited", Some(0)),
            ("failed", Some(7), "failed", Some(8)),
        ] {
            let mut s = store();
            s.record_job_receipt(
                "job_once",
                "bkt_1",
                state,
                exit_code,
                "{}",
                Some(EVIDENCE),
                None,
            )
            .expect("first terminal observation");
            let first = s.get_job_receipt("job_once").unwrap().unwrap();
            s.record_job_receipt(
                "job_once",
                "bkt_other",
                late_state,
                late_code,
                "{}",
                None,
                None,
            )
            .expect("conflicting late callback is harmless");
            assert_eq!(s.get_job_receipt("job_once").unwrap().unwrap(), first);
        }
    }

    #[test]
    fn same_terminal_outcome_can_refresh_final_drain_evidence() {
        let mut s = store();
        s.record_job_receipt("job_drain", "bkt_1", "cancelled", None, "{}", None, None)
            .expect("immediate cancellation receipt");
        s.record_job_receipt(
            "job_drain",
            "bkt_1",
            "cancelled",
            None,
            "{}",
            Some(EVIDENCE),
            None,
        )
        .expect("lifecycle waiter adds final evidence");
        let row = s.get_job_receipt("job_drain").unwrap().unwrap();
        assert_eq!(row.terminal_state, "cancelled");
        assert_eq!(row.exit_code, None);
        assert_eq!(row.metrics_json.as_deref(), Some(EVIDENCE));
    }

    #[test]
    fn ensure_is_idempotent() {
        let mut s = store();
        s.ensure_job_receipts().expect("second ensure is a no-op");
        s.ensure_outcome_evidence()
            .expect("second outcome-evidence ensure is a no-op");
    }

    #[test]
    fn mark_restarted_stamps_once() {
        let mut s = store();
        s.record_job_receipt("job_r", "bkt_1", "exited", Some(0), "{}", None, None)
            .expect("record");
        s.mark_job_receipt_restarted("job_r").expect("mark");
        let r = s.get_job_receipt("job_r").expect("get").expect("present");
        assert!(r.restarted_at.is_some(), "restarted_at must be stamped");
    }

    #[test]
    fn cancelled_state_round_trips_with_null_exit_code() {
        let mut s = store();
        s.record_job_receipt("job_c", "bkt_2", "cancelled", None, "{}", None, None)
            .expect("record");
        let r = s.get_job_receipt("job_c").expect("get").expect("present");
        assert_eq!(r.terminal_state, "cancelled");
        assert_eq!(r.exit_code, None);
    }

    #[test]
    fn absent_evidence_stays_absent_and_is_never_zeroed() {
        // A row written without evidence (the pre-V0008 shape) MUST read back
        // as absent. Substituting zeroes here would recreate the exact defect
        // this feature removes: a genuine pass indistinguishable from a run
        // that produced nothing.
        let mut s = store();
        s.record_job_receipt("job_legacy", "bkt_1", "exited", Some(0), "{}", None, None)
            .expect("record");
        let r = s
            .get_job_receipt("job_legacy")
            .expect("get")
            .expect("present");
        assert_eq!(
            r.metrics_json, None,
            "absent evidence must not be defaulted to a zeroed object"
        );
    }

    #[test]
    fn abandoned_end_cause_round_trips_without_an_exit_code() {
        // Decision D1: abandonment rides `end_cause`, NOT a new terminal state.
        // The terminal state stays the truthful `cancelled`, so an older reader
        // sees a cancelled job rather than a fabricated failure.
        let mut s = store();
        s.record_job_receipt(
            "job_ab",
            "bkt_3",
            "cancelled",
            None,
            "{}",
            None,
            Some("abandoned"),
        )
        .expect("record");
        let r = s.get_job_receipt("job_ab").expect("get").expect("present");
        assert_eq!(r.terminal_state, "cancelled");
        assert_eq!(r.exit_code, None);
        assert_eq!(r.end_cause.as_deref(), Some("abandoned"));
    }

    #[test]
    fn abandonment_never_overwrites_a_real_receipt() {
        // spec 004 review (kimi-k3 HIGH-2). `record_abandoned_jobs` decides
        // from a STALE ledger scan and PTY waiters are never drained, so an
        // abandonment write can land AFTER the job's real exit was persisted.
        // If it won, a genuine `exited 0` with full evidence would become
        // "abandoned, no exit code" -- destroying a real pass and recreating
        // the exact 40-minute re-run loss this feature exists to remove.
        let mut s = store();
        s.record_job_receipt(
            "job_race",
            "bkt_9",
            "exited",
            Some(0),
            "{}",
            Some(r#"{"frames_total":7}"#),
            None,
        )
        .expect("real receipt");

        // The stale abandonment arrives second.
        s.record_job_receipt(
            "job_race",
            "bkt_9",
            "cancelled",
            None,
            "{}",
            None,
            Some(ABANDONED_END_CAUSE),
        )
        .expect("late abandonment");

        let r = s
            .get_job_receipt("job_race")
            .expect("get")
            .expect("present");
        assert_eq!(r.terminal_state, "exited", "the real outcome must survive");
        assert_eq!(r.exit_code, Some(0), "a true pass must stay bankable");
        assert_eq!(r.end_cause, None, "must not be relabelled abandoned");
        assert_eq!(
            r.metrics_json.as_deref(),
            Some(r#"{"frames_total":7}"#),
            "evidence must not be erased by a late abandonment"
        );
    }

    #[test]
    fn a_real_receipt_heals_a_stray_abandonment() {
        // The other direction MUST still overwrite: a quiesce marks a job
        // abandoned, the job then exits for real, and the true outcome has to
        // replace the placeholder.
        let mut s = store();
        s.record_job_receipt(
            "job_heal",
            "bkt_9",
            "cancelled",
            None,
            "{}",
            None,
            Some(ABANDONED_END_CAUSE),
        )
        .expect("abandonment");
        s.record_job_receipt("job_heal", "bkt_9", "exited", Some(3), "{}", None, None)
            .expect("real receipt");

        let r = s
            .get_job_receipt("job_heal")
            .expect("get")
            .expect("present");
        assert_eq!(r.terminal_state, "exited");
        assert_eq!(r.exit_code, Some(3));
        assert_eq!(r.end_cause, None, "the stray abandonment must be healed");
    }

    #[test]
    fn abandonment_is_idempotent_over_a_prior_abandonment() {
        // A double quiesce (or quiesce followed by shutdown) must not fail or
        // wedge the row; abandoned-over-abandoned is the one permitted
        // overwrite.
        let mut s = store();
        for bucket in ["bkt_a", "bkt_b"] {
            s.record_job_receipt(
                "job_twice",
                bucket,
                "cancelled",
                None,
                "{}",
                None,
                Some(ABANDONED_END_CAUSE),
            )
            .expect("abandonment");
        }
        let r = s
            .get_job_receipt("job_twice")
            .expect("get")
            .expect("present");
        assert_eq!(r.end_cause.as_deref(), Some(ABANDONED_END_CAUSE));
        assert_eq!(r.bucket_id, "bkt_b", "the later abandonment applied");
    }

    #[test]
    fn abandoned_constant_matches_the_sql_literal() {
        // `record_job_receipt` hardcodes `WHERE job_receipts.end_cause =
        // 'abandoned'` in SQL. If the constant drifts from that literal the
        // asymmetric guard silently stops matching and abandonment starts
        // clobbering real receipts again -- with every test above still green
        // except this one.
        assert_eq!(ABANDONED_END_CAUSE, "abandoned");
    }
}
