// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Windows named-pipe IPC server (parent daemon).

use std::io;
use std::sync::Arc;
use std::time::Instant;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::watch;

use terminal_commander_supervisor::identity::PeerIdentity;

use crate::ipc::framing::{ReadOutcome, read_request_classified, write_response};
use crate::ipc::peer_windows::peer_identity_for;
use crate::ipc::protocol::{IpcResult, ResponseEnvelope};
use crate::ipc::server::dispatch_envelope;
use crate::state::DaemonState;

/// Win32 error codes consulted when classifying pipe-create failures.
#[cfg(windows)]
mod win32_pipe_errors {
    pub(super) const ERROR_ACCESS_DENIED: i32 = 5;
    pub(super) const ERROR_INVALID_NAME: i32 = 123;
    pub(super) const ERROR_ALREADY_EXISTS: i32 = 183;
}

/// How many consecutive transient create failures before the accept loop
/// stops (100 ms × 30 = 3 s), matching the supervisor replace probe budget.
#[cfg(windows)]
const MAX_PIPE_CREATE_RETRIES: u32 = 30;

#[cfg(windows)]
const PIPE_CREATE_RETRY_DELAY_MS: u64 = 100;

/// Upper bound on the graceful-shutdown drain of in-flight pipe
/// connection handlers. Matches the Unix `DRAIN_CEILING` (10 s): a
/// handler that does not finish in time is aborted so the process can
/// exit rather than hanging shutdown.
#[cfg(windows)]
const PIPE_DRAIN_CEILING: std::time::Duration = std::time::Duration::from_secs(10);

/// Await all in-flight pipe connection handlers, bounded by
/// [`PIPE_DRAIN_CEILING`]. On timeout, abort the stragglers and return
/// so shutdown cannot hang. Mirrors the Unix `drain_connections`.
#[cfg(windows)]
async fn drain_pipe_connections(conns: &mut tokio::task::JoinSet<()>) {
    if conns.is_empty() {
        return;
    }
    let drain = async { while conns.join_next().await.is_some() {} };
    if tokio::time::timeout(PIPE_DRAIN_CEILING, drain)
        .await
        .is_err()
    {
        // Ceiling hit: a connection did not finish in time. Abort the
        // stragglers and return so the process can exit. JoinSet::abort_all
        // is best-effort; we do not re-await aborted handles.
        conns.abort_all();
    }
}

/// Fatal vs transient classification for `ServerOptions::create` /
/// `create_named_pipe_with_sddl` errors.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PipeCreateErrorClass {
    /// Misconfiguration, ACL, or first-instance name collision — retrying
    /// will not help.
    Fatal,
    /// Timing / all instances busy — bounded retry.
    Transient,
}

#[cfg(windows)]
fn classify_pipe_create_error(err: &io::Error, first_instance: bool) -> PipeCreateErrorClass {
    use win32_pipe_errors::{ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_INVALID_NAME};

    let Some(code) = err.raw_os_error() else {
        return PipeCreateErrorClass::Transient;
    };
    match code {
        ERROR_ACCESS_DENIED | ERROR_INVALID_NAME => PipeCreateErrorClass::Fatal,
        ERROR_ALREADY_EXISTS if first_instance => PipeCreateErrorClass::Fatal,
        // ERROR_PIPE_BUSY (231) and other timing errors: bounded retry.
        _ => PipeCreateErrorClass::Transient,
    }
}

#[cfg(windows)]
fn format_os_error_code(err: &io::Error) -> String {
    err.raw_os_error()
        .map_or_else(|| "?".to_string(), |c| c.to_string())
}

/// Log a pipe-create failure and decide whether the accept loop should stop.
#[cfg(windows)]
fn log_pipe_create_failure(err: &io::Error, first_instance: bool, transient_failures: u32) -> bool {
    let os = format_os_error_code(err);
    if classify_pipe_create_error(err, first_instance) == PipeCreateErrorClass::Fatal {
        eprintln!(
            "terminal-commanderd: pipe create failed (fatal, os={os}): {err}; \
             stopping accept loop"
        );
        return true;
    }
    if transient_failures >= MAX_PIPE_CREATE_RETRIES {
        eprintln!(
            "terminal-commanderd: pipe create failed after \
             {transient_failures} retries (os={os}): {err}; stopping accept loop"
        );
        return true;
    }
    eprintln!(
        "terminal-commanderd: pipe create transient error \
         ({transient_failures}/{MAX_PIPE_CREATE_RETRIES}, os={os}): {err}; \
         retrying in {PIPE_CREATE_RETRY_DELAY_MS}ms"
    );
    false
}

