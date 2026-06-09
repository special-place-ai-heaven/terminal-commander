# TC Omni-Tool Master Program

> **For implementers:** Each wave has a dedicated plan under `docs/plans/`. Use superpowers:executing-plans (or goal-chain `.agent/goals/`) wave-by-wave.

**Goal:** Evolve Terminal Commander from ~75% vision-complete (strong core loop, deliberate capability gates) to **100% omni tool** — everything a human can do in a terminal, plus LLM-native capabilities humans lack.

**Architecture:** Keep the two-process model (MCP adapter + daemon). Expand capabilities through **opt-in policy gates**, not by removing safety. Humans get shell/privilege/session through explicit profile capabilities; LLMs get structured supersets (signals, subscriptions, auto-rules, federation).

**Tech stack:** Rust daemon, rmcp MCP adapter, SQLite store, portable-pty, existing policy/sifter/bucket pipeline.

**Baseline:** v0.1.47, 38 live MCP tools, TC33–TC48 runtime chain shipped.

**Date:** 2026-06-09

---

## Why it felt "already there"

The MVP chain (TC01–TC48) delivered the **spine** of the vision:

- Real MCP stdio + daemon IPC
- `run_and_watch` → comb → signal → reason → continue
- Registry, buckets, subscriptions, PTY on Unix, audit, policy

What was **not** delivered — by explicit MVP doctrine, not accident:

| Deliberate MVP constraint | Documented in |
|---|---|
| No shell bridge | `POLICY.md`, `command.rs` shell guard |
| No sudo / privilege | `PRIVILEGE_MODEL.md`, `COMMANDS_DENY` |
| argv-only, fresh spawn per call | `COMMAND_RUNTIME.md` |
| Parse only what rules cover | Sifter + 8 seed packs |
| Local single host | SPEC.md, no TCP listener |
| Windows PTY deferred | ConPTY pending |

README still says "not a generic shell bridge." That was correct for MVP; it **conflicts** with the product vision you describe. Closing the gap requires a **program amendment**, not more MVP polish.

---

## Definition of "100% omni tool"

### Layer A — Human parity (must pass)

An LLM using only TC tools (no raw shell MCP, no `run_terminal_cmd` fallback) can:

1. Run arbitrary shell constructs: pipelines, redirects, globs, compounds, job control.
2. Maintain persistent working state: cwd, env, shell variables across calls.
3. Drive interactive programs: REPLs, prompts, SSH, sudo/password flows (with secret boundary).
4. Install and configure software at user and system level (when operator enables it).
5. Operate on **every supported platform** with feature parity (Linux, WSL, macOS, native Windows).
6. Work on **remote targets** (SSH host, container, CI runner) through the same MCP tool surface.

### Layer B — LLM supersets (must pass)

TC must be **strictly better** than a human terminal for agent work:

| Human terminal | TC omni advantage |
|---|---|
| Reads raw scrollback | Structured signals + severity + pointers |
| Forgets job context | Stable `job_id`, `bucket_id`, `cursor` forever in session |
| Manual grep of output | Declarative rules + auto-suggest from tail |
| One stream at a time | `subscription_pull` multiplexes N jobs/files/PTYs |
| No audit trail | Append-only audit of every gated action |
| Unsafe by default | Policy profiles + explicit capability opt-in |
| Re-types cwd/env every command | Named workspace sessions with snapshots |
| Guesses when command is done | `run_and_watch` receipt + `complete`/`wait_exhausted` contract |
| Cannot recover from IPC blip | Degraded results preserve `job_id` + `recover_hint` |

### Layer C — Self-reliance (must pass)

An LLM never needs to fall back to a separate terminal tool because:

- Unknown output → `command_output_tail` → `registry_suggest` → `registry_test` → activate (closed loop)
- Quiet command → bounded receipt, never silence
- Long-running → `command_stop` + subscriptions
- Platform missing feature → `system_discover` reports why + alternative path
- Trust defects → idempotent retry gate, dedup nonce, no double-spawn

---

## Omni acceptance matrix

Each row is a **release gate** for the program (not MVP).

