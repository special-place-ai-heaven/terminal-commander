// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// When the daemon an adapter was using goes away mid-session (its idle
// shutdown, a crash, a replace), the NEXT tool call must restart it and
// succeed. Before the fix the adapter restarted the daemon but still failed
// that first call with `daemon_unavailable`: on Windows the pipe connect loop
// ran to the call deadline and was reported as a timeout, and a timeout is
// never re-sent. A request that never connected never reached the daemon, so
// re-sending it after recovery is safe even when it has side effects; the
// command test asserts the command ran exactly once.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

#[path = "../../test_support/isolated_env.rs"]
mod isolated_env;

fn target_bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe.parent().and_then(|p| p.parent()).expect("profile dir");
    let mut bin = profile_dir.join(name);
    if cfg!(windows) {
        bin.set_extension("exe");
    }
    bin
}

fn kill_pid(pid: u32) {
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(unix)]
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}

fn daemon_pid(state_dir: &Path) -> Option<u32> {
    terminal_commander_supervisor::pidfile::read_pidfile_raw(state_dir).map(|d| d.pid)
}

/// One adapter process driven over newline-delimited JSON-RPC on stdio.
struct Adapter {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
    state_dir: PathBuf,
    dir: TempDir,
}

impl Adapter {
    fn start() -> Self {
        Self::start_in(TempDir::new().unwrap())
    }

    /// Start with `dir` as the adapter's home, data dir and socket dir.
    fn start_in(dir: TempDir) -> Self {
        let state_dir = dir.path().join("state");
        let mcp_bin = target_bin("terminal-commander-mcp");
        assert!(
            target_bin("terminal-commanderd").exists() && mcp_bin.exists(),
            "adapter/daemon binaries missing; run `cargo build` first"
        );
        #[cfg(unix)]
        let socket = dir.path().join("tcd.sock").display().to_string();
        #[cfg(windows)]
        let socket = format!(
            r"\\.\pipe\tc-test-first-call-{}-{}",
            std::process::id(),
            dir.path().file_name().unwrap().to_string_lossy()
        );
        let mut child = isolated_env::isolate(&mut Command::new(&mcp_bin), dir.path())
            .arg("--state-dir")
            .arg(&state_dir)
            .env("TC_SOCKET", &socket)
            .env("TC_DATA", dir.path())
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
        let mut a = Self {
            child,
            stdin,
            lines,
            next_id: 0,
            state_dir,
            dir,
        };
        let init = a.request(
            "initialize",
            &json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "first-call-test", "version": "0"}}),
        );
        assert!(init.get("result").is_some(), "initialize failed: {init}");
        a.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        a
    }

    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: &Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_mins(1))
                .expect("no response from adapter within 60s");
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg.get("id") == Some(&json!(id)) {
                return msg;
            }
        }
    }

    fn call_tool(&mut self, name: &str, args: &Value) -> Value {
        self.request("tools/call", &json!({"name": name, "arguments": args}))
    }

    /// Stop the daemon the way an idle shutdown or crash would: out from
    /// under the adapter, with no notice. Returns the stopped pid.
    fn lose_daemon(&self) -> u32 {
        let pid = daemon_pid(&self.state_dir).expect("adapter should have started a daemon");
        kill_pid(pid);
        std::thread::sleep(Duration::from_millis(1500));
        pid
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        if let Some(pid) = daemon_pid(&self.state_dir) {
            kill_pid(pid);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn is_success(resp: &Value) -> bool {
    resp.get("error").is_none() && resp["result"]["isError"] != json!(true)
}

#[test]
fn read_only_call_right_after_daemon_loss_succeeds() {
    let mut a = Adapter::start();
    let first = a.call_tool("health", &json!({}));
    assert!(is_success(&first), "health before loss: {first}");

    let old = a.lose_daemon();
    let after = a.call_tool("health", &json!({}));
    assert!(
        is_success(&after),
        "first call after the daemon went away must restart it and succeed, got: {after}"
    );
    // The restarted daemon records its pid just after it starts listening, so
    // the pidfile can briefly lag the successful call under load.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut new = daemon_pid(&a.state_dir);
    while new == Some(old) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        new = daemon_pid(&a.state_dir);
    }
    let new = new.expect("a daemon should be running again");
    assert_ne!(new, old, "the daemon should have been restarted");
}

#[test]
fn command_right_after_daemon_loss_runs_exactly_once() {
    let mut a = Adapter::start();
    let first = a.call_tool("health", &json!({}));
    assert!(is_success(&first), "health before loss: {first}");

    let marker = a.dir.path().join("runs.txt");
    #[cfg(windows)]
    let argv = json!(["cmd", "/c", format!("echo run>>{}", marker.display())]);
    #[cfg(unix)]
    let argv = json!(["sh", "-c", format!("echo run >> '{}'", marker.display())]);

    a.lose_daemon();
    let after = a.call_tool(
        "run_and_watch",
        &json!({"argv": argv, "wait_ms": 20000, "wait_until": "exit"}),
    );
    assert!(
        is_success(&after),
        "first command after the daemon went away must restart it and run, got: {after}"
    );
    let runs = std::fs::read_to_string(&marker).unwrap_or_default();
    assert_eq!(
        runs.lines().filter(|l| l.trim() == "run").count(),
        1,
        "the command must run exactly once, marker file: {runs:?}"
    );
}

/// What an installed autostart snippet amounts to, written the way the fixed
/// autostart behaves: anything that sources the profile runs it, it would act
/// on whatever endpoint it inherited, and it does nothing inside a TC-managed
/// process tree (`autostart-ran` stands for the daemon it would start).
#[cfg(unix)]
const HOSTILE_PROFILE: &str = r#"echo "$$" >> "$HOME/profile-sourced"
if [ -n "${TC_DATA-}${TC_SOCKET-}" ]; then
  echo "TC_DATA=${TC_DATA-} TC_SOCKET=${TC_SOCKET-}" >> "$HOME/endpoint-leaked"
fi
if [ -z "${TC_DAEMON_CHILD-}" ]; then
  echo "$$" >> "$HOME/autostart-ran"
fi
"#;

/// The installed Linux autostart lives in the user's profile and starts a
/// daemon on the `TC_DATA`/`TC_SOCKET` it inherits. The daemon's discovery
/// probes used to run login shells carrying the session's endpoint, so every
/// daemon start -- the first and the one that replaces a lost daemon -- put a
/// second daemon on the session's socket. A daemon must never source the
/// profile on its own; a login shell a model runs does, and must find neither
/// the endpoint nor a reason to start a daemon.
#[cfg(unix)]
#[test]
fn a_daemon_loss_and_recovery_never_runs_the_users_login_profile() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".profile"), HOSTILE_PROFILE).unwrap();
    let mut a = Adapter::start_in(dir);
    // `system_discover` waits for the discovery probes, so they have all run.
    let first = a.call_tool("system_discover", &json!({}));
    assert!(is_success(&first), "system_discover before loss: {first}");
    a.lose_daemon();
    let after = a.call_tool("system_discover", &json!({}));
    assert!(is_success(&after), "system_discover after loss: {after}");

    let home = a.dir.path().to_path_buf();
    let read = |name: &str| std::fs::read_to_string(home.join(name)).unwrap_or_default();
    let sourced = read("profile-sourced");
    assert!(
        sourced.is_empty(),
        "the daemon ran a login shell (pids {sourced:?}); it must not source the user's profile"
    );

    let login = a.call_tool(
        "run_and_watch",
        &json!({"argv": ["sh", "-lc", "true"], "wait_ms": 20000, "wait_until": "exit"}),
    );
    assert!(is_success(&login), "run_and_watch sh -lc: {login}");
    assert_eq!(
        read("profile-sourced").lines().count(),
        1,
        "the model's login shell should have sourced the profile"
    );
    let leaked = read("endpoint-leaked");
    assert!(
        leaked.is_empty(),
        "a login shell under the daemon saw its endpoint: {leaked}"
    );
    let autostart = read("autostart-ran");
    assert!(
        autostart.is_empty(),
        "an autostart that honours TC_DAEMON_CHILD would have started a daemon"
    );
}

