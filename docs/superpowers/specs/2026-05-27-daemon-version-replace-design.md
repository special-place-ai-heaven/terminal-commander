# Spec: Version-Aware Daemon Replacement

Status: Design (decisions locked 2026-05-27, no open questions). Language: ASCII only.

## Objective

Installing a new terminal-commander version (or running
`terminal-commander update`) must cleanly stop the stale running daemon
and start the new binary, so the MCP adapter never talks to a daemon
older than itself. Today it does the opposite: the new adapter +
binaries land on disk, the old daemon keeps running old code, and new
IPC methods fail with `daemon ipc error [Internal]: read length: early
eof` (exactly what happened calling `registry_import_pack` against a
5.4h-uptime stale daemon).

## Root cause (verified)

- `system_discover` IPC returns the daemon's `version`
  (`env!("CARGO_PKG_VERSION")`, server.rs:568).
- The supervisor `ensure_daemon` (ensure.rs) only spawns when the
  endpoint is UNREACHABLE; it never version-checks a reachable daemon
  and reuses it as-is.
- daemon-autostart / bootstrap only "spawn if no socket"; a running
  daemon is reused regardless of age.
- There is NO `update` command and NO restart-on-stale anywhere.
- The daemon writes NO pidfile, so nothing records which PID/version is
  running. `ensure_daemon` only knows the PID of a daemon IT spawned.
- Windows daemon = named pipe; WSL/unix daemon = UDS (`#![cfg(unix)]`).
  Two daemons can coexist (Windows `terminal-commanderd.exe`, WSL npm
  linux binary).
- The MCP adapter (`crates/mcp/src/main.rs`) calls `ensure_daemon` at
  startup -> the natural place to add the version gate.

## Decisions (locked)

1. **Trigger: both** -- auto version-check on every adapter start (the
   safety net, makes install "just work" by next launch) PLUS an
   explicit `terminal-commander update` CLI command PLUS an npm
   `postinstall` hook that calls it (so install replaces cleanly).
2. **Stop: pidfile + hard kill (no graceful drain, no Shutdown IPC).**
   The daemon writes a pidfile (PID + version) at startup -- the
   missing primitive. Replacement = kill the pidfile PID, wait for the
   socket/pipe to clear, spawn the new binary (which writes a fresh
   pidfile). Hard kill chosen over graceful: a version swap is rare and
   operator-initiated, jobs are restartable, and a kill is version-proof
   (works on a daemon too old to know any new Shutdown IPC). No Shutdown
   IPC is added (YAGNI).
3. **No-pidfile fallback (heals the transition):** a daemon that
   predates this feature has no pidfile. When the socket/pipe is
   reachable but no pidfile exists, fall back to an OS query to find the
   listener PID and kill it, then start the new daemon (which writes a
   pidfile). After one transition the pidfile is authoritative.

## Design

### Component 1: daemon pidfile (the keystone) -- crates/daemon

On startup, after binding the socket/pipe, the daemon writes a pidfile
next to the socket: `<state_dir>/terminal-commanderd.pid` containing
JSON `{ "pid": <u32>, "version": "<semver>", "endpoint": "<path>" }`.
Removed on clean exit (best-effort; a stale pidfile whose PID is dead is
treated as absent). `state_dir` is the same dir `supervisor::paths`
already resolves. Version = `env!("CARGO_PKG_VERSION")`.

- Crates: daemon (runtime startup + shutdown), reuse
  `supervisor::paths::resolve_state_dir`.
- Acceptance: starting the daemon writes the pidfile with the correct
  pid+version; a clean stop removes it.

### Component 2: version gate + replace -- crates/supervisor

Add to the supervisor a `replace_if_stale` path used by both triggers:

1. Resolve the running daemon's version:
   - read the pidfile if present (cheap, no IPC), else
   - call `system_discover` over the socket (works on any reachable
     daemon, including pre-pidfile ones).
2. Compare to the installed binary version (the supervisor crate's own
   `CARGO_PKG_VERSION` -- the adapter and daemon ship from the same
   workspace version, so adapter-version == expected-daemon-version).
3. If equal: no-op (reuse). If running < installed (or unparseable):
   - find the PID: pidfile PID, else OS query
     (Windows: locate the pipe-owning `terminal-commanderd.exe` via
     `Get-CimInstance Win32_Process` name match scoped to our binary
     path; unix: `fuser`/`lsof` on the UDS path, fallback `pgrep -f
     terminal-commanderd` scoped to our data-dir arg).
   - kill it (Windows `taskkill /PID /F`; unix `kill -TERM` then
     `kill -KILL` after a short wait).
   - wait for the socket/pipe to become unreachable (bounded poll,
     ~3s).
   - spawn the new daemon via the existing `ensure_daemon` spawn path.
4. Safety: only kill a process whose executable path / argv matches OUR
   daemon (data-dir + binary name), never a bare name match that could
   hit an unrelated process. The pidfile's `endpoint` field is
   cross-checked against the resolved socket path before killing.

