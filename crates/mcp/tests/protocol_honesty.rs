// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! MCP advertisement is one modern revision: `2026-07-28`.
//!
//! `get_info`, negotiation, and `system_discover.mcp_spec` agree on
//! `2026-07-28`. Any other opener does not connect.

use std::time::Duration;

use rmcp::model::{CallToolRequestParams, ProtocolVersion};
use rmcp::service::{ClientInitializeError, ServerInitializeError};
use rmcp::{ClientHandler, ClientLifecycleMode, ClientServiceExt, ServerHandler, ServiceExt};

use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;

#[derive(Debug, Default, Clone)]
struct TestClient;

impl ClientHandler for TestClient {}

fn server() -> TerminalCommanderMcpServer {
    TerminalCommanderMcpServer::new(
        McpDaemonClient::new(std::env::temp_dir().join("tc-mcp-protocol-honesty-absent"))
            .with_timeout(Duration::from_millis(50)),
    )
}

fn first_text(result: &rmcp::model::CallToolResult) -> String {
    for item in &result.content {
        if let Some(text) = item.as_text() {
            return text.text.clone();
        }
    }
    panic!("expected text content in call result");
}

#[test]
fn get_info_advertises_only_2026_07_28() {
    let server = server();
    let info = server.get_info();
    assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
    assert_eq!(
        ServerHandler::supported_protocol_versions(&server).as_ref(),
        &[ProtocolVersion::V_2026_07_28]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discover_negotiates_2026_07_28_and_system_discover_matches() {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let mcp = server();
    let server_task =
        tokio::spawn(async move { mcp.serve(server_transport).await.expect("server serve") });
    let client = TestClient
        .serve_with_lifecycle(
            client_transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("modern discover");
    let server = server_task.await.expect("server task join");

    let info = client.peer_info().expect("peer info");
    assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
    assert_eq!(
        info.server_info
            .as_ref()
            .expect("discover advertises server info")
            .name,
        "terminal-commander-mcp"
    );

    let result = client
        .call_tool(CallToolRequestParams::new("system_discover"))
        .await
        .expect("system_discover");
    let payload: serde_json::Value =
        serde_json::from_str(&first_text(&result)).expect("system_discover json");
    assert_eq!(payload["mcp_spec"], "2026-07-28");

    let _ = client.cancel().await;
    let _ = server.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_tools_emits_sep2549_ttl_ms_and_cache_scope() {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let mcp = server();
    let server_task =
        tokio::spawn(async move { mcp.serve(server_transport).await.expect("server serve") });
    let client = TestClient
        .serve_with_lifecycle(
            client_transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("modern discover");
    let server = server_task.await.expect("server task join");

    let listed = client.list_tools(None).await.expect("tools/list");
    // Re-serialize: `skip_serializing_if` drops `None`, so a missing hint fails here.
    let wire = serde_json::to_value(&listed).expect("tools/list json");
    let ttl_ms = wire
        .get("ttlMs")
        .and_then(serde_json::Value::as_u64)
        .expect("ttlMs must be a JSON number >= 0");
    assert_eq!(ttl_ms, 0);
    assert_eq!(
        wire.get("cacheScope").and_then(serde_json::Value::as_str),
        Some("public")
    );
    assert!(wire.get("tools").is_some_and(serde_json::Value::is_array));
    let names: Vec<&str> = wire["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str))
        .collect();
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
        assert!(
            names.contains(&required),
            "full list_tools must advertise {required}"
        );
    }
    assert!(
        !names.contains(&"recipe"),
        "compact facade `recipe` must stay off the full list_tools surface"
    );

    let _ = client.cancel().await;
    let _ = server.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_initialize_is_rejected() {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let mcp = server();
    let server_task = tokio::spawn(async move { mcp.serve(server_transport).await });
    let err = TestClient
        .serve(client_transport)
        .await
        .expect_err("legacy initialize must fail");
    match err {
        ClientInitializeError::JsonRpcError(data) => {
            assert_eq!(data.message.as_ref(), "Unsupported protocol version");
            let supported = data
                .data
                .as_ref()
                .and_then(|value| value.get("supported"))
                .and_then(|value| value.as_array())
                .expect("supported versions in error data");
            assert_eq!(
                supported.as_slice(),
                [serde_json::json!("2026-07-28")].as_slice()
            );
        }
        other => panic!("expected unsupported protocol version, got {other}"),
    }
    match server_task.await.expect("server task join") {
        Err(ServerInitializeError::InitializeFailed(data)) => {
            assert_eq!(data.message.as_ref(), "Unsupported protocol version");
        }
        other => panic!("expected initialize failure, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discover_refuses_older_preferred_version() {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let mcp = server();
    let server_task = tokio::spawn(async move { mcp.serve(server_transport).await });
    let err = TestClient
        .serve_with_lifecycle(
            client_transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2025_11_25],
            },
        )
        .await
        .expect_err("2025-11-25 must not negotiate");
    match err {
        ClientInitializeError::NoCompatibleProtocolVersion {
            client_supported,
            server_supported,
        } => {
            assert_eq!(client_supported, vec![ProtocolVersion::V_2025_11_25]);
            assert_eq!(server_supported, vec![ProtocolVersion::V_2026_07_28]);
        }
        other => panic!("expected no compatible protocol version, got {other}"),
    }
    let running = server_task
        .await
        .expect("server task join")
        .expect("a modern opener is accepted before version selection fails");
    let _ = running.cancel().await;
}
