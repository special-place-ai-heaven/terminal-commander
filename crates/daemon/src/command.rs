// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon command runtime (TC38).
//!
//! Wires `command_start_combed` into the daemon. Non-PTY only. argv
//! is the only execution form accepted at TC38; shell-string
//! passthrough is forbidden by `POLICY.md` and explicitly out of
//! scope until a later goal authorizes it.
//!
//! Pipeline:
//!
//! ```text
//! CommandRuntime::start_combed
//!   -> PolicyEngine::evaluate(CommandStart)            // pre-spawn gate
//!   -> Router::bucket_create + ProbeId mint            // owns the bucket
//!   -> ProcessProbe::spawn                             // tokio child + sifter
//!        | DaemonEventSink (forwards drafts to Router::bucket_append)
//!        | ContextRingManager (raw frames for event_context only)
//!   -> JobManager::start                               // lifecycle tracking
//!   -> spawn waiter task: marks Running -> Exited/Failed/Cancelled
//!                         appends synthetic command_exited / command_failed
//!                         audits ipc_command_exit / ipc_command_killed
//! ```
//!
//! Source-status: live (TC38) for argv command execution on Unix
//! and Windows (tokio::process::Command). PTY-backed interactive
//! commands are handled SEPARATELY in `pty_command.rs` (TC44, shipped);
//! this runtime does NOT touch stdin or pseudo-terminals.
//!
//! Windows: `ProcessProbe::spawn` applies `CREATE_NO_WINDOW` for combed runtime
//! spawns. JS bridge (`lib/wsl/spawn.js`) intentionally does NOT — WWS04 EDR
//! legitimacy ritual. See `docs/release/windows-wsl-bridge-contract.md` §4.4.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use terminal_commander_core::{
    ActivationScope, BucketConfig, BucketId, ContextRingManager, EventDraft, JobConfig, JobId,
    JobManager, JobRecord, JobState, ProbeId, RuleDefinition,
};
use terminal_commander_probes::{EventSink, ProcessProbe, ProcessProbeConfig, ProcessProbeMetrics};
use terminal_commander_sifters::SifterRuntime;
use terminal_commander_store::AuditEntry;
use tokio::sync::oneshot;

use crate::activation::ActivationRegistry;
use crate::audit::AuditSink;
use crate::policy::{PolicyAction, PolicyDecision, PolicyEngine, PolicyProfile};
use crate::router::Router;

/// Maximum argv size accepted in a single request. Prevents an
/// operator from smuggling raw stream content as "command args".
pub const MAX_ARGV_ITEMS: usize = 256;
/// Maximum length of any single argv item.
pub const MAX_ARGV_ITEM_BYTES: usize = 4096;

/// Closed-set deny list for `argv[0]` basenames at the command-
/// runtime layer.
///
/// This is the **shell-bridge guard**: `command_start_combed` MUST
/// NOT become an unrestricted shell entry point. Any known shell
/// interpreter is rejected before the policy engine even sees the
/// request. Adding new variants requires a goal-file amendment.
///
/// Distinct from `policy::COMMANDS_DENY` (which targets privilege
/// escalators: sudo, doas, su, pkexec, kexec, polkit-agent,
/// polkit-auth-agent-1).
///
/// Future shell-execution opt-in is intentionally NOT implemented
/// in TC38. A later goal would need to add an explicit policy
/// capability (e.g. `allow_shell: bool` on `CommandStartRequest`,
/// gated by a new `PolicyAction::CommandShellStart` variant) before
/// this guard can be bypassed.
pub const SHELL_INTERPRETERS_DENY: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "zsh",
    "fish",
    "ksh",
    "csh",
    "tcsh",
    "ash",
    "busybox",
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
    "cmd",
    "cmd.exe",
];

/// Severity floor for the bucket attached to a command. Audit row
/// is emitted regardless; the bucket is the LLM-visible signal
/// stream.
pub const DEFAULT_COMMAND_SEVERITY_MIN: terminal_commander_core::Severity =
    terminal_commander_core::Severity::Info;

/// Upper bound on how long a graceful shutdown waits for in-flight
/// command lifecycle waiters to finish before aborting them. Matches
/// the IPC connection drain ceiling (`server.rs::DRAIN_CEILING`,
/// `pipe_server.rs::PIPE_DRAIN_CEILING`, 10 s) so the two drains have
/// symmetric bounds.
const LIFECYCLE_DRAIN_CEILING: Duration = Duration::from_secs(10);

/// Errors raised by the command runtime.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("policy denied command_start: {0}")]
    PolicyDenied(String),
    #[error(
        "shell interpreter '{0}' is denied by default; command_start_combed is not a shell bridge"
    )]
    ShellInterpreterDenied(String),
    #[error("argv must not be empty")]
    EmptyArgv,
    #[error("argv has {0} items; cap is {MAX_ARGV_ITEMS}")]
    ArgvTooLong(usize),
    #[error("argv item {index} is {len} bytes; cap is {MAX_ARGV_ITEM_BYTES}")]
    ArgvItemTooLong { index: usize, len: usize },
    #[error("bucket create error: {0}")]
    Bucket(#[from] terminal_commander_core::BucketError),
    #[error("sifter build error: {0}")]
    Sifter(String),
    #[error("process spawn error: {0}")]
    Spawn(#[from] terminal_commander_probes::ProcessProbeError),
    #[error("unknown job id: {0}")]
    UnknownJob(JobId),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Which lane started this combed job, threaded through
/// [`CommandRuntime::start_combed_inner`]. `Argv` is the default
/// command path: `argv[0]` is the program and shell interpreters are a
/// hard deny (`SHELL_INTERPRETERS_DENY`). `Shell` is the TC49
/// `shell_exec` lane: `argv` is the daemon-assembled `[shell, "-lc",
/// shell_line]`, the interpreter guard is skipped, and the verdict comes
/// from [`PolicyAction::CommandShellStart`] (gated by `allow_shell`).
///
/// Only THREE sites in `start_combed_inner` branch on this lane: the
/// shell-interpreter guard, the policy action, and the deny/allow audit
/// label. Everything from bucket allocation onward is shared verbatim.
///
/// A small reference-only enum (unit + two `&str`), so it is `Copy`:
/// passing it by value into `start_combed_inner` aliases nothing and
/// keeps the lane selector at the call sites readable.
#[derive(Clone, Copy)]
enum StartLane<'a> {
    Argv,
    Shell { shell_line: &'a str, shell: &'a str },
}

/// `command_start_combed` request shape. Plain Rust struct today;
/// the rmcp / IPC adapter at TC41 wraps it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStartRequest {
    /// Non-empty argv. `argv[0]` is the program; the rest are
    /// passed verbatim. Shell-string passthrough is forbidden.
    pub argv: Vec<String>,
    /// Working directory. Resolved by the OS; the policy gate may
    /// later reject paths outside the project root.
    pub cwd: Option<PathBuf>,
    /// Explicit environment to set on the child. Empty means
    /// inherit. Sensitive variables (none today) would be stripped
    /// here in a follow-up.
    pub env: Vec<(String, String)>,
    /// Bucket config (max_events / TTL). Defaults applied if None.
    pub bucket_config: Option<BucketConfig>,
    /// Optional rule set to bind. If None, the daemon's empty
    /// sifter is used (no events emitted). Hot rule rebinding is
    /// TC42 territory.
    pub rules: Vec<RuleDefinition>,
    /// Optional grace window between graceful and forced terminate.
    pub grace: Option<Duration>,
    /// Optional per-bucket tag for subscription routing (Phase 3). Recorded on
    /// the bucket source so a tag predicate can AND-filter to this probe.
    pub tag: Option<String>,
    /// Optional in-flight dedup nonce (TC-2). Threaded from
    /// `CommandStartParams::dedup_nonce` by `handle_command_start_combed`.
    /// When `Some`, `start_combed` collapses an in-flight duplicate carrying
    /// the SAME nonce to the SAME `(job_id, bucket_id)` instead of spawning
    /// twice. `None` falls back to a very short peer-scoped signature window.
    pub dedup_nonce: Option<String>,
    /// Optional pre-hashed peer discriminator (TC-2 peer-scoped fallback).
    /// Computed in `handle_command_start_combed` from the dispatching
    /// `PeerIdentity` (uid/sid). Folded into the nonce-less fallback key so a
    /// sibling local client cannot guess another client's about-to-run command
    /// and receive its live `(job_id, bucket_id)`. `None` for callers without
    /// a resolvable peer (e.g. direct in-process tests).
    pub peer_discriminator: Option<u64>,
}

// `CommandStartResponse`, `CommandReceipt`, and `CommandStatusResponse`
// are plain serde data over core types (`JobId`/`BucketId`/`ProbeId`/
// `JobState`). They moved into `terminal_commander_ipc::protocol`
// (Phase P1) so every IPC client shares the wire shape. The daemon
// runtime that builds these responses keeps using them via this
// re-import, and the crate-root `pub use command::{...}` surface stays
// stable.
pub use terminal_commander_ipc::protocol::{
    CommandReceipt, CommandStartResponse, CommandStatusResponse,
};

/// EventSink that forwards drafts to the wired `Router`.
///
/// The router writes the event to the in-memory bucket manager AND
/// emits a `bucket_append` audit row through the TC35
/// `PersistentAudit` sink. No raw stream content is ever copied.
#[derive(Clone)]
struct DaemonEventSink {
    router: Arc<Router>,
    job_id: JobId,
    jobs: Arc<JobManager>,
}

impl std::fmt::Debug for DaemonEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonEventSink")
            .field("job_id", &self.job_id)
            .finish_non_exhaustive()
    }
}

impl EventSink for DaemonEventSink {
    fn emit(&self, draft: EventDraft) -> Option<u64> {
        let bucket_id = draft.bucket_id;
        // Append through the router so the audit row lands.
        let ev = self.router.bucket_append(bucket_id, draft).ok()?;
        self.jobs.record_event(self.job_id);
        Some(ev.seq)
    }

    fn patch_dedupe_aggregate(
        &self,
        bucket_id: BucketId,
        patch: &terminal_commander_sifters::DedupeAggregatePatch,
    ) {
        let _ = self.router.bucket_patch_aggregation(bucket_id, patch);
    }
}

/// Per-job state tracked by [`CommandRuntime`]. Carries the live
/// counters, the rebind-able sifter handle, and the per-call inline
/// rules so a global activation change can recompute the merged
/// rule set without losing the inline rules the operator passed at
/// `start_combed` time (TC42b).
#[derive(Debug)]
struct JobBinding {
    metrics: ProcessProbeMetrics,
    sifter: Arc<terminal_commander_sifters::SifterRuntime>,
    inline_rules: Vec<terminal_commander_core::RuleDefinition>,
    /// Identity triple used to resolve scoped activations on rebind
    /// (TC42c). `(bucket_id, job_id, probe_id)` is the per-job key
    /// the `ActivationRegistry::snapshot_for_job` lookup needs.
    bucket_id: BucketId,
    probe_id: ProbeId,
    /// No-silence receipt (TCE-ERG-1). Computed by the lifecycle
    /// waiter at child exit; read by `status()`. `None` until exit, or
    /// when any rule matched.
    receipt: Option<CommandReceipt>,
    /// Bounded, ALREADY-REDACTED argv head (TC-4 Phase 4a): program name
    /// plus up to two tokens with credential spans masked. Computed once
    /// at `start_combed` via `redact_argv_head`; never holds a raw secret.
    argv_head: Vec<String>,
    /// TC-3: the probe's cancel handle, taken at spawn so `stop` can
    /// force-kill this job; `oneshot::Sender` is not Clone, which is why
    /// this struct dropped its Clone derive -- it is never whole-cloned.
    cancel: Option<oneshot::Sender<()>>,
    /// TC-3: a shared handle to the probe's LIVE metrics, cloned at spawn
    /// before the probe is moved into its lifecycle task. `stop` snapshots
    /// it to report the real frame/byte/event counts of a job it kills
    /// (the `metrics` field above is only populated at exit, so it would
    /// read zero for a still-running job). Mirrors the PTY runtime, which
    /// reads probe-side metrics before cancellation.
    metrics_live: Arc<parking_lot::Mutex<terminal_commander_probes::ProcessProbeMetrics>>,
}

