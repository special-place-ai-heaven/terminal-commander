# Per-Harness Session Endpoint (F1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the daemon endpoint per-harness (via an opaque `TC_SESSION` token) instead of per-OS-user, so multiple agents on one machine each get their own daemon, while the unseeded default stays byte-identical to today on both platforms.

**Architecture:** One token-resolution function in `crates/supervisor/src/session.rs` (new) computes a sanitized session token from an injected `EnvSource` with precedence `TC_SOCKET` (full override) > `TC_SESSION` (token) > per-user default. The existing `resolve_state_dir_with` / `resolve_socket_path_with` (supervisor) and `DaemonConfig::pipe_name` / `socket_path` (daemon) route through it, so daemon-bind and client-connect compute identical endpoints. Reuses the F2 `EnvSource` seam for race-free tests.

**Tech Stack:** Rust (workspace, edition 2024, stable 1.95). `cfg(windows)` named-pipe code is invisible to the WSL/Linux build, so Windows-specific tasks verify on Windows; cross-platform tasks verify under WSL for 1.95 lint parity.

**Spec:** `docs/superpowers/specs/2026-05-27-per-harness-session-endpoint-design.md`

---

## File Structure

| File | Responsibility | Create/Modify |
|---|---|---|
| `crates/supervisor/src/session.rs` | Token resolution + sanitization; the single source of truth both sides call | Create |
| `crates/supervisor/src/lib.rs` | Register `pub mod session;` + re-export | Modify |
| `crates/supervisor/src/paths.rs` | Route `resolve_state_dir_with` + `resolve_socket_path_with` through the token | Modify |
| `crates/daemon/src/config.rs` | Route `pipe_name()` through the token (Windows) | Modify |

Unix socket already derives from `data_dir` (config.rs `socket_path()`), so session-keying the state dir auto-keys the unix socket — `socket_path()` is NOT modified. Only the Windows `pipe_name()` needs explicit token routing.

---

## Task 1: Session token resolution module

**Files:**
- Create: `crates/supervisor/src/session.rs`
- Modify: `crates/supervisor/src/lib.rs`

The token has three states the callers care about: a full-socket override (`TC_SOCKET`), an explicit session token, or the per-user default. Model that as an enum so callers branch correctly (the default must NOT get a `/{token}` subdir on unix nor a renamed pipe on Windows).

- [ ] **Step 1: Write the failing test**

Create `crates/supervisor/src/session.rs` with ONLY the tests first (the types they reference come in Step 3):

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Per-harness session identity. Resolves an opaque session token from the
// environment with precedence TC_SOCKET > TC_SESSION > per-user default, and
// sanitizes TC_SESSION against pipe-squat / path-traversal. Both the daemon
// (at bind) and clients (mcp/cli at connect) resolve through here so they
// compute identical endpoints with no coordination.
//
// See docs/superpowers/specs/2026-05-27-per-harness-session-endpoint-design.md

