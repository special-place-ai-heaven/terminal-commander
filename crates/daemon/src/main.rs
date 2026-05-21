// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commanderd`: the Terminal Commander daemon.
//!
//! Source-status: live runtime bootstrap (TC36). UDS IPC (TC37) and
//! rmcp stdio (TC40) replace `start` / extend `start --foreground`.
//! The TC04 scaffold-only `eprintln!` is gone.
//!
//! Subcommands:
//!
//! - `check`         — bootstrap + self-check report, exit. No IPC.
//! - `start`         — bootstrap + foreground idle until shutdown
//!                     signal. No IPC. (TC37 adds the UDS accept loop.)
//! - `print-config`  — render the active resolved config back to TOML.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use terminal_commanderd::{DaemonConfig, RuntimeError, run_foreground_idle, run_self_check};

#[derive(Debug, Parser)]
#[command(
    name = "terminal-commanderd",
    version,
    about = "Terminal Commander daemon",
    long_about = "Local daemon for the Terminal Commander realtime signal channel.\n\
                  Initializes the persistent event store, audit log, policy engine,\n\
                  and in-memory subsystems. Does NOT open network listeners.\n\
                  Does NOT spawn child commands by itself. Foreground-only at TC36."
)]
struct Cli {
    /// Path to a daemon config TOML. If omitted, defaults are used
    /// rooted at the data-dir argument.
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Override `daemon.data_dir`. Useful for `check` / tests.
    /// MUST NOT be under `/mnt/c/...` on WSL2.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Bootstrap runtime, run a self-check, print the report, and exit.
    Check,
    /// Bootstrap runtime and idle in foreground until SIGINT/SIGTERM.
    /// Does NOT open IPC. Does NOT spawn commands.
    Start,
    /// Resolve config and print it back as TOML. Useful for verifying
    /// what the daemon would actually load.
    PrintConfig,
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
        Cmd::Start => {
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
            match rt.block_on(run_foreground_idle(cfg)) {
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

fn resolve_config(cli: &Cli) -> Result<DaemonConfig, ExitCode> {
    if let Some(p) = cli.config.as_ref() {
        let mut cfg = DaemonConfig::load(p).map_err(|e| {
            eprintln!("terminal-commanderd: config load error: {e}");
            ExitCode::from(1)
        })?;
        if let Some(dd) = cli.data_dir.as_ref() {
            cfg.daemon.data_dir.clone_from(dd);
        }
        return Ok(cfg);
    }
    if let Some(dd) = cli.data_dir.as_ref() {
        return Ok(DaemonConfig::defaults_in(dd));
    }
    // No --config, no --data-dir: try the platform default. If it
    // does not exist, fall back to a defaults-in `~/.local/share/
    // terminal-commanderd` (which is what the example config
    // ships). Operators who want a different location must pass
    // --config or --data-dir.
    let default_data = platform_default_data_dir();
    Ok(DaemonConfig::defaults_in(default_data))
}

#[allow(clippy::option_if_let_else)] // three-arm cascade is clearer as if/else if/else
fn platform_default_data_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share/terminal-commanderd")
    } else if let Ok(up) = std::env::var("USERPROFILE") {
        // Windows host (not WSL): provide a working default so the
        // self-check / print-config path does not panic for CI on
        // Windows runners. The store layer still enforces the 9P
        // rejection where applicable.
        PathBuf::from(up).join(".terminal-commanderd")
    } else {
        PathBuf::from(".terminal-commanderd")
    }
}
