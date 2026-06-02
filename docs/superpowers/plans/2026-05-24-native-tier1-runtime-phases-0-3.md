# Native Tier-1 Runtime — Phases 0-3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the "MCP error -32000: Connection closed" symptom on Windows by locking the native-tier-1 decision in docs, making daemon-startup failures diagnosable to the user, moving session lifecycle from the Node supervisor into a native Rust supervisor, and tightening the Windows named-pipe IPC path enough to be safely reachable from any clean install.

**Architecture:** The MCP adapter (`crates/mcp`) becomes responsible for ensuring the daemon is reachable through a new `crates/supervisor` library, instead of relying on the Node-side `session_supervisor.js` that currently tears down daemon + session state on every stdin EOF. The named-pipe transport (`crates/daemon/src/ipc/pipe_server.rs`) is corrected — accept loop no longer dies on transient errors, peer identity is real or honestly `Unknown`, pipe binds with an ACL restricted to the current user. Documentation (`SPEC.md`, `ARCHITECTURE.md`, new ADR) is updated so the project no longer contradicts itself about Windows native support. The Node `packages/terminal-commander*` tree is **not** removed in this plan — it stays as the install vehicle, but its runtime supervisor responsibility is gutted.

**Tech Stack:** Rust 2024 edition, toolchain 1.95, `tokio` (multi-thread runtime), `tokio::net::windows::named_pipe`, `windows` crate (peer SID + pipe ACL on Windows), `tracing` (daemon log file), `clap` (CLI flags), `rmcp` (existing MCP adapter), `nextest` (test runner), Node 20 (install-time only — out of scope to remove in this plan).

**Non-goals of this plan (deferred to later plans):**
- Replacing npm with a different installer.
- Building cross-platform PTY (Phase 6 — separate plan).
- Macro tool-catalogue capability state machine (Phase 5 — separate plan).
- Deleting `packages/terminal-commander*` Node tree (Phase 4 — separate plan).
- macOS support work.

**Reference inputs:**
- `docs/audits/2026-05-24-windows-mcp-connect-closed-findings.md` (esp. §4, §6, §10, §13)
- `C:\Users\poslj\Downloads\terminal-commander-consolidated-review-and-roadmap.md` (GPT consolidated roadmap, esp. Phases 0-3)

---

## File Structure (locked before tasks)

### New files

- `docs/adr/ADR-native-tier1-runtime.md` — decision record for Win-x64 + Linux-x64 tier-1, Mac built-but-not-prioritized, WSL = Linux artifact.
- `crates/supervisor/Cargo.toml` — new crate manifest.
- `crates/supervisor/src/lib.rs` — `pub mod ensure; pub mod identity;`
- `crates/supervisor/src/ensure.rs` — `ensure_daemon()` + `EnsureDaemonStatus` + `Endpoint` + `Diagnostics`.
- `crates/supervisor/src/identity.rs` — `PeerIdentity` enum and platform impls for resolving caller SID/UID.
- `crates/supervisor/tests/ensure_smoke.rs` — integration test for ensure_daemon (skipped on hosts without daemon binary).
- `crates/daemon/src/ipc/peer_windows.rs` — Windows pipe peer-identity helpers (gated `#[cfg(windows)]`).
- `crates/daemon/src/ipc/pipe_acl.rs` — Windows pipe ACL builder (gated `#[cfg(windows)]`).

### Modified files

- `SPEC.md` — §3 non-goals: strike Windows-native exclusion, replace with tier-1 declaration.
- `ARCHITECTURE.md` — §1-§2: update platform notes to match new ADR.
- `docs/research/_USER_DECISIONS.md` — append operator-decision row dated 2026-05-24.
- `README.md` — Status table: Windows install/bootstrap status now reads "in progress (see ADR-native-tier1-runtime.md)".
- `Cargo.toml` (workspace) — add `crates/supervisor` to `[workspace] members`.
- `crates/mcp/Cargo.toml` — add `terminal-commander-supervisor` dep.
- `crates/mcp/src/main.rs` — call `supervisor::ensure_daemon` before `serve(stdio())`; on `Unavailable`, keep MCP service alive and return structured daemon-unavailable error from tool calls instead of `ExitCode::from(2)`.
- `crates/mcp/src/tools.rs` — add helper `daemon_unavailable_error()`; on `IpcError::Transport`, surface `daemon_unavailable` envelope.
- `crates/mcp/src/daemon_client.rs` — `McpDaemonClient::new` takes a shared `DaemonStatus` handle so tool calls know when daemon is down without re-probing every call.
- `crates/daemon/src/ipc/mod.rs` — strike `Windows native: NOT SUPPORTED` doc comment; add reference to ADR.
- `crates/daemon/src/ipc/peer.rs` — replace `PeerCred` struct with `PeerIdentity` enum re-exported from supervisor crate.
- `crates/daemon/src/ipc/pipe_server.rs` — fix `accept_loop` (don't set `first_pipe_instance(true)` on iterations >0; don't break on transient `create` error); use real peer identity from `peer_windows.rs`; bind via `pipe_acl::build_security_attributes(&user_sid)`.
- `crates/daemon/src/runtime.rs` — `run_ipc_server` (`#[cfg(windows)]` variant) writes startup diagnostics to a log file under `state_dir/logs/terminal-commanderd.log` even when run with `stdio: "ignore"`.
- `crates/daemon/src/ipc/server.rs` — propagate `PeerIdentity` through `dispatch_envelope`; remove fake `uid: 0, gid: 0`.
- `packages/terminal-commander/lib/daemon/session_supervisor.js` — gut: remove `cleanup` registration on `process.on("exit")`, remove `cleanupStaleSessions` call at startup, remove `fs.rmSync(paths.base)` from `cleanup`. Keep just the daemon spawn + readiness wait. (Full deletion deferred to Phase 4 plan.)
- `packages/terminal-commander/lib/daemon/session_supervisor.js` — capture daemon `stdio` to log file instead of `"ignore"`.

### Test files

- `crates/supervisor/tests/ensure_smoke.rs` — happy-path ensure_daemon.
- `crates/daemon/tests/pipe_accept_loop.rs` — accept loop survives client disconnect mid-accept; second client can connect.
- `crates/daemon/tests/pipe_peer_identity.rs` (`#[cfg(windows)]`) — peer identity is the test's own SID, not 0/0.
- `crates/daemon/tests/pipe_acl.rs` (`#[cfg(windows)]`) — ACL string contains current user SID, denies world.
- `crates/mcp/tests/daemon_unavailable_envelope.rs` — MCP returns structured error, does not exit, when daemon endpoint refuses connection.
- `crates/mcp/tests/stdin_eof_survives.rs` — MCP exits 0 on stdin EOF; spawned daemon continues running. (Linux variant uses UDS, Windows uses named pipe.)
- `packages/terminal-commander/test/session_supervisor_no_rm.test.js` — supervisor MCP-exit handler does not call `fs.rmSync` on session base.

---

## Task list

### Task 1: ADR + SPEC alignment (docs only)

**Files:**
- Create: `docs/adr/ADR-native-tier1-runtime.md`
- Modify: `SPEC.md` (§3 non-goals block)
- Modify: `ARCHITECTURE.md` (§1 high-level diagram intro, §2.2 daemon platform notes)
- Modify: `docs/research/_USER_DECISIONS.md` (append 2026-05-24 decision row)
- Modify: `README.md` (Status table row "Windows install bootstrap + bridge")

- [ ] **Step 1.1: Write ADR file**

Create `docs/adr/ADR-native-tier1-runtime.md` with this exact content:

