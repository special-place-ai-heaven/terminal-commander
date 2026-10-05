// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Static guard: every process the daemon or the probes crate starts gets the
//! daemon-child environment (`TC_SOCKET`/`TC_DATA` removed, `TC_DAEMON_CHILD`
//! set). A process that inherited the daemon's endpoint and started a
//! terminal-commanderd -- the installed Linux autostart does, from any login
//! shell -- took over the daemon's socket. A new spawn site fails this test
//! until it is reviewed into `SITES`.

use std::path::{Path, PathBuf};

/// What a site built on `std::process::Command` (or tokio's) must call.
const STD: &[&str] = &["as_daemon_child("];
/// What a site whose builder is not a std `Command` must spell out.
const OTHER_BUILDER: &[&str] = &["DAEMON_ENDPOINT_ENV", "DAEMON_CHILD_ENV"];
/// Spawns in `#[cfg(test)]` code; they start no daemon child.
const TEST_ONLY: &[&str] = &[];

/// The required calls must appear within this many lines after the
/// constructor.
const WINDOW: usize = 6;

struct Site {
    /// Path under `crates/`.
    file: &'static str,
    /// Text identifying the constructor on its line.
    constructor: &'static str,
    what: &'static str,
    requires: &'static [&'static str],
}

const SITES: &[Site] = &[
    Site {
        file: "daemon/src/environment/probe.rs",
        constructor: "Command::new(program)",
        what: "host discovery probe",
        requires: STD,
    },
    Site {
        file: "daemon/src/credential.rs",
        constructor: "Command::new(askpass)",
        what: "$SSH_ASKPASS credential prompt",
        requires: STD,
    },
    Site {
        file: "daemon/src/credential.rs",
        constructor: r#"Command::new("ssh-askpass")"#,
        what: "ssh-askpass credential prompt",
        requires: STD,
    },
    Site {
        file: "daemon/src/credential.rs",
        constructor: r#"Command::new("zenity")"#,
        what: "zenity credential prompt",
        requires: STD,
    },
    Site {
        file: "daemon/src/credential.rs",
        constructor: r#"Command::new("kdialog")"#,
        what: "kdialog credential prompt",
        requires: STD,
    },
    Site {
        file: "daemon/src/credential.rs",
        constructor: r#"Command::new("pinentry")"#,
        what: "pinentry credential prompt",
        requires: STD,
    },
    Site {
        file: "daemon/src/ipc/handlers/recipe.rs",
        constructor: r#"Command::new("ping")"#,
        what: "test stand-in job",
        requires: TEST_ONLY,
    },
    Site {
        file: "daemon/src/ipc/handlers/recipe.rs",
        constructor: r#"Command::new("sleep")"#,
        what: "test stand-in job",
        requires: TEST_ONLY,
    },
    Site {
        file: "probes/src/process.rs",
        constructor: "Command::new(&argv[0])",
        what: "command lane (run_and_watch, shell_exec, recipes)",
        requires: STD,
    },
    Site {
        file: "probes/src/process.rs",
        constructor: r#"Command::new("kill")"#,
        what: "process-group TERM/KILL on cancel",
        requires: STD,
    },
    Site {
        file: "probes/src/pty.rs",
        constructor: "pty_process::Command::new(&argv[0])",
        what: "unix PTY lane and shell sessions",
        requires: OTHER_BUILDER,
    },
    Site {
        file: "probes/src/pty.rs",
        constructor: "CommandBuilder::new(&argv[0])",
        what: "Windows ConPTY lane",
        requires: OTHER_BUILDER,
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

#[test]
fn every_spawn_site_starts_a_daemon_child() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_files(&crates.join("daemon/src"), &mut files);
    rust_files(&crates.join("probes/src"), &mut files);

    let mut used = vec![false; SITES.len()];
    let mut problems = Vec::new();
    for path in files {
        let key = path
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(&path).unwrap();
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
                     environment (`as_daemon_child`) and add it to SITES"
                ));
                continue;
            };
            used[n] = true;
            let window: Vec<&str> = lines[i..lines.len().min(i + 1 + WINDOW)]
                .iter()
                .copied()
                .filter(|l| !is_comment(l))
                .collect();
            for call in SITES[n].requires {
                if !window.iter().any(|l| l.contains(call)) {
                    problems.push(format!(
                        "{at} ({}): no `{call}` within {WINDOW} lines",
                        SITES[n].what
                    ));
                }
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
