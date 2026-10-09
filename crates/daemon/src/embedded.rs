// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Supported in-process entry point. Every operation uses the daemon dispatcher;
//! no socket is opened and no external daemon is discovered or attached.
//!
//! The host is the trust boundary. Untrusted guests must receive a host-controlled
//! projection of these methods, never a Rust handle or a caller-supplied identity.

use crate::{BootstrapError, DaemonConfig, DaemonState};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use terminal_commander_supervisor::identity::PeerIdentity;
use tokio::sync::RwLock;

// This facade mirrors the complete typed protocol through the methods table.
#[allow(clippy::wildcard_imports)]
use protocol::*;
pub use terminal_commander_core as core;
pub use terminal_commander_ipc::compound::{RunAndWatchOptions, RunAndWatchOutcome};
pub use terminal_commander_ipc::engine::*;
pub use terminal_commander_ipc::protocol;
pub use terminal_commander_probes as probes;
pub use terminal_commander_sifters as sifters;

pub use protocol::IsolatedCommandParams as IsolatedCommand;

/// A successful explicit shutdown acknowledges store closure and bounded task drain.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShutdownReport {
    pub store_closed: bool,
    pub abandoned_jobs: u32,
    pub lifecycle_drained: bool,
}

/// A trusted host may opt in to owner/admin methods for its actual OS identity.
/// This grant must never be selected from untrusted room requests or JSON.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EmbeddedAuthority {
    #[default]
    PolicyOnly,
    HostAdministrator,
}

/// Local owner UI selection; never selected by a serialized guest request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EmbeddedOwnerInteraction {
    #[default]
    HostManaged,
    /// Ask through the existing OS/helper prompt; if unavailable, return the
    /// same typed host challenge as `HostManaged` (never an unbound CLI hint).
    NativePrompt,
}

/// Trusted bootstrap choices. Deliberately not serializable as wire authority.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmbeddedOptions {
    pub authority: EmbeddedAuthority,
    pub owner_interaction: EmbeddedOwnerInteraction,
}

struct Inner {
    state: Arc<DaemonState>,
    boot: Instant,
    peer: PeerIdentity,
    admission: Arc<RwLock<()>>,
    stopping: AtomicBool,
    closed: AtomicBool,
    shutdown_report: parking_lot::Mutex<Option<ShutdownReport>>,
    session_reaper: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
    data_dir_lock:
        parking_lot::Mutex<Option<terminal_commander_supervisor::proc_lock::ProcessLock>>,
}

/// A complete policy-checked engine. Clones share one boot, store and lifecycle.
#[derive(Clone)]
pub struct EmbeddedEngine {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for EmbeddedEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddedEngine")
            .field("identity", &self.identity())
            .field("stopping", &self.inner.stopping.load(Ordering::Acquire))
            .finish()
    }
}

macro_rules! methods {
    ($($name:ident($params:ty) => $request:ident, $response:ident($result:ty);)*) => {
        $(pub async fn $name(&self, params: $params) -> Result<$result, IpcError> {
            match self.execute(IpcRequest::$request(params)).await? {
                IpcResponse::$response(value) => Ok(value),
                _ => Err(unexpected_response()),
            }
        })*
    };
}

macro_rules! no_params {
    ($($name:ident => $request:ident($result:ty);)*) => {
        $(pub async fn $name(&self) -> Result<$result, IpcError> {
            match self.execute(IpcRequest::$request).await? {
                IpcResponse::$request(value) => Ok(value),
                _ => Err(unexpected_response()),
            }
        })*
    };
}

impl EmbeddedEngine {
    /// Uses only the supplied configuration. Does not load ambient daemon config,
    /// register a listener, discover sessions, or attach a host MCP service.
    pub fn bootstrap(config: DaemonConfig) -> Result<Self, BootstrapError> {
        Self::bootstrap_with_authority(config, EmbeddedAuthority::PolicyOnly)
    }

    /// Explicit authority is attached only to this engine and the host's real OS peer.
    pub fn bootstrap_with_authority(
        config: DaemonConfig,
        authority: EmbeddedAuthority,
    ) -> Result<Self, BootstrapError> {
        Self::bootstrap_with_options(
            config,
            EmbeddedOptions {
                authority,
                ..EmbeddedOptions::default()
            },
        )
    }

