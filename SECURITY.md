# Security Doctrine - Terminal Commander

Status: Baseline (TC02 wave 0 deliverable).
Scope: doctrine. The policy engine (`crates/daemon/src/policy.rs`), the
persistent audit log and the command executor are shipped; no privileged
helper ships (plan-only, see `docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md`).
This document defines the threat model, trust boundaries, sensitive
operations, denied paths, and audit expectations that all later goals
MUST honor.

Language: ASCII only. No smart quotes, no em-dashes.
Note (2026-10-05): the `README.md:294-297` citation below points at the
README as of the TC01 baseline; the list in section 5 is the authority and
matches `DEFAULT_DENY_PATH_SUFFIXES` and the tree/prefix lists in
`crates/daemon/src/policy.rs`.

## 1. Purpose and audience

Terminal Commander (TC) is a local, MCP-operated terminal and file
signal-combing layer. The LLM-facing MCP server can run commands,
read terminal streams, and watch files on behalf of an agent. This
power must be bounded by explicit policy and auditable.

This document tells:

- contributors (human or AI) what is in scope and out of scope for
  the MVP security posture;
- operators what guarantees the MVP provides and what it does not;
- downstream goals (TC09, TC10, TC15-TC25, TC29) which invariants
  they MUST preserve.

## 2. Trust model

### 2.1 Threat actors considered

| Actor | Trust level | Notes |
|---|---|---|
| The local operator (human running TC) | Trusted | Owns the host; can read TC's audit log, edit config, restart the daemon. |
| The LLM client over MCP | Semi-trusted | Authoring intent is unverified. May try (accidentally or via prompt injection) to run dangerous commands, read secrets, or exfiltrate output. `credential_request` keeps a password out of every TC surface on the assumption that TC is the model's only way to run programs; a harness that also gives the model a raw shell can pipe `terminal-commander credential provide`, which the audit row records as `cli-stdin` (a typed answer is `cli-tty`). |
| A compromised probe child process | Untrusted | A process spawned by TC can be malicious or buggy. Its output is data, not commands. |
| A remote network attacker | Out of scope | TC has no network-reachable listener. The MCP transport is local stdio (rmcp 3.4.1). The one TCP socket is the owner's password page for `credential_request`: bound to 127.0.0.1 only while a PTY password prompt is pending, single-use token, 300 s. The daemon overwrites its copy of the password best-effort (OS and browser copies are not wiped). A local HTTP-inspecting proxy or filter on 127.0.0.1 (for example AdGuard) can observe the loopback POST: that is the accepted trade-off of the URL channel; `terminal-commander credential provide` avoids it. |
| A privileged co-tenant on the host | Out of scope | TC does not defend against root or kernel attackers on the same host. |

### 2.2 Threats considered

The MVP threat model centers on three threat classes:

1. **Accidental misuse by the agent.** The LLM issues commands or
   probe requests it should not. Mitigation: policy gates at every
   command/probe entry, bounded outputs, default-deny sensitive paths.

2. **Prompt-injection-driven misuse.** A noisy terminal stream or a
   watched file contains text that an LLM may interpret as
   instructions. TC's outputs to the agent are STRUCTURED signal
   events with summaries, not raw stream dumps; raw context is
   bounded and explicitly requested.

