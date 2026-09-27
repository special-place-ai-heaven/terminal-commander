// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Durable argv-recipe store. Same SQLite file as the event store and
//! the rule registry; tables are `recipe*` only. Not [`crate::registry`].

use rusqlite::{OptionalExtension, params};
use serde_json as sj;
use terminal_commander_core::{ActivationScope, RecipeDefinition, RecipeStatus};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::registry::{DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT, parse_scope};
use crate::{EventStore, EventStoreError, Result};

const MIGRATION_V0009: &str = include_str!("../migrations/V0009__recipes.sql");

/// One FTS hit. Latest non-tombstoned version only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeSearchHit {
    pub recipe_id: String,
    pub version: u32,
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub status: RecipeStatus,
    pub argv0: String,
}

/// One currently-open activation plus its definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRecipe {
    pub definition: RecipeDefinition,
    pub scope: ActivationScope,
}

/// `(version, created_at)` row from [`RecipeStore::list_versions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeVersionMeta {
    pub version: u32,
    pub created_at: OffsetDateTime,
}

/// Outcome of importing the built-in argv seed bank. Not a rule pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeSeedImport {
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
}

/// Borrowed writer/reader over the recipe tables.
pub struct RecipeStore<'a> {
    conn: &'a mut rusqlite::Connection,
}

impl EventStore {
    /// Run V0009. Idempotent.
    pub fn ensure_recipes(&mut self) -> Result<()> {
        let v9: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 9",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if v9 == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(MIGRATION_V0009)
                .map_err(|e| EventStoreError::Migration(e.to_string()))?;
            let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (9, ?1)",
                params![now_s],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Recipe API. Ensures the migration first.
    pub fn recipe_store(&mut self) -> Result<RecipeStore<'_>> {
        self.ensure_recipes()?;
        Ok(RecipeStore {
            conn: &mut self.conn,
        })
    }

    /// Import the compiled-in argv seed bank.
    ///
    /// `promote_active` stores each seed as `active` so a later activation
    /// gate can open a row. Otherwise seeds stay `tested` and are not
    /// activatable. Re-import of the same id and status skips that id
    /// (no extra version). A status change (tested, then active) stores
    /// a new version. Does not write `rules*` rows and does not activate.
    pub fn import_recipe_seeds(&mut self, promote_active: bool) -> Result<RecipeSeedImport> {
        let status = if promote_active {
            RecipeStatus::Active
        } else {
            RecipeStatus::Tested
        };
        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        for seed in terminal_commander_core::RECIPE_SEEDS {
            let incoming = seed
                .definition(status)
                .map_err(|err| EventStoreError::InvalidPayload(err.to_string()))?;
            let same = {
                let store = self.recipe_store()?;
                store
                    .get_latest(seed.recipe_id)?
                    .is_some_and(|latest| recipe_body_eq(&latest, &incoming))
            };
            if same {
                skipped.push(seed.recipe_id.to_owned());
                continue;
            }
            self.recipe_store()?.create_recipe_version(&incoming)?;
            imported.push(seed.recipe_id.to_owned());
        }
        Ok(RecipeSeedImport { imported, skipped })
    }
}

/// Content identity for skip-on-reimport. Version is assigned by the
/// store, so it is not part of the comparison. Status is: promoting
/// tested seeds to active must mint a new version.
fn recipe_body_eq(stored: &RecipeDefinition, incoming: &RecipeDefinition) -> bool {
    let mut left = stored.clone();
    let mut right = incoming.clone();
    left.version = 0;
    right.version = 0;
    left == right
}

