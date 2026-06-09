# Cursor Report 03 — Vision Gap Analysis

**Subject:** How far is Terminal Commander (TC) from the stated vision?  
**Version audited:** 0.1.47  
**Date:** 2026-06-09  
**Method:** Code-grounded gap analysis (not vibes). Claims traced to source, contracts, and shipped MCP tools.

---

## The Vision (Stated Intent)

Terminal Commander should be able to do **anything a human user can do** over a terminal:

- Run commands directly, install software, wait for results, so an LLM can reason and continue.
- Act as a **Swiss knife** for any LLM that needs terminal interaction.
- **Systematically and declaratively parse** command output so only actionable results return to the LLM — saving tokens by stripping cruft, bulk, and noise.

The core loop: **run → comb → signal → reason → continue**, token-cheap.

---

## Executive Summary

| Dimension | Status |
|---|---|
| Core loop (run, wait, signal, reason, continue) | **Shipped and working** |
| Declarative parsing + token savings | **Shipped** (rules + bounded fallbacks) |
| User-space argv commands | **Shipped** |
| "Anything a human can do" | **~70–80%** — deliberately gated gaps remain |
| "Parse anything automatically" | **Partial** — rules you define, not semantic auto-extraction |

**Bottom line:** The spine of the vision is real and provably works end-to-end (e.g. `run_and_watch` → one actionable npm signal → verify → continue). The distance is not in the loop — it is in the word **ANYTHING**, plus the hardest open problem: parsing output you have not yet written rules for.

---

## What Already Works (Evidence)

### 1. Core command loop

| Capability | MCP tool(s) | Status |
|---|---|---|
| Start argv command + comb output | `command_start_combed` | Live |
| One-shot: start + wait + return signals + exit | `run_and_watch` | Live |
| Poll lifecycle / counters | `command_status` | Live |
| Kill runaway command | `command_stop` | Live |
| Rule-free bounded read (exploratory) | `command_output_tail` | Live (200 lines / 64 KiB cap) |

**Proof pattern:** `run_and_watch` with `["wsl","bash","-lc","npm install -g terminal-commander@latest"]` returns one actionable signal (`changed N packages in Xs`) instead of npm's full scrollback. The LLM reasons on that and continues.

### 2. Declarative parsing → token savings

- Inline rules on start + persistent registry (`registry_upsert`, `registry_activate`, `registry_test`).
- Curated packs via `registry_import_pack` — eight built-in packs:
  `generic.terminal`, `apt`, `cargo`, `npm`, `pytest`, `gcc`, `make`, `cleanup`.
- Quiet commands return a **bounded receipt**, never silence (`run_and_watch` completion contract).
- No *unbounded* raw stdout/stderr — structured events, cursors, and counters only; the sole raw path is the explicitly bounded `command_output_tail` (200 lines / 64 KiB, truncation-flagged) for exploratory reads.

Source: `crates/daemon/src/command.rs`, `crates/mcp/src/tools.rs`, `crates/store/src/import.rs`.

### 3. Streams, observability, subscriptions

| Surface | Tools |
|---|---|
| Buckets | `bucket_events_since`, `bucket_wait`, `bucket_summary` |
| Context | `event_context` |
| Aggregate view | `runtime_state`, `probe_list`, `probe_status` |
| Multiplexed pull | `subscription_open`, `subscription_pull`, `subscription_list`, `subscription_close`, `subscription_seek` |
| Files | `file_read_window`, `file_search`, `file_watch_start/stop/list` |
| Discovery | `system_discover`, `health`, `policy_status`, `self_check` |

**Tool count:** 38 live MCP tools (TC45 surface + `registry_import_pack` + `command_stop`).

### 4. Trust + resilience

