// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Deterministic match from a shell-misuse deny to one activated recipe.
//!
//! No ML and no fuzzy score. A miss falls back to argv teach, which is
//! preferred over naming the wrong recipe.
//!
//! ponytail: eligibility is argv0 basename equality, or argv0 equal to
//! the recipe id (the operator typed the id as the command). Later
//! tokens never count as that id, so `rg git.status` does not steer.
//! Tags and id segments break ties; a tag never overrides a different
//! argv0. Leading shell interpreters and a short wrapper-flag list are
//! stripped, not a real shell parser (`wsl` / `env` carriers miss).
//! Upgrade path: reuse the daemon's carrier split if those misses show
//! up in dogfood.

use std::collections::BTreeMap;

use crate::recipe::RecipeDefinition;
use crate::shell_deny::shell_interpreter_denied;

/// What the denied call was trying to run.
#[derive(Debug, Clone, Copy)]
pub enum RecipeTeachIntent<'a> {
    /// `shell_exec` line, or a shell string nested in argv.
    ShellLine(&'a str),
    /// Argv-lane `argv` (`command_start_combed`, `run_and_watch`, PTY).
    Argv(&'a [String]),
}

/// `Some(recipe_id)` when exactly one activated recipe matches `intent`.
///
/// `active` must be the open activations only. Drafts are ignored even if
/// a caller passes them. Two open rows for the same id collapse when their
/// argv agrees; disagreeing argv is a miss for that id.
#[must_use]
pub fn match_activated_recipe<'a>(
    intent: RecipeTeachIntent<'_>,
    active: &'a [RecipeDefinition],
) -> Option<&'a str> {
    let tokens = intent_tokens(intent);
    let program = tokens.first().map(String::as_str)?;
    let recipes = consistent_active(active);
    // Command token only. A later token (`rg git.status`, `logs/git.log`)
    // is not "typed the recipe id as the command".
    let exact: Vec<&RecipeDefinition> = recipes
        .iter()
        .copied()
        .filter(|recipe| program.eq_ignore_ascii_case(&recipe.recipe_id))
        .collect();
    match exact.as_slice() {
        [only] => return Some(only.recipe_id.as_str()),
        [_, _, ..] => return None,
        [] => {}
    }
    let eligible: Vec<&RecipeDefinition> = recipes
        .iter()
        .copied()
        .filter(|recipe| argv0_eq(recipe, program))
        .collect();
    match eligible.as_slice() {
        [only] => return Some(only.recipe_id.as_str()),
        [] => return None,
        _ => {}
    }
    let narrowed: Vec<&RecipeDefinition> = eligible
        .iter()
        .copied()
        .filter(|recipe| {
            specific_tokens(recipe, program)
                .iter()
                .any(|spec| tokens.iter().any(|token| token == spec))
        })
        .collect();
    match narrowed.as_slice() {
        [only] => Some(only.recipe_id.as_str()),
        _ => None,
    }
}

fn consistent_active(active: &[RecipeDefinition]) -> Vec<&RecipeDefinition> {
    let mut groups: BTreeMap<&str, Vec<&RecipeDefinition>> = BTreeMap::new();
    for recipe in active {
        if !recipe.status.is_activatable() || recipe.argv.is_empty() {
            continue;
        }
        groups
            .entry(recipe.recipe_id.as_str())
            .or_default()
            .push(recipe);
    }
    groups
        .into_values()
        .filter_map(|group| {
            let first = *group.first()?;
            group
                .iter()
                .all(|row| row.argv == first.argv)
                .then_some(first)
        })
        .collect()
}

fn argv0_eq(recipe: &RecipeDefinition, program: &str) -> bool {
    recipe
        .argv
        .first()
        .is_some_and(|argv0| program_key(argv0) == program)
}

fn specific_tokens(recipe: &RecipeDefinition, program: &str) -> Vec<String> {
    let mut out = Vec::new();
    for arg in recipe.argv.iter().skip(1) {
        push_specific(&mut out, &program_key(arg), program);
    }
    for seg in id_segments(&recipe.recipe_id).into_iter().skip(1) {
        push_specific(&mut out, &seg, program);
    }
    for tag in &recipe.tags {
        push_specific(&mut out, &program_key(tag), program);
    }
    out
}

fn push_specific(out: &mut Vec<String>, token: &str, program: &str) {
    if token.is_empty() || token.starts_with('-') || token == program {
        return;
    }
    out.push(token.to_owned());
}

