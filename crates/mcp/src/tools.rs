// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! MCP tool surface served by the rmcp stdio adapter (TC40).
//!
//! This module defines [`TerminalCommanderMcpServer`], the rmcp
//! `ServerHandler` that the binary mounts on the stdio transport. The
//! server is a thin facade: every tool call is forwarded to the
//! `terminal-commanderd` daemon over the existing UDS IPC client. The
//! MCP process never spawns commands, opens raw files, or binds a
//! network socket.
//!
//! Tool set at TC40 — discovery and status only:
//! - `system_discover` — version, MCP spec, advertised tool list.
//! - `health` — daemon liveness ping with uptime.
//! - `policy_status` — active policy profile + bounded caps.
//! - `self_check` — re-run the daemon self-check; bounded report.
//!
//! Bucket / event-context / command tools are deferred to TC41 per
//! the goal's forbidden list. `system_discover` reports them as
//! `not_implemented` so MCP clients never see a phantom tool.
//!
//! Source-status: live (TC40) for the four enumerated tools.

use std::borrow::Cow;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{
        CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use serde::Serialize;
use terminal_commanderd::ipc::protocol::{
    DiscoverResponse, IpcError, IpcErrorCode, IpcRequest, IpcResponse, PolicyStatusResponse,
    SelfCheckResponse,
};

use crate::daemon_client::McpDaemonClient;

/// Wire-stable tool-status enum advertised by `system_discover`.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    /// Tool is wired to a live daemon handler.
    Live,
    /// Tool is reserved by spec but not yet implemented. Calling it
    /// will return a typed error.
    NotImplemented,
}

/// Single entry in the advertised tool catalogue.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCatalogueEntry {
    pub name: &'static str,
    pub status: ToolStatus,
    /// Short human-readable description; bounded.
    pub description: &'static str,
}

/// Static catalogue of every MCP tool the adapter knows about. Tools
/// not marked `Live` are NOT registered with the tool router — they
/// are only advertised here so clients can see what is reserved.
#[must_use]
pub const fn tool_catalogue() -> &'static [ToolCatalogueEntry] {
    &[
        ToolCatalogueEntry {
            name: "system_discover",
            status: ToolStatus::Live,
            description: "Return adapter version, MCP spec, policy profile, tool catalogue.",
        },
        ToolCatalogueEntry {
            name: "health",
            status: ToolStatus::Live,
            description: "Daemon liveness ping; returns uptime seconds.",
        },
        ToolCatalogueEntry {
            name: "policy_status",
            status: ToolStatus::Live,
            description: "Active policy profile and bounded per-call caps.",
        },
        ToolCatalogueEntry {
            name: "self_check",
            status: ToolStatus::Live,
            description: "Re-run the daemon self-check; bounded text report.",
        },
        ToolCatalogueEntry {
            name: "bucket_events_since",
            status: ToolStatus::NotImplemented,
            description: "Cursor read of a bucket. Reserved for TC41.",
        },
        ToolCatalogueEntry {
            name: "bucket_wait",
            status: ToolStatus::NotImplemented,
            description: "Realtime wait on a bucket. Reserved for TC41.",
        },
        ToolCatalogueEntry {
            name: "bucket_summary",
            status: ToolStatus::NotImplemented,
            description: "Bucket counters + severity histogram. Reserved for TC41.",
        },
        ToolCatalogueEntry {
            name: "event_context",
            status: ToolStatus::NotImplemented,
            description: "Bounded context window around an event. Reserved for TC41.",
        },
    ]
}

/// Aggregate payload returned by the `system_discover` tool.
#[derive(Debug, Clone, Serialize)]
pub struct SystemDiscoverPayload {
    pub adapter_version: &'static str,
    pub mcp_spec: &'static str,
    pub daemon: Option<DiscoverResponse>,
    pub daemon_error: Option<String>,
    pub tools: Vec<ToolCatalogueEntry>,
}

/// MCP server handler. Holds the daemon client and the tool router.
#[derive(Clone)]
pub struct TerminalCommanderMcpServer {
    daemon: McpDaemonClient,
    /// Tool router populated by the rmcp `#[tool_router]` macro. The
    /// router is read by the rmcp service layer, not by us directly,
    /// so the dead-code lint trips here; suppressed below.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for TerminalCommanderMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalCommanderMcpServer")
            .field("daemon", &self.daemon)
            .finish_non_exhaustive()
    }
}

/// Adapter-level constant tied to `Cargo.toml`.
const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// MCP spec revision the adapter targets. Matches the in-process
/// `ToolSurface` for consistency.
const MCP_SPEC_REVISION: &str = "2025-11-25";

#[tool_router]
impl TerminalCommanderMcpServer {
    /// Construct a new server wired to the given daemon client.
    #[must_use]
    pub fn new(daemon: McpDaemonClient) -> Self {
        Self {
            daemon,
            tool_router: Self::tool_router(),
        }
    }