/// Handle for the named-pipe accept loop.
pub struct PipeServerHandle {
    shutdown_tx: watch::Sender<bool>,
    join: Option<tokio::task::JoinHandle<()>>,
}

impl PipeServerHandle {
    pub async fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(j) = self.join.take() {
            let _ = j.await;
        }
    }
}

/// Named-pipe IPC server.
pub struct PipeServer {
    state: Arc<DaemonState>,
    boot: Instant,
    pipe_name: String,
}

impl PipeServer {
    #[must_use]
    pub fn new(state: Arc<DaemonState>, pipe_name: String) -> Self {
        Self {
            state,
            boot: Instant::now(),
            pipe_name,
        }
    }

    pub fn spawn(self) -> io::Result<PipeServerHandle> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let state = Arc::clone(&self.state);
        let boot = self.boot;
        let pipe_name = self.pipe_name;
        let join = tokio::spawn(async move {
            accept_loop(pipe_name, state, boot, shutdown_rx).await;
        });
        Ok(PipeServerHandle {
            shutdown_tx,
            join: Some(join),
        })
    }
}

async fn accept_loop(
    pipe_name: String,
    state: Arc<DaemonState>,
    boot: Instant,
    mut shutdown: watch::Receiver<bool>,
) {
    if *shutdown.borrow() {
        return;
    }
    // Per-connection tasks are tracked in a JoinSet (rather than
    // detached `tokio::spawn`) so a graceful shutdown can DRAIN them,
    // mirroring the Unix UDS server. When the shutdown flag flips, the
    // connection serving the `Shutdown` request finishes flushing its
    // ACK and every other in-flight request runs to completion before
    // the loop returns. The accept loop owns the set;
    // `PipeServerHandle::shutdown` joins THIS task, so awaiting the join
    // handle transitively waits for the drain below. Without this, an
    // in-flight handler could outlive `shutdown_store` and touch a
    // torn-down store.
    let mut conns: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();
    // SDDL is process-stable (current user's SID does not change), so
    // build once before the loop instead of on every accept iteration.
    #[cfg(windows)]
    let sddl = match crate::ipc::pipe_acl::build_sddl_for_current_user() {
        Ok(s) if !s.is_empty() => s,
        other => {
            // Fail closed: never fall back to the default named-pipe DACL.
            // That default grants broader access than the intended
            // LocalSystem + Administrators + current-user restriction and
            // would let a lower-privileged local process connect. The Unix
            // transport fails closed on peer-credential failure; on Windows
            // the pipe ACL is the equivalent (and only) access gate, so a
            // failure to build it must refuse to listen rather than bind an
            // unrestricted pipe. `build_sddl_for_current_user` only fails on
            // rare token-API errors; the supervisor will then report the
            // daemon unreachable and a cold respawn retries.
            match other {
                Ok(_) => eprintln!(
                    "terminal-commanderd: SDDL build produced an empty descriptor; refusing to bind named pipe (fail-closed)"
                ),
                Err(e) => eprintln!(
                    "terminal-commanderd: SDDL build failed: {e}; refusing to bind named pipe (fail-closed)"
                ),
            }
            return;
        }
    };
    let mut first = true;
    #[cfg(windows)]
    let mut transient_create_failures = 0u32;
    loop {
        if *shutdown.borrow() {
            break;
        }
        let mut builder = ServerOptions::new();
        if first {
            builder.first_pipe_instance(true);
        }
        #[cfg(windows)]
        let server_result = if sddl.is_empty() {
            builder.create(&pipe_name)
        } else {
            crate::ipc::pipe_acl::create_named_pipe_with_sddl(&pipe_name, &sddl, first)
        };
        #[cfg(not(windows))]
        let server_result = builder.create(&pipe_name);
        let server = match server_result {
            Ok(s) => {
                #[cfg(windows)]
                {
                    transient_create_failures = 0;
                }
                s
            }
            Err(e) => {
                #[cfg(windows)]
                {
                    transient_create_failures += 1;
                    if log_pipe_create_failure(&e, first, transient_create_failures) {
                        break;
                    }
                    tokio::select! {
                        biased;
                        res = shutdown.changed() => {
                            if res.is_err() || *shutdown.borrow() { break; }
                        }
                        () = tokio::time::sleep(std::time::Duration::from_millis(
                            PIPE_CREATE_RETRY_DELAY_MS,
                        )) => {}
                    }
                    continue;
                }
                #[cfg(not(windows))]
                {
                    eprintln!("terminal-commanderd: pipe create error: {e}; stopping accept loop");
                    break;
                }
            }
        };
        first = false;
        tokio::select! {
            biased;
            res = shutdown.changed() => {
                if res.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            // Reap finished connection tasks as they complete so the
            // JoinSet does not grow without bound under steady load.
            // Guarded so `join_next` is only polled when non-empty (it
            // resolves to `None` immediately on an empty set, which
            // would otherwise busy-spin this branch). Mirrors the Unix
            // accept loop.
            Some(_joined) = conns.join_next(), if !conns.is_empty() => {}
            res = server.connect() => {
                if res.is_ok() {
                    let state = Arc::clone(&state);
                    let shutdown_for_conn = shutdown.clone();
                    conns.spawn(async move {
                        if let Err(e) = handle_pipe_connection(server, state, boot, shutdown_for_conn).await {
                            eprintln!("terminal-commanderd: pipe connection error: {e}");
                        }
                    });
                }
            }
        }
    }

    // Drain: we have stopped accepting. The shutdown flag is now
    // `true`, so each in-flight `handle_pipe_connection` breaks out of
    // its own loop at the next request boundary; a connection that is
    // mid-dispatch finishes the current request (and flushes its
    // response) first. Wait for all of them, bounded by
    // PIPE_DRAIN_CEILING, before returning so no handler outlives
    // `shutdown_store`. Mirrors the Unix `drain_connections`.
    drain_pipe_connections(&mut conns).await;
}

async fn handle_pipe_connection(
    mut server: NamedPipeServer,
    state: Arc<DaemonState>,
    boot: Instant,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let identity: PeerIdentity = peer_identity_for(&server);
    #[cfg(any(test, feature = "test-util"))]
    state.test_record_peer_identity(identity.clone());
    // Sticky shutdown: if the flag is already true, do not start
    // serving requests on this connection.
    if *shutdown.borrow() {
        return Ok(());
    }
    loop {
        // Mirror the UDS `handle_connection`: select on shutdown
        // ALONGSIDE the read so a handler parked between frames wakes
        // immediately when shutdown is requested, instead of blocking on
        // the read until the drain ceiling aborts it. A request already
        // being read/dispatched/written runs to completion (the drain
        // waits for it); only a connection idle between frames is closed
        // promptly. This is what makes `PipeServerHandle::shutdown`'s
        // drain bounded and fast.
        let read = tokio::select! {
            biased;
            res = shutdown.changed() => {
                if res.is_err() || *shutdown.borrow() {
                    break;
                }
                continue;
            }
            read = read_request_classified(&mut server) => read,
        };
        // A clean EOF between frames closes the connection silently, but
        // a malformed frame / protocol error gets a typed `IpcError`
        // envelope written back to the client (correlation_id 0) before
        // the connection is closed. The shared classified read returns
        // the same `IpcErrorCode` values the UDS path surfaces
        // (FrameTooLarge / MalformedJson / Internal).
        let req = match read {
            ReadOutcome::Ok(req) => req,
            ReadOutcome::Eof => break,
            ReadOutcome::Err(error) => {
                let env = ResponseEnvelope {
                    correlation_id: 0,
                    result: IpcResult::Err { error },
                };
                // Best-effort: the client may have already gone away.
                let _ = write_response(&mut server, &env).await;
                // Malformed framing is connection-fatal.
                break;
            }
        };
        let resp = dispatch_envelope(&state, boot, &req, &identity).await;
        write_response(&mut server, &resp).await?;
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod pipe_create_error_tests {
    use super::*;
    use std::io;

    fn err(code: i32) -> io::Error {
        io::Error::from_raw_os_error(code)
    }

    #[test]
    fn access_denied_and_invalid_name_are_fatal() {
        use win32_pipe_errors as w;
        assert_eq!(
            classify_pipe_create_error(&err(w::ERROR_ACCESS_DENIED), true),
            PipeCreateErrorClass::Fatal
        );
        assert_eq!(
            classify_pipe_create_error(&err(w::ERROR_INVALID_NAME), false),
            PipeCreateErrorClass::Fatal
        );
    }

    #[test]
    fn already_exists_fatal_only_for_first_instance() {
        use win32_pipe_errors as w;
        assert_eq!(
            classify_pipe_create_error(&err(w::ERROR_ALREADY_EXISTS), true),
            PipeCreateErrorClass::Fatal
        );
        assert_eq!(
            classify_pipe_create_error(&err(w::ERROR_ALREADY_EXISTS), false),
            PipeCreateErrorClass::Transient
        );
    }

    #[test]
    fn pipe_busy_is_transient() {
        // Win32 ERROR_PIPE_BUSY (231): between accept and recreate.
        assert_eq!(
            classify_pipe_create_error(&err(231), true),
            PipeCreateErrorClass::Transient
        );
    }

    #[test]
    fn unknown_os_code_is_transient() {
        assert_eq!(
            classify_pipe_create_error(&err(9999), true),
            PipeCreateErrorClass::Transient
        );
    }
}
