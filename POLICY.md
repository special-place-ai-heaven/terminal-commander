# Policy Doctrine - Terminal Commander

Status: Baseline (TC02 wave 0 deliverable).
Scope: documentation only. No policy code, profile loader, or
enforcement runtime exists yet. This document defines the policy
shape that TC22 (policy engine) MUST implement, and that TC23, TC24,
TC25, TC26, TC29 MUST honor.

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

MVP ships FOUR named profiles. Profile names are stable identifiers;
goals MUST refer to them by exact name.

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

## 3. Profile selection

A daemon instance loads exactly one profile at startup, named in
`terminal-commander.toml`:

```toml
[policy]
profile = "developer_local"  # or repo_only, read_only_observer, admin_debug
profile_version = "1"
```

Profile switching at runtime is OUT OF MVP. To change profiles,
operator stops the daemon, edits config, restarts. This is a
deliberate constraint: profile changes are easier to audit when they
are restart boundaries.

## 4. Profile schema (informative; binding lands in TC22)

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
shell_passthrough = false   # never invoke /bin/sh -c "<llm string>"
require_argv_quoting = true # MCP argv lists, not joined strings

[probes]
allow_kinds = ["process", "terminal", "pty", "file", "directory"]
deny_kinds  = ["journal"]   # MVP: journal probe deferred

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
   e. Else evaluate allow lists; if no match -> deny
      ("no_allow_rule").
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
