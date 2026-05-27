# Version-Aware Daemon Replacement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Installing a new version (or `terminal-commander update`) cleanly kills the stale daemon and starts the new binary, so the MCP adapter never talks to a daemon older than itself.

**Architecture:** The daemon writes a pidfile `{pid,version,endpoint}` at startup (the missing primitive). A supervisor `replace_if_stale` reads the running version (pidfile, else `system_discover`), compares to the installed `CARGO_PKG_VERSION`, and on `running < installed` finds the PID (pidfile, else OS-query), hard-kills it, waits for the socket/pipe to clear, and respawns. Wired into three triggers: the MCP adapter's startup `ensure_daemon`, a `terminal-commanderd update` run-mode + `terminal-commander update` CLI, and an npm postinstall. Hard-kill only; no graceful Shutdown IPC.

**Tech Stack:** Rust (daemon, supervisor, mcp crates), clap subcommands, tokio; Node CLI + npm postinstall; Windows named pipe + `taskkill`/`Win32_Process`, unix UDS + `kill`/`fuser`/`pgrep`. cfg(unix) IPC tests run under WSL2.

---

## Background: verified facts (do not re-derive)

- `crates/daemon/src/main.rs`: clap `Cmd` enum = `Check | Start{mode} | PrintConfig`; global `--config`/`--data-dir`; dispatch in `main()` ~line 81. `StartMode = IpcServer | ForegroundIdle`.
- `crates/daemon/src/runtime.rs`: `run_ipc_server(config)` exists TWICE -- `#[cfg(unix)]` (~224) and `#[cfg(windows)]` (~256). Both: `init_file_logging`, `run_self_check` -> `(state, rep)`, `state.config.socket_path()` (unix) / pipe (windows), `server.spawn() -> handle`, then `wait_for_shutdown_signal().await`, then `handle.shutdown().await`. PIDFILE writes after a successful `spawn()`, removed after `handle.shutdown()`. `config.daemon.data_dir` is the state dir the daemon actually uses.
- `crates/supervisor/src/paths.rs`: `resolve_state_dir() -> PathBuf` (TC_DATA env, else LOCALAPPDATA/HOME defaults). `resolve_socket_path()`, `endpoint_from_socket_path()`, `Endpoint` enum (UDS path vs Windows pipe).
- `crates/supervisor/src/ensure.rs`: `ensure_daemon(opts: EnsureDaemonOptions) -> EnsureDaemonStatus`. `opts` has `daemon_binary, state_dir, log_dir, endpoint, startup_timeout, allow_spawn`. Probes endpoint -> `AlreadyRunning{endpoint,pid:None}` if reachable; spawns if `allow_spawn`; `child.id()` is the only PID it knows. `EnsureDaemonStatus` variants include `AlreadyRunning`, `Spawned{pid}`, `Unavailable{reason,diagnostics}`.
- `crates/mcp/src/main.rs`: calls `ensure_daemon(opts)` at startup (~135), `allow_spawn()` gate (~99). Forbidden from `Command::spawn` directly -- but it calls the supervisor which spawns; replace_if_stale lives in the supervisor (allowed), the adapter just calls it.
- `system_discover` IPC returns `version: String` (the daemon's `CARGO_PKG_VERSION`).
- THIS host's stale daemon: Windows `terminal-commanderd.exe` PID 39792 at `C:/AI_STUFF/PROGRAMMING/terminal-commander/packages/terminal-commander-windows-x64/bin`, ~5.4h uptime, NO pidfile (predates this feature) -> the OS-query fallback fixture.
- Workspace version is shared: adapter, daemon, supervisor all build at the same `CARGO_PKG_VERSION` from the workspace, so adapter-version == expected-daemon-version.

## File Structure

- **Create** `crates/supervisor/src/pidfile.rs` -- pidfile read/write/path + `RunningDaemon { pid, version, endpoint }`.
- **Create** `crates/supervisor/src/replace.rs` -- `replace_if_stale`, version compare, PID discovery (pidfile + OS-query), hard-kill, wait-clear.
- **Modify** `crates/supervisor/src/lib.rs` -- export the two new modules.
- **Modify** `crates/daemon/src/runtime.rs` -- write/remove pidfile in both `run_ipc_server` arms.
- **Modify** `crates/daemon/src/main.rs` -- add `Cmd::Update` run-mode calling the supervisor replace.
- **Modify** `crates/mcp/src/main.rs` -- call `replace_if_stale` after `ensure_daemon` (gated by `allow_spawn`), reconnect.
- **Modify** `packages/terminal-commander/lib/cli/parser.js` (+ new `lib/cli/update.js`) -- `update` command, WSL bridge.
- **Modify** `packages/terminal-commander/package.json` -- `postinstall` best-effort.

---

## Task 1: Pidfile primitive (supervisor)

**Files:**
- Create: `crates/supervisor/src/pidfile.rs`
- Modify: `crates/supervisor/src/lib.rs`
- Test: `crates/supervisor/src/pidfile.rs` `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the pidfile module + failing tests**

Create `crates/supervisor/src/pidfile.rs`:

```rust
//! Daemon pidfile: records the running daemon's pid + version +
//! endpoint so a newer install can find and replace a stale daemon
//! without depending on any IPC method the stale daemon may lack.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Contents of the daemon pidfile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningDaemon {
    pub pid: u32,
    pub version: String,
    /// The endpoint path/pipe the daemon bound, cross-checked before
    /// any kill so we never kill a process bound to a different socket.
    pub endpoint: String,
}

