// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! A model-issued WSL launch must not forward a secret-shaped ambient
//! `WSLENV` entry into WSL. `wsl.exe` forwards every Windows variable named in
//! `WSLENV`, so `WSLENV=SUDO_PASSWORD/u` would hand the password to the Linux
//! process. The real adapter and daemon run with such an ambient `WSLENV`;
//! `wsl.exe` here is a copy of `cmd.exe` that echoes the `WSLENV` it received,
//! so nothing launches a real distro.

#![cfg(windows)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

const SECRET_NAME: &str = "TC_TEST_SUDO_PASSWORD";
const SECRET_VALUE: &str = "value-never-surfaces-7Q";
const AMBIENT: &str = "TC_TEST_SUDO_PASSWORD/u:TC_TEST_HARMLESS/u";

fn target_bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe.parent().and_then(|p| p.parent()).expect("profile dir");
    profile_dir.join(name).with_extension("exe")
}

struct Adapter {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    state_dir: PathBuf,
    dir: TempDir,
    next_id: u64,
    /// Every message the adapter sent, for the leak check.
    seen: Vec<String>,
}

impl Adapter {
    fn start() -> Self {
        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join("state");
        let socket = format!(
            r"\\.\pipe\tc-test-wslenv-{}-{}",
            std::process::id(),
            dir.path().file_name().unwrap().to_string_lossy()
        );
        let mut child = Command::new(target_bin("terminal-commander-mcp"))
            .arg("--state-dir")
            .arg(&state_dir)
            .env("TC_SOCKET", &socket)
            .env("TC_DATA", dir.path())
            .env("WSLENV", AMBIENT)
            .env(SECRET_NAME, SECRET_VALUE)
            .env("TC_TEST_HARMLESS", "harmless")
            .env_remove("TC_SESSION")
            .env_remove("TC_SURFACE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn adapter");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            state_dir,
            dir,
            next_id: 0,
            seen: Vec::new(),
        }
    }

    fn call(&mut self, name: &str, arguments: &Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                         "params": {"name": name, "arguments": arguments}});
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_mins(1))
                .expect("no response from adapter within 60s");
            self.seen.push(line.clone());
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg.get("id") == Some(&json!(id)) {
                let text = msg["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{name} failed: {msg}"));
                return serde_json::from_str(text).unwrap_or_else(|_| json!({ "text": text }));
            }
        }
    }

    /// `WSLENV` as the child saw it, via the echo rule.
    fn child_wslenv(&mut self, argv: &[&str], env: &Value) -> (String, Value) {
        let r = self.call(
            "run_and_watch",
            &json!({"argv": argv, "env": env, "wait_ms": 20000, "wait_until": "exit",
                   "rules": [{"pattern": "^WSLENV="}]}),
        );
        let seen = r["signals"][0]["summary"]
            .as_str()
            .unwrap_or_else(|| panic!("no WSLENV line: {r}"))
            .trim()
            .to_owned();
        (seen, r)
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        if let Some(d) = terminal_commander_supervisor::pidfile::read_pidfile_raw(&self.state_dir) {
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &d.pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `cmd.exe` copied under `name`: `name /c echo WSLENV=%WSLENV%` prints the
/// `WSLENV` it was started with.
fn echo_program(dir: &Path, name: &str) -> String {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    let to = dir.join(name);
    std::fs::copy(Path::new(&system_root).join(r"System32\cmd.exe"), &to).unwrap();
    to.display().to_string()
}

fn files_containing(dir: &Path, needle: &[u8], out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_containing(&path, needle, out);
        } else if std::fs::read(&path).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle)) {
            out.push(path);
        }
    }
}

#[test]
fn secret_shaped_wslenv_entries_stay_out_of_model_issued_wsl_launches() {
    let mut a = Adapter::start();
    let bin = TempDir::new().unwrap();
    let wsl = echo_program(bin.path(), "wsl.exe");
    let other = echo_program(bin.path(), "notwsl.exe");
    let echo = ["/c", "echo", "WSLENV=%WSLENV%"];

    // A WSL launch keeps the harmless entry and names the dropped one.
    let argv: Vec<&str> = std::iter::once(wsl.as_str()).chain(echo).collect();
    let (seen, r) = a.child_wslenv(&argv, &json!([]));
    assert_eq!(seen, "WSLENV=TC_TEST_HARMLESS/u", "{r}");
    assert_eq!(r["wslenv_dropped"]["names"], json!([SECRET_NAME]), "{r}");

    // A WSLENV the caller passes is used as given.
    let (seen, r) = a.child_wslenv(
        &argv,
        &json!([{"key": "WSLENV", "value": "TC_TEST_SUDO_PASSWORD/u"}]),
    );
    assert_eq!(seen, "WSLENV=TC_TEST_SUDO_PASSWORD/u", "{r}");
    assert!(r.get("wslenv_dropped").is_none(), "{r}");

    // Not WSL and not a shell: the env is untouched.
    let argv_other: Vec<&str> = std::iter::once(other.as_str()).chain(echo).collect();
    let (seen, r) = a.child_wslenv(&argv_other, &json!([]));
    assert_eq!(seen, format!("WSLENV={AMBIENT}"), "{r}");
    assert!(r.get("wslenv_dropped").is_none(), "{r}");

    // The PTY lane filters the same way.
    let p = a.call("pty_command_start", &json!({"argv": argv}));
    assert_eq!(p["wslenv_dropped"]["names"], json!([SECRET_NAME]), "{p}");
    let job = p["job_id"].as_str().unwrap().to_owned();
    let mut tail = Value::Null;
    for _ in 0..100 {
        tail = a.call(
            "command_output_tail",
            &json!({"job_id": job, "strip_ansi": true}),
        );
        if tail.to_string().contains("WSLENV=") {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        tail.to_string().contains("WSLENV=TC_TEST_HARMLESS/u"),
        "{tail}"
    );
    assert!(!tail.to_string().contains(SECRET_NAME), "{tail}");

    // self_check names the exposure, never a value.
    let report = a.call("self_check", &json!({}));
    assert!(report.to_string().contains(SECRET_NAME), "{report}");

    // The audit rows name the dropped variable.
    let mut audit = Vec::new();
    files_containing(a.dir.path(), b"wslenv_dropped", &mut audit);
    assert!(!audit.is_empty(), "no audit row names the dropped variable");

    // No value of the variable anywhere: responses, audit, logs, state.
    let all = a.seen.join("\n");
    assert!(!all.contains(SECRET_VALUE), "a response carries the value");
    let mut leaks = Vec::new();
    files_containing(a.dir.path(), SECRET_VALUE.as_bytes(), &mut leaks);
    assert!(leaks.is_empty(), "value written to {leaks:?}");
}
