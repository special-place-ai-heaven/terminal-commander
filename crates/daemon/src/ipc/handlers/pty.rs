// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

#[cfg(any(unix, windows))]
use terminal_commander_core::RecipeTeachIntent;

use super::common::{attach_recipe_steer, enrich_shell_teach};
use crate::ipc::protocol::{
    CredentialProvideParams, CredentialRequestParams, CredentialUrlParams, IpcError, IpcErrorCode,
    IpcResponse, IpcResult, PtyCommandStartParams, PtyCommandStopParams,
    PtyCommandWriteStdinParams,
};
#[cfg(any(unix, windows))]
use crate::ipc::protocol::{
    DEFAULT_BUCKET_READ_LIMIT, MAX_BUCKET_WAIT_MS, MAX_COMMAND_ENV_ITEMS, MAX_COMMAND_INLINE_RULES,
    MAX_PTY_ARGV_ITEMS, MAX_PTY_STDIN_BYTES, PtyCommandListEntry, PtyCommandListResponse,
    PtyCommandStartResponse, PtyCommandStopResponse, PtyCommandWriteStdinResponse,
};
use crate::state::DaemonState;
use terminal_commander_supervisor::identity::PeerIdentity;

#[cfg(not(any(unix, windows)))]
pub(in crate::ipc::server) fn pty_ipc_unsupported() -> IpcError {
    IpcError::new(
        IpcErrorCode::UnsupportedPlatform,
        "PTY command runtime is not available on this platform yet (ConPTY support pending)",
    )
}

#[cfg(not(any(unix, windows)))]
pub(in crate::ipc::server) fn handle_pty_command_start(
    _state: &Arc<DaemonState>,
    _params: &PtyCommandStartParams,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(any(unix, windows)))]
#[allow(clippy::unused_async)] // async matches the unix signature; removed when unix impl lands
pub(in crate::ipc::server) async fn handle_pty_command_write_stdin(
    _state: &Arc<DaemonState>,
    _params: &PtyCommandWriteStdinParams,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(any(unix, windows)))]
pub(in crate::ipc::server) fn handle_pty_command_stop(
    _state: &Arc<DaemonState>,
    _params: &PtyCommandStopParams,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(any(unix, windows)))]
pub(in crate::ipc::server) fn handle_pty_command_list(
    _state: &Arc<DaemonState>,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(any(unix, windows)))]
#[allow(clippy::unused_async)] // async matches the PTY-host signature
pub(in crate::ipc::server) async fn handle_credential_request(
    _state: &Arc<DaemonState>,
    _params: &CredentialRequestParams,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(any(unix, windows)))]
#[allow(clippy::unused_async)] // async matches the PTY-host signature
pub(in crate::ipc::server) async fn handle_credential_provide(
    _state: &Arc<DaemonState>,
    _params: &CredentialProvideParams,
    _peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(any(unix, windows)))]
#[allow(clippy::unused_async)] // async matches the PTY-host signature
pub(in crate::ipc::server) async fn handle_credential_url(
    _state: &Arc<DaemonState>,
    _params: &CredentialUrlParams,
    _peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    Err(pty_ipc_unsupported())
}

#[cfg(not(any(unix, windows)))]
pub(in crate::ipc::server) fn dispatch_pty_command_list(
    state: &Arc<DaemonState>,
) -> (&'static str, IpcResult) {
    match handle_pty_command_list(state) {
        Ok(r) => ("pty_command_list", IpcResult::Ok { response: r }),
        Err(e) => ("pty_command_list", IpcResult::Err { error: e }),
    }
}