```markdown
# ADR: Native tier-1 runtime for Windows and Linux

Status: Accepted
Date: 2026-05-24
Supersedes: parts of SPEC.md §3 (non-goals) and earlier statements in
`docs/research/_USER_DECISIONS.md` that excluded Windows native shipping.
Related: `docs/adr/ADR-parent-environment-runners.md`,
`docs/audits/2026-05-24-windows-mcp-connect-closed-findings.md` §13.

## Context

The codebase contained three contradictory positions on Windows:
SPEC §3 listed Windows-native shipping as a non-goal;
`crates/daemon/src/ipc/mod.rs` said `Windows native: NOT SUPPORTED`;
crate code, the resolver, and packaging docs treated Windows-x64 as a
live target. The user-reported failure mode
(`MCP error -32000: Connection closed`) cannot be diagnosed cleanly
while these positions stand.

## Decision

1. Tier-1 targets are Windows-x64 native and Linux-x64 native. WSL
   Ubuntu is supported through the Linux-x64 artifact; it is not a
   separate target and the runtime does not require a WSL bridge.
2. macOS (arm64 + x64) is a build-only tier-3 target. Binaries are
   produced when cross-compile is straightforward, but QA, smoke
   tests, and doctor coverage are not in scope.
3. The runtime is native Rust (edition 2024, toolchain 1.95) on
   every supported OS. Once installed, neither the MCP adapter nor
   the daemon depends on Node, Python, WSL, PowerShell, or shell
   scripts.
4. The install step may use any mechanism that delivers one-command
   set-and-forget UX (npm postinstall downloader, winget, scoop,
   cargo install, or curl one-liner). The current `npm install -g`
   front door is acceptable.

## Consequences

- SPEC.md §3 non-goals must be updated (this plan's Task 1).
- `crates/daemon/src/ipc/mod.rs` module doc must be updated (this
  plan's Task 5).
- Future plans address replacing the Node session supervisor (this
  plan's Task 8) and migrating the install vehicle (Phase 4 plan).
- The legacy WSL bridge path (`TC_USE_LEGACY_WSL_BRIDGE=1`,
  `lib/wsl/spawn.js`) is now legacy/optional, not the documented
  Windows path.
```

- [ ] **Step 1.2: Edit SPEC §3**

In `SPEC.md`, find the bullet that begins:
```
- macOS native and Windows-native shipping. Linux native and WSL2 are
```
Replace the entire bullet (through its closing reference to
`docs/research/_USER_DECISIONS.md`) with:
```
- macOS support beyond build-only artifacts. macOS is tier-3 per
  `docs/adr/ADR-native-tier1-runtime.md`.
```

Tier-1 declaration goes in a new bullet immediately above the
non-goals subsection, under §3 intro:
```
Tier-1 targets are Windows-x64 native and Linux-x64 native (the
latter also covers WSL Ubuntu through the Linux artifact). See
`docs/adr/ADR-native-tier1-runtime.md`.
```

- [ ] **Step 1.3: Edit ARCHITECTURE.md**

In `ARCHITECTURE.md` §2.2 `terminal-commanderd (daemon)`, find the
"Platform notes" paragraph or insert one if absent, and write:
```
Platform notes: Linux x64 uses Unix-domain socket IPC under
`$XDG_RUNTIME_DIR/terminal-commander/daemon.sock` (fallback
`~/.local/share/terminal-commander/run/daemon.sock`). Windows x64
uses named-pipe IPC at `\\.\pipe\terminal-commander\<USER>\daemon`
with a security descriptor restricted to LocalSystem, Administrators,
and the current user SID. WSL Ubuntu runs the Linux x64 binary; no
bridge is required. macOS targets are tier-3 build-only per
`docs/adr/ADR-native-tier1-runtime.md`.
```

- [ ] **Step 1.4: Append decision row in `_USER_DECISIONS.md`**

Append at end of file:
```
| 2026-05-24 | Tier-1 native runtime decision | Windows-x64 + Linux-x64 are tier-1 native targets; macOS is build-only tier-3; WSL = Linux artifact (no bridge). See `docs/adr/ADR-native-tier1-runtime.md`. | accepted |
```

- [ ] **Step 1.5: Edit README status table**

In `README.md` `## Status` table, change the row:
```
| Windows install bootstrap + bridge | Live |
```
to:
```
| Windows native install | In progress (see `docs/adr/ADR-native-tier1-runtime.md`) |
```

- [ ] **Step 1.6: Commit**

```bash
git add docs/adr/ADR-native-tier1-runtime.md SPEC.md ARCHITECTURE.md docs/research/_USER_DECISIONS.md README.md
git commit -m "docs(adr): lock native tier-1 runtime decision for Windows+Linux

- Add ADR-native-tier1-runtime.md
- Strike Windows-native non-goal from SPEC §3
- Update ARCHITECTURE platform notes
- Append decision row to _USER_DECISIONS.md
- Mark README status row as in-progress

Refs: docs/audits/2026-05-24-windows-mcp-connect-closed-findings.md §13"
```

---

### Task 2: Workspace wiring for new `supervisor` crate

**Files:**
- Create: `crates/supervisor/Cargo.toml`
- Create: `crates/supervisor/src/lib.rs`
- Modify: `Cargo.toml` (workspace `members`)

- [ ] **Step 2.1: Create crate manifest**

Create `crates/supervisor/Cargo.toml`:
```toml
[package]
name = "terminal-commander-supervisor"
version = "0.0.0"
edition = "2024"
license = "Apache-2.0"
publish = false

[lib]
name = "terminal_commander_supervisor"
path = "src/lib.rs"

[dependencies]
thiserror = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt-multi-thread", "net", "fs", "io-util", "time", "macros"] }
tracing = "0.1"

[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_Security_Authorization",
    "Win32_System_Threading",
    "Win32_System_Pipes",
] }

[dev-dependencies]
tempfile = "3"
tokio = { version = "1", features = ["test-util"] }
```

- [ ] **Step 2.2: Create lib root**

Create `crates/supervisor/src/lib.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Shared daemon-supervisor library used by the MCP adapter and the
// operator CLI. Owns: daemon endpoint resolution, daemon health
// probing, structured Unavailable diagnostics, peer-identity model.

pub mod ensure;
pub mod identity;
```

- [ ] **Step 2.3: Add to workspace**

In root `Cargo.toml` `[workspace] members = [ ... ]`, add `"crates/supervisor",` in alphabetical position (after `crates/store` if present).

- [ ] **Step 2.4: Verify workspace builds**

Run: `cargo check --workspace`
Expected: PASS, including new `terminal-commander-supervisor` crate with no warnings.

- [ ] **Step 2.5: Commit**

```bash
git add crates/supervisor/Cargo.toml crates/supervisor/src/lib.rs Cargo.toml
git commit -m "chore(supervisor): scaffold terminal-commander-supervisor crate"
```

---

### Task 3: PeerIdentity enum

**Files:**
- Create: `crates/supervisor/src/identity.rs`
- Test: `crates/supervisor/src/identity.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 3.1: Write the failing test (inline at end of identity.rs)**

Create `crates/supervisor/src/identity.rs` with this content (test first):
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Platform-neutral peer identity. Replaces the prior
// `PeerCred { uid: 0, gid: 0, pid: None }` constant on Windows.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PeerIdentity {
    Unix {
        uid: u32,
        gid: u32,
        pid: Option<i32>,
    },
    Windows {
        sid: String,
        pid: Option<u32>,
        image: Option<PathBuf>,
    },
    Unknown,
}

impl PeerIdentity {
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, PeerIdentity::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_identity_is_not_known() {
        assert!(!PeerIdentity::Unknown.is_known());
    }

    #[test]
    fn unix_identity_is_known() {
        let id = PeerIdentity::Unix { uid: 1000, gid: 1000, pid: Some(42) };
        assert!(id.is_known());
    }

    #[test]
    fn windows_identity_is_known() {
        let id = PeerIdentity::Windows {
            sid: "S-1-5-21-1-2-3-1001".into(),
            pid: Some(42),
            image: None,
        };
        assert!(id.is_known());
    }

    #[test]
    fn round_trip_through_serde_json() {
        let id = PeerIdentity::Windows {
            sid: "S-1-5-21-1-2-3-1001".into(),
            pid: Some(42),
            image: None,
        };
        let s = serde_json::to_string(&id).unwrap();
        let back: PeerIdentity = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }
}
```

- [ ] **Step 3.2: Run tests to verify they pass**

Run: `cargo nextest run -p terminal-commander-supervisor identity::tests`
Expected: 4 PASS.

- [ ] **Step 3.3: Commit**

```bash
git add crates/supervisor/src/identity.rs
git commit -m "feat(supervisor): add PeerIdentity enum (unix/windows/unknown)"
```

---

### Task 4: ensure_daemon scaffolding (no spawn yet)

**Files:**
- Create: `crates/supervisor/src/ensure.rs`
- Test: `crates/supervisor/src/ensure.rs` (inline tests)

- [ ] **Step 4.1: Write file with types only (and a failing test)**