- Four policy profiles (`developer_local`, `repo_only`, `read_only_observer`, `admin_debug`).
- Structural deny for privilege escalators across all profiles.
- Persistent audit log (V0003+), argv redaction, bounded envelopes.
- Degraded-but-job-identified results on IPC blips (`run_and_watch` TC-1b preserve `job_id` on transport failure).
- Per-harness session isolation via `TC_SESSION` (one daemon per harness).

### 5. Interactive commands (Linux/WSL)

- `pty_command_start`, `pty_command_write_stdin`, `pty_command_stop`, `pty_command_list` — live on Unix.
- Secret-prompt boundary: stdin denied while `secret_prompt_active`.
- Prompt detection rules (generic, sudo, password) emit structured events without collecting secrets.

---

## Gap Analysis — "Anything a Human Can Do"

### Gap 1: No direct shell (BIGGEST)

**What humans do:** `cat x | grep y > z && foo` — pipelines, compounds, globs, redirects in one shot.

**What TC does:** argv-only. Each tool call is a discrete `argv[]` list. No shell-string passthrough.

**Code evidence:**

```73:77:crates/daemon/src/command.rs
/// Future shell-execution opt-in is intentionally NOT implemented
/// in TC38. A later goal would need to add an explicit policy
/// capability (e.g. `allow_shell: bool` on `CommandStartRequest`,
/// gated by a new `PolicyAction::CommandShellStart` variant) before
/// this guard can be bypassed.
```

Closed-set deny on `argv[0]` basenames: `sh`, `bash`, `dash`, `zsh`, `fish`, `powershell`, `pwsh`, `cmd`, etc. — rejected **before** the policy engine runs.

**Windows escape hatch (partial):** `wsl bash -lc '…'` works because `argv[0]` is `wsl`, not `bash`. On a pure-Linux daemon there is no equivalent trick.

**Policy doc:** `POLICY.md` — `shell_passthrough = false` (binding invariant).

**README alignment:** TC is explicitly "an argv-first command and file signal channel" — **not** "a generic shell bridge."

**Distance:** Designed seam exists; implementation does not. Building `allow_shell` (gated, audited, off by default) would close most of this gap.

---

### Gap 2: No sudo / system-level changes

**What humans do:** `sudo apt install`, system services, privileged operations.

**What TC does:** Structurally denies `sudo`, `doas`, `su`, `pkexec`, `kexec`, `polkit-agent`, `polkit-auth-agent-1` in every profile.

```82:90:crates/daemon/src/policy.rs
pub const COMMANDS_DENY: &[&str] = &[
    "sudo",
    "doas",
    "su",
    "pkexec",
    "kexec",
    "polkit-agent",
    "polkit-auth-agent-1",
];
```

**What still works:** User-space installs — `npm -g` (user prefix), `pip --user`, `cargo install`, etc.

**Sanctioned sudo path (WSL cleanup only):** Scoped NOPASSWD sudoers drop-in documented in `docs/integrations/wsl-cleanup-and-sudo.md` — not a general LLM sudo capability.

**Distance:** Policy design decision as much as code. A gated `allow_privileged` capability would need explicit threat modeling, not just a flag.

---

### Gap 3: Windows PTY missing

**What humans do:** Interactive REPLs, SSH sessions, password prompts on Windows natively.

**What TC does:** PTY runtime is `#[cfg(unix)]` only. ConPTY is pending.

**Contract evidence** (`tests/fixtures/contracts/mcp-tools/system_discover.v1.json`):

> On a host without a PTY runtime (non-unix; ConPTY pending) the four `pty_*` tools report `available:false` with reason: `PTY runtime unavailable on this platform (ConPTY pending)`.

**Distance:** Platform parity work. Linux/WSL agents have interactive path; native Windows agents do not yet.

---

### Gap 4: No persistent shell session

**What humans have:** Continuous terminal state — `cd` sticks, env vars persist, shell history, job control context.

**What TC does:** Each command is a **fresh spawn** with per-call `cwd` and `env` overlay. No session that carries state across calls.

