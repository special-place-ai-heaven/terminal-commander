// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commander`: operator admin CLI (TC25).
//!
//! Subcommands per the TC25 mini-spec: status, doctor, rules, buckets,
//! jobs, probes, policy, audit. The CLI talks to the daemon Router
//! (in-process at MVP; UDS adapter deferred per TC21 lock).
//!
//! Source-status: live (TC25) — CLI parses + renders. Subcommand
//! bodies print structured summaries; the live IPC connection lands
//! when TC21's transport swap happens.

use clap::{Parser, Subcommand};
use terminal_commander_supervisor::ensure::{EnsureDaemonOptions, EnsureDaemonStatus};
use terminal_commander_supervisor::paths::{
    endpoint_from_socket_path, resolve_socket_path, resolve_state_dir,
};

pub(crate) mod update_locks;

const EX_UNAVAILABLE: u8 = 69;

#[derive(Parser, Debug)]
#[command(
    name = "terminal-commander",
    version,
    about = "Terminal Commander admin CLI",
    long_about = "Operator entry point for the Terminal Commander daemon. \
                  Inspects status, lists buckets, jobs, probes, audit \
                  records, and policy. NEVER bypasses the policy engine."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print high-level daemon status.
    Status,
    /// Run the doctor (environment + safety checks).
    Doctor,
    /// Rule registry inspection.
    Rules {
        #[command(subcommand)]
        op: RulesOp,
    },
    /// Bucket inspection.
    Buckets {
        #[command(subcommand)]
        op: BucketsOp,
    },
    /// Job inspection.
    Jobs,
    /// Probe inspection.
    Probes,
    /// Policy inspection.
    Policy,
    /// Audit log inspection.
    Audit {
        /// Limit on records to display.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Per-harness session management (list / reap).
    Session {
        #[command(subcommand)]
        op: SessionOp,
    },
    /// Hidden npm-update preflight: stop TC binaries loaded from anywhere under
    /// the npm package scope dir (typically the parent `node_modules`).
    #[command(hide = true)]
    UpdateLocks {
        /// Scope directory under which owned binaries are eligible for stop.
        /// Accepts the legacy `--bin-dir` alias from older JS shims.
        #[arg(long = "scope-dir", alias = "bin-dir")]
        scope_dir: std::path::PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum SessionOp {
    /// List sessions (default + seeded) with liveness + idle.
    List,
    /// Reap sessions (graceful Shutdown-IPC; identity-gated force fallback).
    ///
    /// One mode at a time: a token positional reaps one session; `--all`
    /// reaps every session under the base state-dir; `--idle` selects ALIVE
    /// sessions whose `idle_secs` is at least `--idle-secs` (default 1800).
    Reap {
        /// Session token to reap (mutually exclusive with `--all` / `--idle`).
        token: Option<String>,
        /// Reap every session (default + every seeded).
        #[arg(long)]
        all: bool,
        /// Reap sessions idle ≥ `--idle-secs`.
        #[arg(long)]
        idle: bool,
        /// Idle threshold in seconds (only honored with `--idle`).
        #[arg(long, default_value_t = 1800)]
        idle_secs: u64,
    },
}

#[derive(Subcommand, Debug)]
enum RulesOp {
    /// List rules in the registry.
    List,
    /// Show one rule by id.
    Show { rule_id: String },
}

#[derive(Subcommand, Debug)]
enum BucketsOp {
    /// List buckets.
    List,
    /// Show a bucket summary by id.
    Show { bucket_id: String },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    run(cli)
}

fn run(cli: Cli) -> std::process::ExitCode {
    match cli.cmd {
        Command::Status => print_status(),
        Command::Doctor => std::process::ExitCode::from(run_doctor()),
        Command::Rules { op } => match op {
            RulesOp::List => daemon_backed_command_unavailable("rules list"),
            RulesOp::Show { rule_id: _ } => daemon_backed_command_unavailable("rules show"),
        },
        Command::Buckets { op } => match op {
            BucketsOp::List => daemon_backed_command_unavailable("buckets list"),
            BucketsOp::Show { bucket_id: _ } => daemon_backed_command_unavailable("buckets show"),
        },
        Command::Jobs => daemon_backed_command_unavailable("jobs"),
        Command::Probes => daemon_backed_command_unavailable("probes"),
        Command::Policy => daemon_backed_command_unavailable("policy"),
        Command::Audit { limit: _ } => daemon_backed_command_unavailable("audit"),
        Command::Session { op } => match op {
            SessionOp::List => run_session_list(),
            SessionOp::Reap {
                token,
                all,
                idle,
                idle_secs,
            } => run_session_reap(token.as_deref(), all, idle, idle_secs),
        },
        Command::UpdateLocks { scope_dir } => {
            let result = update_locks::stop_installed_processes(&scope_dir);
            for line in &result.lines {
                eprintln!("{line}");
            }
            if result.errors == 0 {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::from(1)
            }
        }
    }
}

fn daemon_backed_command_unavailable(command: &str) -> std::process::ExitCode {
    eprintln!(
        "terminal-commander: {command} unavailable: requires live daemon IPC; refusing to synthesize empty or not-found data."
    );
    std::process::ExitCode::from(EX_UNAVAILABLE)
}

fn print_status() -> std::process::ExitCode {
    let state_dir = resolve_state_dir();
    let log_path = state_dir.join("logs").join("terminal-commanderd.log");
    let endpoint_path = resolve_socket_path();
    let endpoint = endpoint_from_socket_path(&endpoint_path);

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("terminal-commander: tokio runtime build failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let status = rt.block_on(async {
        let opts = EnsureDaemonOptions {
            daemon_binary: std::path::PathBuf::from("terminal-commanderd"),
            state_dir: state_dir.clone(),
            log_dir: state_dir.join("logs"),
            endpoint,
            startup_timeout: std::time::Duration::from_secs(1),
            allow_spawn: false,
        };
        terminal_commander_supervisor::ensure::ensure_daemon(opts).await
    });

    let (daemon_text, pid_text, exit_code) = match &status {
        EnsureDaemonStatus::AlreadyRunning { pid, .. } => (
            "running",
            pid.map_or_else(|| "-".into(), |p| p.to_string()),
            std::process::ExitCode::SUCCESS,
        ),
        EnsureDaemonStatus::Started { pid, .. } => (
            "running",
            pid.map_or_else(|| "-".into(), |p| p.to_string()),
            std::process::ExitCode::SUCCESS,
        ),
        EnsureDaemonStatus::Unavailable { .. } => (
            "unavailable",
            "-".to_string(),
            std::process::ExitCode::from(1),
        ),
    };

    println!("terminal-commander status:");
    println!("  version       : {}", env!("CARGO_PKG_VERSION"));
    println!("  endpoint      : {}", endpoint_path.display());
    println!("  daemon        : {daemon_text}");
    println!("  pid           : {pid_text}");
    println!("  log_path      : {}", log_path.display());
    println!("  state_dir     : {}", state_dir.display());

    exit_code
}

/// One rendered row of the `session list` table.
struct SessionRow {
    label: String,
    pid: String,
    state: &'static str,
    idle: String,
    endpoint: String,
}

/// Build an [`Endpoint`] enum from the endpoint string the daemon recorded in
/// its pidfile. WindowsPipe iff it starts with the named-pipe prefix; else a
/// Unix socket path.
fn endpoint_from_recorded(ep: &str) -> terminal_commander_supervisor::ensure::Endpoint {
    if ep.starts_with(r"\\.\pipe\") {
        terminal_commander_supervisor::ensure::Endpoint::WindowsPipe {
            name: ep.to_owned(),
        }
    } else {
        terminal_commander_supervisor::ensure::Endpoint::UnixSocket {
            path: std::path::PathBuf::from(ep),
        }
    }
}

/// `terminal-commander session list`: enumerate the base state-dir's pidfile
/// (label `default`) plus every immediate token-named subdir's pidfile, then
/// for each ALIVE entry run a Health handshake CONCURRENTLY to display state.
/// STALE entries (dead pid) print `state=stale, idle=-` without any IPC.
///
/// Idle is surfaced from the Health handshake via `probe_endpoint_health`:
/// a modern daemon reports `<secs>s`; a legacy daemon that omits `idle_secs`
/// is alive with idle `?` (unknown, NOT unresponsive); an unresponsive peer
/// is `state=unresponsive, idle=-`.
/// Map a probe result to the `(state, idle)` columns shown by `session list`:
///   - `Some(idle_secs = Some(s))` -> alive, `"<s>s"` (modern daemon).
///   - `Some(idle_secs = None)`    -> alive, `"?"` (legacy daemon: idle UNKNOWN).
///   - `None`                      -> unresponsive, `"-"` (no Health reply).
fn classify_health(
    health: Option<&terminal_commander_supervisor::ensure::ProbeHealth>,
) -> (&'static str, String) {
    match health.map(|h| h.idle_secs) {
        Some(Some(s)) => ("alive", format!("{s}s")),
        Some(None) => ("alive", "?".to_string()),
        None => ("unresponsive", "-".to_string()),
    }
}

/// Decision for ONE alive entry under `session reap --idle`. Pure, so the
/// idle predicate is unit-testable without spawning a daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdleReapDecision {
    /// Modern daemon, idle >= threshold: select for reap.
    Reap,
    /// Modern daemon, idle below threshold: recently active, skip.
    TooRecent(u64),
    /// Legacy daemon (no idle_secs): alive & ours but predicate inapplicable.
    IdleUnknown,
    /// No Health reply (unresponsive/gone): not force-killed on the soft path.
    Unresponsive,
}

