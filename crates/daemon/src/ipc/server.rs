// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Unix-domain-socket server for terminal-commanderd (TC37).
//!
//! Bounded length-prefixed framing. Every accepted connection has
//! its peer credentials resolved (SO_PEERCRED on Linux, getpeereid
//! on macOS/BSD). On Linux/WSL a missing credential set is a hard
//! refusal: the connection is dropped without invoking the
//! dispatcher.
//!
//! Concurrency: one task per connection, per-connection serial
//! request/response. The accept loop hands the connection to a
//! spawned task and immediately returns to `accept()`.
//!
//! No TCP. No UDP. No HTTP. No command execution. Method set is the
//! TC37 minimum (see `protocol.rs`); TC38/TC39/TC41 add more.

use std::sync::Arc;
use std::time::Instant;

#[cfg(unix)]
use std::path::{Path, PathBuf};

#[cfg(unix)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
#[cfg(unix)]
use tokio::sync::watch;

use terminal_commander_supervisor::identity::PeerIdentity;

#[path = "handlers/mod.rs"]
mod handlers;

use crate::environment::{EnvironmentRouter, RouteOutcome};
#[cfg(unix)]
use crate::ipc::peer;
use crate::ipc::protocol::{
    DiscoverResponse, IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult,
    PolicyStatusResponse, RequestEnvelope, ResponseEnvelope, SelfCheckResponse,
};
#[cfg(unix)]
use crate::ipc::protocol::{MAX_FRAME_BYTES, decode_payload, encode_frame};
use crate::state::DaemonState;
#[cfg(unix)]
use handlers::common::emit_audit_internal_error;
use handlers::common::{emit_audit, identity_audit_subject};
use terminal_commander_core::EnvironmentSpec;

#[cfg(unix)]
/// Handle returned from [`IpcServer::spawn`]. Drop the handle to
/// signal the accept loop to stop; call [`ServerHandle::shutdown`]
/// to await orderly shutdown.
///
/// Backed by `tokio::sync::watch::channel(false)`: the sticky
/// "shutdown requested" flag survives the race between
/// [`IpcServer::spawn`] returning and the accept loop's first poll.
#[cfg(unix)]
pub struct ServerHandle {
    shutdown_tx: watch::Sender<bool>,
    join: Option<tokio::task::JoinHandle<()>>,
    socket_path: PathBuf,
}

#[cfg(unix)]
impl ServerHandle {
    /// Signal shutdown and wait for the accept loop to exit. The
    /// socket file is removed before returning.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(j) = self.join.take() {
            let _ = j.await;
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }

    /// Socket path the server is listening on. Test helper.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

