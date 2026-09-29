// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! MCP advertises `2026-07-28` and accepts older client revisions.
//!
//! Cursor and OMP still open with legacy `initialize`; Codex can opt into
//! the modern revision. Both paths must reach the same tools.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, ProtocolVersion};
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
fn get_info_advertises_modern_and_supports_legacy() {
    let server = server();
    let info = server.get_info();
    assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
    assert_eq!(
        ServerHandler::supported_protocol_versions(&server).as_ref(),
        &[
            ProtocolVersion::V_2026_07_28,
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2025_06_18,
        ]
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
async fn legacy_initialize_connects_and_calls_tools() {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let mcp = server();
    let server_task =
        tokio::spawn(async move { mcp.serve(server_transport).await.expect("server serve") });
    let client = TestClient
        .serve(client_transport)
        .await
        .expect("legacy initialize");
    let server = server_task.await.expect("server task join");

    assert_eq!(
        client.peer_info().expect("peer info").protocol_version,
        ProtocolVersion::V_2025_11_25
    );
    let result = client
        .call_tool(CallToolRequestParams::new("system_discover"))
        .await
        .expect("legacy tool call");
    assert!(!first_text(&result).is_empty());

    let _ = client.cancel().await;
    let _ = server.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discover_accepts_older_preferred_version() {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let mcp = server();
    let server_task =
        tokio::spawn(async move { mcp.serve(server_transport).await.expect("server serve") });
    let client = TestClient
        .serve_with_lifecycle(
            client_transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2025_11_25],
            },
        )
        .await
        .expect("legacy preferred version");
    let server = server_task.await.expect("server task join");
    assert_eq!(
        client.peer_info().expect("peer info").protocol_version,
        ProtocolVersion::V_2025_11_25
    );
    let _ = client.cancel().await;
    let _ = server.cancel().await;
}

/// Release presmoke/verify jobs and smoke scripts hand-roll the MCP opener
/// instead of using rmcp's client. v0.2.0's release died at presmoke because
/// they still sent a legacy `initialize`. Every `protocolVersion` they send
/// must be one this server supports, and none may open with `initialize`.
#[test]
fn raw_wire_drivers_speak_a_supported_revision() {
    let supported: Vec<String> = ServerHandler::supported_protocol_versions(&server())
        .iter()
        .map(ToString::to_string)
        .collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in [".github", "scripts"] {
        collect_files(&root.join(dir), &mut files);
    }
    let mut pinned = 0;
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = path.strip_prefix(&root).unwrap_or(&path).display();
        assert!(
            !text.contains("\"initialize\"") && !text.contains("initialize\\\""),
            "{rel}: opens with a legacy `initialize`; use `server/discover` + `_meta`"
        );
        for (at, _) in text.match_indices("protocolVersion") {
            let tail: String = text[at..].chars().take(48).collect();
            if let Some(version) = first_date(&tail) {
                assert!(
                    supported.contains(&version),
                    "{rel}: sends protocolVersion {version}; server supports {supported:?}"
                );
                pinned += 1;
            }
        }
    }
    assert!(
        pinned > 0,
        "found no raw-wire driver; the guard scans the wrong tree"
    );
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read_dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// First `YYYY-MM-DD` in `s`.
fn first_date(s: &str) -> Option<String> {
    s.as_bytes().windows(10).find_map(|w| {
        let is_date = w.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        });
        is_date.then(|| String::from_utf8_lossy(w).into_owned())
    })
}