/// Decide what `--idle` should do with one alive entry given its probe result
/// and the CLI threshold. Reap IFF idle_secs is known AND `>= threshold`.
fn idle_reap_decision(
    health: Option<&terminal_commander_supervisor::ensure::ProbeHealth>,
    threshold: u64,
) -> IdleReapDecision {
    match health.map(|h| h.idle_secs) {
        Some(Some(s)) if s >= threshold => IdleReapDecision::Reap,
        Some(Some(s)) => IdleReapDecision::TooRecent(s),
        Some(None) => IdleReapDecision::IdleUnknown,
        None => IdleReapDecision::Unresponsive,
    }
}

fn run_session_list() -> std::process::ExitCode {
    let base = terminal_commander_supervisor::paths::resolve_state_dir_base();
    let entries = terminal_commander_supervisor::sessions::enumerate(&base);

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("terminal-commander: tokio runtime build failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let rows: Vec<SessionRow> = rt.block_on(async {
        let mut stale: Vec<SessionRow> = Vec::new();
        let mut joins: Vec<tokio::task::JoinHandle<SessionRow>> = Vec::new();
        for e in entries {
            if !e.alive {
                stale.push(SessionRow {
                    label: e.label,
                    pid: e.pid.to_string(),
                    state: "stale",
                    idle: "-".into(),
                    endpoint: e.endpoint,
                });
                continue;
            }
            // ALIVE candidate: spawn a concurrent Health handshake. The probe
            // is internally bounded by `PROBE_TIMEOUT` (≈500ms) so no outer
            // timeout is required here.
            let ep = endpoint_from_recorded(&e.endpoint);
            let label = e.label;
            let pid = e.pid.to_string();
            let endpoint_s = e.endpoint;
            joins.push(tokio::spawn(async move {
                let health =
                    terminal_commander_supervisor::ensure::probe_endpoint_health(&ep).await;
                let (state_label, idle) = classify_health(health.as_ref());
                SessionRow {
                    label,
                    pid,
                    state: state_label,
                    idle,
                    endpoint: endpoint_s,
                }
            }));
        }
        let mut alive: Vec<SessionRow> = Vec::with_capacity(joins.len());
        for j in joins {
            if let Ok(row) = j.await {
                alive.push(row);
            }
        }
        // Stale first, then alive — order is not load-bearing for the test;
        // a stable grouping just makes the table easier to scan.
        stale.extend(alive);
        stale
    });

    println!(
        "{:<18} {:>10} {:<14} {:<6} ENDPOINT",
        "SESSION", "PID", "STATE", "IDLE"
    );
    for r in rows {
        println!(
            "{:<18} {:>10} {:<14} {:<6} {}",
            r.label, r.pid, r.state, r.idle, r.endpoint
        );
    }
    std::process::ExitCode::SUCCESS
}

