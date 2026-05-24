// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Windows parent → WSL runner daemon (control channel, not MCP stdio).

use std::path::PathBuf;
use std::time::Duration;

use crate::ipc::protocol::{IpcRequest, IpcResponse};

use super::router::{RouteError, RouteOutcome};

/// Default runner socket inside the distro (Linux path).
const RUNNER_SOCK_SUFFIX: &str = ".local/share/terminal-commanderd/terminal-commanderd.sock";

/// Forward one IPC request to the runner daemon in `distro`.
pub async fn forward_to_runner(distro: &str, request: &IpcRequest) -> Result<RouteOutcome, RouteError> {
    let sock = runner_socket_path(distro)?;
    #[cfg(unix)]
    {
        let client = crate::ipc::DaemonClient::new(&sock).with_timeout(Duration::from_secs(30));
        let response = client
            .call(1, request.clone())
            .await
            .map_err(|e| RouteError::RunnerIpc(e.message))?;
        return Ok(RouteOutcome::RunnerResponse(response));
    }
    #[cfg(windows)]
    {
        let _ = (sock, request);
        return Err(RouteError::Unavailable(
            "WSL runner IPC from Windows parent uses a frame relay (planned); \
             use EnvironmentSpec::Local for native Windows terminals"
                .to_owned(),
        ));
    }
}

/// Resolve runner UDS path via `\\wsl.localhost\<distro>\...` when possible.
fn runner_socket_path(distro: &str) -> Result<PathBuf, RouteError> {
    if distro.is_empty() {
        return Err(RouteError::Bootstrap("empty WSL distro name".to_owned()));
    }
    #[cfg(windows)]
    {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_owned());
        // WSL interop exposes Linux home under \\wsl.localhost\<distro>\home\<user>\...
        // Use WSL to resolve $HOME when the UNC path is unavailable.
        let unc = format!(
            "\\\\wsl.localhost\\{distro}\\home\\{}\\.local\\share\\terminal-commanderd\\terminal-commanderd.sock",
            wsl_username(distro)?
        );
        if std::path::Path::new(&unc).exists() {
            return Ok(PathBuf::from(unc));
        }
        let _ = home;
    }
    Err(RouteError::Bootstrap(format!(
        "runner socket not found for distro '{distro}'; run npm install -g terminal-commander inside WSL"
    )))
}

#[cfg(windows)]
fn wsl_username(distro: &str) -> Result<String, RouteError> {
    use std::process::Command;
    let out = Command::new("wsl.exe")
        .args(["-d", distro, "--", "bash", "-lc", "whoami"])
        .output()
        .map_err(|e| RouteError::Bootstrap(format!("wsl whoami: {e}")))?;
    if !out.status.success() {
        return Err(RouteError::Bootstrap(format!(
            "wsl whoami failed for distro '{distro}'"
        )));
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if name.is_empty() {
        return Err(RouteError::Bootstrap("empty WSL username".to_owned()));
    }
    Ok(name)
}
