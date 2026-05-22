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
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use terminal_commander_core::{BucketConfig, RuleDefinition, Severity};
use terminal_commanderd::ipc::protocol::{
    BucketEventsSinceParams, BucketEventsSinceResponse, BucketSummaryParams, BucketSummaryResponse,
    BucketWaitParams, BucketWaitResponse, CommandStartParams, CommandStartResponse,
    CommandStatusParams, CommandStatusResponse, ContextUnavailableReason, DiscoverResponse,
    EventContextParams, EventContextResponse, IpcContextFrame, IpcError, IpcErrorCode, IpcRequest,
    IpcResponse, PolicyStatusResponse, SelfCheckResponse,
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
            name: "command_start_combed",
            status: ToolStatus::Live,
            description: "Start a non-PTY argv command; bounded metadata response. No raw stdout/stderr.",
        },
        ToolCatalogueEntry {
            name: "command_status",
            status: ToolStatus::Live,
            description: "Lifecycle + counters lookup for a previously started job.",
        },
        ToolCatalogueEntry {
            name: "bucket_events_since",
            status: ToolStatus::Live,
            description: "Cursor read of a bucket. Bounded; severity / kind filters supported.",
        },
        ToolCatalogueEntry {
            name: "bucket_wait",
            status: ToolStatus::Live,
            description: "Realtime wait on a bucket. Heartbeat returned on timeout.",
        },
        ToolCatalogueEntry {
            name: "bucket_summary",
            status: ToolStatus::Live,
            description: "Bucket counters + severity histogram. No raw stream content.",
        },
        ToolCatalogueEntry {
            name: "event_context",
            status: ToolStatus::Live,
            description: "Bounded context window around an event. Pointer-based.",
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

    /// `command_start_combed` — start a non-PTY argv command on the
    /// daemon and return bounded metadata. Never returns raw output.
    #[tool(
        description = "Start a non-PTY argv command. Returns job_id, bucket_id, probe_id, initial cursor. No stdout/stderr text is returned. Shell interpreters are denied by default."
    )]
    async fn command_start_combed(
        &self,
        Parameters(params): Parameters<McpCommandStartParams>,
    ) -> Result<CallToolResult, McpError> {
        let ipc = params.into_ipc();
        match self.daemon.call(IpcRequest::CommandStartCombed(ipc)).await {
            Ok(IpcResponse::CommandStartCombed(CommandStartResponse {
                job_id,
                bucket_id,
                probe_id,
                cursor,
            })) => json_tool_result(&serde_json::json!({
                "job_id": job_id,
                "bucket_id": bucket_id,
                "probe_id": probe_id,
                "cursor": cursor,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `command_status` — lifecycle counters + exit info for a job.
    #[tool(
        description = "Lookup lifecycle counters and exit info for a previously started job. Bounded; never returns raw output."
    )]
    async fn command_status(
        &self,
        Parameters(params): Parameters<McpCommandStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let job_id = parse_id::<terminal_commander_core::ids::JobIdKind>("job_id", &params.job_id)
            .map_err(invalid_params)?;
        let ipc = CommandStatusParams { job_id };
        match self.daemon.call(IpcRequest::CommandStatus(ipc)).await {
            Ok(IpcResponse::CommandStatus(s)) => json_tool_result(&command_status_payload(&s)),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `bucket_events_since` — cursor read; bounded; severity / kind
    /// filters supported.
    #[tool(
        description = "Cursor-based read of bucket events. Bounded by daemon caps. Filters: severity_min, kind_filter."
    )]
    async fn bucket_events_since(
        &self,
        Parameters(params): Parameters<McpBucketEventsSinceParams>,
    ) -> Result<CallToolResult, McpError> {
        let ipc = params.into_ipc().map_err(invalid_params)?;
        match self.daemon.call(IpcRequest::BucketEventsSince(ipc)).await {
            Ok(IpcResponse::BucketEventsSince(r)) => json_tool_result(&bucket_events_payload(&r)),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `bucket_wait` — realtime wait; returns heartbeat on timeout.
    #[tool(
        description = "Realtime wait on a bucket. Returns a heartbeat when the timeout expires without matching events. Bounded by daemon caps."
    )]
    async fn bucket_wait(
        &self,
        Parameters(params): Parameters<McpBucketWaitParams>,
    ) -> Result<CallToolResult, McpError> {
        let ipc = params.into_ipc().map_err(invalid_params)?;
        match self.daemon.call(IpcRequest::BucketWait(ipc)).await {
            Ok(IpcResponse::BucketWait(r)) => json_tool_result(&bucket_wait_payload(&r)),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `bucket_summary` — counters + severity histogram.
    #[tool(
        description = "Bucket counters and severity histogram. No raw stream content is returned."
    )]
    async fn bucket_summary(
        &self,
        Parameters(params): Parameters<McpBucketSummaryParams>,
    ) -> Result<CallToolResult, McpError> {
        let bucket_id =
            parse_id::<terminal_commander_core::ids::BucketIdKind>("bucket_id", &params.bucket_id)
                .map_err(invalid_params)?;
        let ipc = BucketSummaryParams { bucket_id };
        match self.daemon.call(IpcRequest::BucketSummary(ipc)).await {
            Ok(IpcResponse::BucketSummary(s)) => json_tool_result(&bucket_summary_payload(&s)),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `event_context` — pointer-bounded context around an event.
    #[tool(
        description = "Bounded context window around an event. Returns frames or a typed unavailable_reason when no pointer exists. Pointer-based; never streams."
    )]
    async fn event_context(
        &self,
        Parameters(params): Parameters<McpEventContextParams>,
    ) -> Result<CallToolResult, McpError> {
        let ipc = params.into_ipc().map_err(invalid_params)?;
        match self.daemon.call(IpcRequest::EventContext(ipc)).await {
            Ok(IpcResponse::EventContext(r)) => json_tool_result(&event_context_payload(&r)),
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

/// Build an MCP `invalid_params` error from a free-form reason. Used
/// when an MCP tool input fails wire-form validation before any
/// daemon call.
fn invalid_params(reason: String) -> McpError {
    McpError::invalid_params(Cow::Owned(reason), None)
}

/// Parse a wire-form identifier from an MCP tool input field.
fn parse_id<K: terminal_commander_core::ids::TypedIdKind>(
    field: &str,
    s: &str,
) -> Result<terminal_commander_core::ids::TypedId<K>, String> {
    terminal_commander_core::ids::TypedId::<K>::parse_wire(s).map_err(|e| format!("{field}: {e}"))
}

fn parse_severity_filter(s: &str) -> Result<Severity, String> {
    match s {
        "trace" => Ok(Severity::Trace),
        "debug" => Ok(Severity::Debug),
        "info" => Ok(Severity::Info),
        "low" => Ok(Severity::Low),
        "medium" => Ok(Severity::Medium),
        "high" => Ok(Severity::High),
        "critical" => Ok(Severity::Critical),
        other => Err(format!(
            "severity_min '{other}' is not one of trace|debug|info|low|medium|high|critical"
        )),
    }
}

/// Env entry pair for the MCP wire form. Avoids relying on
/// `JsonSchema` for a tuple struct.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
}

/// MCP-facing parameters for `command_start_combed`. Strings + ints
/// only so the JSON Schema stays consumer-friendly. Translated to the
/// daemon-side `CommandStartParams` in `into_ipc`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpCommandStartParams {
    /// Non-empty argv. argv[0] is the program; rest are args.
    /// Shell interpreters are rejected by the daemon.
    pub argv: Vec<String>,
    /// Optional working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Optional explicit environment. Empty = inherit.
    #[serde(default)]
    pub env: Vec<EnvEntry>,
    /// Optional grace window between graceful and forced terminate,
    /// in milliseconds. Clamped at the daemon.
    #[serde(default)]
    pub grace_ms: Option<u64>,
}

impl McpCommandStartParams {
    fn into_ipc(self) -> CommandStartParams {
        let cwd = self.cwd.map(std::path::PathBuf::from);
        let env: Vec<(String, String)> = self.env.into_iter().map(|e| (e.key, e.value)).collect();
        CommandStartParams {
            argv: self.argv,
            cwd,
            env,
            bucket_config: None::<BucketConfig>,
            rules: Vec::<RuleDefinition>::new(),
            grace_ms: self.grace_ms,
        }
    }
}

/// MCP-facing parameters for `command_status`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpCommandStatusParams {
    /// Job id returned by `command_start_combed`.
    pub job_id: String,
}

/// MCP-facing parameters for `bucket_events_since`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpBucketEventsSinceParams {
    pub bucket_id: String,
    pub cursor: u64,
    /// Lowercase severity name: trace|debug|info|low|medium|high|critical.
    #[serde(default)]
    pub severity_min: Option<String>,
    #[serde(default)]
    pub kind_filter: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

impl McpBucketEventsSinceParams {
    fn into_ipc(self) -> Result<BucketEventsSinceParams, String> {
        let bucket_id =
            parse_id::<terminal_commander_core::ids::BucketIdKind>("bucket_id", &self.bucket_id)?;
        let severity_min = match self.severity_min {
            Some(s) => Some(parse_severity_filter(&s)?),
            None => None,
        };
        Ok(BucketEventsSinceParams {
            bucket_id,
            cursor: self.cursor,
            severity_min,
            kind_filter: self.kind_filter,
            limit: self.limit,
        })
    }
}

/// MCP-facing parameters for `bucket_wait`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpBucketWaitParams {
    pub bucket_id: String,
    pub cursor: u64,
    #[serde(default)]
    pub severity_min: Option<String>,
    #[serde(default)]
    pub kind_filter: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    /// Wait timeout in milliseconds. Clamped at the daemon.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl McpBucketWaitParams {
    fn into_ipc(self) -> Result<BucketWaitParams, String> {
        let bucket_id =
            parse_id::<terminal_commander_core::ids::BucketIdKind>("bucket_id", &self.bucket_id)?;
        let severity_min = match self.severity_min {
            Some(s) => Some(parse_severity_filter(&s)?),
            None => None,
        };
        Ok(BucketWaitParams {
            bucket_id,
            cursor: self.cursor,
            severity_min,
            kind_filter: self.kind_filter,
            limit: self.limit,
            timeout_ms: self.timeout_ms,
        })
    }
}

/// MCP-facing parameters for `bucket_summary`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpBucketSummaryParams {
    pub bucket_id: String,
}

