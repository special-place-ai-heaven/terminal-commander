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
    let _ = terminal_commander_supervisor::paths::ensure_private_dir(&log_dir);
    let file_appender = tracing_appender::rolling::never(&log_dir, "terminal-commanderd.log");
    let _ = terminal_commander_supervisor::paths::restrict_file(
        &log_dir.join("terminal-commanderd.log"),
    );
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
    /// Another daemon owns the data directory or the endpoint.
    #[error("{0}")]
    InUse(String),
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

    // 4. Policy engine: the default full_access profile inherits the
    //    harness's trust and denies no escalator; every hardened profile
    //    must still deny sudo structurally.
    if state.policy.profile == crate::policy::PolicyProfile::FullAccess {
        rep.ok("policy: full_access (harness trust; structural denies off)");
    } else {
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

    // 5. Honest capability summary (DEFECT C): this build WIRES the IPC
    //    transport and command execution. The old "not wired" / "no UDS"
    //    text was frozen TC37 scaffold that mis-described 0.1.69 and
    //    actively misled incident diagnosis. Derive the served-method count
    //    from the same authority `system_discover` advertises so it cannot
    //    rot into a new hand-maintained wrong number.
    rep.ok("transport: UDS (Unix) / named-pipe (Windows) IPC wired");
    rep.ok(format!(
        "command execution: wired ({} IPC methods served; see system_discover)",
        crate::ipc::server::DISCOVERABLE_METHODS.len()
    ));

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

/// Decision returned by [`acquire_bringup_guard`].
enum BringUpGuard {
    /// We took the cross-process bring-up lock; hold this guard for the
    /// process lifetime so a later cold-starting peer rendezvous on it.
    Held(terminal_commander_supervisor::proc_lock::ProcessLock),
    /// Proceed to bind WITHOUT holding the lock. This is the NORMAL path
    /// for a daemon that our own supervisor spawned: the supervisor holds
    /// the lock across (probe -> spawn -> wait-for-bind), so the child it
    /// launched necessarily sees `Contended`. Contention is expected, not
    /// an error.
    Proceed,
    /// A DIFFERENT, live daemon already owns this endpoint. Exit
    /// gracefully WITHOUT binding, so we do not `remove_file` + rebind the
    /// socket and orphan it (the orphaning bug H6 fixes on Unix).
    AlreadyServed,
}

/// Belt-and-suspenders daemon-side single-flight guard (H6).
///
/// Tries the same `<state_dir>/terminal-commanderd.lock` the supervisor
/// uses. CRITICAL subtlety: the daemon NORMALLY sees `Contended` because
/// the very supervisor that spawned it is holding the lock while it waits
/// for this process to bind. That is the EXPECTED path and is NOT an
/// error — we [`BringUpGuard::Proceed`] to bind in that case. We only
/// bail ([`BringUpGuard::AlreadyServed`]) when the pidfile shows a
/// DIFFERENT (`pid != self`), live, endpoint-matching daemon already
/// bound — the actual orphaning scenario. A lock open/IO error is
/// non-fatal: log and proceed to bind.
fn acquire_bringup_guard(state_dir: &std::path::Path, endpoint: &str) -> BringUpGuard {
    use terminal_commander_supervisor::pidfile;
    use terminal_commander_supervisor::proc_lock::{self, TryLockResult};

    let lock_path = pidfile::lock_path(state_dir);
    match proc_lock::try_acquire(&lock_path) {
        Ok(TryLockResult::Acquired(guard)) => BringUpGuard::Held(guard),
        Ok(TryLockResult::Contended) => {
            // `read_pidfile` is liveness-gated (returns None if the
            // recorded pid is dead). Only a DIFFERENT, live daemon on the
            // SAME endpoint is a real competitor; our own supervisor
            // holding the lock (no matching live pidfile, or the pid is
            // ours) is the normal spawn path.
            match pidfile::read_pidfile(state_dir) {
                Some(rec) if rec.pid != std::process::id() && rec.endpoint == endpoint => {
                    tracing::info!(
                        "bring-up lock contended and a live daemon (pid {}) already \
                         serves {endpoint}; exiting without rebinding to avoid orphaning it",
                        rec.pid
                    );
                    BringUpGuard::AlreadyServed
                }
                _ => {
                    tracing::debug!(
                        "bring-up lock contended (supervisor holds it); proceeding to bind"
                    );
                    BringUpGuard::Proceed
                }
            }
        }
        Err(e) => {
            tracing::warn!("bring-up lock unavailable ({e}); proceeding to bind");
            BringUpGuard::Proceed
        }
    }
}

/// How long a daemon waits for a previous owner of its data directory to
/// finish exiting before it refuses to start.
const DATA_DIR_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Hold `<data_dir>/terminal-commanderd.data.lock` for the daemon's life, taken
/// before the store opens, so a second daemon never shares this data
/// directory's database or pidfile. The bring-up lock cannot do this: the
/// supervisor holds it while its child starts, so a supervisor-started daemon
/// never holds it once running. A lock that cannot be opened at all is logged
/// and skipped, like the bring-up lock.
async fn claim_data_dir(
    data_dir: &std::path::Path,
) -> Result<Option<terminal_commander_supervisor::proc_lock::ProcessLock>, RuntimeError> {
    use terminal_commander_supervisor::proc_lock::{self, TryLockResult};

    let _ = std::fs::create_dir_all(data_dir);
    let path = data_dir.join("terminal-commanderd.data.lock");
    let deadline = std::time::Instant::now() + DATA_DIR_LOCK_WAIT;
    loop {
        match proc_lock::try_acquire(&path) {
            Ok(TryLockResult::Acquired(lock)) => return Ok(Some(lock)),
            Ok(TryLockResult::Contended) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Ok(TryLockResult::Contended) => {
                let owner = terminal_commander_supervisor::pidfile::read_pidfile(data_dir)
                    .map_or_else(String::new, |rec| format!(" (pid {})", rec.pid));
                return Err(RuntimeError::InUse(format!(
                    "data directory {} is in use by another terminal-commanderd{owner}; \
                     refusing to start a second daemon on it. Stop that daemon, or pass a \
                     different --data-dir",
                    data_dir.display()
                )));
            }
            Err(e) => {
                tracing::warn!("data-dir lock unavailable ({e}); starting without it");
                return Ok(None);
            }
        }
    }
}

/// True when something answers on `socket_path`. Binding there would unlink
/// that listener's socket and take its clients -- what a daemon started with an
/// inherited `TC_SOCKET` but another data directory used to do.
#[cfg(unix)]
async fn socket_is_served(socket_path: &std::path::Path) -> bool {
    match tokio::net::UnixStream::connect(socket_path).await {
        Ok(_) => true,
        // A full accept backlog still means a listener.
        Err(e) => e.kind() == std::io::ErrorKind::WouldBlock,
    }
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
    ignore_sighup()?;
    let state_dir = config.daemon.data_dir.clone();
    let _data_dir_lock = claim_data_dir(&state_dir).await?;

    let (state, rep) = run_self_check(config)?;
    tracing::info!("{}", rep.render());

    let socket_path = state.config.socket_path();
    let endpoint = socket_path.display().to_string();
    if socket_is_served(&socket_path).await {
        return Err(RuntimeError::InUse(format!(
            "another daemon is serving {endpoint}; refusing to take over its socket. \
             Stop that daemon, or start this one with a different TC_SOCKET"
        )));
    }
    tracing::info!("binding UDS at {endpoint}");
    // Keep an Arc handle to the state so we can await the internal
    // shutdown trigger (flipped by the `Shutdown` IPC dispatch arm)
    // alongside the OS-signal path below.
    let state = Arc::new(state);

    // H6 daemon-side single-flight guard. Take the same bring-up lock the
    // supervisor uses BEFORE binding, so a stray second daemon does not
    // `remove_file` + rebind the socket and orphan a live first daemon.
    // NOTE: seeing `Contended` here is the NORMAL case — our own supervisor
    // holds the lock while it waits for us to bind — so we proceed unless a
    // DIFFERENT live daemon already serves this endpoint.
    let _bringup_guard = match acquire_bringup_guard(&state_dir, &endpoint) {
        BringUpGuard::Held(guard) => Some(guard),
        BringUpGuard::Proceed => None,
        BringUpGuard::AlreadyServed => return Ok(()),
    };

    let server = IpcServer::new(Arc::clone(&state), socket_path);
    let handle = server
        .spawn()
        .map_err(|e| RuntimeError::Signal(format!("UDS bind: {e}")))?;
    // Identity-gated claim (same decision logic as the 15s self-heal): a
    // missing/stale pidfile is written, but a DIFFERENT live daemon's record
    // is never stolen. Observed 2026-07-16: a second daemon sharing this
    // state dir under another endpoint seized the pidfile at bind and the
    // rightful owner WARN-stormed every 15s until the interloper idle-reaped.
    reassert_pidfile_once(&state_dir, &endpoint, true);
    tracing::info!(
        "IPC server bound. \
         Serving {} IPC methods (authoritative set; query system_discover for the list). \
         Send SIGINT (Ctrl-C) or SIGTERM (or a `Shutdown` IPC request) to shut down.",
        crate::ipc::server::DISCOVERABLE_METHODS.len()
    );

    // F1 idle self-reap: if configured, spawn a background timer that fires
    // `trigger_shutdown` once the daemon has been idle (no real IPC) for at
    // least `idle_ttl_secs`. The select! below picks it up via
    // `state.shutdown_notified()` and drains cleanly. ttl=0 disables.
    spawn_idle_reaper(&state);
    // P1 / TC50: reclaim sessions idle past their per-session TTL.
    spawn_session_reaper(&state);
    spawn_discovery_prewarm(&state_dir);
    // Re-assert the pidfile if it goes missing (the daemon writes it once at
    // bind above and never used to recover a lost one). Closes the
    // pidfile-less window the version-aware replace path mis-reads as stale.
    let reasserter = spawn_pidfile_reasserter(state_dir.clone(), endpoint.clone());

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
    // Drain command lifecycle waiters AFTER IPC connections have stopped
    // (no new commands can start) and BEFORE the store closes. A command
    // exiting inside the shutdown window otherwise loses its final
    // command_exited event + audit row (D7).
    state.command.drain_lifecycle_tasks().await;
    // spec 004 FR-012: after the drain, anything still non-terminal was ended
    // BY this shutdown. Record it while the store is still writable.
    state.record_abandoned_jobs();
    shutdown_store(&state);
    // Clean shutdown releases the host ceiling (Linux: the empty
    // tc-jobs-<pid> cgroup dir) instead of leaving it to the next boot sweep.
    if let Err(reason) = terminal_commander_probes::governor::uninstall_host_ceiling() {
        tracing::warn!("host ceiling release failed (non-fatal): {reason}");
    }
    release_pidfile(reasserter, &state_dir).await;
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
    let _data_dir_lock = claim_data_dir(&state_dir).await?;

    let (state, rep) = run_self_check(config)?;
    tracing::info!("{}", rep.render());

    let pipe_name = state.config.pipe_name();
    tracing::info!("binding named pipe at {pipe_name}");
    let state = Arc::new(state);

    // H6 daemon-side single-flight guard. Windows pipe bind already treats
    // a duplicate first-instance as fatal (so it never orphans like the
    // Unix socket rebind), but taking the same bring-up lock here still
    // collapses the spawn-then-die race and keeps the two OSes symmetric.
    // NOTE: `Contended` is the NORMAL case (our supervisor holds the lock);
    // we bail only when a DIFFERENT live daemon already serves this pipe.
    let _bringup_guard = match acquire_bringup_guard(&state_dir, &pipe_name) {
        BringUpGuard::Held(guard) => Some(guard),
        BringUpGuard::Proceed => None,
        BringUpGuard::AlreadyServed => return Ok(()),
    };

    let server = PipeServer::new(Arc::clone(&state), pipe_name.clone());
    let handle = server
        .spawn()
        .map_err(|e| RuntimeError::Signal(format!("pipe bind: {e}")))?;
    // Identity-gated claim; see the Unix arm for the 2026-07-16 rationale.
    reassert_pidfile_once(&state_dir, &pipe_name, true);
    tracing::info!(
        "IPC server bound (Windows named pipe). \
         Send Ctrl-C to shut down."
    );

    // F1 idle self-reap: see the Unix branch for rationale. ttl=0 disables.
    spawn_idle_reaper(&state);
    // Session idle-reap (no-op on non-unix; sessions are PTY-backed).
    spawn_session_reaper(&state);
    spawn_discovery_prewarm(&state_dir);
    // Re-assert the pidfile if it goes missing (cross-platform; see the Unix
    // arm). Closes the pidfile-less window mis-read as stale by the replace path.
    let reasserter = spawn_pidfile_reasserter(state_dir.clone(), pipe_name.clone());
    // Same-user shutdown event at medium integrity. A normal update can signal
    // it when TerminateProcess is denied; it does not expose the command pipe.
    crate::shutdown_event::spawn_waiter(Arc::clone(&state));

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
    // Drain command lifecycle waiters AFTER IPC connections have stopped
    // (no new commands can start) and BEFORE the store closes. A command
    // exiting inside the shutdown window otherwise loses its final
    // command_exited event + audit row (D7).
    state.command.drain_lifecycle_tasks().await;
    state.record_abandoned_jobs();
    shutdown_store(state.as_ref());
    // Clean shutdown releases the host ceiling (Linux: the empty
    // tc-jobs-<pid> cgroup dir) instead of leaving it to the next boot sweep.
    if let Err(reason) = terminal_commander_probes::governor::uninstall_host_ceiling() {
        tracing::warn!("host ceiling release failed (non-fatal): {reason}");
    }
    release_pidfile(reasserter, &state_dir).await;
    tracing::info!("IPC server exited cleanly.");
    Ok(())
}

/// Discover the host in the background once the daemon serves, so the first
/// `system_discover` or default-shell lookup reuses the result instead of
/// waiting for the probes.
///
/// Confirmed answers persist in the shared state-dir base, which the default
/// daemon (data dir = base) and every per-session daemon (data dir =
/// base/<session>) see. A daemon with any other data dir keeps its own.
fn spawn_discovery_prewarm(state_dir: &std::path::Path) {
    let base = terminal_commander_supervisor::paths::resolve_state_dir_base();
    crate::environment::persist_discovery_in(discovery_dir(state_dir, &base));
    std::thread::spawn(|| {
        let _ = crate::environment::cached_host_environment();
    });
}

/// Where a daemon with data dir `state_dir` keeps `host-discovery.json`: the
/// state-dir `base` when it is the default daemon's or a per-session daemon's
/// (`base/<session>`), else its own data dir.
fn discovery_dir<'a>(
    state_dir: &'a std::path::Path,
    base: &'a std::path::Path,
) -> &'a std::path::Path {
    if state_dir == base || state_dir.parent() == Some(base) {
        base
    } else {
        state_dir
    }
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
                // S4: live work vetoes the reap. A still-running command,
                // file watch, or PTY job means an agent is waiting on this
                // daemon even if no IPC arrived for a full TTL; reaping now
                // would orphan the child (the lifecycle-waiter drain aborts
                // after its ceiling without killing it) and lose the job's
                // receipt, exit event, and audit row.
                if st.has_live_work() {
                    tracing::info!(
                        "idle TTL exceeded (idle_secs={}, ttl={ttl}) but live \
                         work exists; deferring self-reap",
                        st.idle_secs()
                    );
                    continue;
                }
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

