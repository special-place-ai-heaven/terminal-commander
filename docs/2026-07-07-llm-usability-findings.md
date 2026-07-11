# TC field report — policy defaults & schema ergonomics block "run any command"

**Date:** 2026-07-07
**Reporter:** Claude (Opus 4.8), driving TC as an LLM agent during a real ops session (WSL disk cleanup)
**TC version:** adapter/daemon `0.1.73`, mcp_spec `2025-11-25`, profile `DeveloperLocal` (the default)
**Platform:** Windows 11 (daemon on Windows side; pty backend `windows_conpty`)

## TL;DR

The observe-and-signal engine — TC's core thesis — **works**. On a real failing command
(`wsl --manage … --set-sparse true`) TC returned 4 clean signals including the load-bearing
cause line and a correct `state: failed` / `exit_code: -1`, instead of a wall of output. That
is the value proposition delivering.

But three things sit **between the LLM and that engine** and, in combination, mean TC
**cannot "run any command" out of the box**. All three reproduced live this session. The
operator's verdict: *"policy defaults and schema ergonomics sitting between the LLM and that
engine — fine, as long as we address this. The end result is failure. We must properly
address it."*

---

## Finding 1 — `allow_shell: false` in the default profile defeats "run any command" (P1)

The `#[default]` profile `DeveloperLocal` ships with shell denied:

```rust
// crates/daemon/src/policy.rs
pub enum PolicyProfile {
    #[default]
    DeveloperLocal,      // <- default
    ...
}
// DeveloperLocal caps:
caps: PolicyCaps {
    allow_shell: false,      // <- shell lane OFF by default
    allow_session: false,
    allow_privileged: false,
    allow_remote: false,
}
```

**Reproduction (3 confirmations):**

1. `policy_status` → `{"caps":{"allow_shell":false,...},"profile":"DeveloperLocal"}`
2. `system_discover` → `shell_exec: { available:false, reason:"allow_shell capability is off in the active policy profile" }`
3. Live call: `command{action:"exec", shell_line:"echo hello | tr a-z A-Z"}`
   → `MCP error -32602: daemon ipc error [PolicyDenied]: shell_exec denied: allow_shell capability is off or profile forbids shell`

**Impact.** The argv lane (`run_and_watch` with `argv:[...]`) works, but the moment a task needs a
pipe, `&&`, or a redirect — i.e. a large fraction of real shell work — the default profile
refuses. The product's stated mission is "run *any* command and observe output"; the shipped
default runs only argv-vector commands. This is the gap.

**The comb pipeline is the safety layer.** `shell_exec` still routes every line through the
same bounded, combed, never-raw pipeline as argv commands (per the `TC49` comment in
`policy.rs`). So enabling it does not bypass TC's output discipline — it only unlocks the
shell grammar. The deny-by-default posture protects against `argv[0]` deny-list evasion via
`shell_line` (COMMANDS_DENY is argv[0]-only, "accepted residual risk, Decision 1"), but that
is a *different* risk than raw-output flooding, and it's the one thing standing between TC and
its own mission.

**Ask.** Make the default posture actually run commands. Options for the TC agent to weigh:
- Flip `DeveloperLocal` default to `allow_shell: true`, and close the `shell_line` deny-scan
  gap (Decision 1) so the deny-list also covers shell lines — removing the stated reason shell
  is off.
- Or ship a distinct default profile intended for LLM agents where shell is on behind the comb
  pipeline, and document `DeveloperLocal` as the hardened opt-in.
- Either way: **out of the box, `echo x | tr` must work**, or the tagline is false.

---

## Finding 2 — per-action field-subset mismatch forces multi-roundtrip trial-and-error (P2)

The `command` facade advertises a **superset** schema (every field on one object), but each
`action` accepts a strict **subset** and hard-rejects the rest. An LLM composing a single call
gets whack-a-mole. Reproduced building ONE `exec` call — three consecutive rejections:

1. `command{action:"run_and_watch", shell_line:"…"}`
   → `missing required field(s): argv; does not accept field(s): shell_line. Field 'shell_line' is accepted by action 'exec'.`
2. `command{action:"exec", shell_line:"…", compact:true}`
   → `does not accept field(s): compact. Field 'compact' is accepted by action 'events'.`
3. `command{action:"exec", shell_line:"…"}` → finally reaches the daemon (the PolicyDenied above).

**What's good.** The error messages are genuinely excellent — each names the offending field
*and* which action does accept it. That's the right instinct.

**What still costs.** It's 3 API round-trips to discover that `run_and_watch` ≠ `exec` ≠
`events` in accepted fields — because the schema doesn't tell the model the per-action
required/accepted set up front, and presentation-only fields (`compact`) hard-error instead of
being ignored where inapplicable.

**Ask (either/both):**
- Encode per-action required/accepted fields in the JSON Schema (e.g. `oneOf` variants keyed
  by `action`, or per-action `required`), so a conformant client composes it right first try.
- For purely presentational fields like `compact`, **silently ignore** them on actions that
  don't use them instead of `-32602`. A cosmetic hint should never fail a call.

---

## Finding 3 — no elevation path: privileged commands dead-end instead of prompting (P1)

TC currently has **no way to run a command that needs admin/root**. It surfaces the OS denial
and stops.

**Reproduction:** `command{action:"run_and_watch", argv:["net","session"]}` (needs admin)
→ `exit_code:2`, `state:"failed"`, one signal: `"Access is denied."`

`system_discover` shows the intended slot is stubbed:
`privileged_helper: { available:false, reason:"threat_review_pending" }`
and `allow_privileged: false` in the default caps.

