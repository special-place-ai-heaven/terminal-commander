---
goal_id: TC25
title: Admin Cli And Doctor Commands
chain_id: terminal-commander-mvp
phase: Wave 8 - Operator tooling
status: "Pending"
depends_on: ["TC21", "TC22"]
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
risk_level: "medium"
---

# TC25 - Admin Cli And Doctor Commands

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC25-admin-cli-and-doctor-commands.md

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
- Implement a human admin CLI for status, doctor, rules, buckets, jobs, probes, policy, and audit inspection without bypassing daemon policy.

non_goals:
- Do not create an alternate unrestricted command execution interface.
- Do not duplicate MCP behavior in a way that diverges from daemon contracts.
- Do not expose secrets or unbounded raw context through CLI defaults.

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC25-admin-cli-and-doctor-commands.md
- crates/terminal-commander-cli/Cargo.toml
- crates/terminal-commander-cli/src/**
- crates/terminal-commander-cli/tests/**
- crates/terminal-commanderd/src/**
- docs/cli/**

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Unrelated application behavior, unrelated documentation, or unrelated repository restructuring.

contracts_or_interfaces:
- CLI must call the same daemon/local API contracts used by MCP where applicable.
- doctor must check workspace build assumptions, config paths, data directories, registry availability, policy config, and MCP readiness where possible.
- CLI output must be human-readable and bounded by default.
- `doctor` probes the daemon via the TC21 IPC transport and reports transport mode. IPC transport details deferred to TC21.

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Implement terminal-commander status and doctor commands.
- Implement read-only list/show commands for buckets, jobs, probes, rules, policy, and audit records as available.
- Add explicit flags for any bounded context or event output limits.
- Add tests for CLI argument parsing and command dispatch using test daemon adapters or fixtures.
- Document CLI commands and examples in docs/cli/README.md.
- `doctor` MUST inspect `/proc/self/mountinfo` on Linux/WSL and warn on any configured probe paths that resolve to 9P mounts, reporting the active watcher transport (inotify vs PollWatcher).

acceptance_criteria:
- terminal-commander --help and doctor produce useful bounded output.
- Read-only CLI commands do not bypass policy or directly read sensitive paths.
- CLI tests verify argument parsing and at least one status/doctor path.
- docs/cli/README.md documents commands, limits, and source-status notes.
- No unrestricted shell or raw stream command is added.

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
cargo clippy -p terminal-commander-cli --all-targets -- -D warnings
cargo nextest run -p terminal-commander-cli
```

## Task Prompt

Run TC25 only on branch `feature/terminal-commander-mvp`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Implement a human admin CLI for status, doctor, rules, buckets, jobs, probes, policy, and audit inspection without bypassing daemon policy.

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
- TC26 and TC26-installer-service-and-wsl-startup-docs.md