/// Spawn the per-session idle-TTL reaper (P1 / TC50).
///
/// Unix-only: sessions are PTY-backed. Ticks on a bounded interval and
/// calls [`ShellSessionRuntime::reap_idle`](crate::shell_session::ShellSessionRuntime::reap_idle),
/// which tears down sessions idle past `[shell_session] idle_ttl_secs` (and
/// any already-terminal entry) so their PTY children + bucket resources are
/// reclaimed. A zero TTL disables the idle path; terminal entries are still
/// reaped on the next session start. The task lives for the daemon's life
/// (it shares the process exit; the IPC server's shutdown drops everything).
#[cfg(unix)]
fn spawn_session_reaper(state: &Arc<crate::state::DaemonState>) {
    let ttl = state.config.shell_session.idle_ttl_secs;
    if ttl == 0 {
        tracing::info!("session idle-reap disabled (shell_session.idle_ttl_secs=0)");
        return;
    }
    let tick = (ttl / 2).clamp(1, 60);
    tracing::info!("session idle-reap enabled: idle_ttl_secs={ttl} tick={tick}s");
    let st = Arc::clone(state);
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(std::time::Duration::from_secs(tick));
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            iv.tick().await;
            let reaped = st.sessions.reap_idle();
            if reaped > 0 {
                tracing::info!("session idle-reap: reclaimed {reaped} session(s)");
            }
        }
    });
}

