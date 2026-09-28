// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! MCP recipe_activate/deactivate deny when the flag is false, and recipe_run
//! on the argv lane after an admin activation.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::{ClientHandler, ClientServiceExt, ServiceExt};

use terminal_commander_core::{ActivationScope, RecipeDefinition, RecipeStatus};
use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcRequest, IpcServer, RecipeActivateParams,
    ServerHandle,
};

#[derive(Default, Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-mcp-recipe-{tag}-{}-{n}", std::process::id()));
    p
}

fn watched_true() -> RecipeDefinition {
    RecipeDefinition {
        recipe_id: "echo.true".to_owned(),
        version: 1,
        title: "True".to_owned(),
        summary: "Exit immediately".to_owned(),
        argv: vec!["true".to_owned()],
        status: RecipeStatus::Active,
        tags: vec![],
        cwd: None,
        env_allowlist: vec![],
        timeout_ms: Some(2_000),
        rule_pack_ids: vec![],
        placeholders: vec![],
    }
}

fn spawn_daemon(data: &std::path::Path) -> (ServerHandle, Arc<DaemonState>) {
    let cfg = DaemonConfig::defaults_in(data);
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"));
    let handle = IpcServer::new(Arc::clone(&state), state.config.socket_path())
        .spawn()
        .expect("ipc spawn");
    (handle, state)
}

async fn paired(
    handle: &ServerHandle,
) -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let daemon = McpDaemonClient::new(handle.socket_path().to_path_buf())
        .with_timeout(Duration::from_secs(8));
    let server = TerminalCommanderMcpServer::new(daemon);
    let server_handle =
        tokio::spawn(async move { server.serve(server_transport).await.expect("serve") });
    let client = TestClient
        .serve_with_lifecycle(
            client_transport,
            rmcp::ClientLifecycleMode::Discover {
                preferred_versions: vec![rmcp::model::ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("client");
    let server = server_handle.await.expect("server join");
    (server, client)
}

async fn call_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
    name: &'static str,
    arguments: serde_json::Value,
) -> Result<rmcp::model::CallToolResult, rmcp::service::ServiceError> {
    let mut params = CallToolRequestParams::new(name);
    if let serde_json::Value::Object(map) = arguments {
        params.arguments = Some(map);
    }
    client.call_tool(params).await
}

fn first_text(result: &rmcp::model::CallToolResult) -> String {
    for item in &result.content {
        if let Some(text) = item.as_text() {
            return text.text.clone();
        }
    }
    panic!("expected text content");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)] // deny gate plus one argv-lane run
