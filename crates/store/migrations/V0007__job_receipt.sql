-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
-- Persistent job/bucket receipts (P1 / TC-B3, omni spec 001 FR-027).
--
-- A job receipt is a compact, durable record of a command's terminal
-- transition: enough for a status poll AFTER a daemon restart (when the
-- in-memory job map is gone) to return a known terminal/restart-marked
-- result instead of a bare error. Written on every terminal transition
-- (Exited / Cancelled / Failed). `final_signal_counts` is a small JSON
-- object of rule-driven event counts; `restarted_at` is NULL until a
-- post-restart read stamps it. Keyed by the opaque job id.

CREATE TABLE IF NOT EXISTS job_receipts (
    job_id               TEXT PRIMARY KEY,
    bucket_id            TEXT NOT NULL,
    terminal_state       TEXT NOT NULL,
    exit_code            INTEGER,
    final_signal_counts  TEXT NOT NULL,
    restarted_at         TEXT,
    created_at           TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_job_receipts_created_at
    ON job_receipts(created_at);
