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
//! [`tool_catalogue`] is the single source of truth for the 50 live
//! tools, spanning discovery (`system_discover`), status (`health`,
//! `policy_status`, `self_check`), command/bucket/event, registry
//! (including `registry_suggest_from_samples`),
//! file, PTY, persistent shell sessions + workspace snapshots
//! (`shell_session_*`, `workspace_snapshot_*`), aggregate runtime views,
//! predicate-routed subscriptions
//! (`subscription_open/pull/list/close/seek`), and remote federation
//! (`target_list`, `target_probe`). Each daemon-backed tool maps 1:1 to a
//! daemon IPC method. `system_discover` and `target_list` are
//! local-daemon-independent (they answer from adapter-side state); every
//! other tool returns the structured `daemon_unavailable` envelope when
//! the daemon is unreachable.
//!
//! P5 remote federation: every daemon-backed command tool accepts an
//! optional `target_id` (default = local). When set, the request routes
//! to that target's daemon over an operator-forwarded LOCAL socket
//! (constitution IV: no public TCP; constitution I: the adapter never
//! spawns -- the `ssh -L` tunnel is the OPERATOR's, not ours).
//!
//! Source-status: live; all 50 tools forward through daemon IPC.

use std::borrow::Cow;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolRequestParams, CallToolResult, Content, Implementation, ListToolsResult,
        LoggingLevel, LoggingMessageNotificationParam, PaginatedRequestParams, ProtocolVersion,
        ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use terminal_commander_core::{BucketConfig, RuleDefinition, RuleType, Severity};
use terminal_commanderd::ipc::protocol::{
    BucketEventsSinceParams, BucketEventsSinceResponse, BucketSummaryParams, BucketSummaryResponse,
    BucketWaitParams, BucketWaitResponse, CommandOutputTailParams, CommandOutputTailResponse,
    CommandStartParams, CommandStartResponse, CommandStatusParams, CommandStatusResponse,
    CommandStopParams, CommandStopResponse, ContextUnavailableReason, DiscoverResponse,
    EventContextParams, EventContextResponse, FileListDirParams, FileListDirResponse,
    FileReadWindowParams, FileReadWindowResponse, FileSearchParams, FileSearchResponse,
    FileWatchListResponse, FileWatchStartParams, FileWatchStartResponse, FileWatchStopParams,
    FileWatchStopResponse, FileWriteParams, FileWriteResponse, IpcContextFrame, IpcError,
    IpcErrorCode, IpcRequest, IpcResponse, ListLimitParams, PolicyCapsView, PolicyStatusResponse,
    ProbeListResponse, ProbeStatusParams, ProbeStatusResponse, PtyCommandListResponse,
    PtyCommandStartParams, PtyCommandStartResponse, PtyCommandStopParams, PtyCommandStopResponse,
    PtyCommandWriteStdinParams, RegistryActivateParams, RegistryActivateResponse,
    RegistryDeactivateBulkParams, RegistryDeactivateBulkResponse, RegistryDeactivateParams,
    RegistryDeactivateResponse, RegistryGetParams, RegistryGetResponse, RegistryImportPackParams,
    RegistryImportPackResponse, RegistryListActiveResponse, RegistrySearchParams,
    RegistrySearchResponse, RegistrySuggestFromSamplesParams, RegistrySuggestFromSamplesResponse,
    RegistryTestParams, RegistryTestResponse, RegistryTestSample, RegistryUpsertParams,
    RegistryUpsertResponse, SelfCheckResponse, ShellExecParams, ShellSessionExecParams,
    ShellSessionExecResponse, ShellSessionListResponse, ShellSessionStartParams,
    ShellSessionStartResponse, ShellSessionStatusParams, ShellSessionStatusResponse,
    ShellSessionStopParams, ShellSessionStopResponse, SubscriptionCloseParams,
    SubscriptionCloseResponse, SubscriptionListParams, SubscriptionListResponse,
    SubscriptionOpenParams, SubscriptionOpenResponse, SubscriptionPredicate,
    SubscriptionPullParams, SubscriptionPullResponse, SubscriptionSeekParams,
    SubscriptionSeekResponse, SubscriptionSourceSel, WorkspaceSnapshotApplyParams,
    WorkspaceSnapshotApplyResponse, WorkspaceSnapshotCreateParams, WorkspaceSnapshotCreateResponse,
};

use crate::daemon_client::McpDaemonClient;
use crate::target_router::{TargetRouteError, TargetRouter};
use terminal_commander_supervisor::ensure::EnsureDaemonStatus;
use terminal_commanderd::{RemoteTarget, TargetsConfig};

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
            description: "Return adapter/daemon metadata, tool catalogue, and bounded host-environment probes with ranked access routes and an exact beachhead argv template.",
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
            name: "command_stop",
            status: ToolStatus::Live,
            description: "Force-kill a running combed (non-PTY) command by job_id; returns final bounded counters. Never returns raw output.",
        },
        ToolCatalogueEntry {
            name: "shell_exec",
            status: ToolStatus::Live,
            description: "Run ONE shell line (pipelines/compounds/redirects) through the comb pipeline; requires allow_shell; combed, never raw.",
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
            description: "Deactivate (rule_id, version?, scope); omit version = latest stored. Future commands skip the rule.",
        },
        ToolCatalogueEntry {
            name: "registry_list_active",
            status: ToolStatus::Live,
            description: "Snapshot of every currently-active rule (id + version + severity).",
        },
        ToolCatalogueEntry {
            name: "registry_suggest_from_samples",
            status: ToolStatus::Live,
            description: "Suggest candidate parsing rules from raw output samples via pure heuristics. Returns DRAFT proposals + confidence + the explicit test->upsert->activate next steps. NEVER activates or persists a rule.",
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
            name: "file_write",
            status: ToolStatus::Live,
            description: "Write UTF-8 content to one file. Policy-gated by paths.write_allow; audited before write; bounded size; atomic. Mutating + non-idempotent.",
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
            name: "shell_session_start",
            status: ToolStatus::Live,
            description: "Start a persistent shell session (sticky cwd/env across sends). Requires allow_session; denied by default; combed output only. (unix-only; unavailable on Windows)",
        },
        ToolCatalogueEntry {
            name: "shell_session_exec",
            status: ToolStatus::Live,
            description: "Run ONE line in a session shell; returns the combed signals it produced. Sticky cwd/env from the prior lines; never a raw stream. (unix-only; unavailable on Windows)",
        },
        ToolCatalogueEntry {
            name: "shell_session_status",
            status: ToolStatus::Live,
            description: "Session lifecycle state, current cwd, and a bounded env snapshot. (unix-only; unavailable on Windows)",
        },
        ToolCatalogueEntry {
            name: "shell_session_stop",
            status: ToolStatus::Live,
            description: "Stop a session (graceful then forced); reports the terminal state. (unix-only; unavailable on Windows)",
        },
        ToolCatalogueEntry {
            name: "shell_session_list",
            status: ToolStatus::Live,
            description: "Snapshot of every currently-live session (id, state, cwd, last_active). (unix-only; unavailable on Windows)",
        },
        ToolCatalogueEntry {
            name: "workspace_snapshot_create",
            status: ToolStatus::Live,
            description: "Save a session's cwd + bounded env as a restorable workspace snapshot. (unix-only; unavailable on Windows)",
        },
        ToolCatalogueEntry {
            name: "workspace_snapshot_apply",
            status: ToolStatus::Live,
            description: "Restore a workspace snapshot's cwd/env into a session. (unix-only; unavailable on Windows)",
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
        ToolCatalogueEntry {
            name: "target_list",
            status: ToolStatus::Live,
            description: "List registered remote-federation targets + reachability (from targets.toml; default none = local-only). Read-only; reachability probed over the operator-forwarded LOCAL socket, never a public network port.",
        },
        ToolCatalogueEntry {
            name: "target_probe",
            status: ToolStatus::Live,
            description: "Probe ONE target's health over its operator-forwarded LOCAL socket; returns reachable + daemon_version. Requires allow_remote; never opens a public network port.",
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
    // `system_discover` answers from adapter metadata; `target_list` answers
    // from the adapter-side targets.toml registry (P5). Both are reportable
    // even when the local daemon is down. `target_probe` DOES require the
    // local daemon (it confirms allow_remote before dialing off-host).
    !matches!(name, "system_discover" | "target_list")
}

/// Whether a tool name belongs to the PTY command family.
///
/// The PTY runtime is `#[cfg(unix)]`-only; on every other platform the
/// daemon answers every `pty_*` IPC with `UnsupportedPlatform`. TB-1:
/// `system_discover` must surface that platform truth so a naive client
/// never calls a PTY tool that can only fail.
#[must_use]
/// The four interactive PTY command tools. These are available on every host
/// with a PTY backend: unix `pty-process` AND Windows ConPTY (US3a/TC53). On a
/// platform with no PTY backend the daemon answers `UnsupportedPlatform` and
/// discovery reports them `available: false`.
const fn is_pty_command_tool(name: &str) -> bool {
    matches!(
        name.as_bytes(),
        b"pty_command_start"
            | b"pty_command_write_stdin"
            | b"pty_command_stop"
            | b"pty_command_list"
    )
}

/// Persistent shell sessions + workspace snapshots. A session is a long-lived
/// login-shell PTY job, but the SESSION runtime (`shell_session.rs`,
/// `#[cfg(unix)]`, login-shell `[shell, "-i"]`) is still unix-only -- Windows
/// session support is a SEPARATE slice. On a non-unix host the daemon answers
/// each with `UnsupportedPlatform`, and discovery must report them
/// `available: false`.
const fn is_session_tool(name: &str) -> bool {
    matches!(
        name.as_bytes(),
        b"shell_session_start"
            | b"shell_session_exec"
            | b"shell_session_status"
            | b"shell_session_stop"
            | b"shell_session_list"
            | b"workspace_snapshot_create"
            | b"workspace_snapshot_apply"
    )
}

/// Whether the interactive PTY command runtime is available on this host.
///
/// TC44's unix `pty-process` backend AND the US3a/TC53 Windows ConPTY backend
/// (`portable-pty`) both expose the abstract `PtyProbe` surface the daemon
/// drives, so the four `pty_command_*` tools are live on `unix` and `windows`.
/// On any other platform the daemon answers `UnsupportedPlatform` and
/// discovery reports them `available: false`.
#[must_use]
const fn pty_command_available() -> bool {
    cfg!(any(unix, windows))
}

/// Whether the persistent shell-session runtime is available on this host.
///
/// The session runtime (`shell_session.rs`) is still `#[cfg(unix)]` (login
/// shell `[shell, "-i"]`); Windows session support is a separate slice. So the
/// `shell_session_*` / `workspace_snapshot_*` tools are unix-only for now.
#[must_use]
const fn session_runtime_available() -> bool {
    cfg!(unix)
}

/// The PTY backend label for the omni capability matrix (US6/T056).
///
/// Honest platform truth, evaluated at compile time from the same `cfg!`
/// gates that drive [`pty_command_available`]: `"posix"` for the unix
/// `pty-process` backend (Linux/WSL/macOS), `"windows_conpty"` for the
/// US3a/TC53 Windows ConPTY backend (`portable-pty`), and `"unavailable"`
/// on any host with no PTY backend at all (matching `available: false`).
#[must_use]
const fn pty_platform() -> &'static str {
    if cfg!(windows) {
        "windows_conpty"
    } else if cfg!(unix) {
        "posix"
    } else {
        "unavailable"
    }
}

/// Reason surfaced for `pty_command_*` tools on a host with no PTY backend
/// (neither unix `pty-process` nor Windows ConPTY).
const PTY_UNAVAILABLE_REASON: &str = "PTY runtime unavailable on this platform";
/// Reason surfaced for session/snapshot tools on a non-unix host (the session
/// runtime is unix-only for now; Windows sessions are a separate slice).
const SESSION_UNAVAILABLE_REASON: &str =
    "shell-session runtime unavailable on this platform (unix-only)";

/// Reason surfaced for the privileged helper in the omni capability matrix
/// (US6/T056, P4). The privileged helper (`terminal-commander-privileged`) is
/// PLAN-ONLY this program: no code lands until a dedicated threat review
/// completes (research R-9 / tasks.md US6 note). The matrix MUST report it
/// `available: false` with this reason -- never `true` -- so discovery does
/// not claim a capability that is not wired.
const PRIVILEGED_HELPER_UNAVAILABLE_REASON: &str = "threat_review_pending";

/// Reason surfaced for `shell_exec` when the active policy profile has
/// `allow_shell` OFF. The shell lane is WIRED, but a call is PolicyDenied, so
/// discovery reports it unavailable with this cap-truthful reason rather than
/// advertising a call that can only fail (the exact BUG-1 lie this closes).
const SHELL_CAP_DENIED_REASON: &str = "allow_shell capability is off in the active policy profile";
/// Reason surfaced for session/snapshot tools when `allow_session` is OFF on a
/// host whose platform DOES provide the session runtime. Platform absence takes
/// precedence and surfaces [`SESSION_UNAVAILABLE_REASON`] instead.
const SESSION_CAP_DENIED_REASON: &str =
    "allow_session capability is off in the active policy profile";
/// Reason surfaced for `target_probe` when `allow_remote` is OFF. Remote
/// federation is opt-in; a probe is denied until the operator grants the cap.
const REMOTE_CAP_DENIED_REASON: &str =
    "allow_remote capability is off in the active policy profile";

/// Policy-cap gate for a tool, evaluated only when the daemon is reachable and
/// its caps are known. Returns the cap-truthful reason a call WOULD be denied,
/// or `None` when no cap gates this tool (or the gating cap is granted).
///
/// This is the discovery counterpart of the daemon's PolicyDenied gate: a tool
/// whose cap is off is reported `available: false` so a client never calls a
/// lane policy can only reject (`shell_exec` under `allow_shell: false`,
/// sessions under `allow_session: false`, `target_probe` under
/// `allow_remote: false`). Platform gating is handled separately and takes
/// precedence (a cap is moot when the runtime does not exist on this host).
#[must_use]
fn tool_policy_block_reason(name: &str, caps: PolicyCapsView) -> Option<&'static str> {
    if name == "shell_exec" && !caps.allow_shell {
        Some(SHELL_CAP_DENIED_REASON)
    } else if is_session_tool(name) && !caps.allow_session {
        Some(SESSION_CAP_DENIED_REASON)
    } else if name == "target_probe" && !caps.allow_remote {
        Some(REMOTE_CAP_DENIED_REASON)
    } else {
        None
    }
}

