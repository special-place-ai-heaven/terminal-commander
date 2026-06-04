// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! IPC wire protocol (TC37).
//!
//! Length-prefixed JSON frames. Every frame begins with a 4-byte
//! big-endian `u32` payload length, followed by exactly that many
//! bytes of UTF-8 JSON. Frames larger than [`MAX_FRAME_BYTES`] are
//! rejected before the payload is decoded.
//!
//! Method set at TC37 is deliberately tiny:
//! - `system_discover` — version + capabilities + tool list.
//! - `health` — daemon liveness ping.
//! - `policy_status` — active profile + the daemon-side caps.
//! - `self_check` — re-run the bounded TC36 self-check report.
//!
//! No `command_*`, no `bucket_*`, no `event_context`, no
//! `file_read_*` — those land in TC38 (process wiring), TC39
//! (bucket/event daemon API), and TC41 (MCP tool surface). TC37
//! deliberately ships a minimal, safe method set so the transport
//! lock-in does not race ahead of policy / wiring goals.
//!
//! Source-status: live (TC37).

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use terminal_commander_core::{
    ActivationScope, BucketConfig, BucketId, EventId, JobId, RuleDefinition, RuleStatus, Severity,
    SignalEvent, SourceStream,
};

/// Bounded response shape. Carries identifiers and counters, never
/// raw stdout/stderr.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStartResponse {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: terminal_commander_core::ProbeId,
    /// Initial bucket cursor: clients pass this to `bucket_events_since`.
    pub cursor: u64,
}

/// No-silence exit receipt (TCE-ERG-1).
///
/// Present ONLY when a finished process command produced ZERO
/// rule-driven events. This is the one sanctioned exception to "TC
/// never returns raw output": a bounded, truthful tail so a zero-rule
/// command does not read as breakage.
///
/// PTY/file-watch jobs never reach this path (`CommandService` holds
/// process probes only; `handle_command_status` routes PTY job ids to
/// `UnknownJob`), so a tail cannot include secret-prompt input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub exit_code: Option<i32>,
    /// Frames the command produced that no rule matched, i.e. lines
    /// the agent would otherwise have scrolled. `frames_total` for a
    /// zero-rule run.
    pub lines_suppressed: u64,
    /// Last N frame texts (oldest first), byte-capped.
    pub tail: Vec<String>,
    /// True when the ring evicted earlier frames; the tail may omit
    /// the start of output.
    pub tail_incomplete: bool,
}

/// Bounded status shape. Counters + final exit state only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStatusResponse {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: terminal_commander_core::ProbeId,
    pub state: terminal_commander_core::JobState,
    pub frames_total: u64,
    pub frames_stdout: u64,
    pub frames_stderr: u64,
    pub bytes_total: u64,
    pub events_emitted: u64,
    #[serde(default)]
    pub frames_suppressed: u64,
    #[serde(default)]
    pub frames_suppressed_progress: u64,
    #[serde(default)]
    pub frames_suppressed_dedupe: u64,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub duration_ms: Option<u64>,
    /// No-silence receipt; `Some` only for a finished process command
    /// with zero rule-driven events. See [`CommandReceipt`].
    pub receipt: Option<CommandReceipt>,
}

/// Hard cap on a complete frame (length prefix + payload). Anything
/// above this is rejected without payload parse.
///
/// 256 KiB is large enough for the JSON shapes ahead (e.g. a full
/// `bucket_events_since` response capped at 10000 events ~ 25 bytes
/// per event minimum overhead) and small enough that one malicious
/// client cannot exhaust daemon memory by streaming a giant frame.
/// Bucket / event-context responses still carry their own per-call
/// caps; this is the transport-layer envelope cap.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;

/// Soft cap on the request side.
///
/// Applied before the request is dispatched. Currently identical to
/// [`MAX_FRAME_BYTES`]; kept as a named constant so future tools can
/// raise / lower it independently from response sizing.
pub const MAX_REQUEST_BYTES: usize = MAX_FRAME_BYTES;

/// Soft cap on the response side. Matches the frame cap today.
pub const MAX_RESPONSE_BYTES: usize = MAX_FRAME_BYTES;

/// Wire correlation id.
///
/// The client picks; the server echoes. Used to distinguish responses
/// on a multiplexed connection. Today the connection is request /
/// response one-at-a-time; the field future-proofs the protocol.
pub type CorrelationId = u64;

/// Top-level request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub correlation_id: CorrelationId,
    pub request: IpcRequest,
}

/// Top-level response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub correlation_id: CorrelationId,
    pub result: IpcResult,
}

/// Maximum wait timeout the daemon will accept for `bucket_wait`.
/// Requests above this are clamped at the dispatcher.
pub const MAX_BUCKET_WAIT_MS: u64 = 30_000;
/// Default wait timeout when the client omits one.
pub const DEFAULT_BUCKET_WAIT_MS: u64 = 5_000;
/// Hard cap on events returned by `bucket_events_since` /
/// `bucket_wait`. Mirrors the codebase `MAX_READ_LIMIT`.
pub const MAX_BUCKET_READ_LIMIT: usize = 10_000;
/// Default events-per-call when the client omits a limit.
pub const DEFAULT_BUCKET_READ_LIMIT: usize = 200;
/// Hard cap on context-window frames returned by `event_context`.
pub const MAX_CONTEXT_FRAMES: u32 = 1024;
/// Default `before` count when the client omits one.
pub const DEFAULT_CONTEXT_BEFORE: u32 = 5;
/// Default `after` count when the client omits one.
pub const DEFAULT_CONTEXT_AFTER: u32 = 5;
/// Hard cap on event_context payload bytes. Mirrors the per-ring
/// `max_bytes` cap; the dispatcher clamps oversize values.
pub const MAX_CONTEXT_BYTES: usize = 64 * 1024;

/// Maximum number of subscriptions open at once in the in-memory registry.
///
/// Opening beyond this returns
/// [`IpcErrorCode::SubscriptionLimitExceeded`]; the caller frees a
/// slot via `subscription_close` and retries. (Subscriptions design,
/// Phase 1, Task 7.)
pub const MAX_SUBSCRIPTIONS: usize = 64;
/// Hard cap on in-scope buckets a single subscription scans per pull.
///
/// The `list_bucket_ids()` ∩ side-table scan is bounded by this; over-cap
/// is flagged `truncated`. (Subscriptions design §1 "Routing scan is
/// bounded".)
pub const MAX_BUCKETS_PER_SUBSCRIPTION: usize = 200;

/// Hard cap on events returned by one `subscription_pull`. The caller's
/// `max` is clamped to this; the combined events+liveness response stays
/// under [`MAX_FRAME_BYTES`]. (Subscriptions design §3 step 8.)
pub const MAX_PULL_EVENTS: usize = 50;
/// Default `subscription_pull` timeout when the caller omits `timeout_ms`.
pub const DEFAULT_PULL_TIMEOUT_MS: u64 = 5_000;
/// Hard cap on a `subscription_pull` timeout.
///
/// Strictly below the unix `DRAIN_CEILING` (10 s) so a blocked pull returns
/// its normal empty+liveness at its own timeout before a graceful drain
/// would abort it. (Subscriptions design §3 "Timeout reconciliation".)
pub const MAX_PULL_TIMEOUT_MS: u64 = 8_000;

