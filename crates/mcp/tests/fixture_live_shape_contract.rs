// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Live-shape contract for the per-tool MCP fixtures.
//!
//! `fixture_catalogue_contract` only checks names, counts and the fixture
//! map. This test closes the remaining drift class: for every tool that can
//! be driven deterministically and harmlessly it calls the REAL tool against
//! one isolated live daemon and requires the response's KEY SET (recursive;
//! for arrays the first element when both sides are non-empty) to equal the
//! key set of the fixture's success example. Values are never compared.
//!
//! Every tool in `mcp-tool-fixture-map.v1.json` must be either driven here
//! or listed in `SKIPPED` with its reason, so a new tool without coverage
//! fails this test. Optional keys need a per-call allow-list entry with a
//! reason; an unused allow-list entry also fails (it would hide nothing).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ClientServiceExt, ServiceExt};
use serde_json::{Value, json};

use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
#[cfg(unix)]
use terminal_commanderd::IpcServer;
#[cfg(windows)]
use terminal_commanderd::PipeServer;
use terminal_commanderd::{DaemonConfig, DaemonState};

/// `system_discover` keys whose presence depends on the machine (and its
/// load) running the test: the terminal name comes from `TERM`, the WSL block
/// exists only on Windows, and a version string is absent when its bounded
/// probe timed out or failed (`ProgramProbe.version` is `None` then).
/// `stale_confirmed_age_ms` appears only when a probe timed out and an earlier
/// discovery (in memory or `host-discovery.json`) had confirmed it.
const DISCOVER_HOST_DEPENDENT: &[&str] = &[
    "daemon.environment.terminal.name",
    "daemon.environment.wsl.default_shell",
    "daemon.environment.wsl.distributions",
    "daemon.environment.wsl.version",
    "daemon.environment.beachhead.version",
    "daemon.environment.routes[].version",
    "daemon.environment.shells[].version",
    "daemon.environment.tools[].version",
    "daemon.environment.shells[].stale_confirmed_age_ms",
    "daemon.environment.tools[].stale_confirmed_age_ms",
    "daemon.environment.wsl.stale_confirmed_age_ms",
];

/// Governor keys whose presence depends on the host: `peak_memory_bytes`
/// exists only where the enforcement mechanism can measure it (Job Object,
/// cgroup `memory.peak`, not rlimit).
const GOVERNOR_HOST_DEPENDENT: &[&str] = &["peak_memory_bytes"];

/// Tools deliberately not driven by this test. Each entry carries its reason.
fn skipped() -> Vec<(&'static str, &'static str)> {
    #[allow(unused_mut)] // only the Windows build pushes more entries
    let mut skips = vec![
        (
            "credential_request",
            "needs a PTY job blocked on a real password prompt; not deterministic or harmless to provoke",
        ),
        (
            "target_probe",
            "needs a registered remote target in the adapter-side targets.toml",
        ),
    ];
    #[cfg(windows)]
    for tool in [
        "shell_session_start",
        "shell_session_exec",
        "shell_session_status",
        "shell_session_list",
        "shell_session_stop",
        "workspace_snapshot_create",
        "workspace_snapshot_apply",
    ] {
        skips.push((
            tool,
            "persistent shell sessions and workspace snapshots are unix-only (unsupported_platform on Windows)",
        ));
    }
    skips
}

/// Harmless argv that prints `text` and exits, on every platform.
fn echo_argv(text: &str) -> Vec<String> {
    if cfg!(windows) {
        ["cmd", "/C", "echo", text].map(str::to_owned).to_vec()
    } else {
        ["echo", text].map(str::to_owned).to_vec()
    }
}

/// Harmless argv that prints 40 numbered lines and exits.
fn many_lines_argv() -> Vec<String> {
    if cfg!(windows) {
        ["cmd", "/C", "for /L %i in (1,1,40) do @echo line%i"]
            .map(str::to_owned)
            .to_vec()
    } else {
        ["seq", "1", "40"].map(str::to_owned).to_vec()
    }
}

