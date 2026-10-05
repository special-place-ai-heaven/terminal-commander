// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Every daemon config sample in the docs loads as written, with no
//! `config_warnings`: a documented key that does nothing would teach
//! operators a setting that is silently ignored.

use std::path::Path;

use terminal_commanderd::DaemonConfig;

/// The ```` ```toml ```` blocks of `doc`, repo-relative.
fn toml_blocks(doc: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(doc);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{doc}: {e}"));
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        match (&mut current, line.trim_end()) {
            (None, "```toml") => current = Some(String::new()),
            (Some(block), "```") => {
                blocks.push(std::mem::take(block));
                current = None;
            }
            (Some(block), l) => {
                block.push_str(l);
                block.push('\n');
            }
            (None, _) => {}
        }
    }
    blocks
}

#[test]
fn documented_config_samples_load_without_warnings() {
    let mut checked = 0;
    for doc in ["POLICY.md", "docs/runtime/SHELL_SESSION.md"] {
        for block in toml_blocks(doc) {
            // A fragment of terminal-commander.toml: add the required
            // tables it leaves out.
            let mut sample = String::new();
            if !block.contains("[daemon]") {
                sample.push_str("[daemon]\ndata_dir = \"/tmp/tc-doc-sample\"\n");
            }
            if !block.contains("[policy]") {
                sample.push_str("[policy]\nprofile = \"full_access\"\n");
            }
            sample.push_str(&block);
            let cfg = DaemonConfig::from_toml(&sample)
                .unwrap_or_else(|e| panic!("{doc} sample does not load: {e}\n{sample}"));
            assert!(
                cfg.warnings.is_empty(),
                "{doc} sample warns: {:?}\n{sample}",
                cfg.warnings
            );
            checked += 1;
        }
    }
    assert!(checked >= 4, "only {checked} samples found");
}