/// No-op session reaper on non-unix (the shell-session runtime is unix-only;
/// Windows session support is a separate slice). The Windows PTY command lane
/// has no idle-session concept to reap.
#[cfg(not(unix))]
const fn spawn_session_reaper(_state: &Arc<crate::state::DaemonState>) {}

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

/// Self-heal tick interval for the pidfile re-assert task.
///
/// The daemon writes its pidfile exactly once at bind and historically never
/// re-asserted a lost one, so any event that removed a live daemon's pidfile
/// (a mis-classified reap, a transient fs error, a lost atomic rename, manual
/// cleanup) left it running but pidfile-less — and the version-aware replace
/// path then mis-classified it as "predates the pidfile feature => stale".
/// A bounded 15s re-assert closes that window: short enough that the gap a
/// `replace`/`session list`/`reap` could observe is tiny, long enough that the
/// idle daemon does a single cheap stat (no /proc walk) per tick.
const PIDFILE_REASSERT_TICK_SECS: u64 = 15;

/// What the periodic self-heal task should do about the pidfile it found.
///
/// Pure decision so the safety policy is unit-testable without touching the
/// filesystem or spawning a daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReassertDecision {
    /// The pidfile is missing, records a now-dead pid, or already records OUR
    /// pid with the wrong endpoint — (re)write it to point at us. Covers the
    /// "missing", "stale-dead", and "ours-but-wrong-endpoint" cases.
    Rewrite,
    /// The pidfile already records OUR pid + OUR endpoint. Nothing to do.
    LeaveOurs,
    /// The pidfile records a DIFFERENT, LIVE pid bound to OUR endpoint. That is
    /// an anomaly (two daemons claiming one endpoint) — log warn and do NOT
    /// overwrite; leave it for the operator.
    AnomalyOurEndpoint { pid: u32 },
    /// The pidfile records a DIFFERENT, LIVE pid bound to a DIFFERENT endpoint.
    /// It belongs to another daemon; never clobber it.
    AnomalyDifferentEndpoint { pid: u32 },
}