/// Harmless argv that prints one line, then stays alive ~30 s unless stopped.
fn sleeper_argv() -> Vec<String> {
    if cfg!(windows) {
        ["ping", "-n", "30", "127.0.0.1"]
            .map(str::to_owned)
            .to_vec()
    } else {
        ["sh", "-c", "echo up; exec sleep 30"]
            .map(str::to_owned)
            .to_vec()
    }
}

/// Harmless interactive program for the PTY tools (echoes its stdin).
fn pty_argv() -> Vec<String> {
    if cfg!(windows) {
        ["cmd", "/Q"].map(str::to_owned).to_vec()
    } else {
        vec!["cat".to_owned()]
    }
}

/// Start an in-process daemon on an isolated endpoint (UDS on unix, a
/// uniquely named pipe on Windows) and return the client endpoint plus the
/// server handle that must stay alive for the test.
#[cfg(unix)]
fn start_daemon(state: Arc<DaemonState>) -> (PathBuf, impl Sized) {
    let socket = state.config.socket_path();
    let handle = IpcServer::new(state, socket)
        .spawn()
        .expect("ipc server spawn");
    (handle.socket_path().to_path_buf(), handle)
}

#[cfg(windows)]
fn start_daemon(state: Arc<DaemonState>) -> (PathBuf, impl Sized) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let pipe = format!(r"\\.\pipe\tc-fixture-shape-{}-{nanos}", std::process::id());
    let handle = PipeServer::new(state, pipe.clone())
        .spawn()
        .expect("pipe server spawn");
    (PathBuf::from(pipe), handle)
}

#[derive(Default, Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

type Client = rmcp::service::RunningService<rmcp::RoleClient, TestClient>;

fn tmp_data_dir() -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-fixture-shape-{}-{nanos}", std::process::id()));
    p
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn read_json(path: &Path) -> Value {
    let raw =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Keys whose value is a map with caller-defined keys (rule capture names) or
/// a variant-tagged enum object; their children are data, not shape, so only
/// the key's own presence is compared.
const OPAQUE_KEYS: &[&str] = &["captures", "liveness"];

/// Key-set differences between a fixture example and a live payload, as
/// `(side, path)` with side `-` (fixture only) or `+` (live only). Objects
/// compare keys and recurse into shared keys that are objects on both sides;
/// arrays recurse into the first element when both are non-empty.
fn diff_keys(path: &str, expected: &Value, actual: &Value, out: &mut Vec<(char, String)>) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for k in e.keys().filter(|k| !a.contains_key(*k)) {
                out.push(('-', format!("{path}.{k}")));
            }
            for k in a.keys().filter(|k| !e.contains_key(*k)) {
                out.push(('+', format!("{path}.{k}")));
            }
            for (k, ev) in e {
                if OPAQUE_KEYS.contains(&k.as_str()) {
                    continue;
                }
                if let Some(av) = a.get(k) {
                    diff_keys(&format!("{path}.{k}"), ev, av, out);
                }
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            if let (Some(ef), Some(af)) = (e.first(), a.first()) {
                diff_keys(&format!("{path}[]"), ef, af, out);
            }
        }
        _ => {}
    }
}

struct Harness {
    client: Client,
    fixtures: PathBuf,
    covered: BTreeSet<String>,
    failures: Vec<String>,
}

impl Harness {
    async fn call(&self, tool: &'static str, args: Value) -> Value {
        let mut params = CallToolRequestParams::new(tool);
        if let Value::Object(m) = args {
            params.arguments = Some(m);
        }
        let result = self
            .client
            .call_tool(params)
            .await
            .unwrap_or_else(|e| panic!("call_tool({tool}) failed: {e}"));
        let text = result
            .content
            .iter()
            .find_map(|c| c.as_text().map(|t| t.text.clone()))
            .unwrap_or_else(|| panic!("{tool}: no text content: {result:?}"));
        assert!(
            result.is_error != Some(true),
            "{tool}: tool returned an error result (the scenario is wrong): {text}"
        );
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{tool}: payload not JSON: {e}: {text}"))
    }