#[cfg(unix)]
impl Drop for ServerHandle {
    fn drop(&mut self) {
        // Best-effort cleanup if the operator does not call shutdown.
        let _ = self.shutdown_tx.send(true);
        if let Some(j) = self.join.take() {
            j.abort();
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[cfg(unix)]
/// IPC server. Owns the listener, the daemon state, and the boot
/// timestamp used by the `health` method.
pub struct IpcServer {
    state: Arc<DaemonState>,
    boot: Instant,
    socket_path: PathBuf,
}

#[cfg(unix)]
impl IpcServer {
    /// Construct a server. Does NOT bind the listener yet.
    #[must_use]
    pub fn new(state: Arc<DaemonState>, socket_path: PathBuf) -> Self {
        Self {
            state,
            boot: Instant::now(),
            socket_path,
        }
    }

    /// Bind the listener and spawn the accept loop. Returns a handle
    /// that can be used to shut down. MUST be called from within a
    /// tokio runtime; `UnixListener::bind` registers the listener
    /// with the current reactor.
    pub fn spawn(self) -> Result<ServerHandle, std::io::Error> {
        // Pre-clean a leftover socket file if any.
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }
        let listener = UnixListener::bind(&self.socket_path)?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let socket_path = self.socket_path.clone();
        let state = Arc::clone(&self.state);
        let boot = self.boot;
        let join = tokio::spawn(async move {
            accept_loop(listener, state, boot, shutdown_rx).await;
        });
        Ok(ServerHandle {
            shutdown_tx,
            join: Some(join),
            socket_path,
        })
    }
}

/// Upper bound on how long the accept loop waits for in-flight
/// connection tasks to finish during a graceful drain. A wedged
/// connection (e.g. a client that stopped reading mid-response) must
/// not be able to hang daemon shutdown forever: once this ceiling is
/// hit, remaining tasks are aborted and the loop returns so the
/// process can exit.
#[cfg(unix)]
const DRAIN_CEILING: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(unix)]
async fn accept_loop(
    listener: UnixListener,
    state: Arc<DaemonState>,
    boot: Instant,
    mut shutdown: watch::Receiver<bool>,
) {
    // Per-connection tasks are tracked in a JoinSet (rather than
    // detached `tokio::spawn`) so a graceful shutdown can DRAIN them:
    // when the shutdown flag flips, the connection serving the
    // `Shutdown` request finishes flushing its ACK and every other
    // in-flight request runs to completion before the loop returns.
    // The accept loop owns the set; `ServerHandle::shutdown` joins
    // THIS task, so awaiting the join handle transitively waits for
    // the drain below.
    let mut conns: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();

    // Sticky shutdown: if the flag is already true (the operator
    // dropped the handle before the loop ran its first poll), exit
    // before issuing an accept.
    if *shutdown.borrow() {
        return;
    }
    loop {
        tokio::select! {
            biased;
            res = shutdown.changed() => {
                // The sender was dropped, or the value moved to true.
                // Either way: stop accepting. (We do not distinguish;
                // both mean "no more requests should be accepted".)
                if res.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            // Reap finished connection tasks as they complete so the
            // JoinSet does not grow without bound under steady load.
            // Guarded so `join_next` is only polled when non-empty
            // (it resolves to `None` immediately on an empty set,
            // which would otherwise busy-spin this branch).
            Some(_joined) = conns.join_next(), if !conns.is_empty() => {}
            res = listener.accept() => {
                match res {
                    Ok((stream, _addr)) => {
                        let state = Arc::clone(&state);
                        let shutdown_for_conn = shutdown.clone();
                        conns.spawn(async move {
                            handle_connection(stream, state, boot, shutdown_for_conn).await;
                        });
                    }
                    Err(e) => {
                        // Accept errors are typically not fatal
                        // (EMFILE, ECONNABORTED). Audit and continue.
                        emit_audit_internal_error(
                            &state,
                            "ipc_accept",
                            &format!("accept failed: {e}"),
                        );
                    }
                }
            }
        }
    }

    // Drain: we have stopped accepting. The shutdown flag is now
    // `true`, so each in-flight `handle_connection` will break out of
    // its own loop at the next request boundary; a connection that is
    // mid-dispatch finishes the current request (and flushes its
    // response) first. Wait for all of them, bounded by DRAIN_CEILING.
    drain_connections(&mut conns).await;
}

/// Await every remaining connection task, bounded by [`DRAIN_CEILING`].
/// If the ceiling is reached with tasks still running, abort the
/// remainder so shutdown cannot hang on a wedged connection.
#[cfg(unix)]
async fn drain_connections(conns: &mut tokio::task::JoinSet<()>) {
    if conns.is_empty() {
        return;
    }
    let drain = async { while conns.join_next().await.is_some() {} };
    if tokio::time::timeout(DRAIN_CEILING, drain).await.is_err() {
        // Ceiling hit: a connection did not finish in time. Abort the
        // stragglers and return so the process can exit. JoinSet::abort_all
        // is best-effort; we do not re-await aborted handles.
        conns.abort_all();
    }
}

#[cfg(unix)]
async fn handle_connection(
    mut stream: UnixStream,
    state: Arc<DaemonState>,
    boot: Instant,
    mut shutdown: watch::Receiver<bool>,
) {
    let peer_cred = peer::resolve(&stream);
    // Build a PeerIdentity from the resolved cred (or Unknown).
    let identity: PeerIdentity = peer_cred.map_or_else(
        || PeerIdentity::unknown_because("peer credentials unavailable"),
        |c| PeerIdentity::Unix {
            uid: c.uid,
            gid: c.gid,
            pid: c.pid,
        },
    );

    // Linux/WSL: fail-closed when peer creds are missing.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        if !identity.is_known() {
            // Emit an audit row for the refused connection. The
            // subject is a synthetic descriptor so we can correlate
            // refusals in the audit log without ever writing peer
            // metadata we could not obtain.
            emit_audit(
                &state,
                "ipc_connect",
                "unknown_peer",
                "deny",
                Some("peer credentials unavailable (Linux/WSL fail-closed)".to_owned()),
                &identity,
            );
            // Refuse: send a structured error, then close.
            let env = ResponseEnvelope {
                correlation_id: 0,
                result: IpcResult::Err {
                    error: IpcError::new(
                        IpcErrorCode::PeerCredentialFailure,
                        "peer credentials unavailable; connection refused",
                    ),
                },
            };
            let _ = write_envelope(&mut stream, &env).await;
            return;
        }
    }

