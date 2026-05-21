# Registry Store - Terminal Commander

Status: TC13 baseline.

Persistent rule registry. Lives in the same SQLite database file as
the event store (TC12); tables are namespaced under `rules*`.

Language: ASCII only.

## 1. Lock summary

- Backend: shared with TC12 (rusqlite 0.39 bundled, FTS5, WAL, NORMAL
  sync, busy_timeout 5s).
- Migrations: manual runner (matches TC12). Migration V0002 introduces
  the registry tables.
- Filesystem: 9P/drvfs placement is REJECTED at writer open
  (inherited from TC12).

## 2. Version identifier

Locked 2026-05-22 at TC13: monotonically increasing `u32` per
`rule_id`, starting at 1. Editing creates a new row in
`rule_versions(rule_id, version+1)` and updates
`rules.latest_version` inside the SAME transaction.

Content hashes are out of MVP scope (deferred to a post-MVP goal).

## 3. `latest` pointer

Locked 2026-05-22: dedicated `rules.latest_version` column. Updated
in the same tx as the version insert. No window-function lookups.

## 4. Validation

Locked 2026-05-22: application layer only. The store calls
`RuleDefinition::validate()` (TC09) before every insert. DB triggers
add complexity without buying anything for an in-process daemon.

## 5. Schema (v2, migration V0002)

```sql
CREATE TABLE rules (
    rule_id        TEXT NOT NULL PRIMARY KEY,
    latest_version INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    tombstoned     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE rule_versions (
    rule_id        TEXT NOT NULL,
    version        INTEGER NOT NULL,
    status         TEXT NOT NULL,
    severity       TEXT NOT NULL,
    kind           TEXT NOT NULL,
    event_kind     TEXT NOT NULL,
    definition     TEXT NOT NULL,   -- full RuleDefinition as JSON
    created_at     TEXT NOT NULL,
    PRIMARY KEY (rule_id, version),
    FOREIGN KEY (rule_id) REFERENCES rules(rule_id) ON DELETE CASCADE
);

CREATE TABLE rule_tags (
    rule_id  TEXT NOT NULL,
    version  INTEGER NOT NULL,
    tag      TEXT NOT NULL,
    PRIMARY KEY (rule_id, version, tag)
);

CREATE TABLE rule_activations (
    rule_id        TEXT NOT NULL,
    version        INTEGER NOT NULL,
    activated_at   TEXT NOT NULL,
    deactivated_at TEXT,
    profile        TEXT,
    actor          TEXT,
    PRIMARY KEY (rule_id, version, activated_at)
);

-- FTS5 over rule_versions.summary_template + tags (concatenated).
CREATE VIRTUAL TABLE rule_search USING fts5(
    rule_id, event_kind, summary_template, tags_text,
    content='rule_versions', content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2'
);
```

## 6. API

`RegistryStore::create_version(def)` — validates, picks `next_version
= max(latest_version, 0) + 1`, inserts a row, updates `rules`.

`RegistryStore::get_latest(rule_id)` / `get_version(rule_id, v)` —
returns the deserialized `RuleDefinition`.

`RegistryStore::list_versions(rule_id)` — returns `(version,
created_at)` pairs in ascending order.

`RegistryStore::search(query)` — bounded text + tag search via FTS5.
Default `LIMIT=50`, max `LIMIT=500`; out-of-range requests clamp.

`RegistryStore::record_activation(rule_id, version, profile, actor)`
— inserts an activation row. Advisory only at MVP; the runtime is
not bound here (TC14 / TC21).

`RegistryStore::tombstone(rule_id)` — sets `tombstoned=1` on the
parent row. Versions remain queryable; new versions cannot be added
while tombstoned.

## 7. Source-status

| Area | Status |
|---|---|
| create_version (regex + keyword) | live (TC13) |
| get_latest / get_version / list_versions | live |
| search (FTS5) | live |
| record_activation | live (advisory only) |
| tombstone | live |
| seed import from rule packs | reserved for TC14 |
| MCP `registry_*` tools | reserved for TC24 |
| kernel-level enforcement (Landlock, seccomp) | post-MVP |
