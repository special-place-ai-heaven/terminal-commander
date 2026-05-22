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

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use terminal_commander_store::AuditEntry;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;

use crate::audit::AuditSink;
use crate::command::{CommandError, CommandStartRequest};
use crate::ipc::peer::{self, PeerCred};
use crate::ipc::protocol::{
    BucketEventsSinceParams, BucketEventsSinceResponse, BucketSummaryParams, BucketSummaryResponse,
    BucketWaitParams, BucketWaitResponse, CommandStartParams, CommandStatusParams,
    ContextUnavailableReason, DEFAULT_BUCKET_READ_LIMIT, DEFAULT_CONTEXT_AFTER,
    DEFAULT_CONTEXT_BEFORE, DEFAULT_FILE_READ_BYTES, DEFAULT_FILE_READ_LINES,
    DEFAULT_FILE_SEARCH_MATCHES, DEFAULT_FILE_SEARCH_SNIPPET_BYTES, DiscoverResponse,
    EventContextParams, EventContextResponse, FileLine, FileReadWindowParams,
    FileReadWindowResponse, FileSearchMatch, FileSearchParams, FileSearchResponse,
    FileWatchListEntry, FileWatchListResponse, FileWatchStartParams, FileWatchStartResponse,
    FileWatchStopParams, FileWatchStopResponse, IpcContextFrame, IpcError, IpcErrorCode,
    IpcRequest, IpcResponse, IpcResult, MAX_BUCKET_READ_LIMIT, MAX_COMMAND_ENV_ITEMS,
    MAX_COMMAND_INLINE_RULES, MAX_CONTEXT_BYTES, MAX_CONTEXT_FRAMES, MAX_FILE_READ_BYTES,
    MAX_FILE_READ_LINES, MAX_FILE_SEARCH_MATCHES, MAX_FILE_SEARCH_SCAN_BYTES,
    MAX_FILE_SEARCH_SNIPPET_BYTES, MAX_FRAME_BYTES, MAX_PTY_ARGV_ITEMS, MAX_PTY_STDIN_BYTES,
    MAX_REGISTRY_TEST_SAMPLE_BYTES, MAX_REGISTRY_TEST_SAMPLES, PolicyStatusResponse,
    PtyCommandListEntry, PtyCommandListResponse, PtyCommandStartParams, PtyCommandStartResponse,
    PtyCommandStopParams, PtyCommandStopResponse, PtyCommandWriteStdinParams,
    PtyCommandWriteStdinResponse, RegistryActivateParams, RegistryActivateResponse,
    RegistryActiveEntry, RegistryDeactivateParams, RegistryDeactivateResponse, RegistryGetParams,
    RegistryGetResponse, RegistryListActiveResponse, RegistrySearchHit, RegistrySearchParams,
    RegistrySearchResponse, RegistryTestMatch, RegistryTestParams, RegistryTestResponse,
    RegistryUpsertParams, RegistryUpsertResponse, RequestEnvelope, ResponseEnvelope,
    SelfCheckResponse, SeverityHistogram, decode_payload, encode_frame,
};
use crate::state::DaemonState;

/// Handle returned from [`IpcServer::spawn`]. Drop the handle to
/// signal the accept loop to stop; call [`ServerHandle::shutdown`]
/// to await orderly shutdown.
///
/// Backed by `tokio::sync::watch::channel(false)`: the sticky
/// "shutdown requested" flag survives the race between
/// [`IpcServer::spawn`] returning and the accept loop's first poll.
pub struct ServerHandle {
    shutdown_tx: watch::Sender<bool>,
    join: Option<tokio::task::JoinHandle<()>>,
    socket_path: PathBuf,
}

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

/// IPC server. Owns the listener, the daemon state, and the boot
/// timestamp used by the `health` method.
pub struct IpcServer {
    state: Arc<DaemonState>,
    boot: Instant,
    socket_path: PathBuf,
}

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

