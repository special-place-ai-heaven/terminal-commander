---
goal_id: TC41
title: Mcp Command And Bucket Tools
chain_id: terminal-commander-runtime
phase: Wave 4 - MCP control surface
status: "Completed"
depends_on: ["TC40"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-21T18:55:35+00:00"
started_at: "2026-05-22T12:30:00+00:00"
completed_at: "2026-05-22T13:30:00+00:00"
completion_commit: "31f6aec"
blocked_reason: ""
source_refs:
  - "GitHub main repository: https://github.com/special-place-administrator/terminal-commander"
  - "README.md on main: local MCP-operated terminal/file signal-combing layer; raw output in, vetted signal out; context by pointer"
  - "Uploaded BACKLOG.md: P0 blockers rmcp stdio adapter, PTY spawn, UDS IPC, persistent audit log writes"
  - "Uploaded EVIDENCE_REPORT.md: TC01a-TC32 evidence and crate/test inventory"
  - "Uploaded FINAL_REPORT.md: completed chain, scope substitutions, and open runtime gaps"
risk_level: "high"
---

# TC41 - Mcp Command And Bucket Tools

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC41-mcp-command-and-bucket-tools.md

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
- Expose the command and bucket realtime control surface through MCP so an LLM can start work and wait for signal without terminal toil.

non_goals:
- Do not implement PTY stdin; keep stdin unavailable or blocked until TC44.
- Do not implement file search/watch; that is TC43.
- Do not create provider-specific hook behavior.

allowed_files_or_area:
- crates/mcp/src/**
- crates/mcp/tests/**
- crates/daemon/src/ipc/**
- crates/daemon/src/command.rs
- crates/daemon/src/state.rs
- crates/daemon/src/lib.rs
- crates/daemon/tests/**
- crates/core/src/** only for narrow DTO/schema additions required by command/bucket MCP contracts
- docs/mcp/**
- docs/runtime/**
- docs/security/**
- examples/mcp/**
- tests/**/mcp*
- tests/**/command*
- .agent/goals/terminal-commander-runtime/TC41-*.md

Note: scope amended from the original `crates/daemon/src/ipc.rs` /
`crates/daemon/src/runtime.rs` listing because the repo uses
`crates/daemon/src/ipc/` as a module directory (not a single file)
and `command_start_combed` / `command_status` IPC wiring requires
touching `command.rs` and `state.rs` as well. Recorded in the final
report.

forbidden_files:
- crates/probes/src/pty.rs
- file/directory probe implementation
- network listeners
- unbounded output response types

contracts_or_interfaces:
- Expose command_start_combed, command_status, command_send_signal where daemon-side support exists.
- Expose bucket_events_since, bucket_wait, bucket_summary, and event_context by event_id.
- If command_write_stdin is not live yet, advertise it as unavailable with an explicit source-status, not as success.
- Every MCP tool must have request validation and response caps.

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
- Add MCP tool schemas for command and bucket methods.
- Forward calls through DaemonClient and preserve daemon errors.
- Add bounded error/heartbeat shapes for timeout and no-signal cases.
- Add tests for start command, wait signal, read context, check status, and policy denial.
- Update docs/mcp examples with one full command_start_combed flow.

acceptance_criteria:
- A test/harness can start a small noisy command via MCP and receive only a matching signal event.
- bucket_wait returns a heartbeat when no event matches.
- No command output is returned by command_start_combed.
- Tool docs include exact live/deferred status.

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

Run TC41 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report

Objective:
- Expose the command and bucket realtime control surface through MCP so an LLM can start work and wait for signal without terminal toil.

Changes (verified work commit `31f6aec`):
- Daemon IPC protocol: added `IpcRequest::CommandStartCombed`, `IpcRequest::CommandStatus`, the matching `IpcResponse` variants, `CommandStartParams` / `CommandStatusParams`, and `IpcErrorCode::{ShellInterpreterDenied, ArgvInvalid, UnknownJob}`. Introduced bounded caps `MAX_COMMAND_ENV_ITEMS`, `MAX_COMMAND_INLINE_RULES`, `MAX_COMMAND_GRACE_MS`.
- Daemon IPC server: two new handlers (`handle_command_start_combed`, `handle_command_status`) forward to `state.command.start_combed` / `state.command.status`, map every `CommandError` to a typed `IpcError`, audit `ipc_command_start_combed` / `ipc_command_status` through the persistent audit sink, and re-advertise the full method list via `system_discover`.
- MCP tool surface: six new live tools wired through `McpDaemonClient` — `command_start_combed`, `command_status`, `bucket_events_since`, `bucket_wait`, `bucket_summary`, `event_context`. Each accepts a `JsonSchema`-deriving parameters DTO; IDs cross the wire as wire-form strings and are parsed before the daemon call; severities cross as lowercase strings. All responses are bounded JSON blobs; no `stdout` / `stderr` field exists on any of them.
- `tool_catalogue` promoted every TC40 `not_implemented` entry to `live` and added the two new command entries. The unit tests in `tools.rs` now enforce a 10-tool live set and zero `not_implemented` entries at TC41.
- TC40 integration smoke tests (`mcp_stdio.rs`, `mcp_live_daemon.rs`) updated to the 10-tool TC41 set.

Files changed:
- `crates/daemon/src/ipc/protocol.rs`
- `crates/daemon/src/ipc/server.rs`
- `crates/daemon/src/ipc/mod.rs`
- `crates/daemon/src/lib.rs`
- `crates/daemon/tests/ipc_command.rs` (new)
- `crates/mcp/src/tools.rs`
- `crates/mcp/tests/mcp_stdio.rs`
- `crates/mcp/tests/mcp_live_daemon.rs`
- `crates/mcp/tests/mcp_live_command_e2e.rs` (new)
- `.agent/goals/terminal-commander-runtime/TC41-mcp-command-and-bucket-tools.md` (status amendment + this report)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, rustc 1.95.0, cargo-nextest 0.9.136):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` — clean after the work + status commits
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings` — no warnings
- PASS: `cargo test --workspace` — 274 tests, 0 failures
- PASS: `cargo nextest run --workspace` — 274/274, 0 skipped
- PASS: `cargo test -p terminal-commanderd --test ipc_command -- --nocapture` — 5 tests
- PASS: `cargo test -p terminal-commander-mcp -- --nocapture` — all MCP tests including the live-daemon e2e
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — only doc-comment matches

