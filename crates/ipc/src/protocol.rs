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
    ActivationScope, BucketConfig, BucketId, EventId, JobId, RuleDefinition, RuleStatus, SessionId,
    Severity, SignalEvent, SourceStream,
};

/// US2 (FR-011): a non-blocking hint that a curated rule pack exists
/// for the tool being started but is not active. Surfaced on
/// command-start responses so an agent can self-serve the comb rules.
///
/// This is advisory only: it changes no behavior and activates
/// nothing (constitution VII). The agent acts on it by calling
/// `registry_import_pack` (named in `action`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackAvailableHint {
    /// Always `"pack_available"`. A discriminant for forward-compat.
    pub kind: String,
    /// The recognized pack id (e.g. `"docker"`, `"git"`).
    pub pack: String,
    /// Always `"registry_import_pack"`: the tool to call to act on the
    /// hint.
    pub action: String,
}

impl PackAvailableHint {
    /// Build a `pack_available` hint for the named pack.
    #[must_use]
    pub fn for_pack(pack: impl Into<String>) -> Self {
        Self {
            kind: "pack_available".to_owned(),
            pack: pack.into(),
            action: "registry_import_pack".to_owned(),
        }
    }
}

/// Bounded response shape. Carries identifiers and counters, never
/// raw stdout/stderr.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStartResponse {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: terminal_commander_core::ProbeId,
    /// Initial bucket cursor: clients pass this to `bucket_events_since`.
    pub cursor: u64,
    /// US2 (FR-011): optional hint that a curated pack exists for this
    /// tool but is not active. Omitted (None) when the tool is
    /// unrecognized or its pack is already active. Advisory only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<PackAvailableHint>,
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
    /// TC-B3 (FR-027): `true` when this status was reconstructed from a
    /// PERSISTED job receipt because the in-memory job is gone (a daemon
    /// restart happened since the job ran). The terminal `state` /
    /// `exit_code` are then authoritative-from-disk; the live counters
    /// (`frames_*`, `bytes_total`) are zero because the in-memory probe
    /// metrics did not survive. An honest terminal result, never a bare
    /// error. Defaults to `false`; additive and non-breaking.
    #[serde(default)]
    pub restarted: bool,
}

/// Params for `command_stop` (TC-3): force-kill a running combed
/// command by `job_id`.
///
/// Mirrors [`PtyCommandStopParams`] for the non-PTY runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStopParams {
    pub job_id: JobId,
}

/// Bounded response for `command_stop` (TC-3).
///
/// Mirrors [`PtyCommandStopResponse`] minus the PTY-only counters
/// (`stdin_bytes_written`, `secret_prompts_total`), which have no
/// meaning for a non-interactive combed command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStopResponse {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub frames_total: u64,
    pub events_emitted: u64,
    pub bytes_total: u64,
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
/// names; the rmcp adapter maps each tool 1:1 to a method. 49 IPC
/// methods are live: the 48-method set carried via IPC plus the P4
/// `audit_since` read surface. `audit_since` is the one CLI-only read
/// method with no rmcp tool, so the MCP legacy tool catalogue stays at
/// 50 live tools (see `docs/mcp/TOOL_CONTROL_SURFACE.md` §2) while the
/// IPC method set carries the extra audit-log reader. The compact MCP
/// surface adds facade tools (e.g. `command`) on top, gated by
/// `TC_SURFACE=compact`; those forward to the same IPC methods.
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
    /// Force-kill a running combed command by `job_id` (TC-3). Bounded
    /// metadata response; never returns raw output.
    CommandStop(CommandStopParams),
    /// Rule-free bounded read of a job's captured output tail (F1).
    CommandOutputTail(CommandOutputTailParams),
    /// Start a shell-lane command (TC49): run ONE shell line through the
    /// comb pipeline behind the `allow_shell` capability. Denied by
    /// default; the wire carries `shell_line` ONLY, never a cap flag.
    /// Bounded metadata response (reuses [`CommandStartResponse`]);
    /// never returns raw stdout/stderr.
    ShellExec(ShellExecParams),
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
    /// US2 (FR-011): deactivate an entire seed pack or an explicit list
    /// of rule ids in ONE call, under one explicit scope, reporting
    /// per-rule outcomes. The single-rule [`RegistryDeactivate`] wire
    /// contract is untouched.
    RegistryDeactivateBulk(RegistryDeactivateBulkParams),
    /// Snapshot of every currently-active `(rule_id, version)`. Bounded
    /// by [`MAX_LIST_LIMIT`] / the request `limit`.
    RegistryListActive(ListLimitParams),
    /// US2 (FR-007): suggest candidate parsing rules from bounded
    /// output samples using PURE heuristics. Returns DRAFT proposals +
    /// a confidence label + the explicit next-step loop. NEVER
    /// activates or persists a rule (FR-008 / constitution VII).
    /// Read-only: a blind retry recomputes the same deterministic
    /// proposals, so it is idempotent.
    RegistrySuggestFromSamples(RegistrySuggestFromSamplesParams),
    /// Bounded line/byte window read of a regular file. Never
    /// returns the whole file; the daemon clamps the window.
    FileReadWindow(FileReadWindowParams),
    /// Bounded substring/regex search over one file. Returns
    /// structured match pointers + short snippets only.
    FileSearch(FileSearchParams),
    /// Bounded single-level listing of one directory (US3). Returns
    /// name/kind/size/mtime per entry, dirs-first deterministic order,
    /// capped with a truthful truncation flag. Same read-path policy gate
    /// as `file_read_window`. Symlinks/reparse points reported by kind,
    /// never followed.
    FileListDir(FileListDirParams),
    /// Write UTF-8 content to a single regular file (TC22 A3).
    /// MUTATING + NON-idempotent: a blind retry would double-write.
    /// The daemon policy-gates the canonical target against
    /// `paths.write_allow`, audits BEFORE the write, bounds the content
    /// size, and writes atomically (temp file + rename).
    FileWrite(FileWriteParams),
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
    /// Start a persistent shell session (P1 / TC50): a long-lived
    /// login-shell PTY behind the `allow_session` capability. Denied by
    /// default; policy-checked + audited before spawn. Bounded metadata
    /// response (`session_id`/`bucket_id`/`state`); never raw output.
    ShellSessionStart(ShellSessionStartParams),
    /// Send ONE line to a live session shell and read back the combed
    /// signals it produced. Output is read from the session bucket; never
    /// a raw stream. A send to a non-live session fails loudly.
    ShellSessionExec(ShellSessionExecParams),
    /// Query a session's lifecycle state, current cwd, and bounded env
    /// snapshot.
    ShellSessionStatus(ShellSessionStatusParams),
    /// Stop a session (graceful then forced) and report the terminal
    /// state.
    ShellSessionStop(ShellSessionStopParams),
    /// Snapshot of every currently-live session.
    ShellSessionList,
    /// Persist a session's cwd + bounded env as a restorable workspace
    /// snapshot (SQLite). Read-once of session state + a DB write.
    WorkspaceSnapshotCreate(WorkspaceSnapshotCreateParams),
    /// Restore a workspace snapshot's cwd/env into a (live) session.
    WorkspaceSnapshotApply(WorkspaceSnapshotApplyParams),
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