use crate::paths::EnvSource;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeEnv(HashMap<String, String>);
    impl FakeEnv {
        fn new() -> Self {
            Self(HashMap::new())
        }
        fn with(mut self, k: &str, v: &str) -> Self {
            self.0.insert(k.to_owned(), v.to_owned());
            self
        }
    }
    impl EnvSource for FakeEnv {
        fn get(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    #[test]
    fn tc_socket_wins_as_full_override() {
        let env = FakeEnv::new()
            .with("TC_SOCKET", "/custom/x.sock")
            .with("TC_SESSION", "abc");
        assert_eq!(
            resolve_session(&env),
            SessionEndpoint::FullOverride("/custom/x.sock".into())
        );
    }

    #[test]
    fn tc_session_selects_token_when_no_socket() {
        let env = FakeEnv::new().with("TC_SESSION", "agent-1");
        assert_eq!(
            resolve_session(&env),
            SessionEndpoint::Session("agent-1".to_owned())
        );
    }

    #[test]
    fn unseeded_is_per_user_default() {
        let env = FakeEnv::new();
        assert_eq!(resolve_session(&env), SessionEndpoint::Default);
    }

    #[test]
    fn empty_values_are_treated_as_unset() {
        let env = FakeEnv::new().with("TC_SOCKET", "").with("TC_SESSION", "");
        assert_eq!(resolve_session(&env), SessionEndpoint::Default);
    }

    #[test]
    fn malformed_session_falls_back_to_default() {
        for bad in [
            "../evil",
            r"a\b",
            "a/b",
            r"\\.\pipe\x",
            "has space",
            &"x".repeat(65),
        ] {
            let env = FakeEnv::new().with("TC_SESSION", bad);
            assert_eq!(
                resolve_session(&env),
                SessionEndpoint::Default,
                "malformed token {bad:?} must fall back to Default"
            );
        }
    }

    #[test]
    fn well_formed_session_is_accepted() {
        for ok in ["agent-1", "abc.def", "A_B-9", &"x".repeat(64)] {
            let env = FakeEnv::new().with("TC_SESSION", ok);
            assert_eq!(
                resolve_session(&env),
                SessionEndpoint::Session(ok.to_owned()),
                "well-formed token {ok:?} must be accepted"
            );
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commander-supervisor session 2>&1 | tail -20'`
Expected: FAIL — `cannot find type SessionEndpoint` / `cannot find function resolve_session` (and `pub mod session;` not yet registered, so also a module error until Step 4; that is expected at this step).

- [ ] **Step 3: Write the minimal implementation**

Insert ABOVE the `#[cfg(test)]` line in `crates/supervisor/src/session.rs`:

```rust
/// Maximum length of a sanitized `TC_SESSION` token.
const MAX_SESSION_TOKEN_LEN: usize = 64;

/// Resolved session intent, in precedence order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEndpoint {
    /// `TC_SOCKET` set: use this verbatim as the full endpoint.
    FullOverride(String),
    /// `TC_SESSION` set and well-formed: per-harness token.
    Session(String),
    /// Nothing set (or malformed): per-user default, byte-identical to pre-F1.
    Default,
}

/// True iff `token` is a safe session id: `[A-Za-z0-9._-]`, length 1..=64,
/// no path separators / pipe prefixes / `..` (rejects pipe-squat + traversal).
#[must_use]
pub fn is_valid_session_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_SESSION_TOKEN_LEN
        && token != ".."
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Resolve session intent from the environment. Precedence:
/// `TC_SOCKET` (full override) > `TC_SESSION` (token) > per-user default.
/// A malformed `TC_SESSION` soft-fails to [`SessionEndpoint::Default`].
#[must_use]
pub fn resolve_session(env: &impl EnvSource) -> SessionEndpoint {
    if let Some(sock) = env.get("TC_SOCKET").filter(|s| !s.is_empty()) {
        return SessionEndpoint::FullOverride(sock);
    }
    if let Some(tok) = env.get("TC_SESSION").filter(|s| !s.is_empty()) {
        if is_valid_session_token(&tok) {
            return SessionEndpoint::Session(tok);
        }
        eprintln!(
            "terminal-commander: ignoring malformed TC_SESSION (must be \
             [A-Za-z0-9._-], 1..=64 chars); using per-user default"
        );
    }
    SessionEndpoint::Default
}
```

NOTE: `..` is rejected both by the explicit `token != ".."` guard and because `.` chars alone (`..`) pass the char-class check — the explicit guard is what blocks it. A token like `a..b` is allowed (no separator, harmless as a pipe/dir name component).

- [ ] **Step 4: Register the module**

Modify `crates/supervisor/src/lib.rs` — add alongside the existing `pub mod` lines (the file already has `pub mod paths;` etc.):

```rust
pub mod session;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commander-supervisor session 2>&1 | grep -E "test result|FAILED|error\["'`
Expected: PASS — `test result: ok. 6 passed` (the 6 session tests).

- [ ] **Step 6: Clippy (WSL, cross-platform module)**

Run: `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo clippy -p terminal-commander-supervisor --all-targets -- -D warnings 2>&1 | tail -4'`
Expected: `Finished` with no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/supervisor/src/session.rs crates/supervisor/src/lib.rs
git commit -m "feat(supervisor): session token resolution (F1 task 1)"
```

---

## Task 2: Route the unix state dir through the session token

**Files:**
- Modify: `crates/supervisor/src/paths.rs:57-80` (`resolve_state_dir_with`)

Spec resolution detail #1: `TC_DATA` relocates the base; the session subdir hangs *under* it. So the current early-return on `TC_DATA` (L58) must be reworked: resolve the base (TC_DATA or platform default), THEN append `/{token}` only for an explicit session.

- [ ] **Step 1: Write the failing test**

Add to the `tests` mod in `crates/supervisor/src/paths.rs` (the `FakeEnv` helper already exists there):

```rust
    #[cfg(unix)]
    #[test]
    fn explicit_session_gets_subdir_under_base() {
        let env = FakeEnv::new()
            .with("HOME", "/test-home")
            .with("TC_SESSION", "agent-1");
        assert_eq!(
            resolve_state_dir_with(&env),
            std::path::PathBuf::from("/test-home/.local/share/terminal-commanderd/agent-1"),
            "explicit session appends /{{token}} under the default base"
        );
    }

    #[cfg(unix)]
    #[test]
    fn session_subdir_hangs_under_tc_data_base() {
        let env = FakeEnv::new()
            .with("TC_DATA", "/custom/root")
            .with("TC_SESSION", "agent-1");
        assert_eq!(
            resolve_state_dir_with(&env),
            std::path::PathBuf::from("/custom/root/agent-1"),
            "TC_DATA relocates the base; session subdir hangs under it"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unseeded_state_dir_is_byte_identical_to_pre_f1() {
        let env = FakeEnv::new().with("HOME", "/test-home");
        assert_eq!(
            resolve_state_dir_with(&env),
            std::path::PathBuf::from("/test-home/.local/share/terminal-commanderd"),
            "default (no TC_SESSION) must NOT add any subdir"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commander-supervisor explicit_session_gets_subdir session_subdir_hangs unseeded_state_dir 2>&1 | grep -E "test result|FAILED|assertion"'`
Expected: FAIL — `explicit_session_gets_subdir` and `session_subdir_hangs` fail (no subdir appended yet); `unseeded_state_dir...` passes (current behavior already correct).

- [ ] **Step 3: Rewrite `resolve_state_dir_with`**

Replace the body at `crates/supervisor/src/paths.rs:57-80`:

```rust
#[must_use]
pub fn resolve_state_dir_with(env: &impl EnvSource) -> PathBuf {
    let base = state_dir_base(env);
    match crate::session::resolve_session(env) {
        // A full TC_SOCKET override does not affect the state dir base.
        crate::session::SessionEndpoint::Session(token) => base.join(token),
        _ => base,
    }
}

/// The state-dir base before any per-session subdir: `TC_DATA`, else the
/// platform default. Byte-identical to the pre-F1 `resolve_state_dir_with`.
fn state_dir_base(env: &impl EnvSource) -> PathBuf {
    if let Some(p) = env.get("TC_DATA").filter(|s| !s.is_empty()) {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    {
        if let Some(p) = env.get("LOCALAPPDATA") {
            return PathBuf::from(p).join("terminal-commanderd").join("state");
        }
    }
    #[cfg(unix)]
    {
        // Do NOT consult XDG_STATE_HOME — daemon ignores it.
        if let Some(p) = env.get("HOME") {
            return PathBuf::from(p)
                .join(".local")
                .join("share")
                .join("terminal-commanderd");
        }
    }
    std::env::temp_dir()
        .join("terminal-commanderd")
        .join("state")
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commander-supervisor 2>&1 | grep -E "test result|FAILED|error\["'`
Expected: PASS — all supervisor tests green (the 3 new unix tests + existing).

- [ ] **Step 5: Verify pidfile/DB/log co-locate under the session subdir (spec detail #3)**

Pidfile (`pidfile.rs:28` = `state_dir.join(...)`), DB (`config.rs:246` = `data_dir.join(...)`), and logs (`paths.rs:141` = `<state_dir>/logs/...`) all derive from the resolved state dir, and the daemon's `data_dir` comes from `default_state_dir()` → `resolve_state_dir()` (`config.rs:347`). So they follow the session subdir automatically. Pin it with a test in the `tests` mod of `crates/supervisor/src/paths.rs`:

```rust
    #[cfg(unix)]
    #[test]
    fn session_state_pidfile_log_socket_all_co_locate() {
        let env = FakeEnv::new()
            .with("HOME", "/test-home")
            .with("TC_SESSION", "agent-1");
        let state = resolve_state_dir_with(&env);
        let expected = std::path::PathBuf::from(
            "/test-home/.local/share/terminal-commanderd/agent-1",
        );
        assert_eq!(state, expected, "state dir is the session subdir");
        // pidfile + log + socket are all <state>/...; assert they nest under it.
        assert_eq!(crate::pidfile::pidfile_path(&state), expected.join("terminal-commanderd.pid"));
        assert_eq!(resolve_log_path_with(&env), expected.join("logs").join("terminal-commanderd.log"));
        assert_eq!(resolve_socket_path_with(&env), expected.join("terminal-commanderd.sock"));
    }
```

Run: `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commander-supervisor session_state_pidfile_log_socket 2>&1 | grep -E "test result|FAILED|assertion"'`
Expected: PASS — all four paths nest under the session subdir.

- [ ] **Step 6: Clippy (WSL)**

Run: `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo clippy -p terminal-commander-supervisor --all-targets -- -D warnings 2>&1 | tail -4'`
Expected: `Finished`, no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/supervisor/src/paths.rs
git commit -m "feat(supervisor): per-session unix state dir subdir (F1 task 2)"
```

---

## Task 3: Route the Windows pipe name through the session token (client side)

**Files:**
- Modify: `crates/supervisor/src/paths.rs:98-122` (`resolve_socket_path_with`)

On Windows the socket path IS the pipe name. Default + `TC_SOCKET` keep today's behavior; an explicit session uses `\\.\pipe\terminal-commander-{token}`. On unix this fn already delegates to `resolve_state_dir_with` (now session-aware from Task 2), so no unix change is needed beyond what Task 2 gave us.

- [ ] **Step 1: Write the failing test**

Add to the `tests` mod in `crates/supervisor/src/paths.rs`:

```rust
    #[cfg(windows)]
    #[test]
    fn windows_default_pipe_is_byte_identical_to_pre_f1() {
        let env = FakeEnv::new().with("USERNAME", "alice");
        assert_eq!(
            resolve_socket_path_with(&env).to_string_lossy(),
            r"\\.\pipe\terminal-commander-alice",
            "unseeded Windows pipe MUST stay the legacy name (no rename)"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_explicit_session_uses_token_pipe() {
        let env = FakeEnv::new()
            .with("USERNAME", "alice")
            .with("TC_SESSION", "agent-1");
        assert_eq!(
            resolve_socket_path_with(&env).to_string_lossy(),
            r"\\.\pipe\terminal-commander-agent-1",
            "explicit session uses the token, not the username"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_tc_socket_overrides_everything() {
        let env = FakeEnv::new()
            .with("USERNAME", "alice")
            .with("TC_SESSION", "agent-1")
            .with("TC_SOCKET", r"\\.\pipe\custom");
        assert_eq!(
            resolve_socket_path_with(&env).to_string_lossy(),
            r"\\.\pipe\custom",
            "TC_SOCKET full override wins over session + default"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails (Windows)**

Run: `cargo test -p terminal-commander-supervisor windows_ 2>&1 | Select-String "test result|FAILED|assertion"`
Expected: FAIL — `windows_explicit_session_uses_token_pipe` fails (still emits username, not token). The default + TC_SOCKET tests pass (current behavior).

- [ ] **Step 3: Rewrite `resolve_socket_path_with`**

Replace the body at `crates/supervisor/src/paths.rs:98-122`:

```rust
#[must_use]
pub fn resolve_socket_path_with(env: &impl EnvSource) -> PathBuf {
    use crate::session::{SessionEndpoint, resolve_session};
    if let SessionEndpoint::FullOverride(sock) = resolve_session(env) {
        return PathBuf::from(sock);
    }
    #[cfg(windows)]
    {
        // Default => legacy pipe terminal-commander-{USERNAME} (byte-identical
        // to pre-F1). Explicit session => terminal-commander-{token}.
        let label = match resolve_session(env) {
            SessionEndpoint::Session(token) => token,
            // Default: USERNAME ?? USER ?? "default" (matches DaemonConfig).
            _ => env
                .get("USERNAME")
                .or_else(|| env.get("USER"))
                .unwrap_or_else(|| "default".to_owned()),
        };
        return PathBuf::from(format!(r"\\.\pipe\terminal-commander-{label}"));
    }
    #[cfg(unix)]
    {
        // Unix socket derives from the (now session-aware) state dir.
        return resolve_state_dir_with(env).join("terminal-commanderd.sock");
    }
    #[allow(unreachable_code)]
    resolve_state_dir_with(env).join("terminal-commanderd.sock")
}
```

- [ ] **Step 4: Run tests to verify they pass (Windows)**

Run: `cargo test -p terminal-commander-supervisor 2>&1 | Select-String "test result|FAILED|error\["`
Expected: PASS — all supervisor tests green including the 3 new windows tests.

- [ ] **Step 5: Run unix tests too (WSL — confirm no unix regression)**

Run: `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commander-supervisor 2>&1 | grep -E "test result|FAILED"'`
Expected: PASS (windows-only tests filtered out on Linux; unix + cross-platform tests pass).

- [ ] **Step 6: Clippy both platforms**

Run (Windows): `cargo clippy -p terminal-commander-supervisor --all-targets -- -D warnings 2>&1 | Select-Object -Last 4`
Run (WSL): `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo clippy -p terminal-commander-supervisor --all-targets -- -D warnings 2>&1 | tail -4'`
Expected: both `Finished`, no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/supervisor/src/paths.rs
git commit -m "feat(supervisor): per-session Windows pipe name (F1 task 3)"
```

---

## Task 4: Route the daemon's bind name through the session token

**Files:**
- Modify: `crates/daemon/src/config.rs:261-268` (`DaemonConfig::pipe_name`)

The daemon's `pipe_name()` must compute the SAME name the client does (cross-side invariant #1). It currently builds the username pipe directly. Route it through `supervisor::paths::resolve_socket_path_with(&ProcessEnv)` for the non-custom case so daemon and client share one code path. The daemon's unix `socket_path()` already derives from `data_dir` (which `DaemonConfig` sets from the session-aware resolver via `default_data_dir`), so it needs no change.

- [ ] **Step 1: Write the failing test**

Add to the `tests` mod in `crates/daemon/src/config.rs`:

```rust
    #[cfg(windows)]
    #[test]
    fn pipe_name_matches_supervisor_client_resolution() {
        // Cross-side invariant: the daemon's bind name must equal what the
        // client (supervisor::paths) resolves for the same env. With no custom
        // socket_path and an unseeded env, both must be the legacy username pipe.
        let cfg = DaemonConfig::default();
        let client = terminal_commander_supervisor::paths::resolve_socket_path();
        assert_eq!(
            cfg.pipe_name(),
            client.to_string_lossy(),
            "daemon pipe_name() must equal client resolve_socket_path()"
        );
    }
```

NOTE: this asserts equality against the live process env (no `TC_SESSION` in the test runner), exercising the default tier on both sides. The per-tier matrix is already unit-tested in Task 3 on the client side; this guards the daemon/client agreement.

- [ ] **Step 2: Run test to verify it fails (Windows)**

Run: `cargo test -p terminal-commanderd pipe_name_matches 2>&1 | Select-String "test result|FAILED|assertion"`
Expected: It may PASS already for the default case (both emit username) — if so, that confirms the default path is correct; the real change is wiring the session token through. Proceed to Step 3 to make the daemon session-aware (the test then also covers any future env with TC_SESSION because both sides call the same fn).

- [ ] **Step 3: Rewrite `pipe_name`**

Replace `crates/daemon/src/config.rs:261-268`:

```rust
    /// Windows named-pipe path for parent IPC (`\\.\pipe\...`).
    ///
    /// Delegates to `supervisor::paths::resolve_socket_path` for the
    /// non-custom case so the daemon binds EXACTLY the name the client
    /// (mcp/cli) resolves: TC_SOCKET > TC_SESSION token > username default.
    #[must_use]
    pub fn pipe_name(&self) -> String {
        if let Some(ref custom) = self.daemon.socket_path {
            return custom.to_string_lossy().into_owned();
        }
        terminal_commander_supervisor::paths::resolve_socket_path()
            .to_string_lossy()
            .into_owned()
    }
```

NOTE: `resolve_socket_path()` on Windows returns the `\\.\pipe\...` string (Task 3); on unix it returns a `.sock` path, but `pipe_name()` is only used on the Windows bind path (`main.rs:167` is `#[cfg(windows)]`), so the unix shape is never consumed here.

- [ ] **Step 4: Verify supervisor is a daemon dependency**

Run: `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && grep terminal-commander-supervisor crates/daemon/Cargo.toml'`
Expected: a line like `terminal-commander-supervisor.workspace = true`. (It already is — `config.rs:348` calls `supervisor::paths::resolve_state_dir`. If absent, add it under `[dependencies]`.)

- [ ] **Step 5: Run tests + build (Windows)**

Run: `cargo test -p terminal-commanderd pipe_name_matches 2>&1 | Select-String "test result|FAILED"`
Expected: PASS.

- [ ] **Step 6: Clippy both platforms + build consumers**

Run (Windows): `cargo clippy -p terminal-commanderd --all-targets -- -D warnings 2>&1 | Select-Object -Last 4`
Run (WSL): `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -4 && cargo build -p terminal-commander-mcp -p terminal-commander-cli 2>&1 | tail -2'`
Expected: both `Finished`, no warnings; consumers build.

- [ ] **Step 7: Commit**

```bash
git add crates/daemon/src/config.rs
git commit -m "feat(daemon): bind pipe via shared session resolver (F1 task 4)"
```

---

## Task 5: End-to-end cross-side equality test + docs

**Files:**
- Modify: `crates/supervisor/src/session.rs` (add a doc-level integration test)
- Modify: `crates/supervisor/src/paths.rs` (doc comment on `resolve_socket_path` mentioning TC_SESSION)

- [ ] **Step 1: Write the cross-side equality test**

Add to the `tests` mod in `crates/supervisor/src/paths.rs`:

```rust
    #[test]
    fn daemon_and_client_resolve_identically_for_each_tier() {
        // The cross-side invariant in one place: for a given env, the endpoint
        // string is a pure function of that env. We assert resolve_socket_path_with
        // is deterministic and tier-correct; the daemon calls the same fn (Task 4),
        // so daemon-bind == client-connect by construction.
        let default_env = FakeEnv::new().with("USERNAME", "bob").with("HOME", "/h");
        let a = resolve_socket_path_with(&default_env);
        let b = resolve_socket_path_with(&default_env);
        assert_eq!(a, b, "resolution must be deterministic for identical env");

        let sess = FakeEnv::new()
            .with("USERNAME", "bob")
            .with("HOME", "/h")
            .with("TC_SESSION", "s1");
        assert_ne!(
            resolve_socket_path_with(&default_env),
            resolve_socket_path_with(&sess),
            "a distinct session must yield a distinct endpoint"
        );
    }
```

- [ ] **Step 2: Run it (WSL)**

Run: `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo test -p terminal-commander-supervisor daemon_and_client_resolve 2>&1 | grep -E "test result|FAILED"'`
Expected: PASS.

- [ ] **Step 3: Document `TC_SESSION` on the resolver**

Append to the doc comment above `resolve_socket_path` in `crates/supervisor/src/paths.rs` (before `#[must_use]` at L91):

```rust
/// Per-harness isolation: set `TC_SESSION` to an opaque token ([A-Za-z0-9._-],
/// 1..=64) and each harness gets its own endpoint. Precedence: `TC_SOCKET`
/// (full path override) > `TC_SESSION` (token) > per-user default (unchanged
/// from pre-F1). See `crate::session`.
```

- [ ] **Step 4: Full workspace verify, both platforms**

Run (WSL): `wsl.exe -- bash -lc 'cd /mnt/c/AI_STUFF/PROGRAMMING/terminal-commander && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3 && cargo test -p terminal-commander-supervisor -p terminal-commanderd --test-threads=8 2>&1 | grep -E "test result|FAILED" | head'`
Run (Windows): `cargo test -p terminal-commander-supervisor -p terminal-commanderd 2>&1 | Select-String "test result|FAILED" | Select-Object -First 8`
Expected: both green (Windows runs the cfg(windows) pipe tests; WSL runs the unix tests).

- [ ] **Step 5: Commit**

```bash
git add crates/supervisor/src/paths.rs crates/supervisor/src/session.rs
git commit -m "test(supervisor): cross-side endpoint equality + TC_SESSION docs (F1 task 5)"
```

---

## Task 6: Update the audit doc — F1 closed

**Files:**
- Modify: `docs/audits/2026-05-27-full-spectrum-flakiness-fragility-audit.md`

- [ ] **Step 1: Mark F1 shipped**

In the severity index table, change the F1 row status from open to shipped, and add a one-line note under the F1 finding pointing at the spec + the per-session `TC_SESSION` mechanism.

- [ ] **Step 2: Commit**

```bash
git add docs/audits/2026-05-27-full-spectrum-flakiness-fragility-audit.md
git commit -m "docs(audit): F1 closed via per-harness session endpoint"
```

---

## Verification Summary

After all tasks:
- `cargo clippy --workspace --all-targets -- -D warnings` clean on **both** Windows and WSL (the pipe code is cfg(windows); WSL alone would not compile it, Windows alone missed a 1.95 lint before — both required).
- Unseeded endpoints byte-identical to pre-F1 on both platforms (invariant #2): unix socket literal test + Windows legacy-pipe test.
- `TC_SESSION` distinct token → distinct endpoint, daemon and client agree (cross-side invariant #1, Task 4 + Task 5).
- Malformed `TC_SESSION` soft-fails to default (Task 1).
