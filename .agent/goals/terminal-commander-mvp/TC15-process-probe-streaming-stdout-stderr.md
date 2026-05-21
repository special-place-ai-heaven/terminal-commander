---
goal_id: TC15
title: Process Probe Streaming Stdout Stderr
chain_id: terminal-commander-mvp
phase: Wave 5 - Probes and jobs
status: "Pending"
depends_on: ["TC10", "TC12"]
target_branch: "feature/terminal-commander-mvp"
prohibited_branches: ["main", "master"]
worktree_hint: ""
created_at: "2026-05-21T00:00:00+02:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "User request: Terminal Commander / live terminal-stream signal-combing abstraction for LLMs, 2026-05-21"
  - "Repository: https://github.com/special-place-administrator/terminal-commander.git"
  - "User note: repository is initially empty except the generated README.md already added by user"
  - "Planning source: Terminal Commander product specification v0.1 from ChatGPT session"
risk_level: "high"
---

# TC15 - Process Probe Streaming Stdout Stderr

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC15-process-probe-streaming-stdout-stderr.md

## Goal File Workflow

0. Use the Branch Guard below before editing this goal file, source code, migrations, docs, tests, or generated artifacts.
1. After Branch Guard passes, update this file's frontmatter: set `status` to `In progress` and set `started_at` to an ISO-8601 timestamp.
2. Execute only this goal's mini-spec. Keep changes inside `allowed_files_or_area` and stop if a stop condition is hit.
3. If acceptance criteria pass, run the verification command(s), commit the verified work, then update this file: set `status` to `Completed`, set `completed_at`, and set `completion_commit` to the exact verified work commit hash.
4. Commit the goal-status update as a separate commit unless the repository policy says otherwise.
5. If blocked, set `status` to `Blocked`, set `blocked_reason`, leave `completion_commit` empty unless a verified partial commit exists, and record the blocker in the final report.

## Branch Guard

This goal belongs only to branch:

```text
feature/terminal-commander-mvp
```

Before changing anything, run:

```bash
git branch --show-current
git status --short
```

The branch output must be exactly:

```text
feature/terminal-commander-mvp
```

If the current branch is one of the prohibited branches, or anything other than `feature/terminal-commander-mvp`, do not edit there. Switch to or create the correct worktree/branch, then rerun this Branch Guard. Stop if the correct branch/worktree is unavailable, dirty with unrelated work, or still does not print `feature/terminal-commander-mvp`.

## Mission Context

- Target project: `https://github.com/special-place-administrator/terminal-commander.git`
- Goal chain: `terminal-commander-mvp`
- Source material: user-provided Terminal Commander concept, confirmed branch policy, initial README already added by user, and the Terminal Commander product specification produced in the planning session.
- Current known state: repository is new and user reports it contains the initial README.md; all code, tests, registry, daemon, MCP server, probes, and packaging are otherwise unverified or absent.
- Desired end state: a provider-neutral MCP-operated local signal-combing layer that can run commands, observe terminal/file sources, dynamically manage rules, expose realtime signal buckets, and provide bounded context without raw noisy output.

## Mini-Spec

objective:
- Implement a non-interactive process probe that starts a command, continuously reads stdout/stderr, normalizes frames, writes context, and feeds the sifter runtime.

non_goals:
- Do not implement PTY or interactive stdin support yet.
- Do not implement MCP command_start_combed yet.
- Do not run privileged commands or sudo.
- Do not stream raw output to callers.
- Do not implement Windows-native process spawn paths. `pty-process` / `process-wrap` MVP is POSIX (Linux native + WSL2). Windows-native is deferred (see ROADMAP).

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC15-process-probe-streaming-stdout-stderr.md
- crates/terminal-commander-probes/Cargo.toml
- crates/terminal-commander-probes/src/**
- crates/terminal-commander-probes/tests/**
- crates/terminal-commander-core/src/**
- crates/terminal-commander-sifters/src/**
- tests/fixtures/process/**

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Unrelated application behavior, unrelated documentation, or unrelated repository restructuring.

contracts_or_interfaces:
- Process probe must continuously consume stdout and stderr without waiting for periodic tail reads.
- Each normalized frame must identify stream stdout or stderr and have a source pointer.
- Probe output to callers must be events or bounded context, not raw continuous output.
- Pin: `process-wrap = "9.1"` (with the `tokio1` integration), launching every child in its own process GROUP. Termination ladder is SIGTERM-to-group then SIGKILL after a configurable grace window (shared default with TC16).
- `crates/terminal-commander-probes/Cargo.toml` declares `license.workspace = true` (SPDX `Apache-2.0`); per-file Apache-2.0 headers per project convention.
- cwd and environment are passed through to the child with an explicit advisory-policy placeholder seam: the placeholder denies nothing and is NOT security. Real policy (cap-std Dir handles, default-deny enumeration, audit log) lands in TC22; the placeholder names the hook so TC22 can swap in the live policy without API churn.
- <<DECISION REQUIRED: grace-window default between SIGTERM and SIGKILL (5s vs 10s); shared with TC16 cancellation>>

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Define ProcessProbeConfig, ProcessProbeHandle, ProbeEventSink, and normalized stream frame interfaces.
- Implement command spawning for non-interactive commands via `process-wrap` 9.1 (tokio1) with each child in its own process group. cwd and environment are passed through with an explicit advisory-policy placeholder seam (placeholder denies nothing and is NOT security; real policy lands in TC22). Document the placeholder in code comments and the per-crate README.
- Read stdout and stderr concurrently or without starvation and convert bytes to normalized frames, handling partial lines.
- Feed frames into the sifter runtime and append matched events to an in-memory or test event sink.
- Add tests using safe local commands or fixture-driven fake process streams to prove stdout/stderr handling and event emission.

acceptance_criteria:
- Process probe can execute a safe command in tests and capture both stdout and stderr frames.
- Sifter matches from process output produce signal events with pointers.
- Large output tests avoid returning raw output to the test caller except through bounded test assertions.
- The probe records bytes/frames seen and events emitted metrics.
- No interactive stdin, PTY, sudo, or MCP command tool is added.

evidence_required:
- Branch evidence: `git branch --show-current` output exactly `feature/terminal-commander-mvp`.
- File paths changed.
- Verification command output summary.
- Any new public type, API, route, migration, feature flag, environment variable, event, or status enum introduced.
- Explicit source-status notes for live, partial, degraded, disabled, test-only, mock, blocked, unknown, or deleted behavior touched.

stop_conditions:
- Current branch is not exactly `feature/terminal-commander-mvp`.
- The goal requires touching forbidden files.
- The goal expands into another goal's scope.
- A required interface, route, package, repository path, migration path, branch, or runtime dependency is missing or contradicts this mini-spec.
- Verification cannot run for a reason that is not clearly pre-existing and documented.
- A security, credential, data-retention, privacy, production-safety, or destructive-change question appears that is not answered by this goal file.

verification_command:
```bash
cargo fmt --check
cargo clippy -p terminal-commander-probes --all-targets -- -D warnings
cargo nextest run -p terminal-commander-probes
```

## Task Prompt

Run TC15 only on branch `feature/terminal-commander-mvp`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Implement a non-interactive process probe that starts a command, continuously reads stdout/stderr, normalizes frames, writes context, and feeds the sifter runtime.

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
- TC16 and TC16-job-manager-and-command-exit-events.md
