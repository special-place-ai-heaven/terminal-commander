// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Integration tests for `terminal-commander code {run, stubs, idioms}` and
//! `terminal-commander q <op> [-a k=v]`.
//!
//! Covers:
//! 1. Offline `code stubs` (all 60 + filtered `TypedDict`), `code idioms`, and
//!    `code run --help` Windows `.cmd` newline guidance (exit 0 without daemon).
//! 2. Offline honesty: `code run` and `q health` against an unreachable endpoint
//!    exit 69 (`EX_UNAVAILABLE`) with empty stdout and diagnostic stderr.
//! 3. Live daemon e2e: `code run -` (multiline Python on stdin) + `q health` +
//!    `q runtime_state -a limit=0` emit compact JSON on stdout and exit 0, and
//!    script/op errors exit 1 cleanly.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use terminal_commander_ipc::{DaemonClient, IpcRequest, IpcResponse};
use terminal_commander_supervisor::pidfile::{pidfile_path, read_pidfile_raw};

#[path = "../../test_support/isolated_env.rs"]
mod isolated_env;

const UNAVAILABLE_EXIT: i32 = 69;

fn offline_cli() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_terminal-commander"));
    cmd.env_remove("TC_SESSION")
        .env_remove("TC_DATA")
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .env(
            "TC_SOCKET",
            if cfg!(windows) {
                r"\\.\pipe\terminal-commander-code-cli-offline-no-daemon"
            } else {
                "/nonexistent/terminal-commander-code-cli-offline.sock"
            },
        );
    cmd
}

fn target_bin(name: &str) -> PathBuf {
    // Derive sibling `<profile>/<name>[.exe]` from `CARGO_BIN_EXE_terminal-commander`
    // (which always lives directly in `<target>/<profile>/`), so custom target layouts
    // that place test binaries under `build/.../out/` still locate `terminal-commanderd`.
    let cli_exe = PathBuf::from(env!("CARGO_BIN_EXE_terminal-commander"));
    let profile_dir = cli_exe.parent().expect("cli binary parent profile dir");
    let mut bin = profile_dir.join(name);
    if cfg!(windows) {
        bin.set_extension("exe");
    }
    bin
}

fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-cli-code-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn wait_for_daemon_ready(endpoint: &str, timeout: Duration) -> bool {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("readiness runtime");
    let client = DaemonClient::new(endpoint).with_timeout(Duration::from_millis(250));
    let deadline = Instant::now() + timeout;
    let mut correlation_id = 1;

    loop {
        if matches!(
            runtime.block_on(client.call(correlation_id, IpcRequest::Health)),
            Ok(IpcResponse::Health { .. })
        ) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        correlation_id += 1;
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct LiveDaemon {
    child: std::process::Child,
    base: PathBuf,
    token: String,
}

impl LiveDaemon {
    fn spawn(tag: &str) -> Self {
        let base = tmp_data_dir(tag);
        let token = format!(
            "c{:x}{:04x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos())
                & 0xffff
        );
        let state_dir = base.join(&token);

        let daemon_bin = target_bin("terminal-commanderd");
        assert!(
            daemon_bin.exists(),
            "daemon binary not found at {}; cargo dev-dep should have built it",
            daemon_bin.display()
        );
        let child = isolated_env::isolate(&mut Command::new(&daemon_bin), &base)
            .args(["start", "--mode", "ipc-server"])
            .env("TC_DATA", &base)
            .env("TC_SESSION", &token)
            .env_remove("TC_SOCKET")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn daemon");

        let pid_path = pidfile_path(&state_dir);
        let bind_deadline = Instant::now() + Duration::from_secs(5);
        while !pid_path.exists() && Instant::now() < bind_deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        if !pid_path.exists() {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&base);
            panic!(
                "daemon never bound: pidfile missing at {}",
                pid_path.display()
            );
        }
        let endpoint = read_pidfile_raw(&state_dir)
            .expect("pidfile present after bind")
            .endpoint;
        if !wait_for_daemon_ready(&endpoint, Duration::from_secs(5)) {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&base);
            panic!("daemon endpoint never became responsive: {endpoint}");
        }

        Self { child, base, token }
    }

    fn run_cli(&self, args: &[&str], stdin_bytes: Option<&[u8]>) -> std::process::Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_terminal-commander"));
        isolated_env::isolate(&mut cmd, &self.base)
            .args(args)
            .env("TC_DATA", &self.base)
            .env("TC_SESSION", &self.token)
            .env_remove("TC_SOCKET")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(input) = stdin_bytes {
            cmd.stdin(Stdio::piped());
            let mut child = cmd.spawn().expect("spawn cli with stdin");
            child
                .stdin
                .as_mut()
                .expect("stdin pipe")
                .write_all(input)
                .expect("write stdin");
            child.wait_with_output().expect("wait cli")
        } else {
            cmd.stdin(Stdio::null());
            cmd.output().expect("run cli")
        }
    }
}

