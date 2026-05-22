---
goal_id: TC47
title: Load Noise And Backpressure Gate
chain_id: terminal-commander-runtime
phase: Wave 8 - Provider-facing validation
status: "Pending"
depends_on: ["TC46"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-21T18:55:35+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "GitHub main repository: https://github.com/special-place-administrator/terminal-commander"
  - "README.md on main: local MCP-operated terminal/file signal-combing layer; raw output in, vetted signal out; context by pointer"
  - "Uploaded BACKLOG.md: P0 blockers rmcp stdio adapter, PTY spawn, UDS IPC, persistent audit log writes"
  - "Uploaded EVIDENCE_REPORT.md: TC01a-TC32 evidence and crate/test inventory"
  - "Uploaded FINAL_REPORT.md: completed chain, scope substitutions, and open runtime gaps"
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/probes/src/process.rs"
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/daemon/src/router.rs"
risk_level: "medium"
---

# TC47 - Load Noise And Backpressure Gate

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC47-load-noise-and-backpressure-gate.md

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
- Source material: current `main` repository, uploaded BACKLOG/EVIDENCE/FINAL reports, and this runtime-pivot chain.
- Current known state: TC01a-TC32 are reported complete and merged to `main`; real-deployment P0 items remain around rmcp stdio, PTY spawn, UDS IPC, and persistent audit writes.
- Desired end state: Terminal Commander becomes a provider-neutral MCP realtime signal abstraction layer where LLMs control probes/tools and receive only structured signal, bounded context, and searchable file/terminal intelligence.

## Mini-Spec

objective:
- Validate that the runtime can comb megabyte-scale noisy terminal/file streams while preserving realtime signal, bounded memory, and explicit backpressure behavior.

non_goals:
- Do not optimize by removing safety checks.
- Do not require multi-gigabyte test artifacts.
- Do not tune for unrealistic benchmark numbers at the expense of correctness.

allowed_files_or_area:
- crates/daemon/tests/**
- crates/mcp/tests/**
- crates/probes/tests/**
- crates/sifters/tests/**
- scripts/smoke/**
- docs/runtime/**
- docs/mcp/**
- docs/security/**
- docs/testing/**
- TESTING.md
- BACKLOG.md
- .agent/goals/terminal-commander-runtime/TC47-*.md
- .agent/goals/terminal-commander-runtime/GOAL_CHAIN_INDEX.md
- .agent/goals/terminal-commander-runtime/RUN_ORDER.md

Note: TC47 is a stress / quality gate, NOT a feature-building goal. Product-code paths are deliberately OUT of normal scope:
- `crates/core/src/**`
- `crates/daemon/src/**`
- `crates/probes/src/**`
- `crates/sifters/src/**`
- `crates/mcp/src/**`

Any of those files may be touched ONLY for a narrow real bug fix discovered by the load gate. If a fix is needed: record the exact bug, evidence, and changed file in the final report. If the fix is larger than narrow, STOP and report instead of expanding TC47.

forbidden_files:
- MCP tool surface redesign
- new MCP tools
- new runtime capabilities
- routing / fanout rewrite
- network listener
- direct command spawn from `crates/mcp`
- direct file reads from `crates/mcp`
- shell execution feature expansion
- raw stdout / stderr / file / PTY stream endpoint
- privileged helper
- installer / service work
- unbounded raw output fixtures committed to repo
- pretending degraded or missing stress evidence is success

contracts_or_interfaces:
- Load tests must generate noisy data at runtime or use small compressed/fixture seeds; do not commit huge raw logs.
- Metrics surfaced: `frames_total` (probe), `events_emitted` (probe + sink), `bytes_total` (probe), `BucketSummary.dropped_count` (TC07), `secret_prompts_total` (PTY, TC44), and the aggregate counters in `RuntimeStateResponse` (TC45).
- Do NOT claim a `frames_suppressed` counter. The daemon does not surface a dedicated suppression counter today. Tests may derive a noise-reduction ratio from `frames_total / events_emitted` where the test owns both the input volume and the matching rule. The missing explicit `frames_suppressed` counter is a backlog item; record it accordingly.
- Signal latency should be measured or bounded enough for beta; if not, record as P0/P1 with evidence.
- A failure must be explicit, not hidden by lower assertion thresholds.

Stress targets (each must be covered or marked Not Run with exact reason):
- At least one megabyte-scale noisy process-output test (REQUIRED — `process` probe via `command_start_combed`).
- File-watch load if feasible without rewriting the polling backend (`crates/probes::file` is poll-based; document the polling boundary if push-rate is bounded by it).
- PTY ANSI/CR/progress-noise load if feasible and stable on WSL.
- `bucket_wait` under concurrent events.
- `bucket_events_since` with capped limits (the existing `MAX_BUCKET_READ_LIMIT = 10_000` cap is the hard ceiling).
- `event_context` bounded windows (`MAX_CONTEXT_FRAMES` / `MAX_CONTEXT_BYTES`).
- `runtime_state` / `probe_list` / `probe_status` under live load.
- Registry activation during active streams (TC42b rebind path) if feasible.

Script decision:
- TC47 implementation MAY create `scripts/smoke/verify-load-gate.sh` only if useful.
- If pure Rust tests are enough, no script is required; the goal is satisfied by `cargo test` evidence.
- The existing `scripts/smoke/verify-runtime-smoke.sh` (TC46) remains a regression gate and MUST still pass.

invariants:
- The product is a realtime signal channel and abstraction layer for LLM agents, not a raw terminal/log dumping tool.
- MCP-facing code must not be an unrestricted root shell and must not spawn commands directly.
- No network listener, no setuid helper, no polkit/system-service install behavior unless a later explicit goal authorizes it.
- Responses visible to the LLM must be bounded, structured, and source-status honest.
- Raw terminal/file output is unavailable by default; bounded context is available only through pointers, file windows, or explicit capped reads.
- Every severity >= Medium signal event must have a source pointer or a pointer_unavailable_reason.
- Do not treat mock, test-only, scaffold-only, degraded, unknown, or disabled behavior as live success.

scope_substitution_policy:
- If the exact implementation path is impossible on the current host, do not silently substitute. Record the reason, source evidence, lost behavior, new source-status, and backlog priority in this goal file and final report.
- A substitute is only acceptable when it preserves the LLM-visible contract: bounded output, policy gate, auditability, source pointer/context, and no raw stream by default.

implementation_steps:
- Create runtime-generated noisy stream fixtures with sparse signal lines.
- Run process, file, and PTY/file-probe paths where live.
- Assert that LLM-visible outputs remain bounded and signal-only.
- Capture metrics in a concise report.
- Update TESTING/BACKLOG with measured results and any new limits.

acceptance_criteria:
- At least one megabyte-scale noisy process-output test passes.
- At least one file-watch/search load test passes or is explicitly blocked with source-status.
- Metrics show suppression ratio and event count.
- No test commits giant raw output files.

evidence_required:
- Branch evidence: `git branch --show-current` output exactly `main`.
- File paths changed.
- Verification command output summary.
- Any new public type, API, route, migration, feature flag, environment variable, event, or status enum introduced.
- Explicit source-status notes for live, partial, degraded, disabled, test-only, mock, blocked, unknown, or deleted behavior touched.
- Evidence that bounded-output and pointer invariants remain true for every LLM-visible response touched by this goal.

stop_conditions:
- Current branch is not exactly `main`.
- The goal requires touching forbidden files.
- The goal expands into another goal's scope.
- A required interface, route, package, repository path, migration path, branch, or runtime dependency is missing or contradicts this mini-spec.
- Verification cannot run for a reason that is not clearly pre-existing and documented.
- A security, credential, data-retention, privacy, production-safety, or destructive-change question appears that is not answered by this goal file.
- A change would create an unbounded raw-output path to the LLM.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo nextest run --workspace
# targeted load/noise/backpressure tests (names finalized in implementation; one per stress target)
cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture
# regression gate from TC46
bash scripts/smoke/verify-runtime-smoke.sh
# optional gate created only if useful
test -x scripts/smoke/verify-load-gate.sh && bash scripts/smoke/verify-load-gate.sh
# privilege model guards on the MCP crate
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
# prove MCP does not read files directly
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Scope Amendment (TC47 prep)

This amendment aligns the original TC47 mini-spec with the actual repo layout as of TC46 and locks the stress / quality gate boundary. Same precedent as TC41 / TC42 / TC43 / TC44 / TC45 / TC46.

Drift corrected:

- `tests/load/**` and `tests/e2e/**` are not real paths in this repo. Tests live in `crates/<crate>/tests/`. Replaced with `crates/daemon/tests/**`, `crates/mcp/tests/**`, `crates/probes/tests/**`, `crates/sifters/tests/**`.
- `scripts/dev/**` is not a real directory. Replaced with `scripts/smoke/**` (the directory TC46 established).
- `scripts/dev/verify-load-gate.sh` does not exist. Replaced with `scripts/smoke/verify-load-gate.sh` AND it is OPTIONAL — TC47 implementation creates it only if useful.
- Allowed creation under `docs/runtime/**`, `docs/mcp/**`, `docs/security/**`, `docs/testing/**` for the stress report and any new metric backlog notes.
- `GOAL_CHAIN_INDEX.md` / `RUN_ORDER.md` allowed only if TC47 wording needs alignment.

Scope rule (locked):

- TC47 is a stress / quality gate, NOT a feature-building goal.
- Product-code paths (`crates/core/src/**`, `crates/daemon/src/**`, `crates/probes/src/**`, `crates/sifters/src/**`, `crates/mcp/src/**`) are OUT of normal TC47 scope.
- Any product-code change must be a narrow real-bug fix discovered by the load gate. Record exact bug + evidence + changed file in the final report. If the fix is larger than narrow, STOP and report.

Forbidden list locked:

- new MCP tools
- new runtime capabilities
- routing / fanout rewrite
- network listener
- direct command spawn from `crates/mcp`
- direct file reads from `crates/mcp`
- shell execution feature expansion
- raw stdout / stderr / file / PTY stream endpoint
- privileged helper
- installer / service work
- unbounded raw output fixtures committed to repo
- pretending degraded or missing stress evidence is success

Stress targets (locked):

- At least one megabyte-scale noisy process-output test (REQUIRED).
- File-watch load if feasible without rewriting the polling backend. Document the polling boundary if push-rate is bounded by it.
- PTY ANSI/CR/progress-noise load if feasible and stable on WSL.
- `bucket_wait` under concurrent events.
- `bucket_events_since` with capped limits (existing `MAX_BUCKET_READ_LIMIT = 10_000` is the ceiling).
- `event_context` bounded windows (`MAX_CONTEXT_FRAMES` / `MAX_CONTEXT_BYTES`).
- `runtime_state` / `probe_list` / `probe_status` under live load.
- Registry activation during active streams (TC42b rebind path) if feasible.

Metric wording:

- Do NOT claim a `frames_suppressed` counter — the daemon does not surface one today. Tests may derive noise reduction from `frames_total / events_emitted` where the test owns both input volume and the matching rule. Record the missing explicit `frames_suppressed` counter as a backlog item.

Acceptance clarification:

- Large noisy output MUST NOT appear raw in MCP responses.
- Matching signal MUST still be emitted.
- Bucket reads / context windows remain bounded by the existing caps.
- Backpressure / drop / loss state MUST be visible where loss occurs.
- If a specific stress area is `Not Run`, record the exact reason; do NOT treat it as pass.
- Full WSL / Linux verification required.

Verification additions:

- `git branch --show-current`, `git status --short`, `cargo test --workspace`, targeted `cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture` (name placeholder; finalized in implementation), `bash scripts/smoke/verify-runtime-smoke.sh`, conditional `bash scripts/smoke/verify-load-gate.sh` (only if created), `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp`, `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` are now part of the verification command set so the gates are explicit and reproducible.

## Task Prompt

Run TC47 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Validate that the runtime can comb megabyte-scale noisy terminal/file streams while preserving realtime signal, bounded memory, and explicit backpressure behavior.

Changes:
- <focused list of implementation changes>

Files changed:
- <paths>

Verification:
- PASS/FAIL: `<command>` — <summary>

Evidence:
- <source-status notes, test output summaries, route/status evidence, screenshots only if rendered UI changed>

Commit:
- Verified work commit: `<hash or none>`
- Goal status commit: `<hash or none>`

Known gaps / blockers:
- <none or explicit blocker>

Next goal:
- TC48-beta-gate-evidence-review-and-backlog-rerank.md
