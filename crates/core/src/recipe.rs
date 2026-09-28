// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Argv recipe definition. A recipe names a command to run. It is not a
//! signal rule and not a beachhead route.
//!
//! The schema rejects secret fields, `shell_line`, marketplace URLs, and
//! CAP01 payloads by not having those fields (`deny_unknown_fields`).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::shell_deny::shell_argv_denied;

/// Maximum `recipe_id` length in bytes.
pub const MAX_RECIPE_ID_BYTES: usize = 128;
/// Maximum title length in bytes.
pub const MAX_RECIPE_TITLE_BYTES: usize = 200;
/// Maximum summary length in bytes.
pub const MAX_RECIPE_SUMMARY_BYTES: usize = 2_000;
/// Matches the command-start argv item cap.
pub const MAX_RECIPE_ARGV_ITEMS: usize = 256;
/// Matches the command-start per-item cap.
pub const MAX_RECIPE_ARGV_ITEM_BYTES: usize = 4_096;
/// Maximum tags stored on one version.
pub const MAX_RECIPE_TAGS: usize = 32;
/// Maximum bytes in one tag.
pub const MAX_RECIPE_TAG_BYTES: usize = 64;
/// Maximum cwd hint length in bytes.
pub const MAX_RECIPE_CWD_BYTES: usize = 4_096;
/// Upper bound on `timeout_ms` (24h). Zero is rejected.
pub const MAX_RECIPE_TIMEOUT_MS: u64 = 86_400_000;
/// Maximum soft rule-pack references.
pub const MAX_RECIPE_PACK_IDS: usize = 32;
/// Maximum named placeholders.
pub const MAX_RECIPE_PLACEHOLDERS: usize = 32;
/// MCP watch budget cap. Matches `run_and_watch`'s 60s wall clock.
pub const RECIPE_WATCH_CAP_MS: u64 = 60_000;

/// Lifecycle status stored on the definition. Only [`Active`](Self::Active)
/// may be activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeStatus {
    Draft,
    Tested,
    Active,
}

impl RecipeStatus {
    /// Column / wire label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Tested => "tested",
            Self::Active => "active",
        }
    }

    /// Whether `record_activation_scoped` may open a row for this status.
    #[must_use]
    pub const fn is_activatable(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Parse the status column.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "tested" => Some(Self::Tested),
            "active" => Some(Self::Active),
            _ => None,
        }
    }
}

/// Why a definition was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecipeError {
    /// Human-readable validation failure. The store surfaces this text.
    #[error("{0}")]
    Invalid(String),
}

/// Immutable argv recipe body. `version` on the wire is overwritten by
/// the store with the assigned version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDefinition {
    pub recipe_id: String,
    pub version: u32,
    pub title: String,
    pub summary: String,
    pub argv: Vec<String>,
    pub status: RecipeStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Environment variable names only. Values are rejected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_allowlist: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Soft references to existing rule packs. Not shared table ownership.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rule_pack_ids: Vec<String>,
    /// Named slots filled later at run. No free-form argv rewrite.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placeholders: Vec<String>,
}

impl RecipeDefinition {
    /// Check required fields, argv shape, and the shell-interpreter deny.
    ///
    /// # Errors
    /// Returns [`RecipeError::Invalid`] when the definition cannot be stored.
    pub fn validate(&self) -> Result<(), RecipeError> {
        validate_id(&self.recipe_id)?;
        if self.version == 0 {
            return Err(invalid("version must be >= 1"));
        }
        validate_text("title", &self.title, MAX_RECIPE_TITLE_BYTES)?;
        validate_text("summary", &self.summary, MAX_RECIPE_SUMMARY_BYTES)?;
        validate_argv(&self.argv)?;
        validate_tags(&self.tags)?;
        if let Some(cwd) = &self.cwd {
            validate_cwd(cwd)?;
        }
        validate_env_names(&self.env_allowlist)?;
        if let Some(ms) = self.timeout_ms
            && (ms == 0 || ms > MAX_RECIPE_TIMEOUT_MS)
        {
            return Err(invalid(format!(
                "timeout_ms must be 1..={MAX_RECIPE_TIMEOUT_MS}"
            )));
        }
        validate_names("rule_pack_ids", &self.rule_pack_ids, MAX_RECIPE_PACK_IDS)?;
        validate_placeholders(&self.placeholders)?;
        Ok(())
    }