/// Method-typed request union.
///
/// Method names are namespaced `<domain>_<verb>` to match the MCP tool
/// names; the rmcp adapter maps each tool 1:1 to a method. All 33
/// methods are live: the 32-method TC45 set carried via IPC plus the
/// P4 `audit_since` read surface. `audit_since` is the one CLI-only
/// read method with no rmcp tool, so the MCP tool catalogue stays at
/// 32 live tools (see `docs/mcp/TOOL_CONTROL_SURFACE.md` §2) while the
/// IPC method set carries the extra audit-log reader.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum IpcRequest {
    /// Get daemon version, MCP spec revision, callable method list.
    SystemDiscover,
    /// Liveness ping. Returns the daemon uptime in seconds.
    Health,
    /// Report active policy profile + the configured per-call caps.
    PolicyStatus,
    /// Re-run the TC36 self-check; returns the report as text.
    SelfCheck,
    /// Cursor-based bucket read. Bounded by `MAX_BUCKET_READ_LIMIT`.
    BucketEventsSince(BucketEventsSinceParams),
    /// Realtime wait. Returns heartbeat on timeout, never raw text.
    BucketWait(BucketWaitParams),
    /// Bounded summary (counters + severity histogram).
    BucketSummary(BucketSummaryParams),
    /// Bounded context window around the event's source pointer.
    /// Resolved by `(bucket_id, event_id)`; the daemon walks the
    /// bucket to find the matching event, then resolves
    /// `(probe_id, pointer.frame_id)` against the context ring.
    EventContext(EventContextParams),
    /// Start a non-PTY argv command. Bounded metadata response only;
    /// never returns raw stdout/stderr. Shell-bridge guard applies.
    CommandStartCombed(CommandStartParams),
    /// Lifecycle + counters lookup for a previously started command.
    CommandStatus(CommandStatusParams),
    /// Rule-free bounded read of a job's captured output tail (F1).
    CommandOutputTail(CommandOutputTailParams),
    /// FTS-backed search over persisted rule definitions.
    RegistrySearch(RegistrySearchParams),
    /// Fetch a specific rule definition by id and optional version.
    RegistryGet(RegistryGetParams),
    /// Insert a new (rule_id, version+1) row from a validated
    /// definition. Existing versions are immutable.
    RegistryUpsert(RegistryUpsertParams),
    /// Evaluate a rule against bounded sample texts and return the
    /// emitted draft shapes. Read-only; never persisted.
    RegistryTest(RegistryTestParams),
    /// Mark `(rule_id, version)` as active in the in-memory
    /// activation registry AND record the persistent activation row.
    RegistryActivate(RegistryActivateParams),
    /// Import an embedded rule pack by name; optionally promote its
    /// rules to Active and activate them in one call.
    RegistryImportPack(RegistryImportPackParams),
    /// Remove `(rule_id, version)` from the active set and close
    /// the persistent activation row.
    RegistryDeactivate(RegistryDeactivateParams),
    /// Snapshot of every currently-active `(rule_id, version)`. Bounded
    /// by [`MAX_LIST_LIMIT`] / the request `limit`.
    RegistryListActive(ListLimitParams),
    /// Bounded line/byte window read of a regular file. Never
    /// returns the whole file; the daemon clamps the window.
    FileReadWindow(FileReadWindowParams),
    /// Bounded substring/regex search over one file. Returns
    /// structured match pointers + short snippets only.
    FileSearch(FileSearchParams),
    /// Start a daemon-owned file probe that emits structured signal
    /// events into a bucket as the file is appended to. Never
    /// streams raw file content.
    FileWatchStart(FileWatchStartParams),
    /// Stop a previously-started file watch by `watch_id`.
    FileWatchStop(FileWatchStopParams),
    /// Snapshot of every currently-live file watch.
    FileWatchList,
    /// Start an interactive PTY argv command. Bounded metadata
    /// response only; never returns raw screen buffer.
    PtyCommandStart(PtyCommandStartParams),
    /// Write bounded stdin bytes to a running PTY job. Returns
    /// `SecretInputDenied` while a secret prompt is active.
    PtyCommandWriteStdin(PtyCommandWriteStdinParams),
    /// Stop a previously started PTY job by `job_id`.
    PtyCommandStop(PtyCommandStopParams),
    /// Snapshot of every currently-live PTY job.
    PtyCommandList,
    /// TC45: bounded aggregate snapshot across every daemon runtime
    /// (command, file watch, PTY) plus active rule scopes and
    /// bucket counters. Read-only. Each of its three vecs is bounded
    /// INDEPENDENTLY by [`MAX_LIST_LIMIT`] / the request `limit`.
    RuntimeState(ListLimitParams),
    /// TC45: flat list of every live probe across all runtimes.
    /// Read-only. Bounded by [`MAX_LIST_LIMIT`] / the request `limit`.
    ProbeList(ListLimitParams),
    /// TC45: bounded lookup for one probe by id. Returns
    /// `UnknownProbe` if no runtime knows the id.
    ProbeStatus(ProbeStatusParams),
    /// Cursor-based read of the persistent audit log. Read-only;
    /// bounded by [`MAX_AUDIT_READ_LIMIT`]. Read failure surfaces
    /// [`IpcErrorCode::Internal`] (the closed error set is not widened
    /// for this method).
    AuditSince(AuditSinceParams),
    /// Open a predicate-routed subscription. Mints a fresh opaque
    /// `sub_id` with its own independent offsets (consumer isolation).
    /// Initial offsets for already-in-scope buckets are their current
    /// tail (from-now for a late open). (Subscriptions §4.)
    SubscriptionOpen(SubscriptionOpenParams),
    /// Multiplexed, lossless pull over an open subscription. Returns
    /// bounded, source-tagged events + per-source liveness. Idle returns
    /// SUCCESS empty+liveness, never an error; unknown/expired `sub_id`
    /// returns [`IpcErrorCode::UnknownSubscription`]. Blocks up to the
    /// (clamped) timeout. (Subscriptions §3.)
    SubscriptionPull(SubscriptionPullParams),
    /// Bounded snapshot of every open subscription. (Subscriptions §6.)
    SubscriptionList(SubscriptionListParams),
    /// Close a subscription, freeing its registry slot. (Subscriptions §4.)
    SubscriptionClose(SubscriptionCloseParams),
    /// Reposition one bucket's offset for a subscription (explicit re-read).
    /// The requested seq is clamped to the bucket's live range. (Subscriptions
    /// §3 seek.)
    SubscriptionSeek(SubscriptionSeekParams),
    /// Request a graceful shutdown. The daemon ACKs immediately
    /// (`ShutdownAck`), stops accepting new connections, drains in-flight
    /// requests, removes its pidfile, and exits 0. New connections during the
    /// drain receive `ShuttingDown` (retryable).
    Shutdown,
}

/// Success / error union. Success carries a typed payload per method;
/// error carries a structured code + message.
///
/// Boxing the `Ok` payload would change neither the JSON wire form
/// nor the public Rust API (serde renders `Box<T>` as `T`), so the
/// large-variant lint is suppressed in favor of keeping the
/// pattern-matched shape that every dispatcher and test uses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
#[allow(clippy::large_enum_variant)]
pub enum IpcResult {
    Ok { response: IpcResponse },
    Err { error: IpcError },
}

/// Method-typed success union. Each variant matches one
/// [`IpcRequest`] variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "method")]
pub enum IpcResponse {
    SystemDiscover(DiscoverResponse),
    Health {
        uptime_secs: u64,
        /// Seconds since the last real IPC request. Optional for
        /// backward compat: a legacy daemon omits it; clients treat
        /// absence as unknown.
        #[serde(default)]
        idle_secs: Option<u64>,
    },
    PolicyStatus(PolicyStatusResponse),
    SelfCheck(SelfCheckResponse),
    BucketEventsSince(BucketEventsSinceResponse),
    BucketWait(BucketWaitResponse),
    BucketSummary(BucketSummaryResponse),
    EventContext(EventContextResponse),
    CommandStartCombed(CommandStartResponse),
    CommandStatus(CommandStatusResponse),
    CommandOutputTail(CommandOutputTailResponse),
    RegistrySearch(RegistrySearchResponse),
    RegistryGet(RegistryGetResponse),
    RegistryUpsert(RegistryUpsertResponse),
    RegistryTest(RegistryTestResponse),
    RegistryActivate(RegistryActivateResponse),
    RegistryImportPack(RegistryImportPackResponse),
    RegistryDeactivate(RegistryDeactivateResponse),
    RegistryListActive(RegistryListActiveResponse),
    FileReadWindow(FileReadWindowResponse),
    FileSearch(FileSearchResponse),
    FileWatchStart(FileWatchStartResponse),
    FileWatchStop(FileWatchStopResponse),
    FileWatchList(FileWatchListResponse),
    PtyCommandStart(PtyCommandStartResponse),
    PtyCommandWriteStdin(PtyCommandWriteStdinResponse),
    PtyCommandStop(PtyCommandStopResponse),
    PtyCommandList(PtyCommandListResponse),
    RuntimeState(RuntimeStateResponse),
    ProbeList(ProbeListResponse),
    ProbeStatus(ProbeStatusResponse),
    AuditSince(AuditSinceResponse),
    SubscriptionOpen(SubscriptionOpenResponse),
    SubscriptionPull(SubscriptionPullResponse),
    SubscriptionList(SubscriptionListResponse),
    SubscriptionClose(SubscriptionCloseResponse),
    SubscriptionSeek(SubscriptionSeekResponse),
    /// Ack for `Shutdown`. `draining=true` once the daemon has stopped accepting
    /// new connections and begun draining.
    ShutdownAck {
        draining: bool,
    },
}

/// `system_discover` payload. Mirrors the contract laid out in
/// `docs/mcp/TOOL_CONTROL_SURFACE.md`. The advertised method list is
/// tied to the dispatcher's actual handler set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverResponse {
    pub version: String,
    pub mcp_spec: String,
    pub policy_profile: String,
    pub methods: Vec<String>,
}

/// `policy_status` payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyStatusResponse {
    pub profile: String,
    pub commands_deny_count: usize,
    pub default_deny_path_suffix_count: usize,
    /// Per-call file_read_window cap (from `LimitsSection`, clamped
    /// at config load to the codebase hard cap).
    pub file_window_bytes: usize,
    /// Per-call bucket-read cap.
    pub bucket_read_limit: usize,
}

/// `self_check` payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfCheckResponse {
    pub report: String,
    pub failures: u32,
}

