// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// A harness that reconnects (for example after an upgrade replaced the adapter
// process) may send its first request to the new process without the
// `initialize` handshake. rmcp then treats the connection as a 2026-07-28
// stateless one, rejects the request for missing `_meta` fields and the
// adapter exits, so the harness shows "fetching tools failed" and Terminal
// Commander is unusable until a manual reconnect. The adapter must instead
// serve such a client as if it had initialized.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
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

/// Stop the daemon the adapter started for this test. The daemon records its
/// pid just after it starts listening, so wait briefly for the pidfile rather
/// than reading it once (a fast test could otherwise leave the daemon running).
fn stop_test_daemon(state_dir: &std::path::Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let pid = loop {
        if let Some(d) = terminal_commander_supervisor::pidfile::read_pidfile_raw(state_dir) {
            break Some(d.pid);
        }
        if std::time::Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let Some(pid) = pid else { return };
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(unix)]
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}

struct Adapter {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    state_dir: PathBuf,
    _dir: TempDir,
}

impl Adapter {
    /// Start the adapter WITHOUT any handshake.
    fn start() -> Self {
        let dir = TempDir::new().unwrap();
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
            r"\\.\pipe\tc-test-no-init-{}-{}",
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
        Self {
            child,
            stdin,
            lines,
            state_dir,
            _dir: dir,
        }
    }

    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Wait for the response with `id`; returns every line seen until then.
    fn response(&self, id: &Value) -> (Value, Vec<Value>) {
        let mut seen = Vec::new();
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_mins(1))
                .expect("no response from adapter within 60s (did it exit?)");
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg.get("id") == Some(id) {
                return (msg, seen);
            }
            seen.push(msg);
        }
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        stop_test_daemon(&self.state_dir);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tool_names(resp: &Value) -> Vec<String> {
    resp["result"]["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|t| t["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn tools_list_as_first_message_is_served() {
    let mut a = Adapter::start();
    a.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}));
    let (resp, extra) = a.response(&json!(1));
    assert!(
        resp.get("error").is_none(),
        "tools/list without initialize failed: {resp}"
    );
    let names = tool_names(&resp);
    assert!(
        names.iter().any(|n| n == "run_and_watch"),
        "tools: {names:?}"
    );
    assert!(
        extra.is_empty(),
        "the client must not see replies it never asked for: {extra:?}"
    );

    // The connection keeps working: a tool call right after.
    a.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                   "params": {"name": "health", "arguments": {}}}));
    let (resp, _) = a.response(&json!(2));
    assert!(
        resp.get("error").is_none(),
        "tools/call after implicit handshake: {resp}"
    );
    assert!(
        a.child.try_wait().unwrap().is_none(),
        "adapter must still be running"
    );
}

#[test]
fn initialized_notification_as_first_message_is_served() {
    let mut a = Adapter::start();
    a.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    a.send(&json!({"jsonrpc": "2.0", "id": "list", "method": "tools/list", "params": {}}));
    let (resp, _) = a.response(&json!("list"));
    assert!(resp.get("error").is_none(), "tools/list failed: {resp}");
    assert!(!tool_names(&resp).is_empty());
}

#[test]
fn ping_then_tools_list_is_served() {
    let mut a = Adapter::start();
    a.send(&json!({"jsonrpc": "2.0", "id": 10, "method": "ping"}));
    let (pong, _) = a.response(&json!(10));
    assert!(pong.get("error").is_none(), "ping: {pong}");
    a.send(&json!({"jsonrpc": "2.0", "id": 11, "method": "tools/list", "params": {}}));
    let (resp, _) = a.response(&json!(11));
    assert!(
        resp.get("error").is_none(),
        "tools/list after ping failed: {resp}"
    );
    assert!(!tool_names(&resp).is_empty());
}

/// CI probes and scripts pipe a request in and close stdin at once. The reply
/// must still reach stdout before the adapter exits: the bridge forwards
/// output from a separate task, which the adapter drains before exiting.
#[test]
fn piped_requests_then_eof_still_get_their_replies() {
    let dir = TempDir::new().unwrap();
    let state_dir = dir.path().join("state");
    #[cfg(unix)]
    let socket = dir.path().join("tcd.sock").display().to_string();
    #[cfg(windows)]
    let socket = format!(r"\\.\pipe\tc-test-no-init-eof-{}", std::process::id());
    let mut child = isolated_env::isolate(
        &mut Command::new(target_bin("terminal-commander-mcp")),
        dir.path(),
    )
    .arg("--state-dir")
    .arg(&state_dir)
    .env("TC_SOCKET", &socket)
    .env("TC_DATA", dir.path())
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn adapter");
    {
        let mut stdin = child.stdin.take().unwrap();
        // A 2026-07-28 stateless request (as the release verify probes send)...
        let discover = json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
                "io.modelcontextprotocol/clientInfo": {"name": "probe", "version": "0"}
            }
        }});
        writeln!(stdin, "{discover}").unwrap();
    } // stdin closed here
    let out = child.wait_with_output().expect("adapter output");
    stop_test_daemon(&state_dir);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("supportedVersions"),
        "the reply to a piped request must be written before exit; stdout: {stdout}"
    );
    assert!(out.status.success(), "adapter exit: {:?}", out.status);
}