/// MCP-facing parameters for `event_context`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpEventContextParams {
    pub bucket_id: String,
    pub event_id: String,
    #[serde(default)]
    pub before: Option<u32>,
    #[serde(default)]
    pub after: Option<u32>,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

impl McpEventContextParams {
    fn into_ipc(self) -> Result<EventContextParams, String> {
        let bucket_id =
            parse_id::<terminal_commander_core::ids::BucketIdKind>("bucket_id", &self.bucket_id)?;
        let event_id =
            parse_id::<terminal_commander_core::ids::EventIdKind>("event_id", &self.event_id)?;
        Ok(EventContextParams {
            bucket_id,
            event_id,
            before: self.before,
            after: self.after,
            max_bytes: self.max_bytes,
        })
    }
}

fn command_status_payload(s: &CommandStatusResponse) -> serde_json::Value {
    serde_json::json!({
        "job_id": s.job_id,
        "bucket_id": s.bucket_id,
        "probe_id": s.probe_id,
        "state": s.state,
        "frames_total": s.frames_total,
        "frames_stdout": s.frames_stdout,
        "frames_stderr": s.frames_stderr,
        "bytes_total": s.bytes_total,
        "events_emitted": s.events_emitted,
        "exit_code": s.exit_code,
        "signal": s.signal,
        "duration_ms": s.duration_ms,
    })
}