async fn accept_loop(
    listener: UnixListener,
    state: Arc<DaemonState>,
    boot: Instant,
    mut shutdown: watch::Receiver<bool>,
) {
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
                // Either way: exit. (We do not distinguish; both mean
                // "no more requests should be accepted".)
                if res.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            res = listener.accept() => {
                match res {
                    Ok((stream, _addr)) => {
                        let state = Arc::clone(&state);
                        let shutdown_for_conn = shutdown.clone();
                        tokio::spawn(async move {
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
}

async fn handle_connection(
    mut stream: UnixStream,
    state: Arc<DaemonState>,
    boot: Instant,
    mut shutdown: watch::Receiver<bool>,
) {
    let peer = peer::resolve(&stream);
    // Linux/WSL: fail-closed when peer creds are missing.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        if peer.is_none() {
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
                None,
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
        let subject = peer.map_or_else(|| "unknown_peer".to_owned(), |p| p.to_audit_string());
        emit_audit(&state, "ipc_connect", &subject, "info", None, peer);
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
                        let resp = dispatch(&state, boot, &req_env, peer).await;
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

#[allow(clippy::large_enum_variant)]
enum ReadOutcome {
    Ok(RequestEnvelope),
    Err(IpcError),
    Eof,
}

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
    peer: Option<PeerCred>,
) -> ResponseEnvelope {
    let (method_name, response_result) = match &req_env.request {
        IpcRequest::SystemDiscover => {
            let r = handle_system_discover(state);
            ("system_discover", IpcResult::Ok { response: r })
        }
        IpcRequest::Health => {
            let r = IpcResponse::Health {
                uptime_secs: boot.elapsed().as_secs(),
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
        IpcRequest::BucketEventsSince(p) => match handle_bucket_events_since(state, p) {
            Ok(r) => ("bucket_events_since", IpcResult::Ok { response: r }),
            Err(e) => ("bucket_events_since", IpcResult::Err { error: e }),
        },
        IpcRequest::BucketWait(p) => match handle_bucket_wait(state, p).await {
            Ok(r) => ("bucket_wait", IpcResult::Ok { response: r }),
            Err(e) => ("bucket_wait", IpcResult::Err { error: e }),
        },
        IpcRequest::BucketSummary(p) => match handle_bucket_summary(state, p) {
            Ok(r) => ("bucket_summary", IpcResult::Ok { response: r }),
            Err(e) => ("bucket_summary", IpcResult::Err { error: e }),
        },
        IpcRequest::EventContext(p) => match handle_event_context(state, p) {
            Ok(r) => ("event_context", IpcResult::Ok { response: r }),
            Err(e) => ("event_context", IpcResult::Err { error: e }),
        },
        IpcRequest::CommandStartCombed(p) => match handle_command_start_combed(state, p) {
            Ok(r) => ("command_start_combed", IpcResult::Ok { response: r }),
            Err(e) => ("command_start_combed", IpcResult::Err { error: e }),
        },
        IpcRequest::CommandStatus(p) => match handle_command_status(state, p) {
            Ok(r) => ("command_status", IpcResult::Ok { response: r }),
            Err(e) => ("command_status", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistrySearch(p) => match handle_registry_search(state, p) {
            Ok(r) => ("registry_search", IpcResult::Ok { response: r }),
            Err(e) => ("registry_search", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistryGet(p) => match handle_registry_get(state, p) {
            Ok(r) => ("registry_get", IpcResult::Ok { response: r }),
            Err(e) => ("registry_get", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistryUpsert(p) => match handle_registry_upsert(state, p) {
            Ok(r) => ("registry_upsert", IpcResult::Ok { response: r }),
            Err(e) => ("registry_upsert", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistryTest(p) => match handle_registry_test(state, p) {
            Ok(r) => ("registry_test", IpcResult::Ok { response: r }),
            Err(e) => ("registry_test", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistryActivate(p) => match handle_registry_activate(state, p) {
            Ok(r) => ("registry_activate", IpcResult::Ok { response: r }),
            Err(e) => ("registry_activate", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistryDeactivate(p) => match handle_registry_deactivate(state, p) {
            Ok(r) => ("registry_deactivate", IpcResult::Ok { response: r }),
            Err(e) => ("registry_deactivate", IpcResult::Err { error: e }),
        },
        IpcRequest::RegistryListActive => {
            let r = handle_registry_list_active(state);
            ("registry_list_active", IpcResult::Ok { response: r })
        }
        IpcRequest::FileReadWindow(p) => match handle_file_read_window(state, p) {
            Ok(r) => ("file_read_window", IpcResult::Ok { response: r }),
            Err(e) => ("file_read_window", IpcResult::Err { error: e }),
        },
        IpcRequest::FileSearch(p) => match handle_file_search(state, p) {
            Ok(r) => ("file_search", IpcResult::Ok { response: r }),
            Err(e) => ("file_search", IpcResult::Err { error: e }),
        },
        IpcRequest::FileWatchStart(p) => match handle_file_watch_start(state, p) {
            Ok(r) => ("file_watch_start", IpcResult::Ok { response: r }),
            Err(e) => ("file_watch_start", IpcResult::Err { error: e }),
        },
        IpcRequest::FileWatchStop(p) => match handle_file_watch_stop(state, p) {
            Ok(r) => ("file_watch_stop", IpcResult::Ok { response: r }),
            Err(e) => ("file_watch_stop", IpcResult::Err { error: e }),
        },
        IpcRequest::FileWatchList => {
            let r = handle_file_watch_list(state);
            ("file_watch_list", IpcResult::Ok { response: r })
        }
        IpcRequest::PtyCommandStart(p) => match handle_pty_command_start(state, p) {
            Ok(r) => ("pty_command_start", IpcResult::Ok { response: r }),
            Err(e) => ("pty_command_start", IpcResult::Err { error: e }),
        },
        IpcRequest::PtyCommandWriteStdin(p) => match handle_pty_command_write_stdin(state, p).await
        {
            Ok(r) => ("pty_command_write_stdin", IpcResult::Ok { response: r }),
            Err(e) => ("pty_command_write_stdin", IpcResult::Err { error: e }),
        },
        IpcRequest::PtyCommandStop(p) => match handle_pty_command_stop(state, p) {
            Ok(r) => ("pty_command_stop", IpcResult::Ok { response: r }),
            Err(e) => ("pty_command_stop", IpcResult::Err { error: e }),
        },
        IpcRequest::PtyCommandList => {
            let r = handle_pty_command_list(state);
            ("pty_command_list", IpcResult::Ok { response: r })
        }
    };
    // Audit one row per accepted request. The decision label reflects
    // whether the dispatcher produced an `Ok` response or a typed
    // error.
    let subject = peer.map_or_else(|| "unknown_peer".to_owned(), |p| p.to_audit_string());
    let decision = if matches!(response_result, IpcResult::Ok { .. }) {
        "info"
    } else {
        "error"
    };
    emit_audit(state, method_name, &subject, decision, None, peer);

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
    let mut g = state.store.lock();
    match g.audit_count() {
        Ok(n) => lines.push(format!("audit_count: {n}")),
        Err(e) => lines.push(format!("audit_count: error: {e}")),
    }
    drop(g);
    IpcResponse::SelfCheck(SelfCheckResponse {
        report: lines.join("\n"),
        failures: 0,
    })
}

fn map_bucket_error(e: terminal_commander_core::BucketError) -> IpcError {
    use terminal_commander_core::BucketError;
    match e {
        BucketError::NotFound(_) => IpcError::new(IpcErrorCode::BucketNotFound, e.to_string()),
        other => IpcError::new(IpcErrorCode::Internal, other.to_string()),
    }
}

fn handle_bucket_events_since(
    state: &Arc<DaemonState>,
    params: &BucketEventsSinceParams,
) -> Result<IpcResponse, IpcError> {
    use terminal_commander_core::BucketReadRequest;
    let limit = params
        .limit
        .unwrap_or(DEFAULT_BUCKET_READ_LIMIT)
        .min(MAX_BUCKET_READ_LIMIT);
    let req = BucketReadRequest {
        cursor: params.cursor,
        severity_min: params.severity_min,
        kind_filter: params.kind_filter.clone(),
        limit: Some(limit),
    };
    let resp = state
        .router
        .bucket_events_since(params.bucket_id, &req)
        .map_err(map_bucket_error)?;
    Ok(IpcResponse::BucketEventsSince(BucketEventsSinceResponse {
        bucket_id: params.bucket_id,
        cursor_in: resp.cursor_in,
        next_cursor: resp.next_cursor,
        has_more: resp.has_more,
        dropped_count: resp.dropped_count,
        events: resp.events,
    }))
}

async fn handle_bucket_wait(
    state: &Arc<DaemonState>,
    params: &BucketWaitParams,
) -> Result<IpcResponse, IpcError> {
    use terminal_commander_core::BucketWaitRequest;
    let limit = params
        .limit
        .unwrap_or(DEFAULT_BUCKET_READ_LIMIT)
        .min(MAX_BUCKET_READ_LIMIT);
    let req = BucketWaitRequest {
        cursor: params.cursor,
        severity_min: params.severity_min,
        kind_filter: params.kind_filter.clone(),
        limit: Some(limit),
        timeout: params.timeout(),
    };
    let resp = state
        .router
        .bucket_wait(params.bucket_id, req)
        .await
        .map_err(map_bucket_error)?;
    Ok(IpcResponse::BucketWait(BucketWaitResponse {
        bucket_id: params.bucket_id,
        cursor_in: resp.cursor_in,
        next_cursor: resp.next_cursor,
        heartbeat: resp.heartbeat,
        dropped_count: resp.dropped_count,
        events: resp.events,
    }))
}

fn handle_bucket_summary(
    state: &Arc<DaemonState>,
    params: &BucketSummaryParams,
) -> Result<IpcResponse, IpcError> {
    let s = state
        .router
        .bucket_summary(params.bucket_id)
        .map_err(map_bucket_error)?;
    Ok(IpcResponse::BucketSummary(BucketSummaryResponse {
        bucket_id: params.bucket_id,
        head_seq: s.head_seq,
        tail_seq: s.tail_seq,
        event_count: s.event_count,
        dropped_count: s.dropped_count,
        by_severity: SeverityHistogram {
            trace: s.by_severity.trace,
            debug: s.by_severity.debug,
            info: s.by_severity.info,
            low: s.by_severity.low,
            medium: s.by_severity.medium,
            high: s.by_severity.high,
            critical: s.by_severity.critical,
        },
    }))
}

#[allow(clippy::too_many_lines)] // straight-line pipeline; splitting hurts clarity
fn handle_event_context(
    state: &Arc<DaemonState>,
    params: &EventContextParams,
) -> Result<IpcResponse, IpcError> {
    use terminal_commander_core::{BucketReadRequest, Severity};

    // 1. Locate the event in the bucket by event_id. We scan from
    //    cursor 0 in MAX_BUCKET_READ_LIMIT pages. Buckets are
    //    bounded by retention (TC07) so the scan terminates.
    let mut cursor: u64 = 0;
    let target_event = loop {
        let page = state
            .router
            .bucket_events_since(
                params.bucket_id,
                &BucketReadRequest {
                    cursor,
                    severity_min: None,
                    kind_filter: None,
                    limit: Some(MAX_BUCKET_READ_LIMIT),
                },
            )
            .map_err(map_bucket_error)?;
        if let Some(ev) = page.events.iter().find(|e| e.event_id == params.event_id) {
            break Some(ev.clone());
        }
        if !page.has_more {
            break None;
        }
        cursor = page.next_cursor;
    };
    let Some(event) = target_event else {
        return Err(IpcError::new(
            IpcErrorCode::EventNotFound,
            format!(
                "event {} not found in bucket {}",
                params.event_id.to_wire_string(),
                params.bucket_id.to_wire_string()
            ),
        ));
    };

    // 2. Pointer / unavailable-reason path. Below-Medium events
    //    carry no pointer by design; surface that explicitly.
    let Some(pointer) = event.pointer.as_ref() else {
        let reason = if event.pointer_unavailable_reason.is_some() {
            ContextUnavailableReason::SyntheticEvent
        } else if event.severity < Severity::Medium {
            ContextUnavailableReason::NoPointer
        } else {
            // TC02 invariant: severity>=Medium without pointer MUST
            // carry pointer_unavailable_reason. We surface what the
            // event itself recorded.
            ContextUnavailableReason::SyntheticEvent
        };
        return Ok(IpcResponse::EventContext(EventContextResponse {
            bucket_id: params.bucket_id,
            event_id: params.event_id,
            anchor_missing: false,
            unavailable_reason: Some(reason),
            pointer_unavailable_reason: event.pointer_unavailable_reason,
            frames: Vec::new(),
            total_bytes: 0,
            truncated: false,
        }));
    };

    // 3. Clamp request limits.
    let before = params
        .before
        .unwrap_or(DEFAULT_CONTEXT_BEFORE)
        .min(MAX_CONTEXT_FRAMES);
    let after = params
        .after
        .unwrap_or(DEFAULT_CONTEXT_AFTER)
        .min(MAX_CONTEXT_FRAMES);
    let max_bytes = params
        .max_bytes
        .unwrap_or(MAX_CONTEXT_BYTES)
        .min(MAX_CONTEXT_BYTES);

    // 4. Window resolution.
    let window = state
        .router
        .event_context(
            event.source.probe_id,
            pointer.frame_id,
            before,
            after,
            Some(max_bytes),
        )
        .map_err(|e| IpcError::new(IpcErrorCode::Internal, e.to_string()))?;

    // 5. anchor_missing path (ring eviction).
    if window.anchor_missing {
        return Ok(IpcResponse::EventContext(EventContextResponse {
            bucket_id: params.bucket_id,
            event_id: params.event_id,
            anchor_missing: true,
            unavailable_reason: Some(ContextUnavailableReason::AnchorEvicted),
            pointer_unavailable_reason: event.pointer_unavailable_reason.clone(),
            frames: Vec::new(),
            total_bytes: 0,
            truncated: false,
        }));
    }

    // 6. Project ContextLine -> IpcContextFrame. The wire form
    //    carries no extra fields beyond what the ring frame already
    //    holds. No raw stream beyond the bounded text already
    //    capped by the ring.
    let mut frames: Vec<IpcContextFrame> = Vec::with_capacity(window.frames.len());
    let mut total_bytes: usize = 0;
    for line in &window.frames {
        total_bytes = total_bytes.saturating_add(line.text.len());
        frames.push(IpcContextFrame {
            probe_id: event.source.probe_id,
            frame_id: line.frame_id,
            stream: line.stream.clone(),
            line: line.line,
            text: line.text.clone(),
        });
    }

    let truncated = window.truncated_before
        || window.truncated_after
        || window.truncated_bytes
        || window.truncated_frames;
    Ok(IpcResponse::EventContext(EventContextResponse {
        bucket_id: params.bucket_id,
        event_id: params.event_id,
        anchor_missing: false,
        unavailable_reason: None,
        pointer_unavailable_reason: event.pointer_unavailable_reason.clone(),
        frames,
        total_bytes,
        truncated,
    }))
}

fn map_command_error(e: CommandError) -> IpcError {
    match e {
        CommandError::PolicyDenied(msg) => IpcError::new(IpcErrorCode::PolicyDenied, msg),
        CommandError::ShellInterpreterDenied(shell) => IpcError::new(
            IpcErrorCode::ShellInterpreterDenied,
            format!(
                "shell interpreter '{shell}' denied; command_start_combed is not a shell bridge"
            ),
        ),
        CommandError::EmptyArgv => {
            IpcError::new(IpcErrorCode::ArgvInvalid, "argv must not be empty")
        }
        CommandError::ArgvTooLong(n) => {
            IpcError::new(IpcErrorCode::ArgvInvalid, format!("argv too long: {n}"))
        }
        CommandError::ArgvItemTooLong { index, len } => IpcError::new(
            IpcErrorCode::ArgvInvalid,
            format!("argv[{index}] is {len} bytes; exceeds per-item cap"),
        ),
        CommandError::UnknownJob(id) => {
            IpcError::new(IpcErrorCode::UnknownJob, format!("unknown job: {id}"))
        }
        other => IpcError::new(IpcErrorCode::Internal, other.to_string()),
    }
}

fn handle_command_start_combed(
    state: &Arc<DaemonState>,
    params: &CommandStartParams,
) -> Result<IpcResponse, IpcError> {
    if params.env.len() > MAX_COMMAND_ENV_ITEMS {
        return Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            format!("env entries {} exceed cap", params.env.len()),
        ));
    }
    if params.rules.len() > MAX_COMMAND_INLINE_RULES {
        return Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            format!("inline rules {} exceed cap", params.rules.len()),
        ));
    }
    let req = CommandStartRequest {
        argv: params.argv.clone(),
        cwd: params.cwd.clone(),
        env: params.env.clone(),
        bucket_config: params.bucket_config.clone(),
        rules: params.rules.clone(),
        grace: params.grace(),
    };
    let resp = state.command.start_combed(req).map_err(map_command_error)?;
    Ok(IpcResponse::CommandStartCombed(resp))
}

fn handle_command_status(
    state: &Arc<DaemonState>,
    params: &CommandStatusParams,
) -> Result<IpcResponse, IpcError> {
    let resp = state
        .command
        .status(params.job_id)
        .map_err(map_command_error)?;
    Ok(IpcResponse::CommandStatus(resp))
}

fn map_store_error(e: terminal_commander_store::EventStoreError) -> IpcError {
    use terminal_commander_store::EventStoreError;
    match e {
        EventStoreError::InvalidPayload(msg) => IpcError::new(IpcErrorCode::RuleInvalid, msg),
        other => IpcError::new(IpcErrorCode::Internal, other.to_string()),
    }
}

fn handle_registry_search(
    state: &Arc<DaemonState>,
    params: &RegistrySearchParams,
) -> Result<IpcResponse, IpcError> {
    let limit = params
        .limit
        .map(|n| n.min(crate::ipc::protocol::MAX_REGISTRY_SEARCH_LIMIT));
    let g = state.store.lock();
    let hits = g
        .search_rules(&params.query, limit)
        .map_err(map_store_error)?;
    drop(g);
    let projected: Vec<RegistrySearchHit> = hits
        .into_iter()
        .map(|h| RegistrySearchHit {
            rule_id: h.rule_id,
            version: h.version,
            event_kind: h.event_kind,
            summary_template: h.summary_template,
            tags: h.tags,
            severity: h.severity,
            status: h.status,
        })
        .collect();
    Ok(IpcResponse::RegistrySearch(RegistrySearchResponse {
        hits: projected,
    }))
}

fn lookup_rule_def(
    state: &Arc<DaemonState>,
    rule_id: &str,
    version: Option<u32>,
) -> Result<terminal_commander_core::RuleDefinition, IpcError> {
    let g = state.store.lock();
    let opt = match version {
        Some(v) => g.get_rule_version(rule_id, v).map_err(map_store_error)?,
        None => g.get_latest_rule(rule_id).map_err(map_store_error)?,
    };
    drop(g);
    opt.ok_or_else(|| {
        let message = version.map_or_else(
            || format!("rule '{rule_id}' not found"),
            |v| format!("rule '{rule_id}' version {v} not found"),
        );
        IpcError::new(IpcErrorCode::RuleNotFound, message)
    })
}

fn handle_registry_get(
    state: &Arc<DaemonState>,
    params: &RegistryGetParams,
) -> Result<IpcResponse, IpcError> {
    let def = lookup_rule_def(state, &params.rule_id, params.version)?;
    Ok(IpcResponse::RegistryGet(RegistryGetResponse {
        definition: def,
    }))
}

fn handle_registry_upsert(
    state: &Arc<DaemonState>,
    params: &RegistryUpsertParams,
) -> Result<IpcResponse, IpcError> {
    // Validate up front so the operator gets a typed RuleInvalid
    // instead of a generic Internal error.
    params
        .definition
        .validate()
        .map_err(|e| IpcError::new(IpcErrorCode::RuleInvalid, e.to_string()))?;
    let mut g = state.store.lock();
    let version = g
        .create_rule_version(&params.definition)
        .map_err(map_store_error)?;
    drop(g);
    Ok(IpcResponse::RegistryUpsert(RegistryUpsertResponse {
        rule_id: params.definition.id.clone(),
        version,
    }))
}

fn handle_registry_test(
    state: &Arc<DaemonState>,
    params: &RegistryTestParams,
) -> Result<IpcResponse, IpcError> {
    use terminal_commander_core::{RuleStatus, SourceFrame, SourceStream};
    use terminal_commander_sifters::SifterRuntime;

    if params.samples.len() > MAX_REGISTRY_TEST_SAMPLES {
        return Err(IpcError::new(
            IpcErrorCode::RuleInvalid,
            format!(
                "samples count {} exceeds cap {MAX_REGISTRY_TEST_SAMPLES}",
                params.samples.len()
            ),
        ));
    }

    let mut def = lookup_rule_def(state, &params.rule_id, params.version)?;
    // Force-active so a Draft rule can still be evaluated against
    // samples without persisting an activation. Read-only.
    def.status = RuleStatus::Active;
    let sifter = SifterRuntime::build(std::slice::from_ref(&def))
        .map_err(|e| IpcError::new(IpcErrorCode::RuleInvalid, e.to_string()))?;

    let probe = terminal_commander_core::ProbeId::new();
    let bucket = terminal_commander_core::BucketId::new();
    let mut matches: Vec<RegistryTestMatch> = Vec::new();
    let mut truncated_total: u32 = 0;

    for (i, sample) in params.samples.iter().enumerate() {
        // Per-sample cap; bytes beyond it are dropped before the
        // sifter even sees them.
        let mut text = sample.text.clone();
        if text.len() > MAX_REGISTRY_TEST_SAMPLE_BYTES {
            let mut end = MAX_REGISTRY_TEST_SAMPLE_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let dropped = u32::try_from(text.len() - end).unwrap_or(u32::MAX);
            text.truncate(end);
            truncated_total = truncated_total.saturating_add(dropped);
        }
        let stream = sample.stream.clone().unwrap_or(SourceStream::Stdout);
        let frame = SourceFrame::new(probe, stream, text);
        let drafts = sifter.evaluate(&frame, bucket);
        for draft in drafts {
            let mut captures: std::collections::BTreeMap<String, String> =
                std::collections::BTreeMap::new();
            if let Some(c) = draft.captures.as_ref() {
                for (k, v) in c {
                    captures.insert(k.clone(), v.clone());
                }
            }
            matches.push(RegistryTestMatch {
                sample_index: i,
                severity: draft.severity,
                kind: draft.kind,
                summary: draft.summary,
                captures,
            });
        }
    }

    Ok(IpcResponse::RegistryTest(RegistryTestResponse {
        matches,
        truncated_bytes: truncated_total,
    }))
}

fn handle_registry_activate(
    state: &Arc<DaemonState>,
    params: &RegistryActivateParams,
) -> Result<IpcResponse, IpcError> {
    // TC42d: scope is REQUIRED. A missing scope is rejected with a
    // typed error rather than silently widened to Global. The
    // dispatcher emits the `ipc_registry_activate` audit row with
    // decision=error so the rejection is durably recorded.
    let scope = params.scope.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::ScopeInvalid,
            "scope is required; pass {kind:'global'} for explicit global activation",
        )
    })?;
    let def = lookup_rule_def(state, &params.rule_id, params.version)?;
    let version = def.version;
    validate_scope_against_live_jobs(state, scope)?;
    let was_already_active = state.activation.is_active(&def.id, version, scope);
    // In-memory authority first so a concurrent command_start picks
    // up the rule even if the persistent insert is slow.
    state.activation.activate(def.clone(), scope);
    // Persistent activation row for the audit trail and restart
    // recovery.
    let profile = format!("{:?}", state.policy.profile);
    let mut g = state.store.lock();
    g.record_activation_scoped(&def.id, version, scope, Some(&profile), Some("ipc"))
        .map_err(map_store_error)?;
    drop(g);
    // TC42c: push the new rule set into every already-running
    // command's sifter that the scope matches. Global scope rebinds
    // every live job (TC42b behavior preserved). Scoped activations
    // only touch matching jobs.
    let cmd_report = state.command.rebind_jobs_in_scope(Some(scope));
    // TC43: file watches share the activation registry.
    let watch_report = state.watch.rebind_watches_in_scope(Some(scope));
    // TC44: PTY jobs share the activation registry.
    let pty_report = state.pty.rebind_jobs_in_scope(Some(scope));
    Ok(IpcResponse::RegistryActivate(RegistryActivateResponse {
        rule_id: def.id,
        version,
        was_already_active,
        scope,
        jobs_rebound: cmd_report
            .jobs_rebound
            .saturating_add(watch_report.watches_rebound)
            .saturating_add(pty_report.jobs_rebound),
    }))
}

fn handle_registry_deactivate(
    state: &Arc<DaemonState>,
    params: &RegistryDeactivateParams,
) -> Result<IpcResponse, IpcError> {
    // TC42d: scope is REQUIRED. See `handle_registry_activate` for
    // rationale.
    let scope = params.scope.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::ScopeInvalid,
            "scope is required; pass {kind:'global'} for explicit global deactivation",
        )
    })?;
    validate_scope_against_live_jobs(state, scope)?;
    let was_in_memory = state
        .activation
        .deactivate(&params.rule_id, params.version, scope);
    let mut g = state.store.lock();
    let was_persisted = g
        .deactivate_rule_scoped(&params.rule_id, params.version, scope)
        .map_err(map_store_error)?;
    drop(g);
    // TC42c: rebind every running command the scope matches so
    // future frames stop matching against the deactivated rule.
    // In-flight frames finish against the snapshot they captured
    // (no fake historical un-matches).
    let cmd_report = state.command.rebind_jobs_in_scope(Some(scope));
    let watch_report = state.watch.rebind_watches_in_scope(Some(scope));
    let pty_report = state.pty.rebind_jobs_in_scope(Some(scope));
    Ok(IpcResponse::RegistryDeactivate(
        RegistryDeactivateResponse {
            rule_id: params.rule_id.clone(),
            version: params.version,
            was_deactivated: was_in_memory || was_persisted,
            scope,
            jobs_rebound: cmd_report
                .jobs_rebound
                .saturating_add(watch_report.watches_rebound)
                .saturating_add(pty_report.jobs_rebound),
        },
    ))
}

fn handle_registry_list_active(state: &Arc<DaemonState>) -> IpcResponse {
    let entries: Vec<RegistryActiveEntry> = state
        .activation
        .snapshot_entries()
        .into_iter()
        .map(|e| RegistryActiveEntry {
            rule_id: e.definition.id,
            version: e.definition.version,
            severity: e.definition.severity,
            event_kind: e.definition.event_kind,
            tags: e.definition.tags,
            scope: e.scope,
        })
        .collect();
    IpcResponse::RegistryListActive(RegistryListActiveResponse { entries })
}

/// Validate that a caller-supplied [`ActivationScope`] resolves to a
/// known live entity (where applicable). `Global` is always valid.
/// A `Bucket` / `Job` / `Probe` scope referring to an id the daemon
/// does not currently have a live job for is rejected with
/// [`IpcErrorCode::ScopeInvalid`] instead of silently widening to
/// `Global`.
///
/// Note on liveness: we deliberately only check against the
/// command-runtime's live-job map. A scope referring to a future
/// bucket/job/probe id that has not been started yet is not
/// legitimately scopeable; the operator can create the command
/// first, then activate. A scope referring to a recently-exited job
/// is treated as invalid for the same reason.
fn validate_scope_against_live_jobs(
    state: &Arc<DaemonState>,
    scope: terminal_commander_core::ActivationScope,
) -> Result<(), IpcError> {
    use terminal_commander_core::ActivationScope;
    match scope {
        ActivationScope::Global => Ok(()),
        ActivationScope::Bucket { bucket_id } => {
            let in_command = state
                .command
                .live_jobs()
                .iter()
                .any(|j| j.bucket_id == bucket_id);
            let in_watch = state
                .watch
                .live_watches()
                .iter()
                .any(|w| w.bucket_id == bucket_id);
            let in_pty = state
                .pty
                .live_jobs()
                .iter()
                .any(|j| j.bucket_id == bucket_id);
            if in_command || in_watch || in_pty {
                Ok(())
            } else {
                Err(IpcError::new(
                    IpcErrorCode::ScopeInvalid,
                    format!(
                        "scope bucket_id={} does not resolve to a live job, watch, or pty",
                        bucket_id.to_wire_string()
                    ),
                ))
            }
        }
        ActivationScope::Job { job_id } => {
            let in_command = state.command.live_jobs().iter().any(|j| j.job_id == job_id);
            let in_watch = state
                .watch
                .live_watches()
                .iter()
                .any(|w| w.watch_id == job_id);
            let in_pty = state.pty.live_jobs().iter().any(|j| j.job_id == job_id);
            if in_command || in_watch || in_pty {
                Ok(())
            } else {
                Err(IpcError::new(
                    IpcErrorCode::ScopeInvalid,
                    format!(
                        "scope job_id={} does not resolve to a live job, watch, or pty",
                        job_id.to_wire_string()
                    ),
                ))
            }
        }
        ActivationScope::Probe { probe_id } => {
            let in_command = state
                .command
                .live_jobs()
                .iter()
                .any(|j| j.probe_id == probe_id);
            let in_watch = state
                .watch
                .live_watches()
                .iter()
                .any(|w| w.probe_id == probe_id);
            let in_pty = state.pty.live_jobs().iter().any(|j| j.probe_id == probe_id);
            if in_command || in_watch || in_pty {
                Ok(())
            } else {
                Err(IpcError::new(
                    IpcErrorCode::ScopeInvalid,
                    format!(
                        "scope probe_id={} does not resolve to a live job, watch, or pty",
                        probe_id.to_wire_string()
                    ),
                ))
            }
        }
    }
}

fn emit_audit(
    state: &Arc<DaemonState>,
    action: &str,
    subject: &str,
    decision: &str,
    reason: Option<String>,
    peer: Option<PeerCred>,
) {
    let mut entry = AuditEntry::new(format!("ipc_{action}"), subject, decision).with_actor("ipc");
    if let Some(r) = reason {
        entry = entry.with_reason(r);
    }
    if let Some(p) = peer {
        // Pre-serialized JSON metadata. Stays well inside
        // MAX_AUDIT_METADATA_BYTES.
        let meta = format!(
            r#"{{"uid":{},"gid":{},"pid":{}}}"#,
            p.uid,
            p.gid,
            p.pid.map_or_else(|| "null".to_owned(), |x| x.to_string())
        );
        entry = entry.with_metadata_json(meta);
    }
    // Best-effort; audit unhealth must not DOS the IPC path.
    let sink: Arc<dyn AuditSink> = Arc::clone(&state.audit) as Arc<dyn AuditSink>;
    let _ = sink.emit(&entry);
}

fn emit_audit_internal_error(state: &Arc<DaemonState>, action: &str, message: &str) {
    let entry = AuditEntry::new(format!("ipc_{action}"), "internal", "error")
        .with_actor("ipc")
        .with_reason(message);
    let sink: Arc<dyn AuditSink> = Arc::clone(&state.audit) as Arc<dyn AuditSink>;
    let _ = sink.emit(&entry);
}

// =====================================================================
// TC43: file_read_window / file_search / file_watch_* handlers.
//
// Common invariants:
// - Path-policy gate via `state.policy.evaluate(...)` BEFORE any I/O.
// - Existence + regular-file check returns typed `FileNotFound`.
// - Non-UTF-8 / binary content returns typed `FileBinary`.
// - All caps clamp at the dispatcher; oversized payloads surface
//   `OversizedRequest`.
// =====================================================================

fn map_path_policy(
    state: &Arc<DaemonState>,
    path: &std::path::Path,
    is_watch: bool,
) -> Result<(), IpcError> {
    let action = if is_watch {
        crate::policy::PolicyAction::FileWatch { path }
    } else {
        crate::policy::PolicyAction::FileRead { path }
    };
    let verdict = state.policy.evaluate(&action);
    if verdict.decision == crate::policy::PolicyDecision::Deny {
        return Err(IpcError::new(IpcErrorCode::PathDenied, verdict.reason));
    }
    Ok(())
}

fn require_regular_file(path: &std::path::Path) -> Result<std::fs::Metadata, IpcError> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => Ok(m),
        Ok(_) => Err(IpcError::new(
            IpcErrorCode::FileNotFound,
            format!("'{}' is not a regular file", path.display()),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(IpcError::new(
            IpcErrorCode::FileNotFound,
            format!("'{}' does not exist", path.display()),
        )),
        Err(e) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("stat '{}': {e}", path.display()),
        )),
    }
}

fn handle_file_read_window(
    state: &Arc<DaemonState>,
    params: &FileReadWindowParams,
) -> Result<IpcResponse, IpcError> {
    use std::io::{BufRead, BufReader};

    map_path_policy(state, &params.path, false)?;
    let meta = require_regular_file(&params.path)?;
    let file_bytes = meta.len();

    let start_line = params.start_line.unwrap_or(1).max(1);
    let max_lines = params
        .max_lines
        .unwrap_or(DEFAULT_FILE_READ_LINES)
        .min(MAX_FILE_READ_LINES);
    let max_bytes = params
        .max_bytes
        .unwrap_or(DEFAULT_FILE_READ_BYTES)
        .min(MAX_FILE_READ_BYTES);

    let f = std::fs::File::open(&params.path)
        .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("open: {e}")))?;
    let mut reader = BufReader::new(f);
    let mut byte_offset: u64 = 0;
    let mut line_no: u64 = 0;
    let mut out_lines: Vec<FileLine> = Vec::new();
    let mut total_bytes: usize = 0;
    let mut truncated = false;
    let mut buf = String::new();
    let next_byte_offset: u64;

    loop {
        buf.clear();
        let read = reader.read_line(&mut buf).map_err(|e| {
            if matches!(e.kind(), std::io::ErrorKind::InvalidData) {
                IpcError::new(
                    IpcErrorCode::FileBinary,
                    format!("'{}' contains non-UTF-8 bytes", params.path.display()),
                )
            } else {
                IpcError::new(IpcErrorCode::Internal, format!("read_line: {e}"))
            }
        })?;
        if read == 0 {
            next_byte_offset = byte_offset;
            break;
        }
        line_no = line_no.saturating_add(1);
        let line_start = byte_offset;
        byte_offset = byte_offset.saturating_add(read as u64);
        if line_no < start_line {
            continue;
        }
        let trimmed = buf.trim_end_matches('\n').trim_end_matches('\r').to_owned();
        let line_size = trimmed.len();
        if total_bytes.saturating_add(line_size) > max_bytes {
            truncated = true;
            next_byte_offset = line_start;
            break;
        }
        total_bytes = total_bytes.saturating_add(line_size);
        out_lines.push(FileLine {
            line: line_no,
            byte_offset: line_start,
            text: trimmed,
        });
        if u32::try_from(out_lines.len()).unwrap_or(u32::MAX) >= max_lines {
            truncated = true;
            next_byte_offset = byte_offset;
            break;
        }
    }

    Ok(IpcResponse::FileReadWindow(FileReadWindowResponse {
        path: params.path.clone(),
        lines: out_lines,
        file_bytes,
        truncated,
        next_byte_offset,
    }))
}

fn handle_file_search(
    state: &Arc<DaemonState>,
    params: &FileSearchParams,
) -> Result<IpcResponse, IpcError> {
    use std::io::{BufRead, BufReader};

    if params.query.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            "query must be non-empty",
        ));
    }
    map_path_policy(state, &params.path, false)?;
    require_regular_file(&params.path)?;

    let max_matches = params
        .max_matches
        .unwrap_or(DEFAULT_FILE_SEARCH_MATCHES)
        .min(MAX_FILE_SEARCH_MATCHES);
    let max_snippet = params
        .max_snippet_bytes
        .unwrap_or(DEFAULT_FILE_SEARCH_SNIPPET_BYTES)
        .min(MAX_FILE_SEARCH_SNIPPET_BYTES);
    let case_insensitive = params.case_insensitive.unwrap_or(false);
    let needle_lower = params.query.to_ascii_lowercase();

    let f = std::fs::File::open(&params.path)
        .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("open: {e}")))?;
    let mut reader = BufReader::new(f);
    let mut matches: Vec<FileSearchMatch> = Vec::new();
    let mut bytes_scanned: u64 = 0;
    let mut byte_offset: u64 = 0;
    let mut line_no: u64 = 0;
    let mut truncated = false;
    let mut buf = String::new();

    loop {
        buf.clear();
        let read = reader.read_line(&mut buf).map_err(|e| {
            if matches!(e.kind(), std::io::ErrorKind::InvalidData) {
                IpcError::new(
                    IpcErrorCode::FileBinary,
                    format!("'{}' contains non-UTF-8 bytes", params.path.display()),
                )
            } else {
                IpcError::new(IpcErrorCode::Internal, format!("read_line: {e}"))
            }
        })?;
        if read == 0 {
            break;
        }
        line_no = line_no.saturating_add(1);
        bytes_scanned = bytes_scanned.saturating_add(read as u64);
        let line_start = byte_offset;
        byte_offset = byte_offset.saturating_add(read as u64);

        let line = buf.trim_end_matches('\n').trim_end_matches('\r');
        let pos = if case_insensitive {
            line.to_ascii_lowercase().find(&needle_lower)
        } else {
            line.find(&params.query)
        };
        if let Some(col) = pos {
            let snippet = if line.len() > max_snippet {
                let mut end = max_snippet;
                while !line.is_char_boundary(end) && end > 0 {
                    end -= 1;
                }
                line[..end].to_owned()
            } else {
                line.to_owned()
            };
            matches.push(FileSearchMatch {
                line: line_no,
                byte_offset: line_start.saturating_add(col as u64),
                snippet,
            });
            if u32::try_from(matches.len()).unwrap_or(u32::MAX) >= max_matches {
                truncated = true;
                break;
            }
        }
        if bytes_scanned >= MAX_FILE_SEARCH_SCAN_BYTES {
            truncated = true;
            break;
        }
    }

    Ok(IpcResponse::FileSearch(FileSearchResponse {
        path: params.path.clone(),
        matches,
        truncated,
        bytes_scanned,
    }))
}

