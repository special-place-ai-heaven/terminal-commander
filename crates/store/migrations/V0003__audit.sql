-- SPDX-License-Identifier: Apache-2.0
-- Persistent audit log (TC35). Replaces the in-memory
-- AuditPlaceholder seam with durable rows.
-- See docs/storage/AUDIT_LOG.md for the design.

CREATE TABLE IF NOT EXISTS audit_records (
    audit_id        INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp       TEXT NOT NULL,
    action          TEXT NOT NULL,
    subject         TEXT NOT NULL,
    decision        TEXT NOT NULL,
    profile         TEXT,
    reason          TEXT,
    actor           TEXT,
    metadata_json   TEXT
);

CREATE INDEX IF NOT EXISTS idx_audit_records_timestamp
    ON audit_records(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_records_action
    ON audit_records(action);
CREATE INDEX IF NOT EXISTS idx_audit_records_decision
    ON audit_records(decision);
