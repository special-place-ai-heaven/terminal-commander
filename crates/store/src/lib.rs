// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Persistent event store + registry for Terminal Commander.
//!
//! Two responsibilities live behind the same SQLite file: the event
//! store (TC12) and the rule registry (TC13). They share the
//! connection so a single transaction can touch both (e.g. when a
//! sifter run records both the emitted events and the activation
//! that produced them).
//!
//! Backed by SQLite (rusqlite 0.39 bundled) with FTS5 search and a
//! manual migration runner. Single-writer; readers open the DB
//! read-only.
//!
//! See `docs/storage/EVENT_STORE.md` for the locked event-store design
//! and `docs/storage/AUDIT_LOG.md` for the persistent audit log
//! introduced at TC35.
//!
//! Source-status: live (TC12) for append + cursor reads + retention
//! eviction + VACUUM INTO backup. Registry persistence (TC13) and
//! the persistent audit log (TC35) ride on the same database file
//! but live behind their own modules.

pub mod audit;
pub mod import;
pub mod registry;
pub use audit::{
    ALLOWED_AUDIT_DECISIONS, AuditEntry, AuditReadRequest, AuditRow, DEFAULT_AUDIT_READ_LIMIT,
    MAX_AUDIT_METADATA_BYTES, MAX_AUDIT_READ_LIMIT, MAX_AUDIT_REASON_BYTES,
    MAX_AUDIT_SUBJECT_BYTES,
};
pub use import::{
    ImportResult, RULE_PACK_DFA_SIZE_LIMIT, RULE_PACK_REGEX_SIZE_LIMIT, RulePackFile, RulePackMeta,
};
pub use registry::{
    ActivationRecord, ActiveRuleDef, DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT, RuleSearchHit,
};

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json as sj;
use terminal_commander_core::{
    BucketId, EventId, EventSource, RuleRef, Severity, SignalEvent, SourcePointer,
};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Embedded V0001 migration. Manual runner because refinery 0.9
/// transitively pins rusqlite <= 0.38, which conflicts with our
/// 0.39 + bundled requirement.
const MIGRATION_V0001: &str = include_str!("../migrations/V0001__initial_schema.sql");

/// Default per-bucket maximum event count (count-based retention).
pub const DEFAULT_BUCKET_MAX_EVENTS: u64 = 100_000;

/// Default per-bucket TTL (24 hours).
pub const DEFAULT_BUCKET_TTL: Duration = Duration::from_hours(24);

/// Default upper bound on a single read.
pub const DEFAULT_READ_LIMIT: usize = 200;

/// Hard cap on a single read. Mirrors TC07.
pub const MAX_READ_LIMIT: usize = 10_000;

/// Per-bucket configuration kept in the `buckets` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreBucketConfig {
    pub max_events: u64,
    pub ttl: Duration,
}

impl Default for StoreBucketConfig {
    fn default() -> Self {
        Self {
            max_events: DEFAULT_BUCKET_MAX_EVENTS,
            ttl: DEFAULT_BUCKET_TTL,
        }
    }
}

/// Cursor-based read request.
#[derive(Debug, Clone)]
pub struct StoreReadRequest {
    pub cursor: u64,
    pub severity_min: Option<Severity>,
    pub kind_filter: Option<String>,
    pub limit: Option<usize>,
}

impl StoreReadRequest {
    /// Construct a minimal request reading from `cursor` with no
    /// filters and the default limit.
    #[must_use]
    pub const fn new(cursor: u64) -> Self {
        Self {
            cursor,
            severity_min: None,
            kind_filter: None,
            limit: None,
        }
    }
}

/// Response shape for `events_since`.
#[derive(Debug, Clone)]
pub struct StoreReadResponse {
    pub bucket_id: BucketId,
    pub cursor_in: u64,
    pub next_cursor: u64,
    pub has_more: bool,
    pub dropped_count: u64,
    pub events: Vec<SignalEvent>,
}

/// Bucket summary, mirroring `terminal_commander_core::BucketState`
/// but read from the persistent store.
#[derive(Debug, Clone)]
pub struct StoreBucketSummary {
    pub bucket_id: BucketId,
    pub created_at: OffsetDateTime,
    pub head_seq: u64,
    pub tail_seq: u64,
    pub event_count: u64,
    pub dropped_count: u64,
}

