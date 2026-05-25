// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

// These lints fire on the repetitive daemon-status guard pattern and
// inline `use` items in tool handlers.  The patterns are intentional
// (guard must appear before each IPC call; `use` is scoped to keep
// type names out of the module top-level).  Suppress rather than
// restructure, per Task 3.5.5 "do not change tool implementations".
#![allow(clippy::collapsible_if, clippy::items_after_statements)]

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
    EventContextParams, EventContextResponse, FileReadWindowParams, FileReadWindowResponse,
    FileSearchParams, FileSearchResponse, FileWatchListResponse, FileWatchStartParams,
    FileWatchStartResponse, FileWatchStopParams, FileWatchStopResponse, IpcContextFrame, IpcError,
    IpcErrorCode, IpcRequest, IpcResponse, PolicyStatusResponse, ProbeListResponse,
    ProbeStatusParams, ProbeStatusResponse, PtyCommandListResponse, PtyCommandStartParams,
    PtyCommandStartResponse, PtyCommandStopParams, PtyCommandStopResponse,
    PtyCommandWriteStdinParams, PtyCommandWriteStdinResponse, RegistryActivateParams,
    RegistryActivateResponse, RegistryDeactivateParams, RegistryDeactivateResponse,
    RegistryGetParams, RegistryGetResponse, RegistryListActiveResponse, RegistrySearchParams,
    RegistrySearchResponse, RegistryTestParams, RegistryTestResponse, RegistryTestSample,
    RegistryUpsertParams, RegistryUpsertResponse, RuntimeStateResponse, SelfCheckResponse,
};

use crate::daemon_client::McpDaemonClient;
use terminal_commander_supervisor::ensure::EnsureDaemonStatus;

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

/// Tool entry returned by `system_discover`. This wraps the static
/// catalogue with current runtime availability so clients do not have
/// to learn daemon reachability by trial-and-error tool calls.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredToolEntry {
    pub name: &'static str,
    pub status: ToolStatus,
    pub description: &'static str,
    pub requires_daemon: bool,
    pub available: bool,
    pub unavailable_reason: Option<&'static str>,
}

/// Static catalogue of every MCP tool the adapter knows about. Tools
/// not marked `Live` are NOT registered with the tool router — they
/// are only advertised here so clients can see what is reserved.
#[allow(clippy::too_many_lines)] // flat catalogue
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
        ToolCatalogueEntry {
            name: "registry_search",
            status: ToolStatus::Live,
            description: "FTS search over persisted rule definitions. Bounded.",
        },
        ToolCatalogueEntry {
            name: "registry_get",
            status: ToolStatus::Live,
            description: "Fetch a rule definition by id and optional version.",
        },
        ToolCatalogueEntry {
            name: "registry_upsert",
            status: ToolStatus::Live,
            description: "Insert a new immutable (rule_id, version+1) row from a JSON definition.",
        },
        ToolCatalogueEntry {
            name: "registry_test",
            status: ToolStatus::Live,
            description: "Dry-run a rule against bounded samples; never persists, no raw stream lane.",
        },
        ToolCatalogueEntry {
            name: "registry_activate",
            status: ToolStatus::Live,
            description: "Activate (rule_id, version?) for every newly-started command.",
        },
        ToolCatalogueEntry {
            name: "registry_deactivate",
            status: ToolStatus::Live,
            description: "Deactivate (rule_id, version); future commands skip the rule.",
        },
        ToolCatalogueEntry {
            name: "registry_list_active",
            status: ToolStatus::Live,
            description: "Snapshot of every currently-active rule (id + version + severity).",
        },
        ToolCatalogueEntry {
            name: "file_read_window",
            status: ToolStatus::Live,
            description: "Bounded line/byte window read of one file. Never returns the whole file.",
        },
        ToolCatalogueEntry {
            name: "file_search",
            status: ToolStatus::Live,
            description: "Bounded substring search over one file. Returns structured match pointers + capped snippets.",
        },
        ToolCatalogueEntry {
            name: "file_watch_start",
            status: ToolStatus::Live,
            description: "Start a daemon-owned file watch bound to a bucket; emits signal only when scoped rules match.",
        },
        ToolCatalogueEntry {
            name: "file_watch_stop",
            status: ToolStatus::Live,
            description: "Stop a previously started file watch by watch_id.",
        },
        ToolCatalogueEntry {
            name: "file_watch_list",
            status: ToolStatus::Live,
            description: "Snapshot of every currently-live file watch.",
        },
        ToolCatalogueEntry {
            name: "pty_command_start",
            status: ToolStatus::Live,
            description: "Start an interactive non-shell argv command attached to a PTY. Bounded metadata only.",
        },
        ToolCatalogueEntry {
            name: "pty_command_write_stdin",
            status: ToolStatus::Live,
            description: "Write bounded stdin bytes to a running PTY job. Denied while a secret prompt is active.",
        },
        ToolCatalogueEntry {
            name: "pty_command_stop",
            status: ToolStatus::Live,
            description: "Stop a previously started PTY job by job_id. Returns final counters.",
        },
        ToolCatalogueEntry {
            name: "pty_command_list",
            status: ToolStatus::Live,
            description: "Snapshot of every currently-live PTY job (including secret_prompt_active).",
        },
        ToolCatalogueEntry {
            name: "runtime_state",
            status: ToolStatus::Live,
            description: "Bounded aggregate runtime snapshot: probes, buckets, active rule scopes.",
        },
        ToolCatalogueEntry {
            name: "probe_list",
            status: ToolStatus::Live,
            description: "Flat list of every live probe across all runtimes (command / pty / file watch).",
        },
        ToolCatalogueEntry {
            name: "probe_status",
            status: ToolStatus::Live,
            description: "Bounded per-probe lookup by probe_id. Returns UnknownProbe if not live.",
        },
    ]
}

