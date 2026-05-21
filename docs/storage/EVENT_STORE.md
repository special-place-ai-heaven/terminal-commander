# Event Store - Terminal Commander

Status: TC12 baseline.

This document captures the locked design of the persistent event
store backed by SQLite + FTS5 + refinery migrations.

Language: ASCII only.

## 1. Backend lock

Per `docs/research/sqlite-fts5.md` and the locked decision in
`docs/research/_USER_DECISIONS.md`:

- `rusqlite = 0.39` with the `bundled` feature (no system libsqlite
  dependency; ships SQLite with FTS5 enabled).
- `refinery = 0.9` for forward-only migrations (`rusqlite-bundled`
  feature).
- `journal_mode = WAL`, `synchronous = NORMAL`, `busy_timeout = 5000`
  (ms). Set as `PRAGMA`s on every connection.

## 2. Single-writer invariant

The event-store DB has exactly one writer process at a time: the
Terminal Commander daemon (`terminal-commanderd`).

- MCP server, admin CLI, and any other readers MUST open the DB
  read-only via `OpenFlags::SQLITE_OPEN_READ_ONLY`.
- All writes go through `EventStore::with_writer(path) -> Self`.
- All reads can use `EventStore::with_reader(path) -> Self`.

## 3. WSL filesystem placement

SQLite WAL is unreliable on the 9P drvfs filesystem that backs
`/mnt/c` on WSL2 (per `docs/research/wsl-boundary.md`). The store
detects placement at startup via `/proc/self/mountinfo` and
REJECTS a DB path that resolves to a 9P mount.

The error variant is `EventStoreError::DbOn9P`.

This check is skipped on Windows native (where /proc/self/mountinfo
doesn't exist) and on macOS (out of MVP scope).

## 4. Schema (v1)

The events table is intentionally BLOB-free. All fields are TEXT,
INTEGER, or JSON-as-TEXT.

```sql
CREATE TABLE IF NOT EXISTS buckets (
    bucket_id TEXT NOT NULL PRIMARY KEY,
    created_at TEXT NOT NULL,
    head_seq INTEGER NOT NULL DEFAULT 0,
    tail_seq INTEGER NOT NULL DEFAULT 0,
    dropped_count INTEGER NOT NULL DEFAULT 0,
    max_events INTEGER NOT NULL,
    ttl_secs INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
    bucket_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    event_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    severity_rank INTEGER NOT NULL,
    severity TEXT NOT NULL,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    rule_id TEXT,
    rule_version INTEGER,
    captures TEXT NOT NULL,                -- JSON object string
    source TEXT NOT NULL,                  -- JSON object string
    pointer TEXT,                          -- JSON object string or NULL
    pointer_unavailable_reason TEXT,
    tags TEXT,                             -- JSON array string or NULL
    count INTEGER NOT NULL DEFAULT 1,
    first_seen TEXT,
    last_seen TEXT,
    suppressed INTEGER NOT NULL DEFAULT 0, -- bool
    PRIMARY KEY (bucket_id, seq),
    FOREIGN KEY (bucket_id) REFERENCES buckets(bucket_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_events_event_id ON events(event_id);
CREATE INDEX IF NOT EXISTS idx_events_bucket_timestamp
    ON events(bucket_id, timestamp);

-- FTS5 external-content table over (summary, kind, captures_text)
CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
    summary, kind, captures_text,
    content='events',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2'
);
```

No BLOB columns. A schema test asserts this invariant; new
columns must remain TEXT/INTEGER.

## 5. Append API

`EventStore::append(bucket_id, event)`:

1. Verify bucket exists; create with default config if absent.
2. Validate the event's TC02 pointer invariant (severity >= Medium
   requires pointer OR pointer_unavailable_reason).
3. Assign monotonic seq = max(existing_tail_seq, 0) + 1 if
   event.seq == 0; else verify seq > tail_seq.
4. Insert row inside a transaction.
5. Update buckets.tail_seq.
6. Apply per-bucket count cap (FIFO eviction) and TTL eviction
   inside the same transaction; bump dropped_count.

## 6. Read API

`EventStore::events_since(bucket_id, request)`:

- Cursor-based: `WHERE seq > ?cursor`.
- Bounded: caller-provided `limit`, clamped to MAX_READ_LIMIT (200
  default, 10_000 hard cap; mirrors TC07).
- Severity-min filter via `severity_rank >= ?`.
- Optional kind filter via `kind = ?`.
- Result includes `dropped_count` so callers detect loss.

`EventStore::get_event(event_id)` retrieves a single event by its
typed-id wire form.

`EventStore::summary(bucket_id)` returns the same shape as
`BucketSummary` from TC07.

## 7. Retention policy

- Time-based: each bucket has `ttl_secs`; rows older than
  `now - ttl_secs` are evicted on `evict_expired()`.
- Count-based: each bucket has `max_events`; FIFO eviction beyond
  the cap on every `append` and `evict_expired()`.
- Defaults: 24h / 100_000 events. Operator-tunable.
- `vacuum()` is an operator-driven entry point (not automatic at
  MVP). Reclaims space after large eviction sweeps.

## 8. Backup

`EventStore::backup_to(path)` runs `VACUUM INTO '<path>'`. The
backup is a consistent point-in-time snapshot. File-level copy
(with WAL gymnastics) is NOT used.

## 9. seq u64 -> SQLite i64 conversion site

`SignalEvent::seq` is `u64`. SQLite `INTEGER` is signed 64-bit. The
conversion site is here. Values above `i64::MAX` are unreachable at
MVP event rates (would require ~9.2e18 events per bucket); the store
asserts via `i64::try_from` and returns `EventStoreError::SeqOverflow`
if the bound is somehow crossed.

## 10. Source-status

| Component | Status |
|---|---|
| Append + cursor reads | live (TC12) |
| FTS5 index (auto-maintained via triggers) | live (TC12) |
| Search APIs over FTS5 | reserved-for-TC13 (registry search) |
| Audit log table | deferred to TC22 |
| Registry table | deferred to TC13 |
| Backup/VACUUM INTO | live (TC12) |
| Retention eviction | live (TC12) |