3. **Operator misconfiguration.** An operator activates an over-broad
   policy profile or installs a rule with a runaway regex. Mitigation:
   opt-in hardened profiles (the default `full_access` inherits the
   harness's trust, see `POLICY.md` section 2.5), rule validation, regex
   safety checks (TC10/TC29), audit trail.

The MVP threat model does NOT center on:

- A fully compromised local process. Once an attacker has arbitrary
  code execution as the TC user, advisory enforcement (per TC02
  policy doctrine; see `POLICY.md`) is bypassable. Kernel enforcement
  (Landlock, seccomp-bpf) is documented as a post-MVP hardening
  roadmap, not an MVP guarantee.
- Side-channel or timing attacks against the daemon.
- Supply-chain compromise of upstream crates. Mitigated separately by
  `cargo-deny`, `cargo-machete`, and the pinned `rmcp = "=3.4.1"`.

## 3. Trust boundaries

TC is a two-process system. Boundaries below MUST hold for every
goal that adds behavior touching them.

```text
+-------------------+      MCP stdio      +-------------------------+
|  LLM client       | <-----------------> |  terminal-commander-mcp |
| (Claude Code,     |  rmcp 3.4.1         |  thin adapter           |
|  Codex, Cline...) |                     |                         |
+-------------------+                     +-------------------------+
                                                     |
                                          local IPC (UDS / named pipe)
                                                     v
                                          +-------------------------+
                                          |   terminal-commanderd    |
                                          |   (daemon)               |
                                          +-------------------------+
                                                     |
       +---------------+-----------+-----------+-----+-----+----------------+
       v               v           v           v           v                v
  process probes  file probes  PTY probes  registry   bucket mgr     context spool
                                                       +---+
                                                           |
                                                           v
                                                    policy engine
                                                    audit log
```

Boundary list (each is a hard contract):

| # | Boundary | Direction of trust | What must hold |
|---:|---|---|---|
| B1 | LLM client / MCP server | Down | The MCP server NEVER trusts the client to bypass policy. Every tool call is mediated. |
| B2 | MCP server / daemon | Down | The MCP server holds no privilege the daemon does not grant. The MCP server CANNOT execute commands directly; it forwards requests over IPC. |
| B3 | Daemon / probe child process | Down | Probe processes are spawned by the daemon under a defined policy profile. Their stdout/stderr are data, never executed as commands. |
| B4 | Daemon / file system | Down (default-deny) | Every read or watch is path-checked. Denied paths fail closed with an audit record. |
| B5 | Daemon / signal buckets | Down (bounded) | Bucket entries are structured events with summaries and pointers, NOT raw stream copies. Raw context is fetched explicitly via `event_context` and is bounded by `before`/`after` limits. |
| B6 | Daemon / registry | Down (validated) | Rules entering the registry MUST pass validation (TC09) and regex safety checks (TC29) before activation. |
| B7 | Daemon / audit log | One-way append | Every privileged or policy-relevant action MUST emit an audit record before the action succeeds or fails. The audit log is append-only from TC's perspective. |
| B8 | Daemon / optional privileged helper | Up (gated) | A privileged helper (post-MVP) NEVER auto-elevates from the MCP server. Elevation requires explicit operator opt-in and is logged. |

## 4. Sensitive operations

The following operations MUST be policy-gated and audit-logged for
their entire lifecycle (start, success, failure, cancellation):

1. **Command execution** (`command_start_combed`, `shell_exec`,
   `pty_command_start`, `pty_command_write_stdin`, `command_stop`,
   `shell_session_*`, `recipe_run`). See TC15-TC16, TC19, TC22.
2. **File read, write or watch** (`file_read_window`, `file_search`,
   `file_write`, `file_watch_start`). See TC18, TC20, TC22.
3. **Probe creation** (there is no standalone `probe_create`; the
   probe-creating operations are `command_start_combed`,
   `pty_command_start` / `shell_session_start` and `file_watch_start`,
   see `POLICY.md` section 4.2). See TC21, TC22.
4. **Registry and recipe mutation** (`registry_upsert`,
   `registry_activate`, `registry_deactivate`, `recipe_upsert`,
   `recipe_activate`, `recipe_deactivate`). See TC13-TC14, TC22.
5. **Bucket export or summary** that returns raw substrings beyond
   structured event fields. See TC07, TC17, TC23.
6. **Policy or profile change at runtime** (out of MVP scope; flagged
   here so it cannot land silently).
7. **Privileged-helper invocation** (out of MVP scope; flagged here
   so it cannot land silently).

## 5. Default-deny sensitive paths

Every hardened policy profile (see `POLICY.md`) MUST inherit a base list
of default-denied path patterns; the default `full_access` profile
inherits the harness's trust and applies none. The MVP base list is anchored on
README.md:294-297 and expanded for known credential stores:

```text
# Credential stores and tokens
~/.ssh/**            # SSH private keys and config
~/.gnupg/**          # GPG private keylore
~/.pgpass            # PostgreSQL password file
~/.netrc             # Generic credentials
~/.aws/credentials   # AWS credentials
~/.aws/config        # AWS config (may contain SSO tokens)
~/.config/gcloud/**  # gcloud auth tokens
~/.kube/config       # Kubernetes context (often embeds tokens)
~/.docker/config.json # Docker registry credentials
~/.npmrc             # npm tokens (auth, _auth, _authToken)
~/.pypirc            # PyPI tokens

# System secrets (Linux)
/etc/shadow
/etc/sudoers
/etc/sudoers.d/**
/etc/ssh/ssh_host_*
/etc/ssl/private/**

# Browser / app session stores
~/.mozilla/**
~/.config/google-chrome/**
~/.config/chromium/**

# Common token caches
~/.vault-token
~/.config/op/**      # 1Password CLI
~/.config/bw/**      # Bitwarden CLI
```

Profiles MAY add allow-rules but MUST NOT remove a default-deny entry
without an explicit opt-in line in the profile and an audit-log
emission at probe-start time. See `POLICY.md` section 5 for the
override mechanism.

Implementation enforcement (MVP): canonical path resolution plus an
in-process policy check in the daemon before any `open()`/create/truncate.
The path returned by authorization is the path subsequently accessed;
see section 6.

## 6. Enforcement posture

### 6.1 MVP enforcement: advisory in-process checks

Per `docs/research/policy-prior-art.md` and the locked decision in
`docs/research/_USER_DECISIONS.md`:

> Policy enforcement (MVP): Advisory in-process + audit log.
> Kernel-level Landlock/seccomp-bpf documented as post-MVP hardening
> roadmap.

This is the honest framing all goals MUST use. The MVP daemon:

- evaluates policy in-process before every gated action (section 4);
- writes an audit-log record (section 7) for every gated action;
- canonicalizes existing read/watch targets and existing write targets,
  canonicalizes write parents for new files, rejects write `..` components,
  and applies policy before opening, creating, or truncating;
- denies sensitive paths by default (section 5);
- bounds raw stream exposure to explicit, sized `event_context` calls.

If the TC process itself is compromised, advisory enforcement is
bypassable. That is acknowledged, not denied.

### 6.2 Roadmap: kernel enforcement

Documented as post-MVP, NOT shipping in the chain:

1. **Landlock** (Linux 5.13+, WSL2 5.15.57.1+) compiled from the same
   advisory policy file. ABI v4+ also gives network controls.
2. **seccomp-bpf** (via `seccompiler` 0.5.x) allow-listing the syscalls
   probes are expected to perform; catches `execve`, raw sockets,
   ptrace.
3. **systemd unit hardening** at the deployment layer (bare-metal
   Linux only; WSL deployments cannot rely on systemd per
   `docs/research/wsl-boundary.md`).

The advisory layer is the source of truth; future kernel layers
derive from it.

## 7. Audit log expectations

Every gated action (section 4) MUST emit a record with at minimum:

- `audit_id` (monotonic integer),
- `timestamp`,
- `actor` (mcp client id or operator),
- `action` (for example `command_start`, `command_shell_start`,
  `pty_command_start`, `pty_command_write_stdin`, `file_read_window`,
  `file_write`, `file_watch_start`, `registry_upsert`,
  `registry_activate`, `shell_session_start`, `credential_provided`;
  rejections use `command_rejected` / `command_shell_rejected`),
- `subject` (redacted command argv or shell-line preview, file path,
  probe id, rule id),
- `profile` (name),
- `decision` (one of: `allow`, `deny`, `allow_with_audit`, `error`,
  `info`),
- `reason` (human-readable; references rule id when denied),
- `metadata_json` (bounded, redacted; for example the `nested_shell` tag).

Follow-up rows record outcomes where a lifecycle exists (for example
`command_exit`, `pty_command_exit`; `file_watch_exit` is written as an
`error` row when a watch terminates unexpectedly).

The audit log MUST be:

- append-only from TC's perspective (operator may rotate or archive);
- persisted on disk (TC12 storage backend);
- emitted BEFORE the gated action succeeds (so deny-then-act is the
  hard failure mode, not act-then-deny);
- exposed to the operator via `admin_cli` (TC25) and, read-only and
  bounded (cursor plus limit), to the MCP client through the
  `audit_since` tool (`crates/mcp/src/tools.rs`; added in commit
  9a335c2, pinned by `tests/fixtures/contracts/mcp-tools/audit_since.v1.json`).
  Subjects and metadata are redacted before they are written.

## 8. Data retention

MVP defaults:

| Data | Retention | Notes |
|---|---|---|
| Audit log | 30 days, rolling | Operator-tunable in `terminal-commander.toml`. Older records archived or deleted. |
| Signal events (buckets) | `[retention]` `ttl_seconds` (default 86400, 24h) and `max_events` (default 100000) per bucket | Operator-tunable in `terminal-commander.toml`. There is no `bucket_pin` tool. |
| Context spool (raw frames) | In-memory ring per probe, no time-based expiry | Default 4096 frames / 1 MiB (`DEFAULT_RING_FRAMES`, `DEFAULT_RING_BYTES` in `crates/core/src/context.rs`). Head eviction (FIFO) on overflow. |
| Registry rules | Indefinite | User assets. Deletion is explicit (TC13). |
| MCP tool-call trace | Disabled by default | Opt-in for debugging. When enabled, scrubbed of file contents and command arguments matching the default-deny list. |

No telemetry leaves the host. TC has no outbound network calls.

## 9. Out of scope (MVP)

The following are explicit non-goals for the MVP doctrine and any
goal that adds them MUST first amend this document:

- Multi-user host policy (each Unix user is its own TC instance).
- Network-exposed MCP (rmcp HTTP/SSE transport, etc.).
- A TC-provided privileged helper (plan-only). Under the default
  `full_access` profile `sudo`/`su` run as ordinary audited argv
  commands; hardened profiles deny them (`POLICY.md` section 2.5).
- Cross-host policy distribution.
- Encrypted audit log (defer to filesystem ACLs + disk encryption).
- Sandboxed probe execution (Landlock/seccomp/bubblewrap/firejail).

## 10. Goal map

Every goal that touches a sensitive operation MUST trace back to this
document. Forward references:

| Goal | Touches | Required link |
|---|---|---|
| TC09 | Rule validation | Section 4.4 + B6 |
| TC10, TC11 | Sifter runtime | Section 3 B5 (bounded outputs) |
| TC12 | Persistent event store | Section 8 retention |
| TC13, TC14 | Registry CRUD + seed | Section 4.4 + B6 |
| TC15, TC16, TC19 | Command execution | Section 4.1 + B3 |
| TC17 | Bucket waiter | Section 3 B5 |
| TC18, TC20 | File / directory probes | Section 4.2 + section 5 + B4 |
| TC21 | Daemon local API | Section 3 B2 |
| TC22 | Policy engine + audit log | All of sections 3-7 (THIS doctrine becomes executable here) |
| TC23, TC24 | MCP tool surface | Section 3 B1 + B2 |
| TC25 | Admin CLI | Section 7 audit-log access |
| TC26 | Installer / WSL startup | Section 6 hardening (documented, not enabled) |
| TC29 | Security hardening + fuzz-like | All of sections 2-7 (THIS doctrine is the test oracle) |

## 11. Open questions logged as risks

Unresolved security questions are logged in
`.agent/goals/terminal-commander-mvp/RISK_REGISTER.md` rather than
silently presumed. TC02 adds the following entries to that register:

- Sudo/privileged-helper transport (deferred; MUST be a separate goal
  before any privileged code lands).
- Audit-log tamper-resistance (operator-side concern; documented as
  post-MVP).
- Cross-process trust between MCP server and daemon over IPC
  (resolved when TC21 picks the IPC transport).
