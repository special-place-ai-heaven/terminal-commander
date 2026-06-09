# Terminal Commander — Omni Tool Program (LLM Implementation Brief)

**Give this entire file to your coding LLM.** It is self-contained: vision, gaps, architecture, waves, acceptance tests, file paths, security rules, and where to start.

**Repository:** `terminal-commander` (Rust workspace + npm MCP package)  
**Baseline version:** 0.1.47  
**Program goal:** Make TC a **100% self-reliant omni terminal tool** for any LLM — human terminal parity **plus** LLM-native advantages. No fallback to a separate raw shell tool.

**Program status:** NOT STARTED (planning complete). Execute waves in order unless noted.

---

## Instructions for the coding LLM

1. **Read this file fully** before writing code.
2. **Execute waves sequentially:** Wave 0 verify → Wave 1 → (Wave 2 ∥ Wave 3) → Wave 4 → Wave 5 → Wave 6.
3. **One goal at a time:** Complete TC49 before TC50, etc. Run tests after each goal.
4. **Never break security invariants** (section below). Opt-in capabilities only.
5. **Always comb output** — LLM never receives unbounded raw stdout/stderr on normal tool responses.
6. **Verification gate:** `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace`
7. **Update tool count anchors** whenever adding MCP tools (search repo for `38` tool references).
8. **Commit message style:** Conventional commits; focus on why.

---

## Product vision

Terminal Commander is the **Swiss army knife terminal layer for LLMs**:

- Run commands, install software, wait for results, kill runaways, watch files, drive interactive programs.
- **Declaratively parse** output — only actionable signals return to the LLM (token savings).
- Loop: **run → comb → signal → reason → continue**.

TC must do **everything a human can do in a terminal**, and **more** (structured events, subscriptions, audit, auto-rules, federation, degraded recovery).

---

## Current state (what is already shipped)

### Working today (v0.1.47)

| Capability | MCP tools | Notes |
|---|---|---|
| argv command + comb | `command_start_combed`, `run_and_watch` | No raw stream in response |
| Job lifecycle | `command_status`, `command_stop` | 38 live tools total |
| Exploratory tail | `command_output_tail` | 200 lines / 64 KiB max |
| Buckets / wait / context | `bucket_*`, `event_context` | |
| Subscriptions | `subscription_*` | Multiplex many sources |
| Registry + packs | `registry_*`, `registry_import_pack` | 8 built-in packs |
| Files | `file_read_window`, `file_search`, `file_watch_*` | |
| PTY (Unix only) | `pty_command_*` | Windows: unavailable |
| Discovery | `system_discover`, `health`, `policy_status` | |
| Trust (mostly) | idempotent retry, degraded `run_and_watch` | Verify Wave 0 |

**Built-in rule packs** (`crates/store/src/import.rs`): `generic.terminal`, `apt`, `cargo`, `npm`, `pytest`, `gcc`, `make`, `cleanup`.

### Deliberate gaps (why ~75% not 100%)

| Gap | Evidence |
|---|---|
| No shell pipelines | `SHELL_INTERPRETERS_DENY` in `crates/daemon/src/command.rs:78-95` |
| No sudo / system install | `COMMANDS_DENY` in `crates/daemon/src/policy.rs:82-90` |
| No persistent session | Fresh spawn per call in `crates/probes/src/process.rs` |
| Windows PTY missing | `system_discover` fixture: ConPTY pending |
| Parse = rules you wrote | Fallback: `command_output_tail` |
| Single host only | Local IPC; no remote daemon |

**Designed but unbuilt seam for shell** (`crates/daemon/src/command.rs:73-77`):

```text
Future: allow_shell on CommandStartRequest,
gated by PolicyAction::CommandShellStart
```

---

## Definition of done (100% omni)

### Layer A — Human parity

LLM using **only TC tools** can:

1. Run shell pipelines, redirects, globs, compounds.
2. Keep sticky cwd/env across calls (sessions).
3. Drive REPLs and interactive prompts on all platforms.
4. Install system software when operator enables privileged helper.
5. Work on Linux, WSL, macOS, native Windows with parity.
6. Target remote hosts/containers via same tool surface.