    /// Select local owner interaction in addition to explicit authority.
    pub fn bootstrap_with_options(
        mut config: DaemonConfig,
        options: EmbeddedOptions,
    ) -> Result<Self, BootstrapError> {
        config.validate_and_clamp()?;
        std::fs::create_dir_all(&config.daemon.data_dir).map_err(|source| {
            BootstrapError::CreateDataDir {
                path: config.daemon.data_dir.clone(),
                source,
            }
        })?;
        let lock_path = config.daemon.data_dir.join("terminal-commanderd.data.lock");
        let data_dir_lock = match terminal_commander_supervisor::proc_lock::try_acquire(&lock_path)
            .map_err(BootstrapError::DataDirLock)?
        {
            terminal_commander_supervisor::proc_lock::TryLockResult::Acquired(lock) => lock,
            terminal_commander_supervisor::proc_lock::TryLockResult::Contended => {
                return Err(BootstrapError::DataDirInUse(config.daemon.data_dir));
            }
        };
        let mut state = DaemonState::bootstrap(config)?;
        state.embedded_host_admin = options.authority == EmbeddedAuthority::HostAdministrator;
        #[cfg(any(unix, windows))]
        Arc::get_mut(&mut state.credentials)
            .expect("new engine exclusively owns its credential broker")
            .use_embedded_owner_interaction(
                state.boot_id,
                options.owner_interaction == EmbeddedOwnerInteraction::NativePrompt,
            );
        let state = Arc::new(state);
        let session_reaper = crate::runtime::spawn_session_reaper(&state);
        Ok(Self {
            inner: Arc::new(Inner {
                state,
                boot: Instant::now(),
                peer: current_identity(),
                admission: Arc::new(RwLock::new(())),
                stopping: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                shutdown_report: parking_lot::Mutex::new(None),
                session_reaper: parking_lot::Mutex::new(session_reaper),
                data_dir_lock: parking_lot::Mutex::new(Some(data_dir_lock)),
            }),
        })
    }

    pub fn identity(&self) -> EngineIdentity {
        engine_identity(&self.inner.state)
    }

    pub fn capabilities(&self) -> Vec<EngineCapability> {
        engine_capabilities(&self.inner.state)
    }

    /// Transport-independent service entry point. Hosts may serialize the existing
    /// request/response envelopes on a room-owned transport; no listener is created.
    pub async fn execute_envelope(&self, envelope: RequestEnvelope) -> ResponseEnvelope {
        let result = match ensure_bounded(&envelope) {
            Ok(()) => self.execute(envelope.request).await,
            Err(error) => Err(error),
        };
        ResponseEnvelope {
            correlation_id: envelope.correlation_id,
            result: match result {
                Ok(response) => IpcResult::Ok { response },
                Err(error) => IpcResult::Err { error },
            },
        }
    }

    /// Typed exhaustive operation union, including operations added to IPC in future.
    /// No automatic retries. Cancelling this future does not replay its operation;
    /// an admitted operation finishes under the shutdown drain gate.
    pub async fn execute(&self, request: IpcRequest) -> Result<IpcResponse, IpcError> {
        if matches!(request, IpcRequest::Shutdown) {
            self.shutdown().await?;
            return Ok(IpcResponse::ShutdownAck { draining: false });
        }
        ensure_bounded(&request)?;
        let inner = Arc::clone(&self.inner);
        let permit = Arc::clone(&inner.admission).read_owned().await;
        if inner.stopping.load(Ordering::Acquire) {
            return Err(shutting_down());
        }
        tokio::spawn(async move {
            let _permit = permit;
            let envelope = RequestEnvelope {
                correlation_id: 0,
                request,
            };
            let response = crate::ipc::server::dispatch_envelope(
                &inner.state,
                inner.boot,
                &envelope,
                &inner.peer,
            )
            .await;
            match response.result {
                IpcResult::Ok { response } => {
                    ensure_bounded(&response)?;
                    Ok(response)
                }
                IpcResult::Err { error } => Err(error),
            }
        })
        .await
        .map_err(|_| IpcError::new(IpcErrorCode::Internal, "engine operation task failed"))?
    }

    pub async fn health(&self) -> Result<EngineHealth, IpcError> {
        match self.execute(IpcRequest::Health).await? {
            IpcResponse::Health {
                uptime_secs,
                idle_secs,
                identity: Some(identity),
                ..
            } => Ok(EngineHealth {
                identity,
                uptime_secs,
                idle_secs: idle_secs.unwrap_or_default(),
            }),
            _ => Err(unexpected_response()),
        }
    }

