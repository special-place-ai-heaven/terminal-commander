// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Rule-pack import path (TC14).
//!
//! Reads a JSON rule pack from disk, validates every rule via
//! `RuleDefinition::validate()`, additionally compiles every regex
//! with bounded `regex::RegexBuilder::size_limit` / `dfa_size_limit`,
//! then inserts the rules into the registry via `create_rule_version`.
//!
//! Source-status: live (TC14). Activation on probe attach is
//! reserved for TC21.

use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json as sj;
use terminal_commander_core::{RuleDefinition, RuleType, compile_bounded_regex};

use crate::{EventStore, EventStoreError, Result};

/// Hard cap on a regex pattern's compiled state machine size.
///
/// Re-exported from [`terminal_commander_core`] so the rule-pack import path,
/// rule validation, and the sifter runtime all share one canonical bound.
pub use terminal_commander_core::REGEX_SIZE_LIMIT as RULE_PACK_REGEX_SIZE_LIMIT;
pub use terminal_commander_core::REGEX_DFA_SIZE_LIMIT as RULE_PACK_DFA_SIZE_LIMIT;

/// JSON shape of a rule pack file. Mirrors the TC14 seed packs in
/// `/rules/*.json`.
#[derive(Debug, Deserialize)]
pub struct RulePackFile {
    #[serde(rename = "_meta")]
    pub meta: RulePackMeta,
    pub rules: Vec<RuleDefinition>,
}

#[derive(Debug, Deserialize)]
pub struct RulePackMeta {
    pub pack: String,
    pub version: u32,
    #[serde(default)]
    pub description: Option<String>,
}

/// Outcome of importing a single pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportResult {
    pub pack: String,
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
}

/// The eight seed packs, embedded so the daemon needs no repo checkout
/// at runtime. Paths are relative to THIS source file
/// (`crates/store/src/import.rs`; repo root is `../../../`).
// Rules live INSIDE this crate (crates/store/rules/) so `cargo publish`
// can package them. include_str! reaching outside the crate root
// (../../../rules) breaks publish even though it works in a workspace
// build. Paths are relative to this source file (crates/store/src/).
const SEED_PACKS: &[(&str, &str)] = &[
    (
        "generic.terminal",
        include_str!("../rules/generic.terminal.json"),
    ),
    ("apt", include_str!("../rules/apt.json")),
    ("cargo", include_str!("../rules/cargo.json")),
    ("npm", include_str!("../rules/npm.json")),
    ("pytest", include_str!("../rules/pytest.json")),
    ("gcc", include_str!("../rules/gcc.json")),
    ("make", include_str!("../rules/make.json")),
    ("cleanup", include_str!("../rules/cleanup.json")),
];

/// Resolve a pack name to its embedded JSON, or `None` if unknown.
#[must_use]
pub fn resolve_pack_json(name: &str) -> Option<&'static str> {
    SEED_PACKS.iter().find(|(n, _)| *n == name).map(|(_, j)| *j)
}

/// The list of known pack names (for teaching errors).
#[must_use]
pub fn known_pack_names() -> Vec<&'static str> {
    SEED_PACKS.iter().map(|(n, _)| *n).collect()
}

impl EventStore {
    /// Import a rule pack from a JSON file at `path`. Returns a
    /// per-pack result summarizing imported and skipped rules.
    pub fn import_rule_pack(&mut self, path: impl AsRef<Path>) -> Result<ImportResult> {
        let path = path.as_ref();
        let bytes = fs::read_to_string(path).map_err(|e| {
            EventStoreError::InvalidPayload(format!("read {}: {e}", path.display()))
        })?;
        self.import_rule_pack_str(&bytes)
    }

    /// Same as `import_rule_pack`, but takes a JSON string. Useful
    /// for tests and admin-driven imports.
    pub fn import_rule_pack_str(&mut self, json: &str) -> Result<ImportResult> {
        let parsed: RulePackFile = sj::from_str(json)?;
        self.import_parsed_pack(parsed)
    }

