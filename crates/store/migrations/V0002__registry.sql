-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
-- Registry schema for the Terminal Commander rule store (TC13).
-- See docs/storage/REGISTRY_STORE.md for the design.

CREATE TABLE IF NOT EXISTS rules (
    rule_id        TEXT NOT NULL PRIMARY KEY,
    latest_version INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    tombstoned     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS rule_versions (
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

CREATE TABLE IF NOT EXISTS rule_tags (
    rule_id   TEXT NOT NULL,
    version   INTEGER NOT NULL,
    tag       TEXT NOT NULL,
    PRIMARY KEY (rule_id, version, tag),
    FOREIGN KEY (rule_id, version) REFERENCES rule_versions(rule_id, version) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_rule_tags_tag ON rule_tags(tag);

CREATE TABLE IF NOT EXISTS rule_activations (
    rule_id      TEXT NOT NULL,
    version      INTEGER NOT NULL,
    activated_at TEXT NOT NULL,
    deactivated_at TEXT,
    profile      TEXT,
    actor        TEXT,
    PRIMARY KEY (rule_id, version, activated_at),
    FOREIGN KEY (rule_id, version) REFERENCES rule_versions(rule_id, version)
);

CREATE VIRTUAL TABLE IF NOT EXISTS rule_search USING fts5(
    rule_id,
    event_kind,
    summary_template,
    tags_text,
    content='rule_versions',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2'
);