```240:257:crates/probes/src/process.rs
let mut cmd = Command::new(&argv[0]);
cmd.args(&argv[1..]);
// ...
if let Some(cwd) = &config.cwd {
    cmd.current_dir(cwd);
}
// OVERLAY semantics: child inherits daemon env; supplied env ADDED/overrides.
for (k, v) in &config.env {
    cmd.env(k, v);
}
```

**Workaround today:** LLM passes explicit `cwd`/`env` on every call, or wraps logic in a single `wsl bash -lc '…'` (shell-bridge escape, Gap 1).

**Distance:** Medium — a `session_start` / `session_exec` model with bounded lifetime and audit would address this without reopening full shell passthrough.

---

### Gap 5: "Parse ANY output" ≠ parse anything automatically

**What the vision implies:** Semantic understanding of arbitrary unknown command output.

**What TC delivers:** Efficient regex/keyword/condition rules you define. For unknown formats, bounded fallback:

| Path | Behavior |
|---|---|
| Rules match | Structured `SignalEvent` only — token-cheap |
| No rules | `command_output_tail` — 200 lines / 64 KiB, truncation-flagged |
| Quiet command | Bounded receipt via `run_and_watch` — never silence |

**Built-in coverage:** 8 curated rule packs for common toolchains. Everything else needs `registry_upsert` or inline rules.

**Distance:** Hardest / most open-ended gap. Bridging options:

1. LLM-in-the-loop: "here is the tail, suggest a rule" → `registry_test` → `registry_activate`.
2. Heuristic extractors for common patterns (errors, progress bars, exit summaries).
3. Accept that bounded raw tail is the honest fallback for one-offs.

---

### Gap 6: Local single machine

**What some agents need:** Remote hosts, containers, multi-host orchestration as first-class.

**What TC is:** One daemon per host (per `TC_SESSION`). Local IPC only — UDS on Unix, named pipe on Windows. No TCP listener. No remote daemon model.

**Distance:** Architectural scope expansion, not a missing method on existing tools.

---

## How Far Are We?

```
Vision completeness (approximate)

Core loop          ████████████████████  100%  shipped
Token-efficient    ███████████████████░   95%  rules + bounded fallbacks
User-space cmds    ███████████████████░   90%  argv-only, no shell
Interactive (Unix) ███████████████████░   90%  PTY live
Interactive (Win)  ████░░░░░░░░░░░░░░░░   20%  ConPTY pending
Shell parity       ████░░░░░░░░░░░░░░░░   20%  deliberate deny + designed seam
Privileged ops     ██░░░░░░░░░░░░░░░░░░   10%  structural deny
Session state      ██████░░░░░░░░░░░░░░   30%  per-call cwd/env only
Auto-parse unknown ████████░░░░░░░░░░░░   40%  tail fallback, no semantic auto
Multi-host         ░░░░░░░░░░░░░░░░░░░░    0%  out of scope today

Overall "Swiss knife / anything"     ~70–80%
Overall "run → comb → signal → continue"   ~95%
```

---

## Prioritized Roadmap (Effort vs Payoff)

### P0 — `allow_shell` capability (highest leverage)

| | |
|---|---|
| **Problem** | Pipelines, compounds, globs, redirects blocked by shell-bridge guard |
| **Seam** | Already documented in `command.rs`: `allow_shell` + `PolicyAction::CommandShellStart` |
| **Design** | Opt-in per profile; `AllowWithAudit` default; argv quoting preserved; no silent widening |
| **Effort** | Small–medium (policy variant + guard bypass + tests + audit) |
| **Payoff** | Closes ~60% of the "anything" gap for agents on Linux/macOS/WSL |
| **Risk** | Shell injection if LLM-supplied strings reach `-c` unquoted — must stay argv-structured or use explicit `shell_line` field with separate policy gate |

### P1 — Windows ConPTY

