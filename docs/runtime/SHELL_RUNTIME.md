# Shell Runtime (TC49)

Status: Live (TC49). One-shot `shell_exec` only. Sessions
(`shell_session_*`) and the cwd sentinel are DEFERRED to TC50.

Crate paths:
- `crates/daemon/src/shell.rs` -- `ShellRuntime` facade + DTOs.
- `crates/daemon/src/command.rs` -- `StartLane`, `start_combed_shell`,
  `redact_shell_line` (the shared spawn core lives here).
- `crates/daemon/src/policy.rs` -- `PolicyAction::CommandShellStart`,
  `PolicyCaps`.
- `crates/mcp/src/tools.rs` -- the `shell_exec` MCP tool.

## 1. Purpose

The shell runtime runs ONE shell line (pipelines, compounds, redirects)
through the SAME comb / bucket pipeline the argv command lane uses,
behind the `allow_shell` capability -- without weakening the
argv-first default. The LLM still never sees raw stdout/stderr; output
is combed into structured signals plus a bounded receipt, exactly like
`command_start_combed`.

It exists because some real work is irreducibly a pipeline
(`grep -r foo . | wc -l`, `make 2>&1 | tee build.log`) that argv-only
execution cannot express. Rather than relax the argv lane's
shell-interpreter guard, TC49 adds a SEPARATE lane with its own policy
gate, so the default surface stays exactly as safe as before.

## 2. What is in scope (TC49)

- One-shot `shell_exec { shell_line, shell?, cwd, env, rules, tag,
  wait_ms }`. The daemon spawns `[shell, "-lc", shell_line]`.
- A new `ShellRuntime` facade over the existing `CommandRuntime`.
- The `allow_shell` capability gate (`[policy.caps]`, default false;
  see `POLICY.md` section 4.1).
- A dedicated policy action `PolicyAction::CommandShellStart`,
  evaluated to `AllowWithAudit` only on an exec-capable profile with
  the cap on; otherwise `Deny`.
- Dedicated audit rows `command_shell_start` / `command_shell_rejected`
  with a redacted line subject.
- The full combed pipeline: bucket, probe, rules, exit receipt.

## 3. What is NOT in scope (deferred)

- **The `allow_shell`-on-`command_start` ALT DOOR.** A second door
  (`command_start { argv, allow_shell }`) is DEFERRED, harness-compat
  only (reconciliation Decision 2). It is added ONLY if a concrete
  harness cannot send a `shell_line` field, and only normalized into
  the SAME `CommandShellStart` verdict ("one lock"). v1 ships ONE shell
  shape: `shell_exec`.
- **TC50 sessions.** `shell_session_start` / `shell_session_exec`
  (daemon-owned `{cwd, env}` state, one fresh `[shell,"-lc",line]` per
  call) are DEFERRED to TC50.
- **The cwd sentinel.** The RS-framed `\036<marker>:<pwd>\036` cwd
  capture that persists `cd` across session lines (Decisions 4/6) is
  part of TC50, NOT TC49. A one-shot `shell_exec` has no session cwd to
  persist.
- **Privilege.** No setuid / polkit / sudo path. Privilege stays a
  separate closed helper (Wave 4, `allow_privileged`). See section 7.
- **Remote.** No `target_id` / federation (Wave 5).
- **Raising `MAX_SHELL_LINE_BYTES`.** See section 4.

## 4. Bounded payload caps

| Field | Cap |
|---|---|
| `shell_line` | `MAX_SHELL_LINE_BYTES = 4096` (= `MAX_ARGV_ITEM_BYTES`) |
| Redacted audit subject | 128 bytes (char boundary) |

`MAX_SHELL_LINE_BYTES` equals `MAX_ARGV_ITEM_BYTES` ON PURPOSE: the
lane assembles `argv = [shell, "-lc", shell_line]`, so `shell_line`
lands as `argv[2]` and `validate_argv` would reject anything over 4096
as `ArgvItemTooLong { index: 2, .. }`. A larger cap here would lie.
Raising it later needs a lane-aware validator that exempts `argv[2]`
under the shell lane -- an explicit follow-up, NOT TC49.

