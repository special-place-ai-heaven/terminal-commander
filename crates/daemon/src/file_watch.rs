// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon-owned file-watch runtime (TC43).
//!
//! `WatchRuntime` is the file-side counterpart to `CommandRuntime`: it
//! creates a bucket, spawns a `FileProbe`, threads a per-watch
//! `SifterRuntime` against the activation registry, and tracks the
//! live (`bucket_id`, `watch_id`, `probe_id`) triple so scoped
//! activations can target a single watch. The runtime is deliberately
//! separate from `CommandRuntime`: TC43 must not touch `command.rs`.
//!
//! Pipeline:
//!
//! ```text
//! WatchRuntime::start
//!   -> PolicyEngine::evaluate(FileWatch)            // path policy gate
//!   -> Router::bucket_create + ProbeId / JobId mint
//!   -> FileProbe::spawn                              // tokio + sifter
//!        | DaemonEventSink (forwards drafts to Router::bucket_append)
//!        | ContextRingManager (raw frames for event_context only)
//!   -> audit row `file_watch_start`
//!   -> waiter task marks the JobManager record on cancel/exit
//! ```
//!
//! Source-status: live (TC43) with polling backend inherited from
//! `crates/probes::file`. Native notify/inotify is explicitly out of
//! scope per the TC43 prep amendment.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use terminal_commander_core::{
    ActivationScope, BucketConfig, BucketError, BucketId, ContextRingManager, EventDraft,
    JobConfig, JobId, JobManager, ProbeId, RuleDefinition,
};
use terminal_commander_probes::{
    EventSink, FileProbe, FileProbeConfig, FileProbeError, FileProbeMetrics,
};
use terminal_commander_sifters::SifterRuntime;
use terminal_commander_store::AuditEntry;

use crate::activation::ActivationRegistry;
use crate::audit::AuditSink;
use crate::policy::{PolicyAction, PolicyDecision, PolicyEngine, PolicyProfile};
use crate::router::Router;

/// Errors raised by the watch runtime.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("policy denied file_watch: {0}")]
    PolicyDenied(String),
    #[error("path does not exist or is not a regular file: {0}")]
    NotFound(PathBuf),
    #[error("bucket create error: {0}")]
    Bucket(#[from] BucketError),
    #[error("sifter build error: {0}")]
    Sifter(String),
    #[error("file probe spawn error: {0}")]
    Spawn(#[from] FileProbeError),
    #[error("unknown watch id: {0}")]
    UnknownWatch(JobId),
}

/// Per-watch binding. Mirrors `command::JobBinding` (TC42b/TC42c)
/// without sharing code so `command.rs` stays untouched.
#[derive(Debug, Clone)]
struct WatchBinding {
    bucket_id: BucketId,
    probe_id: ProbeId,
    path: PathBuf,
    sifter: Arc<SifterRuntime>,
    inline_rules: Vec<RuleDefinition>,
    metrics: Arc<parking_lot::Mutex<FileProbeMetrics>>,
    cancel: Arc<parking_lot::Mutex<Option<FileProbe>>>,
}

/// Identity triple for a live file watch. Returned by
/// [`WatchRuntime::live_watches`] so the IPC scope validator can
/// accept `Bucket` / `Job` / `Probe` scope ids that target a watch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveWatchIdentity {
    pub watch_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: ProbeId,
}

/// Bounded report returned by [`WatchRuntime::rebind_watches_in_scope`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WatchRebindReport {
    pub watches_considered: u32,
    pub watches_rebound: u32,
    pub rebuild_failures: u32,
}

/// Internal work tuple captured under the live-map read lock so the
/// rebuild loop runs without holding the lock. Mirrors
/// `command::RebindWork`. Factored out to stay under the
/// `clippy::type_complexity` threshold.
type WatchRebindWork = (
    JobId,
    BucketId,
    ProbeId,
    Arc<SifterRuntime>,
    Vec<RuleDefinition>,
);

/// EventSink that forwards drafts to the router. Same shape as the
/// command-runtime sink but with a `watch_id` for audit metadata.
struct WatchEventSink {
    router: Arc<Router>,
    metrics: Arc<parking_lot::Mutex<FileProbeMetrics>>,
}

impl std::fmt::Debug for WatchEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchEventSink").finish_non_exhaustive()
    }
}

impl EventSink for WatchEventSink {
    fn emit(&self, draft: EventDraft) -> Option<u64> {
        let bucket_id = draft.bucket_id;
        let ev = self.router.bucket_append(bucket_id, draft).ok()?;
        let mut g = self.metrics.lock();
        g.events_emitted = g.events_emitted.saturating_add(1);
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

/// The watch runtime owned by `DaemonState`. Single instance per
/// daemon process.
pub struct WatchRuntime {
    router: Arc<Router>,
    rings: Arc<ContextRingManager>,
    jobs: Arc<JobManager>,
    audit: Arc<dyn AuditSink>,
    policy: PolicyEngine,
    profile_label: String,
    live: Arc<RwLock<HashMap<JobId, WatchBinding>>>,
    activation: Arc<ActivationRegistry>,
}

impl std::fmt::Debug for WatchRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchRuntime")
            .field("profile", &self.profile_label)
            .finish_non_exhaustive()
    }
}