| | |
|---|---|
| **Problem** | `pty_command_*` unavailable on native Windows |
| **Effort** | Medium (portable-pty + ConPTY integration, platform tests) |
| **Payoff** | Interactive parity for Windows-native agents (Cursor on Windows without WSL) |
| **Note** | Research already points at `portable-pty` in `docs/research/async-runtime.md` |

### P2 — Persistent session state

| | |
|---|---|
| **Problem** | No sticky `cd`, env, or working context across calls |
| **Design options** | (a) `session_start` + `session_exec` with bounded TTL; (b) named workspace env snapshots |
| **Effort** | Medium |
| **Payoff** | Reduces LLM ceremony (re-passing cwd/env every call); closer to human terminal ergonomics |
| **Dependency** | Independent of P0; complementary |

### P3 — Gated privileged operations

| | |
|---|---|
| **Problem** | `apt install`, system services blocked |
| **Design** | New profile or capability flag; scoped allowlists (not blanket sudo); human approval hook for `registry_activate`-style gating |
| **Effort** | Medium–large (policy + threat model + docs) |
| **Payoff** | Unlocks system software install path |
| **Risk** | Highest — requires explicit operator consent model |

### P4 — Rule suggestion from tail (gap #5 bridge)

| | |
|---|---|
| **Problem** | Unknown output formats fall back to bounded raw tail |
| **Design** | MCP tool or workflow: `command_output_tail` → LLM proposes rule → `registry_test` → `registry_activate` |
| **Effort** | Small (orchestration) to large (daemon-side heuristic extractor) |
| **Payoff** | Closes the "parse anything" loop without claiming semantic magic |

### P5 — Multi-host (optional, scope decision)

| | |
|---|---|
| **Problem** | Single-machine daemon model |
| **Effort** | Large (transport, identity, policy federation) |
| **Payoff** | Only if product scope expands beyond local agent harness |
| **Recommendation** | Defer until P0–P2 land; document as non-goal for MVP |

---

## What TC Is (Product Contract)

From `README.md` — intentional positioning, not accidental limitation:

| It is | It is not |
|---|---|
| A local daemon + MCP stdio adapter | A remote service |
| An argv-first command and file signal channel | A generic shell bridge |
| A bounded JSON tool surface for LLMs | A human terminal UI |
| One daemon per harness (`TC_SESSION`) | A shared multi-tenant daemon |

The vision tension is real: **"Swiss knife for terminal interaction"** vs **"not a generic shell bridge."** The roadmap above resolves that tension by making shell/privilege **opt-in capabilities** rather than default behavior — preserving the security model while closing the capability gap for operators who need it.

---

## Recommended Next Step

Start with **P0 (`allow_shell`)** — the seam exists in code, the policy variant is named, and it unlocks the largest slice of "anything a human can do" without rewriting the architecture. Pair with contract tests that prove:

1. Default profile still denies shell interpreters.
2. Opt-in profile allows gated shell execution with audit.
3. Pipelines via `-c` cannot bypass argv quoting invariants.

---

## References

| Artifact | Path |
|---|---|
| Shell-bridge guard + future seam | `crates/daemon/src/command.rs` |
| Privilege deny list | `crates/daemon/src/policy.rs` |
| Command runtime scope doc | `docs/runtime/COMMAND_RUNTIME.md` |
| Policy profiles + shell_passthrough | `POLICY.md` |
| PTY availability contract | `tests/fixtures/contracts/mcp-tools/system_discover.v1.json` |
| Runtime chain evidence (TC33–TC48) | `EVIDENCE_REPORT_RUNTIME.md` |
| Rule packs | `crates/store/src/import.rs` |
| MCP tool implementations | `crates/mcp/src/tools.rs` |

---

*Report generated from live codebase audit at v0.1.47. Supersedes the pre-TC48 mental model in `docs/audits/runtime-gap-audit.md` (TC33 snapshot — most items listed there are now shipped).*
