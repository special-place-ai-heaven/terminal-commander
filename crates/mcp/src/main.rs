// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commander-mcp`: real rmcp stdio MCP adapter.
//!
//! Boots an rmcp `ServerHandler` over the stdio transport and forwards
//! every tool call through the daemon IPC client. This crate:
//!
//! - does NOT spawn child commands (`Command::spawn` is forbidden),
//! - does NOT open network sockets (`TcpListener`/`UdpSocket` forbidden),
//! - does NOT open files outside its own config / daemon IPC path,
//! - exposes the full daemon tool surface (29 tools), each forwarded
//!   1:1 to a daemon IPC method.
//!
//! Platform: Unix (UDS) and Windows (named pipe). Both transports are
//! handled by `terminal_commanderd::DaemonClient` and the supervisor
//! path resolver; the binary starts correctly on both platforms.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use rmcp::{ServiceExt, service::ServerInitializeError, transport::stdio};
use terminal_commander_mcp::daemon_client::{
    DaemonStatusHandle, McpDaemonClient, resolve_socket_path,
};
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commander_supervisor::ensure::{
    EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon,
};
use terminal_commander_supervisor::paths::{self, endpoint_from_socket_path};

/// Default daemon startup timeout.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Parser)]
#[command(
    name = "terminal-commander-mcp",
    version,
    about = "Terminal Commander MCP stdio adapter",
    long_about = "Thin MCP stdio adapter for the Terminal Commander daemon.\n\
                  Serves the daemon tool surface over MCP and\n\
                  forwards every call to the daemon over its local IPC.\n\
                  Does NOT spawn commands, open network sockets, or read\n\
                  arbitrary files."
)]
struct Cli {
    /// Explicit path to the daemon IPC endpoint. Overrides
    /// `TC_SOCKET` and the default location.
    #[arg(long)]
    socket: Option<PathBuf>,

    /// Explicit path to the daemon binary. When omitted, the adapter
    /// looks for `terminal-commanderd` next to its own executable.
    #[arg(long)]
    daemon_binary: Option<PathBuf>,

    /// Explicit state / data directory for the daemon. Defaults to
    /// the platform canonical default matching the daemon.
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
        #[cfg(windows)]
        let sibling_name = "terminal-commanderd.exe";
        #[cfg(not(windows))]
        let sibling_name = "terminal-commanderd";
        let sibling = exe
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(sibling_name);
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from("terminal-commanderd")
}

/// Derive the daemon state directory: CLI flag > supervisor canonical default.
///
/// Delegates to [`terminal_commander_supervisor::paths::resolve_state_dir`] for
/// the default, which matches `DaemonConfig`'s startup path exactly.
fn resolve_state_dir(cli_override: Option<PathBuf>) -> PathBuf {
    if let Some(p) = cli_override {
        return p;
    }
    paths::resolve_state_dir()
}

/// Return true when `TC_SUPERVISOR_ALLOW_SPAWN` is NOT set to `"0"`.
fn allow_spawn() -> bool {
    std::env::var("TC_SUPERVISOR_ALLOW_SPAWN").map_or(true, |v| v != "0")
}

fn main() -> ExitCode {
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
        // Use endpoint_from_socket_path to select the right Endpoint variant
        // (UnixSocket on Unix, WindowsPipe on Windows).
        let endpoint = endpoint_from_socket_path(&socket_path);
        let opts = EnsureDaemonOptions {
            daemon_binary,
            state_dir,
            log_dir,
            endpoint,
            startup_timeout: STARTUP_TIMEOUT,
            allow_spawn,
        };
        let mut status = ensure_daemon(opts.clone()).await;

        // Step 1b: if the running daemon is OLDER than this adapter, it
        // will reject new IPC methods with "early eof". Replace it with
        // the current binary, then re-ensure. Gated by allow_spawn (a
        // read-only adapter must not kill). Best-effort: a failed replace
        // logs and proceeds against whatever daemon answers.
        if allow_spawn && matches!(&status, EnsureDaemonStatus::AlreadyRunning { .. }) {
            match terminal_commander_supervisor::replace::replace_if_stale(
                &opts,
                env!("CARGO_PKG_VERSION"),
                // Auto-check on adapter start replaces only a STALE daemon,
                // never force-kills a current one.
                false,
            )
            .await
            {
                terminal_commander_supervisor::replace::ReplaceOutcome::Replaced { old, new } => {
                    eprintln!(
                        "terminal-commander-mcp: replaced stale daemon {old} -> {new}; respawning"
                    );
                    status = ensure_daemon(opts.clone()).await;
                }
                terminal_commander_supervisor::replace::ReplaceOutcome::Skipped { reason } => {
                    eprintln!("terminal-commander-mcp: stale-check skipped: {reason}");
                }
                _ => {}
            }
        }

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
            // stdin EOF before the MCP initialize handshake completes (the
            // transport closed before any message arrived), or cancellation
            // during init. Treat both as clean shutdown — the host dropped the
            // pipe immediately (e.g. the stdin_eof_survives test).
            Err(ServerInitializeError::ConnectionClosed(_) | ServerInitializeError::Cancelled) => {
                return ExitCode::SUCCESS;
            }
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
