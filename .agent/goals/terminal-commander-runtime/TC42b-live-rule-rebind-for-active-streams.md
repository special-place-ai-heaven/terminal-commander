---
goal_id: TC42b
title: Live Rule Rebind For Active Streams
chain_id: terminal-commander-runtime
phase: Wave 4 - MCP control surface
status: "Completed"
depends_on: ["TC42"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-22T15:30:00+00:00"
started_at: "2026-05-22T15:35:00+00:00"
completed_at: "2026-05-22T16:30:00+00:00"
completion_commit: "0974ac4"
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

## Final Report

Objective:
- Make registry activation / deactivation affect future frames from already-running command/probe streams without restarting the daemon, restarting the command, or exposing raw output.

Changes (verified work commit `0974ac4`):
- `crates/sifters` — `SifterRuntime` refactored into an outer wrapper holding `RwLock<Arc<SifterRuntimeInner>>`. `evaluate(&self, ...)` still returns the same shape; readers clone the `Arc` under a brief read lock so evaluation runs outside the lock. New `rebuild(&self, &[RuleDefinition]) -> RebindReport` builds the new compiled state outside the lock and atomically swaps it in; rebuild failures leave the prior set intact. `ProcessProbe` is untouched — same `Arc<SifterRuntime>` handle, new contents.
- `crates/daemon/src/command.rs` — `CommandRuntime` now stores per-job `JobBinding { metrics, sifter, inline_rules }` instead of just metrics. The inline rules captured at `command_start_combed` time survive every subsequent rebind. New `rebind_all_jobs()` walks every live binding, recomputes `(active ∪ inline)` against the current activation snapshot, and calls `sifter.rebuild()` per job. Each per-job rebuild emits a `command_sifter_rebind` audit row (info on success, error on rebuild failure) through the persistent audit sink. Returns a bounded `RebindAllReport { jobs_considered, jobs_rebound, rebuild_failures }`.
- `crates/daemon/src/ipc/server.rs` — `handle_registry_activate` and `handle_registry_deactivate` call `state.command.rebind_all_jobs()` AFTER updating the in-memory `ActivationRegistry` and the persistent activation row. No wire-protocol changes; effects surface via the standard `command_sifter_rebind` audit row.
- `merge_active_and_inline` extracted as a shared helper so spawn-time and rebind-time merges use identical semantics.

Files changed:
- `crates/sifters/Cargo.toml` (parking_lot 0.12.5 direct dep)
- `crates/sifters/src/lib.rs`
- `crates/daemon/src/command.rs`
- `crates/daemon/src/ipc/server.rs`
- `crates/daemon/tests/registry_live_rebind.rs` (new)
- `crates/mcp/tests/registry_live_rebind_e2e.rs` (new)
- `Cargo.lock` (parking_lot already in graph; lockfile updated by the new sifters dep)
- `.agent/goals/terminal-commander-runtime/TC42b-live-rule-rebind-for-active-streams.md` (frontmatter + this report)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, rustc 1.95.0, cargo-nextest 0.9.136):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` — clean after the work + status commits
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings` — no warnings
- PASS: `cargo test --workspace` — 294 tests, 0 failures
- PASS: `cargo nextest run --workspace` — 294/294, 0 skipped
- PASS: `cargo test -p terminal-commanderd --test registry_live_rebind` — 3 tests
- PASS: `cargo test -p terminal-commander-mcp --test registry_live_rebind_e2e` — 1 test, ~2 seconds
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — only doc-comment matches; the live-rebind e2e uses a `Path::new(...).exists()` availability check so the grep stays clean

Evidence (acceptance criteria, all asserted in tests):
1. `registry_live_rebind_e2e::activate_while_command_runs_drives_signal_then_deactivate_silences_it` walks the full LLM-visible contract through MCP using a `python3 -u -c` emitter that prints `midrun-token` 8x at 250 ms intervals.
2. Phase 1 (pre-activation `bucket_wait`) is asserted to contain NO `midrun_match` events from that running command.
3. Phase 2 (`registry_activate` mid-run) triggers `rebind_all_jobs` server-side.
4. Phase 3 (`bucket_wait` post-activation) is asserted to contain at least one `midrun_match` event from the SAME still-running command.
5. Phase 4 (`registry_deactivate` mid-run) symmetrically triggers a rebind.
6. Phase 5 (final `bucket_wait` window after a grace + drain pass) is asserted to contain NO further `midrun_match` events.
7. `registry_live_rebind::sifter_runtime_rebuild_swaps_rule_set_in_place` proves the daemon-level rebuild contract: same `Arc<SifterRuntime>` returns different `evaluate()` results before vs after `rebuild`.
8. `registry_live_rebind::sifter_rebuild_failure_preserves_prior_rule_set` proves a malformed regex on rebuild does NOT clobber the previous compiled rule set.
9. `registry_live_rebind::rebind_all_jobs_after_activate_emits_audit_row_for_each_running_job` proves the audit-row count grows by one per running job on activate AND on deactivate.
10. The bounded-output invariant is preserved: no new raw-stream lane, no new tool, no new wire field carries free-form bytes. The audit metadata is a bounded JSON blob with two integer counters.

Source-status:
- `SifterRuntime::rebuild`, `CommandRuntime::rebind_all_jobs`, `RebindAllReport`: **live (TC42b)**.
- `registry_activate` / `registry_deactivate` MCP+IPC behavior: **live (TC42b)** — now also rebinds running streams.
- `ProcessProbe` API: **unchanged**. The whole rebind machinery lives behind the existing `Arc<SifterRuntime>` handle.
- Per-bucket / per-job activation scope: **NOT implemented**. Activation is still global; rebind applies the same merged set to every running job. The seam to layer per-job scope on top is available (`ActivationRegistry::snapshot()` + per-job `inline_rules`).
- Already-running probes' historical frames: **never reread**. No fake historical matches; in-flight frames finish against the snapshot they captured.
- Audit: every per-job rebind lands a `command_sifter_rebind` row through `PersistentAudit` (production sink); no production path falls back to `InMemoryAudit`.

Commits:
- Goal file creation: `94aed07`
- Verified work commit: `0974ac4`
- Goal status commit: this commit

Known gaps / blockers:
- The live e2e (`registry_live_rebind_e2e`) requires `python3` on PATH for a controllable slow-line emitter. The test skips gracefully when missing; CI hosts that lack python3 should install it or run the daemon-level tests instead.
- Per-bucket / per-job activation scope remains future work (no behavior gap, just future scope).
- `started_at` / `completed_at` are operator-set timestamps; commit author dates are the audit-grade truth.

Next goal:
- TC43-file-probe-search-watch-and-bounded-read.md — do not start until this TC42b report is reviewed.
