---
goal_id: TC46
title: Provider Harness Integration Smoke
chain_id: terminal-commander-runtime
phase: Wave 8 - Provider-facing validation
status: "Pending"
depends_on: ["TC45"]
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
risk_level: "medium"
---

# TC46 - Provider Harness Integration Smoke

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC46-provider-harness-integration-smoke.md

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
- Prove a provider-neutral MCP harness can use Terminal Commander as the abstraction layer for command execution, file probing, bucket_wait, and bounded context.

non_goals:
- Do not depend on proprietary provider hooks.
- Do not require Claude-only behavior or Codex-only behavior.
- Do not send real secrets or run destructive commands.
- Do not add production installer behavior.

allowed_files_or_area:
- docs/integrations/**
- examples/**
- scripts/dev/**
- tests/e2e/**
- crates/mcp/src/** only for test harness fixes
- crates/daemon/src/** only for test harness fixes
- .agent/goals/terminal-commander-runtime/**

forbidden_files:
- privileged install files
- network listener additions
- provider credential files
- destructive filesystem commands

contracts_or_interfaces:
- Smoke tests must use MCP stdio and the local daemon/UDS path, not in-process mocks.
- The demo command must produce lots of noise and one or more matching signal lines.
- The harness must prove command_start_combed -> bucket_wait -> event_context -> file_read/search where applicable.
- If a provider cannot run in CI/local environment, document the exact blocker and keep the generic MCP harness as evidence.

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
- Create deterministic e2e scripts that start daemon runtime, start MCP stdio adapter, issue MCP tool calls, and stop both cleanly.
- Add Claude Code and Codex CLI config examples without credentials or absolute user-only paths.
- Run one command-output scenario and one file-search/watch scenario.
- Capture bounded output excerpts and cursor/context evidence in docs/integrations.
- Add cleanup logic for temp dirs, sockets, child processes, and stores.

acceptance_criteria:
- Generic MCP stdio harness passes locally.
- Docs include commands/config for at least two MCP-capable clients or explain a verified blocker.
- Smoke evidence shows signal-only output and bounded context, not raw stream dumping.
- No provider secrets, tokens, or credentials are committed.

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
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
bash scripts/dev/verify-runtime-smoke.sh
```

## Task Prompt

Run TC46 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Prove a provider-neutral MCP harness can use Terminal Commander as the abstraction layer for command execution, file probing, bucket_wait, and bounded context.

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
- TC47-load-noise-and-backpressure-gate.md
