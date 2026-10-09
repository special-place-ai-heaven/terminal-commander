// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Static guard over every process the daemon or the probes crate starts.
//!
//! 1. It gets the daemon-child environment (`TC_SOCKET`/`TC_DATA` removed,
//!    `TC_DAEMON_CHILD` set). A process that inherited the daemon's endpoint
//!    and started a terminal-commanderd -- the installed Linux autostart
//!    does, from any login shell -- took over the daemon's socket.
//! 2. It cannot inherit the daemon's own `WSLENV` unfiltered. A daemon started
//!    by the npm shim or the Windows logon task holds the ambient `WSLENV`,
//!    and `wsl.exe` forwards every Windows variable it names into Linux.
//!
//! A new spawn site fails this test until it is reviewed into `SITES`.

use std::path::{Path, PathBuf};

/// What a site built on `std::process::Command` (or tokio's) must call.
const STD: &[&str] = &["as_daemon_child("];
/// What a site whose builder is not a std `Command` must spell out.
const OTHER_BUILDER: &[&str] = &["DAEMON_ENDPOINT_ENV", "DAEMON_CHILD_ENV"];
/// Spawns in `#[cfg(test)]` code; they start no daemon child.
const TEST_ONLY: &[&str] = &[];

/// The daemon-child calls must appear within this many code lines after the
/// constructor.
const WINDOW: usize = 6;
/// The `WSLENV` handling must appear within this many code lines after it.
/// Includes the explicit clear/inherit branch and native cleanup setup.
const WSLENV_WINDOW: usize = 16;

/// How a site keeps the daemon's `WSLENV` from reaching its child unfiltered.
enum Wslenv {
    /// Rebuilt to the TC-only value (`core::sanitize_wslenv`).
    Rebuilt,
    /// The probe applies `config.env` after the daemon-child calls, and every
    /// daemon caller of the lane puts `filter_wslenv_for_spawn`'s result there
    /// (checked by `every_probe_lane_entry_filters_wslenv`).
    FromConfig,
    /// Compiled on unix only: `WSLENV` is read by `wsl.exe` on Windows, so a
    /// unix child cannot carry it into WSL. The nearest enclosing `#[cfg]`
    /// must be `#[cfg(unix)]`.
    UnixOnly,
    /// `#[cfg(test)]` code.
    TestOnly,
}

struct Site {
    /// Path under `crates/`.
    file: &'static str,
    /// Text identifying the constructor on its line.
    constructor: &'static str,
    what: &'static str,
    requires: &'static [&'static str],
    wslenv: Wslenv,
}

