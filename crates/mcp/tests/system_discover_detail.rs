// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// The model is told to call system_discover first, so every session pays for
// its reply. The default reply is a summary of what is needed to choose how to
// run a command; `detail: "full"` returns the whole payload. These tests drive
// the real adapter and daemon binaries over stdio.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;
use terminal_commander_mcp::tools::tool_catalogue;

#[path = "../../test_support/isolated_env.rs"]
mod isolated_env;

/// The summary measured 9.8k characters on a Windows host with four shells,
/// WSL, and 21 probed tools (the full reply was 33.2k). The ceiling leaves
/// room for hosts with longer paths and more routes.
const SUMMARY_BUDGET_CHARS: usize = 16_000;

fn target_bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe.parent().and_then(|p| p.parent()).expect("profile dir");
    let mut bin = profile_dir.join(name);
    if cfg!(windows) {
        bin.set_extension("exe");
    }
    bin
}

fn stop_test_daemon(state_dir: &Path) {
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
    next_id: u64,
    state_dir: PathBuf,
    _dir: TempDir,
}

impl Adapter {
    fn start() -> Self {
        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join("state");
        assert!(
            target_bin("terminal-commanderd").exists()
                && target_bin("terminal-commander-mcp").exists(),
            "adapter/daemon binaries missing; run `cargo build` first"
        );
        #[cfg(unix)]
        let socket = dir.path().join("tcd.sock").display().to_string();
        #[cfg(windows)]
        let socket = format!(
            r"\\.\pipe\tc-test-discover-detail-{}-{}",
            std::process::id(),
            dir.path().file_name().unwrap().to_string_lossy()
        );
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
        let mut adapter = Self {
            child,
            stdin,
            lines,
            next_id: 0,
            state_dir,
            _dir: dir,
        };
        let init = adapter.request(
            "initialize",
            &json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "discover-detail-test", "version": "0"}}),
        );
        assert!(init.get("result").is_some(), "initialize failed: {init}");
        writeln!(
            adapter.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .unwrap();
        adapter
    }

    fn request(&mut self, method: &str, params: &Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        self.stdin.flush().unwrap();
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

    /// Call `system_discover` and return the reply text.
    fn discover(&mut self, arguments: &Value) -> String {
        let resp = self.request(
            "tools/call",
            &json!({"name": "system_discover", "arguments": arguments}),
        );
        resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("system_discover failed: {resp}"))
            .to_owned()
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        stop_test_daemon(&self.state_dir);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn unavailable_in_full(full: &Value) -> BTreeSet<String> {
    full["tools"]
        .as_array()
        .expect("full reply carries the tool catalogue")
        .iter()
        .filter(|tool| tool["available"] == json!(false))
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect()
}

fn unavailable_in_summary(summary: &Value) -> BTreeSet<String> {
    summary["tool_catalogue"]["unavailable"]
        .as_array()
        .expect("summary lists unavailable tools")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_default_reply_is_a_summary_and_full_is_the_whole_payload() {
    let mut adapter = Adapter::start();
    let default_text = adapter.discover(&json!({}));
    let summary: Value = serde_json::from_str(&default_text).unwrap();
    assert_eq!(summary["detail"], "summary", "{default_text}");
    assert!(
        default_text.len() <= SUMMARY_BUDGET_CHARS,
        "summary is {} chars, budget {SUMMARY_BUDGET_CHARS}",
        default_text.len()
    );
    assert!(
        summary["note"]
            .as_str()
            .unwrap()
            .contains("\"detail\":\"full\""),
        "the summary says how to get the rest"
    );
    let explicit: Value =
        serde_json::from_str(&adapter.discover(&json!({"detail": "summary"}))).unwrap();
    assert_eq!(explicit["detail"], "summary");

    let full: Value = serde_json::from_str(&adapter.discover(&json!({"detail": "full"}))).unwrap();
    let keys: BTreeSet<&str> = full
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "adapter_version",
            "daemon",
            "daemon_available",
            "daemon_error",
            "mcp_spec",
            "omni_status",
            "tools",
        ]),
        "detail:full is the whole payload"
    );
    assert_eq!(
        full["tools"].as_array().unwrap().len(),
        tool_catalogue().len()
    );
    assert_eq!(summary["tool_catalogue"]["count"], tool_catalogue().len());
    assert_eq!(unavailable_in_summary(&summary), unavailable_in_full(&full));

    assert_eq!(full["daemon_available"], true, "the test daemon must be up");
    let methods = full["daemon"]["methods"].as_array().unwrap().len();
    assert_eq!(summary["daemon"]["method_count"], methods);
    let environment = &full["daemon"]["environment"];
    let summary_environment = &summary["daemon"]["environment"];
    for field in ["os", "arch", "terminal", "shells", "wsl", "discovery_ms"] {
        assert_eq!(summary_environment[field], environment[field], "{field}");
    }
    // The full reply was asked for later, so its discovery is as old or older.
    assert!(
        summary_environment["discovery_age_ms"].as_u64().unwrap()
            <= environment["discovery_age_ms"].as_u64().unwrap()
    );
    let routes = environment["access_routes"].as_array().unwrap();
    let argv_tools: Vec<&str> = routes
        .iter()
        .filter(|route| route["kind"] == "direct_argv")
        .map(|route| {
            route["route_id"]
                .as_str()
                .unwrap()
                .trim_start_matches("argv:")
        })
        .collect();
    let other_routes: Vec<&Value> = routes
        .iter()
        .filter(|route| route["kind"] != "direct_argv")
        .collect();
    assert_eq!(summary_environment["argv_tools"], json!(argv_tools));
    assert_eq!(summary_environment["routes"], json!(other_routes));
    let tool_names = |tools: &Value| -> Vec<String> {
        tools
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        tool_names(&summary_environment["tools"]),
        tool_names(&environment["tools"]),
        "every probed tool is listed, available or not"
    );
}

#[test]
fn the_advertised_schema_carries_detail() {
    let mut adapter = Adapter::start();
    let list = adapter.request("tools/list", &json!({}));
    let tool = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "system_discover")
        .expect("system_discover is listed")
        .clone();
    let schema = tool["inputSchema"].to_string();
    assert!(
        tool["inputSchema"]["properties"]["detail"].is_object(),
        "{schema}"
    );
    assert!(
        schema.contains("\"summary\"") && schema.contains("\"full\""),
        "{schema}"
    );
}
