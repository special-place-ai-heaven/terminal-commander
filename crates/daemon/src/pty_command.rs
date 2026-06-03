// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon-owned PTY command runtime (TC44).
//!
//! Unix-only implementation today. On Windows the IPC handlers return
//! `UnsupportedPlatform` (ConPTY pending) — no runtime spawn path to flag.
//! If a Windows PTY spawn is added later, apply the same `CREATE_NO_WINDOW`
//! rule as `ProcessProbe::spawn` (see `docs/release/windows-wsl-bridge-contract.md` §4.4).

#[cfg(unix)]
mod runtime {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use parking_lot::RwLock;
    use terminal_commander_core::{
        ActivationScope, BucketConfig, BucketError, BucketId, ContextRingManager, EventDraft,
        JobConfig, JobId, JobManager, ProbeId, RuleDefinition,
    };
    use terminal_commander_probes::{
        EventSink, PtyProbe, PtyProbeConfig, PtyProbeError, PtyProbeMetrics, WriteStdinError,
    };
    use terminal_commander_sifters::SifterRuntime;
    use terminal_commander_store::AuditEntry;

    use crate::activation::ActivationRegistry;
    use crate::audit::AuditSink;
    use crate::command::SHELL_INTERPRETERS_DENY;
    use crate::policy::{PolicyAction, PolicyDecision, PolicyEngine, PolicyProfile};
    use crate::router::Router;

