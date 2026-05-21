---
goal_id: TC34
title: Realtime Signal Channel Contract
chain_id: terminal-commander-runtime
phase: Wave 0 - Reality audit and vision lock
status: "Completed"
depends_on: ["TC33"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-21T18:55:35+00:00"
started_at: "2026-05-21T19:45:00+00:00"
completed_at: "2026-05-21T20:15:00+00:00"
completion_commit: "eda2096"
blocked_reason: ""
source_refs:
  - "GitHub main repository: https://github.com/special-place-administrator/terminal-commander"
  - "README.md on main: local MCP-operated terminal/file signal-combing layer; raw output in, vetted signal out; context by pointer"
  - "Uploaded BACKLOG.md: P0 blockers rmcp stdio adapter, PTY spawn, UDS IPC, persistent audit log writes"
  - "Uploaded EVIDENCE_REPORT.md: TC01a-TC32 evidence and crate/test inventory"
  - "Uploaded FINAL_REPORT.md: completed chain, scope substitutions, and open runtime gaps"
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/daemon/src/main.rs"
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/mcp/src/main.rs"
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/mcp/src/lib.rs"
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/daemon/src/router.rs"
risk_level: "medium"
---

# TC34 - Realtime Signal Channel Contract

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC34-realtime-signal-channel-contract.md

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
- Codify the realtime signal-channel contract and MCP tool-control surface so every later implementation goal optimizes capability without noise.

non_goals:
- Do not implement daemon IPC, MCP transport, command execution, probe spawning, or file indexing.
- Do not change policy decisions or security posture.
- Do not rename the product or create marketing copy unrelated to runtime contracts.

allowed_files_or_area:
- SPEC.md
- ARCHITECTURE.md
- README.md
- docs/runtime/**
- docs/mcp/**
- docs/contracts/**
- .agent/goals/terminal-commander-runtime/**

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**
- config/**
- tests/** except contract fixture docs if already present

contracts_or_interfaces:
- Define the product as a local abstraction layer with tool-surface control: command execution, probes, filters, indexes, buckets, and bounded context.
- Define the minimum live beta flow: LLM starts command through MCP, daemon runs it, probes stream output, sifters extract signal, bucket_wait returns signal, event_context returns bounded context.
- Define file discovery/search/read semantics as bounded probe/index operations, not full-file dumping.
- Define dynamic filter registry semantics: search, create, test, activate, and bind by unique rule ID.

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
- Read the TC33 audit first.
- Write `docs/runtime/REALTIME_SIGNAL_CHANNEL.md` with product semantics and non-goals.
- Write or update `docs/mcp/TOOL_CONTROL_SURFACE.md` with required beta tool list, request/response shape, and source-status.
- Update SPEC/ARCHITECTURE/README only where they conflict with TC33 evidence or the refined product contract.
- Add unresolved decisions as explicit markers, not silent assumptions.

acceptance_criteria:
- Docs state that the system is more than stored knowledge: it actively probes files and realtime terminal output for the LLM.
- Tool surface lists live, partial, and deferred tools with bounded-output behavior.
- Docs preserve the no-root-shell, no-network-listener, pointer, and bounded-output invariants.
- No implementation code changes are made.

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
```

## Task Prompt

Run TC34 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Codify the realtime signal-channel contract and MCP tool-control surface so every later implementation goal optimizes capability without noise.

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
- TC35-persistent-audit-log-v0003.md