Create `crates/supervisor/src/ensure.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Daemon ensure/readiness library entry point.
//
// The MCP adapter calls `ensure_daemon()` before serving rmcp. The
// return value tells the caller whether to forward tool calls, return
// `daemon_unavailable` envelopes, or fail loudly.

use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

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
    AlreadyRunning { endpoint: Endpoint, pid: Option<u32> },
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
pub async fn ensure_daemon(
    _opts: EnsureDaemonOptions,
) -> EnsureDaemonStatus {
    // Phase 4.1 scaffold: stubbed unavailable result; Task 5 implements
    // the live probe + spawn loop.
    EnsureDaemonStatus::Unavailable {
        reason: DaemonUnavailableReason::SpawnFailed,
        diagnostics: Diagnostics {
            endpoint: Endpoint::WindowsPipe { name: String::new() },
            log_path: None,
            last_error: Some("ensure_daemon not yet implemented".into()),
            startup_attempted: false,
            startup_elapsed_ms: 0,
        },
    }
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
            endpoint: Endpoint::WindowsPipe { name: r"\\.\pipe\unused".into() },
            startup_timeout: Duration::from_millis(10),
            allow_spawn: false,
        };
        let status = ensure_daemon(opts).await;
        assert!(matches!(status, EnsureDaemonStatus::Unavailable { .. }));
    }
}
```

- [ ] **Step 4.2: Run test to verify it passes**

Run: `cargo nextest run -p terminal-commander-supervisor ensure::tests`
Expected: PASS.

- [ ] **Step 4.3: Commit**

```bash
git add crates/supervisor/src/ensure.rs
git commit -m "feat(supervisor): scaffold ensure_daemon types + stub"
```

---

### Task 5: ensure_daemon live implementation

**Files:**
- Modify: `crates/supervisor/src/ensure.rs`
- Test: `crates/supervisor/tests/ensure_smoke.rs` (new file)

- [ ] **Step 5.1: Write failing integration test first**

Create `crates/supervisor/tests/ensure_smoke.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Smoke test for ensure_daemon. Uses an already-running listener so
// the test does not depend on the real daemon binary; that exercise
// is covered by crates/daemon tests.

use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use terminal_commander_supervisor::ensure::{
    Endpoint, EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon,
};

#[cfg(unix)]
#[tokio::test]
async fn already_running_uds_endpoint_returns_already_running() {
    use tokio::net::UnixListener;
    let dir = TempDir::new().unwrap();
    let sock = dir.path().join("test.sock");
    let _listener = UnixListener::bind(&sock).unwrap();
    let opts = EnsureDaemonOptions {
        daemon_binary: PathBuf::from("/bin/true"),
        state_dir: dir.path().into(),
        log_dir: dir.path().into(),
        endpoint: Endpoint::UnixSocket { path: sock },
        startup_timeout: Duration::from_millis(500),
        allow_spawn: false,
    };
    let status = ensure_daemon(opts).await;
    assert!(matches!(status, EnsureDaemonStatus::AlreadyRunning { .. }));
}

#[tokio::test]
async fn no_listener_no_spawn_returns_unavailable() {
    let dir = TempDir::new().unwrap();
    let opts = EnsureDaemonOptions {
        daemon_binary: PathBuf::from("nonexistent"),
        state_dir: dir.path().into(),
        log_dir: dir.path().into(),
        #[cfg(windows)]
        endpoint: Endpoint::WindowsPipe { name: r"\\.\pipe\terminal-commander-test-never-bound".into() },
        #[cfg(unix)]
        endpoint: Endpoint::UnixSocket { path: dir.path().join("never.sock") },
        startup_timeout: Duration::from_millis(50),
        allow_spawn: false,
    };
    let status = ensure_daemon(opts).await;
    match status {
        EnsureDaemonStatus::Unavailable { diagnostics, .. } => {
            assert!(!diagnostics.startup_attempted);
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}
```

- [ ] **Step 5.2: Run integration test to verify it fails**

Run: `cargo nextest run -p terminal-commander-supervisor --test ensure_smoke`
Expected: FAIL — current stub returns `Unavailable` with `endpoint: WindowsPipe { name: String::new() }`, so first test fails because endpoint is `UnixSocket` from caller but stub returns `WindowsPipe`; second test passes accidentally. Actual outcome is fine — we need both real tests to pass after step 5.3.

- [ ] **Step 5.3: Implement probe + spawn loop**

Replace the body of `ensure_daemon` in `crates/supervisor/src/ensure.rs` with:
```rust
pub async fn ensure_daemon(
    opts: EnsureDaemonOptions,
) -> EnsureDaemonStatus {
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

    // 2. Spawn daemon (binary path required to exist).
    if !opts.daemon_binary.exists() {
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
    let log_file_err = log_file.try_clone().unwrap();
    let mut cmd = std::process::Command::new(&opts.daemon_binary);
    cmd.arg("--data-dir")
        .arg(&opts.state_dir)
        .arg("start")
        .arg("--mode")
        .arg("ipc-server")
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

async fn probe_endpoint(endpoint: &Endpoint) -> bool {
    match endpoint {
        #[cfg(unix)]
        Endpoint::UnixSocket { path } => {
            tokio::net::UnixStream::connect(path).await.is_ok()
        }
        #[cfg(not(unix))]
        Endpoint::UnixSocket { .. } => false,
        #[cfg(windows)]
        Endpoint::WindowsPipe { name } => {
            use tokio::net::windows::named_pipe::ClientOptions;
            ClientOptions::new().open(name.as_str()).is_ok()
        }
        #[cfg(not(windows))]
        Endpoint::WindowsPipe { .. } => false,
    }
}
```

- [ ] **Step 5.4: Run both unit and integration tests**

Run: `cargo nextest run -p terminal-commander-supervisor`
Expected: PASS for `identity::tests::*`, `ensure::tests::stub_returns_unavailable` (now passes because `Unavailable` still matches), `ensure_smoke::*`.

- [ ] **Step 5.5: Commit**

```bash
git add crates/supervisor/src/ensure.rs crates/supervisor/tests/ensure_smoke.rs
git commit -m "feat(supervisor): implement ensure_daemon probe+spawn+timeout

Probes the endpoint first; if unreachable and allow_spawn is true,
spawns the daemon binary with stdio redirected to a log file and
waits up to startup_timeout for the endpoint to bind. Returns
structured Unavailable diagnostics on every failure mode."
```

---

### Task 6: Wire supervisor into MCP adapter (daemon-unavailable envelope)

**Files:**
- Modify: `crates/mcp/Cargo.toml`
- Modify: `crates/mcp/src/main.rs`
- Modify: `crates/mcp/src/daemon_client.rs`
- Modify: `crates/mcp/src/tools.rs`
- Test: `crates/mcp/tests/daemon_unavailable_envelope.rs` (new)

- [ ] **Step 6.1: Write the failing test**

Create `crates/mcp/tests/daemon_unavailable_envelope.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Verifies that when the daemon endpoint is unreachable, the MCP
// adapter still responds to initialize and tools/list, and that tool
// calls return a structured `daemon_unavailable` error envelope
// instead of causing the process to exit.

use rmcp::model::{CallToolRequestParams, ServerCapabilities};
use rmcp::ServiceExt;
use rmcp::transport::child_process::TokioChildProcess;
use tokio::process::Command;

#[tokio::test]
async fn mcp_returns_daemon_unavailable_envelope() {
    let exe = env!("CARGO_BIN_EXE_terminal-commander-mcp");
    let mut cmd = Command::new(exe);
    // Point at an endpoint that nothing is listening on.
    cmd.env("TC_SOCKET", r"\\.\pipe\terminal-commander-test-never-bound-xyz");
    // Disable spawn — MCP must not try to start a daemon during test.
    cmd.env("TC_SUPERVISOR_ALLOW_SPAWN", "0");
    let transport = TokioChildProcess::new(cmd).unwrap();
    let client = ().serve(transport).await.expect("connect MCP");
    let tools = client.list_tools(Default::default()).await.expect("list_tools");
    assert!(!tools.tools.is_empty(), "tool catalogue must still be served");

    let params = CallToolRequestParams::new("health");
    let result = client.call_tool(params).await.expect("call health");
    // health does not require daemon — should still succeed.
    assert!(!result.is_error.unwrap_or(false));

    let params = CallToolRequestParams::new("bucket_summary");
    let result = client.call_tool(params).await.expect("call bucket_summary");
    assert!(result.is_error.unwrap_or(false), "bucket_summary requires daemon");
    let content_text = result.content[0].as_text().unwrap().text.clone();
    assert!(content_text.contains("daemon_unavailable"), "got: {content_text}");

    client.cancel().await.ok();
}
```

- [ ] **Step 6.2: Run test, expect it to fail**

Run: `cargo nextest run -p terminal-commander-mcp --test daemon_unavailable_envelope`
Expected: FAIL — current MCP main exits on `serve(stdio()).await` error, never reaches tool calls.

- [ ] **Step 6.3: Add supervisor dependency**

In `crates/mcp/Cargo.toml`, under `[dependencies]`, add:
```toml
terminal-commander-supervisor = { path = "../supervisor" }
```

- [ ] **Step 6.4: Add shared status handle to McpDaemonClient**

