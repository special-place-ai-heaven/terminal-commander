# Persistent Audit Log

Status: Live (TC35).
Migration: V0003 (`crates/store/migrations/V0003__audit.sql`).
Storage crate module: `crates/store/src/audit.rs`.
Daemon-side sink module: `crates/daemon/src/audit.rs`.

The audit log replaces the in-memory `AuditPlaceholder` introduced
at TC21. It is the durable record of policy-relevant runtime
actions: bucket lifecycle, registry mutations, file reads, command
starts, and policy decisions.

## Design goals

1. Durable across daemon restarts.
2. Append-only at the row level. Monotonic `audit_id` via SQLite
   `INTEGER PRIMARY KEY AUTOINCREMENT` is the cursor.
3. Bounded per-row payloads. Audit MUST NOT become a backdoor for
   raw stream content or large blobs.
4. Closed-set decision labels. `decision` is one of
   `allow`, `deny`, `allow_with_audit`, `error`, `info`.
5. Same SQLite file as the event store and rule registry. Single
   writer, WAL journal mode.
6. Lazy migration: `EventStore::ensure_audit` is idempotent and
   runs on first use.
7. Closed-set decisions are enforced at the store layer; the
   daemon-side `InMemoryAudit` mirrors the check so unit tests catch
   drift.

## Schema (V0003)

```sql
CREATE TABLE audit_records (
    audit_id        INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp       TEXT NOT NULL,        -- RFC 3339 UTC
    action          TEXT NOT NULL,        -- open string, e.g. "bucket_create"
    subject         TEXT NOT NULL,        -- wire id or short label
    decision        TEXT NOT NULL,        -- closed set, see above
    profile         TEXT,                 -- policy profile name
    reason          TEXT,                 -- short human-readable, truncated
    actor           TEXT,                 -- "router", "mcp", "cli", ...
    metadata_json   TEXT                  -- pre-serialized JSON, capped
);
CREATE INDEX idx_audit_records_timestamp ON audit_records(timestamp);
CREATE INDEX idx_audit_records_action    ON audit_records(action);
CREATE INDEX idx_audit_records_decision  ON audit_records(decision);
```

No BLOB columns. No raw stream column. No environment-dump column.
A future doctrine change is required to add any column that could
carry stream-shaped data.

## Caps and validators

| Field | Cap | Behavior on overflow |
|---|---|---|
| `subject` | `MAX_AUDIT_SUBJECT_BYTES = 1024` | reject insert |
| `metadata_json` | `MAX_AUDIT_METADATA_BYTES = 4096` | reject insert |
| `reason` | `MAX_AUDIT_REASON_BYTES = 1024` | truncate at char boundary |
| `action` | non-empty | reject empty insert |
| `decision` | closed set | reject unknown insert |

Reads via `audit_since` are bounded by `MAX_AUDIT_READ_LIMIT = 10_000`
with a default of `DEFAULT_AUDIT_READ_LIMIT = 200`.

## Storage-side API (`crates/store/src/audit.rs`)

```rust
// Apply the V0003 migration if not already applied.
EventStore::ensure_audit(&mut self) -> Result<()>;

// Insert a new row. Returns the assigned audit_id.
EventStore::record_audit(&mut self, entry: &AuditEntry) -> Result<u64>;

// Cursor-based read with optional action/decision filters.
EventStore::audit_since(&mut self, req: &AuditReadRequest) -> Result<Vec<AuditRow>>;

// Operator-side count metric. Not exposed via MCP.
EventStore::audit_count(&mut self) -> Result<u64>;

// Schema reflection for structural tests.
EventStore::audit_table_columns(&mut self) -> Result<Vec<(String, String)>>;
```

## Daemon-side API (`crates/daemon/src/audit.rs`)

`AuditSink` trait abstracts the emission target. Two implementations
ship:

| Implementation | When used |
|---|---|
| `PersistentAudit` over `Arc<Mutex<EventStore>>` | Production / daemon runtime. Use `Router::with_sink`. |
| `InMemoryAudit` | Tests, library smoke, and the default `Router::new` constructor when no `EventStore` is configured. |

`PersistentAudit::ensure_migration` exists to push the V0003 migration
out of the first-emit critical path. Production callers should call
it once at daemon boot.

Router calls are best-effort with respect to the audit sink: an
audit emission error is dropped so the router can continue serving
the realtime signal channel. This matches the doctrine that audit
unhealthiness must not become a denial-of-service against the LLM
flow. Failures still surface in `audit_count()` (no increment) and
production callers should monitor the underlying `EventStore` health.

## Source-status

| Component | Status |
|---|---|
| V0003 migration | live (TC35) |
| `EventStore::record_audit` / `audit_since` / `audit_count` | live (TC35) |
| `PersistentAudit` sink | live (TC35) |
| `InMemoryAudit` sink | live (TC35; library + test only) |
| `Router::with_sink` constructor | live (TC35) |
| `Router::new` default sink | live (TC35; uses `InMemoryAudit`) |
| MCP-side audit read tools | not implemented (operator CLI reads only) |
| Hash chain / tamper evidence | deferred (BACKLOG P1) |

## Recorded contract tension

The closed-set audit-action doctrine in
`docs/contracts/enums/audit-action.md` (TC05 deliverable) is narrower
than the actions the router emits today
(`bucket_create`, `bucket_append`, `bucket_events_since`,
`bucket_wait`, `bucket_summary`, `event_context`, `job_start`,
`job_finish`, `job_cancel`). The store layer keeps `action TEXT` as
an open string because:

1. Forcing the router to emit only doctrine-listed actions would
   collapse useful runtime telemetry into one umbrella value.
2. Amending `docs/contracts/enums/audit-action.md` is out of TC35
   scope (it lives under `docs/contracts/**`, not in the allowed
   set).

A follow-up doctrine goal should reconcile the enum with runtime
emissions and either:

a) Expand the closed set to include router-runtime actions, or
b) Introduce a typed `AuditAction` enum at the daemon layer that
   maps to a documented superset stored as a free `action` string
   at the SQLite layer.

Recorded for a future docs-only goal; not in TC35.

## Test coverage

`crates/store/tests/audit_persistence.rs` (10 integration tests):

- `audit_rows_survive_store_reopen`
- `migration_v0003_idempotent_across_reopen`
- `rejects_unknown_decision`
- `rejects_empty_action`
- `caps_metadata_json_size`
- `caps_subject_size`
- `truncates_oversized_reason_without_failing`
- `filters_by_action_and_decision`
- `schema_does_not_have_raw_or_blob_columns`
- `raw_stream_text_rejected_via_metadata_cap`

`crates/daemon/tests/audit_router.rs` (2 integration tests):

- `router_audit_persists_across_store_reopen`
- `router_job_lifecycle_persists_audit`

`crates/daemon/src/audit.rs` (3 unit tests):

- `inmem_emit_and_read`
- `inmem_rejects_unknown_decision`
- `inmem_cursor_and_filters`