/// Pidfile path inside the given state dir.
#[must_use]
pub fn pidfile_path(state_dir: &Path) -> PathBuf {
    state_dir.join("terminal-commanderd.pid")
}

/// Write the pidfile atomically (tmp + rename).
pub fn write_pidfile(state_dir: &Path, rec: &RunningDaemon) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let path = pidfile_path(state_dir);
    let tmp = path.with_extension(format!("pid.tmp-{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(rec).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, &path)
}

/// Remove the pidfile (best-effort; ignore missing).
pub fn remove_pidfile(state_dir: &Path) {
    let _ = std::fs::remove_file(pidfile_path(state_dir));
}

/// Read the pidfile if present + parseable. A pidfile whose pid is no
/// longer alive is treated as absent (returns None).
#[must_use]
pub fn read_pidfile(state_dir: &Path) -> Option<RunningDaemon> {
    let bytes = std::fs::read(pidfile_path(state_dir)).ok()?;
    let rec: RunningDaemon = serde_json::from_slice(&bytes).ok()?;
    if pid_alive(rec.pid) { Some(rec) } else { None }
}

/// Cross-platform "is this pid alive" check.
#[must_use]
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // signal 0 = existence check.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        // OpenProcess with minimal rights; success => alive.
        use std::process::Command;
        // Avoid a winapi dep: tasklist filter is portable enough here.
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_read_roundtrip() {
        let dir = std::env::temp_dir().join(format!("tc-pidfile-{}", std::process::id()));
        let rec = RunningDaemon {
            pid: std::process::id(),
            version: "0.1.14".into(),
            endpoint: "/tmp/x.sock".into(),
        };
        write_pidfile(&dir, &rec).unwrap();
        let got = read_pidfile(&dir).unwrap();
        assert_eq!(got, rec);
        remove_pidfile(&dir);
        assert!(read_pidfile(&dir).is_none());
    }

    #[test]
    fn dead_pid_reads_as_absent() {
        let dir = std::env::temp_dir().join(format!("tc-pidfile-dead-{}", std::process::id()));
        let rec = RunningDaemon { pid: 999_999_999, version: "0.1.0".into(), endpoint: "x".into() };
        write_pidfile(&dir, &rec).unwrap();
        assert!(read_pidfile(&dir).is_none(), "dead pid must read as absent");
        remove_pidfile(&dir);
    }
}
```

Add `libc` to `crates/supervisor/Cargo.toml` `[target.'cfg(unix)'.dependencies]` (it is already a workspace dep used elsewhere; reuse `libc.workspace = true`). `serde`/`serde_json` are workspace deps.

- [ ] **Step 2: Export from lib.rs**

In `crates/supervisor/src/lib.rs` add `pub mod pidfile;` (and `pub mod replace;` -- Task 2 will fill it; declare now or in Task 2).

- [ ] **Step 3: Run tests**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-supervisor pidfile"
```
Expected: 2 PASS. (Also runs on Windows cargo if preferred; pidfile is cross-platform.)