impl Drop for LiveDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[test]
fn code_stubs_idioms_and_help_work_offline() {
    let stubs = offline_cli()
        .args(["code", "stubs"])
        .output()
        .expect("run code stubs");
    assert_eq!(stubs.status.code(), Some(0));
    let stubs_out = String::from_utf8_lossy(&stubs.stdout);
    assert!(stubs_out.contains("def run_and_watch(*,"));
    assert!(stubs_out.contains("def health() ->"));

    let filtered = offline_cli()
        .args(["code", "stubs", "run_and_watch", "--limit", "1"])
        .output()
        .expect("run code stubs filtered");
    assert_eq!(filtered.status.code(), Some(0));
    let filtered_out = String::from_utf8_lossy(&filtered.stdout);
    assert!(filtered_out.contains("from typing import TypedDict"));
    assert!(filtered_out.contains("def run_and_watch("));

    let idioms = offline_cli()
        .args(["code", "idioms"])
        .output()
        .expect("run code idioms");
    assert_eq!(idioms.status.code(), Some(0));
    let idioms_out = String::from_utf8_lossy(&idioms.stdout);
    assert!(idioms_out.contains("Keyword arguments ONLY"));
    assert!(idioms_out.contains("Registered functions (60)"));

    let help = offline_cli()
        .args(["code", "run", "--help"])
        .output()
        .expect("run code run --help");
    assert_eq!(help.status.code(), Some(0));
    let help_out = String::from_utf8_lossy(&help.stdout);
    assert!(
        help_out.contains(".cmd") && help_out.contains("code run -"),
        "help must document Windows .cmd caution and `code run -`: {help_out}"
    );
}

#[test]
fn code_run_and_q_exit_69_honestly_when_daemon_offline() {
    let code_out = offline_cli()
        .args(["code", "run", "-c", "health()"])
        .output()
        .expect("run offline code run");
    assert_eq!(
        code_out.status.code(),
        Some(UNAVAILABLE_EXIT),
        "offline code run must exit 69, stderr={}",
        String::from_utf8_lossy(&code_out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&code_out.stdout).trim(), "");
    assert!(String::from_utf8_lossy(&code_out.stderr).contains("unavailable"));

    let q_out = offline_cli()
        .args(["q", "health"])
        .output()
        .expect("run offline q health");
    assert_eq!(
        q_out.status.code(),
        Some(UNAVAILABLE_EXIT),
        "offline q health must exit 69, stderr={}",
        String::from_utf8_lossy(&q_out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&q_out.stdout).trim(), "");
    assert!(String::from_utf8_lossy(&q_out.stderr).contains("unavailable"));
}

#[test]
fn live_daemon_code_run_stdin_and_q_emit_json() {
    let daemon = LiveDaemon::spawn("live");

    // 1. `q health` prints pure compact JSON and exits 0.
    let q_health = daemon.run_cli(&["q", "health"], None);
    assert_eq!(
        q_health.status.code(),
        Some(0),
        "q health failed: stderr={}",
        String::from_utf8_lossy(&q_health.stderr)
    );
    let health_json: Value =
        serde_json::from_slice(&q_health.stdout).expect("q health stdout is pure JSON");
    assert_eq!(health_json["ok"], true);
    assert!(health_json["uptime_secs"].is_number());

    // 2. `query runtime_state -a limit=0` parses `-a k=v` and exits 0.
    let q_rt = daemon.run_cli(&["query", "runtime_state", "-a", "limit=0"], None);
    assert_eq!(
        q_rt.status.code(),
        Some(0),
        "query runtime_state failed: stderr={}",
        String::from_utf8_lossy(&q_rt.stderr)
    );
    let rt_json: Value =
        serde_json::from_slice(&q_rt.stdout).expect("query runtime_state stdout is pure JSON");
    assert!(rt_json["command_jobs"].is_number());
    assert_eq!(rt_json["probes"].as_array().map(Vec::len), Some(0));

    // 3. `code run -` executes multiline Python on stdin against live daemon.
    let argv_json = if cfg!(windows) {
        r#"["cmd", "/C", "echo", "CLI_CODE_SIGNAL"]"#
    } else {
        r#"["echo", "CLI_CODE_SIGNAL"]"#
    };
    let script = format!(
        "h = health()\nr = run_and_watch(argv={argv_json}, rules=[{{\"pattern\": \"CLI_CODE_SIGNAL\"}}], wait_ms=10000)\n{{\"ok\": h[\"ok\"], \"exit_code\": r[\"exit_code\"], \"signals\": r[\"signal_count\"]}}\n"
    );
    let run_out = daemon.run_cli(&["code", "run", "-"], Some(script.as_bytes()));
    assert_eq!(
        run_out.status.code(),
        Some(0),
        "code run - failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&run_out.stdout),
        String::from_utf8_lossy(&run_out.stderr)
    );
    let outcome: Value =
        serde_json::from_slice(&run_out.stdout).expect("code run - stdout is pure JSON");
    assert_eq!(outcome["status"], "ok");
    assert_eq!(outcome["calls"], 2);
    assert_eq!(outcome["mutations"], 1);
    assert_eq!(outcome["value"]["ok"], true);
    assert_eq!(outcome["value"]["exit_code"], 0);
    assert!(outcome["value"]["signals"].as_u64().unwrap_or(0) >= 1);

    // 4. Python script runtime error exits 1 while printing ExecuteOutcome JSON.
    let err_out = daemon.run_cli(&["code", "run", "-c", "1 / 0"], None);
    assert_eq!(err_out.status.code(), Some(1));
    let err_json: Value =
        serde_json::from_slice(&err_out.stdout).expect("error outcome is still JSON");
    assert_eq!(err_json["status"], "error");
    assert_eq!(err_json["error"]["exception_type"], "ZeroDivisionError");

    // 5. Unknown op in `q` exits 1.
    let bad_op = daemon.run_cli(&["q", "not_a_real_op"], None);
    assert_eq!(bad_op.status.code(), Some(1));
}