impl IpcRequest {
    /// Whether this RPC is safe to blindly re-send after a transport
    /// failure (the daemon pipe/socket dropped mid-call so the client
    /// never learned the outcome).
    ///
    /// Governing rule: return `false` for any RPC whose retry could
    /// create or duplicate a server-side resource, mint a fresh id, or
    /// advance server-held state; return `true` only for pure bounded
    /// reads and idempotent-effect repositioning. When in doubt, return
    /// `false` -- a missed retry is an error the caller can re-issue
    /// deliberately, but a silent double-effect cannot be undone.
    ///
    /// The match is EXHAUSTIVE (no wildcard arm) so that adding a new
    /// `IpcRequest` variant fails to compile until it is deliberately
    /// classified here.
    #[must_use]
    pub const fn is_idempotent(&self) -> bool {
        match self {
            // Mutating / unsafe to blind-retry: each one creates or
            // duplicates server-side state, mints a fresh id, or advances
            // a server-held offset.
            Self::CommandStartCombed(_)
            // Shell-lane start (TC49): spawns a fresh `[shell,"-lc",line]`
            // child + mints a job/bucket exactly like CommandStartCombed,
            // so a blind retry double-spawns. Non-idempotent.
            | Self::ShellExec(_)
            // Force-kill: fires a one-shot cancel + sets the job terminal.
            // A blind re-send is a harmless no-op on an already-terminal job,
            // but it is a server-side state MUTATION, so it is classified
            // non-idempotent alongside the other Command*/Pty* mutators.
            | Self::CommandStop(_)
            | Self::PtyCommandStart(_)
            | Self::PtyCommandWriteStdin(_)
            | Self::PtyCommandStop(_)
            // Session lane (P1 / TC50): start spawns a fresh session
            // shell + mints ids; exec writes stdin + advances the read
            // cursor server-side; stop fires a one-shot cancel; the
            // workspace snapshot create/apply mutate persisted state or
            // re-inject cwd/env into the session shell. All non-idempotent
            // alongside the PTY mutators.
            | Self::ShellSessionStart(_)
            | Self::ShellSessionExec(_)
            | Self::ShellSessionStop(_)
            | Self::WorkspaceSnapshotCreate(_)
            | Self::WorkspaceSnapshotApply(_)
            | Self::RegistryUpsert(_)
            | Self::RegistryActivate(_)
            | Self::RegistryDeactivate(_)
            // Bulk deactivate mutates the active set (durable rows +
            // in-memory authority) exactly like the single-rule form; a
            // blind retry re-closes already-closed rows (harmless no-op)
            // but is still a server-side mutation, so it groups with the
            // other registry mutators.
            | Self::RegistryDeactivateBulk(_)
            | Self::RegistryImportPack(_)
            // File WRITE (TC22 A3): creates or overwrites a file on disk.
            // A blind retry double-writes (or re-truncates) the target, so
            // it MUST be non-idempotent -- the BACKLOG P0.1 client self-heal
            // can never auto-retry a write. Classified MUTATING alongside
            // command_start / file_watch_start, NOT with the read-only
            // file_read_window / file_search.
            | Self::FileWrite(_)
            | Self::FileWatchStart(_)
            | Self::FileWatchStop(_)
            // Mints a fresh sub_id + a registry slot; a blind retry leaks
            // a slot and can trip SubscriptionLimitExceeded.
            | Self::SubscriptionOpen(_)
            // Frees a slot; conservative non-retry.
            | Self::SubscriptionClose(_)
            // NOT a safe read: per-consumer offsets are advanced and
            // committed SERVER-SIDE inside the pull (the drain advances
            // offsets and `commit` persists them, subscriptions/pull.rs
            // commit sites at 543/633) BEFORE the response is serialized.
            // A lost-then-
            // retried pull restarts from the already-advanced offset, so
            // the previously-drained events are silently dropped --
            // converting the documented lossless pull into a lossy one.
            // Contrast BucketWait, which is client-cursor-driven and fully
            // replayable.
            | Self::SubscriptionPull(_)
            | Self::Shutdown => false,

            // Pure bounded reads + idempotent-effect repositioning: a
            // retry observes state without changing it (or re-applies the
            // same absolute reposition).
            Self::Health
            | Self::SystemDiscover
            | Self::PolicyStatus
            | Self::SelfCheck
            | Self::CommandStatus(_)
            | Self::CommandOutputTail(_)
            | Self::BucketWait(_)
            | Self::BucketEventsSince(_)
            | Self::BucketSummary(_)
            | Self::EventContext(_)
            | Self::RuntimeState(_)
            | Self::ProbeList(_)
            | Self::ProbeStatus(_)
            | Self::PtyCommandList
            // Session read-only lookups: pure bounded reads, replayable.
            | Self::ShellSessionStatus(_)
            | Self::ShellSessionList
            | Self::FileReadWindow(_)
            | Self::FileSearch(_)
            // Directory listing (US3): a pure bounded read, replayable and
            // side-effect free, so it groups with the file read/search reads.
            | Self::FileListDir(_)
            | Self::FileWatchList
            | Self::RegistrySearch(_)
            | Self::RegistryGet(_)
            | Self::RegistryTest(_)
            | Self::RegistryListActive(_)
            // Suggestion is a pure deterministic heuristic over the
            // supplied samples: a retry recomputes the identical
            // proposal set and never activates/persists anything.
            | Self::RegistrySuggestFromSamples(_)
            | Self::SubscriptionList(_)
            // Set-position (absolute clamped offset), not advance-position,
            // so a re-send re-applies the same reposition. Caveat: the
            // clamp target is live state (head_seq/tail_seq), so a retry
            // after bucket eviction may clamp to a later position than the
            // first attempt -- identical to the hazard of any deliberate
            // re-seek, hence still idempotent in shape.
            | Self::SubscriptionSeek(_)
            | Self::AuditSince(_) => true,
        }
    }
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
        /// The responding daemon's own compile-time crate version
        /// (`env!("CARGO_PKG_VERSION")`). Lets a client assert WHICH
        /// build is live. `#[serde(default)]` keeps back-compat: a
        /// legacy daemon omits it, so an empty string means "unknown".
        #[serde(default)]
        version: String,
    },
    PolicyStatus(PolicyStatusResponse),
    SelfCheck(SelfCheckResponse),
    BucketEventsSince(BucketEventsSinceResponse),
    BucketWait(BucketWaitResponse),
    BucketSummary(BucketSummaryResponse),
    EventContext(EventContextResponse),
    CommandStartCombed(CommandStartResponse),
    CommandStatus(CommandStatusResponse),
    CommandStop(CommandStopResponse),
    CommandOutputTail(CommandOutputTailResponse),
    RegistrySearch(RegistrySearchResponse),
    RegistryGet(RegistryGetResponse),
    RegistryUpsert(RegistryUpsertResponse),
    RegistryTest(RegistryTestResponse),
    RegistryActivate(RegistryActivateResponse),
    RegistryImportPack(RegistryImportPackResponse),
    RegistryDeactivate(RegistryDeactivateResponse),
    RegistryDeactivateBulk(RegistryDeactivateBulkResponse),
    RegistryListActive(RegistryListActiveResponse),
    RegistrySuggestFromSamples(RegistrySuggestFromSamplesResponse),
    FileReadWindow(FileReadWindowResponse),
    FileSearch(FileSearchResponse),
    FileListDir(FileListDirResponse),
    FileWrite(FileWriteResponse),
    FileWatchStart(FileWatchStartResponse),
    FileWatchStop(FileWatchStopResponse),
    FileWatchList(FileWatchListResponse),
    PtyCommandStart(PtyCommandStartResponse),
    PtyCommandWriteStdin(PtyCommandWriteStdinResponse),
    PtyCommandStop(PtyCommandStopResponse),
    PtyCommandList(PtyCommandListResponse),
    ShellSessionStart(ShellSessionStartResponse),
    ShellSessionExec(ShellSessionExecResponse),
    ShellSessionStatus(ShellSessionStatusResponse),
    ShellSessionStop(ShellSessionStopResponse),
    ShellSessionList(ShellSessionListResponse),
    WorkspaceSnapshotCreate(WorkspaceSnapshotCreateResponse),
    WorkspaceSnapshotApply(WorkspaceSnapshotApplyResponse),
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