- [ ] **Step 4: fmt + clippy**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo fmt -p terminal-commander-supervisor && cargo clippy -p terminal-commander-supervisor --all-targets -- -D warnings"
```

- [ ] **Step 5: Commit**

Subject: `feat(supervisor): daemon pidfile primitive (pid+version+endpoint)`

---

## Task 2: Daemon writes/removes the pidfile (daemon runtime)

**Files:**
- Modify: `crates/daemon/src/runtime.rs` (both `run_ipc_server` arms)

- [ ] **Step 1: Add a pidfile helper call in the unix run_ipc_server**

After the `let handle = server.spawn()...?;` line in the `#[cfg(unix)]` `run_ipc_server`, before the `wait_for_shutdown_signal().await?`:

```rust
    let _pidfile_dir = config_data_dir.clone(); // captured below
    terminal_commander_supervisor::pidfile::write_pidfile(
        &config_data_dir,
        &terminal_commander_supervisor::pidfile::RunningDaemon {
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            endpoint: socket_path.display().to_string(),
        },
    )
    .unwrap_or_else(|e| tracing::warn!("pidfile write failed (non-fatal): {e}"));
```

where `config_data_dir` is bound right after `init_file_logging`:
`let config_data_dir = config.daemon.data_dir.clone();` (capture before `config` is moved into `run_self_check`). NOTE: `run_self_check(config)` consumes `config`; bind `config_data_dir` BEFORE that call.

After `handle.shutdown().await;`:
```rust
    terminal_commander_supervisor::pidfile::remove_pidfile(&config_data_dir);
```

- [ ] **Step 2: Mirror in the windows run_ipc_server arm**

Same two insertions in the `#[cfg(windows)]` `run_ipc_server`, using the windows endpoint string (the pipe name -- use `state.config.pipe_name()` or the bound pipe path the windows arm already computes; match the variable that arm uses for the pipe, e.g. `pipe_name.clone()` for the endpoint field). Bind `config_data_dir` before `run_self_check` consumes config.

- [ ] **Step 3: Supervisor dep already present (no cycle)**

VERIFIED: `crates/daemon/Cargo.toml` ALREADY has
`terminal-commander-supervisor = { path = "../supervisor", ... }`, and
supervisor does NOT depend on the daemon crate. So `crates/daemon` can
call `terminal_commander_supervisor::pidfile::*` directly -- no new dep,
no cycle, no need to relocate pidfile.rs. Nothing to add here; this step
is a confirmation only (`grep supervisor crates/daemon/Cargo.toml`).

- [ ] **Step 4: Build both targets**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo build -p terminal-commanderd"
```
And Windows (the daemon that matters for the live proof):
```
cargo build -p terminal-commanderd
```
Expected: both compile.

- [ ] **Step 5: Integration test (unix) that startup writes the pidfile**

Add to an existing daemon test file or new `crates/daemon/tests/pidfile_lifecycle.rs` (`#![cfg(unix)]`): bootstrap a daemon in a temp data-dir via the existing `build_server`/`IpcServer` harness, assert `pidfile::read_pidfile(data_dir)` returns the right version+pid while bound. (If `run_ipc_server` is awkward to drive in-test, assert the write helper is called by checking the pidfile after a `spawn()` in the harness.)

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commanderd pidfile"
```

- [ ] **Step 6: fmt + clippy + commit**

Subject: `feat(daemon): write pidfile on startup, remove on clean exit`

---

## Task 3: replace_if_stale (supervisor core)

**Files:**
- Create: `crates/supervisor/src/replace.rs`
- Modify: `crates/supervisor/src/lib.rs`
- Test: `crates/supervisor/src/replace.rs` `#[cfg(test)] mod tests`

- [ ] **Step 1: Write replace.rs**