#[cfg(any(unix, windows))]
#[allow(clippy::too_many_lines)] // one arm per PtyRuntimeError; the deny texts are long
pub(in crate::ipc::server) fn handle_pty_command_start(
    state: &Arc<DaemonState>,
    params: &PtyCommandStartParams,
) -> Result<IpcResponse, IpcError> {
    if params.argv.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            "argv must not be empty",
        ));
    }
    if params.argv.len() > MAX_PTY_ARGV_ITEMS {
        return Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            format!(
                "argv has {} items; cap is {MAX_PTY_ARGV_ITEMS}",
                params.argv.len()
            ),
        ));
    }
    if params.env.len() > MAX_COMMAND_ENV_ITEMS {
        return Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            "env exceeds bounded item cap",
        ));
    }
    if params.rules.len() > MAX_COMMAND_INLINE_RULES {
        return Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            "rules exceeds bounded item cap",
        ));
    }
    let env_os: Vec<(std::ffi::OsString, std::ffi::OsString)> = params
        .env
        .iter()
        .map(|(k, v)| (std::ffi::OsString::from(k), std::ffi::OsString::from(v)))
        .collect();
    let req = crate::pty_command::PtyStartRequest {
        argv: params.argv.clone(),
        cwd: params.cwd.clone(),
        env: env_os,
        bucket_config: params.bucket_config.clone(),
        rules: params.rules.clone(),
        rows: params.rows,
        cols: params.cols,
        tag: params.tag.clone(),
    };
    let started = match state.pty.start(req) {
        Ok(r) => Ok(IpcResponse::PtyCommandStart(PtyCommandStartResponse {
            job_id: r.job_id,
            bucket_id: r.bucket_id,
            probe_id: r.probe_id,
            cursor: 0,
        })),
        Err(crate::pty_command::PtyRuntimeError::PolicyDenied(reason)) => {
            // Same failsafe typed-code mapping as the argv lane
            // (`map_command_error`): a deletion of OS-critical infrastructure
            // surfaces as `OsCriticalPathProtected`, not a generic deny.
            let code = if reason.contains(terminal_commander_core::FAILSAFE_REASON_TAG) {
                IpcErrorCode::OsCriticalPathProtected
            } else {
                IpcErrorCode::PolicyDenied
            };
            Err(IpcError::new(code, reason))
        }
        Err(crate::pty_command::PtyRuntimeError::ShellInterpreterDenied(shell)) => {
            Err(IpcError::new(
                IpcErrorCode::ShellInterpreterDenied,
                format!(
                    "shell interpreter '{shell}' denied: allow_shell is off. \
                     Run the program directly as argv (e.g. [\"cargo\",\"build\"] instead of \
                     {}).",
                    terminal_commander_core::shell_deny::denied_argv_example(&shell)
                ),
            ))
        }
        // US8 (FR-060): wsl-carrier nested shell on the PTY argv lane. Same
        // wire code as the command lane, carrier-aware teaching message.
        Err(crate::pty_command::PtyRuntimeError::WslNestedShellDenied {
            interpreter,
            carrier,
        }) if interpreter == "unrecognized construction" => Err(IpcError::new(
            IpcErrorCode::ShellInterpreterDenied,
            format!(
                "unrecognized '{carrier}' construction denied (fail closed): allow_shell is off. \
                 Use a recognized form ({carrier} -e <program> ..., or {carrier} --list / --status)."
            ),
        )),
        Err(crate::pty_command::PtyRuntimeError::WslNestedShellDenied {
            interpreter,
            carrier,
        }) => Err(IpcError::new(
            IpcErrorCode::ShellInterpreterDenied,
            format!(
                "shell interpreter '{interpreter}' denied inside a '{carrier}' invocation: \
                 allow_shell is off. Run the Linux program directly as argv ({carrier} -e <program> ...)."
            ),
        )),
        Err(crate::pty_command::PtyRuntimeError::EmptyArgv) => Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            "argv must not be empty",
        )),
        Err(crate::pty_command::PtyRuntimeError::ArgvInvalid(reason)) => {
            Err(IpcError::new(IpcErrorCode::ArgvInvalid, reason))
        }
        Err(crate::pty_command::PtyRuntimeError::Sifter(reason)) => {
            Err(IpcError::new(IpcErrorCode::RuleInvalid, reason))
        }
        // Same wire error as the argv lane (`map_command_error`).
        Err(crate::pty_command::PtyRuntimeError::ProgramNotFound(argv0)) => {
            let message = format!(
                "program not found: '{argv0}'. Remedy: check the spelling of argv[0] and \
                 ensure the program is on the daemon's PATH (or pass an absolute path)."
            );
            Err(IpcError::program_not_found(argv0, message))
        }
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("pty_command_start: {other}"),
        )),
    };
    started.map_err(|e| {
        let err = enrich_shell_teach(&state.policy, "pty_command_start", e);
        attach_recipe_steer(state, RecipeTeachIntent::Argv(&params.argv), err)
    })
}

