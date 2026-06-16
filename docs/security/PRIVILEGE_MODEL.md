# Privilege Model - Terminal Commander

Status: Baseline (TC02 wave 0 deliverable).
Companion to: `SECURITY.md` (threat model + trust boundaries) and
`POLICY.md` (profile catalog + decision algorithm).

Scope: documentation only. No privileged helper, sudo bridge, or
service installer exists yet. This file pins the privilege story so
TC22, TC23, TC25, TC26, and TC29 cannot quietly elevate it.

Language: ASCII only.

## 1. Headline invariant

> **The MCP server MUST NOT directly run arbitrary privileged shell
> commands.**

This is the single hardest rule in the project. Every component
listed below exists to keep this rule true even as features land.

## 2. Process model and uid map

TC is a two-process system (per `docs/research/_USER_DECISIONS.md`):

```text
              Operator's Unix user (uid=N)
                       |
        +--------------+--------------+
        |                             |
        v                             v
  terminal-commander-mcp        terminal-commanderd
  (rmcp 1.7.0 stdio,            (long-running daemon,
   spawned per-MCP-session)      single instance per user)
        |                             |
        |    local IPC (TC21)         |
        +----------------------------->
```

Both processes run under the operator's Unix uid. Neither requires
root, setuid, or capabilities for MVP behavior. In particular:

- `terminal-commander-mcp` is unprivileged. It owns the rmcp stdio
  channel and nothing else.
- `terminal-commanderd` is unprivileged. It owns probes, the
  registry, buckets, context spool, policy engine, and audit log.
- Probe child processes inherit the daemon's uid. They get NO extra
  capability.

There is NO setuid binary in TC. There is NO suid wrapper. There is
NO polkit rule.

## 3. The MCP boundary

`terminal-commander-mcp` is a thin adapter. It:

- accepts MCP tool calls from the LLM client over rmcp stdio;
- forwards each call to `terminal-commanderd` over the IPC transport
  (TC21);
- forwards the daemon's response back over rmcp.

It does NOT:

- execute commands directly;
- open files outside what the daemon allows;
- hold any state across MCP sessions;
- access the network;
- write to the audit log directly (the daemon writes audit; the MCP
  server is itself audited as an actor).

This separation is structural, not merely conventional. The MCP
binary contains no `std::process::Command::spawn`, no
`tokio::fs::File::open` outside its own config, and no privileged
APIs. TC22 verification (and TC29) MUST include a grep-test on the
`terminal-commander-mcp` crate confirming this.

## 4. The local transport

The MCP transport for MVP is rmcp 1.7.0 stdio (per
`docs/research/mcp-transport-pattern.md` and the locked decision in
`_USER_DECISIONS.md`). Implications:

- The MCP server is launched by the LLM harness (Claude Code, Codex
  CLI, etc.) as a child process and communicates via stdin/stdout.
- There is no listening network socket. The host firewall is
  irrelevant; the kernel never opens a port for TC.
- Each MCP session has a fresh `terminal-commander-mcp` process.
- The daemon transport (MCP <-> daemon IPC) is TC21-deferred.
  Candidate transports MUST be local-only: Unix domain socket with
  `0600` perms, or anonymous pipe, or Windows named pipe under WSL.
  No `127.0.0.1:PORT` TCP listener.

This deferral is recorded in `ASSUMPTIONS.md` and as the open
question logged in `RISK_REGISTER.md`.

## 5. The privileged-helper question (deferred)

Some envisioned operations (system service install, host-wide
configuration, journald reading on Linux) MAY require root in a
future version. The MVP doctrine is:

- **No privileged helper exists in MVP.** No sudo bridge, no polkit
  rule, no setuid binary.
- **If/when a helper is added**, it MUST be a separate goal with its
  own threat model amendment, audit-log scheme, and acceptance
  criteria. The helper MUST be a small, single-purpose program
  installed by the operator (not by TC itself), invoked over an
  explicit named transport, and authorized per-call (not per-session).
- Any future helper invocation MUST be:
  - opt-in per operator (off by default);
  - explicitly named in the active profile;
  - one of a closed allow-list of operations (never a generic shell);
  - emit a high-severity audit record;
  - subject to the same TC22 policy engine as everything else.

This file pre-binds the constraint so a future goal cannot quietly
add `sudo -n ...` and call it "privileged behavior we already
documented."

## 5a. Privileged helper architecture (PLANNED -- NOT YET IMPLEMENTED)

Status: PLAN-ONLY. The omni program (P4 / Wave 4) specifies a privileged
helper but ships NO code for it. It is BLOCKED on a dedicated threat
review (`docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md`). This section
records the intended architecture so the constraints are fixed before
any code is written; nothing here is live.

What `system_discover` reports today:
`omni_status.privileged_helper = { available: false, reason:
"threat_review_pending" }`. It MUST keep reporting that until the threat
review is signed off.

The planned shape (each element gated by the review):

- **Separate binary** `terminal-commander-privileged`. The MCP server
  and the daemon stay unprivileged (section 2 is unchanged). The helper
  is a tiny, single-purpose program installed by the OPERATOR (via an
  explicit `setup privileged-helper`, never by npm install and never by
  TC itself). Its privilege mechanism (setuid root vs polkit) is itself
  a review decision, not yet made.
- **Closed allow-list, no shell.** The helper accepts ONLY a fixed set
  of named ops (Linux candidate v1: `apt_install`, `apt_update`,
  `systemctl_start`, `systemctl_stop`, `systemctl_restart`,
  `journal_read_window`; Windows v1: `winget_install`, `sc_start`,
  `sc_stop`). It NEVER accepts a shell line or a generic command. Each
  op validates its own typed params; the helper builds the privileged
  argv itself. There is no generic-`sudo` path.