    #[derive(Debug, thiserror::Error)]
    pub enum PtyRuntimeError {
        #[error("policy denied pty_command_start: {0}")]
        PolicyDenied(String),
        #[error(
            "shell interpreter '{0}' is denied by default; pty_command_start is not a shell bridge"
        )]
        ShellInterpreterDenied(String),
        #[error("argv must not be empty")]
        EmptyArgv,
        #[error("bucket create error: {0}")]
        Bucket(#[from] BucketError),
        #[error("sifter build error: {0}")]
        Sifter(String),
        #[error("pty spawn error: {0}")]
        Spawn(#[from] PtyProbeError),
        #[error("unknown pty job id: {0}")]
        UnknownJob(JobId),
        #[error("secret prompt active; LLM-supplied input denied")]
        SecretInputDenied,
        #[error("stdin payload exceeds bounded cap")]
        OversizedStdin,
        #[error("io error: {0}")]
        Io(#[from] std::io::Error),
    }

    struct PtyBinding {
        bucket_id: BucketId,
        probe_id: ProbeId,
        argv: Vec<String>,
        sifter: Arc<SifterRuntime>,
        inline_rules: Vec<RuleDefinition>,
        probe: Arc<tokio::sync::Mutex<Option<PtyProbe>>>,
        metrics_snapshot: Arc<parking_lot::Mutex<PtyProbeMetrics>>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LivePtyIdentity {
        pub job_id: JobId,
        pub bucket_id: BucketId,
        pub probe_id: ProbeId,
    }

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct PtyRebindReport {
        pub jobs_considered: u32,
        pub jobs_rebound: u32,
        pub rebuild_failures: u32,
    }

    #[derive(Debug, Clone, Copy)]
    pub struct PtyStartResponse {
        pub job_id: JobId,
        pub bucket_id: BucketId,
        pub probe_id: ProbeId,
    }

    #[derive(Debug, Clone, Copy)]
    pub struct PtyWriteResponse {
        pub bytes_written: u64,
        pub secret_prompt_active: bool,
    }

    struct PtyEventSink {
        router: Arc<Router>,
        metrics: Arc<parking_lot::Mutex<PtyProbeMetrics>>,
    }

    impl std::fmt::Debug for PtyEventSink {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PtyEventSink").finish_non_exhaustive()
        }
    }

    impl EventSink for PtyEventSink {
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

    #[derive(Debug, Clone)]
    pub struct PtyStartRequest {
        pub argv: Vec<String>,
        pub cwd: Option<PathBuf>,
        pub env: Vec<(OsString, OsString)>,
        pub bucket_config: Option<BucketConfig>,
        pub rules: Vec<RuleDefinition>,
        pub rows: Option<u16>,
        pub cols: Option<u16>,
        /// Optional per-bucket tag for subscription routing (Phase 3).
        pub tag: Option<String>,
    }

    pub struct PtyRuntime {
        router: Arc<Router>,
        rings: Arc<ContextRingManager>,
        jobs: Arc<JobManager>,
        audit: Arc<dyn AuditSink>,
        policy: PolicyEngine,
        profile_label: String,
        live: Arc<RwLock<HashMap<JobId, PtyBinding>>>,
        activation: Arc<ActivationRegistry>,
        /// Bucket source side-table (subscriptions MUST-ADD #2).
        /// Recorded at `start` immediately after `bucket_create`.
        sources: Arc<crate::subscriptions::source::BucketSourceTable>,
    }

    impl std::fmt::Debug for PtyRuntime {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PtyRuntime")
                .field("profile", &self.profile_label)
                .finish_non_exhaustive()
        }
    }

    #[allow(
        clippy::too_many_lines,
        clippy::needless_pass_by_value,
        clippy::type_complexity,
        clippy::assigning_clones,
        clippy::collapsible_if,
        clippy::option_if_let_else
    )]
    impl PtyRuntime {
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
                .with_actor("pty_runtime")
                .with_profile(self.profile_label.clone());
            if let Some(r) = reason {
                entry = entry.with_reason(r);
            }
            if let Some(m) = metadata {
                entry = entry.with_metadata_json(m);
            }
            let _ = self.audit.emit(&entry);
        }

        #[must_use]
        pub fn live_jobs(&self) -> Vec<LivePtyIdentity> {
            let g = self.live.read();
            g.iter()
                .map(|(jid, b)| LivePtyIdentity {
                    job_id: *jid,
                    bucket_id: b.bucket_id,
                    probe_id: b.probe_id,
                })
                .collect()
        }

        fn shell_interpreter_basename(argv0: &str) -> Option<&'static str> {
            let basename = Path::new(argv0)
                .file_name()
                .and_then(|os| os.to_str())
                .unwrap_or(argv0);
            for &shell in SHELL_INTERPRETERS_DENY {
                if basename == shell {
                    return Some(shell);
                }
                let is_exe_variant = Path::new(shell)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
                if is_exe_variant && basename.eq_ignore_ascii_case(shell) {
                    return Some(shell);
                }
            }
            None
        }

        pub fn start(&self, req: PtyStartRequest) -> Result<PtyStartResponse, PtyRuntimeError> {
            if req.argv.is_empty() {
                return Err(PtyRuntimeError::EmptyArgv);
            }
            if let Some(shell) = Self::shell_interpreter_basename(&req.argv[0]) {
                self.audit(
                    "pty_command_start",
                    &req.argv[0],
                    "deny",
                    Some(format!(
                        "shell interpreter '{shell}' denied by default; pty_command_start is not a shell bridge"
                    )),
                    None,
                );
                return Err(PtyRuntimeError::ShellInterpreterDenied(shell.to_owned()));
            }
            let cwd_for_policy = req.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
            let verdict = self.policy.evaluate(&PolicyAction::CommandStart {
                argv: &req.argv,
                cwd: cwd_for_policy.as_path(),
            });
            if verdict.decision == PolicyDecision::Deny {
                self.audit(
                    "pty_command_start",
                    &req.argv[0],
                    "deny",
                    Some(verdict.reason.clone()),
                    None,
                );
                return Err(PtyRuntimeError::PolicyDenied(verdict.reason));
            }

            let bucket_id = BucketId::new();
            let probe_id = ProbeId::new();
            let job_id = JobId::new();
            let bucket_cfg = req.bucket_config.unwrap_or_default();
            self.router.bucket_create(bucket_id, bucket_cfg)?;
            // Record the bucket's source identity for subscription routing
            // (MUST-ADD #2). PTY is unix-only (this module is `#[cfg(unix)]`),
            // so the write is naturally unix-gated. Bumps the dirty epoch.
            self.sources.record(
                bucket_id,
                crate::subscriptions::source::BucketSource {
                    kind: terminal_commander_ipc::ProbeKind::Pty,
                    job_id: Some(job_id),
                    probe_id: Some(probe_id),
                    path: None,
                    tag: req.tag.clone(),
                },
            );

            let active_for_job = self
                .activation
                .snapshot_for_job(bucket_id, job_id, probe_id);
            let merged: Vec<RuleDefinition> = merge_active_and_inline(&active_for_job, &req.rules);
            let sifter = Arc::new(
                SifterRuntime::build(&merged)
                    .map_err(|e| PtyRuntimeError::Sifter(e.to_string()))?,
            );

            let metrics = Arc::new(parking_lot::Mutex::new(PtyProbeMetrics::default()));
            let sink: Arc<dyn EventSink> = Arc::new(PtyEventSink {
                router: Arc::clone(&self.router),
                metrics: Arc::clone(&metrics),
            });

            let mut cfg = PtyProbeConfig::for_bucket(bucket_id);
            cfg.probe_id = Some(probe_id);
            cfg.cwd = req.cwd.clone();
            cfg.env = req.env.clone();
            cfg.rows = req.rows;
            cfg.cols = req.cols;

            let probe = PtyProbe::spawn(
                &req.argv,
                &cfg,
                Arc::clone(&self.rings),
                Arc::clone(&sifter),
                sink,
            )?;

            let job_cfg = JobConfig {
                job_id,
                argv: req.argv.clone(),
                bucket_id,
                probe_id,
                grace_secs: 0,
            };
            let _ = self.jobs.start(job_cfg);
            self.jobs.mark_running(job_id);

            self.live.write().insert(
                job_id,
                PtyBinding {
                    bucket_id,
                    probe_id,
                    argv: req.argv.clone(),
                    sifter,
                    inline_rules: req.rules,
                    probe: Arc::new(tokio::sync::Mutex::new(Some(probe))),
                    metrics_snapshot: metrics,
                },
            );

            self.audit(
                "pty_command_start",
                &job_id.to_wire_string(),
                "allow",
                None,
                Some(format!(
                    "{{\"argv0\":{},\"bucket_id\":{}}}",
                    serde_json::Value::String(req.argv[0].clone()),
                    serde_json::Value::String(bucket_id.to_wire_string())
                )),
            );

            Ok(PtyStartResponse {
                job_id,
                bucket_id,
                probe_id,
            })
        }

        pub async fn write_stdin(
            &self,
            job_id: JobId,
            bytes: &[u8],
        ) -> Result<PtyWriteResponse, PtyRuntimeError> {
            let probe_handle = {
                let g = self.live.read();
                let binding = g.get(&job_id).ok_or(PtyRuntimeError::UnknownJob(job_id))?;
                Arc::clone(&binding.probe)
            };
            let guard = probe_handle.lock().await;
            let probe = guard.as_ref().ok_or(PtyRuntimeError::UnknownJob(job_id))?;
            let byte_count = bytes.len();
            match probe.write_stdin(bytes).await {
                Ok(written) => {
                    self.audit(
                        "pty_command_write_stdin",
                        &job_id.to_wire_string(),
                        "allow",
                        None,
                        Some(format!(
                            "{{\"byte_count\":{byte_count},\"prompt_kind\":\"none\"}}"
                        )),
                    );
                    Ok(PtyWriteResponse {
                        bytes_written: written as u64,
                        secret_prompt_active: probe.is_secret_prompt_active(),
                    })
                }
                Err(WriteStdinError::SecretInputActive) => {
                    self.audit(
                        "pty_command_write_stdin",
                        &job_id.to_wire_string(),
                        "deny",
                        Some("secret prompt active".to_owned()),
                        Some(format!(
                            "{{\"byte_count\":{byte_count},\"prompt_kind\":\"secret\"}}"
                        )),
                    );
                    Err(PtyRuntimeError::SecretInputDenied)
                }
                Err(WriteStdinError::Oversized) => {
                    self.audit(
                        "pty_command_write_stdin",
                        &job_id.to_wire_string(),
                        "deny",
                        Some("oversized".to_owned()),
                        Some(format!("{{\"byte_count\":{byte_count}}}")),
                    );
                    Err(PtyRuntimeError::OversizedStdin)
                }
                Err(WriteStdinError::Closed) => Err(PtyRuntimeError::UnknownJob(job_id)),
                Err(WriteStdinError::Io(e)) => Err(PtyRuntimeError::Io(e)),
            }
        }

        pub fn stop(&self, job_id: JobId) -> Result<(BucketId, PtyProbeMetrics), PtyRuntimeError> {
            let removed = self.live.write().remove(&job_id);
            let Some(b) = removed else {
                return Err(PtyRuntimeError::UnknownJob(job_id));
            };
            // Read probe-side metrics BEFORE cancellation so the
            // frame / byte counters reflect the real workload. The
            // PtyEventSink only records `events_emitted` into the
            // binding snapshot; everything else lives on the probe.
            let probe_metrics = if let Ok(mut g) = b.probe.try_lock() {
                let snap = g
                    .as_ref()
                    .map_or_else(PtyProbeMetrics::default, PtyProbe::metrics);
                if let Some(p) = g.as_mut() {
                    p.cancel();
                }
                snap
            } else {
                PtyProbeMetrics::default()
            };
            let sink_snap = b.metrics_snapshot.lock().clone();
            // Combine: probe owns the frame/byte/prompt counters; the
            // sink owns `events_emitted`. Take whichever is larger
            // for `events_emitted` so a race doesn't lose the count.
            let metrics = PtyProbeMetrics {
                events_emitted: probe_metrics.events_emitted.max(sink_snap.events_emitted),
                ..probe_metrics
            };
            let _ = self.jobs.finish(job_id, Some(0), None);
            self.audit(
                "pty_command_stop",
                &job_id.to_wire_string(),
                "info",
                None,
                Some(format!(
                    "{{\"frames\":{},\"events\":{},\"bytes\":{}}}",
                    metrics.frames_total, metrics.events_emitted, metrics.bytes_total
                )),
            );
            Ok((b.bucket_id, metrics))
        }

        #[must_use]
        pub fn list(&self) -> Vec<(JobId, BucketId, ProbeId, Vec<String>, PtyProbeMetrics, bool)> {
            let g = self.live.read();
            g.iter()
                .map(|(jid, b)| {
                    let metrics = b.metrics_snapshot.lock().clone();
                    let secret = if let Ok(guard) = b.probe.try_lock() {
                        guard
                            .as_ref()
                            .is_some_and(PtyProbe::is_secret_prompt_active)
                    } else {
                        false
                    };
                    (
                        *jid,
                        b.bucket_id,
                        b.probe_id,
                        b.argv.clone(),
                        metrics,
                        secret,
                    )
                })
                .collect()
        }

        pub fn rebind_jobs_in_scope(&self, scope: Option<ActivationScope>) -> PtyRebindReport {
            let work: Vec<PtyRebindWork> = {
                let g = self.live.read();
                g.iter()
                    .filter_map(|(jid, b)| {
                        let matches = match scope {
                            None | Some(ActivationScope::Global) => true,
                            Some(s) => s.matches(b.bucket_id, *jid, b.probe_id),
                        };
                        if !matches {
                            return None;
                        }
                        Some((
                            *jid,
                            b.bucket_id,
                            b.probe_id,
                            Arc::clone(&b.sifter),
                            b.inline_rules.clone(),
                        ))
                    })
                    .collect()
            };
            let mut report = PtyRebindReport {
                jobs_considered: u32::try_from(work.len()).unwrap_or(u32::MAX),
                ..PtyRebindReport::default()
            };
            let scope_label = scope.map_or("any", |s| s.kind_label());
            for (job_id, bucket_id, probe_id, sifter, inline_rules) in work {
                let active = self
                    .activation
                    .snapshot_for_job(bucket_id, job_id, probe_id);
                let merged = merge_active_and_inline(&active, &inline_rules);
                match sifter.rebuild(&merged) {
                    Ok(rb) => {
                        report.jobs_rebound = report.jobs_rebound.saturating_add(1);
                        self.audit(
                            "pty_sifter_rebind",
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
                            "pty_sifter_rebind",
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
    }

    type PtyRebindWork = (
        JobId,
        BucketId,
        ProbeId,
        Arc<SifterRuntime>,
        Vec<RuleDefinition>,
    );

    fn merge_active_and_inline(
        active: &[RuleDefinition],
        inline: &[RuleDefinition],
    ) -> Vec<RuleDefinition> {
        // Defense in depth against the draft-poison footgun: only
        // runtime-eligible (Active) rules may reach `SifterRuntime::build`.
        // A non-eligible active entry self-heals (skipped, PTY keeps
        // running) instead of failing every rebuild with
        // `SifterError::NotActive`; a non-eligible inline rule is skipped
        // for this PTY job. Mirrors command.rs / file_watch.rs.
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
}

#[cfg(unix)]
pub use runtime::{
    LivePtyIdentity, PtyRebindReport, PtyRuntime, PtyRuntimeError, PtyStartRequest,
    PtyStartResponse, PtyWriteResponse,
};