```rust
//! Version-aware daemon replacement. Reads the running daemon's
//! version (pidfile, else system_discover), compares to the installed
//! binary version, and on a stale daemon: finds the pid (pidfile, else
//! OS query), hard-kills it, waits for the endpoint to clear, then
//! respawns via the normal ensure path. Hard-kill only; works on a
//! daemon too old to have any Shutdown IPC.
use std::path::Path;
use std::time::Duration;

use crate::pidfile::{self, RunningDaemon};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaceOutcome {
    UpToDate { version: String },
    Replaced { old: String, new: String },
    NoDaemonRunning,
    /// Reachable but version undeterminable AND no killable pid found.
    Skipped { reason: String },
}

/// Compare semver-ish version strings. Returns true if `running` is
/// strictly older than `installed`. Unparseable running => treat as
/// stale (replace), since an unknown daemon is not trustworthy.
#[must_use]
pub fn is_stale(running: &str, installed: &str) -> bool {
    match (parse3(running), parse3(installed)) {
        (Some(r), Some(i)) => r < i,
        _ => true,
    }
}

fn parse3(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().trim_start_matches('v');
    let mut it = core.split('.').map(|s| s.split('-').next().unwrap_or(s));
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next().unwrap_or("0").parse().ok()?;
    Some((a, b, c))
}

/// Kill a pid hard, after confirming it matches our daemon.
/// `expected_endpoint` is cross-checked against the pidfile record
/// (caller passes the record). OS-query path verifies the binary name.
pub fn hard_kill(pid: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        // brief grace, then SIGKILL.
        std::thread::sleep(Duration::from_millis(800));
        if pidfile::pid_alive(pid) {
            unsafe { libc::kill(pid as i32, libc::SIGKILL); }
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()?;
        if out.status.success() { Ok(()) } else {
            Err(std::io::Error::other(String::from_utf8_lossy(&out.stderr).to_string()))
        }
    }
}

/// OS-query fallback: find the pid of a terminal-commanderd process
/// whose command line references `state_dir` (so we only ever target
/// OUR daemon, never a bare name match). Returns the first match.
#[must_use]
pub fn find_daemon_pid_os(state_dir: &Path) -> Option<u32> {
    let needle = state_dir.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        // Win32_Process: match Name + CommandLine containing our state dir.
        let ps = format!(
            "Get-CimInstance Win32_Process -Filter \"Name='terminal-commanderd.exe'\" | \
             Where-Object {{ $_.CommandLine -like '*{}*' }} | \
             Select-Object -First 1 -ExpandProperty ProcessId",
            needle.replace('\\', "\\\\")
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output().ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
    #[cfg(unix)]
    {
        // pgrep -f matches the full argv; the daemon is launched with
        // `--data-dir <state_dir>` so the state dir is in argv.
        let out = std::process::Command::new("pgrep")
            .args(["-f", &format!("terminal-commanderd.*{needle}")])
            .output().ok()?;
        String::from_utf8_lossy(&out.stdout).lines().next()?.trim().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_compare() {
        assert!(is_stale("0.1.13", "0.1.14"));
        assert!(is_stale("0.1.13", "0.2.0"));
        assert!(!is_stale("0.1.14", "0.1.14"));
        assert!(!is_stale("0.2.0", "0.1.14"));
        assert!(is_stale("garbage", "0.1.14"), "unparseable running => stale");
        assert!(!is_stale("v0.1.14", "0.1.14"), "v-prefix tolerated");
    }
}
```

(The async orchestration -- read version, decide, kill, wait-clear, respawn -- is added in Step 2 as `replace_if_stale`, consuming the `ensure_daemon` probe + `EnsureDaemonOptions`.)

- [ ] **Step 2: Add the async orchestrator**

VERIFIED: supervisor's `probe_endpoint` is connect-only (`UnixStream::connect(path).is_ok()`); there is NO IPC client in supervisor that reads a `system_discover` response. We do NOT add one. The pidfile makes it unnecessary: a reachable daemon with NO pidfile predates the pidfile feature, so it is stale BY CONSTRUCTION (older than any pidfile-writing build). So version resolution is pidfile-only, and pidfile-absent => stale.

Append `pub async fn replace_if_stale(opts: &EnsureDaemonOptions, installed_version: &str) -> ReplaceOutcome` that:
1. If `!probe_endpoint(&opts.endpoint).await` -> `NoDaemonRunning` (caller then spawns normally).
2. Read the pidfile: `match pidfile::read_pidfile(&opts.state_dir)`:
   - `Some(rec)`: if `!is_stale(&rec.version, installed_version)` -> `UpToDate { version: rec.version }`. Else cross-check `rec.endpoint` matches the resolved endpoint string (mismatch -> `Skipped{reason:"pidfile endpoint mismatch"}`), then kill `rec.pid`.
   - `None` (no pidfile but reachable): the daemon predates the pidfile feature => STALE. Find the pid via `find_daemon_pid_os(&opts.state_dir)`; none -> `Skipped{reason:"reachable daemon, no pidfile, no killable pid found"}`. Treat its version as `"pre-pidfile"` for the `Replaced.old` field.
3. `hard_kill(pid)`, then poll `!probe_endpoint` up to ~3s (return `Skipped{reason:"daemon did not exit"}` if still reachable).
4. `pidfile::remove_pidfile(&opts.state_dir)` (defensive), return `Replaced{ old, new: installed_version.to_owned() }`. The CALLER (ensure_daemon / update mode) then spawns the new daemon.