`ShellRuntime::exec` reports an oversize line as
`CommandError::ArgvItemTooLong { index: 0, .. }` -- `index: 0` names the
`shell_line` itself (the request's single user input), not its
downstream `argv[2]` position.

## 5. The lane design (`StartLane`)

The argv path and the shell path share ONE spawn core. The core is
parameterized by a private enum threaded through `start_combed_inner`:

```rust
enum StartLane<'a> {
    Argv,
    Shell { shell_line: &'a str, shell: &'a str },
}
```

Only THREE sites in `start_combed_inner` branch on the lane; everything
from bucket allocation onward is shared verbatim:

1. **Shell-interpreter guard.** `SHELL_INTERPRETERS_DENY` (the closed
   set `sh`/`bash`/`zsh`/`pwsh`/`cmd`/...) runs ONLY for
   `StartLane::Argv`. It stays a PURE HARD DENY on the argv lane -- TC49
   does not touch it. The shell lane assembles `argv[0]` = the chosen
   interpreter ON PURPOSE, so the guard is skipped for `Shell` (it
   would otherwise self-deny the lane's own `bash`).
2. **Policy action.** `Argv -> PolicyAction::CommandStart`;
   `Shell -> PolicyAction::CommandShellStart { shell_line, cwd, shell }`.
3. **Audit label.** Deny is `command_rejected` (argv) vs
   `command_shell_rejected` (shell); allow on the shell lane emits
   `command_shell_start` before spawn.

The single public seam into the shell lane is
`CommandRuntime::start_combed_shell(req, shell_line, shell)`, which
calls `start_combed_inner(req, None, StartLane::Shell { .. })`. The
argv lane (`start_combed`) is unchanged and a Task-4 regression test
locks `argv[0] = "sh"` as still `ShellInterpreterDenied`.

This is a lane THREAD, not a spawn-core extraction: a full extraction
was judged premature for one shell shape. TC50 can add a mode later if
it duplicates real logic.

## 6. `ShellRuntime::exec`

`ShellRuntime` holds an `Arc<CommandRuntime>` and is the daemon-level
entry. `exec` is SYNC (it never awaits -- `start_combed_shell` enqueues
the spawn onto a `JoinSet`), so async IPC / MCP handlers call it
inline. It MUST run inside a tokio runtime (`ProcessProbe::spawn` uses
`tokio::process::Command`).

`exec` steps:

1. Reject an empty / whitespace `shell_line` (`EmptyArgv`).
2. Reject a line over `MAX_SHELL_LINE_BYTES`
   (`ArgvItemTooLong { index: 0, .. }`).
3. Resolve the shell: `req.shell` or `default_shell()` (`/bin/bash` on
   unix, `bash` otherwise).
4. Assemble `argv = [shell, "-lc", shell_line]`.
5. Call `command.start_combed_shell(cmd_req, &shell_line, &shell)`,
   which runs the policy gate, audit, and shared spawn core.

`shell_line` and `shell` MUST be the same strings used to build `argv`
-- they are the policy inputs and the audit subject.

Errors surfaced by `exec`:

| Error | Cause |
|---|---|
| `CommandError::EmptyArgv` | empty / whitespace `shell_line` |
| `CommandError::ArgvItemTooLong { index: 0 }` | over `MAX_SHELL_LINE_BYTES` |
| `CommandError::PolicyDenied(_)` | `allow_shell` off, or profile forbids shell |
| other `CommandError::*` | propagated from the spawn core |

## 7. Policy gate (`CommandShellStart`)

The shell lane is gated by `PolicyAction::CommandShellStart`, evaluated
in `PolicyEngine::evaluate` BEFORE the per-profile match so a single
deny-first rule covers every profile:

- `AllowWithAudit` when the profile is exec-capable
  (`developer_local`, `admin_debug`, `full_access`) AND
  `caps.allow_shell` is set.
- `Deny` otherwise (cap off, or `read_only_observer` / `repo_only`).

The verdict is `AllowWithAudit`, never plain `Allow`: shell use always
audits.

**Accepted residual risk.** `COMMANDS_DENY` (`sudo`/`doas`/`su`/
`pkexec`/`kexec`) is checked on `argv[0]` ONLY and deliberately does
NOT scan `shell_line`. Once `allow_shell` is on, a host where `sudo` is
otherwise reachable can have `sudo ...` embedded inside a `shell_line`,
and the argv[0] deny will not catch it. This is intended: it is WHY the
shell lane is a trusted-profile, opt-in, single-operator capability,
and WHY privilege escalation stays a SEPARATE closed helper (Wave 4,
`allow_privileged`) rather than a generic shell. See `POLICY.md`
section 4.1 and `docs/security/PRIVILEGE_MODEL.md`.

## 8. Audit redaction

Both shell-lane audit rows carry a REDACTED line as the audit
`subject`, never the raw `shell_line`. `redact_shell_line`
(`command.rs`):

1. splits the line on whitespace;
2. masks each token with `mask_token_inline` -- the SAME per-token
   secret masker used for argv audits, so credential spans hidden in
   argv audits are hidden here too;
3. joins and truncates to 128 bytes on a char boundary (panic-free on
   multibyte input).

The full `argv` metadata (`format_argv_metadata`) accompanies the row
as the metadata field, exactly like the argv lane.

## 9. MCP surface

`shell_exec` is the 39th live MCP tool (catalogue group `command`).
It is a thin facade: it forwards 1:1 to `IpcRequest::ShellExec`, holds
NO guard literals, and on success returns the same bounded start
metadata as `command_start_combed`:
`{ job_id, bucket_id, probe_id, cursor }` -- never raw output.

Tool description (catalogue):

> Run ONE shell line (pipelines/compounds/redirects) through the comb
> pipeline; requires allow_shell; combed, never raw.

`wait_ms` is accepted for forward parity with `run_and_watch` but is
currently ignored -- `shell_exec` is start-only and returns immediately.

Layering (same fields, three layers):

| Layer | Type | Crate |
|---|---|---|
| MCP params | `McpShellExecParams` | `crates/mcp/src/tools.rs` |
| IPC params | `ShellExecParams` | `crates/ipc/src/protocol.rs` |
| Daemon request | `ShellExecRequest` | `crates/daemon/src/shell.rs` |

## 10. Test coverage

- `crates/daemon/tests/shell_runtime.rs` -- default-deny, runs a
  pipeline when the cap is on, oversize-line rejection, distinct lines
  -> distinct `job_id`s (dedup).
- `crates/daemon/tests/command_runtime.rs` -- the argv-lane regression
  lock: `argv[0] = "sh"` is still `ShellInterpreterDenied`.
- `crates/mcp/tests/mcp_live_daemon.rs` -- the 39-tool count + the
  sorted catalogue including `shell_exec`.
- `crates/mcp/tests/shell_live_e2e.rs` -- O-01 pipeline e2e (combed,
  not raw) under `full_access`; default-deny e2e under
  `developer_local`.
- `tests/fixtures/contracts/mcp-tools/shell_exec.v1.json` -- the tool
  contract fixture.

## 11. Related docs

- `POLICY.md` section 2.5 (`full_access`), section 4.1
  (`[policy.caps]` + residual risk + shell-lane audit actions).
- `docs/runtime/COMMAND_RUNTIME.md` -- the argv lane this reuses.
- `docs/security/PRIVILEGE_MODEL.md` -- why privilege stays a separate
  helper.
- `docs/plans/2026-06-09-tc49-shell-exec-implementation.md` -- the
  implementation plan.
- `docs/plans/2026-06-09-tc-omni-decisions-and-reconciliation.md` --
  Decisions 1, 2, 5, 7.
