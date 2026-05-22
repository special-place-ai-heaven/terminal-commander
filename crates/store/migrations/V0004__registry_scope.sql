-- SPDX-License-Identifier: Apache-2.0
-- TC42c: scoped activation columns on rule_activations.
--
-- Adds `scope_kind` + `scope_value` so the same (rule_id, version)
-- can be active under disjoint scopes simultaneously. Existing rows
-- (TC42 / TC42b) backfill as global by the column default. No
-- existing primary key is altered; the open-row uniqueness invariant
-- is enforced by the daemon's deactivate_rule UPDATE which matches
-- on (rule_id, version, scope_kind, scope_value, deactivated_at IS NULL).

ALTER TABLE rule_activations ADD COLUMN scope_kind  TEXT NOT NULL DEFAULT 'global';
ALTER TABLE rule_activations ADD COLUMN scope_value TEXT;

CREATE INDEX IF NOT EXISTS idx_rule_activations_scope
    ON rule_activations(rule_id, version, scope_kind, scope_value, deactivated_at);
