---
goal_id: TC33
title: Code Reality Audit And Runtime Pivot
chain_id: terminal-commander-runtime
phase: Wave 0 - Reality audit and vision lock
status: "Completed"
depends_on: []
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-21T18:55:35+00:00"
started_at: "2026-05-21T19:05:00+00:00"
completed_at: "2026-05-21T19:35:00+00:00"
completion_commit: "0dcf2a2"
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

# TC33 - Code Reality Audit And Runtime Pivot

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC33-code-reality-audit-and-runtime-pivot.md

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
- Produce an evidence-backed audit of the current `main` branch and pivot the next runtime goals toward the actual realtime signal-channel product state.

non_goals:
- Do not implement runtime features.
- Do not add dependencies.
- Do not delete, move, or rewrite completed TC01-TC32 goal files.
- Do not claim scaffold-only or partial surfaces are live.

allowed_files_or_area:
- .agent/goals/terminal-commander-runtime/**
- docs/audits/runtime-gap-audit.md
- docs/audits/runtime-source-map.md
- docs/audits/runtime-tool-surface-gap.md
- BACKLOG.md only for source-status clarification, not for deleting P0 items

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- config/**
- rules/**
- scripts/**
- tests/**
- installer/service files

contracts_or_interfaces:
- Classify every audited surface as live, partial, scaffold-only, test-only, degraded, blocked, deferred, or unknown.
- Audit at least daemon binary, MCP binary, ToolSurface, Router, ProcessProbe, PTY, file probe, directory probe, event store, registry, bucket_wait, event_context, policy, audit, and CLI.
- Map each P0/P1 backlog item to exactly one follow-up goal or mark it explicitly deferred.
- Record whether `command_start_combed` can currently run through MCP end-to-end.

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
- Run Branch Guard and capture current commit hash.
- Run baseline verification commands before editing.
- Inspect the code paths listed in `contracts_or_interfaces` and compare them to README/SPEC intent.
- Create `docs/audits/runtime-gap-audit.md` with source-status and evidence for each surface.
- Create `docs/audits/runtime-source-map.md` mapping product concepts to actual modules/files.
- Create `docs/audits/runtime-tool-surface-gap.md` mapping intended MCP tools to implemented, partial, and missing paths.
- If any downstream goal is contradicted by verified code state, amend only the affected runtime goal file and document the amendment.

acceptance_criteria:
- The three audit files exist and are source-status honest.
- Every P0 backlog item is mapped to a specific TC35-TC48 goal or kept explicitly deferred with a reason.
- The audit states whether current `main` can serve an LLM through real MCP stdio and command execution.
- No code implementation occurs in this goal.

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

Run TC33 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Produce an evidence-backed audit of the current `main` branch and pivot the next runtime goals toward the actual realtime signal-channel product state.

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
- TC34-realtime-signal-channel-contract.md