### Layer B — LLM supersets (better than human terminal)

| Human | TC omni |
|---|---|
| Raw scrollback | Structured signals + severity |
| Forgets job IDs | Stable `job_id`, `bucket_id`, `cursor` |
| Manual grep | Rules + `registry_suggest_from_samples` |
| One stream | `subscription_pull` multiplex |
| No audit | Append-only audit log |
| Unsafe default | Policy profiles + opt-in capabilities |
| Re-type cwd every call | `shell_session_*` + workspace snapshots |
| Guess if command done | `run_and_watch` `complete` / `wait_exhausted` |
| Lost on IPC blip | `degraded: true` + `recover_hint` |

### Layer C — Self-reliance

LLM never needs a separate terminal tool:

- Unknown output → tail → suggest → test → activate → re-run (closed loop)
- Quiet command → bounded receipt, never silence
- Platform gap → `system_discover.omni_status` explains why

---

## Omni acceptance matrix (release gates)

Automate these as smokes in Wave 6. All must pass to declare omni complete.

| ID | Test | Wave |
|---|---|---|
| O-01 | `echo a \| wc -c` in one call via `shell_exec` | 1 |
| O-02 | `cd /tmp` then `pwd` → `/tmp` via `shell_session_exec` without re-passing cwd | 1 |
| O-03 | Python REPL via `pty_command_*` | 1–2 |
| O-04 | `npm install -g` → one actionable signal | 0 ✓ |
| O-05 | Unknown CLI → tail → suggest rule → re-run → signals only | 3 |
| O-06 | `apt install` via privileged helper (approved) | 4 |
| O-07 | Native Windows PTY python REPL | 2 |
| O-08 | macOS full agent smoke | 2 |
| O-09 | SSH remote host command with signals | 5 |
| O-10 | Docker/container command with signals | 5 |
| O-11 | 3 parallel jobs, one subscription | 0 ✓ |
| O-12 | Client timeout on start → no double spawn | 0 |
| O-13 | Mid-wait IPC blip → degraded + recover | 0 ✓ |
| O-14 | Cursor + Codex + Claude trust smokes green | 6 |

---

## Architecture (do not change)

```text
LLM harness (Cursor/Codex/Claude)
        ↓ stdio MCP
terminal-commander-mcp  (thin adapter, no Command::spawn)
        ↓ local IPC (UDS / named pipe)
terminal-commanderd     (daemon: policy, probes, sifters, buckets, audit, SQLite)
        ↓
   ProcessProbe / PtyRuntime / ShellRuntime / FileProbe
        ↓
   SifterRuntime → BucketManager → SignalEvents (bounded, no raw stream)
```

**Keep:** two-process model, local-only IPC, policy-before-spawn, combed output.  
**Expand:** opt-in capabilities through policy gates, not by removing guards.

---

## Security invariants (NEVER violate)

1. **Default profile denies shell** — `commands.shell_passthrough = false` until operator enables.
2. **No argv smuggling** — shell via dedicated `shell_line` field on `shell_exec`, never `argv[0]=bash` on `command_start_combed`.
3. **No generic sudo** — privileged ops via closed allow-list helper binary only.
4. **No unbounded raw output** — even shell commands go through sifter; `command_output_tail` is explicitly bounded.
5. **No public TCP daemon** — remote access via SSH `-L` tunnel to existing socket only.
6. **MCP adapter never spawns commands** — grep-test must stay green.
7. **Suggest never auto-activates rules** — always `registry_test` + explicit activate.
8. **Audit every gated action** — shell, privileged, registry activate.

---

## Codebase map (key files)

