-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
-- Recipe registry. Separate from rules* / rule_search (Ada fence:
-- copy the registry pattern, never share those tables).
-- Scope columns are included here; rules grew them later in V0004.

CREATE TABLE IF NOT EXISTS recipes (
    recipe_id      TEXT NOT NULL PRIMARY KEY,
    latest_version INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    tombstoned     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS recipe_versions (
    recipe_id   TEXT NOT NULL,
    version     INTEGER NOT NULL,
    status      TEXT NOT NULL,
    definition  TEXT NOT NULL,   -- full RecipeDefinition as JSON
    created_at  TEXT NOT NULL,
    PRIMARY KEY (recipe_id, version),
    FOREIGN KEY (recipe_id) REFERENCES recipes(recipe_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS recipe_tags (
    recipe_id TEXT NOT NULL,
    version   INTEGER NOT NULL,
    tag       TEXT NOT NULL,
    PRIMARY KEY (recipe_id, version, tag),
    FOREIGN KEY (recipe_id, version) REFERENCES recipe_versions(recipe_id, version) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_recipe_tags_tag ON recipe_tags(tag);

CREATE TABLE IF NOT EXISTS recipe_activations (
    recipe_id      TEXT NOT NULL,
    version        INTEGER NOT NULL,
    activated_at   TEXT NOT NULL,
    deactivated_at TEXT,
    profile        TEXT,
    actor          TEXT,
    scope_kind     TEXT NOT NULL DEFAULT 'global',
    scope_value    TEXT,
    PRIMARY KEY (recipe_id, version, activated_at),
    FOREIGN KEY (recipe_id, version) REFERENCES recipe_versions(recipe_id, version)
);

CREATE INDEX IF NOT EXISTS idx_recipe_activations_scope
    ON recipe_activations(recipe_id, version, scope_kind, scope_value, deactivated_at);

-- FTS5 over title / summary / tags / argv0. External content is
-- maintained by RecipeStore the same way rule_search is: explicit
-- rowid inserts, no triggers.
CREATE VIRTUAL TABLE IF NOT EXISTS recipe_search USING fts5(
    recipe_id,
    title,
    summary,
    tags_text,
    argv0,
    content='recipe_versions',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2'
);
