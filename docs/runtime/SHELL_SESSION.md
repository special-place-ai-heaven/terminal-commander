# Shell Session Runtime (Omni P1 / TC50)

Status: Live (omni program P1, US1). UNIX-ONLY. Gated by the
`allow_session` capability (default deny). Windows session support is a
separate, not-yet-shipped slice.

This document supersedes the "sessions deferred to TC50" note in
`docs/runtime/SHELL_RUNTIME.md` section 3: persistent sessions and
workspace snapshots are now implemented. One-shot `shell_exec` (TC49)
remains the separate lane documented in `SHELL_RUNTIME.md`.

Crate paths:
- `crates/daemon/src/shell_session.rs` -- `ShellSessionRuntime` + DTOs
  (the whole module is `#![cfg(unix)]`).
- `crates/daemon/src/pty_command.rs` -- `PtyRuntime::start_session`
  (the gated PTY spawn; performs the policy gate + audit row).
- `crates/daemon/src/policy.rs` -- `PolicyAction::SessionStart`, the
  `allow_session` cap.
- `crates/daemon/src/config.rs` -- `[shell_session]` config section
  (`max_sessions`, `idle_ttl_secs`).
- `crates/daemon/src/ipc/handlers/session.rs` -- the IPC handlers;
  non-unix builds compile to stubs that return `UnsupportedPlatform`.
- `crates/store/src/workspace.rs` -- the SQLite workspace-snapshot store
  (migration `V0006`).
- `crates/mcp/src/tools.rs` -- the `shell_session_*` and
  `workspace_snapshot_*` MCP tools.

## 1. Purpose

A *session* is a long-lived interactive login shell attached to a PTY.
It exists so an agent can run a SEQUENCE of shell steps that share
working directory and environment -- `cd build`, then `cmake ..`, then
`make` -- without re-passing `cwd`/`env` on every call.

The session is built ON TOP of the existing PTY runtime, not as a new
process model: the session shell is a PTY job running `[shell, "-i"]`,
so sticky `cwd`/`env` come for free from the persistent shell process.
The next `shell_session_exec` line runs in whatever directory the
previous `cd` left it in.

Like every other lane, session output is COMBED: `shell_session_exec`
writes the line to the shell and reads bounded structured signals back
from the session bucket. The LLM never sees raw stdout/stderr.

## 2. What is in scope

- Five session tools: `shell_session_start`, `shell_session_exec`,
  `shell_session_status`, `shell_session_stop`, `shell_session_list`.
- Two workspace tools: `workspace_snapshot_create`,
  `workspace_snapshot_apply`.
- Daemon-owned `{cwd, env}` stickiness via the persistent interactive
  shell process (NOT a re-implemented environment model).
- The `allow_session` capability gate (`[policy.caps]`, default false;
  see `POLICY.md` section 4.1) via `PolicyAction::SessionStart`.
- A bounded `max_sessions` cap enforced BEFORE spawn, and a per-session
  idle-TTL reaper.
- Audit rows: `shell_session_start` (allow) with a redacted subject;
  the `SessionStart` deny path is audited like every gated action.
- Persistent workspace snapshots in SQLite (cwd + bounded env).

## 3. What is NOT in scope (honest limits)

- **Windows / non-unix sessions.** The whole `shell_session` module is
  `#[cfg(unix)]`. On a non-unix daemon the `state.sessions` field is
  absent and every session IPC handler returns
  `IpcErrorCode::UnsupportedPlatform` with the message "persistent
  shell sessions are not available on this platform (unix-only; Windows
  session support is a separate slice)". This holds even though the PTY
  command lane (`pty_command_*`) is dual-backend (unix + Windows
  ConPTY): the session layer on top of it is unix-only for now.
- **A fully authoritative `status.cwd`.** `cwd` tracking is BEST-EFFORT
  (see section 6). The persistent shell remains the source of truth; a
  `shell_session_exec` of `pwd` returns the real directory as a combed
  signal regardless.
- **Inheriting host secrets into status/snapshot.** The tracked env is
  ONLY the caller-supplied overlay (bounded), never the inherited parent
  environment (section 5).

## 4. Lifecycle