/// The four resolved per-call capabilities (POLICY.md section 4.1).
///
/// Surfaced on [`PolicyStatusResponse`] so an operator can see the ACTIVE caps
/// -- including those preset ON by `full_access` -- without reading TOML.
/// These are the values the policy engine actually evaluates against (base
/// profile `||` `full_access` preset), never the raw config.
// 4 independent opt-in capability flags; a bitfield/enum would hurt the wire/serde surface
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PolicyCapsView {
    /// Gates the `shell_exec` lane (TC49).
    pub allow_shell: bool,
    /// Gates the `shell_session_*` lane (TC50; not yet live).
    pub allow_session: bool,
    /// Gates the Wave-4 privileged helper (not yet live).
    pub allow_privileged: bool,
    /// Gates remote federation / `target_id` (Wave 5; not yet live).
    pub allow_remote: bool,
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
    /// Resolved per-call capabilities (POLICY.md section 4.1). Exposes the
    /// caps the engine evaluates against -- so `full_access` (all preset ON)
    /// and a base profile + `[policy.caps] allow_shell = true` both show the
    /// active set, with no opaque "full_access magic".
    pub caps: PolicyCapsView,
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
    /// F7: the program named in `argv[0]` does not exist (the OS spawn
    /// returned `ErrorKind::NotFound`). A CALLER-fixable command attempt
    /// (typo / wrong PATH / missing binary), NOT a daemon or transport
    /// fault. Surfaced as a structured `program_not_found` receipt at the
    /// MCP boundary (`invalid_params`, carrying `error_kind` + `argv0`)
    /// instead of an opaque `Internal` error so the agent corrects its
    /// argv and keeps routing through Terminal Commander. Distinct from
    /// every other spawn failure, which stays `Internal`.
    ProgramNotFound,
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
    /// `shell_session_*` referenced a `session_id` the daemon does not
    /// know (never started, already reaped, or reset by a daemon
    /// restart). P1 / TC50 (omni spec 001).
    UnknownSession,
    /// `shell_session_exec` (or a snapshot apply) targeted a session that
    /// is not in the `Live` state. The terminal-state guard refuses the
    /// send loudly instead of hanging on a dead shell. P1 / TC50.
    SessionNotLive,
    /// `shell_session_start` was refused because the configured
    /// `max_sessions` cap is already reached. Caller stops a session and
    /// retries. P1 / TC50.
    SessionLimitExceeded,
    /// Returned to a new request that arrives while the daemon is draining for
    /// shutdown. Retryable: the client should cold-spawn a fresh daemon.
    ShuttingDown,
}

/// Structured error payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
    /// F7: the offending `argv[0]` for an [`IpcErrorCode::ProgramNotFound`]
    /// error, carried as a TYPED field rather than recovered by parsing the
    /// human-readable `message`. The message still names the program for logs
    /// and humans, but this field is the authoritative source the MCP boundary
    /// reads to populate the `argv0` data key -- so the receipt survives any
    /// wording change to `message` (including apostrophes in the program name,
    /// which the old prose quote-count parse could not handle). Additive and
    /// `serde(default)`: omitted from the wire when `None`, so every other
    /// error (and any pre-F7 client payload) round-trips unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argv0: Option<String>,
    /// F14: the MCP tool name for an [`IpcErrorCode::UnsupportedPlatform`]
    /// error, carried as a TYPED field so the MCP boundary can name the
    /// caller-routable tool (e.g. `shell_session_start`) in its structured
    /// `unsupported_platform` receipt without parsing the human-readable
    /// `message`. An unsupported platform is a caller-ROUTABLE fact (route to
    /// WSL / a different tool), not a server fault, so the receipt must say
    /// WHICH tool is unavailable here. Additive and `serde(default)`: omitted
    /// from the wire when `None`, so every other error round-trips unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
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
            argv0: None,
            tool: None,
        }
    }

    /// F7: construct a [`IpcErrorCode::ProgramNotFound`] error that carries the
    /// offending `argv[0]` as a TYPED field. The `message` is still
    /// human/log-facing (and may name the program however it likes), but the
    /// `argv0` field -- not the prose -- is the authoritative value the MCP
    /// boundary reads into the structured `program_not_found` receipt.
    #[must_use]
    pub fn program_not_found(argv0: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: IpcErrorCode::ProgramNotFound,
            message: message.into(),
            argv0: Some(argv0.into()),
            tool: None,
        }
    }

    /// F14: construct an [`IpcErrorCode::UnsupportedPlatform`] error that
    /// carries the offending MCP `tool` name as a TYPED field. The `message`
    /// stays human/log-facing, but the `tool` field -- not the prose -- is the
    /// authoritative value the MCP boundary reads into the structured
    /// `unsupported_platform` receipt. An unsupported platform is a
    /// caller-ROUTABLE fact (route to WSL / a different tool), so naming the
    /// unavailable tool keeps the agent reasoning instead of abandoning TC.
    #[must_use]
    pub fn unsupported_platform(tool: &str, message: impl Into<String>) -> Self {
        Self {
            code: IpcErrorCode::UnsupportedPlatform,
            message: message.into(),
            argv0: None,
            tool: Some(tool.to_owned()),
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
            argv0: None,
            tool: None,
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
    /// NOW OPTIONAL (US5 / FR-040). Supplied: exactly today's
    /// single-bucket resolution (event absent from that bucket =
    /// `EventNotFound`, so a contradicting `bucket_id` is an error, never
    /// silently ignored). Absent: the daemon resolves the owning bucket
    /// by scanning in-scope buckets for the globally-unique `event_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_id: Option<BucketId>,
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

/// Serde default for `bool` fields that default to `true` (e.g.
/// `CommandStartParams::strip_ansi`). A bare `#[serde(default)]` would
/// yield `false`, inverting the intended TC-B1 default; this helper keeps
/// an omitted field meaning "strip on".
const fn default_true() -> bool {
    true
}

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
    /// Strip ANSI/CSI/OSC escapes before sifter matching and in emitted
    /// summaries (TC-B1, FR-026). RAW bytes are always preserved in the
    /// frame store; this affects ONLY what the sifter sees and echoes.
    /// Defaults to `true`. Additive and non-breaking: old clients omit it
    /// and decode via the `default_true` serde default.
    #[serde(default = "default_true")]
    pub strip_ansi: bool,
    /// Optional in-flight dedup hint (TC-2). A client that MAY re-send
    /// the same logical start (e.g. a transport retry of a mutating
    /// `command_start_combed`) SHOULD send the SAME nonce on every
    /// re-send; the daemon collapses an in-flight duplicate to the SAME
    /// `(job_id, bucket_id)` instead of spawning twice. Two DISTINCT
    /// logical starts MUST use distinct nonces (or none). Additive and
    /// non-breaking: old clients omit it and decode via serde default;
    /// the daemon then falls back to a very short peer-scoped
    /// signature window. This is NOT a server-honored idempotency-key
    /// protocol (no TTL store, no envelope change) -- just an in-flight
    /// collapse hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_nonce: Option<String>,
}

impl CommandStartParams {
    /// Resolve the effective grace `Duration`, clamping to the cap.
    #[must_use]
    pub fn grace(&self) -> Option<Duration> {
        self.grace_ms
            .map(|ms| Duration::from_millis(ms.min(MAX_COMMAND_GRACE_MS)))
    }
}

/// Wire shape for `shell_exec` (TC49).
///
/// The shell lane runs ONE shell
/// line (pipelines / compounds / redirects) through the comb pipeline
/// behind the `allow_shell` capability. Mirrors the daemon's
/// `ShellExecRequest`; carries the dedicated `shell_line` ONLY — there
/// is NO capability flag on the wire (caps are config/TOML, never
/// MCP-flippable). Denied by default.
///
/// `wait_ms` is deliberately ABSENT here: like `command_start_combed`,
/// the bounded-wait control is an MCP-layer concern (`McpShellExecParams`
/// strips it before building the IPC start), never forwarded into the
/// IPC start params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellExecParams {
    /// The shell line to run. Becomes `argv[2]` of `[shell, "-lc",
    /// shell_line]`; bounded by the daemon's `MAX_SHELL_LINE_BYTES`.
    pub shell_line: String,
    /// Interpreter override. `None` -> the daemon's `default_shell`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Working directory for the spawned child. `None` inherits the
    /// daemon's cwd (the policy gate may still reject paths outside the
    /// project root on containment profiles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Explicit environment for the child. Empty means inherit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<(String, String)>,
    /// Optional inline rule set to comb this job's output. Empty means
    /// the daemon's empty sifter (no events emitted).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RuleDefinition>,
    /// Bucket configuration override (max_events / TTL). Defaults
    /// applied if None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_config: Option<BucketConfig>,
    /// Optional per-bucket tag for subscription routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
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

/// Maximum sample lines accepted by `registry_suggest_from_samples`.
///
/// US2 / FR-007. Lines beyond this are ignored before the heuristics
/// run so a huge sample set cannot blow the bounded-output budget.
pub const MAX_SUGGEST_SAMPLES: usize = 200;
/// Maximum bytes inspected per suggestion sample line.
pub const MAX_SUGGEST_SAMPLE_BYTES: usize = 4096;
/// Hard cap on the number of proposed rules returned by
/// `registry_suggest_from_samples`, regardless of the caller's
/// `max_rules`.
pub const MAX_SUGGEST_PROPOSED_RULES: usize = 8;

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
    /// F8b (trust): `sample_index` values whose text the rule's regex
    /// WOULD match, but whose stream the rule's `stream` filter
    /// excludes -- so the rule produced no match for a reason invisible
    /// in the sample text alone. Empty when no sample is a stream
    /// mismatch. Additive: omitted from the wire when empty so older
    /// clients keep the historical shape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stream_mismatches: Vec<usize>,
}