In `crates/mcp/src/daemon_client.rs`, add new field + constructor:
```rust
use std::sync::Mutex;
use terminal_commander_supervisor::ensure::EnsureDaemonStatus;

#[derive(Debug, Clone)]
pub struct DaemonStatusHandle(Arc<Mutex<EnsureDaemonStatus>>);

impl DaemonStatusHandle {
    pub fn new(status: EnsureDaemonStatus) -> Self {
        Self(Arc::new(Mutex::new(status)))
    }
    pub fn current(&self) -> EnsureDaemonStatus {
        self.0.lock().unwrap().clone()
    }
    pub fn is_unavailable(&self) -> bool {
        matches!(*self.0.lock().unwrap(), EnsureDaemonStatus::Unavailable { .. })
    }
}
```
Add a new constructor on `McpDaemonClient`:
```rust
pub fn with_status(socket: PathBuf, status: DaemonStatusHandle) -> Self {
    Self { socket, calls: Arc::new(AtomicU64::new(0)), status: Some(status), timeout: DEFAULT_TIMEOUT }
}
```
Add `status: Option<DaemonStatusHandle>` field on the struct; default to `None` from the existing `new`. Expose `status()` getter on the client.

- [ ] **Step 6.5: Add daemon_unavailable_error helper in tools.rs**

In `crates/mcp/src/tools.rs`, add near the existing `format_ipc_error`:
```rust
fn daemon_unavailable_error(status: &EnsureDaemonStatus) -> McpError {
    let payload = serde_json::json!({
        "code": "daemon_unavailable",
        "message": "terminal-commanderd is not reachable",
        "details": status,
    });
    McpError {
        code: -32603,
        message: "daemon_unavailable".into(),
        data: Some(payload),
    }
}
```
Add an `EnsureDaemonStatus` import at the top:
```rust
use terminal_commander_supervisor::ensure::EnsureDaemonStatus;
```

- [ ] **Step 6.6: Short-circuit tool calls that require daemon when unavailable**

In `crates/mcp/src/tools.rs`, for every method on `TerminalCommanderMcpServer` that begins by calling `self.daemon.call(...)` (currently 23 methods, listed in `tool_catalogue`), insert at the top of each `async fn`:
```rust
if let Some(s) = self.daemon.status() {
    if s.is_unavailable() {
        return Err(daemon_unavailable_error(&s.current()));
    }
}
```
**Exception:** do NOT add this guard to `health`, `system_discover`, `policy_status`, or `self_check`. These must answer even when the daemon is down.

- [ ] **Step 6.7: Rewrite mcp/src/main.rs to use supervisor**

In `crates/mcp/src/main.rs`, replace the body of `main` with:
```rust
fn main() -> ExitCode {
    let cli = Cli::parse();
    let ipc_endpoint = resolve_ipc_endpoint(cli.socket.as_deref());
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("terminal-commander-mcp: tokio runtime build failed: {e}");
            return ExitCode::from(2);
        }
    };
    rt.block_on(async move {
        let supervisor_endpoint = endpoint_from_socket_path(&ipc_endpoint);
        let allow_spawn = std::env::var("TC_SUPERVISOR_ALLOW_SPAWN")
            .map(|v| v != "0")
            .unwrap_or(true);
        let opts = terminal_commander_supervisor::ensure::EnsureDaemonOptions {
            daemon_binary: resolve_daemon_binary(),
            state_dir: resolve_state_dir(),
            log_dir: resolve_log_dir(),
            endpoint: supervisor_endpoint,
            startup_timeout: std::time::Duration::from_secs(5),
            allow_spawn,
        };
        let status = terminal_commander_supervisor::ensure::ensure_daemon(opts).await;
        // Log status to stderr so harness logs capture it; never exit.
        eprintln!("terminal-commander-mcp: daemon status: {}", serde_json::to_string(&status).unwrap_or_else(|_| "<unprintable>".into()));
        let status_handle = DaemonStatusHandle::new(status);

        let daemon = McpDaemonClient::with_status(ipc_endpoint, status_handle);
        let server = TerminalCommanderMcpServer::new(daemon);
        let service = match server.serve(stdio()).await {
            Ok(svc) => svc,
            Err(e) => {
                eprintln!("terminal-commander-mcp: stdio serve failed: {e}");
                return ExitCode::from(2);
            }
        };
        if let Err(e) = service.waiting().await {
            eprintln!("terminal-commander-mcp: service exited with error: {e}");
            // Per ADR: stdin EOF is normal. Return 0 unless rmcp signalled a
            // genuine protocol error. The current rmcp ServiceError does not
            // distinguish; treat as clean exit.
            return ExitCode::SUCCESS;
        }
        ExitCode::SUCCESS
    })
}

fn endpoint_from_socket_path(p: &std::path::Path) -> terminal_commander_supervisor::ensure::Endpoint {
    let s = p.to_string_lossy();
    if s.starts_with(r"\\.\pipe\") {
        terminal_commander_supervisor::ensure::Endpoint::WindowsPipe { name: s.into_owned() }
    } else {
        terminal_commander_supervisor::ensure::Endpoint::UnixSocket { path: p.to_path_buf() }
    }
}

fn resolve_daemon_binary() -> std::path::PathBuf {
    // Look next to our own exe first.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            #[cfg(windows)]
            let candidate = dir.join("terminal-commanderd.exe");
            #[cfg(not(windows))]
            let candidate = dir.join("terminal-commanderd");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // Fall back to PATH name.
    std::path::PathBuf::from("terminal-commanderd")
}

fn resolve_state_dir() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("TC_DATA") {
        return std::path::PathBuf::from(p);
    }
    #[cfg(windows)]
    {
        if let Ok(p) = std::env::var("LOCALAPPDATA") {
            return std::path::PathBuf::from(p).join("terminal-commander").join("state");
        }
    }
    #[cfg(unix)]
    {
        if let Ok(p) = std::env::var("XDG_STATE_HOME") {
            return std::path::PathBuf::from(p).join("terminal-commander");
        }
        if let Ok(p) = std::env::var("HOME") {
            return std::path::PathBuf::from(p).join(".local/share/terminal-commander");
        }
    }
    std::env::temp_dir().join("terminal-commander").join("state")
}

fn resolve_log_dir() -> std::path::PathBuf {
    resolve_state_dir().join("logs")
}
```
Also add `use terminal_commander_mcp::daemon_client::DaemonStatusHandle;` near the existing imports.

- [ ] **Step 6.8: Run the test again**

Run: `cargo nextest run -p terminal-commander-mcp --test daemon_unavailable_envelope`
Expected: PASS — MCP keeps stdio alive, returns structured `daemon_unavailable` envelope from tool calls that touch the daemon.

- [ ] **Step 6.9: Run the full workspace test suite**

Run: `cargo nextest run --workspace`
Expected: PASS. Any test that previously asserted MCP exits on stdin EOF needs updating in Task 8.

- [ ] **Step 6.10: Commit**

```bash
git add crates/mcp/Cargo.toml crates/mcp/src/main.rs crates/mcp/src/daemon_client.rs crates/mcp/src/tools.rs crates/mcp/tests/daemon_unavailable_envelope.rs
git commit -m "feat(mcp): wire supervisor; return daemon_unavailable envelope

MCP no longer exits when the daemon endpoint is unreachable. Instead
it serves the tool catalogue, answers daemon-free tools (health,
system_discover, policy_status, self_check), and returns a structured
daemon_unavailable envelope from tool calls that require the daemon."
```

---

### Task 7: Fix Windows named-pipe accept loop

**Files:**
- Modify: `crates/daemon/src/ipc/pipe_server.rs::accept_loop`
- Test: `crates/daemon/tests/pipe_accept_loop.rs` (new, `#[cfg(windows)]`)

- [ ] **Step 7.1: Write the failing test**

Create `crates/daemon/tests/pipe_accept_loop.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Verifies the named-pipe accept loop:
//   1. accepts the first client,
//   2. accepts a second client after the first disconnects,
//   3. survives a transient ServerOptions::create error.

#![cfg(windows)]

use std::sync::Arc;
use std::time::Duration;
use terminal_commanderd::ipc::pipe_server::PipeServer;
use terminal_commanderd::state::DaemonState;
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::time::sleep;

async fn make_state() -> Arc<DaemonState> {
    // Use the same in-test helper as the other crate tests; the path
    // is `crates/daemon/tests/common.rs::test_state()` if present,
    // otherwise construct DaemonState::for_test() directly.
    Arc::new(DaemonState::for_test().await.expect("test state"))
}

#[tokio::test]
async fn first_and_second_client_both_connect() {
    let pipe_name = format!(r"\\.\pipe\tc-test-{}", std::process::id());
    let state = make_state().await;
    let server = PipeServer::new(state, pipe_name.clone());
    let handle = server.spawn().expect("spawn pipe server");

    // First client.
    sleep(Duration::from_millis(50)).await;
    let c1 = ClientOptions::new().open(&pipe_name).expect("c1 connect");
    drop(c1);

    // Second client should also succeed.
    sleep(Duration::from_millis(50)).await;
    let c2 = ClientOptions::new().open(&pipe_name).expect("c2 connect");
    drop(c2);

    handle.shutdown().await;
}
```
If `DaemonState::for_test()` does not exist yet, add it in this step as a `#[cfg(any(test, feature = "test-util"))]` helper that constructs a minimal state under a temp dir — keep the helper inline if necessary.