- Crates: supervisor (new `replace_if_stale` + OS-query helpers, both
  `#[cfg(windows)]` and `#[cfg(unix)]`).
- Acceptance: against a deliberately-stale daemon, `replace_if_stale`
  kills it and starts the current one; against a current daemon it is a
  no-op; it never kills a non-matching process.

### Component 3: auto-check on adapter start -- crates/mcp

`main.rs` already calls `ensure_daemon`. Add: after ensuring a daemon
is reachable, run `replace_if_stale` (gated by `allow_spawn`, same as
spawn -- a read-only/no-spawn adapter must not kill). On replace, the
adapter reconnects to the fresh daemon before serving MCP.

- Crates: mcp.
- Acceptance: launching the adapter against a stale daemon transparently
  replaces it; the first `registry_import_pack` call then succeeds.

### Component 4: `terminal-commander update` CLI + postinstall -- packages/

- `packages/terminal-commander/lib/cli/parser.js`: add `update` command.
- New `lib/cli/update.js`: resolve the daemon binary (reuse
  `resolveDaemonBinary`) and invoke a Rust daemon run-mode
  `terminal-commanderd <data-dir args> update` that wraps Component 2's
  `replace_if_stale` (single source of truth -- the node CLI is a thin
  shell-out, no duplicated kill/version logic in JS). On WSL, run it
  through the existing `wsl.exe` bridge (mirror `ensure_wsl_runtime`).
  The `update` run-mode prints `old_version -> new_version` (or
  `up-to-date`) and exits 0 on success.
- `package.json` `postinstall`: call the update path (best-effort,
  non-fatal -- a failed postinstall must not break `npm install`; the
  adapter auto-check is the backstop).

- Crates/packages: daemon (a `--replace-if-stale`/`update` run mode
  wrapping Component 2), cli parser + update.js, package.json.
- Acceptance: `terminal-commander update` against a stale daemon
  replaces it and reports old->new version; npm install triggers it;
  failure is non-fatal.

## Net behavior

```
npm install <new version>
  -> postinstall: terminal-commander update
       -> replace_if_stale: running 0.1.13 < installed 0.1.14
          -> kill old daemon (pidfile PID, or OS-query if no pidfile)
          -> wait socket clear -> spawn new daemon -> writes pidfile
  (backstop) next MCP adapter start
       -> ensure_daemon + replace_if_stale: versions match -> no-op
  => new IPC methods (registry_import_pack) work immediately
```

## Verification

- Daemon pidfile: unit/integration test that startup writes
  pid+version+endpoint and clean stop removes it. (`#![cfg(unix)]` IPC
  tests run under WSL2; pidfile write is cross-platform.)
- `replace_if_stale`: test the version-compare + no-op-on-match;
  the kill/respawn path verified live on this host (the current stale
  Windows daemon is the natural fixture -- replacing it is the proof).
- OS-query fallback: verified live by replacing the CURRENT pidfile-less
  daemon on this Windows host.
- End-to-end proof (the goal): after replacement,
  `registry_import_pack {pack:"cargo", activate:true, scope:global}`
  through the live MCP succeeds (no `early eof`), then a `cargo build`
  surfaces a `compile_error` signal.
- Safety: a test that `replace_if_stale` refuses to kill a process whose
  path/argv doesn't match our daemon.

## Risks + mitigations

- **Killing the wrong process:** mitigated by path/argv + endpoint
  cross-check before kill; bare-name match alone is never used.
- **Two daemons (Windows + WSL):** each has its own state-dir + socket +
  pidfile; replace_if_stale operates on the endpoint the adapter is
  configured for, not "all daemons." The WSL npm daemon is replaced by
  the WSL update path; the Windows daemon by the Windows path.
- **postinstall failure breaking npm install:** postinstall is
  best-effort/non-fatal; the adapter auto-check is the backstop.
- **Race: two adapters replacing at once:** the existing bootstrap
  `lock.js` lock (or a pidfile-dir lock) serializes replacement.
- **In-flight jobs killed:** accepted (hard-kill decision); version
  swaps are rare + operator-initiated; jobs are restartable.

## Out of scope

- Graceful drain / Shutdown IPC (explicitly dropped per the hard-kill
  decision).
- Downgrade handling (running > installed) -- treat as no-op; we only
  replace when running is OLDER (or unparseable).
- The crates/ release trigger + glibc work (separate, shipped/in-flight).

## Provenance

The early-eof failure: live `registry_import_pack` call this session
against a 5.4h-uptime stale Windows daemon. Verified: no Shutdown IPC,
no pidfile, ensure_daemon spawn-only, system_discover returns version.
Decisions locked via brainstorm 2026-05-27 (both-triggers, pidfile +
hard-kill, OS-query fallback for the pidfile-less transition).