#[cfg(any(unix, windows))]
pub(in crate::ipc::server) async fn handle_pty_command_write_stdin(
    state: &Arc<DaemonState>,
    params: &PtyCommandWriteStdinParams,
) -> Result<IpcResponse, IpcError> {
    let bytes = params.bytes.as_bytes();
    if bytes.len() > MAX_PTY_STDIN_BYTES {
        return Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            format!("stdin payload {} > cap {MAX_PTY_STDIN_BYTES}", bytes.len()),
        ));
    }
    match state.pty.write_stdin(params.job_id, bytes).await {
        Ok(r) => {
            // FR-041: an optional bounded settle read over the PTY job's
            // bucket, mirroring `shell_session_exec`. The write already
            // happened above (secret-prompt denial fired BEFORE it, inside
            // `write_stdin`). Absent `wait_ms` -> every combed field is
            // `None`, so the response serializes byte-identically to today.
            let (cursor_in, next_cursor, has_more, dropped_count, events) = if let Some(wait_ms) =
                params.wait_ms
            {
                use terminal_commander_core::BucketWaitRequest;
                let wait_ms = wait_ms.min(MAX_BUCKET_WAIT_MS);
                let req = BucketWaitRequest {
                    cursor: params.cursor.unwrap_or(0),
                    severity_min: None,
                    kind_filter: None,
                    limit: Some(DEFAULT_BUCKET_READ_LIMIT),
                    timeout: std::time::Duration::from_millis(wait_ms),
                };
                let settled = state
                    .router
                    .bucket_wait(r.bucket_id, req)
                    .await
                    .map_err(super::common::map_bucket_error)?;
                (
                    Some(settled.cursor_in),
                    Some(settled.next_cursor),
                    Some(!settled.heartbeat && settled.events.len() >= DEFAULT_BUCKET_READ_LIMIT),
                    Some(settled.dropped_count),
                    Some(settled.events),
                )
            } else {
                (None, None, None, None, None)
            };
            Ok(IpcResponse::PtyCommandWriteStdin(
                PtyCommandWriteStdinResponse {
                    job_id: params.job_id,
                    bytes_written: r.bytes_written,
                    secret_prompt_active: r.secret_prompt_active,
                    awaiting_credential: state.pty.awaiting_credential(params.job_id),
                    cursor_in,
                    next_cursor,
                    has_more,
                    dropped_count,
                    events,
                },
            ))
        }
        // TC44: the model never types into a password prompt. Teach the
        // owner path instead of a dead end.
        Err(crate::pty_command::PtyRuntimeError::SecretInputDenied) => Err(IpcError::new(
            IpcErrorCode::SecretInputDenied,
            format!(
                "secret prompt active; LLM-supplied input denied. This job is waiting for a \
                 password; TC never accepts passwords from the model -- call \
                 credential_request {{\"job_id\":\"{}\"}} and the owner will be asked \
                 directly; then poll command_status for the job",
                params.job_id.to_wire_string()
            ),
        )),
        Err(crate::pty_command::PtyRuntimeError::OversizedStdin) => Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            "stdin exceeds bounded cap",
        )),
        Err(crate::pty_command::PtyRuntimeError::UnknownJob(id)) => Err(IpcError::new(
            IpcErrorCode::UnknownJob,
            format!("pty job '{}' is not live", id.to_wire_string()),
        )),
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("pty_command_write_stdin: {other}"),
        )),
    }
}