    /// True when run should follow the `run_and_watch` path (timeout or pack hint).
    #[must_use]
    pub const fn prefers_watch(&self) -> bool {
        self.timeout_ms.is_some() || !self.rule_pack_ids.is_empty()
    }

    /// Bounded wait for [`Self::prefers_watch`]. `None` means combed start only.
    #[must_use]
    pub fn watch_budget_ms(&self) -> Option<u64> {
        if !self.prefers_watch() {
            return None;
        }
        // ponytail: one MCP wait budget, capped like run_and_watch. A longer
        // daemon-side waiter can replace this if recipes outgrow 60s.
        Some(
            self.timeout_ms
                .unwrap_or(5_000)
                .clamp(1, RECIPE_WATCH_CAP_MS),
        )
    }

    /// Substitute declared placeholders, then re-run the argv / interpreter gate.
    ///
    /// # Errors
    /// Unknown fills, missing fills for tokens that appear in argv, or an argv
    /// that fails [`validate`](Self::validate)'s argv rules (including shells).
    pub fn resolve_argv(
        &self,
        fills: &BTreeMap<String, String>,
    ) -> Result<Vec<String>, RecipeError> {
        let allowed: BTreeSet<String> = self
            .placeholders
            .iter()
            .map(|s| brace_name(s).to_owned())
            .collect();
        for key in fills.keys() {
            if !allowed.contains(key) {
                return Err(invalid(format!(
                    "fill '{key}' is not a declared placeholder"
                )));
            }
        }
        let mut argv = Vec::with_capacity(self.argv.len());
        for (index, arg) in self.argv.iter().enumerate() {
            argv.push(apply_fills(arg, &allowed, fills, index)?);
        }
        validate_argv(&argv)?;
        Ok(argv)
    }
}

fn invalid(msg: impl Into<String>) -> RecipeError {
    RecipeError::Invalid(msg.into())
}

fn validate_id(id: &str) -> Result<(), RecipeError> {
    if id.is_empty() {
        return Err(invalid("recipe_id is empty"));
    }
    if id.len() > MAX_RECIPE_ID_BYTES {
        return Err(invalid(format!(
            "recipe_id exceeds {MAX_RECIPE_ID_BYTES} bytes"
        )));
    }
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return Err(invalid("recipe_id is empty"));
    };
    if !first.is_ascii_alphanumeric() {
        return Err(invalid(
            "recipe_id must start with an ASCII letter or digit",
        ));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-') {
        return Err(invalid(
            "recipe_id may contain only ASCII letters, digits, '.', '_', and '-'",
        ));
    }
    Ok(())
}

fn validate_text(field: &str, value: &str, max: usize) -> Result<(), RecipeError> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{field} is empty")));
    }
    if value.len() > max {
        return Err(invalid(format!("{field} exceeds {max} bytes")));
    }
    if value.contains('\0') {
        return Err(invalid(format!("{field} contains NUL")));
    }
    Ok(())
}