#[must_use]
fn tool_requires_daemon(name: &str) -> bool {
    name != "system_discover"
}

#[must_use]
fn discovered_tools(daemon_available: bool) -> Vec<DiscoveredToolEntry> {
    tool_catalogue()
        .iter()
        .map(|tool| {
            let requires_daemon = tool_requires_daemon(tool.name);
            let implemented = matches!(tool.status, ToolStatus::Live);
            let available = implemented && (!requires_daemon || daemon_available);
            let unavailable_reason = if !implemented {
                Some("not_implemented")
            } else if requires_daemon && !daemon_available {
                Some("daemon_unavailable")
            } else {
                None
            };
            DiscoveredToolEntry {
                name: tool.name,
                status: tool.status,
                description: tool.description,
                requires_daemon,
                available,
                unavailable_reason,
            }
        })
        .collect()
}

/// Aggregate payload returned by the `system_discover` tool.
#[derive(Debug, Clone, Serialize)]
pub struct SystemDiscoverPayload {
    pub adapter_version: &'static str,
    pub mcp_spec: &'static str,
    pub daemon_available: bool,
    pub daemon: Option<DiscoverResponse>,
    pub daemon_error: Option<String>,
    pub tools: Vec<DiscoveredToolEntry>,
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

    fn unavailable_startup_daemon_error(&self) -> Option<McpError> {
        let status = self.daemon.status()?;
        status
            .is_unavailable()
            .then(|| daemon_unavailable_error(&status.current()))
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
        let daemon_available = daemon.is_some() && daemon_error.is_none();
        let payload = SystemDiscoverPayload {
            adapter_version: ADAPTER_VERSION,
            mcp_spec: MCP_SPEC_REVISION,
            daemon_available,
            daemon,
            daemon_error,
            tools: discovered_tools(daemon_available),
        };
        json_tool_result(&payload)
    }