#[cfg(any(unix, windows))]
pub(in crate::ipc::server) fn handle_pty_command_stop(
    state: &Arc<DaemonState>,
    params: &PtyCommandStopParams,
) -> Result<IpcResponse, IpcError> {
    match state.pty.stop(params.job_id) {
        Ok((bucket_id, m)) => Ok(IpcResponse::PtyCommandStop(PtyCommandStopResponse {
            job_id: params.job_id,
            bucket_id,
            frames_total: m.frames_total,
            events_emitted: m.events_emitted,
            bytes_total: m.bytes_total,
            stdin_bytes_written: m.stdin_bytes_written,
            secret_prompts_total: m.secret_prompts_total,
        })),
        Err(crate::pty_command::PtyRuntimeError::UnknownJob(id)) => Err(IpcError::new(
            IpcErrorCode::UnknownJob,
            format!("pty job '{}' is not live", id.to_wire_string()),
        )),
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("pty_command_stop: {other}"),
        )),
    }
}

#[cfg(any(unix, windows))]
pub(in crate::ipc::server) fn handle_pty_command_list(state: &Arc<DaemonState>) -> IpcResponse {
    let entries: Vec<PtyCommandListEntry> = state
        .pty
        .list()
        .into_iter()
        // The binding lingers after exit for per-job lifecycle lookups; the
        // operator-facing live list excludes terminal jobs.
        .filter(|(job_id, ..)| {
            !matches!(
                state.pty.liveness(*job_id),
                terminal_commander_ipc::Liveness::Exited { .. }
                    | terminal_commander_ipc::Liveness::Failed { .. }
                    | terminal_commander_ipc::Liveness::Cancelled
                    | terminal_commander_ipc::Liveness::Stopped
            )
        })
        .map(
            |(job_id, bucket_id, probe_id, argv, m, secret_prompt_active)| {
                let awaiting_credential = state.pty.awaiting_credential(job_id);
                // Only an owner-facing prompt needs the spawned program.
                let (program, program_env) = awaiting_credential
                    .and_then(|_| state.pty.program_of(job_id))
                    .map_or((None, Vec::new()), |(p, e)| (Some(p), e));
                PtyCommandListEntry {
                    job_id,
                    bucket_id,
                    probe_id,
                    argv,
                    frames_total: m.frames_total,
                    events_emitted: m.events_emitted,
                    bytes_total: m.bytes_total,
                    stdin_bytes_written: m.stdin_bytes_written,
                    secret_prompts_total: m.secret_prompts_total,
                    secret_prompt_active,
                    awaiting_credential,
                    program,
                    program_env,
                }
            },
        )
        .collect();
    IpcResponse::PtyCommandList(PtyCommandListResponse { entries })
}

#[cfg(any(unix, windows))]
fn pty_job_not_live(id: terminal_commander_core::JobId) -> IpcError {
    IpcError::new(
        IpcErrorCode::UnknownJob,
        format!("pty job '{}' is not live", id.to_wire_string()),
    )
}

/// `credential_request`: ask the owner for the password a PTY job waits on.
/// The response is a status only; the secret never enters it.
#[cfg(any(unix, windows))]
pub(in crate::ipc::server) async fn handle_credential_request(
    state: &Arc<DaemonState>,
    params: &CredentialRequestParams,
) -> Result<IpcResponse, IpcError> {
    match state.credentials.request(&state.pty, params.job_id).await {
        Ok(r) => Ok(IpcResponse::CredentialRequest(r)),
        Err(crate::pty_command::PtyRuntimeError::UnknownJob(id)) => Err(pty_job_not_live(id)),
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("credential_request: {other}"),
        )),
    }
}

