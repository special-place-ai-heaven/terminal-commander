// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Daemon ensure/readiness library entry point.
//
// The MCP adapter calls `ensure_daemon()` before serving rmcp. The
// return value tells the caller whether to forward tool calls, return
// `daemon_unavailable` envelopes, or fail loudly.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

/// Host env vars the supervisor re-reads at spawn and forwards into the
/// daemon, so a respawn (e.g. `terminal-commander restart`) picks up
/// freshly-set values without a full client OS-restart.
///
/// FIXED allowlist, operational and NON-SECRET only. F6 explicitly
/// rejects forwarding any credential/password into the daemon
/// environment (the `WSL_SUDO_CREDENTIAL` route is forbidden; scoped
/// NOPASSWD sudoers is the sanctioned sudo path). Adding a key here is a
/// deliberate, reviewed act -- never "forward everything".
///
/// - `WSLENV`: tells WSL which Windows env vars to project into the
///   Linux side; needed for the WSL cleanup bridge to see operator vars.
/// - `TC_WSL_DISTRO`: operator's chosen WSL distro override.
///
/// NOTE (F1): `TC_SESSION` is deliberately absent — but NOT because this
/// allowlist withholds it. The daemon spawn uses `std::process::Command`, which
/// inherits the FULL parent env (see the spawn site in `ensure_daemon`), so the
/// child receives `TC_SESSION` regardless of this list. This allowlist only
/// controls which keys get a FRESH-READ overlay on top of inheritance. The
/// actual guard against the daemon re-resolving the token is precedence: the
/// parent computes the endpoint ONCE and sets `TC_SOCKET` on the child, and
/// `TC_SOCKET` outranks `TC_SESSION` in `session::resolve_session`. So the
/// daemon binds the given socket and never re-resolves. Adding `TC_SESSION` to
/// the overlay would be pointless (it is already inherited) and would muddy that
/// invariant — do not add it here.
pub const FORWARDED_ENV_ALLOWLIST: &[&str] = &["WSLENV", "TC_WSL_DISTRO"];

/// Build the map of allowlisted host env vars currently set, read fresh
/// from the process environment at call time. Only keys in
/// [`FORWARDED_ENV_ALLOWLIST`] are ever included.
#[must_use]
pub fn build_forward_env() -> BTreeMap<String, String> {
    build_forward_env_with(&crate::paths::ProcessEnv)
}