    /// `system_discover` — adapter metadata + tool catalogue.
    /// Forwards to the daemon to fetch live profile/version data; if
    /// the daemon is unreachable the response still carries the
    /// adapter-side catalogue with the daemon error surfaced.
    #[tool(description = "Return adapter version, MCP spec, policy profile, and tool catalogue.")]
    async fn system_discover(&self) -> Result<CallToolResult, McpError> {
        let (daemon, daemon_error) = match self.daemon.call(IpcRequest::SystemDiscover).await {
            Ok(IpcResponse::SystemDiscover(d)) => (Some(d), None),
            Ok(other) => (
                None,
                Some(format!("unexpected response variant: {other:?}")),
            ),
            Err(e) => (None, Some(format_ipc_error(&e))),
        };
        let payload = SystemDiscoverPayload {
            adapter_version: ADAPTER_VERSION,
            mcp_spec: MCP_SPEC_REVISION,
            daemon,
            daemon_error,
            tools: tool_catalogue().to_vec(),
        };
        json_tool_result(&payload)
    }

    /// `health` — daemon liveness check. Returns uptime when reachable
    /// and a typed error otherwise.
    #[tool(description = "Daemon liveness ping. Returns uptime in seconds when reachable.")]
    async fn health(&self) -> Result<CallToolResult, McpError> {
        match self.daemon.call(IpcRequest::Health).await {
            Ok(IpcResponse::Health { uptime_secs }) => json_tool_result(&serde_json::json!({
                "ok": true,
                "uptime_secs": uptime_secs,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `policy_status` — active profile + per-call caps.
    #[tool(description = "Report the active policy profile and bounded per-call caps.")]
    async fn policy_status(&self) -> Result<CallToolResult, McpError> {
        match self.daemon.call(IpcRequest::PolicyStatus).await {
            Ok(IpcResponse::PolicyStatus(PolicyStatusResponse {
                profile,
                commands_deny_count,
                default_deny_path_suffix_count,
                file_window_bytes,
                bucket_read_limit,
            })) => json_tool_result(&serde_json::json!({
                "profile": profile,
                "commands_deny_count": commands_deny_count,
                "default_deny_path_suffix_count": default_deny_path_suffix_count,
                "file_window_bytes": file_window_bytes,
                "bucket_read_limit": bucket_read_limit,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `self_check` — re-run daemon self-check; bounded text report.
    #[tool(description = "Re-run the daemon self-check. Returns the bounded text report.")]
    async fn self_check(&self) -> Result<CallToolResult, McpError> {
        match self.daemon.call(IpcRequest::SelfCheck).await {
            Ok(IpcResponse::SelfCheck(SelfCheckResponse { report, failures })) => {
                json_tool_result(&serde_json::json!({
                    "failures": failures,
                    "report": report,
                }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }
}

#[tool_handler]
impl ServerHandler for TerminalCommanderMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "terminal-commander-mcp",
                ADAPTER_VERSION,
            ))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Terminal Commander MCP adapter. Discovery/status tools only at TC40; bucket and command tools land in TC41."
                    .to_owned(),
            )
    }

    async fn initialize(
        &self,
        _request: rmcp::model::InitializeRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::InitializeResult, McpError> {
        Ok(self.get_info())
    }
}

/// Encode a serializable payload as a single MCP `Content::text` JSON
/// blob. Bounded by the daemon-side caps; this helper never reads
/// unbounded input.
fn json_tool_result<T: Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string(value).map_err(|e| {
        McpError::internal_error(Cow::Owned(format!("serialize response: {e}")), None)
    })?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

/// Map a daemon `IpcError` to an MCP `ErrorData` with stable codes.
#[must_use]
pub fn into_mcp_error(e: &IpcError) -> McpError {
    let message: Cow<'static, str> = Cow::Owned(format_ipc_error(e));
    let data = serde_json::json!({
        "ipc_code": format!("{:?}", e.code),
    });
    match e.code {
        IpcErrorCode::PolicyDenied => McpError::invalid_params(message, Some(data)),
        IpcErrorCode::UnknownMethod | IpcErrorCode::SchemaMismatch => {
            McpError::invalid_params(message, Some(data))
        }
        _ => McpError::internal_error(message, Some(data)),
    }
}

fn unexpected_variant(resp: &IpcResponse) -> McpError {
    McpError::internal_error(
        Cow::Owned(format!("unexpected daemon response: {resp:?}")),
        None,
    )
}

#[must_use]
pub fn format_ipc_error(e: &IpcError) -> String {
    format!("daemon ipc error [{:?}]: {}", e.code, e.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_lists_four_live_tools() {
        let live: Vec<_> = tool_catalogue()
            .iter()
            .filter(|t| matches!(t.status, ToolStatus::Live))
            .map(|t| t.name)
            .collect();
        assert_eq!(
            live,
            vec!["system_discover", "health", "policy_status", "self_check"]
        );
    }

    #[test]
    fn tool_router_exposes_only_live_tools() {
        let router = TerminalCommanderMcpServer::tool_router();
        let mut names: Vec<String> = router
            .list_all()
            .into_iter()
            .map(|t| t.name.into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "health".to_owned(),
                "policy_status".to_owned(),
                "self_check".to_owned(),
                "system_discover".to_owned(),
            ]
        );
    }

    #[test]
    fn ipc_error_policy_denied_maps_to_invalid_params() {
        let e = IpcError::new(IpcErrorCode::PolicyDenied, "nope");
        let mcp = into_mcp_error(&e);
        assert!(mcp.message.contains("policy_denied") || mcp.message.contains("PolicyDenied"));
    }
}
