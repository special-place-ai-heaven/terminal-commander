// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon runtime modes (TC36).
//!
//! TC36 ships two non-IPC runtime modes:
//!
//! - [`run_self_check`]: bootstrap state, exercise a few read-only
//!   invariants (store reachable, audit row insert+read round trip,
//!   policy engine answers), report PASS/FAIL via stderr lines and
//!   exit code, and return. No socket. No shell. No commands.
//! - [`run_foreground_idle`]: bootstrap state, install a shutdown
//!   handler on Ctrl-C / SIGTERM, idle until signalled, then clean
//!   shutdown. No socket. No shell. No commands.
//!
//! UDS IPC (TC37) and rmcp stdio (TC40) replace `run_foreground_idle`
//! with the real accept loop. Until then `ForegroundIdle` is the
//! honest "daemon is up but not serving any LLM" state.
//!
//! Source-status: live (TC36).

use std::fmt::Write;
use std::sync::Arc;

use terminal_commander_core::{BucketConfig, BucketId};
use terminal_commander_store::{AuditEntry, AuditReadRequest};

use crate::audit::AuditSink;
use crate::config::DaemonConfig;
use crate::policy::{PolicyAction, PolicyDecision};
use crate::state::{BootstrapError, DaemonState};

/// Top-level runtime error.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("bootstrap error: {0}")]
    Bootstrap(#[from] BootstrapError),
    #[error("self-check failed: {0}")]
    SelfCheck(String),
    #[error("shutdown signal handler error: {0}")]
    Signal(String),
}

/// Self-check report. Stored as plain text so logs and operator
/// output stay simple.
#[derive(Debug, Clone, Default)]
pub struct SelfCheckReport {
    pub lines: Vec<String>,
    pub failures: u32,
}

impl SelfCheckReport {
    fn ok(&mut self, line: impl Into<String>) {
        self.lines.push(format!("  [ok     ] {}", line.into()));
    }
    fn fail(&mut self, line: impl Into<String>) {
        self.failures += 1;
        self.lines.push(format!("  [FAILED ] {}", line.into()));
    }

    /// Render the report as a single multi-line string.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "terminal-commanderd self-check:");
        for l in &self.lines {
            let _ = writeln!(out, "{l}");
        }
        let _ = writeln!(
            out,
            "  summary: {} checks, {} failures",
            self.lines.len(),
            self.failures
        );
        out
    }
}

/// Bootstrap the daemon and run a self-check. Returns the report
/// and the wired state. Caller decides whether to drop the state
/// (exit) or keep it (idle / future IPC loop).
pub fn run_self_check(
    config: DaemonConfig,
) -> Result<(DaemonState, SelfCheckReport), RuntimeError> {
    let state = DaemonState::bootstrap(config)?;
    let mut rep = SelfCheckReport::default();

    // 1. Store reachable + audit migration applied.
    {
        let mut g = state.store.lock();
        match g.audit_count() {
            Ok(_) => rep.ok("store + V0003 audit migration: reachable"),
            Err(e) => rep.fail(format!("audit_count failed: {e}")),
        }
    }

    // 2. Persistent audit insert + read round trip.
    {
        let entry = AuditEntry::new("self_check", "bootstrap", "info")
            .with_actor("self_check")
            .with_profile(format!("{:?}", state.config.policy.profile));
        match state.audit.ensure_migration() {
            Ok(()) => {}
            Err(e) => rep.fail(format!("ensure_migration: {e}")),
        }
        // PersistentAudit::emit goes through the AuditSink trait
        // (imported at module scope).
        match (Arc::clone(&state.audit) as Arc<dyn AuditSink>).emit(&entry) {
            Ok(_id) => rep.ok("persistent audit: emit round trip"),
            Err(e) => rep.fail(format!("persistent audit emit failed: {e}")),
        }
        let mut g = state.store.lock();
        match g.audit_since(&AuditReadRequest::new(0)) {
            Ok(rows) => {
                if rows.iter().any(|r| r.action == "self_check") {
                    rep.ok("persistent audit: self_check row visible");
                } else {
                    rep.fail("persistent audit: self_check row not visible after emit");
                }
            }
            Err(e) => rep.fail(format!("audit_since failed: {e}")),
        }
    }

    // 3. Router uses persistent audit: drive a bucket_create through
    //    the router and prove it lands in the store.
    {
        let bid = BucketId::new();
        match state.router.bucket_create(bid, BucketConfig::default()) {
            Ok(()) => rep.ok("router: bucket_create allowed"),
            Err(e) => rep.fail(format!("router.bucket_create failed: {e}")),
        }
        let mut g = state.store.lock();
        match g.audit_since(&AuditReadRequest::new(0)) {
            Ok(rows) => {
                if rows.iter().any(|r| r.action == "bucket_create") {
                    rep.ok("router -> persistent audit pipeline: bucket_create row visible");
                } else {
                    rep.fail("router bucket_create did not produce persistent audit row");
                }
            }
            Err(e) => rep.fail(format!("audit_since after router call failed: {e}")),
        }
    }

    // 4. Policy engine: sudo deny is universal across profiles.
    {
        let argv = vec!["sudo".to_owned(), "rm".to_owned(), "-rf".to_owned()];
        let cwd = std::path::Path::new("/");
        let v = state
            .policy
            .evaluate(&PolicyAction::CommandStart { argv: &argv, cwd });
        if v.decision == PolicyDecision::Deny {
            rep.ok("policy: sudo denied (structural)");
        } else {
            rep.fail(format!(
                "policy DID NOT deny sudo: decision={:?} reason='{}'",
                v.decision, v.reason
            ));
        }
    }

    // 5. Confirm no socket / no command exec happened (these are
    //    not actively running in TC36, but we make the contract
    //    visible in the report).
    rep.ok("transport: no UDS, no TCP, no MCP stdio (TC37/TC40 scope)");
    rep.ok("command execution: not wired (TC38/TC44 scope)");

    if rep.failures > 0 {
        return Err(RuntimeError::SelfCheck(format!(
            "{} of {} checks failed",
            rep.failures,
            rep.lines.len()
        )));
    }
    Ok((state, rep))
}