/// Identity triple for a single live job.
///
/// Exposed via [`CommandRuntime::live_jobs`] so the IPC layer can
/// validate a caller-supplied [`ActivationScope`] against the set of
/// known live entities before persisting an activation row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveJobIdentity {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: ProbeId,
}

/// Bounded report returned by [`CommandRuntime::rebind_all_jobs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebindAllReport {
    /// Jobs that were live at the moment the rebind began.
    pub jobs_considered: u32,
    /// Jobs whose sifter was successfully rebuilt.
    pub jobs_rebound: u32,
    /// Jobs whose rebuild failed; the prior sifter is unchanged.
    pub rebuild_failures: u32,
}

/// Internal work tuple captured under the live-map read lock so the
/// rebuild loop can run without holding the lock. Refactored out so
/// the closure type stays under clippy's `type_complexity` threshold.
type RebindWork = (
    JobId,
    BucketId,
    ProbeId,
    Arc<terminal_commander_sifters::SifterRuntime>,
    Vec<terminal_commander_core::RuleDefinition>,
);

/// One in-flight dedup entry (TC-2). Holds the REAL ids minted for the
/// first start carrying a given fingerprint, plus the insertion instant
/// for the short TTL backstop. A duplicate start within the window
/// returns these ids verbatim (no fake success -- the live job they name
/// is the one already spawned). Evicted on EVERY completion path; the TTL
/// is only the backstop for a leaked entry.
#[derive(Clone, Copy, Debug)]
struct DedupEntry {
    job_id: JobId,
    bucket_id: BucketId,
    probe_id: ProbeId,
    inserted: std::time::Instant,
}

/// How long a nonce-less / fallback in-flight fingerprint stays live
/// before the TTL backstop drops it. Deliberately short: it only needs
/// to span a transport re-send of a single mutating start, not the whole
/// job lifetime (eviction-on-completion is the primary mechanism).
const DEDUP_TTL: std::time::Duration = std::time::Duration::from_secs(3);

/// The command runtime owned by `DaemonState`. Single instance per
/// daemon process.
pub struct CommandRuntime {
    router: Arc<Router>,
    rings: Arc<ContextRingManager>,
    jobs: Arc<JobManager>,
    audit: Arc<dyn AuditSink>,
    policy: PolicyEngine,
    profile_label: String,
    live: Arc<RwLock<std::collections::HashMap<JobId, JobBinding>>>,
    /// Activation registry consulted at `start_combed` AND at every
    /// `rebind_all_jobs` call. The active rule snapshot is merged
    /// with the per-call inline rules before the per-job
    /// `SifterRuntime` is built (TC42) or rebuilt (TC42b).
    activation: Arc<ActivationRegistry>,
    /// Bucket source side-table (subscriptions MUST-ADD #2). Recorded
    /// at `start_combed` immediately after `bucket_create`.
    sources: Arc<crate::subscriptions::source::BucketSourceTable>,
    /// Lifecycle waiter tasks spawned by `start_combed` (one per live
    /// command). Tracked in a `JoinSet` rather than a detached
    /// `tokio::spawn` so a graceful shutdown can DRAIN them via
    /// [`CommandRuntime::drain_lifecycle_tasks`] BEFORE the store actor
    /// is torn down. A command that exits inside the shutdown window
    /// would otherwise have its waiter call `bucket_append` / `audit.emit`
    /// AFTER the store is gone, silently losing the final
    /// `command_exited` event and audit row. Mirrors the IPC-server
    /// connection-drain pattern (`server.rs::drain_connections`,
    /// `pipe_server.rs::drain_pipe_connections`). Wrapped in a
    /// `parking_lot::Mutex` so the synchronous `start_combed` can enqueue
    /// without an `.await`; the drain takes the set out under the lock and
    /// joins it without holding the lock across an await.
    lifecycle_tasks: Arc<parking_lot::Mutex<tokio::task::JoinSet<()>>>,
    /// In-flight dedup map (TC-2). A SEPARATE `parking_lot::Mutex<HashMap>`,
    /// NOT the `live` `RwLock` (different key `u64` fingerprint and value
    /// `DedupEntry` shape; keeping them apart avoids lock coupling). Keyed by
    /// a `DefaultHasher` digest of either the client nonce or the peer-scoped
    /// `(peer, argv, cwd, tag)` fallback. Cloned into the lifecycle waiter
    /// closure (`let waiter_dedup = Arc::clone(&self.dedup);`) so eviction can
    /// run on the completion paths alongside `waiter_live`. The lock is held
    /// ONLY for map ops -- never across `.await`, never across the spawn.
    dedup: Arc<parking_lot::Mutex<std::collections::HashMap<u64, DedupEntry>>>,
}

impl std::fmt::Debug for CommandRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandRuntime")
            .field("profile", &self.profile_label)
            .finish_non_exhaustive()
    }
}