#[must_use]
fn discovered_tools(
    daemon_available: bool,
    caps: Option<PolicyCapsView>,
) -> Vec<DiscoveredToolEntry> {
    let pty_available = pty_command_available();
    let session_available = session_runtime_available();
    tool_catalogue()
        .iter()
        .map(|tool| {
            let requires_daemon = tool_requires_daemon(tool.name);
            let implemented = matches!(tool.status, ToolStatus::Live);
            // Platform truth, evaluated even when the daemon is reachable, so a
            // tool never advertises `available: true` on a host that can only
            // answer it with `UnsupportedPlatform`. PTY command tools are live
            // on unix + Windows (ConPTY); session/snapshot tools are unix-only.
            let pty_blocked = is_pty_command_tool(tool.name) && !pty_available;
            let session_blocked = is_session_tool(tool.name) && !session_available;
            let platform_blocked = pty_blocked || session_blocked;
            // Policy-cap truth (BUG 1): only meaningful once the tool would
            // otherwise be callable (implemented, platform-supported, daemon
            // reachable). A cap-off tool is reported unavailable so discovery
            // never claims a call that policy can only PolicyDeny. Caps are
            // `None` when the daemon is down (already `daemon_unavailable`) or a
            // caps probe failed -- both fall back to presence-only gating.
            let would_be_callable =
                implemented && !platform_blocked && (!requires_daemon || daemon_available);
            let policy_reason = if would_be_callable {
                caps.and_then(|c| tool_policy_block_reason(tool.name, c))
            } else {
                None
            };
            let available = would_be_callable && policy_reason.is_none();
            let unavailable_reason = if !implemented {
                Some("not_implemented")
            } else if pty_blocked {
                Some(PTY_UNAVAILABLE_REASON)
            } else if session_blocked {
                Some(SESSION_UNAVAILABLE_REASON)
            } else if requires_daemon && !daemon_available {
                Some("daemon_unavailable")
            } else if policy_reason.is_some() {
                policy_reason
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
    /// US6/T056: read-only omni capability matrix. Assembled honestly from
    /// live runtime + config state -- platform `cfg!` truth, daemon
    /// reachability, and the loaded `targets.toml`. NEVER claims a capability
    /// `available: true` that is not actually wired (the privileged helper is
    /// always `false`; PTY/session reflect this host's real backends).
    pub omni_status: OmniStatus,
}

/// US6/T056 (FR-023): the omni capability matrix surfaced via discovery.
///
/// "Omni" is the program promise that an LLM never needs a separate terminal
/// tool. This matrix is the HONEST, machine-readable proof of what that holds
/// for on THIS host right now: each capability is reported from live runtime
/// state (compile-time platform `cfg!`, daemon reachability, loaded targets),
/// and any unavailable capability carries the reason it is off. It is a
/// READ-ONLY discovery addition -- no new tool, no behavior change.
#[derive(Debug, Clone, Serialize)]
pub struct OmniStatus {
    /// The omni program version (the adapter/workspace package version). Not a
    /// claim that every gate is green -- just which build emitted the matrix.
    pub program_version: &'static str,
    pub matrix: OmniMatrix,
}

/// The capability matrix rows. Shape pinned by `contracts/mcp-tools.md` (P6).
#[derive(Debug, Clone, Serialize)]
pub struct OmniMatrix {
    pub shell_exec: ShellExecStatus,
    pub sessions: SessionsStatus,
    pub pty: PtyStatus,
    pub privileged_helper: PrivilegedHelperStatus,
    pub remote_targets: RemoteTargetsStatus,
}

/// Shell-lane capability (US1/TC49).
///
/// `available` is the CAP-TRUTHFUL verdict: the lane is WIRED (the `shell_exec`
/// tool is live and the daemon is reachable) AND the active profile grants
/// `allow_shell`. A deny-by-default profile (allow_shell off) reports
/// `available: false` with `reason` set, because a call would be PolicyDenied
/// (BUG 1). `reason` is `None` when available, or when caps could not be read
/// (the presence-only fallback). The precise cap value is still reported by
/// `policy_status`.
#[derive(Debug, Clone, Serialize)]
pub struct ShellExecStatus {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// Persistent shell-session runtime (US2/TC50).
///
/// Unix-only for now: the session runtime is `#[cfg(unix)]`, so `available`
/// mirrors [`session_runtime_available`] AND daemon reachability.
#[derive(Debug, Clone, Serialize)]
pub struct SessionsStatus {
    pub available: bool,
}

/// PTY backend (US3/TC53).
///
/// `available` reflects the real dual backend: unix `pty-process` OR Windows
/// ConPTY; `platform` is `"posix"`, `"windows_conpty"`, or `"unavailable"`.
/// Mirrors [`pty_command_available`] AND daemon reachability.
#[derive(Debug, Clone, Serialize)]
pub struct PtyStatus {
    pub available: bool,
    pub platform: &'static str,
}

/// Privileged helper (US4/P4).
///
/// ALWAYS `available: false` this program: P4 is plan-only and no code lands
/// until a dedicated threat review completes. The reason is the stable
/// `"threat_review_pending"` label.
#[derive(Debug, Clone, Serialize)]
pub struct PrivilegedHelperStatus {
    pub available: bool,
    pub reason: &'static str,
}

/// Remote federation targets (US5/P5).
///
/// `count` is the number of targets loaded from `targets.toml`; `reachable` is
/// how many answered a short bounded probe over their operator-forwarded LOCAL
/// socket. Local-only hosts report `{count: 0, reachable: 0}`.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteTargetsStatus {
    pub count: usize,
    pub reachable: usize,
}

/// Long-poll client timeout for `subscription_pull`. STRICTLY ABOVE the
/// server-side pull cap (`MAX_PULL_TIMEOUT_MS` = 8 s): an idle ~8 s pull
/// must return SUCCESS empty+liveness, never a -32603 client timeout
/// (AC13 / MUST-ADD #7). Timeout hierarchy: pull server cap (8 s) <
/// DRAIN_CEILING (10 s) < this MCP pull client timeout (12 s).
const SUBSCRIPTION_PULL_CLIENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

/// Bound on a `target_probe` / `target_list` reachability dial (P5). Short
/// on purpose: probing a target whose tunnel is down must fail fast and
/// report `reachable: false`, never hang a tool. A live forwarded UDS
/// round trip is sub-millisecond; this only caps the failure case.
const TARGET_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

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
    /// P5 remote-target router. Resolves an optional `target_id` to the
    /// local daemon client (default) or a target's operator-forwarded
    /// LOCAL socket. Local-only (no targets) when the server is built via
    /// `new` / `with_notify`; populated from `targets.toml` via
    /// `with_targets`. Never spawns, never opens TCP, never reads fs.
    target_router: TargetRouter,
    /// Tool router populated by the rmcp `#[tool_router]` macro. The
    /// router is read by the rmcp service layer, not by us directly,
    /// so the dead-code lint trips here; suppressed below.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    /// Optional surface override for tests. When `Some`, bypasses the live
    /// `TC_SURFACE` env read so tests can inject a known surface without
    /// mutating process-global env (`set_var` is `unsafe` in edition 2024
    /// and `unsafe_code` is `forbid` workspace-wide). Production code
    /// always leaves this `None` and reads the env live.
    surface_override: Option<crate::surface::Surface>,
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
        // Default surface is LOCAL-ONLY: no remote targets registered, so
        // every tool routes to the local daemon exactly as before P5. Remote
        // federation is opt-in via `with_targets`.
        let router = TargetRouter::local_only(daemon.clone());
        Self::with_targets_and_notify(daemon, router, notify)
    }

    /// Construct a server wired to a target router loaded from `targets.toml`
    /// (P5). When the router has no targets this is identical to `new`. The
    /// router's local client MUST be the same `daemon` passed here so the
    /// default (no-`target_id`) path and the explicit-local path agree.
    #[must_use]
    pub fn with_targets(daemon: McpDaemonClient, targets: TargetsConfig) -> Self {
        let router = TargetRouter::new(daemon.clone(), targets);
        Self::with_targets_and_notify(daemon, router, notify_enabled())
    }

    /// Shared constructor: wire the local client, the long-poll client, the
    /// notification opt-in, and the P5 target router.
    #[must_use]
    fn with_targets_and_notify(
        daemon: McpDaemonClient,
        target_router: TargetRouter,
        notify: bool,
    ) -> Self {
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
            target_router,
            tool_router: Self::tool_router(),
            surface_override: None,
        }
    }

    /// Override the active surface for this server instance, bypassing the
    /// live `TC_SURFACE` env read. Intended for integration tests that cannot
    /// mutate process-global env (`set_var` is `unsafe` in edition 2024 and
    /// `unsafe_code` is `forbid` workspace-wide).
    ///
    /// Production binaries MUST NOT call this; they rely on `surface_from_env`
    /// so an operator can flip the surface at runtime without rebuild.
    ///
    /// # Note
    ///
    /// This is `#[doc(hidden)]` rather than `#[cfg(test)]` because integration
    /// tests live in a separate binary and cannot see `#[cfg(test)]` items from
    /// the library. The method is only intended for test harnesses.
    #[doc(hidden)]
    #[must_use]
    pub const fn with_surface(mut self, surface: crate::surface::Surface) -> Self {
        self.surface_override = Some(surface);
        self
    }

    /// Single daemon-availability + version-skew guard shared by every
    /// daemon-backed tool. Returns the structured `daemon_unavailable`
    /// envelope when the daemon is unreachable, or the `daemon_version_skew`
    /// error (DEFECT B) when the daemon is ALIVE but the wrong build;
    /// otherwise `Ok(())`. Call `self.ensure_daemon_available().await?` at the
    /// top of each handler before issuing IPC.
    ///
    /// Reachability + self-heal live in [`Self::ensure_daemon_reachable`]; this
    /// method layers the skew gate on top. The diagnostic pair (`health`,
    /// `system_discover`) deliberately call `ensure_daemon_reachable` instead,
    /// so an operator can still inspect a version-skewed daemon.
    async fn ensure_daemon_available(&self) -> Result<(), McpError> {
        self.ensure_daemon_reachable().await?;
        // DEFECT B: an ALIVE but version-skewed daemon (e.g. a 0.1.47 WSL
        // runtime under a 0.1.69 adapter) must fail with an HONEST
        // `daemon_version_skew` error, not a misleading `daemon_unavailable`.
        // `health` / `system_discover` bypass this via `ensure_daemon_reachable`
        // so an operator can still diagnose while skewed.
        if let Some((daemon_ver, adapter_ver)) = self.daemon.status().and_then(|s| s.version_skew())
        {
            return Err(daemon_version_skew_error(&daemon_ver, &adapter_ver));
        }
        Ok(())
    }

    /// Daemon-REACHABILITY gate WITHOUT the version-skew check. The diagnostic
    /// pair (`health`, `system_discover`) use this so an operator can still
    /// inspect a version-skewed-but-alive daemon (DEFECT B exemption); every
    /// other daemon-backed tool routes through [`Self::ensure_daemon_available`],
    /// which layers the skew gate on top.
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
    async fn ensure_daemon_reachable(&self) -> Result<(), McpError> {
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

    /// Resolve the daemon client a tool should dial for an optional
    /// `target_id` (P5 / T050).
    ///
    /// - `None` => the LOCAL client (default; identical to pre-P5 behaviour).
    ///   The caller is expected to have already run
    ///   [`Self::ensure_daemon_available`].
    /// - `Some(id)` => REMOTE routing. Gated by the operator's `allow_remote`
    ///   cap on the LOCAL daemon (constitution II, default-deny): the adapter
    ///   reads the live `policy_status` and refuses with a typed
    ///   `remote_denied` error unless `allow_remote` is set. On grant, the
    ///   request routes to the target's operator-forwarded LOCAL socket
    ///   ([`TargetRouter::resolve`]); the REMOTE daemon then evaluates and
    ///   AUDITS the actual action under its own policy, so combing + audit +
    ///   bounded output are identical on both ends. An unknown id is a typed
    ///   `unknown_target` error.
    ///
    /// Constitution IV: the returned client only ever dials a local socket
    /// PATH; no public TCP listener or network address is constructed.
    async fn daemon_for_target(
        &self,
        target_id: Option<&str>,
    ) -> Result<McpDaemonClient, McpError> {
        let Some(id) = target_id else {
            return Ok(self.daemon.clone());
        };

        // Remote use is opt-in: confirm the operator enabled `allow_remote`
        // on the local daemon BEFORE routing anything off-host.
        self.ensure_remote_allowed(id).await?;

        self.target_router
            .resolve(Some(id))
            .map_err(|e| route_error_to_mcp(&e))
    }

    /// Enforce the `allow_remote` cap before any remote routing (T051).
    ///
    /// Reads the LIVE policy from the local daemon. A daemon that is itself
    /// unreachable surfaces the standard `daemon_unavailable` envelope (we
    /// cannot prove the operator opted in). When `allow_remote` is false the
    /// remote request is refused with a typed, teaching `remote_denied`
    /// error -- the default-deny posture for federation.
    async fn ensure_remote_allowed(&self, target_id: &str) -> Result<(), McpError> {
        self.ensure_daemon_available().await?;
        match self.daemon.call(IpcRequest::PolicyStatus).await {
            Ok(IpcResponse::PolicyStatus(status)) => {
                if status.caps.allow_remote {
                    Ok(())
                } else {
                    Err(remote_denied_error(target_id))
                }
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// Probe a single target's forwarded LOCAL socket (P5 / T051).
    ///
    /// Dials the operator-forwarded socket with a SHORT bounded timeout and
    /// issues one `SystemDiscover`. Returns `Some(daemon_version)` when the
    /// remote daemon answers, `None` when the socket is unreachable or the
    /// timeout elapses (a down/unforwarded target is NOT an error here -- the
    /// caller reports `reachable: false`). Never spawns, never opens TCP.
    async fn probe_target(&self, target: &RemoteTarget) -> Option<String> {
        let client = self
            .target_router
            .client_for(target)
            .with_timeout(TARGET_PROBE_TIMEOUT);
        match client.call(IpcRequest::SystemDiscover).await {
            Ok(IpcResponse::SystemDiscover(d)) => Some(d.version),
            _ => None,
        }
    }

    /// `system_discover` — adapter metadata, tool catalogue, and verified host routes.
    /// Forwards to the daemon to fetch live profile/version data; if
    /// the daemon is unreachable the response still carries the
    /// adapter-side catalogue with the daemon error surfaced.
    #[tool(
        description = "Discover adapter/daemon metadata and the execution environment. Returns bounded OS, terminal, shell/PowerShell, WSL, and tool probes plus ranked access_routes and an exact beachhead argv template; call this before choosing an interpreter."
    )]
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
        // BUG 1: policy caps drive the cap-truthful availability of policy-gated
        // tools (shell_exec/allow_shell, sessions/allow_session,
        // target_probe/allow_remote). `DiscoverResponse` does NOT carry caps, so
        // read them with one bounded `PolicyStatus` round trip -- but ONLY when
        // the daemon is reachable (a down daemon already makes every
        // daemon-gated tool `daemon_unavailable`). A caps-probe failure leaves
        // caps `None`: discovery falls back to presence-only gating rather than
        // guessing a deny.
        let caps = if daemon_available {
            match self.daemon.call(IpcRequest::PolicyStatus).await {
                Ok(IpcResponse::PolicyStatus(p)) => Some(p.caps),
                _ => None,
            }
        } else {
            None
        };
        // US6/T056: read-only omni capability matrix, assembled before we move
        // `daemon`/`daemon_error` into the payload. Honest from live state.
        let omni_status = self.omni_status(daemon_available, caps).await;
        let payload = SystemDiscoverPayload {
            adapter_version: ADAPTER_VERSION,
            mcp_spec: MCP_SPEC_REVISION,
            daemon_available,
            daemon,
            daemon_error,
            tools: discovered_tools(daemon_available, caps),
            omni_status,
        };
        json_tool_result(&payload)
    }

    /// Assemble the US6/T056 omni capability matrix HONESTLY from live state.
    ///
    /// READ-ONLY: this never starts a job, opens a file, or mutates anything.
    /// It reads compile-time platform truth (the same `cfg!` gates that drive
    /// tool availability), the passed daemon reachability, and the loaded
    /// `targets.toml` registry. Remote reachability is measured with the same
    /// short bounded probe `target_list` uses (a down target is not an error).
    ///
    /// Invariant: no row may claim `available: true` for a capability that is
    /// not actually wired. The privileged helper is always `false`
    /// (`threat_review_pending`); PTY/session reflect this host's real backend.
    async fn omni_status(
        &self,
        daemon_available: bool,
        caps: Option<PolicyCapsView>,
    ) -> OmniStatus {
        // Shell lane (US1): CAP-TRUTHFUL (BUG 1). The lane is wired iff the
        // `shell_exec` tool is live and the daemon is reachable, but a profile
        // with `allow_shell` off can only PolicyDeny the call -- so when caps
        // are known, an off cap reports `available: false` with the cap reason.
        // (Previously this reported capability PRESENCE regardless of the cap,
        // which let discovery claim available:true for a call policy denies.)
        // Caps `None` (daemon down / probe failed) falls back to presence.
        let shell_exec_live = tool_catalogue()
            .iter()
            .any(|t| t.name == "shell_exec" && matches!(t.status, ToolStatus::Live));
        let shell_exec_wired = shell_exec_live && daemon_available;
        let shell_denied = caps.is_some_and(|c| !c.allow_shell);
        let shell_exec_available = shell_exec_wired && !shell_denied;
        let shell_exec_reason = if shell_exec_wired && shell_denied {
            Some(SHELL_CAP_DENIED_REASON)
        } else {
            None
        };

        // Sessions (US2): unix-only runtime AND a reachable daemon.
        let sessions_available = session_runtime_available() && daemon_available;

        // PTY (US3): dual backend (unix pty-process / Windows ConPTY) AND a
        // reachable daemon. `platform` is honest even when the daemon is down.
        let pty_available = pty_command_available() && daemon_available;

        // Remote targets (US5): count from the loaded config; reachable from a
        // bounded probe of each operator-forwarded LOCAL socket.
        let targets = self.target_router.targets();
        let count = targets.len();
        let mut reachable = 0_usize;
        for t in targets {
            if self.probe_target(t).await.is_some() {
                reachable += 1;
            }
        }

        OmniStatus {
            program_version: ADAPTER_VERSION,
            matrix: OmniMatrix {
                shell_exec: ShellExecStatus {
                    available: shell_exec_available,
                    reason: shell_exec_reason,
                },
                sessions: SessionsStatus {
                    available: sessions_available,
                },
                pty: PtyStatus {
                    available: pty_available,
                    platform: pty_platform(),
                },
                // P4 is plan-only: NEVER available this program.
                privileged_helper: PrivilegedHelperStatus {
                    available: false,
                    reason: PRIVILEGED_HELPER_UNAVAILABLE_REASON,
                },
                remote_targets: RemoteTargetsStatus { count, reachable },
            },
        }
    }

    /// `health` — daemon liveness check. Returns uptime when reachable
    /// and a typed error otherwise.
    #[tool(description = "Daemon liveness ping. Returns uptime in seconds when reachable.")]
    async fn health(&self) -> Result<CallToolResult, McpError> {
        // DEFECT B exemption: health is a diagnostic — reachability only, no
        // skew gate, so an operator can still ping a version-skewed daemon.
        self.ensure_daemon_reachable().await?;
        match self.daemon.call(IpcRequest::Health).await {
            Ok(IpcResponse::Health {
                uptime_secs,
                version,
                ..
            }) => json_tool_result(&serde_json::json!({
                "ok": true,
                "uptime_secs": uptime_secs,
                "version": version,
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
                caps,
            })) => json_tool_result(&serde_json::json!({
                "profile": profile,
                "commands_deny_count": commands_deny_count,
                "default_deny_path_suffix_count": default_deny_path_suffix_count,
                "file_window_bytes": file_window_bytes,
                "bucket_read_limit": bucket_read_limit,
                // Resolved per-call caps (POLICY.md section 4.1). Surfacing the
                // active set keeps `full_access` and explicit `[policy.caps]`
                // grants visible -- there is no opaque "full_access magic".
                "caps": {
                    "allow_shell": caps.allow_shell,
                    "allow_session": caps.allow_session,
                    "allow_privileged": caps.allow_privileged,
                    "allow_remote": caps.allow_remote,
                },
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

    /// `target_list` — registered remote-federation targets + reachability
    /// (P5 / T051). READ-ONLY: lists the targets parsed from `targets.toml`
    /// (default empty = local-only) and probes each one's
    /// operator-forwarded LOCAL socket for liveness. Listing is not gated by
    /// `allow_remote` (it reveals no remote data, only configuration);
    /// ACTUALLY routing a tool to a target is gated separately.
    #[tool(
        description = "List registered remote-federation targets and whether each is reachable. Targets come from targets.toml (default: none = local-only). Reachability is probed over the operator-established LOCAL forward socket; no public network port is opened. Use target_probe for a single target's daemon_version. Routing a tool to a target requires the operator's allow_remote cap."
    )]
    async fn target_list(&self) -> Result<CallToolResult, McpError> {
        let mut targets = Vec::new();
        for t in self.target_router.targets() {
            let reachable = self.probe_target(t).await.is_some();
            targets.push(serde_json::json!({
                "target_id": t.target_id,
                "host": t.host,
                "transport": t.transport,
                "local_forward_socket": t.local_forward_socket.to_string_lossy(),
                "reachable": reachable,
            }));
        }
        json_tool_result(&serde_json::json!({ "targets": targets }))
    }

    /// `target_probe` — dial ONE target's forwarded LOCAL socket and report
    /// reachability + the remote daemon version (P5 / T051).
    ///
    /// Gated by `allow_remote` (probing a target reaches off-host through the
    /// tunnel). Returns `{ target_id, reachable, daemon_version? }`. A down or
    /// unforwarded target reports `reachable: false` rather than erroring, so
    /// an agent can poll readiness. An unknown `target_id` is a typed error.
    #[tool(
        description = "Probe ONE registered target's health: dials its operator-forwarded LOCAL socket and returns { reachable, daemon_version }. A target whose tunnel is down reports reachable=false (not an error). Requires the operator's allow_remote cap; never opens a public network port."
    )]
    async fn target_probe(
        &self,
        Parameters(params): Parameters<McpTargetProbeParams>,
    ) -> Result<CallToolResult, McpError> {
        // Gate remote reach on allow_remote (also validates the local daemon is
        // up, since the cap lives there) before dialing off-host.
        self.ensure_remote_allowed(&params.target_id).await?;
        let target = self
            .target_router
            .target(&params.target_id)
            .ok_or_else(|| {
                route_error_to_mcp(&TargetRouteError::UnknownTarget(params.target_id.clone()))
            })?;
        let daemon_version = self.probe_target(target).await;
        json_tool_result(&serde_json::json!({
            "target_id": target.target_id,
            "reachable": daemon_version.is_some(),
            "daemon_version": daemon_version,
        }))
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
        // P5: route to the optional target_id (None => local). Resolve BEFORE
        // consuming params; remote use is gated on allow_remote + dialed via
        // the target's operator-forwarded LOCAL socket.
        let daemon = self.daemon_for_target(params.target_id.as_deref()).await?;
        let ipc = params.into_ipc()?;
        match daemon.call(IpcRequest::CommandStartCombed(ipc)).await {
            Ok(IpcResponse::CommandStartCombed(CommandStartResponse {
                job_id,
                bucket_id,
                probe_id,
                cursor,
                hint,
            })) => {
                // US2 (FR-011): forward the optional pack-available hint
                // verbatim. Omitted from the JSON when None.
                let mut body = serde_json::json!({
                    "job_id": job_id,
                    "bucket_id": bucket_id,
                    "probe_id": probe_id,
                    "cursor": cursor,
                });
                if let Some(h) = hint {
                    body["hint"] = serde_json::to_value(h).unwrap_or(serde_json::Value::Null);
                }
                json_tool_result(&body)
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `run_and_watch` — one-shot: start a command, wait for its rule
    /// signals + exit, return both. Composes command_start_combed ->
    /// bucket_wait (bounded) -> command_status so the agent needs ONE
    /// call instead of four.
    #[tool(
        description = "Run a command and get its matching signals AND exit code in ONE call. Composes start + bounded wait + status so you don't poll. Pass inline `rules` (minimal: [{\"pattern\": \"ERROR\"}]) to comb the output; returns {signals, exit_code, state, receipt, complete, wait_exhausted, cursor, degraded, recover_hint}. A quiet command (no rule matches) returns a bounded receipt instead of an error — TC never bounces you to the shell for running a small command. Bounded: waits up to wait_ms (default 5000, max 60000) as a WALL-CLOCK budget (honored within one ~1s slice plus a round-trip) and returns up to max_signals (default 50). If `complete` is false (wait_exhausted), the command is STILL RUNNING; continue signals with bucket_wait using the returned bucket_id/cursor/timeout_ms, and poll command_status with job_id for final state/exit_code. command_status does not return signals. If `degraded` is true, an IPC error interrupted the wait but the job is still tracked: confirm daemon health, then follow recover_hint — once a job_id exists this call returns a degraded, job-identified result, never a bare error. Argv only; shell interpreters denied. Prefer plain shell for tiny one-off commands whose full verbatim output you want."
    )]
    async fn run_and_watch(
        &self,
        Parameters(params): Parameters<McpRunAndWatchParams>,
    ) -> Result<CallToolResult, McpError> {
        use terminal_commander_core::JobState;

        self.ensure_daemon_available().await?;
        let (start_params, controls) = params.into_parts();
        let RunAndWatchControls {
            wait_ms,
            max_signals,
            compact,
            wait_until_exit,
        } = controls;
        // P5: resolve the daemon client for the optional target_id. None =>
        // local (default). A remote target is gated by allow_remote + dialed
        // through its operator-forwarded LOCAL socket; the whole one-shot
        // (start + wait + status) runs against that ONE resolved client so
        // signals are combed by the SAME daemon that ran the command.
        let target_id = start_params.target_id.clone();
        let daemon = self.daemon_for_target(target_id.as_deref()).await?;
        let start_ipc = start_params.into_ipc()?;

        // 1. Start.
        let (job_id, bucket_id, mut cursor) =
            match daemon.call(IpcRequest::CommandStartCombed(start_ipc)).await {
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
        //    signal cap is hit, or the wall-clock wait budget is spent.
        //    Each bucket_wait blocks up to a per-iteration slice bounded by
        //    the time remaining, so a fast command returns fast AND the
        //    advertised wait_ms cap stays honest (TC-6).
        //
        //    TC-E2 (wait_until_exit): the signal cap NO LONGER ends the wait
        //    -- only a terminal state or the deadline does. Signals stop
        //    accumulating once the cap is reached, but the loop keeps waiting
        //    for exit, bounded by the SAME `wait_ms` cap (never exceeded).
        let mut signals: Vec<terminal_commander_core::SignalEvent> = Vec::new();
        // TC-6: a wall-clock deadline keeps total wall time bounded by wait_ms
        // plus at most one in-flight slice and the round-trips.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
        // TC-1b: track the LAST OBSERVED state -- never a silent `Running`
        // default. A degraded result then reports what we actually know (or
        // "unknown" if the daemon failed before the first poll), so the agent
        // cannot mistake an unconfirmed job for a confirmed-running one.
        let mut last_observed_state: Option<JobState> = None;
        let mut exit_code: Option<i32> = None;
        // Deferred init: every normal loop exit assigns `receipt` first, and the
        // degraded arms pass `None` (a degraded result carries no receipt), so a
        // `= None` here would be a dead store under -D unused-assignments.
        let mut receipt: Option<serde_json::Value>;

        // do-while: always poll at least once (mirrors the old `.max(1)`), so
        // even wait_ms=0 returns a real observed state.
        loop {
            // Poll status first so a command that already exited short-
            // circuits without burning a full wait slice.
            let status = match daemon
                .call(IpcRequest::CommandStatus(CommandStatusParams { job_id }))
                .await
            {
                Ok(IpcResponse::CommandStatus(s)) => s,
                Ok(other) => return Err(unexpected_variant(&other)),
                // TC-1b: the job_id is already known; a transport failure here
                // must NOT discard it. Return a degraded, job-identified result
                // so the agent can recover the live job instead of a bare error.
                // The underlying error is surfaced in the hint: swallowing it
                // made the degradation cause undiagnosable (dogfood 2026-07-02).
                Err(e) => {
                    return run_and_watch_result(
                        job_id,
                        bucket_id,
                        cursor,
                        last_observed_state,
                        exit_code,
                        &signals,
                        None,
                        true,
                        Some(&degraded_wait_hint(&e)),
                        compact,
                        // F6: an interrupted wait did not necessarily cap;
                        // degraded:true already marks the result incomplete.
                        false,
                    );
                }
            };
            last_observed_state = Some(status.state);
            exit_code = status.exit_code;
            receipt = status.receipt.as_ref().map(|r| serde_json::json!(r));

            let terminal = matches!(
                status.state,
                JobState::Exited | JobState::Cancelled | JobState::Failed
            );

            // Per-slice wait is capped by the time left in the budget, so the
            // advertised wait_ms is honored to within one slice + a round-trip.
            let remaining_ms = u64::try_from(
                deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis(),
            )
            .unwrap_or(u64::MAX);
            let slice_ms = if terminal {
                0
            } else {
                MAX_WAIT_SLICE_MS.min(remaining_ms)
            };

            // Drain any signals available since the last cursor. Only rule-
            // driven events count as "signals"; probe-lifecycle markers (e.g.
            // the `command_exited` meta event, which has no `rule`) are the exit
            // indicator surfaced via exit_code/state and the receipt, not a
            // matched signal -- see collect_rule_signals.
            let wait = BucketWaitParams {
                bucket_id,
                cursor,
                severity_min: None,
                kind_filter: None,
                limit: Some(max_signals.saturating_sub(signals.len()).max(1)),
                timeout_ms: Some(slice_ms),
            };
            match daemon.call(IpcRequest::BucketWait(wait)).await {
                Ok(IpcResponse::BucketWait(r)) => {
                    cursor = r.next_cursor;
                    collect_rule_signals(r.events, &mut signals, max_signals);
                }
                Ok(other) => return Err(unexpected_variant(&other)),
                // TC-1b: same as the status arm -- preserve the job handle.
                Err(e) => {
                    return run_and_watch_result(
                        job_id,
                        bucket_id,
                        cursor,
                        last_observed_state,
                        exit_code,
                        &signals,
                        None,
                        true,
                        Some(&degraded_wait_hint(&e)),
                        compact,
                        // F6: an interrupted wait did not necessarily cap;
                        // degraded:true already marks the result incomplete.
                        false,
                    );
                }
            }

            // TC-E2: in wait_until_exit mode the signal cap does NOT end the
            // wait -- only a terminal state does (still bounded by the
            // deadline below). In the default mode, a full signal buffer ends
            // the wait as before.
            let cap_reached = !wait_until_exit && signals.len() >= max_signals;
            if terminal || cap_reached {
                break;
            }
            // TC-6: stop once the wall-clock budget is spent. Before building
            // the wait-exhausted result, do ONE final non-blocking drain so
            // events that landed since the last cursor are not lost. On
            // wait_exhausted the cursor stays authoritative for resumption and
            // the signals list is best-effort.
            if std::time::Instant::now() >= deadline {
                let drain = BucketWaitParams {
                    bucket_id,
                    cursor,
                    severity_min: None,
                    kind_filter: None,
                    limit: Some(max_signals.saturating_sub(signals.len()).max(1)),
                    timeout_ms: Some(0),
                };
                if let Ok(IpcResponse::BucketWait(r)) =
                    daemon.call(IpcRequest::BucketWait(drain)).await
                {
                    cursor = r.next_cursor;
                    collect_rule_signals(r.events, &mut signals, max_signals);
                }
                break;
            }
        }

        // 3. Compose the (non-degraded) response through the shared builder so
        //    the normal and degraded payloads stay a strict superset of one
        //    another. The receipt rides only the zero-signal success path
        //    (no-silence rule): a quiet command yields a receipt, never an
        //    error. `complete`/`wait_exhausted` disambiguate the bounded wait
        //    (see run_and_watch_result / run_and_watch_completion).
        run_and_watch_result(
            job_id,
            bucket_id,
            cursor,
            last_observed_state,
            exit_code,
            &signals,
            receipt,
            false,
            None,
            compact,
            // F6 (truncation honesty): the returned `signals` array was limited
            // by `max_signals` iff it reached the cap -- true in default mode
            // when `cap_reached` ended the wait, and also in wait_until_exit mode
            // where collect_rule_signals stops appending at `max_signals`. Either
            // way more matches may exist beyond `cursor`.
            signals.len() >= max_signals,
        )
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
        let daemon = self.daemon_for_target(params.target_id.as_deref()).await?;
        let job_id = parse_id::<terminal_commander_core::ids::JobIdKind>("job_id", &params.job_id)
            .map_err(invalid_params)?;
        let ipc = CommandStatusParams { job_id };
        match daemon.call(IpcRequest::CommandStatus(ipc)).await {
            Ok(IpcResponse::CommandStatus(s)) => json_tool_result(&command_status_payload(&s)),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    #[tool(
        name = "command_stop",
        description = "Force-kill a running combed (non-PTY) command by job_id; returns final bounded counters. Never returns raw output."
    )]
    async fn command_stop(
        &self,
        Parameters(params): Parameters<McpCommandStopParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let daemon = self.daemon_for_target(params.target_id.as_deref()).await?;
        let job_id = parse_id::<terminal_commander_core::ids::JobIdKind>("job_id", &params.job_id)
            .map_err(invalid_params)?;
        match daemon
            .call(IpcRequest::CommandStop(CommandStopParams { job_id }))
            .await
        {
            Ok(IpcResponse::CommandStop(CommandStopResponse {
                job_id,
                bucket_id,
                frames_total,
                events_emitted,
                bytes_total,
            })) => json_tool_result(&serde_json::json!({
                "job_id": job_id,
                "bucket_id": bucket_id,
                "frames_total": frames_total,
                "events_emitted": events_emitted,
                "bytes_total": bytes_total,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `shell_exec` — run ONE shell line through the gated shell lane.
    ///
    /// Forwards `IpcRequest::ShellExec`; the daemon spawns
    /// `[shell, "-lc", shell_line]` ONLY on an `AllowWithAudit` verdict for
    /// `PolicyAction::CommandShellStart` (gated by the `allow_shell`
    /// capability, denied by default). The shell lane skips the
    /// argv `SHELL_INTERPRETERS_DENY` guard, so its denials are
    /// `PolicyDenied`, never `ShellInterpreterDenied`. The reply reuses the
    /// `command_start_combed` bounded shape (`job_id`/`bucket_id`/`probe_id`/
    /// `cursor`) and never returns raw stdout/stderr.
    ///
    /// MCP carries `shell_line` ONLY — capabilities are config/TOML, never an
    /// MCP-flippable flag.
    #[tool(
        description = "Run ONE shell line (pipelines/compounds/redirects via [shell,-lc,line]) and get back ONLY the lines your rules match plus exit state, never the raw stream. Requires the allow_shell capability (config/TOML, denied by default); a denied daemon returns a policy error. Returns job_id, bucket_id, probe_id, initial cursor. Use command_start_combed (argv only) when you do not need shell syntax; prefer plain shell for tiny one-off commands whose full output you want verbatim."
    )]
    async fn shell_exec(
        &self,
        Parameters(params): Parameters<McpShellExecParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        // Preserve the original text only for observability classification;
        // the daemon receives the exact same shell line through into_params.
        let shell_line = params.shell_line.clone();
        let ipc = params.into_params()?;
        match self.daemon.call(IpcRequest::ShellExec(ipc)).await {
            Ok(IpcResponse::CommandStartCombed(response)) => {
                json_tool_result(&shell_exec_payload(&response, &shell_line))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
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
        let compact = params.compact;
        let ipc = params.into_ipc().map_err(invalid_params)?;
        match self.daemon.call(IpcRequest::BucketEventsSince(ipc)).await {
            Ok(IpcResponse::BucketEventsSince(r)) => {
                json_tool_result(&bucket_events_payload(&r, compact))
            }
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
        let compact = params.compact;
        let ipc = params.into_ipc().map_err(invalid_params)?;
        match self.daemon.call(IpcRequest::BucketWait(ipc)).await {
            Ok(IpcResponse::BucketWait(r)) => json_tool_result(&bucket_wait_payload(&r, compact)),
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
                stream_mismatches,
            })) => json_tool_result(&serde_json::json!({
                "matches": matches,
                "truncated_bytes": truncated_bytes,
                // F8b (trust): sample indices whose regex matched but whose
                // stream the rule's `stream` filter excluded -- surfaced so
                // the operator sees WHY an apparent match did not fire.
                "stream_mismatches": stream_mismatches,
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
                superseded_versions,
            })) => json_tool_result(&serde_json::json!({
                "rule_id": rule_id,
                "version": version,
                "was_already_active": was_already_active,
                "scope": scope,
                "jobs_rebound": jobs_rebound,
                "superseded_versions": superseded_versions,
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

    /// Resolve the LATEST stored version of `rule_id` via one bounded
    /// `RegistryGet` round trip (the daemon returns the latest version when the
    /// request omits `version`). Used by `registry_deactivate` to turn an
    /// omitted MCP `version` into the concrete version the wire IPC requires,
    /// symmetric with `registry_activate`'s daemon-side latest resolution. A
    /// missing rule surfaces the daemon's `RuleNotFound` error unchanged.
    async fn resolve_latest_rule_version(&self, rule_id: &str) -> Result<u32, McpError> {
        let ipc = RegistryGetParams {
            rule_id: rule_id.to_owned(),
            version: None,
        };
        match self.daemon.call(IpcRequest::RegistryGet(ipc)).await {
            Ok(IpcResponse::RegistryGet(RegistryGetResponse { definition })) => {
                Ok(definition.version)
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `registry_deactivate` — remove a rule, a list, or a whole pack from the active set.
    #[tool(
        description = "Deactivate rules under one scope. Provide EXACTLY ONE selector: rule_id (single rule), rule_ids (explicit list, one call), or pack (every member of a seed pack, one call). version is OPTIONAL and only valid with rule_id: omit it to deactivate the LATEST stored version (echoed in the response), or pass it to target a specific version -- used verbatim, never widened. scope is REQUIRED and must match the scope used at activation (e.g. {kind:'global'}); an omitted scope is rejected. Bulk selectors report per-rule outcomes (deactivated / not_active / unknown_rule) -- partial success is never silent. Future commands skip the rule(s); already-running commands keep the rules they were started with."
    )]
    async fn registry_deactivate(
        &self,
        Parameters(params): Parameters<McpRegistryDeactivateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        // `scope` is schema-required (TB-5): rmcp rejects an omitted
        // scope before this handler runs.
        let scope = params.scope.into_ipc_scope()?;

        // Exactly ONE selector of {rule_id, rule_ids, pack}. All three are
        // schema-optional; this validator owns required-ness and teaches
        // the whole set in one error (US2/FR-011).
        let selector_count = usize::from(params.rule_id.is_some())
            + usize::from(params.rule_ids.is_some())
            + usize::from(params.pack.is_some());
        if selector_count != 1 {
            return Err(invalid_params(
                "registry deactivate requires EXACTLY ONE of 'rule_id' (single rule), \
                 'rule_ids' (list of rule ids), or 'pack' (whole seed pack); you supplied \
                 none or more than one"
                    .to_owned(),
            ));
        }
        // `version` is meaningful only with a single `rule_id`.
        if params.version.is_some() && params.rule_id.is_none() {
            return Err(invalid_params(
                "'version' is only valid with 'rule_id'; a bulk deactivate by 'rule_ids' or \
                 'pack' closes each rule's active version(s) under the scope"
                    .to_owned(),
            ));
        }

        if let Some(rule_id) = params.rule_id {
            // Single-rule path: byte-identical to the historical behavior.
            // An omitted version resolves to the LATEST stored version, so a
            // version-less deactivate is symmetric with the version-less
            // activate that opened the row. The wire IPC keeps `scope:
            // Option` for backward compatibility, so wrap in `Some`.
            let version = match params.version {
                Some(v) => v,
                None => self.resolve_latest_rule_version(&rule_id).await?,
            };
            let ipc = RegistryDeactivateParams {
                rule_id,
                version,
                scope: Some(scope),
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
        } else {
            // Bulk path: rule_ids or pack -> RegistryDeactivateBulk. The
            // daemon reports one outcome per requested rule (partial
            // success is the normal shape) and rebinds live jobs once.
            let ipc = RegistryDeactivateBulkParams {
                pack: params.pack,
                rule_ids: params.rule_ids,
                scope,
            };
            match self
                .daemon
                .call(IpcRequest::RegistryDeactivateBulk(ipc))
                .await
            {
                Ok(IpcResponse::RegistryDeactivateBulk(RegistryDeactivateBulkResponse {
                    outcomes,
                    jobs_rebound,
                })) => json_tool_result(&serde_json::json!({
                    "outcomes": outcomes,
                    "jobs_rebound": jobs_rebound,
                })),
                Ok(other) => Err(unexpected_variant(&other)),
                Err(e) => Err(into_mcp_error_for(false, &e)),
            }
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

    /// `registry_suggest_from_samples` -- suggest DRAFT parsing rules from
    /// raw output samples (US2 / FR-007). Thin forwarder: the daemon runs
    /// the pure heuristic suggester. NEVER activates or persists a rule;
    /// the response names the explicit test->upsert->activate loop.
    #[tool(
        description = "Suggest candidate parsing rules from raw output sample lines using pure deterministic heuristics (error/warning/FAILED/file:line/exit shapes). Returns DRAFT proposals, a confidence label, and the explicit registry_test->registry_upsert->registry_activate next steps. NEVER activates or persists a rule; low-signal input returns an empty proposal set with an explanation, not a junk rule."
    )]
    async fn registry_suggest_from_samples(
        &self,
        Parameters(params): Parameters<McpRegistrySuggestFromSamplesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = RegistrySuggestFromSamplesParams {
            samples: params.sample_lines,
            intent: params.intent,
            max_rules: params.max_rules,
        };
        match self
            .daemon
            .call(IpcRequest::RegistrySuggestFromSamples(ipc))
            .await
        {
            Ok(IpcResponse::RegistrySuggestFromSamples(RegistrySuggestFromSamplesResponse {
                proposed_rules,
                confidence,
                next_steps,
                explanation,
            })) => json_tool_result(&serde_json::json!({
                "proposed_rules": proposed_rules,
                "confidence": confidence,
                "next_steps": next_steps,
                "explanation": explanation,
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

    /// `file_list_dir` — bounded single-level directory listing (US3).
    ///
    /// A `files` facade ACTION forwarder, deliberately NOT a granular `#[tool]`,
    /// so the MCP tool count is unchanged. Thin 1:1 forward to the daemon
    /// `FileListDir` IPC method: the adapter never touches the filesystem. The
    /// daemon applies the same read-path policy gate as `read` and returns a
    /// bounded, deterministically ordered, truncation-flagged listing.
    async fn file_list_dir(
        &self,
        Parameters(params): Parameters<McpFileListDirParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = FileListDirParams {
            path: params.path,
            max_entries: params.max_entries,
        };
        match self.daemon.call(IpcRequest::FileListDir(ipc)).await {
            Ok(IpcResponse::FileListDir(FileListDirResponse {
                path,
                entries,
                total_entries,
                truncated,
            })) => json_tool_result(&serde_json::json!({
                "path": path,
                "entries": entries,
                "total_entries": total_entries,
                "truncated": truncated,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `file_write` — write UTF-8 content to one file (TC22 A3).
    ///
    /// Thin 1:1 forward to the daemon `FileWrite` IPC method, identical in
    /// shape to `file_read_window`: the adapter never touches the filesystem.
    /// The daemon policy-gates the canonical target against `paths.write_allow`,
    /// audits BEFORE the write, bounds the content size, and writes atomically.
    #[tool(
        description = "Write UTF-8 content to a file. Policy-gated by paths.write_allow; audited before write; bounded size; atomic (no torn writes). Mutating + non-idempotent: never auto-retried."
    )]
    async fn file_write(
        &self,
        Parameters(params): Parameters<McpFileWriteParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = FileWriteParams {
            path: std::path::PathBuf::from(params.path),
            content: params.content,
            create_dirs: params.create_dirs.unwrap_or(false),
            append: params.append.unwrap_or(false),
        };
        match self.daemon.call(IpcRequest::FileWrite(ipc)).await {
            Ok(IpcResponse::FileWrite(FileWriteResponse {
                path,
                bytes_written,
            })) => json_tool_result(&serde_json::json!({
                "path": path,
                "bytes_written": bytes_written,
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
            cursor: params.cursor,
            wait_ms: params.wait_ms,
        };
        match self
            .daemon
            .call(IpcRequest::PtyCommandWriteStdin(ipc))
            .await
        {
            // FR-041: serialize the response directly. The combed-batch
            // fields carry `skip_serializing_if = Option::is_none`, so a
            // no-wait response omits every one -> byte-identical to today;
            // a wait_ms response surfaces the combed batch in one call.
            Ok(IpcResponse::PtyCommandWriteStdin(resp)) => json_tool_result(&resp),
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

    /// `shell_session_start` — start a persistent shell session.
    ///
    /// Thin forwarder: the adapter NEVER spawns (constitution I). The
    /// daemon's session runtime performs the `SessionStart` policy gate +
    /// audit BEFORE the shell PTY is spawned. Denied by default
    /// (`allow_session` off). Returns bounded start metadata only.
    #[tool(
        description = "Start a persistent shell session: a long-lived login shell whose cwd/env stay sticky across shell_session_exec lines. Requires the allow_session capability; denied by default. Combed output only, never a raw stream."
    )]
    async fn shell_session_start(
        &self,
        Parameters(params): Parameters<McpShellSessionStartParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let env: Vec<(String, String)> = params.env.into_iter().map(|e| (e.key, e.value)).collect();
        let (bucket_config, rules) =
            parse_bucket_and_rules(params.bucket_config_json, params.rules, params.rules_json)?;
        let ipc = ShellSessionStartParams {
            shell: params.shell,
            cwd: params.cwd.map(std::path::PathBuf::from),
            env,
            rules,
            bucket_config,
            tag: params.tag,
        };
        match self.daemon.call(IpcRequest::ShellSessionStart(ipc)).await {
            Ok(IpcResponse::ShellSessionStart(ShellSessionStartResponse {
                session_id,
                bucket_id,
                state,
            })) => json_tool_result(&serde_json::json!({
                "session_id": session_id,
                "bucket_id": bucket_id,
                "state": state,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `shell_session_exec` — run ONE line in a session shell.
    ///
    /// Thin forwarder. The daemon writes the line to the persistent shell
    /// and returns the combed signals the line produced (read from the
    /// session bucket via a bounded wait). A send to a non-live session
    /// fails loudly; never a raw stream.
    #[tool(
        description = "Run ONE shell line inside a persistent session. The line executes in the session's sticky cwd/env (set by prior lines). Returns the combed signals the line produced plus the next cursor; never a raw stream."
    )]
    async fn shell_session_exec(
        &self,
        Parameters(params): Parameters<McpShellSessionExecParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        use terminal_commander_core::ids::SessionIdKind;
        let session_id =
            parse_id::<SessionIdKind>("session_id", &params.session_id).map_err(invalid_params)?;
        let ipc = ShellSessionExecParams {
            session_id,
            line: params.line,
            cursor: params.cursor.unwrap_or(0),
            wait_ms: params.wait_ms,
        };
        match self.daemon.call(IpcRequest::ShellSessionExec(ipc)).await {
            Ok(IpcResponse::ShellSessionExec(ShellSessionExecResponse {
                session_id,
                bucket_id,
                bytes_written,
                cursor_in,
                next_cursor,
                has_more,
                dropped_count,
                events,
            })) => json_tool_result(&serde_json::json!({
                "session_id": session_id,
                "bucket_id": bucket_id,
                "bytes_written": bytes_written,
                "cursor_in": cursor_in,
                "next_cursor": next_cursor,
                "has_more": has_more,
                "dropped_count": dropped_count,
                "events": events,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `shell_session_status` — session state, cwd, bounded env snapshot.
    #[tool(
        description = "Report a session's lifecycle state, current working directory, and a bounded environment snapshot."
    )]
    async fn shell_session_status(
        &self,
        Parameters(params): Parameters<McpShellSessionRefParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        use terminal_commander_core::ids::SessionIdKind;
        let session_id =
            parse_id::<SessionIdKind>("session_id", &params.session_id).map_err(invalid_params)?;
        let ipc = ShellSessionStatusParams { session_id };
        match self.daemon.call(IpcRequest::ShellSessionStatus(ipc)).await {
            Ok(IpcResponse::ShellSessionStatus(ShellSessionStatusResponse {
                session_id,
                bucket_id,
                state,
                cwd,
                env_snapshot,
                last_active_at,
            })) => json_tool_result(&serde_json::json!({
                "session_id": session_id,
                "bucket_id": bucket_id,
                "state": state,
                "cwd": cwd,
                "env_snapshot": env_snapshot,
                "last_active_at": last_active_at,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `shell_session_stop` — terminate a session (graceful then forced).
    #[tool(
        description = "Stop a persistent session: terminate its shell (graceful then forced) and report the terminal state."
    )]
    async fn shell_session_stop(
        &self,
        Parameters(params): Parameters<McpShellSessionRefParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        use terminal_commander_core::ids::SessionIdKind;
        let session_id =
            parse_id::<SessionIdKind>("session_id", &params.session_id).map_err(invalid_params)?;
        let ipc = ShellSessionStopParams { session_id };
        match self.daemon.call(IpcRequest::ShellSessionStop(ipc)).await {
            Ok(IpcResponse::ShellSessionStop(ShellSessionStopResponse {
                session_id,
                state,
                terminal_reason,
            })) => json_tool_result(&serde_json::json!({
                "session_id": session_id,
                "state": state,
                "terminal_reason": terminal_reason,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `shell_session_list` — snapshot of every live session.
    #[tool(description = "Snapshot of every currently-live session (id, state, cwd, last_active).")]
    async fn shell_session_list(&self) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        match self.daemon.call(IpcRequest::ShellSessionList).await {
            Ok(IpcResponse::ShellSessionList(ShellSessionListResponse { sessions })) => {
                json_tool_result(&serde_json::json!({ "sessions": sessions }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }

    /// `workspace_snapshot_create` — persist a session's cwd + bounded env.
    #[tool(
        description = "Save a session's current working directory and bounded environment as a restorable workspace snapshot. Returns the snapshot id."
    )]
    async fn workspace_snapshot_create(
        &self,
        Parameters(params): Parameters<McpWorkspaceSnapshotCreateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        use terminal_commander_core::ids::SessionIdKind;
        let session_id =
            parse_id::<SessionIdKind>("session_id", &params.session_id).map_err(invalid_params)?;
        let ipc = WorkspaceSnapshotCreateParams {
            session_id,
            name: params.name,
        };
        match self
            .daemon
            .call(IpcRequest::WorkspaceSnapshotCreate(ipc))
            .await
        {
            Ok(IpcResponse::WorkspaceSnapshotCreate(WorkspaceSnapshotCreateResponse {
                snapshot_id,
            })) => json_tool_result(&serde_json::json!({ "snapshot_id": snapshot_id })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
        }
    }

    /// `workspace_snapshot_apply` — restore a snapshot's cwd/env into a session.
    #[tool(
        description = "Restore a saved workspace snapshot's cwd + bounded env into a session. Returns the restored cwd."
    )]
    async fn workspace_snapshot_apply(
        &self,
        Parameters(params): Parameters<McpWorkspaceSnapshotApplyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        use terminal_commander_core::ids::SessionIdKind;
        let session_id =
            parse_id::<SessionIdKind>("session_id", &params.session_id).map_err(invalid_params)?;
        let ipc = WorkspaceSnapshotApplyParams {
            snapshot_id: params.snapshot_id,
            session_id,
        };
        match self
            .daemon
            .call(IpcRequest::WorkspaceSnapshotApply(ipc))
            .await
        {
            Ok(IpcResponse::WorkspaceSnapshotApply(WorkspaceSnapshotApplyResponse {
                applied,
                session_id,
                cwd,
            })) => json_tool_result(&serde_json::json!({
                "applied": applied,
                "session_id": session_id,
                "cwd": cwd,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error_for(false, &e)),
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
            // US4 / FR-031: the adapter ALWAYS requests the liveness delta --
            // the agent-facing token saving IS the feature (SC-004). The daemon
            // sends the full snapshot on the first pull and after a seek, then
            // only changed entries.
            liveness_delta: true,
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
                // US4 / FR-031: include the liveness section ONLY when the delta
                // is non-empty (first pull after open/seek = full snapshot;
                // steady idle = omitted). This is the agent-facing byte saving.
                let mut payload = serde_json::json!({
                    "events": events,
                    "lagged": lagged,
                    "truncated": truncated,
                });
                if !liveness.is_empty() {
                    payload["liveness"] = serde_json::json!(liveness);
                }
                json_tool_result(&payload)
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

    /// `command` -- run + observe + stream a one-shot command (compact surface).
    #[tool(
        name = "command",
        description = "Run and observe a one-shot command. Key contracts: \
`run_and_watch`: `argv` + `wait_ms` (default 5,000; max 60,000; not `timeout_ms`). \
If incomplete, resume signals with `wait`: `bucket_id` + `cursor` + `timeout_ms` \
(not `job_id` or `wait_ms`), and check state with `status`: `job_id`. \
`summary`: `bucket_id` only (no `compact`). `exec`: `shell_line` (policy-gated); common pipelines are \
executed unchanged and an omitted shell uses the highest-ranked confirmed host route; inspect status \
system_discover for host-specific syntax. Shell-side tail/head/grep makes the receipt observability-limited because discarded \
lines never reach TC; use `run_and_watch` + rules or `files` search when full evidence matters. \
Other actions: run, output_tail, stop, events, event_context, sub_open, sub_pull, \
sub_seek, sub_close, sub_list."
    )]
    pub(crate) async fn command_facade(
        &self,
        Parameters(call): Parameters<crate::facades::CommandFacadeCall>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        use crate::facades::CommandFacadeCall as C;
        match call {
            C::Run(p) => self.command_start_combed(Parameters(p)).await,
            C::RunAndWatch(p) => self.run_and_watch(Parameters(p)).await,
            C::Exec(p) => self.shell_exec(Parameters(p)).await,
            C::Status(p) => self.command_status(Parameters(p)).await,
            C::OutputTail(p) => self.command_output_tail(Parameters(p)).await,
            C::Stop(p) => self.command_stop(Parameters(p)).await,
            C::Events(p) => self.bucket_events_since(Parameters(p)).await,
            C::Wait(p) => self.bucket_wait(Parameters(p)).await,
            C::Summary(p) => self.bucket_summary(Parameters(p)).await,
            C::EventContext(p) => self.event_context(Parameters(p)).await,
            C::SubOpen(p) => self.subscription_open(Parameters(p)).await,
            C::SubPull(p) => self.subscription_pull(Parameters(p), ctx).await,
            C::SubSeek(p) => self.subscription_seek(Parameters(p)).await,
            C::SubClose(p) => self.subscription_close(Parameters(p)).await,
            C::SubList(p) => self.subscription_list(Parameters(p)).await,
        }
    }

    /// `session` facade — PTY commands + persistent shell sessions.
    #[tool(
        name = "session",
        description = "PTY commands and persistent shell sessions. To start a PTY command use \
action=\"pty_start\"; write stdin with pty_stdin; stop with pty_stop; list with pty_list. \
For sticky-cwd sessions (unix-only; unavailable on Windows): sh_start (requires allow_session), sh_exec, sh_status, sh_stop, sh_list."
    )]
    pub(crate) async fn session_facade(
        &self,
        Parameters(call): Parameters<crate::facades::SessionFacadeCall>,
    ) -> Result<CallToolResult, McpError> {
        use crate::facades::SessionFacadeCall as S;
        match call {
            S::PtyStart(p) => self.pty_command_start(Parameters(p)).await,
            S::PtyStdin(p) => self.pty_command_write_stdin(Parameters(p)).await,
            S::PtyStop(p) => self.pty_command_stop(Parameters(p)).await,
            S::PtyList => self.pty_command_list().await,
            S::ShStart(p) => self.shell_session_start(Parameters(p)).await,
            S::ShExec(p) => self.shell_session_exec(Parameters(p)).await,
            S::ShStatus(p) => self.shell_session_status(Parameters(p)).await,
            S::ShStop(p) => self.shell_session_stop(Parameters(p)).await,
            S::ShList => self.shell_session_list().await,
        }
    }

    /// `files` facade — file read/search/write + watch + workspace snapshots.
    #[tool(
        name = "files",
        description = "File operations: bounded read (action=\"read\"), directory listing (action=\"list\"), \
substring search, atomic write, file-watch start/stop/list, and workspace snapshots \
(snapshot_create, snapshot_apply). All paths must be absolute."
    )]
    pub(crate) async fn files_facade(
        &self,
        Parameters(call): Parameters<crate::facades::FilesFacadeCall>,
    ) -> Result<CallToolResult, McpError> {
        use crate::facades::FilesFacadeCall as F;
        match call {
            F::Read(p) => self.file_read_window(Parameters(p)).await,
            F::Search(p) => self.file_search(Parameters(p)).await,
            F::List(p) => self.file_list_dir(Parameters(p)).await,
            F::Write(p) => self.file_write(Parameters(p)).await,
            F::WatchStart(p) => self.file_watch_start(Parameters(p)).await,
            F::WatchStop(p) => self.file_watch_stop(Parameters(p)).await,
            F::WatchList => self.file_watch_list().await,
            F::SnapshotCreate(p) => self.workspace_snapshot_create(Parameters(p)).await,
            F::SnapshotApply(p) => self.workspace_snapshot_apply(Parameters(p)).await,
        }
    }

    /// `registry` facade — rule registry CRUD + test + suggest.
    #[tool(
        name = "registry",
        description = "Rule registry: search, get, upsert, test (dry-run), activate, deactivate, \
list_active, import_pack (25 built-in packs), suggest_from_samples (heuristic DRAFT proposals). \
`import_pack` requires `pack`; activate=true additionally requires a `scope` object, \
usually {\"kind\":\"global\"}. Rules comb command output into structured signals."
    )]
    pub(crate) async fn registry_facade(
        &self,
        Parameters(call): Parameters<crate::facades::RegistryFacadeCall>,
    ) -> Result<CallToolResult, McpError> {
        use crate::facades::RegistryFacadeCall as R;
        match call {
            R::Search(p) => self.registry_search(Parameters(p)).await,
            R::Get(p) => self.registry_get(Parameters(p)).await,
            R::Upsert(p) => self.registry_upsert(Parameters(p)).await,
            R::Test(p) => self.registry_test(Parameters(p)).await,
            R::Activate(p) => self.registry_activate(Parameters(p)).await,
            R::Deactivate(p) => self.registry_deactivate(Parameters(p)).await,
            R::ListActive(p) => self.registry_list_active(Parameters(p)).await,
            R::ImportPack(p) => self.registry_import_pack(Parameters(p)).await,
            R::SuggestFromSamples(p) => self.registry_suggest_from_samples(Parameters(p)).await,
        }
    }

    /// `status` facade — health, policy, runtime state, probes, targets.
    #[tool(
        name = "status",
        description = "Adapter and daemon status: health ping (action=\"health\"), self_check, \
policy_status, runtime_state (aggregate snapshot), probe_list, probe_status, target_list, target_probe. \
Call system_discover before choosing an interpreter: its environment contains bounded shell/PowerShell/WSL/tool probes, \
ranked access_routes, and a beachhead with the exact confirmed argv template to follow."
    )]
    pub(crate) async fn status_facade(
        &self,
        Parameters(call): Parameters<crate::facades::StatusFacadeCall>,
    ) -> Result<CallToolResult, McpError> {
        use crate::facades::StatusFacadeCall as St;
        match call {
            St::Health => self.health().await,
            St::SelfCheck => self.self_check().await,
            St::PolicyStatus => self.policy_status().await,
            St::RuntimeState(p) => self.runtime_state(Parameters(p)).await,
            St::ProbeList(p) => self.probe_list(Parameters(p)).await,
            St::ProbeStatus(p) => self.probe_status(Parameters(p)).await,
            St::SystemDiscover => self.system_discover().await,
            St::TargetList => self.target_list().await,
            St::TargetProbe(p) => self.target_probe(Parameters(p)).await,
        }
    }
}

// Hand-written `ServerHandler` (replaces the rmcp `#[tool_handler]` macro) so
// the `tools/list` and `tools/call` paths can honor `TC_SURFACE`:
//   - `list_tools` advertises the compact facade(s) under `TC_SURFACE=compact`,
//     else the unchanged granular tools (with facade names filtered OUT so the
//     full surface stays EXACTLY the 50 legacy tools).
//   - `call_tool` runs the admission gate, then delegates to the SAME router
//     the macro used (`ToolCallContext` + `self.tool_router.call`).
//   - `get_tool` mirrors the macro (router lookup) to preserve task-support
//     validation behavior identically.
// `get_info` / `initialize` are unchanged.
//
// Signatures confirmed against rmcp 1.7.0 (`rmcp::handler::server::ServerHandler`
// + `rmcp-macros` `tool_handler`):
//   call_tool(&self, CallToolRequestParams, RequestContext<RoleServer>)
//     -> Result<CallToolResult, McpError>
//   list_tools(&self, Option<PaginatedRequestParams>, RequestContext<RoleServer>)
//     -> Result<ListToolsResult, McpError>
impl ServerHandler for TerminalCommanderMcpServer {
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = match self
            .surface_override
            .unwrap_or_else(crate::surface::surface_from_env)
        {
            crate::surface::Surface::Compact => crate::surface_list::compact_surface_tools(),
            // `full` keeps the granular surface EXACTLY the 50 legacy tools:
            // the facade handler is registered on the same router, so filter
            // its name(s) OUT of `list_all()` -- the facade must not leak into
            // the full list.
            crate::surface::Surface::Full => self
                .tool_router
                .list_all()
                .into_iter()
                .filter(|t| !crate::surface_list::COMPACT_TOOL_NAMES.contains(&t.name.as_ref()))
                .collect(),
        };
        Ok(ListToolsResult {
            tools,
            next_cursor: None,
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        // Admission gate FIRST: under `compact`, reject any name not on the
        // facade set with a clear "set TC_SURFACE=full" message. Under `full`
        // every name is admitted and the router routes by name as before.
        crate::surface_list::enforce_surface(
            self.surface_override
                .unwrap_or_else(crate::surface::surface_from_env),
            request.name.as_ref(),
        )?;
        // US1 strictness: for the five facade tools, validate the raw call
        // object against the advertised action schema BEFORE the router
        // deserializes the *FacadeCall enum. A well-formed call passes untouched
        // (byte-identical dispatch); a malformed one gets ONE teaching error
        // naming the action + every missing/unknown field. Legacy granular tools
        // are not facades and are not validated here.
        if crate::surface_list::COMPACT_TOOL_NAMES.contains(&request.name.as_ref()) {
            let call = serde_json::Value::Object(request.arguments.clone().unwrap_or_default());
            crate::facade_strict::validate_facade_call(request.name.as_ref(), &call)?;
        }
        // Delegate to the SAME router the macro used -- dispatch still flows
        // through `self.tool_router`; no hand-rolled per-tool match.
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        // Mirror the macro: look the tool up in the router so task-support
        // validation (rmcp `handle_request`) behaves identically. The gate in
        // `call_tool` is what enforces the surface; `get_tool` only feeds
        // task-mode validation and never bypasses it.
        self.tool_router.get(name).cloned()
    }

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

/// Build the shell-start receipt. Common LLM shell pipelines remain valid
/// and execute byte-for-byte as requested. When an obvious shell-side
/// prefilter discards output before daemon ingestion, add a factual
/// observability receipt so the caller knows TC cannot recover those lines.
fn shell_exec_payload(response: &CommandStartResponse, shell_line: &str) -> serde_json::Value {
    let CommandStartResponse {
        job_id,
        bucket_id,
        probe_id,
        cursor,
        hint: _,
    } = response;
    let mut payload = serde_json::json!({
        "job_id": job_id,
        "bucket_id": bucket_id,
        "probe_id": probe_id,
        "cursor": cursor,
    });

    let lower = shell_line.to_ascii_lowercase();
    let mut detected = Vec::new();
    for (name, needles) in [
        ("tail", &["| tail", "|tail"][..]),
        ("head", &["| head", "|head"][..]),
        ("grep", &["| grep", "|grep"][..]),
        ("findstr", &["| findstr", "|findstr"][..]),
        (
            "select-object",
            &[
                "| select-object -first",
                "|select-object -first",
                "| select-object -last",
                "|select-object -last",
            ][..],
        ),
        ("select-string", &["| select-string", "|select-string"][..]),
    ] {
        if needles.iter().any(|needle| lower.contains(needle)) {
            detected.push(name);
        }
    }

    if !detected.is_empty() {
        payload.as_object_mut().expect("shell receipt is an object").insert(
            "observability".into(),
            serde_json::json!({
                "status": "limited",
                "executed_unchanged": true,
                "reason": "shell_prefilter",
                "detected_prefilters": detected,
                "message": "The shell filtered output before TC ingestion; TC executed the line unchanged and cannot recover discarded lines.",
                "better_route": r#"Run the producer directly with command action="run_and_watch" plus rules; use files action="search" for existing logs."#,
            }),
        );
    }

    payload
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
    // coded but is "could not reach the daemon", not a server fault. The raw
    // transport detail stays out of the top-level message but rides in the
    // structured details as `transport_detail` -- diagnosability beats
    // tidiness (dogfood 2026-07-02: five opaque failures, one lost day).
    if e.is_transport() {
        return transport_unavailable_error(request_is_idempotent, e);
    }
    let message: Cow<'static, str> = Cow::Owned(format_ipc_error(e));
    let mut data = serde_json::json!({
        "ipc_code": format!("{:?}", e.code),
    });
    // F7: a non-existent program is a COMMAND ATTEMPT that failed, not a
    // daemon/transport fault. Enrich the structured `data` payload with the
    // failure-receipt vocabulary the agent reasons over -- `error_kind`,
    // `argv0` (read from the TYPED `IpcError::argv0` carrier), and an explicit
    // null `exit_code` (the process never started) -- so a missing program
    // reads as a structured `program_not_found` receipt rather than an
    // opaque error. The code itself is classified `invalid_params` below.
    if e.code == IpcErrorCode::ProgramNotFound {
        if let serde_json::Value::Object(map) = &mut data {
            map.insert(
                "error_kind".to_owned(),
                serde_json::Value::String("program_not_found".to_owned()),
            );
            map.insert("exit_code".to_owned(), serde_json::Value::Null);
            // `argv0` rides as a discrete TYPED field on the IpcError (set by
            // the daemon via `IpcError::program_not_found`), so we copy it
            // straight into the data payload -- no fragile prose parsing. The
            // value survives any wording change to `message` and carries
            // verbatim even when the program name itself contains an
            // apostrophe (the case the old quote-count parse could not
            // recover). When `argv0` is None we OMIT the field entirely --
            // same graceful-degradation contract as before (the message still
            // names the program).
            if let Some(argv0) = &e.argv0 {
                map.insert("argv0".to_owned(), serde_json::Value::String(argv0.clone()));
            }
        }
    }
    // F14: an unsupported-platform error is a caller-ROUTABLE fact, not a
    // daemon fault: the session/snapshot tools are unix-only, so on Windows
    // the agent should route to WSL or a different tool rather than conclude
    // TC is broken. Enrich the structured `data` with the receipt vocabulary
    // the agent reasons over -- `error_kind`, the HONEST host `platform`
    // (`std::env::consts::OS`), and the unavailable `tool` (read from the
    // TYPED `IpcError::tool` carrier, set by the daemon via
    // `IpcError::unsupported_platform`). When `tool` is None we OMIT the field
    // entirely -- same graceful-degradation contract as `argv0`. The code
    // itself is classified `invalid_params` below.
    if e.code == IpcErrorCode::UnsupportedPlatform {
        if let serde_json::Value::Object(map) = &mut data {
            map.insert(
                "error_kind".to_owned(),
                serde_json::Value::String("unsupported_platform".to_owned()),
            );
            map.insert(
                "platform".to_owned(),
                serde_json::Value::String(std::env::consts::OS.to_owned()),
            );
            if let Some(tool) = &e.tool {
                map.insert("tool".to_owned(), serde_json::Value::String(tool.clone()));
            }
        }
    }
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
        // F7: a non-existent program is a caller-fixable command attempt
        // (typo / wrong PATH / missing binary). `invalid_params` (-32602)
        // tells the agent to correct argv0, NOT `internal_error` (-32603)
        // which would train it to abandon TC for raw shell.
        | IpcErrorCode::ProgramNotFound
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
        | IpcErrorCode::SubscriptionLimitExceeded
        // Session lane (P1 / TC50): all caller-fixable. UnknownSession ->
        // re-start; SessionNotLive -> the shell is dead, start a new one;
        // SessionLimitExceeded -> stop a session and retry.
        | IpcErrorCode::UnknownSession
        | IpcErrorCode::SessionNotLive
        | IpcErrorCode::SessionLimitExceeded
        // F14: an unsupported platform is a caller-ROUTABLE fact, not a server
        // fault. The session/snapshot tools are unix-only; on Windows the agent
        // should route to WSL or pick a different tool. `invalid_params`
        // (-32602) keeps it reasoning (the data payload names the unavailable
        // tool + host platform), whereas `internal_error` (-32603) would read
        // as "TC is broken" and train it to abandon TC for raw shell.
        | IpcErrorCode::UnsupportedPlatform => McpError::invalid_params(message, Some(data)),
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

/// Append only rule-driven events to `signals`, never exceeding `max_signals`.
/// Probe-lifecycle markers (no `rule`) are skipped: they are the exit indicator
/// surfaced via exit_code/state and the receipt, not matched signals -- counting
/// them would defeat the zero-signal receipt path.
fn collect_rule_signals(
    events: Vec<terminal_commander_core::SignalEvent>,
    signals: &mut Vec<terminal_commander_core::SignalEvent>,
    max_signals: usize,
) {
    for ev in events {
        if ev.rule.is_some() && signals.len() < max_signals {
            signals.push(ev);
        }
    }
}

/// Project a [`SignalEvent`] to the compact presentation shape
/// `{summary, stream, seq, severity}` (TC-E1 / FR-028). This is the set
/// of load-bearing fields an agent reads in the common case; the id
/// plumbing (`event_id`, `bucket_id`, `source`, `pointer`, ...) that
/// dominates token cost is dropped. PRESENTATION ONLY: the event store
/// retains the full record, re-fetchable via `bucket_events_since`.
fn project_signal_compact(ev: &terminal_commander_core::SignalEvent) -> serde_json::Value {
    serde_json::json!({
        "summary": ev.summary,
        "stream": ev.source.stream,
        "seq": ev.seq,
        "severity": ev.severity,
    })
}

/// Build the `run_and_watch` result payload. ONE builder for BOTH the normal
/// and the degraded (mid-wait IPC error) paths, so the degraded result is a
/// strict superset of the normal one and the two cannot drift.
///
/// `degraded` is true only when an IPC error interrupted the wait AFTER a
/// job_id was minted (TC-1b): the job stays tracked, so the call returns an
/// isError:false, job-identified result carrying `degraded:true` and a
/// `recover_hint` rather than a bare error. `last_observed_state` is `None`
/// when the daemon failed before the first status poll -- the state is then
/// reported as "unknown", never a silent "running".
#[allow(clippy::too_many_arguments)]
fn run_and_watch_result(
    job_id: terminal_commander_core::JobId,
    bucket_id: terminal_commander_core::BucketId,
    cursor: u64,
    last_observed_state: Option<terminal_commander_core::JobState>,
    exit_code: Option<i32>,
    signals: &[terminal_commander_core::SignalEvent],
    receipt: Option<serde_json::Value>,
    degraded: bool,
    recover_hint: Option<&str>,
    compact: bool,
    // F6 (truncation honesty): true iff the returned `signals` array was
    // limited by `max_signals` and more matches may exist beyond `cursor`.
    // Computed by the caller (which owns `max_signals`) as
    // `signals.len() >= max_signals`; the degraded paths pass `false` because
    // an interrupted wait did not necessarily cap, and `degraded:true` already
    // marks the result incomplete.
    signals_capped: bool,
) -> Result<CallToolResult, McpError> {
    // A degraded result is never "complete" (the wait was interrupted);
    // otherwise derive completion from the last observed state.
    let (complete, wait_exhausted) = if degraded {
        (false, true)
    } else {
        last_observed_state.map_or((false, true), run_and_watch_completion)
    };
    let recover_hint = recover_hint
        .or_else(|| (!degraded && wait_exhausted).then_some(RUN_AND_WATCH_WAIT_EXHAUSTED_HINT));
    // State honesty: report the last observed state, or "unknown" when we never
    // got one -- never a silent "running".
    let state_json = last_observed_state.map_or_else(
        || serde_json::json!("unknown"),
        |state| serde_json::json!(state),
    );
    // The receipt rides only the zero-signal success path; a degraded result
    // carries none.
    let include_receipt = !degraded && signals.is_empty();
    // TC-E1: when compact, project each signal to the load-bearing fields
    // only -- a presentation concern; the event store is untouched and the
    // full records remain re-fetchable via bucket_events_since.
    let signals_json = if compact {
        serde_json::json!(
            signals
                .iter()
                .map(project_signal_compact)
                .collect::<Vec<_>>()
        )
    } else {
        serde_json::json!(signals)
    };
    // TC-E2: a still-running result advertises a poll interval hint so the
    // agent paces command_status polling instead of busy-spinning. Terminal
    // and degraded results omit it (nothing left to poll for / recover first).
    let poll_hint_ms = if complete || degraded {
        serde_json::Value::Null
    } else {
        serde_json::json!(RUN_AND_WATCH_POLL_HINT_MS)
    };
    json_tool_result(&serde_json::json!({
        "job_id": job_id,
        "bucket_id": bucket_id,
        "state": state_json,
        "exit_code": exit_code,
        "signals": signals_json,
        "signal_count": signals.len(),
        // F6: explicit truncation flag -- true when `signals` hit `max_signals`
        // and more matches may exist beyond `cursor` (poll/resume to fetch them).
        "signals_capped": signals_capped,
        "compact": compact,
        "receipt": if include_receipt { receipt } else { None },
        "complete": complete,
        "wait_exhausted": wait_exhausted,
        "wait_cap_ms": RUN_AND_WATCH_MAX_WAIT_MS,
        "poll_hint_ms": poll_hint_ms,
        "cursor": cursor,
        "degraded": degraded,
        "recover_hint": recover_hint,
    }))
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

/// Build the structured `daemon_version_skew` MCP error (DEFECT B).
///
/// Returned when the daemon is ALIVE (it answered `system_discover`) but its
/// version does not match this adapter. Distinct from `daemon_unavailable`: the
/// daemon is reachable, it is the WRONG build, so a misleading "not reachable"
/// envelope would send an operator chasing a phantom connectivity problem. The
/// message names BOTH versions and the remedy points at the WSL npm runtime
/// update / restart.
fn daemon_version_skew_error(daemon: &str, adapter: &str) -> McpError {
    let payload = serde_json::json!({
        "code": "daemon_version_skew",
        "message": format!(
            "terminal-commanderd version {daemon} does not match this adapter version {adapter}"
        ),
        "details": {
            "daemon_version": daemon,
            "adapter_version": adapter,
            "remedy": "update the WSL terminal-commander npm runtime so the daemon version matches this adapter (or run `terminal-commander restart`)",
        },
    });
    McpError::internal_error("daemon_version_skew", Some(payload))
}

/// Map a [`TargetRouteError`] to a typed MCP error (P5 / T050).
///
/// An unknown `target_id` is a caller-fixable input problem, so it routes
/// through `invalid_params` (JSON-RPC -32602), carrying the offending id and
/// a remedy pointing at `target_list`.
fn route_error_to_mcp(err: &TargetRouteError) -> McpError {
    match err {
        TargetRouteError::UnknownTarget(id) => {
            let payload = serde_json::json!({
                "code": "unknown_target",
                "message": format!("unknown target_id '{id}'"),
                "details": {
                    "target_id": id,
                    "remedy": "call target_list to see registered target_ids, or omit target_id to run locally",
                },
            });
            McpError::invalid_params(
                Cow::Owned(format!("unknown target_id '{id}'")),
                Some(payload),
            )
        }
    }
}

/// Build the typed `remote_denied` error returned when a tool is invoked
/// with a `target_id` but the local daemon has `allow_remote = false`
/// (constitution II default-deny; T051).
fn remote_denied_error(target_id: &str) -> McpError {
    let payload = serde_json::json!({
        "code": "remote_denied",
        "message": "remote federation is disabled by policy (allow_remote = false)",
        "details": {
            "target_id": target_id,
            "cap": "allow_remote",
            "remedy": "the operator must enable [policy.caps] allow_remote = true on the local daemon to route tools to a remote target",
        },
    });
    McpError::invalid_params(
        Cow::Borrowed("remote_denied: allow_remote cap is not enabled"),
        Some(payload),
    )
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
fn transport_unavailable_error(operation_is_idempotent: bool, cause: &IpcError) -> McpError {
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
            // The underlying transport failure, verbatim. Kept OUT of the
            // top-level message (bounded, non-alarming) but carried in the
            // structured details: without it, "timed out" vs "connect
            // refused" vs "pipe busy" are indistinguishable and the true
            // cause is undiagnosable (dogfood 2026-07-02, BACKLOG P1.0f).
            "transport_detail": cause.message,
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

// =====================================================================
// TB-12: lenient option coercion for real-world MCP clients.
//
// schemars derives an `Option<uN>` field as the union schema
// `{"type":["integer","null"]}` and `Option<Vec<T>>` as
// `{"type":["array","null"]}` with `$defs` refs. Several real MCP
// clients flatten those unions (and `$ref`s) away, leaving the param
// UNTYPED — an untyped param is then sent as a STRING ("45000") or a
// JSON-encoded array string, and serde rejects it with an error the
// caller cannot act on (`invalid type: string "45000", expected u64`).
// Two defenses are applied together on every optional field:
//   1. `#[schemars(with = "uN")]` pins a PLAIN string `"type"` on the
//      field schema (optionality is already expressed by the field's
//      absence from `required[]`, so dropping the `null` union member
//      loses nothing).
//   2. The deserializers below additionally ACCEPT the stringified
//      form, with a teaching error on anything unparseable.
// =====================================================================

/// Core of the lenient unsigned-integer coercion: JSON number, numeric
/// string, or null. Everything else (and non-integer numbers) gets a
/// teaching error naming both accepted forms.
fn opt_u64_from_value<E: serde::de::Error>(value: serde_json::Value) -> Result<Option<u64>, E> {
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(ref n) => n
            .as_u64()
            .map(Some)
            .ok_or_else(|| E::custom(format!("expected an unsigned integer (e.g. 5000); got {n}"))),
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(None);
            }
            t.parse::<u64>().map(Some).map_err(|_| {
                E::custom(format!(
                    "expected an unsigned integer as a number or numeric string \
                     (e.g. 5000 or \"5000\"); got \"{s}\""
                ))
            })
        }
        other => Err(E::custom(format!(
            "expected an unsigned integer (e.g. 5000); got {}",
            json_type_name(&other)
        ))),
    }
}

/// Narrow a lenient u64 to a smaller unsigned width with a range check.
fn opt_uint_narrow<T, E>(value: serde_json::Value) -> Result<Option<T>, E>
where
    T: TryFrom<u64>,
    E: serde::de::Error,
{
    opt_u64_from_value::<E>(value)?.map_or_else(
        || Ok(None),
        |v| {
            T::try_from(v)
                .map(Some)
                .map_err(|_| E::custom(format!("{v} exceeds this parameter's accepted range")))
        },
    )
}

fn de_opt_u64_lenient<'de, D>(de: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    opt_u64_from_value(serde_json::Value::deserialize(de)?)
}

fn de_opt_u32_lenient<'de, D>(de: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    opt_uint_narrow(serde_json::Value::deserialize(de)?)
}

fn de_opt_u16_lenient<'de, D>(de: D) -> Result<Option<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    opt_uint_narrow(serde_json::Value::deserialize(de)?)
}

fn de_opt_usize_lenient<'de, D>(de: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    opt_uint_narrow(serde_json::Value::deserialize(de)?)
}

/// Lenient bool: JSON bool, `"true"`/`"false"` (ASCII case-insensitive)
/// string, or null.
fn de_opt_bool_lenient<'de, D>(de: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    match serde_json::Value::deserialize(de)? {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Bool(b) => Ok(Some(b)),
        serde_json::Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            "" => Ok(None),
            other => Err(D::Error::custom(format!(
                "expected a boolean (true/false, as a bool or string); got \"{other}\""
            ))),
        },
        other => Err(D::Error::custom(format!(
            "expected a boolean; got {}",
            json_type_name(&other)
        ))),
    }
}

/// Lenient `rules`: a JSON array of rule objects, or the SAME array
/// JSON-encoded as a string (what a client that lost the array type
/// sends). Null/empty-string mean "no rules".
fn de_opt_rules_lenient<'de, D>(de: D) -> Result<Option<Vec<RuleInput>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    match serde_json::Value::deserialize(de)? {
        serde_json::Value::Null => Ok(None),
        value @ serde_json::Value::Array(_) => {
            serde_json::from_value(value).map(Some).map_err(|e| {
                D::Error::custom(format!(
                    "rules must be an array of rule objects \
                     (minimal: [{{\"pattern\": \"ERROR\"}}]); {e}"
                ))
            })
        }
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(None);
            }
            serde_json::from_str::<Vec<RuleInput>>(t)
                .map(Some)
                .map_err(|e| {
                    D::Error::custom(format!(
                        "rules arrived as a string; it must contain a JSON array of rule \
                     objects (minimal: [{{\"pattern\": \"ERROR\"}}]); parse failed: {e}"
                    ))
                })
        }
        other => Err(D::Error::custom(format!(
            "rules must be an array of rule objects \
             (minimal: [{{\"pattern\": \"ERROR\"}}]); got {}",
            json_type_name(&other)
        ))),
    }
}

/// Lenient scope object: a JSON object, or the SAME object JSON-encoded
/// as a string. Shared by the required and optional scope fields.
fn scope_from_value<E: serde::de::Error>(
    value: serde_json::Value,
) -> Result<McpActivationScope, E> {
    match value {
        value @ serde_json::Value::Object(_) => serde_json::from_value(value).map_err(|e| {
            E::custom(format!(
                "scope must be an object like {{\"kind\":\"global\"}}; {e}"
            ))
        }),
        serde_json::Value::String(s) => serde_json::from_str(s.trim()).map_err(|e| {
            E::custom(format!(
                "scope arrived as a string; it must contain a JSON object like \
                 {{\"kind\":\"global\"}}; parse failed: {e}"
            ))
        }),
        other => Err(E::custom(format!(
            "scope must be an object like {{\"kind\":\"global\"}}; got {}",
            json_type_name(&other)
        ))),
    }
}

fn de_scope_lenient<'de, D>(de: D) -> Result<McpActivationScope, D::Error>
where
    D: serde::Deserializer<'de>,
{
    scope_from_value(serde_json::Value::deserialize(de)?)
}

fn de_opt_scope_lenient<'de, D>(de: D) -> Result<Option<McpActivationScope>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match serde_json::Value::deserialize(de)? {
        serde_json::Value::Null => Ok(None),
        value => scope_from_value(value).map(Some),
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
    /// Human id. Default auto-minted `inline-<n>-<matcher digest>` (stable
    /// for the same matcher, distinct for different matchers).
    #[serde(default)]
    pub id: Option<String>,
    /// Rule version. Default 1.
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
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
            // Alias the names an LLM is MOST likely to guess first onto the
            // closest live level, so the obvious first call succeeds instead
            // of bouncing on taxonomy trivia. The canonical names still win
            // verbatim; only unknown values error (teaching the live set).
            Some("error" | "err") => Severity::High,
            Some("warn" | "warning") => Severity::Medium,
            Some("fatal") => Severity::Critical,
            Some(s) => parse_severity_filter(s).map_err(|_| {
                format!(
                    "severity '{s}' is not valid; one of: trace, debug, info, low, medium, \
                     high, critical (aliases: error->high, warn/warning->medium, \
                     fatal->critical)"
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
            id: self.id.unwrap_or_else(|| {
                // A purely positional `inline-{index}` collides ACROSS calls:
                // every call's first id-less rule minted "inline-0", so two
                // different matchers shared one RuleId and events could not
                // say which inline rule matched. Digest the matcher into the
                // id so it follows content; index keeps within-call order.
                use std::hash::{Hash, Hasher};
                let mut h = std::hash::DefaultHasher::new();
                self.pattern.hash(&mut h);
                self.keywords.hash(&mut h);
                format!("inline-{index}-{:08x}", h.finish() & 0xffff_ffff)
            }),
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
            .into_iter()
            .map(|mut def| {
                // Inline rules are bound directly to this job/watch and must
                // run immediately — exactly the reasoning RuleInput::finalize
                // applies on the typed path. A rules_json definition without
                // an explicit "status" deserializes to the Draft default,
                // and the daemon's draft-poison guard then SILENTLY skips it
                // (the command runs, zero signals fire, the receipt claims
                // "zero rules matched"). Normalize every non-runtime-eligible
                // inline definition to Active so passing a rule always means
                // running it; drafts belong in registry_upsert.
                if !def.status.is_runtime_eligible() {
                    def.status = terminal_commander_core::RuleStatus::Active;
                }
                def
            })
            .collect()
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
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64")]
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
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    #[schemars(with = "Vec<RuleInput>")]
    pub rules: Option<Vec<RuleInput>>,
    /// Deprecated string form of `rules`: a JSON-array string of full
    /// `RuleDefinition`s. Prefer the typed `rules` field. Omit for none.
    #[serde(default)]
    pub rules_json: Option<String>,
    /// Optional per-bucket tag (Phase 3). Tag this probe so a subscription
    /// opened with a matching `tag` predicate routes to it. Omit for none.
    #[serde(default)]
    pub tag: Option<String>,
    /// Strip ANSI/CSI/OSC color + control escapes before rule matching and
    /// in emitted summaries (TC-B1). RAW bytes are always preserved in the
    /// frame store (retrievable via `command_output_tail` / `event_context`);
    /// this affects ONLY what the sifter matches and what summaries echo.
    /// Defaults to `true` so anchored rules (`^ERROR`) and summaries are not
    /// silently defeated by color codes. Set `false` to match raw bytes.
    #[serde(default = "default_true_mcp")]
    pub strip_ansi: bool,
    /// P5 remote federation: optional registered `target_id`. Omitted/None
    /// (the default) runs LOCALLY -- exact backward compatibility. When set,
    /// the command runs on that target's daemon, reached ONLY through the
    /// operator-forwarded LOCAL socket (no public TCP). Requires the
    /// operator to have enabled `allow_remote`; an unknown id is rejected.
    /// Combing + bounded output are identical local vs remote. Adapter-side
    /// routing only -- never forwarded into the IPC request.
    #[serde(default)]
    pub target_id: Option<String>,
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
            // TC-B1: forward the strip flag to the daemon (default true).
            strip_ansi: self.strip_ansi,
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

/// Params for `target_probe` (P5 / T051).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpTargetProbeParams {
    /// Registered `target_id` to probe (see `target_list`). The adapter
    /// dials this target's operator-forwarded LOCAL socket; it never
    /// contacts a network address.
    pub target_id: String,
}

/// MCP-facing parameters for `command_status`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpCommandStatusParams {
    /// Opaque job id returned by `command_start_combed` /
    /// `run_and_watch` (e.g. `job_<32hex>`); copy it verbatim, not
    /// free-form.
    pub job_id: String,
    /// P5: optional `target_id` of the daemon that owns this job. Must
    /// match the target the job was started on. Omit for a local job.
    #[serde(default)]
    pub target_id: Option<String>,
}

/// Parameters for the `command_stop` tool: the opaque job id of the
/// running combed (non-PTY) command to force-kill.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpCommandStopParams {
    /// Opaque job id returned by `command_start_combed` /
    /// `run_and_watch` (e.g. `job_<32hex>`); copy it verbatim, not
    /// free-form.
    pub job_id: String,
    /// P5: optional `target_id` of the daemon that owns this job. Must
    /// match the target the job was started on. Omit for a local job.
    #[serde(default)]
    pub target_id: Option<String>,
}

/// Parameters for `shell_exec` (TC49).
///
/// Carries the dedicated
/// `shell_line` ONLY — there is NO capability flag here: `allow_shell`
/// is config/TOML, not an MCP-flippable parameter. Mirrors the IPC
/// [`ShellExecParams`] field set, with the MCP-layer `env` array shape
/// and `wait_ms` bounded-wait control.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpShellExecParams {
    /// The shell line to run, e.g. `"echo a | wc -c"` or
    /// `"grep -r foo . && echo done"`. Pipelines, compounds, and
    /// redirects are allowed — this is the whole point of the shell
    /// lane. Bounded by the daemon's `MAX_SHELL_LINE_BYTES`.
    pub shell_line: String,
    /// Interpreter override, e.g. `"/bin/sh"`. Omit to use the daemon's
    /// default shell.
    #[serde(default)]
    pub shell: Option<String>,
    /// Optional working directory for the spawned child.
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
    /// Inline rules bound to this job only — same shape as
    /// `command_start_combed`. Minimal: `[{"pattern": "ERROR"}]`. Omit
    /// to collect no rule signals (you still get the exit receipt).
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    #[schemars(with = "Vec<RuleInput>")]
    pub rules: Option<Vec<RuleInput>>,
    /// Deprecated string form of `rules`: a JSON-array string of full
    /// `RuleDefinition`s. Prefer the typed `rules` field. Omit for none.
    #[serde(default)]
    pub rules_json: Option<String>,
    /// Optional per-job bucket override as a JSON object
    /// `{ "max_events": N, "ttl": <seconds> }`. Omit for daemon defaults.
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    /// Optional per-bucket tag (Phase 3). Tag this probe so a
    /// subscription opened with a matching `tag` predicate routes to it.
    /// Omit for none.
    #[serde(default)]
    pub tag: Option<String>,
    /// Reserved bounded-wait control (ms). Accepted for forward parity
    /// with `run_and_watch`; `shell_exec` is start-only and returns
    /// bounded start metadata immediately, so this is currently ignored.
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64")]
    pub wait_ms: Option<u64>,
}

impl McpShellExecParams {
    /// Lower the MCP params into the IPC [`ShellExecParams`]. The
    /// `env` array becomes `(key, value)` pairs and the inline rules are
    /// resolved via the shared `parse_bucket_and_rules` path. There is
    /// no dedup nonce on the shell lane: the daemon's nonce-less
    /// fallback key `(peer, argv, cwd, tag)` already includes
    /// `argv[2] = shell_line`, so two distinct lines never collapse.
    fn into_params(self) -> Result<ShellExecParams, McpError> {
        let cwd = self.cwd.map(std::path::PathBuf::from);
        let env: Vec<(String, String)> = self.env.into_iter().map(|e| (e.key, e.value)).collect();
        let (bucket_config, rules) =
            parse_bucket_and_rules(self.bucket_config_json, self.rules, self.rules_json)?;
        Ok(ShellExecParams {
            shell_line: self.shell_line,
            shell: self.shell,
            cwd,
            env,
            rules,
            bucket_config,
            tag: self.tag,
        })
    }
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
/// The loop runs against a wall-clock deadline (TC-6); each iteration waits up
/// to `min(MAX_WAIT_SLICE_MS, remaining_budget)`, so the advertised `wait_ms`
/// cap is honored to within one slice plus a round-trip. Kept at 1000ms: a
/// smaller slice would double the `bucket_wait` RPC rate under load.
const MAX_WAIT_SLICE_MS: u64 = 1_000;

/// TC-E2 poll-interval hint, in ms, advertised on a still-running
/// `run_and_watch` result so the agent paces `command_status` polling
/// rather than busy-spinning. Matches the wait-slice cadence.
const RUN_AND_WATCH_POLL_HINT_MS: u64 = 1_000;

const RUN_AND_WATCH_WAIT_EXHAUSTED_HINT: &str = "Wait budget exhausted; command is still running. Continue signal collection with command.wait (full surface: bucket_wait) using bucket_id, cursor, and timeout_ms=poll_hint_ms; carry forward next_cursor. Check state/exit_code with command.status (full surface: command_status) using job_id. Do not re-run.";

/// Recovery guidance attached to a degraded `run_and_watch` result (TC-1b).
/// An IPC error interrupted the wait, but the job is still tracked by the
/// daemon, so the agent should confirm liveness before polling -- not re-run.
const RUN_AND_WATCH_RECOVER_HINT: &str = "IPC error interrupted the wait, but the job is still tracked. First confirm daemon liveness with status action=\"health\" (full surface: health). Then continue signal collection with command.wait (full surface: bucket_wait) using bucket_id, cursor, and timeout_ms=poll_hint_ms; carry forward next_cursor. Check state/exit_code with command.status (full surface: command_status) using job_id. Do not re-run.";

/// [`RUN_AND_WATCH_RECOVER_HINT`] with the underlying error appended. A
/// daemon-returned code and a transport drop need different recovery, and a
/// fixed phrase made the degradation cause undiagnosable -- the agent (and any
/// bug report) needs the real code + message.
fn degraded_wait_hint(e: &IpcError) -> String {
    format!(
        "{RUN_AND_WATCH_RECOVER_HINT} (underlying: {:?}: {})",
        e.code, e.message
    )
}

/// Serde default for MCP `bool` params that default to `true`
/// (`strip_ansi` on `command_start_combed` / `run_and_watch`). A bare
/// `#[serde(default)]` would yield `false`, inverting the TC-B1 default.
const fn default_true_mcp() -> bool {
    true
}

/// Bounded wait + presentation controls for `run_and_watch`, split out of
/// [`McpRunAndWatchParams`] so the wait loop reads named fields instead of a
/// widening tuple. All caps are server-side and honored to the wire (TC-E2).
#[derive(Debug, Clone, Copy)]
struct RunAndWatchControls {
    /// Clamped wall-clock wait budget, in ms (TC-6 deadline source).
    wait_ms: u64,
    /// Clamped max signals to return.
    max_signals: usize,
    /// Project each signal to `{summary, stream, seq, severity}` (TC-E1).
    compact: bool,
    /// Wait until the job is terminal, bounded by `wait_ms` (TC-E2).
    wait_until_exit: bool,
}

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
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64")]
    pub grace_ms: Option<u64>,
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    /// Inline rules; see `command_start_combed`. Minimal:
    /// `[{"pattern": "ERROR"}]`. Omit to collect no rule signals (you
    /// still get the exit receipt).
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    #[schemars(with = "Vec<RuleInput>")]
    pub rules: Option<Vec<RuleInput>>,
    /// Deprecated string form of `rules`. Prefer `rules`.
    #[serde(default)]
    pub rules_json: Option<String>,
    /// Max time to wait for signals + exit, in ms. Default 5000, capped
    /// at 60000.
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64")]
    pub wait_ms: Option<u64>,
    /// Max signals to return. Default 50, capped at 500.
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    #[schemars(with = "usize")]
    pub max_signals: Option<usize>,
    /// Optional per-bucket tag (Phase 3). Tag this probe so a subscription
    /// opened with a matching `tag` predicate routes to it, and so the
    /// runtime_state / probe_list rows carry it. Omit for none.
    #[serde(default)]
    pub tag: Option<String>,
    /// Strip ANSI/CSI/OSC escapes before rule matching and in summaries
    /// (TC-B1); see `command_start_combed`. Default `true`. Raw bytes stay
    /// in the frame store.
    #[serde(default = "default_true_mcp")]
    pub strip_ansi: bool,
    /// Compact projection (TC-E1): when `true`, each returned signal carries
    /// ONLY `{summary, stream, seq, severity}` -- the load-bearing fields --
    /// dropping the id plumbing that dominates token cost for the common
    /// case. Default `false` (full signal records). Presentation-only; the
    /// event store is unchanged and the same signals are re-fetchable in full.
    #[serde(default)]
    pub compact: bool,
    /// Honest wait mode (TC-E2). `"exit"` waits for the job to reach a
    /// terminal state, bounded by the SAME server-side `wait_ms` cap (never
    /// exceeded). Any other value (or omitted) keeps the default
    /// signal-cap-or-budget wait. The cap is advertised in the response.
    #[serde(default)]
    pub wait_until: Option<String>,
    /// P5 remote federation: optional registered `target_id`. Omitted/None
    /// runs LOCALLY (default; exact backward compatibility). When set, the
    /// whole one-shot runs on that target's daemon, reached ONLY through the
    /// operator-forwarded LOCAL socket; combed signals come back from THAT
    /// daemon. Requires `allow_remote`; an unknown id is rejected.
    #[serde(default)]
    pub target_id: Option<String>,
}

impl McpRunAndWatchParams {
    /// Split into the start params and the (clamped) wait + presentation
    /// controls. The wait budget and signal cap are clamped to their
    /// server-side maxima here, so the wait loop can never exceed an
    /// advertised cap (TC-E2 / constitution VII).
    fn into_parts(self) -> (McpCommandStartParams, RunAndWatchControls) {
        let wait_ms = self
            .wait_ms
            .unwrap_or(RUN_AND_WATCH_DEFAULT_WAIT_MS)
            .min(RUN_AND_WATCH_MAX_WAIT_MS);
        let max_signals = self
            .max_signals
            .unwrap_or(RUN_AND_WATCH_DEFAULT_MAX_SIGNALS)
            .min(RUN_AND_WATCH_MAX_SIGNALS);
        // TC-E2: only "exit" requests the terminal-state wait; any other
        // value (or omitted) keeps the default behavior. Case-insensitive
        // so `"Exit"` is accepted.
        let wait_until_exit = self
            .wait_until
            .as_deref()
            .is_some_and(|w| w.eq_ignore_ascii_case("exit"));
        let start = McpCommandStartParams {
            argv: self.argv,
            cwd: self.cwd,
            env: self.env,
            grace_ms: self.grace_ms,
            bucket_config_json: self.bucket_config_json,
            rules: self.rules,
            rules_json: self.rules_json,
            tag: self.tag,
            // TC-B1: forward the strip flag onto the underlying start.
            strip_ansi: self.strip_ansi,
            // P5: carry the target_id onto the start params so run_and_watch
            // resolves the same daemon client for the whole one-shot.
            target_id: self.target_id,
        };
        (
            start,
            RunAndWatchControls {
                wait_ms,
                max_signals,
                compact: self.compact,
                wait_until_exit,
            },
        )
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
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
    pub max_lines: Option<u32>,
    /// Maximum bytes to return. Clamped to 64 KiB server-side.
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
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
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    #[schemars(with = "usize")]
    pub limit: Option<usize>,
    /// When true, each returned signal is projected to the load-bearing field
    /// set `{summary, stream, seq, severity}` and the payload echoes
    /// `compact: true`. Presentation only: the full records stay re-fetchable
    /// by re-reading the same cursor with `compact` omitted.
    #[serde(default)]
    pub compact: bool,
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
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    #[schemars(with = "usize")]
    pub limit: Option<usize>,
    /// Wait timeout in milliseconds. Clamped at the daemon.
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64")]
    pub timeout_ms: Option<u64>,
    /// When true, each returned signal is projected to the load-bearing field
    /// set `{summary, stream, seq, severity}` and the payload echoes
    /// `compact: true`. Presentation only: the full records stay re-fetchable
    /// by re-reading the same cursor with `compact` omitted.
    #[serde(default)]
    pub compact: bool,
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
    /// verbatim, not free-form. OPTIONAL (US5 / FR-040): omit it to
    /// resolve the owning bucket from `event_id` alone. When supplied it
    /// must be the event's real bucket -- a contradiction errors
    /// (EventNotFound), it is never silently corrected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // `with = "String"` keeps the advertised schema shape a plain string
    // (matching bucket_id on the `events`/`wait`/`summary` actions) while
    // `serde(default)` leaves it optional -- so the facade flatten stays
    // collision-free and the strict validator does not require it.
    #[schemars(with = "String")]
    pub bucket_id: Option<String>,
    /// Opaque event id from a bucket read (e.g. `evt_<32hex>`); copy it
    /// verbatim, not free-form.
    pub event_id: String,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
    pub before: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
    pub after: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    #[schemars(with = "u32")]
    pub max_bytes: Option<usize>,
}

impl McpEventContextParams {
    fn into_ipc(self) -> Result<EventContextParams, String> {
        // FR-040: bucket_id is optional. Parse it only when supplied; an
        // absent bucket_id resolves the owning bucket from event_id alone.
        let bucket_id = self
            .bucket_id
            .as_deref()
            .map(|b| parse_id::<terminal_commander_core::ids::BucketIdKind>("bucket_id", b))
            .transpose()?;
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
        // TC-B3 (FR-027): true when this terminal result was reconstructed
        // from a persisted receipt after a daemon restart (the in-memory job
        // was gone). The state/exit_code are authoritative-from-disk; the
        // live counters are zero. An honest terminal result, never an error.
        "restarted": s.restarted,
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

fn bucket_events_payload(r: &BucketEventsSinceResponse, compact: bool) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "bucket_id": r.bucket_id,
        "cursor_in": r.cursor_in,
        "next_cursor": r.next_cursor,
        "has_more": r.has_more,
        "dropped_count": r.dropped_count,
        "events": project_events(&r.events, compact),
    });
    // FR-030: echo `compact:true` ONLY when set, so a full read stays
    // byte-identical to today's payload.
    if compact {
        payload["compact"] = serde_json::json!(true);
    }
    payload
}

fn bucket_wait_payload(r: &BucketWaitResponse, compact: bool) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "bucket_id": r.bucket_id,
        "cursor_in": r.cursor_in,
        "next_cursor": r.next_cursor,
        "heartbeat": r.heartbeat,
        "dropped_count": r.dropped_count,
        "events": project_events(&r.events, compact),
    });
    // FR-030: echo `compact:true` ONLY when set, so a full read stays
    // byte-identical to today's payload.
    if compact {
        payload["compact"] = serde_json::json!(true);
    }
    payload
}

/// Project a bucket read's signals for the wire: full records by default, or
/// the load-bearing compact set (FR-030) when `compact` is set. Shared by
/// `bucket_wait` and `bucket_events_since` so both echo the identical shape
/// `run_and_watch` established.
fn project_events(
    events: &[terminal_commander_core::SignalEvent],
    compact: bool,
) -> serde_json::Value {
    if compact {
        serde_json::json!(
            events
                .iter()
                .map(project_signal_compact)
                .collect::<Vec<_>>()
        )
    } else {
        serde_json::json!(events)
    }
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
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    #[schemars(with = "usize")]
    pub limit: Option<usize>,
}

/// MCP-facing parameters for `registry_get`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryGetParams {
    pub rule_id: String,
    /// Omit for the latest stored version.
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
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
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
    pub version: Option<u32>,
    pub samples: Vec<McpRegistryTestSample>,
}

/// MCP-facing parameters for `registry_suggest_from_samples` (US2).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistrySuggestFromSamplesParams {
    /// Raw output sample lines to analyze. Bounded by the daemon
    /// (sample count + per-line bytes); excess is ignored.
    ///
    /// Renamed from `samples` to avoid a shape collision with
    /// `McpRegistryTestParams.samples` (array of objects vs array of
    /// strings) in the flat `registry` facade schema. The
    /// `#[serde(alias = "samples")]` keeps RUNTIME deserialization of a
    /// literal `samples` key working for hand-written callers, but
    /// schemars does NOT advertise the alias: `tools/list` exposes only
    /// `sample_lines`, so schema-introspecting clients see and send that.
    #[serde(alias = "samples")]
    pub sample_lines: Vec<String>,
    /// Optional free-text hint about the tool/intent. Advisory only.
    #[serde(default)]
    pub intent: Option<String>,
    /// Optional cap on the number of proposals. Clamped by the daemon.
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
    pub max_rules: Option<u32>,
}

/// MCP-facing parameters for `registry_activate`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryActivateParams {
    pub rule_id: String,
    /// Omit to activate the latest stored version.
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
    pub version: Option<u32>,
    /// REQUIRED scope (TC42c/TC42d). There is no default and it is in the
    /// schema `required[]`: an omitted scope is rejected so a rule is
    /// never silently widened to global. Use `{ "kind": "global" }` for
    /// the common single-agent case (watch every command you start).
    #[serde(deserialize_with = "de_scope_lenient")]
    pub scope: McpActivationScope,
}

/// MCP-facing parameters for `registry_import_pack`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryImportPackParams {
    /// Pack name. One of the 25 built-in seed packs: generic.terminal,
    /// apt, cargo, npm, pytest, gcc, make, cleanup, docker, kubectl,
    /// git, pip, uv, go, systemd, msbuild, winget, choco, terraform,
    /// ansible, dotnet, bundler, yarn, pnpm, ssh.
    #[schemars(extend("enum" = [
        "generic.terminal", "apt", "cargo", "npm", "pytest", "gcc", "make", "cleanup",
        "docker", "kubectl", "git", "pip", "uv", "go", "systemd", "msbuild", "winget",
        "choco", "terraform", "ansible", "dotnet", "bundler", "yarn", "pnpm", "ssh"
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
    #[serde(default, deserialize_with = "de_opt_scope_lenient")]
    #[schemars(with = "McpActivationScope")]
    pub scope: Option<McpActivationScope>,
}

/// MCP-facing parameters for `registry_deactivate`.
///
/// Three selectors, EXACTLY ONE required (the schema marks all three
/// optional; the adapter's exactly-one-of validator owns required-ness
/// and teaches the whole set in one error -- US2/FR-011):
/// - `rule_id`: a single rule (byte-identical to the historical path).
/// - `rule_ids`: an explicit list of rule ids, one call.
/// - `pack`: every member of a seed pack, one call.
///
/// `scope` stays schema-required. `version` is only valid with
/// `rule_id`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryDeactivateParams {
    /// Selector: a single rule id. Schema-optional; exactly one of
    /// `rule_id` / `rule_ids` / `pack` must be supplied. Advertised as a
    /// plain `string` (like the `rule_id` on other registry actions) so
    /// the facade flatten stays collision-safe; optionality is expressed
    /// by absence from the schema `required[]`.
    #[serde(default)]
    #[schemars(with = "String")]
    pub rule_id: Option<String>,
    /// Selector: deactivate this explicit list of rule ids in one call.
    /// Per-rule outcomes (deactivated / not_active / unknown_rule) are
    /// reported; partial success is never silent.
    #[serde(default)]
    #[schemars(with = "Vec<String>")]
    pub rule_ids: Option<Vec<String>>,
    /// Selector: deactivate every member of this seed pack in one call.
    /// Pack membership resolves from the embedded pack JSON only.
    /// Advertised as a plain `string` (matching `import_pack`'s `pack`).
    #[serde(default)]
    #[schemars(with = "String")]
    pub pack: Option<String>,
    /// Omit to deactivate the LATEST stored version (mirrors registry_get /
    /// registry_activate); the adapter resolves it and the response echoes the
    /// resolved version. Provide a value to target a specific version. An
    /// explicit version is used verbatim -- never silently widened. Only
    /// valid with `rule_id`.
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
    pub version: Option<u32>,
    /// REQUIRED scope (TC42c/TC42d). No default and it is in the schema
    /// `required[]`: an omitted scope is rejected. MUST match the scope
    /// used at activation; deactivating with a different scope will not
    /// close the previously-opened activation row. Use
    /// `{ "kind": "global" }` to close a global activation.
    #[serde(deserialize_with = "de_scope_lenient")]
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
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64", extend("minimum" = 1))]
    pub start_line: Option<u64>,
    /// Max lines returned. Clamped by the daemon.
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
    pub max_lines: Option<u32>,
    /// Max payload bytes returned. Clamped by the daemon.
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    #[schemars(with = "usize")]
    pub max_bytes: Option<usize>,
}

/// MCP-facing parameters for the `list` files action (US3 directory listing).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpFileListDirParams {
    /// Absolute path to the directory to list (e.g. `/home/u/project`).
    /// Absolute is required: the daemon has no workspace root, so a relative
    /// path is rejected rather than resolved against the daemon's working
    /// directory. Gated by the same read-path policy as `read`.
    pub path: String,
    /// Max entries returned. Clamped by the daemon to `[1, 500]`; omitted =
    /// 200. Over-cap listings are truncation-flagged with the true total.
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
    pub max_entries: Option<u32>,
}

/// MCP-facing parameters for `file_write` (TC22 A3).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpFileWriteParams {
    /// Absolute path to the target file (e.g. `/home/u/project/out.txt`).
    /// Absolute is required: the daemon has no workspace root, so a relative
    /// path is rejected rather than resolved against the daemon's working
    /// directory. The write is gated by `paths.write_allow`; a path outside a
    /// configured allow-list is denied.
    pub path: String,
    /// UTF-8 content to write. The daemon bounds this (currently 192 KiB);
    /// oversize content is rejected before any filesystem touch. Write larger
    /// files as multiple bounded calls.
    pub content: String,
    /// Create missing parent directories within an allowed path. The parent
    /// must still pass the write policy gate -- `create_dirs` never widens the
    /// allow-list. Defaults to false.
    #[serde(default, deserialize_with = "de_opt_bool_lenient")]
    #[schemars(with = "bool")]
    pub create_dirs: Option<bool>,
    /// Append `content` to the target instead of replacing it. Same policy
    /// gate, same size cap, same missing-file creation; `bytes_written` is the
    /// number of bytes appended. Defaults to false (full replace).
    #[serde(default, deserialize_with = "de_opt_bool_lenient")]
    #[schemars(with = "bool")]
    pub append: Option<bool>,
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
    #[serde(default, deserialize_with = "de_opt_bool_lenient")]
    #[schemars(with = "bool")]
    pub case_insensitive: Option<bool>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    #[schemars(with = "u32")]
    pub max_matches: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    #[schemars(with = "usize")]
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
    #[serde(default, deserialize_with = "de_opt_bool_lenient")]
    #[schemars(with = "bool")]
    pub follow_from_beginning: Option<bool>,
    /// Optional per-job bucket override as a JSON object
    /// `{ "max_events": N, "ttl": <seconds> }`. Omit for daemon defaults.
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    /// Inline rules bound to this watch only. Minimal example:
    /// `[{"pattern": "ERROR"}]`. See `command_start_combed` for the full
    /// field list. Omit for none.
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    #[schemars(with = "Vec<RuleInput>")]
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
    #[serde(default, deserialize_with = "de_opt_u16_lenient")]
    #[schemars(with = "u16")]
    pub rows: Option<u16>,
    #[serde(default, deserialize_with = "de_opt_u16_lenient")]
    #[schemars(with = "u16")]
    pub cols: Option<u16>,
    /// Optional per-job bucket override as a JSON object
    /// `{ "max_events": N, "ttl": <seconds> }`. Omit for daemon defaults.
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    /// Inline rules bound to this PTY job only. Minimal example:
    /// `[{"pattern": "ERROR"}]`. See `command_start_combed` for the full
    /// field list. Omit for none.
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    #[schemars(with = "Vec<RuleInput>")]
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
    /// Cursor into the PTY job bucket to read the settle window from.
    /// Omit / `0` for the bucket head; pass the prior response's
    /// `next_cursor`. Only meaningful with `wait_ms`.
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64")]
    pub cursor: Option<u64>,
    /// Bounded wait (ms) for combed signals to appear after the write.
    /// Clamped daemon-side. Omit for today's immediate return; supply it
    /// to receive the echo + result signals in the SAME call (US5 /
    /// FR-041, same shape family as `shell_session_exec`).
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64")]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpPtyCommandStopParams {
    /// Opaque job id returned by `pty_command_start` (e.g.
    /// `job_<32hex>`); copy it verbatim, not free-form.
    pub job_id: String,
}

// =====================================================================
// P1 (TC50): persistent shell session + workspace snapshot MCP DTOs.
// =====================================================================

/// `shell_session_start` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpShellSessionStartParams {
    /// Interpreter override (e.g. `/bin/bash`). Omit for the daemon's
    /// default login shell.
    #[serde(default)]
    pub shell: Option<String>,
    /// Initial working directory for the session shell. Omit to inherit
    /// the daemon's cwd.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Environment overlay as an ARRAY of {"key":"NAME","value":"VAL"}
    /// objects (NOT a map). Merged onto the inherited environment; bounded.
    #[serde(default, deserialize_with = "deserialize_env")]
    #[schemars(with = "Vec<EnvEntry>")]
    pub env: Vec<EnvEntry>,
    /// Inline rules bound to the session bucket so the session's combed
    /// output emits structured signals. Minimal example:
    /// `[{"pattern": "ERROR"}]`. Omit for none.
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    #[schemars(with = "Vec<RuleInput>")]
    pub rules: Option<Vec<RuleInput>>,
    /// Deprecated JSON-array string form of `rules`. Prefer `rules`.
    #[serde(default)]
    pub rules_json: Option<String>,
    /// Optional per-job bucket override `{ "max_events": N, "ttl": <s> }`.
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    /// Optional per-bucket tag for subscription routing.
    #[serde(default)]
    pub tag: Option<String>,
}

/// `shell_session_exec` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpShellSessionExecParams {
    /// Opaque session id from `shell_session_start` (e.g. `ses_<32hex>`);
    /// copy it verbatim, not free-form.
    pub session_id: String,
    /// The shell line to run (no trailing newline; the daemon appends it).
    pub line: String,
    /// Cursor into the session bucket to read combed signals from. Omit /
    /// `0` for the bucket head; pass the prior response's `next_cursor`.
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64")]
    pub cursor: Option<u64>,
    /// Bounded wait (ms) for combed signals to appear after the line runs.
    /// Clamped daemon-side. Omit for the default settle window.
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64")]
    pub wait_ms: Option<u64>,
}

/// Shared `{ session_id }` param for `shell_session_status` /
/// `shell_session_stop`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpShellSessionRefParams {
    /// Opaque session id from `shell_session_start` (e.g. `ses_<32hex>`);
    /// copy it verbatim, not free-form.
    pub session_id: String,
}

/// `workspace_snapshot_create` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpWorkspaceSnapshotCreateParams {
    /// Opaque session id whose cwd + bounded env is captured.
    pub session_id: String,
    /// Optional human-friendly label stored with the snapshot.
    #[serde(default)]
    pub name: Option<String>,
}

/// `workspace_snapshot_apply` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpWorkspaceSnapshotApplyParams {
    /// Opaque snapshot id from `workspace_snapshot_create` (e.g.
    /// `snap_<hex>`); copy it verbatim, not free-form.
    pub snapshot_id: String,
    /// The session to restore the snapshot's cwd/env into.
    pub session_id: String,
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
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    #[schemars(with = "usize")]
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
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    #[schemars(with = "usize")]
    pub max: Option<usize>,
    /// Blocking timeout in milliseconds. Omit for 5000; clamped to
    /// [1, 8000]. The MCP client waits longer (12 s) so an idle pull is
    /// SUCCESS empty+liveness, never a timeout error.
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    #[schemars(with = "u64")]
    pub timeout_ms: Option<u64>,
}

/// MCP-facing parameters for `subscription_list`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct McpSubscriptionListParams {
    /// Max rows. Omit for the daemon default/cap (64). Over-cap sets
    /// `truncated`.
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    #[schemars(with = "usize")]
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

    fn shell_start_response() -> CommandStartResponse {
        CommandStartResponse {
            job_id: terminal_commander_core::JobId::new(),
            bucket_id: terminal_commander_core::BucketId::new(),
            probe_id: terminal_commander_core::ProbeId::new(),
            cursor: 0,
            hint: None,
        }
    }

    #[test]
    fn shell_exec_payload_marks_prefiltered_input_without_rejecting_it() {
        let payload = shell_exec_payload(
            &shell_start_response(),
            r#"wsl -e bash -lc "build 2>&1 | tail -30""#,
        );

        assert!(payload.get("job_id").is_some(), "the command still starts");
        assert_eq!(payload["observability"]["status"], "limited");
        assert_eq!(payload["observability"]["executed_unchanged"], true);
        assert_eq!(
            payload["observability"]["detected_prefilters"],
            serde_json::json!(["tail"])
        );
    }

    #[test]
    fn shell_exec_payload_keeps_ordinary_shell_receipt_unchanged() {
        let payload = shell_exec_payload(&shell_start_response(), "echo ready");
        assert!(
            payload.get("observability").is_none(),
            "ordinary shell commands keep the compact legacy receipt"
        );
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
        assert!(
            def.id.starts_with("inline-0-"),
            "auto id is positional + matcher digest, got: {}",
            def.id
        );
        // The whole point: it validates AND is runtime-eligible.
        def.validate().expect("shorthand rule must validate");
        assert!(def.status.is_runtime_eligible());
    }

    #[test]
    fn shorthand_auto_ids_differ_when_matchers_differ() {
        // Regression (dogfood 2026-07-02): two id-less inline rules from two
        // separate calls both minted "inline-0" -> one shared RuleId, so an
        // event could not name which inline rule matched.
        let a = finalize_one(r#"{"pattern": ".+"}"#).unwrap();
        let b = finalize_one(r#"{"pattern": "WATCH_HIT"}"#).unwrap();
        assert_ne!(a.id, b.id, "different matchers must mint different ids");
        // Same matcher stays stable across calls (id follows content).
        let a2 = finalize_one(r#"{"pattern": ".+"}"#).unwrap();
        assert_eq!(a.id, a2.id, "same matcher must mint the same auto id");
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
    fn rules_json_without_status_normalizes_to_active() {
        // S2 regression: a rules_json definition carries serde's Draft
        // default when "status" is omitted, and the daemon's draft-poison
        // guard then SILENTLY skips it (zero signals, receipt claims "zero
        // rules matched"). Inline rules are definitionally active — the
        // parse must normalize them so passing a rule always means running
        // it. Failure mode asserted: the parsed rule must be runtime
        // eligible, not Draft.
        let raw = r#"[{"id":"x","version":1,"kind":"regex","severity":"high",
            "event_kind":"signal","pattern":"XMARKER","summary_template":"${line}"}]"#;
        let (_, rules) = parse_bucket_and_rules(None, None, Some(raw.to_owned())).unwrap();
        assert_eq!(rules.len(), 1);
        assert!(
            rules[0].status.is_runtime_eligible(),
            "inline rules_json rule must be normalized to Active, got {:?}",
            rules[0].status
        );
    }

    // --- TB-12: lenient coercion for clients that lose Option types ---

    #[test]
    fn tb12_numeric_strings_coerce() {
        // Real clients flatten {"type":["integer","null"]} to an untyped
        // param and then send numbers as STRINGS. The exact failure this
        // guards against: `invalid type: string "45000", expected u64`.
        let p: McpRunAndWatchParams = serde_json::from_str(
            r#"{"argv":["node"],"wait_ms":"45000","max_signals":"20","grace_ms":"1000"}"#,
        )
        .expect("stringified numerics must coerce");
        assert_eq!(p.wait_ms, Some(45000));
        assert_eq!(p.max_signals, Some(20));
        assert_eq!(p.grace_ms, Some(1000));

        let t: McpCommandOutputTailParams =
            serde_json::from_str(r#"{"job_id":"job_x","max_lines":"30","max_bytes":"4096"}"#)
                .expect("tail numerics must coerce");
        assert_eq!(t.max_lines, Some(30));
        assert_eq!(t.max_bytes, Some(4096));

        // Native numbers still parse unchanged.
        let n: McpRunAndWatchParams =
            serde_json::from_str(r#"{"argv":["node"],"wait_ms":5000}"#).unwrap();
        assert_eq!(n.wait_ms, Some(5000));
    }

    #[test]
    fn tb12_rules_string_form_coerces() {
        // The exact failure this guards against:
        // `invalid type: string "[...]", expected a sequence`.
        let p: McpRunAndWatchParams =
            serde_json::from_str(r#"{"argv":["node"],"rules":"[{\"pattern\": \"ERROR\"}]"}"#)
                .expect("JSON-encoded rules array must coerce");
        let rules = p.rules.expect("rules present");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern.as_deref(), Some("ERROR"));
    }

    #[test]
    fn tb12_scope_string_form_coerces() {
        // Same client behavior on the scope object param.
        let p: McpRegistryDeactivateParams =
            serde_json::from_str(r#"{"rule_id":"r","version":1,"scope":"{\"kind\": \"global\"}"}"#)
                .expect("JSON-encoded scope object must coerce");
        assert_eq!(p.scope.kind, "global");
    }

    #[test]
    fn tb12_bool_string_form_coerces() {
        let p: McpFileSearchParams =
            serde_json::from_str(r#"{"path":"/f","query":"x","case_insensitive":"true"}"#).unwrap();
        assert_eq!(p.case_insensitive, Some(true));
    }

    #[test]
    fn tb12_garbage_numeric_string_teaches() {
        let err =
            serde_json::from_str::<McpRunAndWatchParams>(r#"{"argv":["node"],"wait_ms":"soon"}"#)
                .unwrap_err()
                .to_string();
        assert!(
            err.contains("unsigned integer"),
            "teaching error must name the accepted forms; got: {err}"
        );
    }

    #[test]
    fn tb12_optional_param_schemas_carry_plain_types() {
        // The schema half of the defense: every coerced optional field pins
        // a PLAIN string "type" so clients that strip union types still see
        // a typed param and send proper JSON in the first place.
        let raw = schemars::schema_for!(McpRunAndWatchParams);
        let v = raw.as_value();
        for (ptr, want) in [
            ("/properties/wait_ms/type", "integer"),
            ("/properties/max_signals/type", "integer"),
            ("/properties/grace_ms/type", "integer"),
            ("/properties/rules/type", "array"),
        ] {
            let got = v.pointer(ptr).and_then(serde_json::Value::as_str);
            assert_eq!(got, Some(want), "pointer {ptr} in run_and_watch schema");
        }
        let tail = schemars::schema_for!(McpCommandOutputTailParams);
        assert_eq!(
            tail.as_value()
                .pointer("/properties/max_lines/type")
                .and_then(serde_json::Value::as_str),
            Some("integer"),
            "command_output_tail.max_lines must be plainly typed"
        );
        let frw = schemars::schema_for!(McpFileReadWindowParams);
        assert_eq!(
            frw.as_value()
                .pointer("/properties/start_line/type")
                .and_then(serde_json::Value::as_str),
            Some("integer"),
            "file_read_window.start_line must be plainly typed"
        );
    }

    #[test]
    fn severity_aliases_map_to_live_levels() {
        // S11: the names an LLM guesses first must not bounce on taxonomy.
        for (alias, want) in [
            ("error", Severity::High),
            ("err", Severity::High),
            ("warn", Severity::Medium),
            ("warning", Severity::Medium),
            ("fatal", Severity::Critical),
        ] {
            let def = finalize_one(&format!(r#"{{"pattern":"x","severity":"{alias}"}}"#)).unwrap();
            assert_eq!(def.severity, want, "alias {alias}");
        }
        let err = finalize_one(r#"{"pattern":"x","severity":"blah"}"#).unwrap_err();
        assert!(
            err.contains("aliases"),
            "unknown severity must teach: {err}"
        );
    }

    // --- TC-1b: run_and_watch degraded / superset result builder ---

    /// Build a `run_and_watch_result` payload and return its parsed JSON. A
    /// success-shaped result (Ok) is itself the TC-1b property: once a job_id
    /// exists, run_and_watch never returns a bare error.
    fn build_run_and_watch_json(
        last_observed_state: Option<terminal_commander_core::JobState>,
        exit_code: Option<i32>,
        degraded: bool,
    ) -> serde_json::Value {
        build_run_and_watch_json_with(last_observed_state, exit_code, degraded, &[], false)
    }

    /// Like [`build_run_and_watch_json`] but lets a test supply the `signals`
    /// slice and the F6 `signals_capped` flag, so the truncation-honesty field
    /// can be exercised through the real builder.
    fn build_run_and_watch_json_with(
        last_observed_state: Option<terminal_commander_core::JobState>,
        exit_code: Option<i32>,
        degraded: bool,
        signals: &[terminal_commander_core::SignalEvent],
        signals_capped: bool,
    ) -> serde_json::Value {
        let recover_hint = degraded.then_some(RUN_AND_WATCH_RECOVER_HINT);
        let result = run_and_watch_result(
            terminal_commander_core::JobId::new(),
            terminal_commander_core::BucketId::new(),
            7,
            last_observed_state,
            exit_code,
            signals,
            None,
            degraded,
            recover_hint,
            false,
            signals_capped,
        )
        .expect("run_and_watch_result must build an Ok (success-shaped) result");
        for item in &result.content {
            if let Some(text) = item.as_text() {
                return serde_json::from_str(&text.text).expect("payload is JSON");
            }
        }
        panic!("run_and_watch_result produced no text content");
    }

    #[test]
    fn run_and_watch_degraded_without_status_is_recoverable_unknown_not_running() {
        // TC-1b: an IPC error before the first status poll must NOT invent a
        // "running" state nor return a bare error -- it returns a success-shaped,
        // job-identified, degraded result the agent can recover from.
        let v = build_run_and_watch_json(None, None, true);
        assert_eq!(v["degraded"], serde_json::json!(true));
        assert_eq!(
            v["state"],
            serde_json::json!("unknown"),
            "degraded state is never a silent 'running'"
        );
        assert_eq!(v["complete"], serde_json::json!(false));
        assert_eq!(v["wait_exhausted"], serde_json::json!(true));
        assert!(
            v["job_id"].as_str().is_some_and(|s| s.starts_with("job_")),
            "the live job_id is preserved"
        );
        assert!(
            v["bucket_id"]
                .as_str()
                .is_some_and(|s| s.starts_with("bkt_"))
        );
        assert_eq!(
            v["cursor"],
            serde_json::json!(7),
            "cursor is preserved and authoritative for resumption"
        );
        assert!(
            v["recover_hint"]
                .as_str()
                .is_some_and(|h| h.contains("command_status") && h.contains("health")),
            "recover_hint tells the agent to confirm health, then poll status"
        );
        assert_eq!(
            v["receipt"],
            serde_json::Value::Null,
            "a degraded result carries no receipt"
        );
    }

    #[test]
    fn run_and_watch_degraded_reports_last_observed_state() {
        // TC-1b honesty: when a state WAS observed before the error, report it.
        let v =
            build_run_and_watch_json(Some(terminal_commander_core::JobState::Running), None, true);
        assert_eq!(v["degraded"], serde_json::json!(true));
        assert_eq!(v["state"], serde_json::json!("running"));
        assert_eq!(v["complete"], serde_json::json!(false));
        assert_eq!(v["wait_exhausted"], serde_json::json!(true));
    }

    #[test]
    fn run_and_watch_normal_terminal_is_complete_and_a_strict_superset() {
        // The normal (non-degraded) payload carries the SAME degraded/recover_hint/
        // cursor keys as the degraded one, so the two paths cannot drift apart.
        let v = build_run_and_watch_json(
            Some(terminal_commander_core::JobState::Exited),
            Some(0),
            false,
        );
        assert_eq!(v["degraded"], serde_json::json!(false));
        assert_eq!(v["recover_hint"], serde_json::Value::Null);
        assert_eq!(v["state"], serde_json::json!("exited"));
        assert_eq!(v["complete"], serde_json::json!(true));
        assert_eq!(v["wait_exhausted"], serde_json::json!(false));
        assert_eq!(v["cursor"], serde_json::json!(7));
        assert_eq!(v["exit_code"], serde_json::json!(0));
    }

    // --- F6: truncation honesty -- the single `signals_capped` bool ---

    /// Mint `n` minimal low-severity rule signals for builder tests. Low
    /// severity sidesteps the pointer invariant; the builder only serializes
    /// the slice and reads its length, so the content is otherwise inert.
    fn capped_test_signals(n: usize) -> Vec<terminal_commander_core::SignalEvent> {
        (0..n)
            .map(|i| {
                let value = serde_json::json!({
                    "event_id": terminal_commander_core::EventId::new(),
                    "bucket_id": terminal_commander_core::BucketId::new(),
                    "seq": i as u64,
                    "timestamp": "2026-06-24T00:00:00Z",
                    "severity": "low",
                    "kind": "test_signal",
                    "summary": "f6 builder fixture signal",
                    "source": {
                        "probe_id": terminal_commander_core::ProbeId::new(),
                        "source_type": "process",
                        "stream": "stdout"
                    }
                });
                serde_json::from_value(value).expect("fixture signal must deserialize")
            })
            .collect()
    }

    #[test]
    fn run_and_watch_result_marks_signals_capped_true_at_cap() {
        // F6: when the returned signals array reached `max_signals`, the result
        // must say so explicitly so a reader cannot conclude "done, N matches"
        // while more matches exist beyond the cursor.
        let max_signals = 3usize;
        let signals = capped_test_signals(max_signals);
        let signals_capped = signals.len() >= max_signals;
        assert!(signals_capped, "fixture must hit the cap");
        let v = build_run_and_watch_json_with(
            Some(terminal_commander_core::JobState::Running),
            None,
            false,
            &signals,
            signals_capped,
        );
        assert_eq!(
            v["signals_capped"],
            serde_json::json!(true),
            "capped signal set must flag signals_capped:true"
        );
        assert_eq!(v["signal_count"], serde_json::json!(max_signals));
    }

    #[test]
    fn run_and_watch_result_marks_signals_capped_false_under_cap() {
        // F6: under the cap the array is complete -- signals_capped must be false.
        let max_signals = 50usize;
        let signals = capped_test_signals(2);
        let signals_capped = signals.len() >= max_signals;
        assert!(!signals_capped, "fixture must be under the cap");
        let v = build_run_and_watch_json_with(
            Some(terminal_commander_core::JobState::Exited),
            Some(0),
            false,
            &signals,
            signals_capped,
        );
        assert_eq!(
            v["signals_capped"],
            serde_json::json!(false),
            "an under-cap signal set must flag signals_capped:false"
        );
        assert_eq!(v["signal_count"], serde_json::json!(2));
    }

    #[test]
    fn run_and_watch_degraded_does_not_claim_capping() {
        // F6: a degraded result passes signals_capped:false -- an interrupted
        // wait did not necessarily cap, and degraded:true already marks it
        // incomplete, so it must not falsely claim truncation.
        let v = build_run_and_watch_json(None, None, true);
        assert_eq!(v["degraded"], serde_json::json!(true));
        assert_eq!(
            v["signals_capped"],
            serde_json::json!(false),
            "a degraded result must not falsely claim it capped signals"
        );
    }

    #[test]
    fn catalogue_lists_fifty_live_tools() {
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
                "command_stop",
                "shell_exec",
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
                "registry_suggest_from_samples",
                "file_read_window",
                "file_search",
                "file_write",
                "file_watch_start",
                "file_watch_stop",
                "file_watch_list",
                "pty_command_start",
                "pty_command_write_stdin",
                "pty_command_stop",
                "pty_command_list",
                "shell_session_start",
                "shell_session_exec",
                "shell_session_status",
                "shell_session_stop",
                "shell_session_list",
                "workspace_snapshot_create",
                "workspace_snapshot_apply",
                "runtime_state",
                "probe_list",
                "probe_status",
                "subscription_open",
                "subscription_pull",
                "subscription_list",
                "subscription_close",
                "subscription_seek",
                "target_list",
                "target_probe",
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
                "command".to_owned(),
                "command_output_tail".to_owned(),
                "command_start_combed".to_owned(),
                "command_status".to_owned(),
                "command_stop".to_owned(),
                "event_context".to_owned(),
                "file_read_window".to_owned(),
                "file_search".to_owned(),
                "file_watch_list".to_owned(),
                "file_watch_start".to_owned(),
                "file_watch_stop".to_owned(),
                "file_write".to_owned(),
                "files".to_owned(),
                "health".to_owned(),
                "policy_status".to_owned(),
                "probe_list".to_owned(),
                "probe_status".to_owned(),
                "pty_command_list".to_owned(),
                "pty_command_start".to_owned(),
                "pty_command_stop".to_owned(),
                "pty_command_write_stdin".to_owned(),
                "registry".to_owned(),
                "registry_activate".to_owned(),
                "registry_deactivate".to_owned(),
                "registry_get".to_owned(),
                "registry_import_pack".to_owned(),
                "registry_list_active".to_owned(),
                "registry_search".to_owned(),
                "registry_suggest_from_samples".to_owned(),
                "registry_test".to_owned(),
                "registry_upsert".to_owned(),
                "run_and_watch".to_owned(),
                "runtime_state".to_owned(),
                "self_check".to_owned(),
                "session".to_owned(),
                "shell_exec".to_owned(),
                "shell_session_exec".to_owned(),
                "shell_session_list".to_owned(),
                "shell_session_start".to_owned(),
                "shell_session_status".to_owned(),
                "shell_session_stop".to_owned(),
                "status".to_owned(),
                "subscription_close".to_owned(),
                "subscription_list".to_owned(),
                "subscription_open".to_owned(),
                "subscription_pull".to_owned(),
                "subscription_seek".to_owned(),
                "system_discover".to_owned(),
                "target_list".to_owned(),
                "target_probe".to_owned(),
                "workspace_snapshot_apply".to_owned(),
                "workspace_snapshot_create".to_owned(),
            ]
        );
    }

    #[test]
    fn facade_consts_match_tool_attribute_descriptions() {
        // Drift guard: the 5 facade description consts in surface_list.rs are
        // hand-copied from the #[tool(description=...)] attributes here. Assert
        // they stay byte-identical so the duplication cannot silently diverge.
        use crate::surface_list::{
            COMMAND_FACADE_DESCRIPTION, FILES_FACADE_DESCRIPTION, REGISTRY_FACADE_DESCRIPTION,
            SESSION_FACADE_DESCRIPTION, STATUS_FACADE_DESCRIPTION,
        };
        let router = TerminalCommanderMcpServer::tool_router();
        let cases: &[(&str, &str)] = &[
            ("command", COMMAND_FACADE_DESCRIPTION),
            ("session", SESSION_FACADE_DESCRIPTION),
            ("files", FILES_FACADE_DESCRIPTION),
            ("registry", REGISTRY_FACADE_DESCRIPTION),
            ("status", STATUS_FACADE_DESCRIPTION),
        ];
        for (name, konst) in cases {
            let tool = router
                .list_all()
                .into_iter()
                .find(|t| t.name.as_ref() == *name)
                .unwrap_or_else(|| panic!("facade '{name}' must be a live router tool"));
            let attr = tool
                .description
                .as_deref()
                .unwrap_or_else(|| panic!("facade '{name}' must carry a #[tool] description"));
            assert_eq!(
                attr, *konst,
                "facade '{name}': #[tool] attribute description and surface_list const have drifted"
            );
        }
    }

    #[test]
    fn command_facade_registers_under_name_command() {
        let router = TerminalCommanderMcpServer::tool_router();
        let names: Vec<String> = router
            .list_all()
            .into_iter()
            .map(|t| t.name.into_owned())
            .collect();
        assert!(
            names.contains(&"command".to_string()),
            "facade must register as 'command' (was the #[tool] name attr dropped?)"
        );
        assert!(
            !names.contains(&"command_facade".to_string()),
            "facade must NOT register under its fn identifier 'command_facade'"
        );
    }

    #[test]
    fn command_facade_action_map_is_total() {
        // Every CommandFacadeCall action parses and is a known variant. If a new
        // action is added without a dispatch arm, the match in `command_facade`
        // fails to compile -- this test guards the *schema* side: every advertised
        // action name round-trips into a variant (no silent drop of an action).
        let actions = [
            "run",
            "run_and_watch",
            "exec",
            "status",
            "output_tail",
            "stop",
            "events",
            "wait",
            "summary",
            "event_context",
            "sub_open",
            "sub_pull",
            "sub_seek",
            "sub_close",
            "sub_list",
        ];
        let schema = serde_json::to_value(schemars::schema_for!(crate::facades::CommandFacadeCall))
            .expect("schema serializes");
        let blob = schema.to_string();
        for a in actions {
            assert!(
                blob.contains(a),
                "action '{a}' missing from CommandFacadeCall schema"
            );
        }
    }

    #[test]
    fn system_discover_tools_explain_daemon_unavailable() {
        let tools = discovered_tools(false, None);
        assert_eq!(tools.len(), tool_catalogue().len());

        for tool in &tools {
            // system_discover and target_list (P5) answer from adapter-side
            // state, so neither requires the local daemon; every other tool
            // does. target_probe DOES require the local daemon (confirms
            // allow_remote before dialing off-host).
            let expected_requires_daemon = !matches!(tool.name, "system_discover" | "target_list");
            assert_eq!(
                tool.requires_daemon, expected_requires_daemon,
                "{} requires_daemon mismatch",
                tool.name
            );

            // Platform truth takes precedence over daemon reachability for a
            // tool that can only ever return UnsupportedPlatform on this host.
            // PTY command tools are blocked only where there is no PTY backend
            // (neither unix nor Windows); session/snapshot tools are blocked on
            // any non-unix host (the session runtime is unix-only).
            let pty_blocked = is_pty_command_tool(tool.name) && !pty_command_available();
            let session_blocked = is_session_tool(tool.name) && !session_runtime_available();

            if !expected_requires_daemon {
                assert!(tool.available, "{} should remain callable", tool.name);
                assert_eq!(tool.unavailable_reason, None);
            } else if pty_blocked {
                assert!(
                    !tool.available,
                    "{} should be unavailable without a PTY runtime",
                    tool.name
                );
                assert_eq!(tool.unavailable_reason, Some(PTY_UNAVAILABLE_REASON));
            } else if session_blocked {
                assert!(
                    !tool.available,
                    "{} should be unavailable without a session runtime",
                    tool.name
                );
                assert_eq!(tool.unavailable_reason, Some(SESSION_UNAVAILABLE_REASON));
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

    /// BUG 1: with the daemon UP but `allow_shell` OFF in the active profile,
    /// the catalogue MUST report `shell_exec` unavailable with the cap-truthful
    /// reason -- never `available:true` for a call policy can only PolicyDeny.
    /// Granting the cap flips it back to available with no reason. The same gate
    /// covers `target_probe` / `allow_remote`. (shell_exec + target_probe are
    /// platform-independent, so this asserts identically on unix and Windows.)
    #[test]
    fn catalogue_marks_policy_gated_tools_unavailable_when_cap_off() {
        fn entry(tools: &[DiscoveredToolEntry], name: &str) -> DiscoveredToolEntry {
            tools
                .iter()
                .find(|t| t.name == name)
                .cloned()
                .unwrap_or_else(|| panic!("{name} missing from discovered tools"))
        }

        // Daemon up, every cap OFF (the DeveloperLocal deny-by-default posture).
        let denied = discovered_tools(true, Some(PolicyCapsView::default()));
        let shell = entry(&denied, "shell_exec");
        assert!(
            !shell.available,
            "shell_exec must be unavailable when allow_shell is off"
        );
        assert_eq!(shell.unavailable_reason, Some(SHELL_CAP_DENIED_REASON));
        let probe = entry(&denied, "target_probe");
        assert!(
            !probe.available,
            "target_probe must be unavailable when allow_remote is off"
        );
        assert_eq!(probe.unavailable_reason, Some(REMOTE_CAP_DENIED_REASON));

        // Grant allow_shell + allow_remote: both become available, no reason.
        let granted = discovered_tools(
            true,
            Some(PolicyCapsView {
                allow_shell: true,
                allow_remote: true,
                ..PolicyCapsView::default()
            }),
        );
        let shell = entry(&granted, "shell_exec");
        assert!(
            shell.available,
            "shell_exec must be available when allow_shell is on"
        );
        assert_eq!(shell.unavailable_reason, None);
        let probe = entry(&granted, "target_probe");
        assert!(
            probe.available,
            "target_probe must be available when allow_remote is on"
        );
        assert_eq!(probe.unavailable_reason, None);
    }

    /// TB-1 regression: on a host without a PTY runtime (non-unix; ConPTY
    /// pending) the four `pty_*` tools MUST be reported `available: false`
    /// with the platform reason -- even when the daemon is reachable. They
    /// must never advertise a live PTY surface the daemon can only reject
    /// with `UnsupportedPlatform`.
    #[cfg(not(unix))]
    #[test]
    fn pty_tools_unavailable_on_unsupported_platform() {
        // daemon_available = true on purpose: a platform block must fire
        // regardless of daemon reachability.
        let tools = discovered_tools(true, None);

        // The four PTY command tools are platform-available wherever a PTY
        // backend exists: unix (pty-process) AND Windows (ConPTY / US3a-TC53).
        // On any other platform they are blocked with the PTY reason.
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
            if pty_command_available() {
                assert!(
                    tool.available,
                    "{name} must be available where a PTY backend exists (daemon up)"
                );
                assert_eq!(tool.unavailable_reason, None, "{name} must have no reason");
            } else {
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

        // The session / snapshot tools remain unix-only; on a non-unix host
        // they must surface the session platform reason.
        let session_names = [
            "shell_session_start",
            "shell_session_exec",
            "shell_session_status",
            "shell_session_stop",
            "shell_session_list",
            "workspace_snapshot_create",
            "workspace_snapshot_apply",
        ];
        for name in session_names {
            let tool = tools
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name} missing from discovered tools"));
            if session_runtime_available() {
                assert!(
                    tool.available,
                    "{name} must be available on a unix host with the daemon up"
                );
                assert_eq!(tool.unavailable_reason, None, "{name} must have no reason");
            } else {
                assert!(
                    !tool.available,
                    "{name} must be unavailable without a session runtime"
                );
                assert_eq!(
                    tool.unavailable_reason,
                    Some(SESSION_UNAVAILABLE_REASON),
                    "{name} must surface the session platform reason"
                );
            }
        }
    }

    /// TB-1 companion: on a unix host with the daemon reachable the PTY
    /// tools ARE available (the platform block does not fire). This keeps
    /// the fix narrow -- it never suppresses a real PTY surface.
    #[cfg(unix)]
    #[test]
    fn pty_tools_available_on_supported_platform_with_daemon() {
        let tools = discovered_tools(true, None);
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
        // US2/FR-011: scope stays schema-required.
        assert!(
            required.iter().any(|f| f == "scope"),
            "registry_deactivate schema must list scope in required[]; got {required:?}"
        );
        // US2/FR-011: rule_id is now schema-OPTIONAL -- it is one of three
        // selectors {rule_id, rule_ids, pack} and the adapter's
        // exactly-one-of validator owns required-ness. NONE of the three
        // selectors is in required[].
        for selector in ["rule_id", "rule_ids", "pack"] {
            assert!(
                !required.iter().any(|f| f == selector),
                "selector {selector} must be schema-optional (exactly-one-of validator owns \
                 required-ness); got {required:?}"
            );
        }
        // version is OPTIONAL (omit = latest stored) and only valid with rule_id.
        assert!(
            !required.iter().any(|f| f == "version"),
            "version must be optional on deactivate (omit = latest); got {required:?}"
        );
    }

    #[test]
    fn deactivate_omitted_version_deserializes_as_none() {
        // BUG 2 regression: an omitted `version` must DESERIALIZE (to None,
        // resolved to the latest stored version by the adapter) instead of
        // failing with "missing field `version`". scope stays required.
        let params = serde_json::from_str::<McpRegistryDeactivateParams>(
            r#"{"rule_id":"r","scope":{"kind":"global"}}"#,
        )
        .expect("deactivate without version must deserialize (version is optional)");
        assert_eq!(params.rule_id.as_deref(), Some("r"));
        assert_eq!(params.version, None, "omitted version must parse as None");
        assert_eq!(params.scope.kind, "global");

        // An explicit version still parses and is preserved verbatim.
        let explicit = serde_json::from_str::<McpRegistryDeactivateParams>(
            r#"{"rule_id":"r","version":3,"scope":{"kind":"global"}}"#,
        )
        .expect("deactivate with explicit version must deserialize");
        assert_eq!(explicit.version, Some(3));
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

    // --- DEFECT B: version-skew gate at the tool edge ---

    /// Build a server whose daemon status is AVAILABLE (so reachability passes)
    /// but carries the given version-skew verdict. The socket is never bound, so
    /// any tool that gets past the gate and issues IPC sees a transport error
    /// (the existing `daemon_unavailable` path).
    fn available_status_with_skew(skew: Option<(String, String)>) -> TerminalCommanderMcpServer {
        let sock = std::env::temp_dir().join(format!(
            "tc-mcp-skew-unit-test-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let status = EnsureDaemonStatus::AlreadyRunning {
            endpoint: terminal_commander_supervisor::ensure::Endpoint::UnixSocket {
                path: sock.clone(),
            },
            pid: Some(1),
        };
        let daemon = McpDaemonClient::with_status(
            sock,
            crate::daemon_client::DaemonStatusHandle::with_skew(status, skew),
        )
        .with_timeout(std::time::Duration::from_millis(10));
        TerminalCommanderMcpServer::new(daemon)
    }

    #[tokio::test]
    async fn version_skew_blocks_tools_but_exempts_health_and_discover() {
        // A daemon-backed tool fails with an HONEST daemon_version_skew naming
        // BOTH versions -- never a misleading daemon_unavailable.
        let server = available_status_with_skew(Some(("0.1.47".to_owned(), "0.1.69".to_owned())));
        let policy = server
            .policy_status()
            .await
            .expect_err("policy_status must fail on version skew");
        let rendered = policy.to_string();
        assert!(
            rendered.contains("daemon_version_skew"),
            "skew must surface daemon_version_skew, got: {rendered}"
        );
        assert!(
            rendered.contains("0.1.47") && rendered.contains("0.1.69"),
            "skew error must name BOTH versions, got: {rendered}"
        );
        assert!(
            !rendered.contains("daemon_unavailable"),
            "skew must NOT masquerade as daemon_unavailable, got: {rendered}"
        );

        // Exemption: health is not blocked by the skew gate. It still fails
        // (no live daemon) but as daemon_unavailable, so an operator can probe.
        let health = server
            .health()
            .await
            .expect_err("health fails: no live daemon");
        let health_rendered = health.to_string();
        assert!(
            !health_rendered.contains("daemon_version_skew"),
            "health must be exempt from the skew gate, got: {health_rendered}"
        );

        // Exemption: system_discover returns its catalogue (daemon_error in the
        // payload), never a skew gate error.
        server
            .system_discover()
            .await
            .expect("system_discover must not be blocked by the skew gate");

        // No-skew available handle: the gate injects NO skew error; a down
        // daemon still yields the existing daemon_unavailable (no regression).
        let ok_server = available_status_with_skew(None);
        let down = ok_server
            .policy_status()
            .await
            .expect_err("no live daemon -> daemon_unavailable");
        let r = down.to_string();
        assert!(
            !r.contains("daemon_version_skew"),
            "a no-skew handle must never raise daemon_version_skew, got: {r}"
        );
        assert!(
            r.contains("daemon_unavailable"),
            "a down no-skew daemon must remain daemon_unavailable, got: {r}"
        );
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
    fn run_and_watch_wait_exhaustion_teaches_both_resume_identities() {
        let v = build_run_and_watch_json(
            Some(terminal_commander_core::JobState::Running),
            None,
            false,
        );
        assert_eq!(v["complete"], serde_json::json!(false));
        assert_eq!(v["wait_exhausted"], serde_json::json!(true));
        assert_eq!(
            v["recover_hint"],
            serde_json::json!(
                "Wait budget exhausted; command is still running. Continue signal collection with command.wait (full surface: bucket_wait) using bucket_id, cursor, and timeout_ms=poll_hint_ms; carry forward next_cursor. Check state/exit_code with command.status (full surface: command_status) using job_id. Do not re-run."
            )
        );
        assert!(
            v["job_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("job_"))
        );
        assert!(
            v["bucket_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("bkt_"))
        );
        assert_eq!(v["cursor"], serde_json::json!(7));
        assert_eq!(
            v["poll_hint_ms"],
            serde_json::json!(RUN_AND_WATCH_POLL_HINT_MS)
        );
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
            IpcErrorCode::ProgramNotFound,
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
            // F14: an unsupported platform is caller-ROUTABLE (route to WSL /
            // a different tool), not a server fault -- it must stay
            // invalid_params so the agent reasons instead of abandoning TC.
            IpcErrorCode::UnsupportedPlatform,
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

    /// F7: a missing program surfaces a STRUCTURED failure receipt, not the
    /// opaque `internal_error` (-32603) the generic `Spawn` arm produced.
    /// The MCP error must be `invalid_params` (-32602) and carry a data
    /// payload with the failure vocabulary the agent reasons over:
    /// `error_kind = "program_not_found"`, the offending `argv0`, and an
    /// explicit null `exit_code` (the process never started). This pins the
    /// exact F7 client contract.
    #[test]
    fn program_not_found_surfaces_structured_failure_receipt_not_internal() {
        // The daemon carries `argv0` as a TYPED field via
        // `IpcError::program_not_found`. Use a DELIBERATELY reworded message
        // (note the apostrophe in "daemon's" -- which the old prose
        // quote-count parse REQUIRED be absent) to prove `argv0` recovery is
        // driven by the typed field, not by message wording.
        let e = IpcError::program_not_found(
            "tc_nonexistent_program_f7_xyz",
            "could not find the program 'tc_nonexistent_program_f7_xyz' on the daemon's PATH.",
        );
        let mcp = into_mcp_error(&e);

        // (1) Caller-fixable, NOT a server fault. This is the core F7 fix:
        // the old behavior was internal_error (-32603).
        assert_eq!(
            mcp.code.0, -32602,
            "a missing program is a caller-fixable command attempt, must be invalid_params"
        );

        // (2) Structured data payload mirrors the failed-receipt vocabulary.
        let data = mcp
            .data
            .expect("program_not_found carries a structured data payload");
        assert_eq!(
            data["ipc_code"].as_str(),
            Some("ProgramNotFound"),
            "the discriminator code must be present; got: {data}"
        );
        assert_eq!(
            data["error_kind"].as_str(),
            Some("program_not_found"),
            "error_kind must name the failure class; got: {data}"
        );
        assert_eq!(
            data["argv0"].as_str(),
            Some("tc_nonexistent_program_f7_xyz"),
            "the offending argv0 must be a discrete field; got: {data}"
        );
        assert!(
            data["exit_code"].is_null(),
            "exit_code must be null (the process never started); got: {data}"
        );
    }

    /// F7 (typed carrier): an `argv0` that itself contains an apostrophe -- the
    /// exact case the OLD prose quote-count parse could not recover (it would
    /// have truncated to `my` or omitted the field) -- now rides verbatim on
    /// the TYPED `IpcError::argv0` field. The receipt must carry the full
    /// `my'prog`, proving recovery no longer depends on message wording.
    #[test]
    fn program_not_found_typed_argv0_with_apostrophe_survives_to_data() {
        // `argv0` = `my'prog`; the message wording is intentionally arbitrary
        // (it is no longer the source of truth).
        let e = IpcError::program_not_found(
            "my'prog",
            "the program could not be located on PATH (it does not exist).",
        );
        let mcp = into_mcp_error(&e);

        assert_eq!(mcp.code.0, -32602, "still a caller-fixable command attempt");

        let data = mcp
            .data
            .expect("program_not_found carries a structured data payload");
        assert_eq!(
            data["error_kind"].as_str(),
            Some("program_not_found"),
            "error_kind must name the failure class; got: {data}"
        );
        assert!(
            data["exit_code"].is_null(),
            "exit_code must be null (the process never started); got: {data}"
        );
        // The apostrophe-bearing argv0 must now survive VERBATIM via the typed
        // field -- not be truncated or omitted as the prose parse required.
        assert_eq!(
            data["argv0"].as_str(),
            Some("my'prog"),
            "the typed argv0 must carry the apostrophe-bearing name verbatim; got: {data}"
        );
    }

    /// F7 (graceful degradation): a `ProgramNotFound` error WITHOUT a typed
    /// `argv0` (e.g. built via the generic `IpcError::new`, or decoded from a
    /// pre-F7 client) must still surface the rest of the structured receipt
    /// (`-32602`, `error_kind`, null `exit_code`) and simply OMIT the `argv0`
    /// data key -- never `null`, never a fabricated value. The message still
    /// names the program, so the agent is not left blind.
    #[test]
    fn program_not_found_omits_argv0_data_field_when_typed_field_absent() {
        let e = IpcError::new(
            IpcErrorCode::ProgramNotFound,
            "program not found: 'somewhere'. Remedy: check the spelling of argv[0].",
        );
        assert!(e.argv0.is_none(), "precondition: no typed argv0");
        let mcp = into_mcp_error(&e);

        assert_eq!(
            mcp.code.0, -32602,
            "still a caller-fixable command attempt regardless of argv0 presence"
        );

        let data = mcp
            .data
            .expect("program_not_found carries a structured data payload");
        assert_eq!(
            data["error_kind"].as_str(),
            Some("program_not_found"),
            "error_kind must be present even when argv0 is omitted; got: {data}"
        );
        assert!(
            data["exit_code"].is_null(),
            "exit_code must be null even when argv0 is omitted; got: {data}"
        );
        // No typed argv0 -> the data key is OMITTED entirely (not null, not
        // recovered from the prose).
        assert!(
            data.get("argv0").is_none(),
            "argv0 data key must be omitted when the typed field is absent; got: {data}"
        );
    }

    /// F14: an unsupported-platform error (the unix-only session/snapshot tools
    /// on a non-unix host) surfaces a STRUCTURED, caller-ROUTABLE receipt --
    /// `invalid_params` (-32602), not the `internal_error` (-32603) that reads
    /// as "TC is broken" and trains the agent to abandon TC for raw shell. The
    /// data payload carries the vocabulary the agent routes on: `error_kind`,
    /// the HONEST host `platform`, and the unavailable `tool` (typed carrier).
    /// This MAPPING test runs on every platform (it does not need the daemon's
    /// `#[cfg(not(unix))]` stub compiled), so it is NOT cfg-gated.
    #[test]
    fn unsupported_platform_surfaces_structured_routable_receipt_not_internal() {
        let e = IpcError::unsupported_platform(
            "shell_session_start",
            "persistent shell sessions are not available on this platform (unix-only)",
        );
        let mcp = into_mcp_error(&e);

        // (1) Caller-ROUTABLE, NOT a server fault. This is the core F14 fix:
        // the old behavior was internal_error (-32603).
        assert_eq!(
            mcp.code.0, -32602,
            "an unsupported platform is caller-routable, must be invalid_params"
        );

        // (2) Structured data payload names the failure class, the host, and
        // the unavailable tool the agent must route around.
        let data = mcp
            .data
            .expect("unsupported_platform carries a structured data payload");
        assert_eq!(
            data["error_kind"].as_str(),
            Some("unsupported_platform"),
            "error_kind must name the failure class; got: {data}"
        );
        assert_eq!(
            data["tool"].as_str(),
            Some("shell_session_start"),
            "the unavailable tool must be a discrete typed field; got: {data}"
        );
        // `platform` is the HONEST host truth (`std::env::consts::OS`), whatever
        // CI host runs this -- assert present + non-empty, never hard-pinned.
        let platform = data["platform"]
            .as_str()
            .expect("platform must be a non-empty host string");
        assert!(
            !platform.is_empty(),
            "platform must be a non-empty host string; got: {data}"
        );
    }

    /// F14 (graceful degradation): an `UnsupportedPlatform` error WITHOUT a
    /// typed `tool` (e.g. built via the generic `IpcError::new`) must still
    /// surface the rest of the structured receipt (`-32602`, `error_kind`,
    /// `platform`) and simply OMIT the `tool` data key -- never `null`, never a
    /// fabricated value. Mirrors the F7 `argv0` omission contract.
    #[test]
    fn unsupported_platform_omits_tool_data_field_when_typed_field_absent() {
        let e = IpcError::new(IpcErrorCode::UnsupportedPlatform, "x");
        assert!(e.tool.is_none(), "precondition: no typed tool");
        let mcp = into_mcp_error(&e);

        assert_eq!(
            mcp.code.0, -32602,
            "still caller-routable regardless of tool presence"
        );

        let data = mcp
            .data
            .expect("unsupported_platform carries a structured data payload");
        assert_eq!(
            data["error_kind"].as_str(),
            Some("unsupported_platform"),
            "error_kind must be present even when tool is omitted; got: {data}"
        );
        // No typed tool -> the data key is OMITTED entirely (not null, not
        // fabricated).
        assert!(
            data.get("tool").is_none(),
            "tool data key must be omitted when the typed field is absent; got: {data}"
        );
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
        // The transport detail is CONFINED to the structured
        // `details.transport_detail` (diagnosability -- P1.0f) while the
        // top-level message stays clean and the raw ipc_code stays absent.
        assert!(
            !rendered.contains("ipc_code"),
            "transport failure must not surface a raw ipc_code, got: {rendered}"
        );
        let data = into_mcp_error(&transport)
            .data
            .expect("transport envelope carries a data payload");
        assert_eq!(
            data["message"].as_str(),
            Some("terminal-commanderd became unreachable mid-call"),
            "top-level envelope message stays clean; got: {data}"
        );
        assert!(
            data["details"]["transport_detail"]
                .as_str()
                .is_some_and(|d| d.contains("pipe connect")),
            "the underlying transport failure must ride in details.transport_detail \
             (P1.0f: an opaque envelope made the true cause undiagnosable); got: {data}"
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

        // The raw detail rides ONLY in details.transport_detail; the
        // envelope's own message stays clean.
        assert!(
            data["details"]["transport_detail"]
                .as_str()
                .is_some_and(|d| d.contains("pipe connect")),
            "the mutating envelope must carry the underlying cause in \
             details.transport_detail; got: {data}"
        );
        assert_eq!(
            data["message"].as_str(),
            Some("terminal-commanderd became unreachable mid-call"),
            "top-level envelope message stays clean; got: {data}"
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
    fn tb9_import_pack_schema_enumerates_all_seed_packs() {
        // TB-9: the schema's pack enum must match the daemon's seed-pack
        // set exactly (25 packs incl. cleanup + the US2 additions), so
        // the schema can never again list fewer packs than the daemon
        // accepts.
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
                "docker",
                "kubectl",
                "git",
                "pip",
                "uv",
                "go",
                "systemd",
                "msbuild",
                "winget",
                "choco",
                "terraform",
                "ansible",
                "dotnet",
                "bundler",
                "yarn",
                "pnpm",
                "ssh",
            ],
            "import_pack schema must enumerate all 25 seed packs; got {packs:?}"
        );
        assert_eq!(packs.len(), 25);
        assert!(
            packs.iter().any(|p| p == "cleanup"),
            "TB-9: cleanup must be present in the pack enum"
        );
        for p0 in ["docker", "kubectl", "git"] {
            assert!(
                packs.iter().any(|p| p == p0),
                "TB-9: P0 pack {p0} must be present in the enum (FR-010)"
            );
        }
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
    fn run_and_watch_into_parts_threads_tag_through() {
        // Regression guard for the silent `tag: None` drop (TC-4 Phase 4b):
        // `run_and_watch` must thread its `tag` into the start params so the
        // probe's bucket source carries it.
        let params = McpRunAndWatchParams {
            argv: vec!["sleep".to_string(), "1".to_string()],
            cwd: None,
            env: Vec::new(),
            grace_ms: None,
            bucket_config_json: None,
            rules: None,
            rules_json: None,
            wait_ms: None,
            max_signals: None,
            tag: Some("X".to_string()),
            strip_ansi: true,
            compact: false,
            wait_until: None,
            target_id: None,
        };
        let (start, _controls) = params.into_parts();
        assert_eq!(start.tag, Some("X".to_string()));

        // A `None` tag must stay `None` (no fabricated tag).
        let untagged = McpRunAndWatchParams {
            argv: vec!["sleep".to_string(), "1".to_string()],
            cwd: None,
            env: Vec::new(),
            grace_ms: None,
            bucket_config_json: None,
            rules: None,
            rules_json: None,
            wait_ms: None,
            max_signals: None,
            tag: None,
            strip_ansi: true,
            compact: false,
            wait_until: None,
            target_id: None,
        };
        let (start, _controls) = untagged.into_parts();
        assert_eq!(start.tag, None);
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
