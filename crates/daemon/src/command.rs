// SPDX-License-Identifier: Apache-2.0
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
//! commands remain deferred to TC44; this runtime does NOT touch
//! stdin or pseudo-terminals.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use terminal_commander_core::{
    BucketConfig, BucketId, ContextRingManager, EventDraft, JobConfig, JobId, JobManager,
    JobRecord, ProbeId, RuleDefinition,
};
use terminal_commander_probes::{EventSink, ProcessProbe, ProcessProbeConfig, ProcessProbeMetrics};
use terminal_commander_sifters::SifterRuntime;
use terminal_commander_store::AuditEntry;

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
}

/// Bounded response shape. Carries identifiers and counters, never
/// raw stdout/stderr.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStartResponse {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: ProbeId,
    /// Initial bucket cursor: clients pass this to `bucket_events_since`.
    pub cursor: u64,
}

/// Bounded status shape. Counters + final exit state only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStatusResponse {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: ProbeId,
    pub state: terminal_commander_core::JobState,
    pub frames_total: u64,
    pub frames_stdout: u64,
    pub frames_stderr: u64,
    pub bytes_total: u64,
    pub events_emitted: u64,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub duration_ms: Option<u64>,
}

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
    fn emit(&self, draft: EventDraft) {
        let bucket_id = draft.bucket_id;
        // Append through the router so the audit row lands.
        let _ = self.router.bucket_append(bucket_id, draft);
        self.jobs.record_event(self.job_id);
    }
}