async fn mcp_activate_denied_and_run_uses_argv_lane() {
    let data = tmp_data_dir("e2e");
    let (handle, state) = spawn_daemon(&data);
    let (_server, client) = paired(&handle).await;

    let listed = client.list_all_tools().await.expect("list_tools");
    let names: Vec<String> = listed.iter().map(|tool| tool.name.to_string()).collect();
    for required in [
        "recipe_search",
        "recipe_get",
        "recipe_upsert",
        "recipe_test",
        "recipe_activate",
        "recipe_deactivate",
        "recipe_list_active",
        "recipe_run",
    ] {
        assert!(names.iter().any(|name| name == required), "{required}");
    }
    assert!(!names.iter().any(|name| name == "recipe"));

    let definition = serde_json::to_string(&watched_true()).unwrap();
    let upserted = call_tool(
        &client,
        "recipe_upsert",
        serde_json::json!({ "definition_json": definition }),
    )
    .await
    .expect("upsert");
    let upsert_body: serde_json::Value =
        serde_json::from_str(&first_text(&upserted)).expect("upsert json");
    assert_eq!(upsert_body["activated"], false);

    let tested = call_tool(
        &client,
        "recipe_test",
        serde_json::json!({
            "recipe_id": "echo.true",
            "expect_argv0": "true"
        }),
    )
    .await
    .expect("test");
    let test_body: serde_json::Value = serde_json::from_str(&first_text(&tested)).unwrap();
    assert_eq!(test_body["activated"], false);
    assert_eq!(test_body["expects_met"], true);

    let active = call_tool(&client, "recipe_list_active", serde_json::json!({}))
        .await
        .expect("list");
    let active_body: serde_json::Value = serde_json::from_str(&first_text(&active)).unwrap();
    assert_eq!(active_body["entries"].as_array().map(Vec::len), Some(0));

    for tool in ["recipe_activate", "recipe_deactivate"] {
        let err = call_tool(
            &client,
            tool,
            serde_json::json!({
                "recipe_id": "echo.true",
                "scope": {"kind": "global"}
            }),
        )
        .await
        .expect_err(tool);
        let rendered = err.to_string();
        assert!(
            rendered.contains("recipe_activate_requires_admin"),
            "{tool}: {rendered}"
        );
    }

    // This test binary is not the `terminal-commander` image. Release
    // builds (this e2e links the daemon without cfg(test)) deny the claim.
    let ipc =
        DaemonClient::new(handle.socket_path().to_path_buf()).with_timeout(Duration::from_secs(5));
    let denied = ipc
        .call(
            1,
            IpcRequest::RecipeActivate(RecipeActivateParams {
                recipe_id: "echo.true".to_owned(),
                version: Some(1),
                scope: Some(ActivationScope::Global),
                from_mcp: false,
            }),
        )
        .await
        .expect_err("non-cli peer cannot self-claim admin");
    assert!(
        denied.message.contains("recipe_activate_requires_admin"),
        "{}",
        denied.message
    );
    assert!(
        state
            .store
            .record_recipe_activation_scoped(
                "echo.true",
                1,
                ActivationScope::Global,
                Some("test"),
                Some("admin"),
            )
            .expect("store activate")
    );

    let ran = call_tool(
        &client,
        "recipe",
        serde_json::json!({
            "action": "run",
            "recipe_id": "echo.true",
            "scope": {"kind": "global"}
        }),
    )
    .await
    .expect("recipe facade run");
    let body: serde_json::Value = serde_json::from_str(&first_text(&ran)).expect("run json");
    assert_eq!(body["lane"], "argv");
    assert_eq!(body["watched"], true);
    assert_eq!(body["argv"][0], "true");
    assert!(
        body["state"] == "exited" || body["complete"] == serde_json::json!(true),
        "bounded watch should observe exit: {body}"
    );

    let _ = client.cancel().await;
    handle.shutdown().await;
    let _ = std::fs::remove_dir_all(&data);
}

fn python3_available() -> bool {
    ["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"]
        .iter()
        .any(|c| std::path::Path::new(c).exists())
}

/// Prints more matches than `recipe_run`'s fixed 50-signal cap, then keeps
/// running past it (FCR2-011 cap path).
fn cap_needle_recipe() -> RecipeDefinition {
    RecipeDefinition {
        recipe_id: "echo.cap-needle".to_owned(),
        version: 1,
        title: "Cap needle".to_owned(),
        summary: "Print more matches than max_signals, then keep running".to_owned(),
        argv: vec![
            "python3".to_owned(),
            "-u".to_owned(),
            "-c".to_owned(),
            "import time\nfor i in range(60):\n    print('NEEDLE', i)\ntime.sleep(2)".to_owned(),
        ],
        status: RecipeStatus::Active,
        tags: vec![],
        cwd: None,
        env_allowlist: vec![],
        timeout_ms: Some(5_000),
        rule_pack_ids: vec![],
        placeholders: vec![],
    }
}

/// Sleeps well past its own short `timeout_ms` (FCR2-011 deadline path).
fn deadline_sleep_recipe() -> RecipeDefinition {
    RecipeDefinition {
        recipe_id: "echo.deadline-sleep".to_owned(),
        version: 1,
        title: "Deadline sleep".to_owned(),
        summary: "Outlive a short wait_ms so recipe_run reports wait_exhausted".to_owned(),
        argv: vec![
            "python3".to_owned(),
            "-u".to_owned(),
            "-c".to_owned(),
            "import time\ntime.sleep(2)\nprint('done')".to_owned(),
        ],
        status: RecipeStatus::Active,
        tags: vec![],
        cwd: None,
        env_allowlist: vec![],
        timeout_ms: Some(300),
        rule_pack_ids: vec![],
        placeholders: vec![],
    }
}