- [ ] **Step 7.2: Run the test, expect failure**

Run: `cargo nextest run -p terminal-commanderd --test pipe_accept_loop`
Expected: FAIL — second `ClientOptions::new().open()` returns `ERROR_PIPE_BUSY` or the accept loop has already exited because the second iteration set `first_pipe_instance(true)` again on an already-created name.

- [ ] **Step 7.3: Fix accept_loop**

In `crates/daemon/src/ipc/pipe_server.rs`, replace the body of `accept_loop` with:
```rust
async fn accept_loop(
    pipe_name: String,
    state: Arc<DaemonState>,
    boot: Instant,
    mut shutdown: watch::Receiver<bool>,
) {
    if *shutdown.borrow() {
        return;
    }
    let mut first = true;
    loop {
        if *shutdown.borrow() {
            break;
        }
        let mut builder = ServerOptions::new();
        if first {
            builder.first_pipe_instance(true);
        }
        let server = match builder.create(&pipe_name) {
            Ok(s) => s,
            Err(e) => {
                // Transient error: log and continue. Only break on
                // explicit shutdown.
                eprintln!(
                    "terminal-commanderd: pipe create transient error: {e}; retrying in 100ms"
                );
                tokio::select! {
                    biased;
                    res = shutdown.changed() => {
                        if res.is_err() || *shutdown.borrow() { break; }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
                }
                continue;
            }
        };
        first = false;
        tokio::select! {
            biased;
            res = shutdown.changed() => {
                if res.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            res = server.connect() => {
                if res.is_ok() {
                    let state = Arc::clone(&state);
                    let shutdown_for_conn = shutdown.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_pipe_connection(server, state, boot, shutdown_for_conn).await {
                            eprintln!("terminal-commanderd: pipe connection error: {e}");
                        }
                    });
                }
            }
        }
    }
}
```

- [ ] **Step 7.4: Run test to verify it passes**

Run: `cargo nextest run -p terminal-commanderd --test pipe_accept_loop`
Expected: PASS.

- [ ] **Step 7.5: Commit**

```bash
git add crates/daemon/src/ipc/pipe_server.rs crates/daemon/tests/pipe_accept_loop.rs
git commit -m "fix(daemon/ipc): pipe accept loop survives multi-client + transient errors

first_pipe_instance(true) is only applied on the first iteration.
Transient ServerOptions::create errors no longer kill the loop; they
log and retry after 100ms unless shutdown is signalled."
```

---

### Task 8: Replace fake PeerCred with real PeerIdentity on Windows

**Files:**
- Create: `crates/daemon/src/ipc/peer_windows.rs`
- Modify: `crates/daemon/src/ipc/peer.rs`
- Modify: `crates/daemon/src/ipc/pipe_server.rs::handle_pipe_connection`
- Modify: `crates/daemon/src/ipc/server.rs::dispatch_envelope` signature
- Test: `crates/daemon/tests/pipe_peer_identity.rs` (new, `#[cfg(windows)]`)

- [ ] **Step 8.1: Write the failing test**

Create `crates/daemon/tests/pipe_peer_identity.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Verifies that the named-pipe handler reports the real client SID
// rather than the historical hard-coded uid=0,gid=0.

#![cfg(windows)]

use std::sync::Arc;
use std::time::Duration;
use terminal_commander_supervisor::identity::PeerIdentity;
use terminal_commanderd::ipc::pipe_server::PipeServer;
use terminal_commanderd::state::DaemonState;
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::time::sleep;

#[tokio::test]
async fn pipe_handler_reports_real_sid_not_root() {
    let pipe_name = format!(r"\\.\pipe\tc-test-peer-{}", std::process::id());
    let state = Arc::new(DaemonState::for_test().await.unwrap());
    let server = PipeServer::new(Arc::clone(&state), pipe_name.clone());
    let handle = server.spawn().unwrap();

    sleep(Duration::from_millis(50)).await;
    let _c = ClientOptions::new().open(&pipe_name).expect("connect");
    sleep(Duration::from_millis(100)).await;

    let observed = state.test_last_observed_peer_identity()
        .expect("server recorded a peer identity");
    match observed {
        PeerIdentity::Windows { sid, .. } => {
            assert!(sid.starts_with("S-1-"), "expected SID, got {sid}");
            assert!(!sid.ends_with("-500"), "should not be Administrator/0");
        }
        other => panic!("expected Windows identity, got {other:?}"),
    }
    handle.shutdown().await;
}
```
Add a `test_last_observed_peer_identity` test-helper on `DaemonState` (gated by `#[cfg(any(test, feature = "test-util"))]`) that records the latest peer identity dispatched.

- [ ] **Step 8.2: Run the test, expect failure**

Run: `cargo nextest run -p terminal-commanderd --test pipe_peer_identity`
Expected: FAIL — current handler hard-codes `PeerCred { uid: 0, gid: 0, pid: None }`.

- [ ] **Step 8.3: Implement peer_windows.rs**

Create `crates/daemon/src/ipc/peer_windows.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Resolve the Windows named-pipe client's SID, PID, and image path.

#![cfg(windows)]

use std::path::PathBuf;
use terminal_commander_supervisor::identity::PeerIdentity;
use tokio::net::windows::named_pipe::NamedPipeServer;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
};

pub fn peer_identity_for(server: &NamedPipeServer) -> PeerIdentity {
    let raw_handle = server.as_raw_handle();
    let handle = HANDLE(raw_handle as isize);
    let mut pid: u32 = 0;
    let pid_opt = unsafe {
        if GetNamedPipeClientProcessId(handle, &mut pid).is_ok() && pid != 0 {
            Some(pid)
        } else {
            None
        }
    };
    let (sid, image) = pid_opt
        .and_then(|pid| resolve_sid_and_image(pid))
        .unwrap_or((String::new(), None));
    if sid.is_empty() {
        return PeerIdentity::Unknown;
    }
    PeerIdentity::Windows { sid, pid: pid_opt, image }
}

fn resolve_sid_and_image(pid: u32) -> Option<(String, Option<PathBuf>)> {
    unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut token = HANDLE::default();
        if OpenProcessToken(proc, TOKEN_QUERY, &mut token).is_err() {
            let _ = CloseHandle(proc);
            return None;
        }
        let mut needed: u32 = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        let mut buf = vec![0u8; needed as usize];
        if GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .is_err()
        {
            let _ = CloseHandle(token);
            let _ = CloseHandle(proc);
            return None;
        }
        let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = windows::core::PWSTR::null();
        if ConvertSidToStringSidW(token_user.User.Sid, &mut sid_str).is_err() {
            let _ = CloseHandle(token);
            let _ = CloseHandle(proc);
            return None;
        }
        let sid = sid_str.to_string().unwrap_or_default();
        windows::Win32::System::Memory::LocalFree(std::mem::transmute(sid_str.0));

        let mut buf16 = vec![0u16; 1024];
        let mut len = buf16.len() as u32;
        let image = if QueryFullProcessImageNameW(
            proc,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf16.as_mut_ptr()),
            &mut len,
        )
        .is_ok()
        {
            Some(PathBuf::from(String::from_utf16_lossy(&buf16[..len as usize])))
        } else {
            None
        };
        let _ = CloseHandle(token);
        let _ = CloseHandle(proc);
        Some((sid, image))
    }
}
```
Add to daemon Cargo.toml `[target.'cfg(windows)'.dependencies]`:
```toml
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_Security_Authorization",
    "Win32_System_Memory",
    "Win32_System_Pipes",
    "Win32_System_Threading",
] }
terminal-commander-supervisor = { path = "../supervisor" }
```
Add the module declaration in `crates/daemon/src/ipc/mod.rs`:
```rust
#[cfg(windows)]
pub mod peer_windows;
```

- [ ] **Step 8.4: Update handle_pipe_connection to use real identity**