fn validate_argv(argv: &[String]) -> Result<(), RecipeError> {
    if argv.is_empty() {
        return Err(invalid("argv must not be empty"));
    }
    if argv.len() > MAX_RECIPE_ARGV_ITEMS {
        return Err(invalid(format!(
            "argv has {} items; cap is {MAX_RECIPE_ARGV_ITEMS}",
            argv.len()
        )));
    }
    for (index, arg) in argv.iter().enumerate() {
        if arg.is_empty() || arg.len() > MAX_RECIPE_ARGV_ITEM_BYTES || arg.contains('\0') {
            return Err(invalid(format!(
                "argv item {index} is empty, contains NUL, or exceeds {MAX_RECIPE_ARGV_ITEM_BYTES} bytes"
            )));
        }
    }
    // Shared predicate: case, Windows extensions, wrappers, script flags.
    // `git -c` is not this shape: git is not on the deny list.
    if let Some(shell) = shell_argv_denied(argv) {
        return Err(invalid(format!(
            "shell interpreter '{shell}' is denied; recipe argv must not launch a shell \
             (including wrapped argv and -c, -ec, -Command, -EncodedCommand, /k)"
        )));
    }
    Ok(())
}

fn validate_tags(tags: &[String]) -> Result<(), RecipeError> {
    if tags.len() > MAX_RECIPE_TAGS {
        return Err(invalid(format!("tags exceed {MAX_RECIPE_TAGS}")));
    }
    for tag in tags {
        if tag.trim().is_empty() || tag.len() > MAX_RECIPE_TAG_BYTES || tag.contains('\0') {
            return Err(invalid(format!(
                "tag must be non-empty and at most {MAX_RECIPE_TAG_BYTES} bytes"
            )));
        }
    }
    Ok(())
}

fn validate_cwd(cwd: &str) -> Result<(), RecipeError> {
    validate_text("cwd", cwd, MAX_RECIPE_CWD_BYTES)?;
    let lower = cwd.trim().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Err(invalid(
            "cwd must not be a remote URL; recipes are local argv only",
        ));
    }
    Ok(())
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn validate_env_names(names: &[String]) -> Result<(), RecipeError> {
    if names.len() > MAX_RECIPE_TAGS {
        return Err(invalid("env_allowlist is too long"));
    }
    for name in names {
        if !valid_env_name(name) {
            return Err(invalid(format!(
                "env_allowlist entry '{name}' must be a name, not a value"
            )));
        }
    }
    Ok(())
}

fn validate_names(field: &str, names: &[String], max: usize) -> Result<(), RecipeError> {
    if names.len() > max {
        return Err(invalid(format!("{field} exceeds {max}")));
    }
    for name in names {
        if name.is_empty()
            || name.len() > MAX_RECIPE_ID_BYTES
            || name.contains('\0')
            || name.chars().any(char::is_whitespace)
        {
            return Err(invalid(format!("{field} entry '{name}' is not a bare id")));
        }
    }
    Ok(())
}

fn brace_name(slot: &str) -> &str {
    slot.strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(slot)
}

fn reject_fill_value(name: &str, value: &str) -> Result<(), RecipeError> {
    if value.is_empty() || value.contains('\0') {
        return Err(invalid(format!("fill '{name}' is empty or contains NUL")));
    }
    Ok(())
}

fn require_fill(
    name: &str,
    fills: &BTreeMap<String, String>,
    index: usize,
) -> Result<String, RecipeError> {
    let value = fills.get(name).ok_or_else(|| {
        invalid(format!(
            "placeholder '{name}' in argv[{index}] was not filled"
        ))
    })?;
    reject_fill_value(name, value)?;
    Ok(value.clone())
}

/// Replace declared `{name}` tokens in the original text only. Fill values
/// are not scanned again, so a value cannot introduce another slot.
fn apply_fills(
    arg: &str,
    allowed: &BTreeSet<String>,
    fills: &BTreeMap<String, String>,
    index: usize,
) -> Result<String, RecipeError> {
    let bare = brace_name(arg);
    let whole = allowed.contains(arg)
        || (arg.starts_with('{')
            && arg.ends_with('}')
            && allowed.contains(bare)
            && format!("{{{bare}}}") == arg);
    if whole {
        let name = if allowed.contains(arg) { arg } else { bare };
        return require_fill(name, fills, index);
    }
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('}') {
            let name = &after[..end];
            if allowed.contains(name) {
                let value = fills.get(name).ok_or_else(|| {
                    invalid(format!(
                        "placeholder '{{{name}}}' in argv[{index}] was not filled"
                    ))
                })?;
                reject_fill_value(name, value)?;
                out.push_str(value);
                rest = &after[end + 1..];
                continue;
            }
        }
        out.push('{');
        rest = after;
    }
    out.push_str(rest);
    Ok(out)
}

