// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon-owned PTY command runtime (TC44 + US3a/TC53).
//!
//! Cross-platform: this runtime speaks ONLY the abstract
//! `terminal_commander_probes::PtyProbe` surface, so the SAME code drives the
//! unix `pty-process` backend and the Windows ConPTY backend (`portable-pty`).
//! ConPTY requires Windows 10 1809+ (build 17763). On a platform with no PTY
//! backend the module is absent and the IPC handlers return
//! `UnsupportedPlatform`. The ConPTY child runs under its own pseudoconsole
//! (no extra visible window), consistent with the hidden-window posture of
//! `ProcessProbe::spawn` (see `docs/release/windows-wsl-bridge-contract.md`
//! §4.4); `portable-pty` sets the child's std handles to the pty, not a
//! console window.

// US3a/TC53: the PTY runtime is available on every host with a PTY backend
// (unix `pty-process` and Windows ConPTY via `portable-pty`). The runtime
// logic here speaks ONLY the abstract `terminal_commander_probes::PtyProbe`
// surface, so the SAME daemon code drives both backends.
#[cfg(any(unix, windows))]
mod runtime {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::Arc;

    use parking_lot::RwLock;
    use terminal_commander_core::{
        ActivationScope, BucketConfig, BucketError, BucketId, ContextRingManager, EventDraft,
        JobConfig, JobId, JobManager, ProbeId, RuleDefinition, shell_argv_denied,
    };
    use terminal_commander_ipc::protocol::{AwaitingCredential, CredentialKind};
    use terminal_commander_probes::{
        EventSink, PromptKind, PtyProbe, PtyProbeConfig, PtyProbeError, PtyProbeMetrics,
        WriteStdinError,
    };
    use terminal_commander_sifters::SifterRuntime;
    use terminal_commander_store::AuditEntry;

    use crate::activation::ActivationRegistry;
    use crate::audit::AuditSink;
    use crate::command::{WslArgvClass, classify_wsl_nested_shell, wsl_carrier_label};
    use crate::policy::{PolicyAction, PolicyDecision, PolicyEngine, PolicyProfile};
    use crate::router::Router;