/// The daemon is started with `TC_SOCKET` (and here `TC_DATA`) naming its own
/// endpoint. A command it runs, in either lane, must not inherit them -- a
/// terminal-commander process that command starts would attach to this
/// daemon's socket, or start a second daemon on it -- and must carry
/// `TC_DAEMON_CHILD=1`.
#[test]
fn commands_carry_the_child_marker_and_not_the_daemons_endpoint() {
    let mut a = Adapter::start();
    for tool in ["run_and_watch", "pty_command_start"] {
        let out = a.dir.path().join(format!("{tool}.env"));
        #[cfg(windows)]
        let argv = json!(["cmd", "/c", format!("set>{}", out.display())]);
        #[cfg(unix)]
        let argv = json!([
            "sh",
            "-c",
            format!("env > '{p}.tmp' && mv '{p}.tmp' '{p}'", p = out.display())
        ]);
        let params = if tool == "run_and_watch" {
            json!({"argv": argv, "wait_ms": 20000, "wait_until": "exit"})
        } else {
            json!({"argv": argv})
        };
        let ran = a.call_tool(tool, &params);
        assert!(is_success(&ran), "{tool}: {ran}");

        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let env = loop {
            let env = std::fs::read_to_string(&out).unwrap_or_default();
            if env
                .lines()
                .any(|l| l.to_ascii_uppercase().starts_with("PATH="))
                || std::time::Instant::now() >= deadline
            {
                break env;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        assert!(
            env.lines()
                .any(|l| l.to_ascii_uppercase().starts_with("PATH=")),
            "{tool}: the command must still inherit the rest of the environment: {env:?}"
        );
        let leaked: Vec<&str> = env
            .lines()
            .filter(|l| l.starts_with("TC_SOCKET=") || l.starts_with("TC_DATA="))
            .collect();
        assert!(
            leaked.is_empty(),
            "{tool}: the command inherited {leaked:?}"
        );
        assert!(
            env.lines().any(|l| l.trim_end() == "TC_DAEMON_CHILD=1"),
            "{tool}: the command must carry TC_DAEMON_CHILD=1"
        );
    }
}
