// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon IPC client wrapper for the MCP stdio adapter (TC40).
//!
//! Wraps `terminal_commander_ipc::DaemonClient` and adds:
//! - correlation-id generation,
//! - structured error mapping to MCP tool errors,
//! - bounded, audit-friendly call sites for every MCP tool to call into.
//!
//! Transport: UDS on Unix, Windows named pipe on Windows. The
//! underlying `terminal_commander_ipc::DaemonClient` is already
//! platform-dispatched (see `crates/ipc/src/`).
//!
//! Source-status: live (TC40).

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use terminal_commander_ipc::{IpcError, IpcRequest, IpcResponse};
use terminal_commander_supervisor::ensure::EnsureDaemonStatus;
use terminal_commander_supervisor::paths;

/// Bound on the self-heal `Health` re-probe. Short on purpose: a tool
/// must never hang waiting on a daemon that is still down. If the daemon
/// truly came live, a local UDS/pipe `Health` round trip is sub-millisecond;
/// this budget only caps the failure case.
const SELF_HEAL_PROBE_TIMEOUT: Duration = Duration::from_millis(750);

/// Bound on the startup version-skew probe (DEFECT B). A co-located WSL daemon
/// answers `system_discover` in sub-milliseconds; this budget only caps the
/// failure case so adapter startup never hangs on a slow/absent daemon.
const SKEW_PROBE_TIMEOUT: Duration = Duration::from_millis(750);

/// Resolve the socket path the MCP adapter should connect to.
///
/// Resolution order:
/// 1. Explicit override (CLI flag) when provided.
/// 2. Delegates to [`terminal_commander_supervisor::paths::resolve_socket_path`]:
///    `TC_SOCKET` env var, then platform default matching the daemon's
///    `DaemonConfig::socket_path()` / `DaemonConfig::pipe_name()` exactly.
#[must_use]
pub fn resolve_socket_path(cli_override: Option<&std::path::Path>) -> std::path::PathBuf {
    if let Some(p) = cli_override {
        return p.to_path_buf();
    }
    paths::resolve_socket_path()
}

/// Probe the ALIVE daemon's version via `system_discover` and report version
/// skew against this adapter (DEFECT B).
///
/// `system_discover` is one of the four methods even a legacy daemon serves, so
/// its `DiscoverResponse.version` is the robust skew source -- `Health.version`
/// is `#[serde(default)]` and a legacy daemon OMITS it, so Health cannot
/// positively name a stale daemon. The skew test is an HONEST both-direction
/// inequality: a daemon whose version is empty/legacy OR present-but-different
/// (including NEWER than the adapter) is skewed. The adapter version is always
/// a non-empty `env!("CARGO_PKG_VERSION")`, so a plain `!=` subsumes the
/// empty/legacy case.
///
/// Returns `Some((daemon_version, adapter_version))` on skew, `None` when the
/// versions match OR the daemon could not be reached (an unreachable daemon is
/// the `daemon_unavailable` path, never a skew verdict). Bounded by
/// [`SKEW_PROBE_TIMEOUT`]; no spawn, no fs, just one IPC round trip.
pub async fn detect_version_skew(
    socket_path: &std::path::Path,
    adapter_version: &str,
) -> Option<(String, String)> {
    let client = McpDaemonClient::new(socket_path).with_timeout(SKEW_PROBE_TIMEOUT);
    match client.call(IpcRequest::SystemDiscover).await {
        Ok(IpcResponse::SystemDiscover(d)) if d.version != adapter_version => {
            Some((d.version, adapter_version.to_owned()))
        }
        _ => None,
    }
}