| ID | Scenario | Tool path | Wave |
|---|---|---|---|
| O-01 | `echo a \| wc -c` in one call | `shell_exec` or `allow_shell` | 1 |
| O-02 | `cd /tmp` then `pwd` returns `/tmp` without re-passing cwd | `shell_session_exec` | 1 |
| O-03 | Python REPL: import, print, exit | `pty_command_*` | 1–2 |
| O-04 | `npm install -g pkg` → one actionable signal | `run_and_watch` + npm pack | 0 (done) |
| O-05 | Unknown CLI → tail → suggest rule → re-run → signals only | `registry_suggest` loop | 3 |
| O-06 | `sudo apt install` with operator-approved profile | privileged helper | 4 |
| O-07 | Native Windows PTY python REPL | ConPTY | 2 |
| O-08 | macOS Cursor agent full smoke | tier-1 macOS | 2 |
| O-09 | SSH to remote host, run command, get signals | `remote_attach` | 5 |
| O-10 | Docker exec into container, same tools | `remote_attach` | 5 |
| O-11 | 3 parallel builds, one subscription | `subscription_*` | 0 (done) |
| O-12 | Client timeout on start → no double spawn | idempotency + dedup | 0 |
| O-13 | Mid-wait IPC blip → degraded + recover | `run_and_watch` | 0 (done) |
| O-14 | Codex + Cursor + Claude live trust test all green | harness smokes | 6 |

---

## Program waves (summary)

```text
                    TC Omni-Tool Program
    ┌─────────────────────────────────────────────────────────┐
    │ Wave 0: Trust hardening (finish BACKLOG P0/P1)          │
    └──────────────────────────┬──────────────────────────────┘
                               v
    ┌─────────────────────────────────────────────────────────┐
    │ Wave 1: Shell + session (human parity core)             │
    │  allow_shell, shell_session_*, workspace snapshots      │
    └──────────────────────────┬──────────────────────────────┘
                               v
         ┌─────────────────────┴─────────────────────┐
         v                                           v
┌────────────────────┐                   ┌────────────────────┐
│ Wave 2: Platform   │                   │ Wave 3: Parse omni │
│ ConPTY, macOS,     │                   │ suggest, packs,    │
│ notify backends    │                   │ universal extract  │
└─────────┬──────────┘                   └─────────┬──────────┘
          v                                        v
    ┌─────────────────────────────────────────────────────────┐
    │ Wave 4: Privileged helper (closed allow-list)           │
    └──────────────────────────┬──────────────────────────────┘
                               v
    ┌─────────────────────────────────────────────────────────┐
    │ Wave 5: Remote federation (SSH, container, multi-host)  │
    └──────────────────────────┬──────────────────────────────┘
                               v
    ┌─────────────────────────────────────────────────────────┐
    │ Wave 6: Omni certification + doc/positioning realign    │
    └─────────────────────────────────────────────────────────┘
```

---

## Wave 0 — Trust hardening (prerequisite)

**Status:** Mostly shipped; verify green before Wave 1.

| Item | Source | Verify |
|---|---|---|
| TC-1a idempotent retry gate | BACKLOG P0.1 | `crates/mcp/src/daemon_client.rs`, `retry_gate.rs` |
| TC-1b degraded run_and_watch | BACKLOG P0.2 | `tools.rs` degraded payload |
| TC-2 dedup nonce | BACKLOG P1.0a | `CommandRuntime` dedup map |
| TC-3 command_stop | BACKLOG P1.0b | 38 tools, live e2e |

**Deliverable:** All trust campaign tests green; update BACKLOG to mark resolved.

**Effort:** ~0–1 week (verification + any remaining gaps).

---

## Wave 1 — Shell + session (largest human-parity unlock)

**Problem:** argv-only + fresh spawn blocks pipelines and sticky state.

**Deliverables:**