/// Structured error code. Closed set. Adding a variant requires a
/// goal-file amendment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorCode {
    /// Frame exceeded [`MAX_FRAME_BYTES`].
    FrameTooLarge,
    /// Payload was not valid UTF-8 JSON.
    MalformedJson,
    /// Payload decoded but didn't match the wire schema.
    SchemaMismatch,
    /// Method not recognized.
    UnknownMethod,
    /// Policy engine denied the request.
    PolicyDenied,
    /// Daemon-internal error while handling the request.
    Internal,
    /// Peer credential check failed; connection refused.
    PeerCredentialFailure,
    /// Platform does not support UDS (Windows native).
    UnsupportedPlatform,
    /// The requested bucket does not exist.
    BucketNotFound,
    /// The requested event id was not found in the bucket.
    EventNotFound,
    /// The cursor is invalid (e.g. far above the current tail).
    InvalidCursor,
    /// `argv[0]` basename matches the shell-bridge deny list.
    /// `command_start_combed` is not a shell entry point.
    ShellInterpreterDenied,
    /// argv shape is invalid (empty, too long, or item too large).
    ArgvInvalid,
    /// `command_status` was called with a job id the daemon does not
    /// know.
    UnknownJob,
    /// `registry_get` / `registry_test` / `registry_activate` /
    /// `registry_deactivate` referenced a `(rule_id, version?)` the
    /// daemon does not know.
    RuleNotFound,
    /// `registry_upsert` or `registry_test` payload failed rule
    /// validation (empty id, bad regex, kind/keywords mismatch,
    /// etc.).
    RuleInvalid,
    /// `registry_activate` / `registry_deactivate` was issued with
    /// a scope value the daemon cannot resolve to a live entity
    /// (unknown bucket / job / probe id) or with a malformed scope
    /// payload. The activation is NOT silently widened to Global.
    ScopeInvalid,
    /// `file_*` request referenced a path the policy engine rejected
    /// (default-deny suffix or future per-profile path policy).
    PathDenied,
    /// `file_*` request referenced a path that does not exist on
    /// disk OR is not a regular file (directories rejected here so
    /// TC43 does not balloon into directory probe expansion).
    FileNotFound,
    /// `file_read_window` / `file_search` detected non-UTF-8 bytes
    /// in the requested window. Binary content is rejected with a
    /// typed code instead of streaming bytes to the LLM.
    FileBinary,
    /// Request exceeds a bounded cap (line count, byte count, glob
    /// breadth, search result count). The dispatcher clamps where
    /// safe; payloads that cannot be clamped surface this code.
    OversizedRequest,
    /// `file_watch_stop` referenced a watch id the daemon does not
    /// know.
    UnknownWatch,
    /// `pty_command_write_stdin` was issued while the target PTY job
    /// has an active secret prompt. The LLM input MUST NOT be
    /// written. TC44 contract: no automatic password entry, no
    /// LLM-supplied password forwarding.
    SecretInputDenied,
    /// `probe_status` referenced a probe id the daemon does not
    /// know across any of its runtimes.
    UnknownProbe,
    /// `registry_activate` referenced a rule whose status is not
    /// runtime-eligible (Draft / Deprecated / Tombstoned). Activating
    /// a non-Active rule would silently bind a definition the sifter
    /// runtime then rejects at command-start time with
    /// `SifterError::NotActive`, blocking every newly-started command
    /// in scope. The activation is refused up front with the remedy
    /// in the message (promote the rule to status=Active and re-upsert)
    /// rather than poisoning the scope. See the agent-ergonomics chain.
    RuleNotActive,
    /// `subscription_pull`/`subscription_close` referenced a `sub_id` the
    /// daemon does not know (unknown or reset by a daemon restart). Caller
    /// re-opens. Approved goal-file amendment 2026-06-02.
    UnknownSubscription,
    /// `subscription_open` exceeded the max-subscriptions cap. Caller frees
    /// a slot (subscription_close) and retries. Approved 2026-06-02.
    SubscriptionLimitExceeded,
    /// Returned to a new request that arrives while the daemon is draining for
    /// shutdown. Retryable: the client should cold-spawn a fresh daemon.
    ShuttingDown,
}

/// Structured error payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
}

impl IpcError {
    /// Marker lead-in for CLIENT-SIDE transport failures (connect, write,
    /// read, request timeout, correlation mismatch). These never carry a
    /// daemon-authored payload -- they fail before, or instead of, a decoded
    /// response -- so the marker lets a caller distinguish "could not reach
    /// the daemon" (recoverable: re-ensure + retry, then surface a clean
    /// `daemon_unavailable` envelope) from a daemon-RETURNED [`IpcErrorCode`]
    /// (a real, caller-actionable fault). The marker is a human-readable
    /// prefix on an otherwise-`Internal` error so the rendered message stays
    /// meaningful while [`IpcError::is_transport`] can detect it without
    /// fragile substring scanning of OS error text. The daemon NEVER
    /// constructs transport errors, so its `Internal` errors are never
    /// misclassified.
    pub const TRANSPORT_PREFIX: &'static str = "transport: ";

    /// Constructor.
    #[must_use]
    pub fn new(code: IpcErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Construct a client-side TRANSPORT failure: an [`IpcErrorCode::Internal`]
    /// error tagged with [`Self::TRANSPORT_PREFIX`] so [`Self::is_transport`]
    /// recognizes it. Use this only for failures to REACH or COMMUNICATE with
    /// the daemon (connect / write / read / timeout / correlation mismatch),
    /// never for a daemon-returned error.
    #[must_use]
    pub fn transport(message: impl AsRef<str>) -> Self {
        Self {
            code: IpcErrorCode::Internal,
            message: format!("{}{}", Self::TRANSPORT_PREFIX, message.as_ref()),
        }
    }

    /// True when this is a client-side transport failure (see
    /// [`Self::transport`]). Distinguishes "could not reach the daemon" from a
    /// daemon-returned error so the MCP adapter can self-heal + retry and then
    /// surface a clean `daemon_unavailable` envelope instead of a raw
    /// `internal_error` (-32603) that trains agents to abandon the tool.
    #[must_use]
    pub fn is_transport(&self) -> bool {
        self.code == IpcErrorCode::Internal && self.message.starts_with(Self::TRANSPORT_PREFIX)
    }
}

/// Parameters for `bucket_events_since`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketEventsSinceParams {
    pub bucket_id: BucketId,
    pub cursor: u64,
    /// Optional minimum severity. Omitted = `trace` (no filter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_min: Option<terminal_commander_core::Severity>,
    /// Optional exact-match kind filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_filter: Option<String>,
    /// Result count cap. Clamped to `MAX_BUCKET_READ_LIMIT` at the
    /// dispatcher. Omitted = `DEFAULT_BUCKET_READ_LIMIT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Response shape for `bucket_events_since` / `bucket_wait`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketEventsSinceResponse {
    pub bucket_id: BucketId,
    pub cursor_in: u64,
    pub next_cursor: u64,
    pub has_more: bool,
    pub dropped_count: u64,
    pub events: Vec<SignalEvent>,
}

/// Parameters for `bucket_wait`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketWaitParams {
    pub bucket_id: BucketId,
    pub cursor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_min: Option<terminal_commander_core::Severity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Maximum wait in milliseconds. Clamped to `MAX_BUCKET_WAIT_MS`.
    /// Omitted = `DEFAULT_BUCKET_WAIT_MS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl BucketWaitParams {
    /// Resolve the effective `Duration`, clamping to the hard cap.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        let raw = self.timeout_ms.unwrap_or(DEFAULT_BUCKET_WAIT_MS);
        Duration::from_millis(raw.min(MAX_BUCKET_WAIT_MS))
    }
}

/// Response shape for `bucket_wait`. Identical to
/// `BucketEventsSinceResponse` plus a `heartbeat` flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketWaitResponse {
    pub bucket_id: BucketId,
    pub cursor_in: u64,
    pub next_cursor: u64,
    /// `true` when the wait timed out and no matching events
    /// arrived. The `events` array MUST be empty in that case.
    pub heartbeat: bool,
    pub dropped_count: u64,
    pub events: Vec<SignalEvent>,
}

/// Parameters for `bucket_summary`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketSummaryParams {
    pub bucket_id: BucketId,
}

/// Response shape for `bucket_summary`. Counters only; never raw
/// stream content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketSummaryResponse {
    pub bucket_id: BucketId,
    pub head_seq: u64,
    pub tail_seq: u64,
    pub event_count: u64,
    pub dropped_count: u64,
    /// Per-severity histogram (trace / debug / info / low / medium /
    /// high / critical), in wire-stable order.
    pub by_severity: SeverityHistogram,
}

/// Wire-stable severity histogram. Independent of the in-memory
/// `BucketSummary` so the protocol locks the field order.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct SeverityHistogram {
    pub trace: u64,
    pub debug: u64,
    pub info: u64,
    pub low: u64,
    pub medium: u64,
    pub high: u64,
    pub critical: u64,
}

/// Parameters for `event_context`. Resolves the event's source
/// pointer and returns bounded context around that frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventContextParams {
    pub bucket_id: BucketId,
    pub event_id: EventId,
    /// Frames to include BEFORE the anchor. Clamped to
    /// `MAX_CONTEXT_FRAMES`. Omitted = `DEFAULT_CONTEXT_BEFORE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<u32>,
    /// Frames to include AFTER the anchor. Clamped to
    /// `MAX_CONTEXT_FRAMES`. Omitted = `DEFAULT_CONTEXT_AFTER`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<u32>,
    /// Hard byte cap on the response. Clamped to
    /// `MAX_CONTEXT_BYTES`. Omitted = `MAX_CONTEXT_BYTES`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

/// One context frame on the wire. Same shape as `core::ContextLine`
/// but kept inside the IPC module so the protocol owns its serde
/// surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcContextFrame {
    pub probe_id: terminal_commander_core::ProbeId,
    pub frame_id: terminal_commander_core::FrameId,
    pub stream: terminal_commander_core::SourceStream,
    pub line: Option<u64>,
    pub text: String,
}

/// Reasons a context window may be empty or partial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextUnavailableReason {
    /// Severity below Medium: event carries no pointer by design.
    NoPointer,
    /// Event carried a `pointer_unavailable_reason` instead of a
    /// pointer (synthetic lifecycle events).
    SyntheticEvent,
    /// Anchor frame already evicted from the ring.
    AnchorEvicted,
    /// Probe id not found in the context-ring manager.
    UnknownProbe,
}

/// Response shape for `event_context`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventContextResponse {
    pub bucket_id: BucketId,
    pub event_id: EventId,
    /// `true` when the anchor frame was no longer in the ring at
    /// resolution time.
    pub anchor_missing: bool,
    /// Set when the daemon could not produce a window for a
    /// non-error reason (severity below threshold, synthetic event,
    /// etc.). When set, `frames` is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<ContextUnavailableReason>,
    /// Echo of the event's `pointer_unavailable_reason` if the event
    /// carried one. Never raw stream content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer_unavailable_reason: Option<String>,
    /// Bounded window. Empty when `anchor_missing` or
    /// `unavailable_reason` is set.
    pub frames: Vec<IpcContextFrame>,
    /// Reported by the underlying ring; helps clients reason about
    /// truncation.
    pub total_bytes: usize,
    pub truncated: bool,
}