impl CommandRuntime {
    /// Construct. The audit sink is the same persistent sink wired
    /// at daemon bootstrap (TC36), NOT an in-memory fallback. The
    /// `activation` registry is the TC42 source of truth for which
    /// rules are currently active; the runtime consults it at every
    /// `start_combed` call.
    #[must_use]
    pub fn new(
        router: Arc<Router>,
        rings: Arc<ContextRingManager>,
        jobs: Arc<JobManager>,
        audit: Arc<dyn AuditSink>,
        policy: PolicyEngine,
        activation: Arc<ActivationRegistry>,
        sources: Arc<crate::subscriptions::source::BucketSourceTable>,
    ) -> Self {
        let profile_label = match policy.profile {
            PolicyProfile::DeveloperLocal => "developer_local".to_owned(),
            PolicyProfile::RepoOnly => "repo_only".to_owned(),
            PolicyProfile::ReadOnlyObserver => "read_only_observer".to_owned(),
            PolicyProfile::AdminDebug => "admin_debug".to_owned(),
            PolicyProfile::FullAccess => "full_access".to_owned(),
        };
        Self {
            router,
            rings,
            jobs,
            audit,
            policy,
            profile_label,
            live: Arc::new(RwLock::new(std::collections::HashMap::default())),
            activation,
            sources,
            lifecycle_tasks: Arc::new(parking_lot::Mutex::new(tokio::task::JoinSet::new())),
            dedup: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::default())),
        }
    }

    /// Validate argv shape before policy. Bounded sizes only.
    fn validate_argv(argv: &[String]) -> Result<(), CommandError> {
        if argv.is_empty() {
            return Err(CommandError::EmptyArgv);
        }
        if argv.len() > MAX_ARGV_ITEMS {
            return Err(CommandError::ArgvTooLong(argv.len()));
        }
        for (i, a) in argv.iter().enumerate() {
            if a.len() > MAX_ARGV_ITEM_BYTES {
                return Err(CommandError::ArgvItemTooLong {
                    index: i,
                    len: a.len(),
                });
            }
        }
        Ok(())
    }

    /// Reject argv[0] whose basename is a known shell interpreter.
    /// This is the shell-bridge guard; it runs BEFORE the policy
    /// engine so the audit log records the rejection with the
    /// specific reason "shell interpreter denied" instead of a
    /// generic policy reason.
    ///
    /// Matches both the bare basename and the path tail
    /// (`/bin/sh` -> "sh"). Case-insensitive on the Windows-style
    /// `.exe` variants because Windows file matching is case-
    /// insensitive at the OS layer; Linux comparisons are exact.
    fn shell_interpreter_basename(argv0: &str) -> Option<&'static str> {
        // Extract the last path component.
        let basename = std::path::Path::new(argv0)
            .file_name()
            .and_then(|os| os.to_str())
            .unwrap_or(argv0);
        for &shell in SHELL_INTERPRETERS_DENY {
            if basename == shell {
                return Some(shell);
            }
            // Case-insensitive match for the .exe family (Windows).
            let is_exe_variant = std::path::Path::new(shell)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
            if is_exe_variant && basename.eq_ignore_ascii_case(shell) {
                return Some(shell);
            }
        }
        // Bare "powershell" / "pwsh" / "cmd" without extension on
        // Windows would already be caught above; the loop handles
        // both cases.
        None
    }

    /// Audit one row through the persistent sink. Best-effort: an
    /// audit-store failure must not block the command pipeline.
    fn audit(
        &self,
        action: &str,
        subject: &str,
        decision: &str,
        reason: Option<String>,
        metadata: Option<String>,
    ) {
        let mut entry = AuditEntry::new(action, subject, decision)
            .with_actor("command_runtime")
            .with_profile(self.profile_label.clone());
        if let Some(r) = reason {
            entry = entry.with_reason(r);
        }
        if let Some(m) = metadata {
            entry = entry.with_metadata_json(m);
        }
        let _ = self.audit.emit(&entry);
    }

    /// Public `command_start_combed` entry point. Validates, gates on
    /// policy, builds a bucket + probe, spawns the child, returns a
    /// bounded response. Allocates a FRESH bucket for the spawned job.
    /// IDENTICAL external behavior to the pre-TC-5 method: every existing
    /// caller and test reaches the same code path via
    /// `start_combed_inner(req, None)`.
    ///
    /// MUST be called from within a tokio runtime; ProcessProbe::spawn
    /// uses tokio::process::Command.
    pub fn start_combed(
        &self,
        req: CommandStartRequest,
    ) -> Result<CommandStartResponse, CommandError> {
        self.start_combed_inner(req, None, StartLane::Argv)
    }

    /// TC-5 self-check spawn entry point. Like [`Self::start_combed`]
    /// but lets the caller REUSE an existing bucket instead of minting a
    /// fresh one. The self-check passes `None` on its first probe (which
    /// allocates and returns the bucket id to cache) and `Some(cached)`
    /// on every subsequent probe, so the daemon's `bucket_count` grows by
    /// exactly one over its whole lifetime no matter how many self-checks
    /// run. Distinct jobs land in the SAME reused bucket with DISTINCT
    /// `(job_id, probe_id)`; the bucket is never re-created, so the second
    /// reuse never trips `bucket_create`'s already-exists guard.
    pub fn start_combed_reusing(
        &self,
        req: CommandStartRequest,
        reuse_bucket: Option<BucketId>,
    ) -> Result<CommandStartResponse, CommandError> {
        self.start_combed_inner(req, reuse_bucket, StartLane::Argv)
    }

    /// TC49 shell-lane entry point. The single public seam into
    /// `StartLane::Shell`: it spawns the daemon-assembled
    /// `req.argv` = `[shell, "-lc", shell_line]` while SKIPPING the
    /// argv-lane shell-interpreter guard and instead gating on
    /// [`PolicyAction::CommandShellStart`] (the `allow_shell` capability).
    /// `shell_line` and `shell` MUST be the same strings the caller used to
    /// build `req.argv` — they are the audit subject and the policy inputs.
    ///
    /// SYNC, like [`Self::start_combed`]: `start_combed_inner` never awaits,
    /// so the `&str` borrows held in `StartLane::Shell` live only for the
    /// duration of this call. Async IPC/MCP handlers call it inline.
    ///
    /// MUST be called from within a tokio runtime; `ProcessProbe::spawn`
    /// uses `tokio::process::Command`.
    pub fn start_combed_shell(
        &self,
        req: CommandStartRequest,
        shell_line: &str,
        shell: &str,
    ) -> Result<CommandStartResponse, CommandError> {
        self.start_combed_inner(req, None, StartLane::Shell { shell_line, shell })
    }

    /// Shared `command_start_combed` engine. `reuse_bucket` is `None` for
    /// the normal public path (a fresh bucket is created + recorded) and
    /// `Some(bid)` for the TC-5 self-check reuse path (the bucket already
    /// exists; we MUST NOT re-create or re-record it, only spawn a new job
    /// into it). Every other stage -- dedup guard, argv/shell validate,
    /// policy gate, rule compile, spawn, waiter, live binding, audit, and
    /// the response -- is IDENTICAL to the pre-TC-5 method and keyed off
    /// the resolved `bucket_id`.
    #[allow(clippy::too_many_lines)] // sequential pipeline; splitting it hurts readability
    fn start_combed_inner(
    &self,
    req: CommandStartRequest,
    reuse_bucket: Option<BucketId>,
    mode: StartLane<'_>,
) -> Result<CommandStartResponse, CommandError> {
        Self::validate_argv(&req.argv)?;

        // TC-2 in-flight dedup guard. BEFORE the id mint: if an identical
        // logical start is already in flight (same client nonce, or the
        // same peer-scoped signature within DEDUP_TTL), return the REAL
        // ids of that job instead of spawning a second process. The lock
        // is held ONLY for this map lookup -- released before any spawn or
        // await. A nonce hit is honored regardless of age (the entry is
        // evicted on completion); a nonce-less fallback hit is honored
        // only within the TTL window. probe_id is stored in the entry so
        // the duplicate response is identical to the original.
        let (dedup_k, fallback_gated) = dedup_key(&req);
        {
            let mut map = self.dedup.lock();
            if let Some(entry) = map.get(&dedup_k).copied() {
                let fresh = !fallback_gated || entry.inserted.elapsed() < DEDUP_TTL;
                if fresh {
                    return Ok(CommandStartResponse {
                        job_id: entry.job_id,
                        bucket_id: entry.bucket_id,
                        probe_id: entry.probe_id,
                        cursor: 0,
                    });
                }
                // Stale fallback entry the TTL backstop should have
                // dropped: remove it so this start proceeds as fresh.
                map.remove(&dedup_k);
            }
        }

        // Shell-bridge guard. Runs BEFORE the policy engine so the
        // rejection reason is precise. command_start_combed is not
        // a shell bridge: a future opt-in policy capability is the
        // only sanctioned path to invoke an interpreter, and TC38
        // does NOT add that capability.
        //
        // ARGV LANE ONLY. The TC49 shell lane (`StartLane::Shell`)
        // assembles `argv[0]` = the chosen interpreter ON PURPOSE, so
        // this guard would self-deny it. The shell lane is instead gated
        // by `PolicyAction::CommandShellStart` (allow_shell cap) below.
        if matches!(mode, StartLane::Argv)
            && let Some(shell) = Self::shell_interpreter_basename(&req.argv[0])
        {
            self.audit(
                "command_rejected",
                &subject_for_argv(&req.argv),
                "deny",
                Some(format!(
                    "shell interpreter '{shell}' denied by default; \
                             command_start_combed is not a shell bridge"
                )),
                Some(format_argv_metadata(&req.argv)),
            );
            return Err(CommandError::ShellInterpreterDenied(shell.to_owned()));
        }

        // Pre-spawn policy gate. The lane selects BOTH the policy action
        // and the audit-row labels (a denied argv start is
        // `command_rejected`; a denied shell start is
        // `command_shell_rejected`). The deny path is otherwise identical
        // across lanes.
        let cwd_for_policy = req.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
        let verdict = match mode {
            StartLane::Argv => self.policy.evaluate(&PolicyAction::CommandStart {
                argv: &req.argv,
                cwd: cwd_for_policy.as_path(),
            }),
            StartLane::Shell { shell_line, shell } => {
                self.policy.evaluate(&PolicyAction::CommandShellStart {
                    shell_line,
                    cwd: cwd_for_policy.as_path(),
                    shell,
                })
            }
        };
        if verdict.decision == PolicyDecision::Deny {
            match mode {
                StartLane::Argv => self.audit(
                    "command_rejected",
                    &subject_for_argv(&req.argv),
                    "deny",
                    Some(verdict.reason.clone()),
                    Some(format_argv_metadata(&req.argv)),
                ),
                StartLane::Shell { shell_line, .. } => self.audit(
                    "command_shell_rejected",
                    &redact_shell_line(shell_line),
                    "deny",
                    Some(verdict.reason.clone()),
                    Some(format_argv_metadata(&req.argv)),
                ),
            }
            return Err(CommandError::PolicyDenied(verdict.reason));
        }

        // Shell lane: the verdict is allowed (AllowWithAudit). Emit the
        // dedicated `command_shell_start` audit row with a redacted shell
        // line BEFORE spawning, so the allow decision is recorded even if
        // the spawn itself later fails. The argv lane's own
        // `command_start` row is emitted further down the shared core.
        if let StartLane::Shell { shell_line, .. } = mode {
            self.audit(
                "command_shell_start",
                &redact_shell_line(shell_line),
                "allow_with_audit",
                Some(verdict.reason),
                Some(format_argv_metadata(&req.argv)),
            );
        }

        // Allocate identifiers. These are pure value allocations (no
        // backing resource), so generating them before any side effect
        // is free and lets the scope snapshot + rule compile run first.
        //
        // TC-5: when `reuse_bucket` is `Some`, the bucket already exists
        // (a prior self-check spawn created it); reuse its id verbatim so
        // this new job lands in the SAME immortal bucket. `BucketId` is
        // `Copy`, so reading `reuse_bucket` here and again at the guard
        // below is free and aliases nothing.
        let bucket_id = reuse_bucket.unwrap_or_default();
        let probe_id = ProbeId::new();
        let job_id = JobId::new();

        // Merge scope-resolved active rules (TC42c) with the per-call
        // inline rules, then COMPILE them BEFORE allocating the bucket.
        // An invalid inline rule is a caller-fixable error: failing the
        // compile here (before `bucket_create`) means a bad rule fails
        // fast with no orphaned bucket left behind. `bucket_create` has
        // no inverse on the Router, so allocating it only after the rule
        // set is known good avoids a leak on the build-failure path.
        // Same merge helper TC42b uses on every rebind so semantics stay
        // identical. The scoped snapshot returns only entries whose scope
        // matches `(global ∪ matching-bucket ∪ matching-job ∪
        // matching-probe)`.
        let active_for_job = self
            .activation
            .snapshot_for_job(bucket_id, job_id, probe_id);
        let merged_rules: Vec<RuleDefinition> =
            merge_active_and_inline(&active_for_job, &req.rules);
        let sifter = Arc::new(
            SifterRuntime::build(&merged_rules).map_err(|e| CommandError::Sifter(e.to_string()))?,
        );

        // Rule set compiled cleanly: now allocate the bucket. Reaching
        // here guarantees we will not orphan it on a rule-compile error.
        //
        // TC-5: only create + record the bucket on the fresh path. On the
        // reuse path the bucket already exists and is already recorded in
        // the source side-table; re-creating it would trip the Router's
        // already-exists guard and re-recording it is redundant. The new
        // job's source identity (job_id/probe_id) is per-job and recorded
        // only for the bucket that owns it on first create -- the reuse
        // path deliberately leaves the original source row intact.
        let bucket_cfg = req.bucket_config.unwrap_or_default();
        if reuse_bucket.is_none() {
            self.router.bucket_create(bucket_id, bucket_cfg)?;
            // Record the bucket's source identity for subscription routing
            // (MUST-ADD #2). Bumps the side-table dirty epoch so a `sources: all`
            // subscription picks this new bucket up on its next pull.
            self.sources.record(
                bucket_id,
                crate::subscriptions::source::BucketSource {
                    kind: terminal_commander_ipc::ProbeKind::Command,
                    job_id: Some(job_id),
                    probe_id: Some(probe_id),
                    path: None,
                    tag: req.tag.clone(),
                },
            );
        }
        // Keep a handle for the live binding so TC42b's
        // `rebind_all_jobs` can swap this job's rule set without
        // restarting the probe.
        let sifter_for_binding = Arc::clone(&sifter);

        // Build the event sink BEFORE start so the spawn closure
        // captures the right router/job pair.
        let sink: Arc<dyn EventSink> = Arc::new(DaemonEventSink {
            router: Arc::clone(&self.router),
            job_id,
            jobs: Arc::clone(&self.jobs),
        });

        // Probe config.
        let env_os: Vec<(std::ffi::OsString, std::ffi::OsString)> = req
            .env
            .iter()
            .map(|(k, v)| (std::ffi::OsString::from(k), std::ffi::OsString::from(v)))
            .collect();
        let probe_cfg = ProcessProbeConfig {
            probe_id: Some(probe_id),
            bucket_id,
            cwd: req.cwd.clone(),
            env: env_os,
            grace: req
                .grace
                .unwrap_or(terminal_commander_probes::DEFAULT_GRACE),
        };

        // TC-2: register the in-flight dedup entry NOW, BEFORE the spawn,
        // so a duplicate retry that arrives WHILE this start is still
        // spawning collapses to these ids (the slow-spawn window is the
        // exact case the guard targets). The lock is held only for the
        // insert -- never across the spawn below. The entry is evicted on
        // the spawn-failure path just below and on every completion path
        // in the lifecycle waiter; the TTL is only the backstop.
        self.dedup.lock().insert(
            dedup_k,
            DedupEntry {
                job_id,
                bucket_id,
                probe_id,
                inserted: std::time::Instant::now(),
            },
        );

        // Spawn the probe. If this fails, we audit and bail without
        // creating a job record.
        // Windows: ProcessProbe::spawn applies CREATE_NO_WINDOW for combed runtime
        // spawns. JS bridge (lib/wsl/spawn.js) intentionally does NOT — WWS04 EDR
        // legitimacy ritual. See docs/release/windows-wsl-bridge-contract.md §4.4.
        let mut probe =
            match ProcessProbe::spawn(&req.argv, &probe_cfg, Arc::clone(&self.rings), sifter, sink)
            {
                Ok(p) => p,
                Err(e) => {
                    // Eviction-on-spawn-failure: the job never came to
                    // life, so drop its fingerprint immediately. A leaked
                    // entry must NEVER block a legitimate identical retry.
                    self.dedup.lock().remove(&dedup_k);
                    self.audit(
                        "command_start",
                        &subject_for_argv(&req.argv),
                        "error",
                        Some(format!("spawn failed: {e}")),
                        Some(format_argv_metadata(&req.argv)),
                    );
                    return Err(CommandError::Spawn(e));
                }
            };

        // TC-3: take the probe's cancel handle out NOW, while we still own
        // `probe` by value and BEFORE it is moved into the lifecycle closure
        // via `drive_to_exit(probe)` below. The handle is stored in the live
        // binding so `stop()` can fire the one-shot kill itself. `probe`
        // retains its own `cancel()` path for in-probe teardown, but that
        // sender has been moved here, so only `stop()` (via this handle) can
        // trigger the kill from outside.
        let cancel_handle = probe.take_cancel_handle();
        // TC-3: clone the LIVE metrics handle (a shared Arc) BEFORE `probe`
        // is moved into the lifecycle closure, so `stop()` can snapshot the
        // real frame/byte/event counts of a job it kills.
        let metrics_live = probe.metrics_handle();

        // Register the job and audit the allow decision. Consume
        // req.argv exactly once here; downstream code uses
        // job_cfg.argv (already moved into the JobManager record)
        // or a stored argv_for_meta clone.
        let argv_for_meta = req.argv.clone();
        // Snapshot the inline rules into the binding so a future
        // `rebind_all_jobs` can recompute `(active ∪ inline)` for
        // this job without losing the operator's per-call rules.
        let inline_rules_for_binding = req.rules.clone();
        let job_cfg = JobConfig {
            job_id,
            argv: req.argv,
            bucket_id,
            probe_id,
            grace_secs: req
                .grace
                .unwrap_or(terminal_commander_probes::DEFAULT_GRACE)
                .as_secs(),
        };
        let _ = self.jobs.start(job_cfg);
        self.jobs.mark_running(job_id);
        self.live.write().insert(
            job_id,
            JobBinding {
                metrics: ProcessProbeMetrics::default(),
                sifter: sifter_for_binding,
                inline_rules: inline_rules_for_binding,
                bucket_id,
                probe_id,
                receipt: None,
                // TC-4: store the redacted, bounded argv head so probe
                // listings can surface it without re-touching the raw argv.
                // Computed from the `argv_for_meta` clone (req.argv was
                // already moved into `job_cfg.argv` above).
                argv_head: redact_argv_head(&argv_for_meta),
                // TC-3: the probe's cancel handle, taken just above. `stop()`
                // fires this to force-kill the job.
                cancel: cancel_handle,
                // TC-3: shared handle to the probe's live metrics for `stop()`.
                metrics_live,
            },
        );
        self.audit(
            "command_start",
            &job_id.to_wire_string(),
            "allow",
            None,
            Some(format_argv_metadata(&argv_for_meta)),
        );

        // Spawn the lifecycle waiter task. When the child exits we
        // emit a synthetic lifecycle event into the bucket and an
        // audit row.
        let waiter_jobs = Arc::clone(&self.jobs);
        let waiter_router = Arc::clone(&self.router);
        let waiter_audit = Arc::clone(&self.audit);
        let waiter_profile = self.profile_label.clone();
        let waiter_live = Arc::clone(&self.live);
        let waiter_rings = Arc::clone(&self.rings);
        // TC-2: clone the dedup map into the waiter so the in-flight entry
        // is evicted on EVERY terminal outcome (normal exit AND cancel --
        // both reach `drive_to_exit` and the eviction below). The fixed
        // `dedup_k` (a Copy `u64`) is moved in. Eviction-on-completion is
        // the PRIMARY release; the TTL is only the backstop for a leak.
        let waiter_dedup = Arc::clone(&self.dedup);
        // Enqueue into the lifecycle JoinSet (not a detached
        // `tokio::spawn`) so a graceful shutdown can await this waiter
        // BEFORE the store actor closes; otherwise a command exiting in
        // the shutdown window loses its final command_exited event +
        // audit row. `JoinSet::spawn` needs an active runtime context,
        // which every `start_combed` caller has (IPC handlers run on the
        // daemon runtime). The lock is held only for the enqueue.
        self.lifecycle_tasks.lock().spawn(async move {
            let (mut final_metrics, outcome) = drive_to_exit(probe).await;

            // TCE-ERG-1: build the no-silence receipt while
            // `final_metrics.events_emitted` still reflects ONLY
            // rule-driven events. The lifecycle bump below would
            // otherwise inflate it by one. The receipt is emitted only
            // when zero rules matched (the sanctioned carve-out to the
            // "never raw output" contract); any rule match leaves it
            // `None`. Process-only by construction.
            let rule_driven_events = final_metrics.events_emitted;
            let receipt_exit_code = match &outcome {
                ProbeOutcome::Exited { code, .. } => *code,
                ProbeOutcome::Cancelled => None,
            };
            let receipt = if rule_driven_events == 0 {
                let tail = waiter_rings.tail_frames(probe_id, 5, 4096).unwrap_or(
                    terminal_commander_core::RingTail {
                        lines: Vec::new(),
                        evicted_frames: 0,
                        truncated: false,
                    },
                );
                Some(CommandReceipt {
                    exit_code: receipt_exit_code,
                    lines_suppressed: final_metrics.frames_total,
                    tail: tail.lines,
                    tail_incomplete: tail.evicted_frames > 0 || tail.truncated,
                })
            } else {
                None
            };

            // Publish the receipt into the live binding BEFORE the job
            // state flips terminal. `status()` reads the state from
            // `jobs` (set by `finish`/`cancel` below) and the receipt
            // from `live`; if `finish` ran first, a `command_status` /
            // `run_and_watch` poll landing in the gap would observe a
            // terminal state with a still-`None` receipt and wrongly
            // conclude the quiet command produced no receipt. Writing
            // the receipt first makes "terminal" imply "receipt present"
            // for the no-rule path. Metrics are refreshed after the
            // lifecycle append below (they depend on its `events_emitted`
            // bump), so only the receipt is published here.
            if let Some(b) = waiter_live.write().get_mut(&job_id) {
                b.receipt = receipt;
            }

            // TC-3: if `stop()` already finalized this job (set it terminal under the
            // live write lock), skip -- do not double-finish / double-append. Mirrors the
            // PTY waiter guard. Unlike the PTY waiter, the combed runtime owns the TC-2
            // in-flight dedup entry, so we MUST still evict it on this early-return path:
            // `stop()` does not touch the dedup map, and a nonce-keyed entry is NOT
            // TTL-gated (it is released ONLY on completion). Skipping the eviction here
            // would leak a stopped job's fingerprint forever and block a future
            // identical-nonce start (the TC-2 never-block invariant). The kill IS the
            // completion, so evict now, then return.
            if waiter_jobs.get(job_id).is_some_and(|r| {
                matches!(
                    r.state,
                    terminal_commander_core::JobState::Exited
                        | terminal_commander_core::JobState::Failed
                        | terminal_commander_core::JobState::Cancelled
                )
            }) {
                waiter_dedup.lock().remove(&dedup_k);
                return;
            }

            // Lifecycle draft.
            let draft = match outcome {
                ProbeOutcome::Exited { code, signal } => waiter_jobs.finish(job_id, code, signal),
                ProbeOutcome::Cancelled => waiter_jobs.cancel(job_id),
            };

            // TC-2 eviction-on-completion (PRIMARY release). The job is now
            // terminal on BOTH outcomes (normal exit and cancel reach here),
            // so the in-flight fingerprint can no longer collapse a future
            // identical start -- drop it. A re-run AFTER completion must get
            // a FRESH job (the never-block invariant). Lock held only for the
            // remove.
            waiter_dedup.lock().remove(&dedup_k);

            // Append the lifecycle event to the bucket (router does
            // the audit on bucket_append).
            if let Some(d) = draft.as_ref()
                && waiter_router.bucket_append(bucket_id, d.clone()).is_ok()
            {
                final_metrics.events_emitted = final_metrics.events_emitted.saturating_add(1);
                waiter_jobs.record_event(job_id);
            }

            // Update the stored metrics for command_status callers.
            // The binding's `sifter`, `inline_rules`, and the receipt
            // published above are preserved so a deactivation racing
            // with the exit still has a sifter handle to swap (the no-op
            // rebuild is harmless on a finished probe).
            if let Some(b) = waiter_live.write().get_mut(&job_id) {
                b.metrics = final_metrics;
            }

            // Emit our own structured audit row for the exit.
            let (action, decision, reason) = match draft.as_ref() {
                Some(d) if d.kind == "command_exited" => ("command_exit", "info", None),
                Some(d) if d.kind == "command_failed" => (
                    "command_exit",
                    "info",
                    Some(format!("nonzero exit: {}", d.summary)),
                ),
                _ => ("command_exit", "info", Some("no draft".to_owned())),
            };
            let mut entry = AuditEntry::new(action, job_id.to_wire_string(), decision)
                .with_actor("command_runtime")
                .with_profile(waiter_profile);
            if let Some(r) = reason {
                entry = entry.with_reason(r);
            }
            entry = entry.with_metadata_json(format_argv_metadata(&argv_for_meta));
            let _ = waiter_audit.emit(&entry);
        });

        Ok(CommandStartResponse {
            job_id,
            bucket_id,
            probe_id,
            cursor: 0,
        })
    }

    /// Force-kill a running combed command by `job_id` (TC-3).
    ///
    /// SECURITY-CRITICAL ORDERING. The steps run in this exact order so a
    /// policy-denied caller learns nothing about job existence and the deny
    /// audit row names the PEER, not the job:
    ///
    /// 1. DENY-FIRST: evaluate `CommandSignal` BEFORE any live-map lookup. A
    ///    denied caller therefore gets NO existence oracle, and the deny
    ///    row's subject is `peer_subject` (the caller identity), never a
    ///    `job_id`. No live map is touched on the deny path.
    /// 2. ALLOW: a single check-then-set critical section under the live
    ///    write lock. This is the ONLY lock held across the
    ///    already-terminal check, the handle take, and the `jobs.cancel`,
    ///    so it serializes against the natural-exit waiter (which must take
    ///    `live.write` to publish its receipt before it finishes the job).
    ///    Setting the job Cancelled under the held `live` lock guarantees
    ///    the waiter -- once it acquires `live.write` -- observes a terminal
    ///    state and guards out (no double-finish / double-append).
    /// 3. job-id-bearing ALLOW audit, then fire the one-shot kill.
    ///
    /// LOCK ORDERING: this nests `live.write()` THEN `jobs.cancel`
    /// (`jobs.write`, taken and released INSIDE `cancel`). No other code
    /// path takes these in the reverse order: the natural-exit waiter takes
    /// `live.write`, DROPS it, then touches `jobs` -- sequential, never
    /// nested -- so there is no `jobs -> live` nesting to deadlock against.
    pub fn stop(
        &self,
        job_id: JobId,
        peer_subject: &str,
    ) -> Result<(BucketId, ProcessProbeMetrics), CommandError> {
        // (1) DENY-FIRST. Evaluate the signal policy BEFORE any live-map
        // lookup so a denied caller gets no existence oracle and the deny
        // row's subject is the PEER, not the job_id.
        let verdict = self.policy.evaluate(&PolicyAction::CommandSignal);
        if verdict.decision == PolicyDecision::Deny {
            self.audit(
                "command_stop",
                peer_subject,
                "deny",
                Some(verdict.reason.clone()),
                None,
            );
            // NO live-map touch, NO job_id anywhere on this path.
            return Err(CommandError::PolicyDenied(verdict.reason));
        }

        // (2) ALLOW: check-then-set under the live write lock -- the single
        // critical section vs the natural-exit waiter (which must take
        // live.write to publish its receipt before it finishes the job).
        let (bucket_id, metrics, handle) = {
            let mut live = self.live.write();
            let Some(b) = live.get_mut(&job_id) else {
                return Err(CommandError::UnknownJob(job_id));
            };
            let bucket_id = b.bucket_id;
            // Snapshot the LIVE probe metrics (real frame/byte/event counts),
            // not the binding's `metrics` field which stays default until exit.
            let metrics = b.metrics_live.lock().clone();
            // Already terminal -> no-op, return the terminal state. (Cosmetic
            // race window with a natural exit, identical to the PTY design --
            // documented.)
            if self.jobs.get(job_id).is_some_and(|r| {
                matches!(
                    r.state,
                    JobState::Exited | JobState::Failed | JobState::Cancelled
                )
            }) {
                return Ok((bucket_id, metrics));
            }
            let handle = b.cancel.take();
            // Set Cancelled under the held live lock so the waiter (blocked on
            // live.write) observes terminal and guards out.
            let _ = self.jobs.cancel(job_id);
            (bucket_id, metrics, handle)
        }; // live dropped here

        // (3) job-id-bearing ALLOW audit BEFORE firing the kill.
        self.audit(
            "command_stop",
            &job_id.to_wire_string(),
            "allow",
            None,
            Some(format!(
                "{{\"frames\":{},\"events\":{},\"bytes\":{}}}",
                metrics.frames_total, metrics.events_emitted, metrics.bytes_total
            )),
        );
        if let Some(tx) = handle {
            let _ = tx.send(()); // fire the kill
        }
        Ok((bucket_id, metrics))
    }

    /// Await every in-flight lifecycle waiter task, bounded by
    /// [`LIFECYCLE_DRAIN_CEILING`]. Called by `run_ipc_server` AFTER the
    /// IPC connections have drained and BEFORE `shutdown_store`, so a
    /// command that exits inside the shutdown window still gets its
    /// `command_exited` event appended and its exit audit row emitted
    /// before the store actor is torn down.
    ///
    /// The set is taken out under the lock and joined OUTSIDE the lock so
    /// the brief enqueue lock in `start_combed` is never blocked across an
    /// await. Tasks spawned after the take (vanishingly unlikely once
    /// shutdown has begun and IPC has stopped accepting) land in a fresh
    /// set and are not awaited; this is acceptable -- the contract is to
    /// drain the waiters in flight when shutdown began. Mirrors the
    /// IPC-server drain (`server.rs::drain_connections`). Cross-platform:
    /// the command runtime is not unix-only.
    pub async fn drain_lifecycle_tasks(&self) {
        let mut tasks = {
            let mut guard = self.lifecycle_tasks.lock();
            std::mem::take(&mut *guard)
        };
        if tasks.is_empty() {
            return;
        }
        let drain = async { while tasks.join_next().await.is_some() {} };
        if tokio::time::timeout(LIFECYCLE_DRAIN_CEILING, drain)
            .await
            .is_err()
        {
            // Ceiling hit: a waiter did not finish in time (e.g. a child
            // ignoring its grace deadline). Abort the stragglers so the
            // process can exit; best-effort, not re-awaited.
            tasks.abort_all();
        }
    }
    /// Recompute every live job's sifter from the current
    /// activation registry snapshot + that job's stored inline
    /// rules. TC42c: pass an optional scope filter to restrict the
    /// rebind to jobs the scope would touch (matching `bucket_id` /
    /// `job_id` / `probe_id`, or every job for [`ActivationScope::Global`]).
    /// Pass `None` to rebind every live job (used at bootstrap or
    /// when the caller does not have a specific scope in hand).
    ///
    /// The probe API is unchanged; the swap happens inside the
    /// `Arc<SifterRuntime>` each job already holds.
    ///
    /// This is the entry point: an LLM-issued `registry_activate` /
    /// `registry_deactivate` calls this after updating the
    /// activation registry so already-running commands see the new
    /// rule set on future frames. In-flight frames finish against
    /// the snapshot they captured (no fake historical matches).
    ///
    /// Per-job rebuild failures (e.g. the active set now contains a
    /// rule the sifter cannot compile) leave that job's prior sifter
    /// in place and are counted in the returned report. They are
    /// audited individually via the daemon's persistent audit sink
    /// using the standard `command_runtime` actor. The audit
    /// metadata includes the resolved scope so an auditor can prove
    /// only the matching jobs were touched.
    pub fn rebind_all_jobs(&self) -> RebindAllReport {
        self.rebind_jobs_in_scope(None)
    }

    /// TC42c entry point. Rebinds only the live jobs the supplied
    /// scope matches. `None` is equivalent to [`ActivationScope::Global`]
    /// (every job).
    pub fn rebind_jobs_in_scope(&self, scope: Option<ActivationScope>) -> RebindAllReport {
        // Take a snapshot of `(job_id, sifter_handle, inline_rules,
        // identity)` under the read lock so the rebuild loop does
        // not hold the map lock across a regex compile.
        let work: Vec<RebindWork> = {
            let g = self.live.read();
            g.iter()
                .filter_map(|(jid, binding)| {
                    let matches = match scope {
                        None | Some(ActivationScope::Global) => true,
                        Some(s) => s.matches(binding.bucket_id, *jid, binding.probe_id),
                    };
                    if !matches {
                        return None;
                    }
                    Some((
                        *jid,
                        binding.bucket_id,
                        binding.probe_id,
                        Arc::clone(&binding.sifter),
                        binding.inline_rules.clone(),
                    ))
                })
                .collect()
        };
        let mut report = RebindAllReport {
            jobs_considered: u32::try_from(work.len()).unwrap_or(u32::MAX),
            jobs_rebound: 0,
            rebuild_failures: 0,
        };
        let scope_label = scope.map_or("any", |s| s.kind_label());
        for (job_id, bucket_id, probe_id, sifter, inline_rules) in work {
            let active_for_job = self
                .activation
                .snapshot_for_job(bucket_id, job_id, probe_id);
            let merged = merge_active_and_inline(&active_for_job, &inline_rules);
            match sifter.rebuild(&merged) {
                Ok(rb) => {
                    report.jobs_rebound = report.jobs_rebound.saturating_add(1);
                    self.audit(
                        "command_sifter_rebind",
                        &job_id.to_wire_string(),
                        "info",
                        None,
                        Some(format!(
                            "{{\"old_rule_count\":{},\"new_rule_count\":{},\"scope\":\"{}\"}}",
                            rb.old_rule_count, rb.new_rule_count, scope_label
                        )),
                    );
                }
                Err(e) => {
                    report.rebuild_failures = report.rebuild_failures.saturating_add(1);
                    self.audit(
                        "command_sifter_rebind",
                        &job_id.to_wire_string(),
                        "error",
                        Some(e.to_string()),
                        None,
                    );
                }
            }
        }
        report
    }

    /// Snapshot of every live job's identity triple. The IPC layer
    /// uses this to validate a caller-supplied [`ActivationScope`]
    /// before persisting a scoped activation row. Returned cloned so
    /// callers can hold the values across an async boundary without
    /// keeping the live-map lock.
    #[must_use]
    pub fn live_jobs(&self) -> Vec<LiveJobIdentity> {
        let g = self.live.read();
        g.iter()
            .map(|(jid, binding)| LiveJobIdentity {
                job_id: *jid,
                bucket_id: binding.bucket_id,
                probe_id: binding.probe_id,
            })
            .collect()
    }

    /// Returns the bounded, redacted argv head recorded for a live job, or None
    /// if the job is not in the live map. Single read lock, never held across await.
    pub fn argv_head(&self, job_id: JobId) -> Option<Vec<String>> {
        self.live.read().get(&job_id).map(|b| b.argv_head.clone())
    }

    /// `command_status` entry point. Returns bounded counters + the
    /// final exit state for the given job. Never returns raw text.
    pub fn status(&self, job_id: JobId) -> Result<CommandStatusResponse, CommandError> {
        let rec = self
            .jobs
            .get(job_id)
            .ok_or(CommandError::UnknownJob(job_id))?;
        let (metrics, receipt) = self
            .live
            .read()
            .get(&job_id)
            .map(|b| (b.metrics.clone(), b.receipt.clone()))
            .unwrap_or_default();
        Ok(CommandStatusResponse {
            job_id,
            bucket_id: rec.config.bucket_id,
            probe_id: rec.config.probe_id,
            state: rec.state,
            frames_total: metrics.frames_total,
            frames_stdout: metrics.frames_stdout,
            frames_stderr: metrics.frames_stderr,
            bytes_total: metrics.bytes_total,
            events_emitted: metrics.events_emitted,
            frames_suppressed: metrics.frames_suppressed,
            frames_suppressed_progress: metrics.frames_suppressed_progress,
            frames_suppressed_dedupe: metrics.frames_suppressed_dedupe,
            exit_code: rec.exit_info.as_ref().and_then(|e| e.exit_code),
            signal: rec.exit_info.as_ref().and_then(|e| e.signal.clone()),
            duration_ms: rec.exit_info.as_ref().map(|e| e.duration_ms),
            receipt,
        })
    }

    /// Test helper.
    #[must_use]
    pub fn job_record(&self, job_id: JobId) -> Option<JobRecord> {
        self.jobs.get(job_id)
    }
}