/// `registry_suggest_from_samples` parameters (US2 / FR-007).
///
/// PURE heuristic suggestion: the daemon runs deterministic line-shape
/// detectors over `samples` and returns DRAFT rule proposals. It NEVER
/// activates or persists anything (FR-008 / constitution VII).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySuggestFromSamplesParams {
    /// Raw output sample lines to analyze. Capped at
    /// [`MAX_SUGGEST_SAMPLES`]; per-line bytes capped at
    /// [`MAX_SUGGEST_SAMPLE_BYTES`].
    pub samples: Vec<String>,
    /// Optional free-text hint describing the tool/intent. Advisory
    /// only; the heuristics are deterministic and ignore it for
    /// matching (it is echoed back for the caller's context).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    /// Optional cap on proposals. Clamped to
    /// [`MAX_SUGGEST_PROPOSED_RULES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rules: Option<u32>,
}

/// `registry_suggest_from_samples` response (US2 / FR-007).
///
/// `proposed_rules` are DRAFT [`RuleDefinition`]s. They are NOT active
/// and NOT persisted: the caller must run the explicit
/// `registry_test` -> `registry_upsert` -> `registry_activate` loop
/// named in `next_steps`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySuggestFromSamplesResponse {
    /// Candidate DRAFT rules. May be empty for low-signal input.
    pub proposed_rules: Vec<RuleDefinition>,
    /// Always `"heuristic"`. The suggestions are deterministic
    /// line-shape heuristics, never an ML score.
    pub confidence: String,
    /// The explicit, ordered activation loop the caller MUST follow to
    /// make any proposal live. Constant by design.
    pub next_steps: Vec<String>,
    /// Human-readable explanation of what was (or was not) detected.
    /// For empty/low-signal input this explains why no rule was
    /// proposed instead of fabricating one.
    pub explanation: String,
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
    /// Other versions of the SAME rule id that were active under this
    /// scope and were closed by this activation (S5 activate-supersedes:
    /// version stacking within one scope fires duplicate events per
    /// frame, so activating vN deactivates the rest). Empty when nothing
    /// was superseded. `serde(default)` keeps pre-S5 payloads decodable.
    #[serde(default)]
    pub superseded_versions: Vec<u32>,
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

/// `registry_deactivate_bulk` parameters (US2 / FR-011).
///
/// Deactivate an entire seed pack OR an explicit list of rule ids in ONE
/// call, under exactly ONE scope. Exactly one of `pack` / `rule_ids`
/// must be present; the daemon rejects zero or both with a teaching
/// error. The single-rule [`RegistryDeactivateParams`] wire contract is
/// untouched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryDeactivateBulkParams {
    /// Selector 1: deactivate every member of this seed pack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<String>,
    /// Selector 2: deactivate these rule ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_ids: Option<Vec<String>>,
    /// Required. ONE scope per call.
    pub scope: ActivationScope,
}

/// The disposition of one rule in a bulk deactivate (US2 / FR-011).
///
/// Partial success is the NORMAL shape: `not_active` and `unknown_rule`
/// are reported per-rule, never as a call-level error.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BulkOutcomeKind {
    /// At least one active version under the scope was closed.
    Deactivated,
    /// Known rule, but nothing was open under this scope.
    NotActive,
    /// Rule id not in the registry (or not a member of the named pack).
    UnknownRule,
}

/// One per-rule outcome entry in a bulk deactivate response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BulkDeactivateOutcome {
    pub rule_id: String,
    /// The version acted on; `None` for `unknown_rule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    pub outcome: BulkOutcomeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryDeactivateBulkResponse {
    /// One entry per requested rule, ALWAYS, in request order (pack
    /// order = pack-file order).
    pub outcomes: Vec<BulkDeactivateOutcome>,
    /// Live jobs rebound ONCE after the whole loop.
    pub jobs_rebound: u64,
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
/// Hard cap on entries returned by `file_list_dir` in a single call.
///
/// (US3 FR-020.) Bounds a single-level directory listing the same way the
/// read-window / search lanes bound their output: a directory with more than
/// this many entries is returned truncation-flagged with the true total, never
/// silently partial (Constitution III).
pub const MAX_FILE_LIST_ENTRIES: usize = 500;
/// Default `file_list_dir` entry cap when the caller omits `max_entries`.
pub const DEFAULT_FILE_LIST_ENTRIES: usize = 200;
/// Hard cap on the content size accepted by a single `file_write` call.
///
/// (TC22 A3.) Bounds the request the same way the read window / search
/// scan budgets bound their lanes: a write larger than this is rejected
/// with [`IpcErrorCode::OversizedRequest`] before any filesystem touch,
/// so a single tool call can never be coerced into writing an unbounded
/// blob.
///
/// MUST stay comfortably below [`MAX_FRAME_BYTES`] (256 KiB): the content
/// is carried inline in the request frame as escaped JSON, so the cap is
/// 192 KiB to leave ~64 KiB headroom for the path, field names, and worst-
/// case JSON string escaping. This guarantees the dedicated
/// `OversizedRequest` verdict fires at the handler, rather than a generic
/// transport `FrameTooLarge`, giving the caller an actionable, lane-specific
/// error. (Larger files are written as multiple bounded calls.)
pub const MAX_FILE_WRITE_BYTES: usize = 192 * 1024;

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

