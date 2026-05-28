# Session Supervisor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add session lifecycle to terminal-commander — enumerate per-harness daemons, self-reap idle ones, a `session list|reap` CLI — plus two trust fixes (a daemon-identity liveness handshake and a TC-only WSLENV allowlist).

**Architecture:** Builds on F1 (per-harness `TC_SESSION` endpoints). The filesystem of per-session state dirs + their existing pidfiles IS the registry — no central store, no always-on process. Daemons self-reap on an in-memory idle TTL; `session reap` is a manual graceful-shutdown-via-IPC override with an identity-gated force fallback. Per the approved spec `docs/superpowers/specs/2026-05-28-session-supervisor-design.md` + ADR `docs/adr/ADR-session-supervisor-scope.md` (codex-approved, 4 passes).

**Tech Stack:** Rust (workspace crates `supervisor`, `daemon`/`terminal-commanderd`, `cli`, `mcp`), tokio IPC (UDS on unix, named pipe on Windows), Node.js npm wrapper (`packages/terminal-commander`).

**Verification environments:**
- Unix-gated Rust tests: WSL — `wsl -e bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test --workspace --features test-util 2>&1 | tail -30"`
- `cfg(windows)` Rust: Windows cargo directly.
- JS: `cd packages/terminal-commander && npm test`.
- Workspace gate before phase commits: `cargo fmt --check` (touched files), `cargo clippy -p <crate> --all-targets --features test-util -- -D warnings`, then `cargo clean` at phase boundaries.

**Branch:** `feat/f1-launcher-wiring` (F1 + supervisor merge together per operator decision).

---

## File Structure

**Phase A — `default` token reservation (smallest, unblocks enumeration labels):**
- Modify: `crates/supervisor/src/session.rs` — reserve `default` (case-insensitive) in `is_valid_session_token`.
- Modify: `packages/terminal-commander/lib/session/mint.js` — mirror the reservation in `isValidSessionToken`.

**Phase B — `supervisor::sessions` enumeration:**
- Create: `crates/supervisor/src/sessions.rs` — `enumerate`, `SessionEntry`, raw pidfile parse, compare-before-delete cleanup.
- Modify: `crates/supervisor/src/lib.rs` — `pub mod sessions;`.
- Modify: `crates/supervisor/src/pidfile.rs` — add `read_pidfile_raw` (parse without the dead-pid filter).

**Phase C — Health gains `idle_secs` + non-bumping audit-free peek + last_activity:**
- Modify: `crates/daemon/src/ipc/protocol.rs` — `IpcResponse::Health { uptime_secs, idle_secs }` (idle optional via `#[serde(default)]`).
- Modify: `crates/daemon/src/ipc/server.rs` — Health handler emits idle; Health skips the per-request audit + does not bump `last_activity`; add `last_activity` to `DaemonState`, bump on non-Health dispatch.
- Modify: `crates/daemon/src/state.rs` (or wherever `DaemonState` lives) — `last_activity: Arc<Mutex<Instant>>` (or `AtomicU64` epoch secs).

**Phase D — liveness handshake in probe:**
- Modify: `crates/supervisor/src/ensure.rs` — `probe_endpoint` sends `IpcRequest::Health`, requires a well-formed `IpcResponse::Health` (lenient decode); returns `idle_secs`.

**Phase E — Shutdown protocol + daemon self-reap + graceful drain:**
- Modify: `crates/daemon/src/ipc/protocol.rs` — `IpcRequest::Shutdown`, `IpcResponse::ShutdownAck { draining }`, `IpcErrorCode::ShuttingDown`.
- Modify: `crates/daemon/src/ipc/server.rs` — `Shutdown` dispatch arm; in-flight connection tracking + drain; reject-new-during-drain with `ShuttingDown`.
- Modify: `crates/daemon/src/runtime.rs` — internal shutdown source (idle-timer + Shutdown-IPC) `select!`ed alongside the OS signal.
- Modify: `crates/daemon/src/config.rs` — read `TC_IDLE_TTL_SECS` (default 1800, 0=off).

**Phase F — `session list|reap` CLI:**
- Modify: `crates/cli/src/main.rs` — `Command::Session { op: SessionOp }`, `list`/`reap` handlers.
- Modify: `crates/mcp/src/daemon_client.rs` or `crates/cli` client — a `shutdown()` client call (reuses `call(IpcRequest::Shutdown)`).

**Phase G — WSLENV TC-only allowlist:**
- Modify: `packages/terminal-commander/lib/wsl/spawn.js` — `ensureSessionInWslEnv` rebuilds WSLENV as `TC_SESSION/u` only.

**Phase H — E2E smoke:**
- Modify: `scripts/smoke/verify-session-isolation-smoke.ps1` — add list/reap coverage.

---

## Phase A — Reserve `default` as a session token

### Task A1: Reject `default` in the Rust validator