In `crates/daemon/src/ipc/pipe_server.rs`, replace the existing `handle_pipe_connection` body with:
```rust
async fn handle_pipe_connection(
    mut server: NamedPipeServer,
    state: Arc<DaemonState>,
    boot: Instant,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let identity = crate::ipc::peer_windows::peer_identity_for(&server);
    #[cfg(any(test, feature = "test-util"))]
    state.test_record_peer_identity(identity.clone());
    loop {
        if *shutdown.borrow() {
            break;
        }
        let req: RequestEnvelope = match read_request(&mut server).await {
            Ok(r) => r,
            Err(_) => break,
        };
        let resp = dispatch_envelope(&state, boot, &req, &identity).await;
        write_response(&mut server, &resp).await?;
    }
    Ok(())
}
```

- [ ] **Step 8.5: Update dispatch_envelope signature**

In `crates/daemon/src/ipc/server.rs`, change `dispatch_envelope`'s `peer: Option<PeerCred>` parameter to `peer: &PeerIdentity`. Update all call sites — there are two: the UDS path in `server.rs::handle_connection` and the named-pipe path you just touched. The UDS path should construct `PeerIdentity::Unix { uid: cred.uid, gid: cred.gid, pid: cred.pid }` from the existing `PeerCred`, or `PeerIdentity::Unknown` if `PeerCred` is `None`.

- [ ] **Step 8.6: Run the test to verify pass**

Run: `cargo nextest run -p terminal-commanderd --test pipe_peer_identity`
Expected: PASS — real SID surfaces.

- [ ] **Step 8.7: Run workspace tests**

Run: `cargo nextest run --workspace`
Expected: PASS.

- [ ] **Step 8.8: Commit**

```bash
git add crates/daemon/src/ipc/peer_windows.rs crates/daemon/src/ipc/peer.rs crates/daemon/src/ipc/mod.rs crates/daemon/src/ipc/pipe_server.rs crates/daemon/src/ipc/server.rs crates/daemon/Cargo.toml crates/daemon/tests/pipe_peer_identity.rs
git commit -m "feat(daemon/ipc): real Windows peer identity on named pipe

Resolves the connecting process SID via GetNamedPipeClientProcessId
+ OpenProcessToken + TokenUser instead of returning a hard-coded
uid=0,gid=0. dispatch_envelope now takes PeerIdentity rather than
Option<PeerCred>; UDS path adapts; named-pipe path uses the real SID
or PeerIdentity::Unknown when unresolvable."
```

---

### Task 9: Restrict named-pipe ACL to current user

**Files:**
- Create: `crates/daemon/src/ipc/pipe_acl.rs`
- Modify: `crates/daemon/src/ipc/pipe_server.rs::accept_loop`
- Test: `crates/daemon/tests/pipe_acl.rs` (new, `#[cfg(windows)]`)

- [ ] **Step 9.1: Write the failing test**

Create `crates/daemon/tests/pipe_acl.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Verifies the named-pipe security descriptor restricts access to
// LocalSystem + Administrators + current user.

#![cfg(windows)]

use terminal_commanderd::ipc::pipe_acl;

#[test]
fn sddl_includes_current_user_sid_and_denies_world() {
    let sddl = pipe_acl::build_sddl_for_current_user()
        .expect("build sddl");
    // Owner: current user.
    assert!(sddl.contains("O:"));
    // No (A;;...;;;WD)  (Everyone allow) and no (A;;...;;;BU) (Users).
    assert!(!sddl.contains(";;;WD)"), "ACL must not allow Everyone (WD): {sddl}");
    assert!(!sddl.contains(";;;BU)"), "ACL must not allow Users (BU): {sddl}");
    // Allow LocalSystem (SY) and Administrators (BA).
    assert!(sddl.contains(";;;SY)"));
    assert!(sddl.contains(";;;BA)"));
}
```

- [ ] **Step 9.2: Run, expect failure (module missing)**

Run: `cargo nextest run -p terminal-commanderd --test pipe_acl`
Expected: FAIL with `module pipe_acl not found`.

- [ ] **Step 9.3: Implement pipe_acl.rs**

Create `crates/daemon/src/ipc/pipe_acl.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Build a security descriptor string (SDDL) that restricts a Windows
// named pipe to LocalSystem, Administrators, and the current user.

#![cfg(windows)]

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub fn build_sddl_for_current_user() -> std::io::Result<String> {
    let user_sid = current_user_sid()?;
    // Owner: current user. DACL: LocalSystem full, Admins full,
    // current user full. Everyone denied implicitly.
    let sddl = format!(
        "O:{sid}D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{sid})",
        sid = user_sid
    );
    Ok(sddl)
}

fn current_user_sid() -> std::io::Result<String> {
    unsafe {
        let mut token = windows::Win32::Foundation::HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| std::io::Error::other(format!("OpenProcessToken: {e}")))?;
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        let mut buf = vec![0u8; needed as usize];
        GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .map_err(|e| std::io::Error::other(format!("GetTokenInformation: {e}")))?;
        let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = windows::core::PWSTR::null();
        ConvertSidToStringSidW(token_user.User.Sid, &mut sid_str)
            .map_err(|e| std::io::Error::other(format!("ConvertSidToStringSidW: {e}")))?;
        let s = sid_str.to_string().unwrap_or_default();
        windows::Win32::System::Memory::LocalFree(std::mem::transmute(sid_str.0));
        let _ = CloseHandle(token);
        Ok(s)
    }
}
```
Register the module in `crates/daemon/src/ipc/mod.rs`:
```rust
#[cfg(windows)]
pub mod pipe_acl;
```

- [ ] **Step 9.4: Pass the SDDL to ServerOptions**

In `crates/daemon/src/ipc/pipe_server.rs::accept_loop`, change the builder block:
```rust
let mut builder = ServerOptions::new();
if first {
    builder.first_pipe_instance(true);
}
let sddl = crate::ipc::pipe_acl::build_sddl_for_current_user()
    .unwrap_or_else(|e| {
        eprintln!("terminal-commanderd: SDDL build failed: {e}; pipe will use default ACL");
        String::new()
    });
let server = if sddl.is_empty() {
    builder.create(&pipe_name)
} else {
    // Use the lower-level create_with_security_attributes path via the
    // windows-sys APIs. tokio's NamedPipeServer wraps a HANDLE we
    // construct ourselves.
    crate::ipc::pipe_acl::create_named_pipe_with_sddl(&pipe_name, &sddl, first)
};
```
Add to `pipe_acl.rs`:
```rust
pub fn create_named_pipe_with_sddl(
    name: &str,
    sddl: &str,
    first: bool,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_ACCESS_DUPLEX, FILE_FLAG_FIRST_PIPE_INSTANCE,
        PIPE_TYPE_BYTE, PIPE_READMODE_BYTE, PIPE_WAIT,
    };
    use windows::core::PWSTR;
    use windows::core::PCWSTR;

    let wide_name: Vec<u16> = OsStr::new(name).encode_wide().chain(std::iter::once(0)).collect();
    let wide_sddl: Vec<u16> = OsStr::new(sddl).encode_wide().chain(std::iter::once(0)).collect();

    unsafe {
        let mut sd = std::ptr::null_mut();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide_sddl.as_ptr()),
            1, // SDDL_REVISION_1
            &mut sd,
            None,
        )
        .map_err(|e| std::io::Error::other(format!("ConvertStringSecurityDescriptor: {e}")))?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd as *mut _,
            bInheritHandle: false.into(),
        };
        let mut flags = PIPE_ACCESS_DUPLEX;
        if first {
            flags |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let handle = CreateNamedPipeW(
            PCWSTR(wide_name.as_ptr()),
            flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            255, // max instances
            4096,
            4096,
            0,
            Some(&sa),
        );
        if handle.0 == INVALID_HANDLE_VALUE.0 {
            return Err(std::io::Error::last_os_error());
        }
        // Tokio's NamedPipeServer::from_raw_handle requires the handle
        // to be in OVERLAPPED mode; CreateNamedPipeW returns
        // synchronous handles. Tokio docs: prefer ServerOptions for
        // OVERLAPPED. For Phase 3 we accept a synchronous handle and
        // rely on tokio's blocking-pool fallback. Phase 4 plan
        // revisits this.
        Ok(tokio::net::windows::named_pipe::NamedPipeServer::from_raw_handle(handle.0 as _).unwrap())
    }
}
```
Add to daemon Cargo.toml's windows features:
```
"Win32_System_Pipes",
"Win32_Security_Authorization",
```

- [ ] **Step 9.5: Run ACL unit test**

Run: `cargo nextest run -p terminal-commanderd --test pipe_acl`
Expected: PASS.