fn needle_recipe() -> RecipeDefinition {
    RecipeDefinition {
        recipe_id: "echo.needle".to_owned(),
        version: 1,
        title: "Needle".to_owned(),
        summary: "Print a keyword the active rule can match".to_owned(),
        argv: vec!["echo".to_owned(), "FCRNEEDLE".to_owned()],
        status: RecipeStatus::Active,
        tags: vec![],
        cwd: None,
        env_allowlist: vec![],
        timeout_ms: Some(5_000),
        rule_pack_ids: vec!["git".to_owned()],
        placeholders: vec![],
    }
}

fn keyword_rule_json(id: &str, keyword: &str, event_kind: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id,
        "version": 1,
        "kind": "keyword",
        "status": "active",
        "severity": "medium",
        "event_kind": event_kind,
        "stream": null,
        "description": "fcr watched recipe",
        "pattern": null,
        "keywords": [keyword],
        "captures": [],
        "summary_template": "matched keyword",
        "tags": ["test"],
        "rate_limit_per_min": null,
        "redact": [],
        "context_hint": { "before_lines": 0, "after_lines": 0 },
        "examples": []
    }))
    .expect("rule json")
}

fn rule_event_ids(events: &serde_json::Value) -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::new();
    for ev in events.as_array().into_iter().flatten() {
        if ev.get("rule").is_some_and(|rule| !rule.is_null())
            && let Some(id) = ev["event_id"].as_str()
        {
            ids.insert(id.to_owned());
        }
    }
    ids
}

async fn events_since(
    client: &rmcp::service::RunningService<rmcp::RoleClient, TestClient>,
    bucket_id: &str,
    mut cursor: u64,
) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for _ in 0..8 {
        let page = call_tool(
            client,
            "bucket_events_since",
            serde_json::json!({ "bucket_id": bucket_id, "cursor": cursor }),
        )
        .await
        .expect("events");
        let body: serde_json::Value =
            serde_json::from_str(&first_text(&page)).expect("events json");
        if let Some(events) = body["events"].as_array() {
            out.extend(events.clone());
        }
        let next = body["next_cursor"].as_u64().unwrap_or(cursor);
        if !body["has_more"].as_bool().unwrap_or(false) || next == cursor {
            break;
        }
        cursor = next;
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)] // one watched run: contract keys, signals, resume cursor
async fn watched_recipe_run_returns_signals_and_resume_cursor_loses_nothing() {
    let data = tmp_data_dir("watch");
    let (handle, state) = spawn_daemon(&data);
    let (_server, client) = paired(&handle).await;

    call_tool(
        &client,
        "registry_upsert",
        serde_json::json!({
            "definition_json": keyword_rule_json("fcr-needle", "FCRNEEDLE", "fcr_needle"),
        }),
    )
    .await
    .expect("rule upsert");
    call_tool(
        &client,
        "registry_activate",
        serde_json::json!({"rule_id": "fcr-needle", "scope": {"kind": "global"}}),
    )
    .await
    .expect("rule activate");

    let definition = serde_json::to_string(&needle_recipe()).unwrap();
    let upserted = call_tool(
        &client,
        "recipe_upsert",
        serde_json::json!({ "definition_json": definition }),
    )
    .await
    .expect("recipe upsert");
    let upsert_body: serde_json::Value =
        serde_json::from_str(&first_text(&upserted)).expect("upsert json");
    let version = u32::try_from(upsert_body["version"].as_u64().expect("version")).unwrap();
    assert!(
        state
            .store
            .record_recipe_activation_scoped(
                "echo.needle",
                version,
                ActivationScope::Global,
                Some("test"),
                Some("admin"),
            )
            .expect("store activate")
    );

    let ran = call_tool(
        &client,
        "recipe_run",
        serde_json::json!({
            "recipe_id": "echo.needle",
            "scope": {"kind": "global"}
        }),
    )
    .await
    .expect("recipe_run");
    let body: serde_json::Value = serde_json::from_str(&first_text(&ran)).expect("run json");
    assert_eq!(body["watched"], true);
    assert_eq!(body["degraded"], false);
    assert_eq!(body["lane"], "argv");
    // FCR2-011: watched recipe_run is documented as the same contract as
    // run_and_watch (signals_capped, wait_exhausted, receipt, outcome_trust,
    // poll_hint_ms, wait_cap_ms). Before the fix these keys were absent, so
    // indexing them yielded Value::Null and every assertion below failed.
    assert_eq!(
        body["signals_capped"], false,
        "signals_capped must be present and false (well under the cap): {body}"
    );
    assert_eq!(
        body["wait_exhausted"], false,
        "wait_exhausted must be present and false (the job finished): {body}"
    );
    assert_eq!(
        body["outcome_trust"], "observed",
        "outcome_trust must be present (a status poll succeeded): {body}"
    );
    assert!(
        body.get("wait_cap_ms")
            .and_then(serde_json::Value::as_u64)
            .is_some(),
        "wait_cap_ms must be present: {body}"
    );
    assert!(
        body.as_object()
            .is_some_and(|obj| obj.contains_key("poll_hint_ms")),
        "poll_hint_ms key must be present (null once complete): {body}"
    );
    assert!(
        body.as_object()
            .is_some_and(|obj| obj.contains_key("receipt")),
        "receipt key must be present (null here: signals are non-empty): {body}"
    );
    let signal_ids = rule_event_ids(&body["signals"]);
    assert!(
        body["signals"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["kind"].as_str() == Some("fcr_needle"))
        }),
        "watched recipe_run must return the rule signal: {body}"
    );
    let bucket_id = body["bucket_id"].as_str().expect("bucket").to_owned();
    let cursor = body["cursor"].as_u64().expect("cursor");
    let history = serde_json::Value::Array(events_since(&client, &bucket_id, 0).await);
    let tail = serde_json::Value::Array(events_since(&client, &bucket_id, cursor).await);
    let history_ids = rule_event_ids(&history);
    let tail_ids = rule_event_ids(&tail);
    assert!(!history_ids.is_empty(), "bucket history has the rule event");
    for id in &history_ids {
        assert!(
            signal_ids.contains(id) || tail_ids.contains(id),
            "rule event {id} is neither in signals nor after cursor {cursor}"
        );
    }

    let _ = client.cancel().await;
    handle.shutdown().await;
    let _ = std::fs::remove_dir_all(&data);
}