    /// Import an embedded pack by NAME. When `promote_active` is true,
    /// each rule is stored with `status = Active` (so the caller can
    /// activate it through the normal eligibility gate). When false,
    /// rules keep their on-disk status (typically Draft, the vetting
    /// path).
    ///
    /// Returns `InvalidPayload` for an unknown pack name, with the
    /// known names listed so the caller can self-correct.
    pub fn import_rule_pack_by_name(
        &mut self,
        name: &str,
        promote_active: bool,
    ) -> Result<ImportResult> {
        let json = resolve_pack_json(name).ok_or_else(|| {
            EventStoreError::InvalidPayload(format!(
                "unknown rule pack '{name}'; known packs: {}",
                known_pack_names().join(", ")
            ))
        })?;
        let mut parsed: RulePackFile = sj::from_str(json)?;
        if promote_active {
            for rule in &mut parsed.rules {
                rule.status = terminal_commander_core::RuleStatus::Active;
            }
        }
        self.import_parsed_pack(parsed)
    }

    /// Shared import loop: validate each rule, bounded-compile regex
    /// rules, and insert via `create_rule_version`. Skipped rules are
    /// reported, not fatal.
    fn import_parsed_pack(&mut self, parsed: RulePackFile) -> Result<ImportResult> {
        let pack = parsed.meta.pack.clone();
        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        for rule in parsed.rules {
            // Application-layer validation first.
            if rule.validate().is_err() {
                skipped.push(rule.id.clone());
                continue;
            }
            // Bounded regex compile for regex rules.
            if rule.kind == RuleType::Regex {
                let Some(pat) = rule.pattern.as_deref() else {
                    skipped.push(rule.id.clone());
                    continue;
                };
                if compile_bounded_regex(pat).is_err() {
                    skipped.push(rule.id.clone());
                    continue;
                }
            }
            let id_for_log = rule.id.clone();
            self.create_rule_version(&rule)?;
            imported.push(id_for_log);
        }
        Ok(ImportResult {
            pack,
            imported,
            skipped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_pack_names_resolve_to_json() {
        assert!(resolve_pack_json("cargo").is_some());
        assert!(resolve_pack_json("pytest").is_some());
        assert!(resolve_pack_json("nope").is_none());
    }

    #[test]
    fn known_pack_names_lists_all_eight() {
        let names = known_pack_names();
        assert_eq!(names.len(), 8);
        assert!(names.contains(&"cargo"));
        assert!(names.contains(&"generic.terminal"));
        assert!(names.contains(&"cleanup"));
    }

    #[test]
    fn cleanup_pack_resolves_and_has_core_rules() {
        let json = resolve_pack_json("cleanup").expect("cleanup pack present");
        let parsed: RulePackFile = sj::from_str(json).unwrap();
        let ids: Vec<&str> = parsed.rules.iter().map(|r| r.id.as_str()).collect();
        for want in [
            "cleanup.disk-usage",
            "cleanup.dir-size",
            "cleanup.docker-usage",
            "cleanup.fstrim",
            "cleanup.space-reclaimed",
        ] {
            assert!(ids.contains(&want), "missing {want}");
        }
    }

    #[test]
    fn cleanup_pack_imports_and_renders_a_summary() {
        let mut s = EventStore::in_memory().unwrap();
        let res = s.import_rule_pack_by_name("cleanup", true).unwrap();
        assert!(res.skipped.is_empty(), "skipped: {:?}", res.skipped);
        let r = s.get_latest_rule("cleanup.fstrim").unwrap().unwrap();
        // The rule's template uses ${...} so render substitutes values.
        let mut caps = indexmap::IndexMap::new();
        caps.insert("mount".to_owned(), "/".to_owned());
        caps.insert("human".to_owned(), "2.1 GiB".to_owned());
        let rendered = r.render_summary(&caps).unwrap();
        assert_eq!(rendered.0, "trimmed /: 2.1 GiB");
    }

    #[test]
    fn import_by_name_active_promotes_status() {
        let mut s = EventStore::in_memory().unwrap();
        let res = s.import_rule_pack_by_name("cargo", true).unwrap();
        assert_eq!(res.pack, "cargo");
        assert!(!res.imported.is_empty());
        for id in &res.imported {
            let got = s.get_latest_rule(id).unwrap().unwrap();
            assert_eq!(got.status, terminal_commander_core::RuleStatus::Active);
        }
    }

    #[test]
    fn import_by_name_draft_keeps_status() {
        let mut s = EventStore::in_memory().unwrap();
        let res = s.import_rule_pack_by_name("cargo", false).unwrap();
        for id in &res.imported {
            let got = s.get_latest_rule(id).unwrap().unwrap();
            assert_eq!(got.status, terminal_commander_core::RuleStatus::Draft);
        }
    }

    #[test]
    fn import_by_unknown_name_is_err() {
        let mut s = EventStore::in_memory().unwrap();
        assert!(s.import_rule_pack_by_name("nope", false).is_err());
    }

    fn pack_minimal() -> &'static str {
        r#"{
            "_meta": { "pack": "test", "version": 1 },
            "rules": [
                {
                    "id": "test.permission",
                    "version": 1,
                    "kind": "keyword",
                    "status": "draft",
                    "severity": "high",
                    "event_kind": "permission_denied",
                    "keywords": ["Permission denied"],
                    "captures": [],
                    "summary_template": "permission denied",
                    "tags": ["terminal"]
                }
            ]
        }"#
    }

    #[test]
    fn import_minimal_pack_seeds_registry() {
        let mut s = EventStore::in_memory().unwrap();
        let res = s.import_rule_pack_str(pack_minimal()).unwrap();
        assert_eq!(res.pack, "test");
        assert_eq!(res.imported, vec!["test.permission".to_owned()]);
        assert!(res.skipped.is_empty());
        let got = s.get_latest_rule("test.permission").unwrap().unwrap();
        assert_eq!(got.severity, terminal_commander_core::Severity::High);
    }

    #[test]
    fn import_skips_invalid_rule() {
        let mut s = EventStore::in_memory().unwrap();
        let bad = r#"{
            "_meta": { "pack": "test", "version": 1 },
            "rules": [
                { "id": "", "version": 1, "kind": "keyword", "status": "draft",
                  "severity": "low", "event_kind": "x",
                  "keywords": ["k"], "captures": [], "summary_template": "ok", "tags": [] }
            ]
        }"#;
        let res = s.import_rule_pack_str(bad).unwrap();
        assert!(res.imported.is_empty());
        assert_eq!(res.skipped, vec![String::new()]);
    }

