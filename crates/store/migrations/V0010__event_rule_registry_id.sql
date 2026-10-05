-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
-- Persist the human registry rule id on signal events.
--
-- `events.rule_id` holds RuleRef.id, a one-way UUIDv5 of the registry id, so a
-- stored event could never tell a caller WHICH rule fired in terms they can
-- pass to registry_get / registry_activate. This adds the registry id itself.
--
-- Additive and nullable: rows written before this migration keep NULL and
-- decode exactly as before (RuleRef.registry_id = None). Older binaries keep
-- working against a migrated database because every statement they issue
-- names its columns explicitly.

ALTER TABLE events ADD COLUMN rule_registry_id TEXT;