/// Decide whether to re-assert the pidfile, given the raw pidfile contents
/// (`read_pidfile_raw` — liveness-unfiltered so we can classify dead pids),
/// our own identity (`our_pid` + `our_endpoint`), and a liveness predicate.
///
/// SAFETY: the only outcomes that lead to a write are [`ReassertDecision::Rewrite`]
/// (missing / stale-dead / ours-with-wrong-endpoint). A DIFFERENT live pid is
/// NEVER overwritten, whether it is bound to our endpoint (an anomaly we surface)
/// or a different one (someone else's daemon).
fn reassert_decision(
    current: Option<&terminal_commander_supervisor::pidfile::RunningDaemon>,
    our_pid: u32,
    our_endpoint: &str,
    is_alive: impl Fn(u32) -> bool,
) -> ReassertDecision {
    let Some(rec) = current else {
        // Missing or unparseable — re-assert.
        return ReassertDecision::Rewrite;
    };
    if rec.pid == our_pid {
        // It is ours. Fix the endpoint if it somehow drifted, else no-op.
        if rec.endpoint == our_endpoint {
            ReassertDecision::LeaveOurs
        } else {
            ReassertDecision::Rewrite
        }
    } else if !is_alive(rec.pid) {
        // A different pid, but it is dead — stale entry, safe to reclaim.
        ReassertDecision::Rewrite
    } else if rec.endpoint == our_endpoint {
        // A different LIVE pid claims OUR endpoint: two daemons, one endpoint.
        // Do not clobber; surface it.
        ReassertDecision::AnomalyOurEndpoint { pid: rec.pid }
    } else {
        // A different LIVE pid on a different endpoint: not ours, leave it.
        ReassertDecision::AnomalyDifferentEndpoint { pid: rec.pid }
    }
}

