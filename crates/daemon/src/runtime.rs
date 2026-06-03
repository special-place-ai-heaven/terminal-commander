// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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

/// Initialize a non-blocking file appender that writes to
/// `<data_dir>/logs/terminal-commanderd.log`.
///
/// Returns a [`tracing_appender::non_blocking::WorkerGuard`] that
/// must be kept alive for the duration of the process — dropping it
/// early will flush and close the writer prematurely.
///
/// Uses `try_init` so that a pre-existing global subscriber (e.g.
/// in integration tests) does not cause a panic.
fn init_file_logging(data_dir: &std::path::Path) -> tracing_appender::non_blocking::WorkerGuard {
    let log_dir = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::never(&log_dir, "terminal-commanderd.log");
    let (nb, guard) = tracing_appender::non_blocking(file_appender);
    let _ = tracing_subscriber::fmt()
        .with_writer(nb)
        .with_ansi(false)
        .with_target(false)
        .try_init();
    guard
}

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
    match state.store.audit_count() {
        Ok(_) => rep.ok("store + V0003 audit migration: reachable"),
        Err(e) => rep.fail(format!("audit_count failed: {e}")),
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
        match state.store.audit_since(&AuditReadRequest::new(0)) {
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
        match state.store.audit_since(&AuditReadRequest::new(0)) {
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

/// Drain the store actor (queued ops + WAL checkpoint) before process exit.
fn shutdown_store(state: &DaemonState) {
    match state.store.shutdown() {
        Ok(()) => tracing::debug!("store actor shutdown complete"),
        Err(e) => tracing::warn!("store actor shutdown failed (non-fatal): {e}"),
    }
}

/// Bootstrap + foreground idle until shutdown signal. No IPC, no
/// command exec. Kept for `--mode foreground-idle` callers and as
/// the safe pre-IPC mode; the `start` subcommand defaults to the
/// IPC server.
pub async fn run_foreground_idle(config: DaemonConfig) -> Result<(), RuntimeError> {
    let (state, rep) = run_self_check(config)?;
    eprintln!("{}", rep.render());
    eprintln!(
        "terminal-commanderd: foreground idle. \
         No IPC bound (operator chose foreground_idle mode). \
         Send SIGINT (Ctrl-C) or SIGTERM to shut down."
    );
    wait_for_shutdown_signal().await?;
    shutdown_store(&state);
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

    let _log_guard = init_file_logging(&config.daemon.data_dir);
    let state_dir = config.daemon.data_dir.clone();

    let (state, rep) = run_self_check(config)?;
    tracing::info!("{}", rep.render());

    let socket_path = state.config.socket_path();
    let endpoint = socket_path.display().to_string();
    tracing::info!("binding UDS at {endpoint}");
    // Keep an Arc handle to the state so we can await the internal
    // shutdown trigger (flipped by the `Shutdown` IPC dispatch arm)
    // alongside the OS-signal path below.
    let state = Arc::new(state);
    let server = IpcServer::new(Arc::clone(&state), socket_path);
    let handle = server
        .spawn()
        .map_err(|e| RuntimeError::Signal(format!("UDS bind: {e}")))?;
    write_daemon_pidfile(&state_dir, &endpoint);
    tracing::info!(
        "IPC server bound. \
         Method set: system_discover, health, policy_status, self_check. \
         Send SIGINT (Ctrl-C) or SIGTERM (or a `Shutdown` IPC request) to shut down."
    );

    // F1 idle self-reap: if configured, spawn a background timer that fires
    // `trigger_shutdown` once the daemon has been idle (no real IPC) for at
    // least `idle_ttl_secs`. The select! below picks it up via
    // `state.shutdown_notified()` and drains cleanly. ttl=0 disables.
    spawn_idle_reaper(&state);

    // Two shutdown sources: an OS signal (SIGINT/SIGTERM) or an
    // internal `Shutdown` IPC request that flipped the state trigger.
    // Whichever fires first wins; the other branch is dropped.
    tokio::select! {
        r = wait_for_shutdown_signal() => {
            r?;
            tracing::info!("OS shutdown signal received, draining...");
        }
        () = state.shutdown_notified() => {
            tracing::info!("internal shutdown (Shutdown IPC), draining...");
        }
    }
    handle.shutdown().await;
    shutdown_store(&state);
    terminal_commander_supervisor::pidfile::remove_pidfile(&state_dir);
    tracing::info!("IPC server exited cleanly.");
    Ok(())
}

/// Windows named-pipe IPC server.
#[cfg(windows)]
pub async fn run_ipc_server(config: DaemonConfig) -> Result<(), RuntimeError> {
    use std::sync::Arc;

    use crate::ipc::PipeServer;

    let _log_guard = init_file_logging(&config.daemon.data_dir);
    let state_dir = config.daemon.data_dir.clone();

    let (state, rep) = run_self_check(config)?;
    tracing::info!("{}", rep.render());

    let pipe_name = state.config.pipe_name();
    tracing::info!("binding named pipe at {pipe_name}");
    let state = Arc::new(state);
    let server = PipeServer::new(Arc::clone(&state), pipe_name.clone());
    let handle = server
        .spawn()
        .map_err(|e| RuntimeError::Signal(format!("pipe bind: {e}")))?;
    write_daemon_pidfile(&state_dir, &pipe_name);
    tracing::info!(
        "IPC server bound (Windows named pipe). \
         Send Ctrl-C to shut down."
    );

    // F1 idle self-reap: see the Unix branch for rationale. ttl=0 disables.
    spawn_idle_reaper(&state);

    // Two shutdown sources, mirroring the Unix arm: an OS signal
    // (Ctrl-C) or an internal `Shutdown` IPC request that flipped the
    // state trigger via `trigger_shutdown`. Without the
    // `shutdown_notified()` branch the daemon would ACK a `Shutdown`
    // IPC request (ShutdownAck) but never actually exit on Windows --
    // a false success. Whichever fires first wins; the other branch is
    // dropped.
    tokio::select! {
        r = wait_for_shutdown_signal_windows() => {
            r?;
            tracing::info!("OS shutdown signal (Ctrl-C) received, draining...");
        }
        () = state.shutdown_notified() => {
            tracing::info!("internal shutdown (Shutdown IPC), draining...");
        }
    }
    handle.shutdown().await;
    shutdown_store(state.as_ref());
    terminal_commander_supervisor::pidfile::remove_pidfile(&state_dir);
    tracing::info!("IPC server exited cleanly.");
    Ok(())
}

/// Spawn the idle self-reap timer (F1).
///
/// Reads `state.config.daemon.idle_ttl_secs`. If `> 0`, spawns a background
/// task that ticks on a small interval and calls
/// [`DaemonState::trigger_shutdown`] once `state.idle_secs() >= ttl`. The
/// task exits after firing (the `select!` in `run_ipc_server` picks the
/// shutdown up via `state.shutdown_notified()`). `0` disables.
///
/// The tick interval is `max(1, min(60, ttl/2))` seconds so a tiny TTL
/// (e.g. `1`) is still observable in tests, while a large TTL (e.g. the
/// default 1800) doesn't busy-poll.
fn spawn_idle_reaper(state: &Arc<crate::state::DaemonState>) {
    let ttl = state.config.daemon.idle_ttl_secs;
    if ttl == 0 {
        tracing::info!("idle self-reap disabled (idle_ttl_secs=0)");
        return;
    }
    // u64/2 can't overflow; the explicit clamp keeps the tick in [1, 60]
    // so a small ttl is still observable in tests and a large ttl doesn't
    // busy-poll. `(ttl / 2).clamp(1, 60)` is equivalent to the original
    // `max(1, min(60, ttl/2)).max(1)` because 1 <= 60.
    let tick = (ttl / 2).clamp(1, 60);
    tracing::info!("idle self-reap enabled: idle_ttl_secs={ttl} tick={tick}s");
    let st = Arc::clone(state);
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(std::time::Duration::from_secs(tick));
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            iv.tick().await;
            if st.idle_secs() >= ttl {
                tracing::info!(
                    "idle TTL exceeded (idle_secs={}, ttl={ttl}); triggering shutdown",
                    st.idle_secs()
                );
                st.trigger_shutdown();
                break;
            }
        }
    });
}

/// Write the daemon pidfile (pid + version + endpoint) so a newer
/// install can find and replace this daemon. Non-fatal on failure.
fn write_daemon_pidfile(state_dir: &std::path::Path, endpoint: &str) {
    let rec = terminal_commander_supervisor::pidfile::RunningDaemon {
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        endpoint: endpoint.to_owned(),
    };
    if let Err(e) = terminal_commander_supervisor::pidfile::write_pidfile(state_dir, &rec) {
        tracing::warn!("pidfile write failed (non-fatal): {e}");
    }
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
