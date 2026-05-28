// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Windows named-pipe IPC server (parent daemon).

use std::io;
use std::sync::Arc;
use std::time::Instant;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::watch;

use terminal_commander_supervisor::identity::PeerIdentity;

use crate::ipc::framing::{read_request, write_response};
use crate::ipc::peer_windows::peer_identity_for;
use crate::ipc::protocol::RequestEnvelope;
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
    // SDDL is process-stable (current user's SID does not change), so
    // build once before the loop instead of on every accept iteration.
    #[cfg(windows)]
    let sddl = match crate::ipc::pipe_acl::build_sddl_for_current_user() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("terminal-commanderd: SDDL build failed: {e}; falling back to default ACL");
            String::new()
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
            res = server.connect() => {
                if res.is_ok() {
                    let state = Arc::clone(&state);
                    let shutdown_for_conn = shutdown.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_pipe_connection(server, state, boot, shutdown_for_conn).await {
                            eprintln!("terminal-commanderd: pipe connection error: {e}");
                        }
                    });
                }
            }
        }
    }
}

async fn handle_pipe_connection(
    mut server: NamedPipeServer,
    state: Arc<DaemonState>,
    boot: Instant,
    shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let identity: PeerIdentity = peer_identity_for(&server);
    #[cfg(any(test, feature = "test-util"))]
    state.test_record_peer_identity(identity.clone());
    loop {
        if *shutdown.borrow() {
            break;
        }
        let req: RequestEnvelope = match read_request(&mut server).await {
            Ok(r) => r,
            Err(_) => break,
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
