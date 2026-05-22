// SPDX-License-Identifier: Apache-2.0
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

pub use crate::command::{CommandStartResponse, CommandStatusResponse};

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

/// Method-typed request union. Method names are namespaced
/// `<domain>_<verb>` (matching the MCP tool naming so the eventual
/// rmcp adapter at TC40 maps 1:1).
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
    /// Remove `(rule_id, version)` from the active set and close
    /// the persistent activation row.
    RegistryDeactivate(RegistryDeactivateParams),
    /// Snapshot of every currently-active `(rule_id, version)`.
    RegistryListActive,
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
    Health { uptime_secs: u64 },
    PolicyStatus(PolicyStatusResponse),
    SelfCheck(SelfCheckResponse),
    BucketEventsSince(BucketEventsSinceResponse),
    BucketWait(BucketWaitResponse),
    BucketSummary(BucketSummaryResponse),
    EventContext(EventContextResponse),
    CommandStartCombed(CommandStartResponse),
    CommandStatus(CommandStatusResponse),
    RegistrySearch(RegistrySearchResponse),
    RegistryGet(RegistryGetResponse),
    RegistryUpsert(RegistryUpsertResponse),
    RegistryTest(RegistryTestResponse),
    RegistryActivate(RegistryActivateResponse),
    RegistryDeactivate(RegistryDeactivateResponse),
    RegistryListActive(RegistryListActiveResponse),
    FileReadWindow(FileReadWindowResponse),
    FileSearch(FileSearchResponse),
    FileWatchStart(FileWatchStartResponse),
    FileWatchStop(FileWatchStopResponse),
    FileWatchList(FileWatchListResponse),
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
}

/// Structured error payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
}

impl IpcError {
    /// Constructor.
    #[must_use]
    pub fn new(code: IpcErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
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

/// Wire shape for `command_start_combed`. Mirrors
/// [`crate::command::CommandStartRequest`] but uses millis instead of
/// `Duration` so the JSON form stays human-readable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStartParams {
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
}