1. **`PolicyAction::CommandShellStart`** + profile flag `commands.shell_passthrough` (opt-in, default false).
2. **`shell_exec`** MCP tool — explicit shell line field (NOT smuggled via argv[0]); combed like commands; audited `AllowWithAudit`.
3. **`shell_session_start` / `shell_session_exec` / `shell_session_stop` / `shell_session_list`** — persistent PTY-backed or shell-backed session with bounded TTL, cwd/env carry-forward, per-session bucket.
4. **`workspace_snapshot` / `workspace_restore`** (optional) — named env+cwd checkpoints for LLM reproducibility.
5. Update `POLICY.md`, `PRIVILEGE_MODEL.md`, `README.md` positioning: "opt-in shell capability" not "never a shell."
6. Contract fixtures + live e2e for O-01, O-02.

**Seam already in code:** `crates/daemon/src/command.rs:73-77`.

**Security invariants:**

- Default profile: shell denied (unchanged).
- `shell_line` is a dedicated field; never `-c` via smuggled argv.
- Max line length cap; no nested shell spawn without audit.
- Session TTL + max sessions per daemon.

**Effort:** ~3–4 weeks (1 goal chain: TC49–TC52).

**Plan:** [2026-06-09-tc-omni-wave1-shell-session.md](./2026-06-09-tc-omni-wave1-shell-session.md)

---

## Wave 2 — Platform parity

**Problem:** Native Windows has no PTY; macOS tier-3; file probes poll on 9P.

**Deliverables:**

1. **Windows ConPTY** — `portable-pty`; `pty_command_*` available:true on Windows; O-07.
2. **macOS tier-1** — daemon + MCP on macOS with full smoke; O-08.
3. **notify/inotify backends** — replace 250ms polling where native FS supports it (BACKLOG P1).
4. **process-wrap** — SIGTERM ladder + process groups (BACKLOG P1).

**Effort:** ~4–6 weeks (ConPTY alone ~2–3 weeks).

**Plan:** [2026-06-09-tc-omni-wave2-platform-parity.md](./2026-06-09-tc-omni-wave2-platform-parity.md)

---

## Wave 3 — Parse omni (LLM superset core)

**Problem:** "Parse anything" requires rules you wrote; tail fallback is token-expensive for unknown formats.

**Deliverables:**

1. **`registry_suggest_from_samples`** — input: bounded tail lines; output: proposed `RuleDefinition[]` (regex/keyword); no persist until `registry_test` + operator/LLM activate.
2. **Universal extractors** (daemon-side, always-on, low severity):
   - stderr error lines (common prefixes)
   - exit summary on process exit
   - progress/spinner strip (reuse stall/progress sifters)
3. **Rule pack expansion** — docker, kubectl, git, systemd, pip, uv, go, rustc, msbuild, winget, choco (target: 25+ packs).
4. **`run_and_watch` auto-pack hint** — if argv[0] matches known tool, suggest/import pack in response metadata (not silent activation).
5. O-05 acceptance: unknown CLI closed loop without raw shell fallback.

**Effort:** ~3–5 weeks.

**Plan:** [2026-06-09-tc-omni-wave3-parse-and-packs.md](./2026-06-09-tc-omni-wave3-parse-and-packs.md)

---

## Wave 4 — Privileged helper

**Problem:** System install, services, journald — blocked by structural sudo deny.

**Deliverables (per PRIVILEGE_MODEL.md §5 pre-bind):**

1. **`terminal-commander-privileged`** — separate small binary, operator-installed, NOT spawned by LLM string.
2. **Closed allow-list RPCs:** `apt_install`, `systemctl_restart`, `journal_read_window`, etc. — never generic shell.
3. **Profile `admin_local`** extension: `privileged_helper.enabled`, `privileged_helper.allow_ops[]`.
4. **Human approval hook** — high-severity audit + optional `admin_cli approve <token>` before execute (configurable).
5. **WSL NOPASSWD** pattern generalized into documented operator setup, not hardcoded.
6. O-06 acceptance.

**Effort:** ~4–6 weeks (policy + threat model + implementation).

**Plan:** [2026-06-09-tc-omni-wave4-privilege-helper.md](./2026-06-09-tc-omni-wave4-privilege-helper.md)

---

## Wave 5 — Remote federation

**Problem:** Single-host daemon limits agents working across SSH/containers/CI.

**Deliverables:**