    #[derive(Debug, thiserror::Error)]
    pub enum PtyRuntimeError {
        #[error("policy denied pty_command_start: {0}")]
        PolicyDenied(String),
        #[error(
            "shell interpreter '{0}' denied: allow_shell is off; pty_command_start is not a shell bridge"
        )]
        ShellInterpreterDenied(String),
        /// US8 (FR-060): a shell smuggled through a `wsl`/`wsl.exe` carrier on
        /// the PTY argv lane. Maps to the same `ShellInterpreterDenied` wire
        /// code as the command lane -- lane parity is a constitutional
        /// invariant (policy-wsl.md "both lanes, one truth").
        #[error(
            "nested shell interpreter '{interpreter}' denied inside a '{carrier}' invocation; pty_command_start is not a shell bridge on either side of the WSL boundary"
        )]
        WslNestedShellDenied {
            interpreter: String,
            carrier: String,
        },
        #[error("argv must not be empty")]
        EmptyArgv,
        /// Caller-fixable argv the spawn refuses (NUL, CR/LF in batch args).
        #[error("{0}")]
        ArgvInvalid(String),
        /// `argv[0]` did not resolve to a program; carries `argv[0]`.
        #[error("program not found: {0}")]
        ProgramNotFound(String),
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
        /// Credential delivery to a job with no password prompt up (or a
        /// different prompt than the one the owner answered).
        #[error("pty job {0} is not waiting for a password")]
        NotAwaitingCredential(JobId),
        #[error("stdin payload exceeds bounded cap")]
        OversizedStdin,
        #[error("io error: {0}")]
        Io(#[from] std::io::Error),
    }

    struct PtyBinding {
        bucket_id: BucketId,
        probe_id: ProbeId,
        argv: Vec<String>,
        /// Absolute program the spawn ran; shown to the owner.
        program: String,
        /// Request env keys that change program resolution or loading.
        program_env: Vec<String>,
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

    /// What `credential_request` needs to know about a PTY job's prompt.
    #[derive(Debug, Clone)]
    pub struct CredentialPrompt {
        /// Secret-prompt generation; one owner answer per generation.
        pub generation: u64,
        pub awaiting: Option<AwaitingCredential>,
        pub argv: Vec<String>,
        /// The absolute program the daemon spawned (not the typed name).
        pub program: String,
        pub program_env: Vec<String>,
    }

    const fn credential_kind(kind: PromptKind) -> CredentialKind {
        match kind {
            PromptKind::SudoPassword => CredentialKind::Sudo,
            PromptKind::SshPassword => CredentialKind::Ssh,
            _ => CredentialKind::Password,
        }
    }

    fn awaiting_of(probe: &PtyProbe) -> Option<AwaitingCredential> {
        probe
            .awaiting_credential()
            .map(|(kind, since_ms)| AwaitingCredential {
                kind: credential_kind(kind),
                since_ms,
            })
    }

    #[derive(Debug, Clone, Copy)]
    pub struct PtyWriteResponse {
        /// The PTY job's bucket, so the IPC handler can run the optional
        /// settle-window read (US5 / FR-041) without a second lookup.
        pub bucket_id: BucketId,
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
        /// Single-writer store actor. spec 004: the PTY lane persists a job
        /// receipt on its terminal transition, mirroring the combed lane, so a
        /// PTY outcome is reconstructable after a restart instead of vanishing.
        store: crate::store_actor::StoreClient,
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
        clippy::option_if_let_else,
        // spec 004: `new` gained the store client so the PTY lane can persist a
        // receipt on its terminal transition, matching the combed lane.
        clippy::too_many_arguments
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
            store: crate::store_actor::StoreClient,
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
                store,
            }
        }

        // `pub(crate)`, not private: `ShellSessionRuntime` (a different
        // module, same crate) reuses this to write the same audit row for
        // the os_guard refusal on the `shell_session_exec` line as the
        // `command_rejected` / `command_shell_rejected` rows this runtime's
        // own deny paths write above.
        pub(crate) fn audit(
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

        /// Authoritative wire [`Liveness`] for a PTY job, derived from the job
        /// ledger (`JobManager`) exactly like the command runtime. A natural exit,
        /// failure, or cancellation is recorded by the lifecycle waiter spawned in
        /// `start`; the binding LINGERS in `live` after exit (like command's), so a
        /// terminated PTY reports `Exited{code}` / `Failed` / `Cancelled` here
        /// instead of `Running` from mere live-map presence. An unknown job (record
        /// dropped/forgotten) reports `Stopped`.
        pub fn liveness(&self, job_id: JobId) -> terminal_commander_ipc::Liveness {
            self.jobs
                .get(job_id)
                .map_or(terminal_commander_ipc::Liveness::Stopped, |rec| {
                    crate::liveness::command_liveness(
                        rec.state,
                        rec.exit_info.as_ref().and_then(|e| e.exit_code),
                        rec.exit_info.as_ref().and_then(|e| e.signal.clone()),
                    )
                })
        }

        pub fn start(&self, req: PtyStartRequest) -> Result<PtyStartResponse, PtyRuntimeError> {
            if req.argv.is_empty() {
                return Err(PtyRuntimeError::EmptyArgv);
            }
            if req.argv.iter().any(|a| a.contains('\0')) {
                return Err(PtyRuntimeError::ArgvInvalid(
                    "argv must not contain NUL bytes".to_owned(),
                ));
            }
            // FCR2-002: gate the program the backend will run, not the typed
            // name. Unresolved, the gate sees the stem the backend's own
            // extension-replacing search would substitute (`bash.txt` ->
            // `bash`), and the spawn below is refused.
            let spawn_argv0 = resolve_pty_argv0(&req.argv[0], &req.env);
            let mut gate_argv = req.argv.clone();
            gate_argv[0] = spawn_argv0.clone().unwrap_or_else(|| {
                std::path::Path::new(&req.argv[0])
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
            });
            // Interpreter gate, same as the command argv lane: denied under
            // allow_shell=false; under allow_shell=true it runs through
            // `CommandShellStart` and the allow row is tagged `nested_shell`.
            // WSL carriers skip it; the nested-shell gate below owns them.
            let cwd_for_policy = req.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
            let shell_start_gate = |shell: &str| -> Result<(), PtyRuntimeError> {
                let verdict = self.policy.evaluate(&PolicyAction::CommandShellStart {
                    shell_line: "",
                    cwd: cwd_for_policy.as_path(),
                    shell,
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
                Ok(())
            };
            let shell_tag = if matches!(classify_wsl_nested_shell(&gate_argv), WslArgvClass::NotWsl)
                && let Some(shell) = shell_argv_denied(&gate_argv)
            {
                if !self.policy.caps_allow_shell() {
                    self.audit(
                        "pty_command_start",
                        &req.argv[0],
                        "deny",
                        Some(format!(
                            "shell interpreter '{shell}' denied: allow_shell is off \
                             ([policy.caps] allow_shell = true enables it and shell_exec)"
                        )),
                        None,
                    );
                    return Err(PtyRuntimeError::ShellInterpreterDenied(shell.to_owned()));
                }
                shell_start_gate(shell)?;
                Some(("nested_shell", shell.to_owned()))
            } else {
                None
            };
            // WSL nested-shell gate (US8 / FR-060). Same classifier as the
            // command argv lane -- a shell smuggled through a `wsl`/`wsl.exe`
            // carrier is denied under allow_shell=false, identically on both
            // lanes (lane divergence would be a defect).
            // Under allow_shell=true the payload runs through `CommandShellStart`
            // and the allow row is tagged, as on the command argv lane.
            let wsl_tag = match classify_wsl_nested_shell(&gate_argv) {
                WslArgvClass::NestedShell { interpreter } if !self.policy.caps_allow_shell() => {
                    let carrier = wsl_carrier_label(
                        terminal_commander_core::shell_deny::launched_argv(&req.argv)
                            .first()
                            .map_or("", String::as_str),
                    );
                    self.audit(
                        "pty_command_start",
                        &req.argv[0],
                        "deny",
                        Some(format!(
                            "nested shell interpreter '{interpreter}' denied inside a \
                             '{carrier}' invocation; pty_command_start is not a shell bridge \
                             on either side of the WSL boundary (allow_shell gate)"
                        )),
                        None,
                    );
                    return Err(PtyRuntimeError::WslNestedShellDenied {
                        interpreter,
                        carrier,
                    });
                }
                WslArgvClass::UnknownConstruction if !self.policy.caps_allow_shell() => {
                    let carrier = wsl_carrier_label(
                        terminal_commander_core::shell_deny::launched_argv(&req.argv)
                            .first()
                            .map_or("", String::as_str),
                    );
                    self.audit(
                        "pty_command_start",
                        &req.argv[0],
                        "deny",
                        Some(format!(
                            "unrecognized '{carrier}' construction denied (fail closed); \
                             pty_command_start is not a shell bridge on either side of the \
                             WSL boundary (allow_shell gate)"
                        )),
                        None,
                    );
                    return Err(PtyRuntimeError::WslNestedShellDenied {
                        interpreter: "unrecognized construction".to_owned(),
                        carrier,
                    });
                }
                WslArgvClass::NestedShell { interpreter } => {
                    shell_start_gate(&interpreter)?;
                    Some(("nested_shell", interpreter))
                }
                WslArgvClass::UnknownConstruction => {
                    shell_start_gate("unrecognized construction")?;
                    Some(("wsl_construction", "unknown".to_owned()))
                }
                _ => None,
            };
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

            // Probe-kind gate (TC22 A2; POLICY.md section 6 steps 2c / 2e).
            // SECONDARY deny-first filter layered on top of the CommandStart
            // gate above: this op creates a Pty probe, so it is gated as "pty".
            let probe_verdict = self
                .policy
                .evaluate(&PolicyAction::ProbeCreate { kind: "pty" });
            if probe_verdict.decision == PolicyDecision::Deny {
                self.audit(
                    "pty_command_start",
                    &req.argv[0],
                    "deny",
                    Some(probe_verdict.reason.clone()),
                    None,
                );
                return Err(PtyRuntimeError::PolicyDenied(probe_verdict.reason));
            }

            // Hand the backend the resolved path so its search cannot rewrite
            // it; nothing resolved means nothing the gate has vetted.
            let Some(spawn_argv0) = spawn_argv0 else {
                return Err(PtyRuntimeError::ProgramNotFound(req.argv[0].clone()));
            };
            let mut spawn_argv = req.argv.clone();
            spawn_argv[0] = spawn_argv0;
            self.spawn_pty_job(req, &spawn_argv, shell_tag.or(wsl_tag), "pty_command_start")
        }

        /// Spawn a long-lived session shell PTY (P1 / TC50).
        ///
        /// Built ON TOP of the same PTY spawn core as [`PtyRuntime::start`]
        /// via the shared [`PtyRuntime::spawn_pty_job`], but with TWO
        /// deliberate differences from the `pty_command_*` argv lane:
        ///
        /// 1. The shell-interpreter deny (`SHELL_INTERPRETERS_DENY`) is
        ///    SKIPPED: a session shell IS an interpreter on purpose (the
        ///    caller never hand-builds the argv -- the session runtime
        ///    assembles `[shell, "-i"]`).
        /// 2. The gate is [`PolicyAction::SessionStart`] behind the
        ///    independent `allow_session` capability (default deny), not
        ///    `CommandStart`. Audited as `shell_session_start`.
        ///
        /// The caller (`ShellSessionRuntime`) owns the `session_id <->
        /// job_id` mapping, the `max_sessions` cap, the idle TTL reaper,
        /// and the terminal-state guard; this method only performs the
        /// gated spawn.
        pub fn start_session(
            &self,
            req: PtyStartRequest,
        ) -> Result<PtyStartResponse, PtyRuntimeError> {
            if req.argv.is_empty() {
                return Err(PtyRuntimeError::EmptyArgv);
            }
            // NOTE: no shell-interpreter deny here -- the session lane runs
            // a login shell ON PURPOSE and is gated by `SessionStart`
            // instead (mirrors the command runtime's shell lane skipping
            // the argv guard under `CommandShellStart`).
            let shell = req.argv.first().cloned().unwrap_or_default();
            let cwd_for_policy = req.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
            let verdict = self.policy.evaluate(&PolicyAction::SessionStart {
                shell: &shell,
                cwd: cwd_for_policy.as_path(),
            });
            if verdict.decision == PolicyDecision::Deny {
                self.audit(
                    "shell_session_start",
                    &shell,
                    "deny",
                    Some(verdict.reason.clone()),
                    None,
                );
                return Err(PtyRuntimeError::PolicyDenied(verdict.reason));
            }

            // Probe-kind gate (TC22 A2; POLICY.md section 6 steps 2c / 2e).
            // SECONDARY deny-first filter on top of the SessionStart gate above.
            // A session shell is recorded as a Pty probe, so it is gated as
            // "pty" (the same kind as the one-shot pty_command_start lane).
            let probe_verdict = self
                .policy
                .evaluate(&PolicyAction::ProbeCreate { kind: "pty" });
            if probe_verdict.decision == PolicyDecision::Deny {
                self.audit(
                    "shell_session_start",
                    &shell,
                    "deny",
                    Some(probe_verdict.reason.clone()),
                    None,
                );
                return Err(PtyRuntimeError::PolicyDenied(probe_verdict.reason));
            }

            let spawn_argv = req.argv.clone();
            self.spawn_pty_job(req, &spawn_argv, None, "shell_session_start")
        }

        /// Shared PTY spawn core for the argv lane ([`PtyRuntime::start`])
        /// and the session lane ([`PtyRuntime::start_session`]).
        ///
        /// The caller is responsible for the per-lane GATE (shell-deny +
        /// policy verdict) BEFORE calling this; this method performs no
        /// policy evaluation. It allocates the bucket/probe/job, builds the
        /// sifter from active + inline rules, spawns the [`PtyProbe`], wires
        /// the lifecycle waiter, inserts the live binding, and writes the
        /// `allow` audit row labelled with `audit_action`.
        fn spawn_pty_job(
            &self,
            req: PtyStartRequest,
            spawn_argv: &[String],
            shell_tag: Option<(&'static str, String)>,
            audit_action: &'static str,
        ) -> Result<PtyStartResponse, PtyRuntimeError> {
            let bucket_id = BucketId::new();
            let probe_id = ProbeId::new();
            let job_id = JobId::new();
            let program = shown_program(&spawn_argv[0], &req.env, req.cwd.as_deref());
            let program_env = program_env_overrides(&req.env);
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

            let mut probe = PtyProbe::spawn(
                spawn_argv,
                &cfg,
                Arc::clone(&self.rings),
                Arc::clone(&sifter),
                sink,
            )
            .map_err(|e| match e {
                PtyProbeError::InvalidArgument(m) => PtyRuntimeError::ArgvInvalid(m),
                // Unix spawns the name as typed, so a missing program is the
                // spawn's `NotFound`, carved out like the argv lane's F7.
                PtyProbeError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                    PtyRuntimeError::ProgramNotFound(req.argv[0].clone())
                }
                e => PtyRuntimeError::Spawn(e),
            })?;
            // Take the completion receiver BEFORE the probe is moved into the
            // live binding. The lifecycle waiter below owns it so it can flip
            // the job ledger on exit without ever locking the probe mutex
            // (which `write_stdin` holds across `.await`).
            let completion = probe.take_completion();
            // spec 004: hold the probe behind a shared cell created BEFORE the
            // lifecycle waiter spawns, so the waiter can read the probe's real
            // final counters at exit. Persisting a receipt from the sink
            // snapshot alone would record events_emitted with zeroed frames and
            // bytes -- an evidence-stripped receipt, which is precisely the
            // defect this work removes.
            let probe_cell = Arc::new(tokio::sync::Mutex::new(Some(probe)));

            let job_cfg = JobConfig {
                job_id,
                argv: req.argv.clone(),
                bucket_id,
                probe_id,
                source_type: terminal_commander_core::SourceType::Terminal,
                grace_secs: 0,
            };
            let _ = self.jobs.start(job_cfg);
            self.jobs.mark_running(job_id);

            // Lifecycle waiter (mirrors command.rs::start_combed). When the
            // child exits naturally we flip the ledger to Exited/Failed; when
            // it is cancelled (stop / drop) we flip it to Cancelled. The
            // binding deliberately LINGERS in `live` afterward so the runtime
            // view (`collect_probes` via `PtyRuntime::liveness`) reports the
            // terminal state instead of `Running`. `pty_command_list` filters
            // terminal jobs out so the operator-facing live list stays clean.
            if let Some(completion_rx) = completion {
                let waiter_jobs = Arc::clone(&self.jobs);
                let waiter_router = Arc::clone(&self.router);
                let waiter_audit = Arc::clone(&self.audit);
                let waiter_profile = self.profile_label.clone();
                let argv0 = req.argv[0].clone();
                let waiter_store = self.store.clone();
                let waiter_probe = Arc::clone(&probe_cell);
                let waiter_metrics = Arc::clone(&metrics);
                tokio::spawn(async move {
                    // A dropped sender (probe dropped before it could send)
                    // is treated as a cancellation: the job did not exit
                    // cleanly on its own.
                    let outcome = completion_rx
                        .await
                        .unwrap_or(terminal_commander_probes::PtyExitOutcome::Cancelled);
                    // `stop()` finalizes the ledger synchronously (so
                    // `pty_command_list` is immediately consistent) and removes
                    // the binding. If it already ran, the job is terminal: skip
                    // so we do not double-append a lifecycle event or flip a
                    // Cancelled job to Exited.
                    if waiter_jobs.get(job_id).is_some_and(|r| {
                        matches!(
                            r.state,
                            terminal_commander_core::JobState::Exited
                                | terminal_commander_core::JobState::Failed
                                | terminal_commander_core::JobState::Cancelled
                        )
                    }) {
                        return;
                    }
                    // Capture the exit code before `outcome` is moved into the
                    // draft match below. A cancelled PTY job has no exit code.
                    let receipt_exit_code = match &outcome {
                        terminal_commander_probes::PtyExitOutcome::Exited { code, .. } => *code,
                        terminal_commander_probes::PtyExitOutcome::Cancelled => None,
                    };
                    let (draft, action, reason) = match outcome {
                        terminal_commander_probes::PtyExitOutcome::Exited { code, signal } => {
                            let nonzero = !matches!(code, Some(0)) || signal.is_some();
                            let reason = nonzero
                                .then(|| format!("nonzero exit: code={code:?} signal={signal:?}"));
                            (
                                waiter_jobs.finish(job_id, code, signal),
                                "pty_command_exit",
                                reason,
                            )
                        }
                        terminal_commander_probes::PtyExitOutcome::Cancelled => (
                            waiter_jobs.cancel(job_id),
                            "pty_command_exit",
                            Some("cancelled".to_owned()),
                        ),
                    };
                    // Append the lifecycle event to the bucket so a bucket /
                    // subscription consumer sees the exit, exactly like the
                    // command runtime does.
                    if let Some(d) = draft.as_ref() {
                        let _ = waiter_router.bucket_append(bucket_id, d.clone());
                    }

                    // spec 004 FR-002: persist the terminal outcome WITH the
                    // evidence a live observer would have had, so a PTY job
                    // stays reconstructable after a restart. Read the probe's
                    // real counters the same way `list()` and `status()` do.
                    //
                    // Unlike the combed lane there is no +1 here: the append
                    // above goes straight to the router, bypassing the sink
                    // that owns `events_emitted`, so the counter is already
                    // final.
                    let final_metrics = if let Ok(guard) = waiter_probe.try_lock() {
                        let pm = guard
                            .as_ref()
                            .map_or_else(PtyProbeMetrics::default, PtyProbe::metrics);
                        let snap = waiter_metrics.lock().clone();
                        combine_pty_metrics(&pm, &snap)
                    } else {
                        waiter_metrics.lock().clone()
                    };
                    let rec = waiter_jobs.get(job_id);
                    let terminal_state = rec
                        .as_ref()
                        .map_or(terminal_commander_core::JobState::Failed, |r| r.state);
                    let duration_ms = rec
                        .as_ref()
                        .and_then(|r| r.exit_info.as_ref().map(|e| e.duration_ms));
                    let evidence = pty_evidence_json(&final_metrics, duration_ms, probe_id);
                    crate::command::persist_job_receipt(
                        &waiter_store,
                        job_id,
                        bucket_id,
                        terminal_state,
                        receipt_exit_code,
                        final_metrics.events_emitted,
                        Some(&evidence),
                        None,
                    );
                    let mut entry = AuditEntry::new(action, job_id.to_wire_string(), "info")
                        .with_actor("pty_runtime")
                        .with_profile(waiter_profile)
                        .with_metadata_json(format!(
                            "{{\"argv0\":{}}}",
                            serde_json::Value::String(argv0)
                        ));
                    if let Some(r) = reason {
                        entry = entry.with_reason(r);
                    }
                    let _ = waiter_audit.emit(&entry);
                });
            }

            self.live.write().insert(
                job_id,
                PtyBinding {
                    bucket_id,
                    probe_id,
                    argv: req.argv.clone(),
                    program,
                    program_env,
                    sifter,
                    inline_rules: req.rules,
                    probe: probe_cell,
                    metrics_snapshot: metrics,
                },
            );

            let mut metadata = serde_json::json!({
                "argv0": req.argv[0],
                "bucket_id": bucket_id.to_wire_string(),
            });
            if let Some((key, val)) = &shell_tag {
                metadata[*key] = serde_json::Value::String(val.clone());
            }
            self.audit(
                audit_action,
                &job_id.to_wire_string(),
                "allow",
                // Same reason text as the command argv lane.
                shell_tag.as_ref().map(|(key, val)| {
                    if terminal_commander_core::shell_deny::launched_argv(&req.argv)
                        .first()
                        .is_some_and(|a| terminal_commander_core::shell_deny::is_wsl_carrier(a))
                    {
                        format!("wsl {key} classification: {val}")
                    } else {
                        format!("{key}: {val}")
                    }
                }),
                Some(metadata.to_string()),
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
            let (probe_handle, bucket_id) = {
                let g = self.live.read();
                let binding = g.get(&job_id).ok_or(PtyRuntimeError::UnknownJob(job_id))?;
                (Arc::clone(&binding.probe), binding.bucket_id)
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
                        bucket_id,
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
                Err(WriteStdinError::NoSecretPrompt) => {
                    Err(PtyRuntimeError::NotAwaitingCredential(job_id))
                }
                Err(WriteStdinError::Io(e)) => Err(PtyRuntimeError::Io(e)),
            }
        }

        /// The job's password prompt, if any. Same non-blocking read as
        /// `list()`: a busy probe reads as "not awaiting" for that instant.
        #[must_use]
        pub fn awaiting_credential(&self, job_id: JobId) -> Option<AwaitingCredential> {
            let probe = Arc::clone(&self.live.read().get(&job_id)?.probe);
            let guard = probe.try_lock().ok()?;
            guard.as_ref().and_then(awaiting_of)
        }

        /// Prompt state for `credential_request`.
        pub async fn credential_prompt(
            &self,
            job_id: JobId,
        ) -> Result<CredentialPrompt, PtyRuntimeError> {
            let (probe, argv, program, program_env) = {
                let g = self.live.read();
                let b = g.get(&job_id).ok_or(PtyRuntimeError::UnknownJob(job_id))?;
                (
                    Arc::clone(&b.probe),
                    b.argv.clone(),
                    b.program.clone(),
                    b.program_env.clone(),
                )
            };
            let guard = probe.lock().await;
            let probe = guard.as_ref().ok_or(PtyRuntimeError::UnknownJob(job_id))?;
            Ok(CredentialPrompt {
                generation: probe.secret_prompt_generation(),
                awaiting: awaiting_of(probe),
                argv,
                program,
                program_env,
            })
        }

        /// The spawned program and its program-affecting env keys, for the
        /// `pty_command_list` entry the admin CLI shows the owner.
        #[must_use]
        pub fn program_of(&self, job_id: JobId) -> Option<(String, Vec<String>)> {
            let g = self.live.read();
            g.get(&job_id)
                .map(|b| (b.program.clone(), b.program_env.clone()))
        }

        /// Type an OWNER-supplied secret (plus Enter) into the job's
        /// password prompt. Never reachable with model input: the callers are
        /// the native owner prompt and the admin-CLI-gated
        /// `credential_provide`. `generation` pins the prompt the owner saw;
        /// `None` answers whatever prompt is up. The audit row names the job,
        /// prompt kind, and source, never the value or its length. Returns
        /// the answered generation.
        pub async fn deliver_credential(
            &self,
            job_id: JobId,
            secret: &[u8],
            generation: Option<u64>,
            source: &'static str,
        ) -> Result<u64, PtyRuntimeError> {
            let probe_handle = {
                let g = self.live.read();
                let b = g.get(&job_id).ok_or(PtyRuntimeError::UnknownJob(job_id))?;
                Arc::clone(&b.probe)
            };
            // Held across the write, like `write_stdin`, so no model write
            // can interleave with the secret.
            let guard = probe_handle.lock().await;
            let probe = guard.as_ref().ok_or(PtyRuntimeError::UnknownJob(job_id))?;
            let current = probe.secret_prompt_generation();
            let Some(awaiting) = awaiting_of(probe) else {
                return Err(PtyRuntimeError::NotAwaitingCredential(job_id));
            };
            if generation.is_some_and(|g| g != current) {
                return Err(PtyRuntimeError::NotAwaitingCredential(job_id));
            }
            let mut line = Vec::with_capacity(secret.len() + 1);
            line.extend_from_slice(secret);
            // Enter, as a keyboard sends it: ConPTY needs CR to end a cooked
            // line, and a unix tty maps it to NL (ICRNL).
            line.push(b'\r');
            let written = probe.write_owner_secret(&line).await;
            line.fill(0);
            std::hint::black_box(&line);
            match written {
                Ok(()) => {}
                Err(WriteStdinError::NoSecretPrompt) => {
                    return Err(PtyRuntimeError::NotAwaitingCredential(job_id));
                }
                Err(WriteStdinError::Oversized) => return Err(PtyRuntimeError::OversizedStdin),
                Err(WriteStdinError::Closed) => return Err(PtyRuntimeError::UnknownJob(job_id)),
                Err(WriteStdinError::SecretInputActive) => {
                    return Err(PtyRuntimeError::SecretInputDenied);
                }
                Err(WriteStdinError::Io(e)) => return Err(PtyRuntimeError::Io(e)),
            }
            self.audit(
                "credential_provided",
                &job_id.to_wire_string(),
                "allow",
                None,
                Some(serde_json::json!({ "kind": awaiting.kind, "source": source }).to_string()),
            );
            Ok(current)
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
            // Combine: probe owns the frame/byte/prompt counters; the
            // sink owns `events_emitted` (see `combine_pty_metrics`).
            let sink_snap = b.metrics_snapshot.lock().clone();
            let metrics = combine_pty_metrics(&probe_metrics, &sink_snap);
            // An operator stop IS a cancellation, not a clean exit: record it
            // as Cancelled so the ledger (and any lingering runtime view)
            // reflects the kill. Synchronous so `pty_command_list` is
            // immediately consistent; the lifecycle waiter spawned in `start`
            // sees the terminal state and skips, avoiding a double event.
            let _ = self.jobs.cancel(job_id);

            // spec 004 review (grok BLOCKER / kimi-k3 HIGH-1): the waiter's
            // terminal-skip above means THIS is the only place a stopped PTY
            // job can ever be persisted. Without the receipt, the binding is
            // gone, no lane answers, and `command_status` reports `JobLost` --
            // telling the agent the daemon died mid-run when the operator
            // simply stopped the session. FR-005: an outcome that used to be
            // readable must not become unreadable.
            //
            // `end_cause` stays None: an operator stop is an ordinary cancel,
            // NOT an abandonment, and must not be written through the
            // abandonment lane (which never overwrites a real receipt).
            let duration_ms = self
                .jobs
                .get(job_id)
                .and_then(|r| r.exit_info.as_ref().map(|e| e.duration_ms));
            let evidence = pty_evidence_json(&metrics, duration_ms, b.probe_id);
            crate::command::persist_job_receipt(
                &self.store,
                job_id,
                b.bucket_id,
                terminal_commander_core::JobState::Cancelled,
                // A cancelled PTY job has no exit status. Never invent one.
                None,
                metrics.events_emitted,
                Some(&evidence),
                None,
            );

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
                    // The PtyEventSink only writes `events_emitted` into the
                    // binding snapshot; the real frame / byte / prompt /
                    // suppression counters live on the probe (mirrors
                    // `stop()`). Lock the probe ONCE per entry to read both
                    // the metrics and the secret-prompt flag. If the probe is
                    // busy (e.g. `write_stdin` holding it across `.await`),
                    // fall back to the snapshot metrics + secret=false: a
                    // momentary stale read is acceptable and must never block
                    // `list()`. `list()` is read-only — never cancel here.
                    let (metrics, secret) = if let Ok(guard) = b.probe.try_lock() {
                        let probe_metrics = guard
                            .as_ref()
                            .map_or_else(PtyProbeMetrics::default, PtyProbe::metrics);
                        let secret = guard
                            .as_ref()
                            .is_some_and(PtyProbe::is_secret_prompt_active);
                        // The sink owns `events_emitted` (see
                        // `combine_pty_metrics`); everything else is the
                        // probe's real workload.
                        let sink_snap = b.metrics_snapshot.lock().clone();
                        let metrics = combine_pty_metrics(&probe_metrics, &sink_snap);
                        (metrics, secret)
                    } else {
                        (b.metrics_snapshot.lock().clone(), false)
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

        /// spec 004 FR-004: answer a `command_status` read for a job THIS lane
        /// owns, with real counters.
        ///
        /// The job ledger is shared across lanes, so `CommandRuntime::status`
        /// used to find PTY jobs and report zeroes for them while asserting
        /// they were observed live. The counters were never missing -- they
        /// live on the probe, exactly where `list()` reads them. Returns `None`
        /// when this lane does not own the id, so the caller can try the next.
        #[must_use]
        pub fn status(
            &self,
            job_id: JobId,
        ) -> Option<terminal_commander_ipc::protocol::CommandStatusResponse> {
            use terminal_commander_ipc::protocol::OutcomeTrust;

            let (bucket_id, probe_id, metrics, awaiting_credential) = {
                let g = self.live.read();
                let b = g.get(&job_id)?;
                // Same read shape as `list()`: prefer the probe's live counters,
                // fall back to the sink snapshot if the probe is momentarily
                // busy. Never block a status read.
                let (metrics, awaiting) = if let Ok(guard) = b.probe.try_lock() {
                    let probe_metrics = guard
                        .as_ref()
                        .map_or_else(PtyProbeMetrics::default, PtyProbe::metrics);
                    let sink_snap = b.metrics_snapshot.lock().clone();
                    (
                        combine_pty_metrics(&probe_metrics, &sink_snap),
                        guard.as_ref().and_then(awaiting_of),
                    )
                } else {
                    (b.metrics_snapshot.lock().clone(), None)
                };
                (b.bucket_id, b.probe_id, metrics, awaiting)
            };
            let rec = self.jobs.get(job_id)?;
            Some(terminal_commander_ipc::protocol::CommandStatusResponse {
                job_id,
                bucket_id,
                probe_id,
                state: rec.state,
                frames_total: metrics.frames_total,
                // A PTY is ONE merged stream by construction -- there is no
                // separate stderr channel to report. Attributing the frames to
                // stdout is truthful; splitting them would be invented detail.
                frames_stdout: metrics.frames_total,
                frames_stderr: 0,
                bytes_total: metrics.bytes_total,
                events_emitted: metrics.events_emitted,
                frames_suppressed: metrics.frames_suppressed,
                frames_suppressed_progress: metrics.frames_suppressed_progress,
                frames_suppressed_dedupe: metrics.frames_suppressed_dedupe,
                exit_code: rec.exit_info.as_ref().and_then(|e| e.exit_code),
                signal: rec.exit_info.as_ref().and_then(|e| e.signal.clone()),
                duration_ms: rec.exit_info.as_ref().map(|e| e.duration_ms),
                // The no-silence receipt is a combed-lane construct; the PTY
                // waiter never builds one.
                receipt: None,
                restarted: false,
                outcome_trust: OutcomeTrust::Observed,
                pipeline_exit_masked: false,
                awaiting_credential,
            })
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

    /// FCR2-002: absolute path of the program the ConPTY spawn will run, or
    /// `None` when nothing on disk matches.
    ///
    /// `portable-pty` searches PATH by REPLACING a typed extension with each
    /// PATHEXT entry (`bash.txt` -> `bash.exe`), even for an absolute path that
    /// does not exist. Resolve with the argv lane's stem-preserving search
    /// instead, honoring a request `PATH` like the backend does, and return an
    /// absolute path its search keeps verbatim.
    #[cfg(windows)]
    fn resolve_pty_argv0(argv0: &str, env: &[(OsString, OsString)]) -> Option<String> {
        let found = if argv0.contains(['\\', '/']) {
            std::path::Path::new(argv0)
                .is_file()
                .then(|| argv0.to_owned())?
        } else {
            let path = env
                .iter()
                .rev()
                .find(|(k, _)| k.eq_ignore_ascii_case("PATH"))
                .map(|(_, v)| v.clone())
                .or_else(|| std::env::var_os("PATH"))?;
            let pathext =
                std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());
            crate::command::resolve_on_path(argv0, &path, &pathext)?
        };
        std::path::absolute(found)
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// Request env keys that decide which program runs or what it loads.
    const PROGRAM_ENV: [&str; 6] = [
        "PATH",
        "PATHEXT",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "SSH_ASKPASS",
        "SUDO_ASKPASS",
    ];

    /// Keys only (never values), case-insensitive, first spelling kept.
    fn program_env_overrides(env: &[(OsString, OsString)]) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for (key, _) in env {
            let key = key.to_string_lossy();
            let upper = key.to_ascii_uppercase();
            if (PROGRAM_ENV.contains(&upper.as_str()) || upper.starts_with("DYLD_"))
                && !keys.iter().any(|k| k.eq_ignore_ascii_case(&key))
            {
                keys.push(key.into_owned());
            }
        }
        keys
    }

    /// The program the owner is told about. On Windows the spawn path is
    /// already resolved and absolute.
    #[cfg(windows)]
    fn shown_program(
        spawn_argv0: &str,
        _env: &[(OsString, OsString)],
        _cwd: Option<&std::path::Path>,
    ) -> String {
        spawn_argv0.to_owned()
    }

    /// The program the owner is told about. Unix spawns the typed name and
    /// the exec searches the request's PATH (else the daemon's), so search
    /// it the same way here.
    ///
    /// ponytail: display-only search; an empty PATH entry resolves against
    /// the daemon's cwd, not the job's.
    #[cfg(not(windows))]
    fn shown_program(
        argv0: &str,
        env: &[(OsString, OsString)],
        cwd: Option<&std::path::Path>,
    ) -> String {
        use std::os::unix::fs::PermissionsExt;
        let typed = std::path::Path::new(argv0);
        let found = if argv0.contains('/') {
            Some(match cwd {
                Some(dir) if typed.is_relative() => dir.join(typed),
                _ => typed.to_path_buf(),
            })
        } else {
            let search = env
                .iter()
                .rev()
                .find(|(k, _)| k == "PATH")
                .map(|(_, v)| v.clone())
                .or_else(|| std::env::var_os("PATH"))
                .unwrap_or_default();
            std::env::split_paths(&search)
                .map(|dir| dir.join(argv0))
                .find(|c| {
                    c.metadata()
                        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                })
        };
        found
            .and_then(|p| std::path::absolute(p).ok())
            .map_or_else(|| argv0.to_owned(), |p| p.to_string_lossy().into_owned())
    }

    /// Unix `execvp` searches PATH without rewriting the name: run it as typed.
    #[cfg(not(windows))]
    #[allow(clippy::unnecessary_wraps)]
    fn resolve_pty_argv0(argv0: &str, _env: &[(OsString, OsString)]) -> Option<String> {
        Some(argv0.to_owned())
    }

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

    /// Combine probe-side and sink-side PTY metrics into the value
    /// surfaced by `stop()` and `list()`.
    ///
    /// The probe owns the real workload counters
    /// (`frames_total` / `bytes_total` / prompt / suppression); the
    /// `PtyEventSink` only records `events_emitted` into the binding
    /// snapshot. Everything but `events_emitted` therefore comes from
    /// `probe`; `events_emitted` is the max of the two so a race between
    /// the probe finalizing and the sink emitting cannot lose the count.
    ///
    /// This was the exact F9 footgun (the snapshot's zeroed frame/byte
    /// counters leaking into `list()`); keeping the combine in one place
    /// means there is a single line to get right and a single line to
    /// test.
    fn combine_pty_metrics(probe: &PtyProbeMetrics, snapshot: &PtyProbeMetrics) -> PtyProbeMetrics {
        PtyProbeMetrics {
            events_emitted: probe.events_emitted.max(snapshot.events_emitted),
            ..*probe
        }
    }

    /// Build the bounded evidence object persisted alongside a PTY job receipt
    /// (spec 004 FR-002).
    ///
    /// NUMERIC AND IDENTIFIER DATA ONLY -- constitution III bars raw frames from
    /// persistent output, and a PTY stream is exactly where secret-prompt echo
    /// could appear, so no frame text is written here under any circumstance.
    ///
    /// A PTY is ONE merged stream, so there is no stdout/stderr split to record;
    /// the reader attributes `frames_total` to stdout, matching `status()`.
    fn pty_evidence_json(
        metrics: &PtyProbeMetrics,
        duration_ms: Option<u64>,
        probe_id: ProbeId,
    ) -> String {
        let duration = duration_ms.map_or_else(|| "null".to_owned(), |d| d.to_string());
        format!(
            "{{\"frames_total\":{},\"frames_stdout\":{},\"frames_stderr\":0,\
             \"bytes_total\":{},\"frames_suppressed\":{},\
             \"frames_suppressed_progress\":{},\"frames_suppressed_dedupe\":{},\
             \"duration_ms\":{},\"probe_id\":\"{}\"}}",
            metrics.frames_total,
            metrics.frames_total,
            metrics.bytes_total,
            metrics.frames_suppressed,
            metrics.frames_suppressed_progress,
            metrics.frames_suppressed_dedupe,
            duration,
            probe_id.to_wire_string(),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::{PtyProbeMetrics, combine_pty_metrics};

        #[test]
        fn combine_takes_workload_counters_from_probe_not_snapshot() {
            // The probe carries the real workload; the snapshot's
            // frame/byte/prompt/suppression counters are structurally
            // zero (only the sink's `events_emitted` is ever written
            // there). This is the F9 regression guard: those probe
            // counters must survive the combine even when the snapshot
            // is all-default.
            let probe = PtyProbeMetrics {
                frames_total: 192,
                bytes_total: 1876,
                events_emitted: 5,
                prompts_total: 3,
                secret_prompts_total: 1,
                stdin_bytes_written: 42,
                stdin_writes_denied_secret: 2,
                frames_suppressed: 7,
                frames_suppressed_progress: 4,
                frames_suppressed_dedupe: 3,
            };
            let snapshot = PtyProbeMetrics::default();

            let combined = combine_pty_metrics(&probe, &snapshot);

            assert_eq!(combined.frames_total, 192, "frames must come from probe");
            assert_eq!(combined.bytes_total, 1876, "bytes must come from probe");
            assert_eq!(combined.prompts_total, 3);
            assert_eq!(combined.secret_prompts_total, 1);
            assert_eq!(combined.stdin_bytes_written, 42);
            assert_eq!(combined.stdin_writes_denied_secret, 2);
            assert_eq!(combined.frames_suppressed, 7);
            assert_eq!(combined.frames_suppressed_progress, 4);
            assert_eq!(combined.frames_suppressed_dedupe, 3);
        }

        #[test]
        fn combine_events_emitted_is_max_probe_greater() {
            let probe = PtyProbeMetrics {
                events_emitted: 9,
                frames_total: 10,
                ..PtyProbeMetrics::default()
            };
            let snapshot = PtyProbeMetrics {
                events_emitted: 4,
                ..PtyProbeMetrics::default()
            };

            let combined = combine_pty_metrics(&probe, &snapshot);

            assert_eq!(combined.events_emitted, 9, "probe events > snapshot wins");
            assert_eq!(
                combined.frames_total, 10,
                "non-event fields still from probe"
            );
        }

        #[test]
        fn combine_events_emitted_is_max_snapshot_greater() {
            // The sink may have emitted more than the probe has recorded
            // at the instant we read (e.g. probe metrics lagging the sink
            // by a frame). The snapshot value must win for `events_emitted`
            // so the count is never lost.
            let probe = PtyProbeMetrics {
                events_emitted: 4,
                frames_total: 10,
                ..PtyProbeMetrics::default()
            };
            let snapshot = PtyProbeMetrics {
                events_emitted: 9,
                // A non-zero snapshot frame count must NOT leak through;
                // only `events_emitted` is taken from the snapshot.
                frames_total: 999,
                ..PtyProbeMetrics::default()
            };

            let combined = combine_pty_metrics(&probe, &snapshot);

            assert_eq!(combined.events_emitted, 9, "snapshot events > probe wins");
            assert_eq!(
                combined.frames_total, 10,
                "frames must come from probe, never the snapshot"
            );
        }
    }
}

#[cfg(any(unix, windows))]
pub use runtime::{
    CredentialPrompt, LivePtyIdentity, PtyRebindReport, PtyRuntime, PtyRuntimeError,
    PtyStartRequest, PtyStartResponse, PtyWriteResponse,
};