fn handle_file_watch_start(
    state: &Arc<DaemonState>,
    params: &FileWatchStartParams,
) -> Result<IpcResponse, IpcError> {
    if params.rules.len() > MAX_COMMAND_INLINE_RULES {
        return Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            format!(
                "rules has {} items; cap is {MAX_COMMAND_INLINE_RULES}",
                params.rules.len()
            ),
        ));
    }
    let bucket_cfg = params.bucket_config.clone().unwrap_or_default();
    let follow_from_beginning = params.follow_from_beginning.unwrap_or(false);
    match state.watch.start(
        params.path.clone(),
        bucket_cfg,
        params.rules.clone(),
        follow_from_beginning,
    ) {
        Ok((watch_id, bucket_id, probe_id)) => {
            Ok(IpcResponse::FileWatchStart(FileWatchStartResponse {
                watch_id,
                bucket_id,
                probe_id,
                cursor: 0,
            }))
        }
        Err(crate::file_watch::WatchError::PolicyDenied(reason)) => {
            Err(IpcError::new(IpcErrorCode::PathDenied, reason))
        }
        Err(crate::file_watch::WatchError::NotFound(p)) => Err(IpcError::new(
            IpcErrorCode::FileNotFound,
            format!("'{}' is not a regular file", p.display()),
        )),
        Err(crate::file_watch::WatchError::Sifter(e)) => {
            Err(IpcError::new(IpcErrorCode::RuleInvalid, e))
        }
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("file_watch_start: {other}"),
        )),
    }
}