    /// Same validation, policy, limits, sifters, receipts and audit path as argv
    /// execution. Requires absolute executable + existing absolute cwd; child env
    /// is exactly the supplied set. This is not a filesystem or network sandbox.
    pub async fn command_start_isolated(
        &self,
        request: IsolatedCommand,
    ) -> Result<CommandStartResponse, IpcError> {
        match self
            .execute(IpcRequest::CommandStartIsolated(request))
            .await?
        {
            IpcResponse::CommandStartCombed(response) => Ok(response),
            _ => Err(unexpected_response()),
        }
    }

    /// Start once and observe within a bounded wall-clock budget. A degraded
    /// observation retains the identifiers and cursor needed to resume safely.
    pub async fn run_and_watch(
        &self,
        start: CommandStartParams,
        options: RunAndWatchOptions,
    ) -> Result<RunAndWatchOutcome, IpcError> {
        terminal_commander_ipc::compound::run_and_watch(start, options, |request| {
            self.execute(request)
        })
        .await
    }

    /// Stop admission, finish already admitted requests, cancel every live lane,
    /// drain lifecycle receipts, mark uncertain jobs abandoned, then close SQLite.
    /// Idempotent. A bounded drain timeout is reported, never a fabricated exit.
    pub async fn shutdown(&self) -> Result<ShutdownReport, IpcError> {
        self.inner.stopping.store(true, Ordering::Release);
        let engine = self.clone();
        // Once requested, shutdown owns its lifetime even if a caller drops
        // the waiting future (for example when its transport disconnects).
        tokio::spawn(async move { engine.shutdown_inner().await })
            .await
            .map_err(|_| IpcError::new(IpcErrorCode::Internal, "engine shutdown task failed"))?
    }

    async fn shutdown_inner(&self) -> Result<ShutdownReport, IpcError> {
        let _permit = self.inner.admission.write().await;
        let previous_report = self.inner.shutdown_report.lock().clone();
        if let Some(report) = previous_report {
            return Ok(report);
        }
        self.inner.state.trigger_shutdown();
        let session_reaper = self.inner.session_reaper.lock().take();
        if let Some(task) = session_reaper {
            task.abort();
            let _ = task.await;
        }
        stop_process_lanes(&self.inner.state);
        let watches_drained = self.inner.state.watch.drain_watches().await;
        let commands_drained = self
            .inner
            .state
            .command
            .drain_lifecycle_tasks_report()
            .await;
        #[cfg(any(unix, windows))]
        let pty_drained = self.inner.state.pty.drain_lifecycle_tasks().await;
        #[cfg(not(any(unix, windows)))]
        let pty_drained = true;
        let abandoned_jobs = self.inner.state.record_abandoned_jobs();
        let state = Arc::clone(&self.inner.state);
        tokio::task::spawn_blocking(move || state.store.shutdown())
            .await
            .map_err(|_| IpcError::new(IpcErrorCode::Internal, "store shutdown task failed"))?
            .map_err(|_| IpcError::new(IpcErrorCode::Internal, "store shutdown failed"))?;
        self.inner.closed.store(true, Ordering::Release);
        self.inner.data_dir_lock.lock().take();
        let report = ShutdownReport {
            store_closed: true,
            abandoned_jobs,
            lifecycle_drained: commands_drained && pty_drained && watches_drained,
        };
        *self.inner.shutdown_report.lock() = Some(report.clone());
        Ok(report)
    }

    no_params! {
        system_discover => SystemDiscover(DiscoverResponse);
        policy_status => PolicyStatus(PolicyStatusResponse);
        self_check => SelfCheck(SelfCheckResponse);
        file_watch_list => FileWatchList(FileWatchListResponse);
        pty_command_list => PtyCommandList(PtyCommandListResponse);
        shell_session_list => ShellSessionList(ShellSessionListResponse);
    }