/// Maximum number of explicit env entries on `command_start_combed`.
/// Symmetric with `MAX_ARGV_ITEMS`; protects the wire path from
/// accidental fan-out via env.
pub const MAX_COMMAND_ENV_ITEMS: usize = 256;
/// Maximum number of inline rules accepted on `command_start_combed`.
/// Hot rule binding is TC42 territory; TC41 only accepts the empty
/// default unless the operator passes a small per-call list.
pub const MAX_COMMAND_INLINE_RULES: usize = 64;
/// Maximum grace window before forced terminate. Clamped at the
/// dispatcher.
pub const MAX_COMMAND_GRACE_MS: u64 = 60_000;

/// Wire shape for `command_start_combed`. Mirrors the daemon's
/// `CommandStartRequest` but uses millis instead of `Duration` so the
/// JSON form stays human-readable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStartParams {
    /// Target environment (default local parent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<terminal_commander_core::EnvironmentSpec>,
    /// argv. `argv[0]` is the program; rest are passed verbatim.
    /// Shell-string passthrough is forbidden; `argv[0]` matching the
    /// shell-bridge deny list is rejected before the policy gate.
    pub argv: Vec<String>,
    /// Working directory. Optional; resolves against the daemon's
    /// own cwd when None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Explicit environment for the child. Empty means inherit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<(String, String)>,
    /// Bucket configuration override (max_events / TTL). Defaults
    /// applied if None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_config: Option<BucketConfig>,
    /// Optional inline rule set. Empty means use the daemon's empty
    /// sifter (no events emitted). Hot rule binding lives in TC42.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RuleDefinition>,
    /// Grace window between graceful and forced terminate, in
    /// milliseconds. Clamped to `MAX_COMMAND_GRACE_MS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace_ms: Option<u64>,
    /// Optional per-bucket tag for subscription routing (Phase 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

impl CommandStartParams {
    /// Resolve the effective grace `Duration`, clamping to the cap.
    #[must_use]
    pub fn grace(&self) -> Option<Duration> {
        self.grace_ms
            .map(|ms| Duration::from_millis(ms.min(MAX_COMMAND_GRACE_MS)))
    }
}

/// Wire shape for `command_status`. Carries just the job id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStatusParams {
    pub job_id: JobId,
}

/// Wire shape for `command_output_tail` (F1). Rule-free bounded read
/// of a job's captured output. Caps enforced server-side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutputTailParams {
    pub job_id: JobId,
    #[serde(default = "default_tail_lines")]
    pub max_lines: u32,
    #[serde(default = "default_tail_bytes")]
    pub max_bytes: u32,
}

const fn default_tail_lines() -> u32 {
    50
}
const fn default_tail_bytes() -> u32 {
    65_536
}

/// Response for `command_output_tail`.
///
/// Bounded; never returns the full raw stream. `truncated_lines` is
/// true when the ring held more frames than `max_lines` (after
/// server-side clamping). `truncated_bytes` is true when the byte cap
/// was hit before the line cap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutputTailResponse {
    pub job_id: JobId,
    pub lines: Vec<String>,
    pub returned_lines: u32,
    pub truncated_lines: bool,
    pub truncated_bytes: bool,
    pub evicted_frames: u64,
}

/// Maximum number of lines returned by `command_output_tail`.
pub const MAX_TAIL_LINES: usize = 200;
/// Maximum bytes returned by `command_output_tail`.
pub const MAX_TAIL_BYTES: usize = 65_536;

/// Maximum hits returned by `registry_search` in a single call.
pub const MAX_REGISTRY_SEARCH_LIMIT: usize = 200;
/// Default hits returned when the caller omits a limit.
pub const DEFAULT_REGISTRY_SEARCH_LIMIT: usize = 50;
/// Maximum number of samples accepted by `registry_test`.
pub const MAX_REGISTRY_TEST_SAMPLES: usize = 32;
/// Maximum size of a single sample text. Mirrors the sifter's
/// per-frame cap; bytes above this are truncated before evaluation.
pub const MAX_REGISTRY_TEST_SAMPLE_BYTES: usize = 8192;

/// `registry_search` parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySearchParams {
    /// FTS5 query string. Operator-supplied; the daemon performs no
    /// rewriting beyond what SQLite's FTS5 layer enforces.
    pub query: String,
    /// Result cap. Clamped to `MAX_REGISTRY_SEARCH_LIMIT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// One hit returned by `registry_search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySearchHit {
    pub rule_id: String,
    pub version: u32,
    pub event_kind: String,
    pub summary_template: String,
    pub tags: Vec<String>,
    pub severity: Severity,
    pub status: RuleStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySearchResponse {
    pub hits: Vec<RegistrySearchHit>,
}

/// `registry_get` parameters. If `version` is `None`, the daemon
/// returns the latest stored version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryGetParams {
    pub rule_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryGetResponse {
    pub definition: RuleDefinition,
}

/// `registry_upsert` parameters. The daemon validates the definition,
/// assigns the next version, and persists an immutable row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryUpsertParams {
    pub definition: RuleDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryUpsertResponse {
    pub rule_id: String,
    pub version: u32,
}

/// A single bounded sample for `registry_test`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryTestSample {
    /// Sample text. Bytes above `MAX_REGISTRY_TEST_SAMPLE_BYTES` are
    /// truncated by the daemon before evaluation; the dropped byte
    /// count surfaces in the response.
    pub text: String,
    /// Stream tag used to drive `rule.stream` filtering. Defaults to
    /// `stdout` so an operator does not have to set it for simple
    /// keyword tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<SourceStream>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryTestParams {
    pub rule_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    pub samples: Vec<RegistryTestSample>,
}

/// One match produced by `registry_test`. Bounded by design:
/// captures are projected to a flat `BTreeMap<String, String>` so
/// the response never carries arbitrary deeply-nested JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryTestMatch {
    pub sample_index: usize,
    pub severity: Severity,
    pub kind: String,
    pub summary: String,
    pub captures: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryTestResponse {
    pub matches: Vec<RegistryTestMatch>,
    /// Bytes dropped by per-sample truncation. Helps the operator
    /// reason about why a tail-anchored regex did not fire.
    pub truncated_bytes: u32,
}

/// `registry_activate` parameters.
///
/// The optional `scope` field (TC42c) selects which live stream(s) the
/// activation reaches. Omitted scope deserializes to
/// [`ActivationScope::Global`], preserving TC42/TC42b wire compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryActivateParams {
    pub rule_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ActivationScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryActivateResponse {
    pub rule_id: String,
    pub version: u32,
    /// `true` when the rule was already active under this scope
    /// before this call. The activation row is still persisted in
    /// either case so the audit trail records the operator intent.
    pub was_already_active: bool,
    /// Echo of the scope that was applied. Always populated so
    /// pre-TC42c clients can ignore the field and post-TC42c clients
    /// can verify their request.
    pub scope: ActivationScope,
    /// Number of live jobs whose sifter was rebound by this call.
    /// Zero is valid (e.g. no commands running, or no live job
    /// matched the scope).
    pub jobs_rebound: u32,
}

/// Import a named, embedded rule pack into the registry.
///
/// When `activate` is true, `scope` is REQUIRED and every imported
/// rule is promoted to Active and activated in that scope -- one call
/// for "give me expert signals for X". When false, rules import at
/// their on-disk status (the vetting path) and nothing is activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryImportPackParams {
    pub pack: String,
    #[serde(default)]
    pub activate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ActivationScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryImportPackResponse {
    pub pack: String,
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    /// Rules that imported AND activated successfully. Only the rules
    /// whose activation completed appear here; a rule listed in
    /// `imported` but missing from both `activated` and `failed` means
    /// `activate` was false (nothing was activated).
    pub activated: Vec<String>,
    /// Partial-success channel (M7): rules that imported and were
    /// promoted to Active but whose *activation* failed mid-loop. Each
    /// entry carries the rule id + a human-readable reason. This is an
    /// ADDITIVE field: it serializes only when non-empty, so the
    /// all-success wire shape is unchanged and older clients that do
    /// not know the field still deserialize. A non-empty `failed` is
    /// still a SUCCESSFUL response (no IPC error code) -- the caller
    /// inspects `failed` to learn which rules need a retry rather than
    /// receiving a bare error that hides the rules that did activate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<RegistryImportFailure>,
}

/// One rule whose activation failed during a partial-success import.
///
/// The rule WAS imported (and promoted to Active in the store) by
/// `registry_import_pack`; only the in-memory/durable activation step
/// failed. `reason` is the typed IPC error message surfaced to the
/// caller so it can decide whether to retry that single rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryImportFailure {
    pub rule_id: String,
    pub reason: String,
}

/// `registry_deactivate` parameters. Scope follows the same default
/// rule as `registry_activate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryDeactivateParams {
    pub rule_id: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ActivationScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryDeactivateResponse {
    pub rule_id: String,
    pub version: u32,
    /// `false` when the rule was not in the in-memory active set
    /// for this scope (e.g. operator deactivated something already
    /// inactive). The daemon still attempts to close the persistent
    /// row.
    pub was_deactivated: bool,
    /// Echo of the scope that was applied.
    pub scope: ActivationScope,
    /// Number of live jobs whose sifter was rebound by this call.
    pub jobs_rebound: u32,
}

