-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
-- Initial schema for the Terminal Commander event store (TC12).
-- See docs/storage/EVENT_STORE.md for the design.

CREATE TABLE IF NOT EXISTS buckets (
    bucket_id      TEXT NOT NULL PRIMARY KEY,
    created_at     TEXT NOT NULL,
    head_seq       INTEGER NOT NULL DEFAULT 0,
    tail_seq       INTEGER NOT NULL DEFAULT 0,
    dropped_count  INTEGER NOT NULL DEFAULT 0,
    max_events     INTEGER NOT NULL,
    ttl_secs       INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
    bucket_id                    TEXT NOT NULL,
    seq                          INTEGER NOT NULL,
    event_id                     TEXT NOT NULL,
    timestamp                    TEXT NOT NULL,
    severity_rank                INTEGER NOT NULL,
    severity                     TEXT NOT NULL,
    kind                         TEXT NOT NULL,
    summary                      TEXT NOT NULL,
    rule_id                      TEXT,
    rule_version                 INTEGER,
    captures                     TEXT NOT NULL,
    source                       TEXT NOT NULL,
    pointer                      TEXT,
    pointer_unavailable_reason   TEXT,
    tags                         TEXT,
    count                        INTEGER NOT NULL DEFAULT 1,
    first_seen                   TEXT,
    last_seen                    TEXT,
    suppressed                   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket_id, seq),
    FOREIGN KEY (bucket_id) REFERENCES buckets(bucket_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_events_event_id ON events(event_id);
CREATE INDEX IF NOT EXISTS idx_events_bucket_timestamp
    ON events(bucket_id, timestamp);

CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
    summary,
    kind,
    captures_text,
    content='events',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS events_ai
    AFTER INSERT ON events
BEGIN
    INSERT INTO events_fts(rowid, summary, kind, captures_text)
    VALUES (new.rowid, new.summary, new.kind, new.captures);
END;

CREATE TRIGGER IF NOT EXISTS events_ad
    AFTER DELETE ON events
BEGIN
    INSERT INTO events_fts(events_fts, rowid, summary, kind, captures_text)
    VALUES ('delete', old.rowid, old.summary, old.kind, old.captures);
END;

CREATE TRIGGER IF NOT EXISTS events_au
    AFTER UPDATE ON events
BEGIN
    INSERT INTO events_fts(events_fts, rowid, summary, kind, captures_text)
    VALUES ('delete', old.rowid, old.summary, old.kind, old.captures);
    INSERT INTO events_fts(rowid, summary, kind, captures_text)
    VALUES (new.rowid, new.summary, new.kind, new.captures);
END;