impl WatchRuntime {
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
            live: Arc::new(RwLock::new(HashMap::default())),
            activation,
        }
    }

    fn audit(
        &self,
        action: &str,
        subject: &str,
        decision: &str,
        reason: Option<String>,
        metadata: Option<String>,
    ) {
        let mut entry = AuditEntry::new(action, subject, decision)
            .with_actor("watch_runtime")
            .with_profile(self.profile_label.clone());
        if let Some(r) = reason {
            entry = entry.with_reason(r);
        }
        if let Some(m) = metadata {
            entry = entry.with_metadata_json(m);
        }
        let _ = self.audit.emit(&entry);
    }

    /// Snapshot of live watches. Used by the IPC scope validator.
    #[must_use]
    pub fn live_watches(&self) -> Vec<LiveWatchIdentity> {
        let g = self.live.read();
        g.iter()
            .map(|(wid, b)| LiveWatchIdentity {
                watch_id: *wid,
                bucket_id: b.bucket_id,
                probe_id: b.probe_id,
            })
            .collect()
    }

    /// Start a file watch. Policy-gates the path, allocates a
    /// `(watch_id, bucket_id, probe_id)` triple, builds the per-watch
    /// `SifterRuntime` against the current scoped activation snapshot,
    /// spawns a `FileProbe` in follow mode, audits the start, and
    /// returns the bounded triple. Must be called from a tokio runtime
    /// because the probe spawn is async.
    pub fn start(
        &self,
        path: PathBuf,
        bucket_cfg: BucketConfig,
        inline_rules: Vec<RuleDefinition>,
        follow_from_beginning: bool,
    ) -> Result<(JobId, BucketId, ProbeId), WatchError> {
        // Path-policy gate. The default-deny suffix list inside
        // PolicyEngine already covers ssh keys, sudoers, .aws, etc.
        let verdict = self
            .policy
            .evaluate(&PolicyAction::FileWatch { path: &path });
        if verdict.decision == PolicyDecision::Deny {
            self.audit(
                "file_watch_start",
                &path.display().to_string(),
                "deny",
                Some(verdict.reason.clone()),
                None,
            );
            return Err(WatchError::PolicyDenied(verdict.reason));
        }
        // Existence check: the polling backend tolerates create-later
        // semantics, but TC43 deliberately requires the file to exist
        // at start time so the LLM gets a fast, honest typed error
        // instead of an open watch that may never emit.
        match std::fs::metadata(&path) {
            Ok(m) if m.is_file() => {}
            _ => {
                self.audit(
                    "file_watch_start",
                    &path.display().to_string(),
                    "error",
                    Some("path is not a regular file".to_owned()),
                    None,
                );
                return Err(WatchError::NotFound(path));
            }
        }

        // Allocate identifiers + bucket.
        let bucket_id = BucketId::new();
        let probe_id = ProbeId::new();
        let watch_id = JobId::new();
        self.router.bucket_create(bucket_id, bucket_cfg)?;

        // Merge scope-resolved active rules with the per-call inline
        // rules. Same helper semantics as TC42c.
        let active_for_watch = self
            .activation
            .snapshot_for_job(bucket_id, watch_id, probe_id);
        let merged_rules: Vec<RuleDefinition> =
            merge_active_and_inline(&active_for_watch, &inline_rules);
        let sifter = Arc::new(
            SifterRuntime::build(&merged_rules).map_err(|e| WatchError::Sifter(e.to_string()))?,
        );

        let metrics = Arc::new(parking_lot::Mutex::new(FileProbeMetrics::default()));
        let sink: Arc<dyn EventSink> = Arc::new(WatchEventSink {
            router: Arc::clone(&self.router),
            metrics: Arc::clone(&metrics),
        });

        let mut cfg = if follow_from_beginning {
            FileProbeConfig::follow_beginning(path.clone(), bucket_id)
        } else {
            FileProbeConfig::follow_end(path.clone(), bucket_id)
        };
        cfg.probe_id = Some(probe_id);
        cfg.poll_interval = Duration::from_millis(120);

        let probe = FileProbe::spawn(cfg, Arc::clone(&self.rings), Arc::clone(&sifter), sink)?;

        // Register a JobManager entry so `bucket_wait` / lifecycle
        // bookkeeping has the same shape as a command job.
        let job_cfg = JobConfig {
            job_id: watch_id,
            argv: vec![format!("file_watch:{}", path.display())],
            bucket_id,
            probe_id,
            grace_secs: 0,
        };
        let _ = self.jobs.start(job_cfg);
        self.jobs.mark_running(watch_id);

        self.live.write().insert(
            watch_id,
            WatchBinding {
                bucket_id,
                probe_id,
                path: path.clone(),
                sifter,
                inline_rules,
                metrics,
                cancel: Arc::new(parking_lot::Mutex::new(Some(probe))),
            },
        );

        self.audit(
            "file_watch_start",
            &watch_id.to_wire_string(),
            "allow",
            None,
            Some(format!(
                "{{\"path\":{},\"bucket_id\":{}}}",
                serde_json::Value::String(path.display().to_string()),
                serde_json::Value::String(bucket_id.to_wire_string())
            )),
        );

        Ok((watch_id, bucket_id, probe_id))
    }

    /// Stop a live watch. Returns the final metrics so the caller can
    /// echo them to the LLM. Cancellation is idempotent.
    pub fn stop(&self, watch_id: JobId) -> Result<(BucketId, FileProbeMetrics), WatchError> {
        let removed = self.live.write().remove(&watch_id);
        let Some(b) = removed else {
            return Err(WatchError::UnknownWatch(watch_id));
        };
        // Cancel the running probe. We deliberately do not block on
        // probe.wait() here because the dispatcher must return
        // promptly; the probe is fire-and-forget after cancel.
        let taken = b.cancel.lock().take();
        if let Some(mut p) = taken {
            p.cancel();
        }
        let metrics = b.metrics.lock().clone();
        // Mark the JobManager record finished so bucket_wait /
        // command_status (if anyone reads it) see a non-Running
        // state. exit_code=0 because the cancel is a clean stop.
        let _ = self.jobs.finish(watch_id, Some(0), None);
        self.audit(
            "file_watch_stop",
            &watch_id.to_wire_string(),
            "info",
            None,
            Some(format!(
                "{{\"frames\":{},\"events\":{},\"bytes\":{}}}",
                metrics.frames_total, metrics.events_emitted, metrics.bytes_total
            )),
        );
        Ok((b.bucket_id, metrics))
    }

    /// Snapshot bounded info about every live watch (for
    /// `file_watch_list`). Metrics are cloned out of each binding's
    /// `Mutex` under a brief lock.
    #[must_use]
    pub fn list(&self) -> Vec<(JobId, BucketId, ProbeId, PathBuf, FileProbeMetrics)> {
        let g = self.live.read();
        g.iter()
            .map(|(wid, b)| {
                let m = b.metrics.lock().clone();
                (*wid, b.bucket_id, b.probe_id, b.path.clone(), m)
            })
            .collect()
    }

    /// Recompute per-watch sifters from the current scoped activation
    /// registry. Mirrors `CommandRuntime::rebind_jobs_in_scope` so a
    /// global or `Bucket`/`Job`/`Probe`-scoped activation change
    /// reaches the matching watches. `None` = every live watch.
    pub fn rebind_watches_in_scope(&self, scope: Option<ActivationScope>) -> WatchRebindReport {
        let work: Vec<WatchRebindWork> = {
            let g = self.live.read();
            g.iter()
                .filter_map(|(wid, b)| {
                    let matches = match scope {
                        None | Some(ActivationScope::Global) => true,
                        Some(s) => s.matches(b.bucket_id, *wid, b.probe_id),
                    };
                    if !matches {
                        return None;
                    }
                    Some((
                        *wid,
                        b.bucket_id,
                        b.probe_id,
                        Arc::clone(&b.sifter),
                        b.inline_rules.clone(),
                    ))
                })
                .collect()
        };
        let mut report = WatchRebindReport {
            watches_considered: u32::try_from(work.len()).unwrap_or(u32::MAX),
            ..WatchRebindReport::default()
        };
        let scope_label = scope.map_or("any", |s| s.kind_label());
        for (watch_id, bucket_id, probe_id, sifter, inline_rules) in work {
            let active = self
                .activation
                .snapshot_for_job(bucket_id, watch_id, probe_id);
            let merged = merge_active_and_inline(&active, &inline_rules);
            match sifter.rebuild(&merged) {
                Ok(rb) => {
                    report.watches_rebound = report.watches_rebound.saturating_add(1);
                    self.audit(
                        "file_watch_sifter_rebind",
                        &watch_id.to_wire_string(),
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
                        "file_watch_sifter_rebind",
                        &watch_id.to_wire_string(),
                        "error",
                        Some(e.to_string()),
                        None,
                    );
                }
            }
        }
        report
    }
}

/// Same `(active ∪ inline)` semantics command.rs uses, replicated here
/// so `command.rs` is not touched. Inline rules win on key collision.
fn merge_active_and_inline(
    active: &[RuleDefinition],
    inline: &[RuleDefinition],
) -> Vec<RuleDefinition> {
    // Defense in depth against the draft-poison footgun: only
    // runtime-eligible (Active) rules may reach `SifterRuntime::build`.
    // A non-eligible active entry (e.g. bound by a pre-gate binary) is
    // skipped so the watch self-heals instead of failing every rebuild
    // with `SifterError::NotActive`; a non-eligible inline rule is
    // skipped for this watch instead of hard-failing it. Mirrors the
    // identical guard in command.rs / pty_command.rs.
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