```text
shell_session_start
  -> reap any terminal sessions (a dead session never pins a cap slot)
  -> enforce max_sessions on the LIVE count (refuse loudly if reached)
  -> resolve shell (request `shell` or the default /bin/bash)
  -> argv = [shell, "-i"]            (interactive: cd state persists)
  -> PtyRuntime::start_session
       -> PolicyAction::SessionStart  (allow_session cap gate)
       -> shell_session_start audit row (redacted subject) BEFORE spawn
       -> shared PTY spawn core (bucket + probe + waiter)
  -> prime the shell quietly (PS1=; stty -onlcr -echo; bracketed-paste off)
  -> record SessionEntry { session_id <-> job_id / bucket_id }

shell_session_exec { session_id, line }
  -> terminal-state guard: a send to a non-Live session fails loudly
     (SessionError::NotLive) instead of hanging on a dead shell
  -> oversize guard: line over MAX_SESSION_LINE_BYTES is refused
  -> write `line\n` through the PTY (secret-prompt guard applies)
  -> advance best-effort cwd tracking on a recognizable `cd`
  -> caller reads combed signals from the session bucket by cursor

shell_session_status { session_id }
  -> { state, cwd, env_snapshot (bounded), last_active_at }
  -> a status read counts as activity (a polled-but-idle session is not
     reaped out from under an attentive caller)

shell_session_stop { session_id }
  -> graceful-then-forced terminate of the PTY job; idempotent

shell_session_list {}
  -> bounded list of { session_id, state, cwd, last_active_at }
```

### Refusal, never silent hang

`max_sessions` is enforced BEFORE the spawn. When the live count is at
the cap, `shell_session_start` returns
`IpcErrorCode::SessionLimitExceeded` ("stop a session and retry") --
never a silent block. A send to a session whose shell has exited
returns `SessionNotLive`, not a hang.

### Idle reaper

A periodic pass (`reap_idle`) stops and drops any session idle past the
TTL, plus any already-terminal entry. It collects victims under a read
lock and stops them WITHOUT holding the session write lock (stopping
calls into the PTY runtime, which takes its own locks). A zero TTL
disables the idle path; terminal entries are still reaped.

## 5. cwd / env stickiness

`cwd` and `env` are sticky because the SHELL PROCESS is persistent, not
because the daemon re-applies them. The daemon also keeps a lightweight
`SessionEntry` for the wire responses:

- `cwd`: seeded from the requested start `cwd`, then advanced when an
  `exec` line is a recognizable `cd <single-arg>` (best-effort; see
  section 6).
- `env_snapshot`: the bounded `(key, value)` overlay the caller supplied
  at start, capped at `MAX_SESSION_ENV_ITEMS`. It NEVER includes the
  inherited parent environment, so no host secrets leak into a
  `status` or `snapshot` response.

## 6. The partial `status.cwd` caveat (read this)

`status.cwd` is BEST-EFFORT and can lag the real shell. The daemon
tracks `cwd` by parsing `exec` lines with `parse_cd_target`, which only
recognizes a plain `cd <single-arg>`:

- It accepts `cd /tmp`, `cd "/some/dir"`, `cd build`.
- It returns `None` (no update) for `cd` with no arg, `cd -`, a compound
  or pipeline (`cd a && cd b`, `cd a | x`), or `cd` with multiple args.

When tracking returns `None`, `status.cwd` keeps its previous value even
though the live shell may have moved. This is intentional: cwd tracking
is advisory bookkeeping, and the persistent shell is the real source of
truth. To read the authoritative directory, run
`shell_session_exec { line: "pwd" }` and read the combed signal -- it
returns the real directory regardless of what the tracker believes.

## 7. Configuration

The `[shell_session]` section sizes the runtime once sessions are
permitted. It does NOT grant the capability -- that is `[policy.caps]
allow_session` (default false; see `POLICY.md` section 4.1).

```toml
[shell_session]
max_sessions   = 16   # max concurrent LIVE sessions; enforced before spawn
idle_ttl_secs  = 900  # per-session idle TTL; 0 disables the reaper
```