/// One entry in `registry_list_active`. Carries the scope the rule
/// is bound to so a rule active under several scopes appears once
/// per scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryActiveEntry {
    pub rule_id: String,
    pub version: u32,
    pub severity: Severity,
    pub event_kind: String,
    pub tags: Vec<String>,
    pub scope: ActivationScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryListActiveResponse {
    pub entries: Vec<RegistryActiveEntry>,
    /// More active entries existed than `entries` carries (bounded by
    /// [`MAX_LIST_LIMIT`] / the request `limit`).
    #[serde(default)]
    pub truncated: bool,
}

// =====================================================================
// TC43: file probe surface.
//
// Three families:
//   - `file_read_window` — bounded line/byte window read of one file.
//   - `file_search`       — bounded substring/regex match over one file.
//   - `file_watch_*`      — daemon-owned follow-mode FileProbe attached
//                           to a bucket so scoped rules emit signal.
// =====================================================================

/// Hard cap on lines returned by `file_read_window` in a single call.
pub const MAX_FILE_READ_LINES: u32 = 2_000;
/// Default lines when the caller omits a limit.
pub const DEFAULT_FILE_READ_LINES: u32 = 200;
/// Hard cap on bytes returned by `file_read_window`. Same envelope cap
/// shape as the bucket / event-context payloads.
pub const MAX_FILE_READ_BYTES: usize = 64 * 1024;
/// Default `file_read_window` byte cap.
pub const DEFAULT_FILE_READ_BYTES: usize = MAX_FILE_READ_BYTES;
/// Hard cap on matches returned by `file_search` in a single call.
pub const MAX_FILE_SEARCH_MATCHES: u32 = 500;
/// Default `file_search` match cap.
pub const DEFAULT_FILE_SEARCH_MATCHES: u32 = 100;
/// Hard cap on snippet bytes returned per `file_search` match.
pub const MAX_FILE_SEARCH_SNIPPET_BYTES: usize = 512;
/// Default snippet bytes per match.
pub const DEFAULT_FILE_SEARCH_SNIPPET_BYTES: usize = 240;
/// Hard cap on bytes scanned by a single `file_search` call. Protects
/// the daemon from a request that asks to search a gigabyte file.
pub const MAX_FILE_SEARCH_SCAN_BYTES: u64 = 16 * 1024 * 1024;

/// `file_read_window` parameters.
///
/// Either `start_line` (1-based) drives a line-window read or
/// `start_byte` drives a byte-window read. If both are omitted the
/// daemon reads from line 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadWindowParams {
    pub path: std::path::PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

/// One line returned by `file_read_window`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLine {
    /// 1-based line number within the file.
    pub line: u64,
    /// Byte offset where this line begins. Useful for follow-up
    /// reads / context windows.
    pub byte_offset: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadWindowResponse {
    pub path: std::path::PathBuf,
    pub lines: Vec<FileLine>,
    /// File size in bytes at read time.
    pub file_bytes: u64,
    /// `true` when the response was clamped by line / byte cap.
    pub truncated: bool,
    /// First byte offset past the last line returned. Lets the
    /// caller compute a follow-up window without rereading.
    pub next_byte_offset: u64,
}

/// `file_search` parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchParams {
    pub path: std::path::PathBuf,
    /// Substring to find. Required.
    pub query: String,
    /// Case-insensitive match. Defaults to false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_insensitive: Option<bool>,
    /// Hard cap on returned matches. Clamped to
    /// [`MAX_FILE_SEARCH_MATCHES`]. Omitted = [`DEFAULT_FILE_SEARCH_MATCHES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_matches: Option<u32>,
    /// Hard cap on snippet bytes per match. Clamped to
    /// [`MAX_FILE_SEARCH_SNIPPET_BYTES`]. Omitted =
    /// [`DEFAULT_FILE_SEARCH_SNIPPET_BYTES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_snippet_bytes: Option<usize>,
}

/// One `file_search` match. Bounded shape: never the whole line, never
/// arbitrary bytes — `snippet` is capped at `max_snippet_bytes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchMatch {
    /// 1-based line number.
    pub line: u64,
    /// Byte offset of the matching position within the file.
    pub byte_offset: u64,
    /// Bounded text snippet around the match. Replaced with the
    /// owning line, truncated to `max_snippet_bytes`. Never raw
    /// stream bytes; always UTF-8.
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchResponse {
    pub path: std::path::PathBuf,
    pub matches: Vec<FileSearchMatch>,
    /// `true` when the search hit the per-call cap or the scan-bytes
    /// budget before completing.
    pub truncated: bool,
    /// Bytes actually scanned (may be lower than file size when the
    /// scan-bytes budget tripped first).
    pub bytes_scanned: u64,
}

