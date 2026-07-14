# Policy Doctrine - Terminal Commander

Status: Baseline (TC02 wave 0 deliverable).
Scope: documentation only. This document defines the policy shape that
TC22 (policy engine) MUST implement, and that TC23, TC24, TC25, TC26,
TC29 MUST honor.

Implementation status (as of 2026-05-29): PARTIALLY implemented in
`crates/daemon/src/policy.rs`. SHIPPED: the cross-profile command deny
set, the default-deny sensitive-path suffix list, and the per-profile
mutation gates (sections relating to read_only_observer / admin_debug /
registry_activate). NOT YET SHIPPED: command allow-lists, the
default-deny posture of section 6, $REPO_ROOT containment, the
declarative profile schema of section 4, the limits of section 4, and
the allow_override mechanism of section 5. WARNING: `repo_only`
(section 2.2) does NOT yet confine to $REPO_ROOT — it currently behaves
identically to `developer_local`. Do not rely on it as a sandbox. The
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
  via cap-std `Dir` handles plus in-process path/argv checks. Kernel
  enforcement (Landlock, seccomp-bpf) is a documented roadmap, not
  an MVP feature. See `docs/security/PRIVILEGE_MODEL.md`.
- Policy is **auditable**: every decision (allow, deny, error) emits
  an audit record before the gated action runs.
- Policy is **default-deny on sensitive paths** (see `SECURITY.md`
  section 5). Profiles may NOT remove a default-deny entry without
  an explicit, logged override.
- Policy is **profile-scoped**: exactly one active profile per TC
  daemon instance. Profile switching requires daemon restart in MVP.

The TC22 policy engine takes a request `(actor, action, subject,
profile)` and returns `(decision, reason, audit_record)`. Nothing in
MVP runs without going through it.

## 2. Profile catalog

MVP shipped FOUR named profiles. TC49 adds a FIFTH, `full_access`
(section 2.5), for the trusted/unrestricted case. Profile names are
stable identifiers; goals MUST refer to them by exact name.

### 2.1 `developer_local`

Intended for: a developer running TC on their own workstation
against their own repos.

```text
permits:
  - read+watch under $HOME/projects/**, $REPO_ROOT/**, and an
    operator-listed allow-set.
  - command execution under $REPO_ROOT/** with a curated command
    allow-list (cargo, npm, pytest, make, etc.; see TC14 seed pack).
  - PTY commands under $REPO_ROOT/** with prompt-detection enabled.
  - registry CRUD by the operator over the admin CLI; LLM-driven
    registry create/test allowed, activate gated.
denies (in addition to default-deny):
  - command execution outside the allow-list.
  - file reads outside the allow-set without explicit profile edit.
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
  - read+watch under $REPO_ROOT/** (rooted via cap-std Dir).
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

Intended for: a TRUSTED, single-operator machine where the agent is
explicitly allowed the full capability surface (shell, session,
privileged helper, remote). This is the convenience profile that
bundles every opt-in capability (section 4.1) ON in one declaration,
instead of listing each cap by hand on a base profile.

```text
permits:
  - everything developer_local permits (it is exec-capable and shares
    the developer_local / repo_only verdict path).
  - the gated shell lane (shell_exec), because its loader preset turns
    allow_shell ON.
  - the gated session lane (shell_session_* + workspace_snapshot_*,
    allow_session ON; LIVE, unix-only) and remote federation
    (target_id, allow_remote ON; LIVE via an operator ssh -L forward).
  - allow_privileged is preset ON but gates the Wave-4 privileged
    helper, which is PLAN-ONLY: no privileged code ships this program
    (blocked on a threat review), so the cap currently gates nothing
    runnable. See docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md.
denies (in addition to default-deny):
  - the cross-profile closed deny set (sudo/doas/su/pkexec/kexec) as
    argv[0] is STILL denied -- full_access does NOT remove COMMANDS_DENY.
limits: same as developer_local.
audit_requirements: every gated action. Capability use stays
  AllowWithAudit (no audit-off short-circuit).
```

The five binding guardrails (Decision 1; the implementation honors
each):

1. **NEVER default.** `developer_local` stays the safe default;
   `full_access` requires an explicit `profile = "full_access"` in
   TOML plus a daemon restart.
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
5. **Trusted-machine / single-operator only.** Documented as a
   trusted-host capability. Do NOT enable it on a shared, multi-tenant,
   or untrusted host. See the residual-risk note in section 4.1.

Cap semantics under `full_access`: the loader applies `base || full`,
so EVERY cap resolves ON even if `[policy.caps]` lists one as `false`.
To run a SUBSET of capabilities, do NOT use `full_access` -- use a
base profile (`developer_local`) plus explicit `[policy.caps]` entries
(section 4.1).

## 3. Profile selection

A daemon instance loads exactly one profile at startup, named in
`terminal-commander.toml`:

```toml
[policy]
profile = "developer_local"  # or repo_only, read_only_observer,
                             # admin_debug, full_access