/// FCR2-011: a recipe whose job outproduces the 50-signal cap while still
/// running must report the cap honestly (signals_capped, incomplete,
/// recover_hint), and the omitted matches must still be reachable through
/// bucket_wait -- the same contract run_and_watch's cap tests pin.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)] // one capped run: cap fields, then a lossless resume
async fn watched_recipe_run_reports_capped_signals_and_recoverable_cursor() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let data = tmp_data_dir("watch-cap");
    let (handle, state) = spawn_daemon(&data);
    let (_server, client) = paired(&handle).await;

    call_tool(
        &client,
        "registry_upsert",
        serde_json::json!({
            "definition_json": keyword_rule_json("fcr-cap-needle", "NEEDLE", "cap_needle_match"),
        }),
    )
    .await
    .expect("rule upsert");
    call_tool(
        &client,
        "registry_activate",
        serde_json::json!({"rule_id": "fcr-cap-needle", "scope": {"kind": "global"}}),
    )
    .await
    .expect("rule activate");

    let definition = serde_json::to_string(&cap_needle_recipe()).unwrap();
    let upserted = call_tool(
        &client,
        "recipe_upsert",
        serde_json::json!({ "definition_json": definition }),
    )
    .await
    .expect("recipe upsert");
    let upsert_body: serde_json::Value =
        serde_json::from_str(&first_text(&upserted)).expect("upsert json");
    let version = u32::try_from(upsert_body["version"].as_u64().expect("version")).unwrap();
    assert!(
        state
            .store
            .record_recipe_activation_scoped(
                "echo.cap-needle",
                version,
                ActivationScope::Global,
                Some("test"),
                Some("admin"),
            )
            .expect("store activate")
    );

    let ran = call_tool(
        &client,
        "recipe_run",
        serde_json::json!({
            "recipe_id": "echo.cap-needle",
            "scope": {"kind": "global"}
        }),
    )
    .await
    .expect("recipe_run");
    let body: serde_json::Value = serde_json::from_str(&first_text(&ran)).expect("run json");
    assert_eq!(body["signals_capped"], true, "cap must be hit: {body}");
    assert_eq!(
        body["complete"], false,
        "the job is still sleeping past the cap: {body}"
    );
    assert!(
        body["recover_hint"].as_str().is_some(),
        "a capped, incomplete result must carry a recover_hint: {body}"
    );
    let signals = body["signals"].as_array().expect("signals array");
    assert_eq!(
        signals.len(),
        50,
        "signals must be truncated to the fixed max_signals cap: {body}"
    );
    // Only this rule's matches count: `command_exited` shares the bucket.
    let needle_ids = |events: &serde_json::Value| -> std::collections::BTreeSet<String> {
        events
            .as_array()
            .into_iter()
            .flatten()
            .filter(|ev| ev["kind"] == "cap_needle_match")
            .filter_map(|ev| ev["event_id"].as_str().map(str::to_owned))
            .collect()
    };
    let signal_ids = needle_ids(&body["signals"]);
    assert_eq!(
        signal_ids.len(),
        50,
        "every signal is a needle match: {body}"
    );

    let bucket_id = body["bucket_id"].as_str().expect("bucket").to_owned();
    let mut cursor = body["cursor"].as_u64().expect("cursor");
    let mut resumed_ids = std::collections::BTreeSet::new();
    for _ in 0..5 {
        let resumed = call_tool(
            &client,
            "bucket_wait",
            serde_json::json!({
                "bucket_id": bucket_id,
                "cursor": cursor,
                "timeout_ms": 3000,
                "limit": 20
            }),
        )
        .await
        .expect("bucket_wait");
        let resumed_body: serde_json::Value =
            serde_json::from_str(&first_text(&resumed)).expect("bucket_wait json");
        resumed_ids.extend(needle_ids(&resumed_body["events"]));
        let next = resumed_body["next_cursor"].as_u64().unwrap_or(cursor);
        if resumed_ids.len() >= 10 || next == cursor {
            break;
        }
        cursor = next;
    }
    assert!(
        signal_ids.is_disjoint(&resumed_ids),
        "the resume cursor must not replay returned signals: {resumed_ids:?}"
    );
    assert_eq!(
        signal_ids.len() + resumed_ids.len(),
        60,
        "the 10 matches omitted by the cap must all be recoverable via bucket_wait: {resumed_ids:?}"
    );

    let _ = client.cancel().await;
    handle.shutdown().await;
    let _ = std::fs::remove_dir_all(&data);
}