enum ProbeOutcome {
    Exited {
        code: Option<i32>,
        signal: Option<String>,
    },
    Cancelled,
}

async fn drive_to_exit(mut probe: ProcessProbe) -> (ProcessProbeMetrics, ProbeOutcome) {
    let outcome = match probe.wait().await {
        Ok(status) => ProbeOutcome::Exited {
            code: status.code(),
            signal: extract_signal(status),
        },
        Err(terminal_commander_probes::ProcessProbeError::Cancelled) => ProbeOutcome::Cancelled,
        Err(e) => ProbeOutcome::Exited {
            code: None,
            signal: Some(format!("error:{e}")),
        },
    };
    (probe.metrics(), outcome)
}

#[cfg(unix)]
fn extract_signal(status: std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|s| format!("SIG{s}"))
}

#[cfg(not(unix))]
const fn extract_signal(_status: std::process::ExitStatus) -> Option<String> {
    None
}

/// Compute the per-job rule set: `(active ∪ inline)`. Inline rules
/// win on `(rule_id, version)` collisions so an operator can shadow
/// an active rule for one job without deactivating it globally.
/// Deterministic ordering: active rules first (already sorted by the
/// `ActivationRegistry::snapshot` contract), then inline rules in
/// caller order.
fn merge_active_and_inline(
    active: &[terminal_commander_core::RuleDefinition],
    inline: &[terminal_commander_core::RuleDefinition],
) -> Vec<terminal_commander_core::RuleDefinition> {
    // Defense in depth against the draft-poison footgun. Only
    // runtime-eligible (Active) rules may reach `SifterRuntime::build`.
    // Filtering here means:
    //   * a daemon already holding a Draft entry (bound by a pre-gate
    //     binary) no longer fails EVERY command_start with a scope-wide
    //     `SifterError::NotActive` -> the live poison self-heals: the
    //     bad rule is skipped, the command runs, other rules still fire;
    //   * a non-eligible INLINE rule on a single command is skipped for
    //     that command instead of hard-failing the start.
    // The IPC activate gate still rejects new Draft activations up front
    // with the clear `RuleNotActive` remedy; this is the last-line guard
    // so no path can turn a non-Active rule into a blocked command.
    let mut seen: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();
    for r in inline {
        seen.insert((r.id.clone(), r.version));
    }
    let mut out = Vec::with_capacity(active.len() + inline.len());
    for r in active {
        if r.status.is_runtime_eligible() && !seen.contains(&(r.id.clone(), r.version)) {
            out.push(r.clone());
        }
    }
    out.extend(
        inline
            .iter()
            .filter(|r| r.status.is_runtime_eligible())
            .cloned(),
    );
    out
}