impl RecipeStore<'_> {
    /// Insert the next immutable version. Does not activate.
    pub fn create_recipe_version(&mut self, def: &RecipeDefinition) -> Result<u32> {
        def.validate()
            .map_err(|e| EventStoreError::InvalidPayload(e.to_string()))?;
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        let tx = self.conn.transaction()?;

        let tombstoned: i64 = tx
            .query_row(
                "SELECT tombstoned FROM recipes WHERE recipe_id = ?1",
                params![&def.recipe_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if tombstoned == 1 {
            return Err(EventStoreError::InvalidPayload(format!(
                "recipe '{}' is tombstoned; cannot add a new version",
                def.recipe_id
            )));
        }

        let latest: i64 = tx
            .query_row(
                "SELECT latest_version FROM recipes WHERE recipe_id = ?1",
                params![&def.recipe_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let next_version_u = u32::try_from(latest).unwrap_or(0).saturating_add(1);
        let next_version_i = i64::from(next_version_u);

        if latest == 0 {
            tx.execute(
                "INSERT INTO recipes (recipe_id, latest_version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?3)",
                params![&def.recipe_id, next_version_i, &now_s],
            )?;
        } else {
            tx.execute(
                "UPDATE recipes SET latest_version = ?1, updated_at = ?2 WHERE recipe_id = ?3",
                params![next_version_i, &now_s, &def.recipe_id],
            )?;
        }

        let mut stored = def.clone();
        stored.version = next_version_u;
        let def_json = sj::to_string(&stored)?;
        tx.execute(
            "INSERT INTO recipe_versions (recipe_id, version, status, definition, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &def.recipe_id,
                next_version_i,
                stored.status.as_str(),
                &def_json,
                &now_s,
            ],
        )?;
        let rowid = tx.last_insert_rowid();

        for tag in &stored.tags {
            tx.execute(
                "INSERT INTO recipe_tags (recipe_id, version, tag) VALUES (?1, ?2, ?3)",
                params![&def.recipe_id, next_version_i, tag],
            )?;
        }

        let argv0 = stored.argv.first().map_or("", String::as_str);
        tx.execute(
            "INSERT INTO recipe_search (rowid, recipe_id, title, summary, tags_text, argv0)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                rowid,
                &stored.recipe_id,
                &stored.title,
                &stored.summary,
                stored.tags.join(" "),
                argv0,
            ],
        )?;

        tx.commit()?;
        Ok(next_version_u)
    }

    /// Latest stored definition, including a tombstoned parent.
    pub fn get_latest(&self, recipe_id: &str) -> Result<Option<RecipeDefinition>> {
        let latest: Option<i64> = self
            .conn
            .query_row(
                "SELECT latest_version FROM recipes WHERE recipe_id = ?1",
                params![recipe_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(latest) = latest else {
            return Ok(None);
        };
        if latest == 0 {
            return Ok(None);
        }
        self.get_version(recipe_id, u32::try_from(latest).unwrap_or(0))
    }

    /// One immutable version.
    pub fn get_version(&self, recipe_id: &str, version: u32) -> Result<Option<RecipeDefinition>> {
        let def_json: Option<String> = self
            .conn
            .query_row(
                "SELECT definition FROM recipe_versions WHERE recipe_id = ?1 AND version = ?2",
                params![recipe_id, i64::from(version)],
                |row| row.get(0),
            )
            .optional()?;
        match def_json {
            Some(s) => Ok(Some(sj::from_str(&s)?)),
            None => Ok(None),
        }
    }

    /// Versions oldest-first.
    pub fn list_versions(&self, recipe_id: &str) -> Result<Vec<RecipeVersionMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT version, created_at FROM recipe_versions
             WHERE recipe_id = ?1 ORDER BY version ASC",
        )?;
        let mut rows = stmt.query(params![recipe_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let version: i64 = row.get(0)?;
            let ts: String = row.get(1)?;
            out.push(RecipeVersionMeta {
                version: u32::try_from(version).unwrap_or(0),
                created_at: OffsetDateTime::parse(&ts, &Rfc3339)?,
            });
        }
        Ok(out)
    }

    /// Bounded FTS search. Metacharacters are quoted, same as `search_rules`.
    /// Tombstoned parents are omitted.
    pub fn search(&self, query: &str, limit: Option<usize>) -> Result<Vec<RecipeSearchHit>> {
        let lim = limit
            .unwrap_or(DEFAULT_SEARCH_LIMIT)
            .clamp(1, MAX_SEARCH_LIMIT);
        let match_query = fts5_quote_terms(query);
        if match_query.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT rv.recipe_id, rv.version, rv.status, rv.definition
               FROM recipe_search rs
               JOIN recipe_versions rv ON rv.rowid = rs.rowid
               JOIN recipes ru
                 ON ru.recipe_id = rv.recipe_id AND ru.latest_version = rv.version
              WHERE recipe_search MATCH ?1
                AND ru.tombstoned = 0
              ORDER BY rank
              LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![match_query, i64::try_from(lim).unwrap_or(i64::MAX)])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let recipe_id: String = row.get(0)?;
            let version_i: i64 = row.get(1)?;
            let status_s: String = row.get(2)?;
            let definition_s: String = row.get(3)?;
            let def: RecipeDefinition = sj::from_str(&definition_s)?;
            let status = RecipeStatus::parse(&status_s).ok_or_else(|| {
                EventStoreError::InvalidPayload(format!("unknown recipe status {status_s:?}"))
            })?;
            out.push(RecipeSearchHit {
                recipe_id,
                version: u32::try_from(version_i).unwrap_or(0),
                title: def.title.clone(),
                summary: def.summary.clone(),
                tags: def.tags.clone(),
                status,
                argv0: def.argv.first().cloned().unwrap_or_default(),
            });
        }
        Ok(out)
    }

    /// Open one scoped activation. Returns `true` when a new row was inserted.
    /// Only `status = active` versions of a non-tombstoned parent.
    pub fn record_activation_scoped(
        &mut self,
        recipe_id: &str,
        version: u32,
        scope: ActivationScope,
        profile: Option<&str>,
        actor: Option<&str>,
    ) -> Result<bool> {
        let tombstoned: i64 = self
            .conn
            .query_row(
                "SELECT tombstoned FROM recipes WHERE recipe_id = ?1",
                params![recipe_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if tombstoned == 1 {
            return Err(EventStoreError::InvalidPayload(format!(
                "recipe '{recipe_id}' is tombstoned"
            )));
        }
        let status_s: Option<String> = self
            .conn
            .query_row(
                "SELECT status FROM recipe_versions WHERE recipe_id = ?1 AND version = ?2",
                params![recipe_id, i64::from(version)],
                |row| row.get(0),
            )
            .optional()?;
        let Some(status_s) = status_s else {
            return Err(EventStoreError::InvalidPayload(format!(
                "recipe '{recipe_id}' version {version} not found"
            )));
        };
        let status = RecipeStatus::parse(&status_s).ok_or_else(|| {
            EventStoreError::InvalidPayload(format!("unknown recipe status {status_s:?}"))
        })?;
        if !status.is_activatable() {
            return Err(EventStoreError::InvalidPayload(format!(
                "recipe '{recipe_id}' v{version} has status {}, which cannot be activated; \
                 re-upsert with status \"active\"",
                status.as_str()
            )));
        }

        let scope_kind = scope.kind_label();
        let scope_value = scope.value_wire();
        let already_open: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM recipe_activations
              WHERE recipe_id = ?1
                AND version = ?2
                AND scope_kind = ?3
                AND ((?4 IS NULL AND scope_value IS NULL) OR scope_value = ?4)
                AND deactivated_at IS NULL",
            params![recipe_id, i64::from(version), scope_kind, scope_value],
            |row| row.get(0),
        )?;
        if already_open > 0 {
            return Ok(false);
        }
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        self.conn.execute(
            "INSERT INTO recipe_activations
                (recipe_id, version, activated_at, profile, actor, scope_kind, scope_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                recipe_id,
                i64::from(version),
                now_s,
                profile,
                actor,
                scope_kind,
                scope_value
            ],
        )?;
        Ok(true)
    }

    /// Close every open row for `(recipe_id, version, scope)`. `true` if any closed.
    pub fn deactivate_scoped(
        &mut self,
        recipe_id: &str,
        version: u32,
        scope: ActivationScope,
    ) -> Result<bool> {
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        let scope_kind = scope.kind_label();
        let scope_value = scope.value_wire();
        let changed = self.conn.execute(
            "UPDATE recipe_activations
                SET deactivated_at = ?1
              WHERE recipe_id = ?2
                AND version = ?3
                AND scope_kind = ?4
                AND ((?5 IS NULL AND scope_value IS NULL) OR scope_value = ?5)
                AND deactivated_at IS NULL",
            params![
                now_s,
                recipe_id,
                i64::from(version),
                scope_kind,
                scope_value
            ],
        )?;
        Ok(changed > 0)
    }

    /// Open activations, oldest first.
    pub fn list_active(&self) -> Result<Vec<ActiveRecipe>> {
        let mut stmt = self.conn.prepare(
            "SELECT rv.definition, ra.scope_kind, ra.scope_value
               FROM recipe_activations ra
               JOIN recipe_versions rv
                 ON rv.recipe_id = ra.recipe_id AND rv.version = ra.version
              WHERE ra.deactivated_at IS NULL
              ORDER BY ra.activated_at ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let def_s: String = row.get(0)?;
            let scope_kind: Option<String> = row.get(1).ok();
            let scope_value: Option<String> = row.get(2).ok();
            out.push(ActiveRecipe {
                definition: sj::from_str(&def_s)?,
                scope: parse_scope(scope_kind.as_deref(), scope_value.as_deref())?,
            });
        }
        Ok(out)
    }

    /// Mark the parent tombstoned. Versions stay readable. `false` if no such id.
    pub fn tombstone(&mut self, recipe_id: &str) -> Result<bool> {
        let now_s = OffsetDateTime::now_utc().format(&Rfc3339)?;
        let changed = self.conn.execute(
            "UPDATE recipes SET tombstoned = 1, updated_at = ?1 WHERE recipe_id = ?2",
            params![now_s, recipe_id],
        )?;
        Ok(changed > 0)
    }
}

/// Quote each whitespace-delimited term as an FTS5 string literal.
/// Mirrors `search_rules`: agent text must not be a query language.
fn fts5_quote_terms(query: &str) -> String {
    query
        .split_whitespace()
        .map(|tok| format!("\"{}\"", tok.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{RecipeDefinition, RecipeStatus};

    fn def(status: RecipeStatus) -> RecipeDefinition {
        RecipeDefinition {
            recipe_id: "git.status".to_owned(),
            version: 1,
            title: "Git status".to_owned(),
            summary: "Short working tree status".to_owned(),
            argv: vec!["git".to_owned(), "status".to_owned(), "--short".to_owned()],
            status,
            tags: vec!["git".to_owned(), "vcs".to_owned()],
            cwd: None,
            env_allowlist: vec![],
            timeout_ms: None,
            rule_pack_ids: vec![],
            placeholders: vec![],
        }
    }

    fn rules_schema(store: &EventStore) -> Vec<(String, Option<String>)> {
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT name, sql FROM sqlite_master
                 WHERE name IN ('rules','rule_versions','rule_tags','rule_activations','rule_search')
                 ORDER BY name",
            )
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn lifecycle_search_activate_tombstone_leaves_rules_alone() {
        let mut store = EventStore::in_memory().unwrap();
        store.ensure_registry().unwrap();
        let rule: terminal_commander_core::RuleDefinition = serde_json::from_str(
            r#"{"id":"r","version":1,"kind":"keyword","severity":"low","event_kind":"k","keywords":["x"],"summary_template":"s"}"#,
        )
        .unwrap();
        store.create_rule_version(&rule).unwrap();
        let schema_before = rules_schema(&store);
        let rules_before: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM rules", [], |row| row.get(0))
            .unwrap();

        let v1 = {
            let mut recipes = store.recipe_store().unwrap();
            let v1 = recipes
                .create_recipe_version(&def(RecipeStatus::Draft))
                .unwrap();
            assert_eq!(v1, 1);
            let got = recipes.get_latest("git.status").unwrap().unwrap();
            assert_eq!(got.version, 1);
            assert_eq!(got.argv[0], "git");
            let hits = recipes.search("status", None).unwrap();
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].recipe_id, "git.status");
            assert_eq!(hits[0].argv0, "git");
            // FTS metacharacters must not error.
            recipes.search("\" * : ( OR", None).unwrap();
            assert!(recipes.search("   ", None).unwrap().is_empty());
            let err = recipes
                .record_activation_scoped("git.status", v1, ActivationScope::Global, None, None)
                .unwrap_err();
            assert!(err.to_string().contains("cannot be activated"));
            v1
        };

        let v2 = {
            let mut recipes = store.recipe_store().unwrap();
            let mut active = def(RecipeStatus::Active);
            active.summary = "Short status for search".to_owned();
            let v2 = recipes.create_recipe_version(&active).unwrap();
            assert_eq!(v2, 2);
            assert!(
                recipes
                    .record_activation_scoped(
                        "git.status",
                        v2,
                        ActivationScope::Global,
                        Some("test"),
                        Some("unit")
                    )
                    .unwrap()
            );
            // Second activate is an open-row no-op.
            assert!(
                !recipes
                    .record_activation_scoped("git.status", v2, ActivationScope::Global, None, None)
                    .unwrap()
            );
            let listed = recipes.list_active().unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].definition.version, v2);
            assert_eq!(listed[0].scope, ActivationScope::Global);
            assert!(
                recipes
                    .deactivate_scoped("git.status", v2, ActivationScope::Global)
                    .unwrap()
            );
            assert!(recipes.list_active().unwrap().is_empty());
            assert!(recipes.tombstone("git.status").unwrap());
            assert!(recipes.get_latest("git.status").unwrap().is_some());
            assert!(recipes.search("status", None).unwrap().is_empty());
            let err = recipes.create_recipe_version(&active).unwrap_err();
            assert!(err.to_string().contains("tombstoned"));
            let versions = recipes.list_versions("git.status").unwrap();
            assert_eq!(versions.len(), 2);
            v2
        };
        let _ = (v1, v2);

        assert_eq!(rules_schema(&store), schema_before);
        let rules_after: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM rules", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rules_before, rules_after);
        let recipe_rows: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM recipes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(recipe_rows, 1);
    }

    #[test]
    fn import_recipe_seeds_lists_eight_and_keeps_shells_out() {
        use terminal_commander_core::shell_interpreter_denied;

        let mut store = EventStore::in_memory().unwrap();
        store.ensure_registry().unwrap();
        let rules_before: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM rules", [], |row| row.get(0))
            .unwrap();

        let first = store.import_recipe_seeds(false).unwrap();
        assert!(
            first.imported.len() >= 8,
            "import must list at least 8, got {:?}",
            first.imported
        );
        assert_eq!(first.imported.len(), 8, "{:?}", first.imported);
        assert!(first.skipped.is_empty());
        assert!(!first.imported.iter().any(|id| id == "rg.files"));
        assert!(
            store
                .recipe_store()
                .unwrap()
                .list_active()
                .unwrap()
                .is_empty()
        );
        for id in &first.imported {
            let def = store
                .recipe_store()
                .unwrap()
                .get_latest(id)
                .unwrap()
                .unwrap();
            assert_eq!(def.status, RecipeStatus::Tested);
            assert!(!def.argv.is_empty());
            assert!(
                shell_interpreter_denied(&def.argv[0]).is_none(),
                "{id} argv[0]={} is denied",
                def.argv[0]
            );
        }

        let again = store.import_recipe_seeds(false).unwrap();
        assert!(again.imported.is_empty());
        assert_eq!(again.skipped.len(), 8);

        let promoted = store.import_recipe_seeds(true).unwrap();
        assert_eq!(promoted.imported.len(), 8);
        for id in &promoted.imported {
            let def = store
                .recipe_store()
                .unwrap()
                .get_latest(id)
                .unwrap()
                .unwrap();
            assert_eq!(def.status, RecipeStatus::Active);
            assert_eq!(def.version, 2);
            assert!(shell_interpreter_denied(&def.argv[0]).is_none());
        }

        let rules_after: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM rules", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rules_before, rules_after);
    }

    #[test]
    fn interpreter_deny_rejects_before_insert() {
        let mut store = EventStore::in_memory().unwrap();
        let mut recipes = store.recipe_store().unwrap();
        for argv0 in ["bash", "sh", "zsh", "fish", "powershell", "pwsh", "cmd"] {
            let mut bad = def(RecipeStatus::Draft);
            bad.argv = vec![argv0.to_owned(), "-c".to_owned(), "echo".to_owned()];
            if argv0 == "pwsh" || argv0 == "powershell" {
                bad.argv = vec![
                    argv0.to_owned(),
                    "-Command".to_owned(),
                    "Get-Date".to_owned(),
                ];
            }
            let err = recipes.create_recipe_version(&bad).unwrap_err();
            assert!(
                err.to_string().contains("shell interpreter"),
                "{argv0}: {err}"
            );
        }
        assert!(recipes.get_latest("git.status").unwrap().is_none());
    }
}
