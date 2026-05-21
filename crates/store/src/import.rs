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

use regex::RegexBuilder;
use serde::Deserialize;
use serde_json as sj;
use terminal_commander_core::{RuleDefinition, RuleType};

use crate::{EventStore, EventStoreError, Result};

/// Hard cap on a regex pattern's compiled state machine size.
pub const RULE_PACK_REGEX_SIZE_LIMIT: usize = 65_536;
pub const RULE_PACK_DFA_SIZE_LIMIT: usize = 65_536;

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
                if RegexBuilder::new(pat)
                    .size_limit(RULE_PACK_REGEX_SIZE_LIMIT)
                    .dfa_size_limit(RULE_PACK_DFA_SIZE_LIMIT)
                    .build()
                    .is_err()
                {
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
        // Run from the workspace root; CARGO_MANIFEST_DIR points
        // at the crate (crates/store), so go up two levels.
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
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
            let path = workspace_root.join(p);
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