| Area | Path |
|---|---|
| Shell guard + future seam | `crates/daemon/src/command.rs` |
| Policy engine | `crates/daemon/src/policy.rs` |
| PTY runtime (unix) | `crates/daemon/src/pty_command.rs` |
| Process spawn | `crates/probes/src/process.rs` |
| MCP tools | `crates/mcp/src/tools.rs` |
| IPC protocol | `crates/ipc/src/protocol.rs` |
| IPC handlers | `crates/daemon/src/ipc/handlers/` |
| Rule packs | `crates/store/rules/*.json`, `crates/store/src/import.rs` |
| Sifters | `crates/sifters/src/` |
| Policy docs | `POLICY.md`, `docs/security/PRIVILEGE_MODEL.md` |
| Command runtime doc | `docs/runtime/COMMAND_RUNTIME.md` |
| MCP contracts | `tests/fixtures/contracts/mcp-tools/` |
| Live e2e tests | `crates/mcp/tests/mcp_live_*.rs`, `*_live_e2e.rs` |
| Backlog | `BACKLOG.md` |

---

## Program waves overview

```text
Wave 0: Trust verify          (0–1 wk)
Wave 1: Shell + session       (3–4 wk)  ← START HERE after Wave 0
Wave 2: Platform parity       (4–6 wk)  ∥ Wave 3
Wave 3: Parse omni            (4–5 wk)  ∥ Wave 2
Wave 4: Privileged helper     (4–6 wk)
Wave 5: Remote federation     (6–10 wk)
Wave 6: Certification + docs  (2–3 wk)
```

**Total:** ~16–24 weeks with parallel tracks; ~22–35 weeks sequential.

---

# WAVE 0 — Trust hardening (verify before Wave 1)

**Goal:** Confirm trust campaign items are green; fix any gaps.

| Item | Verify in |
|---|---|
| TC-1a idempotent retry | `crates/mcp/src/daemon_client.rs`, `crates/mcp/tests/retry_gate.rs` |
| TC-1b degraded run_and_watch | `crates/mcp/src/tools.rs` (`degraded: true`, `recover_hint`) |
| TC-2 dedup nonce | `crates/daemon/src/command.rs` dedup map |
| TC-3 command_stop | tool exists; 38 tools in live e2e |

**Commands:**

```bash
cargo nextest run --workspace -E 'test(retry_gate) or test(run_and_watch) or test(command_stop)'
```

**Done when:** All pass; mark resolved in `BACKLOG.md` P0/P1 trust items.

---

# WAVE 1 — Shell + persistent session (TC49–TC52)

**Unlocks:** O-01, O-02. Largest human-parity gap.

## TC49 — `shell_exec` (one-shot shell)

### New MCP tool: `shell_exec`

```json
{
  "shell_line": "cat foo | grep bar > out.txt && wc -l out.txt",
  "cwd": "/optional/path",
  "env": {"KEY": "val"},
  "rules": [],
  "wait_ms": 5000,
  "shell": "/bin/bash"
}
```

- `shell_line`: **required dedicated field** (max bytes cap, e.g. 8192).
- Spawn: `[shell, "-lc", shell_line]` only when policy allows.
- Output: same as `run_and_watch` — signals + receipt, no raw stream.

### Policy change

Add to `crates/daemon/src/policy.rs`:

```rust
pub enum PolicyAction<'a> {
    // ... existing variants ...
    CommandShellStart {
        shell_line: &'a str,
        cwd: &'a Path,
        shell: &'a str,
    },
}
```

Profile config (`terminal-commander.toml`):

```toml
[commands]
shell_passthrough = false  # default; set true to enable shell_exec
```

Evaluation: default deny; `developer_local` + `shell_passthrough = true` → `AllowWithAudit`.

### Implementation

| Action | File |
|---|---|
| Create | `crates/daemon/src/shell.rs` — `ShellRuntime::exec` |
| Modify | `crates/daemon/src/runtime.rs` — wire ShellRuntime |
| Modify | `crates/ipc/src/protocol.rs` — `ShellExec` request/response |
| Create | `crates/daemon/src/ipc/handlers/shell.rs` |
| Modify | `crates/daemon/src/ipc/server.rs` — dispatch |
| Modify | `crates/mcp/src/tools.rs` — `shell_exec` tool |
| Create | `docs/runtime/SHELL_RUNTIME.md` |
| Modify | `POLICY.md` — CommandShellStart algorithm |
| Test | `crates/daemon/tests/shell_policy.rs` |
| Test | `crates/mcp/tests/shell_live_e2e.rs` |
| Fixture | `tests/fixtures/contracts/mcp-tools/shell_exec.v1.json` |

