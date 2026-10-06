# Policy Doctrine - Terminal Commander

Status: Baseline (TC02 wave 0 deliverable).
Scope: documentation only. This document defines the policy shape that
TC22 (policy engine) MUST implement, and that TC23, TC24, TC25, TC26,
TC29 MUST honor.

Implementation status (as of 2026-10-05): PARTIALLY implemented in
`crates/daemon/src/policy.rs`. SHIPPED: the cross-profile command deny
set, the default-deny sensitive-path suffix list, the per-profile
mutation gates (sections relating to read_only_observer / admin_debug /
registry_activate), the `[policy.commands] allow_roots` allow-list,
the `[policy.paths]` and `[policy.probes]` lists, `[policy.caps]`, the
`full_access` default, the OS-critical-deletion failsafe, and `repo_only`
$REPO_ROOT containment (file read/watch and command cwd outside the
configured `repo_root` are denied). NOT SHIPPED: the per-profile limits
of section 2 (max active jobs, event and stream rates); the two
`[limits]` keys of section 4.2 are enforced. There is no default-deny
override (section 5). A config key the daemon does not act on (unknown,
or listed as unused) is named as a warning at startup, in `self_check`,
and in `policy_status.config_warnings`. The
implementation plan is `docs/specs/2026-05-29-tc22-policy-engine-
implementation.md`.

Language: ASCII only.

## 1. What "policy" means in MVP

Policy is the layer that decides whether a gated action (see
`SECURITY.md` section 4) is allowed, denied, or allowed-with-audit.
In MVP:

- Policy is **declarative**: a TOML profile names allowed paths,
  allowed command roots, allowed probe kinds, and rate/size limits.
- Policy is **advisory**: enforcement happens in TC's own process
  via canonical path resolution plus in-process path/argv checks. Kernel
  enforcement (Landlock, seccomp-bpf) is a documented roadmap, not
  an MVP feature. See `docs/security/PRIVILEGE_MODEL.md`.
- Policy is **auditable**: every decision (allow, deny, error) emits
  an audit record before the gated action runs.
- Under a hardened profile, policy is **default-deny on sensitive
  paths** (see `SECURITY.md` section 5). The default `full_access`
  profile inherits the harness's trust and applies no such deny.
- Policy is **profile-scoped**: exactly one active profile per TC
  daemon instance. Profile switching requires daemon restart in MVP.

The TC22 policy engine takes a request `(actor, action, subject,
profile)` and returns `(decision, reason, audit_record)`. Nothing in
MVP runs without going through it.

## 2. Profile catalog

