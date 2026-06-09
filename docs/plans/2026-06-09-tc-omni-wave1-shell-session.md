# Wave 1 — Shell Capability + Persistent Session

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Unlock human shell parity (pipelines, compounds, sticky cwd/env) through opt-in, audited, combed shell execution — without removing the argv-first default.

**Architecture:** Add `CommandShellStart` policy action; new `ShellRuntime` alongside `CommandRuntime`; persistent sessions backed by PTY + line discipline or long-lived shell child; all output through existing sifter/bucket pipeline.

**Tech stack:** Rust, existing `PolicyEngine`, `ProcessProbe`/`PtyRuntime`, IPC protocol extensions, MCP tools.

**Depends on:** Wave 0 trust hardening verified green.

**Acceptance:** O-01, O-02 from master program.

---

## Goal chain (proposed)

| Goal | Outcome |
|---|---|
| TC49 | Policy + `CommandShellStart` + `shell_exec` one-shot |
| TC50 | `ShellSessionRuntime` + session MCP tools |
| TC51 | Workspace snapshots + session TTL/limits |
| TC52 | Docs, fixtures, live e2e, README realignment |

---

## TC49 — One-shot shell execution

### Design

**New MCP tool: `shell_exec`**

```json
{
  "shell_line": "cat foo | grep bar > out.txt && wc -l out.txt",
  "cwd": "/path",
  "env": {"KEY": "val"},
  "rules": [],
  "wait_ms": 5000,
  "shell": "/bin/bash"
}
```

- `shell_line` is REQUIRED dedicated field — never `-c` smuggled through argv.
- Daemon spawns `[shell, "-lc", shell_line]` ONLY when policy allows `CommandShellStart`.
- Default profile: **deny** (unchanged behavior).
- `developer_local` + config: `commands.shell_passthrough = true` → AllowWithAudit.

**Policy changes (`crates/daemon/src/policy.rs`):**

```rust
pub enum PolicyAction<'a> {
    // existing...
    CommandShellStart { shell_line: &'a str, cwd: &'a Path, shell: &'a str },
}
```

**Guard change (`crates/daemon/src/command.rs`):**

- Keep `SHELL_INTERPRETERS_DENY` for `command_start_combed` (argv path unchanged).
- New `ShellRuntime::exec` bypasses argv[0] guard; hits `CommandShellStart` policy instead.

**Capability schema (`crates/daemon/src/config.rs`, `[policy.caps]` — nested under `PolicySection`, beside `commands`/`paths`/`probes`):**

```toml
[policy]
profile = "developer_local"
[policy.caps]            # all default false; full_access preset flips these true
allow_shell      = false
allow_session    = false
allow_privileged = false
allow_remote     = false
```

Add `caps: Option<PolicyCapsSection>` to `PolicySection`. `shell_exec` requires
`caps.allow_shell = true` AND a profile whose `CommandShellStart` verdict is Allow/AllowWithAudit.
Default profiles leave it false -> shell denied, behavior unchanged. `policy_status` surfaces
`{ profile, caps:{...} }` (no opaque toggle). Caps are TOML-only, never MCP-flippable. The
`full_access` profile preset sets all four true in the loader — no `evaluate()` bypass.

**Alt door DEFERRED (reconciliation round 2):** Wave-1 ships `shell_exec` ONLY. The
`command_start{argv, allow_shell}` alt door is deferred / harness-compat-only on YAGNI grounds
(doubles test matrix, splits playbook, extra argv-shape validation, drift). No second spawn path in
v1. See `2026-06-09-tc-omni-decisions-and-reconciliation.md` Decision 2.

### Files

| Action | Path |
|---|---|
| Modify | `crates/daemon/src/policy.rs` |
| Create | `crates/daemon/src/shell.rs` |
| Modify | `crates/daemon/src/state.rs` (wire ShellRuntime into `DaemonState::bootstrap`, beside CommandRuntime/WatchRuntime/PtyRuntime @228-247 — VERIFIED target, not runtime.rs) |
| Modify | `crates/daemon/src/config.rs` (`[caps]` schema + `full_access` preset) |
| Modify | `crates/daemon/src/lib.rs` (re-export ShellRuntime / ShellExec types) |
| Modify | `crates/ipc/src/protocol.rs` (ShellExec request/response) |
| Modify | `crates/daemon/src/ipc/handlers/` (new handler) |
| Modify | `crates/mcp/src/tools.rs` (`shell_exec`, `run_and_watch` variant optional) |
| Modify | `POLICY.md`, `docs/runtime/SHELL_RUNTIME.md` (new) |
| Test | `crates/daemon/tests/shell_policy.rs` |
| Test | `crates/mcp/tests/shell_live_e2e.rs` |
| Fixture | `tests/fixtures/contracts/mcp-tools/shell_exec.v1.json` |