    #[test]
    fn import_skips_oversized_regex() {
        use std::fmt::Write as _;
        let mut s = EventStore::in_memory().unwrap();
        // 1023 alternations: most regex engines build a large DFA.
        let mut pat = String::from("^(");
        for i in 0..1023 {
            if i > 0 {
                pat.push('|');
            }
            let _ = write!(pat, "a{i}b{i}c{i}d{i}e{i}");
        }
        pat.push_str(")$");
        let body = sj::to_string(&sj::json!({
            "_meta": { "pack": "test", "version": 1 },
            "rules": [{
                "id": "test.oversized",
                "version": 1,
                "kind": "regex",
                "status": "draft",
                "severity": "low",
                "event_kind": "x",
                "pattern": pat,
                "captures": [],
                "summary_template": "ok",
                "tags": []
            }]
        }))
        .unwrap();
        let res = s.import_rule_pack_str(&body).unwrap();
        // Either validate() catches the length cap, or the regex
        // builder catches the size limit. Either way, skipped.
        assert!(res.imported.is_empty());
        assert_eq!(res.skipped, vec!["test.oversized".to_owned()]);
    }

    #[test]
    fn import_all_seed_packs_from_repo() {
        let mut s = EventStore::in_memory().unwrap();
        // Rules ship inside this crate (crates/store/rules/), so they
        // resolve from CARGO_MANIFEST_DIR directly.
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
        let packs = [
            "rules/generic.terminal.json",
            "rules/apt.json",
            "rules/cargo.json",
            "rules/npm.json",
            "rules/pytest.json",
            "rules/gcc.json",
            "rules/make.json",
        ];
        let mut total_imported = 0;
        for p in packs {
            let path = crate_root.join(p);
            let r = s.import_rule_pack(&path).unwrap_or_else(|e| {
                panic!("import {p}: {e}");
            });
            assert!(r.skipped.is_empty(), "pack {} skipped {:?}", p, r.skipped);
            assert!(!r.imported.is_empty(), "pack {p} was empty");
            total_imported += r.imported.len();
        }
        assert!(total_imported >= 12); // 7 packs, ~13 rules total
        // A representative search hits the apt pack.
        let hits = s.search_rules("apt", None).unwrap();
        assert!(!hits.is_empty());
    }
}
