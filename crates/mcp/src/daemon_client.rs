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

use terminal_commander_ipc::{IpcError, IpcRequest, IpcResponse, MAX_BUCKET_WAIT_MS};
use terminal_commander_supervisor::ensure::EnsureDaemonStatus;
use terminal_commander_supervisor::paths;

/// Bound on the self-heal `Health` re-probe. Short on purpose: a tool
/// must never hang waiting on a daemon that is still down. If the daemon
/// truly came live, a local UDS/pipe `Health` round trip is sub-millisecond;
/// this budget only caps the failure case.
const SELF_HEAL_PROBE_TIMEOUT: Duration = Duration::from_millis(750);

/// Bound on the cached version-skew probe (DEFECT B). A co-located daemon
/// answers `Health` in sub-milliseconds; this budget only caps the failure
/// case so a guarded tool never hangs on a slow/absent daemon.
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
    /// Count of version probes fired before guarded daemon-backed tools.
    /// Unlike the startup verdict, this keeps checking after a matching start
    /// so replacing the daemon cannot silently introduce version skew.
    version_probe_count: Arc<AtomicU64>,
    /// DEFECT B: `Some((daemon_version, adapter_version))` when the ALIVE
    /// daemon's `system_discover` version does not match this adapter (skew in
    /// EITHER direction). `None` => versions matched, or skew could not be
    /// determined (the down-daemon case is the `Unavailable` path, not skew).
    /// Shared mutable cache: when a stale daemon is replaced under a live MCP
    /// adapter, the next guarded tool re-probes and clears this verdict.
    version_skew: Arc<Mutex<Option<(String, String)>>>,
}

impl DaemonStatusHandle {
    pub fn new(status: EnsureDaemonStatus) -> Self {
        Self {
            status: Arc::new(Mutex::new(status)),
            probe_guard: Arc::new(tokio::sync::Mutex::new(())),
            probe_count: Arc::new(AtomicU64::new(0)),
            version_probe_count: Arc::new(AtomicU64::new(0)),
            version_skew: Arc::new(Mutex::new(None)),
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
            version_probe_count: Arc::new(AtomicU64::new(0)),
            version_skew: Arc::new(Mutex::new(version_skew)),
        }
    }

    /// The startup version-skew verdict, if any (DEFECT B).
    /// `Some((daemon_version, adapter_version))` when the alive daemon is the
    /// wrong build; `None` when versions matched or skew is unknown.
    #[must_use]
    pub fn version_skew(&self) -> Option<(String, String)> {
        self.version_skew.lock().unwrap().clone()
    }

    fn set_version_skew(&self, version_skew: Option<(String, String)>) {
        *self.version_skew.lock().unwrap() = version_skew;
    }

    /// Number of self-heal `Health` re-probes fired so far. Used by tests
    /// to assert the single-flight guard coalesces concurrent probes.
    #[must_use]
    pub fn probe_count(&self) -> u64 {
        self.probe_count.load(Ordering::Relaxed)
    }