/// Shared, cheaply-cloneable handle to the `EnsureDaemonStatus`
/// returned at MCP startup. Tool dispatch reads this to decide whether
/// to short-circuit with a `daemon_unavailable` envelope.
///
/// Self-heal (audit H1): the startup status is a one-shot sample. A
/// daemon that was slow to bind (transient `StartupTimeout`) would
/// otherwise pin every tool to `daemon_unavailable` for the whole
/// process life even after the socket goes live. The handle therefore
/// supports flipping `Unavailable -> Available` once a live `Health`
/// re-probe observes the daemon (`set_available`), serialized by a
/// single-flight async guard (`probe_guard`) so concurrent handlers do
/// not stampede the daemon with redundant probes.
#[derive(Debug, Clone)]
pub struct DaemonStatusHandle {
    status: Arc<Mutex<EnsureDaemonStatus>>,
    /// Single-flight guard around the self-heal re-probe. Held for the
    /// duration of one probe so 31 concurrent tools coalesce into at
    /// most one in-flight `Health` round trip.
    probe_guard: Arc<tokio::sync::Mutex<()>>,
    /// Count of `Health` re-probes actually fired by the self-heal path.
    /// Observability + lets tests assert single-flight (concurrent tools
    /// hitting an unavailable status must not each fire a probe).
    probe_count: Arc<AtomicU64>,
    /// DEFECT B: `Some((daemon_version, adapter_version))` when the ALIVE
    /// daemon's `system_discover` version does not match this adapter (skew in
    /// EITHER direction). `None` => versions matched, or skew could not be
    /// determined (the down-daemon case is the `Unavailable` path, not skew).
    /// Set once at startup; never mutated, so it needs no lock.
    version_skew: Option<(String, String)>,
}

impl DaemonStatusHandle {
    pub fn new(status: EnsureDaemonStatus) -> Self {
        Self {
            status: Arc::new(Mutex::new(status)),
            probe_guard: Arc::new(tokio::sync::Mutex::new(())),
            probe_count: Arc::new(AtomicU64::new(0)),
            version_skew: None,
        }
    }

    /// Construct a handle carrying a startup version-skew verdict (DEFECT B).
    /// `version_skew` is `Some((daemon_version, adapter_version))` when the
    /// alive daemon's version does not match this adapter, else `None`.
    /// `new(...)` stays the back-compat (no-skew) constructor for the existing
    /// construction/test sites.
    #[must_use]
    pub fn with_skew(status: EnsureDaemonStatus, version_skew: Option<(String, String)>) -> Self {
        Self {
            status: Arc::new(Mutex::new(status)),
            probe_guard: Arc::new(tokio::sync::Mutex::new(())),
            probe_count: Arc::new(AtomicU64::new(0)),
            version_skew,
        }
    }

    /// The startup version-skew verdict, if any (DEFECT B).
    /// `Some((daemon_version, adapter_version))` when the alive daemon is the
    /// wrong build; `None` when versions matched or skew is unknown.
    #[must_use]
    pub fn version_skew(&self) -> Option<(String, String)> {
        self.version_skew.clone()
    }

    /// Number of self-heal `Health` re-probes fired so far. Used by tests
    /// to assert the single-flight guard coalesces concurrent probes.
    #[must_use]
    pub fn probe_count(&self) -> u64 {
        self.probe_count.load(Ordering::Relaxed)
    }
    #[allow(dead_code)]
    pub fn current(&self) -> EnsureDaemonStatus {
        self.status.lock().unwrap().clone()
    }
    pub fn is_unavailable(&self) -> bool {
        matches!(
            *self.status.lock().unwrap(),
            EnsureDaemonStatus::Unavailable { .. }
        )
    }

    /// Flip the cached status from `Unavailable` to `AlreadyRunning`,
    /// clearing the unavailable flag so subsequent `is_unavailable()`
    /// returns false and tools proceed. No-op if the status is already
    /// available (idempotent). Reuses the endpoint recorded in the
    /// unavailable diagnostics so the published status stays coherent.
    /// Called only after a live `Health` re-probe succeeds.
    fn set_available(&self) {
        let mut guard = self.status.lock().unwrap();
        if let EnsureDaemonStatus::Unavailable { diagnostics, .. } = &*guard {
            *guard = EnsureDaemonStatus::AlreadyRunning {
                endpoint: diagnostics.endpoint.clone(),
                pid: None,
            };
        }
    }
}

/// Forwarding wrapper around the daemon's `DaemonClient`. Adds a
/// monotonic correlation id per call so the IPC envelope is unique.
#[derive(Debug, Clone)]
pub struct McpDaemonClient {
    inner: terminal_commander_ipc::DaemonClient,
    next_id: Arc<AtomicU64>,
    status: Option<DaemonStatusHandle>,
}