**Real-world bite.** This session's actual goal — compacting the WSL `ext4.vhdx` to reclaim
~239 GB — **could not be done through TC** because `diskpart compact vdisk` requires
elevation. It had to be run outside TC via a manual self-elevating
`Start-Process -Verb RunAs`, which popped a UAC prompt the operator confirmed, and it worked
cleanly. An LLM doing genuine ops work hits "needs admin" constantly; right now TC is a
spectator for all of it.

**Operator's explicit ask (verbatim intent):**
> *"if you call an admin command in TC, it should launch a simple admin popup I can confirm,
> giving it proper access."*

i.e. the desired behavior is exactly the pattern that worked manually: TC detects a privileged
command, triggers a **UAC elevation consent prompt** (Windows `runas`/UAC; `pelevated helper`
on the daemon side), the human clicks yes, and TC re-runs the command elevated — still through
the comb pipeline, still bounded/never-raw. This is what the `privileged_helper` slot is for;
it's blocked on `threat_review_pending`.

**Ask.** Unblock the privileged-helper design with a **human-in-the-loop consent** model:
- On a privileged command (and only with `allow_privileged` on the profile), TC requests
  elevation via the OS consent UI (UAC on Windows / `pkexec`-style on Linux) rather than
  failing.
- The elevated child's stdout/stderr still flow through the same combing/bounded pipeline —
  elevation changes *who* runs the command, not TC's output discipline.
- The UAC prompt itself is the security gate (the human must physically confirm), which
  directly answers the `threat_review_pending` concern: consent is not silent.

### Directive: failure is not acceptable — build the OS-native workaround

The operator was explicit: **"failure is not acceptable, so we must work around that in a safe
way and appropriate for the OS this is running on."** So finding 3 is not "document a
limitation" — it is "ship a per-OS elevation path so an admin command *succeeds* (with consent)
instead of dead-ending."

TC already branches per `target_os` (ConPTY on Windows, unix elsewhere across
`pty_command.rs`, `command.rs`, `shell_session.rs`, `ipc/*`), so a per-OS elevation helper
slots into the **existing** architecture rather than bolting on. Concrete design, per platform:

**Windows (this host).** Mirror exactly what worked manually this session — the elevation is a
first-class OS primitive:
- Detect the need: either the profile/command is flagged privileged, or a first non-elevated
  attempt returns the OS's elevation-required signal (`ERROR_ELEVATION_REQUIRED` / access
  denied on an admin op).
- Elevate via `ShellExecuteW`/`ShellExecuteExW` with the **`"runas"` verb** (or
  `CreateProcess` + the elevation COM moniker). Windows itself renders the UAC consent dialog;
  the human clicks Yes. This is precisely the
  `Start-Process <cmd> -Verb RunAs` that ran `diskpart compact vdisk` cleanly here.
- Because the elevated child cannot inherit the daemon's pipes across the UAC boundary the same
  way, capture its output via a **named-pipe / temp-file bridge** the daemon owns (TC already
  runs a `pipe_server` on Windows) and feed that back through the comb pipeline. Output
  discipline is preserved; only the process token changes.

**Linux/unix.** Same shape, native mechanism:
- Prefer `pkexec` (polkit) so a **graphical/polkit consent prompt** appears for the human,
  matching the UAC pattern. Fall back to `sudo` only in a TTY/PTY context where TC can drive
  the password prompt through its existing secret-prompt handling
  (`pty_command_write_stdin` already refuses writes "while a secret prompt is active" — reuse
  that machinery).
- Elevated child's stdout/stderr flow through the same bounded/combed pipeline.

**Invariants that make it safe (answers `threat_review_pending`):**
1. **Consent is never silent** — the OS's own consent UI (UAC / polkit) is the gate; TC never
   fabricates or auto-approves elevation.
2. **Gated by `allow_privileged`** on the active profile — off by default; the workaround is
   opt-in, not ambient.
3. **Output discipline unchanged** — elevation changes the token, not the never-raw/bounded
   contract. The comb pipeline still owns everything the LLM sees.
4. **Audited** — the privileged request is written to the audit lane before the consent prompt
   fires (TC already audits `file_write` before acting; extend that to elevation).

Net: an LLM asking TC to run `diskpart compact`, `net session`, `systemctl restart`, etc.
gets a **consent popup and then a successful, combed result** — never a bare `exit_code:2 /
Access is denied`. That is the "no-failure, OS-appropriate, safe" behavior the operator
requires.

---

## What is NOT broken (so the agent doesn't chase ghosts)

- **The signal engine.** Combing, bounded output, `state`/`exit_code` accuracy, the quiet-command
  receipt, `compact` projection — all behaved correctly on every run this session.
- **Honesty of `system_discover`.** Every unavailable tool reported a precise `unavailable_reason`
  (unix-only sessions, caps off, threat-review-pending). The introspection is trustworthy; the
  problem is what the *defaults* deny, not that TC lies about them.
- **Platform gating of `shell_session_*` / `workspace_snapshot_*`** on Windows is honestly
  reported as unix-only — a known limitation, not a regression. Not in scope here.

## Suggested priority

| # | Finding | Priority | Type |
|---|---------|----------|------|
| 1 | `allow_shell:false` default defeats "run any command" | **P1** | policy default + Decision-1 deny-scan |
| 3 | No elevation path; privileged commands dead-end | **P1** | privileged_helper design (UAC consent) |
| 2 | Per-action field-subset mismatch → multi-roundtrip | **P2** | schema/validation ergonomics |

All three reproductions are copy-pasteable above and were captured against `0.1.73` on
2026-07-07.
