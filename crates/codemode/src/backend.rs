// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Host backend traits, async-to-sync runtime bridge, `DaemonClient` transport,
//! and complete 60-operation dispatcher matching Terminal Commander's live MCP
//! wire contracts.

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use terminal_commander_core::ids::{BucketIdKind, JobIdKind, ProbeIdKind, SessionIdKind};
use terminal_commander_core::{
    BucketId, BucketWakeReason, JobId, JobState, RecipeDefinition, RuleDefinition, Severity,
    SignalEvent,
};
use terminal_commander_ipc::compound::{
    MAX_WAIT_SLICE_MS, RUN_AND_WATCH_DEFAULT_MAX_SIGNALS, RUN_AND_WATCH_DEFAULT_WAIT_MS,
    RUN_AND_WATCH_MAX_SIGNALS, RUN_AND_WATCH_MAX_WAIT_MS, RunAndWatchOptions, RunAndWatchOutcome,
    run_and_watch_with_ceiling,
};
use terminal_commander_ipc::protocol::{SubscriptionSeekParams, SubscriptionSeekResponse};
use terminal_commander_ipc::{
    AuditSinceParams, BucketEventsSinceResponse, BucketSummaryParams, BucketSummaryResponse,
    BucketWaitParams, BucketWaitResponse, CREDENTIAL_REQUEST_WAIT_MS, CommandOutputTailParams,
    CommandOutputTailResponse, CommandStartResponse, CommandStatusParams, CommandStatusResponse,
    CommandStopParams, CommandStopResponse, ContextUnavailableReason, CredentialRequestParams,
    DaemonClient, DiscoverResponse, EventContextResponse, FileReadWindowParams,
    FileReadWindowResponse, FileSearchParams, FileWatchListResponse, FileWatchStartParams,
    FileWatchStartResponse, FileWatchStopParams, FileWatchStopResponse, FileWriteParams,
    FileWriteResponse, GovernorModeWire, IpcContextFrame, IpcError, IpcErrorCode, IpcRequest,
    IpcResponse, LimitsApplied, ListLimitParams, OutcomeTrust, PolicyStatusResponse,
    ProbeListResponse, ProbeStatusParams, ProbeStatusResponse, PtyCommandListResponse,
    PtyCommandStartParams, PtyCommandStartResponse, PtyCommandStopParams, PtyCommandStopResponse,
    PtyCommandWriteStdinParams, RecipeActivateParams, RecipeActivateResponse,
    RecipeDeactivateParams, RecipeDeactivateResponse, RecipeGetParams, RecipeGetResponse,
    RecipeListActiveResponse, RecipeRunParams, RecipeRunResponse, RecipeSearchParams,
    RecipeSearchResponse, RecipeTestParams, RecipeUpsertParams, RecipeUpsertResponse,
    RegistryActivateParams, RegistryActivateResponse, RegistryDeactivateBulkParams,
    RegistryDeactivateBulkResponse, RegistryDeactivateParams, RegistryDeactivateResponse,
    RegistryGetParams, RegistryGetResponse, RegistryImportPackParams, RegistryImportPackResponse,
    RegistryListActiveResponse, RegistrySearchParams, RegistrySearchResponse,
    RegistrySuggestFromSamplesParams, RegistrySuggestFromSamplesResponse, RegistryTestParams,
    RegistryTestResponse, RegistryTestSample, RegistryUpsertParams, RegistryUpsertResponse,
    SelfCheckResponse, ShellSessionExecParams, ShellSessionExecResponse, ShellSessionListResponse,
    ShellSessionStartParams, ShellSessionStartResponse, ShellSessionStatusParams,
    ShellSessionStatusResponse, ShellSessionStopParams, ShellSessionStopResponse,
    SubscriptionCloseParams, SubscriptionCloseResponse, SubscriptionListParams,
    SubscriptionListResponse, SubscriptionOpenParams, SubscriptionOpenResponse,
    SubscriptionPullParams, SubscriptionPullResponse, WorkspaceSnapshotApplyParams,
    WorkspaceSnapshotApplyResponse, WorkspaceSnapshotCreateParams, WorkspaceSnapshotCreateResponse,
};

use super::catalog::TcCodeOp;
use super::params::{
    AuditSinceInput, BucketEventsSinceInput, BucketSummaryInput, BucketWaitInput,
    CommandJobRefInput, CommandOutputTailInput, CommandStartInput, DiscoverDetailInput,
    EmptyParams, EventContextInput, FileReadWindowInput, FileSearchInput, FileWatchStartInput,
    FileWatchStopInput, FileWriteInput, JobLimitsInput, ListLimitInput, ProbeStatusInput,
    PtyCommandStartInput, PtyCommandWriteStdinInput, PtyJobRefInput, RecipeActivateInput,
    RecipeDeactivateInput, RecipeGetInput, RecipeRunInput, RecipeSearchInput, RecipeTestInput,
    RecipeUpsertInput, RegistryActivateInput, RegistryDeactivateInput, RegistryGetInput,
    RegistryImportPackInput, RegistrySearchInput, RegistrySuggestInput, RegistryTestInput,
    RegistryUpsertInput, RunAndWatchInput, ShellExecInput, ShellSessionExecInput,
    ShellSessionRefInput, ShellSessionStartInput, SubscriptionCloseInput, SubscriptionOpenInput,
    SubscriptionPullInput, SubscriptionSeekInput, SystemDiscoverInput, TargetProbeInput,
    WorkspaceSnapshotApplyInput, WorkspaceSnapshotCreateInput, decode_op_kwargs,
    normalize_keyword_pattern, parse_bucket_and_rules, parse_id, parse_source_stream,
};

/// Boxed future alias for object-safe async transport traits.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Error returned by a [`TcCodeBackend`] when an operation is refused or invalid.
///
/// When `kind == "type_error"`, [`crate::runner::execute_script`] raises a Python
/// `TypeError` inside Monty; all other kinds raise `RuntimeError("<kind>: <message>")`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeBackendError {
    pub kind: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl CodeBackendError {
    #[must_use]
    pub fn invalid_args(message: impl Into<String>) -> Self {
        Self {
            kind: "invalid_request".to_owned(),
            message: message.into(),
            data: None,
        }
    }

    #[must_use]
    pub fn type_error(message: impl Into<String>) -> Self {
        Self {
            kind: "type_error".to_owned(),
            message: message.into(),
            data: None,
        }
    }

    #[must_use]
    pub fn refusal(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            data: None,
        }
    }

    #[must_use]
    pub fn from_ipc_error(err: &IpcError) -> Self {
        let kind = if err.is_transport() {
            "daemon_unavailable".to_owned()
        } else {
            format!("{:?}", err.code)
        };
        let mut data_map = Map::new();
        data_map.insert(
            "ipc_code".to_owned(),
            Value::String(format!("{:?}", err.code)),
        );
        if let Some(argv0) = &err.argv0 {
            data_map.insert("argv0".to_owned(), Value::String(argv0.clone()));
        }
        if let Some(tool) = &err.tool {
            data_map.insert("tool".to_owned(), Value::String(tool.clone()));
        }
        if let Some(teach) = &err.teach {
            data_map.insert(
                "teach".to_owned(),
                serde_json::to_value(teach).unwrap_or(Value::Null),
            );
        }
        if let Some(details) = &err.details {
            data_map.insert(
                "details".to_owned(),
                serde_json::to_value(details).unwrap_or(Value::Null),
            );
        }
        Self {
            kind,
            message: err.message.clone(),
            data: Some(Value::Object(data_map)),
        }
    }
}

impl std::fmt::Display for CodeBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for CodeBackendError {}

/// Synchronous host backend invoked by `MontyRepl` on each sandbox `FunctionCall`.
pub trait TcCodeBackend: Send + Sync {
    fn execute_op(
        &self,
        op: TcCodeOp,
        kwargs: Map<String, Value>,
        remaining_wall: Duration,
    ) -> Result<Value, CodeBackendError>;
}

impl<F> TcCodeBackend for F
where
    F: Fn(TcCodeOp, Map<String, Value>, Duration) -> Result<Value, CodeBackendError> + Send + Sync,
{
    fn execute_op(
        &self,
        op: TcCodeOp,
        kwargs: Map<String, Value>,
        remaining_wall: Duration,
    ) -> Result<Value, CodeBackendError> {
        self(op, kwargs, remaining_wall)
    }
}

/// Async IPC caller abstraction implemented by `DaemonClientCaller`, MCP `TargetRouter`
/// adapters, and in-process `EmbeddedEngine` callers.
pub trait AsyncIpcCaller: Send + Sync {
    /// Execute one typed `IpcRequest` against the local daemon/engine (`target_id = None`)
    /// or a registered federation target (`target_id = Some(...)`).
    fn call_ipc<'a>(
        &'a self,
        request: IpcRequest,
        target_id: Option<&'a str>,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<IpcResponse, CodeBackendError>>;

    /// Return the `target_list` payload (`{"targets": [...]}`).
    /// Defaults to empty local-only targets for standalone `DaemonClient` / `EmbeddedEngine`.
    fn list_targets(&self) -> BoxFuture<'_, Result<Value, CodeBackendError>> {
        Box::pin(async { Ok(json!({ "targets": [] })) })
    }

    /// Probe one registered remote target (`target_probe`).
    fn probe_target<'a>(
        &'a self,
        target_id: &'a str,
    ) -> BoxFuture<'a, Result<Value, CodeBackendError>> {
        Box::pin(async move {
            Err(CodeBackendError::refusal(
                "unknown_target",
                format!("unknown target_id '{target_id}'"),
            ))
        })
    }
}

/// Alias matching cross-slice contract naming.
pub use AsyncIpcCaller as AsyncIpcTransport;

/// Standalone [`AsyncIpcCaller`] backed directly by [`terminal_commander_ipc::DaemonClient`].
#[derive(Debug)]
pub struct DaemonClientCaller {
    client: DaemonClient,
    default_timeout: Duration,
    next_correlation_id: AtomicU64,
}

impl DaemonClientCaller {
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        let default_timeout = Duration::from_secs(5);
        Self {
            client: DaemonClient::new(socket_path).with_timeout(default_timeout),
            default_timeout,
            next_correlation_id: AtomicU64::new(1),
        }
    }

    #[must_use]
    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self.client = self.client.with_timeout(timeout);
        self
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        self.client.socket_path()
    }
}