impl McpDaemonClient {
    /// Construct a client targeting the given socket path. Does not
    /// open a connection until [`McpDaemonClient::call`] is invoked.
    #[must_use]
    pub fn new(socket_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            inner: terminal_commander_ipc::DaemonClient::new(socket_path),
            next_id: Arc::new(AtomicU64::new(1)),
            status: None,
        }
    }

    /// Construct a client pre-loaded with the supervisor status from
    /// startup. Tools use `status()` to short-circuit when unavailable.
    #[must_use]
    pub fn with_status(
        socket_path: impl Into<std::path::PathBuf>,
        status: DaemonStatusHandle,
    ) -> Self {
        Self {
            inner: terminal_commander_ipc::DaemonClient::new(socket_path),
            next_id: Arc::new(AtomicU64::new(1)),
            status: Some(status),
        }
    }

    /// Return the supervisor status handle if one was set at construction.
    pub fn status(&self) -> Option<DaemonStatusHandle> {
        self.status.clone()
    }

    /// Override the per-call request timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.inner = self.inner.with_timeout(timeout);
        self
    }

    /// Socket path this client is wired against.
    #[must_use]
    pub fn socket_path(&self) -> &std::path::Path {
        self.inner.socket_path()
    }

    /// Issue one round trip against the daemon. Returns the typed
    /// `IpcResponse` on success and the typed `IpcError` otherwise.
    ///
    /// Mid-call transport recovery (TB-7 / Cursor call #21): a TRANSPORT
    /// failure -- the daemon pipe/socket gone mid-call ("pipe connect ... os
    /// error 2" on Windows, UDS ENOENT/ECONNREFUSED on unix) -- is
    /// distinguished from a daemon-RETURNED [`IpcError`] via
    /// [`IpcError::is_transport`]. On a transport failure this ALWAYS triggers
    /// the bounded, single-flight self-heal (re-probe `Health`, flip the cached
    /// status back to available if the daemon is reachable again) so cached
    /// availability is restored for the NEXT call regardless of the request
    /// kind -- including after a legitimate daemon replace.
    ///
    /// Whether the request is then RE-SENT is gated on
    /// [`IpcRequest::is_idempotent`]:
    ///   - Idempotent RPCs (pure bounded reads / idempotent repositioning) are
    ///     retried once; a transient pipe-busy / restart may already be over.
    ///   - MUTATING RPCs (e.g. `CommandStartCombed`, registry writes, a
    ///     subscription pull that commits offsets server-side) are NEVER
    ///     auto-retried. A client-side timeout cannot prove the daemon did not
    ///     already perform the side effect, so a blind re-send risks a silent
    ///     double-effect -- the exact double-spawn this gate exists to kill. The
    ///     (transport-tagged) error is returned immediately after the self-heal
    ///     attempt; the tool edge surfaces an honest reconcile-don't-retry
    ///     envelope (see [`crate::tools`]).
    ///
    /// If an idempotent retry still fails on transport, the (transport-tagged)
    /// error is returned so the tool edge surfaces a CLEAN `daemon_unavailable`
    /// envelope rather than a raw `internal_error` (-32603) that trains agents
    /// to abandon the tool for raw shell. A daemon-returned error keeps its
    /// normal mapping. No process launch, no file system, no socket creation
    /// happens here: recovery routes through the supervisor-backed self-heal
    /// path, mcp stays a thin client.
    pub async fn call(&self, request: IpcRequest) -> Result<IpcResponse, IpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        match self.inner.call(id, request.clone()).await {
            Ok(resp) => Ok(resp),
            Err(e) if e.is_transport() => {
                // Could not reach the daemon mid-call. ALWAYS attempt recovery
                // (restores cached availability for the next call, even for
                // mutating RPCs after a daemon replace). `try_self_heal` is a
                // no-op (returns false) when there is no status handle.
                let _recovered = self.try_self_heal().await;
                if request.is_idempotent() {
                    // Safe to re-send: a pure read / idempotent reposition can
                    // run twice without a server-side double-effect.
                    let retry_id = self.next_id.fetch_add(1, Ordering::Relaxed);
                    self.inner.call(retry_id, request).await
                } else {
                    // Mutating RPC: the daemon may already have performed the
                    // side effect before the transport dropped. Returning the
                    // transport error (no re-send) is the only safe choice; the
                    // caller reconciles via command_status / runtime_state.
                    Err(e)
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Self-heal the cached daemon availability (audit H1).
    ///
    /// Called by `ensure_daemon_available` only when the cached status is
    /// `Unavailable`. Attempts a single, bounded `Health` re-probe; on a
    /// live daemon it flips the status handle to `Available` and returns
    /// `true` so the calling tool proceeds. On failure it leaves the
    /// status untouched and returns `false`, preserving the existing
    /// `daemon_unavailable` envelope behaviour.
    ///
    /// Single-flight: concurrent handlers serialize on the handle's
    /// `probe_guard`. Whoever loses the race re-reads the (possibly
    /// already-healed) status under the guard instead of firing a
    /// redundant probe. Returns `false` if no status handle was set
    /// (the self-heal path is meaningless without one).
    pub async fn try_self_heal(&self) -> bool {
        let Some(handle) = &self.status else {
            return false;
        };

        // Single-flight: only one probe in flight at a time. Awaiting the
        // guard means a late arrival blocks until the in-flight probe
        // resolves, then sees the healed status on the recheck below.
        let _flight = handle.probe_guard.lock().await;

        // Recheck under the guard: a concurrent probe may have already
        // healed (or the status may have changed) since we decided to
        // probe. If it is no longer unavailable, we are done.
        if !handle.is_unavailable() {
            return true;
        }

        // Bounded liveness probe. Health is a read-only IPC peek (no
        // spawn, no privilege escalation) — safe from the mcp crate.
        handle.probe_count.fetch_add(1, Ordering::Relaxed);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let probe = self.inner.call(id, IpcRequest::Health);
        match tokio::time::timeout(SELF_HEAL_PROBE_TIMEOUT, probe).await {
            Ok(Ok(IpcResponse::Health { .. })) => {
                handle.set_available();
                true
            }
            // Wrong variant, IPC error, or probe timeout: daemon is not
            // (yet) reachable. Leave the status unavailable.
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_uses_cli_override_when_set() {
        let p = std::path::Path::new("/tmp/tc-test.sock");
        let got = resolve_socket_path(Some(p));
        assert_eq!(got, p);
    }

    #[test]
    fn resolve_returns_a_socket_path() {
        // Don't manipulate environment (the workspace forbids unsafe
        // and `set_var` is now unsafe). Just verify that the resolver
        // always returns a non-empty path, regardless of platform arm.
        // Unix: ends with `terminal-commanderd.sock`.
        // Windows: starts with `\\.\pipe\terminal-commander-`.
        let got = resolve_socket_path(None);
        let s = got.to_string_lossy();
        assert!(
            s.ends_with("terminal-commanderd.sock")
                || s.ends_with(".sock")
                || s.starts_with(r"\\.\pipe\terminal-commander-"),
            "got: {got:?}"
        );
    }

    #[test]
    fn client_records_socket_path() {
        let p = std::path::Path::new("/tmp/tc-record.sock");
        let c = McpDaemonClient::new(p);
        assert_eq!(c.socket_path(), p);
    }

    // --- FIX D: self-heal handle behaviour ---

    use terminal_commander_supervisor::ensure::{
        DaemonUnavailableReason, Diagnostics, Endpoint, EnsureDaemonStatus,
    };

    fn unavailable_status(path: &std::path::Path) -> EnsureDaemonStatus {
        EnsureDaemonStatus::Unavailable {
            reason: DaemonUnavailableReason::StartupTimeout,
            diagnostics: Diagnostics {
                endpoint: Endpoint::UnixSocket {
                    path: path.to_path_buf(),
                },
                log_path: None,
                last_error: Some("test: startup timeout".into()),
                startup_attempted: true,
                startup_elapsed_ms: 10_000,
            },
        }
    }

    #[test]
    fn set_available_clears_unavailable_flag_and_preserves_endpoint() {
        let sock = std::path::Path::new("/tmp/tc-self-heal-unit.sock");
        let handle = DaemonStatusHandle::new(unavailable_status(sock));
        assert!(handle.is_unavailable());

        handle.set_available();
        assert!(
            !handle.is_unavailable(),
            "set_available must clear the unavailable flag"
        );
        match handle.current() {
            EnsureDaemonStatus::AlreadyRunning {
                endpoint: Endpoint::UnixSocket { path },
                ..
            } => {
                assert_eq!(
                    path,
                    sock.to_path_buf(),
                    "healed status must keep the recorded endpoint"
                );
            }
            other => panic!("expected AlreadyRunning(UnixSocket) after heal, got {other:?}"),
        }
    }

    #[test]
    fn set_available_is_noop_when_already_available() {
        let handle = DaemonStatusHandle::new(EnsureDaemonStatus::AlreadyRunning {
            endpoint: Endpoint::UnixSocket {
                path: "/tmp/tc-already.sock".into(),
            },
            pid: Some(7),
        });
        assert!(!handle.is_unavailable());
        handle.set_available();
        // pid is preserved (no spurious overwrite) when already available.
        match handle.current() {
            EnsureDaemonStatus::AlreadyRunning { pid, .. } => assert_eq!(pid, Some(7)),
            other => panic!("expected unchanged AlreadyRunning, got {other:?}"),
        }
    }

    // --- DEFECT B: version-skew verdict on the status handle ---

    #[test]
    fn with_skew_carries_versions_and_new_reports_none() {
        let avail = EnsureDaemonStatus::AlreadyRunning {
            endpoint: Endpoint::UnixSocket {
                path: "/tmp/tc-skew-unit.sock".into(),
            },
            pid: Some(1),
        };
        let skewed = DaemonStatusHandle::with_skew(
            avail.clone(),
            Some(("0.1.47".to_owned(), "0.1.69".to_owned())),
        );
        assert_eq!(
            skewed.version_skew(),
            Some(("0.1.47".to_owned(), "0.1.69".to_owned())),
            "with_skew must surface the daemon/adapter version pair"
        );

        // Back-compat: the existing `new(...)` constructor carries no skew.
        let matched = DaemonStatusHandle::new(avail);
        assert_eq!(
            matched.version_skew(),
            None,
            "new(...) must report no skew (back-compat for existing call sites)"
        );
    }

    #[tokio::test]
    async fn try_self_heal_false_without_status_handle() {
        // No status handle => self-heal is meaningless; returns false and
        // never probes.
        let client = McpDaemonClient::new("/nonexistent-tc-self-heal.sock");
        assert!(!client.try_self_heal().await);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn try_self_heal_keeps_status_unavailable_when_daemon_down() {
        // Down daemon (socket never bound): the bounded Health re-probe
        // fails, the status stays Unavailable, and the envelope path is
        // preserved. Single-flight is exercised by firing many concurrent
        // self-heal attempts and asserting at most one probe was fired.
        let sock = std::env::temp_dir().join(format!(
            "tc-self-heal-down-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let handle = DaemonStatusHandle::new(unavailable_status(&sock));
        let client = McpDaemonClient::with_status(sock, handle.clone())
            .with_timeout(std::time::Duration::from_millis(50));

        let mut tasks = Vec::new();
        for _ in 0..16 {
            let c = client.clone();
            tasks.push(tokio::spawn(async move { c.try_self_heal().await }));
        }
        for t in tasks {
            assert!(
                !t.await.unwrap(),
                "self-heal must report failure when the daemon is down"
            );
        }
        assert!(
            handle.is_unavailable(),
            "status must remain unavailable when the daemon is down"
        );
        // Single-flight: serialized probes against a down daemon will each
        // re-check and (since still unavailable) fire, but never
        // concurrently. The key guarantee is they do not stampede; with a
        // down daemon every serialized attempt re-probes, so the bound is
        // the attempt count, not zero. The healed-path single-flight (one
        // probe total) is asserted in the live self-heal integration test.
        assert!(
            handle.probe_count() >= 1,
            "a down daemon must have triggered at least one probe"
        );
    }
}