1. **`remote_attach`** — register remote endpoint (SSH config ref, docker context, local socket forward spec).
2. **Remote daemon protocol** — same IPC envelope over authenticated tunnel (SSH `-L`, not public TCP).
3. **Tool routing** — MCP tools accept optional `target_id`; default local.
4. **Policy federation** — remote host profile snapshot in `system_discover`.
5. O-09, O-10 acceptance.

**Architecture note:** Prefer **remote TC daemon** on each host (agent installs TC once) over SSH-exec-without-combing. Federation connects daemons; does not replace them.

**Effort:** ~6–10 weeks (largest architectural expansion).

**Plan:** [2026-06-09-tc-omni-wave5-remote-federation.md](./2026-06-09-tc-omni-wave5-remote-federation.md)

---

## Wave 6 — Omni certification

**Deliverables:**

1. **`docs/testing/omni-acceptance-suite.md`** — O-01..O-14 as automated smokes.
2. **Provider harness parity** — Cursor, Codex, Claude Code, Claude Desktop on Linux + WSL + Windows + macOS.
3. **README / SPEC realignment** — "omni tool for LLMs" primary; shell bridge as opt-in capability.
4. **`system_discover.omni_status`** — wave completion + platform matrix.
5. **Agent playbook** — `docs/mcp/OMNI_PLAYBOOK.md`: when to use shell_session vs run_and_watch vs pty vs remote.

**Effort:** ~2–3 weeks continuous across program; final gate ~1 week.

**Plan:** [2026-06-09-tc-omni-wave6-omni-certification.md](./2026-06-09-tc-omni-wave6-omni-certification.md)

---

## Effort summary

| Wave | Focus | Calendar (1 eng) | Parallelizable |
|---|---|---|---|
| 0 | Trust | 0–1 wk | — |
| 1 | Shell + session | 3–4 wk | — |
| 2 | Platform | 4–6 wk | partial with 3 |
| 3 | Parse omni | 3–5 wk | partial with 2 |
| 4 | Privilege | 4–6 wk | after 1 |
| 5 | Remote | 6–10 wk | after 1+2 |
| 6 | Certification | 2–3 wk | continuous |

**Total (sequential):** ~22–35 weeks  
**Total (2 parallel tracks after Wave 1):** ~16–24 weeks

---

## Policy / docs amendments required

These are **program blockers**, not nice-to-haves:

| Document | Change |
|---|---|
| `README.md` | Primary identity: omni LLM terminal tool; shell bridge opt-in |
| `POLICY.md` | `shell_passthrough` profile-scoped; `CommandShellStart` algorithm |
| `PRIVILEGE_MODEL.md` | Privileged helper spec (Wave 4) |
| `SPEC.md` | Move shell/session/remote from "deferred" to "omni program" |
| `docs/runtime/COMMAND_RUNTIME.md` | shell_exec + session runtime |
| MCP tool catalogue | +8–15 new tools (estimate 46–53 at omni complete) |

---

## What NOT to do

- **Do not** remove shell guard globally — opt-in only.
- **Do not** add generic `sudo -c` — closed allow-list helper only.
- **Do not** stream unbounded raw stdout to LLM — even with shell, comb first.
- **Do not** open public TCP daemon listener — federation via SSH tunnel / local forward.
- **Do not** claim semantic magic — auto-parse is suggest + test + activate.

---

## Immediate next steps

1. **Approve program amendment** — reconcile README "not a shell bridge" with omni vision.
2. **Verify Wave 0 green** — run trust campaign tests + update BACKLOG.
3. **Kick Wave 1** — goal file `TC49-shell-capability-and-session-runtime.md` from [wave1 plan](./2026-06-09-tc-omni-wave1-shell-session.md).
4. **Parallel doc PR** — SPEC.md omni section + acceptance matrix link.

---

## References

- Gap analysis: `.planning/cursor_report_03.md`
- Runtime evidence: `EVIDENCE_REPORT_RUNTIME.md`
- Active backlog: `BACKLOG.md`
- Shell seam: `crates/daemon/src/command.rs`
- Privilege doctrine: `docs/security/PRIVILEGE_MODEL.md`