fn subject_for_argv(argv: &[String]) -> String {
    let head = argv.first().map_or("", String::as_str);
    // Limit subject size — caller has already bounded items, but
    // build a stable short label.
    let trimmed = if head.len() > 256 { &head[..256] } else { head };
    trimmed.to_owned()
}

/// Audit-safe preview of a TC49 shell line. Reuses the per-token
/// secret masker (`mask_token_inline`) over whitespace splits so the
/// same credential spans hidden in argv audits are hidden here, then
/// caps the joined result at 128 bytes on a char boundary (panic-free on
/// multibyte input). This is the `subject` of the `command_shell_start`
/// / `command_shell_rejected` audit rows — never the raw line.
fn redact_shell_line(line: &str) -> String {
    let masked: Vec<String> = line.split_whitespace().map(mask_token_inline).collect();
    let joined = masked.join(" ");
    let mut end = joined.len().min(128);
    while end > 0 && !joined.is_char_boundary(end) {
        end -= 1;
    }
    joined[..end].to_owned()
}

/// Compute the in-flight dedup key for a start request (TC-2).
///
/// PREFERS the client nonce: when `dedup_nonce` is `Some`, the key is a
/// `DefaultHasher` digest of just the nonce (the nonce is already
/// client-unique, so two DISTINCT logical starts -- which the adapter
/// gives DISTINCT nonces -- never collide). A non-empty nonce returns
/// `(key, false)`: a nonce match is exact intent and is NOT TTL-gated
/// beyond the eviction-on-completion backstop.
///
/// FALLBACK (nonce-less old-adapter blind retry): digests
/// `(peer_discriminator, argv, cwd, tag)` and returns `(key, true)` to
/// mark the entry as window-gated -- this low-entropy fingerprint is only
/// honored within `DEDUP_TTL`. Peer-scoping the fallback prevents a
/// sibling local client from guessing another client's about-to-run
/// command and receiving its live `(job_id, bucket_id)`.
fn dedup_key(req: &CommandStartRequest) -> (u64, bool) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match req.dedup_nonce.as_deref() {
        Some(nonce) if !nonce.is_empty() => {
            // Domain-separate the nonce path from the fallback path so a
            // nonce that happens to equal a serialized fallback can never
            // collide.
            0u8.hash(&mut h);
            nonce.hash(&mut h);
            (h.finish(), false)
        }
        _ => {
            1u8.hash(&mut h);
            req.peer_discriminator.hash(&mut h);
            req.argv.hash(&mut h);
            req.cwd.hash(&mut h);
            req.tag.hash(&mut h);
            (h.finish(), true)
        }
    }
}