- **Human-approval flow.** When enabled with `require_human_approve`,
  the LLM gets a `pending_approval` handle; a human operator approves
  out of band via the admin CLI (`privileged_approve`); only an
  approval token bound to the exact op + params, single-use and
  short-lived, authorizes execution. The LLM can request and retry but
  can never approve.
- **`allow_privileged` capability.** Gated by `[policy.caps]
  allow_privileged` (default false; POLICY.md section 4.1), evaluated by
  the same policy engine as every other action. Per-call authorization,
  not per-session. Off-list ops are refused regardless of approval.
- **Audit before exec.** Every privileged op emits a high-severity audit
  record (redacted subject) BEFORE execution. The audit log stays
  daemon-owned, `0600`, and NEVER LLM-readable (section 8).

The full attack-surface analysis, the closed-allow-list rationale, the
approval-token threat model, and the "why no generic sudo / no shell
line" reasoning live in
`docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md`. The capability matrix
in section 9 still reads `Sudo / pkexec = NO` for every component,
because that structural rule does NOT change: the helper is a closed,
named-op surface, not a sudo bridge.

## 6. Sudo / doas / pkexec posture (MVP)

The MVP `commands.deny` list in every profile (see `POLICY.md`
section 4) names these binaries explicitly:

- `sudo`, `doas`, `su`, `pkexec`, `kexec`

The daemon's policy engine refuses to spawn them even if the LLM
client requests them, and refuses to spawn ANY argv whose `argv[0]`
basename matches. The audit record reads
`decision=deny reason=command_denied`.

There is no path through the MCP boundary that bypasses this.

## 7. WSL specifics

Per `docs/research/wsl-boundary.md`:

- TC under WSL2 runs as the WSL user (typically uid=1000), inside
  the WSL distro's Linux user space.
- The WSL bridge to Windows (`/mnt/c`, `wsl.exe`, drvfs/9p) is a
  filesystem detail handled by the file-probe research; it is NOT a
  privilege boundary TC tries to cross.
- TC MUST NOT attempt to spawn Windows processes from the WSL side
  (no `wsl.exe -e ...`, no `cmd.exe /c ...`). Cross-host shelling is
  out of MVP scope.
- TC under WSL still cannot rely on systemd; the daemon is supervised
  by the user's shell or a userspace runner. See TC26 for the
  installer/startup story.

## 8. Audit-log access

`audit_log` is owned by `terminal-commanderd`. Access:

- The daemon writes records (append-only from its perspective).
- The operator reads records via `admin_cli` (TC25). The admin CLI
  talks to the daemon over the same local IPC the MCP server uses,
  but is restricted to the `admin_debug` profile (see `POLICY.md`
  section 2.4).
- The MCP client (LLM) NEVER reads the audit log.
- File-system permissions: audit log file is `0600`, owned by the
  daemon's uid. Operator may copy it under their own user once the
  daemon flushes.

Audit tamper-resistance (hash chains, off-host shipping) is OUT OF
MVP scope and documented as post-MVP in `SECURITY.md` section 9.

## 9. Capability matrix

Quick reference: which component is allowed to do what, BY DESIGN.

| Capability | LLM client | terminal-commander-mcp | terminal-commanderd | Probe child | admin_cli |
|---|---|---|---|---|---|
| Issue MCP tool calls | yes | (receives them) | (services them) | no | no |
| Spawn command | no | NO | yes (policy-gated) | no (probes are spawned BY the daemon) | yes (operator, gated) |
| Open file | no | only its own config | yes (policy-gated, cap-std) | only what it inherited as Dir handles | yes (operator, gated) |
| Write registry | no (proposal yes, activate no) | (forwards) | yes (policy-gated) | no | yes (operator) |
| Read audit log | NO | no | (writes it) | no | yes (operator, admin_debug only) |
| Open network socket | no | NO | no (MVP) | no | no |
| Sudo / pkexec | no | NO | NO | NO | NO |
| Modify profile at runtime | no | no | no (MVP: restart only) | no | no |

`NO` in caps marks rules that are STRUCTURAL in MVP (the code path
does not exist), not merely policy-denied.

## 10. Verification expectations

TC29 (security hardening + fuzz-like tests) MUST verify, at minimum:

1. `terminal-commander-mcp` contains no `Command::spawn` or
   equivalent process-spawn API call (grep test).
2. `terminal-commander-mcp` contains no `bind`, `connect`, or TCP/UDP
   listener (grep test on `tokio::net` and `std::net`).
3. Every profile parses with `sudo`, `doas`, `su`, `pkexec`, `kexec`
   in `commands.deny`.
4. The audit log is `0600` and owned by the daemon uid (filesystem
   test).
5. Attempting to spawn `sudo` via the MCP tool surface results in a
   `deny` audit record AND a policy error to the caller, and no
   process is created.
6. Attempting to read a default-deny path via `file_read_window`
   results in a `deny` audit record AND a policy error to the caller,
   and no `open()` syscall succeeds on the path.

## 11. Roadmap (post-MVP, NOT shipping in this chain)

In rough order of value, per `docs/research/policy-prior-art.md`:

1. **Landlock** ruleset compiled from the same profile (Linux 5.13+,
   WSL2 5.15.57.1+).
2. **seccomp-bpf** allow-list of probe syscalls.
3. **Audit-log hash chain** for tamper-evidence.
4. **Privileged helper** for the small closed set of operations that
   genuinely need root (e.g. service install).
5. **macOS / Windows native** policy backends.

None of these land in MVP; each requires its own goal with its own
mini-spec, threat-model amendment, and acceptance criteria.
