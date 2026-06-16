// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! US2 (T027) pack import + match tests for the P0 packs.
//!
//! Proves docker / kubectl / git rule packs:
//! 1. import cleanly (no skipped rules) into a fresh registry, and
//! 2. produce structured signal when evaluated against representative
//!    fixture output under `tests/fixtures/terminal/`.
//!
//! This is the closing half of FR-010 (the packs exist) plus the
//! evidence that they actually MATCH real tool output, not just that
//! they parse.

use std::path::PathBuf;

use terminal_commander_core::{BucketId, ProbeId, RuleDefinition, SourceFrame, SourceStream};
use terminal_commander_sifters::SifterRuntime;
use terminal_commander_store::{EventStore, RulePackFile};

/// Workspace root: `crates/daemon` -> `..` -> `..`.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// Read a terminal fixture into its lines.
fn fixture_lines(name: &str) -> Vec<String> {
    let path = workspace_root().join("tests/fixtures/terminal").join(name);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
    raw.lines().map(str::to_owned).collect()
}

/// Import a pack by name (promoted active) and return its rule defs,
/// asserting the import skipped nothing.
fn active_pack_rules(name: &str) -> Vec<RuleDefinition> {
    // Resolve the embedded pack JSON via the store seed loader so this
    // test exercises the SAME data the daemon ships.
    let json = terminal_commander_store::resolve_pack_json(name)
        .unwrap_or_else(|| panic!("pack {name} is not a known seed pack"));
    let parsed: RulePackFile = serde_json::from_str(json).expect("pack json parses");

    // Validate via a throwaway in-memory store (mirrors the daemon's
    // import path), asserting no rule is skipped.
    let mut store = EventStore::in_memory().expect("in-memory store");
    let result = store
        .import_rule_pack_str(json)
        .unwrap_or_else(|e| panic!("import {name}: {e}"));
    assert!(
        result.skipped.is_empty(),
        "pack {name} skipped rules on import: {:?}",
        result.skipped
    );
    assert!(!result.imported.is_empty(), "pack {name} imported nothing");

    // Promote to Active so the sifter runtime accepts them.
    parsed
        .rules
        .into_iter()
        .map(|mut r| {
            r.status = terminal_commander_core::RuleStatus::Active;
            r
        })
        .collect()
}

/// Evaluate every fixture line on both streams and collect the kinds
/// of every emitted draft.
fn match_kinds(rules: &[RuleDefinition], lines: &[String]) -> Vec<String> {
    let sifter = SifterRuntime::build(rules).expect("sifter builds from pack");
    let probe = ProbeId::new();
    let bucket = BucketId::new();
    let mut kinds = Vec::new();
    for line in lines {
        for stream in [SourceStream::Stdout, SourceStream::Stderr] {
            let frame = SourceFrame::new(probe, stream, line.clone());
            for draft in sifter.evaluate(&frame, bucket) {
                kinds.push(draft.kind);
            }
        }
    }
    kinds
}

#[test]
fn docker_pack_imports_and_matches_fixture() {
    let rules = active_pack_rules("docker");
    let lines = fixture_lines("docker-build-error.stderr");
    let kinds = match_kinds(&rules, &lines);
    assert!(
        kinds.iter().any(|k| k == "build_failed"),
        "docker pack must flag the ERROR build line; got {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "daemon_unreachable"),
        "docker pack must flag the daemon-unreachable line; got {kinds:?}"
    );
}

#[test]
fn kubectl_pack_imports_and_matches_fixture() {
    let rules = active_pack_rules("kubectl");
    let lines = fixture_lines("kubectl-errors.stderr");
    let kinds = match_kinds(&rules, &lines);
    assert!(
        kinds.iter().any(|k| k == "api_unreachable"),
        "kubectl pack must flag connection-refused; got {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "resource_not_found"),
        "kubectl pack must flag NotFound; got {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "permission_denied"),
        "kubectl pack must flag Forbidden; got {kinds:?}"
    );
}

#[test]
fn git_pack_imports_and_matches_fixture() {
    let rules = active_pack_rules("git");
    let lines = fixture_lines("git-conflict.stdout");
    let kinds = match_kinds(&rules, &lines);
    assert!(
        kinds.iter().any(|k| k == "merge_conflict"),
        "git pack must flag the CONFLICT line; got {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "command_failed"),
        "git pack must flag the fatal line; got {kinds:?}"
    );
}

#[test]
fn p0_packs_are_all_known_seed_packs() {
    for name in ["docker", "kubectl", "git"] {
        assert!(
            terminal_commander_store::resolve_pack_json(name).is_some(),
            "P0 pack {name} must be a registered seed pack (FR-010)"
        );
    }
}