const SITES: &[Site] = &[
    Site {
        file: "daemon/src/environment/probe.rs",
        constructor: "Command::new(program)",
        what: "host discovery probe",
        requires: STD,
        wslenv: Wslenv::Rebuilt,
    },
    Site {
        file: "daemon/src/credential.rs",
        constructor: "Command::new(askpass)",
        what: "$SSH_ASKPASS credential prompt",
        requires: STD,
        wslenv: Wslenv::UnixOnly,
    },
    Site {
        file: "daemon/src/credential.rs",
        constructor: r#"Command::new("ssh-askpass")"#,
        what: "ssh-askpass credential prompt",
        requires: STD,
        wslenv: Wslenv::UnixOnly,
    },
    Site {
        file: "daemon/src/credential.rs",
        constructor: r#"Command::new("zenity")"#,
        what: "zenity credential prompt",
        requires: STD,
        wslenv: Wslenv::UnixOnly,
    },
    Site {
        file: "daemon/src/credential.rs",
        constructor: r#"Command::new("kdialog")"#,
        what: "kdialog credential prompt",
        requires: STD,
        wslenv: Wslenv::UnixOnly,
    },
    Site {
        file: "daemon/src/credential.rs",
        constructor: r#"Command::new("pinentry")"#,
        what: "pinentry credential prompt",
        requires: STD,
        wslenv: Wslenv::UnixOnly,
    },
    Site {
        file: "daemon/src/ipc/handlers/recipe.rs",
        constructor: r#"Command::new("ping")"#,
        what: "test stand-in job",
        requires: TEST_ONLY,
        wslenv: Wslenv::TestOnly,
    },
    Site {
        file: "daemon/src/ipc/handlers/recipe.rs",
        constructor: r#"Command::new("sleep")"#,
        what: "test stand-in job",
        requires: TEST_ONLY,
        wslenv: Wslenv::TestOnly,
    },
    Site {
        file: "probes/src/process.rs",
        constructor: "Command::new(&argv[0])",
        what: "command lane (run_and_watch, shell_exec, recipes)",
        requires: STD,
        wslenv: Wslenv::FromConfig,
    },
    Site {
        file: "probes/src/process.rs",
        constructor: r#"Command::new("sh")"#,
        what: "test-only ownership cleanup handshake",
        requires: TEST_ONLY,
        wslenv: Wslenv::TestOnly,
    },
    Site {
        file: "probes/src/pty.rs",
        constructor: "pty_process::Command::new(&argv[0])",
        what: "unix PTY lane and shell sessions",
        requires: OTHER_BUILDER,
        wslenv: Wslenv::FromConfig,
    },
    Site {
        file: "probes/src/pty.rs",
        constructor: "CommandBuilder::new(&argv[0])",
        what: "Windows ConPTY lane",
        requires: OTHER_BUILDER,
        wslenv: Wslenv::FromConfig,
    },
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// `(path under crates/, lines)` for every source file of `dirs`.
fn sources(dirs: &[&str]) -> Vec<(String, String)> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for dir in dirs {
        rust_files(&crates.join(dir), &mut files);
    }
    files
        .into_iter()
        .map(|path| {
            let key = path
                .strip_prefix(crates)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (key, std::fs::read_to_string(&path).unwrap())
        })
        .collect()
}

/// The first `n` code lines from `i` on (the constructor's line included).
fn code_after<'a>(lines: &[&'a str], i: usize, n: usize) -> Vec<&'a str> {
    lines[i..]
        .iter()
        .copied()
        .filter(|l| !is_comment(l))
        .take(n + 1)
        .collect()
}

/// Whether the nearest `#[cfg(..)]` on an item enclosing line `i` is
/// `#[cfg(unix)]`.
fn under_cfg_unix(lines: &[&str], i: usize) -> bool {
    let depth = indent(lines[i]);
    lines[..i]
        .iter()
        .rev()
        .find(|l| l.trim_start().starts_with("#[cfg(") && indent(l) < depth)
        .is_some_and(|l| l.trim() == "#[cfg(unix)]")
}

/// Problems with the `WSLENV` handling of the site whose constructor is on
/// line `i`.
fn wslenv_problem(site: &Site, lines: &[&str], i: usize) -> Option<String> {
    let after = code_after(lines, i, WSLENV_WINDOW);
    let within = |call: &str| after.iter().position(|l| l.contains(call));
    match site.wslenv {
        Wslenv::Rebuilt => within("sanitize_wslenv(")
            .is_none()
            .then(|| format!("no `sanitize_wslenv(` within {WSLENV_WINDOW} lines")),
        Wslenv::FromConfig => {
            let marked = site.requires.iter().filter_map(|c| within(c)).max();
            match (marked, within("config.env")) {
                (Some(m), Some(e)) if e > m => None,
                _ => Some(format!(
                    "`config.env` must be applied after the daemon-child calls, within \
                     {WSLENV_WINDOW} lines (it carries the lane's filtered WSLENV)"
                )),
            }
        }
        Wslenv::UnixOnly => (!under_cfg_unix(lines, i))
            .then(|| "listed as unix-only but not under `#[cfg(unix)]`".to_owned()),
        Wslenv::TestOnly => (!lines[..i].iter().any(|line| {
            indent(line) < indent(lines[i])
                && matches!(line.trim(), "#[cfg(test)]" | "#[cfg(all(test, unix))]")
        }))
        .then(|| "listed as test-only but not enclosed by a test cfg".to_owned()),
    }
}