    // Audit the connection itself once, before any request.
    {
        let subject = identity_audit_subject(&identity);
        emit_audit(&state, "ipc_connect", &subject, "info", None, &identity);
    }

    // Sticky shutdown: if the flag is already true, do not even
    // start serving requests on this connection.
    if *shutdown.borrow() {
        return;
    }
    loop {
        tokio::select! {
            biased;
            res = shutdown.changed() => {
                if res.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            res = read_envelope(&mut stream) => {
                match res {
                    ReadOutcome::Eof => break,
                    ReadOutcome::Err(err) => {
                        let env = ResponseEnvelope {
                            correlation_id: 0,
                            result: IpcResult::Err { error: err },
                        };
                        let _ = write_envelope(&mut stream, &env).await;
                        // Treat malformed framing as connection-fatal.
                        break;
                    }
                    ReadOutcome::Ok(req_env) => {
                        let resp = dispatch(&state, boot, &req_env, &identity).await;
                        if let Err(io_err) = write_envelope(&mut stream, &resp).await {
                            emit_audit_internal_error(
                                &state,
                                "ipc_write",
                                &format!("write failed: {io_err}"),
                            );
                            break;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(unix)]
#[allow(clippy::large_enum_variant)]
enum ReadOutcome {
    Ok(RequestEnvelope),
    Err(IpcError),
    Eof,
}

#[cfg(unix)]
async fn read_envelope(stream: &mut UnixStream) -> ReadOutcome {
    // 4-byte length prefix.
    let mut len_buf = [0_u8; 4];
    match stream.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return ReadOutcome::Eof,
        Err(e) => {
            return ReadOutcome::Err(IpcError::new(
                IpcErrorCode::Internal,
                format!("read length: {e}"),
            ));
        }
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return ReadOutcome::Err(IpcError::new(
            IpcErrorCode::FrameTooLarge,
            format!("frame {len} bytes > MAX_FRAME_BYTES {MAX_FRAME_BYTES}"),
        ));
    }
    let mut payload = vec![0_u8; len];
    if let Err(e) = stream.read_exact(&mut payload).await {
        return ReadOutcome::Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("read payload: {e}"),
        ));
    }
    match decode_payload::<RequestEnvelope>(&payload) {
        Ok(env) => ReadOutcome::Ok(env),
        Err(err) => ReadOutcome::Err(err),
    }
}

#[cfg(unix)]
async fn write_envelope(
    stream: &mut UnixStream,
    env: &ResponseEnvelope,
) -> Result<(), std::io::Error> {
    let frame = match encode_frame(env) {
        Ok(bytes) => bytes,
        Err(err) => {
            // Encoding failed (likely FrameTooLarge); replace with a
            // small error envelope and re-encode.
            let small = ResponseEnvelope {
                correlation_id: env.correlation_id,
                result: IpcResult::Err {
                    error: IpcError::new(err.code, err.message),
                },
            };
            // This second encode cannot fail at MAX_FRAME_BYTES for
            // a small error payload; if it somehow does, drop.
            encode_frame(&small)
                .map_err(|e| std::io::Error::other(format!("encode small err: {}", e.message)))?
        }
    };
    stream.write_all(&frame).await
}

#[allow(clippy::too_many_lines)] // method dispatcher
async fn dispatch(
    state: &Arc<DaemonState>,
    boot: Instant,
    req_env: &RequestEnvelope,
    peer: &PeerIdentity,
) -> ResponseEnvelope {
    // Every real (non-peek) request resets the idle timer. Health is a
    // pure inspection peek: it must NOT bump activity, or `session list`
    // polling would defeat the daemon's idle self-reap.
    if !matches!(&req_env.request, IpcRequest::Health) {
        state.bump_activity();
    }
    let (method_name, response_result) = match &req_env.request {
        IpcRequest::SystemDiscover => {
            let r = handle_system_discover(state);
            ("system_discover", IpcResult::Ok { response: r })
        }
        IpcRequest::Health => {
            let r = IpcResponse::Health {
                uptime_secs: boot.elapsed().as_secs(),
                idle_secs: Some(state.idle_secs()),
            };
            ("health", IpcResult::Ok { response: r })
        }
        IpcRequest::PolicyStatus => {
            let r = handle_policy_status(state);
            ("policy_status", IpcResult::Ok { response: r })
        }
        IpcRequest::SelfCheck => {
            let r = handle_self_check(state);
            ("self_check", IpcResult::Ok { response: r })
        }
        IpcRequest::BucketEventsSince(p) => {
            match handlers::bucket::handle_bucket_events_since(state, p) {
                Ok(r) => ("bucket_events_since", IpcResult::Ok { response: r }),
                Err(e) => ("bucket_events_since", IpcResult::Err { error: e }),
            }
        }
        IpcRequest::BucketWait(p) => match handlers::bucket::handle_bucket_wait(state, p).await {
            Ok(r) => ("bucket_wait", IpcResult::Ok { response: r }),
            Err(e) => ("bucket_wait", IpcResult::Err { error: e }),
        },
        IpcRequest::BucketSummary(p) => match handlers::bucket::handle_bucket_summary(state, p) {
            Ok(r) => ("bucket_summary", IpcResult::Ok { response: r }),
            Err(e) => ("bucket_summary", IpcResult::Err { error: e }),
        },
        IpcRequest::EventContext(p) => match handlers::bucket::handle_event_context(state, p) {
            Ok(r) => ("event_context", IpcResult::Ok { response: r }),
            Err(e) => ("event_context", IpcResult::Err { error: e }),
        },
        IpcRequest::CommandStartCombed(p) => {
            let env = p.environment.clone().unwrap_or_default();
            if matches!(env, EnvironmentSpec::Local) {
                match handlers::command::handle_command_start_combed(state, p) {
                    Ok(r) => ("command_start_combed", IpcResult::Ok { response: r }),
                    Err(e) => ("command_start_combed", IpcResult::Err { error: e }),
                }
            } else {
                match EnvironmentRouter::route_request(state, &env, &req_env.request).await {
                    Ok(RouteOutcome::RunnerResponse(r)) => {
                        ("command_start_combed", IpcResult::Ok { response: *r })
                    }
                    Ok(RouteOutcome::Local) => {
                        match handlers::command::handle_command_start_combed(state, p) {
                            Ok(r) => ("command_start_combed", IpcResult::Ok { response: r }),
                            Err(e) => ("command_start_combed", IpcResult::Err { error: e }),
                        }
                    }
                    Err(e) => (
                        "command_start_combed",
                        IpcResult::Err {
                            error: IpcError::new(IpcErrorCode::Internal, e.to_string()),
                        },
                    ),
                }
            }
        }
        IpcRequest::CommandStatus(p) => match handlers::command::handle_command_status(state, p) {
            Ok(r) => ("command_status", IpcResult::Ok { response: r }),
            Err(e) => ("command_status", IpcResult::Err { error: e }),
        },
        IpcRequest::CommandOutputTail(p) => {
            match handlers::command::handle_command_output_tail(state, p) {
                Ok(r) => ("command_output_tail", IpcResult::Ok { response: r }),
                Err(e) => ("command_output_tail", IpcResult::Err { error: e }),
            }
        }
        IpcRequest::RegistrySearch(p) => match handlers::registry::handle_registry_search(state, p)
        {
            Ok(r) => ("registry_search", IpcResult::Ok { response: r }),
            Err(e) => ("registry_search", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistryGet(p) => match handlers::registry::handle_registry_get(state, p) {
            Ok(r) => ("registry_get", IpcResult::Ok { response: r }),
            Err(e) => ("registry_get", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistryUpsert(p) => match handlers::registry::handle_registry_upsert(state, p)
        {
            Ok(r) => ("registry_upsert", IpcResult::Ok { response: r }),
            Err(e) => ("registry_upsert", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistryTest(p) => match handlers::registry::handle_registry_test(state, p) {
            Ok(r) => ("registry_test", IpcResult::Ok { response: r }),
            Err(e) => ("registry_test", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistryActivate(p) => {
            match handlers::registry::handle_registry_activate(state, p) {
                Ok(r) => ("registry_activate", IpcResult::Ok { response: r }),
                Err(e) => ("registry_activate", IpcResult::Err { error: e }),
            }
        }
        IpcRequest::RegistryImportPack(p) => {
            match handlers::registry::handle_registry_import_pack(state, p) {
                Ok(r) => ("registry_import_pack", IpcResult::Ok { response: r }),
                Err(e) => ("registry_import_pack", IpcResult::Err { error: e }),
            }
        }
        IpcRequest::RegistryDeactivate(p) => {
            match handlers::registry::handle_registry_deactivate(state, p) {
                Ok(r) => ("registry_deactivate", IpcResult::Ok { response: r }),
                Err(e) => ("registry_deactivate", IpcResult::Err { error: e }),
            }
        }
        IpcRequest::RegistryListActive => {
            let r = handlers::registry::handle_registry_list_active(state);
            ("registry_list_active", IpcResult::Ok { response: r })
        }
        IpcRequest::FileReadWindow(p) => match handlers::file::handle_file_read_window(state, p) {
            Ok(r) => ("file_read_window", IpcResult::Ok { response: r }),
            Err(e) => ("file_read_window", IpcResult::Err { error: e }),
        },
        IpcRequest::FileSearch(p) => match handlers::file::handle_file_search(state, p) {
            Ok(r) => ("file_search", IpcResult::Ok { response: r }),
            Err(e) => ("file_search", IpcResult::Err { error: e }),
        },
        IpcRequest::FileWatchStart(p) => match handlers::file::handle_file_watch_start(state, p) {
            Ok(r) => ("file_watch_start", IpcResult::Ok { response: r }),
            Err(e) => ("file_watch_start", IpcResult::Err { error: e }),
        },
        IpcRequest::FileWatchStop(p) => match handlers::file::handle_file_watch_stop(state, p) {
            Ok(r) => ("file_watch_stop", IpcResult::Ok { response: r }),
            Err(e) => ("file_watch_stop", IpcResult::Err { error: e }),
        },
        IpcRequest::FileWatchList => {
            let r = handlers::file::handle_file_watch_list(state);
            ("file_watch_list", IpcResult::Ok { response: r })
        }
        IpcRequest::PtyCommandStart(p) => {
            let env = p.environment.clone().unwrap_or_default();
            if matches!(env, EnvironmentSpec::Local) {
                match handlers::pty::handle_pty_command_start(state, p) {
                    Ok(r) => ("pty_command_start", IpcResult::Ok { response: r }),
                    Err(e) => ("pty_command_start", IpcResult::Err { error: e }),
                }
            } else {
                match EnvironmentRouter::route_request(state, &env, &req_env.request).await {
                    Ok(RouteOutcome::RunnerResponse(r)) => {
                        ("pty_command_start", IpcResult::Ok { response: *r })
                    }
                    Ok(RouteOutcome::Local) => {
                        match handlers::pty::handle_pty_command_start(state, p) {
                            Ok(r) => ("pty_command_start", IpcResult::Ok { response: r }),
                            Err(e) => ("pty_command_start", IpcResult::Err { error: e }),
                        }
                    }
                    Err(e) => (
                        "pty_command_start",
                        IpcResult::Err {
                            error: IpcError::new(IpcErrorCode::Internal, e.to_string()),
                        },
                    ),
                }
            }
        }
        IpcRequest::PtyCommandWriteStdin(p) => {
            match handlers::pty::handle_pty_command_write_stdin(state, p).await {
                Ok(r) => ("pty_command_write_stdin", IpcResult::Ok { response: r }),
                Err(e) => ("pty_command_write_stdin", IpcResult::Err { error: e }),
            }
        }
        IpcRequest::PtyCommandStop(p) => match handlers::pty::handle_pty_command_stop(state, p) {
            Ok(r) => ("pty_command_stop", IpcResult::Ok { response: r }),
            Err(e) => ("pty_command_stop", IpcResult::Err { error: e }),
        },
        IpcRequest::PtyCommandList => handlers::pty::dispatch_pty_command_list(state),
        IpcRequest::RuntimeState => {
            let r = handlers::runtime::handle_runtime_state(state);
            ("runtime_state", IpcResult::Ok { response: r })
        }
        IpcRequest::ProbeList => {
            let r = handlers::runtime::handle_probe_list(state);
            ("probe_list", IpcResult::Ok { response: r })
        }
        IpcRequest::ProbeStatus(p) => match handlers::runtime::handle_probe_status(state, p) {
            Ok(r) => ("probe_status", IpcResult::Ok { response: r }),
            Err(e) => ("probe_status", IpcResult::Err { error: e }),
        },
        // Graceful shutdown (E2). Flip the internal trigger and ACK
        // immediately. The ACK is written back to THIS connection
        // before any teardown: `trigger_shutdown` only flips a sticky
        // `watch` flag (it does not cancel this task), so the caller's
        // loop returns this envelope and `write_envelope` flushes the
        // ACK uninterrupted. The runtime's `run_ipc_server` is the one
        // awaiting `shutdown_notified`; it wakes AFTER this dispatch
        // returns, then drives `handle.shutdown()` (stop accepting +
        // drain in-flight) and pidfile removal.
        IpcRequest::Shutdown => {
            state.trigger_shutdown();
            (
                "shutdown",
                IpcResult::Ok {
                    response: IpcResponse::ShutdownAck { draining: true },
                },
            )
        }
    };
    // Audit one row per accepted request. The decision label reflects
    // whether the dispatcher produced an `Ok` response or a typed
    // error. Health is an audit-free peek: skip the persistent row so
    // `session list` polling does not spam the audit log.
    if method_name != "health" {
        let subject = identity_audit_subject(peer);
        let decision = if matches!(response_result, IpcResult::Ok { .. }) {
            "info"
        } else {
            "error"
        };
        emit_audit(state, method_name, &subject, decision, None, peer);
    }

    ResponseEnvelope {
        correlation_id: req_env.correlation_id,
        result: response_result,
    }
}

fn handle_system_discover(state: &Arc<DaemonState>) -> IpcResponse {
    IpcResponse::SystemDiscover(DiscoverResponse {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        mcp_spec: "2025-11-25".to_owned(),
        policy_profile: format!("{:?}", state.policy.profile),
        methods: vec![
            "system_discover".to_owned(),
            "health".to_owned(),
            "policy_status".to_owned(),
            "self_check".to_owned(),
            "bucket_events_since".to_owned(),
            "bucket_wait".to_owned(),
            "bucket_summary".to_owned(),
            "event_context".to_owned(),
            "command_start_combed".to_owned(),
            "command_status".to_owned(),
            "command_output_tail".to_owned(),
            "registry_search".to_owned(),
            "registry_get".to_owned(),
            "registry_upsert".to_owned(),
            "registry_test".to_owned(),
            "registry_activate".to_owned(),
            "registry_deactivate".to_owned(),
            "registry_list_active".to_owned(),
            "file_read_window".to_owned(),
            "file_search".to_owned(),
            "file_watch_start".to_owned(),
            "file_watch_stop".to_owned(),
            "file_watch_list".to_owned(),
            "pty_command_start".to_owned(),
            "pty_command_write_stdin".to_owned(),
            "pty_command_stop".to_owned(),
            "pty_command_list".to_owned(),
            "runtime_state".to_owned(),
            "probe_list".to_owned(),
            "probe_status".to_owned(),
        ],
    })
}

fn handle_policy_status(state: &Arc<DaemonState>) -> IpcResponse {
    IpcResponse::PolicyStatus(PolicyStatusResponse {
        profile: format!("{:?}", state.policy.profile),
        commands_deny_count: crate::policy::COMMANDS_DENY.len(),
        default_deny_path_suffix_count: crate::policy::DEFAULT_DENY_PATH_SUFFIXES.len(),
        file_window_bytes: state.config.limits.file_window_bytes,
        bucket_read_limit: state.config.limits.bucket_read_limit,
    })
}

fn handle_self_check(state: &Arc<DaemonState>) -> IpcResponse {
    // We avoid re-running the full TC36 self-check (it boots a fresh
    // DaemonState). Instead we synthesize a tiny report from the
    // already-bootstrapped state.
    let mut lines = vec![
        format!("data_dir: {}", state.config.daemon.data_dir.display()),
        format!("policy_profile: {:?}", state.policy.profile),
        format!("audit: persistent (TC35)"),
    ];
    match state.store.audit_count() {
        Ok(n) => lines.push(format!("audit_count: {n}")),
        Err(e) => lines.push(format!("audit_count: error: {e}")),
    }
    IpcResponse::SelfCheck(SelfCheckResponse {
        report: lines.join("\n"),
        failures: 0,
    })
}

/// Dispatch entry for alternate transports (named pipe on Windows).
pub async fn dispatch_envelope(
    state: &Arc<DaemonState>,
    boot: Instant,
    req_env: &RequestEnvelope,
    peer: &PeerIdentity,
) -> ResponseEnvelope {
    dispatch(state, boot, req_env, peer).await
}
