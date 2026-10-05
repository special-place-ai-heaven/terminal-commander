// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Every `examples` entry in every built-in rule pack must hold against its
//! own rule, evaluated by the same code `registry_test`, `registry_upsert`
//! and `registry_import_pack` use. Reads `crates/store/rules/*.json` from
//! disk so a new pack file is covered even before it is registered.

use std::path::Path;

use terminal_commander_sifters::evaluate_examples;
use terminal_commander_store::RulePackFile;

#[test]
fn every_pack_file_agrees_with_its_own_examples() {
    let rules_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../store/rules");
    let mut files = 0;
    let mut examples = 0;
    for entry in std::fs::read_dir(&rules_dir).expect("read crates/store/rules") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        files += 1;
        let raw = std::fs::read_to_string(&path).expect("read pack");
        let pack: RulePackFile = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()));
        for rule in &pack.rules {
            for outcome in evaluate_examples(rule).expect("rule builds") {
                examples += 1;
                assert!(
                    outcome.passed(),
                    "{} / {} examples[{}]: {}",
                    pack.meta.pack,
                    rule.id,
                    outcome.index,
                    outcome.failure.unwrap_or_default()
                );
            }
        }
    }
    assert!(
        files >= 25,
        "expected the built-in packs, found {files} files"
    );
    assert!(examples > 0, "no pack examples were evaluated");
}