    methods! {
        command_start_combed(CommandStartParams) => CommandStartCombed, CommandStartCombed(CommandStartResponse);
        command_status(CommandStatusParams) => CommandStatus, CommandStatus(CommandStatusResponse);
        command_stop(CommandStopParams) => CommandStop, CommandStop(CommandStopResponse);
        command_output_tail(CommandOutputTailParams) => CommandOutputTail, CommandOutputTail(CommandOutputTailResponse);
        shell_exec(ShellExecParams) => ShellExec, CommandStartCombed(CommandStartResponse);
        bucket_events_since(BucketEventsSinceParams) => BucketEventsSince, BucketEventsSince(BucketEventsSinceResponse);
        bucket_wait(BucketWaitParams) => BucketWait, BucketWait(BucketWaitResponse);
        bucket_summary(BucketSummaryParams) => BucketSummary, BucketSummary(BucketSummaryResponse);
        event_context(EventContextParams) => EventContext, EventContext(EventContextResponse);
        registry_search(RegistrySearchParams) => RegistrySearch, RegistrySearch(RegistrySearchResponse);
        registry_get(RegistryGetParams) => RegistryGet, RegistryGet(RegistryGetResponse);
        registry_upsert(RegistryUpsertParams) => RegistryUpsert, RegistryUpsert(RegistryUpsertResponse);
        registry_test(RegistryTestParams) => RegistryTest, RegistryTest(RegistryTestResponse);
        registry_activate(RegistryActivateParams) => RegistryActivate, RegistryActivate(RegistryActivateResponse);
        registry_import_pack(RegistryImportPackParams) => RegistryImportPack, RegistryImportPack(RegistryImportPackResponse);
        registry_deactivate(RegistryDeactivateParams) => RegistryDeactivate, RegistryDeactivate(RegistryDeactivateResponse);
        registry_deactivate_bulk(RegistryDeactivateBulkParams) => RegistryDeactivateBulk, RegistryDeactivateBulk(RegistryDeactivateBulkResponse);
        registry_list_active(ListLimitParams) => RegistryListActive, RegistryListActive(RegistryListActiveResponse);
        registry_suggest_from_samples(RegistrySuggestFromSamplesParams) => RegistrySuggestFromSamples, RegistrySuggestFromSamples(RegistrySuggestFromSamplesResponse);
        recipe_search(RecipeSearchParams) => RecipeSearch, RecipeSearch(RecipeSearchResponse);
        recipe_get(RecipeGetParams) => RecipeGet, RecipeGet(RecipeGetResponse);
        recipe_upsert(RecipeUpsertParams) => RecipeUpsert, RecipeUpsert(RecipeUpsertResponse);
        recipe_test(RecipeTestParams) => RecipeTest, RecipeTest(RecipeTestResponse);
        recipe_activate(RecipeActivateParams) => RecipeActivate, RecipeActivate(RecipeActivateResponse);
        recipe_deactivate(RecipeDeactivateParams) => RecipeDeactivate, RecipeDeactivate(RecipeDeactivateResponse);
        recipe_list_active(ListLimitParams) => RecipeListActive, RecipeListActive(RecipeListActiveResponse);
        recipe_run(RecipeRunParams) => RecipeRun, RecipeRun(RecipeRunResponse);
        recipe_list_versions(RecipeListVersionsParams) => RecipeListVersions, RecipeListVersions(RecipeListVersionsResponse);
        recipe_tombstone(RecipeTombstoneParams) => RecipeTombstone, RecipeTombstone(RecipeTombstoneResponse);
        recipe_import_seeds(RecipeImportSeedsParams) => RecipeImportSeeds, RecipeImportSeeds(RecipeImportSeedsResponse);
        file_read_window(FileReadWindowParams) => FileReadWindow, FileReadWindow(FileReadWindowResponse);
        file_search(FileSearchParams) => FileSearch, FileSearch(FileSearchResponse);
        file_list_dir(FileListDirParams) => FileListDir, FileListDir(FileListDirResponse);
        file_write(FileWriteParams) => FileWrite, FileWrite(FileWriteResponse);
        file_watch_start(FileWatchStartParams) => FileWatchStart, FileWatchStart(FileWatchStartResponse);
        file_watch_stop(FileWatchStopParams) => FileWatchStop, FileWatchStop(FileWatchStopResponse);
        pty_command_start(PtyCommandStartParams) => PtyCommandStart, PtyCommandStart(PtyCommandStartResponse);
        pty_command_write_stdin(PtyCommandWriteStdinParams) => PtyCommandWriteStdin, PtyCommandWriteStdin(PtyCommandWriteStdinResponse);
        pty_command_stop(PtyCommandStopParams) => PtyCommandStop, PtyCommandStop(PtyCommandStopResponse);
        credential_request(CredentialRequestParams) => CredentialRequest, CredentialRequest(CredentialRequestResponse);
        credential_provide(CredentialProvideParams) => CredentialProvide, CredentialProvide(CredentialProvideResponse);
        credential_provide_challenge(CredentialProvideChallengeParams) => CredentialProvideChallenge, CredentialProvide(CredentialProvideResponse);
        credential_url(CredentialUrlParams) => CredentialUrl, CredentialUrl(CredentialUrlResponse);
        shell_session_start(ShellSessionStartParams) => ShellSessionStart, ShellSessionStart(ShellSessionStartResponse);
        shell_session_exec(ShellSessionExecParams) => ShellSessionExec, ShellSessionExec(ShellSessionExecResponse);
        shell_session_status(ShellSessionStatusParams) => ShellSessionStatus, ShellSessionStatus(ShellSessionStatusResponse);
        shell_session_stop(ShellSessionStopParams) => ShellSessionStop, ShellSessionStop(ShellSessionStopResponse);
        workspace_snapshot_create(WorkspaceSnapshotCreateParams) => WorkspaceSnapshotCreate, WorkspaceSnapshotCreate(WorkspaceSnapshotCreateResponse);
        workspace_snapshot_apply(WorkspaceSnapshotApplyParams) => WorkspaceSnapshotApply, WorkspaceSnapshotApply(WorkspaceSnapshotApplyResponse);
        runtime_state(ListLimitParams) => RuntimeState, RuntimeState(RuntimeStateResponse);
        probe_list(ListLimitParams) => ProbeList, ProbeList(ProbeListResponse);
        probe_status(ProbeStatusParams) => ProbeStatus, ProbeStatus(ProbeStatusResponse);
        audit_since(AuditSinceParams) => AuditSince, AuditSince(AuditSinceResponse);
        subscription_open(SubscriptionOpenParams) => SubscriptionOpen, SubscriptionOpen(SubscriptionOpenResponse);
        subscription_pull(SubscriptionPullParams) => SubscriptionPull, SubscriptionPull(SubscriptionPullResponse);
        subscription_list(SubscriptionListParams) => SubscriptionList, SubscriptionList(SubscriptionListResponse);
        subscription_close(SubscriptionCloseParams) => SubscriptionClose, SubscriptionClose(SubscriptionCloseResponse);
        subscription_seek(SubscriptionSeekParams) => SubscriptionSeek, SubscriptionSeek(SubscriptionSeekResponse);
    }