fn handle_file_watch_stop(
    state: &Arc<DaemonState>,
    params: &FileWatchStopParams,
) -> Result<IpcResponse, IpcError> {
    match state.watch.stop(params.watch_id) {
        Ok((bucket_id, m)) => Ok(IpcResponse::FileWatchStop(FileWatchStopResponse {
            watch_id: params.watch_id,
            bucket_id,
            frames_total: m.frames_total,
            events_emitted: m.events_emitted,
            bytes_total: m.bytes_total,
        })),
        Err(crate::file_watch::WatchError::UnknownWatch(id)) => Err(IpcError::new(
            IpcErrorCode::UnknownWatch,
            format!("watch id '{}' is not live", id.to_wire_string()),
        )),
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("file_watch_stop: {other}"),
        )),
    }
}

// =====================================================================
// TC44: pty_command_{start,write_stdin,stop,list} handlers.
//
// Reuses the existing `validate_scope_against_live_jobs` /
// `live_jobs` machinery so scoped registry activations transparently
// reach live PTY jobs via the standard rebind path.
// =====================================================================

fn handle_pty_command_start(
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
    };
    match state.pty.start(req) {
        Ok(r) => Ok(IpcResponse::PtyCommandStart(PtyCommandStartResponse {
            job_id: r.job_id,
            bucket_id: r.bucket_id,
            probe_id: r.probe_id,
            cursor: 0,
        })),
        Err(crate::pty_command::PtyRuntimeError::PolicyDenied(reason)) => {
            Err(IpcError::new(IpcErrorCode::PolicyDenied, reason))
        }
        Err(crate::pty_command::PtyRuntimeError::ShellInterpreterDenied(shell)) => {
            Err(IpcError::new(IpcErrorCode::ShellInterpreterDenied, shell))
        }
        Err(crate::pty_command::PtyRuntimeError::EmptyArgv) => Err(IpcError::new(
            IpcErrorCode::ArgvInvalid,
            "argv must not be empty",
        )),
        Err(crate::pty_command::PtyRuntimeError::Sifter(reason)) => {
            Err(IpcError::new(IpcErrorCode::RuleInvalid, reason))
        }
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("pty_command_start: {other}"),
        )),
    }
}

