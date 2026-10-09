// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Static coverage gate: only documented in-scope Windows production spawn sites
//! may carry `windows_silent` / `CREATE_NO_WINDOW`.

#![cfg(windows)]

/// In-scope production sites (rev 4 spec). Any new site requires updating this
/// table and the bridge contract §4.4 paragraph.
const IN_SCOPE_SITES: &[(&str, &str)] = &[
    ("S1 ProcessProbe::spawn", "../probes/src/process.rs"),
    ("S2 host discovery probes", "src/environment/probe.rs"),
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

/// SECURITY gate (mirror of JS `wsl-static-guards`): every in-scope site that
/// launches a Linux process via `wsl.exe -e sh -c` must REBUILD `WSLENV`
/// (via `sanitize_wslenv`) so an ambient `WSLENV=SOME_SECRET/u` cannot leak
/// across the Windows->WSL boundary. The host-side `wsl -l -q` discovery call
/// launches no Linux process and is exempt.
#[test]
fn wsl_linux_spawn_sites_rebuild_wslenv() {
    let path = "src/environment/probe.rs";
    let source =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
            .unwrap_or_else(|e| panic!("read {path}: {e}"));
    assert!(
        source.contains(r#"&["-e", "sh", "-c""#),
        "{path} should still spawn a Linux process (test invariant moved if not)"
    );
    assert!(
        source.contains("sanitize_wslenv"),
        "{path} spawns `wsl.exe -e sh -c`; it MUST call sanitize_wslenv to \
         rebuild WSLENV (stop ambient credential leak across the WSL boundary)"
    );
}

/// SECURITY gate: every daemon file that spawns a model-issued child through a
/// probe must pass its env through `filter_wslenv_for_spawn`, so no spawn lane
/// can forward a secret-shaped ambient `WSLENV` entry into WSL.
#[test]
fn every_probe_spawn_lane_filters_wslenv() {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut lanes = 0;
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        if source.lines().any(|line| {
            !line.trim_start().starts_with("//")
                && (line.contains("ProcessProbe::spawn") || line.contains("PtyProbe::spawn"))
        }) {
            lanes += 1;
            assert!(
                source.contains("filter_wslenv_for_spawn("),
                "{} spawns a probe without filter_wslenv_for_spawn",
                path.display()
            );
        }
    }
    assert!(lanes >= 2, "expected the command and pty spawn lanes");
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