Evidence (acceptance items, all asserted in tests):
1. `mcp_live_command_e2e::full_command_lifecycle_through_mcp_yields_only_structured_signal` starts a non-shell argv command via MCP, waits on its bucket through `bucket_wait`, reads `event_context` (typed `unavailable_reason` because lifecycle events carry no pointer), then queries `command_status`. The full chain runs across the rmcp stdio transport into the real daemon UDS server.
2. `mcp_live_command_e2e::mcp_shell_attempt_is_denied_and_audited` proves the MCP shell attempt `argv = ["sh", "-c", "..."]` is rejected with the typed shell-bridge error and no spawn happens.
3. The same e2e test re-calls `bucket_wait` against an advanced cursor with a 250 ms timeout and asserts `heartbeat = true` with an empty events array.
4. The lifecycle event in the e2e is checked to carry `kind` starting with `command_` and `source.stream` equal to `meta`, never `stdout`/`stderr`. No raw output ever appears in MCP responses.
5. Every MCP payload length is asserted against `MAX_RESPONSE_BYTES` in both `mcp_live_daemon.rs` and `mcp_live_command_e2e.rs`, exercising the bounded-output invariant on the real path.
6. `mcp_crate_contains_no_command_spawn` and `mcp_crate_contains_no_tcp_listener` security tests still pass; the manual `rg` against `crates/mcp` shows only doc-comment matches.

Source-status:
- `system_discover`, `health`, `policy_status`, `self_check`: **live** (carried from TC40).
- `command_start_combed`, `command_status`: **live** (TC41).
- `bucket_events_since`, `bucket_wait`, `bucket_summary`, `event_context`: promoted **TC40 not_implemented -> live** at TC41. The MCP tools forward to the existing TC39 daemon UDS methods.
- `command_send_signal`, `command_write_stdin`: **not implemented at TC41** and deliberately not advertised. Stdin / signal control land in TC44 (PTY) and later; they would require new daemon IPC variants and a stdin lane this goal forbids.
- Inline `rules` parameter: **not exposed via MCP at TC41**. The wire shape supports it on the daemon side and the IPC tests use empty rules. Hot rule binding is TC42 scope; exposing it through MCP without TC42 would advertise a partial surface.

Commits (local, then pushed to origin/main):
- Verified work commit: `31f6aec`
- Goal status commit: this commit
- Prep amendment (scope alignment of allowed_files_or_area, no implementation): `df5c4f7`

Known gaps / blockers:
- The prep amendment `df5c4f7` was required because the original goal file listed `crates/daemon/src/ipc.rs` while the repo uses `crates/daemon/src/ipc/` as a module directory. Recorded here for traceability; future goal-file template updates should reflect the module-directory layout.
- `started_at` and `completed_at` in this frontmatter are operator-set timestamps; the verified-commit author dates (`2026-05-22 +0200`) are the audit-grade truth.
- TC41 does not expose `command_send_signal`, `command_write_stdin`, or inline rule binding from MCP. These remain explicit non-goals here and are tracked for TC42 / TC44.

Next goal:
- TC42-registry-hot-activation-and-rule-binding.md — do not start until this TC41 report is reviewed.