/// Errors from the event store.
#[derive(Debug, thiserror::Error)]
pub enum EventStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(String),
    #[error("json error: {0}")]
    Json(#[from] sj::Error),
    #[error("time format error: {0}")]
    TimeFormat(#[from] time::error::Format),
    #[error("time parse error: {0}")]
    TimeParse(#[from] time::error::Parse),
    #[error(
        "DB path '{0}' resolves to a 9P (drvfs) mount on WSL2 which does not support SQLite WAL safely. Place the DB on a native Linux filesystem."
    )]
    DbOn9P(PathBuf),
    #[error("bucket '{0}' not found")]
    BucketNotFound(BucketId),
    #[error("event '{0}' not found")]
    EventNotFound(EventId),
    #[error("event bucket mismatch: event.bucket_id={event} but appending to {bucket}")]
    EventBucketMismatch { event: BucketId, bucket: BucketId },
    #[error("event seq {seq} is not greater than the bucket tail seq {tail}")]
    NonMonotonicSeq { seq: u64, tail: u64 },
    #[error("event seq {0} exceeds i64::MAX and cannot be stored")]
    SeqOverflow(u64),
    #[error("could not read /proc/self/mountinfo: {0}")]
    MountInfo(String),
    #[error("invalid stored payload: {0}")]
    InvalidPayload(String),
    /// The store backend is unavailable or faulted in a way the caller
    /// cannot act on: the single-writer actor thread is gone, dropped
    /// its reply channel, returned an unexpected reply, or an op panicked
    /// and was isolated. Distinct from `InvalidPayload` (a caller-fixable
    /// bad input) so it can map to a server-fault IPC code, never to a
    /// "fix your input" code.
    #[error("store unavailable: {0}")]
    Unavailable(String),
}

/// Per-mod result alias.
pub type Result<T> = core::result::Result<T, EventStoreError>;

/// The event store. One handle per opened DB connection.
pub struct EventStore {
    conn: Connection,
    read_only: bool,
}

impl EventStore {
    /// Open the store with a writer connection. Runs migrations.
    /// The path is checked against `/proc/self/mountinfo` (where
    /// available) to reject WSL `/mnt/c` placement.
    pub fn with_writer(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        check_filesystem(path)?;
        let conn = Connection::open(path)?;
        configure_pragmas(&conn, false)?;
        let mut me = Self {
            conn,
            read_only: false,
        };
        me.migrate()?;
        Ok(me)
    }

    /// Open a read-only connection. Does NOT run migrations.
    pub fn with_reader(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;
        configure_pragmas(&conn, true)?;
        Ok(Self {
            conn,
            read_only: true,
        })
    }