impl AsyncIpcCaller for DaemonClientCaller {
    fn call_ipc<'a>(
        &'a self,
        request: IpcRequest,
        target_id: Option<&'a str>,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<IpcResponse, CodeBackendError>> {
        Box::pin(async move {
            if let Some(id) = target_id {
                return Err(CodeBackendError::refusal(
                    "unknown_target",
                    format!("unknown target_id '{id}' (standalone client is local-only)"),
                ));
            }
            let corr = self.next_correlation_id.fetch_add(1, Ordering::Relaxed);
            let effective_timeout = timeout.max(self.default_timeout);
            self.client
                .call_with_timeout(corr, request, effective_timeout)
                .await
                .map_err(|e| CodeBackendError::from_ipc_error(&e))
        })
    }
}

/// Synchronous [`TcCodeBackend`] bridging `MontyRepl` calls onto an async [`AsyncIpcCaller`]
/// via `tokio::runtime::Handle::block_on`.
///
/// Must be invoked from a non-async thread (such as [`crate::runner::execute_script_async`]
/// which wraps execution in `tokio::task::spawn_blocking`).
#[derive(Debug, Clone)]
pub struct AsyncBridgeBackend<C> {
    handle: tokio::runtime::Handle,
    caller: C,
}

impl<C: AsyncIpcCaller> AsyncBridgeBackend<C> {
    #[must_use]
    pub const fn new(handle: tokio::runtime::Handle, caller: C) -> Self {
        Self { handle, caller }
    }

    #[must_use]
    pub const fn caller(&self) -> &C {
        &self.caller
    }
}

impl<C: AsyncIpcCaller> TcCodeBackend for AsyncBridgeBackend<C> {
    fn execute_op(
        &self,
        op: TcCodeOp,
        kwargs: Map<String, Value>,
        remaining_wall: Duration,
    ) -> Result<Value, CodeBackendError> {
        self.handle
            .block_on(dispatch_op_async(&self.caller, op, kwargs, remaining_wall))
    }
}

// -------------------------------------------------------------------------
// Transport-Neutral 60-Op Async Dispatcher
// -------------------------------------------------------------------------

const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(5);
const SYSTEM_DISCOVER_RPC_TIMEOUT: Duration = Duration::from_secs(9);
const RPC_HEADROOM: Duration = Duration::from_secs(4);
const RUN_AND_WATCH_POLL_HINT_MS: u64 = 1_000;
const PASSWORD_PROMPT_HINT: &str = "may prompt for a password -- use pty_command_start so the owner can be asked via credential_request";

const RUN_AND_WATCH_WAIT_EXHAUSTED_HINT: &str = "Wait budget exhausted; command is still running. Continue signal collection with bucket_wait using bucket_id, cursor, and timeout_ms=poll_hint_ms; carry forward next_cursor. Check state/exit_code with command_status using job_id. Do not re-run.";

const RUN_AND_WATCH_RECOVER_HINT: &str = "IPC error interrupted the wait, but the job is still tracked. First confirm daemon liveness with health. Then: Continue signal collection with bucket_wait using bucket_id, cursor, and timeout_ms=poll_hint_ms; carry forward next_cursor. Check state/exit_code with command_status using job_id. Do not re-run.";

fn degraded_wait_hint(e: &IpcError) -> String {
    format!(
        "{RUN_AND_WATCH_RECOVER_HINT} (underlying: {:?}: {})",
        e.code, e.message
    )
}

fn unexpected_variant() -> CodeBackendError {
    CodeBackendError::refusal(
        "Internal",
        "daemon returned a response that did not match the request method",
    )
}

fn backend_err_to_ipc_err(err: &CodeBackendError) -> IpcError {
    IpcError::new(IpcErrorCode::Internal, err.to_string())
}

