// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

#![cfg_attr(windows, windows_subsystem = "windows")]

//! `terminal-commanderd`: the Terminal Commander daemon.
//!
//! Source-status: live runtime bootstrap (TC36) + UDS IPC (TC37) on
//! Unix. rmcp stdio (TC40) is the next transport.
//!
//! Subcommands:
//!
//! - `check`              — bootstrap + self-check report, exit. No IPC.
//! - `start`              — bootstrap + bind UDS + idle until shutdown.
//!                          Unix only. Method set is the TC37 minimum.
//!                          Use `--mode foreground-idle` to skip the
//!                          UDS bind (pre-IPC fallback).
//! - `print-config`       — render the active resolved config back to TOML.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use terminal_commanderd::{
    DaemonConfig, RuntimeError, run_foreground_idle, run_ipc_server, run_self_check,
};

#[derive(Debug, Parser)]
#[command(
    name = "terminal-commanderd",
    version,
    about = "Terminal Commander daemon",
    long_about = "Local daemon for the Terminal Commander realtime signal channel.\n\
                  Initializes the persistent event store, audit log, policy engine,\n\
                  and in-memory subsystems. On Unix, binds a local UDS for the\n\
                  TC37 minimal IPC method set. Does NOT open network listeners.\n\
                  Does NOT spawn child commands by itself."
)]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Bootstrap runtime, run a self-check, print the report, exit.
    Check,
    /// Bootstrap runtime and run a non-exit daemon mode. Defaults to
    /// `ipc-server` (named pipe on Windows, UDS on Unix).
    Start {
        /// Override the daemon run mode for this invocation.
        #[arg(long, value_enum)]
        mode: Option<StartMode>,
    },
    /// Resolve config and print it back as TOML.
    PrintConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum StartMode {
    /// Bind the UDS IPC listener and accept connections. Unix only.
    IpcServer,
    /// Idle in foreground without any IPC binding.
    ForegroundIdle,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let cfg = match resolve_config(&cli) {
        Ok(c) => c,
        Err(code) => return code,
    };

    match cli.cmd {
        Cmd::Check => match run_self_check(cfg) {
            Ok((_state, rep)) => {
                eprintln!("{}", rep.render());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("terminal-commanderd: self-check failed: {e}");
                ExitCode::from(1)
            }
        },
        Cmd::Start { mode } => {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("terminal-commanderd: tokio runtime build failed: {e}");
                    return ExitCode::from(2);
                }
            };
            let mode = mode.unwrap_or(default_start_mode());
            let result = match mode {
                StartMode::IpcServer => rt.block_on(run_ipc_server(cfg)),
                StartMode::ForegroundIdle => rt.block_on(run_foreground_idle(cfg)),
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(RuntimeError::SelfCheck(msg)) => {
                    eprintln!("terminal-commanderd: bootstrap self-check failed: {msg}");
                    ExitCode::from(1)
                }
                Err(e) => {
                    eprintln!("terminal-commanderd: runtime error: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Cmd::PrintConfig => {
            println!("{}", terminal_commanderd::config::to_toml(&cfg));
            ExitCode::SUCCESS
        }
    }
}

const fn default_start_mode() -> StartMode {
    StartMode::IpcServer
}

fn resolve_config(cli: &Cli) -> Result<DaemonConfig, ExitCode> {
    let mut cfg = if let Some(p) = cli.config.as_ref() {
        let mut loaded = DaemonConfig::load(p).map_err(|e| {
            eprintln!("terminal-commanderd: config load error: {e}");
            ExitCode::from(1)
        })?;
        if let Some(dd) = cli.data_dir.as_ref() {
            loaded.daemon.data_dir.clone_from(dd);
        }
        loaded
    } else if let Some(dd) = cli.data_dir.as_ref() {
        DaemonConfig::defaults_in(dd)
    } else {
        DaemonConfig::defaults_in(platform_default_data_dir())
    };
    apply_socket_env_override(&mut cfg);
    Ok(cfg)
}

/// Per-harness session supervisor sets `TC_SOCKET` to an isolated UDS or pipe.
fn apply_socket_env_override(cfg: &mut DaemonConfig) {
    if let Ok(socket) = std::env::var("TC_SOCKET")
        && !socket.is_empty()
    {
        cfg.daemon.socket_path = Some(PathBuf::from(socket));
    }
}

#[allow(clippy::option_if_let_else)] // three-arm cascade is clearer as if/else if/else
fn platform_default_data_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share/terminal-commanderd")
    } else if let Ok(up) = std::env::var("USERPROFILE") {
        PathBuf::from(up).join(".terminal-commanderd")
    } else {
        PathBuf::from(".terminal-commanderd")
    }
}
