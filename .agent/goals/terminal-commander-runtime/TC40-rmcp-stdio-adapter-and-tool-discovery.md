---
goal_id: TC40
title: Rmcp Stdio Adapter And Tool Discovery
chain_id: terminal-commander-runtime
phase: Wave 4 - MCP control surface
status: "Pending"
depends_on: ["TC39"]
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

# TC40 - Rmcp Stdio Adapter And Tool Discovery

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC40-rmcp-stdio-adapter-and-tool-discovery.md

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
- Implement the real rmcp stdio adapter so MCP clients can attach to Terminal Commander instead of only using in-process ToolSurface tests.

non_goals:
- Do not spawn commands from MCP directly.
- Do not add network transport.
- Do not change daemon policy decisions.
- Do not expose every future tool; expose only verified live tools and source-status for partial tools.

allowed_files_or_area:
- crates/mcp/src/main.rs
- crates/mcp/src/lib.rs
- crates/mcp/src/daemon_client.rs
- crates/mcp/src/tools.rs
- crates/mcp/Cargo.toml
- Cargo.toml
- Cargo.lock
- docs/mcp/**
- examples/mcp/**
- tests/**/mcp*

forbidden_files:
- crates/daemon/src/** except no changes
- crates/probes/**
- privileged helper or service files
- TcpListener
- UdpSocket

contracts_or_interfaces:
- Use the existing/pinned rmcp dependency if present; if not present, resolve the current supported crate from official source and commit Cargo.lock.
- MCP stdio adapter must forward to DaemonClient/UDS; it must not own Router or spawn commands directly.
- Tool discovery must list only live tools as live; partial/deferred tools must be omitted or marked not_implemented, not silently accepted.
- All responses must preserve bounded-output caps.

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
- Inspect current Cargo.toml/Cargo.lock and official rmcp API compatibility.
- Implement stdio server startup in `terminal-commander-mcp`.
- Map system_discover, policy_status, bucket_events_since, bucket_wait, bucket_summary, event_context, and file_read_window if live.
- Add tool-discovery metadata with source-status.
- Add integration test or harness smoke that exercises stdio request/response without a provider-specific client.

acceptance_criteria:
- `terminal-commander-mcp` can start as an MCP stdio server in a test/harness.
- Tool discovery returns live runtime tools and does not advertise scaffold-only paths as working.
- MCP crate still contains no Command::spawn, TcpListener, or UdpSocket.
- The adapter forwards through daemon client/IPC boundary.

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
grep -R "Command::new\|Command::spawn\|TcpListener\|UdpSocket" -n crates/mcp && false || true
```

## Task Prompt

Run TC40 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Implement the real rmcp stdio adapter so MCP clients can attach to Terminal Commander instead of only using in-process ToolSurface tests.

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
- TC41-mcp-command-and-bucket-tools.md