/// Builds compact JSON audit metadata for a command's FULL argv.
///
/// SECURITY-CRITICAL (TC-4). Reuses the security-approved `redact_argv`
/// masking core to redact credential spans across EVERY argv item (audit
/// forensics keep all items; only secret spans are masked to `<redacted>`),
/// then per-item char-boundary truncates each to `ARGV_HEAD_ITEM_BYTES`. The
/// result is capped at `MAX_AUDIT_METADATA_BYTES` on insert by the persistent
/// audit layer. Unlike the operator-facing head, this is UNBOUNDED in item
/// count, so a secret past the head window is still masked. The char-boundary
/// truncation also makes this panic-free on multibyte input.
fn format_argv_metadata(argv: &[String]) -> String {
    let v = redact_argv(argv, None);
    serde_json::json!({ "argv": v }).to_string()
}

/// Number of leading argv tokens surfaced in a redacted head: the program
/// name plus up to two following tokens.
const ARGV_HEAD_ITEMS: usize = 3;
/// Per-item byte cap applied AFTER masking, on a UTF-8 char boundary.
const ARGV_HEAD_ITEM_BYTES: usize = 128;
/// Replacement token substituted for any masked secret span.
const ARGV_HEAD_REDACTED: &str = "<redacted>";

/// Flags whose VALUE is a pure secret to mask WHOLLY -- the canonical names
/// plus common real-world aliases (an external review found `--api-key=` /
/// `--passwd=` leaking because only the canonical four were listed). Lowercased;
/// matching is case-insensitive. Used by BOTH the Layer-A space form (mask the
/// next token) and the Layer-B attached `--flag=value` form, from this single
/// source so the two layers cannot drift. `-u`/`--user`/`--proxy-user` (colon-
/// gated) and `-h`/`--header` (header rule) are handled separately, NOT here.
const SECRET_VALUE_FLAGS: &[&str] = &[
    "-p",
    "--password",
    "--passwd",
    "--pass",
    "--pwd",
    "--token",
    "--secret",
    "--key",
    "--api-key",
    "--apikey",
    "--api_key",
    "--access-key",
    "--accesskey",
    "--access_key",
    "--secret-key",
    "--secretkey",
    "--auth",
    "--authorization",
    "--bearer",
    "--credential",
    "--credentials",
];

/// Shared credential-redaction core for argv views (TC-4).
///
/// Applies the SECURITY-APPROVED two-layer masking (Layer A flag look-ahead +
/// Layer B per-token inline scan) and then a per-item char-boundary truncation
/// to `ARGV_HEAD_ITEM_BYTES`. The `max_items` bound selects the scope:
///
/// * `Some(n)` — operate on the first `n` tokens only (operator-facing head).
/// * `None` — operate on the FULL argv (audit forensics: keep every item,
///   mask only secret spans).
///
/// The masking and truncation behaviour is identical in both modes; only the
/// number of tokens considered differs. Never panics on empty / single-element
/// / malformed / multibyte input (truncation respects UTF-8 char boundaries).
fn redact_argv(argv: &[String], max_items: Option<usize>) -> Vec<String> {
    // Working copy: bounded head (`Some(n)`) or the full argv (`None`).
    let mut head: Vec<String> =
        max_items.map_or_else(|| argv.to_vec(), |n| argv.iter().take(n).cloned().collect());

    // LAYER A — flag look-ahead (space-separated value form). Walk the working
    // copy; when token[i] names a secret-value flag, mask token[i+1] if present.
    let len = head.len();
    for i in 0..len {
        // Only a flag with a token AFTER it (within the working copy) can leak
        // a space-separated value; the `i + 1 < len` guard short-circuits the
        // dangling-flag case without a nested `if`.
        let key = head[i].trim().to_ascii_lowercase();
        match key.as_str() {
            k if i + 1 < len && SECRET_VALUE_FLAGS.contains(&k) => {
                // The following token is a pure secret value (a canonical
                // secret flag or a common alias -- see SECRET_VALUE_FLAGS,
                // e.g. --api-key / --passwd).
                ARGV_HEAD_REDACTED.clone_into(&mut head[i + 1]);
            }
            // Basic-auth `user:pass`. Mask the WHOLE next token (username
            // included — it is itself often sensitive here) ONLY when it
            // carries a `:`. A bare username (no `:`) is left intact, so benign
            // uses like `sort -u file` or `id -u` are untouched. A `uid:gid`
            // value (e.g. `docker -u 1000:1000`) is intentionally over-redacted
            // — a safe trade-off for an operator listing. The `:` check lives
            // in the guard so the value-bearing case is the only match.
            "-u" | "--user" | "--proxy-user" if i + 1 < len && head[i + 1].contains(':') => {
                ARGV_HEAD_REDACTED.clone_into(&mut head[i + 1]);
            }
            "-h" | "--header" if i + 1 < len => {
                // The following token is a header line; mask only its
                // credential via the header rule (Layer B rule 5). `-h` is
                // treated as `--header` here (NOT `--help`); header-flag values
                // are masked defensively, covering custom secret headers too.
                head[i + 1] = mask_header_credential(&head[i + 1]);
            }
            _ => {}
        }
    }

    // LAYER B — per-token inline scan. Applied to EVERY token independently and
    // idempotently (a token already masked by Layer A is unchanged here).
    for tok in &mut head {
        *tok = mask_token_inline(tok);
    }

    // Truncate AFTER masking, on a char boundary, so a long surviving token
    // (e.g. a URL) cannot blow the per-item budget and cannot split UTF-8.
    for tok in &mut head {
        if tok.len() > ARGV_HEAD_ITEM_BYTES {
            let mut end = ARGV_HEAD_ITEM_BYTES;
            while end > 0 && !tok.is_char_boundary(end) {
                end -= 1;
            }
            *tok = tok[..end].to_owned();
        }
    }

    head
}

/// Builds a bounded, REDACTED view of a command's leading argv tokens for
/// operator-facing probe listings.
///
/// SECURITY-CRITICAL (TC-4). The output is the program name plus up to two
/// following tokens, with credential spans masked to `<redacted>` and each
/// surviving item truncated to `ARGV_HEAD_ITEM_BYTES` on a char boundary.
/// Masking is applied in two idempotent layers:
///
/// * Layer A — flag look-ahead: a known secret-value flag (e.g. `--password`,
///   `-p`, `--token`) masks the NEXT token's value (whole token for credential
///   flags, header rule for `-h`/`--header`, basic-auth `user:pass` for
///   `-u`/`--user`/`--proxy-user`). `-h` is treated as `--header` (NOT
///   `--help`); header-flag values are masked defensively, covering custom
///   secret headers (`X-Api-Key:`, `X-Vault-Token:`) as well as
///   `Authorization`/`Bearer`. A `-u`/`--user` value is masked only when it
///   carries a `:` (the `user:pass` form), leaving bare usernames intact.
/// * Layer B — per-token inline scan: every token is independently scanned for
///   attached secret flags (`--password=`, `-p`, `-u<value>`, `--user=`),
///   attached header flags (`--header=`), URL userinfo passwords,
///   `Authorization`/`Bearer` header credentials, and env-style secret
///   `KEY=VALUE` assignments. Secret-flag values are masked WHOLLY before the
///   partial URL-userinfo rule runs, so a URL-shaped secret-flag value cannot
///   escape with only its inner password masked.
///
/// Program name and flag NAMES always stay visible; only the secret SPAN is
/// masked. Defeat-proof against casing, `=` vs space, short vs long flags,
/// URL userinfo, and rule ordering. Never panics on empty / single-element /
/// malformed input.
pub(crate) fn redact_argv_head(argv: &[String]) -> Vec<String> {
    redact_argv(argv, Some(ARGV_HEAD_ITEMS))
}

/// Rule (e) helper: decide whether an env-style `KEY=VALUE` key names a secret
/// whose value must be masked. `key_lower` is the already-lowercased portion of
/// the token before the first `=`. Matches a curated set of well-known bare
/// secret keys (`password`, `token`, `apikey`, ...) plus a suffix family
/// (`*_token`, `*_secret`, `*_password`, `*_key`, ...). Over-matching benign
/// keys is the SAFE direction for an operator-facing display, so the lists err
/// toward inclusion rather than precision.
fn env_key_is_secret(key_lower: &str) -> bool {
    const EXACT: [&str; 13] = [
        "password",
        "passwd",
        "pwd",
        "pgpassword",
        "mysql_pwd",
        "token",
        "secret",
        "apikey",
        "api_key",
        "access_key",
        "secret_key",
        "auth",
        "authorization",
    ];
    const SUFFIX: [&str; 10] = [
        "_token",
        "_secret",
        "_password",
        "_passwd",
        "_pwd",
        "_key",
        "_apikey",
        "_api_key",
        "_access_key",
        "_secret_key",
    ];
    EXACT.contains(&key_lower) || SUFFIX.iter().any(|s| key_lower.ends_with(s))
}