Reuse the exact `probe_endpoint` + `Endpoint` items already in `ensure.rs` (read it; do not duplicate). The endpoint string for the cross-check = `match &opts.endpoint { Endpoint::UnixSocket{path} => path.display().to_string(), Endpoint::WindowsPipe{name} => name.clone() }` (match the actual `Endpoint` variant names in paths.rs).

- [ ] **Step 3: Run tests + fmt + clippy**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-supervisor && cargo fmt -p terminal-commander-supervisor && cargo clippy -p terminal-commander-supervisor --all-targets -- -D warnings"
```
Expected: is_stale + pidfile tests PASS.

- [ ] **Step 4: Commit**

Subject: `feat(supervisor): replace_if_stale version-gate + hard-kill + OS-query`

---

## Task 4: Daemon `update` run-mode

**Files:**
- Modify: `crates/daemon/src/main.rs`

- [ ] **Step 1: Add `Cmd::Update`**

In the `Cmd` enum add:
```rust
    /// Replace a stale running daemon with this binary, then exit.
    /// Reads the running daemon version (pidfile/system_discover),
    /// and if older than this binary, kills it and spawns this one.
    Update,
```

In `main()` dispatch add an arm:
```rust
        Cmd::Update => {
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()
                .expect("tokio runtime");
            let code = rt.block_on(run_update(&cfg));
            return code;
        }
```

Add `async fn run_update(cfg: &DaemonConfig) -> ExitCode` that builds `EnsureDaemonOptions` from `cfg` (state_dir = `cfg.daemon.data_dir`, endpoint from `supervisor::paths::endpoint_from_socket_path(&cfg.socket_path())`, daemon_binary = `std::env::current_exe()`, `allow_spawn:true`), calls `replace_if_stale(&opts, env!("CARGO_PKG_VERSION"))`, and on `Replaced`/`NoDaemonRunning` spawns the new daemon (reuse `ensure_daemon`'s spawn path -- call `ensure_daemon(opts)` after the replace to bring the fresh one up). Print `old -> new` / `up-to-date`. Exit 0 on success.

- [ ] **Step 2: Build (both platforms)**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo build -p terminal-commanderd"
cargo build -p terminal-commanderd
```

- [ ] **Step 3: Commit**

Subject: `feat(daemon): update run-mode (replace stale daemon)`

---

## Task 5: MCP adapter auto-check

**Files:**
- Modify: `crates/mcp/src/main.rs`

- [ ] **Step 1: Call replace_if_stale after ensure_daemon**

In `main.rs` after the `ensure_daemon(opts).await` call (~135), when status is `AlreadyRunning` AND `allow_spawn`, call `replace_if_stale(&opts, env!("CARGO_PKG_VERSION"))`; on `Replaced` or `NoDaemonRunning`, re-run `ensure_daemon` to (re)spawn + reconnect before serving MCP. Keep it best-effort: a failed replace logs to stderr and proceeds against whatever daemon answers (so the adapter never hard-fails to start).

- [ ] **Step 2: Build + clippy**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo build -p terminal-commander-mcp && cargo clippy -p terminal-commander-mcp --all-targets -- -D warnings"
cargo build -p terminal-commander-mcp
```

- [ ] **Step 3: Commit**

Subject: `feat(mcp): auto-replace stale daemon on adapter start`

---

## Task 6: CLI `update` + npm postinstall

**Files:**
- Modify: `packages/terminal-commander/lib/cli/parser.js`
- Create: `packages/terminal-commander/lib/cli/update.js`
- Modify: `packages/terminal-commander/package.json`

- [ ] **Step 1: Add `update` to the parser**

Read `lib/cli/parser.js` for the existing command-registration pattern (mirror `doctor`/`setup`). Add an `update` command that dispatches to `lib/cli/update.js`.

- [ ] **Step 2: Write update.js**

Thin shell-out (no kill/version logic in JS -- the Rust `update` run-mode owns it):
- Resolve the daemon binary (reuse `resolveDaemonBinary` from `lib/daemon/autostart.js`).
- Run `<daemon-binary> --data-dir <state> update` (Windows: spawn the .exe directly; WSL: mirror `ensure_wsl_runtime`'s `wsl.exe bash -lc` bridge to invoke the linux daemon's `update`).
- Print the daemon's stdout (`old -> new` / `up-to-date`). Exit non-zero only on spawn failure.

- [ ] **Step 3: package.json postinstall (best-effort)**

Add `"postinstall": "node lib/cli/index.js update || true"` (or the project's postinstall convention -- check if one exists first; chain with `|| true` / a try-wrapper so a failed update NEVER fails `npm install`; the adapter auto-check is the backstop).

- [ ] **Step 4: Node test for the parser + update wiring**

If `packages/terminal-commander/test/` has a CLI-parser test (it does -- `cli-parser.test.js`), add a case asserting `update` parses to the update command. Run:
```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander/packages/terminal-commander && npm test 2>&1 | tail -15"
```

- [ ] **Step 5: Commit**

Subject: `feat(cli): terminal-commander update + best-effort postinstall`

---

## Task 7: Verify + PROVE LIVE (the goal)

- [ ] **Step 1: Workspace fmt + clippy + tests**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run -p terminal-commander-supervisor -p terminal-commanderd -p terminal-commander-mcp 2>&1 | tail -15"
```
Expected: clean + green (cfg(unix) IPC tests run here under WSL).