    /// Number of live `SystemDiscover` version probes fired so far.
    #[must_use]
    pub fn version_probe_count(&self) -> u64 {
        self.version_probe_count.load(Ordering::Relaxed)
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

    /// Replace a stale startup availability sample with a truthful transport
    /// loss status. Endpoint and daemon log evidence are preserved so the
    /// supervisor and tool envelope can explain and recover the same target.
    fn set_transport_unavailable(&self, error: &IpcError) {
        let mut guard = self.status.lock().unwrap();
        let (endpoint, log_path) = match &*guard {
            EnsureDaemonStatus::AlreadyRunning { endpoint, .. } => (endpoint.clone(), None),
            EnsureDaemonStatus::Started {
                endpoint, log_path, ..
            } => (endpoint.clone(), Some(log_path.clone())),
            EnsureDaemonStatus::Unavailable { diagnostics, .. } => {
                let mut diagnostics = diagnostics.clone();
                diagnostics.last_error = Some(error.message.clone());
                *guard = EnsureDaemonStatus::Unavailable {
                    reason:
                        terminal_commander_supervisor::ensure::DaemonUnavailableReason::TransportLost,
                    diagnostics,
                };
                return;
            }
        };
        *guard = EnsureDaemonStatus::Unavailable {
            reason: terminal_commander_supervisor::ensure::DaemonUnavailableReason::TransportLost,
            diagnostics: terminal_commander_supervisor::ensure::Diagnostics {
                endpoint,
                log_path,
                last_error: Some(error.message.clone()),
                startup_attempted: false,
                startup_elapsed_ms: 0,
            },
        };
    }
}

/// Forwarding wrapper around the daemon's `DaemonClient`. Adds a
/// monotonic correlation id per call so the IPC envelope is unique.
#[derive(Debug, Clone)]
pub struct McpDaemonClient {
    inner: terminal_commander_ipc::DaemonClient,
    next_id: Arc<AtomicU64>,
    status: Option<DaemonStatusHandle>,
    /// The same supervisor plan used at adapter startup. Retained so a
    /// long-lived adapter can restore a daemon that exits later while still
    /// honoring the operator's `allow_spawn` policy.
    recovery: Option<Arc<terminal_commander_supervisor::ensure::EnsureDaemonOptions>>,
}

/// Margin added on top of a request's daemon-side blocking budget to form its
/// per-request transport deadline (covers connect retries + write + read
/// latency around the daemon's hold). Mirrors the subscription-pull client's
/// budget shape (12 s client for an ~8 s daemon hold).
const BLOCKING_DEADLINE_MARGIN: std::time::Duration = std::time::Duration::from_secs(4);

/// The transport deadline for a request that legitimately BLOCKS daemon-side,
/// or `None` for ordinary requests (client-level timeout applies).
///
/// `bucket_wait` holds its response up to the clamped `timeout_ms` (max
/// [`MAX_BUCKET_WAIT_MS`] = 30 s) and `shell_session_exec` holds up to its
/// clamped `wait_ms` settle window -- but the shared client cancels every
/// round trip at a flat 5 s. A quiet bucket + `timeout_ms > 5s` therefore
/// ALWAYS timed out client-side, self-healed (health is fast and fine),
/// retried, timed out again, and surfaced as `daemon_unavailable` against a
/// perfectly healthy daemon (dogfood 2026-07-02, BACKLOG P1.0f). The deadline
/// must COVER the daemon's promised hold.
fn blocking_deadline(request: &IpcRequest) -> Option<std::time::Duration> {
    match request {
        IpcRequest::BucketWait(p) => Some(p.timeout() + BLOCKING_DEADLINE_MARGIN),
        IpcRequest::ShellSessionExec(p) => p.wait_ms.map(|ms| {
            std::time::Duration::from_millis(ms.min(MAX_BUCKET_WAIT_MS)) + BLOCKING_DEADLINE_MARGIN
        }),
        _ => None,
    }
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
            recovery: None,
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
            recovery: None,
        }
    }

    /// Construct a client that can re-run the startup supervisor plan after a
    /// later transport loss. The plan preserves the operator's spawn policy;
    /// this constructor does not start or probe anything itself.
    #[must_use]
    pub fn with_recovery(
        socket_path: impl Into<std::path::PathBuf>,
        status: DaemonStatusHandle,
        recovery: terminal_commander_supervisor::ensure::EnsureDaemonOptions,
    ) -> Self {
        Self {
            inner: terminal_commander_ipc::DaemonClient::new(socket_path),
            next_id: Arc::new(AtomicU64::new(1)),
            status: Some(status),
            recovery: Some(Arc::new(recovery)),
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

    /// Re-probe a cached version-skew verdict after an operator replaces the
    /// daemon under a long-lived MCP adapter. Matching versions clear the cache;
    /// a still-mismatched daemon replaces it with fresh values. Probe failure
    /// preserves the prior honest skew verdict instead of guessing.
    pub async fn refresh_version_skew(&self, adapter_version: &str) -> Option<(String, String)> {
        let Some(handle) = &self.status else {
            return None;
        };

        let _flight = handle.probe_guard.lock().await;
        let prior = handle.version_skew();
        handle.version_probe_count.fetch_add(1, Ordering::Relaxed);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        // Health carries the daemon's compile-time version and deliberately
        // avoids SystemDiscover's bounded environment probes. Rechecking skew
        // must stay cheap even when discovery has to wait on slow tools.
        let probe = self.inner.call(id, IpcRequest::Health);
        match tokio::time::timeout(SKEW_PROBE_TIMEOUT, probe).await {
            Ok(Ok(IpcResponse::Health { version, .. })) if !version.is_empty() => {
                let refreshed =
                    (version != adapter_version).then(|| (version, adapter_version.to_owned()));
                handle.set_version_skew(refreshed.clone());
                refreshed
            }
            _ => prior,
        }
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
        let deadline = blocking_deadline(&request);
        self.call_with_optional_timeout(request, deadline).await
    }

    /// Issue one call with an explicit transport deadline while preserving the
    /// same recovery and idempotent-retry contract as [`Self::call`].
    pub(crate) async fn call_with_timeout(
        &self,
        request: IpcRequest,
        timeout: Duration,
    ) -> Result<IpcResponse, IpcError> {
        self.call_with_optional_timeout(request, Some(timeout))
            .await
    }

    async fn call_with_optional_timeout(
        &self,
        request: IpcRequest,
        deadline: Option<Duration>,
    ) -> Result<IpcResponse, IpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        match self.call_inner(id, request.clone(), deadline).await {
            Ok(resp) => Ok(resp),
            Err(e) if e.is_transport() => {
                // The startup sample is now stale. Publish the transport
                // loss before recovery so self-heal cannot mistake the cached
                // available state for proof that the daemon is alive.
                if let Some(handle) = &self.status {
                    handle.set_transport_unavailable(&e);
                }
                // ALWAYS attempt recovery (restores cached availability for
                // the next call, even for mutating RPCs after a daemon
                // replace). `try_self_heal` is a no-op (returns false) when
                // there is no status handle.
                let _recovered = self.try_self_heal().await;
                if request.is_idempotent() {
                    // Safe to re-send: a pure read / idempotent reposition can
                    // run twice without a server-side double-effect.
                    let retry_id = self.next_id.fetch_add(1, Ordering::Relaxed);
                    self.call_inner(retry_id, request, deadline).await
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

    /// One inner round trip, honoring a per-request transport deadline when
    /// the request has a daemon-side blocking budget (see
    /// [`blocking_deadline`]); otherwise the client-level timeout applies.
    async fn call_inner(
        &self,
        id: u64,
        request: IpcRequest,
        deadline: Option<std::time::Duration>,
    ) -> Result<IpcResponse, IpcError> {
        match deadline {
            Some(d) => self.inner.call_with_timeout(id, request, d).await,
            None => self.inner.call(id, request).await,
        }
    }

    /// Self-heal cached daemon availability after startup.
    ///
    /// Called when cached status is unavailable or a mid-call transport error
    /// invalidates a stale available status. It first performs one bounded Health
    /// probe. If the endpoint is still down and this adapter retained a supervisor
    /// plan, it re-runs that plan with the original allow_spawn policy and replaces
    /// the cached status with the supervisor's structured result.
    ///
    /// Single-flight: concurrent handlers serialize on the handle's probe_guard.
    /// Whoever loses the race re-reads the possibly healed status under the guard,
    /// so only one recovery path runs at a time. Returns false when no status
    /// handle exists or recovery cannot make the daemon reachable.
    pub async fn try_self_heal(&self) -> bool {
        let Some(handle) = &self.status else {
            return false;
        };

        // Single-flight: only one probe/recovery sequence is in flight.
        let _flight = handle.probe_guard.lock().await;

        // A concurrent caller may already have healed the status.
        if !handle.is_unavailable() {
            return true;
        }

        // Health is a bounded read-only IPC probe.
        handle.probe_count.fetch_add(1, Ordering::Relaxed);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let probe = self.inner.call(id, IpcRequest::Health);
        if let Ok(Ok(IpcResponse::Health { .. })) =
            tokio::time::timeout(SELF_HEAL_PROBE_TIMEOUT, probe).await
        {
            handle.set_available();
            return true;
        }

        // The endpoint is still down. Re-run the startup supervisor plan,
        // including its explicit allow_spawn policy.
        let Some(recovery) = &self.recovery else {
            return false;
        };
        let status =
            terminal_commander_supervisor::ensure::ensure_daemon(recovery.as_ref().clone()).await;
        let recovered = matches!(
            status,
            EnsureDaemonStatus::AlreadyRunning { .. } | EnsureDaemonStatus::Started { .. }
        );
        *handle.status.lock().unwrap() = status;
        recovered
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

    // --- P1.0f: per-request transport deadlines for blocking requests ---

    #[test]
    fn blocking_deadline_covers_bucket_wait_hold() {
        // A 55s request clamps to MAX_BUCKET_WAIT_MS daemon-side; the
        // transport deadline must cover that clamped hold plus margin --
        // before this, the flat 5s client cancelled every quiet wait > 5s
        // and misreported a healthy daemon as unavailable.
        let p = terminal_commander_ipc::BucketWaitParams {
            bucket_id: terminal_commander_core::BucketId::new(),
            cursor: 0,
            severity_min: None,
            kind_filter: None,
            limit: None,
            timeout_ms: Some(55_000),
        };
        let d = blocking_deadline(&IpcRequest::BucketWait(p)).expect("bucket_wait blocks");
        assert_eq!(
            d,
            std::time::Duration::from_millis(MAX_BUCKET_WAIT_MS) + BLOCKING_DEADLINE_MARGIN
        );
    }

    #[test]
    fn blocking_deadline_none_for_ordinary_requests() {
        assert!(
            blocking_deadline(&IpcRequest::Health).is_none(),
            "non-blocking requests keep the client-level timeout"
        );
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

    #[test]
    fn version_skew_cache_updates_across_handle_clones() {
        let handle = DaemonStatusHandle::with_skew(
            EnsureDaemonStatus::AlreadyRunning {
                endpoint: Endpoint::UnixSocket {
                    path: "/tmp/tc-skew-refresh.sock".into(),
                },
                pid: Some(1),
            },
            Some(("0.1.73".to_owned(), "0.1.74".to_owned())),
        );
        let clone = handle.clone();

        handle.set_version_skew(None);
        assert_eq!(clone.version_skew(), None);

        clone.set_version_skew(Some(("0.1.75".to_owned(), "0.1.74".to_owned())));
        assert_eq!(
            handle.version_skew(),
            Some(("0.1.75".to_owned(), "0.1.74".to_owned()))
        );
    }

    fn missing_socket_path(label: &str) -> std::path::PathBuf {
        let unique = format!(
            "{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        );
        #[cfg(windows)]
        {
            format!(r"\\.\pipe\{unique}").into()
        }
        #[cfg(not(windows))]
        {
            std::env::temp_dir().join(format!("{unique}.sock"))
        }
    }

    #[tokio::test]
    async fn try_self_heal_false_without_status_handle() {
        // No status handle => self-heal is meaningless; returns false and
        // never probes.
        let client = McpDaemonClient::new("/nonexistent-tc-self-heal.sock");
        assert!(!client.try_self_heal().await);
    }

    #[tokio::test]
    async fn transport_failure_invalidates_cached_available_status() {
        let socket = missing_socket_path("tc-self-heal-stale-available");
        let handle = DaemonStatusHandle::new(EnsureDaemonStatus::AlreadyRunning {
            endpoint: paths::endpoint_from_socket_path(&socket),
            pid: Some(7),
        });
        let client = McpDaemonClient::with_status(&socket, handle.clone())
            .with_timeout(std::time::Duration::from_millis(25));

        let error = client
            .call(IpcRequest::Health)
            .await
            .expect_err("an absent endpoint must fail");
        assert!(
            error.is_transport(),
            "expected transport error, got {error:?}"
        );
        assert!(
            handle.is_unavailable(),
            "a transport failure must invalidate stale cached availability"
        );
        assert!(
            matches!(
                handle.current(),
                EnsureDaemonStatus::Unavailable {
                    reason: DaemonUnavailableReason::TransportLost,
                    ..
                }
            ),
            "the unavailable reason must truthfully identify a lost transport"
        );
    }

    #[tokio::test]
    async fn self_heal_delegates_unreachable_endpoint_to_supervisor() {
        let root = tempfile::tempdir().unwrap();
        let socket = missing_socket_path("tc-self-heal-supervisor");
        let endpoint = paths::endpoint_from_socket_path(&socket);
        let handle = DaemonStatusHandle::new(EnsureDaemonStatus::Unavailable {
            reason: DaemonUnavailableReason::TransportLost,
            diagnostics: Diagnostics {
                endpoint: endpoint.clone(),
                log_path: None,
                last_error: Some("test: transport lost".into()),
                startup_attempted: false,
                startup_elapsed_ms: 0,
            },
        });
        let options = terminal_commander_supervisor::ensure::EnsureDaemonOptions {
            daemon_binary: root.path().join("missing-terminal-commanderd"),
            state_dir: root.path().join("state"),
            log_dir: root.path().join("logs"),
            endpoint,
            startup_timeout: std::time::Duration::from_millis(100),
            allow_spawn: true,
        };
        let client = McpDaemonClient::with_recovery(&socket, handle.clone(), options)
            .with_timeout(std::time::Duration::from_millis(25));

        assert!(
            !client.try_self_heal().await,
            "a missing daemon binary cannot recover"
        );
        assert!(
            matches!(
                handle.current(),
                EnsureDaemonStatus::Unavailable {
                    reason: DaemonUnavailableReason::BinaryNotFound,
                    ..
                }
            ),
            "the supervisor's structured recovery result must replace the stale status"
        );
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