profile_version = "1"
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
allow_shell      = true    # gates shell_exec (TC49). Default false.
allow_session    = true    # gates shell_session_* + workspace_snapshot_*
                           #   (omni P1 / TC50; LIVE, unix-only). Default false.
allow_privileged = false   # gates the Wave-4 privileged helper, NOT
                           #   generic sudo. PLAN-ONLY; no code shipped
                           #   (blocked on a threat review).
allow_remote     = true    # gates remote federation / target_id
                           #   (omni P5; LIVE via operator ssh -L forward).
                           #   Default false.
```

Rules:

- **ALL four caps default `false`.** An absent `[policy.caps]` block is
  identical to all-false. Deny-first is preserved: a capability does
  nothing until explicitly turned on.
- **Config / TOML ONLY.** Caps are NEVER MCP-flippable -- no tool can
  turn a cap on or off. Changing a cap means editing TOML and
  restarting the daemon, the same boundary as switching profiles
  (section 3). This keeps cap changes auditable at restart boundaries.
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

**Accepted residual risk (Decision 1).** The cross-profile command
deny set (`COMMANDS_DENY`: `sudo`, `doas`, `su`, `pkexec`, `kexec`) is
checked on `argv[0]` ONLY. It deliberately does NOT scan the
`shell_line` of a `shell_exec` call. Once `allow_shell` is on, a host
where `sudo` is otherwise reachable can have `sudo ...` embedded INSIDE
a `shell_line` (e.g. `echo x | sudo tee ...`) and the argv[0] deny will
not catch it. This is intended and is WHY the shell lane is a
trusted-profile capability (default-deny, opt-in, single-operator
machine) rather than an always-on surface. It is also WHY privilege
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

The argv shell-interpreter deny (`SHELL_INTERPRETERS_DENY`, `command.rs`)
catches a bare interpreter in `argv[0]` (`bash`, `sh`, `pwsh`, `cmd`, ...).
Before US8 it did NOT catch a shell smuggled through a `wsl`/`wsl.exe`
carrier: `wsl.exe -e bash -lc "<arbitrary shell>"` has `argv[0] = wsl.exe`,
which is in neither `SHELL_INTERPRETERS_DENY` nor `COMMANDS_DENY`, so both
argv checks passed while an arbitrary Linux shell ran. US8 closes that gap.

**Stance: the shell capability follows the shell across the WSL boundary.**
WSL is THIS host's boundary, not a remote machine (`allow_remote` is not
implicated). A shell reachable through `wsl.exe` is gated by the same
`allow_shell` capability that gates `shell_exec` -- default deny.

- **Inspected: argv only.** The classifier reads the argv the caller
  supplied and nothing else. File contents are never read, and there is no
  second interpreter list -- `SHELL_INTERPRETERS_DENY` is the sole
  authority, matched by basename (split on both `/` and `\` so a
  `C:\...\wsl.exe` path classifies the same on every platform).
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
is denied on the other:

| Classification | `allow_shell=false` | `allow_shell=true` |
|---|---|---|
| not a wsl carrier / WSL management flag (`--list`, `--status`, ...) / `-e` non-shell program (`cargo`, `uname`, ...) | runs (unchanged) | runs (unchanged) |
| nested shell (`-e bash`, bare `wsl bash`, `-- sh -c ...`, bare `echo $(id)`, bare `wsl.exe`) | **DENY** -- `shell_interpreter_denied`, naming the interpreter + the wsl carrier + the `allow_shell` gate / `shell_exec` remedy | runs; `command_start` audit row tagged `"nested_shell": "<interpreter>"` |
| unknown construction (novel WSL flag in payload position) | **DENY** (fail closed) | runs; audit tagged `"wsl_construction": "unknown"` |

**Rationale.** The constitution (Principle II) forbids argv smuggling and
requires the interpreter deny to stay intact in spirit, not just letter.
Adding `wsl.exe` wholesale to `SHELL_INTERPRETERS_DENY` was rejected: it
would break every legitimate non-shell use (`wsl.exe -e cargo build`,
`wsl --list`). Inspecting the Linux-side binary was rejected: argv-only is
the constitutional boundary, and file inspection is unreliable across the
WSL boundary anyway.

### 4.2 Full profile schema (informative)

```toml
[profile]
name = "developer_local"
version = "1"
description = "..."

[paths]
read_allow  = ["/home/me/projects/**", "/srv/repos/**"]
write_allow = ["/home/me/projects/**/target/**", "/tmp/tc/**"]
watch_allow = ["/home/me/projects/**"]
deny_extra  = []   # additional denies beyond default-deny list