/// `credential_provide`: the owner typed a password into the admin CLI.
/// Only the CLI image from the owner's own terminal passes; an MCP-labelled
/// or daemon-started peer is denied even if it sends the request raw.
#[cfg(any(unix, windows))]
pub(in crate::ipc::server) async fn handle_credential_provide(
    state: &Arc<DaemonState>,
    params: &CredentialProvideParams,
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    if !super::recipe::caller_is_owner_cli(state, peer, params.from_mcp) {
        return Err(IpcError::new(
            IpcErrorCode::PolicyDenied,
            "credential_provide_requires_owner: only the admin CLI run from the owner's own \
             terminal (`terminal-commander credential provide <job_id>`) may answer a password \
             prompt; the model never supplies a password. From MCP, call credential_request \
             {job_id} and the owner is asked directly.",
        ));
    }
    if params.secret.as_bytes().len() >= MAX_PTY_STDIN_BYTES {
        return Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            format!("password exceeds the {MAX_PTY_STDIN_BYTES}-byte PTY stdin cap"),
        ));
    }
    match state
        .pty
        .deliver_credential(params.job_id, params.secret.as_bytes(), None, "cli")
        .await
    {
        Ok(generation) => {
            state.credentials.record_provided(params.job_id, generation);
            Ok(IpcResponse::CredentialProvide(
                crate::ipc::protocol::CredentialProvideResponse {
                    job_id: params.job_id,
                },
            ))
        }
        Err(crate::pty_command::PtyRuntimeError::UnknownJob(id)) => Err(pty_job_not_live(id)),
        Err(crate::pty_command::PtyRuntimeError::NotAwaitingCredential(id)) => Err(IpcError::new(
            IpcErrorCode::UnknownJob,
            format!(
                "pty job '{}' is not waiting for a password; nothing was typed",
                id.to_wire_string()
            ),
        )),
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("credential_provide: {other}"),
        )),
    }
}

/// `credential_url`: the MCP adapter's side of URL-mode elicitation. The
/// response carries the page URL (a single-use token), so only the MCP
/// adapter the harness launched may ask; the adapter sends it to the MCP
/// client, never to the model.
#[cfg(any(unix, windows))]
pub(in crate::ipc::server) async fn handle_credential_url(
    state: &Arc<DaemonState>,
    params: &CredentialUrlParams,
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    if !super::recipe::caller_is_harness_adapter(state, peer) {
        return Err(IpcError::new(
            IpcErrorCode::PolicyDenied,
            "credential_url_requires_mcp_adapter: only the MCP adapter launched by the \
             harness may open the owner's password page. Call credential_request {job_id}.",
        ));
    }
    match state
        .credentials
        .url(&state.pty, params.job_id, params.op)
        .await
    {
        Ok(r) => Ok(IpcResponse::CredentialUrl(r)),
        Err(crate::pty_command::PtyRuntimeError::UnknownJob(id)) => Err(pty_job_not_live(id)),
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("credential_url: {other}"),
        )),
    }
}

#[cfg(any(unix, windows))]
pub(in crate::ipc::server) fn dispatch_pty_command_list(
    state: &Arc<DaemonState>,
) -> (&'static str, IpcResult) {
    let r = handle_pty_command_list(state);
    ("pty_command_list", IpcResult::Ok { response: r })
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;

    /// A missing program is the argv lane's caller-fixable `ProgramNotFound`,
    /// with the typed `argv0`, not `Internal`.
    #[test]
    fn missing_program_maps_to_program_not_found() {
        // The unix PTY spawn needs a reactor.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let _guard = runtime.enter();
        let mut data = std::env::temp_dir();
        data.push(format!("tc-pty-not-found-{}", std::process::id()));
        let cfg = crate::config::DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"));
        let argv0 = "tc-no-such-program-4a";
        let err = handle_pty_command_start(
            &state,
            &PtyCommandStartParams {
                environment: None,
                argv: vec![argv0.to_owned()],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                rows: None,
                cols: None,
                tag: None,
            },
        )
        .expect_err("missing program");
        assert_eq!(err.code, IpcErrorCode::ProgramNotFound, "{}", err.message);
        assert_eq!(err.argv0.as_deref(), Some(argv0));
        let _ = std::fs::remove_dir_all(&data);
    }
}