/// [`build_forward_env`] with an injected env source, so tests can verify
/// allowlist filtering without mutating the process-global env table.
#[must_use]
pub fn build_forward_env_with(env: &impl crate::paths::EnvSource) -> BTreeMap<String, String> {
    FORWARDED_ENV_ALLOWLIST
        .iter()
        .filter_map(|k| env.get(k).map(|v| ((*k).to_owned(), v)))
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Endpoint {
    UnixSocket { path: PathBuf },
    WindowsPipe { name: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostics {
    pub endpoint: Endpoint,
    pub log_path: Option<PathBuf>,
    pub last_error: Option<String>,
    pub startup_attempted: bool,
    pub startup_elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EnsureDaemonStatus {
    AlreadyRunning {
        endpoint: Endpoint,
        pid: Option<u32>,
    },
    Started {
        endpoint: Endpoint,
        pid: Option<u32>,
        log_path: PathBuf,
    },
    Unavailable {
        reason: DaemonUnavailableReason,
        diagnostics: Diagnostics,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonUnavailableReason {
    SpawnFailed,
    StartupTimeout,
    EndpointBindFailed,
    BinaryNotFound,
}

#[derive(Debug, Error)]
pub enum EnsureError {
    #[error("daemon binary not found at {0}")]
    BinaryNotFound(PathBuf),
}

#[derive(Debug, Clone)]
pub struct EnsureDaemonOptions {
    pub daemon_binary: PathBuf,
    pub state_dir: PathBuf,
    pub log_dir: PathBuf,
    pub endpoint: Endpoint,
    pub startup_timeout: Duration,
    pub allow_spawn: bool,
}

/// Probe the endpoint; if reachable, return `AlreadyRunning`. If not
/// and `allow_spawn` is true, spawn the daemon and wait up to
/// `startup_timeout` for the endpoint to bind. On failure, return
/// `Unavailable { reason, diagnostics }` with the log path included
/// so callers can surface it.
///
/// This function must not panic; it must always return a structured
/// status the caller can render to the operator.
pub async fn ensure_daemon(opts: EnsureDaemonOptions) -> EnsureDaemonStatus {
    let start = std::time::Instant::now();

    // 1. Probe endpoint first.
    if probe_endpoint(&opts.endpoint).await {
        return EnsureDaemonStatus::AlreadyRunning {
            endpoint: opts.endpoint,
            pid: None,
        };
    }

    if !opts.allow_spawn {
        return EnsureDaemonStatus::Unavailable {
            reason: DaemonUnavailableReason::EndpointBindFailed,
            diagnostics: Diagnostics {
                endpoint: opts.endpoint,
                log_path: None,
                last_error: Some("endpoint unreachable; spawn disabled".into()),
                startup_attempted: false,
                startup_elapsed_ms: start.elapsed().as_millis() as u64,
            },
        };
    }

    // 2. Spawn daemon. Only fail-fast on BinaryNotFound when the caller
    // gave us an absolute or relative path (something with a separator).
    // A bare name like "terminal-commanderd" is intentionally resolved
    // via PATH at spawn time, so we MUST let Command::spawn try first
    // rather than rejecting on a CWD-only existence check.
    //
    // Note: this branch uses blocking std::fs and std::process::Command
    // inside an async fn. Under tokio's multi-threaded runtime this
    // starves a single worker thread per call, not the whole runtime.
    // Spawn is rare and fast on Windows/Linux so the tradeoff is
    // acceptable for Phase 3. If diagnostics fidelity ever requires
    // capturing per-syscall latency or this is called from a hot
    // path, wrap the blocking section in `tokio::task::spawn_blocking`.
    let binary_has_separator =
        opts.daemon_binary.components().nth(1).is_some() || opts.daemon_binary.is_absolute();
    if binary_has_separator && !opts.daemon_binary.exists() {
        return EnsureDaemonStatus::Unavailable {
            reason: DaemonUnavailableReason::BinaryNotFound,
            diagnostics: Diagnostics {
                endpoint: opts.endpoint,
                log_path: None,
                last_error: Some(format!(
                    "daemon binary not found: {}",
                    opts.daemon_binary.display()
                )),
                startup_attempted: false,
                startup_elapsed_ms: start.elapsed().as_millis() as u64,
            },
        };
    }
    let _ = std::fs::create_dir_all(&opts.log_dir);
    let log_path = opts.log_dir.join("terminal-commanderd.log");
    let log_file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(e) => {
            return EnsureDaemonStatus::Unavailable {
                reason: DaemonUnavailableReason::SpawnFailed,
                diagnostics: Diagnostics {
                    endpoint: opts.endpoint,
                    log_path: Some(log_path),
                    last_error: Some(format!("open log: {e}")),
                    startup_attempted: false,
                    startup_elapsed_ms: start.elapsed().as_millis() as u64,
                },
            };
        }
    };
    let log_file_err = match log_file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            return EnsureDaemonStatus::Unavailable {
                reason: DaemonUnavailableReason::SpawnFailed,
                diagnostics: Diagnostics {
                    endpoint: opts.endpoint,
                    log_path: Some(log_path),
                    last_error: Some(format!("clone log fd: {e}")),
                    startup_attempted: false,
                    startup_elapsed_ms: start.elapsed().as_millis() as u64,
                },
            };
        }
    };
    // Derive the TC_SOCKET env var from the user-selected endpoint so
    // the daemon binds exactly the same path/pipe that the MCP adapter
    // is probing. Without this, the daemon would fall back to its
    // compiled-in default socket path while the supervisor probes the
    // user-specified one, causing every cold-start readiness check to
    // time out.
    let tc_socket_val: std::ffi::OsString = match &opts.endpoint {
        Endpoint::UnixSocket { path } => path.as_os_str().into(),
        Endpoint::WindowsPipe { name } => name.into(),
    };
    let mut cmd = std::process::Command::new(&opts.daemon_binary);
    cmd.arg("--data-dir")
        .arg(&opts.state_dir)
        .arg("start")
        .arg("--mode")
        .arg("ipc-server")
        .env("TC_SOCKET", &tc_socket_val)
        // F6: forward a fixed allowlist of operational (non-secret) host
        // vars, read fresh at spawn, so a `restart` picks up a freshly-set
        // WSLENV / TC_WSL_DISTRO without a full client OS-restart. The
        // child still inherits the rest of the parent env; this only
        // guarantees the allowlisted keys reflect the current process env.
        .envs(build_forward_env())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(log_file_err));
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return EnsureDaemonStatus::Unavailable {
                reason: DaemonUnavailableReason::SpawnFailed,
                diagnostics: Diagnostics {
                    endpoint: opts.endpoint,
                    log_path: Some(log_path),
                    last_error: Some(format!("spawn: {e}")),
                    startup_attempted: true,
                    startup_elapsed_ms: start.elapsed().as_millis() as u64,
                },
            };
        }
    };
    let pid = Some(child.id());
    // `child` is dropped at the end of this function. On both Unix
    // and Windows, dropping std::process::Child does NOT terminate
    // the underlying process — it only releases the handle. That is
    // the intended daemon semantics here: the spawned terminal-
    // commanderd outlives the supervisor call.
    drop(child);

    // 3. Wait for endpoint bind up to startup_timeout.
    let deadline = std::time::Instant::now() + opts.startup_timeout;
    while std::time::Instant::now() < deadline {
        if probe_endpoint(&opts.endpoint).await {
            return EnsureDaemonStatus::Started {
                endpoint: opts.endpoint,
                pid,
                log_path,
            };
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    EnsureDaemonStatus::Unavailable {
        reason: DaemonUnavailableReason::StartupTimeout,
        diagnostics: Diagnostics {
            endpoint: opts.endpoint,
            log_path: Some(log_path),
            last_error: Some(format!(
                "endpoint did not bind within {}ms",
                opts.startup_timeout.as_millis()
            )),
            startup_attempted: true,
            startup_elapsed_ms: start.elapsed().as_millis() as u64,
        },
    }
}

/// Whole-handshake budget for [`probe_endpoint`]. The probe connects,
/// sends one `health` frame, and reads one response frame. A legitimate
/// daemon answers in microseconds; this generous bound only exists so a
/// hung/silent peer (a connectable socket that never replies) makes the
/// probe return `false` instead of blocking the ensure path forever.
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Upper bound on a response frame the probe will read. Mirrors the
/// daemon's `MAX_FRAME_BYTES` (256 KiB, see `docs/runtime/UDS_IPC.md`):
/// a length prefix above this is a non-conforming peer, not our daemon.
const PROBE_MAX_FRAME_BYTES: usize = 256 * 1024;

/// Minimal mirror of the daemon's `ResponseEnvelope` (TC37 wire format)
/// sufficient to recognise a well-formed `health` reply WITHOUT taking a
/// dependency on the daemon crate (which would create a supervisor<->daemon
/// cycle: `terminal-commanderd` already depends on this crate).
///
/// We deliberately decode only what the accept rule needs: the result must
/// be `kind = "ok"` and the inner response must be the `health` variant.
/// `uptime_secs` is required by the variant; `idle_secs` is optional with
/// `#[serde(default)]` so a legacy daemon that omits the field still
/// deserialises as Health — the accept test is "deserialises as Health",
/// never "carries idle_secs".
#[derive(Deserialize)]
struct ProbeResponseEnvelope {
    result: ProbeResult,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProbeResult {
    Ok {
        response: ProbeResponse,
    },
    // Any non-ok envelope (e.g. an `err` reply) is not a daemon we can
    // treat as ready; serde maps unknown/other kinds to a decode error,
    // which the caller turns into `false`.
    #[serde(other)]
    Other,
}

/// Health payload surfaced by a successful probe handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeHealth {
    /// Seconds since the daemon's last real (non-peek) IPC request.
    /// `None` when the daemon answered the LEGACY Health shape (no
    /// idle_secs) — idle is UNKNOWN, NOT "not our daemon".
    pub idle_secs: Option<u64>,
}

#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum ProbeResponse {
    Health {
        // uptime_secs is required by the wire variant but the probe does
        // not surface it; keep it parsed so a Health reply still decodes.
        #[allow(dead_code)]
        uptime_secs: u64,
        #[serde(default)]
        idle_secs: Option<u64>,
    },
    // A well-formed envelope carrying some other method is a daemon
    // answering the wrong question — still "not our health handshake".
    #[serde(other)]
    Other,
}

/// Run the one-shot `health` handshake over an already-connected stream:
/// write a length-prefixed `RequestEnvelope { correlation_id: 0,
/// request: Health }`, read one length-prefixed response frame, and
/// return `Some(ProbeHealth)` only if it deserialises as a well-formed
/// `Health` response (carrying `idle_secs` when present, `None` for the
/// legacy shape). Any I/O error, oversize/short frame, or non-Health
/// payload yields `None`. The whole call is the caller's responsibility to
/// bound with [`PROBE_TIMEOUT`].
async fn health_handshake<S>(mut stream: S) -> Option<ProbeHealth>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Request frame. correlation_id 0 is fine: the probe is one-shot and
    // does not multiplex. Payload matches the TC37 wire schema exactly:
    // {"correlation_id":0,"request":{"method":"health"}}.
    const REQUEST_JSON: &[u8] = br#"{"correlation_id":0,"request":{"method":"health"}}"#;
    let len = u32::try_from(REQUEST_JSON.len()).ok()?;
    stream.write_all(&len.to_be_bytes()).await.ok()?;
    stream.write_all(REQUEST_JSON).await.ok()?;
    stream.flush().await.ok()?;

    // Response frame: 4-byte big-endian length prefix, then the payload.
    let mut len_buf = [0_u8; 4];
    stream.read_exact(&mut len_buf).await.ok()?;
    let resp_len = u32::from_be_bytes(len_buf) as usize;
    if resp_len == 0 || resp_len > PROBE_MAX_FRAME_BYTES {
        return None;
    }
    let mut payload = vec![0_u8; resp_len];
    stream.read_exact(&mut payload).await.ok()?;

    match serde_json::from_slice::<ProbeResponseEnvelope>(&payload) {
        Ok(ProbeResponseEnvelope {
            result:
                ProbeResult::Ok {
                    response: ProbeResponse::Health { idle_secs, .. },
                },
        }) => Some(ProbeHealth { idle_secs }),
        _ => None,
    }
}