    /// `health` — daemon liveness check. Returns uptime when reachable
    /// and a typed error otherwise.
    #[tool(description = "Daemon liveness ping. Returns uptime in seconds when reachable.")]
    async fn health(&self) -> Result<CallToolResult, McpError> {
        if let Some(error) = self.unavailable_startup_daemon_error() {
            return Err(error);
        }
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
        if let Some(error) = self.unavailable_startup_daemon_error() {
            return Err(error);
        }
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
        if let Some(error) = self.unavailable_startup_daemon_error() {
            return Err(error);
        }
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
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
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
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
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
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
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
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
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
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
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
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let ipc = params.into_ipc().map_err(invalid_params)?;
        match self.daemon.call(IpcRequest::EventContext(ipc)).await {
            Ok(IpcResponse::EventContext(r)) => json_tool_result(&event_context_payload(&r)),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `registry_search` — FTS over persisted rules.
    #[tool(
        description = "Search persisted rule definitions by free-text query. Returns id, version, event_kind, severity, status, tags, and summary template. Bounded."
    )]
    async fn registry_search(
        &self,
        Parameters(params): Parameters<McpRegistrySearchParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let ipc = RegistrySearchParams {
            query: params.query,
            limit: params.limit,
        };
        match self.daemon.call(IpcRequest::RegistrySearch(ipc)).await {
            Ok(IpcResponse::RegistrySearch(RegistrySearchResponse { hits })) => {
                json_tool_result(&serde_json::json!({ "hits": hits }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `registry_get` — fetch a rule by id (and optional version).
    #[tool(
        description = "Fetch the full rule definition by id. If version is omitted, returns the latest stored version."
    )]
    async fn registry_get(
        &self,
        Parameters(params): Parameters<McpRegistryGetParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let ipc = RegistryGetParams {
            rule_id: params.rule_id,
            version: params.version,
        };
        match self.daemon.call(IpcRequest::RegistryGet(ipc)).await {
            Ok(IpcResponse::RegistryGet(RegistryGetResponse { definition })) => {
                json_tool_result(&serde_json::json!({ "definition": definition }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `registry_upsert` — create a new immutable version from a JSON
    /// rule definition.
    #[tool(
        description = "Create a new immutable (rule_id, version+1) row from a JSON RuleDefinition string. Validates regex/keywords; existing versions are never mutated."
    )]
    async fn registry_upsert(
        &self,
        Parameters(params): Parameters<McpRegistryUpsertParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let definition: RuleDefinition = serde_json::from_str(&params.definition_json)
            .map_err(|e| invalid_params(format!("definition_json: {e}")))?;
        let ipc = RegistryUpsertParams { definition };
        match self.daemon.call(IpcRequest::RegistryUpsert(ipc)).await {
            Ok(IpcResponse::RegistryUpsert(RegistryUpsertResponse { rule_id, version })) => {
                json_tool_result(&serde_json::json!({
                    "rule_id": rule_id,
                    "version": version,
                }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `registry_test` — dry-run a rule against bounded sample texts.
    #[tool(
        description = "Evaluate a rule against bounded sample texts. Returns matches with severity/kind/summary/captures; never persists; never echoes the input back as raw stream output."
    )]
    async fn registry_test(
        &self,
        Parameters(params): Parameters<McpRegistryTestParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let mut samples: Vec<RegistryTestSample> = Vec::with_capacity(params.samples.len());
        for s in params.samples {
            let stream = match s.stream.as_deref() {
                None => None,
                Some(name) => Some(parse_source_stream(name).map_err(invalid_params)?),
            };
            samples.push(RegistryTestSample {
                text: s.text,
                stream,
            });
        }
        let ipc = RegistryTestParams {
            rule_id: params.rule_id,
            version: params.version,
            samples,
        };
        match self.daemon.call(IpcRequest::RegistryTest(ipc)).await {
            Ok(IpcResponse::RegistryTest(RegistryTestResponse {
                matches,
                truncated_bytes,
            })) => json_tool_result(&serde_json::json!({
                "matches": matches,
                "truncated_bytes": truncated_bytes,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `registry_activate` — activate a rule for every newly-started
    /// command.
    #[tool(
        description = "Activate (rule_id, version?) so every newly-started command uses the rule. Already-running commands are not hot-rebound."
    )]
    async fn registry_activate(
        &self,
        Parameters(params): Parameters<McpRegistryActivateParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let scope = match params.scope {
            Some(s) => Some(s.into_ipc_scope()?),
            None => None,
        };
        let ipc = RegistryActivateParams {
            rule_id: params.rule_id,
            version: params.version,
            scope,
        };
        match self.daemon.call(IpcRequest::RegistryActivate(ipc)).await {
            Ok(IpcResponse::RegistryActivate(RegistryActivateResponse {
                rule_id,
                version,
                was_already_active,
                scope,
                jobs_rebound,
            })) => json_tool_result(&serde_json::json!({
                "rule_id": rule_id,
                "version": version,
                "was_already_active": was_already_active,
                "scope": scope,
                "jobs_rebound": jobs_rebound,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `registry_deactivate` — remove a rule from the active set.
    #[tool(
        description = "Deactivate (rule_id, version). Future commands skip the rule; already-running commands keep the rules they were started with."
    )]
    async fn registry_deactivate(
        &self,
        Parameters(params): Parameters<McpRegistryDeactivateParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let scope = match params.scope {
            Some(s) => Some(s.into_ipc_scope()?),
            None => None,
        };
        let ipc = RegistryDeactivateParams {
            rule_id: params.rule_id,
            version: params.version,
            scope,
        };
        match self.daemon.call(IpcRequest::RegistryDeactivate(ipc)).await {
            Ok(IpcResponse::RegistryDeactivate(RegistryDeactivateResponse {
                rule_id,
                version,
                was_deactivated,
                scope,
                jobs_rebound,
            })) => json_tool_result(&serde_json::json!({
                "rule_id": rule_id,
                "version": version,
                "was_deactivated": was_deactivated,
                "scope": scope,
                "jobs_rebound": jobs_rebound,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `registry_list_active` — snapshot of active rules.
    #[tool(
        description = "Snapshot of every currently-active rule. Returns id + version + severity + event_kind + tags."
    )]
    async fn registry_list_active(&self) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        match self.daemon.call(IpcRequest::RegistryListActive).await {
            Ok(IpcResponse::RegistryListActive(RegistryListActiveResponse { entries })) => {
                json_tool_result(&serde_json::json!({ "entries": entries }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `file_read_window` — bounded line/byte window read of one file.
    #[tool(
        description = "Read a bounded line window from a file. Returns structured lines + pointers; never the whole file."
    )]
    async fn file_read_window(
        &self,
        Parameters(params): Parameters<McpFileReadWindowParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let ipc = FileReadWindowParams {
            path: std::path::PathBuf::from(params.path),
            start_line: params.start_line,
            max_lines: params.max_lines,
            max_bytes: params.max_bytes,
        };
        match self.daemon.call(IpcRequest::FileReadWindow(ipc)).await {
            Ok(IpcResponse::FileReadWindow(FileReadWindowResponse {
                path,
                lines,
                file_bytes,
                truncated,
                next_byte_offset,
            })) => json_tool_result(&serde_json::json!({
                "path": path,
                "lines": lines,
                "file_bytes": file_bytes,
                "truncated": truncated,
                "next_byte_offset": next_byte_offset,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `file_search` — bounded substring search over one file.
    #[tool(
        description = "Search a file for a substring. Returns bounded match pointers + capped snippets only."
    )]
    async fn file_search(
        &self,
        Parameters(params): Parameters<McpFileSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let ipc = FileSearchParams {
            path: std::path::PathBuf::from(params.path),
            query: params.query,
            case_insensitive: params.case_insensitive,
            max_matches: params.max_matches,
            max_snippet_bytes: params.max_snippet_bytes,
        };
        match self.daemon.call(IpcRequest::FileSearch(ipc)).await {
            Ok(IpcResponse::FileSearch(FileSearchResponse {
                path,
                matches,
                truncated,
                bytes_scanned,
            })) => json_tool_result(&serde_json::json!({
                "path": path,
                "matches": matches,
                "truncated": truncated,
                "bytes_scanned": bytes_scanned,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `file_watch_start` — daemon-owned file watch bound to a bucket.
    #[tool(
        description = "Start a daemon-owned file watch. Future appended content is sifted by scoped rules and emitted as structured bucket events."
    )]
    async fn file_watch_start(
        &self,
        Parameters(params): Parameters<McpFileWatchStartParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let ipc = FileWatchStartParams {
            path: std::path::PathBuf::from(params.path),
            bucket_config: None,
            rules: vec![],
            follow_from_beginning: params.follow_from_beginning,
        };
        match self.daemon.call(IpcRequest::FileWatchStart(ipc)).await {
            Ok(IpcResponse::FileWatchStart(FileWatchStartResponse {
                watch_id,
                bucket_id,
                probe_id,
                cursor,
            })) => json_tool_result(&serde_json::json!({
                "watch_id": watch_id,
                "bucket_id": bucket_id,
                "probe_id": probe_id,
                "cursor": cursor,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `file_watch_stop` — stop a live watch by id.
    #[tool(
        description = "Stop a previously started file watch by watch_id. Returns final counters."
    )]
    async fn file_watch_stop(
        &self,
        Parameters(params): Parameters<McpFileWatchStopParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        use terminal_commander_core::ids::JobIdKind;
        let watch_id =
            parse_id::<JobIdKind>("watch_id", &params.watch_id).map_err(invalid_params)?;
        let ipc = FileWatchStopParams { watch_id };
        match self.daemon.call(IpcRequest::FileWatchStop(ipc)).await {
            Ok(IpcResponse::FileWatchStop(FileWatchStopResponse {
                watch_id,
                bucket_id,
                frames_total,
                events_emitted,
                bytes_total,
            })) => json_tool_result(&serde_json::json!({
                "watch_id": watch_id,
                "bucket_id": bucket_id,
                "frames_total": frames_total,
                "events_emitted": events_emitted,
                "bytes_total": bytes_total,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `file_watch_list` — snapshot of live file watches.
    #[tool(description = "Snapshot of every currently-live file watch.")]
    async fn file_watch_list(&self) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        match self.daemon.call(IpcRequest::FileWatchList).await {
            Ok(IpcResponse::FileWatchList(FileWatchListResponse { entries })) => {
                json_tool_result(&serde_json::json!({ "entries": entries }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `pty_command_start` — interactive PTY argv command.
    #[tool(
        description = "Start an interactive argv command attached to a PTY. Bounded metadata response only; never returns raw screen buffer. Shell interpreters denied."
    )]
    async fn pty_command_start(
        &self,
        Parameters(params): Parameters<McpPtyCommandStartParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        let env: Vec<(String, String)> = params.env.into_iter().map(|e| (e.key, e.value)).collect();
        let ipc = PtyCommandStartParams {
            environment: None,
            argv: params.argv,
            cwd: params.cwd.map(std::path::PathBuf::from),
            env,
            bucket_config: None,
            rules: vec![],
            rows: params.rows,
            cols: params.cols,
        };
        match self.daemon.call(IpcRequest::PtyCommandStart(ipc)).await {
            Ok(IpcResponse::PtyCommandStart(PtyCommandStartResponse {
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

    /// `pty_command_write_stdin` — bounded stdin write.
    #[tool(
        description = "Write bounded UTF-8 stdin bytes to a running PTY job. Returns SecretInputDenied while a secret prompt is active; no automatic password entry."
    )]
    async fn pty_command_write_stdin(
        &self,
        Parameters(params): Parameters<McpPtyCommandWriteStdinParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        use terminal_commander_core::ids::JobIdKind;
        let job_id = parse_id::<JobIdKind>("job_id", &params.job_id).map_err(invalid_params)?;
        let ipc = PtyCommandWriteStdinParams {
            job_id,
            bytes: params.bytes,
        };
        match self
            .daemon
            .call(IpcRequest::PtyCommandWriteStdin(ipc))
            .await
        {
            Ok(IpcResponse::PtyCommandWriteStdin(PtyCommandWriteStdinResponse {
                job_id,
                bytes_written,
                secret_prompt_active,
            })) => json_tool_result(&serde_json::json!({
                "job_id": job_id,
                "bytes_written": bytes_written,
                "secret_prompt_active": secret_prompt_active,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `pty_command_stop` — stop a live PTY job by id.
    #[tool(description = "Stop a previously started PTY job by job_id. Returns final counters.")]
    async fn pty_command_stop(
        &self,
        Parameters(params): Parameters<McpPtyCommandStopParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        use terminal_commander_core::ids::JobIdKind;
        let job_id = parse_id::<JobIdKind>("job_id", &params.job_id).map_err(invalid_params)?;
        let ipc = PtyCommandStopParams { job_id };
        match self.daemon.call(IpcRequest::PtyCommandStop(ipc)).await {
            Ok(IpcResponse::PtyCommandStop(PtyCommandStopResponse {
                job_id,
                bucket_id,
                frames_total,
                events_emitted,
                bytes_total,
                stdin_bytes_written,
                secret_prompts_total,
            })) => json_tool_result(&serde_json::json!({
                "job_id": job_id,
                "bucket_id": bucket_id,
                "frames_total": frames_total,
                "events_emitted": events_emitted,
                "bytes_total": bytes_total,
                "stdin_bytes_written": stdin_bytes_written,
                "secret_prompts_total": secret_prompts_total,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `pty_command_list` — snapshot of live PTY jobs.
    #[tool(description = "Snapshot of every currently-live PTY job.")]
    async fn pty_command_list(&self) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        match self.daemon.call(IpcRequest::PtyCommandList).await {
            Ok(IpcResponse::PtyCommandList(PtyCommandListResponse { entries })) => {
                json_tool_result(&serde_json::json!({ "entries": entries }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `runtime_state` — bounded aggregate runtime snapshot.
    #[tool(
        description = "Bounded aggregate runtime snapshot across all runtimes. Read-only; never returns raw stream content."
    )]
    async fn runtime_state(&self) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        match self.daemon.call(IpcRequest::RuntimeState).await {
            Ok(IpcResponse::RuntimeState(r)) => {
                let _ = std::any::type_name::<RuntimeStateResponse>();
                json_tool_result(&r)
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `probe_list` — flat list of every live probe.
    #[tool(
        description = "Flat list of every live probe across command / pty / file-watch runtimes."
    )]
    async fn probe_list(&self) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        match self.daemon.call(IpcRequest::ProbeList).await {
            Ok(IpcResponse::ProbeList(ProbeListResponse { probes })) => {
                json_tool_result(&serde_json::json!({ "probes": probes }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `probe_status` — bounded lookup for one probe by id.
    #[tool(
        description = "Bounded per-probe lookup by probe_id. Returns UnknownProbe if no runtime knows the id."
    )]
    async fn probe_status(
        &self,
        Parameters(params): Parameters<McpProbeStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.daemon.status() {
            if s.is_unavailable() {
                return Err(daemon_unavailable_error(&s.current()));
            }
        }
        use terminal_commander_core::ids::ProbeIdKind;
        let probe_id =
            parse_id::<ProbeIdKind>("probe_id", &params.probe_id).map_err(invalid_params)?;
        let ipc = ProbeStatusParams { probe_id };
        match self.daemon.call(IpcRequest::ProbeStatus(ipc)).await {
            Ok(IpcResponse::ProbeStatus(ProbeStatusResponse { probe })) => {
                json_tool_result(&serde_json::json!({ "probe": probe }))
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
        IpcErrorCode::PolicyDenied
        | IpcErrorCode::UnknownMethod
        | IpcErrorCode::SchemaMismatch
        | IpcErrorCode::ScopeInvalid => McpError::invalid_params(message, Some(data)),
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

/// Build a structured `daemon_unavailable` MCP error envelope.
/// Returned by daemon-requiring tools when the supervisor reports the
/// daemon is not reachable, so callers get a typed error instead of a
/// transport-level connection failure.
fn daemon_unavailable_error(status: &EnsureDaemonStatus) -> McpError {
    let payload = serde_json::json!({
        "code": "daemon_unavailable",
        "message": "terminal-commanderd is not reachable",
        "details": status,
    });
    McpError::internal_error("daemon_unavailable", Some(payload))
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
            environment: None,
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

fn parse_source_stream(name: &str) -> Result<terminal_commander_core::SourceStream, String> {
    match name {
        "stdout" => Ok(terminal_commander_core::SourceStream::Stdout),
        "stderr" => Ok(terminal_commander_core::SourceStream::Stderr),
        "meta" => Ok(terminal_commander_core::SourceStream::Meta),
        other => Err(format!(
            "stream '{other}' must be one of stdout|stderr|meta"
        )),
    }
}

/// MCP-facing parameters for `registry_search`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistrySearchParams {
    /// FTS5 query (e.g. `"apt"`, `"missing_package"`).
    pub query: String,
    /// Result cap. Clamped at the daemon.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// MCP-facing parameters for `registry_get`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryGetParams {
    pub rule_id: String,
    /// Omit for the latest stored version.
    #[serde(default)]
    pub version: Option<u32>,
}

/// MCP-facing parameters for `registry_upsert`. The full
/// `RuleDefinition` is passed as a JSON string so the MCP layer does
/// not need to mirror every field of the core schema.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryUpsertParams {
    /// JSON-encoded `RuleDefinition`. Daemon validates regex /
    /// keywords / kind before persisting.
    pub definition_json: String,
}

/// MCP-facing single sample for `registry_test`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryTestSample {
    pub text: String,
    /// Optional stream tag for `rule.stream` filtering. One of
    /// `stdout`, `stderr`, `meta`. Defaults to `stdout` when omitted.
    #[serde(default)]
    pub stream: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryTestParams {
    pub rule_id: String,
    #[serde(default)]
    pub version: Option<u32>,
    pub samples: Vec<McpRegistryTestSample>,
}

/// MCP-facing parameters for `registry_activate`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryActivateParams {
    pub rule_id: String,
    /// Omit to activate the latest stored version.
    #[serde(default)]
    pub version: Option<u32>,
    /// Optional scope (TC42c). Omitted = `global`.
    #[serde(default)]
    pub scope: Option<McpActivationScope>,
}

/// MCP-facing parameters for `registry_deactivate`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryDeactivateParams {
    pub rule_id: String,
    pub version: u32,
    /// Optional scope (TC42c). Omitted = `global`. MUST match the
    /// scope used at activation; deactivating with a different scope
    /// will not close the previously-opened activation row.
    #[serde(default)]
    pub scope: Option<McpActivationScope>,
}

/// MCP-facing scope DTO. Flat string fields so the generated JSON
/// schema is consumer-friendly. Translated to the daemon-side
/// `ActivationScope` in `into_ipc_scope`.
///
/// Exactly one of the four shapes is accepted:
/// - `{ "kind": "global" }`
/// - `{ "kind": "bucket", "bucket_id": "bkt_..." }`
/// - `{ "kind": "job", "job_id": "job_..." }`
/// - `{ "kind": "probe", "probe_id": "prb_..." }`
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpActivationScope {
    /// One of: `global`, `bucket`, `job`, `probe`.
    pub kind: String,
    #[serde(default)]
    pub bucket_id: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub probe_id: Option<String>,
}

impl McpActivationScope {
    /// Translate to the wire `ActivationScope` understood by the
    /// daemon. Returns an MCP `invalid_params` error on shape
    /// mismatch (wrong typed-id prefix, missing id for non-global,
    /// unknown kind).
    pub fn into_ipc_scope(self) -> Result<terminal_commander_core::ActivationScope, McpError> {
        use terminal_commander_core::ActivationScope;
        use terminal_commander_core::ids::{BucketIdKind, JobIdKind, ProbeIdKind};
        match self.kind.as_str() {
            "global" => Ok(ActivationScope::Global),
            "bucket" => {
                let s = self.bucket_id.ok_or_else(|| {
                    invalid_params("scope.kind=bucket requires scope.bucket_id".to_owned())
                })?;
                let bucket_id =
                    parse_id::<BucketIdKind>("scope.bucket_id", &s).map_err(invalid_params)?;
                Ok(ActivationScope::Bucket { bucket_id })
            }
            "job" => {
                let s = self.job_id.ok_or_else(|| {
                    invalid_params("scope.kind=job requires scope.job_id".to_owned())
                })?;
                let job_id = parse_id::<JobIdKind>("scope.job_id", &s).map_err(invalid_params)?;
                Ok(ActivationScope::Job { job_id })
            }
            "probe" => {
                let s = self.probe_id.ok_or_else(|| {
                    invalid_params("scope.kind=probe requires scope.probe_id".to_owned())
                })?;
                let probe_id =
                    parse_id::<ProbeIdKind>("scope.probe_id", &s).map_err(invalid_params)?;
                Ok(ActivationScope::Probe { probe_id })
            }
            other => Err(invalid_params(format!(
                "scope.kind '{other}' is not one of global|bucket|job|probe"
            ))),
        }
    }
}

// =====================================================================
// TC43: MCP-facing DTOs for file tools. Flat string fields so the
// generated JSON Schema is consumer-friendly. The daemon performs
// path-policy validation; the MCP layer must not touch the filesystem.
// =====================================================================

/// MCP-facing parameters for `file_read_window`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpFileReadWindowParams {
    /// Absolute or repo-relative path to a regular file.
    pub path: String,
    /// 1-based start line. Omit to read from line 1.
    #[serde(default)]
    pub start_line: Option<u64>,
    /// Max lines returned. Clamped by the daemon.
    #[serde(default)]
    pub max_lines: Option<u32>,
    /// Max payload bytes returned. Clamped by the daemon.
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

/// MCP-facing parameters for `file_search`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpFileSearchParams {
    pub path: String,
    pub query: String,
    #[serde(default)]
    pub case_insensitive: Option<bool>,
    #[serde(default)]
    pub max_matches: Option<u32>,
    #[serde(default)]
    pub max_snippet_bytes: Option<usize>,
}

/// MCP-facing parameters for `file_watch_start`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpFileWatchStartParams {
    pub path: String,
    /// Follow from beginning (default false = follow-end / tail-like).
    #[serde(default)]
    pub follow_from_beginning: Option<bool>,
}

/// MCP-facing parameters for `file_watch_stop`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpFileWatchStopParams {
    /// Wire-form JobId returned by `file_watch_start`.
    pub watch_id: String,
}

// =====================================================================
// TC44: PTY command MCP DTOs.
// =====================================================================

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpPtyCommandStartParams {
    /// Non-empty argv. argv[0] is the program; rest are args. Shell
    /// interpreters denied.
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: Vec<EnvEntry>,
    #[serde(default)]
    pub rows: Option<u16>,
    #[serde(default)]
    pub cols: Option<u16>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpPtyCommandWriteStdinParams {
    /// Wire-form JobId returned by `pty_command_start`.
    pub job_id: String,
    /// UTF-8 stdin payload. Capped at 4096 bytes by the daemon.
    pub bytes: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpPtyCommandStopParams {
    pub job_id: String,
}

// =====================================================================
// TC45: aggregate runtime view MCP DTOs.
// =====================================================================

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpProbeStatusParams {
    /// Wire-form ProbeId.
    pub probe_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_lists_twenty_nine_live_tools_at_tc45() {
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
                "registry_search",
                "registry_get",
                "registry_upsert",
                "registry_test",
                "registry_activate",
                "registry_deactivate",
                "registry_list_active",
                "file_read_window",
                "file_search",
                "file_watch_start",
                "file_watch_stop",
                "file_watch_list",
                "pty_command_start",
                "pty_command_write_stdin",
                "pty_command_stop",
                "pty_command_list",
                "runtime_state",
                "probe_list",
                "probe_status",
            ]
        );
        let not_impl: Vec<_> = tool_catalogue()
            .iter()
            .filter(|t| matches!(t.status, ToolStatus::NotImplemented))
            .map(|t| t.name)
            .collect();
        assert!(
            not_impl.is_empty(),
            "TC45 carries forward TC44's no-not_implemented invariant; got: {not_impl:?}"
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
                "file_read_window".to_owned(),
                "file_search".to_owned(),
                "file_watch_list".to_owned(),
                "file_watch_start".to_owned(),
                "file_watch_stop".to_owned(),
                "health".to_owned(),
                "policy_status".to_owned(),
                "probe_list".to_owned(),
                "probe_status".to_owned(),
                "pty_command_list".to_owned(),
                "pty_command_start".to_owned(),
                "pty_command_stop".to_owned(),
                "pty_command_write_stdin".to_owned(),
                "registry_activate".to_owned(),
                "registry_deactivate".to_owned(),
                "registry_get".to_owned(),
                "registry_list_active".to_owned(),
                "registry_search".to_owned(),
                "registry_test".to_owned(),
                "registry_upsert".to_owned(),
                "runtime_state".to_owned(),
                "self_check".to_owned(),
                "system_discover".to_owned(),
            ]
        );
    }

    #[test]
    fn system_discover_tools_explain_daemon_unavailable() {
        let tools = discovered_tools(false);
        assert_eq!(tools.len(), tool_catalogue().len());

        for tool in &tools {
            let expected_requires_daemon = tool.name != "system_discover";
            assert_eq!(
                tool.requires_daemon, expected_requires_daemon,
                "{} requires_daemon mismatch",
                tool.name
            );

            if !expected_requires_daemon {
                assert!(tool.available, "{} should remain callable", tool.name);
                assert_eq!(tool.unavailable_reason, None);
            } else if matches!(tool.status, ToolStatus::Live) {
                assert!(
                    !tool.available,
                    "{} should be unavailable without daemon",
                    tool.name
                );
                assert_eq!(tool.unavailable_reason, Some("daemon_unavailable"));
            } else {
                assert!(
                    !tool.available,
                    "{} should be unavailable when not implemented",
                    tool.name
                );
                assert_eq!(tool.unavailable_reason, Some("not_implemented"));
            }
        }
    }

    fn unavailable_status_server() -> TerminalCommanderMcpServer {
        let status = EnsureDaemonStatus::Unavailable {
            reason: terminal_commander_supervisor::ensure::DaemonUnavailableReason::BinaryNotFound,
            diagnostics: terminal_commander_supervisor::ensure::Diagnostics {
                endpoint: terminal_commander_supervisor::ensure::Endpoint::UnixSocket {
                    path: std::env::temp_dir().join("tc-mcp-unavailable-unit-test.sock"),
                },
                log_path: None,
                last_error: Some("test daemon unavailable".into()),
                startup_attempted: false,
                startup_elapsed_ms: 0,
            },
        };
        let daemon = McpDaemonClient::with_status(
            std::env::temp_dir().join("tc-mcp-unavailable-unit-test.sock"),
            crate::daemon_client::DaemonStatusHandle::new(status),
        )
        .with_timeout(std::time::Duration::from_millis(10));
        TerminalCommanderMcpServer::new(daemon)
    }

    fn assert_daemon_unavailable_tool_error(tool: &str, error: &McpError) {
        let rendered = error.to_string();
        assert!(
            rendered.contains("daemon_unavailable"),
            "{tool} should return daemon_unavailable when daemon status is unavailable, got: {rendered}"
        );
        assert!(
            rendered.contains("test daemon unavailable"),
            "{tool} should include startup diagnostics, got: {rendered}"
        );
        assert!(
            !rendered.contains("pipe connect") && !rendered.contains("ipc_code"),
            "{tool} should not leak raw daemon IPC failure details, got: {rendered}"
        );
    }

    #[tokio::test]
    async fn status_tools_short_circuit_on_unavailable_daemon_status() {
        let server = unavailable_status_server();

        let health = server.health().await.expect_err("health should fail");
        assert_daemon_unavailable_tool_error("health", &health);

        let policy = server
            .policy_status()
            .await
            .expect_err("policy_status should fail");
        assert_daemon_unavailable_tool_error("policy_status", &policy);

        let self_check = server
            .self_check()
            .await
            .expect_err("self_check should fail");
        assert_daemon_unavailable_tool_error("self_check", &self_check);
    }

    #[test]
    fn ipc_error_policy_denied_maps_to_invalid_params() {
        let e = IpcError::new(IpcErrorCode::PolicyDenied, "nope");
        let mcp = into_mcp_error(&e);
        assert!(mcp.message.contains("policy_denied") || mcp.message.contains("PolicyDenied"));
    }
}