    /// Open an in-memory store for tests.
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        configure_pragmas(&conn, false)?;
        let mut me = Self {
            conn,
            read_only: false,
        };
        me.migrate()?;
        Ok(me)
    }

    fn migrate(&mut self) -> Result<()> {
        // Track applied migrations in a small bookkeeping table.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
             );",
        )?;
        let already: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if already == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(MIGRATION_V0001)
                .map_err(|e| EventStoreError::Migration(e.to_string()))?;
            let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (1, ?1)",
                params![now_s],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Whether this connection is read-only.
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Ensure a bucket row exists. Idempotent.
    pub fn ensure_bucket(&mut self, bucket_id: BucketId, config: &StoreBucketConfig) -> Result<()> {
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        let ttl_secs = i64::try_from(config.ttl.as_secs()).unwrap_or(i64::MAX);
        let max_events = i64::try_from(config.max_events).unwrap_or(i64::MAX);
        self.conn.execute(
            "INSERT OR IGNORE INTO buckets
                (bucket_id, created_at, max_events, ttl_secs)
             VALUES (?1, ?2, ?3, ?4)",
            params![bucket_id.to_wire_string(), now_s, max_events, ttl_secs],
        )?;
        Ok(())
    }

    /// Append an event to a bucket. Assigns monotonic `seq` when
    /// `event.seq == 0`. Validates the TC02 pointer invariant.
    #[allow(clippy::too_many_lines)]
    pub fn append(&mut self, mut event: SignalEvent) -> Result<u64> {
        let bid = event.bucket_id;
        // Validate the pointer invariant.
        event
            .validate()
            .map_err(|e| EventStoreError::InvalidPayload(e.to_string()))?;
        // Bucket must exist with at least defaults.
        self.ensure_bucket(bid, &StoreBucketConfig::default())?;

        let tx = self.conn.transaction()?;
        let tail: i64 = tx
            .query_row(
                "SELECT tail_seq FROM buckets WHERE bucket_id = ?1",
                params![bid.to_wire_string()],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let tail_u: u64 = u64::try_from(tail).unwrap_or(0);
        let assigned_seq = if event.seq == 0 {
            tail_u.saturating_add(1)
        } else if event.seq <= tail_u && tail_u > 0 {
            return Err(EventStoreError::NonMonotonicSeq {
                seq: event.seq,
                tail: tail_u,
            });
        } else {
            event.seq
        };
        event.seq = assigned_seq;
        let seq_i =
            i64::try_from(assigned_seq).map_err(|_| EventStoreError::SeqOverflow(assigned_seq))?;

        let captures_json = sj::to_string(&event.captures.clone().unwrap_or_default())?;
        let source_json = sj::to_string(&event.source)?;
        let pointer_json = match &event.pointer {
            Some(p) => Some(sj::to_string(p)?),
            None => None,
        };
        let tags_json = match &event.tags {
            Some(v) => Some(sj::to_string(v)?),
            None => None,
        };
        let ts_s = event.timestamp.format(&Rfc3339)?;
        let first_seen_s = match event.first_seen {
            Some(t) => Some(t.format(&Rfc3339)?),
            None => None,
        };
        let last_seen_s = match event.last_seen {
            Some(t) => Some(t.format(&Rfc3339)?),
            None => None,
        };
        let rule_id_s = event.rule.as_ref().map(|r| r.id.to_wire_string());
        let rule_version_i: Option<i64> = event.rule.as_ref().map(|r| i64::from(r.version));

        tx.execute(
            "INSERT INTO events
              (bucket_id, seq, event_id, timestamp, severity_rank, severity,
               kind, summary, rule_id, rule_version, captures, source, pointer,
               pointer_unavailable_reason, tags, count, first_seen, last_seen, suppressed)
             VALUES
              (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                bid.to_wire_string(),
                seq_i,
                event.event_id.to_wire_string(),
                ts_s,
                i64::from(event.severity.rank()),
                event.severity.as_str(),
                event.kind,
                event.summary,
                rule_id_s,
                rule_version_i,
                captures_json,
                source_json,
                pointer_json,
                event.pointer_unavailable_reason,
                tags_json,
                i64::from(event.count),
                first_seen_s,
                last_seen_s,
                i32::from(event.suppressed),
            ],
        )?;
        tx.execute(
            "UPDATE buckets SET tail_seq = ?1 WHERE bucket_id = ?2",
            params![seq_i, bid.to_wire_string()],
        )?;

        // Retention: count cap eviction inside the same tx.
        let max_events: i64 = tx
            .query_row(
                "SELECT max_events FROM buckets WHERE bucket_id = ?1",
                params![bid.to_wire_string()],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(i64::MAX);
        let count_to_drop_row: i64 = tx
            .query_row(
                "SELECT MAX(0, (SELECT COUNT(*) FROM events WHERE bucket_id = ?1) - ?2)",
                params![bid.to_wire_string(), max_events],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if count_to_drop_row > 0 {
            tx.execute(
                "DELETE FROM events WHERE rowid IN (
                    SELECT rowid FROM events WHERE bucket_id = ?1
                    ORDER BY seq ASC LIMIT ?2
                 )",
                params![bid.to_wire_string(), count_to_drop_row],
            )?;
            tx.execute(
                "UPDATE buckets SET dropped_count = dropped_count + ?1 WHERE bucket_id = ?2",
                params![count_to_drop_row, bid.to_wire_string()],
            )?;
        }
        tx.commit()?;
        Ok(assigned_seq)
    }

    /// Evict events older than the bucket's TTL.
    pub fn evict_expired(&mut self, bucket_id: BucketId) -> Result<u64> {
        let row: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT ttl_secs, created_at FROM buckets WHERE bucket_id = ?1",
                params![bucket_id.to_wire_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((ttl_secs, _created_at)) = row else {
            return Err(EventStoreError::BucketNotFound(bucket_id));
        };
        let cutoff = OffsetDateTime::now_utc() - time::Duration::seconds(ttl_secs);
        let cutoff_s = cutoff.format(&Rfc3339)?;
        let tx = self.conn.transaction()?;
        let evicted = tx.execute(
            "DELETE FROM events WHERE bucket_id = ?1 AND timestamp < ?2",
            params![bucket_id.to_wire_string(), cutoff_s],
        )?;
        if evicted > 0 {
            let evicted_i = i64::try_from(evicted).unwrap_or(i64::MAX);
            tx.execute(
                "UPDATE buckets SET dropped_count = dropped_count + ?1 WHERE bucket_id = ?2",
                params![evicted_i, bucket_id.to_wire_string()],
            )?;
        }
        tx.commit()?;
        Ok(evicted as u64)
    }

    /// Read events strictly after `cursor` from a bucket.
    pub fn events_since(
        &mut self,
        bucket_id: BucketId,
        request: &StoreReadRequest,
    ) -> Result<StoreReadResponse> {
        // Eviction is best-effort on the read path (a failed GC must not fail a
        // read), but do not swallow the error silently — surface it so a
        // persistent DB problem is visible instead of hidden behind reads.
        if let Err(e) = self.evict_expired(bucket_id) {
            eprintln!("terminal-commander: evict_expired({bucket_id:?}) failed on read path: {e}");
        }
        let limit = request
            .limit
            .unwrap_or(DEFAULT_READ_LIMIT)
            .clamp(1, MAX_READ_LIMIT);
        let probe_plus_one = limit + 1;

        let cursor_i = i64::try_from(request.cursor).unwrap_or(i64::MAX);
        let sev_rank = request.severity_min.map_or(0_i64, |s| i64::from(s.rank()));
        let kind_pat = request.kind_filter.as_deref();

        let mut stmt = self.conn.prepare(
            "SELECT bucket_id, seq, event_id, timestamp, severity, kind, summary,
                    rule_id, rule_version, captures, source, pointer,
                    pointer_unavailable_reason, tags, count, first_seen, last_seen, suppressed
             FROM events
             WHERE bucket_id = ?1 AND seq > ?2 AND severity_rank >= ?3
               AND (?4 IS NULL OR kind = ?4)
             ORDER BY seq ASC LIMIT ?5",
        )?;
        let mut rows = stmt.query(params![
            bucket_id.to_wire_string(),
            cursor_i,
            sev_rank,
            kind_pat,
            i64::try_from(probe_plus_one).unwrap_or(i64::MAX),
        ])?;

        let mut events = Vec::with_capacity(limit);
        let mut next_cursor = request.cursor;
        let mut count = 0_usize;
        let mut has_more = false;
        while let Some(row) = rows.next()? {
            if count == limit {
                has_more = true;
                break;
            }
            let ev = row_to_event(row)?;
            next_cursor = ev.seq;
            events.push(ev);
            count += 1;
        }
        drop(rows);
        drop(stmt);

        let dropped_count: i64 = self
            .conn
            .query_row(
                "SELECT dropped_count FROM buckets WHERE bucket_id = ?1",
                params![bucket_id.to_wire_string()],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);

        Ok(StoreReadResponse {
            bucket_id,
            cursor_in: request.cursor,
            next_cursor,
            has_more,
            dropped_count: u64::try_from(dropped_count).unwrap_or(0),
            events,
        })
    }

    /// Look up a single event by id.
    pub fn get_event(&self, event_id: EventId) -> Result<SignalEvent> {
        let mut stmt = self.conn.prepare(
            "SELECT bucket_id, seq, event_id, timestamp, severity, kind, summary,
                    rule_id, rule_version, captures, source, pointer,
                    pointer_unavailable_reason, tags, count, first_seen, last_seen, suppressed
             FROM events WHERE event_id = ?1",
        )?;
        let mut rows = stmt.query(params![event_id.to_wire_string()])?;
        rows.next()?.map_or_else(
            || Err(EventStoreError::EventNotFound(event_id)),
            row_to_event,
        )
    }

    /// Bucket summary.
    pub fn summary(&self, bucket_id: BucketId) -> Result<StoreBucketSummary> {
        let row = self.conn.query_row(
            "SELECT created_at, head_seq, tail_seq, dropped_count FROM buckets WHERE bucket_id = ?1",
            params![bucket_id.to_wire_string()],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            )),
        ).optional()?;
        let Some((created_at_s, head_seq, tail_seq, dropped_count)) = row else {
            return Err(EventStoreError::BucketNotFound(bucket_id));
        };
        let created_at = OffsetDateTime::parse(&created_at_s, &Rfc3339)?;
        let event_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM events WHERE bucket_id = ?1",
            params![bucket_id.to_wire_string()],
            |row| row.get(0),
        )?;
        Ok(StoreBucketSummary {
            bucket_id,
            created_at,
            head_seq: u64::try_from(head_seq).unwrap_or(0),
            tail_seq: u64::try_from(tail_seq).unwrap_or(0),
            event_count: u64::try_from(event_count).unwrap_or(0),
            dropped_count: u64::try_from(dropped_count).unwrap_or(0),
        })
    }

    /// Reclaim disk space via VACUUM. Operator-driven; not automatic.
    pub fn vacuum(&self) -> Result<()> {
        self.conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    /// Snapshot the database to a destination path via `VACUUM INTO`.
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let p = path.as_ref().to_string_lossy().into_owned();
        // Quote single quotes for the SQL literal.
        let quoted = p.replace('\'', "''");
        let sql = format!("VACUUM INTO '{quoted}';");
        self.conn.execute_batch(&sql)?;
        Ok(())
    }

    /// Run a full WAL checkpoint, flushing the WAL into the main DB
    /// file. Intended for clean-shutdown discipline by the store
    /// actor (ROB-11): drain queue, checkpoint, exit. SQLite returns
    /// `(busy, log_pages, checkpointed_pages)` from PRAGMA
    /// `wal_checkpoint(FULL)` — we surface success/error only because
    /// callers do not act on the counters.
    pub fn wal_checkpoint_full(&self) -> Result<()> {
        // `query_row` because the PRAGMA returns a single row.
        self.conn
            .query_row("PRAGMA wal_checkpoint(FULL);", [], |_| Ok(()))?;
        Ok(())
    }

    /// Test helper: asserts no BLOB column type appears in the
    /// events table. Used by the schema test and is also a useful
    /// public sanity check.
    #[must_use]
    pub fn events_table_has_no_blob(&self) -> bool {
        let cols: Vec<(String, String)> = self
            .conn
            .prepare("SELECT name, type FROM pragma_table_info('events')")
            .and_then(|mut stmt| {
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap_or_default();
        !cols.iter().any(|(_, ty)| ty.eq_ignore_ascii_case("BLOB"))
    }
}

fn configure_pragmas(conn: &Connection, read_only: bool) -> Result<()> {
    // journal_mode is per-database; setting on a read-only connection
    // is a no-op but harmless.
    if read_only {
        conn.execute_batch(
            "PRAGMA query_only=ON;\
             PRAGMA busy_timeout=5000;",
        )?;
    } else {
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        conn.execute_batch(
            "PRAGMA synchronous=NORMAL;\
             PRAGMA foreign_keys=ON;\
             PRAGMA busy_timeout=5000;",
        )?;
    }
    Ok(())
}

/// Check the path's filesystem; reject 9P (WSL drvfs) placement.
fn check_filesystem(path: &Path) -> Result<()> {
    let mountinfo_path = Path::new("/proc/self/mountinfo");
    if !mountinfo_path.exists() {
        // Not on Linux/WSL. Skip the check.
        return Ok(());
    }
    let mountinfo = std::fs::read_to_string(mountinfo_path)
        .map_err(|e| EventStoreError::MountInfo(e.to_string()))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let abs_target = std::fs::canonicalize(parent).unwrap_or_else(|_| path.to_path_buf());
    let target_str = abs_target.to_string_lossy().into_owned();

    // Find the longest mount-point prefix matching the target;
    // check its fs type.
    let mut best: Option<(&str, &str)> = None;
    for line in mountinfo.lines() {
        // Format: ... mount-point ... - fs-type ...
        // Field 4 is the mount point; field after "-" is the fs type.
        let fields: Vec<&str> = line.split_whitespace().collect();
        let dash_idx = fields.iter().position(|&f| f == "-");
        let Some(dash_idx) = dash_idx else {
            continue;
        };
        if fields.len() < 5 || dash_idx + 1 >= fields.len() {
            continue;
        }
        let mp = fields[4];
        let fs = fields[dash_idx + 1];
        if target_str.starts_with(mp) {
            match best {
                None => best = Some((mp, fs)),
                Some((bmp, _)) if mp.len() > bmp.len() => best = Some((mp, fs)),
                _ => {}
            }
        }
    }
    if let Some((_, fs)) = best
        && fs == "9p"
    {
        return Err(EventStoreError::DbOn9P(path.to_path_buf()));
    }
    Ok(())
}

fn row_to_event(row: &rusqlite::Row) -> Result<SignalEvent> {
    let bucket_id_s: String = row.get(0)?;
    let seq_i: i64 = row.get(1)?;
    let event_id_s: String = row.get(2)?;
    let ts_s: String = row.get(3)?;
    let severity_s: String = row.get(4)?;
    let kind: String = row.get(5)?;
    let summary: String = row.get(6)?;
    let rule_id_s: Option<String> = row.get(7)?;
    let rule_version_i: Option<i64> = row.get(8)?;
    let captures_s: String = row.get(9)?;
    let source_s: String = row.get(10)?;
    let pointer_s: Option<String> = row.get(11)?;
    let pointer_unavailable_reason: Option<String> = row.get(12)?;
    let tags_s: Option<String> = row.get(13)?;
    let count_i: i64 = row.get(14)?;
    let first_seen_s: Option<String> = row.get(15)?;
    let last_seen_s: Option<String> = row.get(16)?;
    let suppressed_i: i64 = row.get(17)?;

    let bucket_id = BucketId::parse_wire(&bucket_id_s)
        .map_err(|e| EventStoreError::InvalidPayload(format!("bucket_id parse: {e}")))?;
    let event_id = EventId::parse_wire(&event_id_s)
        .map_err(|e| EventStoreError::InvalidPayload(format!("event_id parse: {e}")))?;
    let timestamp = OffsetDateTime::parse(&ts_s, &Rfc3339)?;
    let severity = Severity::parse(&severity_s)
        .map_err(|e| EventStoreError::InvalidPayload(format!("severity parse: {e}")))?;
    let rule = match (rule_id_s, rule_version_i) {
        (Some(rid_s), Some(v)) => {
            let id = terminal_commander_core::RuleId::parse_wire(&rid_s)
                .map_err(|e| EventStoreError::InvalidPayload(format!("rule_id parse: {e}")))?;
            Some(RuleRef {
                id,
                version: u32::try_from(v).unwrap_or(u32::MAX),
            })
        }
        _ => None,
    };
    let captures: terminal_commander_core::Captures = if captures_s.is_empty() {
        terminal_commander_core::Captures::new()
    } else {
        sj::from_str(&captures_s)?
    };
    let source: EventSource = sj::from_str(&source_s)?;
    let pointer: Option<SourcePointer> = match pointer_s {
        Some(s) => Some(sj::from_str(&s)?),
        None => None,
    };
    let tags: Option<Vec<String>> = match tags_s {
        Some(s) => Some(sj::from_str(&s)?),
        None => None,
    };
    let first_seen = match first_seen_s {
        Some(s) => Some(OffsetDateTime::parse(&s, &Rfc3339)?),
        None => None,
    };
    let last_seen = match last_seen_s {
        Some(s) => Some(OffsetDateTime::parse(&s, &Rfc3339)?),
        None => None,
    };

    let captures_opt = if captures.is_empty() {
        None
    } else {
        Some(captures)
    };
    Ok(SignalEvent {
        event_id,
        bucket_id,
        seq: u64::try_from(seq_i).unwrap_or(0),
        timestamp,
        severity,
        kind,
        summary,
        rule,
        source,
        captures: captures_opt,
        pointer,
        pointer_unavailable_reason,
        tags,
        count: u32::try_from(count_i).unwrap_or(1),
        first_seen,
        last_seen,
        suppressed: suppressed_i != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{
        BucketId, Captures, EventDraft, EventId, EventSource, FrameId, ProbeId, Severity,
        SourcePointer, SourceStream, SourceType,
    };

    fn make_event(bid: BucketId, sev: Severity, kind: &str) -> SignalEvent {
        let mut caps = Captures::new();
        caps.insert("package".to_owned(), "libssl-dev".to_owned());
        SignalEvent {
            event_id: EventId::new(),
            bucket_id: bid,
            seq: 0,
            timestamp: OffsetDateTime::now_utc(),
            severity: sev,
            kind: kind.to_owned(),
            summary: format!("{kind} summary"),
            rule: None,
            source: EventSource {
                probe_id: ProbeId::new(),
                source_type: SourceType::Process,
                stream: SourceStream::Stderr,
                job_id: None,
            },
            captures: Some(caps),
            pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
            pointer_unavailable_reason: None,
            tags: None,
            count: 1,
            first_seen: None,
            last_seen: None,
            suppressed: false,
        }
    }

    #[test]
    fn store_in_memory_open_and_migrate() {
        let s = EventStore::in_memory().unwrap();
        assert!(!s.is_read_only());
        assert!(s.events_table_has_no_blob());
    }

    #[test]
    fn store_append_assigns_monotonic_seq() {
        let mut s = EventStore::in_memory().unwrap();
        let bid = BucketId::new();
        s.ensure_bucket(bid, &StoreBucketConfig::default()).unwrap();
        let s1 = s.append(make_event(bid, Severity::Low, "k1")).unwrap();
        let s2 = s.append(make_event(bid, Severity::Low, "k2")).unwrap();
        let s3 = s.append(make_event(bid, Severity::Low, "k3")).unwrap();
        assert_eq!((s1, s2, s3), (1, 2, 3));
    }

    #[test]
    fn store_get_event_round_trip() {
        let mut s = EventStore::in_memory().unwrap();
        let bid = BucketId::new();
        let ev = make_event(bid, Severity::High, "k");
        let id = ev.event_id;
        s.append(ev).unwrap();
        let got = s.get_event(id).unwrap();
        assert_eq!(got.event_id, id);
        assert_eq!(got.severity, Severity::High);
        assert_eq!(got.kind, "k");
        // Captures round-trip.
        assert_eq!(
            got.captures
                .as_ref()
                .unwrap()
                .get("package")
                .map(String::as_str),
            Some("libssl-dev")
        );
        // The bucket_id matches.
        assert_eq!(got.bucket_id, bid);
        // The pointer survived JSON round-trip.
        assert!(got.pointer.is_some());
        // Drop unused field reference.
        let _ = ev.summary;
    }

    #[test]
    fn store_events_since_respects_cursor_and_limit() {
        let mut s = EventStore::in_memory().unwrap();
        let bid = BucketId::new();
        for _ in 0..5 {
            s.append(make_event(bid, Severity::Low, "k")).unwrap();
        }
        let mut req = StoreReadRequest::new(0);
        req.limit = Some(3);
        let r = s.events_since(bid, &req).unwrap();
        assert_eq!(r.events.len(), 3);
        assert!(r.has_more);
        assert_eq!(r.next_cursor, 3);
        let r2 = s.events_since(bid, &StoreReadRequest::new(3)).unwrap();
        assert_eq!(r2.events.len(), 2);
        assert!(!r2.has_more);
        assert_eq!(r2.next_cursor, 5);
    }

    #[test]
    fn store_severity_min_filter_excludes_below() {
        let mut s = EventStore::in_memory().unwrap();
        let bid = BucketId::new();
        s.append(make_event(bid, Severity::Low, "k")).unwrap();
        s.append(make_event(bid, Severity::High, "k")).unwrap();
        s.append(make_event(bid, Severity::Critical, "k")).unwrap();
        let mut req = StoreReadRequest::new(0);
        req.severity_min = Some(Severity::High);
        let r = s.events_since(bid, &req).unwrap();
        assert_eq!(r.events.len(), 2);
    }

    #[test]
    fn store_count_cap_evicts_oldest_and_bumps_dropped_count() {
        let mut s = EventStore::in_memory().unwrap();
        let bid = BucketId::new();
        s.ensure_bucket(
            bid,
            &StoreBucketConfig {
                max_events: 3,
                ttl: Duration::from_hours(1),
            },
        )
        .unwrap();
        for _ in 0..5 {
            s.append(make_event(bid, Severity::Low, "k")).unwrap();
        }
        let sum = s.summary(bid).unwrap();
        assert_eq!(sum.event_count, 3);
        assert_eq!(sum.dropped_count, 2);
    }

    #[test]
    fn store_no_blob_columns_in_events_table() {
        let s = EventStore::in_memory().unwrap();
        assert!(s.events_table_has_no_blob());
    }

    #[test]
    fn store_wal_and_pragmas_set_on_writer() {
        let s = EventStore::in_memory().unwrap();
        // In-memory: journal_mode reports 'memory', which is the
        // intended sqlite behavior (you cannot WAL an :memory: db).
        // Assert that the busy_timeout pragma is applied.
        let bt: i64 = s
            .conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(bt, 5000);
        let sync: i64 = s
            .conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sync, 1); // NORMAL
    }

    #[test]
    fn store_event_draft_promotion_round_trips() {
        let mut s = EventStore::in_memory().unwrap();
        let bid = BucketId::new();
        let draft = EventDraft {
            bucket_id: bid,
            timestamp: OffsetDateTime::now_utc(),
            severity: Severity::Medium,
            kind: "kw_match".to_owned(),
            summary: "matched needle".to_owned(),
            rule: None,
            source: EventSource {
                probe_id: ProbeId::new(),
                source_type: SourceType::Process,
                stream: SourceStream::Stderr,
                job_id: None,
            },
            captures: None,
            pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
            pointer_unavailable_reason: None,
            tags: None,
            frame_truncated_bytes: 0,
            count: 1,
            first_seen: None,
            last_seen: None,
            suppressed: false,
        };
        let ev = draft.into_signal_event(0);
        let id = ev.event_id;
        s.append(ev).unwrap();
        let back = s.get_event(id).unwrap();
        assert_eq!(back.kind, "kw_match");
    }

    #[test]
    fn store_get_event_not_found() {
        let s = EventStore::in_memory().unwrap();
        let err = s.get_event(EventId::new()).unwrap_err();
        assert!(matches!(err, EventStoreError::EventNotFound(_)));
    }

    #[test]
    fn store_non_monotonic_seq_rejected() {
        let mut s = EventStore::in_memory().unwrap();
        let bid = BucketId::new();
        let mut ev1 = make_event(bid, Severity::Low, "k");
        ev1.seq = 0;
        s.append(ev1).unwrap();
        let mut ev2 = make_event(bid, Severity::Low, "k");
        ev2.seq = 1; // tail is already 1; not strictly greater
        let err = s.append(ev2).unwrap_err();
        assert!(matches!(err, EventStoreError::NonMonotonicSeq { .. }));
    }

    #[test]
    fn store_backup_to_in_memory_path_uri() {
        // Backup to another in-memory database via URI (rusqlite supports
        // VACUUM INTO of a file path; we use a temp file).
        let dir = std::env::temp_dir().join(format!("tc-store-backup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dst = dir.join("snapshot.db");
        let mut s = EventStore::in_memory().unwrap();
        let bid = BucketId::new();
        s.append(make_event(bid, Severity::Low, "k")).unwrap();
        s.backup_to(&dst).unwrap();
        assert!(dst.exists());
        let _ = std::fs::remove_file(&dst);
        let _ = std::fs::remove_dir(&dir);
    }
}