fn id_segments(recipe_id: &str) -> Vec<String> {
    recipe_id
        .split(['.', '_', '-'])
        .filter(|seg| !seg.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn intent_tokens(intent: RecipeTeachIntent<'_>) -> Vec<String> {
    let mut raw: Vec<String> = match intent {
        RecipeTeachIntent::ShellLine(line) => split_words(line),
        RecipeTeachIntent::Argv(argv) => argv.iter().flat_map(|arg| split_words(arg)).collect(),
    };
    raw = raw
        .into_iter()
        .map(|word| program_key(&word))
        .filter(|word| !word.is_empty())
        .collect();
    let start = skip_interpreter_prefix(&raw);
    raw.into_iter().skip(start).collect()
}

fn skip_interpreter_prefix(raw: &[String]) -> usize {
    if !raw.first().is_some_and(|token| is_shell_token(token)) {
        return 0;
    }
    let mut index = 1;
    while index < raw.len() {
        let token = &raw[index];
        if is_shell_token(token) || is_wrapper_flag(token) {
            index += 1;
            continue;
        }
        if is_valued_wrapper_flag(token) {
            index += 1;
            if index < raw.len() {
                index += 1;
            }
            continue;
        }
        break;
    }
    index
}

fn is_shell_token(token: &str) -> bool {
    shell_interpreter_denied(token).is_some()
}

const WRAPPER_FLAGS: &[&str] = &[
    "-c",
    "-lc",
    "-command",
    "--command",
    "/c",
    "-noprofile",
    "-noninteractive",
    "-nologo",
    "-nol",
    "-noexit",
    "-noni",
    "-nop",
];

const VALUED_WRAPPER_FLAGS: &[&str] = &[
    "-executionpolicy",
    "-inputformat",
    "-outputformat",
    "-windowstyle",
    "-encodedcommand",
    "-args",
    "-file",
];

fn is_wrapper_flag(token: &str) -> bool {
    WRAPPER_FLAGS.contains(&token)
}

fn is_valued_wrapper_flag(token: &str) -> bool {
    VALUED_WRAPPER_FLAGS.contains(&token)
}

fn program_key(raw: &str) -> String {
    let trimmed = raw.trim_matches(|c| c == '"' || c == '\'');
    // `cmd /c` is a flag. A real path has another separator (`/usr/bin/git`).
    let base = if trimmed.starts_with('/') && !trimmed[1..].contains(['/', '\\']) {
        trimmed
    } else {
        trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed)
    };
    let lower = base.to_ascii_lowercase();
    if let Some(stripped) = lower.strip_suffix(".exe")
        && !stripped.is_empty()
    {
        return stripped.to_owned();
    }
    lower
}

/// Whitespace split. Quotes are stripped, not grouped: `-Command "git status"`
/// must yield `git` and `status`. A path that contains spaces therefore misses
/// (over-narrow) instead of being parsed as one argv.
fn split_words(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in input.chars() {
        if ch == '"' || ch == '\'' {
            continue;
        }
        if ch.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::RecipeStatus;

    fn recipe(id: &str, argv: &[&str], tags: &[&str]) -> RecipeDefinition {
        RecipeDefinition {
            recipe_id: id.to_owned(),
            version: 1,
            title: "t".to_owned(),
            summary: "s".to_owned(),
            argv: argv.iter().map(|s| (*s).to_owned()).collect(),
            status: RecipeStatus::Active,
            tags: tags.iter().map(|s| (*s).to_owned()).collect(),
            cwd: None,
            env_allowlist: vec![],
            timeout_ms: None,
            rule_pack_ids: vec![],
            placeholders: vec![],
        }
    }

    fn line(input: &str) -> RecipeTeachIntent<'_> {
        RecipeTeachIntent::ShellLine(input)
    }

    fn matched(intent: RecipeTeachIntent<'_>, active: &[RecipeDefinition]) -> Option<String> {
        match_activated_recipe(intent, active).map(str::to_owned)
    }

    #[test]
    fn unique_argv0_matches_even_without_a_subcommand() {
        let active = [recipe(
            "git.status",
            &["git", "status", "--short"],
            &["git", "vcs"],
        )];
        assert_eq!(matched(line("git"), &active).as_deref(), Some("git.status"));
        assert_eq!(
            matched(line("git status --short"), &active).as_deref(),
            Some("git.status")
        );
    }

    #[test]
    fn no_recipe_or_different_argv0_misses() {
        let cargo = [recipe(
            "cargo.check",
            &["cargo", "check"],
            &["rust", "cargo"],
        )];
        assert_eq!(matched(line("git status"), &cargo), None);
        assert_eq!(matched(line("git status"), &[]), None);
        assert_eq!(matched(line("   "), &cargo), None);
        assert_eq!(matched(line("echo a | wc -c"), &cargo), None);
    }

    #[test]
    fn draft_is_not_a_match() {
        let mut draft = recipe("git.status", &["git", "status"], &["git"]);
        draft.status = RecipeStatus::Draft;
        assert_eq!(matched(line("git status"), &[draft]), None);
    }

    #[test]
    fn subcommand_narrows_and_bare_program_misses_when_ambiguous() {
        let active = [
            recipe("git.status", &["git", "status", "--short"], &["git", "vcs"]),
            recipe("git.diff", &["git", "diff"], &["git", "vcs"]),
        ];
        assert_eq!(matched(line("git"), &active), None);
        assert_eq!(
            matched(line("git status"), &active).as_deref(),
            Some("git.status")
        );
        assert_eq!(
            matched(line("git diff --stat"), &active).as_deref(),
            Some("git.diff")
        );
    }

    #[test]
    fn two_recipes_sharing_the_same_token_miss() {
        let active = [
            recipe("git.status", &["git", "status"], &[]),
            recipe("git.status.now", &["git", "status"], &[]),
        ];
        assert_eq!(matched(line("git status"), &active), None);
    }

    #[test]
    fn tag_and_id_suffix_break_ties_when_argv_is_only_the_program() {
        let by_tag = [
            recipe("alpha", &["git"], &["status"]),
            recipe("beta", &["git"], &["diff"]),
        ];
        assert_eq!(
            matched(line("git status"), &by_tag).as_deref(),
            Some("alpha")
        );
        let by_id = [
            recipe("git.status", &["git"], &["vcs"]),
            recipe("git.diff", &["git"], &["vcs"]),
        ];
        assert_eq!(
            matched(line("git diff"), &by_id).as_deref(),
            Some("git.diff")
        );
    }

    #[test]
    fn tag_does_not_override_a_different_argv0() {
        let tagged = recipe("rg.files", &["rg", "--files"], &["git", "search"]);
        let prefixed = recipe("git.status", &["rg", "--files"], &["search"]);
        assert_eq!(matched(line("git status"), &[tagged]), None);
        assert_eq!(matched(line("git status"), &[prefixed]), None);
    }

    #[test]
    fn exact_recipe_id_matches_only_the_command_token() {
        let active = [
            recipe("git.status", &["git", "status"], &["git"]),
            recipe("git.log", &["git", "log", "--oneline"], &["git"]),
            recipe("git.diff", &["git", "diff"], &["git"]),
        ];
        assert_eq!(
            matched(line("git.status"), &active).as_deref(),
            Some("git.status")
        );
        // Extra words do not create a second exact hit; argv0 is the id.
        assert_eq!(
            matched(line("git.status git.diff"), &active).as_deref(),
            Some("git.status")
        );
        assert_eq!(matched(line("rg git.status"), &active), None);
        assert_eq!(matched(line("echo git.status"), &active), None);
        assert_eq!(matched(line("tail -n 20 logs/git.log"), &active), None);
        let rg = ["rg".to_owned(), "git.status".to_owned()];
        assert_eq!(matched(RecipeTeachIntent::Argv(&rg), &active), None);
        let tail = [
            "tail".to_owned(),
            "-n".to_owned(),
            "20".to_owned(),
            "logs/git.log".to_owned(),
        ];
        assert_eq!(matched(RecipeTeachIntent::Argv(&tail), &active), None);
        let typed = ["git.status".to_owned()];
        assert_eq!(
            matched(RecipeTeachIntent::Argv(&typed), &active).as_deref(),
            Some("git.status")
        );
        let echo = ["echo".to_owned(), "git.status".to_owned()];
        assert_eq!(matched(RecipeTeachIntent::Argv(&echo), &active), None);
    }

    #[test]
    fn interpreter_prefix_case_and_exe_basename() {
        let active = [recipe("git.status", &["git", "status"], &["git"])];
        assert_eq!(
            matched(line("bash -lc 'git status'"), &active).as_deref(),
            Some("git.status")
        );
        assert_eq!(
            matched(
                line(r#"powershell.exe -NoProfile -Command "git status""#),
                &active
            )
            .as_deref(),
            Some("git.status")
        );
        assert_eq!(
            matched(line(r"cmd.exe /c git status"), &active).as_deref(),
            Some("git.status")
        );
        assert_eq!(
            matched(line(r"C:\Git\cmd\git.EXE status"), &active).as_deref(),
            Some("git.status")
        );
        assert_eq!(
            matched(line("/usr/bin/git status"), &active).as_deref(),
            Some("git.status")
        );
        let argv = ["bash".to_owned(), "-c".to_owned(), "git status".to_owned()];
        assert_eq!(
            matched(RecipeTeachIntent::Argv(&argv), &active).as_deref(),
            Some("git.status")
        );
    }

    #[test]
    fn wsl_carrier_misses() {
        let active = [recipe("git.status", &["git", "status"], &["git"])];
        assert_eq!(matched(line("wsl git status"), &active), None);
    }

    #[test]
    fn same_id_collapses_when_argv_agrees_and_misses_when_it_does_not() {
        let row = recipe("git.status", &["git", "status"], &["git"]);
        let again = row.clone();
        assert_eq!(
            matched(line("git status"), &[row, again]).as_deref(),
            Some("git.status")
        );
        let status = recipe("git.status", &["git", "status"], &[]);
        let diff = recipe("git.status", &["git", "diff"], &[]);
        assert_eq!(matched(line("git status"), &[status, diff]), None);
    }
}
