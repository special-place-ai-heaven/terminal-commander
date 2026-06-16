-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
-- Persistent workspace snapshots (P1 / TC50, omni spec 001).
--
-- A workspace snapshot is a saved, restorable (cwd + bounded env)
-- captured from a shell session. The env map is stored as a single JSON
-- object string; the daemon bounds its size before persisting (no
-- unredacted host secrets). Keyed by an opaque snapshot id.

CREATE TABLE IF NOT EXISTS workspace_snapshots (
    snapshot_id        TEXT PRIMARY KEY,
    name               TEXT,
    source_session_id  TEXT,
    cwd                TEXT,
    env_json           TEXT NOT NULL,
    created_at         TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_workspace_snapshots_created_at
    ON workspace_snapshots(created_at);