MVP shipped FOUR named profiles. TC49 adds a FIFTH, `full_access`
(section 2.5), which is the DEFAULT since 2026-09-28 (owner decision:
TC inherits the harness's trust); the other four are opt-in hardening.
Profile names are stable identifiers; goals MUST refer to them by exact
name.

THE ONE FAILSAFE (every profile, no knob): TC never DELETES OS-critical
infrastructure. A destructive-deletion command (rm/rmdir/unlink/shred/srm,
del/erase/rd, Remove-Item/ri, mkfs*/wipefs, dd-to-disk, format, cipher /w),
on the argv, PTY, shell, or session lane -- after escalator/wrapper
unwrapping, through interpreter payloads (`sh -c`, `pwsh -Command`, `cmd /c`,
`wsl`, `su -c`, `eval`, `$(...)`), and with relative operands resolved against
the cwd and any `cd` in the line -- targeting a
protected system tree (/, /usr, /etc, /boot, /System, C:\Windows, a drive
root, a raw disk) is refused with the typed `OsCriticalPathProtected` error.
Writing/editing/creating those paths, and every non-deletion command
(installers, `systemctl`, `reg add`, ...), stay allowed. It is a string-level
guard rail, not a kernel boundary: it does not see a removal performed
indirectly -- through a rename/move, a symlink, variable or `~` expansion, a
heredoc, a `busybox`/`xargs`/`find` action, a script, or text typed into a
session/PTY.

### 2.1 `developer_local`

Intended for: a developer running TC on their own workstation
against their own repos.

```text
permits:
  - zero-config read+watch for paths accessible to the operator, except
    the mandatory SECURITY.md section-5 deny set. A non-empty operator
    allow-set narrows this surface and is authoritative.
  - direct-argv command and PTY execution except the closed structural
    deny set. A non-empty command allow-list narrows this surface.
  - PTY prompt detection and bounded signal/context delivery.
  - registry CRUD by the operator over the admin CLI; LLM-driven
    registry create/test allowed, activate gated.
denies (in addition to default-deny):
  - command execution outside the allow-list.
    This applies when the operator configured a non-empty allow-list;
    an empty list is the documented zero-config posture.
  - file reads outside a configured non-empty path allow-set.
  - paths matching `paths.deny_extra`.
  - any sudo/doas/polkit invocation.
limits:
  - max active jobs: 16
  - per-bucket event rate: 1000 evt/s sustained, 5000 evt/s burst
  - per-probe stream rate: 10 MiB/s with backpressure (TC11, TC28)
  - context-spool ring size: 64 MiB per probe
  - max regex compile time: 50 ms; max single-step regex execution:
    10 ms (TC10/TC29 ReDoS gate)
audit_requirements: every gated action.
```

### 2.2 `repo_only`

Intended for: CI-like or sandboxed runs where TC must touch ONLY
the current repository tree.

```text
permits:
  - read+watch under $REPO_ROOT/** (canonicalized before the policy gate).
  - command execution under $REPO_ROOT/** with the same allow-list
    as developer_local.
denies (in addition to default-deny):
  - any read or watch outside $REPO_ROOT.
  - any write outside $REPO_ROOT.
  - any environment variable that points to a path outside
    $REPO_ROOT being followed (e.g. HOME, TMPDIR are isolated to
    repo-scoped temp).
limits: same as developer_local but max active jobs = 4.
audit_requirements: every gated action.
```

### 2.3 `read_only_observer`

Intended for: long-running observation of an existing system without
running any new commands. The agent can WATCH but not RUN.

```text
permits:
  - file_read_window, file_search, file_watch under an explicit
    allow-set (operator-provided).
  - directory_probe under the same allow-set.
  - bucket reads and event_context.
denies:
  - command_start_combed, command_write_stdin, command_send_signal.
  - probe_create with kind in {process, terminal, pty}.
  - registry mutations from the LLM (operator-only).
  - any write to disk except the audit log.
limits:
  - per-bucket event rate: 200 evt/s (this profile is for triage,
    not heavy ingestion).
  - context-spool ring size: 16 MiB per probe.
audit_requirements: every gated action.
```

### 2.4 `admin_debug`

Intended for: an operator (human) diagnosing TC itself via the admin
CLI. NEVER exposed to the LLM. The admin CLI (TC25) MUST refuse to
serve MCP traffic under this profile.

```text
permits:
  - read+watch anywhere except the default-deny list (section 5 of
    SECURITY.md still applies).
  - command execution from the operator allow-list, plus
    diagnostic commands the operator names at session start.
  - registry inspection and read-only diff.
denies:
  - any MCP tool call. Profile is admin-CLI-only.
  - any modification to live registry rules; debug profile is
    inspect-only. Use developer_local for edits.
  - sudo/doas/polkit (still). This profile does not elevate.
limits:
  - max session length: 8 hours (default-tunable). After expiry,
    operator must re-authenticate (re-open CLI) to continue.
audit_requirements: every gated action; audit records tagged
  `profile=admin_debug` so they can be filtered in retention review.
```

### 2.5 `full_access`

Added: TC49 (Hybrid trust model -- reconciliation Decision 1).

Intended for: the DEFAULT. TC is a tool for LLMs, and the harness that
runs the LLM is the trust boundary; when the harness allows everything,
TC does too (shell, session, remote, recipe admin, escalators, the full
filesystem).

```text
permits:
  - everything developer_local permits (it is exec-capable and shares
    the developer_local / repo_only verdict path).
  - the gated shell lane (shell_exec), because its loader preset turns
    allow_shell ON.
  - the gated session lane (shell_session_* + workspace_snapshot_*,
    allow_session ON; LIVE, unix-only) and remote federation
    (target_id, allow_remote ON; LIVE via an operator ssh -L forward).
  - allow_privileged is accepted but gates nothing: the Wave-4
    privileged helper it was meant for is PLAN-ONLY (blocked on a
    threat review), and setting the key is named as a config warning.
    See docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md.
denies: nothing structural. COMMANDS_DENY (sudo/doas/su/pkexec/kexec
  argv), the shell-line escalator scan, and the sensitive-path list
  apply only under the hardened profiles; an explicit `[policy.caps]`
  false, `allow_roots`, or `[policy.paths]` list still narrows it.
limits: same as developer_local.
audit_requirements: every gated action. Capability use stays
  AllowWithAudit (no audit-off short-circuit).
```

The five binding guardrails (Decision 1; the implementation honors
each):

1. **THE default.** A daemon with no config runs `full_access`; a
   hardened profile needs an explicit `profile = "..."` in TOML plus a
   daemon restart.
2. **TOML-only, NOT MCP-toggleable.** No MCP tool flips the profile or
   any cap. Profile selection is config + restart, identical to the
   other four profiles.
3. **Bundle = all caps ON, NOT audit OFF.** Shell / session /
   privileged / remote stay `AllowWithAudit`; the policy engine
   (`evaluate()`) is never short-circuited. `full_access` only PRESETS
   the cap inputs; it adds no fifth code path that bypasses the engine.
4. **`policy_status` EXPOSES the caps.** The active profile and the
   resolved per-call caps (`allow_shell:true`, ...) are visible via the
   `policy_status` tool -- there is no opaque "full_access magic".
5. **Harness trust.** TC grants what the harness running the LLM
   grants. On a shared, multi-tenant, or untrusted host, select a
   hardened profile.

Cap semantics under `full_access`: every cap is preset ON; an explicit
`[policy.caps]` false revokes that one cap (section 4.1).

## 3. Profile selection

A daemon instance loads exactly one profile at startup, named in
`terminal-commander.toml`:

```toml
[policy]
profile = "full_access"  # the default; or developer_local, repo_only,
                         # read_only_observer, admin_debug (hardened)
```

When `--config` is supplied, that file is authoritative. Otherwise the daemon
loads `terminal-commander.toml` from the selected data directory when the file
exists; a missing file preserves the compiled defaults. An explicit
`--data-dir` remains authoritative over any `daemon.data_dir` value inside
that conventional file, keeping the supervisor and daemon on the same state
directory across respawns.

Profile switching at runtime is OUT OF MVP. To change profiles,
operator stops the daemon, edits config, restarts. This is a
deliberate constraint: profile changes are easier to audit when they
are restart boundaries.

## 4. Profile schema (informative; binding lands in TC22)

### 4.1 `[policy.caps]` (Hybrid trust model; SHIPPED in TC49)

Granular, opt-in CAPABILITIES that extend a base profile. The cap
block is nested under `[policy]` (it is `[policy.caps]`, NOT a
top-level `[caps]`), mirroring the `[policy.commands]` doctrine: caps
are an input to the policy engine's `evaluate()` -- exactly like
`profile` -- not a separate subsystem. The single operator-readable
trust surface stays `[policy]`.

```toml
[policy]
profile = "developer_local"

[policy.caps]
allow_shell      = true    # gates shell_exec (TC49). Default true on
                           #   developer_local, false on the others.
allow_session    = true    # gates shell_session_* + workspace_snapshot_*
                           #   (omni P1 / TC50; LIVE, unix-only). Default false.
allow_remote     = true    # gates remote federation / target_id
                           #   (omni P5; LIVE via operator ssh -L forward).
                           #   Default false.
```

Rules:

- **All four caps default `true` on the default `full_access`;
  `developer_local` grants `allow_shell` only; the others grant none.**
  `allow_shell` is on in `full_access` and `developer_local`: an LLM
  caller abandons a denied tool for raw
  Bash, and shell output stays combed, bounded, and audited
  (`command_shell_start`). Set `[policy.caps] allow_shell = false` to
  harden (the argv interpreter deny and the WSL nested-shell gate then
  apply). A non-empty `[policy.commands] allow_roots` withholds that
  default: `allow_shell` resolves `false` unless `[policy.caps]` sets it
  explicitly, because `shell_exec` does not consult `allow_roots` (an
  explicit `allow_shell = true` still enables it, unconfined by
  `allow_roots`). `allow_session` and `allow_remote` do nothing until
  explicitly turned on. `allow_privileged` is accepted but gates nothing on
  any profile (no privileged helper ships); setting it is named as a config
  warning.
- **Config / TOML ONLY.** Caps are NEVER MCP-flippable -- no tool can
  turn a cap on or off. Changing a cap means editing TOML and
  restarting the daemon, the same boundary as switching profiles
  (section 3). This keeps cap changes auditable at restart boundaries.
  A config that fails to parse stops the daemon (`config load error`,
  exit 1); it is never half-applied. The MCP adapter's auto-start passes
  only `--data-dir`, so it loads `<data-dir>/terminal-commander.toml` or,
  when that file is absent, runs on the `full_access` defaults. A
  hardening file given with `--config` is not what an auto-started daemon
  reads. After hardening, confirm `policy_status` shows `allow_shell: false`.
- **Caps are inputs to `evaluate()`.** They do not bypass the policy
  engine. `allow_shell = true` makes `shell_exec` resolve to
  `AllowWithAudit` (audited) on an exec-capable profile; it does NOT
  short-circuit any check.
- **Exec-capable profiles only.** `allow_shell` grants the shell lane,
  and `allow_session` grants the session lane, only on
  `developer_local`, `admin_debug`, or `full_access`.
  `read_only_observer` and `repo_only` deny both lanes even with the cap
  on.
- **`allow_session` is independent of `allow_shell`.** A persistent
  interactive session (`shell_session_*` + `workspace_snapshot_*`) is a
  SEPARATE operator opt-in from one-shot `shell_exec`: each lane has its
  own cap, its own `PolicyAction` (`SessionStart` vs `CommandShellStart`),
  and its own audit label. Turning one on does not turn the other on. The
  session lane is LIVE and UNIX-ONLY; on a non-unix daemon the session
  tools return `UnsupportedPlatform` regardless of the cap. See
  `docs/runtime/SHELL_SESSION.md`.
- **`full_access` preset.** The `full_access` profile (section 2.5) is
  the only profile whose loader presets all four caps ON (`base ||
  full`). A base profile + explicit `[policy.caps]` is the way to grant
  a SUBSET.
- **Visibility.** The resolved per-call caps are surfaced by the
  `policy_status` tool.

**Accepted residual risk (Decision 1), under a hardened profile.** The
command deny set (`COMMANDS_DENY`: `sudo`, `doas`, `su`, `pkexec`,
`kexec`, `polkit-agent`, `polkit-auth-agent-1`; not applied under the
default `full_access`) is checked on
`argv[0]` ONLY. It deliberately does NOT scan the
`shell_line` of a `shell_exec` call. Once `allow_shell` is on, a host
where `sudo` is otherwise reachable can have `sudo ...` embedded INSIDE
a `shell_line` (e.g. `echo x | sudo tee ...`) and the argv[0] deny will
not catch it. This is intended and is WHY the shell lane is a
trusted-profile capability (audited, single-operator machine;
`[policy.caps] allow_shell = false` hardens it) rather than an
unaudited surface. It is also WHY privilege
escalation stays a SEPARATE, closed, single-purpose helper (Wave 4,
gated by `allow_privileged`) and is never delivered through a generic
shell: the DEFAULT privilege path is never "run an arbitrary shell
line". See `docs/security/PRIVILEGE_MODEL.md` and
`docs/runtime/SHELL_RUNTIME.md`.

For the same reason, once `allow_shell` is on the shell lane is NOT
subject to `[policy.commands] allow_roots` nor to `repo_only`-style
cwd-containment: the `shell_line` is passed UNSCANNED to the interpreter
(`[shell, "-lc", shell_line]`), so allow-root prefixing and repo-root
confinement -- which bind `argv[0]` / the cwd of the ARGV lane -- do not
constrain what a shell line runs. This is consistent with the Decision-1
residual risk above and is another reason the shell lane is a
trusted-profile, opt-in capability rather than an always-on surface.

#### Shell-lane audit actions (TC49)

Every policy decision emits an audit record BEFORE the gated action
runs (section 1). The argv command lane emits `command_start` (allow) /
`command_rejected` (deny). The TC49 shell lane has its OWN labels so
shell starts are filterable apart from argv starts:

| Audit action | Decision | When |
|---|---|---|
| `command_shell_start`    | `allow_with_audit` | `shell_exec` allowed (`allow_shell` on, exec-capable profile). Emitted before spawn. |
| `command_shell_rejected` | `deny`             | `shell_exec` denied (cap off, or profile forbids shell). |

The audit `subject` for both is a REDACTED preview of the shell line,
never the raw line: the SAME two-layer credential masking the argv
audits use (Layer-A flag look-ahead over whitespace tokens + Layer-B
per-token scan), then a 128-byte cap on a char boundary
(`redact_shell_line` in `command.rs`). The accompanying metadata
re-redacts the matching shell-line argv item the same way
(`format_shell_argv_metadata`). It is a best-effort PREVIEW over a shell
line, not a full shell parse. Details and the residual limitation in
`docs/runtime/SHELL_RUNTIME.md` section 8.

#### Session-lane gate + audit (omni P1 / TC50)

A persistent session start is its OWN gated action,
`PolicyAction::SessionStart { shell, cwd }`, behind the independent
`allow_session` capability. It is evaluated with the SAME deny-first
shape as the shell lane, before the per-profile match, so one rule
covers every profile:

```text
SessionStart algorithm (mirrors CommandShellStart):

1. exec_profile = profile in { developer_local, admin_debug, full_access }
2. if exec_profile AND caps.allow_session:
     -> AllowWithAudit
        reason: "shell_session_start allowed by allow_session capability (audited)"
3. else:
     -> Deny
        reason: "shell_session_start denied: allow_session capability is
                 off or profile forbids sessions"
```

Notes that distinguish it from the shell lane:

- It uses `allow_session`, NOT `allow_shell`. The two caps are
  independent (section 4.1).
- The gate + audit row are written by `PtyRuntime::start_session`
  BEFORE the PTY spawn. The session runtime adds no second gate.
- The spawn argv is daemon-assembled (`[shell, "-i"]`, never
  caller-supplied), so the argv shell-interpreter deny list is skipped
  here on purpose; the cap is the door. `COMMANDS_DENY` is still
  argv[0]-only and does not scan what the interactive shell later runs
  (same Decision-1 residual risk as the shell lane).
- UNIX-ONLY: on a non-unix daemon the session tools return
  `UnsupportedPlatform`, independent of the cap or profile.

The session lane has its OWN audit label so session starts are
filterable apart from argv and shell starts:

| Audit action | Decision | When |
|---|---|---|
| `shell_session_start` | `allow_with_audit` | `shell_session_start` allowed (`allow_session` on, exec-capable profile). Emitted before spawn, keyed on the job id, with a redacted subject. |

Full session model, lifecycle, config (`max_sessions` / `idle_ttl_secs`),
and the best-effort `status.cwd` caveat are in
`docs/runtime/SHELL_SESSION.md`.

#### WSL nested-shell gate (US8)

The argv shell-interpreter deny (`SHELL_INTERPRETERS_DENY`, `crates/core/src/shell_deny.rs`)
catches a bare interpreter in `argv[0]` (`bash`, `sh`, `pwsh`, `cmd`, ...).
Before US8 it did NOT catch a shell smuggled through a `wsl`/`wsl.exe`
carrier: `wsl.exe -e bash -lc "<arbitrary shell>"` has `argv[0] = wsl.exe`,
which is in neither `SHELL_INTERPRETERS_DENY` nor `COMMANDS_DENY`, so both
argv checks passed while an arbitrary Linux shell ran. US8 closes that gap.

**Stance: the shell capability follows the shell across the WSL boundary.**
WSL is THIS host's boundary, not a remote machine (`allow_remote` is not
implicated). A shell reachable through `wsl.exe` is gated by the same
`allow_shell` capability that gates `shell_exec`. `allow_shell` is on in
the default `full_access` profile (and `developer_local`), so this gate
applies only once the config hardens with `[policy.caps] allow_shell = false`.

