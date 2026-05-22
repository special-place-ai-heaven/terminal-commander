---
goal_id: TC42b
title: Live Rule Rebind For Active Streams
chain_id: terminal-commander-runtime
phase: Wave 4 - MCP control surface
status: "Pending"
depends_on: ["TC42"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-22T15:30:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "GitHub main repository: https://github.com/special-place-administrator/terminal-commander"
  - "TC42 final report: ActivationRegistry global scope; hot rebind for running probes left as known gap"
risk_level: "high"
---

# TC42b - Live Rule Rebind For Active Streams

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC42b-live-rule-rebind-for-active-streams.md

## Goal File Workflow

0. Use the Branch Guard below before editing this goal file, source code, migrations, docs, tests, or generated artifacts.
1. After Branch Guard passes, update this file's frontmatter: set `status` to `In progress` and set `started_at` to an ISO-8601 timestamp.
2. Execute only this goal's mini-spec. Keep changes inside `allowed_files_or_area` and stop if a stop condition is hit.
3. If acceptance criteria pass, run the verification command(s), commit the verified work, then update this file: set `status` to `Completed`, set `completed_at`, and set `completion_commit` to the exact verified work commit hash.
4. Commit the goal-status update as a separate commit unless repository policy says otherwise.
5. If blocked, set `status` to `Blocked`, set `blocked_reason`, leave `completion_commit` empty unless a verified partial commit exists, and record the blocker in the final report.

## Branch Guard

This goal belongs only to branch:

```text
main
```

Before changing anything, run:

```bash
git branch --show-current
git status --short
```

The branch output must be exactly:

```text
main
```

If the current branch is one of the prohibited branches, or anything other than `main`, do not edit there. Switch to or create the correct worktree/branch, then rerun this Branch Guard. Stop if the correct branch/worktree is unavailable, dirty with unrelated work, or still does not print `main`.

## Mission Context

- Target project: https://github.com/special-place-administrator/terminal-commander
- Goal chain: terminal-commander-runtime
- Source material: TC42 final report and the `ActivationRegistry` it introduced.
- Current known state: TC42 made `registry_activate` / `registry_deactivate` affect every newly-started command but explicitly left already-running commands frozen on the rule set they captured at spawn time.
- Desired end state: registry activation/deactivation also reaches frames produced after the change by commands that are still running, without restarting the daemon, restarting the command, exposing raw output, or replaying past frames.

## Mini-Spec

objective:
- Make registry activation / deactivation affect future frames from already-running command/probe streams. A rule activated mid-run must fire on subsequent matching lines; a rule deactivated mid-run must stop firing on subsequent lines.

non_goals:
- Do not introduce a PTY runtime or stdin lane.
- Do not expose raw stdout/stderr to the LLM.
- Do not implement file/directory/artifact probe features.
- Do not rescan historical frames; "no fake historical matches" is locked.
- Do not change MCP tool names or the wire shape of TC41/TC42 responses beyond bounded metadata.

allowed_files_or_area:
- crates/daemon/**
- crates/sifters/**
- crates/store/src/** only if activation state needs narrow store access
- crates/core/src/** only for narrow DTO/schema additions required by rebind contracts
- crates/mcp/src/** only if existing `registry_activate` / `registry_deactivate` responses need bounded rebind metadata
- crates/mcp/tests/**
- crates/daemon/tests/**
- docs/runtime/**
- docs/rules/**
- docs/mcp/**
- tests/**/registry*
- tests/**/sifter*
- tests/**/mcp*
- .agent/goals/terminal-commander-runtime/TC42b-*.md

forbidden_files:
- crates/probes/** (the probe API is locked at this goal; rebinding must happen inside the sifter or daemon layer)
- PTY spawn implementation
- file / directory / artifact probe implementation
- network listeners
- raw stdout/stderr/log/tail stream endpoint
- shell execution
- privileged helper
- direct command spawn from crates/mcp
- unsafe or unbounded regex execution

contracts_or_interfaces:
- `SifterRuntime` must support an atomic in-place rule-set swap such that callers holding `Arc<SifterRuntime>` keep working (no new probe API).
- `CommandRuntime` must track each running job's inline rules so the merged active+inline set is reproducible on rebind.
- `registry_activate` / `registry_deactivate` IPC handlers must trigger a rebind across all running jobs after updating the activation registry and persistent row.
- Inline rules attached to a specific job MUST survive rebinds caused by global activation changes.
- Rebind work must be bounded, audited, and must not block the request loop for an unbounded time.
- No raw stream content may surface in any response or audit row introduced by this goal.

invariants:
- The product is a realtime signal channel and abstraction layer for LLM agents, not a raw terminal/log dumping tool.
- MCP-facing code must not be an unrestricted root shell and must not spawn commands directly.
- No network listener, no setuid helper, no polkit/system-service install behavior.
- Responses visible to the LLM must be bounded, structured, and source-status honest.
- Raw terminal/file output is unavailable by default; bounded context is available only through pointers.
- Every severity >= Medium signal event must have a source pointer or a `pointer_unavailable_reason`.
- Do not treat mock, test-only, scaffold-only, degraded, unknown, or disabled behavior as live success.

scope_substitution_policy:
- If true rebind is impossible with current internals without touching `crates/probes/**`, do NOT silently degrade. Stop and produce a structured implementation-seam report; mark this goal Blocked.
- A substitute is only acceptable when it preserves the LLM-visible contract: bounded output, policy gate, auditability, source pointer/context, and no raw stream by default.

implementation_steps:
- Refactor `SifterRuntime` so its internal compiled state lives behind an atomic-swap container (e.g. `RwLock<Arc<...>>`). Preserve the `evaluate(&self, ...)` API so `ProcessProbe` is unchanged.
- Add `SifterRuntime::rebuild(&self, rules: &[RuleDefinition])` that builds the new compiled state outside the lock then swaps it in.
- Extend `CommandRuntime`'s per-job tracking to carry the per-job inline rules + the live `Arc<SifterRuntime>` handle.
- Add `CommandRuntime::rebind_all_jobs()` which takes the current activation snapshot, recomputes `(active ∪ inline)` per running job, and calls `rebuild()` on each.
- Make the registry IPC handlers call `rebind_all_jobs()` after a successful activate / deactivate.
- Emit a bounded audit row recording the rebind effect (job count touched).
- Add daemon tests that prove the sifter swap is observable from a frame fed before vs after.
- Add MCP live-daemon e2e proving the LLM-visible contract: pre-activation silence, mid-run activation produces matches on new frames, mid-run deactivation stops matches on subsequent frames.

acceptance_criteria:
- A long-running non-shell command is started.
- Before activation, a matching line from that command produces no rule-driven signal.
- A rule is activated through MCP after the command is already running.
- A later matching line from the SAME still-running command produces the rule's signal.
- That rule is deactivated through MCP while the command is still running.
- A later matching line from the SAME still-running command no longer produces that rule's signal.
- No raw stdout/stderr appears in any MCP or daemon response.
- `command_status`, `bucket_wait`, `bucket_events_since`, `event_context` all keep working.
- All TC41 and TC42 tests still pass.
- All MCP tool catalogues remain honest; no tool is silently advertised as live if it is only partial.

evidence_required:
- Branch evidence: `git branch --show-current` output exactly `main`.
- File paths changed.
- Verification command output summary.
- Any new public type, API, route, migration, feature flag, environment variable, event, or status enum introduced.
- Explicit source-status notes for live, partial, degraded, disabled, test-only, mock, blocked, unknown, or deleted behavior touched.
- Evidence that bounded-output and pointer invariants remain true for every LLM-visible response touched by this goal.

stop_conditions:
- Current branch is not exactly `main`.
- The goal would require touching `crates/probes/**` to make rebinding work.
- Rebinding would require restarting the running command.
- A required change would expose raw stdout/stderr by default.
- Verification cannot run on Linux/WSL.
- The goal expands into another goal's scope (PTY, file probe, registry hot activation core schema, etc.).

verification_command:
```bash
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

## Task Prompt

Run TC42b only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Make registry activation / deactivation affect future frames from already-running command/probe streams.

Changes:
- <focused list of implementation changes>

Files changed:
- <paths>

Verification:
- PASS/FAIL: `<command>` — <summary>

Evidence:
- <source-status notes, test output summaries>

Commit:
- Verified work commit: `<hash or none>`
- Goal status commit: `<hash or none>`

Known gaps / blockers:
- <none or explicit blocker>

Next goal:
- TC43-file-probe-search-watch-and-bounded-read.md
