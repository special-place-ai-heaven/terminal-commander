---
goal_id: TC43
title: File Probe Search Watch And Bounded Read
chain_id: terminal-commander-runtime
phase: Wave 5 - File/disk intelligence probes
status: "Pending"
depends_on: ["TC42d"]
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
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/probes/src/file.rs"
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/probes/src/directory.rs"
risk_level: "high"
---

# TC43 - File Probe Search Watch And Bounded Read

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC43-file-probe-search-watch-and-bounded-read.md

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
- Expose bounded file intelligence through probes/tools so the LLM can ask for file lists, targeted search, line windows, and watched file changes without reading whole files.

non_goals:
- Do not implement a full persistent global search index unless already designed and bounded.
- Do not read denied paths or secret-bearing files.
- Do not return whole large files.
- Do not implement native notify/inotify upgrade unless this goal explicitly confirms the dependency path.

allowed_files_or_area:
- crates/probes/src/file.rs
- crates/probes/src/lib.rs
- crates/daemon/src/ipc/**
- crates/daemon/src/file_watch.rs
- crates/daemon/src/state.rs
- crates/daemon/src/policy.rs
- crates/daemon/src/runtime.rs
- crates/daemon/src/router.rs
- crates/daemon/src/config.rs only for bounded file/search limit wiring
- crates/daemon/src/lib.rs
- crates/daemon/tests/**
- crates/core/src/** only for narrow DTO/schema additions required by file read/search/watch contracts
- crates/store/** only if existing bucket/event/context APIs need narrow integration; no migration unless explicitly justified
- crates/mcp/src/**
- crates/mcp/tests/**
- crates/probes/tests/**
- docs/mcp/**
- docs/runtime/**
- docs/security/**
- docs/rules/** only for file-watch rule examples
- examples/mcp/**
- examples/file-search/** optional
- Cargo.lock only if a genuinely required dependency is added and justified
- relevant crate Cargo.toml only if a genuinely required dependency is added and justified
- .agent/goals/terminal-commander-runtime/TC43-*.md
- .agent/goals/terminal-commander-runtime/GOAL_CHAIN_INDEX.md
- .agent/goals/terminal-commander-runtime/RUN_ORDER.md

Note: `crates/daemon/src/command.rs` is intentionally NOT in the normal allowed edit set. File-watch lifecycle must be a separate runtime path. If implementation proves `command.rs` is required, stop and report the exact seam instead of editing it silently.

forbidden_files:
- PTY spawn
- stdin control
- shell execution
- command execution feature expansion
- directory/artifact probe expansion
- installer/service work
- privileged helper
- sudo behavior
- TCP/HTTP/WebSocket/network listener
- raw stdout/stderr/log/file stream endpoint
- direct file reads from crates/mcp
- direct command spawn from crates/mcp
- native notify/inotify watcher dependency unless separately justified; TC43 must not balloon into the P1 notify backend rewrite
- privileged path bypasses
- unbounded recursive crawl without max depth/count/bytes

contracts_or_interfaces:
- file_read_window must support byte/line windows and max_bytes.
- file_search must search bounded path scopes using allowed globs/max_results/max_bytes and return snippets with pointers, not whole files.
- file_watch/probe_create must create live file probes that emit signal events into buckets.
- Directory/artifact probing must be bounded and policy-gated.

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
- Audit existing file and directory probe code from TC18/TC20.
- Define DTOs for file list, file_search, line window, file_watch, and directory_watch responses.
- Implement policy-gated bounded file search/list/read APIs in daemon.
- Expose live file tools through MCP.
- Add tests for denied path, max bytes, max results, context around line number, future file creation, and file change event.

acceptance_criteria:
- LLM can request line context around a high line number without full-file read.
- Search returns bounded snippets and source pointers.
- File watch emits bucket events when matching content appears.
- Denied secret paths are blocked before reading.

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
git branch --show-current
git status --short
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo nextest run --workspace
# targeted daemon file IPC tests
cargo test -p terminal-commanderd --test file_ipc
# targeted live-daemon MCP file e2e
cargo test -p terminal-commander-mcp --test file_tools_live_e2e
# privilege model guards on the MCP crate
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
# prove MCP does not read files directly
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Scope Amendment (TC43 prep)

This amendment aligns the original TC43 mini-spec with the actual repo layout as of TC42d, and tightens the forbidden list per operator direction. Recorded here per the same precedent as TC41 / TC42.

Drift corrected:

- `crates/daemon/src/ipc.rs` does not exist on `main`. The daemon uses `crates/daemon/src/ipc/` as a module directory; the allowed area now points at `crates/daemon/src/ipc/**`.
- `tests/**/file*` / `tests/**/directory*` are not real paths in this repo. Tests live in `crates/<crate>/tests/`; the allowed area now lists `crates/daemon/tests/**`, `crates/mcp/tests/**`, and `crates/probes/tests/**`.
- The original `allowed_files_or_area` omitted `crates/daemon/src/state.rs`, `crates/daemon/src/policy.rs`, `crates/daemon/src/lib.rs`, and a dedicated `crates/daemon/src/file_watch.rs`, all of which are needed to wire a file-watch lifecycle into the daemon the same way `command.rs` wires a process-probe lifecycle. They are now explicit.
- `crates/probes/src/directory.rs` is removed from the allowed area; TC43 is not the directory/artifact probe expansion goal.
- `Cargo.lock` and per-crate `Cargo.toml` files are now explicitly allowed but only if a genuinely required dependency is added and justified.

Dependency:

- `depends_on` updated from `TC42` to `TC42d` so the chain reflects the TC42b / TC42c / TC42d refinements that landed before file work begins.

Explicit non-allowance:

- `crates/daemon/src/command.rs` is intentionally not in the normal allowed edit set. File-watch lifecycle must be a separate runtime path. If implementation proves `command.rs` is required, stop and report the exact seam instead of editing it silently.

Forbidden list tightened:

- PTY spawn, stdin control, shell execution, command execution feature expansion, directory/artifact probe expansion, installer/service work, privileged helper, sudo behavior, TCP/HTTP/WebSocket/network listener, raw stdout/stderr/log/file stream endpoint, direct file reads from `crates/mcp`, direct command spawn from `crates/mcp`, native notify/inotify watcher dependency unless separately justified (TC43 must not balloon into the P1 notify backend rewrite), privileged path bypasses, unbounded recursive crawl without max depth/count/bytes.

Verification additions:

- `git branch --show-current`, `git status --short`, `cargo test --workspace`, targeted `cargo test -p terminal-commanderd --test file_ipc`, targeted `cargo test -p terminal-commander-mcp --test file_tools_live_e2e`, `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp`, and `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` are now part of the verification command set so the gates are explicit and reproducible.

## Task Prompt

Run TC43 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Expose bounded file intelligence through probes/tools so the LLM can ask for file lists, targeted search, line windows, and watched file changes without reading whole files.

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
- TC44-posix-pty-spawn-and-stdin-control.md
