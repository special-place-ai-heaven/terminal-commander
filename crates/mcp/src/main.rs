// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commander-mcp`: real rmcp stdio MCP adapter (TC40).
//!
//! Boots an rmcp `ServerHandler` over the stdio transport and forwards
//! every tool call through the daemon UDS client. This crate:
//!
//! - does NOT spawn child commands (`Command::spawn` is forbidden),
//! - does NOT open network sockets (`TcpListener`/`UdpSocket` forbidden),
//! - does NOT open files outside its own config / daemon UDS path,
//! - exposes ONLY discovery / status tools at TC40
//!   (`system_discover`, `health`, `policy_status`, `self_check`).
//!
//! Platform: Unix only. On non-Unix targets the binary refuses to
//! start (the daemon UDS transport is Unix-only). Use WSL2 on Windows.

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    unix_main::run()
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "terminal-commander-mcp: requires Unix (Linux/macOS/WSL2). \
         Windows native targets are not supported because the daemon \
         IPC uses a Unix domain socket. Refusing to start."
    );
    std::process::ExitCode::from(64)
}

#[cfg(unix)]
mod unix_main {
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::time::Duration;

    use clap::Parser;
    use rmcp::{ServiceExt, transport::stdio};
    use terminal_commander_mcp::daemon_client::{
        DaemonStatusHandle, McpDaemonClient, resolve_socket_path,
    };
    use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
    use terminal_commander_supervisor::ensure::{
        Endpoint, EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon,
    };

    /// Default daemon startup timeout.
    const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

    #[derive(Debug, Parser)]
    #[command(
        name = "terminal-commander-mcp",
        version,
        about = "Terminal Commander MCP stdio adapter",
        long_about = "Thin MCP stdio adapter for the Terminal Commander daemon.\n\
                      Serves a small discovery/status tool surface over MCP and\n\
                      forwards every call to the daemon over its local UDS.\n\
                      Does NOT spawn commands, open network sockets, or read\n\
                      arbitrary files."
    )]
    struct Cli {
        /// Explicit path to the daemon UDS socket. Overrides
        /// `TC_SOCKET` and the default location.
        #[arg(long)]
        socket: Option<PathBuf>,

        /// Explicit path to the daemon binary. When omitted, the adapter
        /// looks for `terminal-commanderd` next to its own executable.
        #[arg(long)]
        daemon_binary: Option<PathBuf>,

        /// Explicit state / data directory for the daemon. Defaults to
        /// `$HOME/.local/share/terminal-commanderd`.
        #[arg(long)]
        state_dir: Option<PathBuf>,
    }

    /// Derive the daemon binary path: CLI flag > sibling of current exe >
    /// `terminal-commanderd` on PATH (fallback — always returns something).
    fn resolve_daemon_binary(cli_override: Option<PathBuf>) -> PathBuf {
        if let Some(p) = cli_override {
            return p;
        }
        if let Ok(exe) = std::env::current_exe() {
            let sibling = exe
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("terminal-commanderd");
            if sibling.exists() {
                return sibling;
            }
        }
        PathBuf::from("terminal-commanderd")
    }

    /// Derive the daemon state directory: CLI flag > `$HOME/.local/share/terminal-commanderd`.
    fn resolve_state_dir(cli_override: Option<PathBuf>) -> PathBuf {
        if let Some(p) = cli_override {
            return p;
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("terminal-commanderd");
        }
        PathBuf::from(".terminal-commanderd")
    }

    /// Return true when `TC_SUPERVISOR_ALLOW_SPAWN` is NOT set to `"0"`.
    fn allow_spawn() -> bool {
        std::env::var("TC_SUPERVISOR_ALLOW_SPAWN")
            .map(|v| v != "0")
            .unwrap_or(true)
    }

    #[allow(unreachable_pub)]
    pub fn run() -> ExitCode {
        let cli = Cli::parse();
        let socket_path = resolve_socket_path(cli.socket.as_deref());
        let daemon_binary = resolve_daemon_binary(cli.daemon_binary);
        let state_dir = resolve_state_dir(cli.state_dir);
        let log_dir = state_dir.join("logs");
        let allow_spawn = allow_spawn();

        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("terminal-commander-mcp: tokio runtime build failed: {e}");
                return ExitCode::from(2);
            }
        };

        rt.block_on(async move {
            // Step 1: call ensure_daemon; keep stdio alive regardless of outcome.
            let endpoint = Endpoint::UnixSocket {
                path: socket_path.clone(),
            };
            let opts = EnsureDaemonOptions {
                daemon_binary,
                state_dir,
                log_dir,
                endpoint,
                startup_timeout: STARTUP_TIMEOUT,
                allow_spawn,
            };
            let status = ensure_daemon(opts).await;

            // Log availability to stderr (goes to the operator, not to the MCP
            // client which is on stdout).
            match &status {
                EnsureDaemonStatus::AlreadyRunning { .. } => {
                    eprintln!("terminal-commander-mcp: daemon already running");
                }
                EnsureDaemonStatus::Started { log_path, .. } => {
                    eprintln!(
                        "terminal-commander-mcp: daemon started; log: {}",
                        log_path.display()
                    );
                }
                EnsureDaemonStatus::Unavailable {
                    reason,
                    diagnostics,
                } => {
                    eprintln!(
                        "terminal-commander-mcp: daemon unavailable ({reason:?}); \
                         continuing in degraded mode. last_error: {:?}",
                        diagnostics.last_error
                    );
                }
            }

            let status_handle = DaemonStatusHandle::new(status);
            let daemon = McpDaemonClient::with_status(socket_path, status_handle);
            let server = TerminalCommanderMcpServer::new(daemon);

            let service = match server.serve(stdio()).await {
                Ok(svc) => svc,
                Err(e) => {
                    eprintln!("terminal-commander-mcp: stdio serve failed: {e}");
                    return ExitCode::from(2);
                }
            };

            // stdin EOF surfaces as an Err from service.waiting(); treat it as
            // clean shutdown — the MCP host closed the pipe.
            // stdin EOF surfaces as Err; normal quit as Ok(QuitReason).
            // Both are clean shutdown from the MCP host's perspective.
            let _ = service.waiting().await;
            ExitCode::SUCCESS
        })
    }
}
