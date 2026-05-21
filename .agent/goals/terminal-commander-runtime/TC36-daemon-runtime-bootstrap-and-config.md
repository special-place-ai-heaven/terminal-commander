---
goal_id: TC36
title: Daemon Runtime Bootstrap And Config
chain_id: terminal-commander-runtime
phase: Wave 1 - Safety and durable state
status: "Pending"
depends_on: ["TC35"]
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
risk_level: "high"
---

# TC36 - Daemon Runtime Bootstrap And Config

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC36-daemon-runtime-bootstrap-and-config.md

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
- Make `terminal-commanderd` initialize a real daemon runtime state from explicit config instead of exiting as scaffold-only.

non_goals:
- Do not open UDS IPC yet.
- Do not implement rmcp stdio.
- Do not spawn child commands.
- Do not install a system service or privileged helper.

allowed_files_or_area:
- crates/daemon/src/main.rs
- crates/daemon/src/lib.rs
- crates/daemon/src/config.rs
- crates/daemon/src/runtime.rs
- crates/daemon/src/state.rs
- crates/daemon/src/router.rs
- config/**
- docs/install/**
- docs/runtime/**
- tests/**/daemon*
- Cargo.toml only if a daemon-local dependency is truly required
- Cargo.lock only if Cargo.toml changes

forbidden_files:
- crates/mcp/**
- crates/probes/** except no changes
- crates/store/migrations/** except no changes
- setuid/polkit/systemd install behavior
- network listener code

contracts_or_interfaces:
- Daemon runtime state must own or initialize store, registry, bucket manager, context rings, sifter runtime, jobs, probes registry, policy engine, and persistent audit sink.
- Config must expose data_dir, db_path, socket_path, policy profile, response limits, context limits, and runtime mode.
- Daemon main may run a bounded self-check/foreground mode before UDS exists, but it must no longer falsely report TC04 scaffold-only if real state initializes.

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
- Read TC33/TC34 audit and TC35 persistent audit outcome.
- Implement explicit config loading with safe defaults and validation.
- Implement DaemonState/RuntimeState construction from config.
- Wire store, registry import, policy, bucket/context/job managers, sifter runtime, and audit sink.
- Add tests using temp directories and in-memory/test stores.
- Update docs/install to state current runtime start behavior and what remains deferred.

acceptance_criteria:
- `terminal-commanderd` can initialize runtime state in a test or dry-run/self-check mode.
- Config validation errors are specific and do not expose secrets.
- No command execution, MCP stdio, UDS listener, or network listener is added.
- Docs no longer describe daemon runtime state as scaffold-only once verified.

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

Run TC36 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Make `terminal-commanderd` initialize a real daemon runtime state from explicit config instead of exiting as scaffold-only.

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
- TC37-daemon-uds-ipc-and-peer-identity.md