/// Layer B per-token masking. Applies, in a SECURITY-ORDERED sequence, the
/// attached-secret-flag, attached basic-auth, attached-header, URL-userinfo,
/// bare-header-credential, and env-`KEY=VALUE` rules. Each rule is
/// case-insensitive on its key/scheme/label and preserves the visible prefix
/// (`--flag=`, `-p`, `-u`, scheme/user/host, `Authorization:`, `KEY=`) while
/// replacing only the secret span with `<redacted>`. Idempotent.
///
/// Ordering invariant: a token that starts with a secret-value FLAG prefix is
/// masked WHOLLY by the flag rules (1-3) BEFORE the partial URL-userinfo rule
/// (4) can run. So `--password=postgres://u:pw@host` is fully opaque, never the
/// weaker URL-only mask that would leave `--password=postgres://u:` and
/// `@host` visible.
fn mask_token_inline(token: &str) -> String {
    let lower = token.to_ascii_lowercase();

    // Rule (1): attached long secret flag `--password=...` etc. The value is a
    // pure secret regardless of shape (including URL-shaped), so the ENTIRE
    // remainder after the `--flag=` prefix is redacted. Runs BEFORE the
    // URL-userinfo rule so a URL value cannot escape with only its inner
    // password masked.
    // Iterates the shared SECRET_VALUE_FLAGS (canonical names + aliases like
    // --api-key / --passwd) so the attached `--flag=value` form cannot drift
    // from the Layer-A space form. The `= ` requirement both selects the long
    // `--flag=` form (the short `-p<value>` form is rule (2a) below) and makes
    // matching collision-safe: `--pass` cannot mis-fire on `--password=` since
    // the longer flag's own arm matches the `=`.
    for flag in SECRET_VALUE_FLAGS {
        if let Some(after) = lower.strip_prefix(flag)
            && after.starts_with('=')
        {
            // Keep the literal `--flag=` prefix from the ORIGINAL token
            // (preserves casing), replace the whole value.
            let prefix = &token[..=flag.len()];
            return format!("{prefix}{ARGV_HEAD_REDACTED}");
        }
    }

    // Rule (2a): attached short flag `-p<value>` (e.g. `mysql -ppassw0rd`). Not
    // `--`; lowercased starts with `-p`; length > 2 means a value is attached.
    if token.starts_with('-')
        && !token.starts_with("--")
        && lower.starts_with("-p")
        && token.len() > 2
    {
        let prefix = &token[..2];
        return format!("{prefix}{ARGV_HEAD_REDACTED}");
    }

    // Rule (2b): attached short basic-auth flag `-u<value>` (e.g. curl
    // `-ualice:s3cr3t`). Masked ONLY when the value carries a `:` (the
    // `user:pass` form). A bare `-ualice` (no colon) is left intact, and a
    // benign `uid:gid` (e.g. `-u1000:1000`) is intentionally over-redacted —
    // a safe trade-off for an operator listing.
    if token.starts_with('-')
        && !token.starts_with("--")
        && lower.starts_with("-u")
        && token.len() > 2
        && token[2..].contains(':')
    {
        let prefix = &token[..2];
        return format!("{prefix}{ARGV_HEAD_REDACTED}");
    }

    // Rule (2c): attached long basic-auth flag `--user=<value>` /
    // `--proxy-user=<value>`. Masked ONLY when the value carries a `:` (the
    // `user:pass` form); a bare username is left intact.
    for flag in ["--user=", "--proxy-user="] {
        if lower.starts_with(flag) {
            let value = &token[flag.len()..];
            if value.contains(':') {
                let prefix = &token[..flag.len()];
                return format!("{prefix}{ARGV_HEAD_REDACTED}");
            }
        }
    }

    // Rule (3): attached header flag `--header=<value>`. The flag prefix stays
    // visible; the value is run through the (defensive) header rule, which
    // masks `Authorization`/`Bearer` AND custom secret headers
    // (`X-Api-Key:`, `X-Vault-Token:`, ...).
    if lower.starts_with("--header=") {
        let prefix = &token[.."--header=".len()];
        let value = &token["--header=".len()..];
        return format!("{prefix}{}", mask_header_credential(value));
    }

    // Rule (4): URL userinfo password `scheme://user:pass@authority...` for a
    // BARE connection string in a non-secret-flag position. Secret-flag values
    // were already masked wholly above, so this only narrows the password span
    // of operator-supplied bare URLs.
    if let Some(masked) = mask_url_userinfo(token) {
        return masked;
    }

    // Rule (5): header credential carried inline in a single bare token (e.g. a
    // `--header`-less `Authorization: Bearer ...` token, or the bare value).
    // Gated on a `bearer `/`authorization:` boundary so the header rule's
    // custom-header fallback cannot trigger on generic colon-bearing tokens
    // here — only the `-H`/`--header`/`--header=` callers reach that fallback.
    if lower.contains("bearer ") || lower.contains("authorization:") {
        return mask_header_credential(token);
    }

    // Rule (6): env-style `KEY=VALUE` where KEY names a secret (bare well-known
    // key or a secret suffix). Keeps `KEY=` visible, redacts the value.
    if let Some(eq) = token.find('=') {
        let key_lower = token[..eq].to_ascii_lowercase();
        if env_key_is_secret(&key_lower) {
            let prefix = &token[..=eq]; // includes the '='
            return format!("{prefix}{ARGV_HEAD_REDACTED}");
        }
    }

    token.to_owned()
}

/// Rule (4) helper: mask the password span in a `scheme://user:pass@authority`
/// URL. Keeps scheme, user, host, and path; replaces only the span between the
/// FIRST `:` of the userinfo and the `@` that terminates the userinfo. Returns
/// `None` when the token is not a URL with a `user:pass@` userinfo. Never
/// panics on malformed input.
///
/// Per RFC 3986 the userinfo precedes the LAST `@` in the authority (a `@` may
/// legally appear inside the password), so the boundary is `rfind('@')`. The
/// password then starts at the FIRST `:` within that userinfo. Example:
/// `postgres://u:pa@ss@host/db` -> `postgres://u:<redacted>@host/db`.
fn mask_url_userinfo(token: &str) -> Option<String> {
    let scheme_end = token.find("://")?;
    let after_scheme = scheme_end + 3;
    let rest = &token[after_scheme..];

    // The authority runs until the first `/`, `?`, or `#`.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];

    // Userinfo precedes the LAST `@` in the authority (a literal `@` may appear
    // inside the password). The FIRST `:` within that userinfo splits
    // `user:pass`.
    let at = authority.rfind('@')?;
    let userinfo = &authority[..at];
    let colon = userinfo.find(':')?;

    // Reconstruct: keep everything up to and including the first `:` of the
    // userinfo, redact the password, keep `@` + the rest of the token. Offsets
    // are relative to `token`, so add `after_scheme` to the authority-relative
    // `colon`/`at` positions.
    let keep_prefix = &token[..=after_scheme + colon]; // up to and incl `:`
    let keep_suffix = &token[after_scheme + at..]; // from the terminating `@`
    Some(format!("{keep_prefix}{ARGV_HEAD_REDACTED}{keep_suffix}"))
}

/// Rule (5) helper: mask the credential in an HTTP header value.
/// Case-insensitive. Prefers a `bearer ` boundary (keeps the scheme label),
/// else falls back to an `authorization:` boundary; everything after the kept
/// boundary is replaced with `<redacted>`, with a single separating space kept
/// for readability.
///
/// Final fallback: when NEITHER `bearer ` nor `authorization:` is present but
/// the token still contains a `:` (a custom secret header such as
/// `X-Api-Key: SECRET` or `X-Vault-Token: hvs....`), keep the header NAME up to
/// and including the first `:`, then redact the value (one separating space
/// preserved). This fallback is reachable ONLY via the `-H`/`--header`/
/// `--header=` callers — the bare Layer-B rule (5) caller gates entry on a
/// `bearer `/`authorization:` boundary already being present, so generic
/// colon-bearing tokens are NOT affected. It intentionally over-redacts benign
/// custom headers (e.g. `Content-Type: application/json`) when passed via
/// `-H`, which is a safe trade-off for an operator listing. If no boundary and
/// no `:` are present the token is returned unchanged.
fn mask_header_credential(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    if let Some(pos) = lower.find("bearer ") {
        // Keep up to and including `bearer ` (with its trailing space); redact
        // the credential that follows.
        let keep_end = pos + "bearer ".len();
        let prefix = &token[..keep_end];
        return format!("{prefix}{ARGV_HEAD_REDACTED}");
    }
    if let Some(pos) = lower.find("authorization:") {
        // Keep up to and including `authorization:`; redact everything after,
        // re-inserting one space separator when the original had one.
        let keep_end = pos + "authorization:".len();
        let prefix = &token[..keep_end];
        let sep = if token[keep_end..].starts_with(' ') {
            " "
        } else {
            ""
        };
        return format!("{prefix}{sep}{ARGV_HEAD_REDACTED}");
    }
    // Fallback for custom secret headers `Name: value`. Keep up to and
    // including the first `:`, redact the value, preserve one separating space.
    if let Some(colon) = token.find(':') {
        let keep_end = colon + 1; // include the `:`
        let prefix = &token[..keep_end];
        let sep = if token[keep_end..].starts_with(' ') {
            " "
        } else {
            ""
        };
        return format!("{prefix}{sep}{ARGV_HEAD_REDACTED}");
    }
    token.to_owned()
}

#[cfg(test)]
mod redact_tests {
    use super::redact_argv_head;

    /// Joins a redacted head into one string for substring assertions. Asserts
    /// here are on the secret PATTERN, never on length, so a longer/shorter
    /// `<redacted>` token can never accidentally satisfy a "secret gone" check.
    fn joined(argv: &[&str]) -> String {
        let owned: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
        redact_argv_head(&owned).join("\u{1f}") // unit-separator: no real token contains it
    }

    #[test]
    fn header_bearer_value_is_masked_label_kept() {
        let out = joined(&["curl", "-H", "Authorization: Bearer abc123", "https://x"]);
        assert!(
            out.contains("<redacted>"),
            "credential must be redacted: {out}"
        );
        assert!(
            !out.contains("abc123"),
            "raw bearer token must not survive: {out}"
        );
        assert!(out.contains("curl"), "program name stays visible: {out}");
        assert!(out.contains("-H"), "flag name stays visible: {out}");
        // The `Bearer` scheme label is informational, not secret — keep it.
        assert!(
            out.contains("Bearer"),
            "Bearer scheme label stays visible: {out}"
        );
    }