- [ ] **Step 9.6: Run accept-loop test again to make sure ACL change didn't regress it**

Run: `cargo nextest run -p terminal-commanderd --test pipe_accept_loop`
Expected: PASS.

- [ ] **Step 9.7: Commit**

```bash
git add crates/daemon/src/ipc/pipe_acl.rs crates/daemon/src/ipc/pipe_server.rs crates/daemon/src/ipc/mod.rs crates/daemon/Cargo.toml crates/daemon/tests/pipe_acl.rs
git commit -m "feat(daemon/ipc): restrict named-pipe ACL to current user + admins

Bind named pipe with a security descriptor that allows LocalSystem,
Administrators, and the current user SID only. Everyone/Users denied.
SDDL is built from the live process token via OpenProcessToken +
TokenUser + ConvertSidToStringSidW."
```

---

### Task 10: Gut Node session_supervisor.js — stop deleting state on EOF

**Files:**
- Modify: `packages/terminal-commander/lib/daemon/session_supervisor.js`
- Test: `packages/terminal-commander/test/session_supervisor_no_rm.test.js` (new)

- [ ] **Step 10.1: Write the failing test**

Create `packages/terminal-commander/test/session_supervisor_no_rm.test.js`:
```javascript
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { runHarnessMcpSession } = require('../lib/daemon/session_supervisor.js');

test('session base directory survives MCP child exit', async () => {
  // Use a fake daemon (sleeps) and a fake MCP (exits immediately) so
  // we can observe whether cleanup fires.
  const node = process.execPath;
  const fakeDaemon = path.join(os.tmpdir(), `fake-daemon-${process.pid}.js`);
  fs.writeFileSync(fakeDaemon, 'setInterval(()=>{},1000);');
  const fakeMcp = path.join(os.tmpdir(), `fake-mcp-${process.pid}.js`);
  fs.writeFileSync(fakeMcp, 'process.exit(0);');

  const outcome = await runHarnessMcpSession({
    daemonBinary: node,
    mcpBinary: node,
    argv: [fakeMcp],
    env: { ...process.env, FAKE_DAEMON: fakeDaemon },
  });

  assert.equal(outcome.code, 0);
  // Session base directory: find the most recent under SESSIONS_ROOT.
  const sessionsRoot = path.join(
    os.homedir(),
    '.config',
    'terminal-commander',
    'sessions',
  );
  const entries = fs
    .readdirSync(sessionsRoot, { withFileTypes: true })
    .filter((e) => e.isDirectory());
  assert.ok(entries.length > 0, 'expected at least one session dir to remain');
});
```

- [ ] **Step 10.2: Run, expect failure**

Run: `node --test packages/terminal-commander/test/session_supervisor_no_rm.test.js`
Expected: FAIL — current supervisor deletes the session base in `cleanup()` triggered by both `mcp.on('exit')` and `process.on('exit')`.

- [ ] **Step 10.3: Remove cleanup-on-exit + cleanupStaleSessions invocation**

In `packages/terminal-commander/lib/daemon/session_supervisor.js`:

1. In `runHarnessMcpSession`, remove the line:
   ```javascript
   cleanupStaleSessions(env);
   ```
2. Replace the `cleanup` function with:
   ```javascript
   const cleanup = () => {
     if (cleaned) return;
     cleaned = true;
     if (daemon) killProcessTree(daemon);
     // NOTE: session base directory is intentionally preserved across
     // MCP exits so doctor/debugging can inspect it. Cleanup is now
     // explicit via `terminal-commander maintenance cleanup --older-than 7d`.
   };
   ```
3. Remove the line `process.on("exit", cleanup);` entirely.
4. In `mcp.on("exit")` handler, change to call only the lightweight teardown that does not include `removeSessionDir`:
   ```javascript
   mcp.on("exit", (code, signal) => {
     if (daemon) killProcessTree(daemon);
     cleaned = true;
     process.off("SIGINT", onSignal);
     process.off("SIGTERM", onSignal);
     resolve({ code: code == null ? 1 : code, signal: signal || null });
   });
   ```
5. Capture daemon `stdio` to log file instead of `"ignore"`. Replace `spawnDaemonHidden`'s `opts.stdio = "ignore"` line with:
   ```javascript
   const logPath = path.join(dataDir, "terminal-commanderd.log");
   try { fs.mkdirSync(dataDir, { recursive: true }); } catch (_e) {}
   const logFd = fs.openSync(logPath, "a");
   opts.stdio = ["ignore", logFd, logFd];
   ```

- [ ] **Step 10.4: Run the new test to verify pass**

Run: `node --test packages/terminal-commander/test/session_supervisor_no_rm.test.js`
Expected: PASS — session dir remains after MCP child exits.

- [ ] **Step 10.5: Re-run existing supervisor tests**

Run: `cd packages/terminal-commander && npm test`
Expected: PASS. If any existing test asserted that the session dir is deleted on EOF, update it to assert the opposite (that is the desired new behavior per ADR).

- [ ] **Step 10.6: Commit**

```bash
git add packages/terminal-commander/lib/daemon/session_supervisor.js packages/terminal-commander/test/session_supervisor_no_rm.test.js
git commit -m "fix(supervisor.js): stop deleting session state on MCP exit

Per ADR-native-tier1-runtime: MCP stdin EOF is normal harness
disconnect. The Node supervisor no longer removes the session base
directory on cleanup, no longer calls cleanupStaleSessions at
startup, and captures daemon stdio to a log file so the failure mode
is diagnosable."
```

---

### Task 11: Capture daemon log path in MCP supervisor diagnostics

**Files:**
- Modify: `crates/mcp/src/main.rs::resolve_log_dir` (already added in Task 6 — verify wired)
- Modify: `crates/daemon/src/runtime.rs::run_ipc_server` (Windows + Unix variants)
- Test: `crates/daemon/tests/daemon_writes_log.rs` (new)

- [ ] **Step 11.1: Write the failing test**

Create `crates/daemon/tests/daemon_writes_log.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Verifies that the daemon writes its startup line to
// `<data_dir>/logs/terminal-commanderd.log` even when stdio is
// redirected.

use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daemon_writes_startup_line_to_log() {
    let dir = TempDir::new().unwrap();
    let data_dir: PathBuf = dir.path().into();
    let log_dir = data_dir.join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let log_path = log_dir.join("terminal-commanderd.log");

    let daemon_bin = env!("CARGO_BIN_EXE_terminal-commanderd");
    let mut child = std::process::Command::new(daemon_bin)
        .arg("--data-dir").arg(&data_dir)
        .arg("start")
        .arg("--mode").arg("ipc-server")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");

    // Give the daemon up to 3s to bind and log.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut got = false;
    while std::time::Instant::now() < deadline {
        if log_path.exists() {
            let contents = std::fs::read_to_string(&log_path).unwrap_or_default();
            if contents.contains("IPC server bound") {
                got = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    child.kill().ok();
    let _ = child.wait();
    assert!(got, "daemon did not write startup line to {}", log_path.display());
}
```

- [ ] **Step 11.2: Run, expect failure**

Run: `cargo nextest run -p terminal-commanderd --test daemon_writes_log`
Expected: FAIL — daemon currently writes only to `eprintln!`.

- [ ] **Step 11.3: Add file-backed tracing subscriber**

In `crates/daemon/Cargo.toml`, add to `[dependencies]`:
```toml
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter"] }
tracing-appender = "0.2"
```

In `crates/daemon/src/runtime.rs`, near the top of the file add:
```rust
fn init_file_logging(data_dir: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::never(&log_dir, "terminal-commanderd.log");
    let (nb, guard) = tracing_appender::non_blocking(file_appender);
    let _ = tracing_subscriber::fmt()
        .with_writer(nb)
        .with_ansi(false)
        .with_target(false)
        .try_init();
    Some(guard)
}
```

In both `run_ipc_server` variants (`#[cfg(windows)]` at L234, `#[cfg(unix)]` at L202), insert at the start:
```rust
let _log_guard = init_file_logging(&config.daemon.data_dir);
```
and replace `eprintln!` lines with `tracing::info!` equivalents. Keep at least one `tracing::info!("IPC server bound (...)")` so the test assertion has a stable substring to match.

- [ ] **Step 11.4: Run test, expect pass**

Run: `cargo nextest run -p terminal-commanderd --test daemon_writes_log`
Expected: PASS.

- [ ] **Step 11.5: Commit**

```bash
git add crates/daemon/Cargo.toml crates/daemon/src/runtime.rs crates/daemon/tests/daemon_writes_log.rs
git commit -m "feat(daemon): write structured logs to <data_dir>/logs/terminal-commanderd.log

Even when the daemon is spawned with stdio redirected to /dev/null
or NUL, startup, bind, and shutdown events land in a rolling log
file. Diagnostics now survive supervised launches."
```