| Field | Default | Const | Meaning |
|---|---|---|---|
| `max_sessions` | `16` | `DEFAULT_MAX_SESSIONS` | Concurrent live-session cap; a start past it is refused with `SessionLimitExceeded`. |
| `idle_ttl_secs` | `900` | `DEFAULT_SESSION_IDLE_TTL_SECS` | Seconds of inactivity before a session is torn down. `0` disables the per-session reaper (terminal sessions are still reaped). |

An absent `[shell_session]` block uses both defaults. This section is
separate from the daemon-level `daemon.idle_ttl_secs` (default 1800),
which governs the WHOLE daemon's idle self-reap, not a single session.

## 8. Policy gate and audit

A session start is a gated action. The verdict is `PolicyAction::
SessionStart { shell, cwd }`, evaluated like the shell lane but behind
its OWN capability so a persistent session is a separate operator opt-in
from one-shot `shell_exec`:

- `AllowWithAudit` only on an exec-capable profile (`developer_local`,
  `admin_debug`, `full_access`) WITH `allow_session` on.
- `Deny` otherwise (cap off, or a profile that forbids sessions such as
  `read_only_observer` / `repo_only`).

`PtyRuntime::start_session` performs the gate and writes the
`shell_session_start` audit row (redacted subject) BEFORE the spawn.
A denied cap surfaces to the session runtime as a `SessionError::Pty`
carrying `PolicyDenied`. The session runtime adds NO second gate; the
cap is the single door.

The PTY shell-interpreter deny list (which blocks `bash`/`sh`/... as
`argv[0]` on the argv command lane) is intentionally SKIPPED for the
session spawn: the argv is daemon-assembled (`[shell, "-i"]`, never
caller-supplied), and the `SessionStart` cap is the gate instead. The
`shell_line` residual-risk discussion for `shell_exec` (POLICY.md
section 4.1) applies equally here: once `allow_session` is on, the
interactive shell can run anything the host shell can, which is why
sessions are a trusted-profile, opt-in capability rather than always-on.

## 9. Workspace snapshots

A workspace snapshot is a saved, restorable `(cwd + bounded env)`
captured from a live session. It lives in the SAME SQLite file as the
event store and registry (migration `V0006`,
`crates/store/src/workspace.rs`).

```text
workspace_snapshot_create { session_id, name? }
  -> read the session's tracked (cwd, bounded env)
  -> persist a row keyed by an opaque snapshot_id ("snap_<uuid>")
  -> { snapshot_id }

workspace_snapshot_apply { snapshot_id, session_id }
  -> fetch the snapshot row (FileNotFound if unknown)
  -> replay into the target LIVE session via shell_session_exec lines:
       export K=V   for each bounded env entry (validated key)
       cd <cwd>     last, so a later cd is the final tracked state
  -> { applied: true, cwd }
```

Safety properties:

- The env map persisted is the bounded overlay the daemon already
  captured -- no unredacted host secrets are written to SQLite.
- On apply, env keys are validated (`is_safe_env_key`: non-empty, no
  leading digit, only `[A-Za-z0-9_]`) before they are assembled into an
  `export` line, so a malformed key can never build an injection line.
  Values are single-quoted (`shell_single_quote`).
- Apply lines run through `shell_session_exec`, so the terminal-state
  guard, the oversize cap, and the secret-prompt guard all apply.
- `apply` targets a LIVE session; it does NOT spawn one. The
  `session_id` must already exist.

## 10. Bounded payload caps

| Field | Cap |
|---|---|
| `shell_session_exec` line | `MAX_SESSION_LINE_BYTES` (oversize is refused) |
| Start / snapshot env items | `MAX_SESSION_ENV_ITEMS` |
| Concurrent live sessions | `max_sessions` (config; default 16) |

## 11. See also

- `docs/runtime/SHELL_RUNTIME.md` -- one-shot `shell_exec` (TC49).
- `docs/runtime/COMMAND_RUNTIME.md` -- the argv command lane.
- `POLICY.md` section 4.1 -- `[policy.caps]` and the `allow_session`
  capability + `SessionStart` algorithm.
- `docs/security/PRIVILEGE_MODEL.md` -- why the shell/session lanes are
  trusted-profile, opt-in capabilities.
- `docs/storage/EVENT_STORE.md` -- the SQLite file the snapshot store
  shares.