**Keep unchanged:** `SHELL_INTERPRETERS_DENY` on `command_start_combed` path.

### TC49 tests (must pass)

1. Default profile: `shell_exec` → policy deny.
2. `command_start_combed` with `argv[0]=bash` still denied.
3. `shell_line` over max length → reject before spawn.
4. Audit: `command_shell_start` with truncated/redacted preview.
5. O-01: `shell_exec` with `echo a | wc -c` → signal or receipt with exit 0.

---

## TC50 — Persistent shell sessions

### New MCP tools

| Tool | Purpose |
|---|---|
| `shell_session_start` | Returns `session_id`, `bucket_id` |
| `shell_session_exec` | Send line; combed signals in bucket |
| `shell_session_status` | cwd, env snapshot |
| `shell_session_stop` | Tear down |
| `shell_session_list` | Active sessions |

**Implementation (recommended):** PTY + long-lived shell (`bash -l`). Each `shell_session_exec` writes line + `\n`; sifter reads PTY output.

**Limits:**

```toml
[shell_session]
max_sessions = 4
idle_ttl_secs = 3600
```

| Action | File |
|---|---|
| Create | `crates/daemon/src/shell_session.rs` |
| Reuse | `crates/daemon/src/pty_command.rs` PTY infra |
| Test | `crates/daemon/tests/shell_session_ipc.rs` |

### O-02 test

```text
shell_session_start → session_id
shell_session_exec "cd /tmp"
shell_session_exec "pwd" → signal contains /tmp
```

---

## TC51 — Workspace snapshots (optional, high value)

| Tool | Purpose |
|---|---|
| `workspace_snapshot_create` | Save cwd + env for session |
| `workspace_snapshot_apply` | Restore |

Store in SQLite; bounded env size.

---

## TC52 — Docs + README realignment (Wave 1 portion)

- Update `README.md` identity table: "opt-in shell capability" not "never a shell bridge".
- Add shell section to future `docs/mcp/OMNI_PLAYBOOK.md`.
- Provider smoke: one shell pipeline per harness.

**Wave 1 done when:** O-01, O-02 pass on Linux/WSL.

---

# WAVE 2 — Platform parity (TC53–TC56)

**Unlocks:** O-03, O-07, O-08.

## TC53 — Windows ConPTY

- Add `portable-pty` (see `docs/research/async-runtime.md`).
- Unify or add `pty_command` Windows path.
- `system_discover`: `pty_command_*` → `available: true` on Windows.
- Live e2e on Windows CI.

## TC54 — macOS tier-1

- Full daemon + MCP smoke on macOS.
- Policy paths for homebrew dev layouts.

## TC55 — notify/inotify file backends

- Replace 250ms polling on native FS (`crates/probes/src/file.rs`).
- Keep poll fallback for WSL `/mnt/c` (9P).

## TC56 — process-wrap

- SIGTERM-to-group + grace ladder.
- Align cancel paths: command, PTY, shell session.

**Wave 2 done when:** O-07, O-08 pass; PTY available on all tier-1 platforms.

---

# WAVE 3 — Parse omni (TC57–TC60)

**Unlocks:** O-05. Can run parallel with Wave 2.

## TC57 — `registry_suggest_from_samples`

```json
// Request
{"samples": ["error: failed", "warning: unused"], "intent": "errors and warnings", "max_rules": 5}

// Response
{"proposed_rules": [...], "confidence": "heuristic", "next_steps": ["registry_test", "registry_upsert", "registry_activate"]}
```

**v1:** Pure Rust heuristics (error/warning prefixes, FAILED, paths). **No auto-activate.**

## TC58 — Universal extractors

Always-on low-severity signals (config `sifters.universal_extractors = true`):