    /// Poll `command_status` until the job has left `running` (bounded).
    async fn wait_finished(&self, job: &str) {
        for _ in 0..100 {
            let st = self.call("command_status", json!({"job_id": job})).await;
            if st["state"] != "running" {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("job {job} still running after 10 s");
    }

    /// Call `tool`, compare its key set with the fixture example at JSON
    /// pointer `example`, and record any unexplained difference. Returns the
    /// live payload so the scenario can thread ids through.
    async fn check(
        &mut self,
        tool: &'static str,
        args: Value,
        example: &str,
        allow: &[(&str, &str)],
    ) -> Value {
        self.check_host(tool, args, example, allow, &[]).await
    }

    /// Like [`Self::check`], plus `host_dependent` key paths whose presence
    /// depends on the machine running the test (terminal name from `TERM`,
    /// the WSL block on Windows). They are tolerated in either direction and
    /// never reported stale, because absence is as legitimate as presence.
    async fn check_host(
        &mut self,
        tool: &'static str,
        args: Value,
        example: &str,
        allow: &[(&str, &str)],
        host_dependent: &[&str],
    ) -> Value {
        let actual = self.call(tool, args).await;
        let fixture = read_json(&self.fixtures.join(format!("mcp-tools/{tool}.v1.json")));
        let expected = fixture
            .pointer(example)
            .unwrap_or_else(|| panic!("{tool}: fixture has no example at {example}"));
        let mut diffs = Vec::new();
        diff_keys("$", expected, &actual, &mut diffs);
        let host: BTreeSet<String> = host_dependent.iter().map(|p| format!("$.{p}")).collect();
        diffs.retain(|(_, p)| !host.contains(p));
        let allowed: BTreeSet<String> = allow.iter().map(|(p, _)| format!("$.{p}")).collect();
        for (p, reason) in allow {
            assert!(
                !reason.is_empty(),
                "{tool}: allow-list entry {p} needs a reason"
            );
        }
        let unexplained: Vec<String> = diffs
            .iter()
            .filter(|(_, p)| !allowed.contains(p))
            .map(|(side, p)| format!("{side}{p}"))
            .collect();
        let unused: Vec<&String> = allowed
            .iter()
            .filter(|a| !diffs.iter().any(|(_, p)| p == *a))
            .collect();
        if !unexplained.is_empty() || !unused.is_empty() {
            self.failures.push(format!(
                "{tool} ({example}): key-set mismatch\n  unexplained (- fixture only, + live only): {unexplained:?}\n  stale allow-list entries: {unused:?}\n  fixture keys: {}\n  live payload: {actual}",
                serde_json::to_string(expected).unwrap_or_default()
            ));
        }
        self.covered.insert(tool.to_owned());
        actual
    }
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str()
        .unwrap_or_else(|| panic!("missing string {k} in {v}"))
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fixtures_match_live_tool_responses() {
    let data = tmp_data_dir();
    let state = Arc::new(
        DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).expect("daemon bootstrap"),
    );
    let (endpoint, _daemon) = start_daemon(Arc::clone(&state));

    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let daemon = McpDaemonClient::new(endpoint).with_timeout(Duration::from_secs(10));
    let server = TerminalCommanderMcpServer::new(daemon);
    let server_task =
        tokio::spawn(async move { server.serve(server_transport).await.expect("server serve") });
    let client = TestClient
        .serve_with_lifecycle(
            client_transport,
            rmcp::ClientLifecycleMode::Discover {
                preferred_versions: vec![rmcp::model::ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("client serve");
    let _server = server_task.await.expect("server task join");

    let fixtures = workspace_root().join("tests/fixtures/contracts");
    let mut h = Harness {
        client,
        fixtures: fixtures.clone(),
        covered: BTreeSet::new(),
        failures: Vec::new(),
    };

    scenario(&mut h, &data).await;

    // Coverage is driven by the fixture map: every live tool is covered or
    // skipped with a reason, never both, never neither.
    let map = read_json(&fixtures.join("mcp-tool-fixture-map.v1.json"));
    let live: BTreeSet<String> = map["live_tools"]
        .as_array()
        .expect("live_tools")
        .iter()
        .map(|t| t["name"].as_str().expect("tool name").to_owned())
        .collect();
    let skip_list = skipped();
    let skipped: BTreeSet<String> = skip_list.iter().map(|(n, _)| (*n).to_owned()).collect();
    for (name, reason) in &skip_list {
        assert!(!reason.is_empty(), "skip entry {name} needs a reason");
    }
    let both: Vec<_> = h.covered.intersection(&skipped).collect();
    assert!(both.is_empty(), "tools both covered and skipped: {both:?}");
    let accounted: BTreeSet<String> = h.covered.union(&skipped).cloned().collect();
    let uncovered: Vec<_> = live.difference(&accounted).collect();
    let unknown: Vec<_> = accounted.difference(&live).collect();
    assert!(
        uncovered.is_empty(),
        "tools with neither a live shape check nor a SKIPPED entry: {uncovered:?}"
    );
    assert!(
        unknown.is_empty(),
        "covered/skipped names not in the fixture map: {unknown:?}"
    );
    eprintln!(
        "fixture live-shape coverage on {}: {} covered, {} skipped, {} live tools",
        std::env::consts::OS,
        h.covered.len(),
        skipped.len(),
        live.len()
    );

    assert!(
        h.failures.is_empty(),
        "fixture/live key-set drift:\n{}",
        h.failures.join("\n\n")
    );

    drop(h);
    let _ = std::fs::remove_dir_all(&data);
}

/// One deterministic pass over the tool surface. Order matters: later calls
/// reuse state minted by earlier ones.
async fn scenario(h: &mut Harness, data: &Path) {
    let work = data.join("work");
    std::fs::create_dir_all(&work).expect("work dir");
    let work_s = work.to_string_lossy().into_owned();
    let bucket = command_chain(h).await;
    long_lived_command(h).await;
    #[cfg(unix)]
    shell_tools(h, &work_s).await;
    recipe_tools(h).await;
    pty_and_file_tools(h, &work_s, &work).await;
    status_tools(h).await;
    registry_tools(h).await;
    subscription_tools(h, &bucket).await;
}

async fn command_chain(h: &mut Harness) -> String {
    // Quiet run (no rules): the response carries the bounded exit receipt.
    h.check_host(
        "run_and_watch",
        json!({"argv": echo_argv("quiet"), "wait_ms": 3000, "wait_until": "exit"}),
        "/response_example",
        &[],
        GOVERNOR_HOST_DEPENDENT,
    )
    .await;
    // --- command / bucket / probe / event chain ---------------------------
    let run = h
        .check_host(
            "run_and_watch",
            json!({"argv": echo_argv("hello"), "wait_ms": 3000, "wait_until": "exit",
                   "rules": [{"pattern": "hello", "severity": "high"}]}),
            "/response_example",
            &[],
            GOVERNOR_HOST_DEPENDENT,
        )
        .await;
    let job = s(&run, "job_id");
    let bucket = s(&run, "bucket_id");

    // command_status: a finished job whose rule matched (no receipt) ...
    h.wait_finished(&job).await;
    h.check_host(
        "command_status",
        json!({"job_id": job}),
        "/response_example",
        &[],
        GOVERNOR_HOST_DEPENDENT,
    )
    .await;
    // ... and a quiet finished job whose output is longer than the receipt
    // tail, so the receipt carries head lines and lines_omitted.
    let quiet = h
        .call(
            "command_start_combed",
            json!({"argv": many_lines_argv(), "receipt_head_lines": 3, "receipt_tail_lines": 3}),
        )
        .await;
    let quiet_job = s(&quiet, "job_id");
    h.wait_finished(&quiet_job).await;
    h.check_host(
        "command_status",
        json!({"job_id": quiet_job}),
        "/response_example_no_rule_receipt",
        &[],
        GOVERNOR_HOST_DEPENDENT,
    )
    .await;

    let events = h
        .check(
            "bucket_events_since",
            json!({"bucket_id": bucket, "cursor": 0}),
            "/response_example",
            &[(
                "events[].source.job_id",
                "SignalEvent optional field (skip_serializing_if None)",
            )],
        )
        .await;
    let event_id = s(&events["events"][0], "event_id");

    h.check(
        "bucket_summary",
        json!({"bucket_id": bucket}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "bucket_wait",
        json!({"bucket_id": bucket, "cursor": 0, "timeout_ms": 200}),
        "/response_examples/with_events",
        &[(
            "events[].source.job_id",
            "SignalEvent optional field (skip_serializing_if None)",
        )],
    )
    .await;
    h.check(
        "event_context",
        json!({"bucket_id": bucket, "event_id": event_id}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "command_output_tail",
        json!({"job_id": job, "max_lines": 10}),
        "/response_example",
        &[],
    )
    .await;

    h.check(
        "shell_exec",
        json!({"shell_line": "echo hi"}),
        "/response_example",
        &[],
    )
    .await;
    bucket
}

async fn long_lived_command(h: &mut Harness) {
    // A long-lived command to stop.
    let sleeper = h
        .check(
            "command_start_combed",
            json!({"argv": sleeper_argv()}),
            "/response_example",
            &[],
        )
        .await;
    // command_status on a still-running job carries live liveness fields;
    // wait until its first output line has been captured so
    // `last_output_age_ms` is present on every platform.
    let sleeper_job = s(&sleeper, "job_id");
    for _ in 0..50 {
        let st = h
            .call("command_status", json!({"job_id": sleeper_job}))
            .await;
        if st.get("last_output_age_ms").is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    h.check(
        "command_status",
        json!({"job_id": s(&sleeper, "job_id")}),
        "/response_example_running",
        &[],
    )
    .await;
    h.check(
        "probe_status",
        json!({"probe_id": s(&sleeper, "probe_id")}),
        "/response_example",
        &[(
            "probe.argv_head",
            "ProbeListEntry.argv_head: skip_serializing_if None (absent for sources without argv)",
        )],
    )
    .await;
    h.check(
        "probe_list",
        json!({}),
        "/response_examples/with_live_probes",
        &[(
            "probes[].tag",
            "ProbeListEntry.tag: skip_serializing_if None (only set when the bucket was tagged)",
        )],
    )
    .await;
    h.check(
        "runtime_state",
        json!({}),
        "/response_examples/with_runtime_entries",
        &[(
            "probes[].tag",
            "ProbeListEntry.tag: skip_serializing_if None (only set when the bucket was tagged)",
        )],
    )
    .await;
    h.check(
        "command_stop",
        json!({"job_id": s(&sleeper, "job_id")}),
        "/response_example",
        &[],
    )
    .await;
}

#[cfg(unix)]
async fn shell_tools(h: &mut Harness, work_s: &str) {
    // --- shell -------------------------------------------------------------
    let session = h
        .check(
            "shell_session_start",
            json!({"shell": "/bin/sh", "cwd": work_s}),
            "/response_example",
            &[],
        )
        .await;
    let sid = s(&session, "session_id");
    h.check(
        "shell_session_exec",
        json!({"session_id": sid, "line": "pwd", "wait_ms": 500}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "shell_session_status",
        json!({"session_id": sid}),
        "/response_example",
        &[],
    )
    .await;
    h.check("shell_session_list", json!({}), "/response_example", &[])
        .await;
    let snap = h
        .check(
            "workspace_snapshot_create",
            json!({"session_id": sid, "name": "shape"}),
            "/response_example",
            &[],
        )
        .await;
    h.check(
        "workspace_snapshot_apply",
        json!({"snapshot_id": s(&snap, "snapshot_id"), "session_id": sid}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "shell_session_stop",
        json!({"session_id": sid}),
        "/response_example",
        &[],
    )
    .await;
}

async fn pty_and_file_tools(h: &mut Harness, work_s: &str, work: &Path) {
    // --- PTY ---------------------------------------------------------------
    let pty = h
        .check(
            "pty_command_start",
            json!({"argv": pty_argv(), "cwd": work_s}),
            "/response_example",
            &[],
        )
        .await;
    let pjob = s(&pty, "job_id");
    h.check(
        "pty_command_write_stdin",
        json!({"job_id": pjob, "bytes": "hello\r\n"}),
        "/response_example",
        &[],
    )
    .await;
    h.check("pty_command_list", json!({}), "/response_examples/one_live_pty", &[("entries[].awaiting_credential", "AwaitingCredential: skip_serializing_if None (only while blocked on a password prompt)")])
        .await;

    // --- file tools --------------------------------------------------------
    let file = work.join("a.txt");
    let file_s = file.to_string_lossy().into_owned();
    h.check(
        "file_write",
        json!({"path": file_s, "content": "alpha\nbeta\n", "create_dirs": true}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "file_read_window",
        json!({"path": file_s, "start_line": 1, "max_lines": 5}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "file_search",
        json!({"path": file_s, "query": "alpha"}),
        "/response_example",
        &[],
    )
    .await;
    let watch = h
        .check(
            "file_watch_start",
            json!({"path": file_s}),
            "/response_example",
            &[],
        )
        .await;
    h.check(
        "file_watch_list",
        json!({}),
        "/response_examples/one_live_watch",
        &[],
    )
    .await;
    h.check(
        "file_watch_stop",
        json!({"watch_id": s(&watch, "watch_id")}),
        "/response_example",
        &[],
    )
    .await;

    // Stop the PTY now that list has seen it.
    h.check(
        "pty_command_stop",
        json!({"job_id": pjob}),
        "/response_example",
        &[],
    )
    .await;
}

async fn status_tools(h: &mut Harness) {
    // --- lists / status that want live state -------------------------------
    h.check_host(
        "system_discover",
        json!({}),
        "/response_example_summary",
        &[],
        DISCOVER_HOST_DEPENDENT,
    )
    .await;
    h.check_host(
        "system_discover",
        json!({"detail": "full"}),
        "/response_example_full",
        &[],
        DISCOVER_HOST_DEPENDENT,
    )
    .await;
    h.check("health", json!({}), "/response_example", &[]).await;
    // The default memory resolves from host memory, so it can be absent.
    h.check_host(
        "policy_status",
        json!({}),
        "/response_example",
        &[],
        &["governor.default_job_memory_bytes", "governor.note"],
    )
    .await;
    h.check("self_check", json!({}), "/response_example", &[])
        .await;
    h.check(
        "audit_since",
        json!({"cursor": 0, "limit": 5, "action_filter": "file_write"}),
        "/response_examples/with_rows",
        &[(
            "rows[].reason",
            "AuditRowWire.reason: skip_serializing_if None",
        )],
    )
    .await;
    h.check("target_list", json!({}), "/response_example", &[])
        .await;
}

async fn registry_tools(h: &mut Harness) {
    // --- registry ----------------------------------------------------------
    let def = json!({
        "id": "shape-rule", "version": 1, "kind": "keyword", "status": "active",
        "severity": "medium", "event_kind": "shape_event", "stream": null,
        "description": "fixture shape", "pattern": null, "keywords": ["shape"],
        "captures": [], "summary_template": "matched", "tags": ["shape"],
        "rate_limit_per_min": null, "redact": [],
        "context_hint": {"before_lines": 0, "after_lines": 0},
        "examples": [
            {"input": "shape here", "expect": {"kind": "shape_event"}},
            {"input": "nothing relevant", "expect": {"match": false}}
        ]
    });
    h.check(
        "registry_upsert",
        json!({"definition_json": def.to_string()}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "registry_get",
        json!({"rule_id": "shape-rule", "version": 1}),
        "/response_example",
        &[
            (
                "definition.pattern",
                "RuleDefinition.pattern: only regex rules",
            ),
            (
                "definition.stream",
                "RuleDefinition.stream: optional stream filter",
            ),
            (
                "definition.keywords",
                "RuleDefinition.keywords: only keyword rules",
            ),
        ],
    )
    .await;
    h.check(
        "registry_search",
        json!({"query": "shape"}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "registry_test",
        json!({"rule_id": "shape-rule", "version": 1,
               "samples": [{"text": "shape here", "stream": "stderr"}]}),
        "/response_example",
        &[(
            "stream_mismatches",
            "RegistryTestResponse.stream_mismatches: skip_serializing_if Vec::is_empty",
        )],
    )
    .await;
    h.check(
        "registry_activate",
        json!({"rule_id": "shape-rule", "version": 1, "scope": {"kind": "global"}}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "registry_list_active",
        json!({}),
        "/response_examples/one_active_global_rule",
        &[],
    )
    .await;
    h.check(
        "registry_deactivate",
        json!({"rule_id": "shape-rule", "version": 1, "scope": {"kind": "global"}}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "registry_import_pack",
        json!({"pack": "cargo", "activate": false}),
        "/response_example",
        &[(
            "failed",
            "RegistryImportResponse.failed: skip_serializing_if Vec::is_empty",
        )],
    )
    .await;
    h.check(
        "registry_suggest_from_samples",
        json!({"samples": ["error[E0432]: unresolved import", "warning: unused variable"]}),
        "/response_example",
        &[],
    )
    .await;
}

async fn subscription_tools(h: &mut Harness, bucket: &str) {
    // --- subscriptions -----------------------------------------------------
    let sub = h
        .check(
            "subscription_open",
            json!({"sources": {"kind": "buckets", "buckets": [bucket]}}),
            "/response_example",
            &[],
        )
        .await;
    let sub_id = s(&sub, "sub_id");
    h.check(
        "subscription_list",
        json!({}),
        "/response_example",
        &[(
            "subscriptions[].last_pull_at_ms",
            "SubscriptionInfo.last_pull_at_ms: skip_serializing_if None until the first pull",
        )],
    )
    .await;
    h.check(
        "subscription_pull",
        json!({"sub_id": sub_id, "max": 10, "timeout_ms": 100}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "subscription_seek",
        json!({"sub_id": sub_id, "bucket_id": bucket, "seq": 1}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "subscription_close",
        json!({"sub_id": sub_id}),
        "/response_example",
        &[],
    )
    .await;
}

/// Argv recipes: one harmless watched recipe through its whole lifecycle.
async fn recipe_tools(h: &mut Harness) {
    let argv = echo_argv("FIXTURESHAPE");
    let argv0 = argv[0].clone();
    let definition = json!({
        "recipe_id": "fixture.echo", "version": 1, "title": "Fixture echo",
        "summary": "Print a marker and exit", "argv": argv, "status": "active",
        "tags": ["fixture"], "cwd": null, "env_allowlist": [],
        "timeout_ms": 5000, "rule_pack_ids": [], "placeholders": []
    });
    h.check(
        "recipe_upsert",
        json!({"definition_json": definition.to_string()}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "recipe_get",
        json!({"recipe_id": "fixture.echo"}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "recipe_search",
        json!({"query": "fixture"}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "recipe_test",
        json!({"recipe_id": "fixture.echo", "expect_argv0": argv0}),
        "/response_example",
        &[],
    )
    .await;
    h.check(
        "recipe_activate",
        json!({"recipe_id": "fixture.echo", "scope": {"kind": "global"}}),
        "/response_example",
        &[],
    )
    .await;
    h.check("recipe_list_active", json!({}), "/response_example", &[])
        .await;
    h.check_host(
        "recipe_run",
        json!({"recipe_id": "fixture.echo", "scope": {"kind": "global"}}),
        "/response_example",
        &[],
        GOVERNOR_HOST_DEPENDENT,
    )
    .await;
    h.check(
        "recipe_deactivate",
        json!({"recipe_id": "fixture.echo", "scope": {"kind": "global"}}),
        "/response_example",
        &[],
    )
    .await;
}