/// `file_watch_start` parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatchStartParams {
    pub path: std::path::PathBuf,
    /// Optional bucket config (max_events / TTL). Defaults applied
    /// if None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_config: Option<BucketConfig>,
    /// Optional inline rule set bound to this watch only. Empty
    /// means the per-job set is whatever scoped activations the
    /// registry resolves for the watch's `(bucket_id, watch_id,
    /// probe_id)` triple.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RuleDefinition>,
    /// Follow from end (skip existing content) or from beginning.
    /// Defaults to follow-end (typical "tail -F" semantics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_from_beginning: Option<bool>,
    /// Optional per-bucket tag for subscription routing (Phase 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatchStartResponse {
    /// Opaque watch identifier. The LLM uses this with
    /// `file_watch_stop`. Wire form is a JobId so scoped activation
    /// can target a single watch via `ActivationScope::Job { job_id }`.
    pub watch_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: terminal_commander_core::ProbeId,
    pub cursor: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatchStopParams {
    pub watch_id: JobId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatchStopResponse {
    pub watch_id: JobId,
    pub bucket_id: BucketId,
    pub frames_total: u64,
    pub events_emitted: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatchListEntry {
    pub watch_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: terminal_commander_core::ProbeId,
    pub path: std::path::PathBuf,
    pub frames_total: u64,
    pub events_emitted: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatchListResponse {
    pub entries: Vec<FileWatchListEntry>,
}

// =====================================================================
// TC44: PTY command surface.
// =====================================================================

/// Hard cap on argv items for a PTY command. Matches `MAX_ARGV_ITEMS`
/// from `command.rs` so the two surfaces stay symmetric.
pub const MAX_PTY_ARGV_ITEMS: usize = 256;
/// Hard cap on bytes accepted in one `pty_command_write_stdin` call.
/// Mirrors `MAX_PTY_STDIN_BYTES` from `crates/probes::pty`.
pub const MAX_PTY_STDIN_BYTES: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyCommandStartParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<terminal_commander_core::EnvironmentSpec>,
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<std::path::PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_config: Option<BucketConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RuleDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cols: Option<u16>,
    /// Optional per-bucket tag for subscription routing (Phase 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyCommandStartResponse {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: terminal_commander_core::ProbeId,
    pub cursor: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyCommandWriteStdinParams {
    pub job_id: JobId,
    /// Bytes to write. Capped at `MAX_PTY_STDIN_BYTES`. Sent as a
    /// JSON string; non-UTF-8 input must be base64-pre-encoded by the
    /// caller (TC44 surface accepts UTF-8 only).
    pub bytes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyCommandWriteStdinResponse {
    pub job_id: JobId,
    pub bytes_written: u64,
    /// Echoes the post-write secret-prompt-active flag so the LLM
    /// can avoid a follow-up write that would also be rejected.
    pub secret_prompt_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyCommandStopParams {
    pub job_id: JobId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyCommandStopResponse {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub frames_total: u64,
    pub events_emitted: u64,
    pub bytes_total: u64,
    pub stdin_bytes_written: u64,
    pub secret_prompts_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyCommandListEntry {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: terminal_commander_core::ProbeId,
    pub argv: Vec<String>,
    pub frames_total: u64,
    pub events_emitted: u64,
    pub bytes_total: u64,
    pub stdin_bytes_written: u64,
    pub secret_prompts_total: u64,
    pub secret_prompt_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyCommandListResponse {
    pub entries: Vec<PtyCommandListEntry>,
}

// =====================================================================
// TC45: aggregate runtime view (read-only).
//
// `runtime_state`, `probe_list`, and `probe_status` surface the union
// of `CommandRuntime::live_jobs`, `WatchRuntime::list`, and
// `PtyRuntime::list` (when cfg(unix)) plus bucket counters and the
// scoped activation snapshot. No new spawn / cancel / mutation
// capability. No raw stream content.
// =====================================================================

/// Default + hard cap on each list/snapshot vec (subscriptions §6).
///
/// `runtime_state` bounds its THREE vecs (probes, buckets, active_rules)
/// INDEPENDENTLY by this; `probe_list` / `registry_list_active` bound their
/// single vec by it. Over-cap sets the per-vec `truncated` flag.
pub const MAX_LIST_LIMIT: usize = 500;

/// Closed set of probe kinds surfaced by `probe_list` / `probe_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeKind {
    Command,
    FileWatch,
    Pty,
}

/// Single authoritative liveness union for a probe / subscription source.
///
/// Derived from the job ledger (NOT from live-map presence: command
/// bindings linger in the live map after exit, so presence is not
/// "running"). Reused by `ProbeListEntry.liveness` and the subscription
/// `SourceLiveness`. See subscriptions spec MUST-ADD #3.
///
/// `JobState -> Liveness` mapping (command kind):
/// `Starting -> Starting`, `Running -> Running`,
/// `Exited -> Exited{code}`, `Failed -> Failed{code,signal}`,
/// `Cancelled -> Cancelled` (cancel sets signal `"CANCELLED"` and MUST
/// NOT be folded into `Failed`). File-watch / PTY report `Running`
/// while present (no exit-code concept on the live-map path).
/// `Dropped{count}` carries a bucket's `dropped_count` when surfacing
/// bucket-level lag (not used by per-probe derivation today).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum Liveness {
    /// Probe registered, process not yet observed running.
    Starting,
    /// Probe is live (process running, or watch/PTY present).
    Running,
    /// Process exited cleanly (code 0, no signal).
    Exited { code: i32 },
    /// Process exited non-zero or was killed by a signal.
    Failed {
        code: Option<i32>,
        signal: Option<String>,
    },
    /// Operator-initiated kill before exit (JobState::Cancelled).
    Cancelled,
    /// Probe stopped without an exit code (watch/PTY removal).
    Stopped,
    /// Bucket-level lag: `count` events were dropped (eviction).
    Dropped { count: u64 },
}

impl Default for Liveness {
    /// Backstop for `#[serde(default)]` on `ProbeListEntry.liveness`
    /// when decoding an older payload that predates the field. A
    /// present-but-unannotated probe is treated as `Running`.
    fn default() -> Self {
        Self::Running
    }
}

/// One row in `probe_list` / `runtime_state.probes`.
///
/// Bounded; never carries raw stream content. `argv` is the
/// bounded argv passed at spawn time (for FileWatch this is
/// `["file_watch:<path>"]`, matching how `WatchRuntime` registers
/// the JobConfig today; for PTY this is the original argv).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeListEntry {
    pub kind: ProbeKind,
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: terminal_commander_core::ProbeId,
    pub frames_total: u64,
    pub events_emitted: u64,
    #[serde(default)]
    pub frames_suppressed: u64,
    #[serde(default)]
    pub frames_suppressed_progress: u64,
    #[serde(default)]
    pub frames_suppressed_dedupe: u64,
    /// PTY only — surfaces `PtyProbeMetrics::secret_prompts_total`.
    /// Other kinds return 0.
    pub secret_prompts_total: u64,
    /// PTY only — current secret-prompt flag from the probe.
    /// Other kinds return `false`.
    pub secret_prompt_active: bool,
    /// File-watch only — surfaces the watched path. Other kinds
    /// return None.
    pub path: Option<std::path::PathBuf>,
    /// Per-source liveness. Command probes derive this from the job
    /// ledger (`command.status(job).state`) — NOT from live-map
    /// presence, which lingers after exit. File-watch and PTY probes
    /// report `Running` while present. `#[serde(default)]` so older
    /// payloads (pre-liveness) decode as `Running`.
    #[serde(default)]
    pub liveness: Liveness,
}

/// Bucket-level counters surfaced by `runtime_state.buckets`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBucketSummary {
    pub bucket_id: BucketId,
    pub head_seq: u64,
    pub tail_seq: u64,
    pub event_count: u64,
    /// Backpressure indicator: events dropped by the bucket's
    /// retention policy. Already tracked by `BucketSummary`
    /// (TC07); surfaced here in the aggregate view.
    pub dropped_count: u64,
}

/// One active scoped registry binding surfaced by
/// `runtime_state.active_rules`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeActiveRule {
    pub rule_id: String,
    pub version: u32,
    pub event_kind: String,
    pub scope: terminal_commander_core::ActivationScope,
}

/// Optional per-call `limit` for the single-vec list snapshots
/// (`probe_list`, `registry_list_active`) and `runtime_state` (applied
/// independently to each of its three vecs). Clamped to [`MAX_LIST_LIMIT`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListLimitParams {
    /// Max rows per vec. Clamped to [`MAX_LIST_LIMIT`]. Omitted =
    /// [`MAX_LIST_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStateResponse {
    pub command_jobs: u32,
    pub pty_jobs: u32,
    pub file_watches: u32,
    pub bucket_count: u32,
    pub active_rules_count: u32,
    pub probes: Vec<ProbeListEntry>,
    pub buckets: Vec<RuntimeBucketSummary>,
    pub active_rules: Vec<RuntimeActiveRule>,
    /// More probes existed than `probes` carries (bounded independently
    /// by [`MAX_LIST_LIMIT`] / the request `limit`).
    #[serde(default)]
    pub probes_truncated: bool,
    /// More buckets existed than `buckets` carries.
    #[serde(default)]
    pub buckets_truncated: bool,
    /// More active rules existed than `active_rules` carries.
    #[serde(default)]
    pub active_rules_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeListResponse {
    pub probes: Vec<ProbeListEntry>,
    /// More probes existed than `probes` carries (bounded by
    /// [`MAX_LIST_LIMIT`] / the request `limit`).
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeStatusParams {
    pub probe_id: terminal_commander_core::ProbeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeStatusResponse {
    pub probe: ProbeListEntry,
}

// =====================================================================
// P4: audit-log read surface (read-only).
//
// `audit_since` exposes a cursor-paged, bounded view of the persistent
// audit log. The protocol owns its serde surface: [`AuditRowWire`]
// MIRRORS `terminal_commander_store::AuditRow` the same way
// [`SeverityHistogram`] mirrors its in-memory type. The wire form does
// NOT couple to store internals — `timestamp` is carried as an RFC3339
// string so the protocol crate needs no `time` dependency.
// =====================================================================

/// Hard cap on rows returned by `audit_since` in a single call.
/// Mirrors `terminal_commander_store::MAX_AUDIT_READ_LIMIT`. The daemon
/// dispatcher clamps oversize / omitted limits to this value.
pub const MAX_AUDIT_READ_LIMIT: usize = 10_000;
/// Default rows returned when the caller omits a limit. Mirrors
/// `terminal_commander_store::DEFAULT_AUDIT_READ_LIMIT`.
pub const DEFAULT_AUDIT_READ_LIMIT: usize = 200;

/// Parameters for `audit_since`. Reads rows strictly after `cursor`
/// (i.e. `audit_id > cursor`), ordered ascending by `audit_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSinceParams {
    /// Return rows with `audit_id > cursor`. `0` reads from the start.
    pub cursor: u64,
    /// Optional exact-match action filter (e.g. `"registry_activate"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_filter: Option<String>,
    /// Optional exact-match decision filter (e.g. `"info"`, `"error"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_filter: Option<String>,
    /// Result count cap. Clamped to [`MAX_AUDIT_READ_LIMIT`] at the
    /// dispatcher. Omitted = [`DEFAULT_AUDIT_READ_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// One audit row on the wire.
///
/// MIRRORS `terminal_commander_store::AuditRow` field-for-field; the
/// daemon maps the in-memory row to this shape (a unit test guards the
/// mapping against drift). `timestamp` is the row's RFC3339 string —
/// the same encoding the store persists — so the protocol owns its
/// serde surface without a `time` dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRowWire {
    pub audit_id: u64,
    /// RFC3339 timestamp string.
    pub timestamp: String,
    pub action: String,
    pub subject: String,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_json: Option<String>,
}

/// Response shape for `audit_since`. Bounded by
/// [`MAX_AUDIT_READ_LIMIT`]; carries the next cursor so a client can
/// page forward without re-reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSinceResponse {
    /// Echo of the requested cursor.
    pub cursor_in: u64,
    /// `audit_id` of the last returned row, or `cursor_in` when empty.
    /// Pass as the next call's `cursor` to page forward.
    pub next_cursor: u64,
    pub rows: Vec<AuditRowWire>,
}

// =====================================================================
// Subscriptions (Phase 1): predicate-routed, multiplexed event consumer.
//
// Wire types for `subscription_open/pull/list/close`. The daemon holds
// the opaque per-open `sub_id` + server-advanced offsets; the wire form
// carries the predicate, the source-tagged events, and per-source
// liveness. See `docs/superpowers/specs/2026-06-02-subscriptions-design.md`
// §4, §6.
// =====================================================================

/// Per-bucket routing selector on the wire. Mirrors the daemon-internal
/// `SourceSel`. `all` auto-includes future matching buckets; the fixed
/// variants are a closed set of ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SubscriptionSourceSel {
    /// Every bucket; future buckets auto-join on the next routing rebuild.
    All,
    /// A fixed set of owning jobs.
    Jobs { jobs: Vec<JobId> },
    /// A fixed set of bucket ids.
    Buckets { buckets: Vec<BucketId> },
    /// A fixed set of owning probes.
    Probes {
        probes: Vec<terminal_commander_core::ProbeId>,
    },
}

/// The wire predicate. All fields optional; AND semantics. `severity_min`
/// and `kind` are per-EVENT filters; `sources` is per-BUCKET routing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionPredicate {
    /// Minimum severity (per-EVENT). Omitted = `trace` (no filter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_min: Option<Severity>,
    /// Event-kind allowlist (per-EVENT). Omitted = any kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Vec<String>>,
    /// Per-BUCKET routing selector. Omitted = `all`.
    #[serde(default = "default_source_sel")]
    pub sources: SubscriptionSourceSel,
    /// Per-BUCKET tag AND-filter. Omitted = ignore the tag dimension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// Default routing selector (`all`) when `sources` is omitted on the wire.
const fn default_source_sel() -> SubscriptionSourceSel {
    SubscriptionSourceSel::All
}

/// One source-tagged event delivered by `subscription_pull`.
///
/// Reuses the existing [`SignalEvent`] wire shape (the same type
/// `bucket_events_since` returns), plus its provenance so a multiplexed
/// consumer can attribute it without juggling N cursors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionEvent {
    /// Bucket the event was read from.
    pub bucket_id: BucketId,
    /// Owning job, if the bucket's source recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<JobId>,
    /// The event's per-bucket sequence number (provenance echo).
    pub seq: u64,
    /// The matched signal event.
    pub event: SignalEvent,
}

/// Per-source liveness entry returned with every pull (including idle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLiveness {
    /// The in-scope bucket this entry describes.
    pub bucket_id: BucketId,
    /// Owning job, if recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<JobId>,
    /// Owning probe, if recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_id: Option<terminal_commander_core::ProbeId>,
    /// Process / probe liveness (the single authoritative union).
    pub liveness: Liveness,
}

