// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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
    DiscoverResponse, IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, PolicyCapsView,
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

/// The single source of truth mapping every [`IpcRequest`] variant to
/// its stable wire method name. M6 drift-proofing: `dispatch` derives
/// its audit label from this function, `handle_system_discover`
/// advertises exactly these names (via [`DISCOVERABLE_METHODS`]), and a
/// parity test asserts the three surfaces agree. Adding an `IpcRequest`
/// variant forces a new arm here (the match is exhaustive), and the
/// parity test then fails until the name is also added to
/// [`DISCOVERABLE_METHODS`] -- so a method can never silently exist in
/// the dispatcher while being absent from `system_discover`.
const fn method_name(req: &IpcRequest) -> &'static str {
    match req {
        IpcRequest::SystemDiscover => "system_discover",
        IpcRequest::Health => "health",
        IpcRequest::PolicyStatus => "policy_status",
        IpcRequest::SelfCheck => "self_check",
        IpcRequest::BucketEventsSince(_) => "bucket_events_since",
        IpcRequest::BucketWait(_) => "bucket_wait",
        IpcRequest::BucketSummary(_) => "bucket_summary",
        IpcRequest::EventContext(_) => "event_context",
        IpcRequest::CommandStartCombed(_) => "command_start_combed",
        IpcRequest::CommandStatus(_) => "command_status",
        IpcRequest::CommandStop(_) => "command_stop",
        IpcRequest::CommandOutputTail(_) => "command_output_tail",
        IpcRequest::ShellExec(_) => "shell_exec",
        IpcRequest::RegistrySearch(_) => "registry_search",
        IpcRequest::RegistryGet(_) => "registry_get",
        IpcRequest::RegistryUpsert(_) => "registry_upsert",
        IpcRequest::RegistryTest(_) => "registry_test",
        IpcRequest::RegistryActivate(_) => "registry_activate",
        IpcRequest::RegistryImportPack(_) => "registry_import_pack",
        IpcRequest::RegistryDeactivate(_) => "registry_deactivate",
        IpcRequest::RegistryDeactivateBulk(_) => "registry_deactivate_bulk",
        IpcRequest::RegistryListActive(_) => "registry_list_active",
        IpcRequest::RegistrySuggestFromSamples(_) => "registry_suggest_from_samples",
        IpcRequest::FileReadWindow(_) => "file_read_window",
        IpcRequest::FileSearch(_) => "file_search",
        IpcRequest::FileListDir(_) => "file_list_dir",
        IpcRequest::FileWrite(_) => "file_write",
        IpcRequest::FileWatchStart(_) => "file_watch_start",
        IpcRequest::FileWatchStop(_) => "file_watch_stop",
        IpcRequest::FileWatchList => "file_watch_list",
        IpcRequest::PtyCommandStart(_) => "pty_command_start",
        IpcRequest::PtyCommandWriteStdin(_) => "pty_command_write_stdin",
        IpcRequest::PtyCommandStop(_) => "pty_command_stop",
        IpcRequest::PtyCommandList => "pty_command_list",
        IpcRequest::ShellSessionStart(_) => "shell_session_start",
        IpcRequest::ShellSessionExec(_) => "shell_session_exec",
        IpcRequest::ShellSessionStatus(_) => "shell_session_status",
        IpcRequest::ShellSessionStop(_) => "shell_session_stop",
        IpcRequest::ShellSessionList => "shell_session_list",
        IpcRequest::WorkspaceSnapshotCreate(_) => "workspace_snapshot_create",
        IpcRequest::WorkspaceSnapshotApply(_) => "workspace_snapshot_apply",
        IpcRequest::RuntimeState(_) => "runtime_state",
        IpcRequest::ProbeList(_) => "probe_list",
        IpcRequest::ProbeStatus(_) => "probe_status",
        IpcRequest::AuditSince(_) => "audit_since",
        IpcRequest::SubscriptionOpen(_) => "subscription_open",
        IpcRequest::SubscriptionPull(_) => "subscription_pull",
        IpcRequest::SubscriptionList(_) => "subscription_list",
        IpcRequest::SubscriptionClose(_) => "subscription_close",
        IpcRequest::SubscriptionSeek(_) => "subscription_seek",
        IpcRequest::Shutdown => "shutdown",
    }
}

