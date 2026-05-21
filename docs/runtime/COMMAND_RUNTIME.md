# Command Runtime (TC38)

Status: Live (TC38). argv-only, non-PTY. PTY-backed interactive
commands remain deferred to TC44.

Crate paths:
- `crates/daemon/src/command.rs` — runtime + DTOs.
- `crates/probes/src/process.rs` — child process probe (TC15).
- `crates/core/src/job.rs` — JobManager + lifecycle drafts (TC16).

## 1. Purpose

The command runtime is the first **live command-to-signal path** in
Terminal Commander. It lets a local client (today: in-process; later:
the IPC dispatcher TC41 + rmcp adapter TC40) ask the daemon to start
a command and receive **only structured signal events** in return.

The LLM never sees raw stdout/stderr.

## 2. What is in scope (TC38)

- argv-only command execution. Shell-string passthrough is forbidden
  by `POLICY.md` and rejected at the API level (argv only).
- **Shell-bridge guard**: `argv[0]` whose basename is in
  `SHELL_INTERPRETERS_DENY` is denied BEFORE the policy engine
  evaluates the request. Closed-set list:
  `sh`, `bash`, `dash`, `zsh`, `fish`, `ksh`, `csh`, `tcsh`, `ash`,
  `busybox`, `powershell`, `powershell.exe`, `pwsh`, `pwsh.exe`,
  `cmd`, `cmd.exe`. Matches both bare and absolute-path forms
  (`/bin/sh` is denied as `sh`). `.exe` variants are case-insensitive.
  This deny list is orthogonal to `policy::COMMANDS_DENY` (which
  covers privilege escalators).
- Pre-spawn policy gate via `PolicyEngine::evaluate(CommandStart)`.
- stdout/stderr capture through existing `ProcessProbe`.
- Context-ring writes (for later `event_context` lookup).
- Sifter runtime per command (rule set is per-call; hot rebinding
  is TC42).
- Bucket creation + structured `SignalEvent` appends through the
  router (PersistentAudit on every append).
- Lifecycle events: synthetic `command_exited` / `command_failed`
  events appended to the bucket.
- Audit rows: `command_start` (allow), `command_rejected` (deny),
  `command_exit` (info; carries the lifecycle summary).
- Bounded responses: `CommandStartResponse` carries
  `(job_id, bucket_id, probe_id, cursor)`. `CommandStatusResponse`
  carries counters + final exit state.

## 3. What is NOT in scope

- No PTY spawn (TC44).
- No stdin control (TC44).
- No file / directory / artifact probe (TC43 / post-MVP).
- No registry hot activation (TC42).
- No `bucket_wait` / `event_context` over IPC (TC39 / TC41).
- No MCP / rmcp surface (TC41 / TC40).
- No shell-string interpreter — argv only.
- No TCP / network listener.
- No setuid / polkit / privileged helper.
- No raw stdout/stderr field on any response type.

## 4. Bounded payload caps

| Field | Cap |
|---|---|
| `argv` items | `MAX_ARGV_ITEMS = 256` |
| Single argv item | `MAX_ARGV_ITEM_BYTES = 4096` |
| Subject in audit | 256 chars (truncated to char boundary at insert) |
| `format_argv_metadata` per item | 128 chars |
| Audit `metadata_json` | `MAX_AUDIT_METADATA_BYTES = 4096` (store-side) |

## 5. Pipeline (one start_combed call)

```text
client                                                      daemon
  |                                                            |
  |-- CommandStartRequest{argv, cwd, env, rules, ...} --------->|
  |                                                            |
  |                            PolicyEngine::evaluate(CommandStart)
  |          deny -> audit command_rejected, return PolicyDenied
  |                                                            |
  |               Router::bucket_create + ProbeId + JobId mint  |
  |                                                            |
  |                   ProcessProbe::spawn(argv, ProcessProbeConfig)
  |                       (tokio::process::Command, no PTY)    |
  |                                                            |
  |       JobManager::start + mark_running                     |
  |       audit command_start (allow)                          |
  |                                                            |
  |<--- CommandStartResponse{job_id, bucket_id, probe_id, 0} ---|
  |                                                            |
  |                  [waiter task spawns in background]        |
  |                  [stdout/stderr -> sifter -> DaemonEventSink
  |                   -> Router::bucket_append (PersistentAudit)]
  |                  [context ring captures raw frames]        |
  |                                                            |
  |       on exit:  JobManager::finish -> EventDraft           |
  |                  Router::bucket_append (lifecycle event)   |
  |                  audit command_exit (info)                 |
  |                                                            |
  |-- CommandStatusRequest{job_id} (synchronous getter) ------>|
  |<-- CommandStatusResponse{counters + exit state} -----------|
```

