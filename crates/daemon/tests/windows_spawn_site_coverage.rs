// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Static coverage gate: only documented in-scope Windows production spawn sites
//! may carry `windows_silent` / `CREATE_NO_WINDOW`.

#![cfg(windows)]

/// In-scope production sites (rev 4 spec). Any new site requires updating this
/// table and the bridge contract §4.4 paragraph.
const IN_SCOPE_SITES: &[(&str, &str)] = &[
    ("S1 ProcessProbe::spawn", "../probes/src/process.rs"),
    ("S2 wsl_username", "src/environment/wsl.rs"),
];

#[test]
fn in_scope_spawn_sites_use_windows_silent() {
    for (label, path) in IN_SCOPE_SITES {
        let source =
            std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
                .unwrap_or_else(|e| panic!("read {label} at {path}: {e}"));
        assert!(
            source.contains("windows_silent"),
            "{label} ({path}) must call windows_silent()"
        );
    }
}

#[test]
fn process_probe_uses_as_std_mut_for_flags() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../probes/src/process.rs"),
    )
    .expect("read process.rs");
    assert!(
        source.contains("as_std_mut()"),
        "ProcessProbe must apply flags via cmd.as_std_mut()"
    );
}