/// Run one pidfile self-heal pass: read the current pidfile, decide, and
/// (re)write it iff the decision says so. Best-effort — a failed write is
/// logged at warn and never fatal, mirroring [`write_daemon_pidfile`].
/// `at_bind` selects the Rewrite arm's log level: the first claim at bind is
/// INFO (an absent pidfile is the normal case there); a later self-heal
/// rewrite is WARN (the pidfile went missing while we were serving).
fn reassert_pidfile_once(state_dir: &std::path::Path, endpoint: &str, at_bind: bool) {
    use terminal_commander_supervisor::pidfile;
    let current = pidfile::read_pidfile_raw(state_dir);
    let decision = reassert_decision(
        current.as_ref(),
        std::process::id(),
        endpoint,
        pidfile::pid_alive,
    );
    match decision {
        ReassertDecision::LeaveOurs => {}
        ReassertDecision::Rewrite => {
            if at_bind {
                // First claim at bind: absent or stale-dead pidfile is the
                // NORMAL case (clean shutdown removes it), not an anomaly.
                tracing::info!(
                    "pidfile claimed at bind endpoint {endpoint} (pid {}, version {})",
                    std::process::id(),
                    env!("CARGO_PKG_VERSION"),
                );
            } else {
                tracing::warn!(
                    "pidfile missing or stale at bind endpoint {endpoint}; re-asserting it \
                     (pid {}, version {})",
                    std::process::id(),
                    env!("CARGO_PKG_VERSION"),
                );
            }
            write_daemon_pidfile(state_dir, endpoint);
        }
        ReassertDecision::AnomalyOurEndpoint { pid } => {
            tracing::warn!(
                "pidfile records a DIFFERENT live daemon (pid {pid}) bound to OUR endpoint \
                 {endpoint}; leaving it untouched for the operator (not overwriting)"
            );
        }
        ReassertDecision::AnomalyDifferentEndpoint { pid } => {
            tracing::warn!(
                "pidfile records a different live daemon (pid {pid}) bound to a different \
                 endpoint; not overwriting it from {endpoint}"
            );
        }
    }
}