/// Kind of a single directory entry, from `symlink_metadata` (never
/// followed): a symlink is reported as `symlink` regardless of its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirEntryKind {
    File,
    Dir,
    Symlink,
}

/// One entry returned by `file_list_dir` (US3). The discovery unit of the
/// files facade: a single-level entry, never recursed into and, for a
/// symlink/reparse point, never followed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    /// Entry file name only (no path component).
    pub name: String,
    /// `file` / `dir` / `symlink`, taken from `symlink_metadata`.
    pub kind: DirEntryKind,
    /// Size in bytes for regular files only; omitted for dirs/symlinks and
    /// when the entry vanished between enumeration and stat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// Modification time in milliseconds since the Unix epoch; omitted when
    /// unavailable (stat race or platform without an mtime).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_ms: Option<i64>,
}

/// `file_list_dir` parameters (US3 FR-020).
///
/// Single-level listing of one directory. Absolute path required (the daemon
/// has no workspace root); gated by the SAME read-path policy as
/// `file_read_window` (FR-021).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileListDirParams {
    /// Absolute path of the directory to list.
    pub path: String,
    /// Cap on returned entries; clamped to
    /// `[1, MAX_FILE_LIST_ENTRIES]`, default `DEFAULT_FILE_LIST_ENTRIES`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileListDirResponse {
    /// Canonicalized directory that was listed.
    pub path: String,
    /// Sorted: dirs first, then files/symlinks together; each group
    /// lexicographic by `name`.
    pub entries: Vec<DirEntry>,
    /// Total entries present in the directory (`>= entries.len()`).
    pub total_entries: u64,
    /// `true` iff `total_entries > entries.len()` (the cap clamped the list).
    pub truncated: bool,
}

/// `file_write` parameters (TC22 A3).
///
/// Writes `content` to `path` as a single UTF-8 regular file. The daemon
/// canonicalizes the PARENT directory (the target file need not exist
/// yet), policy-gates the canonical target against `paths.write_allow`,
/// audits BEFORE the write, bounds `content` to [`MAX_FILE_WRITE_BYTES`],
/// and writes ATOMICALLY (temp file in the same dir + rename) so a partial
/// or torn write can never be observed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWriteParams {
    /// Absolute path to the target file. Absolute is required: the daemon
    /// has no workspace root, so a relative path is rejected rather than
    /// resolved against the daemon's working directory.
    pub path: std::path::PathBuf,
    /// UTF-8 content to write. Bounded by [`MAX_FILE_WRITE_BYTES`]; an
    /// oversize payload is rejected before any filesystem touch.
    pub content: String,
    /// Create missing parent directories WITHIN an allowed path. The
    /// parent must still pass policy: `create_dirs` never widens the
    /// allow-list, it only saves a separate mkdir for a path the policy
    /// already permits. Defaults to false.
    #[serde(default)]
    pub create_dirs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWriteResponse {
    /// Canonical path that was written.
    pub path: std::path::PathBuf,
    /// Number of content bytes written.
    pub bytes_written: u64,
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
    /// NEW (US5 / FR-041): bucket cursor to read the settle window from
    /// (default `0` = the PTY job bucket head). Only meaningful with
    /// `wait_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    /// NEW (US5 / FR-041): bounded settle window (ms) to wait for combed
    /// signals AFTER the write, clamped server-side like the
    /// `shell_session_exec` settle window. Absent = immediate return
    /// (today's byte-identical behavior).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyCommandWriteStdinResponse {
    pub job_id: JobId,
    pub bytes_written: u64,
    /// Echoes the post-write secret-prompt-active flag so the LLM
    /// can avoid a follow-up write that would also be rejected.
    pub secret_prompt_active: bool,
    /// NEW (US5 / FR-041): the following combed-batch fields are present
    /// ONLY when `wait_ms` was supplied on the request. A no-wait
    /// response omits every one of them, serializing byte-identically to
    /// the pre-US5 shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_in: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<SignalEvent>>,
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
// P1 (TC50): persistent shell sessions + workspace snapshots.
//
// A session is a long-lived login-shell PTY job: sticky cwd/env come
// for free from the persistent shell process. The wire mirrors the
// PTY surface in style. Session output is ALWAYS combed (read from the
// session bucket via cursor); the wire never carries a raw stream.
// Session start is gated by `PolicyAction::SessionStart` behind the
// `allow_session` capability (default deny) and audited before spawn.
// =====================================================================

/// Maximum byte length of a single `shell_session_exec` line.
///
/// A session line is written to the shell PTY as `line + "\n"`. Bounded
/// well under the PTY stdin cap (`MAX_PTY_STDIN_BYTES`) so the appended
/// newline can never push a max-length line over the probe's write cap.
pub const MAX_SESSION_LINE_BYTES: usize = 4000;

/// Maximum number of `(key, value)` pairs returned in a session/snapshot
/// bounded env snapshot. Keeps the status/list/snapshot responses small.
pub const MAX_SESSION_ENV_ITEMS: usize = 256;

/// Lifecycle state of a shell session.
///
/// Mirrors the data-model `Starting | Live | Exited | Failed` set. `Live`
/// is the only state in which `shell_session_exec` is accepted; a send to
/// any other state fails loudly (terminal-state guard), never hangs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Starting,
    Live,
    Exited,
    Failed,
}

/// `shell_session_start` request.
///
/// `shell` is the interpreter override (`None` -> the daemon's default
/// login shell); argv[0] is NOT a user-chosen interpreter on this lane (it
/// is assembled by the daemon), so the argv shell-interpreter guard is
/// intentionally skipped here and the `SessionStart` cap gates instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionStartParams {
    /// Interpreter override. `None` -> the daemon's default login shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Initial working directory for the session shell. `None` inherits
    /// the daemon's cwd (the policy gate still applies on containment
    /// profiles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Environment overlay applied to the session shell. Bounded by
    /// [`MAX_SESSION_ENV_ITEMS`]. Empty = inherit unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<(String, String)>,
    /// Optional inline rules bound to the session bucket so the session's
    /// combed output emits structured signals. Empty = the daemon's empty
    /// sifter (only lifecycle events appear).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RuleDefinition>,
    /// Bucket configuration override (max_events / TTL). Defaults applied
    /// if `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_config: Option<BucketConfig>,
    /// Optional per-bucket tag for subscription routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// `shell_session_start` response: the stable session id, its signal
