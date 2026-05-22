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

    use clap::Parser;
    use rmcp::{ServiceExt, transport::stdio};
    use terminal_commander_mcp::daemon_client::{McpDaemonClient, resolve_socket_path};
    use terminal_commander_mcp::tools::TerminalCommanderMcpServer;

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
    }

    #[allow(unreachable_pub)]
    pub fn run() -> ExitCode {
        let cli = Cli::parse();
        let socket_path = resolve_socket_path(cli.socket.as_deref());
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
            let daemon = McpDaemonClient::new(socket_path);
            let server = TerminalCommanderMcpServer::new(daemon);
            let service = match server.serve(stdio()).await {
                Ok(svc) => svc,
                Err(e) => {
                    eprintln!("terminal-commander-mcp: stdio serve failed: {e}");
                    return ExitCode::from(2);
                }
            };
            if let Err(e) = service.waiting().await {
                eprintln!("terminal-commander-mcp: service exited with error: {e}");
                return ExitCode::from(2);
            }
            ExitCode::SUCCESS
        })
    }
}