/// One row in `subscription_list`. Bounded; the predicate hash lets an
/// agent recognize an equivalent predicate without coupling cursors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSummary {
    /// Opaque per-open handle.
    pub sub_id: String,
    /// Stable hash of the normalized predicate (decimal string).
    pub predicate_hash: String,
    /// Number of buckets this subscription currently tracks an offset for.
    pub source_count: u32,
    /// Milliseconds since the Unix epoch at open.
    pub created_at_ms: u64,
    /// Milliseconds since the Unix epoch of the last pull, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pull_at_ms: Option<u64>,
}

/// `subscription_open` parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionOpenParams {
    pub predicate: SubscriptionPredicate,
}

/// `subscription_open` response. `boot_id` lets a looping agent detect a
/// restart (registry + buckets + offsets reset together).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionOpenResponse {
    /// Opaque per-open handle (uuid string).
    pub sub_id: String,
    /// This daemon's per-boot id (uuid string).
    pub boot_id: String,
    /// Stable hash of the normalized predicate (decimal string).
    pub predicate_hash: String,
    /// Milliseconds since the Unix epoch at open.
    pub created_at_ms: u64,
    /// Number of already-in-scope sources matched at open.
    pub matched_sources: u32,
}

/// `subscription_pull` parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPullParams {
    pub sub_id: String,
    /// Max events to return. Clamped to [`MAX_PULL_EVENTS`]. Omitted =
    /// [`MAX_PULL_EVENTS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<usize>,
    /// Blocking timeout. Clamped to `[1, MAX_PULL_TIMEOUT_MS]`. Omitted =
    /// [`DEFAULT_PULL_TIMEOUT_MS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// `subscription_pull` response. Idle = empty `events` + liveness (never
/// an error). No `next_state` in Phase 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPullResponse {
    pub events: Vec<SubscriptionEvent>,
    pub liveness: Vec<SourceLiveness>,
    /// Any in-scope bucket dropped events under us (eviction lag).
    pub lagged: bool,
    /// The routing scan hit [`MAX_BUCKETS_PER_SUBSCRIPTION`].
    pub truncated: bool,
}

/// `subscription_list` parameters. Bounded by an optional `limit`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubscriptionListParams {
    /// Max rows to return. Clamped to [`MAX_SUBSCRIPTIONS`]. Omitted =
    /// [`MAX_SUBSCRIPTIONS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// `subscription_list` response. Bounded; `truncated` set if more
/// subscriptions exist than were returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionListResponse {
    pub subscriptions: Vec<SubscriptionSummary>,
    pub truncated: bool,
}

/// `subscription_close` parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionCloseParams {
    pub sub_id: String,
}

/// `subscription_close` response. `closed=false` when the `sub_id` was
/// already unknown (idempotent close).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionCloseResponse {
    pub closed: bool,
}

/// `subscription_seek` parameters.
///
/// Reposition ONE bucket's offset for an existing subscription (explicit
/// re-read). The requested `seq` is clamped to the bucket's live range; it is
/// never an error. (Subscriptions §3 seek.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSeekParams {
    /// Opaque handle from `subscription_open`.
    pub sub_id: String,
    /// Bucket to reposition within.
    pub bucket_id: BucketId,
    /// Requested re-read position. Clamped to
    /// `[head_seq.saturating_sub(1), tail_seq]`; never an error.
    pub seq: u64,
}

/// `subscription_seek` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSeekResponse {
    /// The offset actually stored after clamping.
    pub clamped_seq: u64,
    /// True when the requested seq was below `head_seq.saturating_sub(1)`
    /// (the requested events were already evicted).
    pub lagged: bool,
}

/// Serialize an envelope to a length-prefixed wire frame. Returns
/// the bytes ready to write to the socket. Rejects payloads larger
/// than [`MAX_FRAME_BYTES`].
pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, IpcError> {
    let json = serde_json::to_vec(value)
        .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("serialize: {e}")))?;
    if json.len() > MAX_FRAME_BYTES {
        return Err(IpcError::new(
            IpcErrorCode::FrameTooLarge,
            format!(
                "payload {} bytes > MAX_FRAME_BYTES {MAX_FRAME_BYTES}",
                json.len()
            ),
        ));
    }
    let len_u32 = u32::try_from(json.len())
        .map_err(|_| IpcError::new(IpcErrorCode::Internal, "len overflow"))?;
    let mut out = Vec::with_capacity(4 + json.len());
    out.extend_from_slice(&len_u32.to_be_bytes());
    out.extend_from_slice(&json);
    Ok(out)
}