    pub async fn quiesce_for_replace(&self) -> Result<u32, IpcError> {
        match self.execute(IpcRequest::QuiesceForReplace).await? {
            IpcResponse::QuiesceAck { recorded } => Ok(recorded),
            _ => Err(unexpected_response()),
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if !self.closed.load(Ordering::Acquire) {
            self.stopping.store(true, Ordering::Release);
            self.state.trigger_shutdown();
            let session_reaper = self.session_reaper.lock().take();
            if let Some(task) = session_reaper {
                task.abort();
            }
            // No async wait is possible in Drop. Record uncertainty before abort;
            // ProcessProbe's OS ownership guard remains independent of Tokio.
            self.state.record_abandoned_jobs();
            stop_process_lanes(&self.state);
            for watch in self.state.watch.live_watches() {
                let _ = self.state.watch.stop(watch.watch_id);
            }
            self.state.command.abort_lifecycle_tasks();
            #[cfg(any(unix, windows))]
            self.state.pty.abort_lifecycle_tasks();
            let _ = self.state.store.shutdown();
        }
    }
}

fn stop_process_lanes(state: &DaemonState) {
    #[cfg(unix)]
    for session in state.sessions.list() {
        state.sessions.stop(session.session_id);
    }
    for job in state.command.live_jobs() {
        let _ = state.command.stop(job.job_id, "embedded_shutdown");
    }
    #[cfg(any(unix, windows))]
    for job in state.pty.live_jobs() {
        let _ = state.pty.stop(job.job_id);
    }
}

// The build-generated feature list can be empty or nonempty depending on flags.
#[allow(clippy::manual_string_new)]
pub(crate) fn engine_identity(state: &DaemonState) -> EngineIdentity {
    EngineIdentity {
        api_version: ENGINE_API_VERSION,
        schema_version: ENGINE_SCHEMA_VERSION,
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        instance_id: state.boot_id.to_string(),
        build: BuildIdentity {
            source_fingerprint: env!("TC_SOURCE_FINGERPRINT").to_owned(),
            target: env!("TC_BUILD_TARGET").to_owned(),
            compiler: env!("TC_BUILD_COMPILER").to_owned(),
            profile: env!("TC_BUILD_PROFILE").to_owned(),
            features: env!("TC_BUILD_FEATURES").to_owned(),
            provenance: option_env!("TC_BUILD_ID").map(str::to_owned),
        },
    }
}

pub(crate) fn engine_capabilities(state: &DaemonState) -> Vec<EngineCapability> {
    use EngineFeature as F;
    use FeatureAvailability as A;
    let mut capabilities: Vec<_> = [
        F::Commands,
        F::IsolatedCommands,
        F::Files,
        F::FileWatches,
        F::Sifters,
        F::Buckets,
        F::Context,
        F::Tails,
        F::Registry,
        F::Recipes,
        F::Subscriptions,
        F::Policy,
        F::Audit,
        F::ResourceLimits,
        F::ProcessObservation,
    ]
    .into_iter()
    .map(|feature| EngineCapability {
        feature,
        availability: A::Available,
    })
    .collect();
    capabilities.extend([
        EngineCapability {
            feature: F::Shell,
            availability: if state.policy.caps_allow_shell() {
                A::Available
            } else {
                A::DeniedByPolicy
            },
        },
        EngineCapability {
            feature: F::Pty,
            availability: if cfg!(any(unix, windows)) {
                A::Available
            } else {
                A::UnsupportedPlatform
            },
        },
        EngineCapability {
            feature: F::ShellSessions,
            availability: if !cfg!(unix) {
                A::UnsupportedPlatform
            } else if state.config.resolved_caps().allow_session {
                A::Available
            } else {
                A::DeniedByPolicy
            },
        },
        EngineCapability {
            feature: F::WholeJobCpu,
            availability: if cfg!(any(target_os = "linux", windows)) {
                A::Available
            } else {
                A::UnsupportedPlatform
            },
        },
        EngineCapability {
            feature: F::OwnerCredentials,
            availability: if !cfg!(any(unix, windows)) {
                A::UnsupportedPlatform
            } else if state.embedded_host_admin {
                A::Available
            } else {
                A::HostPermissionRequired
            },
        },
        EngineCapability {
            feature: F::RemoteTargets,
            availability: A::HostTransportRequired,
        },
    ]);
    capabilities
}

fn current_identity() -> PeerIdentity {
    #[cfg(unix)]
    {
        // SAFETY: identity getters have no preconditions or side effects.
        PeerIdentity::Unix {
            uid: unsafe { libc::geteuid() },
            gid: unsafe { libc::getegid() },
            pid: i32::try_from(std::process::id()).ok(),
        }
    }
    #[cfg(windows)]
    {
        let pid = std::process::id();
        crate::ipc::peer_windows::resolve_sid_and_image(pid).map_or_else(
            || PeerIdentity::unknown_because("host identity unavailable"),
            |(sid, image)| PeerIdentity::Windows {
                sid,
                pid: Some(pid),
                image,
            },
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        PeerIdentity::unknown_because("host identity unsupported on this platform")
    }
}

pub(crate) fn host_admin_granted(state: &DaemonState, peer: &PeerIdentity) -> bool {
    state.embedded_host_admin && peer.is_known() && peer == &current_identity()
}

fn shutting_down() -> IpcError {
    IpcError::new(
        IpcErrorCode::ShuttingDown,
        "embedded engine is shutting down",
    )
}
fn unexpected_response() -> IpcError {
    IpcError::new(
        IpcErrorCode::Internal,
        "engine response did not match operation",
    )
}

fn ensure_bounded(value: &impl serde::Serialize) -> Result<(), IpcError> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            if self.0 > MAX_FRAME_BYTES {
                return Err(std::io::ErrorKind::FileTooLarge.into());
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter(0), value).map_err(|_| {
        IpcError::new(
            IpcErrorCode::ArgvInvalid,
            "engine payload exceeds frame limit or cannot be serialized",
        )
    })
}