- stderr error lines
- warning lines
- exit summary
- progress ticks (reuse stall/progress sifters)

## TC59 — Rule pack expansion

Add JSON packs in `crates/store/rules/`:

**P0:** docker, kubectl, git  
**P1:** systemd/journal, pip, uv, go, msbuild, winget, choco  
**P2:** terraform, ansible  

Target: **25+ packs** (from current 8).

## TC60 — Pack hints in responses

When starting known tool without active pack, include metadata hint:

```json
{"hint": {"kind": "pack_available", "pack": "docker", "action": "registry_import_pack"}}
```

### O-05 closed loop

```text
run_and_watch → command_output_tail → registry_suggest_from_samples
→ registry_test → registry_upsert → registry_activate → re-run → signals only
```

---

# WAVE 4 — Privileged helper (TC61–TC65)

**Unlocks:** O-06. Requires threat review first.

## Architecture

```text
LLM → MCP → daemon → policy → terminal-commander-privileged (separate binary)
                              → closed allow-list ops ONLY
```

**Never:** `{shell_line: "..."}` or generic `sudo -c`.

## v1 allowed ops

| Op | Description |
|---|---|
| `apt_install` | Named packages only |
| `apt_update` | |
| `systemctl_start/stop/restart` | Named unit |
| `journal_read_window` | Bounded lines |
| `winget_install` | Windows |
| `sc_start/stop` | Windows services |

## New tools

| Tool | Access |
|---|---|
| `privileged_exec` | MCP (policy + optional approval token) |
| `privileged_list_ops` | MCP |
| `privileged_approve` | admin_cli only |

## Config

```toml
[privileged_helper]
enabled = false
allowed_ops = ["apt_install"]
require_human_approve = true
```

## Human approval flow

```text
privileged_exec → pending_approval + approval_id
operator: terminal-commander privileged approve <id>
privileged_exec with approval_token → execute → combed bucket
```

Update `docs/security/PRIVILEGE_MODEL.md` §5 before coding.

---

# WAVE 5 — Remote federation (TC66–TC69)

**Unlocks:** O-09, O-10.

## Principle

**Remote TC daemon on each host** — not SSH-exec without combing.

```text
terminal-commander-mcp → target_router
    ├─ local IPC (default)
    └─ SSH -L → remote terminal-commanderd.sock
```

## Config (`~/.config/terminal-commander/targets.toml`)

```toml
[[targets]]
id = "prod-server"
transport = "ssh_forward"
host = "user@host"
identity_file = "~/.ssh/id_ed25519"
remote_socket = "~/.local/share/terminal-commanderd/terminal-commanderd.sock"
```

## Tool change

Add optional `target_id` to all daemon-backed MCP tools. Default: local.

## New tools

| Tool | Purpose |
|---|---|
| `target_list` | Registered + reachable |
| `target_probe` | Remote health |

**Minimum ship:** SSH only (O-09). Container (O-10) can be v1.1.

---

# WAVE 6 — Certification (TC70–TC74)

## TC70 — Automated omni smokes

Create:

- `scripts/smoke/verify-omni-linux.sh`
- `scripts/smoke/verify-omni-wsl.sh`
- `scripts/smoke/verify-omni-windows.ps1`
- `scripts/smoke/verify-omni-macos.sh`

Each runs O-01..O-14 sequence; exits non-zero on failure.

## TC71 — Provider harness parity

Extend smokes for Cursor, Codex CLI, Claude Code, Claude Desktop.

## TC72 — Documentation

| File | Change |
|---|---|
| `README.md` | Primary identity: omni LLM terminal tool |
| `SPEC.md` | Omni scope |
| `ROADMAP.md` | TC49–TC74 |
| `docs/mcp/OMNI_PLAYBOOK.md` | **Create** — agent decision tree |

### OMNI_PLAYBOOK decision tree

