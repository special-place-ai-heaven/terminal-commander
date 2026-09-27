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
    RecipeUpsertParams, ServerHandle,
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

fn spawn_daemon(data: &std::path::Path) -> ServerHandle {
    let cfg = DaemonConfig::defaults_in(data);
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"));
    IpcServer::new(Arc::clone(&state), state.config.socket_path())
        .spawn()
        .expect("ipc spawn")
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
    let handle = spawn_daemon(&data);
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

    let admin =
        DaemonClient::new(handle.socket_path().to_path_buf()).with_timeout(Duration::from_secs(5));
    admin
        .call(
            1,
            IpcRequest::RecipeUpsert(RecipeUpsertParams {
                definition: watched_true(),
            }),
        )
        .await
        .ok();
    admin
        .call(
            2,
            IpcRequest::RecipeActivate(RecipeActivateParams {
                recipe_id: "echo.true".to_owned(),
                version: Some(1),
                scope: Some(ActivationScope::Global),
                from_mcp: false,
            }),
        )
        .await
        .expect("admin activate");

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
