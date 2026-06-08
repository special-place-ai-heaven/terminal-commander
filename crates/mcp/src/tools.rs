// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

// These lints fire on the repetitive daemon-status guard pattern and
// inline `use` items in tool handlers.  The patterns are intentional
// (guard must appear before each IPC call; `use` is scoped to keep
// type names out of the module top-level).  Suppress rather than
// restructure, per Task 3.5.5 "do not change tool implementations".
#![allow(clippy::collapsible_if, clippy::items_after_statements)]

//! MCP tool surface served by the rmcp stdio adapter.
//!
//! This module defines [`TerminalCommanderMcpServer`], the rmcp
//! `ServerHandler` that the binary mounts on the stdio transport. The
//! server is a thin facade: every tool call is forwarded to the
//! `terminal-commanderd` daemon over the existing UDS IPC client. The
//! MCP process never spawns commands, opens raw files, or binds a
//! network socket.
//!
//! [`tool_catalogue`] is the single source of truth for the 37 live
//! tools, spanning discovery (`system_discover`), status (`health`,
//! `policy_status`, `self_check`), command/bucket/event, registry,
//! file, PTY, aggregate runtime views, and predicate-routed
//! subscriptions (`subscription_open/pull/list/close/seek`). Each maps 1:1
//! to a daemon IPC method. `system_discover` is the only
//! daemon-independent tool; every other tool returns the structured
//! `daemon_unavailable` envelope when the daemon is unreachable.
//!
//! Source-status: live; all 37 tools forward through daemon IPC.

use std::borrow::Cow;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, LoggingLevel, LoggingMessageNotificationParam,
        ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use terminal_commander_core::{BucketConfig, RuleDefinition, RuleType, Severity};
use terminal_commanderd::ipc::protocol::{
    BucketEventsSinceParams, BucketEventsSinceResponse, BucketSummaryParams, BucketSummaryResponse,
    BucketWaitParams, BucketWaitResponse, CommandOutputTailParams, CommandOutputTailResponse,
    CommandStartParams, CommandStartResponse, CommandStatusParams, CommandStatusResponse,
    ContextUnavailableReason, DiscoverResponse, EventContextParams, EventContextResponse,
    FileReadWindowParams, FileReadWindowResponse, FileSearchParams, FileSearchResponse,
    FileWatchListResponse, FileWatchStartParams, FileWatchStartResponse, FileWatchStopParams,
    FileWatchStopResponse, IpcContextFrame, IpcError, IpcErrorCode, IpcRequest, IpcResponse,
    ListLimitParams, PolicyStatusResponse, ProbeListResponse, ProbeStatusParams,
    ProbeStatusResponse, PtyCommandListResponse, PtyCommandStartParams, PtyCommandStartResponse,
    PtyCommandStopParams, PtyCommandStopResponse, PtyCommandWriteStdinParams,
    PtyCommandWriteStdinResponse, RegistryActivateParams, RegistryActivateResponse,
    RegistryDeactivateParams, RegistryDeactivateResponse, RegistryGetParams, RegistryGetResponse,
    RegistryImportPackParams, RegistryImportPackResponse, RegistryListActiveResponse,
    RegistrySearchParams, RegistrySearchResponse, RegistryTestParams, RegistryTestResponse,
    RegistryTestSample, RegistryUpsertParams, RegistryUpsertResponse, SelfCheckResponse,
    SubscriptionCloseParams, SubscriptionCloseResponse, SubscriptionListParams,
    SubscriptionListResponse, SubscriptionOpenParams, SubscriptionOpenResponse,
    SubscriptionPredicate, SubscriptionPullParams, SubscriptionPullResponse,
    SubscriptionSeekParams, SubscriptionSeekResponse, SubscriptionSourceSel,
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
            name: "run_and_watch",
            status: ToolStatus::Live,
            description: "One-shot: start a command, wait (bounded) for its rule signals + exit, return both. Quiet command returns a receipt, not an error.",
        },
        ToolCatalogueEntry {
            name: "command_status",
            status: ToolStatus::Live,
            description: "Lifecycle + counters lookup for a previously started job.",
        },
        ToolCatalogueEntry {
            name: "command_output_tail",
            status: ToolStatus::Live,
            description: "Read the last N lines of a command's captured output WITHOUT a rule. For one-off/exploratory commands whose format you don't know (df -h, docker system df): start it, then read its tail here. Bounded: 200 lines / 64 KiB, truncation-flagged. For recurring signals, define a rule instead.",
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
            name: "registry_import_pack",
            status: ToolStatus::Live,
            description: "Import a curated rule pack by name; optionally activate it in one call.",
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
            description: "Bounded per-probe lookup by probe_id. An id no runtime knows fails with an UnknownProbe error, not a success body.",
        },
        ToolCatalogueEntry {
            name: "subscription_open",
            status: ToolStatus::Live,
            description: "Open ONE predicate-routed subscription (severity/kind/sources) over many sources; returns an opaque sub_id + boot_id. sources:all auto-joins future probes.",
        },
        ToolCatalogueEntry {
            name: "subscription_pull",
            status: ToolStatus::Live,
            description: "Blocking multiplexed pull of matched events + per-source liveness from a sub_id. Idle returns SUCCESS empty+liveness; unknown sub_id errors.",
        },
        ToolCatalogueEntry {
            name: "subscription_list",
            status: ToolStatus::Live,
            description: "Bounded snapshot of open subscriptions (sub_id + predicate_hash + counters).",
        },
        ToolCatalogueEntry {
            name: "subscription_close",
            status: ToolStatus::Live,
            description: "Close a subscription by sub_id, freeing its slot. Idempotent (closed:false if unknown).",
        },
        ToolCatalogueEntry {
            name: "subscription_seek",
            status: ToolStatus::Live,
            description: "Reposition a subscription's offset for one bucket (explicit re-read). The requested seq is clamped to the bucket's live range (never an error); lagged flags an evicted request.",
        },
    ]
}

/// Flat list of every catalogue tool name.
///
/// Sourced from [`tool_catalogue`] and shared so non-rmcp callers
/// (e.g. the legacy in-process `ToolSurface` test facade) advertise
/// exactly the same set without re-hardcoding it.
#[must_use]
pub fn catalogue_tool_names() -> Vec<&'static str> {
    tool_catalogue().iter().map(|entry| entry.name).collect()
}

#[must_use]
fn tool_requires_daemon(name: &str) -> bool {
    name != "system_discover"
}

/// Whether a tool name belongs to the PTY command family.
///
/// The PTY runtime is `#[cfg(unix)]`-only; on every other platform the
/// daemon answers every `pty_*` IPC with `UnsupportedPlatform`. TB-1:
/// `system_discover` must surface that platform truth so a naive client
/// never calls a PTY tool that can only fail.
#[must_use]
const fn is_pty_tool(name: &str) -> bool {
    matches!(
        name.as_bytes(),
        b"pty_command_start"
            | b"pty_command_write_stdin"
            | b"pty_command_stop"
            | b"pty_command_list"
    )
}

/// Whether the PTY runtime is actually available on this host.
///
/// TC44's PTY runtime is gated on `#[cfg(unix)]`; ConPTY support for
/// Windows is still pending. On a non-unix host the four `pty_*` tools
/// can only return `UnsupportedPlatform`, so discovery must report them
/// `available: false` rather than implying a live PTY surface.
#[must_use]
const fn pty_runtime_available() -> bool {
    cfg!(unix)
}

/// Reason string surfaced for PTY tools on a host without a PTY runtime.
const PTY_UNAVAILABLE_REASON: &str = "PTY runtime unavailable on this platform (ConPTY pending)";

