// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Static gate: supervisor production code must never spawn an external
//! process on a Windows code path.
//!
//! The daemon is GUI-subsystem on Windows (`crates/daemon/src/main.rs`), so a
//! console child spawned without CREATE_NO_WINDOW opens a VISIBLE,
//! focus-stealing terminal window. Regression this gate pins: `pid_alive`'s
//! `tasklist` probe popped a Windows Terminal window on every 15s
//! pidfile-reassert tick (observed 2026-07-16). Windows process work in this
//! crate is native Win32 (`replace::windows_native`); only the unix legs may
//! shell out, and only to the tools allowlisted below.
//!
//! The gate is a plain text scan, so it runs (and protects) on every host OS,
//! mirroring the daemon-side `windows_spawn_site_coverage` pattern.

use std::path::{Path, PathBuf};

/// Unix-only tools the supervisor legitimately shells out to. Every
/// production `Command::new(` must name one of these as a string literal, or
/// appear verbatim in [`ALLOWED_SITES`]. Anything else (tasklist, taskkill,
/// powershell, wmic, cmd.exe, or a non-literal program) fails the gate and
/// requires a deliberate decision plus an allowlist edit here.
const ALLOWED_PROGRAMS: &[&str] = &["kill", "pgrep"];

/// Documented non-literal spawn sites (cross-platform by design).
/// `spawn_daemon_impl` launches the GUI-subsystem daemon binary itself, which
/// never creates a console window.
const ALLOWED_SITES: &[&str] = &["Command::new(&opts.daemon_binary)"];

fn rs_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rs_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Everything before the first `#[cfg(test)]` / `#[cfg(all(test, ...))]`
/// marker counts as production source (house style keeps test modules at the
/// end of the file). Cutting early can only skip trailing test code, never a
/// production spawn site above the marker.
fn production_slice(source: &str) -> &str {
    let cut = [source.find("#[cfg(test)]"), source.find("#[cfg(all(test")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(source.len());
    &source[..cut]
}

#[test]
fn production_command_new_is_unix_allowlisted() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_sources(&src, &mut files);
    assert!(!files.is_empty(), "found no supervisor sources under src/");

    for file in &files {
        let source = std::fs::read_to_string(file).expect("read source");
        let prod = production_slice(&source);
        for (idx, _) in prod.match_indices("Command::new(") {
            let site_ok = ALLOWED_SITES.iter().any(|s| prod[idx..].starts_with(s));
            let rest = &prod[idx + "Command::new(".len()..];
            let literal_ok = rest.strip_prefix('"').is_some_and(|r| {
                ALLOWED_PROGRAMS.iter().any(|p| {
                    r.strip_prefix(p)
                        .is_some_and(|after| after.starts_with('"'))
                })
            });
            let line = prod[..idx].lines().count() + 1;
            assert!(
                site_ok || literal_ok,
                "{}:{line}: production Command::new must name an allowlisted \
                 unix tool literal ({ALLOWED_PROGRAMS:?}) or be a documented \
                 site ({ALLOWED_SITES:?}). Windows process work must go \
                 through replace::windows_native (native Win32 — a console \
                 child without CREATE_NO_WINDOW pops a visible terminal \
                 window from the GUI-subsystem daemon).",
                file.display(),
            );
        }
        // Quoted form only: comments may cite the removed tools by name, but
        // spawning one requires the program name as a string literal.
        assert!(
            !prod.contains("\"tasklist\"") && !prod.contains("\"taskkill\""),
            "{}: tasklist/taskkill must never return to supervisor \
             production code (use replace::windows_native)",
            file.display(),
        );
    }
}