fn bucket_events_payload(r: &BucketEventsSinceResponse) -> serde_json::Value {
    serde_json::json!({
        "bucket_id": r.bucket_id,
        "cursor_in": r.cursor_in,
        "next_cursor": r.next_cursor,
        "has_more": r.has_more,
        "dropped_count": r.dropped_count,
        "events": r.events,
    })
}

fn bucket_wait_payload(r: &BucketWaitResponse) -> serde_json::Value {
    serde_json::json!({
        "bucket_id": r.bucket_id,
        "cursor_in": r.cursor_in,
        "next_cursor": r.next_cursor,
        "heartbeat": r.heartbeat,
        "dropped_count": r.dropped_count,
        "events": r.events,
    })
}

fn bucket_summary_payload(s: &BucketSummaryResponse) -> serde_json::Value {
    serde_json::json!({
        "bucket_id": s.bucket_id,
        "head_seq": s.head_seq,
        "tail_seq": s.tail_seq,
        "event_count": s.event_count,
        "dropped_count": s.dropped_count,
        "by_severity": s.by_severity,
    })
}

fn event_context_payload(r: &EventContextResponse) -> serde_json::Value {
    let frames: Vec<serde_json::Value> = r
        .frames
        .iter()
        .map(|f: &IpcContextFrame| {
            serde_json::json!({
                "probe_id": f.probe_id,
                "frame_id": f.frame_id,
                "stream": f.stream,
                "line": f.line,
                "text": f.text,
            })
        })
        .collect();
    let unavail = r.unavailable_reason.map(unavailable_reason_str);
    serde_json::json!({
        "bucket_id": r.bucket_id,
        "event_id": r.event_id,
        "anchor_missing": r.anchor_missing,
        "unavailable_reason": unavail,
        "pointer_unavailable_reason": r.pointer_unavailable_reason,
        "frames": frames,
        "total_bytes": r.total_bytes,
        "truncated": r.truncated,
    })
}

const fn unavailable_reason_str(r: ContextUnavailableReason) -> &'static str {
    match r {
        ContextUnavailableReason::NoPointer => "no_pointer",
        ContextUnavailableReason::SyntheticEvent => "synthetic_event",
        ContextUnavailableReason::AnchorEvicted => "anchor_evicted",
        ContextUnavailableReason::UnknownProbe => "unknown_probe",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_lists_ten_live_tools_at_tc41() {
        let live: Vec<_> = tool_catalogue()
            .iter()
            .filter(|t| matches!(t.status, ToolStatus::Live))
            .map(|t| t.name)
            .collect();
        assert_eq!(
            live,
            vec![
                "system_discover",
                "health",
                "policy_status",
                "self_check",
                "command_start_combed",
                "command_status",
                "bucket_events_since",
                "bucket_wait",
                "bucket_summary",
                "event_context",
            ]
        );
        let not_impl: Vec<_> = tool_catalogue()
            .iter()
            .filter(|t| matches!(t.status, ToolStatus::NotImplemented))
            .map(|t| t.name)
            .collect();
        assert!(
            not_impl.is_empty(),
            "TC41 promotes every TC40-deferred entry to live; expected no not_implemented, got: {not_impl:?}"
        );
    }

    #[test]
    fn tool_router_exposes_all_live_tools() {
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
                "bucket_events_since".to_owned(),
                "bucket_summary".to_owned(),
                "bucket_wait".to_owned(),
                "command_start_combed".to_owned(),
                "command_status".to_owned(),
                "event_context".to_owned(),
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