```text
Need to run something?
├─ Known tool (npm, cargo, docker) → registry_import_pack → run_and_watch
├─ One-shot pipeline → shell_exec (if enabled)
├─ Multi-step shell state → shell_session_*
├─ Interactive REPL → pty_command_*
├─ Unknown output → run_and_watch → command_output_tail → registry_suggest_from_samples
├─ Remote host → target_id on any tool
└─ System install → privileged_exec (if enabled + approved)
```

## TC73 — `system_discover.omni_status`

```json
{
  "omni_status": {
    "program_version": "1.0",
    "matrix": {
      "shell_exec": {"available": true},
      "pty": {"available": true, "platform": "windows_conpty"},
      "privileged_helper": {"available": false, "reason": "not_configured"},
      "remote_targets": {"count": 1, "reachable": 1}
    }
  }
}
```

## TC74 — Release

- Version: **0.2.0** or **1.0.0** when all O-* green.
- Release notes with capability matrix.

### Final certification checklist

- [ ] O-01..O-14 green on Linux + WSL
- [ ] O-07 on Windows native; O-08 on macOS
- [ ] Four provider trust smokes green
- [ ] No open P0 in BACKLOG.md
- [ ] Adversarial security review on shell + privileged paths
- [ ] README/SPEC reflect omni identity

---

## New MCP tools summary (estimated final catalogue)

| Wave | New tools |
|---|---|
| 1 | `shell_exec`, `shell_session_start`, `shell_session_exec`, `shell_session_status`, `shell_session_stop`, `shell_session_list`, `workspace_snapshot_create`, `workspace_snapshot_apply` |
| 3 | `registry_suggest_from_samples` |
| 4 | `privileged_exec`, `privileged_list_ops` |
| 5 | `target_list`, `target_probe` (+ optional `target_id` on existing) |

**Estimated final count:** ~46–53 live tools (from 38 today).

---

## What NOT to do

- Remove shell guard globally
- Add generic `sudo bash -c`
- Stream unbounded raw stdout to LLM on normal responses
- Open public TCP listener for daemon
- Auto-activate suggested rules without test + explicit activate
- Spawn commands from MCP adapter crate

---

## START HERE (first coding session)

### Step 1 — Wave 0 verify (1 day)

```bash
cd terminal-commander
cargo nextest run --workspace
# Fix any failures in trust/run_and_watch/command_stop tests
```

### Step 2 — TC49 shell_exec (1–2 weeks)

1. Add `CommandShellStart` to `policy.rs` + tests.
2. Create `shell.rs` with `ShellRuntime::exec` reusing `ProcessProbe`.
3. Wire IPC + MCP tool + fixture.
4. Live e2e O-01.
5. Update tool count 38 → 39 in all anchor files (grep `38` in `crates/mcp/tests/`).

### Step 3 — TC50 sessions (1–2 weeks)

Implement `shell_session_*` on PTY; pass O-02.

### Step 4 — Continue TC51 → TC74 in order

---

## Goal chain reference

| Goal | Wave | Deliverable |
|---|---|---|
| TC49 | 1 | `shell_exec` + policy |
| TC50 | 1 | `shell_session_*` |
| TC51 | 1 | workspace snapshots |
| TC52 | 1 | docs |
| TC53 | 2 | Windows ConPTY |
| TC54 | 2 | macOS tier-1 |
| TC55 | 2 | notify backends |
| TC56 | 2 | process-wrap |
| TC57 | 3 | registry_suggest |
| TC58 | 3 | universal extractors |
| TC59 | 3 | 25+ rule packs |
| TC60 | 3 | pack hints |
| TC61–TC65 | 4 | privileged helper |
| TC66–TC69 | 5 | remote federation |
| TC70–TC74 | 6 | certification + release |

---

## Related repo documents (optional reading)

- Gap analysis: `.planning/cursor_report_03.md`
- Runtime evidence: `EVIDENCE_REPORT_RUNTIME.md`
- Active backlog: `BACKLOG.md`
- Wave plans: `docs/plans/2026-06-09-tc-omni-wave*.md`
- Program index: `.planning/tc-omni-tool/00-INDEX.md`

---

*End of LLM implementation brief. Execute Wave 0, then TC49.*