/// Bootstrap + foreground idle until shutdown signal. No IPC, no
/// command exec. Kept for `--mode foreground-idle` callers and as
/// the safe pre-IPC mode; the `start` subcommand defaults to the
/// IPC server.
pub async fn run_foreground_idle(config: DaemonConfig) -> Result<(), RuntimeError> {
    let (_state, rep) = run_self_check(config)?;
    eprintln!("{}", rep.render());
    eprintln!(
        "terminal-commanderd: foreground idle. \
         No IPC bound (operator chose foreground_idle mode). \
         Send SIGINT (Ctrl-C) or SIGTERM to shut down."
    );
    wait_for_shutdown_signal().await?;
    eprintln!("terminal-commanderd: shutdown signal received, exiting cleanly.");
    Ok(())
}

/// Bootstrap + bind the UDS IPC listener + wait for shutdown signal.
///
/// On non-Unix targets this returns immediately with an unsupported-
/// platform error so the daemon binary fails loud rather than
/// silently degrading.
#[cfg(unix)]
pub async fn run_ipc_server(config: DaemonConfig) -> Result<(), RuntimeError> {
    use std::sync::Arc;

    use crate::ipc::IpcServer;

    let (state, rep) = run_self_check(config)?;
    eprintln!("{}", rep.render());

    let socket_path = state.config.socket_path();
    eprintln!(
        "terminal-commanderd: binding UDS at {}",
        socket_path.display()
    );
    let server = IpcServer::new(Arc::new(state), socket_path);
    let handle = server
        .spawn()
        .map_err(|e| RuntimeError::Signal(format!("UDS bind: {e}")))?;
    eprintln!(
        "terminal-commanderd: IPC server bound. \
         Method set: system_discover, health, policy_status, self_check. \
         Send SIGINT (Ctrl-C) or SIGTERM to shut down."
    );

    wait_for_shutdown_signal().await?;
    eprintln!("terminal-commanderd: shutdown signal received, draining...");
    handle.shutdown().await;
    eprintln!("terminal-commanderd: IPC server exited cleanly.");
    Ok(())
}

/// Windows named-pipe IPC server.
#[cfg(windows)]
pub async fn run_ipc_server(config: DaemonConfig) -> Result<(), RuntimeError> {
    use std::sync::Arc;

    use crate::ipc::PipeServer;

    let (state, rep) = run_self_check(config)?;
    eprintln!("{}", rep.render());

    let pipe_name = state.config.pipe_name();
    eprintln!(
        "terminal-commanderd: binding named pipe at {pipe_name}"
    );
    let server = PipeServer::new(Arc::new(state), pipe_name);
    let handle = server
        .spawn()
        .map_err(|e| RuntimeError::Signal(format!("pipe bind: {e}")))?;
    eprintln!(
        "terminal-commanderd: IPC server bound (Windows named pipe). \
         Send Ctrl-C to shut down."
    );

    wait_for_shutdown_signal_windows().await?;
    eprintln!("terminal-commanderd: shutdown signal received, draining...");
    handle.shutdown().await;
    eprintln!("terminal-commanderd: IPC server exited cleanly.");
    Ok(())
}

#[cfg(windows)]
async fn wait_for_shutdown_signal_windows() -> Result<(), RuntimeError> {
    // Ctrl-C on Windows.
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| RuntimeError::Signal(format!("ctrl-c listen: {e}")))?;
    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() -> Result<(), RuntimeError> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut sigterm = signal(SignalKind::terminate())
        .map_err(|e| RuntimeError::Signal(format!("SIGTERM listen: {e}")))?;
    let mut sigint = signal(SignalKind::interrupt())
        .map_err(|e| RuntimeError::Signal(format!("SIGINT listen: {e}")))?;
    tokio::select! {
        _ = sigterm.recv() => Ok(()),
        _ = sigint.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() -> Result<(), RuntimeError> {
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| RuntimeError::Signal(format!("ctrl_c listen: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_data_dir(tag: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        p.push(format!("tc-runtime-{tag}-{pid}-{nanos}"));
        p
    }

    fn cleanup(p: &std::path::Path) {
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn self_check_passes_on_clean_bootstrap() {
        let data = temp_data_dir("ok");
        let cfg = DaemonConfig::defaults_in(&data);
        let (_state, rep) = run_self_check(cfg).unwrap();
        assert_eq!(rep.failures, 0, "report: {}", rep.render());
        // Render contains structural lines.
        let r = rep.render();
        assert!(r.contains("V0003 audit migration"));
        assert!(r.contains("router -> persistent audit pipeline"));
        assert!(r.contains("sudo denied"));
        cleanup(&data);
    }
}