fn duration_ms_u64(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Dispatches one `TcCodeOp` + `kwargs` over an [`AsyncIpcCaller`].
///
/// Enforces strict `serde_ignored` keyword checking, clamps blocking wait
/// budgets to `remaining_wall`, and projects responses to exact TC JSON shapes.
#[allow(clippy::too_many_lines)]
pub async fn dispatch_op_async<C: AsyncIpcCaller + ?Sized>(
    caller: &C,
    op: TcCodeOp,
    kwargs: Map<String, Value>,
    remaining_wall: Duration,
) -> Result<Value, CodeBackendError> {
    let remaining_wall_ms = duration_ms_u64(remaining_wall);

    match op {
        // 1. system_discover
        TcCodeOp::SystemDiscover => {
            let input: SystemDiscoverInput = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(
                    IpcRequest::SystemDiscover,
                    None,
                    SYSTEM_DISCOVER_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::SystemDiscover(d) => Ok(project_system_discover(&d, input.detail)),
                _ => Err(unexpected_variant()),
            }
        }

        // 2. health
        TcCodeOp::Health => {
            let _: EmptyParams = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(IpcRequest::Health, None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::Health {
                    uptime_secs,
                    version,
                    identity,
                    ..
                } => Ok(json!({
                    "ok": true,
                    "uptime_secs": uptime_secs,
                    "version": version,
                    "identity": identity,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 3. policy_status
        TcCodeOp::PolicyStatus => {
            let _: EmptyParams = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(IpcRequest::PolicyStatus, None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::PolicyStatus(PolicyStatusResponse {
                    profile,
                    commands_deny_count,
                    default_deny_path_suffix_count,
                    file_window_bytes,
                    bucket_read_limit,
                    caps,
                    llm_can_activate_recipes,
                    bucket_wait_max_ms,
                    run_and_watch_max_wait_ms,
                    config_warnings,
                    governor,
                }) => Ok(json!({
                    "profile": profile,
                    "commands_deny_count": commands_deny_count,
                    "default_deny_path_suffix_count": default_deny_path_suffix_count,
                    "file_window_bytes": file_window_bytes,
                    "bucket_read_limit": bucket_read_limit,
                    "llm_can_activate_recipes": llm_can_activate_recipes,
                    "bucket_wait_max_ms": bucket_wait_max_ms.unwrap_or(30_000),
                    "run_and_watch_max_wait_ms": run_and_watch_max_wait_ms.unwrap_or(60_000),
                    "caps": {
                        "allow_shell": caps.allow_shell,
                        "allow_session": caps.allow_session,
                        "allow_remote": caps.allow_remote,
                    },
                    "config_warnings": config_warnings,
                    "governor": governor,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 4. self_check
        TcCodeOp::SelfCheck => {
            let _: EmptyParams = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(IpcRequest::SelfCheck, None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::SelfCheck(SelfCheckResponse { report, failures }) => Ok(json!({
                    "failures": failures,
                    "report": report,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 5. audit_since
        TcCodeOp::AuditSince => {
            let input: AuditSinceInput = decode_op_kwargs(op, kwargs)?;
            let ipc = AuditSinceParams {
                cursor: input.cursor.unwrap_or(0),
                action_filter: input.action_filter,
                decision_filter: input.decision_filter,
                limit: input.limit,
            };
            match caller
                .call_ipc(IpcRequest::AuditSince(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::AuditSince(resp) => serde_json::to_value(resp)
                    .map_err(|e| CodeBackendError::refusal("Internal", e.to_string())),
                _ => Err(unexpected_variant()),
            }
        }

        // 6. runtime_state
        TcCodeOp::RuntimeState => {
            let input: ListLimitInput = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(
                    IpcRequest::RuntimeState(ListLimitParams { limit: input.limit }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::RuntimeState(r) => serde_json::to_value(r)
                    .map_err(|e| CodeBackendError::refusal("Internal", e.to_string())),
                _ => Err(unexpected_variant()),
            }
        }

        // 7. probe_list
        TcCodeOp::ProbeList => {
            let input: ListLimitInput = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(
                    IpcRequest::ProbeList(ListLimitParams { limit: input.limit }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::ProbeList(ProbeListResponse { probes, truncated }) => Ok(json!({
                    "probes": probes,
                    "truncated": truncated,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 8. probe_status
        TcCodeOp::ProbeStatus => {
            let input: ProbeStatusInput = decode_op_kwargs(op, kwargs)?;
            let probe_id = parse_id::<ProbeIdKind>("probe_id", &input.probe_id)
                .map_err(CodeBackendError::invalid_args)?;
            match caller
                .call_ipc(
                    IpcRequest::ProbeStatus(ProbeStatusParams { probe_id }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::ProbeStatus(ProbeStatusResponse { probe }) => {
                    Ok(json!({ "probe": probe }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 9. target_list
        TcCodeOp::TargetList => {
            let _: EmptyParams = decode_op_kwargs(op, kwargs)?;
            caller.list_targets().await
        }

        // 10. target_probe
        TcCodeOp::TargetProbe => {
            let input: TargetProbeInput = decode_op_kwargs(op, kwargs)?;
            caller.probe_target(&input.target_id).await
        }

        // 11. command_start_combed
        TcCodeOp::CommandStartCombed => {
            let input: CommandStartInput = decode_op_kwargs(op, kwargs)?;
            let (ipc, target_id) = input.into_ipc()?;
            let credential_hint = password_prompt_hint(&ipc.argv);
            match caller
                .call_ipc(
                    IpcRequest::CommandStartCombed(ipc),
                    target_id.as_deref(),
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::CommandStartCombed(CommandStartResponse {
                    job_id,
                    bucket_id,
                    probe_id,
                    cursor,
                    hint,
                    wslenv_dropped,
                    limits_applied,
                    limits_clamped,
                    governor,
                }) => {
                    let mut body = json!({
                        "job_id": job_id,
                        "bucket_id": bucket_id,
                        "probe_id": probe_id,
                        "cursor": cursor,
                    });
                    if let Some(h) = hint {
                        body["hint"] = serde_json::to_value(h).unwrap_or(Value::Null);
                    }
                    if let Some(h) = credential_hint {
                        body["credential_hint"] = json!(h);
                    }
                    add_wslenv_dropped(&mut body, &wslenv_dropped);
                    add_limits(
                        &mut body,
                        limits_applied.as_ref(),
                        &limits_clamped,
                        governor.as_ref(),
                    );
                    Ok(body)
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 12. run_and_watch
        TcCodeOp::RunAndWatch => {
            let input: RunAndWatchInput = decode_op_kwargs(op, kwargs)?;
            let requested_wait = input
                .wait_ms
                .unwrap_or(RUN_AND_WATCH_DEFAULT_WAIT_MS)
                .min(remaining_wall_ms);
            let max_signals = input
                .max_signals
                .unwrap_or(RUN_AND_WATCH_DEFAULT_MAX_SIGNALS)
                .min(RUN_AND_WATCH_MAX_SIGNALS);
            let wait_until_exit = input
                .wait_until
                .as_deref()
                .is_some_and(|w| w.eq_ignore_ascii_case("exit"));
            let compact = input.compact;
            let start_input = CommandStartInput {
                argv: input.argv,
                cwd: input.cwd,
                env: input.env,
                grace_ms: input.grace_ms,
                bucket_config_json: input.bucket_config_json,
                rules: input.rules,
                rules_json: input.rules_json,
                tag: input.tag,
                strip_ansi: input.strip_ansi,
                receipt_head_lines: input.receipt_head_lines,
                receipt_tail_lines: input.receipt_tail_lines,
                target_id: input.target_id,
                limits: input.limits,
            };
            let (start_ipc, target_id) = start_input.into_ipc()?;
            let credential_hint = password_prompt_hint(&start_ipc.argv);
            let target_ref = target_id.as_deref();

            let ceiling_ms = match caller
                .call_ipc(IpcRequest::PolicyStatus, target_ref, DEFAULT_RPC_TIMEOUT)
                .await
            {
                Ok(IpcResponse::PolicyStatus(p)) => p
                    .run_and_watch_max_wait_ms
                    .unwrap_or(RUN_AND_WATCH_MAX_WAIT_MS),
                _ => RUN_AND_WATCH_MAX_WAIT_MS,
            };

            // Capture start failure directly as CodeBackendError so pre-start refusals
            // preserve full structured IPC error details (`PolicyDenied`, `OsCriticalPathProtected`, etc.).
            let IpcResponse::CommandStartCombed(start_resp) = caller
                .call_ipc(
                    IpcRequest::CommandStartCombed(start_ipc),
                    target_ref,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            else {
                return Err(unexpected_variant());
            };
            // Drive compound::run_and_watch by feeding the already-known start on the first call.
            let mut pre_started = Some(start_resp);
            let dummy_start = terminal_commander_ipc::CommandStartParams {
                environment: None,
                argv: Vec::new(),
                cwd: None,
                env: Vec::new(),
                bucket_config: None,
                rules: Vec::new(),
                grace_ms: None,
                tag: None,
                strip_ansi: true,
                dedup_nonce: None,
                receipt_shape: None,
                limits: None,
            };
            let outcome = run_and_watch_with_ceiling(
                dummy_start,
                RunAndWatchOptions {
                    wait_ms: requested_wait,
                    max_signals,
                    wait_until_exit,
                },
                ceiling_ms,
                |req| {
                    let first = pre_started.take();
                    async move {
                        if let Some(started) = first {
                            return Ok(IpcResponse::CommandStartCombed(started));
                        }
                        let rpc_timeout = match &req {
                            IpcRequest::BucketWait(w) => {
                                Duration::from_millis(w.timeout_ms.unwrap_or(MAX_WAIT_SLICE_MS))
                                    + RPC_HEADROOM
                            }
                            _ => DEFAULT_RPC_TIMEOUT,
                        };
                        caller
                            .call_ipc(req, target_ref, rpc_timeout)
                            .await
                            .map_err(|e| backend_err_to_ipc_err(&e))
                    }
                },
            )
            .await
            .map_err(|e| CodeBackendError::from_ipc_error(&e))?;

            let mut body = run_and_watch_outcome_value(&outcome, compact);
            body["wait_cap_ms"] = json!(ceiling_ms);
            if !outcome.degraded
                && let Some(hint) = credential_hint
            {
                body["credential_hint"] = json!(hint);
            }
            Ok(body)
        }

        // 13. command_status
        TcCodeOp::CommandStatus => {
            let input: CommandJobRefInput = decode_op_kwargs(op, kwargs)?;
            let job_id = parse_id::<JobIdKind>("job_id", &input.job_id)
                .map_err(CodeBackendError::invalid_args)?;
            match caller
                .call_ipc(
                    IpcRequest::CommandStatus(CommandStatusParams { job_id }),
                    input.target_id.as_deref(),
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::CommandStatus(s) => Ok(command_status_payload(&s)),
                _ => Err(unexpected_variant()),
            }
        }

        // 14. command_stop
        TcCodeOp::CommandStop => {
            let input: CommandJobRefInput = decode_op_kwargs(op, kwargs)?;
            let job_id = parse_id::<JobIdKind>("job_id", &input.job_id)
                .map_err(CodeBackendError::invalid_args)?;
            match caller
                .call_ipc(
                    IpcRequest::CommandStop(CommandStopParams { job_id }),
                    input.target_id.as_deref(),
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::CommandStop(CommandStopResponse {
                    job_id,
                    bucket_id,
                    frames_total,
                    events_emitted,
                    bytes_total,
                }) => Ok(json!({
                    "job_id": job_id,
                    "bucket_id": bucket_id,
                    "frames_total": frames_total,
                    "events_emitted": events_emitted,
                    "bytes_total": bytes_total,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 15. shell_exec
        TcCodeOp::ShellExec => {
            let input: ShellExecInput = decode_op_kwargs(op, kwargs)?;
            let (ipc, shell_line) = input.into_ipc()?;
            match caller
                .call_ipc(IpcRequest::ShellExec(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::CommandStartCombed(resp) => Ok(shell_exec_payload(&resp, &shell_line)),
                _ => Err(unexpected_variant()),
            }
        }

        // 16. command_output_tail
        TcCodeOp::CommandOutputTail => {
            let input: CommandOutputTailInput = decode_op_kwargs(op, kwargs)?;
            let job_id = parse_id::<JobIdKind>("job_id", &input.job_id)
                .map_err(CodeBackendError::invalid_args)?;
            let ipc = CommandOutputTailParams {
                job_id,
                max_lines: input.max_lines.unwrap_or(50),
                max_bytes: input.max_bytes.unwrap_or(65_536),
                strip_ansi: input.strip_ansi,
            };
            match caller
                .call_ipc(
                    IpcRequest::CommandOutputTail(ipc),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::CommandOutputTail(CommandOutputTailResponse {
                    job_id,
                    lines,
                    returned_lines,
                    truncated_lines,
                    truncated_bytes,
                    evicted_frames,
                }) => Ok(json!({
                    "job_id": job_id,
                    "lines": lines,
                    "returned_lines": returned_lines,
                    "truncated_lines": truncated_lines,
                    "truncated_bytes": truncated_bytes,
                    "evicted_frames": evicted_frames,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 17. bucket_events_since
        TcCodeOp::BucketEventsSince => {
            let input: BucketEventsSinceInput = decode_op_kwargs(op, kwargs)?;
            let (ipc, compact) = input.into_ipc()?;
            match caller
                .call_ipc(
                    IpcRequest::BucketEventsSince(ipc),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::BucketEventsSince(r) => Ok(bucket_events_payload(&r, compact)),
                _ => Err(unexpected_variant()),
            }
        }

        // 18. bucket_wait (clamped to remaining_wall)
        TcCodeOp::BucketWait => {
            let input: BucketWaitInput = decode_op_kwargs(op, kwargs)?;
            let (ipc, compact) = input.into_ipc(remaining_wall_ms)?;
            let rpc_timeout =
                Duration::from_millis(ipc.timeout_ms.unwrap_or(0)).saturating_add(RPC_HEADROOM);
            match caller
                .call_ipc(IpcRequest::BucketWait(ipc), None, rpc_timeout)
                .await?
            {
                IpcResponse::BucketWait(r) => Ok(bucket_wait_payload(&r, compact)),
                _ => Err(unexpected_variant()),
            }
        }

        // 19. bucket_summary
        TcCodeOp::BucketSummary => {
            let input: BucketSummaryInput = decode_op_kwargs(op, kwargs)?;
            let bucket_id = parse_id::<BucketIdKind>("bucket_id", &input.bucket_id)
                .map_err(CodeBackendError::invalid_args)?;
            match caller
                .call_ipc(
                    IpcRequest::BucketSummary(BucketSummaryParams { bucket_id }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::BucketSummary(BucketSummaryResponse {
                    bucket_id,
                    head_seq,
                    tail_seq,
                    event_count,
                    dropped_count,
                    by_severity,
                }) => Ok(json!({
                    "bucket_id": bucket_id,
                    "head_seq": head_seq,
                    "tail_seq": tail_seq,
                    "event_count": event_count,
                    "dropped_count": dropped_count,
                    "by_severity": by_severity,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 20. event_context
        TcCodeOp::EventContext => {
            let input: EventContextInput = decode_op_kwargs(op, kwargs)?;
            let ipc = input.into_ipc()?;
            match caller
                .call_ipc(IpcRequest::EventContext(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::EventContext(r) => Ok(event_context_payload(&r)),
                _ => Err(unexpected_variant()),
            }
        }

        // 21. registry_search
        TcCodeOp::RegistrySearch => {
            let input: RegistrySearchInput = decode_op_kwargs(op, kwargs)?;
            let ipc = RegistrySearchParams {
                query: input.query,
                limit: input.limit,
            };
            match caller
                .call_ipc(IpcRequest::RegistrySearch(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RegistrySearch(RegistrySearchResponse { hits }) => {
                    Ok(json!({ "hits": hits }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 22. registry_get
        TcCodeOp::RegistryGet => {
            let input: RegistryGetInput = decode_op_kwargs(op, kwargs)?;
            let ipc = RegistryGetParams {
                rule_id: input.rule_id,
                version: input.version,
            };
            match caller
                .call_ipc(IpcRequest::RegistryGet(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RegistryGet(RegistryGetResponse { definition }) => {
                    Ok(json!({ "definition": definition }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 23. registry_upsert
        TcCodeOp::RegistryUpsert => {
            let input: RegistryUpsertInput = decode_op_kwargs(op, kwargs)?;
            let mut definition: RuleDefinition = serde_json::from_str(&input.definition_json)
                .map_err(|e| CodeBackendError::invalid_args(format!("definition_json: {e}")))?;
            normalize_keyword_pattern(&mut definition);
            let ipc = RegistryUpsertParams { definition };
            match caller
                .call_ipc(IpcRequest::RegistryUpsert(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RegistryUpsert(RegistryUpsertResponse { rule_id, version }) => {
                    Ok(json!({
                        "rule_id": rule_id,
                        "version": version,
                    }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 24. registry_test
        TcCodeOp::RegistryTest => {
            let input: RegistryTestInput = decode_op_kwargs(op, kwargs)?;
            let mut samples = Vec::with_capacity(input.samples.len());
            for s in input.samples {
                let stream = s
                    .stream
                    .as_deref()
                    .map(parse_source_stream)
                    .transpose()
                    .map_err(CodeBackendError::invalid_args)?;
                samples.push(RegistryTestSample {
                    text: s.text,
                    stream,
                });
            }
            let ipc = RegistryTestParams {
                rule_id: input.rule_id,
                version: input.version,
                samples,
            };
            match caller
                .call_ipc(IpcRequest::RegistryTest(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RegistryTest(RegistryTestResponse {
                    matches,
                    truncated_bytes,
                    stream_mismatches,
                    examples_evaluated,
                    example_results,
                }) => Ok(json!({
                    "matches": matches,
                    "truncated_bytes": truncated_bytes,
                    "examples_evaluated": examples_evaluated,
                    "example_results": example_results,
                    "stream_mismatches": stream_mismatches,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 25. registry_activate
        TcCodeOp::RegistryActivate => {
            let input: RegistryActivateInput = decode_op_kwargs(op, kwargs)?;
            let scope = Some(input.scope.into_ipc_scope()?);
            let ipc = RegistryActivateParams {
                rule_id: input.rule_id,
                version: input.version,
                scope,
            };
            match caller
                .call_ipc(IpcRequest::RegistryActivate(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RegistryActivate(RegistryActivateResponse {
                    rule_id,
                    version,
                    was_already_active,
                    scope,
                    jobs_rebound,
                    superseded_versions,
                }) => Ok(json!({
                    "rule_id": rule_id,
                    "version": version,
                    "was_already_active": was_already_active,
                    "scope": scope,
                    "jobs_rebound": jobs_rebound,
                    "superseded_versions": superseded_versions,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 26. registry_import_pack
        TcCodeOp::RegistryImportPack => {
            let input: RegistryImportPackInput = decode_op_kwargs(op, kwargs)?;
            let scope = input
                .scope
                .map(super::params::ActivationScopeInput::into_ipc_scope)
                .transpose()?;
            let ipc = RegistryImportPackParams {
                pack: input.pack,
                activate: input.activate,
                scope,
            };
            match caller
                .call_ipc(
                    IpcRequest::RegistryImportPack(ipc),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::RegistryImportPack(RegistryImportPackResponse {
                    pack,
                    imported,
                    skipped,
                    activated,
                    failed,
                }) => {
                    let failed_json: Vec<Value> = failed
                        .into_iter()
                        .map(|f| json!({ "rule_id": f.rule_id, "reason": f.reason }))
                        .collect();
                    Ok(json!({
                        "pack": pack,
                        "imported": imported,
                        "skipped": skipped,
                        "activated": activated,
                        "failed": failed_json,
                    }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 27. registry_deactivate (single + bulk)
        TcCodeOp::RegistryDeactivate => {
            let input: RegistryDeactivateInput = decode_op_kwargs(op, kwargs)?;
            let scope = input.scope.into_ipc_scope()?;
            let selector_count = usize::from(input.rule_id.is_some())
                + usize::from(input.rule_ids.is_some())
                + usize::from(input.pack.is_some());
            if selector_count != 1 {
                return Err(CodeBackendError::invalid_args(
                    "registry deactivate requires EXACTLY ONE of 'rule_id' (single rule), \
                     'rule_ids' (list of rule ids), or 'pack' (whole seed pack); you supplied \
                     none or more than one",
                ));
            }
            if input.version.is_some() && input.rule_id.is_none() {
                return Err(CodeBackendError::invalid_args(
                    "'version' is only valid with 'rule_id'; a bulk deactivate by 'rule_ids' or \
                     'pack' closes each rule's active version(s) under the scope",
                ));
            }
            if let Some(rule_id) = input.rule_id {
                let version = match input.version {
                    Some(v) => v,
                    None => resolve_latest_rule_version(caller, &rule_id).await?,
                };
                let ipc = RegistryDeactivateParams {
                    rule_id,
                    version,
                    scope: Some(scope),
                };
                match caller
                    .call_ipc(
                        IpcRequest::RegistryDeactivate(ipc),
                        None,
                        DEFAULT_RPC_TIMEOUT,
                    )
                    .await?
                {
                    IpcResponse::RegistryDeactivate(RegistryDeactivateResponse {
                        rule_id,
                        version,
                        was_deactivated,
                        scope,
                        jobs_rebound,
                    }) => Ok(json!({
                        "rule_id": rule_id,
                        "version": version,
                        "was_deactivated": was_deactivated,
                        "scope": scope,
                        "jobs_rebound": jobs_rebound,
                    })),
                    _ => Err(unexpected_variant()),
                }
            } else {
                let ipc = RegistryDeactivateBulkParams {
                    pack: input.pack,
                    rule_ids: input.rule_ids,
                    scope,
                };
                match caller
                    .call_ipc(
                        IpcRequest::RegistryDeactivateBulk(ipc),
                        None,
                        DEFAULT_RPC_TIMEOUT,
                    )
                    .await?
                {
                    IpcResponse::RegistryDeactivateBulk(RegistryDeactivateBulkResponse {
                        outcomes,
                        jobs_rebound,
                    }) => Ok(json!({
                        "outcomes": outcomes,
                        "jobs_rebound": jobs_rebound,
                    })),
                    _ => Err(unexpected_variant()),
                }
            }
        }

        // 28. registry_list_active
        TcCodeOp::RegistryListActive => {
            let input: ListLimitInput = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(
                    IpcRequest::RegistryListActive(ListLimitParams { limit: input.limit }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::RegistryListActive(RegistryListActiveResponse {
                    entries,
                    truncated,
                }) => Ok(json!({
                    "entries": entries,
                    "truncated": truncated,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 29. registry_suggest_from_samples
        TcCodeOp::RegistrySuggestFromSamples => {
            let input: RegistrySuggestInput = decode_op_kwargs(op, kwargs)?;
            let ipc = RegistrySuggestFromSamplesParams {
                samples: input.sample_lines,
                intent: input.intent,
                max_rules: input.max_rules,
            };
            match caller
                .call_ipc(
                    IpcRequest::RegistrySuggestFromSamples(ipc),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::RegistrySuggestFromSamples(RegistrySuggestFromSamplesResponse {
                    proposed_rules,
                    confidence,
                    next_steps,
                    explanation,
                }) => Ok(json!({
                    "proposed_rules": proposed_rules,
                    "confidence": confidence,
                    "next_steps": next_steps,
                    "explanation": explanation,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 30. recipe_search
        TcCodeOp::RecipeSearch => {
            let input: RecipeSearchInput = decode_op_kwargs(op, kwargs)?;
            let ipc = RecipeSearchParams {
                query: input.query,
                limit: input.limit,
            };
            match caller
                .call_ipc(IpcRequest::RecipeSearch(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RecipeSearch(RecipeSearchResponse { hits }) => {
                    Ok(json!({ "hits": hits }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 31. recipe_get
        TcCodeOp::RecipeGet => {
            let input: RecipeGetInput = decode_op_kwargs(op, kwargs)?;
            let ipc = RecipeGetParams {
                recipe_id: input.recipe_id,
                version: input.version,
            };
            match caller
                .call_ipc(IpcRequest::RecipeGet(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RecipeGet(RecipeGetResponse {
                    definition,
                    tombstoned,
                }) => Ok(json!({
                    "definition": definition,
                    "tombstoned": tombstoned,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 32. recipe_upsert
        TcCodeOp::RecipeUpsert => {
            let input: RecipeUpsertInput = decode_op_kwargs(op, kwargs)?;
            let definition: RecipeDefinition = serde_json::from_str(&input.definition_json)
                .map_err(|e| CodeBackendError::invalid_args(format!("definition_json: {e}")))?;
            let ipc = RecipeUpsertParams { definition };
            match caller
                .call_ipc(IpcRequest::RecipeUpsert(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RecipeUpsert(RecipeUpsertResponse { recipe_id, version }) => {
                    Ok(json!({
                        "recipe_id": recipe_id,
                        "version": version,
                        "activated": false,
                    }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 33. recipe_test
        TcCodeOp::RecipeTest => {
            let input: RecipeTestInput = decode_op_kwargs(op, kwargs)?;
            if input.definition_json.is_some() == input.recipe_id.is_some() {
                return Err(CodeBackendError::invalid_args(
                    "recipe_test requires exactly one of definition_json or recipe_id",
                ));
            }
            let definition = input
                .definition_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|e| CodeBackendError::invalid_args(format!("definition_json: {e}")))?;
            let ipc = RecipeTestParams {
                definition,
                recipe_id: input.recipe_id,
                version: input.version,
                fills: input.fills.unwrap_or_default(),
                expect_argv0: input.expect_argv0,
            };
            match caller
                .call_ipc(IpcRequest::RecipeTest(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RecipeTest(body) => serde_json::to_value(body)
                    .map_err(|e| CodeBackendError::refusal("Internal", e.to_string())),
                _ => Err(unexpected_variant()),
            }
        }

        // 34. recipe_activate
        TcCodeOp::RecipeActivate => {
            let input: RecipeActivateInput = decode_op_kwargs(op, kwargs)?;
            let ipc = RecipeActivateParams {
                recipe_id: input.recipe_id,
                version: input.version,
                scope: Some(input.scope.into_ipc_scope()?),
                from_mcp: true,
            };
            match caller
                .call_ipc(IpcRequest::RecipeActivate(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RecipeActivate(RecipeActivateResponse {
                    recipe_id,
                    version,
                    was_already_active,
                    scope,
                }) => Ok(json!({
                    "recipe_id": recipe_id,
                    "version": version,
                    "was_already_active": was_already_active,
                    "scope": scope,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 35. recipe_deactivate
        TcCodeOp::RecipeDeactivate => {
            let input: RecipeDeactivateInput = decode_op_kwargs(op, kwargs)?;
            let ipc = RecipeDeactivateParams {
                recipe_id: input.recipe_id,
                version: input.version,
                scope: Some(input.scope.into_ipc_scope()?),
                from_mcp: true,
            };
            match caller
                .call_ipc(IpcRequest::RecipeDeactivate(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::RecipeDeactivate(RecipeDeactivateResponse {
                    recipe_id,
                    version,
                    was_deactivated,
                    scope,
                }) => Ok(json!({
                    "recipe_id": recipe_id,
                    "version": version,
                    "was_deactivated": was_deactivated,
                    "scope": scope,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 36. recipe_list_active
        TcCodeOp::RecipeListActive => {
            let input: ListLimitInput = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(
                    IpcRequest::RecipeListActive(ListLimitParams { limit: input.limit }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::RecipeListActive(RecipeListActiveResponse { entries, truncated }) => {
                    Ok(json!({
                        "entries": entries,
                        "truncated": truncated,
                    }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 37. recipe_run (with watched bounded loop when `watched == true`)
        TcCodeOp::RecipeRun => {
            let input: RecipeRunInput = decode_op_kwargs(op, kwargs)?;
            let ipc = RecipeRunParams {
                recipe_id: input.recipe_id,
                version: input.version,
                scope: Some(input.scope.into_ipc_scope()?),
                fills: input.fills.unwrap_or_default(),
                limits: input.limits.map(JobLimitsInput::into_ipc),
            };
            let IpcResponse::RecipeRun(started) = caller
                .call_ipc(IpcRequest::RecipeRun(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            else {
                return Err(unexpected_variant());
            };
            let RecipeRunResponse {
                recipe_id,
                version,
                argv,
                lane,
                watched,
                wait_ms,
                job_id,
                bucket_id,
                probe_id,
                cursor,
                wslenv_dropped,
                limits_applied,
                limits_clamped,
                governor,
            } = started;

            if !watched {
                let mut body = json!({
                    "recipe_id": recipe_id,
                    "version": version,
                    "argv": argv,
                    "lane": lane,
                    "watched": watched,
                    "wait_ms": wait_ms,
                    "job_id": job_id,
                    "bucket_id": bucket_id,
                    "probe_id": probe_id,
                    "cursor": cursor,
                    "state": Value::Null,
                    "exit_code": Value::Null,
                    "complete": false,
                    "signals": Vec::<SignalEvent>::new(),
                    "signal_count": 0,
                    "degraded": false,
                    "recover_hint": Value::Null,
                });
                add_wslenv_dropped(&mut body, &wslenv_dropped);
                add_limits(
                    &mut body,
                    limits_applied.as_ref(),
                    &limits_clamped,
                    governor.as_ref(),
                );
                return Ok(body);
            }

            let clamped_wait_ms = wait_ms
                .min(RUN_AND_WATCH_MAX_WAIT_MS)
                .min(remaining_wall_ms);
            let watch =
                watch_recipe_job(caller, job_id, bucket_id, cursor, clamped_wait_ms).await?;
            let mut value = run_and_watch_result_value(
                job_id,
                bucket_id,
                watch.cursor,
                watch.state,
                watch.outcome_trust,
                watch.exit_code,
                watch.pipeline_exit_masked,
                watch.elapsed_ms,
                watch.last_output_age_ms,
                &watch.signals,
                watch.receipt,
                watch.degraded,
                watch.recover_hint.as_deref(),
                false,
                watch.signals_capped,
            );
            if let Some(obj) = value.as_object_mut() {
                obj.insert("recipe_id".to_owned(), json!(recipe_id));
                obj.insert("version".to_owned(), json!(version));
                obj.insert("argv".to_owned(), json!(argv));
                obj.insert("lane".to_owned(), json!(lane));
                obj.insert("watched".to_owned(), json!(watched));
                obj.insert("wait_ms".to_owned(), json!(wait_ms));
                obj.insert("probe_id".to_owned(), json!(probe_id));
            }
            add_wslenv_dropped(&mut value, &wslenv_dropped);
            add_limits(
                &mut value,
                limits_applied.as_ref(),
                &limits_clamped,
                governor.as_ref(),
            );
            obj_extend(&mut value, watch.governor_fields);
            Ok(value)
        }

        // 38. file_read_window
        TcCodeOp::FileReadWindow => {
            let input: FileReadWindowInput = decode_op_kwargs(op, kwargs)?;
            let ipc = FileReadWindowParams {
                path: PathBuf::from(input.path),
                start_line: input.start_line,
                max_lines: input.max_lines,
                max_bytes: input.max_bytes,
            };
            match caller
                .call_ipc(IpcRequest::FileReadWindow(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::FileReadWindow(FileReadWindowResponse {
                    path,
                    lines,
                    file_bytes,
                    truncated,
                    next_byte_offset,
                }) => Ok(json!({
                    "path": path,
                    "lines": lines,
                    "file_bytes": file_bytes,
                    "truncated": truncated,
                    "next_byte_offset": next_byte_offset,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 39. file_search
        TcCodeOp::FileSearch => {
            let input: FileSearchInput = decode_op_kwargs(op, kwargs)?;
            let ipc = FileSearchParams {
                path: PathBuf::from(input.path),
                query: input.query,
                case_insensitive: input.case_insensitive,
                max_matches: input.max_matches,
                max_snippet_bytes: input.max_snippet_bytes,
            };
            match caller
                .call_ipc(IpcRequest::FileSearch(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::FileSearch(response) => serde_json::to_value(response)
                    .map_err(|e| CodeBackendError::refusal("Internal", e.to_string())),
                _ => Err(unexpected_variant()),
            }
        }

        // 40. file_write
        TcCodeOp::FileWrite => {
            let input: FileWriteInput = decode_op_kwargs(op, kwargs)?;
            let ipc = FileWriteParams {
                path: PathBuf::from(input.path),
                content: input.content,
                create_dirs: input.create_dirs.unwrap_or(false),
                append: input.append.unwrap_or(false),
            };
            match caller
                .call_ipc(IpcRequest::FileWrite(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::FileWrite(FileWriteResponse {
                    path,
                    bytes_written,
                }) => Ok(json!({
                    "path": path,
                    "bytes_written": bytes_written,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 41. file_watch_start
        TcCodeOp::FileWatchStart => {
            let input: FileWatchStartInput = decode_op_kwargs(op, kwargs)?;
            let (bucket_config, rules) =
                parse_bucket_and_rules(input.bucket_config_json, input.rules, input.rules_json)?;
            let ipc = FileWatchStartParams {
                path: PathBuf::from(input.path),
                bucket_config,
                rules,
                follow_from_beginning: input.follow_from_beginning,
                tag: input.tag,
            };
            match caller
                .call_ipc(IpcRequest::FileWatchStart(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::FileWatchStart(FileWatchStartResponse {
                    watch_id,
                    bucket_id,
                    probe_id,
                    cursor,
                }) => Ok(json!({
                    "watch_id": watch_id,
                    "bucket_id": bucket_id,
                    "probe_id": probe_id,
                    "cursor": cursor,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 42. file_watch_stop
        TcCodeOp::FileWatchStop => {
            let input: FileWatchStopInput = decode_op_kwargs(op, kwargs)?;
            let watch_id = parse_id::<JobIdKind>("watch_id", &input.watch_id)
                .map_err(CodeBackendError::invalid_args)?;
            match caller
                .call_ipc(
                    IpcRequest::FileWatchStop(FileWatchStopParams { watch_id }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::FileWatchStop(FileWatchStopResponse {
                    watch_id,
                    bucket_id,
                    frames_total,
                    events_emitted,
                    bytes_total,
                }) => Ok(json!({
                    "watch_id": watch_id,
                    "bucket_id": bucket_id,
                    "frames_total": frames_total,
                    "events_emitted": events_emitted,
                    "bytes_total": bytes_total,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 43. file_watch_list
        TcCodeOp::FileWatchList => {
            let _: EmptyParams = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(IpcRequest::FileWatchList, None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::FileWatchList(FileWatchListResponse { entries }) => {
                    Ok(json!({ "entries": entries }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 44. pty_command_start
        TcCodeOp::PtyCommandStart => {
            let input: PtyCommandStartInput = decode_op_kwargs(op, kwargs)?;
            let env: Vec<(String, String)> =
                input.env.into_iter().map(|e| (e.key, e.value)).collect();
            let (bucket_config, rules) =
                parse_bucket_and_rules(input.bucket_config_json, input.rules, input.rules_json)?;
            let ipc = PtyCommandStartParams {
                environment: None,
                argv: input.argv,
                cwd: input.cwd.map(PathBuf::from),
                env,
                bucket_config,
                rules,
                rows: input.rows,
                cols: input.cols,
                tag: input.tag,
                limits: input.limits.map(JobLimitsInput::into_ipc),
            };
            match caller
                .call_ipc(IpcRequest::PtyCommandStart(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::PtyCommandStart(PtyCommandStartResponse {
                    job_id,
                    bucket_id,
                    probe_id,
                    cursor,
                    wslenv_dropped,
                    limits_applied,
                    limits_clamped,
                    governor,
                }) => {
                    let mut body = json!({
                        "job_id": job_id,
                        "bucket_id": bucket_id,
                        "probe_id": probe_id,
                        "cursor": cursor,
                    });
                    add_wslenv_dropped(&mut body, &wslenv_dropped);
                    add_limits(
                        &mut body,
                        limits_applied.as_ref(),
                        &limits_clamped,
                        governor.as_ref(),
                    );
                    Ok(body)
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 45. pty_command_write_stdin
        TcCodeOp::PtyCommandWriteStdin => {
            let input: PtyCommandWriteStdinInput = decode_op_kwargs(op, kwargs)?;
            let job_id = parse_id::<JobIdKind>("job_id", &input.job_id)
                .map_err(CodeBackendError::invalid_args)?;
            let wait_ms = input.wait_ms.map(|w| w.min(remaining_wall_ms));
            let rpc_timeout = wait_ms.map_or(DEFAULT_RPC_TIMEOUT, |w| {
                Duration::from_millis(w).saturating_add(RPC_HEADROOM)
            });
            let ipc = PtyCommandWriteStdinParams {
                job_id,
                bytes: input.bytes,
                cursor: input.cursor,
                wait_ms,
            };
            match caller
                .call_ipc(IpcRequest::PtyCommandWriteStdin(ipc), None, rpc_timeout)
                .await?
            {
                IpcResponse::PtyCommandWriteStdin(resp) => serde_json::to_value(resp)
                    .map_err(|e| CodeBackendError::refusal("Internal", e.to_string())),
                _ => Err(unexpected_variant()),
            }
        }

        // 46. pty_command_stop
        TcCodeOp::PtyCommandStop => {
            let input: PtyJobRefInput = decode_op_kwargs(op, kwargs)?;
            let job_id = parse_id::<JobIdKind>("job_id", &input.job_id)
                .map_err(CodeBackendError::invalid_args)?;
            match caller
                .call_ipc(
                    IpcRequest::PtyCommandStop(PtyCommandStopParams { job_id }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::PtyCommandStop(PtyCommandStopResponse {
                    job_id,
                    bucket_id,
                    frames_total,
                    events_emitted,
                    bytes_total,
                    stdin_bytes_written,
                    secret_prompts_total,
                }) => Ok(json!({
                    "job_id": job_id,
                    "bucket_id": bucket_id,
                    "frames_total": frames_total,
                    "events_emitted": events_emitted,
                    "bytes_total": bytes_total,
                    "stdin_bytes_written": stdin_bytes_written,
                    "secret_prompts_total": secret_prompts_total,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 47. pty_command_list
        TcCodeOp::PtyCommandList => {
            let _: EmptyParams = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(IpcRequest::PtyCommandList, None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::PtyCommandList(PtyCommandListResponse { entries }) => {
                    Ok(json!({ "entries": entries }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 48. credential_request
        TcCodeOp::CredentialRequest => {
            let input: PtyJobRefInput = decode_op_kwargs(op, kwargs)?;
            let job_id = parse_id::<JobIdKind>("job_id", &input.job_id)
                .map_err(CodeBackendError::invalid_args)?;
            let wait_ms = CREDENTIAL_REQUEST_WAIT_MS.min(remaining_wall_ms);
            let rpc_timeout = Duration::from_millis(wait_ms).saturating_add(RPC_HEADROOM);
            match caller
                .call_ipc(
                    IpcRequest::CredentialRequest(CredentialRequestParams {
                        job_id,
                        wait_ms: Some(wait_ms),
                    }),
                    None,
                    rpc_timeout,
                )
                .await?
            {
                IpcResponse::CredentialRequest(resp) => serde_json::to_value(resp)
                    .map_err(|e| CodeBackendError::refusal("Internal", e.to_string())),
                _ => Err(unexpected_variant()),
            }
        }

        // 49. shell_session_start
        TcCodeOp::ShellSessionStart => {
            let input: ShellSessionStartInput = decode_op_kwargs(op, kwargs)?;
            let env: Vec<(String, String)> =
                input.env.into_iter().map(|e| (e.key, e.value)).collect();
            let (bucket_config, rules) =
                parse_bucket_and_rules(input.bucket_config_json, input.rules, input.rules_json)?;
            let ipc = ShellSessionStartParams {
                shell: input.shell,
                cwd: input.cwd.map(PathBuf::from),
                env,
                rules,
                bucket_config,
                tag: input.tag,
            };
            match caller
                .call_ipc(
                    IpcRequest::ShellSessionStart(ipc),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::ShellSessionStart(ShellSessionStartResponse {
                    session_id,
                    bucket_id,
                    state,
                }) => Ok(json!({
                    "session_id": session_id,
                    "bucket_id": bucket_id,
                    "state": state,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 50. shell_session_exec
        TcCodeOp::ShellSessionExec => {
            let input: ShellSessionExecInput = decode_op_kwargs(op, kwargs)?;
            let session_id = parse_id::<SessionIdKind>("session_id", &input.session_id)
                .map_err(CodeBackendError::invalid_args)?;
            let wait_ms = input.wait_ms.map(|w| w.min(remaining_wall_ms));
            let rpc_timeout = wait_ms.map_or(DEFAULT_RPC_TIMEOUT, |w| {
                Duration::from_millis(w).saturating_add(RPC_HEADROOM)
            });
            let ipc = ShellSessionExecParams {
                session_id,
                line: input.line,
                cursor: input.cursor.unwrap_or(0),
                wait_ms,
            };
            match caller
                .call_ipc(IpcRequest::ShellSessionExec(ipc), None, rpc_timeout)
                .await?
            {
                IpcResponse::ShellSessionExec(ShellSessionExecResponse {
                    session_id,
                    bucket_id,
                    bytes_written,
                    cursor_in,
                    next_cursor,
                    has_more,
                    dropped_count,
                    events,
                }) => Ok(json!({
                    "session_id": session_id,
                    "bucket_id": bucket_id,
                    "bytes_written": bytes_written,
                    "cursor_in": cursor_in,
                    "next_cursor": next_cursor,
                    "has_more": has_more,
                    "dropped_count": dropped_count,
                    "events": events,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 51. shell_session_status
        TcCodeOp::ShellSessionStatus => {
            let input: ShellSessionRefInput = decode_op_kwargs(op, kwargs)?;
            let session_id = parse_id::<SessionIdKind>("session_id", &input.session_id)
                .map_err(CodeBackendError::invalid_args)?;
            match caller
                .call_ipc(
                    IpcRequest::ShellSessionStatus(ShellSessionStatusParams { session_id }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::ShellSessionStatus(ShellSessionStatusResponse {
                    session_id,
                    bucket_id,
                    state,
                    cwd,
                    env_snapshot,
                    last_active_at,
                }) => Ok(json!({
                    "session_id": session_id,
                    "bucket_id": bucket_id,
                    "state": state,
                    "cwd": cwd,
                    "env_snapshot": env_snapshot,
                    "last_active_at": last_active_at,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 52. shell_session_stop
        TcCodeOp::ShellSessionStop => {
            let input: ShellSessionRefInput = decode_op_kwargs(op, kwargs)?;
            let session_id = parse_id::<SessionIdKind>("session_id", &input.session_id)
                .map_err(CodeBackendError::invalid_args)?;
            match caller
                .call_ipc(
                    IpcRequest::ShellSessionStop(ShellSessionStopParams { session_id }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::ShellSessionStop(ShellSessionStopResponse {
                    session_id,
                    state,
                    terminal_reason,
                }) => Ok(json!({
                    "session_id": session_id,
                    "state": state,
                    "terminal_reason": terminal_reason,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 53. shell_session_list
        TcCodeOp::ShellSessionList => {
            let _: EmptyParams = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(IpcRequest::ShellSessionList, None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::ShellSessionList(ShellSessionListResponse { sessions }) => {
                    Ok(json!({ "sessions": sessions }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 54. workspace_snapshot_create
        TcCodeOp::WorkspaceSnapshotCreate => {
            let input: WorkspaceSnapshotCreateInput = decode_op_kwargs(op, kwargs)?;
            let session_id = parse_id::<SessionIdKind>("session_id", &input.session_id)
                .map_err(CodeBackendError::invalid_args)?;
            let ipc = WorkspaceSnapshotCreateParams {
                session_id,
                name: input.name,
            };
            match caller
                .call_ipc(
                    IpcRequest::WorkspaceSnapshotCreate(ipc),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::WorkspaceSnapshotCreate(WorkspaceSnapshotCreateResponse {
                    snapshot_id,
                }) => Ok(json!({ "snapshot_id": snapshot_id })),
                _ => Err(unexpected_variant()),
            }
        }

        // 55. workspace_snapshot_apply
        TcCodeOp::WorkspaceSnapshotApply => {
            let input: WorkspaceSnapshotApplyInput = decode_op_kwargs(op, kwargs)?;
            let session_id = parse_id::<SessionIdKind>("session_id", &input.session_id)
                .map_err(CodeBackendError::invalid_args)?;
            let ipc = WorkspaceSnapshotApplyParams {
                snapshot_id: input.snapshot_id,
                session_id,
            };
            match caller
                .call_ipc(
                    IpcRequest::WorkspaceSnapshotApply(ipc),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::WorkspaceSnapshotApply(WorkspaceSnapshotApplyResponse {
                    applied,
                    session_id,
                    cwd,
                    skipped_redacted,
                }) => {
                    let mut body = json!({
                        "applied": applied,
                        "session_id": session_id,
                        "cwd": cwd,
                    });
                    if !skipped_redacted.is_empty() {
                        body["skipped_redacted"] = json!({
                            "names": skipped_redacted,
                            "note": "Not restored: the snapshot holds only a masked value for these. Set them again in the session if needed.",
                        });
                    }
                    Ok(body)
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 56. subscription_open
        TcCodeOp::SubscriptionOpen => {
            let input: SubscriptionOpenInput = decode_op_kwargs(op, kwargs)?;
            let predicate = input.into_predicate()?;
            match caller
                .call_ipc(
                    IpcRequest::SubscriptionOpen(SubscriptionOpenParams { predicate }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::SubscriptionOpen(SubscriptionOpenResponse {
                    sub_id,
                    boot_id,
                    predicate_hash,
                    created_at_ms,
                    matched_sources,
                }) => Ok(json!({
                    "sub_id": sub_id,
                    "boot_id": boot_id,
                    "predicate_hash": predicate_hash,
                    "created_at_ms": created_at_ms,
                    "matched_sources": matched_sources,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 57. subscription_pull (clamped to remaining_wall)
        TcCodeOp::SubscriptionPull => {
            let input: SubscriptionPullInput = decode_op_kwargs(op, kwargs)?;
            let timeout_ms = input
                .timeout_ms
                .unwrap_or(terminal_commander_ipc::DEFAULT_PULL_TIMEOUT_MS)
                .min(terminal_commander_ipc::MAX_PULL_TIMEOUT_MS)
                .min(remaining_wall_ms);
            let rpc_timeout = Duration::from_millis(timeout_ms).saturating_add(RPC_HEADROOM);
            let ipc = SubscriptionPullParams {
                sub_id: input.sub_id,
                max: input.max,
                timeout_ms: Some(timeout_ms),
                liveness_delta: true,
            };
            match caller
                .call_ipc(IpcRequest::SubscriptionPull(ipc), None, rpc_timeout)
                .await?
            {
                IpcResponse::SubscriptionPull(SubscriptionPullResponse {
                    events,
                    liveness,
                    lagged,
                    truncated,
                }) => {
                    let mut payload = json!({
                        "events": events,
                        "lagged": lagged,
                        "truncated": truncated,
                    });
                    if !liveness.is_empty() {
                        payload["liveness"] = json!(liveness);
                    }
                    Ok(payload)
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 58. subscription_list
        TcCodeOp::SubscriptionList => {
            let input: ListLimitInput = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(
                    IpcRequest::SubscriptionList(SubscriptionListParams { limit: input.limit }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::SubscriptionList(SubscriptionListResponse {
                    subscriptions,
                    truncated,
                }) => Ok(json!({
                    "subscriptions": subscriptions,
                    "truncated": truncated,
                })),
                _ => Err(unexpected_variant()),
            }
        }

        // 59. subscription_close
        TcCodeOp::SubscriptionClose => {
            let input: SubscriptionCloseInput = decode_op_kwargs(op, kwargs)?;
            match caller
                .call_ipc(
                    IpcRequest::SubscriptionClose(SubscriptionCloseParams {
                        sub_id: input.sub_id,
                    }),
                    None,
                    DEFAULT_RPC_TIMEOUT,
                )
                .await?
            {
                IpcResponse::SubscriptionClose(SubscriptionCloseResponse { closed }) => {
                    Ok(json!({ "closed": closed }))
                }
                _ => Err(unexpected_variant()),
            }
        }

        // 60. subscription_seek
        TcCodeOp::SubscriptionSeek => {
            let input: SubscriptionSeekInput = decode_op_kwargs(op, kwargs)?;
            let bucket_id = parse_id::<BucketIdKind>("bucket_id", &input.bucket_id)
                .map_err(CodeBackendError::invalid_args)?;
            let ipc = SubscriptionSeekParams {
                sub_id: input.sub_id,
                bucket_id,
                seq: input.seq,
            };
            match caller
                .call_ipc(IpcRequest::SubscriptionSeek(ipc), None, DEFAULT_RPC_TIMEOUT)
                .await?
            {
                IpcResponse::SubscriptionSeek(SubscriptionSeekResponse {
                    clamped_seq,
                    lagged,
                }) => Ok(json!({
                    "clamped_seq": clamped_seq,
                    "lagged": lagged,
                })),
                _ => Err(unexpected_variant()),
            }
        }
    }
}

// -------------------------------------------------------------------------
// Payload Projection Helpers (Byte-Identical to crates/mcp/src/tools.rs)
// -------------------------------------------------------------------------

fn project_system_discover(d: &DiscoverResponse, detail: DiscoverDetailInput) -> Value {
    match detail {
        DiscoverDetailInput::Full => serde_json::to_value(d).unwrap_or(Value::Null),
        DiscoverDetailInput::Summary => {
            let env = &d.environment;
            let routes: Vec<_> = env
                .access_routes
                .iter()
                .filter(|r| r.kind != "direct_argv")
                .collect();
            let argv_tools: Vec<&str> = env
                .access_routes
                .iter()
                .filter(|r| r.kind == "direct_argv")
                .map(|r| r.route_id.strip_prefix("argv:").unwrap_or(&r.route_id))
                .collect();
            json!({
                "detail": "summary",
                "version": d.version,
                "mcp_spec": d.mcp_spec,
                "policy_profile": d.policy_profile,
                "identity": d.identity,
                "capabilities": d.capabilities,
                "method_count": d.methods.len(),
                "environment": {
                    "os": env.os,
                    "arch": env.arch,
                    "terminal": env.terminal,
                    "shells": env.shells,
                    "wsl": env.wsl,
                    "preferred_shell": env.preferred_shell,
                    "beachhead": env.beachhead,
                    "routes": routes,
                    "argv_tools": argv_tools,
                    "tools": env.tools,
                    "discovery_ms": env.discovery_ms,
                    "discovery_age_ms": env.discovery_age_ms,
                }
            })
        }
    }
}

async fn resolve_latest_rule_version<C: AsyncIpcCaller + ?Sized>(
    caller: &C,
    rule_id: &str,
) -> Result<u32, CodeBackendError> {
    let ipc = RegistryGetParams {
        rule_id: rule_id.to_owned(),
        version: None,
    };
    match caller
        .call_ipc(IpcRequest::RegistryGet(ipc), None, DEFAULT_RPC_TIMEOUT)
        .await?
    {
        IpcResponse::RegistryGet(RegistryGetResponse { definition }) => Ok(definition.version),
        _ => Err(unexpected_variant()),
    }
}

struct RecipeWatchState {
    state: Option<JobState>,
    outcome_trust: Option<OutcomeTrust>,
    exit_code: Option<i32>,
    pipeline_exit_masked: bool,
    elapsed_ms: Option<u64>,
    last_output_age_ms: Option<u64>,
    cursor: u64,
    signals: Vec<SignalEvent>,
    receipt: Option<Value>,
    degraded: bool,
    recover_hint: Option<String>,
    signals_capped: bool,
    governor_fields: Map<String, Value>,
}

#[allow(clippy::too_many_lines)]
async fn watch_recipe_job<C: AsyncIpcCaller + ?Sized>(
    caller: &C,
    job_id: JobId,
    bucket_id: BucketId,
    initial_cursor: u64,
    wait_ms: u64,
) -> Result<RecipeWatchState, CodeBackendError> {
    let max_signals = RUN_AND_WATCH_DEFAULT_MAX_SIGNALS;
    let mut signals = Vec::new();
    let mut cursor = initial_cursor;
    let mut resume_cursor = initial_cursor;
    let mut last_state = None;
    let mut last_outcome_trust = None;
    let mut exit_code = None;
    let mut pipeline_exit_masked = false;
    let mut elapsed_ms = None;
    let mut last_output_age_ms = None;
    let mut receipt: Option<Value>;
    let mut governor_fields: Map<String, Value>;
    let deadline = Instant::now() + Duration::from_millis(wait_ms);

    loop {
        let status = match caller
            .call_ipc(
                IpcRequest::CommandStatus(CommandStatusParams { job_id }),
                None,
                DEFAULT_RPC_TIMEOUT,
            )
            .await
        {
            Ok(IpcResponse::CommandStatus(status)) => status,
            Ok(_) => return Err(unexpected_variant()),
            Err(err) => {
                let ipc_err = backend_err_to_ipc_err(&err);
                return Ok(RecipeWatchState {
                    state: last_state,
                    outcome_trust: last_outcome_trust,
                    exit_code,
                    pipeline_exit_masked,
                    elapsed_ms,
                    last_output_age_ms,
                    cursor: resume_cursor,
                    signals,
                    receipt: None,
                    degraded: true,
                    recover_hint: Some(degraded_wait_hint(&ipc_err)),
                    signals_capped: false,
                    governor_fields: Map::new(),
                });
            }
        };

        last_state = Some(status.state);
        last_outcome_trust = Some(effective_outcome_trust(&status));
        exit_code = status.exit_code;
        pipeline_exit_masked = status.pipeline_exit_masked;
        elapsed_ms = status.elapsed_ms;
        last_output_age_ms = status.last_output_age_ms;
        receipt = status.receipt.as_ref().map(|r| json!(r));
        governor_fields = governor_status_fields(&status);

        let terminal = matches!(
            status.state,
            JobState::Exited | JobState::Cancelled | JobState::Failed
        );
        let remaining_ms = duration_ms_u64(deadline.saturating_duration_since(Instant::now()));
        let slice_ms = if terminal {
            0
        } else {
            MAX_WAIT_SLICE_MS.min(remaining_ms)
        };

        if let Some(err) = drain_recipe_signals(
            caller,
            bucket_id,
            &mut cursor,
            &mut resume_cursor,
            &mut signals,
            max_signals,
            slice_ms,
        )
        .await?
        {
            return Ok(RecipeWatchState {
                state: last_state,
                outcome_trust: last_outcome_trust,
                exit_code,
                pipeline_exit_masked,
                elapsed_ms,
                last_output_age_ms,
                cursor: resume_cursor,
                signals,
                receipt: None,
                degraded: true,
                recover_hint: Some(degraded_wait_hint(&err)),
                signals_capped: false,
                governor_fields: Map::new(),
            });
        }

        if terminal || signals.len() >= max_signals {
            break;
        }
        if Instant::now() >= deadline {
            if let Some(err) = drain_recipe_signals(
                caller,
                bucket_id,
                &mut cursor,
                &mut resume_cursor,
                &mut signals,
                max_signals,
                0,
            )
            .await?
            {
                return Ok(RecipeWatchState {
                    state: last_state,
                    outcome_trust: last_outcome_trust,
                    exit_code,
                    pipeline_exit_masked,
                    elapsed_ms,
                    last_output_age_ms,
                    cursor: resume_cursor,
                    signals,
                    receipt: None,
                    degraded: true,
                    recover_hint: Some(degraded_wait_hint(&err)),
                    signals_capped: false,
                    governor_fields: Map::new(),
                });
            }
            break;
        }
    }

    let signals_capped = signals.len() >= max_signals;
    Ok(RecipeWatchState {
        state: last_state,
        outcome_trust: last_outcome_trust,
        exit_code,
        pipeline_exit_masked,
        elapsed_ms,
        last_output_age_ms,
        cursor: resume_cursor,
        signals,
        receipt,
        degraded: false,
        recover_hint: None,
        signals_capped,
        governor_fields,
    })
}

async fn drain_recipe_signals<C: AsyncIpcCaller + ?Sized>(
    caller: &C,
    bucket_id: BucketId,
    cursor: &mut u64,
    resume_cursor: &mut u64,
    signals: &mut Vec<SignalEvent>,
    max_signals: usize,
    timeout_ms: u64,
) -> Result<Option<IpcError>, CodeBackendError> {
    let rpc_timeout = Duration::from_millis(timeout_ms).saturating_add(RPC_HEADROOM);
    match caller
        .call_ipc(
            IpcRequest::BucketWait(BucketWaitParams {
                bucket_id,
                cursor: *cursor,
                severity_min: None,
                kind_filter: None,
                limit: Some(max_signals.saturating_sub(signals.len()).max(1)),
                timeout_ms: Some(timeout_ms),
                wake_mode: None,
            }),
            None,
            rpc_timeout,
        )
        .await
    {
        Ok(IpcResponse::BucketWait(page)) => {
            let room = max_signals.saturating_sub(signals.len());
            let rule_count = page.events.iter().filter(|ev| ev.rule.is_some()).count();
            *cursor = page.next_cursor;
            for ev in page.events {
                if ev.rule.is_some() && signals.len() < max_signals {
                    signals.push(ev);
                }
            }
            if rule_count <= room {
                *resume_cursor = *cursor;
            }
            Ok(None)
        }
        Ok(_) => Err(unexpected_variant()),
        Err(err) => Ok(Some(backend_err_to_ipc_err(&err))),
    }
}

fn project_signal_compact(ev: &SignalEvent) -> Value {
    json!({
        "summary": ev.summary,
        "stream": ev.source.stream,
        "seq": ev.seq,
        "severity": ev.severity,
    })
}

fn project_events(events: &[SignalEvent], compact: bool) -> Value {
    if compact {
        json!(
            events
                .iter()
                .map(project_signal_compact)
                .collect::<Vec<_>>()
        )
    } else {
        json!(events)
    }
}

fn bucket_events_payload(r: &BucketEventsSinceResponse, compact: bool) -> Value {
    let mut payload = json!({
        "bucket_id": r.bucket_id,
        "cursor_in": r.cursor_in,
        "next_cursor": r.next_cursor,
        "has_more": r.has_more,
        "dropped_count": r.dropped_count,
        "events": project_events(&r.events, compact),
    });
    if compact {
        payload["compact"] = json!(true);
    }
    payload
}

fn bucket_wait_payload(r: &BucketWaitResponse, compact: bool) -> Value {
    let wake_reason = r.wake_reason.unwrap_or(if r.events.is_empty() {
        BucketWakeReason::Timeout
    } else {
        BucketWakeReason::Append
    });
    let mut payload = json!({
        "bucket_id": r.bucket_id,
        "cursor_in": r.cursor_in,
        "next_cursor": r.next_cursor,
        "heartbeat": r.heartbeat,
        "wake_reason": wake_reason,
        "dropped_count": r.dropped_count,
        "events": project_events(&r.events, compact),
    });
    if let Some(ms) = r.timeout_clamped_to_ms {
        payload["timeout_clamped_to_ms"] = json!(ms);
    }
    if let Some(exit) = &r.job_exit {
        payload["job_exit"] = json!(exit);
    }
    if compact {
        payload["compact"] = json!(true);
    }
    payload
}

fn event_context_payload(r: &EventContextResponse) -> Value {
    let frames: Vec<Value> = r
        .frames
        .iter()
        .map(|f: &IpcContextFrame| {
            json!({
                "probe_id": f.probe_id,
                "frame_id": f.frame_id,
                "stream": f.stream,
                "line": f.line,
                "text": f.text,
            })
        })
        .collect();
    let unavail = r.unavailable_reason.map(|u| match u {
        ContextUnavailableReason::NoPointer => "no_pointer",
        ContextUnavailableReason::SyntheticEvent => "synthetic_event",
        ContextUnavailableReason::AnchorEvicted => "anchor_evicted",
        ContextUnavailableReason::UnknownProbe => "unknown_probe",
    });
    json!({
        "bucket_id": r.bucket_id,
        "event_id": r.event_id,
        "anchor_missing": r.anchor_missing,
        "unavailable_reason": unavail,
        "pointer_unavailable_reason": r.pointer_unavailable_reason,
        "frames": frames,
        "total_bytes": r.total_bytes,
        "truncated": r.truncated,
    })
}

fn effective_outcome_trust(s: &CommandStatusResponse) -> OutcomeTrust {
    if s.restarted && s.outcome_trust == OutcomeTrust::Observed {
        OutcomeTrust::Reconstructed
    } else {
        s.outcome_trust
    }
}

fn governor_status_fields(s: &CommandStatusResponse) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(g) = &s.governor {
        m.insert("governor".to_owned(), json!(g));
    }
    if let Some(a) = &s.limits_applied {
        m.insert("limits_applied".to_owned(), json!(a));
    }
    if let Some(p) = s.peak_memory_bytes {
        m.insert("peak_memory_bytes".to_owned(), json!(p));
    }
    if let Some(r) = &s.exit_reason {
        m.insert("exit_reason".to_owned(), json!(r));
    }
    if let Some(j) = s.host_ceiling_joined {
        m.insert("host_ceiling_joined".to_owned(), json!(j));
    }
    m
}

fn obj_extend(v: &mut Value, fields: Map<String, Value>) {
    if let Some(o) = v.as_object_mut() {
        o.extend(fields);
    }
}

fn add_limits(
    body: &mut Value,
    applied: Option<&LimitsApplied>,
    clamped: &[String],
    governor: Option<&GovernorModeWire>,
) {
    if let Some(a) = applied {
        body["limits_applied"] = json!(a);
    }
    if let Some(g) = governor {
        body["governor"] = json!(g);
    }
    if !clamped.is_empty() {
        body["limits_clamped"] = json!(clamped);
    }
}

fn add_wslenv_dropped(body: &mut Value, names: &[String]) {
    if !names.is_empty() {
        body["wslenv_dropped"] = json!({
            "names": names,
            "note": "Not forwarded into WSL because the names look secret. To forward one on purpose, pass WSLENV in this call's env; it is used as given.",
        });
    }
}

fn password_prompt_hint(argv: &[String]) -> Option<&'static str> {
    let launched = terminal_commander_core::shell_deny::launched_argv(argv);
    let program = launched.first()?;
    let base = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    let stem = base.strip_suffix(".exe").unwrap_or(&base);
    matches!(stem, "sudo" | "su" | "doas" | "ssh").then_some(PASSWORD_PROMPT_HINT)
}

fn command_status_payload(s: &CommandStatusResponse) -> Value {
    let mut v = json!({
        "job_id": s.job_id,
        "bucket_id": s.bucket_id,
        "probe_id": s.probe_id,
        "state": s.state,
        "frames_total": s.frames_total,
        "frames_stdout": s.frames_stdout,
        "frames_stderr": s.frames_stderr,
        "bytes_total": s.bytes_total,
        "events_emitted": s.events_emitted,
        "frames_suppressed": s.frames_suppressed,
        "frames_suppressed_progress": s.frames_suppressed_progress,
        "frames_suppressed_dedupe": s.frames_suppressed_dedupe,
        "exit_code": s.exit_code,
        "signal": s.signal,
        "duration_ms": s.duration_ms,
        "receipt": s.receipt,
        "restarted": s.restarted,
        "outcome_trust": effective_outcome_trust(s),
        "pipeline_exit_masked": s.pipeline_exit_masked,
    });
    if let Some(awaiting) = s.awaiting_credential {
        v["awaiting_credential"] = json!(awaiting);
    }
    if let Some(elapsed) = s.elapsed_ms {
        v["elapsed_ms"] = json!(elapsed);
    }
    if let Some(age) = s.last_output_age_ms {
        v["last_output_age_ms"] = json!(age);
    }
    obj_extend(&mut v, governor_status_fields(s));
    if let Some(observation) = &s.process_observation {
        v["process_observation"] = json!(observation);
    }
    if let Some(cleanup) = &s.process_cleanup {
        v["process_cleanup"] = json!(cleanup);
    }
    if let Some(identity) = &s.process_identity {
        v["process_identity"] = json!(identity);
    }
    if let Some(cpu) = &s.cpu {
        v["cpu"] = json!(cpu);
    }
    v
}

fn shell_exec_payload(response: &CommandStartResponse, shell_line: &str) -> Value {
    let CommandStartResponse {
        job_id,
        bucket_id,
        probe_id,
        cursor,
        hint: _,
        wslenv_dropped,
        limits_applied,
        limits_clamped,
        governor,
    } = response;
    let mut payload = json!({
        "job_id": job_id,
        "bucket_id": bucket_id,
        "probe_id": probe_id,
        "cursor": cursor,
    });
    add_wslenv_dropped(&mut payload, wslenv_dropped);
    add_limits(
        &mut payload,
        limits_applied.as_ref(),
        limits_clamped,
        governor.as_ref(),
    );

    let lower = shell_line.to_ascii_lowercase();
    let mut detected = Vec::new();
    for (name, needles) in [
        ("tail", &["| tail", "|tail"][..]),
        ("head", &["| head", "|head"][..]),
        ("grep", &["| grep", "|grep"][..]),
        ("findstr", &["| findstr", "|findstr"][..]),
        (
            "select-object",
            &[
                "| select-object -first",
                "|select-object -first",
                "| select-object -last",
                "|select-object -last",
            ][..],
        ),
        ("select-string", &["| select-string", "|select-string"][..]),
    ] {
        if needles.iter().any(|needle| lower.contains(needle)) {
            detected.push(name);
        }
    }

    if !detected.is_empty()
        && let Some(obj) = payload.as_object_mut()
    {
        obj.insert(
            "observability".into(),
            json!({
                "status": "limited",
                "executed_unchanged": true,
                "reason": "shell_prefilter",
                "detected_prefilters": detected,
                "message": "The shell filtered output before TC ingestion; TC executed the line unchanged and cannot recover discarded lines.",
                "better_route": "Run the producer directly with run_and_watch plus rules; use file_search for existing logs.",
            }),
        );
    }

    payload
}

const fn run_and_watch_completion(state: JobState) -> (bool, bool) {
    let complete = matches!(
        state,
        JobState::Exited | JobState::Cancelled | JobState::Failed
    );
    (complete, !complete)
}

fn run_and_watch_outcome_value(outcome: &RunAndWatchOutcome, compact: bool) -> Value {
    let status = outcome.status.as_ref();
    let recover_hint = outcome.error.as_ref().map(degraded_wait_hint);
    let mut body = run_and_watch_result_value(
        outcome.start.job_id,
        outcome.start.bucket_id,
        outcome.cursor,
        status.map(|s| s.state),
        status.map(effective_outcome_trust),
        status.and_then(|s| s.exit_code),
        status.is_some_and(|s| s.pipeline_exit_masked),
        status.and_then(|s| s.elapsed_ms),
        status.and_then(|s| s.last_output_age_ms),
        &outcome.signals,
        status.and_then(|s| s.receipt.as_ref()).map(|r| json!(r)),
        outcome.degraded,
        recover_hint.as_deref(),
        compact,
        outcome.signals_capped,
    );
    if let Some(details) = outcome
        .error
        .as_ref()
        .and_then(|error| error.details.as_ref())
    {
        body["error_details"] = json!(details);
    }
    if !outcome.degraded {
        add_wslenv_dropped(&mut body, &outcome.start.wslenv_dropped);
        add_limits(
            &mut body,
            outcome.start.limits_applied.as_ref(),
            &outcome.start.limits_clamped,
            outcome.start.governor.as_ref(),
        );
        if let Some(status) = status {
            obj_extend(&mut body, governor_status_fields(status));
        }
    }
    body
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn run_and_watch_result_value(
    job_id: JobId,
    bucket_id: BucketId,
    cursor: u64,
    last_observed_state: Option<JobState>,
    outcome_trust: Option<OutcomeTrust>,
    exit_code: Option<i32>,
    pipeline_exit_masked: bool,
    elapsed_ms: Option<u64>,
    last_output_age_ms: Option<u64>,
    signals: &[SignalEvent],
    receipt: Option<Value>,
    degraded: bool,
    recover_hint: Option<&str>,
    compact: bool,
    signals_capped: bool,
) -> Value {
    let _ = Severity::Info;
    let (complete, wait_exhausted) = if degraded {
        (false, true)
    } else {
        last_observed_state.map_or((false, true), run_and_watch_completion)
    };
    let recover_hint = recover_hint
        .or_else(|| (!degraded && wait_exhausted).then_some(RUN_AND_WATCH_WAIT_EXHAUSTED_HINT));
    let state_json = last_observed_state.map_or_else(|| json!("unknown"), |state| json!(state));
    let include_receipt = !degraded && signals.is_empty();
    let signals_json = project_events(signals, compact);
    let poll_hint_ms = if complete || degraded {
        Value::Null
    } else {
        json!(RUN_AND_WATCH_POLL_HINT_MS)
    };
    let mut v = json!({
        "job_id": job_id,
        "bucket_id": bucket_id,
        "state": state_json,
        "exit_code": exit_code,
        "pipeline_exit_masked": pipeline_exit_masked,
        "signals": signals_json,
        "signal_count": signals.len(),
        "signals_capped": signals_capped,
        "compact": compact,
        "receipt": if include_receipt { receipt } else { None },
        "complete": complete,
        "wait_exhausted": wait_exhausted,
        "wait_cap_ms": RUN_AND_WATCH_MAX_WAIT_MS,
        "poll_hint_ms": poll_hint_ms,
        "cursor": cursor,
        "degraded": degraded,
        "recover_hint": recover_hint,
        "outcome_trust": outcome_trust,
    });
    if !complete {
        if let Some(elapsed) = elapsed_ms {
            v["elapsed_ms"] = json!(elapsed);
        }
        if let Some(age) = last_output_age_ms {
            v["last_output_age_ms"] = json!(age);
        }
    }
    v
}
