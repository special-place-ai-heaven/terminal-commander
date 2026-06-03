-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
-- Copyright 2026 The Terminal Commander Authors
--
-- V0005: drop the dormant `events_fts` FTS5 table and its maintenance
-- triggers.
--
-- V0001 created `events_fts` (external-content FTS5 over `events`) plus the
-- `events_ai` / `events_ad` / `events_au` triggers that mirror every event
-- INSERT/DELETE/UPDATE into the FTS index. No code path ever issues an
-- `events_fts MATCH`, so this was pure write-amplification on the hot
-- append/evict path for a search capability that was never exposed.
-- (See docs/audits/2026-06-01-correctness-trust-audit.md, finding L4.)
--
-- This forward migration removes the triggers first (so the table can no
-- longer be touched by event writes) and then the table. Every statement is
-- `IF EXISTS`, so it is:
--   * safe on a fresh DB (V0001 creates events_fts, this immediately drops it);
--   * safe on an existing/upgraded DB (events_fts present -> dropped);
--   * idempotent / re-runnable (no-op if already dropped).
--
-- The registry FTS table `rule_search` (V0002) is a SEPARATE, actively-queried
-- index and is intentionally left untouched. The `events` base table and all
-- other event-store objects are untouched.

DROP TRIGGER IF EXISTS events_ai;
DROP TRIGGER IF EXISTS events_ad;
DROP TRIGGER IF EXISTS events_au;
DROP TABLE IF EXISTS events_fts;