/// Per-job state tracked by [`CommandRuntime`]. Carries the live
/// counters, the rebind-able sifter handle, and the per-call inline
/// rules so a global activation change can recompute the merged
/// rule set without losing the inline rules the operator passed at
/// `start_combed` time (TC42b).
#[derive(Debug, Clone)]
struct JobBinding {
    metrics: ProcessProbeMetrics,
    sifter: Arc<terminal_commander_sifters::SifterRuntime>,
    inline_rules: Vec<terminal_commander_core::RuleDefinition>,
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
    ) -> Self {
        let profile_label = match policy.profile {
            PolicyProfile::DeveloperLocal => "developer_local".to_owned(),
            PolicyProfile::RepoOnly => "repo_only".to_owned(),
            PolicyProfile::ReadOnlyObserver => "read_only_observer".to_owned(),
            PolicyProfile::AdminDebug => "admin_debug".to_owned(),
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

    /// `command_start_combed` entry point. Validates, gates on
    /// policy, builds a bucket + probe, spawns the child, returns a
    /// bounded response.
    ///
    /// MUST be called from within a tokio runtime; ProcessProbe::spawn
    /// uses tokio::process::Command.
    #[allow(clippy::too_many_lines)] // sequential pipeline; splitting it hurts readability
    pub fn start_combed(
        &self,
        req: CommandStartRequest,
    ) -> Result<CommandStartResponse, CommandError> {
        Self::validate_argv(&req.argv)?;

        // Shell-bridge guard. Runs BEFORE the policy engine so the
        // rejection reason is precise. command_start_combed is not
        // a shell bridge: a future opt-in policy capability is the
        // only sanctioned path to invoke an interpreter, and TC38
        // does NOT add that capability.
        if let Some(shell) = Self::shell_interpreter_basename(&req.argv[0]) {
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

        // Pre-spawn policy gate.
        let cwd_for_policy = req.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
        let verdict = self.policy.evaluate(&PolicyAction::CommandStart {
            argv: &req.argv,
            cwd: cwd_for_policy.as_path(),
        });
        if verdict.decision == PolicyDecision::Deny {
            self.audit(
                "command_rejected",
                &subject_for_argv(&req.argv),
                "deny",
                Some(verdict.reason.clone()),
                Some(format_argv_metadata(&req.argv)),
            );
            return Err(CommandError::PolicyDenied(verdict.reason));
        }

        // Allocate identifiers + bucket.
        let bucket_id = BucketId::new();
        let probe_id = ProbeId::new();
        let job_id = JobId::new();
        let bucket_cfg = req.bucket_config.unwrap_or_default();
        self.router.bucket_create(bucket_id, bucket_cfg)?;

        // Merge globally-active rules (TC42 activation registry) with
        // the per-call inline rules. Same helper TC42b uses on every
        // rebind so semantics stay identical.
        let merged_rules: Vec<RuleDefinition> =
            merge_active_and_inline(&self.activation.snapshot(), &req.rules);
        let sifter = Arc::new(
            SifterRuntime::build(&merged_rules).map_err(|e| CommandError::Sifter(e.to_string()))?,
        );
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

        // Spawn the probe. If this fails, we audit and bail without
        // creating a job record.
        let probe =
            match ProcessProbe::spawn(&req.argv, &probe_cfg, Arc::clone(&self.rings), sifter, sink)
            {
                Ok(p) => p,
                Err(e) => {
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
        tokio::spawn(async move {
            let (final_metrics, outcome) = drive_to_exit(probe).await;
            // Update the stored metrics for command_status callers.
            // The binding's `sifter` and `inline_rules` are
            // preserved so a deactivation racing with the exit
            // still has a sifter handle to swap (the no-op rebuild
            // is harmless on a finished probe).
            if let Some(b) = waiter_live.write().get_mut(&job_id) {
                b.metrics = final_metrics;
            }

            // Lifecycle draft.
            let draft = match outcome {
                ProbeOutcome::Exited { code, signal } => waiter_jobs.finish(job_id, code, signal),
                ProbeOutcome::Cancelled => waiter_jobs.cancel(job_id),
            };

            // Append the lifecycle event to the bucket (router does
            // the audit on bucket_append).
            if let Some(d) = draft.as_ref() {
                let _ = waiter_router.bucket_append(bucket_id, d.clone());
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

    /// Recompute every live job's sifter from the current
    /// activation registry snapshot + that job's stored inline
    /// rules. The probe API is unchanged; the swap happens inside
    /// the `Arc<SifterRuntime>` each job already holds.
    ///
    /// This is the TC42b entry point: an LLM-issued
    /// `registry_activate` / `registry_deactivate` calls this after
    /// updating the activation registry so already-running commands
    /// see the new rule set on future frames. In-flight frames
    /// finish against the snapshot they captured (no fake historical
    /// matches).
    ///
    /// Per-job rebuild failures (e.g. the active set now contains a
    /// rule the sifter cannot compile) leave that job's prior sifter
    /// in place and are counted in the returned report. They are
    /// audited individually via the daemon's persistent audit sink
    /// using the standard `command_runtime` actor.
    pub fn rebind_all_jobs(&self) -> RebindAllReport {
        let active_snapshot = self.activation.snapshot();
        // Take a snapshot of (job_id, sifter_handle, inline_rules)
        // under the read lock so the rebuild loop does not hold the
        // map lock across a regex compile.
        let work: Vec<(
            JobId,
            Arc<terminal_commander_sifters::SifterRuntime>,
            Vec<terminal_commander_core::RuleDefinition>,
        )> = {
            let g = self.live.read();
            g.iter()
                .map(|(jid, binding)| {
                    (
                        *jid,
                        Arc::clone(&binding.sifter),
                        binding.inline_rules.clone(),
                    )
                })
                .collect()
        };
        let mut report = RebindAllReport {
            jobs_considered: u32::try_from(work.len()).unwrap_or(u32::MAX),
            jobs_rebound: 0,
            rebuild_failures: 0,
        };
        for (job_id, sifter, inline_rules) in work {
            let merged = merge_active_and_inline(&active_snapshot, &inline_rules);
            match sifter.rebuild(&merged) {
                Ok(rb) => {
                    report.jobs_rebound = report.jobs_rebound.saturating_add(1);
                    self.audit(
                        "command_sifter_rebind",
                        &job_id.to_wire_string(),
                        "info",
                        None,
                        Some(format!(
                            "{{\"old_rule_count\":{},\"new_rule_count\":{}}}",
                            rb.old_rule_count, rb.new_rule_count
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

    /// `command_status` entry point. Returns bounded counters + the
    /// final exit state for the given job. Never returns raw text.
    pub fn status(&self, job_id: JobId) -> Result<CommandStatusResponse, CommandError> {
        let rec = self
            .jobs
            .get(job_id)
            .ok_or(CommandError::UnknownJob(job_id))?;
        let metrics = self
            .live
            .read()
            .get(&job_id)
            .map(|b| b.metrics.clone())
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
            exit_code: rec.exit_info.as_ref().and_then(|e| e.exit_code),
            signal: rec.exit_info.as_ref().and_then(|e| e.signal.clone()),
            duration_ms: rec.exit_info.as_ref().map(|e| e.duration_ms),
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
    let mut seen: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();
    for r in inline {
        seen.insert((r.id.clone(), r.version));
    }
    let mut out = Vec::with_capacity(active.len() + inline.len());
    for r in active {
        if !seen.contains(&(r.id.clone(), r.version)) {
            out.push(r.clone());
        }
    }
    out.extend(inline.iter().cloned());
    out
}

fn subject_for_argv(argv: &[String]) -> String {
    let head = argv.first().map_or("", String::as_str);
    // Limit subject size — caller has already bounded items, but
    // build a stable short label.
    let trimmed = if head.len() > 256 { &head[..256] } else { head };
    trimmed.to_owned()
}

fn format_argv_metadata(argv: &[String]) -> String {
    // Compact JSON; capped at MAX_AUDIT_METADATA_BYTES on insert by
    // the persistent audit layer. We pre-truncate per-item to keep
    // the metadata small.
    let v: Vec<String> = argv
        .iter()
        .map(|s| {
            let cap = 128usize;
            if s.len() > cap {
                s[..cap].to_owned()
            } else {
                s.clone()
            }
        })
        .collect();
    serde_json::json!({ "argv": v }).to_string()
}