async fn handle_pty_command_write_stdin(
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
        Ok(r) => Ok(IpcResponse::PtyCommandWriteStdin(
            PtyCommandWriteStdinResponse {
                job_id: params.job_id,
                bytes_written: r.bytes_written,
                secret_prompt_active: r.secret_prompt_active,
            },
        )),
        Err(crate::pty_command::PtyRuntimeError::SecretInputDenied) => Err(IpcError::new(
            IpcErrorCode::SecretInputDenied,
            "secret prompt active; LLM-supplied input denied",
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

fn handle_pty_command_stop(
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

fn handle_pty_command_list(state: &Arc<DaemonState>) -> IpcResponse {
    let entries: Vec<PtyCommandListEntry> = state
        .pty
        .list()
        .into_iter()
        .map(
            |(job_id, bucket_id, probe_id, argv, m, secret_prompt_active)| PtyCommandListEntry {
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
            },
        )
        .collect();
    IpcResponse::PtyCommandList(PtyCommandListResponse { entries })
}

fn handle_file_watch_list(state: &Arc<DaemonState>) -> IpcResponse {
    let entries: Vec<FileWatchListEntry> = state
        .watch
        .list()
        .into_iter()
        .map(
            |(watch_id, bucket_id, probe_id, path, m)| FileWatchListEntry {
                watch_id,
                bucket_id,
                probe_id,
                path,
                frames_total: m.frames_total,
                events_emitted: m.events_emitted,
                bytes_total: m.bytes_total,
            },
        )
        .collect();
    IpcResponse::FileWatchList(FileWatchListResponse { entries })
}
