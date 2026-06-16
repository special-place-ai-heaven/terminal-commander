# Privileged Helper Threat Review (Omni P4 / Wave 4)

Status: BLOCKED-ON-REVIEW. PLAN-ONLY. No privileged code has shipped or
will ship until THIS review is completed and signed off.

Date: 2026-06-16

Companion to: `docs/security/PRIVILEGE_MODEL.md` (the architecture
section, also marked planned), `POLICY.md` (the `allow_privileged`
capability), and `SECURITY.md` (threat model + trust boundaries).

Language: ASCII only.

> [!CAUTION]
> This is a security GATE, not a feature description. The privileged
> helper (`terminal-commander-privileged`) does not exist in the
> codebase. `system_discover.omni_status.privileged_helper` reports
> `available: false`, `reason: "threat_review_pending"`, and MUST keep
> reporting that until this review is signed off. The `allow_privileged`
> capability is wired as a config switch but gates nothing runnable.

## 1. Why this review exists

The omni program's stated goal is that an LLM agent never needs a
separate raw terminal tool. One remaining human-parity gap is
system-level operations that genuinely require root: install a named
package, restart a named service, read a bounded journal window.

The project's hardest invariant (PRIVILEGE_MODEL.md section 1) is:

> The MCP server MUST NOT directly run arbitrary privileged shell
> commands.

A privileged helper is the single feature most able to violate that
invariant if built carelessly. Per the omni spec clarification
(`specs/001-omni-completion/spec.md`, Session 2026-06-16), P4 is
explicitly excluded from the implementation run: "NO privileged code
lands until a separate security threat review is completed first." This
document is that review's required artifact. It must be completed,
reviewed, and approved BEFORE any of `terminal-commander-privileged`,
the daemon bridge, or the `privileged_*` MCP tools are written.

## 2. Attack surface

The proposed design adds a NEW trust boundary: a separate, more-
privileged process invoked by the unprivileged daemon. Each element is
an attack surface.

| Element | Surface | Primary concern |
|---|---|---|
| `terminal-commander-privileged` binary | Runs at elevated privilege (setuid root OR polkit -- the mechanism is itself a review decision). | A bug here runs as root. The binary must be tiny, single-purpose, and parse nothing it does not have to. |
| Helper IPC transport | Local socket the daemon dials to request an op. | Anyone who can reach the socket can request privileged ops. Socket must be `0600`, owned by the operator uid, in a per-user runtime dir. |
| Op dispatch | Maps an op id + params to a concrete privileged action. | An off-list op, or param injection into an on-list op, escalates. Closed allow-list + strict per-op param validation (section 3). |
| Approval token | The handle that authorizes a pending op. | A forgeable, replayable, or long-lived token defeats the human gate (section 4). |
| Daemon bridge + `privileged_*` tools | The path from the LLM to the helper. | The LLM must never be able to widen the op set, skip approval, or read the audit log. |
| Audit log | The record of what was requested and run. | Must be written BEFORE execution and never be LLM-readable (section 5). |

The threat actor of record is the LLM agent itself: assume it may be
adversarial, jailbroken, or driven by a poisoned input. The design must
hold even when the agent actively tries to escalate.

## 3. Closed allow-list rationale

The helper accepts ONLY a fixed, closed set of operation ids. There is
NO generic op, NO passthrough, NO shell. An off-list op id is refused
regardless of approval state.

Linux v1 closed set (each op validates its OWN params; no free text):

| Op id | Action | Param shape | Audit severity |
|---|---|---|---|
| `apt_install` | install named packages | list of package names (validated charset) | high |
| `apt_update` | refresh package indexes | none | medium |
| `systemctl_start` | start a named unit | one unit name (validated) | high |
| `systemctl_stop` | stop a named unit | one unit name (validated) | high |
| `systemctl_restart` | restart a named unit | one unit name (validated) | high |
| `journal_read_window` | read a bounded journal window | unit + bounded line/byte window (like `file_read_window`) | medium |

Windows v1 closed set (separate list):

| Op id | Action | Param shape | Audit severity |
|---|---|---|---|
| `winget_install` | install a named package id | one package id (validated) | high |
| `sc_start` | start a named service | one service name (validated) | high |
| `sc_stop` | stop a named service | one service name (validated) | high |

Why a CLOSED allow-list and not capabilities-as-config:

- **No shell line, ever.** The helper NEVER accepts `{ shell_line: ... }`
  or any free-text command. This is the structural defense: there is no
  code path from an op to "run this arbitrary string as root." An
  agent cannot smuggle `; rm -rf /` because there is nowhere to put it.
- **No generic `sudo`.** The default privilege path is never "run an
  arbitrary command with elevation." Compare POLICY.md section 4.1: the
  `COMMANDS_DENY` set (`sudo`/`doas`/`su`/`pkexec`/`kexec`) stays denied
  on the argv lane, and `shell_exec` deliberately is not the privilege
  path. Privilege is a SEPARATE, single-purpose, closed helper.
- **Per-op param validation.** Each op validates its own params against
  a strict shape (package-name charset, single unit name, bounded
  window). No op interpolates raw agent text into a command line; the
  helper builds the argv itself from validated, typed params.
- **Auditable surface.** A closed list is small enough to threat-model
  exhaustively. A capability-config surface is not.