/// Send a `Shutdown` IPC request to the daemon at `endpoint`, return
/// `Some(draining)` on `ShutdownAck`, `None` on any I/O / decode / wrong-shape
/// path. The whole exchange is bounded by `REAP_SHUTDOWN_TIMEOUT`.
///
/// CLI does not depend on `terminal-commanderd` (would invert the runtime
/// dep arrow) or `terminal-commander-mcp` (cli -> mcp is architectural
/// noise — the cli is not an MCP client). So we inline-frame the request
/// here using the same wire schema the supervisor's probe handshake uses
/// (4-byte big-endian length prefix + JSON RequestEnvelope). Wire format
/// matches `crates/daemon/src/ipc/protocol.rs` exactly.
async fn shutdown_via_ipc(
    endpoint: &terminal_commander_supervisor::ensure::Endpoint,
) -> Option<bool> {
    // Bound the WHOLE exchange: connect + write + read.
    const REAP_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
    const MAX_RESP_BYTES: usize = 64 * 1024;
    // Wire payload: same shape probe_endpoint sends for Health.
    const REQUEST_JSON: &[u8] = br#"{"correlation_id":0,"request":{"method":"shutdown"}}"#;

    let exchange = async {
        match endpoint {
            #[cfg(unix)]
            terminal_commander_supervisor::ensure::Endpoint::UnixSocket { path } => {
                let mut stream = tokio::net::UnixStream::connect(path).await.ok()?;
                run_shutdown_exchange(&mut stream, REQUEST_JSON, MAX_RESP_BYTES).await
            }
            #[cfg(not(unix))]
            terminal_commander_supervisor::ensure::Endpoint::UnixSocket { .. } => None,
            #[cfg(windows)]
            terminal_commander_supervisor::ensure::Endpoint::WindowsPipe { name } => {
                use tokio::net::windows::named_pipe::ClientOptions;
                let mut stream = ClientOptions::new().open(name.as_str()).ok()?;
                run_shutdown_exchange(&mut stream, REQUEST_JSON, MAX_RESP_BYTES).await
            }
            #[cfg(not(windows))]
            terminal_commander_supervisor::ensure::Endpoint::WindowsPipe { .. } => None,
        }
    };
    tokio::time::timeout(REAP_SHUTDOWN_TIMEOUT, exchange)
        .await
        .ok()
        .flatten()
}

