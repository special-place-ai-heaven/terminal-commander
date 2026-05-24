// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Windows named-pipe IPC server (parent daemon).

use std::io;
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::watch;

use crate::ipc::framing::{read_request, write_response};
use crate::ipc::peer::PeerCred;
use crate::ipc::protocol::RequestEnvelope;
use crate::ipc::server::dispatch_envelope;
use crate::state::DaemonState;

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
        let pipe_name = self.pipe_name.clone();
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
    let mut first_instance = true;
    loop {
        if *shutdown.borrow() {
            break;
        }
        let server = match if first_instance {
            first_instance = false;
            ServerOptions::new()
                .first_pipe_instance(true)
                .create(&pipe_name)
        } else {
            ServerOptions::new().create(&pipe_name)
        } {
            Ok(s) => s,
            Err(e) => {
                eprintln!("terminal-commanderd: pipe create failed: {e}");
                break;
            }
        };
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
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let peer = Some(PeerCred {
        uid: 0,
        gid: 0,
        pid: None,
    });
    loop {
        if *shutdown.borrow() {
            break;
        }
        let req: RequestEnvelope = match read_request(&mut server).await {
            Ok(r) => r,
            Err(_) => break,
        };
        let resp = dispatch_envelope(&state, boot, &req, peer).await;
        write_response(&mut server, &resp).await?;
    }
    Ok(())
}