## 6. Audit actions emitted by this module

Every row carries `actor = "command_runtime"` and the active profile
label as `profile`.

| `action` | `decision` | Emitted when |
|---|---|---|
| `command_rejected` | `deny` | Policy denied the request; no spawn. |
| `command_start` | `allow` | Probe spawned and job registered. |
| `command_start` | `error` | Spawn failed (e.g. ENOENT); no job created. |
| `command_exit` | `info` | Waiter task observed exit / failure / cancel. |
| `bucket_create` / `bucket_append` | `info` | Emitted by `Router` whenever the runtime touches the bucket. (TC35 + TC36 path.) |

The `audit-action.md` closed-set doctrine still does not list
`command_*` actions; this is the same recorded tension as TC35-TC37
and is addressed by a future docs-only goal.

## 7. Lifecycle event semantics

`JobManager::finish` builds a typed `EventDraft`:

| Exit | `kind` | `severity` | `pointer_unavailable_reason` |
|---|---|---|---|
| code = 0 | `command_exited` | `Low` | absent (severity below Medium threshold) |
| code != 0 OR signal | `command_failed` | `Critical` | `"synthetic command-exit lifecycle event"` |
| cancel | `command_failed` | `Critical` | same |

This satisfies the TC02 invariant: every severity >= Medium event
carries a `SourcePointer` OR a `pointer_unavailable_reason`.

## 8. Source-status

| Component | Status |
|---|---|
| `CommandRuntime::start_combed` (argv only) | live (TC38) |
| `CommandRuntime::status` | live (TC38) |
| `DaemonEventSink` -> `Router::bucket_append` | live (TC38) |
| `DaemonState.command` | live (TC38) |
| Policy gate on every `CommandStart` | live (TC22, exercised by TC38) |
| Persistent audit on every command action | live (TC35, exercised by TC38) |
| MCP `command_*` tools | reserved (TC41) |
| IPC `command_*` methods | reserved (TC41) |
| PTY-backed commands | deferred (TC44) |
| stdin push (`command_write_stdin`) | deferred (TC44) |
| Hot rule rebind on running probe | deferred (TC42) |

## 9. Test coverage

`crates/daemon/tests/command_runtime.rs` (Unix only, 8 tests):

- `command_start_emits_matching_signal_into_bucket_no_raw_text`
  (happy path uses `python3 -c` -- a non-shell helper -- to avoid
  the shell-bridge guard)
- `command_start_denied_for_sudo_argv` (privilege deny + audit row)
- `command_start_denied_for_bare_sh_argv` (shell-bridge guard,
  bare `sh`; proves no process is spawned and audit row records
  the "shell interpreter" reason)
- `command_start_denied_for_absolute_sh_argv` (shell-bridge guard,
  `/bin/sh`; absolute paths are basename-extracted before match)
- `command_start_denies_all_known_shell_interpreters` (every
  member of `SHELL_INTERPRETERS_DENY` in both bare and absolute-
  path forms, plus the Windows `.exe` variants)
- `nonzero_exit_produces_command_failed_event_in_bucket`
  (lifecycle event satisfies TC02 pointer-or-reason; status reports
  `exit_code = Some(7)`)
- `empty_argv_is_rejected_before_spawn`
- `response_types_have_no_raw_stream_lane` (structural)

Underlying probe coverage stays at `crates/probes/src/process.rs`
(21 unit tests) including `probe_response_carries_no_raw_text`.

## 10. Open questions reserved for later goals

| Question | Owner goal |
|---|---|
| Should `bucket_events_since` and `event_context` flow over the daemon UDS / MCP layer? | TC39 |
| How does `command_status` get a long-poll variant? | TC41 |
| Hot rebind of `SifterRuntime` on a running probe when registry activates a new rule version. | TC42 |
| PTY backing for interactive commands. | TC44 |
| Per-job concurrent connection backpressure cap. | TC47 |