### Security tests (must pass)

1. Default profile denies `shell_exec`.
2. `command_start_combed` with `argv[0]=bash` still denied when shell_passthrough false.
3. `shell_line` over max bytes → reject before spawn.
4. Audit row: `command_shell_start` with redacted line hash or truncated preview.
5. Output still combed — no raw stream in response.

### Task checklist

- [ ] Add `CommandShellStart` to policy engine + profile schema in toml
- [ ] Implement `ShellRuntime::exec` reusing ProcessProbe spawn path
- [ ] IPC + MCP tool + system_discover entry
- [ ] Live e2e: O-01 pipeline
- [ ] Update tool count anchors (38 → 39+)

---

## TC50 — Persistent shell sessions

### Design

**New tools:**

| Tool | Purpose |
|---|---|
| `shell_session_start` | Create session; returns `session_id`, `bucket_id` |
| `shell_session_exec` | Send line to session; combed signals in bucket |
| `shell_session_status` | cwd, env snapshot, running jobs |
| `shell_session_stop` | Tear down |

**Model (LOCKED round 3 — Decision 4): daemon-stateful, stateless subprocess per line.**
The session is daemon-owned `{ cwd, env, shell, rules, bucket_id }`; each `shell_session_exec`
spawns a FRESH `[shell,"-lc",line]` with `current_dir(session.cwd)` + env overlay, combed into the
session bucket. NOT a long-lived PTY `bash -l` (that's reserved for interactive `pty_command_*`).

cwd persistence — capture `$PWD` in the SAME invocation via an RS-framed, session-scoped sentinel on
stderr; the wrapper is a CONSTANT `-c` string and the user line is a POSITIONAL arg (injection-safe):

```text
[shell, "-c",
 'eval "$1"; __rc=$?; printf "\036tc_cwd_<session-hex>:%s\036" "$PWD" >&2; exit $__rc',
 "_", line]
# tc_cwd_<session-hex> baked LITERALLY into the daemon-built script (NOT an env var — an env var is
# unset/overwritable by the eval'd line and would spoof the frame). User line stays positional $1.
```

A separate `pwd` spawn is WRONG: `cd` dies with its subprocess, so a later `pwd` child (current_dir
= old session.cwd) returns the stale dir and O-02 fails. The sentinel is subshell-correct (`(cd x)`
leaves `$PWD` unchanged). Strip frames in `ShellSessionRuntime` BEFORE comb (not via sifter rules);
marker MISSING -> keep `session.cwd`, do NOT fail. Full wire format = reconciliation Decision 6.
Exported-env capture is v2. Use `-c` (not `-lc`) per line; capture login env ONCE at session_start.

Long-lived PTY in a session is opt-in only (future `shell_session_mode: interactive`); default stays
stateful-daemon / stateless-subprocess.

Session limits:

- `max_shell_sessions = 4` per daemon (configurable)
- `shell_session_ttl_secs = 3600` idle reap
- Inherits daemon env + per-session env overlay

### O-02 acceptance test

```text
shell_session_start → session_id
shell_session_exec "cd /tmp"
shell_session_exec "pwd" → signal contains /tmp
```

### Files

| Action | Path |
|---|---|
| Create | `crates/daemon/src/shell_session.rs` |
| Modify | `crates/daemon/src/pty_command.rs` (shared PTY infra) |
| Modify | IPC protocol + handlers + MCP tools |
| Test | `crates/daemon/tests/shell_session_ipc.rs` |

---

## TC51 — Workspace snapshots

Optional but high value for LLMs:

- `workspace_snapshot_create(session_id, name)` — capture cwd + env dict
- `workspace_snapshot_apply(session_id, name)` — restore

Stored in SQLite session table; bounded env size.

---

## TC52 — Documentation + certification prep

- New `docs/runtime/SHELL_RUNTIME.md`
- README: update identity table
- MCP playbook section: when `shell_exec` vs `run_and_watch` vs `shell_session_exec`
- Provider smoke: one shell pipeline per harness

---

## Estimated effort

| Goal | Duration |
|---|---|
| TC49 | 1.5 weeks |
| TC50 | 1.5 weeks |
| TC51 | 0.5 weeks |
| TC52 | 0.5 weeks |
| **Total** | **~4 weeks** |

---

## Risks

| Risk | Mitigation |
|---|---|
| Shell injection | Dedicated field + length cap + audit; no argv smuggling |
| Session leak | TTL + idle reap + max sessions |
| PTY complexity on Windows | Wave 1 can ship Linux/WSL first; Windows sessions in Wave 2 |