/// Probe an endpoint by performing a real `health` handshake, not a bare
/// connect. Returns `true` only when a connectable peer answers with a
/// well-formed `IpcResponse::Health` (legacy daemons without `idle_secs`
/// still count). A pre-bound/stale/non-tc listener that connects but does
/// not speak our protocol returns `false` — closing the impersonation gap
/// where any connectable socket was wrongly accepted as "our daemon".
///
/// The entire handshake is bounded by [`PROBE_TIMEOUT`]; a hung or silent
/// peer returns `false` and never hangs the ensure path.
///
/// This is a thin wrapper over [`probe_endpoint_health`]: a peer "is our
/// daemon" iff the handshake yields a [`ProbeHealth`] payload. Callers that
/// need the parsed `idle_secs` should call [`probe_endpoint_health`] directly.
pub async fn probe_endpoint(endpoint: &Endpoint) -> bool {
    probe_endpoint_health(endpoint).await.is_some()
}

/// Probe an endpoint like [`probe_endpoint`], but surface the parsed Health
/// payload instead of collapsing it to a bool.
///
/// Returns `Some(ProbeHealth)` when a connectable peer answers with a
/// well-formed `IpcResponse::Health` — including the LEGACY shape, in which
/// case `idle_secs` is `None` (alive, idle UNKNOWN). Returns `None` on any
/// I/O error, oversize/short frame, non-Health payload, or timeout — i.e.
/// "not our daemon" or unreachable.
///
/// The entire handshake is bounded by [`PROBE_TIMEOUT`]; a hung or silent
/// peer returns `None` and never hangs the caller.
pub async fn probe_endpoint_health(endpoint: &Endpoint) -> Option<ProbeHealth> {
    let handshake = async {
        match endpoint {
            #[cfg(unix)]
            Endpoint::UnixSocket { path } => match tokio::net::UnixStream::connect(path).await {
                Ok(stream) => health_handshake(stream).await,
                Err(_) => None,
            },
            #[cfg(not(unix))]
            Endpoint::UnixSocket { .. } => None,
            #[cfg(windows)]
            Endpoint::WindowsPipe { name } => {
                // ClientOptions::new().open is synchronous; same tokio
                // contract caveat as the blocking I/O in ensure_daemon
                // step 2 (acceptable for Phase 3, revisit if probed in a
                // hot path). The returned NamedPipeClient implements the
                // tokio async I/O traits, so the same handshake applies.
                use tokio::net::windows::named_pipe::ClientOptions;
                match ClientOptions::new().open(name.as_str()) {
                    Ok(stream) => health_handshake(stream).await,
                    Err(_) => None,
                }
            }
            #[cfg(not(windows))]
            Endpoint::WindowsPipe { .. } => None,
        }
    };

    // Bound EVERYTHING: connect + write + read. A silent peer that never
    // answers must make the probe return None, never hang.
    (tokio::time::timeout(PROBE_TIMEOUT, handshake).await).unwrap_or(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_returns_unavailable() {
        let opts = EnsureDaemonOptions {
            daemon_binary: PathBuf::from("nonexistent"),
            state_dir: PathBuf::from("."),
            log_dir: PathBuf::from("."),
            endpoint: Endpoint::WindowsPipe {
                name: r"\\.\pipe\unused".into(),
            },
            startup_timeout: Duration::from_millis(10),
            allow_spawn: false,
        };
        let status = ensure_daemon(opts).await;
        assert!(matches!(status, EnsureDaemonStatus::Unavailable { .. }));
    }

    #[tokio::test]
    async fn bare_binary_name_does_not_fail_fast_on_missing_check() {
        // Bare name "definitely-not-installed-xyz" — PATH resolution must
        // be left to Command::spawn, which will fail at spawn time, not
        // at the existence-check fast-path.
        let dir = tempfile::TempDir::new().unwrap();
        let opts = EnsureDaemonOptions {
            daemon_binary: PathBuf::from("definitely-not-installed-xyz"),
            state_dir: dir.path().into(),
            log_dir: dir.path().into(),
            endpoint: Endpoint::WindowsPipe {
                name: r"\\.\pipe\unused".into(),
            },
            startup_timeout: Duration::from_millis(10),
            allow_spawn: true,
        };
        let status = ensure_daemon(opts).await;
        match status {
            EnsureDaemonStatus::Unavailable {
                reason,
                diagnostics,
            } => {
                // Reason MUST be SpawnFailed, not BinaryNotFound — proves
                // the existence check did not fast-fail on the bare name.
                assert!(
                    matches!(reason, DaemonUnavailableReason::SpawnFailed),
                    "expected SpawnFailed, got {reason:?}"
                );
                assert!(
                    diagnostics.startup_attempted,
                    "startup must have been attempted (spawn was called)"
                );
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn forward_env_allowlist_is_operational_non_secret() {
        // The allowlist must never contain a secret/password-shaped key.
        // F6 explicitly rejects forwarding any credential into the daemon
        // environment (the WSL_SUDO_CREDENTIAL route is forbidden).
        for k in FORWARDED_ENV_ALLOWLIST {
            let lk = k.to_ascii_lowercase();
            assert!(
                !lk.contains("secret")
                    && !lk.contains("password")
                    && !lk.contains("credential")
                    && !lk.contains("token")
                    && !lk.contains("key"),
                "allowlist must be operational-only; '{k}' looks secret"
            );
        }
        assert!(FORWARDED_ENV_ALLOWLIST.contains(&"WSLENV"));
    }

    #[test]
    fn build_forward_env_forwards_only_allowlisted_vars() {
        // In-memory env: no process-global mutation, so this runs race-free
        // alongside every other test regardless of --test-threads.
        struct FakeEnv(std::collections::HashMap<String, String>);
        impl crate::paths::EnvSource for FakeEnv {
            fn get(&self, key: &str) -> Option<String> {
                self.0.get(key).cloned()
            }
        }
        let secret = "TC_F6_TEST_SECRET_THING";
        let mut map = std::collections::HashMap::new();
        map.insert("WSLENV".to_owned(), "TC_F6/u".to_owned());
        map.insert(secret.to_owned(), "nope".to_owned());

        let env = build_forward_env_with(&FakeEnv(map));
        assert_eq!(env.get("WSLENV").map(String::as_str), Some("TC_F6/u"));
        assert!(
            !env.contains_key(secret),
            "non-allowlisted var must not be forwarded"
        );
    }
}