/// Spawn the periodic pidfile self-heal task.
///
/// The daemon claims its pidfile once at bind (identity-gated); this task re-asserts it
/// on a bounded [`PIDFILE_REASSERT_TICK_SECS`] interval whenever it goes missing
/// (or records a dead pid, or records ours with a drifted endpoint), so the
/// pidfile-less window the version-aware replace path mis-reads as "stale" never
/// persists. The pidfile is cross-platform, so this runs on every target. The
/// task runs until [`release_pidfile`] stops it at shutdown.
fn spawn_pidfile_reasserter(
    state_dir: std::path::PathBuf,
    endpoint: String,
) -> tokio::task::JoinHandle<()> {
    tracing::info!(
        "pidfile self-heal enabled: tick={PIDFILE_REASSERT_TICK_SECS}s endpoint={endpoint}"
    );
    tokio::spawn(async move {
        let mut iv =
            tokio::time::interval(std::time::Duration::from_secs(PIDFILE_REASSERT_TICK_SECS));
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            iv.tick().await;
            reassert_pidfile_once(&state_dir, &endpoint, false);
        }
    })
}

/// Stop the pidfile self-heal, then remove the pidfile. In the other order a
/// self-heal pass that runs after the removal (its first tick fires at once, so
/// a daemon stopped right after binding hits this) writes the pidfile back, and
/// the exited daemon leaves a pidfile naming a dead pid.
async fn release_pidfile(reasserter: tokio::task::JoinHandle<()>, state_dir: &std::path::Path) {
    reasserter.abort();
    // Waits out a pass already running on another worker.
    let _ = reasserter.await;
    terminal_commander_supervisor::pidfile::remove_pidfile(state_dir);
}

#[cfg(windows)]
async fn wait_for_shutdown_signal_windows() -> Result<(), RuntimeError> {
    // Ctrl-C on Windows.
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| RuntimeError::Signal(format!("ctrl-c listen: {e}")))?;
    Ok(())
}