**Files:**
- Modify: `crates/supervisor/src/session.rs:36-41`
- Test: same file `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the failing test**

Add to `crates/supervisor/src/session.rs` tests module:
```rust
#[test]
fn default_is_reserved_and_rejected() {
    for reserved in ["default", "Default", "DEFAULT", "deFAULT"] {
        assert!(
            !is_valid_session_token(reserved),
            "{reserved:?} must be reserved (collides with the default session label)"
        );
        let env = FakeEnv::new().with("TC_SESSION", reserved);
        assert_eq!(
            resolve_session(&env),
            SessionEndpoint::Default,
            "reserved token {reserved:?} must soft-fail to the per-user default"
        );
    }
    // A token merely CONTAINING default is still fine.
    assert!(is_valid_session_token("default-1"));
    assert!(is_valid_session_token("my-default"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p terminal-commander-supervisor --lib session::tests::default_is_reserved_and_rejected`
Expected: FAIL — `"default"` currently passes `is_valid_session_token` (matches charset), so the assertion `!is_valid_session_token("default")` fails.

- [ ] **Step 3: Implement the reservation**

Replace `is_valid_session_token` in `crates/supervisor/src/session.rs`:
```rust
/// Reserved session labels that must never be a token (they collide with the
/// session-supervisor's display/selector labels).
const RESERVED_SESSION_TOKENS: &[&str] = &["default"];

#[must_use]
pub fn is_valid_session_token(token: &str) -> bool {
    if RESERVED_SESSION_TOKENS
        .iter()
        .any(|r| token.eq_ignore_ascii_case(r))
    {
        return false;
    }
    !token.is_empty()
        && token.len() <= MAX_SESSION_TOKEN_LEN
        && token.chars().any(|c| c.is_ascii_alphanumeric())
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p terminal-commander-supervisor --lib session::`
Expected: PASS (new test + all existing session tests stay green).

- [ ] **Step 5: Commit**

```bash
git add crates/supervisor/src/session.rs
git commit -m "feat(supervisor): reserve 'default' as a session token (case-insensitive)"
```

### Task A2: Mirror the reservation in JS

**Files:**
- Modify: `packages/terminal-commander/lib/session/mint.js`
- Test: `packages/terminal-commander/test/session-mint.test.js`

- [ ] **Step 1: Write the failing test**

Add to `packages/terminal-commander/test/session-mint.test.js`:
```js
test("isValidSessionToken reserves 'default' (Rust parity)", () => {
  for (const r of ["default", "Default", "DEFAULT"]) {
    assert.equal(isValidSessionToken(r), false, `${r} must be reserved`);
  }
  assert.equal(isValidSessionToken("default-1"), true);
  assert.equal(isValidSessionToken("my-default"), true);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd packages/terminal-commander && node --test test/session-mint.test.js`
Expected: FAIL — `isValidSessionToken("default")` currently returns true.

- [ ] **Step 3: Implement**

In `packages/terminal-commander/lib/session/mint.js`, add near the top of `isValidSessionToken`:
```js
const RESERVED_SESSION_TOKENS = Object.freeze(["default"]);

function isValidSessionToken(token) {
  if (typeof token !== "string") return false;
  if (RESERVED_SESSION_TOKENS.some((r) => token.toLowerCase() === r)) return false;
  // ... existing checks unchanged ...
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd packages/terminal-commander && node --test test/session-mint.test.js`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/terminal-commander/lib/session/mint.js packages/terminal-commander/test/session-mint.test.js
git commit -m "feat(harness): mirror 'default' token reservation in JS validator"
```

---

## Phase B — `supervisor::sessions` enumeration

### Task B1: Raw pidfile parse (no dead-pid filter)

**Files:**
- Modify: `crates/supervisor/src/pidfile.rs`
- Test: same file

- [ ] **Step 1: Write the failing test**

Add to `crates/supervisor/src/pidfile.rs` tests:
```rust
#[test]
fn read_pidfile_raw_returns_dead_pid_contents() {
    let dir = std::env::temp_dir().join(format!("tc-raw-{}", std::process::id()));
    let rec = RunningDaemon { pid: 999_999_999, version: "0.1.0".into(), endpoint: "x".into() };
    write_pidfile(&dir, &rec).unwrap();
    // read_pidfile hides dead pids; read_pidfile_raw must NOT.
    assert!(read_pidfile(&dir).is_none(), "read_pidfile still hides dead pids");
    assert_eq!(read_pidfile_raw(&dir), Some(rec), "raw must return contents regardless of liveness");
    remove_pidfile(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p terminal-commander-supervisor --lib pidfile::tests::read_pidfile_raw`
Expected: FAIL — `read_pidfile_raw` does not exist (compile error).

- [ ] **Step 3: Implement**

Add to `crates/supervisor/src/pidfile.rs`:
```rust
/// Read + parse the pidfile WITHOUT the liveness filter. Returns the recorded
/// `RunningDaemon` even when its pid is dead, so enumeration can classify stale
/// entries. Returns `None` only when the file is absent or unparseable.
#[must_use]
pub fn read_pidfile_raw(state_dir: &Path) -> Option<RunningDaemon> {
    let bytes = std::fs::read(pidfile_path(state_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p terminal-commander-supervisor --lib pidfile::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/supervisor/src/pidfile.rs
git commit -m "feat(supervisor): add read_pidfile_raw (parse without dead-pid filter)"
```

### Task B2: `sessions::enumerate` + `SessionEntry`

**Files:**
- Create: `crates/supervisor/src/sessions.rs`
- Modify: `crates/supervisor/src/lib.rs` (add `pub mod sessions;`)
- Test: in `sessions.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/supervisor/src/sessions.rs`:
```rust
//! Session enumeration for the supervisor CLI. Pure filesystem read: the base
//! dir's pidfile is the "default" session; each immediate subdir with a pidfile
//! is a seeded session labeled by its token. No daemon connection here.

use std::path::{Path, PathBuf};

use crate::pidfile::{pid_alive, pidfile_path, read_pidfile_raw};

/// One enumerated session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    /// "default" for the base-dir session, else the token (subdir name).
    pub label: String,
    pub state_dir: PathBuf,
    pub pid: u32,
    pub version: String,
    pub endpoint: String,
    /// True iff the recorded pid is currently alive.
    pub alive: bool,
}

/// Enumerate sessions under `base_dir`: the base pidfile (label "default") plus
/// every immediate subdir containing a pidfile (label = subdir name).
#[must_use]
pub fn enumerate(base_dir: &Path) -> Vec<SessionEntry> {
    let mut out = Vec::new();
    if let Some(e) = entry_for(base_dir, "default") {
        out.push(e);
    }
    if let Ok(rd) = std::fs::read_dir(base_dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() && pidfile_path(&p).exists() {
                let label = ent.file_name().to_string_lossy().into_owned();
                if let Some(e) = entry_for(&p, &label) {
                    out.push(e);
                }
            }
        }
    }
    out
}

fn entry_for(state_dir: &Path, label: &str) -> Option<SessionEntry> {
    let rec = read_pidfile_raw(state_dir)?;
    Some(SessionEntry {
        label: label.to_owned(),
        state_dir: state_dir.to_path_buf(),
        pid: rec.pid,
        version: rec.version,
        endpoint: rec.endpoint,
        alive: pid_alive(rec.pid),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pidfile::{write_pidfile, RunningDaemon};

    fn tmp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "tc-sessions-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    #[test]
    fn enumerate_finds_default_and_seeded_with_stale_classification() {
        let base = tmp();
        // default session (dead pid -> stale)
        write_pidfile(&base, &RunningDaemon { pid: 999_999_999, version: "0.1.0".into(), endpoint: "base.sock".into() }).unwrap();
        // seeded session "agent-1" (alive pid = this test process)
        let seeded = base.join("agent-1");
        write_pidfile(&seeded, &RunningDaemon { pid: std::process::id(), version: "0.1.1".into(), endpoint: "agent.sock".into() }).unwrap();

        let mut got = enumerate(&base);
        got.sort_by(|a, b| a.label.cmp(&b.label));
        assert_eq!(got.len(), 2);
        let agent = got.iter().find(|e| e.label == "agent-1").unwrap();
        assert!(agent.alive, "seeded session with this pid must be alive");
        assert_eq!(agent.version, "0.1.1");
        let def = got.iter().find(|e| e.label == "default").unwrap();
        assert!(!def.alive, "default session with dead pid must be stale");

        let _ = std::fs::remove_dir_all(&base);
    }
}
```

- [ ] **Step 2: Add the module + run the test (expect fail then pass)**

Add to `crates/supervisor/src/lib.rs`: `pub mod sessions;`
Run: `cargo test -p terminal-commander-supervisor --lib sessions::`
Expected: PASS (the test was written with the implementation in the same step — verify the module compiles and the test is green; if RED, it indicates a wiring error to fix).

> NOTE: B2 bundles impl+test because the module is new and the test needs the types to compile. Treat a RED here as "fix until green," per TDD verify-green.

- [ ] **Step 3: Commit**

```bash
git add crates/supervisor/src/sessions.rs crates/supervisor/src/lib.rs
git commit -m "feat(supervisor): sessions::enumerate (default + seeded, stale classification)"
```

### Task B3: Compare-before-delete stale cleanup

**Files:**
- Modify: `crates/supervisor/src/sessions.rs`
- Test: same file

- [ ] **Step 1: Write the failing test**

Add to `sessions.rs` tests:
```rust
#[test]
fn cleanup_stale_removes_only_matching_dead_pid() {
    let base = tmp();
    write_pidfile(&base, &RunningDaemon { pid: 999_999_999, version: "0".into(), endpoint: "x".into() }).unwrap();
    // Stale (dead pid) -> removed.
    assert!(cleanup_stale(&base, 999_999_999), "matching dead pid must be cleaned");
    assert!(read_pidfile_raw(&base).is_none(), "pidfile must be gone");

    // Race guard: pidfile now names a DIFFERENT pid than the one we classified.
    write_pidfile(&base, &RunningDaemon { pid: std::process::id(), version: "0".into(), endpoint: "x".into() }).unwrap();
    assert!(!cleanup_stale(&base, 999_999_999), "must NOT delete when current pid differs from classified");
    assert!(read_pidfile_raw(&base).is_some(), "live pidfile must survive");
    let _ = std::fs::remove_dir_all(&base);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p terminal-commander-supervisor --lib sessions::tests::cleanup_stale`
Expected: FAIL — `cleanup_stale` does not exist.

- [ ] **Step 3: Implement**

Add to `sessions.rs`:
```rust
/// Remove a stale pidfile, but ONLY if it STILL names `classified_pid` at
/// delete time. Closes the race where a daemon restarts (writing a fresh
/// pidfile with a new pid) between stale classification and cleanup. Returns
/// true iff a file was removed.
#[must_use]
pub fn cleanup_stale(state_dir: &Path, classified_pid: u32) -> bool {
    match read_pidfile_raw(state_dir) {
        Some(rec) if rec.pid == classified_pid => {
            crate::pidfile::remove_pidfile(state_dir);
            true
        }
        _ => false,
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p terminal-commander-supervisor --lib sessions::`
Expected: PASS.

- [ ] **Step 5: Commit + phase verify**

```bash
cargo test -p terminal-commander-supervisor --lib
git add crates/supervisor/src/sessions.rs
git commit -m "feat(supervisor): compare-before-delete stale pidfile cleanup"
```

---

## Phase C — Health `idle_secs` + non-bumping audit-free peek + `last_activity`

### Task C1: Add `last_activity` to `DaemonState`

**Files:**
- Modify: `crates/daemon/src/state.rs` (locate the `DaemonState` struct; if it lives elsewhere, follow the `Arc<DaemonState>` definition)
- Test: a unit test on the bump/read helpers

- [ ] **Step 1: Write the failing test**

Add a unit test near `DaemonState`:
```rust
#[test]
fn last_activity_bumps_and_reads_idle() {
    let state = DaemonState::test_minimal(); // existing test constructor or bootstrap
    let before = state.idle_secs();
    assert_eq!(before, 0, "fresh state is not idle");
    std::thread::sleep(std::time::Duration::from_millis(5));
    // No bump yet -> idle grows (will be 0s at this resolution but non-panicking).
    let _ = state.idle_secs();
    state.bump_activity();
    assert_eq!(state.idle_secs(), 0, "bump resets idle to ~0");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p terminal-commanderd --lib last_activity_bumps`
Expected: FAIL — `bump_activity`/`idle_secs` not defined.

- [ ] **Step 3: Implement**

In `DaemonState`, add a field + methods:
```rust
// field:
last_activity: std::sync::Arc<std::sync::Mutex<std::time::Instant>>,
// (initialize in the constructor to Instant::now())

impl DaemonState {
    /// Record that a real (non-peek) IPC request was served.
    pub fn bump_activity(&self) {
        *self.last_activity.lock().unwrap() = std::time::Instant::now();
    }
    /// Seconds since the last real IPC request.
    #[must_use]
    pub fn idle_secs(&self) -> u64 {
        self.last_activity.lock().unwrap().elapsed().as_secs()
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p terminal-commanderd --lib last_activity`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/state.rs
git commit -m "feat(daemon): track last_activity on DaemonState (bump_activity/idle_secs)"
```

### Task C2: Health returns `idle_secs`; bump on non-Health; Health skips audit

**Files:**
- Modify: `crates/daemon/src/ipc/protocol.rs:208` (Health variant)
- Modify: `crates/daemon/src/ipc/server.rs:379-394,553-562`
- Test: `crates/daemon/tests/ipc_roundtrip.rs` (health round-trip already exists)

- [ ] **Step 1: Write the failing test**

In `crates/daemon/tests/ipc_roundtrip.rs`, extend the health test (or add one):
```rust
#[test]
fn health_returns_idle_secs_and_does_not_bump_or_audit() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("health-idle");
        let (state, handle) = build_server_with_state(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());

        // Two Health calls: idle must NOT reset to 0 between them (non-bumping),
        // and no audit row may be written for either.
        let _ = client.call(1, IpcRequest::Health).await.expect("h1");
        let h2 = client.call(2, IpcRequest::Health).await.expect("h2");
        match h2 {
            IpcResponse::Health { idle_secs, .. } => {
                // idle_secs is present (Some/u64); value unconstrained here.
                let _ = idle_secs;
            }
            other => panic!("unexpected: {other:?}"),
        }
        let rows = { let mut g = state.store.lock(); g.audit_since(&AuditReadRequest::new(0)).unwrap() };
        assert!(
            !rows.iter().any(|r| r.action == "health"),
            "Health is a peek: it must NOT write an audit row; got {rows:?}"
        );
        handle.shutdown().await;
        cleanup(&data);
    });
}
```

> If `build_server` does not return the state, add a `build_server_with_state` helper that returns `(Arc<DaemonState>, ServerHandle)`.

- [ ] **Step 2: Run to verify it fails**

Run on WSL: `wsl -e bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commanderd --features test-util --test ipc_roundtrip health_returns_idle"`
Expected: FAIL — `IpcResponse::Health` has no `idle_secs`; and/or an audit row IS written.

- [ ] **Step 3: Implement**

(a) `protocol.rs:208` — change the Health variant:
```rust
Health {
    uptime_secs: u64,
    /// Seconds since the last real IPC request. Optional for backward
    /// compatibility: a legacy daemon omits it and the client treats idle as
    /// unknown (see ensure::probe_endpoint lenient decode).
    #[serde(default)]
    idle_secs: Option<u64>,
},
```

(b) `server.rs` Health handler (~L390) — populate idle:
```rust
IpcRequest::Health => {
    let r = IpcResponse::Health {
        uptime_secs: boot.elapsed().as_secs(),
        idle_secs: Some(state.idle_secs()),
    };
    ("health", IpcResult::Ok { response: r })
}
```

(c) In `dispatch` (server.rs ~L379), after computing `(method_name, result)`, bump activity for every request EXCEPT Health:
```rust
if !matches!(&req_env.request, IpcRequest::Health) {
    state.bump_activity();
}
```

(d) The audit emission (~L553-562): guard it so Health is not audited:
```rust
if method_name != "health" {
    let subject = identity_audit_subject(peer);
    let decision = if matches!(response_result, IpcResult::Ok { .. }) { "info" } else { "error" };
    emit_audit(state, method_name, &subject, decision, None, peer);
}
```

- [ ] **Step 4: Run to verify it passes**

Run on WSL: `wsl -e bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commanderd --features test-util --test ipc_roundtrip"`
Expected: PASS (new test + existing health round-trip).

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/ipc/protocol.rs crates/daemon/src/ipc/server.rs crates/daemon/tests/ipc_roundtrip.rs
git commit -m "feat(daemon): Health returns idle_secs; non-Health bumps activity; Health is audit-free peek"
```

---

## Phase D — Liveness handshake in probe

### Task D1: `probe_endpoint` does a real Health handshake

**Files:**
- Modify: `crates/supervisor/src/ensure.rs:289-307`
- Test: `crates/supervisor/tests/` (new integration: a non-daemon listener is NOT accepted as our daemon)

- [ ] **Step 1: Write the failing test**

Create `crates/supervisor/tests/probe_handshake.rs` (`#![cfg(unix)]`):
```rust
#![cfg(unix)]
//! probe_endpoint must require a well-formed Health response, not just a
//! connectable socket — a non-daemon listener is NOT "our daemon".

use std::path::PathBuf;
use terminal_commander_supervisor::ensure::{probe_endpoint, Endpoint};

#[tokio::test]
async fn connectable_non_daemon_socket_is_not_our_daemon() {
    let dir = std::env::temp_dir().join(format!("tc-probe-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("x.sock");
    // A dumb listener that accepts + closes (no Health response).
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    tokio::spawn(async move { while let Ok((s, _)) = listener.accept().await { drop(s); } });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let ep = Endpoint::UnixSocket { path: sock.clone() };
    assert!(
        !probe_endpoint(&ep).await,
        "a socket that connects but never answers Health must NOT count as our daemon"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Run to verify it fails**

Run on WSL: `wsl -e bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commander-supervisor --test probe_handshake"`
Expected: FAIL — current `probe_endpoint` is connect-only, returns `true` for any connectable socket.

- [ ] **Step 3: Implement**

Rewrite `probe_endpoint` in `crates/supervisor/src/ensure.rs` to connect, send a `Health` request frame, and require a well-formed Health response (lenient decode tolerates missing `idle_secs`). Reuse the existing client/framing used by `DaemonClient` (the supervisor already depends on the protocol crate). Pseudocode → real:
```rust
pub async fn probe_endpoint(ep: &Endpoint) -> bool {
    // connect with a short timeout (existing connect logic), then:
    let Ok(Some(resp)) = send_health_with_timeout(ep, Duration::from_millis(500)).await else {
        return false;
    };
    // Accept ANY well-formed Health response (idle_secs may be absent on a
    // legacy daemon). Anything else (garbage / wrong variant / timeout) = not ours.
    matches!(resp, IpcResponse::Health { .. })
}
```
Implement `send_health_with_timeout` using the same length-prefixed JSON framing as `DaemonClient::call`, decoding the response into `IpcResponse` with serde (the `#[serde(default)]` on `idle_secs` makes a legacy daemon's response parse).

> The Windows named-pipe branch keeps its existing connect semantics, then performs the same send/recv. Reuse the daemon-client transport rather than re-implementing framing.

- [ ] **Step 4: Run to verify it passes**

Run on WSL: `wsl -e bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commander-supervisor --test probe_handshake && cargo test -p terminal-commander-supervisor"`
Expected: PASS. Also run the daemon's own ensure/roundtrip tests to confirm a REAL daemon still probes as available.

- [ ] **Step 5: Commit**

```bash
git add crates/supervisor/src/ensure.rs crates/supervisor/tests/probe_handshake.rs
git commit -m "feat(supervisor): probe_endpoint requires a real Health handshake (closes impersonation gap)"
```

---

## Phase E — Shutdown protocol + self-reap + graceful drain

### Task E1: Add the Shutdown protocol variants

**Files:**
- Modify: `crates/daemon/src/ipc/protocol.rs` (IpcRequest:185, IpcResponse:238, IpcErrorCode:~354)
- Test: a serde round-trip unit test in `protocol.rs`

- [ ] **Step 1: Write the failing test**

Add to `protocol.rs` tests:
```rust
#[test]
fn shutdown_variants_serde_roundtrip() {
    let req = IpcRequest::Shutdown;
    let s = serde_json::to_string(&req).unwrap();
    assert_eq!(serde_json::from_str::<IpcRequest>(&s).unwrap(), req);

    let resp = IpcResponse::ShutdownAck { draining: true };
    let s = serde_json::to_string(&resp).unwrap();
    match serde_json::from_str::<IpcResponse>(&s).unwrap() {
        IpcResponse::ShutdownAck { draining } => assert!(draining),
        other => panic!("unexpected: {other:?}"),
    }
    // The new error code serializes.
    let _ = serde_json::to_string(&IpcErrorCode::ShuttingDown).unwrap();
}
```
(`IpcRequest` needs `PartialEq` for the assert; if it lacks it, compare the serialized strings instead.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p terminal-commanderd --lib protocol::tests::shutdown_variants`
Expected: FAIL — variants don't exist (compile error).

- [ ] **Step 3: Implement**

(a) `IpcRequest` (add before the closing brace at L185):
```rust
    /// Request a graceful shutdown. The daemon ACKs immediately
    /// (`ShutdownAck`), stops accepting new connections, drains in-flight
    /// requests, removes its pidfile, and exits 0. New connections during the
    /// drain receive `ShuttingDown` (retryable).
    Shutdown,
```
(b) `IpcResponse` (add at L238):
```rust
    /// Ack for `Shutdown`. `draining=true` once the daemon has stopped accepting
    /// new connections and begun draining.
    ShutdownAck { draining: bool },
```
(c) `IpcErrorCode` (add a variant):
```rust
    /// Returned to a new request that arrives while the daemon is draining for
    /// shutdown. Retryable: the client should cold-spawn a fresh daemon.
    ShuttingDown,
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p terminal-commanderd --lib protocol::tests::shutdown_variants`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/ipc/protocol.rs
git commit -m "feat(daemon): add Shutdown request, ShutdownAck response, ShuttingDown error code"
```

### Task E2: Shutdown dispatch + runtime shutdown source + graceful drain

**Files:**
- Modify: `crates/daemon/src/ipc/server.rs` (dispatch arm + in-flight tracking + drain)
- Modify: `crates/daemon/src/runtime.rs:249-323` (select! the internal shutdown source alongside OS signal)
- Test: `crates/daemon/tests/` integration — Shutdown IPC makes the daemon exit + remove pidfile

- [ ] **Step 1: Write the failing test**

Create `crates/daemon/tests/shutdown_ipc.rs` (`#![cfg(unix)]`):
```rust
#![cfg(unix)]
// ... standard tmp_data_dir + rt + build_server helpers (copy the file-header
//     pattern with the AtomicU64 counter from e.g. ipc_command.rs) ...

#[test]
fn shutdown_request_acks_then_daemon_exits_and_removes_pidfile() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("shutdown");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        let resp = client.call(1, IpcRequest::Shutdown).await.expect("shutdown");
        match resp {
            IpcResponse::ShutdownAck { draining } => assert!(draining),
            other => panic!("unexpected: {other:?}"),
        }
        // Endpoint goes unreachable within a bounded wait.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut down = false;
        while std::time::Instant::now() < deadline {
            if !terminal_commander_supervisor::ensure::probe_endpoint(
                &endpoint_from(&handle)).await { down = true; break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(down, "daemon must become unreachable after Shutdown");
        cleanup(&data);
    });
}
```
(`endpoint_from(&handle)` builds an `Endpoint` from the handle's socket path — add a small test helper.)

- [ ] **Step 2: Run to verify it fails**

Run on WSL: `wsl -e bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commanderd --features test-util --test shutdown_ipc"`
Expected: FAIL — `Shutdown` is not dispatched; daemon does not exit.

- [ ] **Step 3: Implement**

(a) `DaemonState` (or the server) gains a shutdown trigger: a `tokio::sync::Notify` (or a `watch` channel) `shutdown_signal`. Add `pub fn trigger_shutdown(&self)` that notifies it, and expose a `shutdown_notified()` future.

(b) `server.rs` dispatch — add the arm (early, before the bump/audit logic; Shutdown should NOT be audited-as-command but a shutdown audit row is fine):
```rust
IpcRequest::Shutdown => {
    state.trigger_shutdown();
    ("shutdown", IpcResult::Ok { response: IpcResponse::ShutdownAck { draining: true } })
}
```

(c) In-flight drain: track spawned connection tasks. Replace the bare `tokio::spawn(handle_connection(...))` (server.rs:195) with a `tokio::task::JoinSet` (or an `AtomicUsize` in-flight counter + a `Notify` when it hits zero) stored on the server. On shutdown: stop the accept loop, then `join_set.join_all().await` (bounded by a ceiling) before returning. New connections that arrive after the shutdown trigger get `IpcError { code: ShuttingDown, .. }` and close.

(d) `runtime.rs` (~L249) — `select!` the internal shutdown alongside the OS signal:
```rust
tokio::select! {
    r = wait_for_shutdown_signal() => { r?; }
    () = state.shutdown_notified() => { tracing::info!("internal shutdown (idle-reap or Shutdown IPC)"); }
}
tracing::info!("shutdown: draining...");
handle.shutdown().await;   // now also joins in-flight conn tasks
terminal_commander_supervisor::pidfile::remove_pidfile(&state_dir);
```

- [ ] **Step 4: Run to verify it passes**

Run on WSL: `wsl -e bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commanderd --features test-util --test shutdown_ipc"`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/ipc/server.rs crates/daemon/src/runtime.rs crates/daemon/src/state.rs crates/daemon/tests/shutdown_ipc.rs
git commit -m "feat(daemon): Shutdown IPC triggers graceful drain + exit; in-flight tracking via JoinSet"
```

### Task E3: Idle self-reap timer

**Files:**
- Modify: `crates/daemon/src/config.rs` (read `TC_IDLE_TTL_SECS`)
- Modify: `crates/daemon/src/runtime.rs` (idle-timer task that triggers shutdown)
- Test: `crates/daemon/tests/` integration with a tiny TTL

- [ ] **Step 1: Write the failing test**

Add to `crates/daemon/tests/shutdown_ipc.rs`:
```rust
#[test]
fn daemon_self_reaps_after_idle_ttl() {
    // Build the server with TC_IDLE_TTL_SECS=1 in its config, no IPC after start.
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("idle");
        let (_state, handle) = build_server_with_idle_ttl(&data, 1); // 1-second TTL
        // No IPC at all. Within ~a few seconds the daemon must self-exit.
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut down = false;
        while std::time::Instant::now() < deadline {
            if !terminal_commander_supervisor::ensure::probe_endpoint(&endpoint_from(&handle)).await { down = true; break; }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(down, "daemon must self-reap after idle TTL with no IPC");
        cleanup(&data);
    });
}
```
(`build_server_with_idle_ttl` sets the config's idle ttl; the idle-timer tick should be small in tests — e.g. derive tick = min(TTL/2, 1s) so a 1s TTL is observable within the 8s window.)

- [ ] **Step 2: Run to verify it fails**

Run on WSL: `wsl -e bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commanderd --features test-util --test shutdown_ipc daemon_self_reaps"`
Expected: FAIL — no idle timer exists; daemon never exits.

- [ ] **Step 3: Implement**

(a) `config.rs` — add `idle_ttl_secs` to `DaemonConfig` (read `TC_IDLE_TTL_SECS`, default 1800, `0` disables). Parse like the existing env reads.

(b) `runtime.rs` — spawn an idle-timer task before the `select!`:
```rust
let ttl = cfg.daemon.idle_ttl_secs; // 0 = disabled
if ttl > 0 {
    let st = Arc::clone(&state);
    let tick = std::time::Duration::from_secs((ttl / 2).max(1).min(60));
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(tick);
        loop {
            iv.tick().await;
            if st.idle_secs() >= ttl {
                tracing::info!("idle-reap: no IPC for {}s (TTL {}s), shutting down", st.idle_secs(), ttl);
                st.trigger_shutdown();
                break;
            }
        }
    });
}
```
(The `trigger_shutdown` reuses the E2 path → graceful drain → pidfile removal → exit.)

- [ ] **Step 4: Run to verify it passes**

Run on WSL: same test command.
Expected: PASS. Also confirm `TC_IDLE_TTL_SECS=0` keeps a daemon alive (add a quick assertion or a separate test).

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/config.rs crates/daemon/src/runtime.rs crates/daemon/tests/shutdown_ipc.rs
git commit -m "feat(daemon): idle self-reap timer (TC_IDLE_TTL_SECS, default 1800, 0=off)"
```

---

## Phase F — `session list|reap` CLI

### Task F1: `session list`

**Files:**
- Modify: `crates/cli/src/main.rs:39-73` (Command enum + handler)
- Test: `crates/cli/tests/` (a list over a fabricated base dir)

- [ ] **Step 1: Write the failing test**

Create `crates/cli/tests/session_list.rs`:
```rust
// Drive the CLI binary with TC_DATA pointed at a fabricated base dir holding
// pidfiles, assert `session list` prints the default + seeded labels.
use std::process::Command;

#[test]
fn session_list_shows_default_and_seeded() {
    let base = std::env::temp_dir().join(format!("tc-cli-list-{}", std::process::id()));
    // write a base pidfile + a seeded subdir pidfile (use the supervisor pidfile
    // writer via a tiny helper binary, OR write the JSON directly).
    write_pidfile_json(&base, 999_999_999, "0.1.0", "base.sock");
    write_pidfile_json(&base.join("agent-1"), 999_999_998, "0.1.0", "agent.sock");

    let out = Command::new(env!("CARGO_BIN_EXE_terminal-commander"))
        .args(["session", "list"])
        .env("TC_DATA", &base)
        .env_remove("TC_SOCKET").env_remove("TC_SESSION")
        .output().expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("default"), "must list default session: {stdout}");
    assert!(stdout.contains("agent-1"), "must list seeded session: {stdout}");
    let _ = std::fs::remove_dir_all(&base);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p terminal-commander-cli --test session_list`
Expected: FAIL — `session` subcommand does not exist (CLI errors / unknown subcommand).

- [ ] **Step 3: Implement**

(a) `main.rs` Command enum — add:
```rust
    /// Per-harness session management (list / reap).
    Session {
        #[command(subcommand)]
        op: SessionOp,
    },
```
```rust
#[derive(Debug, clap::Subcommand)]
enum SessionOp {
    /// List sessions (default + seeded) with liveness + idle.
    List,
    /// Reap sessions (graceful shutdown; force-kill wedged daemons).
    Reap {
        /// A specific session token.
        token: Option<String>,
        #[arg(long)] all: bool,
        #[arg(long)] idle: bool,
        #[arg(long, default_value_t = 1800)] idle_secs: u64,
    },
}
```
(b) Handler `Command::Session { op } => run_session(op)`. `run_session(List)`:
```rust
fn run_session(op: SessionOp) -> std::process::ExitCode {
    let base = terminal_commander_supervisor::paths::resolve_state_dir_base(); // base, ignoring TC_SESSION
    match op {
        SessionOp::List => {
            let entries = terminal_commander_supervisor::sessions::enumerate(&base);
            println!("{:<18} {:>8} {:<12} {:<6} ENDPOINT", "SESSION", "PID", "STATE", "IDLE");
            // For each ALIVE entry, run the handshake concurrently for idle_secs
            // (probe + Health peek); STALE entries print state=stale, idle=-.
            // (build a tokio runtime here to run the concurrent handshakes)
            // ... render rows ...
            std::process::ExitCode::SUCCESS
        }
        SessionOp::Reap { .. } => run_session_reap(/* ... */),
    }
}
```
> `resolve_state_dir_base` must resolve the BASE dir (ignoring `TC_SESSION`) so `list` sees all sessions. If `paths` only exposes `resolve_state_dir_with` (which applies the token), add a `pub fn state_dir_base_from(env)` wrapper around the existing private `state_dir_base`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p terminal-commander-cli --test session_list`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/main.rs crates/cli/tests/session_list.rs crates/supervisor/src/paths.rs
git commit -m "feat(cli): session list (enumerate default + seeded sessions)"
```

### Task F2: `session reap` (graceful + identity-gated force fallback)

**Files:**
- Modify: `crates/cli/src/main.rs` (`run_session_reap`)
- Modify: client transport for a `Shutdown` call (reuse `DaemonClient::call`)
- Test: `crates/cli/tests/session_reap.rs` (`#![cfg(unix)]`, spawns a real daemon, reaps it)

- [ ] **Step 1: Write the failing test**

```rust
#![cfg(unix)]
// Spawn a real daemon under a seeded TC_SESSION, then `session reap <token>`,
// assert the daemon exits + pidfile removed.
#[test]
fn session_reap_token_shuts_down_the_daemon() { /* spawn daemon, reap, assert gone */ }
```

- [ ] **Step 2: Run to verify it fails**

Run on WSL: `wsl -e bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commander-cli --test session_reap"`
Expected: FAIL — reap not implemented.

- [ ] **Step 3: Implement `run_session_reap`**

```
for each target ALIVE session (selected by token / --all / --idle threshold):
  resolve its endpoint (from the pidfile)
  send IpcRequest::Shutdown via DaemonClient::call
  on ShutdownAck: wait (bounded) for probe_endpoint to go false
  if still reachable AND no ACK was received (wedged):
     if pid_belongs_to_daemon(pid, state_dir): hard_kill(pid)
     else: eprintln "endpoint occupied by non-daemon, refusing"; continue
  cleanup_stale(state_dir, pid) for dead entries
report a one-line outcome per session.
```
`--idle` selects sessions whose handshake `idle_secs >= idle_secs flag`. Reuse `pid_belongs_to_daemon` + `hard_kill` from `supervisor::replace`.

- [ ] **Step 4: Run to verify it passes**

Run on WSL: same command.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/main.rs crates/cli/tests/session_reap.rs
git commit -m "feat(cli): session reap (graceful shutdown-IPC, identity-gated force fallback)"
```

---

## Phase G — WSLENV TC-only allowlist

### Task G1: Rebuild WSLENV from the TC allowlist

**Files:**
- Modify: `packages/terminal-commander/lib/wsl/spawn.js` (`ensureSessionInWslEnv`)
- Test: `packages/terminal-commander/test/wsl-spawn.test.js`

- [ ] **Step 1: Write the failing test**

Add to `wsl-spawn.test.js`:
```js
test("WSLENV is rebuilt TC-only: ambient entries (incl. credentials) are dropped", async () => {
  const rec = makeRecorder();
  await spawnWslBridge({
    platform: "win32",
    env: {
      PATH: "C:\\Windows",
      TC_SESSION: "tc-abcdef012345",
      WSLENV: "WSL_SUDO_CREDENTIAL/u:SOME_OTHER/p", // ambient must be dropped
    },
    detect: makeMockDetect(okDetect(["Ubuntu"], "Ubuntu")),
    doctor: makeMockDoctor(DOCTOR_STATUSES.RUNTIME_PRESENT),
    exec: rec.exec,
    returnInsteadOfMirror: true,
  });
  const w = rec.calls[0].env.WSLENV;
  assert.equal(w, "TC_SESSION/u", `WSLENV must be exactly TC_SESSION/u, got ${w}`);
  assert.doesNotMatch(w, /WSL_SUDO_CREDENTIAL/, "credential must NOT cross");
  assert.doesNotMatch(w, /SOME_OTHER/, "ambient entries must be dropped");
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd packages/terminal-commander && node --test test/wsl-spawn.test.js`
Expected: FAIL — current `ensureSessionInWslEnv` preserves ambient entries (would yield `WSL_SUDO_CREDENTIAL/u:SOME_OTHER/p:TC_SESSION/u`).

- [ ] **Step 3: Implement**

Replace `ensureSessionInWslEnv` body:
```js
function ensureSessionInWslEnv(filteredEnv) {
  const out = { ...filteredEnv };
  if (out.TC_SESSION == null || out.TC_SESSION === "") {
    // No session token -> no WSLENV from us. Drop any ambient WSLENV too: the
    // bridge forwards nothing the WSL runtime needs (distro is host-side), and
    // ambient entries could forward credentials (e.g. WSL_SUDO_CREDENTIAL).
    delete out.WSLENV;
    return out;
  }
  // TC-only allowlist: TC_SESSION/u and nothing else. Ambient WSLENV dropped.
  out.WSLENV = "TC_SESSION/u";
  return out;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd packages/terminal-commander && node --test test/wsl-spawn.test.js`
Expected: PASS (this test + the existing WSLENV tests, updated: the prior "preserve ambient" assertions must be replaced by "ambient dropped" — update those tests in this step).

> Update the earlier `WSLENV preserves any pre-existing entries` and `non-/u flag corrected` tests: under the allowlist, ambient is now DROPPED, so those tests assert `WSLENV === "TC_SESSION/u"`.

- [ ] **Step 5: Commit**

```bash
git add packages/terminal-commander/lib/wsl/spawn.js packages/terminal-commander/test/wsl-spawn.test.js
git commit -m "feat(wsl): rebuild WSLENV from TC-only allowlist (drop ambient; closes WSL_SUDO_CREDENTIAL leak)"
```

---

## Phase H — E2E smoke + final gate

### Task H1: Extend the session-isolation smoke with list/reap

**Files:**
- Modify: `scripts/smoke/verify-session-isolation-smoke.ps1`

- [ ] **Step 1: Add list/reap coverage**

After the existing two-token isolation check, add: run `terminal-commander session list` (with `TC_DATA` pointed at the test base), assert both session tokens appear; run `terminal-commander session reap --all`; assert both pipes gone. (Mirror the existing PASS/FAIL + cleanup structure.)

- [ ] **Step 2: Run it**

Run: `& "C:\AI_STUFF\PROGRAMMING\terminal-commander\scripts\smoke\verify-session-isolation-smoke.ps1"`
Expected: `E2E-RESULT: PASS`, exit 0, self-cleaning.

- [ ] **Step 3: Commit**

```bash
git add scripts/smoke/verify-session-isolation-smoke.ps1
git commit -m "test(e2e): extend session smoke with list + reap coverage"
```

### Task H2: Full-workspace verification gate

- [ ] **Step 1: Rust workspace (WSL, 3x for flake)**

Run: `wsl -e bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && for i in 1 2 3; do cargo test --workspace --features test-util >/tmp/r$i.log 2>&1; echo run$i:$?; grep -c 'test result: ok' /tmp/r$i.log; done"`
Expected: each run exit 0, no `FAILED`.

- [ ] **Step 2: Windows cfg(windows) + clippy + fmt**

Run: `cargo test -p terminal-commanderd -p terminal-commander-supervisor -p terminal-commander-cli` (Windows), `cargo clippy --workspace --all-targets --features test-util -- -D warnings`, `cargo fmt --check`.
Expected: green; clippy zero warnings; fmt clean on touched files (fix only touched files; do not sweep pre-existing drift).

- [ ] **Step 3: JS**

Run: `cd packages/terminal-commander && npm test`
Expected: all pass.

- [ ] **Step 4: cargo clean (per global rule)**

Run: `cargo clean`

---

## Self-Review (completed against the spec)

- **Spec coverage:** Unit 1 (enumeration) → Phase B; Unit 2 (self-reap + drain) → Phase C (last_activity) + E (Shutdown/drain/idle-timer); Unit 3 (handshake + idle_secs + non-bumping audit-free peek) → Phase C + D; Unit 4 (CLI + Shutdown protocol + WSLENV) → Phases E/F/G; `default` reservation → Phase A; threat-model items (impersonation → D; WSLENV → G) covered; testing → per-task + Phase H. No spec requirement left without a task.
- **Type consistency:** `IpcResponse::Health { uptime_secs, idle_secs: Option<u64> }`, `IpcResponse::ShutdownAck { draining: bool }`, `IpcErrorCode::ShuttingDown`, `IpcRequest::Shutdown`, `SessionEntry { label, state_dir, pid, version, endpoint, alive }`, `enumerate`, `cleanup_stale`, `read_pidfile_raw`, `bump_activity`/`idle_secs`, `trigger_shutdown`/`shutdown_notified`, `ensureSessionInWslEnv` — used consistently across tasks.
- **Net-new acknowledged (not "reuse"):** Shutdown protocol trio, idle_secs wire field, in-flight drain (JoinSet), raw pidfile parser, default reservation, idle timer, WSLENV rebuild.
- **Unknowns to confirm at execution (flagged, not placeholders):** exact `DaemonState` location/constructor (Phase C — follow `Arc<DaemonState>`); whether `paths` needs a `state_dir_base_from` public wrapper (Phase F); the precise framing reuse for `send_health_with_timeout` (Phase D — reuse `DaemonClient` transport). Each task says how to resolve.