#[must_use]
fn discovered_tools(daemon_available: bool) -> Vec<DiscoveredToolEntry> {
    let pty_available = pty_runtime_available();
    tool_catalogue()
        .iter()
        .map(|tool| {
            let requires_daemon = tool_requires_daemon(tool.name);
            let implemented = matches!(tool.status, ToolStatus::Live);
            // TB-1: a PTY tool is only available when the host actually
            // has a PTY runtime. This is platform truth, evaluated even
            // when the daemon is reachable, so the four pty_* tools never
            // advertise `available: true` on a host that can only answer
            // them with `UnsupportedPlatform`.
            let platform_blocked = is_pty_tool(tool.name) && !pty_available;
            let available =
                implemented && !platform_blocked && (!requires_daemon || daemon_available);
            let unavailable_reason = if !implemented {
                Some("not_implemented")
            } else if platform_blocked {
                Some(PTY_UNAVAILABLE_REASON)
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

/// Long-poll client timeout for `subscription_pull`. STRICTLY ABOVE the
/// server-side pull cap (`MAX_PULL_TIMEOUT_MS` = 8 s): an idle ~8 s pull
/// must return SUCCESS empty+liveness, never a -32603 client timeout
/// (AC13 / MUST-ADD #7). Timeout hierarchy: pull server cap (8 s) <
/// DRAIN_CEILING (10 s) < this MCP pull client timeout (12 s).
const SUBSCRIPTION_PULL_CLIENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

/// MCP server handler. Holds the daemon client and the tool router.
#[derive(Clone)]
pub struct TerminalCommanderMcpServer {
    daemon: McpDaemonClient,
    /// Dedicated long-poll daemon client for `subscription_pull` ONLY.
    /// `with_timeout` is per-CLIENT, so the normal 5 s client cannot be
    /// reused for an 8 s idle pull without surfacing a -32603. This client
    /// shares the same socket but a 12 s timeout (MUST-ADD #7).
    pull_daemon: McpDaemonClient,
    /// BEST-EFFORT subscription notification opt-in (`TC_MCP_NOTIFY=1`), read
    /// ONCE at construction (default OFF). When true, a non-empty
    /// `subscription_pull` batch emits a `notifications/message` nudge as a
    /// hint; delivery of events is ALWAYS the pull, never this notification.
    notify: bool,
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
        // The notification nudge opt-in is read ONCE here (default OFF). Reading
        // at construction keeps the per-pull path a single bool check and gives
        // tests a clean seam (`with_notify`) that does NOT mutate process env --
        // `std::env::set_var` is `unsafe` under edition 2024 and the crate
        // forbids `unsafe_code`.
        Self::with_notify(daemon, notify_enabled())
    }

    /// Construct with an explicit notification opt-in, bypassing the
    /// `TC_MCP_NOTIFY` env read. Used by tests to observe the guarded
    /// `notifications/message` nudge without mutating process-global env.
    #[must_use]
    pub fn with_notify(daemon: McpDaemonClient, notify: bool) -> Self {
        // Dedicated long-poll client for `subscription_pull` (MUST-ADD #7):
        // same socket + status handle, but a 12 s per-CLIENT timeout so an
        // idle ~8 s pull returns SUCCESS empty+liveness, never a -32603.
        let pull_daemon = daemon
            .clone()
            .with_timeout(SUBSCRIPTION_PULL_CLIENT_TIMEOUT);
        Self {
            daemon,
            pull_daemon,
            notify,
            tool_router: Self::tool_router(),
        }
    }

    /// Single daemon-availability guard shared by every daemon-backed tool.
    /// Returns the structured `daemon_unavailable` envelope error when the
    /// daemon is not reachable; otherwise `Ok(())`. Call
    /// `self.ensure_daemon_available().await?` at the top of each handler
    /// before issuing IPC.
    ///
    /// Self-heal (audit H1): the startup status is a one-shot sample, so a
    /// daemon that was slow to bind would pin every tool to
    /// `daemon_unavailable` for the whole process life. When the cached
    /// status is `Unavailable`, this guard first attempts a single,
    /// bounded, single-flight `Health` re-probe; if the daemon is now
    /// live it clears the flag and returns `Ok(())` so the tool proceeds.
    /// A genuinely-down daemon's probe fails and the envelope is returned
    /// unchanged. The happy path (already available) is cheap: it never
    /// probes.
    async fn ensure_daemon_available(&self) -> Result<(), McpError> {
        // Cheap happy path: no status handle, or already available.
        let Some(status) = self.daemon.status() else {
            return Ok(());
        };
        if !status.is_unavailable() {
            return Ok(());
        }

        // Cached-unavailable: attempt a bounded, single-flight self-heal
        // before surfacing the envelope.
        if self.daemon.try_self_heal().await {
            return Ok(());
        }

        Err(daemon_unavailable_error(&status.current()))
    }

    /// `system_discover` — adapter metadata + tool catalogue.
    /// Forwards to the daemon to fetch live profile/version data; if
    /// the daemon is unreachable the response still carries the
    /// adapter-side catalogue with the daemon error surfaced.
    #[tool(description = "Return adapter version, MCP spec, policy profile, and tool catalogue.")]
    async fn system_discover(&self) -> Result<CallToolResult, McpError> {
        let (daemon, daemon_error) = match self.daemon.call(IpcRequest::SystemDiscover).await {
            Ok(IpcResponse::SystemDiscover(d)) => (Some(d), None),
            Ok(_other) => (
                // M6: stable, bounded code. Do not interpolate the response
                // enum's Debug shape into a user-facing error (it would leak
                // internal variant layout / payload fields). A wrong variant
                // here is a daemon protocol violation, not actionable detail.
                None,
                Some("unexpected_ipc_response: daemon returned a response that did not match system_discover".to_owned()),
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
        self.ensure_daemon_available().await?;
        match self.daemon.call(IpcRequest::Health).await {
            Ok(IpcResponse::Health { uptime_secs, .. }) => json_tool_result(&serde_json::json!({
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
        self.ensure_daemon_available().await?;
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
        self.ensure_daemon_available().await?;
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
        description = "Run a command and get back ONLY the lines your rules match, not the whole stream. You read the matching signal plus exit code instead of scrolling thousands of lines, which lets you run commands whose output is too big to fit in your context. If zero rules match, command_status still returns a bounded exit receipt (exit code, suppressed-line count, short tail) so a quiet command never looks broken. Returns job_id, bucket_id, probe_id, initial cursor; no other stdout/stderr text is returned. Argv only; shell interpreters are denied. Prefer plain shell for tiny one-off commands whose full output you want verbatim."
    )]
    async fn command_start_combed(
        &self,
        Parameters(params): Parameters<McpCommandStartParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = params.into_ipc()?;
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
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `run_and_watch` — one-shot: start a command, wait for its rule
    /// signals + exit, return both. Composes command_start_combed ->
    /// bucket_wait (bounded) -> command_status so the agent needs ONE
    /// call instead of four.
    #[tool(
        description = "Run a command and get its matching signals AND exit code in ONE call. Composes start + bounded wait + status so you don't poll. Pass inline `rules` (minimal: [{\"pattern\": \"ERROR\"}]) to comb the output; returns {signals, exit_code, state, receipt, complete, wait_exhausted}. A quiet command (no rule matches) returns a bounded receipt instead of an error — TC never bounces you to the shell for running a small command. Bounded: waits up to wait_ms (default 5000, max 60000) and returns up to max_signals (default 50). If `complete` is false (wait_exhausted), the command is STILL RUNNING; poll command_status with the returned job_id for the final exit_code/signals — do not treat it as finished. Argv only; shell interpreters denied. Prefer plain shell for tiny one-off commands whose full verbatim output you want."
    )]
    async fn run_and_watch(
        &self,
        Parameters(params): Parameters<McpRunAndWatchParams>,
    ) -> Result<CallToolResult, McpError> {
        use terminal_commander_core::JobState;

        self.ensure_daemon_available().await?;
        let (start_params, wait_ms, max_signals) = params.into_parts();
        let start_ipc = start_params.into_ipc()?;

        // 1. Start.
        let (job_id, bucket_id, mut cursor) = match self
            .daemon
            .call(IpcRequest::CommandStartCombed(start_ipc))
            .await
        {
            Ok(IpcResponse::CommandStartCombed(CommandStartResponse {
                job_id,
                bucket_id,
                cursor,
                ..
            })) => (job_id, bucket_id, cursor),
            Ok(other) => return Err(unexpected_variant(&other)),
            Err(e) => return Err(into_mcp_error_for(false, &e)),
        };

        // 2. Wait loop: drain signals until the job is terminal, the
        //    signal cap is hit, or the wait budget is spent. Each
        //    bucket_wait blocks up to a per-iteration slice; we stop as
        //    soon as the command exits so a fast command returns fast.
        let mut signals: Vec<terminal_commander_core::SignalEvent> = Vec::new();
        let deadline_slices = wait_ms.div_ceil(MAX_WAIT_SLICE_MS).max(1);
        let mut final_state = JobState::Running;
        let mut exit_code: Option<i32> = None;
        let mut receipt: Option<serde_json::Value> = None;

        for _ in 0..deadline_slices {
            // Poll status first so a command that already exited short-
            // circuits without burning a full wait slice.
            let status = match self
                .daemon
                .call(IpcRequest::CommandStatus(CommandStatusParams { job_id }))
                .await
            {
                Ok(IpcResponse::CommandStatus(s)) => s,
                Ok(other) => return Err(unexpected_variant(&other)),
                Err(e) => return Err(into_mcp_error(&e)),
            };
            final_state = status.state;
            exit_code = status.exit_code;
            receipt = status.receipt.as_ref().map(|r| serde_json::json!(r));

            let terminal = matches!(
                status.state,
                JobState::Exited | JobState::Cancelled | JobState::Failed
            );

            // Drain any signals available since the last cursor.
            let wait = BucketWaitParams {
                bucket_id,
                cursor,
                severity_min: None,
                kind_filter: None,
                limit: Some(max_signals.saturating_sub(signals.len()).max(1)),
                timeout_ms: Some(if terminal { 0 } else { MAX_WAIT_SLICE_MS }),
            };
            match self.daemon.call(IpcRequest::BucketWait(wait)).await {
                Ok(IpcResponse::BucketWait(r)) => {
                    cursor = r.next_cursor;
                    for ev in r.events {
                        // Only rule-driven events count as "signals". The
                        // bucket also carries probe-lifecycle markers (e.g.
                        // the `command_exited` meta event, which has no
                        // `rule`); those are the exit indicator, surfaced
                        // via exit_code/state and the receipt, not as a
                        // matched signal. Counting them would defeat the
                        // zero-signal receipt path (a quiet command would
                        // look like it matched something).
                        if ev.rule.is_some() && signals.len() < max_signals {
                            signals.push(ev);
                        }
                    }
                }
                Ok(other) => return Err(unexpected_variant(&other)),
                Err(e) => return Err(into_mcp_error(&e)),
            }

            if terminal || signals.len() >= max_signals {
                break;
            }
        }

        // 3. Compose the response. The receipt is included only when the
        //    command finished with zero rule signals (no-silence rule):
        //    a quiet command yields a receipt, never an error.
        //
        // Completion markers (trust contract): the wait budget is bounded,
        // so the loop can return while the job is still `Running`. The
        // success-shaped payload (isError:false) is then ambiguous to a
        // naive reader. `complete`/`wait_exhausted` make it unambiguous:
        //   - `complete` is true iff the job reached a terminal state.
        //   - `wait_exhausted` is true iff the loop returned non-terminal
        //     (the wait budget was spent while the command was still
        //     running). The caller MUST poll `command_status` for the
        //     final exit_code/signals in that case.
        let (complete, wait_exhausted) = run_and_watch_completion(final_state);
        let include_receipt = signals.is_empty();
        json_tool_result(&serde_json::json!({
            "job_id": job_id,
            "bucket_id": bucket_id,
            "state": final_state,
            "exit_code": exit_code,
            "signals": signals,
            "signal_count": signals.len(),
            "receipt": if include_receipt { receipt } else { None },
            "complete": complete,
            "wait_exhausted": wait_exhausted,
        }))
    }

    /// `command_status` — lifecycle counters + exit info for a job.
    #[tool(
        description = "Lookup bounded counters and exit info for a previously started job. Never returns raw stream text, with one exception: when the command finished and ZERO rules matched, a bounded exit receipt (exit code, suppressed-line count, short tail) is included so a no-rule command is never silent."
    )]
    async fn command_status(
        &self,
        Parameters(params): Parameters<McpCommandStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let job_id = parse_id::<terminal_commander_core::ids::JobIdKind>("job_id", &params.job_id)
            .map_err(invalid_params)?;
        let ipc = CommandStatusParams { job_id };
        match self.daemon.call(IpcRequest::CommandStatus(ipc)).await {
            Ok(IpcResponse::CommandStatus(s)) => json_tool_result(&command_status_payload(&s)),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `command_output_tail` — rule-free bounded read of a job's
    /// captured output. Useful for one-off exploratory commands.
    #[tool(
        description = "Read the last N lines of a command's captured output WITHOUT a rule. For one-off/exploratory commands whose format you don't know (df -h, docker system df): start it, then read its tail here. Bounded: 200 lines / 64 KiB, truncation-flagged. For recurring signals, define a rule instead."
    )]
    async fn command_output_tail(
        &self,
        Parameters(params): Parameters<McpCommandOutputTailParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let job_id = parse_id::<terminal_commander_core::ids::JobIdKind>("job_id", &params.job_id)
            .map_err(invalid_params)?;
        let ipc = CommandOutputTailParams {
            job_id,
            max_lines: params.max_lines.unwrap_or(50),
            max_bytes: params.max_bytes.unwrap_or(65_536),
        };
        match self.daemon.call(IpcRequest::CommandOutputTail(ipc)).await {
            Ok(IpcResponse::CommandOutputTail(r)) => {
                json_tool_result(&command_output_tail_payload(&r))
            }
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
        self.ensure_daemon_available().await?;
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
        self.ensure_daemon_available().await?;
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
        self.ensure_daemon_available().await?;
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
        description = "Bounded context window around an event. The success body carries frames, or an empty frames list with a typed unavailable_reason when no pointer exists (NoPointer/SyntheticEvent/AnchorEvicted) -- not an error. Pointer-based; never streams."
    )]
    async fn event_context(
        &self,
        Parameters(params): Parameters<McpEventContextParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
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
        self.ensure_daemon_available().await?;
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
        self.ensure_daemon_available().await?;
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
        description = "Create a new immutable (rule_id, version+1) row from a JSON RuleDefinition string passed as `definition_json`. REQUIRED fields: id, version, kind, severity, event_kind, summary_template (+ pattern when kind=regex, or keywords when kind=keyword). NOTE: `version` is ASSIGNED by the store (monotonic, latest+1); any value you send is ignored and overwritten, and the assigned version (returned in the response) is the one registry_activate/registry_deactivate operate on. `event_kind` is the event label emitted on match (a short string, e.g. \"compile_error\"). `kind` is one of keyword|regex|prompt|exit_code|stream_marker|progress_collapse|dedupe|threshold|sequence|anchor|custom (only keyword and regex are live at MVP). `severity` is one of trace|debug|info|low|medium|high|critical. New rules default to status=Draft (test-only); set \"status\":\"active\" in the definition to make the rule eligible for registry_activate. Complete kind:regex example (this exact shape succeeds on the first try): definition_json = '{\"id\":\"rust-compile-error\",\"version\":1,\"kind\":\"regex\",\"status\":\"active\",\"severity\":\"high\",\"event_kind\":\"compile_error\",\"pattern\":\"error\\\\[E[0-9]+\\\\]\",\"summary_template\":\"${line}\"}'. Call registry_get to see the canonical full shape of any stored rule. Validates regex/keywords; existing versions are never mutated."
    )]
    async fn registry_upsert(
        &self,
        Parameters(params): Parameters<McpRegistryUpsertParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
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
            Err(e) => Err(into_mcp_error_for(false, &e)),
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
        self.ensure_daemon_available().await?;
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
        description = "Activate (rule_id, version?, scope) so every newly-started command uses the rule. scope is REQUIRED -- pass scope {kind:'global'} for the common single-agent case; an omitted scope is rejected (never silently widened to global). Already-running commands are not hot-rebound. Pattern: activate the rule (or import the `cleanup` pack via registry_import_pack), THEN start the command. To read output from a command you already started without a matching rule, use command_output_tail."
    )]
    async fn registry_activate(
        &self,
        Parameters(params): Parameters<McpRegistryActivateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        // `scope` is schema-required (TB-5): rmcp rejects an omitted
        // scope before this handler runs, so it is always present here.
        // We still pass it as `Some(..)` because the wire IPC type keeps
        // `scope: Option` for backward compatibility.
        let scope = Some(params.scope.into_ipc_scope()?);
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
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `registry_import_pack` — one-call expert signal extraction.
    #[tool(
        description = "Import a curated rule pack (cargo, pytest, npm, gcc, apt, make, generic.terminal, cleanup) so you get expert signal extraction without authoring any rule JSON. CONDITIONAL scope contract: pass activate=true + scope {kind:'global'} together to make the rules live immediately for your commands (one call replaces ~6 rule-authoring calls); activate=true WITHOUT scope is rejected with a scope-required error, never silently widened to global. With activate=false (default) the pack is imported but not activated and scope is ignored. An unknown pack name returns the list of available packs."
    )]
    async fn registry_import_pack(
        &self,
        Parameters(params): Parameters<McpRegistryImportPackParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let scope = match params.scope {
            Some(s) => Some(s.into_ipc_scope()?),
            None => None,
        };
        let ipc = RegistryImportPackParams {
            pack: params.pack,
            activate: params.activate,
            scope,
        };
        match self.daemon.call(IpcRequest::RegistryImportPack(ipc)).await {
            Ok(IpcResponse::RegistryImportPack(RegistryImportPackResponse {
                pack,
                imported,
                skipped,
                activated,
                failed,
            })) => {
                // M7 (partial-success): surface `failed` so the agent
                // sees WHICH rules activated and which need a retry,
                // instead of a bare error that hides the ones that
                // succeeded. A non-empty `failed` is still a successful
                // tool result (the daemon returned Ok, not an IPC
                // error). `failed` is rendered as [{rule_id, reason}].
                let failed_json: Vec<serde_json::Value> = failed
                    .into_iter()
                    .map(|f| {
                        serde_json::json!({
                            "rule_id": f.rule_id,
                            "reason": f.reason,
                        })
                    })
                    .collect();
                json_tool_result(&serde_json::json!({
                    "pack": pack,
                    "imported": imported,
                    "skipped": skipped,
                    "activated": activated,
                    "failed": failed_json,
                }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `registry_deactivate` — remove a rule from the active set.
    #[tool(
        description = "Deactivate (rule_id, version, scope). scope is REQUIRED and must match the scope used at activation (e.g. {kind:'global'}); an omitted scope is rejected. Future commands skip the rule; already-running commands keep the rules they were started with."
    )]
    async fn registry_deactivate(
        &self,
        Parameters(params): Parameters<McpRegistryDeactivateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        // `scope` is schema-required (TB-5): rmcp rejects an omitted
        // scope before this handler runs. The wire IPC type keeps
        // `scope: Option` for backward compatibility, so wrap in `Some`.
        let scope = Some(params.scope.into_ipc_scope()?);
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
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `registry_list_active` — snapshot of active rules.
    #[tool(
        description = "Snapshot of every currently-active rule. Returns id + version + severity + event_kind + tags."
    )]
    async fn registry_list_active(
        &self,
        Parameters(params): Parameters<McpListLimitParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = ListLimitParams {
            limit: params.limit,
        };
        match self.daemon.call(IpcRequest::RegistryListActive(ipc)).await {
            Ok(IpcResponse::RegistryListActive(RegistryListActiveResponse {
                entries,
                truncated,
            })) => json_tool_result(&serde_json::json!({
                "entries": entries,
                "truncated": truncated,
            })),
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
        self.ensure_daemon_available().await?;
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
        self.ensure_daemon_available().await?;
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
        self.ensure_daemon_available().await?;
        let (bucket_config, rules) =
            parse_bucket_and_rules(params.bucket_config_json, params.rules, params.rules_json)?;
        let ipc = FileWatchStartParams {
            path: std::path::PathBuf::from(params.path),
            bucket_config,
            rules,
            follow_from_beginning: params.follow_from_beginning,
            tag: params.tag,
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
            Err(e) => Err(into_mcp_error_for(false, &e)),
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
        self.ensure_daemon_available().await?;
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
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `file_watch_list` — snapshot of live file watches.
    #[tool(description = "Snapshot of every currently-live file watch.")]
    async fn file_watch_list(&self) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
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
        self.ensure_daemon_available().await?;
        let env: Vec<(String, String)> = params.env.into_iter().map(|e| (e.key, e.value)).collect();
        let (bucket_config, rules) =
            parse_bucket_and_rules(params.bucket_config_json, params.rules, params.rules_json)?;
        let ipc = PtyCommandStartParams {
            environment: None,
            argv: params.argv,
            cwd: params.cwd.map(std::path::PathBuf::from),
            env,
            bucket_config,
            rules,
            rows: params.rows,
            cols: params.cols,
            tag: params.tag,
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
            Err(e) => Err(into_mcp_error_for(false, &e)),
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
        self.ensure_daemon_available().await?;
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
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `pty_command_stop` — stop a live PTY job by id.
    #[tool(description = "Stop a previously started PTY job by job_id. Returns final counters.")]
    async fn pty_command_stop(
        &self,
        Parameters(params): Parameters<McpPtyCommandStopParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
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
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `pty_command_list` — snapshot of live PTY jobs.
    #[tool(description = "Snapshot of every currently-live PTY job.")]
    async fn pty_command_list(&self) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
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
    async fn runtime_state(
        &self,
        Parameters(params): Parameters<McpListLimitParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = ListLimitParams {
            limit: params.limit,
        };
        match self.daemon.call(IpcRequest::RuntimeState(ipc)).await {
            Ok(IpcResponse::RuntimeState(r)) => json_tool_result(&r),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `probe_list` — flat list of every live probe.
    #[tool(
        description = "Flat list of every live probe across command / pty / file-watch runtimes."
    )]
    async fn probe_list(
        &self,
        Parameters(params): Parameters<McpListLimitParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = ListLimitParams {
            limit: params.limit,
        };
        match self.daemon.call(IpcRequest::ProbeList(ipc)).await {
            Ok(IpcResponse::ProbeList(ProbeListResponse { probes, truncated })) => {
                json_tool_result(&serde_json::json!({ "probes": probes, "truncated": truncated }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `probe_status` — bounded lookup for one probe by id.
    #[tool(
        description = "Bounded per-probe lookup by probe_id. On success returns the probe descriptor; an id no runtime knows fails with an UnknownProbe error (not a success body)."
    )]
    async fn probe_status(
        &self,
        Parameters(params): Parameters<McpProbeStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
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

    /// `subscription_open` — open a predicate-routed subscription.
    #[tool(
        description = "Open ONE multiplexed subscription over many sources by PREDICATE (severity_min, kind allowlist, sources: all|{jobs:[..]}|{buckets:[..]}|{probes:[..]}), instead of juggling N (bucket_id, cursor) triples. Returns an opaque sub_id + boot_id (a changed boot_id across opens means the daemon restarted and all subscriptions reset). sources:all auto-joins future matching probes. A late open starts from-now (no ring replay). Then loop subscription_pull on the sub_id."
    )]
    async fn subscription_open(
        &self,
        Parameters(params): Parameters<McpSubscriptionOpenParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let predicate = params.into_predicate().map_err(invalid_params)?;
        let ipc = SubscriptionOpenParams { predicate };
        match self.daemon.call(IpcRequest::SubscriptionOpen(ipc)).await {
            Ok(IpcResponse::SubscriptionOpen(SubscriptionOpenResponse {
                sub_id,
                boot_id,
                predicate_hash,
                created_at_ms,
                matched_sources,
            })) => json_tool_result(&serde_json::json!({
                "sub_id": sub_id,
                "boot_id": boot_id,
                "predicate_hash": predicate_hash,
                "created_at_ms": created_at_ms,
                "matched_sources": matched_sources,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `subscription_pull` — multiplexed, lossless, blocking pull.
    #[tool(
        description = "Pull matched events from an open subscription, multiplexed across all in-scope buckets. Blocks up to timeout_ms (default 5000, max 8000) until events arrive or the timeout elapses. Returns source-tagged events[] (bounded by max, default/cap 50) + per-source liveness[] (starting|running|exited|failed|cancelled|stopped) + lagged + truncated. An IDLE pull is SUCCESS with empty events[] + liveness (never an error). An unknown/expired sub_id returns an unknown_subscription error (re-open). Loop this for real-time-relative-to-turn delivery."
    )]
    async fn subscription_pull(
        &self,
        Parameters(params): Parameters<McpSubscriptionPullParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = SubscriptionPullParams {
            sub_id: params.sub_id,
            max: params.max,
            timeout_ms: params.timeout_ms,
        };
        // Route through the dedicated long-poll client: an idle ~8 s pull on the
        // default 5 s client would surface a -32603 (AC13 / MUST-ADD #7).
        match self
            .pull_daemon
            .call(IpcRequest::SubscriptionPull(ipc))
            .await
        {
            Ok(IpcResponse::SubscriptionPull(SubscriptionPullResponse {
                events,
                liveness,
                lagged,
                truncated,
            })) => {
                // BEST-EFFORT nudge: ONLY on a non-empty batch AND only when
                // explicitly opted in (TC_MCP_NOTIFY=1). NEVER on the idle
                // path. A send error is ignored -- delivery of events is ALWAYS
                // the pull, never this notification. Claude Code DROPS
                // notifications to idle sessions (claude-code #36665 "not
                // planned", #61797 drop-bug), so this is a hint for harnesses
                // that surface notifications and a silent no-op for those that
                // do not. It is in-process over the already-open stdio pipe (no
                // spawn/fs/socket), so the MCP facade guards stay green.
                if !events.is_empty() && self.notify {
                    let max_sev = events
                        .iter()
                        .map(|e| e.event.severity)
                        .max()
                        .unwrap_or(Severity::Info);
                    let _ = ctx
                        .peer
                        .notify_logging_message(LoggingMessageNotificationParam {
                            level: LoggingLevel::Info,
                            logger: Some("terminal-commander".to_owned()),
                            data: serde_json::json!({
                                "subscription": "new_events",
                                "count": events.len(),
                                "max_severity": max_sev,
                                "lagged": lagged,
                            }),
                        })
                        .await;
                }
                json_tool_result(&serde_json::json!({
                    "events": events,
                    "liveness": liveness,
                    "lagged": lagged,
                    "truncated": truncated,
                }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `subscription_list` — bounded snapshot of open subscriptions.
    #[tool(
        description = "Snapshot of every open subscription: sub_id + predicate_hash + source_count + created_at_ms + last_pull_at_ms. Bounded by an optional limit (default/cap 64); truncated flags when more exist."
    )]
    async fn subscription_list(
        &self,
        Parameters(params): Parameters<McpSubscriptionListParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = SubscriptionListParams {
            limit: params.limit,
        };
        match self.daemon.call(IpcRequest::SubscriptionList(ipc)).await {
            Ok(IpcResponse::SubscriptionList(SubscriptionListResponse {
                subscriptions,
                truncated,
            })) => json_tool_result(&serde_json::json!({
                "subscriptions": subscriptions,
                "truncated": truncated,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `subscription_close` — close a subscription and free its slot.
    #[tool(
        description = "Close a subscription by sub_id, freeing its registry slot. Returns closed: true if it existed, false if it was already unknown (idempotent)."
    )]
    async fn subscription_close(
        &self,
        Parameters(params): Parameters<McpSubscriptionCloseParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = SubscriptionCloseParams {
            sub_id: params.sub_id,
        };
        match self.daemon.call(IpcRequest::SubscriptionClose(ipc)).await {
            Ok(IpcResponse::SubscriptionClose(SubscriptionCloseResponse { closed })) => {
                json_tool_result(&serde_json::json!({ "closed": closed }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `subscription_seek` — reposition a subscription's offset for one bucket.
    #[tool(
        description = "Reposition a subscription's offset for one bucket (explicit re-read). The requested seq is clamped to the bucket's live range and is never an error; lagged=true means the requested events were already evicted. Unknown sub_id errors."
    )]
    async fn subscription_seek(
        &self,
        Parameters(params): Parameters<McpSubscriptionSeekParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let bucket_id =
            parse_id::<terminal_commander_core::ids::BucketIdKind>("bucket_id", &params.bucket_id)
                .map_err(invalid_params)?;
        let ipc = SubscriptionSeekParams {
            sub_id: params.sub_id,
            bucket_id,
            seq: params.seq,
        };
        match self.daemon.call(IpcRequest::SubscriptionSeek(ipc)).await {
            Ok(IpcResponse::SubscriptionSeek(SubscriptionSeekResponse {
                clamped_seq,
                lagged,
            })) => json_tool_result(
                &serde_json::json!({ "clamped_seq": clamped_seq, "lagged": lagged }),
            ),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }
}

#[tool_handler]
impl ServerHandler for TerminalCommanderMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                // Advertise `logging` so the BEST-EFFORT subscription_pull nudge
                // (TC_MCP_NOTIFY, off by default) can ride the open stdio pipe
                // as a `notifications/message`. Delivery of events is ALWAYS the
                // pull; this capability never makes the notification load-bearing.
                .enable_logging()
                .build(),
        )
            .with_server_info(Implementation::new(
                "terminal-commander-mcp",
                ADAPTER_VERSION,
            ))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Terminal Commander runs commands and returns STRUCTURED SIGNALS, not raw output: you define keyword/regex rules and get back only the matching events plus exit state, so you can run noisy or long-running commands without flooding your context. This saves you tokens and scrolling and lets you run commands too large to read. If no rule matches, command_status gives you a bounded receipt (exit code, suppressed-line count, short tail), never silence. Use plain shell instead for tiny, interactive, or one-off commands where the full output is small and you want it verbatim; reach for Terminal Commander when output is large, noisy, long-running, or you only care about specific signals. The adapter is a thin facade: each tool forwards 1:1 to a daemon IPC method (discovery, status, command/bucket/event, registry, file, PTY, runtime)."
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

/// Map a daemon `IpcError` to an MCP `ErrorData`, honest about the mutability
/// of the request that produced it.
///
/// `request_is_idempotent` matters ONLY on the transport-failure short-circuit:
/// a transport loss is "could not reach the daemon", and whether it is safe to
/// re-issue depends on whether the request was a pure read. For a mutating
/// request the daemon may already have applied the side effect before the
/// transport dropped, so the envelope must tell the agent to reconcile, never
/// to blindly retry. Daemon-RETURNED errors (the code match below) are
/// classified identically regardless of mutability.
#[must_use]
pub fn into_mcp_error_for(request_is_idempotent: bool, e: &IpcError) -> McpError {
    // Mid-call TRANSPORT failure (the daemon pipe/socket went away during the
    // call): `McpDaemonClient::call` already attempted self-heal, and re-sent
    // the request once IFF it was idempotent. Surface the clean
    // `daemon_unavailable` envelope the startup-unavailable path produces --
    // self-explanatory, recoverable, and crucially NOT a raw `internal_error`
    // semantics at the application layer (the `code` is `daemon_unavailable`),
    // which trains agents to abandon TC for raw shell (TB-7 / Cursor call #21).
    // This must come BEFORE the code match: a transport error is `Internal`-
    // coded but is "could not reach the daemon", not a server fault. The raw OS
    // detail (e.g. "pipe connect ... os error 2") is deliberately NOT leaked
    // into the message or data.
    if e.is_transport() {
        return transport_unavailable_error(request_is_idempotent);
    }
    let message: Cow<'static, str> = Cow::Owned(format_ipc_error(e));
    let data = serde_json::json!({
        "ipc_code": format!("{:?}", e.code),
    });
    // Trust contract: a caller-fixable error MUST surface as
    // `invalid_params` (JSON-RPC -32602) so the agent corrects its
    // input and keeps routing through Terminal Commander. Mapping such
    // an error to `internal_error` (-32603) signals "the server is
    // broken", which trains agents to abandon TC and fall back to raw
    // shell -- the exact behavior TC exists to prevent. Reserve
    // `internal_error` for the few codes the caller genuinely cannot
    // act on (a real server fault, a transport/credential failure, an
    // unsupported host, or the daemon draining for shutdown). The match
    // is exhaustive on purpose: adding an `IpcErrorCode` variant forces
    // a deliberate classification here rather than silently defaulting
    // to "broken".
    match e.code {
        IpcErrorCode::Internal
        | IpcErrorCode::PeerCredentialFailure
        | IpcErrorCode::UnsupportedPlatform
        | IpcErrorCode::ShuttingDown => McpError::internal_error(message, Some(data)),
        IpcErrorCode::FrameTooLarge
        | IpcErrorCode::MalformedJson
        | IpcErrorCode::SchemaMismatch
        | IpcErrorCode::UnknownMethod
        | IpcErrorCode::PolicyDenied
        | IpcErrorCode::BucketNotFound
        | IpcErrorCode::EventNotFound
        | IpcErrorCode::InvalidCursor
        | IpcErrorCode::ShellInterpreterDenied
        | IpcErrorCode::ArgvInvalid
        | IpcErrorCode::UnknownJob
        | IpcErrorCode::RuleNotFound
        | IpcErrorCode::RuleInvalid
        | IpcErrorCode::ScopeInvalid
        | IpcErrorCode::PathDenied
        | IpcErrorCode::FileNotFound
        | IpcErrorCode::FileBinary
        | IpcErrorCode::OversizedRequest
        | IpcErrorCode::UnknownWatch
        | IpcErrorCode::SecretInputDenied
        | IpcErrorCode::UnknownProbe
        | IpcErrorCode::RuleNotActive
        | IpcErrorCode::UnknownSubscription
        | IpcErrorCode::SubscriptionLimitExceeded => McpError::invalid_params(message, Some(data)),
    }
}

/// Map a daemon `IpcError` to an MCP `ErrorData` with stable codes.
///
/// This is the READ-ONLY edge: a transport failure produces the idempotent
/// transport envelope (the read may be safely re-issued). Tool arms that send a
/// MUTATING request MUST call [`into_mcp_error_for`] with
/// `request_is_idempotent = false` so a transport failure produces the honest
/// reconcile-don't-retry envelope instead.
#[must_use]
pub fn into_mcp_error(e: &IpcError) -> McpError {
    into_mcp_error_for(true, e)
}

fn unexpected_variant(_resp: &IpcResponse) -> McpError {
    // Do NOT interpolate the response's Debug shape ({resp:?}): IpcResponse
    // variants carry payloads (captured stream text, file-window content,
    // rule definitions) that would leak into the client-facing error and
    // bloat it with unbounded tokens. A wrong variant is a daemon-side
    // protocol fault the caller cannot fix, so return a bounded,
    // payload-free message (mirrors the system_discover precedent).
    McpError::internal_error(
        Cow::Borrowed("daemon returned a response that did not match the request method"),
        None,
    )
}

/// The BEST-EFFORT subscription notification nudge is gated behind the
/// `TC_MCP_NOTIFY` opt-in (default OFF). Read here so the call site stays a
/// single boolean check; the env var is only consulted on the non-empty pull
/// path, never on the idle path.
fn notify_enabled() -> bool {
    std::env::var("TC_MCP_NOTIFY").as_deref() == Ok("1")
}

#[must_use]
pub fn format_ipc_error(e: &IpcError) -> String {
    format!("daemon ipc error [{:?}]: {}", e.code, e.message)
}

/// Completion markers for the `run_and_watch` response.
///
/// `run_and_watch` waits only up to a bounded budget, so the loop can
/// return while the job is still non-terminal. Returns
/// `(complete, wait_exhausted)`:
/// - `complete` is true iff `state` is terminal (`Exited`, `Cancelled`,
///   or `Failed`).
/// - `wait_exhausted` is the negation: true when the bounded wait budget
///   was spent while the job was still `Starting`/`Running`. A caller that
///   sees `wait_exhausted` MUST poll `command_status` for the final
///   exit_code/signals rather than treat the success-shaped payload as
///   finished.
#[must_use]
const fn run_and_watch_completion(state: terminal_commander_core::JobState) -> (bool, bool) {
    use terminal_commander_core::JobState;
    let complete = matches!(
        state,
        JobState::Exited | JobState::Cancelled | JobState::Failed
    );
    (complete, !complete)
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

/// Build the `daemon_unavailable` envelope for a MID-CALL transport failure
/// (see [`crate::daemon_client::McpDaemonClient::call`]).
///
/// Same `code: "daemon_unavailable"` + teaching-text shape as
/// [`daemon_unavailable_error`], so a transport loss mid-call is
/// indistinguishable to the agent from a startup-time unavailability. Carries
/// no `EnsureDaemonStatus` (we are past startup) and never leaks the raw OS
/// error.
///
/// Numeric code: this currently routes through [`McpError::internal_error`], so
/// the wire is JSON-RPC `-32603`. The application-level `code` is the honest
/// `daemon_unavailable`; the numeric-code migration off `-32603` is tracked
/// separately and deliberately NOT done here.
///
/// The `remedy` depends on whether the failed request was idempotent:
///   - idempotent (a pure read): the adapter already attempted self-heal + one
///     retry, so a manual re-call of the read is safe -- the remedy says so.
///   - mutating: the adapter did NOT re-send (a blind retry could double the
///     side effect), and a client-side transport failure cannot prove whether
///     the daemon already applied the change. The remedy is operation-neutral
///     and explicitly does NOT say "retry the tool": it tells the agent to
///     reconcile actual state via `command_status` / `runtime_state` first.
fn transport_unavailable_error(operation_is_idempotent: bool) -> McpError {
    let (recovery, remedy) = if operation_is_idempotent {
        (
            "auto-recovery (health re-probe + one retry) was attempted",
            "the daemon was unavailable; retry the tool -- the adapter \
             re-establishes the daemon on the next call",
        )
    } else {
        (
            "auto-recovery (health re-probe) was attempted; the request was \
             NOT re-sent because re-sending a mutating operation could apply \
             it twice",
            "this mutating operation may or may not have taken effect; call \
             command_status or runtime_state to confirm the actual state \
             before re-issuing",
        )
    };
    let payload = serde_json::json!({
        "code": "daemon_unavailable",
        "message": "terminal-commanderd became unreachable mid-call",
        "details": {
            "phase": "mid_call_transport",
            "recovery": recovery,
            "remedy": remedy,
        },
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

/// One environment variable as an object.
///
/// The `env` parameter is an ARRAY of these (NOT a JSON map), e.g.
/// `[{"key":"FOO","value":"bar"}]`. A tuple/map form is intentionally
/// not accepted: the explicit `{key,value}` shape is what the schema
/// teaches so the first call from a naive client succeeds.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EnvEntry {
    /// Variable name, e.g. `"PATH"`.
    pub key: String,
    /// Variable value, e.g. `"/usr/bin"`.
    pub value: String,
}

/// Deserialize the `env` parameter as an array of [`EnvEntry`] objects,
/// mapping serde's bare `invalid type: map, expected a sequence` into a
/// teaching error that names the exact `{key,value}` array shape.
///
/// TB-2c: a naive client often sends `env` as a JSON map
/// (`{"FOO":"bar"}`). serde's default message ("invalid type: map,
/// expected a sequence") does not tell the client what shape to use.
/// We intercept the map case here and return the remedy inline so the
/// client can self-correct in one step. Every other shape error still
/// falls through to serde's own message.
fn deserialize_env<'de, D>(deserializer: D) -> Result<Vec<EnvEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    // Buffer into an untyped value first so we can detect the map shape
    // before attempting the typed conversion. The payload is already
    // bounded by the transport frame cap, so this does not introduce an
    // unbounded allocation.
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Array(_) => serde_json::from_value(value).map_err(D::Error::custom),
        serde_json::Value::Object(_) => Err(D::Error::custom(
            "env must be an ARRAY of {\"key\":\"NAME\",\"value\":\"VAL\"} objects, not a map; \
             e.g. [{\"key\":\"FOO\",\"value\":\"bar\"}]",
        )),
        other => Err(D::Error::custom(format!(
            "env must be an ARRAY of {{\"key\":\"NAME\",\"value\":\"VAL\"}} objects; \
             got {}; e.g. [{{\"key\":\"FOO\",\"value\":\"bar\"}}]",
            json_type_name(&other)
        ))),
    }
}

/// Human-readable JSON type name for teaching errors.
const fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "a map",
    }
}

/// Deserialize the `argv` parameter as an array of strings, mapping
/// serde's bare `invalid type: ...` into a field-prefixed teaching
/// error that names the exact array-of-strings shape with an example.
///
/// TB-11: a naive client often sends `argv` as a single shell-style
/// string (`"node -e ..."`) or a map. serde's default message
/// ("invalid type: string, expected a sequence") does not name the
/// field or the remedy. We intercept the non-array cases here and
/// return the example inline so the client self-corrects in one step.
/// A genuine array still falls through to serde's element-level error.
fn deserialize_argv<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    // Buffer into an untyped value first so we can detect the non-array
    // shape before the typed conversion. The payload is already bounded
    // by the transport frame cap, so this adds no unbounded allocation.
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Array(_) => serde_json::from_value(value).map_err(|e| {
            D::Error::custom(format!(
                "argv must be an array of strings, e.g. [\"node\",\"-e\",\"...\"]; {e}"
            ))
        }),
        other => Err(D::Error::custom(format!(
            "argv must be an array of strings, e.g. [\"node\",\"-e\",\"...\"]; got {}",
            json_type_name(&other)
        ))),
    }
}

/// Lenient, LLM-friendly shorthand for an inline rule (TC erg2 P1).
///
/// Every field is optional except that a rule needs SOME matcher
/// (`pattern` or `keywords`); the rest get sane, overridable defaults via
/// [`RuleInput::finalize`]. `kind`/`severity` are typed as `String` (not
/// the core enums) so this struct can derive `JsonSchema` without
/// `crates/core` taking a schemars dependency; both are parsed in
/// `finalize` with a teaching error that names the legal set.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleInput {
    /// Regex pattern. Presence (without `keywords`) infers `kind=regex`.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Keyword set. Presence (without `pattern`) infers `kind=keyword`.
    #[serde(default)]
    pub keywords: Option<Vec<String>>,
    /// Override the inferred kind. Live kinds: `keyword`, `regex`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Severity. Default `info`. One of trace/debug/info/low/medium/
    /// high/critical.
    #[serde(default)]
    pub severity: Option<String>,
    /// Summary shown per match. Default `${line}` (the matched line).
    /// Use `${name}` to interpolate a named capture.
    #[serde(default)]
    pub summary_template: Option<String>,
    /// Event kind label. Default `match`.
    #[serde(default)]
    pub event_kind: Option<String>,
    /// Human id. Default auto-minted `inline-<n>`.
    #[serde(default)]
    pub id: Option<String>,
    /// Rule version. Default 1.
    #[serde(default)]
    pub version: Option<u32>,
    /// Named captures referenced by `summary_template`.
    #[serde(default)]
    pub captures: Vec<String>,
    /// Stream filter (`stdout`/`stderr`); omit for any.
    #[serde(default)]
    pub stream: Option<String>,
    /// Capture names to redact from emitted events.
    #[serde(default)]
    pub redact: Vec<String>,
    /// Free-text tags (lowercase).
    #[serde(default)]
    pub tags: Vec<String>,
}

impl RuleInput {
    /// Resolve this shorthand into a full, validated-ready
    /// [`RuleDefinition`], applying defaults only where a field is
    /// absent and inferring `kind` from the matcher. `index` seeds the
    /// auto-minted id. Returns a one-line teaching error (never serde's
    /// field-by-field text) on any unresolvable input.
    fn finalize(self, index: usize) -> Result<RuleDefinition, String> {
        let has_pattern = self.pattern.as_ref().is_some_and(|p| !p.is_empty());
        let has_keywords = self.keywords.as_ref().is_some_and(|k| !k.is_empty());

        // Infer or validate kind.
        let kind = match self.kind.as_deref() {
            Some("keyword") => RuleType::Keyword,
            Some("regex") => RuleType::Regex,
            Some(other) => {
                return Err(format!(
                    "kind '{other}' is not a live rule kind; live kinds: keyword, regex \
                     (example: {{\"pattern\": \"ERROR\"}})"
                ));
            }
            None => {
                if has_pattern && has_keywords {
                    return Err(
                        "rule has both `pattern` and `keywords`; set `kind` to disambiguate \
                         (example: {\"pattern\": \"ERROR\"})"
                            .to_owned(),
                    );
                } else if has_pattern {
                    RuleType::Regex
                } else if has_keywords {
                    RuleType::Keyword
                } else {
                    return Err(
                        "rule needs a matcher: provide `pattern` (regex) or `keywords` \
                         (example: {\"pattern\": \"ERROR\"})"
                            .to_owned(),
                    );
                }
            }
        };

        let severity = match self.severity.as_deref() {
            None => Severity::Info,
            Some(s) => parse_severity_filter(s).map_err(|_| {
                format!(
                    "severity '{s}' is not valid; one of: trace, debug, info, low, medium, \
                     high, critical"
                )
            })?,
        };

        let stream = match self.stream.as_deref() {
            None => None,
            Some("stdout") => Some(terminal_commander_core::SourceStream::Stdout),
            Some("stderr") => Some(terminal_commander_core::SourceStream::Stderr),
            Some(other) => {
                return Err(format!(
                    "stream '{other}' is not valid; one of: stdout, stderr (or omit for any)"
                ));
            }
        };

        Ok(RuleDefinition {
            id: self.id.unwrap_or_else(|| format!("inline-{index}")),
            version: self.version.unwrap_or(1),
            kind,
            // Inline rules are bound directly to a job/watch and must run
            // immediately, so they ship Active (not the Draft default).
            status: terminal_commander_core::RuleStatus::Active,
            severity,
            event_kind: self.event_kind.unwrap_or_else(|| "match".to_owned()),
            stream,
            description: None,
            pattern: self.pattern,
            keywords: self.keywords,
            captures: self.captures,
            summary_template: self
                .summary_template
                .unwrap_or_else(|| "${line}".to_owned()),
            tags: self.tags,
            rate_limit_per_min: None,
            redact: self.redact,
            context_hint: terminal_commander_core::ContextHint::default(),
            examples: vec![],
        })
    }
}

/// Parse the optional MCP-supplied bucket config + inline rules into
/// their daemon-side types. Rules may arrive two ways (both accepted;
/// they are mutually exclusive per call):
/// - `rules`: typed shorthand objects (TC erg2 P1) — the preferred,
///   schema-visible path. `{"pattern":"ERROR"}` is a complete rule.
/// - `rules_json`: a JSON-string array of full `RuleDefinition`s — the
///   original wire form, retained for backward compatibility.
///
/// `None`/absent inputs yield `(None, vec![])`. Errors are reported as a
/// single MCP `invalid_params` so the start tools fail fast with one
/// teaching message instead of silently dropping intent.
fn parse_bucket_and_rules(
    bucket_config_json: Option<String>,
    rules: Option<Vec<RuleInput>>,
    rules_json: Option<String>,
) -> Result<(Option<BucketConfig>, Vec<RuleDefinition>), McpError> {
    let bucket_config = bucket_config_json
        .map(|raw| {
            serde_json::from_str::<BucketConfig>(&raw)
                .map_err(|e| invalid_params(format!("bucket_config_json: {e}")))
        })
        .transpose()?;

    let has_typed = rules.as_ref().is_some_and(|v| !v.is_empty());
    let has_json = rules_json.as_ref().is_some_and(|s| !s.is_empty());
    if has_typed && has_json {
        return Err(invalid_params(
            "provide rules via `rules` (typed) OR `rules_json` (string), not both".to_owned(),
        ));
    }

    let resolved = if has_typed {
        rules
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                r.finalize(i)
                    .map_err(|e| invalid_params(format!("rules[{i}]: {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        rules_json
            .map(|raw| {
                serde_json::from_str::<Vec<RuleDefinition>>(&raw)
                    .map_err(|e| invalid_params(format!("rules_json: {e}")))
            })
            .transpose()?
            .unwrap_or_default()
    };
    Ok((bucket_config, resolved))
}

/// Mint a fresh, process-unique, monotonic in-flight dedup nonce for a
/// `command_start_combed` / `run_and_watch` start (TC-2).
///
/// No new dependency and no random crate: a per-process counter combined
/// with this process id and the variant tag is unique enough for an
/// in-flight collapse hint. The value need not be unpredictable -- the
/// daemon only uses it to recognize the SAME re-sent logical start; the
/// real cross-client safety comes from the daemon-side peer-scoped
/// fallback. Because every call increments the counter, two distinct
/// tool calls get distinct nonces and never collapse.
fn fresh_dedup_nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = NONCE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("mcp-{}-{}", std::process::id(), seq)
}

/// MCP-facing parameters for `command_start_combed`. Strings + ints
/// only so the JSON Schema stays consumer-friendly. Translated to the
/// daemon-side `CommandStartParams` in `into_ipc`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpCommandStartParams {
    /// Non-empty argv as an array of strings, e.g.
    /// `["node","-e","..."]`. argv[0] is the program; the rest are args.
    /// Shell interpreters are rejected by the daemon.
    #[serde(deserialize_with = "deserialize_argv")]
    #[schemars(with = "Vec<String>")]
    pub argv: Vec<String>,
    /// Optional working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Environment as an ARRAY of {"key":"NAME","value":"VAL"} objects
    /// (NOT a map), e.g. `[{"key":"FOO","value":"bar"}]`.
    /// Empty/omitted = inherit the daemon's full environment; entries
    /// you provide are ADDED to (or override) that inherited environment
    /// (merged, not replaced) -- PATH, SystemRoot, etc. are kept.
    #[serde(default, deserialize_with = "deserialize_env")]
    #[schemars(with = "Vec<EnvEntry>")]
    pub env: Vec<EnvEntry>,
    /// Optional grace window between graceful and forced terminate,
    /// in milliseconds. Clamped at the daemon.
    #[serde(default)]
    pub grace_ms: Option<u64>,
    /// Optional per-job bucket override as a JSON object
    /// `{ "max_events": N, "ttl": <seconds> }`. Omit for daemon defaults.
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    /// Inline rules bound to this job only — no prior `registry_activate`
    /// required. Each rule is a small object; the only required field is a
    /// matcher. Minimal example: `[{"pattern": "ERROR"}]` (a live regex
    /// rule whose summary echoes the matched line). Optional per rule:
    /// `keywords`, `kind` (keyword|regex, inferred from the matcher),
    /// `severity` (default info), `summary_template` (default `${line}`),
    /// `event_kind`, `captures`, `stream`, `redact`, `tags`, `id`,
    /// `version`. Omit for none.
    #[serde(default)]
    pub rules: Option<Vec<RuleInput>>,
    /// Deprecated string form of `rules`: a JSON-array string of full
    /// `RuleDefinition`s. Prefer the typed `rules` field. Omit for none.
    #[serde(default)]
    pub rules_json: Option<String>,
    /// Optional per-bucket tag (Phase 3). Tag this probe so a subscription
    /// opened with a matching `tag` predicate routes to it. Omit for none.
    #[serde(default)]
    pub tag: Option<String>,
}

impl McpCommandStartParams {
    fn into_ipc(self) -> Result<CommandStartParams, McpError> {
        let cwd = self.cwd.map(std::path::PathBuf::from);
        let env: Vec<(String, String)> = self.env.into_iter().map(|e| (e.key, e.value)).collect();
        let (bucket_config, rules) =
            parse_bucket_and_rules(self.bucket_config_json, self.rules, self.rules_json)?;
        Ok(CommandStartParams {
            environment: None,
            argv: self.argv,
            cwd,
            env,
            bucket_config,
            rules,
            grace_ms: self.grace_ms,
            tag: self.tag,
            // TC-2 dedup split: the adapter ALWAYS mints a FRESH per-call
            // nonce. Two deliberate identical tool calls therefore get
            // DISTINCT nonces and NEVER collapse to one job (the
            // never-collapse invariant holds structurally for this new
            // adapter). The daemon-side nonce-less fallback window only
            // protects OLD adapter binaries that blind-retry a mutating
            // start without a nonce; a same-nonce collapse here happens
            // only on a transport-layer re-send that reuses this exact
            // value, which this adapter no longer does after Phase 1.
            dedup_nonce: Some(fresh_dedup_nonce()),
        })
    }
}

/// MCP-facing parameters for `command_status`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpCommandStatusParams {
    /// Opaque job id returned by `command_start_combed` /
    /// `run_and_watch` (e.g. `job_<32hex>`); copy it verbatim, not
    /// free-form.
    pub job_id: String,
}

/// Default wait budget for `run_and_watch`, in milliseconds.
const RUN_AND_WATCH_DEFAULT_WAIT_MS: u64 = 5_000;
/// Hard cap on the `run_and_watch` wait budget, in milliseconds.
const RUN_AND_WATCH_MAX_WAIT_MS: u64 = 60_000;
/// Default cap on signals returned by `run_and_watch`.
const RUN_AND_WATCH_DEFAULT_MAX_SIGNALS: usize = 50;
/// Hard cap on signals returned by `run_and_watch`.
const RUN_AND_WATCH_MAX_SIGNALS: usize = 500;
/// Per-iteration `bucket_wait` slice for the `run_and_watch` loop, in ms.
/// The loop runs `ceil(wait_ms / slice)` iterations; a smaller slice
/// makes the loop exit sooner after the command becomes terminal.
const MAX_WAIT_SLICE_MS: u64 = 1_000;

/// MCP-facing parameters for `run_and_watch`. A superset of
/// `command_start_combed`'s params plus the bounded wait controls.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRunAndWatchParams {
    /// Non-empty argv as an array of strings, e.g.
    /// `["node","-e","..."]`. argv[0] is the program; the rest are args.
    #[serde(deserialize_with = "deserialize_argv")]
    #[schemars(with = "Vec<String>")]
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Environment as an ARRAY of {"key":"NAME","value":"VAL"} objects
    /// (NOT a map), e.g. `[{"key":"FOO","value":"bar"}]`.
    /// Empty/omitted = inherit the daemon's full environment; entries
    /// you provide are ADDED to (or override) that inherited environment
    /// (merged, not replaced) -- PATH, SystemRoot, etc. are kept.
    #[serde(default, deserialize_with = "deserialize_env")]
    #[schemars(with = "Vec<EnvEntry>")]
    pub env: Vec<EnvEntry>,
    #[serde(default)]
    pub grace_ms: Option<u64>,
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    /// Inline rules; see `command_start_combed`. Minimal:
    /// `[{"pattern": "ERROR"}]`. Omit to collect no rule signals (you
    /// still get the exit receipt).
    #[serde(default)]
    pub rules: Option<Vec<RuleInput>>,
    /// Deprecated string form of `rules`. Prefer `rules`.
    #[serde(default)]
    pub rules_json: Option<String>,
    /// Max time to wait for signals + exit, in ms. Default 5000, capped
    /// at 60000.
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// Max signals to return. Default 50, capped at 500.
    #[serde(default)]
    pub max_signals: Option<usize>,
}

impl McpRunAndWatchParams {
    /// Split into the start params and the (clamped) wait controls.
    fn into_parts(self) -> (McpCommandStartParams, u64, usize) {
        let wait_ms = self
            .wait_ms
            .unwrap_or(RUN_AND_WATCH_DEFAULT_WAIT_MS)
            .min(RUN_AND_WATCH_MAX_WAIT_MS);
        let max_signals = self
            .max_signals
            .unwrap_or(RUN_AND_WATCH_DEFAULT_MAX_SIGNALS)
            .min(RUN_AND_WATCH_MAX_SIGNALS);
        let start = McpCommandStartParams {
            argv: self.argv,
            cwd: self.cwd,
            env: self.env,
            grace_ms: self.grace_ms,
            bucket_config_json: self.bucket_config_json,
            rules: self.rules,
            rules_json: self.rules_json,
            tag: None,
        };
        (start, wait_ms, max_signals)
    }
}

/// MCP-facing parameters for `command_output_tail`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpCommandOutputTailParams {
    /// Opaque job id returned by `command_start_combed` /
    /// `run_and_watch` (e.g. `job_<32hex>`); copy it verbatim, not
    /// free-form.
    pub job_id: String,
    /// Maximum lines to return. Clamped to 200 server-side.
    #[serde(default)]
    pub max_lines: Option<u32>,
    /// Maximum bytes to return. Clamped to 64 KiB server-side.
    #[serde(default)]
    pub max_bytes: Option<u32>,
}

/// MCP-facing parameters for `bucket_events_since`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpBucketEventsSinceParams {
    /// Opaque bucket id from a prior call (e.g. `bkt_<32hex>`); copy it
    /// verbatim, not free-form.
    pub bucket_id: String,
    /// Pagination cursor; pass `0` on the first call, then the
    /// `next_cursor` from the previous response.
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
    /// Opaque bucket id from a prior call (e.g. `bkt_<32hex>`); copy it
    /// verbatim, not free-form.
    pub bucket_id: String,
    /// Pagination cursor; pass `0` on the first call, then the
    /// `next_cursor` from the previous response.
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
    /// Opaque bucket id from a prior call (e.g. `bkt_<32hex>`); copy it
    /// verbatim, not free-form.
    pub bucket_id: String,
}

/// MCP-facing parameters for `event_context`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpEventContextParams {
    /// Opaque bucket id from a prior call (e.g. `bkt_<32hex>`); copy it
    /// verbatim, not free-form.
    pub bucket_id: String,
    /// Opaque event id from a bucket read (e.g. `evt_<32hex>`); copy it
    /// verbatim, not free-form.
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
        "frames_suppressed": s.frames_suppressed,
        "frames_suppressed_progress": s.frames_suppressed_progress,
        "frames_suppressed_dedupe": s.frames_suppressed_dedupe,
        "exit_code": s.exit_code,
        "signal": s.signal,
        "duration_ms": s.duration_ms,
        // No-silence receipt (TCE-ERG-1): null unless the command
        // finished with zero rule-driven events.
        "receipt": s.receipt,
    })
}

fn command_output_tail_payload(r: &CommandOutputTailResponse) -> serde_json::Value {
    serde_json::json!({
        "job_id": r.job_id,
        "lines": r.lines,
        "returned_lines": r.returned_lines,
        "truncated_lines": r.truncated_lines,
        "truncated_bytes": r.truncated_bytes,
        "evicted_frames": r.evicted_frames,
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
    /// JSON-encoded `RuleDefinition` (a STRING, not a nested object).
    ///
    /// REQUIRED fields: `id` (string), `version` (int, e.g. 1), `kind`,
    /// `severity`, `event_kind` (string; the event label emitted on
    /// match, e.g. `"compile_error"`), `summary_template` (string; use
    /// `${line}` to echo the matched line). When `kind` is `regex` you
    /// MUST also supply `pattern`; when `kind` is `keyword` supply
    /// `keywords` (array).
    ///
    /// NOTE: `version` is ASSIGNED by the store (monotonic, latest+1);
    /// any value you send is ignored and overwritten. The assigned
    /// version is what the response returns and what
    /// `registry_activate` / `registry_deactivate` operate on.
    ///
    /// `kind` enum: keyword | regex | prompt | exit_code | stream_marker
    /// | progress_collapse | dedupe | threshold | sequence | anchor |
    /// custom. Only `keyword` and `regex` are live at MVP; the rest
    /// validate only to Draft.
    ///
    /// `severity` enum: trace | debug | info | low | medium | high |
    /// critical.
    ///
    /// New rules default to `status=Draft` (test-only); set
    /// `"status":"active"` in the definition to make the rule eligible
    /// for `registry_activate`.
    ///
    /// Complete kind:regex example that succeeds on the first call:
    /// `definition_json = "{\"id\":\"rust-compile-error\",\"version\":1,\
    /// \"kind\":\"regex\",\"status\":\"active\",\"severity\":\"high\",\
    /// \"event_kind\":\"compile_error\",\"pattern\":\"error\\\\[E[0-9]+\\\\]\",\
    /// \"summary_template\":\"${line}\"}"`.
    ///
    /// Call `registry_get` for the canonical full shape of any stored
    /// rule. The daemon validates regex / keywords / kind before
    /// persisting.
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
    /// REQUIRED scope (TC42c/TC42d). There is no default and it is in the
    /// schema `required[]`: an omitted scope is rejected so a rule is
    /// never silently widened to global. Use `{ "kind": "global" }` for
    /// the common single-agent case (watch every command you start).
    pub scope: McpActivationScope,
}

/// MCP-facing parameters for `registry_import_pack`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryImportPackParams {
    /// Pack name. One of: generic.terminal, apt, cargo, npm, pytest,
    /// gcc, make, cleanup.
    #[schemars(extend("enum" = [
        "generic.terminal", "apt", "cargo", "npm", "pytest", "gcc", "make", "cleanup"
    ]))]
    pub pack: String,
    /// When true, promote the pack's rules to Active and activate them
    /// in `scope` so they take effect immediately. CONDITIONAL contract:
    /// activate=true REQUIRES `scope`; activate=false (the default)
    /// ignores `scope`.
    #[serde(default)]
    pub activate: bool,
    /// Activation scope. REQUIRED when activate=true (no default; an
    /// omitted scope with activate=true is rejected with a scope-required
    /// error, never silently widened to global). Leave it out when
    /// activate=false. `{ "kind": "global" }` is the usual choice for a
    /// single agent watching its own commands.
    #[serde(default)]
    pub scope: Option<McpActivationScope>,
}

/// MCP-facing parameters for `registry_deactivate`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryDeactivateParams {
    pub rule_id: String,
    pub version: u32,
    /// REQUIRED scope (TC42c/TC42d). No default and it is in the schema
    /// `required[]`: an omitted scope is rejected. MUST match the scope
    /// used at activation; deactivating with a different scope will not
    /// close the previously-opened activation row. Use
    /// `{ "kind": "global" }` to close a global activation.
    pub scope: McpActivationScope,
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
    /// Scope discriminant. One of: `global`, `bucket`, `job`, `probe`.
    /// `global` watches every command you start; the other three bind
    /// the rule to a single opaque id (provide the matching `*_id`).
    #[schemars(extend("enum" = ["global", "bucket", "job", "probe"]))]
    pub kind: String,
    /// Opaque bucket id from a prior call (e.g. `bkt_<32hex>`); copy it
    /// verbatim, not free-form. Required when `kind` is `bucket`.
    #[serde(default)]
    pub bucket_id: Option<String>,
    /// Opaque job id from `command_start_combed` / `run_and_watch`
    /// (e.g. `job_<32hex>`); copy it verbatim, not free-form. Required
    /// when `kind` is `job`.
    #[serde(default)]
    pub job_id: Option<String>,
    /// Opaque probe id from `probe_list` / `runtime_state` (e.g.
    /// `prb_<32hex>`); copy it verbatim, not free-form. Required when
    /// `kind` is `probe`.
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
    /// Absolute path to a regular file (e.g. `/home/u/project/Cargo.toml`).
    /// Absolute is required: the daemon has no workspace root, so a
    /// relative path is rejected rather than resolved against the
    /// daemon's working directory.
    pub path: String,
    /// 1-based start line. Omit to read from line 1. A value of `0` is
    /// clamped up to `1` by the daemon (there is no line 0).
    #[serde(default)]
    #[schemars(extend("minimum" = 1))]
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
    /// Absolute path to the single regular file to search (e.g.
    /// `/home/u/project/Cargo.toml`). This is a per-file search, not a
    /// directory walk. Absolute is required: the daemon has no workspace
    /// root, so a relative path is rejected rather than resolved against
    /// the daemon's working directory.
    pub path: String,
    /// The match expression: a plain SUBSTRING (literal), NOT a regex.
    /// Regex metacharacters are matched literally. Use `case_insensitive`
    /// to fold ASCII case.
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
    /// Absolute path to the regular file to watch (e.g.
    /// `/home/u/project/build.log`). Absolute is required: the daemon has
    /// no workspace root, so a relative path is rejected rather than
    /// resolved against the daemon's working directory.
    pub path: String,
    /// Follow from beginning (default false = follow-end / tail-like).
    #[serde(default)]
    pub follow_from_beginning: Option<bool>,
    /// Optional per-job bucket override as a JSON object
    /// `{ "max_events": N, "ttl": <seconds> }`. Omit for daemon defaults.
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    /// Inline rules bound to this watch only. Minimal example:
    /// `[{"pattern": "ERROR"}]`. See `command_start_combed` for the full
    /// field list. Omit for none.
    #[serde(default)]
    pub rules: Option<Vec<RuleInput>>,
    /// Deprecated string form of `rules` (JSON-array string of full
    /// `RuleDefinition`s). Prefer `rules`. Omit for none.
    #[serde(default)]
    pub rules_json: Option<String>,
    /// Optional per-bucket tag (Phase 3). Tag this watch so a subscription
    /// opened with a matching `tag` predicate routes to it. Omit for none.
    #[serde(default)]
    pub tag: Option<String>,
}

/// MCP-facing parameters for `file_watch_stop`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpFileWatchStopParams {
    /// Opaque watch id (a job id) returned by `file_watch_start` (e.g.
    /// `job_<32hex>`); copy it verbatim, not free-form.
    pub watch_id: String,
}

// =====================================================================
// TC44: PTY command MCP DTOs.
// =====================================================================

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpPtyCommandStartParams {
    /// Non-empty argv as an array of strings, e.g.
    /// `["node","-e","..."]`. argv[0] is the program; the rest are args.
    /// Shell interpreters denied.
    #[serde(deserialize_with = "deserialize_argv")]
    #[schemars(with = "Vec<String>")]
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Environment as an ARRAY of {"key":"NAME","value":"VAL"} objects
    /// (NOT a map), e.g. `[{"key":"FOO","value":"bar"}]`.
    /// Empty/omitted = inherit the daemon's full environment; entries
    /// you provide are ADDED to (or override) that inherited environment
    /// (merged, not replaced) -- PATH, SystemRoot, etc. are kept.
    #[serde(default, deserialize_with = "deserialize_env")]
    #[schemars(with = "Vec<EnvEntry>")]
    pub env: Vec<EnvEntry>,
    #[serde(default)]
    pub rows: Option<u16>,
    #[serde(default)]
    pub cols: Option<u16>,
    /// Optional per-job bucket override as a JSON object
    /// `{ "max_events": N, "ttl": <seconds> }`. Omit for daemon defaults.
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    /// Inline rules bound to this PTY job only. Minimal example:
    /// `[{"pattern": "ERROR"}]`. See `command_start_combed` for the full
    /// field list. Omit for none.
    #[serde(default)]
    pub rules: Option<Vec<RuleInput>>,
    /// Deprecated string form of `rules` (JSON-array string of full
    /// `RuleDefinition`s). Prefer `rules`. Omit for none.
    #[serde(default)]
    pub rules_json: Option<String>,
    /// Optional per-bucket tag (Phase 3). Tag this PTY job so a subscription
    /// opened with a matching `tag` predicate routes to it. Omit for none.
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpPtyCommandWriteStdinParams {
    /// Opaque job id returned by `pty_command_start` (e.g.
    /// `job_<32hex>`); copy it verbatim, not free-form.
    pub job_id: String,
    /// UTF-8 stdin payload. Capped at 4096 bytes by the daemon.
    pub bytes: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpPtyCommandStopParams {
    /// Opaque job id returned by `pty_command_start` (e.g.
    /// `job_<32hex>`); copy it verbatim, not free-form.
    pub job_id: String,
}

// =====================================================================
// TC45: aggregate runtime view MCP DTOs.
// =====================================================================

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpProbeStatusParams {
    /// Opaque probe id from `probe_list` / `runtime_state` (e.g.
    /// `prb_<32hex>`); copy it verbatim, not free-form.
    pub probe_id: String,
}

// =====================================================================
// Bounded-ledger + subscriptions MCP DTOs.
// =====================================================================

/// Shared `limit` param for the bounded list snapshots (`runtime_state`,
/// `probe_list`, `registry_list_active`). Clamped daemon-side to
/// `MAX_LIST_LIMIT`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct McpListLimitParams {
    /// Max rows per list (each of runtime_state's three vecs bounds
    /// independently). Omit for the daemon default/cap. Over-cap sets the
    /// response `truncated` flag.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// MCP-facing per-bucket routing selector for `subscription_open`.
///
/// Mirrors the wire `SubscriptionSourceSel` but takes wire-form id STRINGS so
/// an agent passes the same ids it sees in `probe_list` / `runtime_state`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum McpSubscriptionSourceSel {
    /// Every bucket; future matching probes auto-join.
    #[default]
    All,
    /// A fixed set of owning job ids (`job_<hex>`).
    Jobs { jobs: Vec<String> },
    /// A fixed set of bucket ids (`bkt_<hex>`).
    Buckets { buckets: Vec<String> },
    /// A fixed set of owning probe ids (`prb_<hex>`).
    Probes { probes: Vec<String> },
}

impl McpSubscriptionSourceSel {
    /// Resolve into the wire selector, parsing each id string.
    fn into_wire(self) -> Result<SubscriptionSourceSel, String> {
        use terminal_commander_core::ids::{BucketIdKind, JobIdKind, ProbeIdKind};
        match self {
            Self::All => Ok(SubscriptionSourceSel::All),
            Self::Jobs { jobs } => {
                let parsed = jobs
                    .iter()
                    .map(|s| parse_id::<JobIdKind>("sources.jobs", s))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(SubscriptionSourceSel::Jobs { jobs: parsed })
            }
            Self::Buckets { buckets } => {
                let parsed = buckets
                    .iter()
                    .map(|s| parse_id::<BucketIdKind>("sources.buckets", s))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(SubscriptionSourceSel::Buckets { buckets: parsed })
            }
            Self::Probes { probes } => {
                let parsed = probes
                    .iter()
                    .map(|s| parse_id::<ProbeIdKind>("sources.probes", s))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(SubscriptionSourceSel::Probes { probes: parsed })
            }
        }
    }
}

/// MCP-facing parameters for `subscription_open`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpSubscriptionOpenParams {
    /// Minimum severity (per-EVENT): one of
    /// trace|debug|info|low|medium|high|critical. Omit for no severity floor.
    #[serde(default)]
    pub severity_min: Option<String>,
    /// Event-kind allowlist (per-EVENT), e.g. `["error","panic"]`. Omit for
    /// any kind.
    #[serde(default)]
    pub kind: Option<Vec<String>>,
    /// Per-BUCKET routing. Omit for `{ "kind": "all" }` (every source,
    /// future probes auto-join).
    #[serde(default)]
    pub sources: McpSubscriptionSourceSel,
    /// Per-BUCKET tag AND-filter. Omit to ignore the tag dimension; set it to
    /// route only to probes started with a matching `tag`.
    #[serde(default)]
    pub tag: Option<String>,
}

impl McpSubscriptionOpenParams {
    /// Build the wire predicate, validating severity + ids before any IPC.
    fn into_predicate(self) -> Result<SubscriptionPredicate, String> {
        let severity_min = match self.severity_min.as_deref() {
            None => None,
            Some(s) => Some(parse_severity_filter(s)?),
        };
        let sources = self.sources.into_wire()?;
        Ok(SubscriptionPredicate {
            severity_min,
            kind: self.kind,
            sources,
            tag: self.tag,
        })
    }
}

/// MCP-facing parameters for `subscription_pull`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpSubscriptionPullParams {
    /// Opaque sub_id from `subscription_open`; copy it verbatim.
    pub sub_id: String,
    /// Max events to return. Omit for the daemon default/cap (50).
    #[serde(default)]
    pub max: Option<usize>,
    /// Blocking timeout in milliseconds. Omit for 5000; clamped to
    /// [1, 8000]. The MCP client waits longer (12 s) so an idle pull is
    /// SUCCESS empty+liveness, never a timeout error.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// MCP-facing parameters for `subscription_list`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct McpSubscriptionListParams {
    /// Max rows. Omit for the daemon default/cap (64). Over-cap sets
    /// `truncated`.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// MCP-facing parameters for `subscription_close`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpSubscriptionCloseParams {
    /// Opaque sub_id to close; copy it verbatim. An unknown id returns
    /// closed: false (idempotent).
    pub sub_id: String,
}

/// MCP-facing parameters for `subscription_seek`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpSubscriptionSeekParams {
    /// Opaque sub_id from subscription_open; copy it verbatim.
    pub sub_id: String,
    /// Bucket id (`bkt_<hex>`) to reposition within.
    pub bucket_id: String,
    /// Requested re-read position; clamped to the bucket's live range. A
    /// position below the surviving head returns lagged=true (never an error).
    pub seq: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- TC ergonomics Phase 2 (P1): de-stringed rule input ---

    fn finalize_one(json: &str) -> Result<RuleDefinition, String> {
        let input: RuleInput = serde_json::from_str(json).expect("RuleInput must deserialize");
        input.finalize(0)
    }

    #[test]
    fn shorthand_pattern_only_is_a_live_regex_rule() {
        let def = finalize_one(r#"{"pattern": "ERROR"}"#).unwrap();
        assert_eq!(def.kind, RuleType::Regex);
        assert_eq!(def.status, terminal_commander_core::RuleStatus::Active);
        assert_eq!(def.severity, Severity::Info);
        assert_eq!(def.event_kind, "match");
        assert_eq!(def.summary_template, "${line}");
        assert_eq!(def.version, 1);
        assert_eq!(def.id, "inline-0");
        // The whole point: it validates AND is runtime-eligible.
        def.validate().expect("shorthand rule must validate");
        assert!(def.status.is_runtime_eligible());
    }

    #[test]
    fn shorthand_keywords_only_infers_keyword_kind() {
        let def = finalize_one(r#"{"keywords": ["panic", "FAILED"]}"#).unwrap();
        assert_eq!(def.kind, RuleType::Keyword);
        def.validate().unwrap();
    }

    #[test]
    fn shorthand_no_matcher_is_a_teaching_error() {
        let err = finalize_one(r#"{"severity": "high"}"#).unwrap_err();
        assert!(err.contains("matcher"), "{err}");
        assert!(err.contains("{\"pattern\": \"ERROR\"}"), "{err}");
    }

    #[test]
    fn shorthand_both_matchers_requires_explicit_kind() {
        let err = finalize_one(r#"{"pattern": "a", "keywords": ["b"]}"#).unwrap_err();
        assert!(err.contains("kind"), "{err}");
        // ...but explicit kind disambiguates.
        let def = finalize_one(r#"{"pattern": "a", "keywords": ["b"], "kind": "regex"}"#).unwrap();
        assert_eq!(def.kind, RuleType::Regex);
    }

    #[test]
    fn shorthand_reserved_kind_teaches_live_set() {
        let err = finalize_one(r#"{"keywords": ["x"], "kind": "exit_code"}"#).unwrap_err();
        assert!(err.contains("keyword, regex"), "{err}");
    }

    #[test]
    fn shorthand_bad_severity_teaches() {
        let err = finalize_one(r#"{"pattern": "x", "severity": "spicy"}"#).unwrap_err();
        assert!(err.contains("trace") && err.contains("critical"), "{err}");
    }

    #[test]
    fn shorthand_overrides_are_honored() {
        let def = finalize_one(
            r#"{"pattern":"E(?P<code>[0-9]+)","kind":"regex","severity":"high",
                "summary_template":"err ${code}","event_kind":"compile_error",
                "captures":["code"],"stream":"stderr","id":"my-rule","version":3,
                "tags":["build"]}"#,
        )
        .unwrap();
        assert_eq!(def.severity, Severity::High);
        assert_eq!(def.summary_template, "err ${code}");
        assert_eq!(def.event_kind, "compile_error");
        assert_eq!(def.id, "my-rule");
        assert_eq!(def.version, 3);
        assert_eq!(
            def.stream,
            Some(terminal_commander_core::SourceStream::Stderr)
        );
        def.validate().unwrap();
    }

    #[test]
    fn env_array_of_objects_deserializes() {
        // TB-2: the documented array-of-{key,value} shape must parse.
        let params: McpCommandStartParams = serde_json::from_str(
            r#"{"argv":["echo"],"env":[{"key":"FOO","value":"bar"},{"key":"BAZ","value":"qux"}]}"#,
        )
        .expect("array-of-objects env should deserialize");
        assert_eq!(params.env.len(), 2);
        assert_eq!(params.env[0].key, "FOO");
        assert_eq!(params.env[0].value, "bar");
    }

    #[test]
    fn env_omitted_defaults_to_empty() {
        // TB-2: omitted env stays empty (inherit semantics downstream).
        let params: McpCommandStartParams =
            serde_json::from_str(r#"{"argv":["echo"]}"#).expect("omitted env should default");
        assert!(params.env.is_empty());
    }

    #[test]
    fn env_as_map_teaches_key_value_array_shape() {
        // TB-2c: the common map mistake must surface the remedy, not
        // serde's bare "invalid type: map, expected a sequence".
        let err = serde_json::from_str::<McpCommandStartParams>(
            r#"{"argv":["echo"],"env":{"FOO":"bar"}}"#,
        )
        .expect_err("map-form env must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("ARRAY") && msg.contains("\"key\"") && msg.contains("\"value\""),
            "env map error must name the {{key,value}} array shape: {msg}"
        );
        assert!(
            msg.contains("not a map"),
            "env map error must call out the map mistake: {msg}"
        );
    }

    #[test]
    fn env_teaching_error_applies_to_run_and_watch_and_pty() {
        // TB-2: the same teaching error covers every env-bearing tool.
        let raw = r#"{"argv":["echo"],"env":{"FOO":"bar"}}"#;
        let raw_pty = r#"{"argv":["bash"],"env":{"FOO":"bar"}}"#;
        let e1 = serde_json::from_str::<McpRunAndWatchParams>(raw)
            .expect_err("run_and_watch map env must be rejected")
            .to_string();
        let e2 = serde_json::from_str::<McpPtyCommandStartParams>(raw_pty)
            .expect_err("pty_command_start map env must be rejected")
            .to_string();
        for msg in [&e1, &e2] {
            assert!(
                msg.contains("ARRAY") && msg.contains("not a map"),
                "env teaching error missing in: {msg}"
            );
        }
    }

    #[test]
    fn full_rule_definition_json_still_parses_as_rule_input() {
        // TC05 wire-compat: a full RuleDefinition payload (superset) must
        // deserialize through RuleInput and finalize to an equivalent rule.
        let full = r#"{
            "id": "apt-missing", "version": 2, "kind": "regex", "severity": "high",
            "event_kind": "missing_package", "stream": "stderr",
            "pattern": "Unable to locate package (?P<package>[a-z0-9-]+)",
            "captures": ["package"], "summary_template": "missing ${package}",
            "tags": ["apt"], "redact": []
        }"#;
        let def = finalize_one(full).unwrap();
        assert_eq!(def.id, "apt-missing");
        assert_eq!(def.version, 2);
        assert_eq!(def.kind, RuleType::Regex);
        assert_eq!(def.severity, Severity::High);
        assert_eq!(def.event_kind, "missing_package");
        assert_eq!(def.summary_template, "missing ${package}");
        def.validate().unwrap();
    }

    #[test]
    fn parse_rejects_both_typed_and_string_rules() {
        let typed = vec![RuleInput {
            pattern: Some("x".to_owned()),
            ..RuleInput::default()
        }];
        let err = parse_bucket_and_rules(None, Some(typed), Some(r#"[{"id":"a"}]"#.to_owned()))
            .unwrap_err();
        // McpError Display contains the message.
        assert!(format!("{err:?}").contains("not both"), "{err:?}");
    }

    #[test]
    fn parse_typed_rules_finalize_end_to_end() {
        let typed = vec![RuleInput {
            pattern: Some("ERROR".to_owned()),
            ..RuleInput::default()
        }];
        let (_, rules) = parse_bucket_and_rules(None, Some(typed), None).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].kind, RuleType::Regex);
        assert!(rules[0].status.is_runtime_eligible());
    }

    #[test]
    fn parse_legacy_rules_json_string_still_works() {
        // Back-compat: the old stringified full-RuleDefinition array path.
        let raw = r#"[{"id":"k","version":1,"kind":"keyword","severity":"medium",
            "event_kind":"kw","keywords":["needle"],"summary_template":"hit"}]"#;
        let (_, rules) = parse_bucket_and_rules(None, None, Some(raw.to_owned())).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].kind, RuleType::Keyword);
    }

    #[test]
    fn catalogue_lists_thirty_seven_live_tools() {
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
                "run_and_watch",
                "command_status",
                "command_output_tail",
                "bucket_events_since",
                "bucket_wait",
                "bucket_summary",
                "event_context",
                "registry_search",
                "registry_get",
                "registry_upsert",
                "registry_test",
                "registry_activate",
                "registry_import_pack",
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
                "subscription_open",
                "subscription_pull",
                "subscription_list",
                "subscription_close",
                "subscription_seek",
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
                "command_output_tail".to_owned(),
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
                "registry_import_pack".to_owned(),
                "registry_list_active".to_owned(),
                "registry_search".to_owned(),
                "registry_test".to_owned(),
                "registry_upsert".to_owned(),
                "run_and_watch".to_owned(),
                "runtime_state".to_owned(),
                "self_check".to_owned(),
                "subscription_close".to_owned(),
                "subscription_list".to_owned(),
                "subscription_open".to_owned(),
                "subscription_pull".to_owned(),
                "subscription_seek".to_owned(),
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

            // TB-1: a PTY tool on a host without a PTY runtime is
            // platform-blocked; that reason takes precedence over
            // daemon reachability since the tool can only ever return
            // UnsupportedPlatform on this host.
            let platform_blocked = is_pty_tool(tool.name) && !pty_runtime_available();

            if !expected_requires_daemon {
                assert!(tool.available, "{} should remain callable", tool.name);
                assert_eq!(tool.unavailable_reason, None);
            } else if platform_blocked {
                assert!(
                    !tool.available,
                    "{} should be unavailable without a PTY runtime",
                    tool.name
                );
                assert_eq!(tool.unavailable_reason, Some(PTY_UNAVAILABLE_REASON));
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

    /// TB-1 regression: on a host without a PTY runtime (non-unix; ConPTY
    /// pending) the four `pty_*` tools MUST be reported `available: false`
    /// with the platform reason -- even when the daemon is reachable. They
    /// must never advertise a live PTY surface the daemon can only reject
    /// with `UnsupportedPlatform`.
    #[cfg(not(unix))]
    #[test]
    fn pty_tools_unavailable_on_unsupported_platform() {
        // daemon_available = true on purpose: the platform block must
        // fire regardless of daemon reachability.
        let tools = discovered_tools(true);
        let pty_names = [
            "pty_command_start",
            "pty_command_write_stdin",
            "pty_command_stop",
            "pty_command_list",
        ];
        for name in pty_names {
            let tool = tools
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name} missing from discovered tools"));
            assert!(
                !tool.available,
                "{name} must be unavailable without a PTY runtime"
            );
            assert_eq!(
                tool.unavailable_reason,
                Some(PTY_UNAVAILABLE_REASON),
                "{name} must surface the PTY platform reason"
            );
        }
    }

    /// TB-1 companion: on a unix host with the daemon reachable the PTY
    /// tools ARE available (the platform block does not fire). This keeps
    /// the fix narrow -- it never suppresses a real PTY surface.
    #[cfg(unix)]
    #[test]
    fn pty_tools_available_on_supported_platform_with_daemon() {
        let tools = discovered_tools(true);
        for name in [
            "pty_command_start",
            "pty_command_write_stdin",
            "pty_command_stop",
            "pty_command_list",
        ] {
            let tool = tools
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name} missing from discovered tools"));
            assert!(tool.available, "{name} should be available on unix");
            assert_eq!(tool.unavailable_reason, None);
        }
    }

    // --- TB-5: scope is machine-readable REQUIRED on activate/deactivate ---

    /// Collect the `required[]` names from a generated JSON schema.
    fn schema_required(schema: &schemars::Schema) -> Vec<String> {
        schema
            .as_value()
            .pointer("/required")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn activate_schema_lists_scope_as_required() {
        // TB-5: the prose says scope is REQUIRED; the machine-readable
        // schema must agree so a naive LLM is told up front, not after a
        // ScopeInvalid round-trip.
        let schema = schemars::schema_for!(McpRegistryActivateParams);
        let required = schema_required(&schema);
        assert!(
            required.iter().any(|f| f == "scope"),
            "registry_activate schema must list scope in required[]; got {required:?}"
        );
        assert!(
            required.iter().any(|f| f == "rule_id"),
            "rule_id must remain required; got {required:?}"
        );
        // version stays optional (latest-version sugar).
        assert!(
            !required.iter().any(|f| f == "version"),
            "version must stay optional; got {required:?}"
        );
    }

    #[test]
    fn deactivate_schema_lists_scope_as_required() {
        let schema = schemars::schema_for!(McpRegistryDeactivateParams);
        let required = schema_required(&schema);
        for field in ["rule_id", "version", "scope"] {
            assert!(
                required.iter().any(|f| f == field),
                "registry_deactivate schema must list {field} in required[]; got {required:?}"
            );
        }
    }

    #[test]
    fn activate_without_scope_fails_deserialization() {
        // TB-5: dropping the Option means an omitted scope is rejected at
        // the wire-form boundary (no silent widen to global).
        let err = serde_json::from_str::<McpRegistryActivateParams>(r#"{"rule_id":"r"}"#)
            .expect_err("activate without scope must fail");
        assert!(
            err.to_string().contains("scope"),
            "missing-scope error must name the scope field; got: {err}"
        );
    }

    #[test]
    fn deactivate_without_scope_fails_deserialization() {
        let err =
            serde_json::from_str::<McpRegistryDeactivateParams>(r#"{"rule_id":"r","version":1}"#)
                .expect_err("deactivate without scope must fail");
        assert!(
            err.to_string().contains("scope"),
            "missing-scope error must name the scope field; got: {err}"
        );
    }

    #[test]
    fn activate_with_global_scope_deserializes() {
        // The happy path the schema teaches must still parse.
        let params = serde_json::from_str::<McpRegistryActivateParams>(
            r#"{"rule_id":"r","scope":{"kind":"global"}}"#,
        )
        .expect("activate with global scope must deserialize");
        assert_eq!(params.rule_id, "r");
        assert_eq!(params.scope.kind, "global");
    }

    #[test]
    fn import_pack_scope_stays_optional_in_schema() {
        // TB-5: import_pack scope is CONDITIONAL (required only when
        // activate=true), so it must NOT be in the unconditional
        // required[]; the conditional is documented + enforced by the
        // daemon teaching error.
        let schema = schemars::schema_for!(McpRegistryImportPackParams);
        let required = schema_required(&schema);
        assert!(
            required.iter().any(|f| f == "pack"),
            "import_pack must require pack; got {required:?}"
        );
        assert!(
            !required.iter().any(|f| f == "scope"),
            "import_pack scope is conditional and must not be unconditionally required; got {required:?}"
        );
        // activate=false (default) with no scope must still parse.
        serde_json::from_str::<McpRegistryImportPackParams>(r#"{"pack":"cargo"}"#)
            .expect("import_pack without activate/scope must deserialize");
    }

    /// Read a string field from a generated JSON schema by JSON pointer.
    fn schema_str<'a>(schema: &'a schemars::Schema, pointer: &str) -> &'a str {
        schema
            .as_value()
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("schema pointer {pointer} is not a string"))
    }

    /// T2 regression: `event_kind` is a hard-REQUIRED RuleDefinition field
    /// (no serde default; `validate()` rejects an empty value), so it MUST
    /// appear in the authoritative REQUIRED-fields enumeration the client
    /// builds from. If it drifts out of the description again, a naive
    /// first `registry_upsert` call fails with `missing field event_kind`
    /// -- exactly the teaching-error-first failure this work eliminates.
    #[test]
    fn upsert_required_fields_list_includes_event_kind() {
        // 1. Live tool-level description (the surface system_discover shows
        //    and the router advertises).
        let router = TerminalCommanderMcpServer::tool_router();
        let upsert = router
            .list_all()
            .into_iter()
            .find(|t| t.name == "registry_upsert")
            .expect("registry_upsert must be a live tool");
        let tool_desc = upsert
            .description
            .as_deref()
            .expect("registry_upsert must carry a description");
        assert!(
            tool_desc.contains("event_kind"),
            "registry_upsert tool description must list event_kind in its REQUIRED set; got: {tool_desc}"
        );

        // 2. schemars-derived definition_json param doc (the schema surface).
        let schema = schemars::schema_for!(McpRegistryUpsertParams);
        let param_desc = schema_str(&schema, "/properties/definition_json/description");
        assert!(
            param_desc.contains("event_kind"),
            "definition_json schema description must list event_kind; got: {param_desc}"
        );
    }

    /// T2 regression: pin the exact serde failure mode. Omitting
    /// `event_kind` (as a client would if it trusted a list that lacked it)
    /// fails deserialization, while the documented worked example -- which
    /// includes event_kind -- both deserializes AND validates. This proves
    /// the REQUIRED list is the load-bearing surface.
    #[test]
    fn upsert_definition_without_event_kind_fails_but_example_succeeds() {
        let missing = r#"{"id":"r","version":1,"kind":"regex","status":"active","severity":"high","pattern":"x","summary_template":"${line}"}"#;
        let err = serde_json::from_str::<RuleDefinition>(missing)
            .expect_err("definition without event_kind must fail deserialization");
        assert!(
            err.to_string().contains("event_kind"),
            "missing-event_kind error must name the field; got: {err}"
        );

        // The exact shape embedded in the tool description as the
        // first-try example.
        let example = r#"{"id":"rust-compile-error","version":1,"kind":"regex","status":"active","severity":"high","event_kind":"compile_error","pattern":"error\\[E[0-9]+\\]","summary_template":"${line}"}"#;
        let def = serde_json::from_str::<RuleDefinition>(example)
            .expect("documented example must deserialize");
        def.validate()
            .expect("documented example must pass validate()");
    }

    fn unavailable_status_server() -> TerminalCommanderMcpServer {
        // M9: unique per-test socket path (pid + nanos). The socket is never
        // bound (this is the unavailable-daemon path), so a collision would
        // still yield "unavailable" — but a shared fixed path under temp_dir is
        // a latent smell. Compute once so the endpoint and the client agree.
        let sock = std::env::temp_dir().join(format!(
            "tc-mcp-unavailable-unit-test-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let status = EnsureDaemonStatus::Unavailable {
            reason: terminal_commander_supervisor::ensure::DaemonUnavailableReason::BinaryNotFound,
            diagnostics: terminal_commander_supervisor::ensure::Diagnostics {
                endpoint: terminal_commander_supervisor::ensure::Endpoint::UnixSocket {
                    path: sock.clone(),
                },
                log_path: None,
                last_error: Some("test daemon unavailable".into()),
                startup_attempted: false,
                startup_elapsed_ms: 0,
            },
        };
        let daemon = McpDaemonClient::with_status(
            sock,
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

    // --- FIX A: run_and_watch completion markers ---

    #[test]
    fn run_and_watch_completion_marks_terminal_states_complete() {
        use terminal_commander_core::JobState;
        // A terminal job is complete and NOT wait-exhausted.
        for state in [JobState::Exited, JobState::Cancelled, JobState::Failed] {
            let (complete, wait_exhausted) = run_and_watch_completion(state);
            assert!(complete, "{state:?} must be complete");
            assert!(
                !wait_exhausted,
                "{state:?} is terminal, must not be wait_exhausted"
            );
        }
    }

    #[test]
    fn run_and_watch_completion_marks_running_wait_exhausted() {
        use terminal_commander_core::JobState;
        // A still-running (or still-starting) job that returns because the
        // wait budget was spent is NOT complete; it is wait_exhausted, so
        // the caller knows to poll command_status for the real exit.
        for state in [JobState::Running, JobState::Starting] {
            let (complete, wait_exhausted) = run_and_watch_completion(state);
            assert!(!complete, "{state:?} is non-terminal, must not be complete");
            assert!(
                wait_exhausted,
                "{state:?} returned non-terminal -> wait_exhausted"
            );
        }
    }

    #[test]
    fn ipc_error_policy_denied_maps_to_invalid_params() {
        let e = IpcError::new(IpcErrorCode::PolicyDenied, "nope");
        let mcp = into_mcp_error(&e);
        assert!(mcp.message.contains("policy_denied") || mcp.message.contains("PolicyDenied"));
    }

    #[test]
    fn caller_fixable_ipc_errors_map_to_invalid_params_server_faults_stay_internal() {
        // -32602 invalid_params => the agent fixes its input and retries.
        // -32603 internal_error => "TC is broken", agent abandons the tool
        // and falls back to raw shell. Only genuine server faults may use
        // -32603; everything an agent can act on must stay invalid_params
        // so trust in Terminal Commander holds. This pins the classification
        // so a future edit to `into_mcp_error` cannot silently regress a
        // caller-fixable code back to internal_error.
        let caller_fixable = [
            IpcErrorCode::FrameTooLarge,
            IpcErrorCode::MalformedJson,
            IpcErrorCode::SchemaMismatch,
            IpcErrorCode::UnknownMethod,
            IpcErrorCode::PolicyDenied,
            IpcErrorCode::BucketNotFound,
            IpcErrorCode::EventNotFound,
            IpcErrorCode::InvalidCursor,
            IpcErrorCode::ShellInterpreterDenied,
            IpcErrorCode::ArgvInvalid,
            IpcErrorCode::UnknownJob,
            IpcErrorCode::RuleNotFound,
            IpcErrorCode::RuleInvalid,
            IpcErrorCode::ScopeInvalid,
            IpcErrorCode::PathDenied,
            IpcErrorCode::FileNotFound,
            IpcErrorCode::FileBinary,
            IpcErrorCode::OversizedRequest,
            IpcErrorCode::UnknownWatch,
            IpcErrorCode::SecretInputDenied,
            IpcErrorCode::UnknownProbe,
            IpcErrorCode::RuleNotActive,
        ];
        for code in caller_fixable {
            let mcp = into_mcp_error(&IpcError::new(code, "x"));
            assert_eq!(
                mcp.code.0, -32602,
                "{code:?} must map to invalid_params so the agent self-corrects"
            );
        }

        let server_fault = [
            IpcErrorCode::Internal,
            IpcErrorCode::PeerCredentialFailure,
            IpcErrorCode::UnsupportedPlatform,
            IpcErrorCode::ShuttingDown,
        ];
        for code in server_fault {
            let mcp = into_mcp_error(&IpcError::new(code, "x"));
            assert_eq!(
                mcp.code.0, -32603,
                "{code:?} is not caller-fixable and must stay internal_error"
            );
        }
    }

    // --- FIX #2: mid-call transport failure -> clean daemon_unavailable envelope ---

    #[test]
    fn transport_failure_maps_to_daemon_unavailable_envelope_not_raw_internal() {
        // A mid-call connect/IO loss (the daemon pipe/socket went away) arrives
        // as an IpcError::transport. `McpDaemonClient::call` already self-healed +
        // retried; reaching `into_mcp_error` means it is still unreachable, so the
        // edge must return the CLEAN daemon_unavailable envelope -- NOT the raw,
        // leaky internal_error that pushes the LLM to raw shell.
        let transport = IpcError::transport("pipe connect: ... os error 2");
        let mcp = into_mcp_error(&transport);
        let rendered = mcp.to_string();
        assert!(
            rendered.contains("daemon_unavailable"),
            "transport failure must surface the daemon_unavailable envelope, got: {rendered}"
        );
        // The raw transport detail must NOT leak (this is the exact regression
        // `assert_daemon_unavailable_tool_error` guards in the integration test).
        assert!(
            !rendered.contains("pipe connect") && !rendered.contains("ipc_code"),
            "transport failure must not leak the raw IPC failure detail, got: {rendered}"
        );
    }

    #[test]
    fn daemon_returned_internal_still_maps_to_internal_error_unchanged() {
        // A daemon-RETURNED Internal error (no transport marker) is a genuine
        // server fault and must KEEP its internal_error (-32603) mapping -- the
        // transport short-circuit must not swallow real daemon faults.
        let daemon_fault = IpcError::new(IpcErrorCode::Internal, "open: permission denied");
        assert!(!daemon_fault.is_transport());
        let mcp = into_mcp_error(&daemon_fault);
        assert_eq!(
            mcp.code.0, -32603,
            "a real daemon-returned Internal fault must stay internal_error"
        );
        let rendered = mcp.to_string();
        assert!(
            !rendered.contains("daemon_unavailable"),
            "a real server fault must NOT be disguised as daemon_unavailable, got: {rendered}"
        );
    }

    /// Source-status: test-only. A MUTATING request that fails on transport must
    /// NOT be told to "retry the tool" -- a blind re-issue could apply the side
    /// effect twice. The envelope must instead instruct the agent to reconcile via
    /// command_status / runtime_state. (TC-1a honest-remedy half.)
    #[test]
    fn mutating_transport_envelope_has_reconcile_remedy_not_retry() {
        let transport = IpcError::transport("pipe connect: ... os error 2");
        let mcp = into_mcp_error_for(false, &transport);

        // Application-level code stays daemon_unavailable.
        assert_eq!(mcp.message, "daemon_unavailable");

        let data = mcp
            .data
            .expect("mutating transport envelope carries a data payload");
        let remedy = data["details"]["remedy"]
            .as_str()
            .expect("remedy is a string");

        assert!(
            !remedy.contains("retry the tool"),
            "the MUTATING remedy must NOT say 'retry the tool' (it could double the \
             side effect); got: {remedy}"
        );
        assert!(
            remedy.contains("command_status") && remedy.contains("runtime_state"),
            "the MUTATING remedy must instruct the agent to reconcile via \
             command_status / runtime_state; got: {remedy}"
        );
        // Operation-neutral: honest for start, stop, and shutdown alike.
        assert!(
            remedy.contains("may or may not have taken effect"),
            "the MUTATING remedy must be operation-neutral; got: {remedy}"
        );

        // The raw OS detail still must not leak.
        let rendered = serde_json::to_string(&data).unwrap();
        assert!(
            !rendered.contains("pipe connect"),
            "the mutating envelope must not leak the raw IPC failure detail; got: {rendered}"
        );
    }

    /// Source-status: test-only. A READ (idempotent) request that fails on
    /// transport keeps the retry remedy -- the adapter already self-healed + retried
    /// once, and a manual re-issue of a pure read is safe.
    #[test]
    fn idempotent_transport_envelope_keeps_retry_remedy() {
        let transport = IpcError::transport("pipe connect: ... os error 2");

        // Both the explicit idempotent path and the default `into_mcp_error`
        // wrapper (which delegates with request_is_idempotent = true) keep the
        // retry remedy.
        for mcp in [
            into_mcp_error_for(true, &transport),
            into_mcp_error(&transport),
        ] {
            assert_eq!(mcp.message, "daemon_unavailable");
            let data = mcp
                .data
                .expect("idempotent transport envelope carries a data payload");
            let remedy = data["details"]["remedy"]
                .as_str()
                .expect("remedy is a string");
            assert!(
                remedy.contains("retry the tool"),
                "the idempotent remedy keeps 'retry the tool'; got: {remedy}"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn down_daemon_mid_call_yields_daemon_unavailable_envelope() {
        // End-to-end at the client edge: a client wired to a socket/pipe that is
        // NOT bound (daemon down) issues a real Health call. The inner client
        // returns a transport failure; `McpDaemonClient::call` self-heals (fails,
        // daemon still down) + retries (still down) and returns a transport error,
        // which `into_mcp_error` renders as the clean daemon_unavailable envelope.
        let sock = std::env::temp_dir().join(format!(
            "tc-mcp-transport-down-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let client = crate::daemon_client::McpDaemonClient::new(sock)
            .with_timeout(std::time::Duration::from_millis(50));
        let result = client
            .call(terminal_commander_ipc::IpcRequest::Health)
            .await;
        let err = result.expect_err("a down daemon must produce a transport error");
        assert!(
            err.is_transport(),
            "mid-call failure against a down daemon must classify as transport, got: {err:?}"
        );
        let mcp = into_mcp_error(&err);
        let rendered = mcp.to_string();
        assert!(
            rendered.contains("daemon_unavailable"),
            "the down-daemon transport error must render as daemon_unavailable, got: {rendered}"
        );
        assert!(
            !rendered.contains("ipc_code"),
            "must not leak the raw ipc failure detail, got: {rendered}"
        );
    }

    // --- T3 (TB-7/8/9/10/11): clarity polish regressions ---

    /// Collect a JSON-Schema `enum` array at `pointer` as owned strings.
    fn schema_enum(schema: &schemars::Schema, pointer: &str) -> Vec<String> {
        schema
            .as_value()
            .pointer(pointer)
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn tb7_scope_kind_is_a_schema_enum() {
        // TB-7: scope.kind must carry a machine-readable enum so a naive
        // LLM picks a legal discriminant on the first call, not after a
        // ScopeInvalid round-trip.
        let schema = schemars::schema_for!(McpActivationScope);
        let kinds = schema_enum(&schema, "/properties/kind/enum");
        assert_eq!(
            kinds,
            vec!["global", "bucket", "job", "probe"],
            "scope.kind must enumerate the four legal discriminants; got {kinds:?}"
        );
    }

    #[test]
    fn tb7_scope_id_descriptions_teach_verbatim_copy() {
        let schema = schemars::schema_for!(McpActivationScope);
        for (field, prefix) in [
            ("bucket_id", "bkt_"),
            ("job_id", "job_"),
            ("probe_id", "prb_"),
        ] {
            let desc = schema_str(&schema, &format!("/properties/{field}/description"));
            assert!(
                desc.contains(prefix) && desc.contains("verbatim"),
                "scope.{field} description must teach the {prefix} prefix and verbatim copy; got: {desc}"
            );
        }
    }

    #[test]
    fn tb8_cursor_descriptions_teach_pagination() {
        for schema in [
            schemars::schema_for!(McpBucketEventsSinceParams),
            schemars::schema_for!(McpBucketWaitParams),
        ] {
            let desc = schema_str(&schema, "/properties/cursor/description");
            assert!(
                desc.contains("first call") && desc.contains("next_cursor"),
                "cursor description must teach the 0-then-next_cursor protocol; got: {desc}"
            );
        }
    }

    #[test]
    fn tb8_bucket_and_event_id_descriptions_carry_format_notes() {
        let summary = schemars::schema_for!(McpBucketSummaryParams);
        let bkt = schema_str(&summary, "/properties/bucket_id/description");
        assert!(
            bkt.contains("bkt_"),
            "bucket_id must teach the bkt_ prefix; got: {bkt}"
        );

        let ctx = schemars::schema_for!(McpEventContextParams);
        let evt = schema_str(&ctx, "/properties/event_id/description");
        assert!(
            evt.contains("evt_"),
            "event_id must teach the evt_ prefix; got: {evt}"
        );
    }

    #[test]
    fn tb9_import_pack_schema_enumerates_all_eight_seed_packs() {
        // TB-9: the schema's pack enum must match the daemon's seed-pack
        // set exactly (8 packs incl. cleanup), so the schema can never
        // again list fewer packs than the daemon accepts.
        let schema = schemars::schema_for!(McpRegistryImportPackParams);
        let packs = schema_enum(&schema, "/properties/pack/enum");
        assert_eq!(
            packs,
            vec![
                "generic.terminal",
                "apt",
                "cargo",
                "npm",
                "pytest",
                "gcc",
                "make",
                "cleanup",
            ],
            "import_pack schema must enumerate all eight seed packs; got {packs:?}"
        );
        assert!(
            packs.iter().any(|p| p == "cleanup"),
            "TB-9: cleanup must be present in the pack enum"
        );
    }

    #[test]
    fn tb10_file_search_params_carry_descriptions() {
        let schema = schemars::schema_for!(McpFileSearchParams);
        let path = schema_str(&schema, "/properties/path/description");
        assert!(
            !path.is_empty(),
            "file_search.path must carry a description"
        );
        let query = schema_str(&schema, "/properties/query/description");
        assert!(
            query.contains("SUBSTRING") && query.contains("NOT a regex"),
            "file_search.query must teach literal-substring (not regex) behavior; got: {query}"
        );
    }

    #[test]
    fn tb11_argv_string_form_teaches_array_shape() {
        // TB-11: a single shell-style string for argv must surface the
        // field-prefixed array example, not serde's bare "invalid type".
        for raw in [
            r#"{"argv":"node -e x"}"#,
            r#"{"argv":{"0":"node"}}"#,
            r#"{"argv":42}"#,
        ] {
            let err = serde_json::from_str::<McpCommandStartParams>(raw)
                .expect_err("non-array argv must be rejected");
            let msg = err.to_string();
            assert!(
                msg.contains("argv must be an array of strings")
                    && msg.contains("[\"node\",\"-e\""),
                "argv teaching error must name the field and example; got: {msg}"
            );
        }
    }

    #[test]
    fn tb11_argv_teaching_error_covers_run_and_watch_and_pty() {
        let e1 = serde_json::from_str::<McpRunAndWatchParams>(r#"{"argv":"node -e x"}"#)
            .expect_err("run_and_watch string argv must be rejected")
            .to_string();
        let e2 = serde_json::from_str::<McpPtyCommandStartParams>(r#"{"argv":"node -e x"}"#)
            .expect_err("pty_command_start string argv must be rejected")
            .to_string();
        for msg in [&e1, &e2] {
            assert!(
                msg.contains("argv must be an array of strings"),
                "argv teaching error missing in: {msg}"
            );
        }
    }

    #[test]
    fn tb11_valid_argv_array_still_parses() {
        let params: McpCommandStartParams =
            serde_json::from_str(r#"{"argv":["node","-e","1"]}"#).expect("array argv must parse");
        assert_eq!(params.argv, vec!["node", "-e", "1"]);
    }

    #[test]
    fn tb11_file_read_window_start_line_schema_has_minimum_one() {
        // TB-11: the "1-based" prose must agree with the schema; a 0 is
        // clamped to 1 by the daemon, so the schema documents minimum 1.
        let schema = schemars::schema_for!(McpFileReadWindowParams);
        let min = schema
            .as_value()
            .pointer("/properties/start_line/minimum")
            .and_then(serde_json::Value::as_u64);
        assert_eq!(
            min,
            Some(1),
            "start_line schema must declare minimum 1 to match the 1-based prose"
        );
        let desc = schema_str(&schema, "/properties/start_line/description");
        assert!(
            desc.contains("clamped"),
            "start_line description must document the 0->1 clamp; got: {desc}"
        );
    }

    #[test]
    fn tb11_probe_status_description_does_not_mislabel_error_as_return() {
        let router = TerminalCommanderMcpServer::tool_router();
        let probe = router
            .list_all()
            .into_iter()
            .find(|t| t.name == "probe_status")
            .expect("probe_status must be a live tool");
        let desc = probe
            .description
            .as_deref()
            .expect("probe_status must carry a description");
        assert!(
            desc.contains("UnknownProbe error") && desc.contains("not a success body"),
            "probe_status must describe UnknownProbe as an error path, not a returned body; got: {desc}"
        );
    }
}
