---
goal_id: TC35
title: Persistent Audit Log V0003
chain_id: terminal-commander-runtime
phase: Wave 1 - Safety and durable state
status: "Pending"
depends_on: ["TC34"]
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
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/daemon/src/router.rs"
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/store/src/lib.rs"
risk_level: "high"
---

# TC35 - Persistent Audit Log V0003

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC35-persistent-audit-log-v0003.md

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
- Replace the in-memory audit placeholder seam with durable SQLite-backed audit records for policy-relevant runtime actions.

non_goals:
- Do not implement UDS IPC.
- Do not implement rmcp stdio.
- Do not implement command execution or PTY spawn.
- Do not add tamper-evident hash chaining; keep that as P1 unless explicitly authorized.

allowed_files_or_area:
- crates/store/src/**
- crates/store/migrations/**
- crates/daemon/src/audit.rs
- crates/daemon/src/router.rs
- crates/daemon/src/lib.rs
- docs/storage/**
- tests/**/audit*
- BACKLOG.md

forbidden_files:
- crates/mcp/**
- crates/probes/**
- crates/cli/**
- installer/system service files
- network listener code

contracts_or_interfaces:
- Add the next correct audit migration number after inspecting existing migrations; use V0003 only if V0002 is already present or reserved by source evidence.
- Audit records must include action, subject, decision, actor/session if known, timestamp, and bounded metadata.
- Audit must never store raw terminal/file output, secrets, large blobs, or full environment variables.
- Router actions that are already audited through the placeholder must emit durable records once runtime store is configured.

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
- Inspect existing migrations and store schema.
- Define audit schema and Rust API in store.
- Replace or wrap `AuditPlaceholder` with a persistent sink plus test-only fallback.
- Wire Router audit calls through the persistent sink where available.
- Add tests for insert/query, bounded metadata, denied path/action, and no raw output.
- Update storage docs and BACKLOG P0 status only if durable runtime writes are verified.

acceptance_criteria:
- Audit log rows persist across store reopen.
- At least bucket_create, bucket_events_since, bucket_wait, event_context, job_start, job_finish, registry_activate, and file_read_window are representable.
- Tests prove raw output and secret-looking metadata are not stored.
- The old placeholder is removed, isolated as test-only, or source-status labeled partial.

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

Run TC35 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Replace the in-memory audit placeholder seam with durable SQLite-backed audit records for policy-relevant runtime actions.

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
- TC36-daemon-runtime-bootstrap-and-config.md
