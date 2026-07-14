// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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
                  Does NOT spawn arbitrary child commands by itself; the one\n\
                  exception is the hidden `selfcheck-noop` leaf, an inert\n\
                  internal self-probe target the IPC self_check spawns to prove\n\
                  the command pipeline is live."
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
    /// Replace a stale (or, with --force, any) running daemon with this
    /// binary, then ensure a current daemon is running. Reads the running
    /// daemon version from its pidfile; if older than this binary (or no
    /// pidfile, meaning a pre-feature daemon), kills it and starts this
    /// one. Then exits.
    Update {
        /// Replace even when the running daemon version equals this
        /// binary (a forced restart). The endpoint cross-check still
        /// applies, so this never kills a process on a different socket.
        #[arg(long)]
        force: bool,
    },
    /// Hidden internal self-probe target (TC-5). An inert leaf the IPC
    /// `self_check` spawns to prove the command pipeline can launch a
    /// real child to completion. It does ZERO arg/env/config/socket/fs/
    /// policy work and exits 0 immediately; `fn main` short-circuits it
    /// BEFORE `resolve_config` so a broken inherited `--config`/env can
    /// never make the probe false-fail. Not part of the operator CLI.
    #[command(hide = true)]
    SelfcheckNoop,
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

    // TC-5: the hidden self-probe leaf is inert and MUST short-circuit
    // BEFORE `resolve_config`. A child spawned by the IPC self_check
    // inherits this daemon's `--config`/env; if that config were broken,
    // running `resolve_config` here would exit nonzero and turn a healthy
    // self-probe into a false RED. The leaf does NO arg/env/config/socket/
    // fs/policy work -- it just exits 0 so the parent observes a clean
    // round-trip.
    if matches!(cli.cmd, Cmd::SelfcheckNoop) {
        return ExitCode::SUCCESS;
    }

    let cfg = match resolve_config(&cli) {
        Ok(c) => c,
        Err(code) => return code,
    };

    match cli.cmd {
        Cmd::Check => match run_self_check(cfg) {
            Ok((state, rep)) => {
                eprintln!("{}", rep.render());
                let _ = state.store.shutdown();
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
        Cmd::Update { force } => {
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
            rt.block_on(run_update(&cfg, force))
        }
        // Unreachable: `SelfcheckNoop` is short-circuited above, BEFORE
        // `resolve_config`, so control never arrives here. The arm exists
        // only to keep the match exhaustive; it does NO config/policy work.
        Cmd::SelfcheckNoop => unreachable!("selfcheck-noop short-circuited before resolve_config"),
    }
}

/// `update` run-mode: replace a stale running daemon with this binary,
/// then ensure a current daemon is running. Single-flights the whole
/// replace -> spawn under one cross-process lock (H6) via
/// `ensure_or_replace`, so a concurrent adapter cannot bind a competing
/// daemon in the kill -> spawn window. Prints the resulting status and
/// exits 0 on success.
async fn run_update(cfg: &DaemonConfig, force: bool) -> ExitCode {
    use terminal_commander_supervisor::ensure::EnsureDaemonOptions;
    use terminal_commander_supervisor::paths::endpoint_from_socket_path;
    use terminal_commander_supervisor::replace::ensure_or_replace;

    let installed = env!("CARGO_PKG_VERSION");
    let daemon_binary = std::env::current_exe().unwrap_or_else(|_| "terminal-commanderd".into());
    let state_dir = cfg.daemon.data_dir.clone();
    // Resolve the endpoint the daemon actually binds: a named pipe on
    // Windows, a UDS path on Unix. Using socket_path() on Windows would
    // yield a bogus .sock UnixSocket endpoint the daemon never binds.
    #[cfg(windows)]
    let endpoint = endpoint_from_socket_path(std::path::Path::new(&cfg.pipe_name()));
    #[cfg(unix)]
    let endpoint = endpoint_from_socket_path(&cfg.socket_path());
    let log_dir = terminal_commander_supervisor::paths::resolve_log_path()
        .parent()
        .map_or_else(|| state_dir.clone(), std::path::Path::to_path_buf);

    let opts = EnsureDaemonOptions {
        daemon_binary,
        state_dir,
        log_dir,
        endpoint,
        startup_timeout: std::time::Duration::from_secs(10),
        allow_spawn: true,
    };

    // Single-flight: one lock spans the (probe -> kill -> spawn) sequence
    // so two `update` runs (or an `update` racing an adapter cold start)
    // cannot orphan each other's daemon.
    let status = ensure_or_replace(&opts, installed, force).await;
    report_ensure(&status)
}

fn report_ensure(status: &terminal_commander_supervisor::ensure::EnsureDaemonStatus) -> ExitCode {
    use terminal_commander_supervisor::ensure::EnsureDaemonStatus;
    match status {
        EnsureDaemonStatus::AlreadyRunning { .. } | EnsureDaemonStatus::Started { .. } => {
            println!("terminal-commanderd: daemon running");
            ExitCode::SUCCESS
        }
        EnsureDaemonStatus::Unavailable { reason, .. } => {
            eprintln!("terminal-commanderd: daemon unavailable after update: {reason:?}");
            ExitCode::from(2)
        }
    }
}

const fn default_start_mode() -> StartMode {
    StartMode::IpcServer
}

const DEFAULT_CONFIG_FILENAME: &str = "terminal-commander.toml";

fn load_data_dir_config(
    data_dir: &std::path::Path,
) -> terminal_commanderd::config::Result<DaemonConfig> {
    let config_path = data_dir.join(DEFAULT_CONFIG_FILENAME);
    if !config_path.is_file() {
        return Ok(DaemonConfig::defaults_in(data_dir));
    }

    let mut config = DaemonConfig::load(config_path)?;
    config.daemon.data_dir = data_dir.to_path_buf();
    Ok(config)
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
    } else {
        // F5: the selected data dir is the supervisor/daemon rendezvous source
        // of truth. A conventional config inside it may set policy, but cannot
        // redirect the daemon's state away from the endpoint being supervised.
        let data_dir = cli
            .data_dir
            .clone()
            .unwrap_or_else(terminal_commanderd::config::default_state_dir);
        load_data_dir_config(&data_dir).map_err(|e| {
            eprintln!("terminal-commanderd: config load error: {e}");
            ExitCode::from(1)
        })?
    };
    apply_socket_env_override(&mut cfg);
    apply_idle_ttl_env_override(&mut cfg);
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

/// Operator may set `TC_IDLE_TTL_SECS` to override the idle self-reap TTL
/// without editing the config file. `0` disables the idle-timer entirely.
/// On parse failure we warn and keep the existing value (mirrors how other
/// `TC_*` env handling fails open).
fn apply_idle_ttl_env_override(cfg: &mut DaemonConfig) {
    if let Ok(raw) = std::env::var("TC_IDLE_TTL_SECS")
        && !raw.is_empty()
    {
        match raw.parse::<u64>() {
            Ok(v) => cfg.daemon.idle_ttl_secs = v,
            Err(e) => eprintln!(
                "terminal-commanderd: ignoring invalid TC_IDLE_TTL_SECS={raw:?}: {e} \
                 (keeping idle_ttl_secs={})",
                cfg.daemon.idle_ttl_secs
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commanderd::config::DEFAULT_IDLE_TTL_SECS;

    // Env-var tests must serialize: cargo runs tests in parallel by
    // default and process-global env can race otherwise.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn cfg_with_default_idle() -> DaemonConfig {
        DaemonConfig::defaults_in(std::env::temp_dir().join("tc-idle-env-test"))
    }

    #[test]
    fn idle_ttl_env_zero_disables_timer() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: serialized by ENV_LOCK above; tests in this module are the
        // only writers of TC_IDLE_TTL_SECS.
        unsafe {
            std::env::set_var("TC_IDLE_TTL_SECS", "0");
        }
        let mut cfg = cfg_with_default_idle();
        assert_eq!(cfg.daemon.idle_ttl_secs, DEFAULT_IDLE_TTL_SECS);
        apply_idle_ttl_env_override(&mut cfg);
        assert_eq!(
            cfg.daemon.idle_ttl_secs, 0,
            "TC_IDLE_TTL_SECS=0 must disable"
        );
        unsafe {
            std::env::remove_var("TC_IDLE_TTL_SECS");
        }
    }

    #[test]
    fn idle_ttl_env_valid_override() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        unsafe {
            std::env::set_var("TC_IDLE_TTL_SECS", "42");
        }
        let mut cfg = cfg_with_default_idle();
        apply_idle_ttl_env_override(&mut cfg);
        assert_eq!(cfg.daemon.idle_ttl_secs, 42);
        unsafe {
            std::env::remove_var("TC_IDLE_TTL_SECS");
        }
    }

    #[test]
    fn idle_ttl_env_invalid_keeps_existing_value() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        unsafe {
            std::env::set_var("TC_IDLE_TTL_SECS", "not-a-number");
        }
        let mut cfg = cfg_with_default_idle();
        let before = cfg.daemon.idle_ttl_secs;
        apply_idle_ttl_env_override(&mut cfg);
        assert_eq!(
            cfg.daemon.idle_ttl_secs, before,
            "invalid TC_IDLE_TTL_SECS must keep the existing value"
        );
        unsafe {
            std::env::remove_var("TC_IDLE_TTL_SECS");
        }
    }

    #[test]
    fn idle_ttl_env_unset_is_a_noop() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        unsafe {
            std::env::remove_var("TC_IDLE_TTL_SECS");
        }
        let mut cfg = cfg_with_default_idle();
        let before = cfg.daemon.idle_ttl_secs;
        apply_idle_ttl_env_override(&mut cfg);
        assert_eq!(cfg.daemon.idle_ttl_secs, before);
    }

    #[test]
    fn data_dir_startup_loads_conventional_config_and_keeps_selected_data_dir() {
        let data_dir =
            std::env::temp_dir().join(format!("tc-data-dir-config-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data_dir);
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::write(
            data_dir.join("terminal-commander.toml"),
            r#"
                [daemon]
                data_dir = "ignored-by-data-dir-startup"

                [policy]
                profile = "developer_local"

                [policy.caps]
                allow_shell = true
            "#,
        )
        .unwrap();

        let cfg = load_data_dir_config(&data_dir).unwrap();

        assert!(cfg.resolved_caps().allow_shell);
        assert_eq!(cfg.daemon.data_dir, data_dir);
        std::fs::remove_dir_all(&cfg.daemon.data_dir).unwrap();
    }
}
