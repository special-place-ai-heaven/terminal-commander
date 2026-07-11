// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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
    /// Bucket source side-table (subscriptions MUST-ADD #2). Recorded
    /// at `start` immediately after `bucket_create`.
    sources: Arc<crate::subscriptions::source::BucketSourceTable>,
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
            live: Arc::new(RwLock::new(HashMap::default())),
            activation,
            sources,
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
        self.reap_finished();
        let g = self.live.read();
        g.iter()
            .map(|(wid, b)| LiveWatchIdentity {
                watch_id: *wid,
                bucket_id: b.bucket_id,
                probe_id: b.probe_id,
            })
            .collect()
    }

    /// Remove probes that terminated without an explicit `stop` call.
    ///
    /// The live map owns cancellation handles, but map membership alone is not
    /// a liveness signal: the underlying task can fail on an I/O error. Reap
    /// finished tasks before answering any liveness/list query so subscribers
    /// never observe an involuntarily-dead watch as `Running`.
    fn reap_finished(&self) {
        let finished: Vec<JobId> = {
            let live = self.live.read();
            live.iter()
                .filter_map(|(watch_id, binding)| {
                    binding
                        .cancel
                        .lock()
                        .as_ref()
                        .is_some_and(FileProbe::is_finished)
                        .then_some(*watch_id)
                })
                .collect()
        };
        if finished.is_empty() {
            return;
        }

        let removed: Vec<(JobId, WatchBinding)> = {
            let mut live = self.live.write();
            finished
                .into_iter()
                .filter_map(|watch_id| {
                    let still_finished = live.get(&watch_id).is_some_and(|binding| {
                        binding
                            .cancel
                            .lock()
                            .as_ref()
                            .is_some_and(FileProbe::is_finished)
                    });
                    if still_finished {
                        live.remove(&watch_id).map(|binding| (watch_id, binding))
                    } else {
                        None
                    }
                })
                .collect()
        };

        for (watch_id, binding) in removed {
            let probe_metrics = binding
                .cancel
                .lock()
                .as_ref()
                .map_or_else(FileProbeMetrics::default, FileProbe::metrics);
            let sink_metrics = binding.metrics.lock().clone();
            let metrics = combine_file_metrics(&probe_metrics, &sink_metrics);
            let _ = self.jobs.finish(watch_id, Some(1), None);
            self.audit(
                "file_watch_exit",
                &watch_id.to_wire_string(),
                "error",
                Some("file-watch probe terminated unexpectedly".to_owned()),
                Some(format!(
                    "{{\"frames\":{},\"events\":{},\"bytes\":{}}}",
                    metrics.frames_total, metrics.events_emitted, metrics.bytes_total
                )),
            );
        }
    }

    /// Return whether a watch id still owns a live probe handle.
    ///
    /// Bucket-source records intentionally outlive stopped watches so
    /// subscriptions can retain their routing scope. Liveness must therefore
    /// come from the runtime's live registry, not from source-record presence.
    #[must_use]
    pub fn is_live(&self, watch_id: JobId) -> bool {
        self.reap_finished();
        self.live.read().contains_key(&watch_id)
    }

    /// Start a file watch. Policy-gates the path, allocates a
    /// `(watch_id, bucket_id, probe_id)` triple, builds the per-watch
    /// `SifterRuntime` against the current scoped activation snapshot,
    /// spawns a `FileProbe` in follow mode, audits the start, and
    /// returns the bounded triple. Must be called from a tokio runtime
    /// because the probe spawn is async.
    ///
    /// TOCTOU: `path` is the CANONICAL, already-authorized target produced
    /// by `resolve_and_authorize_file` in the only caller
    /// (`handle_file_watch_start`). Every step below -- the policy check, the
    /// `metadata` existence stat, the `BucketSource` record, and the
    /// `FileProbeConfig` -- reuses THIS exact `PathBuf` and never re-resolves
    /// by name, so the probe follows the same target policy authorized (no
    /// re-canonicalization window between check and use).
    // Linear gate (path policy -> probe-kind gate -> existence -> allocate ->
    // spawn). The two security gates run deny-first up front and the rest is a
    // straight allocation sequence; splitting it would scatter the deny-first
    // logic the same way it would for `PolicyEngine::evaluate`.
    #[allow(clippy::too_many_lines)]
    pub fn start(
        &self,
        path: PathBuf,
        bucket_cfg: BucketConfig,
        inline_rules: Vec<RuleDefinition>,
        follow_from_beginning: bool,
        tag: Option<String>,
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
        // Probe-kind gate (TC22 A2; POLICY.md section 6 steps 2c / 2e).
        // SECONDARY deny-first filter layered ON TOP of the FileWatch path gate
        // above: file_watch_start creates a FileWatch probe, so it is gated as
        // "file_watch". Placed here -- BEFORE any bucket/probe/job allocation --
        // so a denied kind creates NO probe, and audited with the SAME
        // `file_watch_start` deny row pattern the path gate uses (constitution
        // Principle V: the probe-kind reason is recorded).
        let probe_verdict = self
            .policy
            .evaluate(&PolicyAction::ProbeCreate { kind: "file_watch" });
        if probe_verdict.decision == PolicyDecision::Deny {
            self.audit(
                "file_watch_start",
                &path.display().to_string(),
                "deny",
                Some(probe_verdict.reason.clone()),
                None,
            );
            return Err(WatchError::PolicyDenied(probe_verdict.reason));
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
        // Record the bucket's source identity for subscription routing
        // (MUST-ADD #2). File-watch records its `watch_id` as the job id and
        // the watched path. Bumps the side-table dirty epoch.
        self.sources.record(
            bucket_id,
            crate::subscriptions::source::BucketSource {
                kind: terminal_commander_ipc::ProbeKind::FileWatch,
                job_id: Some(watch_id),
                probe_id: Some(probe_id),
                path: Some(path.clone()),
                tag,
            },
        );

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
        // US3b (T041): pick the event-driven `notify` backend on native
        // filesystems; WSL `/mnt/c` (9p/drvfs) -- where inotify is silently
        // non-functional (microsoft/WSL#4739) -- falls back to the poll loop.
        // Detected via /proc/self/mountinfo, the same signal the store's 9P
        // guard uses. The 120ms poll_interval above is retained as the WSL
        // fallback cadence and the universal floor.
        cfg.backend = terminal_commander_probes::select_backend_for_path(&path);

        let probe = FileProbe::spawn(cfg, Arc::clone(&self.rings), Arc::clone(&sifter), sink)?;

        // Register a JobManager entry so `bucket_wait` / lifecycle
        // bookkeeping has the same shape as a command job.
        let job_cfg = JobConfig {
            job_id: watch_id,
            argv: vec![format!("file_watch:{}", path.display())],
            bucket_id,
            probe_id,
            source_type: terminal_commander_core::SourceType::File,
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
        //
        // Read the probe's REAL workload metrics from the taken
        // `FileProbe` BEFORE dropping it. The `WatchEventSink` only ever
        // writes `events_emitted` into `b.metrics` (the sink snapshot);
        // `frames_total` / `bytes_total` / rotation / truncation /
        // suppression counters live on the probe and are structurally
        // zero in the snapshot. `combine_file_metrics` overlays the two
        // so the returned metrics and the audit line carry real
        // counters (same F9 footgun fixed for PTY in `combine_pty_metrics`).
        let taken = b.cancel.lock().take();
        let probe_metrics = taken
            .as_ref()
            .map_or_else(FileProbeMetrics::default, FileProbe::metrics);
        if let Some(mut p) = taken {
            p.cancel();
        }
        let sink_snap = b.metrics.lock().clone();
        let metrics = combine_file_metrics(&probe_metrics, &sink_snap);
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
    /// `file_watch_list`). Metrics combine the probe's real workload
    /// counters with the sink snapshot's `events_emitted`.
    #[must_use]
    pub fn list(&self) -> Vec<(JobId, BucketId, ProbeId, PathBuf, FileProbeMetrics)> {
        self.reap_finished();
        let g = self.live.read();
        g.iter()
            .map(|(wid, b)| {
                // The `WatchEventSink` only writes `events_emitted` into
                // `b.metrics`; the real frame / byte / rotation /
                // truncation / suppression counters live on the probe
                // (mirrors `stop()`). Read the probe metrics under a
                // NON-BLOCKING `try_lock()` on `b.cancel` so `list()`
                // never blocks. If the probe lock is contended OR the
                // probe was already taken by `stop()` (`None`), fall back
                // to the sink snapshot: a momentary stale read is
                // acceptable. `list()` is read-only — never cancel here.
                //
                // Lock order is `live.read()` then `cancel.try_lock()`;
                // `stop()` releases `live.write()` before taking
                // `cancel.lock()` and never re-takes `live` while holding
                // it, so there is no lock-order inversion (and `try_lock`
                // cannot deadlock regardless).
                let m = b.cancel.try_lock().map_or_else(
                    || b.metrics.lock().clone(),
                    |guard| {
                        let probe_metrics = guard
                            .as_ref()
                            .map_or_else(FileProbeMetrics::default, FileProbe::metrics);
                        let sink_snap = b.metrics.lock().clone();
                        combine_file_metrics(&probe_metrics, &sink_snap)
                    },
                );
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

/// Combine probe-side and sink-side file-watch metrics into the value
/// surfaced by `stop()` and `list()`.
///
/// The probe owns the real workload counters (`frames_total` /
/// `bytes_total` / `rotations_detected` / `truncations_detected` /
/// suppression); the [`WatchEventSink`] only records `events_emitted`
/// into the binding's sink snapshot. Everything but `events_emitted`
/// therefore comes from `probe`; `events_emitted` is the max of the two
/// so a race between the probe finalizing and the sink emitting cannot
/// lose the count.
///
/// This is the file-watch sibling of `combine_pty_metrics` and guards
/// the exact F9 footgun: the sink snapshot's zeroed frame/byte counters
/// must NEVER leak through into `list()` / `stop()`. Keeping the combine
/// in one place means there is a single line to get right and a single
/// line to test.
fn combine_file_metrics(probe: &FileProbeMetrics, snapshot: &FileProbeMetrics) -> FileProbeMetrics {
    FileProbeMetrics {
        events_emitted: probe.events_emitted.max(snapshot.events_emitted),
        ..probe.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{FileProbeMetrics, combine_file_metrics};

    #[test]
    fn combine_takes_workload_counters_from_probe_not_snapshot() {
        // The probe carries the real workload; the sink snapshot's
        // frame/byte/rotation/truncation/suppression counters are
        // structurally zero (only the sink's `events_emitted` is ever
        // written there). This is the F9 regression guard: those probe
        // counters must survive the combine even when the snapshot is
        // all-default — a zeroed snapshot must not be able to zero out
        // real probe frames.
        let probe = FileProbeMetrics {
            frames_total: 192,
            bytes_total: 1876,
            events_emitted: 5,
            rotations_detected: 2,
            truncations_detected: 1,
            frames_suppressed: 7,
            frames_suppressed_progress: 4,
            frames_suppressed_dedupe: 3,
        };
        let snapshot = FileProbeMetrics::default();

        let combined = combine_file_metrics(&probe, &snapshot);

        assert_eq!(combined.frames_total, 192, "frames must come from probe");
        assert_eq!(combined.bytes_total, 1876, "bytes must come from probe");
        assert_eq!(combined.rotations_detected, 2);
        assert_eq!(combined.truncations_detected, 1);
        assert_eq!(combined.frames_suppressed, 7);
        assert_eq!(combined.frames_suppressed_progress, 4);
        assert_eq!(combined.frames_suppressed_dedupe, 3);
    }

    #[test]
    fn combine_events_emitted_is_max_probe_greater() {
        let probe = FileProbeMetrics {
            events_emitted: 9,
            frames_total: 10,
            ..FileProbeMetrics::default()
        };
        let snapshot = FileProbeMetrics {
            events_emitted: 4,
            ..FileProbeMetrics::default()
        };

        let combined = combine_file_metrics(&probe, &snapshot);

        assert_eq!(combined.events_emitted, 9, "probe events > snapshot wins");
        assert_eq!(
            combined.frames_total, 10,
            "non-event fields still from probe"
        );
    }

    #[test]
    fn combine_events_emitted_is_max_snapshot_greater() {
        // The sink may have emitted more than the probe has recorded at
        // the instant we read (e.g. probe metrics lagging the sink by a
        // frame). The snapshot value must win for `events_emitted` so the
        // count is never lost — but a non-zero snapshot frame count must
        // still NOT leak through; only `events_emitted` is taken from the
        // snapshot.
        let probe = FileProbeMetrics {
            events_emitted: 4,
            frames_total: 10,
            ..FileProbeMetrics::default()
        };
        let snapshot = FileProbeMetrics {
            events_emitted: 9,
            // A bogus non-zero snapshot frame count must be ignored.
            frames_total: 999,
            bytes_total: 999,
            ..FileProbeMetrics::default()
        };

        let combined = combine_file_metrics(&probe, &snapshot);

        assert_eq!(combined.events_emitted, 9, "snapshot events > probe wins");
        assert_eq!(
            combined.frames_total, 10,
            "frames must come from probe, never the snapshot"
        );
        assert_eq!(
            combined.bytes_total, 0,
            "bytes must come from probe, never the snapshot"
        );
    }
}