#[test]
fn every_spawn_site_starts_a_daemon_child() {
    let mut used = vec![false; SITES.len()];
    let mut problems = Vec::new();
    for (key, source) in sources(&["daemon/src", "probes/src"]) {
        let lines: Vec<&str> = source.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if is_comment(line)
                || !(line.contains("Command::new(") || line.contains("CommandBuilder::new("))
            {
                continue;
            }
            let at = format!("{key}:{}", i + 1);
            let Some(n) = SITES
                .iter()
                .position(|s| s.file == key && line.contains(s.constructor))
            else {
                problems.push(format!(
                    "{at}: unreviewed process spawn; give it the daemon-child \
                     environment (`as_daemon_child`), keep the daemon's WSLENV from \
                     reaching it unfiltered, and add it to SITES"
                ));
                continue;
            };
            used[n] = true;
            let site = &SITES[n];
            let window = code_after(&lines, i, WINDOW);
            for call in site.requires {
                if !window.iter().any(|l| l.contains(call)) {
                    problems.push(format!(
                        "{at} ({}): no `{call}` within {WINDOW} lines",
                        site.what
                    ));
                }
            }
            if let Some(problem) = wslenv_problem(site, &lines, i) {
                problems.push(format!("{at} ({}): {problem}", site.what));
            }
        }
    }
    for (site, used) in SITES.iter().zip(used) {
        if !used {
            problems.push(format!(
                "SITES lists `{}` in {} but it is gone; remove the entry",
                site.constructor, site.file
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The `FromConfig` lanes: every daemon call that starts a command or PTY
/// probe filters the env it hands over, earlier in the same function.
#[test]
fn every_probe_lane_entry_filters_wslenv() {
    const FN_STARTS: [&str; 6] = [
        "fn ",
        "pub fn ",
        "pub(crate) fn ",
        "async fn ",
        "pub async fn ",
        "pub(crate) async fn ",
    ];
    let mut entries = 0;
    let mut problems = Vec::new();
    for (key, source) in sources(&["daemon/src"]) {
        let lines: Vec<&str> = source.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if is_comment(line)
                || !(line.contains("ProcessProbe::spawn") || line.contains("PtyProbe::spawn"))
            {
                continue;
            }
            entries += 1;
            let body = lines[..i]
                .iter()
                .rev()
                .take_while(|l| !FN_STARTS.iter().any(|f| l.trim_start().starts_with(f)));
            if !body
                .filter(|l| !is_comment(l))
                .any(|l| l.contains("filter_wslenv_for_spawn("))
            {
                problems.push(format!(
                    "{key}:{}: starts a probe without `filter_wslenv_for_spawn(` on its env \
                     earlier in the same function",
                    i + 1
                ));
            }
        }
    }
    assert!(
        entries >= 2,
        "expected the command and PTY lane entries, found {entries}"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Low-level Clear stays exact, while every isolated daemon child is marked
/// before reaching that lane. A cleared login shell must not start a daemon.
#[test]
fn isolated_daemon_lane_marks_children_without_inheriting_environment() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let process = std::fs::read_to_string(crates.join("probes/src/process.rs")).unwrap();
    let spawn = process
        .split_once("pub fn spawn_with_environment(")
        .unwrap()
        .1;
    let setup = spawn.split_once("cmd.args(").unwrap().0;
    assert!(setup.contains("if environment == EnvironmentMode::Clear"));
    assert!(setup.contains("cmd.env_clear()"));
    assert!(setup.contains("as_daemon_child(cmd.as_std_mut())"));

    let command = std::fs::read_to_string(crates.join("daemon/src/command.rs")).unwrap();
    let start = command
        .split_once("pub fn start_combed_with_environment(")
        .unwrap()
        .1;
    let start = start.split_once("self.start_combed_inner(").unwrap().0;
    let clear = start.find("EnvironmentMode::Clear").unwrap();
    let validated = start.find("validate_isolated_command(&req)?").unwrap();
    let marker = start
        .find(".push((terminal_commander_core::DAEMON_CHILD_ENV.into(), \"1\".into()))")
        .unwrap();
    assert!(clear < validated && validated < marker);
    let validator = command
        .split_once("fn validate_isolated_command(")
        .unwrap()
        .1;
    let validator = validator.split_once("Ok(())").unwrap().0;
    assert!(
        validator
            .contains("normalized == terminal_commander_core::DAEMON_CHILD_ENV && value != \"1\""),
        "an isolated caller must not disable the daemon-child marker"
    );
}
