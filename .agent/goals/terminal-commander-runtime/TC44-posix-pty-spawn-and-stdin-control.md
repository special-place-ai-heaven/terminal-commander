---
goal_id: TC44
title: Posix Pty Spawn And Stdin Control
chain_id: terminal-commander-runtime
phase: Wave 6 - Interactive terminal capability
status: "Pending"
depends_on: ["TC43"]
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
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/probes/src/pty.rs"
risk_level: "high"
---

# TC44 - Posix Pty Spawn And Stdin Control

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC44-posix-pty-spawn-and-stdin-control.md

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
- Add POSIX/WSL PTY spawn and controlled stdin so interactive commands can be observed and steered while still emitting only filtered signal to the LLM.

non_goals:
- Do not support Windows-native ConPTY in this goal.
- Do not pass secrets blindly from the LLM.
- Do not bypass policy for sudo/password prompts.
- Do not expose raw PTY screen buffers by default.

allowed_files_or_area:
- crates/probes/src/pty.rs
- crates/probes/src/process.rs
- crates/probes/src/lib.rs
- crates/daemon/src/runtime.rs
- crates/daemon/src/router.rs
- crates/daemon/src/ipc.rs
- crates/mcp/src/**
- Cargo.toml
- Cargo.lock
- docs/runtime/**
- docs/mcp/**
- tests/**/pty*
- tests/**/stdin*

forbidden_files:
- Windows-native ConPTY implementation
- network listeners
- secret storage
- automatic password entry without explicit policy

contracts_or_interfaces:
- PTY spawn is platform-gated to Linux/WSL POSIX where supported.
- command_write_stdin must target a running interactive job and must be audited.
- Prompt detection must emit prompt events, with secret prompts marked secret and not echoing secret input.
- ANSI/CR normalization remains active before sifter runtime.

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
- Inspect existing AnsiNormalizer and PromptDetector.
- Resolve POSIX PTY crate/dependency with current compatibility and licensing evidence.
- Implement PTY command spawn with stdout/stderr/screen normalization into frames.
- Implement command_write_stdin and prompt event handling.
- Expose command_write_stdin through daemon IPC and MCP only after tests pass.
- Add tests using a pseudo-interactive script, yes/no prompt, and password-like prompt detection without entering a real secret.

acceptance_criteria:
- Interactive PTY command can emit signal events through buckets.
- LLM can write bounded stdin to an interactive job through MCP.
- Secret prompt events are detected and do not leak secret text.
- Unsupported platforms are explicitly blocked, not treated as live success.

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

Run TC44 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Add POSIX/WSL PTY spawn and controlled stdin so interactive commands can be observed and steered while still emitting only filtered signal to the LLM.

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
- TC45-parallel-probe-router-and-multi-bucket-bindings.md