fn validate_placeholders(slots: &[String]) -> Result<(), RecipeError> {
    if slots.len() > MAX_RECIPE_PLACEHOLDERS {
        return Err(invalid(format!(
            "placeholders exceed {MAX_RECIPE_PLACEHOLDERS}"
        )));
    }
    for slot in slots {
        let bare = slot
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .unwrap_or(slot);
        if !valid_env_name(bare) || bare != slot && format!("{{{bare}}}") != *slot {
            return Err(invalid(format!(
                "placeholder '{slot}' must be a name or {{name}}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_def() -> RecipeDefinition {
        RecipeDefinition {
            recipe_id: "git.status".to_owned(),
            version: 1,
            title: "Git status".to_owned(),
            summary: "Short working-tree status".to_owned(),
            argv: vec!["git".to_owned(), "status".to_owned(), "--short".to_owned()],
            status: RecipeStatus::Draft,
            tags: vec!["git".to_owned()],
            cwd: None,
            env_allowlist: vec!["PATH".to_owned()],
            timeout_ms: Some(5_000),
            rule_pack_ids: vec!["git".to_owned()],
            placeholders: vec!["branch".to_owned(), "{rev}".to_owned()],
        }
    }

    #[test]
    fn accepts_a_plain_argv_recipe() {
        ok_def().validate().unwrap();
    }

    #[test]
    fn rejects_shell_interpreters_and_script_flags() {
        for argv in [
            vec!["bash".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
            vec!["/bin/sh".to_owned()],
            vec![
                "pwsh".to_owned(),
                "-Command".to_owned(),
                "Get-Date".to_owned(),
            ],
            vec!["powershell.exe".to_owned()],
            vec!["cmd".to_owned(), "/c".to_owned(), "dir".to_owned()],
            vec!["zsh".to_owned()],
            vec!["fish".to_owned()],
            vec![
                "env".to_owned(),
                "bash".to_owned(),
                "-c".to_owned(),
                "id".to_owned(),
            ],
            vec![
                r"C:\Windows\System32\cmd.exe".to_owned(),
                "/C".to_owned(),
                "dir".to_owned(),
            ],
            vec!["bash.exe".to_owned(), "-c".to_owned(), "whoami".to_owned()],
            vec!["sh.exe".to_owned(), "-c".to_owned(), "id".to_owned()],
            vec!["CMD".to_owned(), "/c".to_owned(), "dir".to_owned()],
            vec![
                "PowerShell".to_owned(),
                "-EncodedCommand".to_owned(),
                "QQ==".to_owned(),
            ],
            vec![
                "PoWeRsHeLl".to_owned(),
                "-Command".to_owned(),
                "Get-Date".to_owned(),
            ],
            vec![
                "env".to_owned(),
                "bash".to_owned(),
                "-ec".to_owned(),
                "id".to_owned(),
            ],
            vec![
                "env".to_owned(),
                "cmd.exe".to_owned(),
                "/k".to_owned(),
                "dir".to_owned(),
            ],
        ] {
            let mut def = ok_def();
            def.argv = argv;
            let err = def.validate().unwrap_err();
            assert!(
                err.to_string().contains("shell interpreter"),
                "expected deny, got {err}"
            );
        }
    }

    #[test]
    fn git_dash_c_is_not_a_shell() {
        let mut def = ok_def();
        def.argv = vec![
            "git".to_owned(),
            "-c".to_owned(),
            "color.ui=auto".to_owned(),
            "status".to_owned(),
        ];
        def.validate().unwrap();
    }

    #[test]
    fn rejects_secret_values_and_unknown_fields() {
        let mut def = ok_def();
        def.env_allowlist = vec!["AWS_SECRET=hunter2".to_owned()];
        assert!(def.validate().is_err());

        let raw = r#"{
            "recipe_id": "git.status",
            "version": 1,
            "title": "t",
            "summary": "s",
            "argv": ["git", "status"],
            "status": "draft",
            "shell_line": "echo hi"
        }"#;
        assert!(serde_json::from_str::<RecipeDefinition>(raw).is_err());
        for extra in ["secret", "marketplace_url", "tentacle", "password"] {
            let with = raw.replace("shell_line", extra);
            assert!(
                serde_json::from_str::<RecipeDefinition>(&with).is_err(),
                "{extra} must be rejected"
            );
        }
    }

    #[test]
    fn only_active_status_is_activatable() {
        assert!(!RecipeStatus::Draft.is_activatable());
        assert!(!RecipeStatus::Tested.is_activatable());
        assert!(RecipeStatus::Active.is_activatable());
    }

    #[test]
    fn resolve_argv_fills_declared_slots_and_denies_shell() {
        let mut def = ok_def();
        def.argv = vec![
            "{bin}".to_owned(),
            "status".to_owned(),
            "--{flag}".to_owned(),
        ];
        def.placeholders = vec!["bin".to_owned(), "{flag}".to_owned()];
        let argv = def
            .resolve_argv(&BTreeMap::from([
                ("bin".to_owned(), "git".to_owned()),
                ("flag".to_owned(), "short".to_owned()),
            ]))
            .unwrap();
        assert_eq!(argv, vec!["git", "status", "--short"]);

        let err = def
            .resolve_argv(&BTreeMap::from([
                ("bin".to_owned(), "bash".to_owned()),
                ("flag".to_owned(), "short".to_owned()),
            ]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("shell interpreter"), "{err}");

        let err = def
            .resolve_argv(&BTreeMap::from([("nope".to_owned(), "git".to_owned())]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a declared placeholder"), "{err}");
    }

    /// Run re-check: a placeholder argv validates, then the fill is denied.
    #[test]
    fn resolve_argv_rechecks_fcr001_shell_bypasses() {
        let cases: &[(&[&str], &str, &str)] = &[
            (&["{bin}", "-c", "id"], "bash.exe", "bash"),
            (&["{bin}", "-c", "id"], "sh.exe", "sh"),
            (&["{bin}", "/c", "dir"], "CMD", "cmd"),
            (
                &["{bin}", "-Command", "Get-Date"],
                "PowerShell",
                "powershell",
            ),
            (
                &["{bin}", "-Command", "Get-Date"],
                "PoWeRsHeLl",
                "powershell",
            ),
            (&["env", "{bin}", "-ec", "id"], "bash", "bash"),
            (&["env", "{bin}", "/k", "dir"], "cmd.exe", "cmd.exe"),
        ];
        for (argv, fill, expect) in cases {
            let mut stored = ok_def();
            stored.argv = argv.iter().map(|s| (*s).to_owned()).collect();
            stored.placeholders = vec!["bin".to_owned()];
            stored
                .validate()
                .unwrap_or_else(|err| panic!("template {argv:?} must validate: {err}"));
            let err = stored
                .resolve_argv(&BTreeMap::from([("bin".to_owned(), (*fill).to_owned())]))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("shell interpreter") && err.contains(expect),
                "run re-check fill {fill} argv {argv:?}: {err}"
            );
        }
    }

    #[test]
    fn watch_hint_follows_timeout_or_pack() {
        let mut def = ok_def();
        def.timeout_ms = None;
        def.rule_pack_ids.clear();
        assert!(!def.prefers_watch());
        def.timeout_ms = Some(2_000);
        assert_eq!(def.watch_budget_ms(), Some(2_000));
        def.timeout_ms = None;
        def.rule_pack_ids = vec!["git".to_owned()];
        assert_eq!(def.watch_budget_ms(), Some(5_000));
    }
}
