// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Built-in argv recipe seeds (Dana bank, N=8).
//!
//! Local only: compiled into the binary, no marketplace and no remote
//! fetch. A seed is title/summary/tags/argv. It cannot carry `shell_line`,
//! secret values, or CAP01 fields because those fields do not exist.
//! Import assigns `tested` or `active`; this module does not activate.

use crate::recipe::{RecipeDefinition, RecipeError, RecipeStatus};

/// One curated argv recipe. Status is chosen at import, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipeSeed {
    pub recipe_id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub tags: &'static [&'static str],
    pub argv: &'static [&'static str],
}

impl RecipeSeed {
    /// Build a definition and run [`RecipeDefinition::validate`].
    ///
    /// # Errors
    /// The same failures as recipe upsert: empty argv, a denied shell
    /// interpreter, or `-c` / `-Command` smuggling.
    pub fn definition(&self, status: RecipeStatus) -> Result<RecipeDefinition, RecipeError> {
        let def = RecipeDefinition {
            recipe_id: self.recipe_id.to_owned(),
            version: 1,
            title: self.title.to_owned(),
            summary: self.summary.to_owned(),
            argv: self.argv.iter().map(|arg| (*arg).to_owned()).collect(),
            status,
            tags: self.tags.iter().map(|tag| (*tag).to_owned()).collect(),
            cwd: None,
            env_allowlist: Vec::new(),
            timeout_ms: None,
            rule_pack_ids: Vec::new(),
            placeholders: Vec::new(),
        };
        def.validate()?;
        Ok(def)
    }
}

/// Dana seed bank. Order is the import order.
///
/// ponytail: one static bank. A second bank would be another const slice
/// plus a name argument on import, same shape as rule packs.
pub const RECIPE_SEEDS: &[RecipeSeed] = &[
    RecipeSeed {
        recipe_id: "git.status",
        title: "Git short status",
        summary: "Show a compact working-tree status (`git status --short`) without opening a shell.",
        tags: &["git", "vcs"],
        argv: &["git", "status", "--short"],
    },
    RecipeSeed {
        recipe_id: "git.diff",
        title: "Git unstaged diff",
        summary: "Show unstaged and uncommitted changes with `git diff` (no pager flags; harness can bound output).",
        tags: &["git", "vcs"],
        argv: &["git", "diff"],
    },
    RecipeSeed {
        recipe_id: "git.log",
        title: "Recent commits (oneline)",
        summary: "List the last 20 commits in oneline form for a quick history skim.",
        tags: &["git", "vcs"],
        argv: &["git", "log", "--oneline", "-n", "20"],
    },
    RecipeSeed {
        recipe_id: "cargo.check",
        title: "Cargo typecheck",
        summary: "Run `cargo check` to typecheck the Rust workspace without producing a full release build.",
        tags: &["rust", "cargo", "build"],
        argv: &["cargo", "check"],
    },
    RecipeSeed {
        recipe_id: "cargo.test",
        title: "Cargo test suite",
        summary: "Run the default `cargo test` suite for the current Rust package or workspace.",
        tags: &["rust", "cargo", "test"],
        argv: &["cargo", "test"],
    },
    RecipeSeed {
        recipe_id: "npm.test",
        title: "npm test script",
        summary: "Run the package `test` script via `npm test` (argv form, not `cmd /c npm test`).",
        tags: &["node", "npm", "test"],
        argv: &["npm", "test"],
    },
    RecipeSeed {
        recipe_id: "node.version",
        title: "Node.js version probe",
        summary: "Print the installed Node.js version with `node --version` as a cheap environment probe.",
        tags: &["node", "probe"],
        argv: &["node", "--version"],
    },
    RecipeSeed {
        recipe_id: "git.ls-files",
        title: "List tracked files",
        summary: "List files tracked by git (`git ls-files`) without relying on `rg --files`.",
        tags: &["git", "vcs", "listing", "probe"],
        argv: &["git", "ls-files"],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell_deny::shell_interpreter_denied;

    const SMUGGLE_FLAGS: &[&str] = &["-c", "-lc", "-command", "/c"];

    #[test]
    fn dana_bank_is_eight_argv_safe_seeds() {
        let ids: Vec<&str> = RECIPE_SEEDS.iter().map(|seed| seed.recipe_id).collect();
        assert!(
            ids.len() >= 8,
            "seed bank must list at least 8, got {ids:?}"
        );
        assert_eq!(
            ids,
            [
                "git.status",
                "git.diff",
                "git.log",
                "cargo.check",
                "cargo.test",
                "npm.test",
                "node.version",
                "git.ls-files",
            ]
        );
        assert!(!ids.contains(&"rg.files"));
        for seed in RECIPE_SEEDS {
            let def = seed
                .definition(RecipeStatus::Tested)
                .unwrap_or_else(|err| panic!("{} failed validate: {err}", seed.recipe_id));
            assert!(!def.argv.is_empty(), "{}", seed.recipe_id);
            assert!(
                shell_interpreter_denied(&def.argv[0]).is_none(),
                "{} argv[0]={} is a denied shell",
                seed.recipe_id,
                def.argv[0]
            );
            assert!(
                def.argv.iter().all(|arg| !SMUGGLE_FLAGS
                    .iter()
                    .any(|flag| arg.eq_ignore_ascii_case(flag))),
                "{} argv smuggles a script flag: {:?}",
                seed.recipe_id,
                def.argv
            );
            assert_eq!(def.title, seed.title);
            assert_eq!(def.summary, seed.summary);
            assert_eq!(def.tags, seed.tags);
        }
    }
}