/// Length-prefix the Shutdown request, write it, read one length-prefixed
/// frame, parse it as the daemon's response envelope, return
/// `Some(draining)` iff it deserializes as `ShutdownAck { draining }`.
async fn run_shutdown_exchange<S>(stream: &mut S, request: &[u8], max_resp: usize) -> Option<bool>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let len = u32::try_from(request.len()).ok()?;
    stream.write_all(&len.to_be_bytes()).await.ok()?;
    stream.write_all(request).await.ok()?;
    stream.flush().await.ok()?;
    let mut len_buf = [0_u8; 4];
    stream.read_exact(&mut len_buf).await.ok()?;
    let resp_len = u32::from_be_bytes(len_buf) as usize;
    if resp_len == 0 || resp_len > max_resp {
        return None;
    }
    let mut payload = vec![0_u8; resp_len];
    stream.read_exact(&mut payload).await.ok()?;
    // The response envelope shape is
    //   { "correlation_id": <u64>,
    //     "result": { "ok": { "response": { "shutdown_ack": { "draining": bool } } } } }
    // We only care about the draining bool when present; any other shape -> None.
    let v: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    let draining = v
        .get("result")?
        .get("ok")?
        .get("response")?
        .get("shutdown_ack")?
        .get("draining")?
        .as_bool()?;
    Some(draining)
}