/// FCR2-011: a recipe that outlives its own short `timeout_ms` must report
/// `wait_exhausted` with a `recover_hint`, distinguishable from the cap path
/// above (no signals here, only the deadline).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watched_recipe_run_reports_wait_exhausted_on_deadline() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let data = tmp_data_dir("watch-deadline");
    let (handle, state) = spawn_daemon(&data);
    let (_server, client) = paired(&handle).await;

    let definition = serde_json::to_string(&deadline_sleep_recipe()).unwrap();
    let upserted = call_tool(
        &client,
        "recipe_upsert",
        serde_json::json!({ "definition_json": definition }),
    )
    .await
    .expect("recipe upsert");
    let upsert_body: serde_json::Value =
        serde_json::from_str(&first_text(&upserted)).expect("upsert json");
    let version = u32::try_from(upsert_body["version"].as_u64().expect("version")).unwrap();
    assert!(
        state
            .store
            .record_recipe_activation_scoped(
                "echo.deadline-sleep",
                version,
                ActivationScope::Global,
                Some("test"),
                Some("admin"),
            )
            .expect("store activate")
    );

    let ran = call_tool(
        &client,
        "recipe_run",
        serde_json::json!({
            "recipe_id": "echo.deadline-sleep",
            "scope": {"kind": "global"}
        }),
    )
    .await
    .expect("recipe_run");
    let body: serde_json::Value = serde_json::from_str(&first_text(&ran)).expect("run json");
    assert_eq!(
        body["wait_exhausted"], true,
        "the job must still outlive wait_ms: {body}"
    );
    assert_eq!(
        body["complete"], false,
        "wait_exhausted implies incomplete: {body}"
    );
    assert_eq!(
        body["signals_capped"], false,
        "no matches were produced, only the deadline hit: {body}"
    );
    assert!(
        body["recover_hint"].as_str().is_some(),
        "a deadline-exhausted result must carry a recover_hint: {body}"
    );

    let _ = client.cancel().await;
    handle.shutdown().await;
    let _ = std::fs::remove_dir_all(&data);
}