/// Every callable method name advertised by `system_discover`. This is
/// the authoritative list `handle_system_discover` returns. It MUST
/// equal the set of names [`method_name`] can produce across all
/// `IpcRequest` variants -- the `system_discover_methods_match_dispatch`
/// parity test enforces both directions, so a method added to the
/// dispatcher but forgotten here (or vice-versa) fails the test.
pub(crate) const DISCOVERABLE_METHODS: &[&str] = &[
    "system_discover",
    "health",
    "policy_status",
    "self_check",
    "bucket_events_since",
    "bucket_wait",
    "bucket_summary",
    "event_context",
    "command_start_combed",
    "command_status",
    "command_stop",
    "command_output_tail",
    "shell_exec",
    "registry_search",
    "registry_get",
    "registry_upsert",
    "registry_test",
    "registry_activate",
    "registry_import_pack",
    "registry_deactivate",
    "registry_deactivate_bulk",
    "registry_list_active",
    "file_read_window",
    "file_search",
    "file_list_dir",
    "file_write",
    "file_watch_start",
    "file_watch_stop",
    "file_watch_list",
    "pty_command_start",
    "pty_command_write_stdin",
    "pty_command_stop",
    "pty_command_list",
    "shell_session_start",
    "shell_session_exec",
    "shell_session_status",
    "shell_session_stop",
    "shell_session_list",
    "workspace_snapshot_create",
    "workspace_snapshot_apply",
    "runtime_state",
    "probe_list",
    "probe_status",
    "audit_since",
    "subscription_open",
    "subscription_pull",
    "subscription_list",
    "subscription_close",
    "subscription_seek",
    "shutdown",
];

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
    // M6 (drift-proofing): the method-name label is derived ONCE from
    // the shared `method_name` authority -- the SAME function
    // `handle_system_discover` advertises and the parity test checks.
    // Each match arm below now only produces the `IpcResult`; it can no
    // longer carry a divergent name literal, so the audit label, the
    // advertised `methods` list, and the dispatch table cannot drift.
    let method_name = method_name(&req_env.request);
    let response_result = match &req_env.request {
        IpcRequest::SystemDiscover => {
            let r = handle_system_discover(state);
            IpcResult::Ok { response: r }
        }
        IpcRequest::Health => {
            let r = IpcResponse::Health {
                uptime_secs: boot.elapsed().as_secs(),
                idle_secs: Some(state.idle_secs()),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            };
            IpcResult::Ok { response: r }
        }
        IpcRequest::PolicyStatus => {
            let r = handle_policy_status(state);
            IpcResult::Ok { response: r }
        }
        IpcRequest::SelfCheck => {
            let r = handle_self_check(state).await;
            IpcResult::Ok { response: r }
        }
        IpcRequest::BucketEventsSince(p) => {
            match handlers::bucket::handle_bucket_events_since(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::BucketWait(p) => match handlers::bucket::handle_bucket_wait(state, p).await {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::BucketSummary(p) => match handlers::bucket::handle_bucket_summary(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::EventContext(p) => match handlers::bucket::handle_event_context(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::CommandStartCombed(p) => {
            let env = p.environment.clone().unwrap_or_default();
            if matches!(env, EnvironmentSpec::Local) {
                match handlers::command::handle_command_start_combed(state, p, peer) {
                    Ok(r) => IpcResult::Ok { response: r },
                    Err(e) => IpcResult::Err { error: e },
                }
            } else {
                match EnvironmentRouter::route_request(state, &env, &req_env.request).await {
                    Ok(RouteOutcome::RunnerResponse(r)) => IpcResult::Ok { response: *r },
                    Ok(RouteOutcome::Local) => {
                        match handlers::command::handle_command_start_combed(state, p, peer) {
                            Ok(r) => IpcResult::Ok { response: r },
                            Err(e) => IpcResult::Err { error: e },
                        }
                    }
                    Err(e) => IpcResult::Err {
                        error: IpcError::new(IpcErrorCode::Internal, e.to_string()),
                    },
                }
            }
        }
        IpcRequest::CommandStatus(p) => match handlers::command::handle_command_status(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::CommandStop(p) => {
            match handlers::command::handle_command_stop(state, p, peer) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::CommandOutputTail(p) => {
            match handlers::command::handle_command_output_tail(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        // Shell-lane start (TC49). Local-only: the shell lane carries no
        // `environment` (no remote routing). `handle_shell_exec` is SYNC
        // (the gated `ShellRuntime::exec` never awaits), so it is called
        // inline without `.await`.
        IpcRequest::ShellExec(p) => match handlers::command::handle_shell_exec(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::RegistrySearch(p) => match handlers::registry::handle_registry_search(state, p)
        {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::RegistryGet(p) => match handlers::registry::handle_registry_get(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::RegistryUpsert(p) => match handlers::registry::handle_registry_upsert(state, p)
        {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::RegistryTest(p) => match handlers::registry::handle_registry_test(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::RegistryActivate(p) => {
            match handlers::registry::handle_registry_activate(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::RegistryImportPack(p) => {
            match handlers::registry::handle_registry_import_pack(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::RegistryDeactivate(p) => {
            match handlers::registry::handle_registry_deactivate(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::RegistryDeactivateBulk(p) => {
            match handlers::registry::handle_registry_deactivate_bulk(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::RegistryListActive(p) => {
            let r = handlers::registry::handle_registry_list_active(state, p);
            IpcResult::Ok { response: r }
        }
        IpcRequest::RegistrySuggestFromSamples(p) => {
            // Pure heuristic suggestion; never activates/persists.
            let r = handlers::registry::handle_registry_suggest_from_samples(p);
            IpcResult::Ok { response: r }
        }
        IpcRequest::FileReadWindow(p) => match handlers::file::handle_file_read_window(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::FileSearch(p) => match handlers::file::handle_file_search(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::FileListDir(p) => match handlers::file::handle_file_list_dir(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::FileWrite(p) => match handlers::file::handle_file_write(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::FileWatchStart(p) => match handlers::file::handle_file_watch_start(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::FileWatchStop(p) => match handlers::file::handle_file_watch_stop(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::FileWatchList => {
            let r = handlers::file::handle_file_watch_list(state);
            IpcResult::Ok { response: r }
        }
        IpcRequest::PtyCommandStart(p) => {
            let env = p.environment.clone().unwrap_or_default();
            if matches!(env, EnvironmentSpec::Local) {
                match handlers::pty::handle_pty_command_start(state, p) {
                    Ok(r) => IpcResult::Ok { response: r },
                    Err(e) => IpcResult::Err { error: e },
                }
            } else {
                match EnvironmentRouter::route_request(state, &env, &req_env.request).await {
                    Ok(RouteOutcome::RunnerResponse(r)) => IpcResult::Ok { response: *r },
                    Ok(RouteOutcome::Local) => {
                        match handlers::pty::handle_pty_command_start(state, p) {
                            Ok(r) => IpcResult::Ok { response: r },
                            Err(e) => IpcResult::Err { error: e },
                        }
                    }
                    Err(e) => IpcResult::Err {
                        error: IpcError::new(IpcErrorCode::Internal, e.to_string()),
                    },
                }
            }
        }
        IpcRequest::PtyCommandWriteStdin(p) => {
            match handlers::pty::handle_pty_command_write_stdin(state, p).await {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::PtyCommandStop(p) => match handlers::pty::handle_pty_command_stop(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::PtyCommandList => {
            // `dispatch_pty_command_list` returns `(name, IpcResult)` for
            // historical reasons; the name it returns MUST match
            // `method_name` (the parity test guards this). Drop its name
            // and keep only the result so the single `method_name`
            // authority above wins.
            let (_name, r) = handlers::pty::dispatch_pty_command_list(state);
            r
        }
        // Session lane (P1 / TC50). `_start` carries the peer so the
        // handler can resolve a redacted audit subject before the PTY
        // runtime's `start_session` writes the `shell_session_start` audit
        // row + spawns behind the `SessionStart` policy gate. On non-unix
        // the handlers are stubs that return `UnsupportedPlatform`.
        IpcRequest::ShellSessionStart(p) => {
            #[cfg(unix)]
            let r = handlers::session::handle_shell_session_start(state, p, peer);
            #[cfg(not(unix))]
            let r = handlers::session::handle_shell_session_start(state, p);
            match r {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::ShellSessionExec(p) => {
            match handlers::session::handle_shell_session_exec(state, p).await {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::ShellSessionStatus(p) => {
            match handlers::session::handle_shell_session_status(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::ShellSessionStop(p) => {
            match handlers::session::handle_shell_session_stop(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::ShellSessionList => match handlers::session::handle_shell_session_list(state) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::WorkspaceSnapshotCreate(p) => {
            match handlers::session::handle_workspace_snapshot_create(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::WorkspaceSnapshotApply(p) => {
            match handlers::session::handle_workspace_snapshot_apply(state, p).await {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::RuntimeState(p) => {
            let r = handlers::runtime::handle_runtime_state(state, p);
            IpcResult::Ok { response: r }
        }
        IpcRequest::ProbeList(p) => {
            let r = handlers::runtime::handle_probe_list(state, p);
            IpcResult::Ok { response: r }
        }
        IpcRequest::ProbeStatus(p) => match handlers::runtime::handle_probe_status(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::AuditSince(p) => match handlers::audit::handle_audit_since(state, p) {
            Ok(r) => IpcResult::Ok { response: r },
            Err(e) => IpcResult::Err { error: e },
        },
        IpcRequest::SubscriptionOpen(p) => {
            match handlers::subscription::handle_subscription_open(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::SubscriptionPull(p) => {
            match handlers::subscription::handle_subscription_pull(state, p).await {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
        IpcRequest::SubscriptionList(p) => {
            let r = handlers::subscription::handle_subscription_list(state, p);
            IpcResult::Ok { response: r }
        }
        IpcRequest::SubscriptionClose(p) => {
            let r = handlers::subscription::handle_subscription_close(state, p);
            IpcResult::Ok { response: r }
        }
        IpcRequest::SubscriptionSeek(p) => {
            match handlers::subscription::handle_subscription_seek(state, p) {
                Ok(r) => IpcResult::Ok { response: r },
                Err(e) => IpcResult::Err { error: e },
            }
        }
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
            IpcResult::Ok {
                response: IpcResponse::ShutdownAck { draining: true },
            }
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
        // M6: advertise EXACTLY the shared method authority. Previously a
        // hand-maintained literal list that drifted from the dispatcher
        // (it omitted registry_import_pack / subscription_seek /
        // shutdown). Deriving from `DISCOVERABLE_METHODS` -- which the
        // parity test pins to `method_name` over every IpcRequest
        // variant -- makes future drift a compile/test failure.
        methods: DISCOVERABLE_METHODS
            .iter()
            .map(|m| (*m).to_owned())
            .collect(),
    })
}

fn handle_policy_status(state: &Arc<DaemonState>) -> IpcResponse {
    let caps = state.policy.resolved_caps();
    IpcResponse::PolicyStatus(PolicyStatusResponse {
        profile: format!("{:?}", state.policy.profile),
        commands_deny_count: crate::policy::COMMANDS_DENY.len(),
        default_deny_path_suffix_count: crate::policy::DEFAULT_DENY_PATH_SUFFIXES.len(),
        file_window_bytes: state.config.limits.file_window_bytes,
        bucket_read_limit: state.config.limits.bucket_read_limit,
        // Surface the RESOLVED per-call caps (POLICY.md section 4.1): the same
        // four flags the engine evaluates against, including any preset ON by
        // `full_access`. `PolicyCaps` is daemon-private; map field-for-field
        // into the wire view.
        caps: PolicyCapsView {
            allow_shell: caps.allow_shell,
            allow_session: caps.allow_session,
            allow_privileged: caps.allow_privileged,
            allow_remote: caps.allow_remote,
        },
    })
}

/// Outcome of the TC-5 self-check spawn probe.
///
/// `failed` drives the `failures` counter on the `SelfCheckResponse`;
/// `line` is a single human-readable report line appended to the
/// self-check report. A policy-gated SKIP is `failed: false` -- never a
/// failure.
pub struct SpawnProbeOutcome {
    pub failed: bool,
    pub line: String,
}

/// Mint a fresh, process-unique, monotonic dedup nonce for a self-check
/// spawn probe (TC-5). Mirrors `mcp::tools::fresh_dedup_nonce` so two
/// back-to-back self_checks get DISTINCT nonces and therefore DISTINCT
/// jobs -- the TC-2 in-flight dedup guard must NOT collapse them. The
/// value need not be unpredictable; it only has to differ per call.
fn fresh_selfcheck_nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = NONCE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("selfcheck-{}-{}", std::process::id(), seq)
}

/// TC-5 self-check spawn probe.
///
/// Runs a REAL, policy-gated `command_start_combed` round-trip into the
/// daemon's ONE cached immortal self-check bucket, polled to terminal
/// state. This is what makes `self_check` unable to return a hardcoded
/// false-green: a broken command pipeline surfaces here as `failed: true`.
///
/// Extracted from [`handle_self_check`] so a test can force a failing argv
/// directly without driving the whole IPC path.
///
/// Lock discipline: the `parking_lot::Mutex` guarding the cached bucket is
/// NEVER held across an `.await`. It is read in one scoped block (the guard
/// is dropped before any await) and written in another scoped block (also
/// dropped before the poll loop's `.await`). The only `.await` in the poll
/// loop is `tokio::time::sleep`, with no lock held.
pub async fn selfcheck_spawn_probe(
    state: &Arc<DaemonState>,
    argv: Vec<String>,
    cwd: std::path::PathBuf,
) -> SpawnProbeOutcome {
    // 1. Pre-evaluate policy. A Deny is a SKIP, never a failure: the
    //    read_only_observer profile, repo_only without a repo_root, or an
    //    allow-list miss must NOT turn a correct policy decision into a
    //    false RED.
    let verdict = state
        .policy
        .evaluate(&crate::policy::PolicyAction::CommandStart {
            argv: &argv,
            cwd: &cwd,
        });
    if verdict.decision == crate::policy::PolicyDecision::Deny {
        return SpawnProbeOutcome {
            failed: false,
            line: format!("spawn probe skipped: {}", verdict.reason),
        };
    }
    // A probe-kind deny ([policy.probes] deny_kinds = ["command"], or a
    // non-empty allow_kinds without it) is likewise a SKIP, not a RED: the
    // real spawn below funnels through start_combed_inner, which evaluates the
    // same ProbeCreate{kind:"command"} gate and would deny. Mirror it here so a
    // configured probe-kind deny reports as a clean skip, not a false self-check
    // failure (TC22 A2; consistent with the CommandStart skip above).
    let probe_verdict = state
        .policy
        .evaluate(&crate::policy::PolicyAction::ProbeCreate { kind: "command" });
    if probe_verdict.decision == crate::policy::PolicyDecision::Deny {
        return SpawnProbeOutcome {
            failed: false,
            line: format!("spawn probe skipped: {}", probe_verdict.reason),
        };
    }

    // 2. Read the cached bucket WITHOUT holding the lock across an await.
    //    `Option<BucketId>` is `Copy`, so the guard is dropped at the end
    //    of this scoped block.
    let reuse = { *state.selfcheck_bucket.lock() };

    // 3. Build the start request. A fresh nonce per call defeats the TC-2
    //    in-flight dedup guard so two back-to-back self_checks spawn two
    //    DISTINCT jobs.
    let req = crate::command::CommandStartRequest {
        argv,
        cwd: Some(cwd),
        env: vec![],
        bucket_config: None,
        rules: vec![],
        grace: None,
        tag: Some("selfcheck".to_owned()),
        dedup_nonce: Some(fresh_selfcheck_nonce()),
        peer_discriminator: None,
        // TC-B1: default-on; self_check output is trivial but consistent.
        strip_ansi: true,
    };

    // 4. Spawn into the (reused-or-fresh) bucket.
    let resp = match state.command.start_combed_reusing(req, reuse) {
        Err(e) => {
            return SpawnProbeOutcome {
                failed: true,
                line: format!("spawn probe FAILED: start error: {e}"),
            };
        }
        Ok(resp) => resp,
    };

    // On the first (fresh) spawn, cache the bucket id so every later
    // self_check reuses it and `bucket_count` grows by exactly one. The
    // lock is held only for this scoped store and dropped BEFORE the poll
    // loop below; the `is_none()` re-check tolerates a racing first probe.
    if reuse.is_none() {
        let mut s = state.selfcheck_bucket.lock();
        if s.is_none() {
            *s = Some(resp.bucket_id);
        }
    }

    // 5. Poll the job to terminal state under a ~2s wall-clock budget. The
    //    ONLY await is the sleep below; no lock is held across it.
    let budget = std::time::Duration::from_secs(2);
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        match state.command.status(resp.job_id) {
            Ok(st) => {
                use terminal_commander_core::JobState;
                if matches!(
                    st.state,
                    JobState::Exited | JobState::Failed | JobState::Cancelled
                ) {
                    // Terminal. `JobState::Exited` is only ever reached on a
                    // clean exit with `exit_code: Some(0)` -- the ledger maps a
                    // nonzero code OR any signal to `Failed` (see
                    // JobManager::finish) -- so a healthy probe is exactly an
                    // Exited job with code 0. A Failed/Cancelled terminal or a
                    // nonzero code is a real failure.
                    let healthy = matches!(st.state, JobState::Exited) && st.exit_code == Some(0);
                    if healthy {
                        return SpawnProbeOutcome {
                            failed: false,
                            line: format!(
                                "spawn probe: ok (terminal {:?} exit {:?})",
                                st.state, st.exit_code
                            ),
                        };
                    }
                    return SpawnProbeOutcome {
                        failed: true,
                        line: format!(
                            "spawn probe FAILED: terminal {:?} exit {:?}",
                            st.state, st.exit_code
                        ),
                    };
                }
            }
            Err(e) => {
                return SpawnProbeOutcome {
                    failed: true,
                    line: format!("spawn probe FAILED: status error: {e}"),
                };
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return SpawnProbeOutcome {
                failed: true,
                line: "spawn probe FAILED: not terminal within 2s".to_owned(),
            };
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
async fn handle_self_check(state: &Arc<DaemonState>) -> IpcResponse {
    // We avoid re-running the full TC36 self-check (it boots a fresh
    // DaemonState). Instead we synthesize a report from the
    // already-bootstrapped state AND run a REAL spawn probe (TC-5) so
    // this can no longer return a hardcoded false-green.
    let mut lines = vec![
        format!("data_dir: {}", state.config.daemon.data_dir.display()),
        format!("policy_profile: {:?}", state.policy.profile),
        format!("audit: persistent (TC35)"),
    ];
    match state.store.audit_count() {
        Ok(n) => lines.push(format!("audit_count: {n}")),
        Err(e) => lines.push(format!("audit_count: error: {e}")),
    }

    let mut failures = 0u32;

    // TC-5: real, profile-gated command-spawn round-trip into the cached
    // immortal self-check bucket. The probe target is THIS binary's hidden
    // `selfcheck-noop` leaf, which exits 0 immediately. `cwd` is the
    // repo_root when configured (so repo_only containment does not false-
    // skip) else the daemon data dir.
    match std::env::current_exe() {
        Ok(exe) => {
            let argv = vec![
                exe.to_string_lossy().into_owned(),
                "selfcheck-noop".to_owned(),
            ];
            let cwd = state
                .config
                .policy
                .repo_root
                .clone()
                .unwrap_or_else(|| state.config.daemon.data_dir.clone());
            let outcome = selfcheck_spawn_probe(state, argv, cwd).await;
            if outcome.failed {
                failures += 1;
            }
            lines.push(outcome.line);
        }
        Err(e) => {
            // current_exe is unavailable: SKIP, never a failure -- the
            // probe target cannot be resolved, but the daemon itself is fine.
            lines.push(format!("spawn probe skipped: current_exe unavailable: {e}"));
        }
    }

    IpcResponse::SelfCheck(SelfCheckResponse {
        report: lines.join("\n"),
        failures,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::{
        AuditSinceParams, BucketEventsSinceParams, BucketSummaryParams, BucketWaitParams,
        CommandOutputTailParams, CommandStartParams, CommandStatusParams, CommandStopParams,
        EventContextParams, FileListDirParams, FileReadWindowParams, FileSearchParams,
        FileWatchStartParams, FileWatchStopParams, FileWriteParams, ListLimitParams,
        ProbeStatusParams, PtyCommandStartParams, PtyCommandStopParams, PtyCommandWriteStdinParams,
        RegistryActivateParams, RegistryDeactivateBulkParams, RegistryDeactivateParams,
        RegistryGetParams, RegistryImportPackParams, RegistrySearchParams, RegistryTestParams,
        RegistryUpsertParams, ShellExecParams, ShellSessionExecParams, ShellSessionStartParams,
        ShellSessionStatusParams, ShellSessionStopParams, SubscriptionCloseParams,
        SubscriptionListParams, SubscriptionOpenParams, SubscriptionPredicate,
        SubscriptionPullParams, SubscriptionSeekParams, SubscriptionSourceSel,
        WorkspaceSnapshotApplyParams, WorkspaceSnapshotCreateParams,
    };
    use std::collections::BTreeSet;
    use terminal_commander_core::{
        BucketId, ContextHint, EventId, JobId, ProbeId, RuleDefinition, RuleStatus, RuleType,
        Severity,
    };

    fn minimal_rule() -> RuleDefinition {
        RuleDefinition {
            id: "parity.rule".to_owned(),
            version: 1,
            kind: RuleType::Keyword,
            status: RuleStatus::Active,
            severity: Severity::Info,
            event_kind: "test".to_owned(),
            stream: None,
            description: None,
            pattern: None,
            keywords: Some(vec!["x".to_owned()]),
            captures: vec![],
            summary_template: "x".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    /// Every [`IpcRequest`] variant, constructed with the cheapest valid
    /// payload. Adding a variant to the enum without adding it here makes
    /// `method_name`'s exhaustive match fail to compile FIRST; the length
    /// cross-check in the parity test catches the reverse (a method added
    /// to `DISCOVERABLE_METHODS` with no matching variant here).
    #[allow(clippy::too_many_lines)] // one line per IpcRequest variant
    fn all_request_variants() -> Vec<IpcRequest> {
        vec![
            IpcRequest::SystemDiscover,
            IpcRequest::Health,
            IpcRequest::PolicyStatus,
            IpcRequest::SelfCheck,
            IpcRequest::BucketEventsSince(BucketEventsSinceParams {
                bucket_id: BucketId::new(),
                cursor: 0,
                severity_min: None,
                kind_filter: None,
                limit: None,
            }),
            IpcRequest::BucketWait(BucketWaitParams {
                bucket_id: BucketId::new(),
                cursor: 0,
                severity_min: None,
                kind_filter: None,
                limit: None,
                timeout_ms: None,
            }),
            IpcRequest::BucketSummary(BucketSummaryParams {
                bucket_id: BucketId::new(),
            }),
            IpcRequest::EventContext(EventContextParams {
                bucket_id: BucketId::new(),
                event_id: EventId::new(),
                before: None,
                after: None,
                max_bytes: None,
            }),
            IpcRequest::CommandStartCombed(CommandStartParams {
                environment: None,
                argv: vec!["true".to_owned()],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                grace_ms: None,
                tag: None,
                dedup_nonce: None,
                strip_ansi: true,
            }),
            IpcRequest::CommandStatus(CommandStatusParams {
                job_id: JobId::new(),
            }),
            IpcRequest::CommandStop(CommandStopParams {
                job_id: JobId::new(),
            }),
            IpcRequest::CommandOutputTail(CommandOutputTailParams {
                job_id: JobId::new(),
                max_lines: 1,
                max_bytes: 1,
            }),
            IpcRequest::ShellExec(ShellExecParams {
                shell_line: "echo a | wc -c".to_owned(),
                shell: None,
                cwd: None,
                env: vec![],
                rules: vec![],
                bucket_config: None,
                tag: None,
            }),
            IpcRequest::RegistrySearch(RegistrySearchParams {
                query: "x".to_owned(),
                limit: None,
            }),
            IpcRequest::RegistryGet(RegistryGetParams {
                rule_id: "x".to_owned(),
                version: None,
            }),
            IpcRequest::RegistryUpsert(RegistryUpsertParams {
                definition: minimal_rule(),
            }),
            IpcRequest::RegistryTest(RegistryTestParams {
                rule_id: "x".to_owned(),
                version: None,
                samples: vec![],
            }),
            IpcRequest::RegistryActivate(RegistryActivateParams {
                rule_id: "x".to_owned(),
                version: None,
                scope: None,
            }),
            IpcRequest::RegistryImportPack(RegistryImportPackParams {
                pack: "cargo".to_owned(),
                activate: false,
                scope: None,
            }),
            IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
                rule_id: "x".to_owned(),
                version: 1,
                scope: None,
            }),
            IpcRequest::RegistryDeactivateBulk(RegistryDeactivateBulkParams {
                pack: None,
                rule_ids: Some(vec!["x".to_owned()]),
                scope: terminal_commander_core::ActivationScope::Global,
            }),
            IpcRequest::RegistryListActive(ListLimitParams { limit: None }),
            IpcRequest::FileReadWindow(FileReadWindowParams {
                path: std::path::PathBuf::from("/x"),
                start_line: None,
                max_lines: None,
                max_bytes: None,
            }),
            IpcRequest::FileSearch(FileSearchParams {
                path: std::path::PathBuf::from("/x"),
                query: "x".to_owned(),
                case_insensitive: None,
                max_matches: None,
                max_snippet_bytes: None,
            }),
            IpcRequest::FileListDir(FileListDirParams {
                path: "/x".to_owned(),
                max_entries: None,
            }),
            IpcRequest::FileWrite(FileWriteParams {
                path: std::path::PathBuf::from("/x"),
                content: "x".to_owned(),
                create_dirs: false,
                append: false,
            }),
            IpcRequest::FileWatchStart(FileWatchStartParams {
                path: std::path::PathBuf::from("/x"),
                bucket_config: None,
                rules: vec![],
                follow_from_beginning: None,
                tag: None,
            }),
            IpcRequest::FileWatchStop(FileWatchStopParams {
                watch_id: JobId::new(),
            }),
            IpcRequest::FileWatchList,
            IpcRequest::PtyCommandStart(PtyCommandStartParams {
                environment: None,
                argv: vec!["true".to_owned()],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                rows: None,
                cols: None,
                tag: None,
            }),
            IpcRequest::PtyCommandWriteStdin(PtyCommandWriteStdinParams {
                job_id: JobId::new(),
                bytes: String::new(),
            }),
            IpcRequest::PtyCommandStop(PtyCommandStopParams {
                job_id: JobId::new(),
            }),
            IpcRequest::PtyCommandList,
            IpcRequest::ShellSessionStart(ShellSessionStartParams {
                shell: None,
                cwd: None,
                env: vec![],
                rules: vec![],
                bucket_config: None,
                tag: None,
            }),
            IpcRequest::ShellSessionExec(ShellSessionExecParams {
                session_id: terminal_commander_core::SessionId::new(),
                line: "pwd".to_owned(),
                cursor: 0,
                wait_ms: None,
            }),
            IpcRequest::ShellSessionStatus(ShellSessionStatusParams {
                session_id: terminal_commander_core::SessionId::new(),
            }),
            IpcRequest::ShellSessionStop(ShellSessionStopParams {
                session_id: terminal_commander_core::SessionId::new(),
            }),
            IpcRequest::ShellSessionList,
            IpcRequest::WorkspaceSnapshotCreate(WorkspaceSnapshotCreateParams {
                session_id: terminal_commander_core::SessionId::new(),
                name: None,
            }),
            IpcRequest::WorkspaceSnapshotApply(WorkspaceSnapshotApplyParams {
                snapshot_id: "snap_x".to_owned(),
                session_id: terminal_commander_core::SessionId::new(),
            }),
            IpcRequest::RuntimeState(ListLimitParams { limit: None }),
            IpcRequest::ProbeList(ListLimitParams { limit: None }),
            IpcRequest::ProbeStatus(ProbeStatusParams {
                probe_id: ProbeId::new(),
            }),
            IpcRequest::AuditSince(AuditSinceParams {
                cursor: 0,
                action_filter: None,
                decision_filter: None,
                limit: None,
            }),
            IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                predicate: SubscriptionPredicate {
                    severity_min: None,
                    kind: None,
                    sources: SubscriptionSourceSel::All,
                    tag: None,
                },
            }),
            IpcRequest::SubscriptionPull(SubscriptionPullParams {
                sub_id: "x".to_owned(),
                max: None,
                timeout_ms: None,
            }),
            IpcRequest::SubscriptionList(SubscriptionListParams { limit: None }),
            IpcRequest::SubscriptionClose(SubscriptionCloseParams {
                sub_id: "x".to_owned(),
            }),
            IpcRequest::SubscriptionSeek(SubscriptionSeekParams {
                sub_id: "x".to_owned(),
                bucket_id: BucketId::new(),
                seq: 0,
            }),
            IpcRequest::Shutdown,
        ]
    }

    /// M6 parity guard: the method names the dispatcher can produce
    /// (`method_name` over every variant) MUST equal exactly the names
    /// `system_discover` advertises (`DISCOVERABLE_METHODS`). Drift in
    /// EITHER direction fails this test, so a method can never be live in
    /// the dispatcher while invisible to `system_discover`, nor advertised
    /// without a real dispatch arm.
    #[test]
    fn system_discover_methods_match_dispatch() {
        let dispatch_names: BTreeSet<&'static str> =
            all_request_variants().iter().map(method_name).collect();
        let advertised: BTreeSet<&'static str> = DISCOVERABLE_METHODS.iter().copied().collect();

        let missing_from_discover: Vec<&&str> = dispatch_names.difference(&advertised).collect();
        assert!(
            missing_from_discover.is_empty(),
            "dispatch arms missing from system_discover.methods: {missing_from_discover:?}"
        );
        let advertised_without_arm: Vec<&&str> = advertised.difference(&dispatch_names).collect();
        assert!(
            advertised_without_arm.is_empty(),
            "system_discover.methods names with no dispatch arm: {advertised_without_arm:?}"
        );

        // Cross-checks that catch a forgotten test-vec entry: the
        // advertised list has no duplicates, and the number of distinct
        // dispatch names equals the advertised count (so a variant
        // omitted from `all_request_variants` -- which would otherwise
        // shrink `dispatch_names` -- is caught here once it is advertised).
        assert_eq!(
            advertised.len(),
            DISCOVERABLE_METHODS.len(),
            "DISCOVERABLE_METHODS contains duplicate method names"
        );
        assert_eq!(
            dispatch_names.len(),
            DISCOVERABLE_METHODS.len(),
            "dispatch name count diverged from advertised count"
        );
    }

    /// The three specific names the audit flagged as historically missing
    /// must now be advertised. A focused regression for M6 so the exact
    /// drift that was found cannot silently return.
    #[test]
    fn system_discover_advertises_previously_missing_methods() {
        for m in ["registry_import_pack", "subscription_seek", "shutdown"] {
            assert!(
                DISCOVERABLE_METHODS.contains(&m),
                "system_discover must advertise '{m}'"
            );
        }
    }
}