    #[test]
    fn url_userinfo_password_is_masked_host_kept() {
        let out = joined(&["psql", "postgres://u:pw@h/db"]);
        assert!(!out.contains(":pw@"), "password span must be masked: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
        assert!(out.contains("psql"), "program name stays visible: {out}");
        assert!(out.contains("postgres://"), "scheme stays visible: {out}");
        assert!(out.contains("@h/db"), "host and path stay visible: {out}");
        assert!(out.contains("u:"), "username stays visible: {out}");
    }

    #[test]
    fn attached_short_password_flag_is_masked() {
        let out = joined(&["mysql", "-ppassw0rd"]);
        assert!(
            !out.contains("passw0rd"),
            "attached password must be masked: {out}"
        );
        assert!(out.contains("mysql"), "program name stays visible: {out}");
        assert!(out.contains("-p"), "the -p flag stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn attached_api_key_alias_flag_value_is_masked() {
        // External review: `--api-key=` must mask like `--password=` (it was leaking).
        let head = redact_argv_head(&[
            "curl".to_owned(),
            "--api-key=SECRETKEY123".to_owned(),
            "https://x".to_owned(),
        ]);
        let joined = head.join(" ");
        assert!(!joined.contains("SECRETKEY123"), "secret leaked: {joined}");
        assert!(joined.contains("--api-key="), "flag name kept: {joined}");
        assert!(joined.contains("<redacted>"), "redaction marker: {joined}");
    }

    #[test]
    fn attached_passwd_alias_flag_value_is_masked() {
        let head = redact_argv_head(&["app".to_owned(), "--passwd=hunter2".to_owned()]);
        let joined = head.join(" ");
        assert!(!joined.contains("hunter2"), "secret leaked: {joined}");
        assert!(joined.contains("--passwd="), "flag name kept: {joined}");
    }

    #[test]
    fn space_separated_secret_flag_aliases_are_masked() {
        for (flag, secret) in [
            ("--api-key", "SECRETKEY123"),
            ("--passwd", "hunter2"),
            ("--access-key", "AKIA_SECRET"),
            ("--bearer", "tok_SECRET"),
        ] {
            let head = redact_argv_head(&[
                "curl".to_owned(),
                flag.to_owned(),
                secret.to_owned(),
                "https://x".to_owned(),
            ]);
            let joined = head.join(" ");
            assert!(
                !joined.contains(secret),
                "secret leaked for {flag}: {joined}"
            );
            assert!(joined.contains(flag), "flag kept for {flag}: {joined}");
            assert!(
                joined.contains("<redacted>"),
                "redaction marker for {flag}: {joined}"
            );
        }
    }

    #[test]
    fn casing_does_not_bypass_alias_flag_masking() {
        // `--pass` must not mis-fire on the longer `--password=` either.
        let head = redact_argv_head(&["app".to_owned(), "--API-KEY=ZZZSECRET".to_owned()]);
        assert!(
            !head.join(" ").contains("ZZZSECRET"),
            "casing bypass: {head:?}"
        );
        let canon = redact_argv_head(&["app".to_owned(), "--password=PWSECRET".to_owned()]);
        assert!(
            !canon.join(" ").contains("PWSECRET"),
            "canonical: {canon:?}"
        );
    }

    #[test]
    fn space_separated_long_password_flag_value_is_masked() {
        let out = joined(&["app", "--password", "s3cr3t"]);
        assert!(
            !out.contains("s3cr3t"),
            "following secret must be masked: {out}"
        );
        assert!(out.contains("--password"), "flag name stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn attached_long_token_flag_value_is_masked() {
        let out = joined(&["app", "--token=abcd"]);
        assert!(
            !out.contains("abcd"),
            "attached token value must be masked: {out}"
        );
        assert!(
            out.contains("--token="),
            "flag name + '=' stays visible: {out}"
        );
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn env_style_password_assignment_is_masked() {
        let out = joined(&["run", "DB_PASSWORD=hunter2"]);
        assert!(
            !out.contains("hunter2"),
            "env secret value must be masked: {out}"
        );
        assert!(
            out.contains("DB_PASSWORD="),
            "env key + '=' stays visible: {out}"
        );
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn casing_does_not_bypass_header_masking() {
        // Lowercased flag name AND lowercased header label/scheme must still mask.
        let out = joined(&["curl", "--HEADER", "authorization: bearer ZZZ"]);
        assert!(
            !out.contains("ZZZ"),
            "casing must not bypass redaction: {out}"
        );
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn head_is_bounded_to_three_items() {
        let owned: Vec<String> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let head = redact_argv_head(&owned);
        assert_eq!(head.len(), 3, "only program + 2 tokens surface: {head:?}");
        assert_eq!(head, vec!["a", "b", "c"]);
    }

    #[test]
    fn truncation_is_char_boundary_safe_after_masking() {
        // A surviving token longer than the per-item cap must truncate on a
        // char boundary (no UTF-8 split, no panic).
        let long = format!("arg{}", "é".repeat(200)); // multibyte tail
        let head = redact_argv_head(&[long]);
        assert!(
            head[0].len() <= 128,
            "item truncated to cap: {}",
            head[0].len()
        );
        // If we got here without panicking, the slice respected char boundaries.
    }

    #[test]
    fn no_panic_on_degenerate_inputs() {
        // Empty argv.
        assert!(redact_argv_head(&[]).is_empty());
        // Single element.
        assert_eq!(redact_argv_head(&["only".to_owned()]), vec!["only"]);
        // Secret flag with NO following value (look-ahead must not panic).
        let dangling = redact_argv_head(&["app".to_owned(), "--password".to_owned()]);
        assert_eq!(dangling, vec!["app", "--password"]);
        // URL with an `@` but no userinfo password (`scheme://noatuserinfo/x`
        // has no `@` at all -> no mask, no panic).
        let no_userinfo =
            redact_argv_head(&["app".to_owned(), "scheme://noatuserinfo/x".to_owned()]);
        assert_eq!(no_userinfo[1], "scheme://noatuserinfo/x");
        // `=` with empty value must not panic and must keep the key visible.
        let empty_val = redact_argv_head(&["run".to_owned(), "API_TOKEN=".to_owned()]);
        assert_eq!(empty_val[1], "API_TOKEN=<redacted>");
    }

    // ---- FIX 2: bare env-secret keys (no `_suffix` required) ----

    #[test]
    fn bare_env_password_key_is_masked() {
        let out = joined(&["app", "PASSWORD=hunter2"]);
        assert!(!out.contains("hunter2"), "bare PASSWORD value leaks: {out}");
        assert!(out.contains("PASSWORD="), "env key stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn bare_env_pgpassword_key_is_masked() {
        let out = joined(&["psql", "PGPASSWORD=topsecret"]);
        assert!(!out.contains("topsecret"), "PGPASSWORD value leaks: {out}");
        assert!(out.contains("PGPASSWORD="), "env key stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn bare_env_mysql_pwd_key_is_masked() {
        let out = joined(&["m", "MYSQL_PWD=abc"]);
        assert!(!out.contains("=abc"), "MYSQL_PWD value leaks: {out}");
        assert!(out.contains("MYSQL_PWD="), "env key stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn bare_env_token_key_is_masked() {
        let out = joined(&["app", "TOKEN=tkn123"]);
        assert!(!out.contains("tkn123"), "bare TOKEN value leaks: {out}");
        assert!(out.contains("TOKEN="), "env key stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn bare_env_apikey_key_is_masked() {
        let out = joined(&["app", "APIKEY=zzz"]);
        assert!(!out.contains("=zzz"), "bare APIKEY value leaks: {out}");
        assert!(out.contains("APIKEY="), "env key stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn bare_env_secret_key_is_masked() {
        let out = joined(&["app", "SECRET=shh"]);
        assert!(!out.contains("=shh"), "bare SECRET value leaks: {out}");
        assert!(out.contains("SECRET="), "env key stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    // ---- FIX 3: basic-auth `-u`/`--user user:pass` ----

    #[test]
    fn space_separated_basic_auth_user_is_masked() {
        let out = joined(&["curl", "-u", "alice:s3cr3t", "https://x"]);
        assert!(!out.contains("s3cr3t"), "basic-auth pass leaks: {out}");
        assert!(!out.contains("alice"), "basic-auth user leaks: {out}");
        assert!(out.contains("curl"), "program name stays visible: {out}");
        assert!(out.contains("-u"), "flag name stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn attached_short_basic_auth_user_is_masked() {
        let out = joined(&["curl", "-ualice:s3cr3t"]);
        assert!(!out.contains("s3cr3t"), "basic-auth pass leaks: {out}");
        assert!(!out.contains("alice"), "basic-auth user leaks: {out}");
        assert!(out.contains("-u"), "flag prefix stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn space_separated_long_user_flag_is_masked() {
        let out = joined(&["curl", "--user", "bob:pw123"]);
        assert!(!out.contains("pw123"), "basic-auth pass leaks: {out}");
        assert!(out.contains("--user"), "flag name stays visible: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn attached_long_user_flag_is_masked() {
        let out = joined(&["curl", "--user=bob:pw123"]);
        assert!(!out.contains("pw123"), "basic-auth pass leaks: {out}");
        assert!(
            out.contains("--user="),
            "flag name + '=' stays visible: {out}"
        );
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    // ---- FIX 3: NON-over-mask sanity (bare `-u` with no colon) ----

    #[test]
    fn bare_sort_u_does_not_mask_following_arg() {
        let out = joined(&["sort", "-u", "file.txt"]);
        assert!(
            out.contains("file.txt"),
            "non-secret -u value must stay visible: {out}"
        );
        assert!(
            !out.contains("<redacted>"),
            "nothing should be masked: {out}"
        );
    }

    #[test]
    fn bare_id_u_does_not_panic_and_stays_visible() {
        let out = joined(&["id", "-u"]);
        assert!(out.contains("-u"), "dangling -u stays visible: {out}");
        assert!(
            !out.contains("<redacted>"),
            "nothing should be masked: {out}"
        );
    }

    // ---- FIX 1: secret-flag + URL value -> WHOLE value masked ----

    #[test]
    fn password_flag_url_value_is_wholly_masked() {
        let out = joined(&["app", "--password=postgres://dbuser:pw@db.internal/prod"]);
        assert!(!out.contains("dbuser"), "URL user leaks: {out}");
        assert!(!out.contains("pw@"), "URL pass leaks: {out}");
        assert!(!out.contains("db.internal"), "URL host leaks: {out}");
        assert!(
            out.contains("--password="),
            "flag name + '=' stays visible: {out}"
        );
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    // ---- FIX 4: custom header credentials ----

    #[test]
    fn custom_header_x_api_key_is_masked() {
        let out = joined(&["curl", "-H", "X-Api-Key: SECRETKEY", "https://x"]);
        assert!(
            !out.contains("SECRETKEY"),
            "custom header value leaks: {out}"
        );
        assert!(
            out.contains("X-Api-Key:"),
            "header name stays visible: {out}"
        );
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn attached_custom_header_vault_token_is_masked() {
        let out = joined(&["curl", "--header=X-Vault-Token: hvs.SEC"]);
        assert!(!out.contains("hvs.SEC"), "vault token leaks: {out}");
        assert!(
            out.contains("X-Vault-Token:"),
            "header name stays visible: {out}"
        );
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    // ---- FIX 5: embedded-`@` password (last-`@` userinfo boundary) ----

    #[test]
    fn url_userinfo_with_embedded_at_in_password_is_masked() {
        let out = joined(&["psql", "postgres://u:pa@ss@host/db"]);
        assert!(!out.contains("pa@ss"), "embedded-@ password leaks: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
        assert!(
            out.contains("postgres://u:"),
            "scheme+user stay visible: {out}"
        );
        assert!(out.contains("@host"), "host structure stays visible: {out}");
    }

    // ---- AUDIT-LOG surface (format_argv_metadata): FULL-argv redaction ----

    /// Helper: owns a `&[&str]` argv and returns the audit JSON string.
    fn audit_json(argv: &[&str]) -> String {
        let owned: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
        super::format_argv_metadata(&owned)
    }

    #[test]
    fn format_argv_metadata_redacts_secret_at_deep_index() {
        // The secret sits well past the 3-item operator head, proving the audit
        // surface is UNBOUNDED in item count yet still masks deep secrets.
        let out = audit_json(&[
            "myprog",
            "--verbose",
            "--out",
            "/tmp/x",
            "--retries",
            "3",
            "--password",
            "s3cr3t-DEEP",
            "tail",
        ]);
        assert!(
            !out.contains("s3cr3t-DEEP"),
            "deep secret must be masked: {out}"
        );
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
        // Non-secret items past the head window survive (unbounded coverage).
        assert!(out.contains("myprog"), "program name stays visible: {out}");
        assert!(
            out.contains("--retries"),
            "deep non-secret flag stays visible: {out}"
        );
        assert!(out.contains("tail"), "trailing arg stays visible: {out}");
    }

    #[test]
    fn format_argv_metadata_masks_url_userinfo_and_env_secret_anywhere() {
        let out = audit_json(&[
            "run",
            "step1",
            "DATABASE_URL=postgres://u:pw@host/db",
            "DB_PASSWORD=hunter2",
        ]);
        assert!(!out.contains(":pw@"), "URL userinfo password leaks: {out}");
        assert!(!out.contains("hunter2"), "env secret value leaks: {out}");
        assert!(
            out.contains("<redacted>"),
            "redaction marker present: {out}"
        );
    }

    #[test]
    fn format_argv_metadata_no_panic_on_multibyte() {
        // A single item far exceeding the 128-byte per-item cap, built from a
        // multibyte char so any naive `s[..128]` slice would split UTF-8 and
        // panic. The char-boundary truncation in `redact_argv` must keep this safe.
        let big = "\u{1f600}".repeat(64); // 64 * 4 bytes = 256 bytes, all multibyte
        let owned = vec!["prog".to_owned(), big];
        let out = super::format_argv_metadata(&owned); // must not panic
        // Result must still be parseable JSON with an `argv` array.
        let parsed: serde_json::Value =
            serde_json::from_str(&out).expect("audit metadata must be valid JSON");
        assert!(
            parsed.get("argv").and_then(|a| a.as_array()).is_some(),
            "argv array present: {out}"
        );
    }

    #[test]
    fn redact_argv_head_still_bounded_to_three() {
        // Sanity: the operator head path is unchanged — only the first 3 items
        // survive, so an item at index 3+ is dropped entirely (not just masked).
        let owned: Vec<String> = ["a", "b", "c", "DROPME"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let head = redact_argv_head(&owned);
        assert_eq!(head.len(), 3, "head bounded to ARGV_HEAD_ITEMS=3: {head:?}");
        assert!(
            !head.iter().any(|t| t.contains("DROPME")),
            "4th item must be dropped by the bounded head: {head:?}"
        );
    }
}