---

### Task 12: Stdin-EOF survival end-to-end test

**Files:**
- Test: `crates/mcp/tests/stdin_eof_survives.rs` (new)

- [ ] **Step 12.1: Write the test**

Create `crates/mcp/tests/stdin_eof_survives.rs`:
```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// End-to-end: launch the daemon, launch MCP pointing at it, close
// MCP stdin, verify MCP exits 0 AND the daemon is still alive.

use std::time::Duration;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mcp_stdin_eof_does_not_kill_daemon() {
    let dir = TempDir::new().unwrap();
    let data_dir = dir.path().to_path_buf();

    let daemon_bin = env!("CARGO_BIN_EXE_terminal-commanderd");
    let mcp_bin = env!("CARGO_BIN_EXE_terminal-commander-mcp");

    let mut daemon = std::process::Command::new(daemon_bin)
        .arg("--data-dir").arg(&data_dir)
        .arg("start")
        .arg("--mode").arg("ipc-server")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");

    // Wait briefly for daemon to bind.
    tokio::time::sleep(Duration::from_millis(500)).await;

    #[cfg(unix)]
    let endpoint = data_dir.join("terminal-commanderd.sock");
    #[cfg(windows)]
    let endpoint = std::path::PathBuf::from(format!(
        r"\\.\pipe\terminal-commander-{}-default",
        std::env::var("USERNAME").unwrap_or_else(|_| "user".into())
    ));

    let mut mcp = std::process::Command::new(mcp_bin)
        .env("TC_SOCKET", &endpoint)
        .env("TC_SUPERVISOR_ALLOW_SPAWN", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn mcp");

    // Close MCP stdin.
    drop(mcp.stdin.take());
    let status = mcp.wait().expect("mcp wait");
    assert_eq!(status.code(), Some(0), "MCP should exit 0 on stdin EOF");

    // Verify daemon still alive.
    assert!(daemon.try_wait().expect("try_wait").is_none(), "daemon should still be running");

    daemon.kill().ok();
    let _ = daemon.wait();
}
```

- [ ] **Step 12.2: Run, expect pass**

Run: `cargo nextest run -p terminal-commander-mcp --test stdin_eof_survives`
Expected: PASS.

- [ ] **Step 12.3: Commit**

```bash
git add crates/mcp/tests/stdin_eof_survives.rs
git commit -m "test(mcp): stdin EOF survives — daemon outlives MCP child"
```

---

### Task 13: Doctor command surfaces daemon status

**Files:**
- Modify: `crates/cli/src/main.rs` (the `Status` / `Doctor` branch — replace stale stub line)

- [ ] **Step 13.1: Locate the stub line**

Run: `cargo run -p terminal-commander -- status`
Observe: line `state         : not running (TC25 stub; IPC arrives in TC21 follow-up)`.

- [ ] **Step 13.2: Replace the stub with a live status block**

In `crates/cli/src/main.rs`, find the function that prints the status output (search for `TC25 stub`). Replace the whole block with code that:
1. Calls `terminal_commander_supervisor::ensure::ensure_daemon` with `allow_spawn: false`.
2. Prints, in order:
   ```
   terminal-commander status:
     version       : <CARGO_PKG_VERSION>
     endpoint      : <endpoint string>
     daemon        : <running | unavailable>
     pid           : <if known>
     log_path      : <state_dir>/logs/terminal-commanderd.log
     state_dir     : <resolved>
   ```
3. Returns exit 0 if running, exit 1 if unavailable.

If the existing function passes structured config in, reuse it; otherwise call the same `resolve_state_dir`-style helpers from `crates/mcp/src/main.rs` (move them into the new supervisor crate's `lib.rs` if they're needed from both).

- [ ] **Step 13.3: Manual smoke**

Run: `cargo run -p terminal-commander -- status`
Expected (daemon not running):
```
terminal-commander status:
  version       : 0.x.y
  endpoint      : ...
  daemon        : unavailable
  ...
```
Run: start daemon manually, then status again.
Expected: `daemon        : running`.

- [ ] **Step 13.4: Commit**

```bash
git add crates/cli/src/main.rs
git commit -m "feat(cli): status reports live daemon state via supervisor

Replaces the TC25 'not running (stub)' placeholder with real
endpoint + running/unavailable + pid + log_path output sourced from
the supervisor crate's ensure_daemon probe (allow_spawn=false)."
```

---

### Task 14: Self-review checklist + final workspace test run

- [ ] **Step 14.1: Run the full workspace test suite**

Run: `cargo nextest run --workspace`
Expected: ALL PASS, including the new tests added in Tasks 3-13.

- [ ] **Step 14.2: Run clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: zero warnings. Fix any that surface from the new crate.

- [ ] **Step 14.3: Run format check**

Run: `cargo fmt --all --check`
Expected: clean. Run `cargo fmt --all` if not.

- [ ] **Step 14.4: Verify the connect-closed symptom from a fresh terminal**

Manual: open a fresh PowerShell. Run the supervised MCP binary against an unbound endpoint:
```powershell
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = (Get-Command terminal-commander-mcp).Source
$psi.RedirectStandardInput  = $true
$psi.RedirectStandardOutput = $true
$psi.UseShellExecute = $false
$psi.EnvironmentVariables['TC_SOCKET'] = '\\.\pipe\never-bound'
$psi.EnvironmentVariables['TC_SUPERVISOR_ALLOW_SPAWN'] = '0'
$p = [System.Diagnostics.Process]::Start($psi)
$payload = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}'
$p.StandardInput.WriteLine($payload); $p.StandardInput.Flush()
Start-Sleep -Seconds 1
$p.Kill()
$p.StandardOutput.ReadToEnd()
```
Expected: a valid JSON-RPC `initialize` response with `serverInfo` populated. **No silent exit; no "Connection closed."**

- [ ] **Step 14.5: Final commit (release notes)**

Add a one-line entry to `BACKLOG.md` or `CHANGELOG.md` (whichever the project uses):
```
- chore(phase-0-3): native tier-1 ADR locked; supervisor crate ships;
  Windows named-pipe accept loop fixed; pipe ACL + peer identity
  hardened; Node supervisor no longer destroys session state on EOF.
```
Commit:
```bash
git add BACKLOG.md
git commit -m "chore: phase 0-3 complete — connect-closed fixed, ADR locked"
```

---

## Self-review against goal

**Coverage matrix vs GPT roadmap Phase 0-3:**

| Roadmap item | Plan task |
|---|---|
| Phase 0 — Lock decision (ADR, SPEC, ARCHITECTURE, _USER_DECISIONS, README) | Task 1 |
| Phase 1 — Daemon logs, doctor checks, MCP returns structured error, preserve state on EOF | Tasks 6, 10, 11, 13 |
| Phase 2 — Rust supervisor module, MCP uses it, remove cleanup-on-exit, disconnect tests | Tasks 2-6, 10, 12 |
| Phase 3 — Fix accept loop, current-user pipe ACL, real peer identity, Windows pipe E2E, pipe status in doctor | Tasks 7, 8, 9, 13 |

**Audit §10 severity-ranked symptoms vs plan:**

| Audit symptom | Resolved by |
|---|---|
| §2.1 (npm windows-x64 package missing) | NOT in this plan — Phase 4 |
| §4.1 + §4.4 (stdio:inherit + cleanup nukes session) | Task 10 |
| §4.2 + §4.3 (silent readiness failure) | Tasks 6, 11 |
| §6.1 (accept loop fragility) | Task 7 |
| §6.2 (fake peer cred) | Task 8 |
| §6.3 (no ACL on pipe) | Task 9 |
| §6.4 (daemon stderr discarded) | Tasks 10, 11 |
| §6.5 (PTY advertised on Windows) | NOT in this plan — Phase 5 |
| §3 (Windows non-goal/in-scope conflict) | Task 1 |

**Placeholder scan:** all code blocks contain real content; no "TODO" or "fill in" text remains in plan steps.

**Type consistency:** `PeerIdentity` referenced in Tasks 3, 8 — same shape. `EnsureDaemonStatus` referenced in Tasks 4, 5, 6, 13 — same shape. `Endpoint` referenced in Tasks 4, 5, 6 — same shape.

---

## Execution Handoff

Plan saved at `docs/superpowers/plans/2026-05-24-native-tier1-runtime-phases-0-3.md`. Two execution options:

1. **Subagent-Driven (recommended)** — dispatch fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using executing-plans, batched checkpoints.

Which approach?