/// bucket, and the initial lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionStartResponse {
    pub session_id: SessionId,
    pub bucket_id: BucketId,
    pub state: SessionState,
}

/// `shell_session_exec` request: send ONE line to a live session shell.
///
/// The line is bounded by [`MAX_SESSION_LINE_BYTES`]; a trailing newline
/// is appended by the daemon (do NOT include it). Output is read back as
/// combed signals from the session bucket via the cursor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionExecParams {
    pub session_id: SessionId,
    /// The shell line to run (no trailing newline). Bounded by
    /// [`MAX_SESSION_LINE_BYTES`].
    pub line: String,
    /// Cursor into the session bucket to read combed signals from. Omit /
    /// `0` to read from the bucket head. The response returns the next
    /// cursor for the following exec.
    #[serde(default)]
    pub cursor: u64,
    /// Bounded wait (ms) for combed signals to appear after the line is
    /// written, clamped server-side to [`MAX_BUCKET_WAIT_MS`]. Omit for
    /// the default settle window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
}

/// `shell_session_exec` response: combed signals appended to the session
/// bucket after the line ran, plus the next cursor. Never a raw stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionExecResponse {
    pub session_id: SessionId,
    pub bucket_id: BucketId,
    pub bytes_written: u64,
    pub cursor_in: u64,
    pub next_cursor: u64,
    pub has_more: bool,
    pub dropped_count: u64,
    pub events: Vec<SignalEvent>,
}

/// `shell_session_status` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionStatusParams {
    pub session_id: SessionId,
}

/// `shell_session_status` response: lifecycle state, current cwd, a
/// bounded env snapshot, and the last-active timestamp (epoch seconds).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionStatusResponse {
    pub session_id: SessionId,
    pub bucket_id: BucketId,
    pub state: SessionState,
    /// Best-known current working directory. Tracked from the requested
    /// start cwd and from `cd`-shaped session lines (see SHELL_SESSION
    /// runtime docs); a value the daemon has not observed reports the
    /// start cwd. Bounded.
    pub cwd: Option<String>,
    /// Bounded env snapshot captured at start (overlay entries), capped at
    /// [`MAX_SESSION_ENV_ITEMS`].
    pub env_snapshot: Vec<(String, String)>,
    /// Seconds since the unix epoch of the last exec/status touch.
    pub last_active_at: u64,
}

/// `shell_session_stop` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionStopParams {
    pub session_id: SessionId,
}

/// `shell_session_stop` response. The session shell is terminated
/// (graceful then forced) and the state moves to [`SessionState::Exited`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionStopResponse {
    pub session_id: SessionId,
    pub state: SessionState,
    /// Short bounded reason for the terminal transition (e.g. "stopped"
    /// or "already terminal").
    pub terminal_reason: String,
}

/// One entry in `shell_session_list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionListEntry {
    pub session_id: SessionId,
    pub bucket_id: BucketId,
    pub state: SessionState,
    pub cwd: Option<String>,
    pub last_active_at: u64,
}

/// `shell_session_list` response: a bounded snapshot of live sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionListResponse {
    pub sessions: Vec<ShellSessionListEntry>,
}

/// `workspace_snapshot_create` request: persist the current cwd + bounded
/// env of a session as a restorable workspace snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshotCreateParams {
    pub session_id: SessionId,
    /// Optional human-friendly label stored alongside the snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `workspace_snapshot_create` response: the new snapshot id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshotCreateResponse {
    pub snapshot_id: String,
}

/// `workspace_snapshot_apply` request: restore a snapshot's cwd/env into
/// the given (live) session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshotApplyParams {
    pub snapshot_id: String,
    pub session_id: SessionId,
}