- [ ] **Step 2: Build the Windows daemon + adapter from current source**

```
cargo build --release -p terminal-commanderd -p terminal-commander-mcp
```
(Release to match what ships; the live daemon is a release build.)

- [ ] **Step 3: Replace this host's stale daemon (the live proof)**

The current Windows daemon (PID 39792, no pidfile) is the OS-query fixture. Run the freshly-built daemon's update mode pointed at the live state dir:
```
powershell -Command "& '<path-to-freshly-built>\terminal-commanderd.exe' update"
```
Expected: it finds PID 39792 via `find_daemon_pid_os` (no pidfile), taskkills it, waits for the pipe to clear, spawns the new daemon (which writes a pidfile). Confirm:
```
powershell -Command "(Get-Process terminal-commanderd).Id; Test-Path \"$env:LOCALAPPDATA\terminal-commanderd\state\terminal-commanderd.pid\""
```
Expected: a NEW pid, pidfile present.

NOTE: the live daemon binary lives at `C:/AI_STUFF/PROGRAMMING/terminal-commander/packages/terminal-commander-windows-x64/bin/terminal-commanderd.exe` and the MCP adapter (`terminal-commander-mcp` on PATH) connects to it. To make the adapter serve the new code, replace that staged binary with the freshly-built one (copy), OR point the update at the same state dir so the respawned daemon is current. Resolve the binary-location detail at execution time by checking which binary the running adapter actually launches (it auto-checks on next adapter start once the code ships).

- [ ] **Step 4: Prove registry_import_pack works (the original failure)**

After the daemon is current, in a fresh MCP session (or this one if the adapter reconnects) call:
```
registry_import_pack { pack: "cargo", activate: true, scope: {kind:"global"} }
```
Expected: SUCCESS (not `early eof`) -- `imported`/`activated` arrays with the 6 cargo rules. Then `command_start_combed cargo build` on a broken crate surfaces a `compile_error` signal. This is the end-to-end deployment proof.

- [ ] **Step 5: Report (DSPIVR)**

Objective, changes (pidfile, replace_if_stale, update mode, adapter auto-check, CLI+postinstall), verification (unit/integration green; live: stale daemon replaced via OS-query, pidfile now present, registry_import_pack succeeds), evidence (old->new pid, import_pack response), known gaps (in-flight jobs hard-killed by design; two-daemon Windows/WSL each via own path).

---

## Spec coverage check

- Component 1 pidfile -> Task 1 (+ daemon writes it Task 2).
- Component 2 replace_if_stale (version compare, pidfile/system_discover, pidfile+OS-query pid, hard-kill, wait-clear, safety cross-check) -> Task 3.
- Component 3 adapter auto-check -> Task 5.
- Component 4 update run-mode + CLI + postinstall -> Tasks 4, 6.
- Live proof (replace this host's stale daemon, import_pack works) -> Task 7.
- Hard-kill no-Shutdown-IPC, OS-query fallback for pidfile-less, safety match -> Tasks 1-3.

## Notes carried from spec

- Dependency-cycle watch (Task 2 Step 3): supervisor must not depend on the daemon crate; if a cycle would form, pidfile moves to core. Verify with `cargo tree` before adding the dep.
- Two daemons (Windows pipe + WSL UDS): replace operates on the endpoint the caller is configured for, not all daemons.
- postinstall is best-effort/non-fatal; adapter auto-check is the backstop.
- The current stale daemon has no pidfile -> exercises the OS-query path on the very first replacement (the transition heals once).