/// Deserialize the JSON portion of a frame (length-prefix stripped
/// by the caller). The caller has already validated that
/// `payload.len() <= MAX_FRAME_BYTES`.
pub fn decode_payload<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> Result<T, IpcError> {
    serde_json::from_slice::<T>(payload)
        .map_err(|e| IpcError::new(IpcErrorCode::MalformedJson, format!("decode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_error_is_classified_and_daemon_internal_is_not() {
        // A client-side transport failure carries the marker and is recognized.
        let t = IpcError::transport("connect: os error 2");
        assert_eq!(t.code, IpcErrorCode::Internal);
        assert!(
            t.is_transport(),
            "a transport() error must classify as transport"
        );
        assert!(
            t.message.starts_with(IpcError::TRANSPORT_PREFIX),
            "transport message keeps a human-readable marker prefix"
        );
        assert!(
            t.message.contains("connect: os error 2"),
            "the underlying cause is preserved in the message"
        );

        // A daemon-RETURNED Internal error (no marker) must NOT classify as
        // transport, so it keeps the real internal_error mapping at the MCP edge.
        let daemon_internal = IpcError::new(IpcErrorCode::Internal, "open: permission denied");
        assert!(
            !daemon_internal.is_transport(),
            "a daemon-returned Internal error must NOT be misread as transport"
        );

        // A caller-fixable code is never transport regardless of message text.
        let fixable = IpcError::new(IpcErrorCode::PathDenied, "transport: not really");
        assert!(
            !fixable.is_transport(),
            "only Internal-coded marker-prefixed errors are transport"
        );
    }

    #[test]
    fn transport_error_survives_wire_round_trip() {
        // The marker rides in the message, so a transport error serialized and
        // decoded over the wire is still recognized (defensive: the daemon never
        // sends one, but the classifier must be robust if it ever did).
        let t = IpcError::transport("pipe connect: os error 2");
        let json = serde_json::to_string(&t).unwrap();
        let back: IpcError = serde_json::from_str(&json).unwrap();
        assert!(back.is_transport());
    }

    #[test]
    fn encode_decode_envelope_round_trip() {
        let req = RequestEnvelope {
            correlation_id: 42,
            request: IpcRequest::SystemDiscover,
        };
        let frame = encode_frame(&req).unwrap();
        // 4-byte length + JSON
        assert!(frame.len() > 4);
        let len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        assert_eq!(len, frame.len() - 4);
        let back: RequestEnvelope = decode_payload(&frame[4..]).unwrap();
        assert_eq!(back.correlation_id, 42);
        assert!(matches!(back.request, IpcRequest::SystemDiscover));
    }

    #[test]
    fn malformed_json_rejected_with_typed_code() {
        let bad = b"{not valid json";
        let err: IpcError = decode_payload::<RequestEnvelope>(bad).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::MalformedJson);
    }

    #[test]
    fn schema_mismatch_is_malformed_json_today() {
        // serde_json reports as a parse error; we surface that as
        // MalformedJson because the variant set is closed-set.
        let s = br#"{"correlation_id": 1, "request": {"method": "totally_bogus"}}"#;
        let err: IpcError = decode_payload::<RequestEnvelope>(s).unwrap_err();
        // Either MalformedJson or SchemaMismatch is acceptable; both
        // keep the bad payload out of the dispatcher.
        assert!(matches!(
            err.code,
            IpcErrorCode::MalformedJson | IpcErrorCode::SchemaMismatch
        ));
    }

    #[test]
    fn frame_too_large_rejected_before_serialize_attempt() {
        // Construct an envelope that, once serialized, would exceed
        // MAX_FRAME_BYTES. Easiest way: a SelfCheck response with a
        // huge report string.
        let huge = "x".repeat(MAX_FRAME_BYTES + 1024);
        let env = ResponseEnvelope {
            correlation_id: 1,
            result: IpcResult::Ok {
                response: IpcResponse::SelfCheck(SelfCheckResponse {
                    report: huge,
                    failures: 0,
                }),
            },
        };
        let err = encode_frame(&env).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::FrameTooLarge);
    }

    #[test]
    fn subscription_error_codes_roundtrip_snake_case() {
        for (code, wire) in [
            (
                IpcErrorCode::UnknownSubscription,
                "\"unknown_subscription\"",
            ),
            (
                IpcErrorCode::SubscriptionLimitExceeded,
                "\"subscription_limit_exceeded\"",
            ),
        ] {
            let s = serde_json::to_string(&code).unwrap();
            assert_eq!(s, wire);
            let back: IpcErrorCode = serde_json::from_str(&s).unwrap();
            assert_eq!(back, code);
        }
    }

    #[test]
    fn subscription_open_pair_round_trips_through_request_and_response() {
        let params = SubscriptionOpenParams {
            predicate: SubscriptionPredicate {
                severity_min: Some(Severity::High),
                kind: Some(vec!["error".to_owned(), "panic".to_owned()]),
                sources: SubscriptionSourceSel::Jobs {
                    jobs: vec![JobId::new()],
                },
                tag: None,
            },
        };
        let req = IpcRequest::SubscriptionOpen(params);
        let back: IpcRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert!(matches!(back, IpcRequest::SubscriptionOpen(_)));

        let resp = IpcResponse::SubscriptionOpen(SubscriptionOpenResponse {
            sub_id: "sub-1".to_owned(),
            boot_id: "boot-1".to_owned(),
            predicate_hash: "12345".to_owned(),
            created_at_ms: 1_700_000_000_000,
            matched_sources: 3,
        });
        let back: IpcResponse =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        match back {
            IpcResponse::SubscriptionOpen(r) => {
                assert_eq!(r.sub_id, "sub-1");
                assert_eq!(r.matched_sources, 3);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn subscription_predicate_defaults_sources_to_all_when_omitted() {
        let json = r#"{"severity_min":"high"}"#;
        let p: SubscriptionPredicate = serde_json::from_str(json).unwrap();
        assert_eq!(p.sources, SubscriptionSourceSel::All);
        assert_eq!(p.severity_min, Some(Severity::High));
        assert!(p.kind.is_none());
    }

    #[test]
    fn subscription_pull_pair_round_trips_through_request_and_response() {
        let params = SubscriptionPullParams {
            sub_id: "sub-1".to_owned(),
            max: Some(25),
            timeout_ms: Some(3_000),
        };
        let req = IpcRequest::SubscriptionPull(params);
        let back: IpcRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        match back {
            IpcRequest::SubscriptionPull(p) => {
                assert_eq!(p.sub_id, "sub-1");
                assert_eq!(p.max, Some(25));
                assert_eq!(p.timeout_ms, Some(3_000));
            }
            other => panic!("unexpected: {other:?}"),
        }

        let resp = IpcResponse::SubscriptionPull(SubscriptionPullResponse {
            events: Vec::new(),
            liveness: vec![SourceLiveness {
                bucket_id: BucketId::new(),
                job_id: Some(JobId::new()),
                probe_id: None,
                liveness: Liveness::Exited { code: 0 },
            }],
            lagged: false,
            truncated: false,
        });
        let back: IpcResponse =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        match back {
            IpcResponse::SubscriptionPull(r) => {
                assert!(r.events.is_empty());
                assert_eq!(r.liveness.len(), 1);
                assert_eq!(r.liveness[0].liveness, Liveness::Exited { code: 0 });
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn subscription_list_pair_round_trips_through_request_and_response() {
        let req = IpcRequest::SubscriptionList(SubscriptionListParams { limit: Some(10) });
        let back: IpcRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert!(matches!(back, IpcRequest::SubscriptionList(_)));

        let resp = IpcResponse::SubscriptionList(SubscriptionListResponse {
            subscriptions: vec![SubscriptionSummary {
                sub_id: "sub-1".to_owned(),
                predicate_hash: "9".to_owned(),
                source_count: 2,
                created_at_ms: 1,
                last_pull_at_ms: Some(2),
            }],
            truncated: true,
        });
        let back: IpcResponse =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        match back {
            IpcResponse::SubscriptionList(r) => {
                assert!(r.truncated);
                assert_eq!(r.subscriptions.len(), 1);
                assert_eq!(r.subscriptions[0].source_count, 2);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn subscription_close_pair_round_trips_through_request_and_response() {
        let req = IpcRequest::SubscriptionClose(SubscriptionCloseParams {
            sub_id: "sub-1".to_owned(),
        });
        let back: IpcRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert!(matches!(back, IpcRequest::SubscriptionClose(_)));

        let resp = IpcResponse::SubscriptionClose(SubscriptionCloseResponse { closed: true });
        let back: IpcResponse =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        match back {
            IpcResponse::SubscriptionClose(r) => assert!(r.closed),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn subscription_seek_pair_round_trips_through_request_and_response() {
        let req = IpcRequest::SubscriptionSeek(SubscriptionSeekParams {
            sub_id: "sub-1".to_owned(),
            bucket_id: BucketId::new(),
            seq: 42,
        });
        let back: IpcRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        match back {
            IpcRequest::SubscriptionSeek(p) => assert_eq!(p.seq, 42),
            other => panic!("unexpected: {other:?}"),
        }

        let resp = IpcResponse::SubscriptionSeek(SubscriptionSeekResponse {
            clamped_seq: 7,
            lagged: true,
        });
        let back: IpcResponse =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        match back {
            IpcResponse::SubscriptionSeek(r) => {
                assert_eq!(r.clamped_seq, 7);
                assert!(r.lagged);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn command_output_tail_params_round_trip() {
        let params = CommandOutputTailParams {
            job_id: JobId::new(),
            max_lines: 50,
            max_bytes: 65_536,
        };
        let json = serde_json::to_string(&params).unwrap();
        let back: CommandOutputTailParams = serde_json::from_str(&json).unwrap();
        assert_eq!(back, params);
        // defaults kick in when fields are absent
        let minimal = format!(r#"{{"job_id":"{}"}}"#, params.job_id);
        let def: CommandOutputTailParams = serde_json::from_str(&minimal).unwrap();
        assert_eq!(def.max_lines, 50);
        assert_eq!(def.max_bytes, 65_536);
    }

    #[test]
    fn response_envelope_err_round_trips() {
        let env = ResponseEnvelope {
            correlation_id: 7,
            result: IpcResult::Err {
                error: IpcError::new(IpcErrorCode::PolicyDenied, "nope"),
            },
        };
        let frame = encode_frame(&env).unwrap();
        let back: ResponseEnvelope = decode_payload(&frame[4..]).unwrap();
        match back.result {
            IpcResult::Err { error } => {
                assert_eq!(error.code, IpcErrorCode::PolicyDenied);
                assert_eq!(error.message, "nope");
            }
            IpcResult::Ok { .. } => panic!("expected err variant"),
        }
    }

    #[test]
    fn shutdown_variants_serde_roundtrip() {
        // Request round-trips. `IpcRequest` does not derive `PartialEq`,
        // so compare the serialized forms instead of the values.
        let req = IpcRequest::Shutdown;
        let s = serde_json::to_string(&req).unwrap();
        let s2 = serde_json::to_string(&serde_json::from_str::<IpcRequest>(&s).unwrap()).unwrap();
        assert_eq!(s, s2);

        // Response round-trips with the draining flag.
        let resp = IpcResponse::ShutdownAck { draining: true };
        let s = serde_json::to_string(&resp).unwrap();
        match serde_json::from_str::<IpcResponse>(&s).unwrap() {
            IpcResponse::ShutdownAck { draining } => assert!(draining),
            other => panic!("unexpected: {other:?}"),
        }

        // The new error code serializes.
        let _ = serde_json::to_string(&IpcErrorCode::ShuttingDown).unwrap();
    }

    #[test]
    fn audit_since_params_round_trip() {
        let full = AuditSinceParams {
            cursor: 7,
            action_filter: Some("registry_activate".to_owned()),
            decision_filter: Some("info".to_owned()),
            limit: Some(50),
        };
        let json = serde_json::to_string(&full).unwrap();
        let back: AuditSinceParams = serde_json::from_str(&json).unwrap();
        assert_eq!(back, full);

        // Optional fields default to None and are omitted on the wire.
        let minimal = AuditSinceParams {
            cursor: 0,
            action_filter: None,
            decision_filter: None,
            limit: None,
        };
        let json = serde_json::to_string(&minimal).unwrap();
        assert_eq!(json, r#"{"cursor":0}"#);
        let back: AuditSinceParams = serde_json::from_str(&json).unwrap();
        assert_eq!(back, minimal);
    }

    #[test]
    fn audit_since_response_round_trips_through_envelope() {
        let resp = AuditSinceResponse {
            cursor_in: 0,
            next_cursor: 2,
            rows: vec![
                AuditRowWire {
                    audit_id: 1,
                    timestamp: "2026-06-01T00:00:00Z".to_owned(),
                    action: "registry_activate".to_owned(),
                    subject: "peer".to_owned(),
                    decision: "info".to_owned(),
                    profile: Some("developer_local".to_owned()),
                    reason: None,
                    actor: Some("cli".to_owned()),
                    metadata_json: None,
                },
                AuditRowWire {
                    audit_id: 2,
                    timestamp: "2026-06-01T00:00:01Z".to_owned(),
                    action: "system_discover".to_owned(),
                    subject: "peer".to_owned(),
                    decision: "info".to_owned(),
                    profile: None,
                    reason: None,
                    actor: None,
                    metadata_json: None,
                },
            ],
        };
        let env = ResponseEnvelope {
            correlation_id: 9,
            result: IpcResult::Ok {
                response: IpcResponse::AuditSince(resp.clone()),
            },
        };
        let frame = encode_frame(&env).unwrap();
        let back: ResponseEnvelope = decode_payload(&frame[4..]).unwrap();
        match back.result {
            IpcResult::Ok {
                response: IpcResponse::AuditSince(r),
            } => assert_eq!(r, resp),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn audit_since_request_round_trips_through_envelope() {
        let req = RequestEnvelope {
            correlation_id: 3,
            request: IpcRequest::AuditSince(AuditSinceParams {
                cursor: 0,
                action_filter: None,
                decision_filter: None,
                limit: Some(50),
            }),
        };
        let frame = encode_frame(&req).unwrap();
        let back: RequestEnvelope = decode_payload(&frame[4..]).unwrap();
        match back.request {
            IpcRequest::AuditSince(p) => {
                assert_eq!(p.cursor, 0);
                assert_eq!(p.limit, Some(50));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}