The candidate v1 list above is drawn from the Wave-4 plan
(`docs/plans/2026-06-09-tc-omni-wave4-privilege-helper.md`). That plan
also floated `npm_global_install` (system-wide npm); it is NOT included
in the closed set proposed for review here -- a system-wide npm install
runs arbitrary postinstall scripts as root and is therefore closer to a
generic-exec surface than to a named, bounded op. If it is wanted, it
must be justified and threat-modeled as its own line item, not folded in
silently.

## 4. Approval-token threat model

When the helper is enabled with `require_human_approve = true`, an
allowed op does not run on request. It enters a pending-approval state;
a human operator approves it OUT OF BAND via the admin CLI; only then
does the op execute.

```text
1. LLM calls privileged_exec { op: apt_install, packages: [curl] }
2. Daemon returns { state: pending_approval, approval_id, expires_at }
3. Operator (human) runs: terminal-commander privileged approve <approval_id>
4. LLM retries privileged_exec with the minted approval_token
5. Helper validates op + params + token, then executes; combed output
   lands in the bucket
```

Properties the token MUST have (to be verified in review):

- **Out-of-band minting.** The token is minted only by the operator via
  the admin CLI (`privileged_approve`), never by any MCP tool. The LLM
  can REQUEST and can RETRY, but cannot APPROVE. This keeps the human in
  the loop as a hard gate, not an advisory one.
- **Bound to the exact op + params.** A token approves ONE op with ONE
  specific param set. A token minted for `apt_install curl` must not
  authorize `apt_install evil-pkg`, `systemctl_stop firewall`, or the
  same op with different packages. Approval is for what the operator
  actually saw.
- **Single-use.** A token is consumed on execution. It cannot be
  replayed to run the op twice.
- **Short-lived.** Tokens carry an `expires_at` and are rejected after
  it. A token that leaks is useful only briefly.
- **Unforgeable.** Tokens are unguessable (CSPRNG) and validated by the
  daemon/helper, not trusted from the caller's claim.

Residual risk to document in review: operator approval fatigue. If a
host enables an auto-approve list for low-risk ops, that list must be
admin-profile-only, narrow, and itself audited; it must never include a
high-severity op.

## 5. Audit-before-exec requirement

Every privileged op MUST emit a high-severity audit record with a
redacted subject BEFORE execution -- identical in discipline to the
command/shell/session lanes (POLICY.md section 1: "every decision emits
an audit record before the gated action runs").

- The record names the op id, the validated params (credential-shaped
  values redacted), the actor, the approval state, and the decision.
- A denied request (off-list op, cap off, missing/expired/mismatched
  token) is audited as a deny BEFORE returning the refusal.
- The audit log is owned by the daemon, is `0600`, and is NEVER readable
  by the LLM (PRIVILEGE_MODEL.md section 8). The operator reviews it via
  `terminal-commander audit`.

## 6. Why NO generic sudo / NO shell line

This is the load-bearing decision and the reason the helper can exist at
all without breaking the headline invariant.

- A generic `sudo`/`pkexec` bridge, or a helper that accepts a shell
  line, would make the MCP boundary a path to arbitrary root execution.
  That is exactly what PRIVILEGE_MODEL.md section 1 forbids.
- A closed, named, param-validated op set has a finite, enumerable blast
  radius. The worst an adversarial agent can do is request an on-list op
  on a bad-but-validated target (e.g. stop a named service it should not
  -- which is why service ops are high-severity and approval-gated), NOT
  run novel code as root.
- The helper builds every privileged argv itself from typed params. The
  agent supplies DATA (a package name, a unit name), never a COMMAND.

## 7. Decision: BLOCKED-ON-REVIEW

Status of each element as of this document:

| Element | Status |
|---|---|
| `terminal-commander-privileged` binary | NOT WRITTEN. Plan-only. |
| Privilege mechanism (setuid vs polkit) | UNDECIDED. A required review decision. |
| Helper IPC protocol | NOT WRITTEN. |
| `privileged_exec` / `privileged_list_ops` / `privileged_approve` | NOT WRITTEN. No such MCP/CLI surface ships. |
| `allow_privileged` capability | Wired as a config switch (default false); gates nothing runnable. |
| `omni_status.privileged_helper` | Hard-coded `available: false`, `reason: "threat_review_pending"`. |

No privileged code is written until this review is completed, reviewed
by a security reviewer, and explicitly approved by the operator/owner.
On sign-off, the helper architecture section in
`docs/security/PRIVILEGE_MODEL.md` flips from "planned" to a shipped
contract, the Wave-4 goal chain (TC61-TC65) is unblocked, and
`omni_status` begins reporting real availability.

## 8. See also

- `docs/security/PRIVILEGE_MODEL.md` section 5 (pre-bound helper
  constraints) and the helper architecture section (planned).
- `POLICY.md` section 4.1 (`allow_privileged`, the cross-profile
  `COMMANDS_DENY` residual risk).
- `docs/plans/2026-06-09-tc-omni-wave4-privilege-helper.md` (the source
  plan this review gates).
- `specs/001-omni-completion/spec.md` User Story 4 (P4 acceptance
  scenarios; "denied by default, approval-gated when enabled").
