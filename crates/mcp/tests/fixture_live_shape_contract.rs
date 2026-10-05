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

#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ClientServiceExt, ServiceExt};
use serde_json::{Value, json};

use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commanderd::{DaemonConfig, DaemonState, IpcServer};

/// Tools deliberately not driven by this test. Each entry carries its reason.
const SKIPPED: &[(&str, &str)] = &[
    (
        "command_status",
        "being changed on another branch (receipt head/lines_omitted, liveness fields) - enable after integration",
    ),
    (
        "system_discover",
        "being changed on another branch (discovery age field, `detail` parameter with lean default) - enable after integration",
    ),
    (
        "credential_request",
        "needs a PTY job blocked on a real password prompt; not deterministic or harmless to provoke",
    ),
    (
        "target_probe",
        "needs a registered remote target in the adapter-side targets.toml",
    ),
    (
        "recipe_search",
        "no per-tool fixture yet (fixture map status missing_fixture)",
    ),
    (
        "recipe_get",
        "no per-tool fixture yet (fixture map status missing_fixture)",
    ),
    (
        "recipe_upsert",
        "no per-tool fixture yet (fixture map status missing_fixture)",
    ),
    (
        "recipe_test",
        "no per-tool fixture yet (fixture map status missing_fixture)",
    ),
    (
        "recipe_activate",
        "no per-tool fixture yet (fixture map status missing_fixture)",
    ),
    (
        "recipe_deactivate",
        "no per-tool fixture yet (fixture map status missing_fixture)",
    ),
    (
        "recipe_list_active",
        "no per-tool fixture yet (fixture map status missing_fixture)",
    ),
    (
        "recipe_run",
        "no per-tool fixture yet (fixture map status missing_fixture)",
    ),
];

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
        let actual = self.call(tool, args).await;
        let fixture = read_json(&self.fixtures.join(format!("mcp-tools/{tool}.v1.json")));
        let expected = fixture
            .pointer(example)
            .unwrap_or_else(|| panic!("{tool}: fixture has no example at {example}"));
        let mut diffs = Vec::new();
        diff_keys("$", expected, &actual, &mut diffs);
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
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket)
        .spawn()
        .expect("ipc server spawn");

    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let daemon = McpDaemonClient::new(handle.socket_path().to_path_buf())
        .with_timeout(Duration::from_secs(10));
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
    let skipped: BTreeSet<String> = SKIPPED.iter().map(|(n, _)| (*n).to_owned()).collect();
    for (name, reason) in SKIPPED {
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
        "fixture live-shape coverage: {} covered, {} skipped, {} live tools",
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
    shell_tools(h, &work_s).await;
    pty_and_file_tools(h, &work_s, &work).await;
    status_tools(h).await;
    registry_tools(h).await;
    subscription_tools(h, &bucket).await;
}

async fn command_chain(h: &mut Harness) -> String {
    // --- command / bucket / probe / event chain ---------------------------
    let run = h
        .check(
            "run_and_watch",
            json!({"argv": ["echo", "hello"], "wait_ms": 3000, "wait_until": "exit",
                   "rules": [{"pattern": "hello", "severity": "high"}]}),
            "/response_example",
            &[],
        )
        .await;
    let job = s(&run, "job_id");
    let bucket = s(&run, "bucket_id");

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

    bucket
}

async fn long_lived_command(h: &mut Harness) {
    // A long-lived command to stop.
    let sleeper = h
        .check(
            "command_start_combed",
            json!({"argv": ["sleep", "30"]}),
            "/response_example",
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

async fn shell_tools(h: &mut Harness, work_s: &str) {
    // --- shell -------------------------------------------------------------
    h.check(
        "shell_exec",
        json!({"shell_line": "echo hi | wc -c"}),
        "/response_example",
        &[],
    )
    .await;
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
            json!({"argv": ["cat"], "cwd": work_s}),
            "/response_example",
            &[],
        )
        .await;
    let pjob = s(&pty, "job_id");
    h.check(
        "pty_command_write_stdin",
        json!({"job_id": pjob, "bytes": "hello\n"}),
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
    h.check("health", json!({}), "/response_example", &[]).await;
    h.check("policy_status", json!({}), "/response_example", &[])
        .await;
    h.check("self_check", json!({}), "/response_example", &[])
        .await;
    h.check(
        "audit_since",
        json!({"cursor": 0, "limit": 5}),
        "/response_examples/with_rows",
        &[
            (
                "rows[].profile",
                "AuditRowWire.profile: skip_serializing_if None",
            ),
            (
                "rows[].reason",
                "AuditRowWire.reason: skip_serializing_if None",
            ),
        ],
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
        "context_hint": {"before_lines": 0, "after_lines": 0}, "examples": []
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
