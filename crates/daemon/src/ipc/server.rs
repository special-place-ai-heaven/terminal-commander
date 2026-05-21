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
use crate::ipc::peer::{self, PeerCred};
use crate::ipc::protocol::{
    DiscoverResponse, IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, MAX_FRAME_BYTES,
    PolicyStatusResponse, RequestEnvelope, ResponseEnvelope, SelfCheckResponse, decode_payload,
    encode_frame,
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
                        let resp = dispatch(&state, boot, &req_env, peer);
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

fn dispatch(
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
    };
    // Audit one row per accepted request.
    let subject = peer.map_or_else(|| "unknown_peer".to_owned(), |p| p.to_audit_string());
    emit_audit(state, method_name, &subject, "info", None, peer);

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