/// `workspace_snapshot_apply` response: the restored cwd echoed back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshotApplyResponse {
    pub applied: bool,
    pub session_id: SessionId,
    pub cwd: Option<String>,
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
    /// Optional per-bucket tag lifted from the bucket source. `#[serde(default,
    /// skip_serializing_if)]` keeps the wire additive: old payloads decode as None,
    /// old daemons omit it, new clients see None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Bounded, REDACTED argv head (program + up to 2 tokens), with secret spans
    /// masked to `<redacted>` and each item truncated to 128 bytes. None when the
    /// source kind carries no argv. Additive: `#[serde(default, skip_serializing_if)]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argv_head: Option<Vec<String>>,
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

    /// F7: the typed `argv0` field is the authoritative carrier for a
    /// `ProgramNotFound` error and must survive a serde round-trip exactly,
    /// independent of the human-readable `message`. The MCP boundary reads
    /// THIS field (not the prose), so the round-trip is the contract.
    #[test]
    fn program_not_found_argv0_survives_wire_round_trip() {
        // A program name with an embedded apostrophe -- the exact case the old
        // prose quote-count parse could not recover. The typed field carries it
        // verbatim regardless of how the message is worded.
        let e = IpcError::program_not_found("my'prog", "program not found: 'my'prog'.");
        assert_eq!(e.code, IpcErrorCode::ProgramNotFound);
        assert_eq!(e.argv0.as_deref(), Some("my'prog"));

        let json = serde_json::to_string(&e).unwrap();
        let back: IpcError = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.argv0.as_deref(),
            Some("my'prog"),
            "the typed argv0 must survive serialization verbatim"
        );
        assert_eq!(back.code, IpcErrorCode::ProgramNotFound);
        assert_eq!(back.message, e.message);
    }

    /// F7: `argv0` is additive and `skip_serializing_if = "Option::is_none"`,
    /// so a non-ProgramNotFound error (the common case) must OMIT the `argv0`
    /// key from the wire entirely -- no key, not `null`. This pins the
    /// skip_serializing_if contract so every other error round-trips unchanged
    /// and pre-F7 clients see no new field.
    #[test]
    fn argv0_key_is_omitted_from_json_when_none() {
        let e = IpcError::new(IpcErrorCode::PathDenied, "nope");
        assert!(e.argv0.is_none());
        let value: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert!(
            value.get("argv0").is_none(),
            "the argv0 key must be absent (not null) when None; got: {value}"
        );
        // And the typical constructors default it to None.
        assert!(IpcError::transport("x").argv0.is_none());
    }

    // Source-status: test-only. TC-2: the dedup_nonce field is additive and
    // serde(default), so an OLD client payload that omits it must still
    // decode, and a payload carrying Some(nonce) must round-trip unchanged.
    #[test]
    fn command_start_params_dedup_nonce_is_optional_and_round_trips() {
        // Old-client payload: no dedup_nonce key at all. Must decode via the
        // serde default to None (additive, non-breaking).
        let old_payload = r#"{"argv":["true"]}"#;
        let decoded: CommandStartParams =
            serde_json::from_str(old_payload).expect("old payload without dedup_nonce must decode");
        assert_eq!(
            decoded.dedup_nonce, None,
            "absent dedup_nonce must default to None"
        );

        // Present nonce: serialize -> decode preserves it exactly.
        let with_nonce = CommandStartParams {
            environment: None,
            argv: vec!["true".to_owned()],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace_ms: None,
            tag: None,
            dedup_nonce: Some("mcp-1234-7".to_owned()),
            strip_ansi: true,
        };
        let json = serde_json::to_string(&with_nonce).expect("serialize ok");
        assert!(
            json.contains("dedup_nonce"),
            "a Some(nonce) must serialize the field; json: {json}"
        );
        let back: CommandStartParams = serde_json::from_str(&json).expect("decode ok");
        assert_eq!(back.dedup_nonce, Some("mcp-1234-7".to_owned()));

        // skip_serializing_if: a None nonce must NOT appear on the wire, so an
        // old daemon never sees an unexpected key.
        let without = CommandStartParams {
            dedup_nonce: None,
            strip_ansi: true,
            ..with_nonce
        };
        let json_none = serde_json::to_string(&without).expect("serialize ok");
        assert!(
            !json_none.contains("dedup_nonce"),
            "a None nonce must be omitted from the wire; json: {json_none}"
        );
    }

    // TC-4 Phase 4a: the ProbeListEntry `tag` and `argv_head` fields are
    // additive and `#[serde(default, skip_serializing_if)]`. An OLD payload
    // that omits both must decode (fields default to None), and a daemon that
    // has neither set must omit both keys from the wire (skip_serializing_if).
    #[test]
    fn probe_list_entry_tag_and_argv_head_are_optional_and_additive() {
        use terminal_commander_core::{BucketId, JobId, ProbeId};

        // Build a fully-populated entry, then serialize it and strip the new
        // keys to reconstruct an OLD-daemon payload deterministically (the typed
        // ids serialize as `<prefix>_<hex>`, not raw UUIDs, so we cannot hand-
        // write the wire form). This also exercises the live wire prefixes.
        let populated = ProbeListEntry {
            kind: ProbeKind::Command,
            job_id: JobId::new(),
            bucket_id: BucketId::new(),
            probe_id: ProbeId::new(),
            frames_total: 0,
            events_emitted: 0,
            frames_suppressed: 0,
            frames_suppressed_progress: 0,
            frames_suppressed_dedupe: 0,
            secret_prompts_total: 0,
            secret_prompt_active: false,
            path: None,
            liveness: Liveness::default(),
            tag: Some("prod".to_owned()),
            argv_head: Some(vec!["curl".to_owned(), "<redacted>".to_owned()]),
        };

        // Present values round-trip exactly.
        let json_full = serde_json::to_string(&populated).expect("encode populated");
        let back: ProbeListEntry =
            serde_json::from_str(&json_full).expect("populated payload round-trips");
        assert_eq!(back.tag.as_deref(), Some("prod"));
        assert_eq!(
            back.argv_head,
            Some(vec!["curl".to_owned(), "<redacted>".to_owned()])
        );

        // skip_serializing_if: a None tag/argv_head must NOT appear on the wire,
        // so an old daemon and a new client agree on the encoded shape.
        let none_entry = ProbeListEntry {
            tag: None,
            argv_head: None,
            ..populated
        };
        let json_none = serde_json::to_string(&none_entry).expect("encode none");
        assert!(
            !json_none.contains("\"tag\""),
            "None tag must be omitted from the wire (skip_serializing_if)"
        );
        assert!(
            !json_none.contains("\"argv_head\""),
            "None argv_head must be omitted from the wire (skip_serializing_if)"
        );

        // Old-daemon payload shape: the encoded none_entry already omits both
        // keys, so decoding it proves an absent tag/argv_head defaults to None.
        let decoded: ProbeListEntry = serde_json::from_str(&json_none)
            .expect("old payload without tag/argv_head must decode");
        assert_eq!(decoded.tag, None, "absent tag must default to None");
        assert_eq!(
            decoded.argv_head, None,
            "absent argv_head must default to None"
        );
    }

    // Source-status: test-only. Verifies the retry-safety classification on
    // `IpcRequest::is_idempotent`, which gates the MCP daemon-client retry so a
    // transport-failed MUTATING RPC (e.g. a >5s-spawning CommandStartCombed) is
    // never blindly re-sent. The list below is exhaustive over the enum; the
    // `is_idempotent` match itself is wildcard-free so a new variant forces a
    // deliberate classification before this test can even be updated.
    #[test]
    #[allow(clippy::too_many_lines)] // one table row per IpcRequest variant
    fn is_idempotent_classifies_every_request_variant() {
        use terminal_commander_core::{JobId, RuleDefinition};

        // A minimal valid RuleDefinition for the two rule-carrying registry
        // variants. Deserialized rather than hand-built so this test stays
        // decoupled from the full struct shape (it only needs *a* value).
        fn sample_rule() -> RuleDefinition {
            serde_json::from_str(
                r#"{
                    "id": "r",
                    "version": 1,
                    "kind": "keyword",
                    "severity": "low",
                    "event_kind": "k",
                    "keywords": ["x"],
                    "summary_template": "s"
                }"#,
            )
            .expect("sample rule deserializes")
        }

        let predicate = SubscriptionPredicate {
            severity_min: None,
            kind: None,
            sources: SubscriptionSourceSel::All,
            tag: None,
        };

        // (variant, expected is_idempotent). MUTATING = false (unsafe to blind
        // re-send), READ / idempotent-reposition = true.
        let cases: Vec<(IpcRequest, bool)> = vec![
            // ---- mutating: must be false ----
            (
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: vec!["x".to_owned()],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: None,
                    tag: None,
                    dedup_nonce: None,
                    strip_ansi: true,
                }),
                false,
            ),
            (
                IpcRequest::ShellExec(ShellExecParams {
                    shell_line: "echo a | wc -c".to_owned(),
                    shell: None,
                    cwd: None,
                    env: vec![],
                    rules: vec![],
                    bucket_config: None,
                    tag: None,
                }),
                false,
            ),
            (
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    argv: vec!["x".to_owned()],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    rows: None,
                    cols: None,
                    tag: None,
                }),
                false,
            ),
            (
                IpcRequest::PtyCommandWriteStdin(PtyCommandWriteStdinParams {
                    job_id: JobId::new(),
                    bytes: "x".to_owned(),
                    cursor: None,
                    wait_ms: None,
                }),
                false,
            ),
            (
                IpcRequest::PtyCommandStop(PtyCommandStopParams {
                    job_id: JobId::new(),
                }),
                false,
            ),
            (
                IpcRequest::CommandStop(CommandStopParams {
                    job_id: JobId::new(),
                }),
                false,
            ),
            (
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: sample_rule(),
                }),
                false,
            ),
            (
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "r".to_owned(),
                    version: None,
                    scope: None,
                }),
                false,
            ),
            (
                IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
                    rule_id: "r".to_owned(),
                    version: 1,
                    scope: None,
                }),
                false,
            ),
            (
                IpcRequest::RegistryImportPack(RegistryImportPackParams {
                    pack: "p".to_owned(),
                    activate: false,
                    scope: None,
                }),
                false,
            ),
            (
                // File WRITE (TC22 A3): MUTATING -- a blind retry double-writes,
                // so is_idempotent MUST be false. Classified with the other
                // file-state mutators, NOT with FileReadWindow / FileSearch.
                IpcRequest::FileWrite(FileWriteParams {
                    path: "x".into(),
                    content: "data".to_owned(),
                    create_dirs: false,
                }),
                false,
            ),
            (
                IpcRequest::FileWatchStart(FileWatchStartParams {
                    path: "x".into(),
                    bucket_config: None,
                    rules: vec![],
                    follow_from_beginning: None,
                    tag: None,
                }),
                false,
            ),
            (
                IpcRequest::FileWatchStop(FileWatchStopParams {
                    watch_id: JobId::new(),
                }),
                false,
            ),
            (
                IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                    predicate: predicate.clone(),
                }),
                false,
            ),
            (
                IpcRequest::SubscriptionClose(SubscriptionCloseParams {
                    sub_id: "s".to_owned(),
                }),
                false,
            ),
            (
                IpcRequest::SubscriptionPull(SubscriptionPullParams {
                    sub_id: "s".to_owned(),
                    max: None,
                    timeout_ms: None,
                }),
                false,
            ),
            (IpcRequest::Shutdown, false),
            // ---- reads / idempotent repositioning: must be true ----
            (IpcRequest::Health, true),
            (IpcRequest::SystemDiscover, true),
            (IpcRequest::PolicyStatus, true),
            (IpcRequest::SelfCheck, true),
            (
                IpcRequest::CommandStatus(CommandStatusParams {
                    job_id: JobId::new(),
                }),
                true,
            ),
            (
                IpcRequest::CommandOutputTail(CommandOutputTailParams {
                    job_id: JobId::new(),
                    max_lines: 10,
                    max_bytes: 1024,
                }),
                true,
            ),
            (
                IpcRequest::BucketWait(BucketWaitParams {
                    bucket_id: BucketId::new(),
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: None,
                    timeout_ms: None,
                }),
                true,
            ),
            (
                IpcRequest::BucketEventsSince(BucketEventsSinceParams {
                    bucket_id: BucketId::new(),
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: None,
                }),
                true,
            ),
            (
                IpcRequest::BucketSummary(BucketSummaryParams {
                    bucket_id: BucketId::new(),
                }),
                true,
            ),
            (
                IpcRequest::EventContext(EventContextParams {
                    bucket_id: Some(BucketId::new()),
                    event_id: EventId::new(),
                    before: None,
                    after: None,
                    max_bytes: None,
                }),
                true,
            ),
            (
                IpcRequest::RuntimeState(ListLimitParams { limit: None }),
                true,
            ),
            (IpcRequest::ProbeList(ListLimitParams { limit: None }), true),
            (
                IpcRequest::ProbeStatus(ProbeStatusParams {
                    probe_id: terminal_commander_core::ProbeId::new(),
                }),
                true,
            ),
            (IpcRequest::PtyCommandList, true),
            (
                IpcRequest::FileReadWindow(FileReadWindowParams {
                    path: "x".into(),
                    start_line: None,
                    max_lines: None,
                    max_bytes: None,
                }),
                true,
            ),
            (
                IpcRequest::FileSearch(FileSearchParams {
                    path: "x".into(),
                    query: "q".to_owned(),
                    case_insensitive: None,
                    max_matches: None,
                    max_snippet_bytes: None,
                }),
                true,
            ),
            (
                // Directory listing (US3): a pure bounded read, replayable.
                IpcRequest::FileListDir(FileListDirParams {
                    path: "/x".to_owned(),
                    max_entries: None,
                }),
                true,
            ),
            (IpcRequest::FileWatchList, true),
            (
                IpcRequest::RegistrySearch(RegistrySearchParams {
                    query: "q".to_owned(),
                    limit: None,
                }),
                true,
            ),
            (
                IpcRequest::RegistryGet(RegistryGetParams {
                    rule_id: "r".to_owned(),
                    version: None,
                }),
                true,
            ),
            (
                IpcRequest::RegistryTest(RegistryTestParams {
                    rule_id: "r".to_owned(),
                    version: None,
                    samples: vec![],
                }),
                true,
            ),
            (
                IpcRequest::RegistryListActive(ListLimitParams { limit: None }),
                true,
            ),
            (
                IpcRequest::SubscriptionList(SubscriptionListParams { limit: None }),
                true,
            ),
            (
                IpcRequest::SubscriptionSeek(SubscriptionSeekParams {
                    sub_id: "s".to_owned(),
                    bucket_id: BucketId::new(),
                    seq: 0,
                }),
                true,
            ),
            (
                IpcRequest::AuditSince(AuditSinceParams {
                    cursor: 0,
                    action_filter: None,
                    decision_filter: None,
                    limit: None,
                }),
                true,
            ),
        ];

        for (req, expected) in &cases {
            assert_eq!(
                req.is_idempotent(),
                *expected,
                "is_idempotent classification wrong for {req:?}"
            );
        }

        // Spot-check the two highest-harm classifications explicitly so a
        // regression names the exact defect.
        assert!(
            !IpcRequest::CommandStartCombed(CommandStartParams {
                environment: None,
                argv: vec!["sleep".to_owned(), "10".to_owned()],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                grace_ms: None,
                tag: None,
                dedup_nonce: None,
                strip_ansi: true,
            })
            .is_idempotent(),
            "CommandStartCombed must be non-idempotent: a blind retry double-spawns"
        );
        assert!(
            !IpcRequest::SubscriptionPull(SubscriptionPullParams {
                sub_id: "s".to_owned(),
                max: None,
                timeout_ms: None,
            })
            .is_idempotent(),
            "SubscriptionPull must be non-idempotent: per-consumer offsets are \
             committed server-side inside the pull (subscriptions/pull.rs 543/633) \
             before the response, so a lost-then-retried pull drops already-drained \
             events -- it is NOT a replayable read like BucketWait"
        );
        assert!(
            !IpcRequest::SubscriptionOpen(SubscriptionOpenParams { predicate }).is_idempotent(),
            "SubscriptionOpen must be non-idempotent: a blind retry mints a second \
             sub_id + registry slot, leaking a slot and risking \
             SubscriptionLimitExceeded"
        );
    }

    /// US3 (W1): the `file_list_dir` response wire shape is the pinned one --
    /// `DirEntryKind` renders snake_case (`file`/`dir`/`symlink`), and the
    /// per-entry `size_bytes`/`mtime_ms` optionals are omitted when `None`
    /// (`skip_serializing_if`). Round-trips back into the same variant.
    #[test]
    fn file_list_dir_response_wire_shape_is_pinned() {
        let resp = IpcResponse::FileListDir(FileListDirResponse {
            path: "/abs/dir".to_owned(),
            entries: vec![
                DirEntry {
                    name: "sub".to_owned(),
                    kind: DirEntryKind::Dir,
                    size_bytes: None,
                    mtime_ms: Some(1),
                },
                DirEntry {
                    name: "f.txt".to_owned(),
                    kind: DirEntryKind::File,
                    size_bytes: Some(3),
                    mtime_ms: None,
                },
                DirEntry {
                    name: "link".to_owned(),
                    kind: DirEntryKind::Symlink,
                    size_bytes: None,
                    mtime_ms: None,
                },
            ],
            total_entries: 3,
            truncated: false,
        });
        let json = serde_json::to_value(&resp).expect("serialize");
        assert_eq!(json["method"], "file_list_dir");
        // snake_case kinds.
        assert_eq!(json["entries"][0]["kind"], "dir");
        assert_eq!(json["entries"][1]["kind"], "file");
        assert_eq!(json["entries"][2]["kind"], "symlink");
        // Absent optionals are omitted, not rendered as null.
        assert!(
            json["entries"][0].get("size_bytes").is_none(),
            "dir omits size_bytes"
        );
        assert!(
            json["entries"][1].get("mtime_ms").is_none(),
            "None mtime_ms omitted"
        );
        assert_eq!(json["entries"][1]["size_bytes"], 3);
        assert_eq!(json["total_entries"], 3);
        assert_eq!(json["truncated"], false);
        // Round-trip.
        let back: IpcResponse = serde_json::from_value(json).expect("deserialize");
        assert!(matches!(back, IpcResponse::FileListDir(_)));
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