[commands]
allow_roots = ["cargo", "npm", "pytest", "make", "ls", "git"]
deny        = ["sudo", "doas", "su", "pkexec", "kexec"]
shell_passthrough = false   # the argv command lane NEVER invokes a
                            #   joined shell string. As of TC49, shell
                            #   passthrough is its own gated lane
                            #   (shell_exec) behind [policy.caps].allow_shell
                            #   (section 4.1), default false -- NOT a flag
                            #   on the argv lane.
require_argv_quoting = true # MCP argv lists, not joined strings

[probes]
# Closed kind set {command, file_watch, pty} (the ProbeKind snake_case wire
# tags), matched case-sensitively. EMPTY allow_kinds == "not configured" ==
# allow any kind (zero-config usable); a non-empty list is authoritative (a
# kind not listed is denied, no_allow_rule). deny_kinds is a hard deny that
# beats allow (probe_kind_denied). Enforced at probe creation (TC22 A2).
allow_kinds = []          # allow all three kinds (zero-config posture)
deny_kinds  = ["pty"]     # ...but forbid interactive PTY probes in this profile

[limits]
max_jobs                   = 16
per_bucket_event_rate_evt_s = 1000
per_bucket_event_burst_evt_s = 5000
per_probe_stream_mib_s      = 10
context_spool_mib_per_probe = 64
regex_compile_ms_max        = 50
regex_step_ms_max           = 10

[audit]
log_path  = "/var/lib/terminal-commander/audit.log"
retention_days = 30
sync = "fsync_per_record"  # or "fsync_per_batch" (operator choice)

[registry]
llm_can_create   = true
llm_can_test     = true
llm_can_activate = false  # admin-CLI approval required
llm_can_delete   = false
```

The exact schema (field names, defaults, validation) lands in TC22.
This block is informative for TC03 fixture design and TC23/TC24 MCP
test coverage.

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
sharp edge for WRITES. With the DEFAULT config (profile `developer_local`,
no `repo_root`, EMPTY `write_allow`), the `file_write` tool can write
ANYWHERE on disk EXCEPT the default-deny sensitive-suffix list -- there is
no path containment at all. `..` (parent-dir) targets are rejected up front
for writes, and the default-deny suffix list still runs, but neither
confines the write to a project tree. This is acceptable for a single local
developer on their own machine. In ANY shared, multi-user, or agent-facing
deployment, an operator who enables the write lane MUST set a non-empty
`write_allow` (or run the `repo_only` profile with a `repo_root`) to confine
writes; leaving `write_allow` empty there is an open write surface.

**Operator notes on path globs.**
- Globs are CASE-SENSITIVE (`/Home/**` does not match `/home/...`).
- `**` matches any run of characters INCLUDING `/` (cross-segment);
  a single `*` matches within ONE segment (stops at `/`); `?` matches
  one non-separator character. Write `**` AFTER a `/` separator
  (`/home/me/projects/**`, not `/home/me/projects**`) so it expands a
  whole subtree rather than gluing onto a partial path component.
- Subjects are matched in CANONICAL form, so author globs against the
  real on-disk path (symlinks resolved, `..` collapsed).

## 5. Default-deny override mechanism

Default-denied paths (see `SECURITY.md` section 5) MAY be overridden
only via the `paths.allow_override` list in a profile, with EACH entry
requiring:

1. an exact path or glob (no wildcard alone, no `**` alone);
2. a justification string (free text; recorded in audit log);
3. an explicit boolean `i_understand_risk = true`.

Example:

```toml
[paths.allow_override]
entries = [
  { path = "/home/me/.npmrc",
    justification = "tooling-research dev container only",
    i_understand_risk = true }
]
```

Loading a profile with `allow_override` entries MUST emit an audit
record at daemon startup naming each overridden path. The override
applies to that profile instance only; profile reload re-emits the
audit record.

## 6. Decision algorithm (informative)

Given request `(actor, action, subject, profile)`:

```text
1. If profile is invalid or missing version, deny ("policy_invalid").
2. If action is in section-4 gated list:
   a. If subject path matches default-deny and no matching
      allow_override exists -> deny ("default_deny_match").
   b. If action is command_* and command argv[0] is in commands.deny
      -> deny ("command_denied").
   c. If action is probe_create and kind in probes.deny_kinds
      -> deny ("probe_kind_denied").
   d. If action is registry_activate and llm_can_activate is false
      and actor is mcp -> deny ("registry_activate_requires_admin").
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
4. Does any new path access go through a cap-std `Dir` rooted at an
   allowed path?
5. Is the new behavior testable under `read_only_observer` (negative
   test: it MUST be denied there if it is a write-class action)?

If any answer is NO, the goal is out of conformance and must either
amend this document or stop.