/// A server has no use for SIGHUP, but a daemon started from a terminal (the
/// Linux profile hook, the npm shim) still gets it when that terminal closes,
/// and its default action ends the process.
///
/// Handled rather than set to `SIG_IGN`, so the commands the daemon starts
/// get the default disposition back at exec.
#[cfg(unix)]
fn ignore_sighup() -> Result<(), RuntimeError> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut hangup = signal(SignalKind::hangup())
        .map_err(|e| RuntimeError::Signal(format!("SIGHUP listen: {e}")))?;
    tokio::spawn(async move {
        while hangup.recv().await.is_some() {
            tracing::info!("SIGHUP ignored; send SIGINT or SIGTERM to shut down");
        }
    });
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

    #[test]
    fn per_session_and_default_daemons_share_the_discovery_file() {
        let base = std::path::Path::new("state");
        let session = base.join("tc-abc");
        assert_eq!(discovery_dir(base, base), base);
        assert_eq!(discovery_dir(&session, base), base);
        let elsewhere = std::path::Path::new("other").join("dir");
        assert_eq!(discovery_dir(&elsewhere, base), elsewhere.as_path());
    }

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
        assert!(r.contains("policy: full_access"));
        cleanup(&data);

        let data = temp_data_dir("ok-hardened");
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.policy.profile = crate::policy::PolicyProfile::DeveloperLocal;
        let (_state, rep) = run_self_check(cfg).unwrap();
        assert!(rep.render().contains("sudo denied"), "{}", rep.render());
        cleanup(&data);
    }

    #[test]
    fn self_check_reports_wired_capabilities_not_stale_scope_text() {
        // DEFECT C: the self-check must reflect the REAL served surface, not
        // the frozen TC37 "not wired" / "no UDS" scaffold text that misled
        // this incident's diagnosis.
        let data = temp_data_dir("wired");
        let cfg = DaemonConfig::defaults_in(&data);
        let (_state, rep) = run_self_check(cfg).unwrap();
        let r = rep.render();
        assert!(
            !r.contains("not wired") && !r.contains("no UDS"),
            "self-check must not carry stale 'not wired' / 'no UDS' claims: {r}"
        );
        assert!(
            r.contains("command execution: wired"),
            "self-check must report command execution wired: {r}"
        );
        // The reported method count must derive from the authoritative served
        // set (the same `DISCOVERABLE_METHODS` `system_discover` advertises),
        // so the diagnostic cannot drift from reality.
        assert!(
            r.contains(&format!(
                "{} IPC methods",
                crate::ipc::server::DISCOVERABLE_METHODS.len()
            )),
            "self-check method count must derive from DISCOVERABLE_METHODS: {r}"
        );
        cleanup(&data);
    }

    use terminal_commander_supervisor::pidfile::RunningDaemon;

    fn rec(pid: u32, endpoint: &str) -> RunningDaemon {
        RunningDaemon {
            pid,
            version: "9.9.9".to_owned(),
            endpoint: endpoint.to_owned(),
        }
    }

    // --- pidfile re-assert decision table (pure, fs-free) ---

    #[test]
    fn reassert_rewrites_when_pidfile_missing() {
        // The flap trigger: a live daemon lost its pidfile entirely.
        let d = reassert_decision(None, 100, "/run/tc.sock", |_| true);
        assert_eq!(d, ReassertDecision::Rewrite);
    }

    #[test]
    fn reassert_leaves_ours_untouched() {
        let existing = rec(100, "/run/tc.sock");
        let d = reassert_decision(Some(&existing), 100, "/run/tc.sock", |_| true);
        assert_eq!(d, ReassertDecision::LeaveOurs);
    }

    #[test]
    fn reassert_rewrites_when_ours_but_endpoint_drifted() {
        // Same pid (ours), wrong endpoint recorded -> correct it.
        let existing = rec(100, "/run/OLD.sock");
        let d = reassert_decision(Some(&existing), 100, "/run/tc.sock", |_| true);
        assert_eq!(d, ReassertDecision::Rewrite);
    }

    #[test]
    fn reassert_rewrites_when_recorded_pid_is_dead() {
        // A different pid, but the liveness predicate says it is gone: stale.
        let existing = rec(200, "/run/tc.sock");
        let d = reassert_decision(Some(&existing), 100, "/run/tc.sock", |_| false);
        assert_eq!(d, ReassertDecision::Rewrite);
    }

    #[test]
    fn reassert_refuses_when_different_live_pid_on_our_endpoint() {
        // Anomaly: two live daemons claim the same endpoint. Never overwrite.
        let existing = rec(200, "/run/tc.sock");
        let d = reassert_decision(Some(&existing), 100, "/run/tc.sock", |_| true);
        assert_eq!(d, ReassertDecision::AnomalyOurEndpoint { pid: 200 });
    }

    #[test]
    fn reassert_refuses_when_different_live_pid_on_different_endpoint() {
        // SAFETY case: a pidfile naming a DIFFERENT live pid on a DIFFERENT
        // endpoint must NOT be overwritten — it belongs to another daemon.
        let existing = rec(200, "/run/OTHER.sock");
        let d = reassert_decision(Some(&existing), 100, "/run/tc.sock", |_| true);
        assert_eq!(d, ReassertDecision::AnomalyDifferentEndpoint { pid: 200 });
    }

    // --- one-pass self-heal against the real filesystem (fs, no daemon) ---

    #[test]
    fn reassert_pidfile_once_recreates_a_missing_pidfile() {
        use terminal_commander_supervisor::pidfile;
        let data = temp_data_dir("reassert-missing");
        std::fs::create_dir_all(&data).unwrap();
        let endpoint = data.join("tc.sock").display().to_string();

        // No pidfile yet.
        assert!(pidfile::read_pidfile_raw(&data).is_none());

        reassert_pidfile_once(&data, &endpoint, false);

        let got = pidfile::read_pidfile_raw(&data).expect("pidfile recreated");
        assert_eq!(got.pid, std::process::id());
        assert_eq!(got.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(got.endpoint, endpoint);
        cleanup(&data);
    }

    // SAFETY at the fs layer: a pidfile owned by a DIFFERENT, LIVE pid bound to
    // a DIFFERENT endpoint must survive a self-heal pass untouched. Uses pid 1
    // (init/launchd, always alive on unix) as the unambiguous foreign live owner.
    #[cfg(unix)]
    #[test]
    fn reassert_pidfile_once_does_not_clobber_a_foreign_live_pidfile() {
        use terminal_commander_supervisor::pidfile;
        let data = temp_data_dir("reassert-foreign");
        std::fs::create_dir_all(&data).unwrap();

        let foreign = rec(1, "/run/SOMEONE_ELSE.sock");
        pidfile::write_pidfile(&data, &foreign).unwrap();

        let our_endpoint = data.join("tc.sock").display().to_string();
        reassert_pidfile_once(&data, &our_endpoint, false);

        let after = pidfile::read_pidfile_raw(&data).expect("pidfile still present");
        assert_eq!(
            after, foreign,
            "a foreign live daemon's pidfile must not be overwritten"
        );
        cleanup(&data);
    }

    #[test]
    fn bind_claim_writes_a_missing_pidfile() {
        use terminal_commander_supervisor::pidfile;
        let data = temp_data_dir("bind-claim-missing");
        std::fs::create_dir_all(&data).unwrap();
        let endpoint = data.join("tc.sock").display().to_string();

        reassert_pidfile_once(&data, &endpoint, true);

        let got = pidfile::read_pidfile_raw(&data).expect("pidfile claimed at bind");
        assert_eq!(got.pid, std::process::id());
        assert_eq!(got.endpoint, endpoint);
        cleanup(&data);
    }

    // Regression: a daemon told to shut down right after binding removed its
    // pidfile, and then the self-heal task got its first poll -- whose first
    // tick fires at once -- found the pidfile missing and wrote it back. The
    // process exited leaving a pidfile that named a dead pid (seen 4 times in
    // 800 loaded runs of `session_reap_token_shuts_down_the_daemon`).
    // `current_thread` keeps the self-heal unpolled until the test yields,
    // which is exactly the late first poll.
    #[tokio::test(flavor = "current_thread")]
    async fn a_released_pidfile_stays_removed_when_the_self_heal_runs_late() {
        use terminal_commander_supervisor::pidfile;
        let data = temp_data_dir("release-pidfile");
        std::fs::create_dir_all(&data).unwrap();
        let endpoint = data.join("tc.sock").display().to_string();
        reassert_pidfile_once(&data, &endpoint, true);

        let reasserter = spawn_pidfile_reasserter(data.clone(), endpoint);
        release_pidfile(reasserter, &data).await;
        // Park the runtime so the timer driver turns and a surviving
        // self-heal gets its first, immediate tick.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(
            pidfile::read_pidfile_raw(&data).is_none(),
            "a released pidfile must not be written back by the self-heal"
        );
        cleanup(&data);
    }

    // Regression (2026-07-16): the bind-time pidfile write was unconditional,
    // so a second daemon sharing this state dir under ANOTHER endpoint seized
    // the pidfile at bind and the rightful owner WARN-stormed every 15s until
    // the interloper idle-reaped. The bind claim must use the same identity
    // gate as the self-heal pass.
    #[cfg(unix)]
    #[test]
    fn bind_claim_does_not_steal_a_foreign_live_pidfile() {
        use terminal_commander_supervisor::pidfile;
        let data = temp_data_dir("bind-claim-foreign");
        std::fs::create_dir_all(&data).unwrap();

        let foreign = rec(1, "/run/SOMEONE_ELSE.sock");
        pidfile::write_pidfile(&data, &foreign).unwrap();

        let our_endpoint = data.join("tc.sock").display().to_string();
        reassert_pidfile_once(&data, &our_endpoint, true);

        let after = pidfile::read_pidfile_raw(&data).expect("pidfile still present");
        assert_eq!(
            after, foreign,
            "the bind claim must never steal a live foreign daemon's pidfile"
        );
        cleanup(&data);
    }
}