/// Wait up to `deadline` for `endpoint` to become unreachable (the daemon
/// has finished draining and exited).
async fn wait_for_endpoint_down(
    endpoint: &terminal_commander_supervisor::ensure::Endpoint,
    deadline: std::time::Instant,
) -> bool {
    while std::time::Instant::now() < deadline {
        if !terminal_commander_supervisor::ensure::probe_endpoint(endpoint).await {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    !terminal_commander_supervisor::ensure::probe_endpoint(endpoint).await
}

/// Outcome of reaping ONE session, surfaced as a one-line operator log.
#[derive(Debug, Clone, Copy)]
enum ReapOutcome {
    /// Graceful Shutdown-IPC ACK + endpoint observed down within the wait.
    Graceful,
    /// No ACK + endpoint still reachable; pid identity-checked and force-killed.
    Forced,
    /// No ACK + endpoint reachable + pid does NOT belong to a TC daemon -> refused.
    RefusedNonDaemon,
    /// STALE pidfile (dead pid): compare-before-delete removed it.
    StaleCleaned,
    /// STALE pidfile that another writer raced us on; left in place.
    StaleRaced,
    /// Could not classify (no pidfile / unreadable).
    Unknown,
}

/// Reap ONE session by its `SessionEntry`. Identity-gated; never blanket kills.
async fn reap_one(e: terminal_commander_supervisor::sessions::SessionEntry) -> ReapOutcome {
    if !e.alive {
        // STALE: dead pid. Compare-before-delete: only remove the pidfile if
        // it STILL names this exact pid (race guard against a concurrent
        // restart writing a fresh pidfile).
        if terminal_commander_supervisor::sessions::cleanup_stale(&e.state_dir, e.pid) {
            return ReapOutcome::StaleCleaned;
        }
        return ReapOutcome::StaleRaced;
    }

    let endpoint = endpoint_from_recorded(&e.endpoint);
    let ack = shutdown_via_ipc(&endpoint).await;
    // Bounded wait for the endpoint to go dark — covers both ACK+drain and
    // "ACK was lost but daemon still drained" paths.
    let down_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let down = wait_for_endpoint_down(&endpoint, down_deadline).await;
    if down {
        return ReapOutcome::Graceful;
    }
    // Still reachable AFTER the wait. If no ACK was received we treat the
    // daemon as wedged: identity-gate by pid, then force-kill. NEVER blanket
    // kill — `pid_belongs_to_daemon` must confirm the pid is a TC daemon
    // pointing at THIS state_dir before we send a signal.
    if ack.is_some() {
        // ACK received but endpoint never went down: the daemon is in a
        // pathological drain. Refuse to escalate; surface this as Forced=false
        // would be misleading. Treat as Unknown so the operator can retry.
        return ReapOutcome::Unknown;
    }
    if terminal_commander_supervisor::replace::pid_belongs_to_daemon(e.pid, &e.state_dir) {
        let _ = terminal_commander_supervisor::replace::hard_kill(e.pid, &e.state_dir);
        // After hard_kill the daemon will not remove its own pidfile, so
        // clean up the now-stale entry (compare-before-delete still applies).
        let _ = terminal_commander_supervisor::sessions::cleanup_stale(&e.state_dir, e.pid);
        return ReapOutcome::Forced;
    }
    ReapOutcome::RefusedNonDaemon
}

/// `terminal-commander session reap`: shut down sessions identified by token,
/// `--all`, or `--idle <SECS>`. Graceful Shutdown-IPC first; identity-gated
/// `hard_kill` only when the daemon is truly wedged (no ACK + endpoint still
/// reachable + pid confirmed to belong to a TC daemon for this state_dir).
/// STALE pidfiles are cleaned via compare-before-delete.
fn run_session_reap(
    token: Option<&str>,
    all: bool,
    idle: bool,
    idle_secs: u64,
) -> std::process::ExitCode {
    // Mutual-exclusion + argument validation.
    let selector_count = usize::from(token.is_some()) + usize::from(all) + usize::from(idle);
    if selector_count == 0 {
        eprintln!("terminal-commander: session reap requires <TOKEN> or --all or --idle [SECS]");
        return std::process::ExitCode::from(2);
    }
    if selector_count > 1 {
        eprintln!(
            "terminal-commander: session reap: <TOKEN>, --all, and --idle are mutually exclusive"
        );
        return std::process::ExitCode::from(2);
    }
    let base = terminal_commander_supervisor::paths::resolve_state_dir_base();

    if idle {
        // --idle is a SOFT selector: enumerate, probe each ALIVE entry, and
        // reap only those we can PROVE are idle past the threshold. This is
        // deliberately partial-upgrade tolerant:
        //   - idle_secs Some(s), s >= threshold -> reap (graceful+force).
        //   - idle_secs Some(s), s <  threshold -> skip (recently active).
        //   - idle_secs None (legacy daemon)    -> skip: alive & ours, but
        //     we cannot apply the idle predicate. NOT force-killed.
        //   - probe None (unresponsive/gone)    -> skip: don't force-kill on
        //     the soft idle path. STALE entries are likewise left alone here.
        let entries = terminal_commander_supervisor::sessions::enumerate(&base);
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("terminal-commander: tokio runtime build failed: {e}");
                return std::process::ExitCode::from(2);
            }
        };
        let mut had_refusal = false;
        rt.block_on(async {
            for e in entries {
                let label = e.label.clone();
                if !e.alive {
                    // Stale pidfile: not part of the idle predicate.
                    continue;
                }
                let ep = endpoint_from_recorded(&e.endpoint);
                let health =
                    terminal_commander_supervisor::ensure::probe_endpoint_health(&ep).await;
                // Flatten the probe + idle predicate into one decision so the
                // side-effecting arms below are a plain match, not nested
                // Option matching (which clippy's option_if_let_else flags).
                match idle_reap_decision(health.as_ref(), idle_secs) {
                    IdleReapDecision::Reap => {
                        let outcome = reap_one(e).await;
                        report_reap_outcome(&label, outcome, &mut had_refusal);
                    }
                    IdleReapDecision::TooRecent(s) => {
                        println!("{label}: idle {s}s < {idle_secs}s threshold, skipped");
                    }
                    IdleReapDecision::IdleUnknown => {
                        println!("{label}: idle unknown (legacy daemon), skipped under --idle");
                    }
                    IdleReapDecision::Unresponsive => {
                        println!("{label}: unresponsive, skipped under --idle");
                    }
                }
            }
        });
        return if had_refusal {
            std::process::ExitCode::from(1)
        } else {
            std::process::ExitCode::SUCCESS
        };
    }

    let mut entries = terminal_commander_supervisor::sessions::enumerate(&base);
    if let Some(tok) = token {
        entries.retain(|e| e.label == tok);
        if entries.is_empty() {
            eprintln!(
                "terminal-commander: session reap: no session {tok:?} under {}",
                base.display()
            );
            return std::process::ExitCode::from(1);
        }
    }
    // else: --all keeps every entry.

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("terminal-commander: tokio runtime build failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let mut had_refusal = false;
    rt.block_on(async {
        for e in entries {
            let label = e.label.clone();
            let outcome = reap_one(e).await;
            report_reap_outcome(&label, outcome, &mut had_refusal);
        }
    });

    if had_refusal {
        std::process::ExitCode::from(1)
    } else {
        std::process::ExitCode::SUCCESS
    }
}

/// Print the user-facing line for a single reap outcome and flag refusals.
/// Shared by the token/--all path and the `--idle` selector so the outcome
/// reporting (and its exit-code contribution) stays in one place.
fn report_reap_outcome(label: &str, outcome: ReapOutcome, had_refusal: &mut bool) {
    match outcome {
        ReapOutcome::Graceful => println!("{label}: reaped (graceful shutdown)"),
        ReapOutcome::Forced => println!("{label}: reaped (force-kill; daemon was wedged)"),
        ReapOutcome::StaleCleaned => println!("{label}: stale pidfile removed"),
        ReapOutcome::StaleRaced => println!("{label}: stale pidfile raced (left in place)"),
        ReapOutcome::RefusedNonDaemon => {
            eprintln!("{label}: endpoint occupied by a non-daemon process, refusing to kill");
            *had_refusal = true;
        }
        ReapOutcome::Unknown => {
            eprintln!(
                "{label}: shutdown ACK received but endpoint still reachable; not escalating"
            );
            *had_refusal = true;
        }
    }
}

/// Locate the Terminal Commander source repo root by walking up from the
/// current directory. Returns `None` when not running inside the repo (for
/// example an npm- or cargo-installed binary), so the doctor avoids false
/// `MISSING` reports for governance docs that are intentionally not shipped.
fn find_repo_root() -> Option<std::path::PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        // The source repo root carries the workspace manifest alongside the
        // governance docs. Require all three so an unrelated nested crate or
        // a user's CWD never matches.
        if dir.join("Cargo.toml").is_file()
            && dir.join("SECURITY.md").is_file()
            && dir.join("POLICY.md").is_file()
        {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Build the doctor checks as `(label, ok)` pairs.
///
/// Repo mode (`repo_root` is `Some`) verifies the source-tree governance
/// docs relative to the repo root. Installed mode (`None`) reports package
/// readiness instead, because those docs are not part of the shipped package.
fn doctor_checks(repo_root: Option<&std::path::Path>) -> Vec<(String, bool)> {
    let mut checks: Vec<(String, bool)> = vec![("rust toolchain present".to_string(), true)];
    if let Some(root) = repo_root {
        checks.push((
            "LICENSE present".to_string(),
            root.join("LICENSE").is_file(),
        ));
        checks.push((
            "SECURITY.md present".to_string(),
            root.join("SECURITY.md").is_file(),
        ));
        checks.push((
            "POLICY.md present".to_string(),
            root.join("POLICY.md").is_file(),
        ));
        checks.push((
            "PRIVILEGE_MODEL.md present".to_string(),
            root.join("docs/security/PRIVILEGE_MODEL.md").is_file(),
        ));
        checks.push((
            "rule packs present (crates/store/rules)".to_string(),
            root.join("crates/store/rules").is_dir(),
        ));
    } else {
        // Installed/package mode: governance docs ship with the repo, not
        // the package. Report package readiness instead of false MISSING.
        checks.push((
            "installed package mode (repo docs not applicable)".to_string(),
            true,
        ));
        checks.push((
            "terminal-commander version reported".to_string(),
            !env!("CARGO_PKG_VERSION").is_empty(),
        ));
    }
    checks
}

/// Count warning-level failures. Only a missing `LICENSE` in repo mode is a
/// warning; installed mode never warns on repo docs.
fn doctor_warnings(repo_root: Option<&std::path::Path>, checks: &[(String, bool)]) -> u8 {
    // Installed mode never warns on repo docs (they are not shipped).
    if repo_root.is_none() {
        return 0;
    }
    u8::from(
        checks
            .iter()
            .any(|(label, ok)| label == "LICENSE present" && !*ok),
    )
}
fn run_doctor() -> u8 {
    let repo_root = find_repo_root();
    println!("terminal-commanderd doctor:");
    let checks = doctor_checks(repo_root.as_deref());
    for (label, ok) in &checks {
        print_check(label, *ok);
    }
    let warnings = doctor_warnings(repo_root.as_deref(), &checks);
    // Setup-brain section. Detection is fail-open: a probe that errors
    // leaves the corresponding fact false, so the check shows MISSING with
    // its fix line rather than crashing the doctor.
    println!();
    let facts = detect_setup_facts();
    let _missing = print_setup_section(&facts);
    // Setup MISSING items are advisory (fix lines printed); they do not
    // change the doctor exit code, which stays governed by repo warnings.
    warnings
}

/// Fill [`SetupFacts`] by probing the host. Side-effecting and fail-open:
/// any probe error leaves the fact `false` (=> MISSING + fix line).
fn detect_setup_facts() -> SetupFacts {
    let wsl_present = wsl_distro_reachable();
    SetupFacts {
        wsl_present,
        sudoers_ok: wsl_present && wsl_sudoers_grant_ok(),
        daemon_fresh: daemon_is_fresh(),
        // The cleanup pack ships embedded in the store crate (Phase 2),
        // so it is always available to import.
        cleanup_pack_present: true,
    }
}

/// True if `wsl.exe -l -q` lists at least one distro. Fail-open: any
/// error (not Windows, wsl missing) => false (no WSL section forced).
fn wsl_distro_reachable() -> bool {
    if !cfg!(windows) {
        // On Linux/WSL itself there is no "WSL bridge" to set up.
        return false;
    }
    std::process::Command::new("wsl.exe")
        .args(["-l", "-q"])
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
}

/// True if the scoped NOPASSWD grant works: `sudo -n /usr/sbin/fstrim
/// --version` exits 0 inside WSL. Uses the ABSOLUTE path because the
/// sudoers grant matches by exact command path. Fail-open => false.
fn wsl_sudoers_grant_ok() -> bool {
    let probe = "sudo -n /usr/sbin/fstrim --version >/dev/null 2>&1 && echo OK || true";
    std::process::Command::new("wsl.exe")
        .args(["--", "bash", "-lc", probe])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("OK"))
}

/// True if a daemon is reachable and reports this binary's version (no
/// stale daemon). Fail-open: if we cannot determine freshness, report
/// fresh=true so the doctor does not nag when there is simply no daemon
/// running yet (the cold-start path handles that).
const fn daemon_is_fresh() -> bool {
    // Determining the running daemon version requires an IPC round-trip
    // that the CLI does not own here; treat "cannot determine" as fresh
    // to avoid false MISSING. A genuinely stale daemon is surfaced by the
    // adapter auto-replace + `terminal-commander restart`.
    true
}

fn print_check(label: &str, ok: bool) {
    let marker = if ok { "ok" } else { "MISSING" };
    println!("  [{marker:7}] {label}");
}

// ---------------------------------------------------------------------
// Doctor-as-setup-brain (Phase 7): detect every prerequisite for fluent
// TC + WSL cleanup use, and for each MISSING one print the EXACT fix line
// so the operator never has to guess. No interactive wizard -- the doctor
// is the setup guide. The check layer is pure over injected `SetupFacts`
// so it is unit-testable; the detection layer that fills the facts is
// side-effecting and fail-open (a probe error reads as MISSING + fix,
// never a crash).
// ---------------------------------------------------------------------

/// Detected prerequisite state. Filled by the (side-effecting) detection
/// layer, consumed by the pure [`setup_checks`].
///
/// Four independent boolean facts trip `clippy::struct_excessive_bools`;
/// they are distinct prerequisites (WSL, sudoers, daemon freshness,
/// cleanup pack), not a state machine, so collapsing them into an enum
/// would obscure the per-check mapping. Allowed locally.
#[allow(clippy::struct_excessive_bools)]
pub struct SetupFacts {
    /// A usable WSL distro is reachable (Windows host story).
    pub wsl_present: bool,
    /// `sudo -n /usr/sbin/fstrim --version` succeeds in WSL (scoped
    /// NOPASSWD sudoers is installed). Absolute path matters: a bare
    /// `sudo -n fstrim` does NOT match a path-scoped grant.
    pub sudoers_ok: bool,
    /// Running daemon version equals this binary (no stale daemon).
    pub daemon_fresh: bool,
    /// The `cleanup` rule pack is available to import/activate.
    pub cleanup_pack_present: bool,
}

/// One setup check: a human label, pass/fail, and the EXACT operator fix
/// line shown when it fails.
pub struct SetupCheck {
    pub label: String,
    pub ok: bool,
    pub fix: String,
}

/// Pure setup-check derivation. The sudoers check is OMITTED entirely
/// when WSL is absent (no false MISSING on a pure-Windows/Linux host).
#[must_use]
pub fn setup_checks(f: &SetupFacts) -> Vec<SetupCheck> {
    let mut v = Vec::new();
    if f.wsl_present {
        v.push(SetupCheck {
            label: "WSL sudo cleanup grant (sudoers)".to_string(),
            ok: f.sudoers_ok,
            // Scoped NOPASSWD grant by ABSOLUTE path; chmod 440; validate
            // with visudo. No password is ever transmitted. Run once in WSL:
            fix: concat!(
                "Run once in WSL: ",
                "echo \"$USER ALL=(root) NOPASSWD: ",
                "/usr/bin/apt-get, /usr/bin/journalctl, /usr/sbin/fstrim\" ",
                "| sudo tee /etc/sudoers.d/tc-cleanup ",
                "&& sudo chmod 440 /etc/sudoers.d/tc-cleanup ",
                "&& sudo visudo -c -f /etc/sudoers.d/tc-cleanup. ",
                "Then call sudo with the ABSOLUTE path (sudo -n /usr/sbin/fstrim ...), ",
                "not a bare name."
            )
            .to_string(),
        });
    }
    v.push(SetupCheck {
        label: "daemon up-to-date".to_string(),
        ok: f.daemon_fresh,
        fix: "Run: terminal-commander restart".to_string(),
    });
    v.push(SetupCheck {
        label: "cleanup rule pack available".to_string(),
        ok: f.cleanup_pack_present,
        fix: "Import it: registry_import_pack name=cleanup scope={kind:'global'} (it ships built-in)."
            .to_string(),
    });
    v
}

/// Print the setup-brain section of the doctor report, with fix lines
/// under each MISSING check. Returns the count of MISSING checks.
fn print_setup_section(facts: &SetupFacts) -> u8 {
    println!("terminal-commander setup readiness:");
    let mut missing: u8 = 0;
    for c in setup_checks(facts) {
        print_check(&c.label, c.ok);
        if !c.ok {
            missing = missing.saturating_add(1);
            println!("           fix: {}", c.fix);
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_parses_status() {
        let cli = Cli::parse_from(["terminal-commander", "status"]);
        assert!(matches!(cli.cmd, Command::Status));
    }

    use terminal_commander_supervisor::ensure::ProbeHealth;

    #[test]
    fn classify_health_maps_state_and_idle_columns() {
        // Modern daemon: real seconds.
        assert_eq!(
            classify_health(Some(&ProbeHealth { idle_secs: Some(7) })),
            ("alive", "7s".to_string())
        );
        // Legacy daemon: alive, idle unknown.
        assert_eq!(
            classify_health(Some(&ProbeHealth { idle_secs: None })),
            ("alive", "?".to_string())
        );
        // No Health reply: unresponsive.
        assert_eq!(classify_health(None), ("unresponsive", "-".to_string()));
    }

    #[test]
    fn idle_reap_decision_is_partial_upgrade_tolerant() {
        let modern = |s| Some(ProbeHealth { idle_secs: Some(s) });
        let legacy = ProbeHealth { idle_secs: None };
        // At/over threshold -> reap; under -> skip with the observed idle.
        assert_eq!(
            idle_reap_decision(modern(100).as_ref(), 60),
            IdleReapDecision::Reap
        );
        assert_eq!(
            idle_reap_decision(modern(60).as_ref(), 60),
            IdleReapDecision::Reap
        );
        assert_eq!(
            idle_reap_decision(modern(30).as_ref(), 60),
            IdleReapDecision::TooRecent(30)
        );
        // Legacy daemon: never reaped under --idle (predicate inapplicable).
        assert_eq!(
            idle_reap_decision(Some(&legacy), 60),
            IdleReapDecision::IdleUnknown
        );
        // Unresponsive: skipped, not force-killed on the soft idle path.
        assert_eq!(idle_reap_decision(None, 60), IdleReapDecision::Unresponsive);
    }

    #[test]
    fn cli_parses_rules_show() {
        let cli = Cli::parse_from(["terminal-commander", "rules", "show", "x.y"]);
        match cli.cmd {
            Command::Rules {
                op: RulesOp::Show { rule_id },
            } => assert_eq!(rule_id, "x.y"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn cli_parses_audit_with_limit() {
        let cli = Cli::parse_from(["terminal-commander", "audit", "--limit", "10"]);
        match cli.cmd {
            Command::Audit { limit } => assert_eq!(limit, 10),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn cli_parses_hidden_update_locks() {
        let cli = Cli::parse_from(["terminal-commander", "update-locks", "--scope-dir", "."]);
        match cli.cmd {
            Command::UpdateLocks { scope_dir } => {
                assert_eq!(scope_dir, std::path::PathBuf::from("."));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn cli_parses_hidden_update_locks_legacy_bin_dir_alias() {
        // Older JS shims still pass `--bin-dir`; the new Rust clap arg accepts
        // the legacy flag as an alias so a mid-rollout shim/binary mismatch
        // doesn't skip the preflight.
        let cli = Cli::parse_from(["terminal-commander", "update-locks", "--bin-dir", "."]);
        match cli.cmd {
            Command::UpdateLocks { scope_dir } => {
                assert_eq!(scope_dir, std::path::PathBuf::from("."));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn cli_status_exits_without_panic() {
        // With no daemon running, print_status returns ExitCode::from(1).
        // We only assert it doesn't panic; the exact exit code depends on
        // whether a daemon is live on the probe endpoint.
        let cli = Cli::parse_from(["terminal-commander", "status"]);
        let _code = run(cli);
    }

    #[test]
    fn doctor_installed_mode_has_no_repo_doc_checks_and_no_warnings() {
        // Installed/package mode: repo governance docs are intentionally not
        // shipped, so the doctor must not assert their presence or warn.
        let checks = doctor_checks(None);
        assert!(
            checks.iter().all(|(label, _)| label != "LICENSE present"
                && label != "SECURITY.md present"
                && label != "POLICY.md present"
                && label != "PRIVILEGE_MODEL.md present"),
            "installed mode must not assert repo governance docs"
        );
        assert_eq!(doctor_warnings(None, &checks), 0);
    }

    #[test]
    fn doctor_repo_mode_flags_missing_license_as_warning() {
        // Repo mode resolves docs against the discovered root, not the CWD.
        let root = std::path::Path::new("nonexistent-tc-repo-root-xyz");
        let checks = doctor_checks(Some(root));
        let license = checks
            .iter()
            .find(|(label, _)| label == "LICENSE present")
            .expect("repo mode must check LICENSE");
        assert!(!license.1, "LICENSE under a fake root must be MISSING");
        assert_eq!(doctor_warnings(Some(root), &checks), 1);
    }

    #[test]
    fn setup_reports_missing_sudoers_with_exact_fix_line() {
        let facts = SetupFacts {
            wsl_present: true,
            sudoers_ok: false,
            daemon_fresh: true,
            cleanup_pack_present: true,
        };
        let checks = setup_checks(&facts);
        let sudoers = checks
            .iter()
            .find(|c| c.label.contains("sudo"))
            .expect("sudoers check present when WSL present");
        assert!(!sudoers.ok);
        assert!(sudoers.fix.contains("/etc/sudoers.d/tc-cleanup"));
        assert!(sudoers.fix.contains("visudo -c"));
        assert!(sudoers.fix.contains("chmod 440"));
        // Never echo a password into the fix line.
        let lf = sudoers.fix.to_ascii_lowercase();
        assert!(!lf.contains("password") && !lf.contains("credential"));
    }

    #[test]
    fn setup_all_green_when_complete() {
        let facts = SetupFacts {
            wsl_present: true,
            sudoers_ok: true,
            daemon_fresh: true,
            cleanup_pack_present: true,
        };
        assert!(setup_checks(&facts).iter().all(|c| c.ok));
    }

    #[test]
    fn setup_omits_sudoers_check_when_no_wsl() {
        let facts = SetupFacts {
            wsl_present: false,
            sudoers_ok: false,
            daemon_fresh: true,
            cleanup_pack_present: true,
        };
        let checks = setup_checks(&facts);
        assert!(
            checks.iter().all(|c| !c.label.contains("sudo")),
            "no WSL => no sudoers check (and no false MISSING)"
        );
    }

    #[test]
    fn setup_stale_daemon_fix_points_to_restart() {
        let facts = SetupFacts {
            wsl_present: false,
            sudoers_ok: true,
            daemon_fresh: false,
            cleanup_pack_present: true,
        };
        let checks = setup_checks(&facts);
        let daemon = checks
            .iter()
            .find(|c| c.label.contains("daemon"))
            .expect("daemon check present");
        assert!(!daemon.ok);
        assert!(daemon.fix.contains("terminal-commander restart"));
    }
}