- **Inspected: argv only.** The classifier reads the argv the caller
  supplied and nothing else. File contents are never read, and there is no
  second interpreter list -- `SHELL_INTERPRETERS_DENY` is the sole
  authority, matched by basename (split on both `/` and `\` so a
  `C:\...\wsl.exe` path classifies the same on every platform). The
  carrier name gets the same Win32 normalize as the interpreter deny
  (`is_wsl_carrier`): `wsl.exe.`, `wsl.exe ` and `wsl.exe::$DATA` launch
  `wsl.exe`, so they are carriers too, as is a carrier behind a wrapper
  (`env wsl.exe ...`, `nohup wsl ...`).
- **`-e`/`--exec` vs a bare command line.** `wsl.exe` bypasses the distro
  shell ONLY for a payload introduced by `-e`/`--exec`; there the first
  payload token is the program that runs directly (no shell). A payload
  with NO `-e`/`--exec` -- a bare command line, or one after `--` -- is
  handed to the distro's DEFAULT SHELL by WSL itself, which interprets it
  (globs, `$(...)`, redirects). Running such a line IS running a shell,
  regardless of the program it names, so it is gated. Distro selectors
  (`~`, `-d`/`--distribution`, `-u`/`--user`, `--cd`, `--system`,
  `--shell-type`) are skipped to find the payload; `~` is the start-in-home
  shorthand, a selector, never a payload.
- **Fail closed.** An unrecognized flag in payload position (a novel WSL
  option) is treated as potentially carrying a payload and is denied under
  `allow_shell=false`.

Enforcement matrix -- both argv lanes (`command_start` and
`pty_command_start`) share ONE classifier, so a payload denied on one lane
is denied on the other. The `allow_shell=false` column is the hardened
opt-in; the default `full_access` profile runs the `allow_shell=true`
column:

| Classification | `allow_shell=false` | `allow_shell=true` |
|---|---|---|
| not a wsl carrier / WSL management flag (`--list`, `--status`, ...) / `-e` non-shell program (`cargo`, `uname`, ...) | runs (unchanged) | runs (unchanged) |
| nested shell (`-e bash`, bare `wsl bash`, `-- sh -c ...`, bare `echo $(id)`, bare `wsl.exe`) | **DENY** -- `shell_interpreter_denied`, naming the interpreter + the wsl carrier + the `allow_shell` gate / `shell_exec` remedy | runs; `command_start` audit row tagged `"nested_shell": "<interpreter>"` |
| unknown construction (novel WSL flag in payload position) | **DENY** (fail closed) | runs; audit tagged `"wsl_construction": "unknown"` |

**Rationale.** The argv lane must not become an unaudited route to a shell, so the
interpreter deny has to hold in spirit, not just letter.
Adding `wsl.exe` wholesale to `SHELL_INTERPRETERS_DENY` was rejected: it
would break every legitimate non-shell use (`wsl.exe -e cargo build`,
`wsl --list`). Inspecting the Linux-side binary was rejected: argv-only is
the design boundary, and file inspection is unreliable across the
WSL boundary anyway.

#### Argv interpreter deny: wrappers, flags, remote carriers

Outside a `wsl` carrier, both argv lanes and recipes use one core check
(`shell_argv_denied`, `crates/core/src/shell_deny.rs`). With `allow_shell`
off it denies with `shell_interpreter_denied` when:

- the launched program is a listed interpreter, after skipping `NAME=value`
  words and wrappers (`env`, `command`, `exec`, `nohup`, `time`, `nice`,
  `timeout`, `stdbuf`, `ionice`, `chrt`, `taskset`, `setsid`, `unbuffer`)
  with their options, numeric operands, and the value operand of
  `timeout`/`taskset`/`chrt` (`timeout inf`, `taskset ff`). Wrapper
  options are read the way getopt reads them: a short cluster ends at the
  letter that takes a value (`env -iu NAME`, `ionice -tc idle`,
  `time -po FILE`), and a long name may be a unique prefix (`env --un NAME`).
  `env -S`/`--split-string` is split the way GNU env splits it and checked
  again: a quoted run keeps its whitespace and joins its word
  (`FOO='x ssh' bash` is the assignment `FOO=x ssh`, then `bash`), `#`
  at the start of a word ends the string, and `\\ \' \" \# \$` are literal. A
  `-S` string holding `$VAR`/`${VAR}`, another `\` escape, or an
  unterminated quote is denied (fail closed); or
- a listed interpreter appears later with a script flag in its option run
  (`strace bash -x -c ...`, `bash -Cc ...`, `fish --command=...`,
  `pwsh -NoProfile -Command ...`, `pwsh -cwa ...`, `cmd /d /c ...`,
  `cmd /q/c ...`, `cmd /cecho ...`).

Non-shell interpreters (`python`, `node`, `perl`, ...) are not listed and
run with any flags. `[policy.caps] allow_shell = true` is the one switch: it
lets a matched argv run (after the `CommandShellStart` policy check, with
the `command_start` audit row tagged `"nested_shell": "<interpreter>"`, like
the WSL gate) and enables `shell_exec`. Recipes follow the same switch: with
`allow_shell` off, `recipe_upsert`, `recipe_test`, and `recipe_run` deny an
interpreter argv; with it on, the recipe validates and `recipe_run` starts it
through the same argv lane, policy check, and `nested_shell` audit tag.

**Remote and container carriers are not scanned.** When the launched
program is `ssh`, `docker`, `podman`, `nerdctl` or `kubectl`, the check
stops: `docker exec c sh -c ...` and `ssh host bash -c ...` run the
interpreter in a container or on another machine, not as this host's
shell. TC does not gate that remote side: `allow_remote` gates only
`target_id` federation and `target_probe`, not an `ssh`/`docker` argv,
which runs as an ordinary argv command. The exemption is by program, not
subcommand, and deliberately covers `podman unshare bash -c ...`, which runs
a shell on this host inside a user namespace. Residual:
`ssh localhost bash -c ...` reaches this host's shell through sshd.

**A guard rail, not a boundary (hardened profiles only).** With `allow_shell = false` this deny is
argv string matching over the listed shells, wrappers, and script flags. It
stops the common routes to a shell, not every route. Known residual classes
that still run: a listed shell behind an unlisted launcher running a script
file (`flock /tmp/l bash run.sh`; with no script flag the shell name is
indistinguishable from an operand such as `rg bash src`); tools that run a
command string themselves (`script -c ...`, and `su -c ...` behind a
wrapper, since the closed privilege deny checks only the launched
`argv[0]`); and Windows names the basename match does not resolve
(drive-relative `C:wsl.exe`, 8.3 short names other than `POWERS~n`); and
option runs are modelled for bash/dash/sh, PowerShell and cmd only, so a
value option of another listed shell (`fish -d all -c ...`, `nu -m light -c
...`, `ksh -R x -c ...`) can hide its script flag. The
complete control is `[policy.commands] allow_roots`: a non-empty list
admits only the programs it names, so leave interpreters and launchers
(`env`, `nice`, `timeout`, ...) off it, and it also withholds the
`developer_local` `allow_shell` default (section 4.1).

### 4.2 Full config schema

Every key the daemon reads, with its default where it has one. A key not
listed here (for example `[limits] max_jobs`, `[audit] retention_days`,
`[registry] llm_can_*`, `[policy] profile_version`) is accepted but has
no effect, and the daemon names it in `config_warnings` (startup log,
`self_check`, `policy_status`).

```toml
[daemon]
data_dir      = "/home/me/.local/share/terminal-commander"  # --data-dir wins
idle_ttl_secs = 1800      # self-reap after this idle time; 0 = never
# socket_path = "/run/user/1000/terminal-commanderd.sock"  # TC_SOCKET wins

[policy]
profile = "developer_local"
# repo_root = "/home/me/projects/app"   # required for repo_only
llm_can_activate_recipes = false        # omitted: profile default

[policy.commands]
allow_roots = ["cargo", "npm", "pytest", "make", "ls", "git"]

[policy.paths]
read_allow  = ["/home/me/projects/**", "/srv/repos/**"]
write_allow = ["/home/me/projects/**/target/**", "/tmp/tc/**"]
watch_allow = ["/home/me/projects/**"]
deny_extra  = []   # additional denies beyond default-deny list

[policy.probes]
# Closed kind set {command, file_watch, pty} (the ProbeKind snake_case wire
# tags), matched case-sensitively. EMPTY allow_kinds == "not configured" ==
# allow any kind (zero-config usable); a non-empty list is authoritative (a
# kind not listed is denied, no_allow_rule). deny_kinds is a hard deny that
# beats allow (probe_kind_denied). Enforced at probe creation (TC22 A2).
allow_kinds = []          # allow all three kinds (zero-config posture)
deny_kinds  = ["pty"]     # ...but forbid interactive PTY probes in this profile

[policy.caps]             # section 4.1; omitted entries keep the profile preset
allow_shell   = true
allow_session = false
allow_remote  = false

[limits]
file_window_bytes = 65536  # cap on one file_read_window (also the maximum)
bucket_read_limit = 10000  # cap on events per bucket read (also the maximum)

[shell_session]
max_sessions  = 16
idle_ttl_secs = 900

[sifters]
universal_extractors = false
```

**Path allow/deny posture (SHIPPED, TC22 A1).** `read_allow`,
`watch_allow`, and `deny_extra` are OPT-IN, matching the
`commands.allow_roots` posture: an EMPTY list is unconfigured = ALLOW
(the structural default-deny layers still run), and a NON-EMPTY list is
authoritative (a miss is denied, `no_allow_rule`). See the decision
algorithm in section 6 step 2e.

**Probe-kind posture (SHIPPED, TC22 A2).** `[probes]` `allow_kinds` /
`deny_kinds` are ENFORCED at probe creation. There is NO standalone
probe_create operation; instead the THREE real probe-creating ops layer a
deny-first probe-kind filter on top of their own primary gate:
`command_start_combed` -> kind `command`, `pty_command_start` /
`shell_session_start` -> kind `pty`, `file_watch_start` -> kind
`file_watch`. Kinds are CASE-SENSITIVE snake_case drawn from the CLOSED set
`{command, file_watch, pty}` (the `ProbeKind` wire tags); any other string
never matches a real probe and is logged as a likely operator typo at
startup. DENY BEATS ALLOW: a kind in `deny_kinds` is denied
(`probe_kind_denied`) even if it is also in `allow_kinds`. An EMPTY
`allow_kinds` means "not configured = ALLOW" (zero-config stays usable),
and a NON-EMPTY `allow_kinds` is authoritative (a kind not listed is denied,
`no_allow_rule`) -- mirroring the path allow-list posture above. The filter
is a TIGHTENING layer only: it can deny a probe its primary op gate would
have allowed, but it never widens. See section 6 steps 2c / 2e.

**OPERATOR WARNING -- zero-config write posture (TC22 A3).** `write_allow`
follows the same OPT-IN posture as the read/watch lists, and that has a
sharp edge for WRITES. With the DEFAULT config (profile `full_access`,
no `repo_root`, EMPTY `write_allow`), the `file_write` tool can write
ANYWHERE on disk (under `developer_local`, anywhere EXCEPT the
default-deny sensitive-suffix list) -- there is no path containment at all. `..` (parent-dir) targets are rejected up front
for writes, and the default-deny suffix list still runs, but neither
confines the write to a project tree. This is acceptable for a single local
developer on their own machine. In ANY shared, multi-user, or agent-facing
deployment, an operator who enables the write lane MUST set a non-empty
`write_allow` (or run the `repo_only` profile with a `repo_root`) to confine
writes; leaving `write_allow` empty there is an open write surface.

**Operator notes on path globs.**
- Globs follow host filesystem semantics: CASE-SENSITIVE on Unix;
  case-folded with `/` and `\` treated as equivalent on Windows.
- `**` matches any run of characters INCLUDING `/` (cross-segment);
  a single `*` matches within ONE segment (stops at `/`); `?` matches
  one non-separator character. Write `**` AFTER a `/` separator
  (`/home/me/projects/**`, not `/home/me/projects**`) so it expands a
  whole subtree rather than gluing onto a partial path component.
- Subjects are matched in CANONICAL form, so author globs against the
  real on-disk path (symlinks resolved, `..` collapsed). Windows
  verbatim prefixes, alternate-data-stream suffixes, and trailing
  dot/space aliases are normalized before matching.

### 4.3 `[governor]` (resource governor)

A job started through Terminal Commander (argv, shell, recipe or PTY lane)
runs under a per-job memory ceiling and a CPU priority, enforced by the
kernel. The thing that enforces is the thing that reports: a start never
silently drops a limit, it reports the mechanism or says why there is none.

```toml
[governor]
enabled = true                      # false: no limits, responses as before
default_job_memory = "60%"          # "<n>%", "24GiB", "512MiB", bytes, or "none"
default_priority = "below_normal"   # idle | below_normal | normal
llm_can_raise_limits = true         # omitted: profile default (below)
host_ceiling = "97%"                # daemon-wide ceiling all jobs share; same syntax
```

- **Percent** resolves against the commit limit on Windows and against total
  memory (`MemTotal`) on Linux. The Windows commit limit includes the page
  file, so 60% of it can exceed physical RAM. If host memory cannot be read,
  the percent default resolves to no limit and `policy_status.governor.note`
  says so.
- **`host_ceiling`** is installed once at daemon start and every governed job
  (every lane, shell sessions included) joins it, so jobs together can never
  exceed it: a parent Job Object on Windows, a parent `tc-jobs` cgroup on
  Linux. `default_job_memory` is clamped to it, and a request above it is
  clamped to it and listed in `limits_clamped`. Where the host has no
  aggregate primitive (rlimit) `host_ceiling_mode` is `{"unavailable": reason}`;
  nothing is faked. `"none"` installs no ceiling. The ceiling is per daemon:
  N session daemons on one host can together commit N times it.
- **`llm_can_raise_limits`** defaults to true only under `full_access` and
  `admin_debug` (an allow-list: every other profile defaults to false). When
  false, a request that asks for more than the
  default (a larger memory ceiling, a higher priority) is clamped to the
  default and the start response lists the clamped axes in `limits_clamped`.
  A request at or below the default is always honoured.
- **Under rlimit** the profile default memory is not applied (`RLIMIT_DATA`
  breaks sanitizers and large reservations and cannot be raised back); an
  explicit `limits.memory` is applied, the default priority still is, and
  `policy_status.governor.note` says so.
- **Requests** carry an optional `limits` object on `command_start_combed`,
  `run_and_watch`, `shell_exec`, `recipe_run` and `pty_command_start`:
  `{"memory": "24GiB" | "40%" | "none", "priority": "idle" | "below_normal" | "normal"}`.
  An omitted axis takes the default. A start response reports what the job
  runs with as `limits_applied` (`memory_bytes`, `priority`) and the enforcing
  mechanism as `governor`, read from the probe right after spawn; a start
  collapsed onto an in-flight duplicate reports the same values (the limits
  are part of the duplicate fingerprint). With `enabled = false` neither
  `limits_applied` nor any governor field appears, and no kernel mechanism is
  probed.
- **`policy_status.governor`** reports `enabled`, `mode_available` (what this
  host can enforce, detected at daemon start), `default_job_memory_bytes`,
  `default_priority`, `llm_can_raise_limits`, `host_ceiling_bytes`,
  `host_ceiling_mode` and an optional `note`.
- **Status and run_and_watch results** of a governed job report `governor`
  (the mechanism: `job_object`, `cgroup`, `rlimit`, or `{"unavailable": reason}`
  when no enforcement was possible and the job ran ungoverned) and
  `limits_applied`, while running and after exit; after exit also
  `peak_memory_bytes` when the mechanism can measure it, and
  `exit_reason`: `"memory_ceiling"` when the job's own ceiling stopped it, or
  `"host_ceiling"` when the daemon-wide host ceiling did while the job's own
  limit was not reached. How each is decided differs by platform, see the
  mechanisms below. A stop
  (`command_stop`, `pty_command_stop`) never carries `exit_reason`. A job that
  ran ungoverned reports `governor: {"unavailable": reason}` and is audited.
  A governed job that failed to join an installed host ceiling reports
  `host_ceiling_joined: false`; the field is omitted otherwise.
- **Audit:** a clamped start writes `governor_clamp` with the requested and
  applied values; a job that runs ungoverned writes `governor_unavailable`
  with the reason, and so does a job that failed to join the host ceiling
  (reason `host_ceiling_join_failed`); a memory_ceiling exit writes `governor_memory_ceiling` and a
  host_ceiling exit writes `governor_host_ceiling`, each with limit and peak.

Mechanisms and their limits:

- **Windows (`job_object`):** a Job Object memory limit on the whole process
  tree. The OS makes commits above the limit fail; it does not kill the job.
  `exit_reason` is decided by the kernel's `JOB_OBJECT_MSG_JOB_MEMORY_LIMIT`
  messages, delivered per job through one I/O completion port, together with
  an abnormal exit. They are facts attributed to the exact job: a single
  oversized allocation refused outright counts even though it never raised
  the peak, and repeated host-ceiling rounds are all seen. The host ceiling
  is a parent Job Object the per-job Job Objects nest under; its messages
  decide `host_ceiling`. `peak_memory_bytes` is reported only.
- **Linux `cgroup`:** the daemon creates a per-job cgroup v2 with `memory.max`
  (swap off) under the `tc-jobs` parent that carries the host ceiling, a
  sibling of its own cgroup, and moves the job into it, so the ceiling covers
  the whole tree,
  and the kernel's OOM kill is what stops it. `memory_ceiling` is a fact:
  the job cgroup's `memory.events.local` `oom` count is above zero and the
  exit was not success. `host_ceiling` stays an inference: the host cgroup's
  local `oom` count rose during this job and the job failed, so a concurrent
  job at the ceiling can be the real cause. This needs the daemon inside a
  writable delegated cgroup, which the systemd user unit installed by
  autostart provides. The cgroup-or-rlimit choice is made once at daemon
  boot; a later per-job cgroup failure is reported `unavailable`, never a
  silent switch to rlimit. A daemon started from a plain `wsl.exe` shell sits in
  `/non-systemd` and gets `rlimit` instead. Peak comes from `memory.peak`
  (kernel 5.19 or later). Nothing needs installing per distro for memory
  governance.
- **Linux `rlimit`:** when no writable cgroup is available (a shell-profile
  autostart, WSL without systemd), the job gets `RLIMIT_DATA`. It is per
  process, not summed across the tree, and there is no peak.
- **macOS and other unix:** no memory primitive, so memory reports
  `unavailable(reason)`; priority is still applied (nice).
- **Windows PTY lane:** a governed ConPTY child runs in a
  `KILL_ON_JOB_CLOSE` Job Object that is released when the child exits. A
  child started with `CREATE_BREAKAWAY_FROM_JOB` fails inside it.
- **CPU limits** are not in this version; `priority` only sets scheduling
  priority (Windows priority class, Linux nice; `normal` inherits). CPU caps
  on Linux would need a root systemd `Delegate` drop-in.
- **A guardrail, not a security boundary.** A job running as the same user
  can lift its own cgroup ceiling or start work outside the Job Object (WMI,
  `schtasks`, `wsl.exe`); the clamp governs the request, not a hostile job.
  Work handed to `wsl.exe` or docker from Windows runs outside the Job
  Object.

## 5. Default-deny override mechanism

None. Default-denied paths (see `SECURITY.md` section 5) cannot be
re-allowed from config: there is no `allow_override` key, and a
`[paths.allow_override]` or `[policy.paths.allow_override]` table is an
unknown key that the daemon ignores and names in `config_warnings`. To
reach a default-denied path, use a profile that does not apply the
default-deny list (`full_access`).

## 6. Decision algorithm (informative)

Given request `(actor, action, subject, profile)`:

```text
1. (No per-request profile check: an unknown `profile` name fails config
   parsing, so the daemon does not start; there is no profile version.)
2. If action is in section-4 gated list:
   a. If subject path matches default-deny -> deny
      ("default_deny_match").
   b. If action is command_* and command argv[0] is in commands.deny
      -> deny ("command_denied").
   c. If action is probe_create and kind in probes.deny_kinds
      -> deny ("probe_kind_denied").
   d. If action is registry_activate and llm_can_activate is false
      and actor is mcp -> deny ("registry_activate_requires_admin").
      Shipped recipe gate (separate from rules): open under the default
      `full_access`; on a hardened profile `[policy] llm_can_activate_recipes`
      defaults false. Recipe admin is the peer image basename
      `terminal-commander` (not `terminal-commander-mcp`, not
      `terminal-commanderd`). `from_mcp` defaults true, so omitting it is
      not a grant, and the MCP adapter image cannot claim admin by clearing
      the field. Other peers are denied with `recipe_activate_requires_admin`
      unless the operator sets `llm_can_activate_recipes` (that opt-in is
      unchanged). The operator CLI (`terminal-commander recipes activate`,
      `recipes deactivate`, `recipes tombstone`, `recipes import --activate`)
      may activate because of its image. Activations are global-only.
      Residual: same-user code can exec that CLI, or a binary whose file
      name is `terminal-commander`. The socket is not a privilege boundary.
      `recipe_run` of an already-activated recipe follows the normal argv
      command policy.
   e. Evaluate the per-action path allow list (`paths.read_allow` for
      file_read, `paths.watch_allow` for file_watch). The list is
      OPT-IN, with the SAME posture as the command allow-list
      (`commands.allow_roots`, section 4.2):
        - an EMPTY / unconfigured list is "not enforced" -> ALLOW
          (zero-config stays usable; the structural layers in 2a above
          still apply);
        - a NON-EMPTY list is AUTHORITATIVE -> a subject that matches
          no glob is denied ("no_allow_rule").
      The structural default-deny layers run REGARDLESS of the allow
      list: the default-deny sensitive-suffix check and `paths.deny_extra`
      (both step 2a), `commands.deny` (2b), and -- for `repo_only` --
      $REPO_ROOT containment, are evaluated whether or not an allow list
      is configured, and DENY beats ALLOW. The allow list is a TIGHTENING
      layer (it can only narrow), never a widening one.
      SECURITY: file_read / file_watch subjects are matched in CANONICAL
      form (`..` / `.` collapsed) before any allow / deny glob runs, so a
      `..` prefix cannot lexically satisfy an allow glob.
3. If decision is allow, check limits (jobs, rates, sizes); if
   exceeded -> deny ("limit_exceeded").
4. Emit audit record BEFORE executing the gated action.
5. If decision is deny, return policy error to caller and end.
6. Execute action; emit result audit record (success or error).
```

This is the algorithm TC22 implements. TC29 fuzz-like tests target
each branch.

**PTY password prompts.** TC44 is unchanged and not a profile knob: while a
PTY job is at a sudo/ssh/password prompt, `pty_command_write_stdin` is
denied (`SecretInputDenied`) in every profile, so the model can never type a
password. The job reports `awaiting_credential`, and `credential_request`
makes the daemon ask the OWNER directly (a one-shot `127.0.0.1` page the MCP
client links the owner to via URL-mode elicitation, else a native dialog the
daemon opens, else the admin CLI `terminal-commander credential provide
<job_id>`). The loopback page is the one exception to the local-socket-only rule
(127.0.0.1 only, single-use token path, at most 300 s, Host-checked; the
value goes only to the waiting child, is overwritten best-effort (OS and
browser copies are not wiped), and is never returned over IPC). The
daemon types the owner's answer itself; the model sees only a status, and
the audit row `credential_provided` records the job, prompt kind, and source,
never the value or its length. The IPC `credential_provide` is accepted only
from the `terminal-commander` peer image outside the daemon's process tree
(the recipe-admin identity check); an MCP-labelled or daemon-started peer is
denied and no policy setting opens it to the model. Same-user code can still
exec that CLI: the socket is not a privilege boundary.

## 7. What policy does NOT cover (MVP)

- **Content-level redaction.** Policy decides whether a path can be
  read; it does NOT scan content for secrets. Content-scrubbing is a
  separate concern (post-MVP).
- **Outbound network egress.** TC has no outbound network in MVP.
  When the helper or MCP transport gains network capability, policy
  MUST be extended.
- **Per-rule policy.** Rules in the registry are validated (TC09)
  but the rule itself does not carry policy decisions; the daemon's
  active profile decides.
- **Multi-actor authorization.** Profiles do not currently encode
  multiple MCP clients with different rights. Each TC daemon serves
  one actor (one MCP client) at a time.
- **Time-of-day or session-length quotas.** Out of MVP scope; only
  `admin_debug` has a session-length default.

## 8. Conformance check

A goal that adds behavior MUST be able to answer YES to:

1. Does the new code path go through TC22's policy engine for every
   gated action it introduces?
2. Does every decision emit an audit record before the action?
3. Does the new code path respect `commands.shell_passthrough = false`
   (no joined-string shell invocation)?
4. Is every path canonicalized before its policy gate, and does the
   caller access the same canonical path that was authorized?
5. Is the new behavior testable under `read_only_observer` (negative
   test: it MUST be denied there if it is a write-class action)?

If any answer is NO, the goal is out of conformance and must either
amend this document or stop.
