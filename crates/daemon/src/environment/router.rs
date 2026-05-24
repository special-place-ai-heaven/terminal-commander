// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Routes probe operations to local runtime or a remote runner daemon.

use std::sync::Arc;

use terminal_commander_core::EnvironmentSpec;
use thiserror::Error;

use crate::ipc::protocol::{IpcRequest, IpcResponse};
use crate::state::DaemonState;

#[derive(Debug, Error)]
pub enum RouteError {
    #[error("environment not available: {0}")]
    Unavailable(String),
    #[error("runner bootstrap failed: {0}")]
    Bootstrap(String),
    #[error("runner IPC failed: {0}")]
    RunnerIpc(String),
}

/// Result of routing: handled locally or forwarded to runner.
#[derive(Debug)]
pub enum RouteOutcome {
    /// Caller should dispatch on parent `DaemonState` as today.
    Local,
    /// Response already obtained from runner IPC.
    RunnerResponse(Box<IpcResponse>),
}

/// Parent-side environment router (M1).
pub struct EnvironmentRouter;

impl EnvironmentRouter {
    /// Decide whether this request stays local or goes to a runner.
    ///
    /// M1: only `EnvironmentSpec::WslDistro` on Windows triggers runner
    /// forwarding when the WSL runner client is available.
    pub async fn route_request(
        _state: &Arc<DaemonState>,
        env: &EnvironmentSpec,
        request: &IpcRequest,
    ) -> Result<RouteOutcome, RouteError> {
        match env {
            EnvironmentSpec::Local => Ok(RouteOutcome::Local),
            EnvironmentSpec::WslDistro { distro } => {
                #[cfg(windows)]
                {
                    return crate::environment::wsl::forward_to_runner(distro, request).await;
                }
                #[cfg(not(windows))]
                {
                    let _ = (distro, request);
                    Err(RouteError::Unavailable(
                        "WSL runner routing is only supported from a Windows parent".to_owned(),
                    ))
                }
            }
            EnvironmentSpec::SshHost { host } => Err(RouteError::Unavailable(format!(
                "SSH runner not implemented (host={host})"
            ))),
        }
    }
}
