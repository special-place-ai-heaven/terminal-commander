---
goal_id: TC43
title: File Probe Search Watch And Bounded Read
chain_id: terminal-commander-runtime
phase: Wave 5 - File/disk intelligence probes
status: "Completed"
depends_on: ["TC42d"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-21T18:55:35+00:00"
started_at: "2026-05-22T18:40:00+00:00"
completed_at: "2026-05-22T19:30:00+00:00"
completion_commit: "06ea2d5"
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

## Final Report

Objective:
- Expose bounded file intelligence through daemon-owned probes/tools so the LLM can request bounded line windows, run bounded search, and observe future file changes — without reading whole files, without MCP-side direct file access, and without raw stream output.

Changes (verified work commit `06ea2d5`):
- New `crates/daemon/src/file_watch.rs`: `WatchRuntime` owns live `FileProbe` handles attached to buckets. Mirrors the role `command.rs` plays for `ProcessProbe` but is a separate runtime path — `command.rs` is NOT touched. Tracks `(watch_id, bucket_id, probe_id)` per live watch, threads a per-watch `SifterRuntime`, audits every start/stop, and supports scope-aware rebind via `rebind_watches_in_scope`.
- `crates/daemon/src/state.rs`: `DaemonState` gains `Arc<WatchRuntime>`; bootstrap wires it alongside `CommandRuntime`.
- `crates/daemon/src/ipc/protocol.rs`: 5 new IPC methods (`FileReadWindow`, `FileSearch`, `FileWatchStart`, `FileWatchStop`, `FileWatchList`), 5 new typed error codes (`PathDenied`, `FileNotFound`, `FileBinary`, `OversizedRequest`, `UnknownWatch`), bounded caps (`MAX_FILE_READ_LINES`, `MAX_FILE_READ_BYTES`, `MAX_FILE_SEARCH_MATCHES`, `MAX_FILE_SEARCH_SNIPPET_BYTES`, `MAX_FILE_SEARCH_SCAN_BYTES`) + `DEFAULT_*` matches.
- `crates/daemon/src/ipc/server.rs`: handlers for the 5 new methods. Path-policy gate via `PolicyEngine::evaluate(PolicyAction::FileRead | FileWatch)` BEFORE any I/O. `require_regular_file` enforces "is regular file" so directories surface `FileNotFound`. Read paths catch `InvalidData` and return `FileBinary` so non-UTF-8 never streams to the LLM. The scope validator now resolves `Bucket`/`Job`/`Probe` ids against the UNION of `state.command.live_jobs()` and `state.watch.live_watches()`, so scoped registry activation can target a watch. `handle_registry_activate` / `handle_registry_deactivate` rebind both command jobs AND watches in the requested scope; the returned `jobs_rebound` is the sum.
- `crates/mcp/src/tools.rs`: 5 new MCP tools (`file_read_window`, `file_search`, `file_watch_start`, `file_watch_stop`, `file_watch_list`) forwarding through the daemon UDS only. New MCP-facing DTOs (`McpFileReadWindowParams`, `McpFileSearchParams`, `McpFileWatchStartParams`, `McpFileWatchStopParams`). The tool catalogue grew 17 -> 22 live tools, zero `not_implemented`.
- `crates/mcp/src/lib.rs`: legacy in-process `ToolSurface::file_read_window` shim REMOVED along with `FileReadWindowResponse` and `MAX_FILE_WINDOW_BYTES`. The TC43 contract forbids `std::fs::read` / `tokio::fs` / `File::open` / `read_to_string` / `read_to_end` anywhere in `crates/mcp/src`. The verification grep returns no matches.
- Existing TC41/TC42/TC42b/TC42d count tests (`crates/mcp/tests/mcp_stdio.rs`, `crates/mcp/tests/mcp_live_daemon.rs`, `crates/mcp/src/tools.rs::tests`) updated to the new 22-tool catalogue.
- `crates/mcp/tests/e2e.rs`: dropped the in-process `file_read_window` test; the new daemon-backed + MCP UDS path is covered by `file_ipc` / `file_tools_live_e2e`.

Files changed:
- `crates/daemon/src/file_watch.rs` (new)
- `crates/daemon/src/state.rs`
- `crates/daemon/src/lib.rs`
- `crates/daemon/src/ipc/mod.rs`
- `crates/daemon/src/ipc/protocol.rs`
- `crates/daemon/src/ipc/server.rs`
- `crates/daemon/tests/file_ipc.rs` (new, 9 tests)
- `crates/mcp/src/lib.rs` (TC43 cleanup: removed direct-fs path)
- `crates/mcp/src/tools.rs`
- `crates/mcp/tests/file_tools_live_e2e.rs` (new, 2 tests)
- `crates/mcp/tests/mcp_live_daemon.rs` (tool list grew 17 -> 22)
- `crates/mcp/tests/mcp_stdio.rs` (tool list grew 17 -> 22)
- `crates/mcp/tests/e2e.rs` (dropped removed-shim test)
- `.agent/goals/terminal-commander-runtime/TC43-*.md` (this file)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, rustc 1.95.0, cargo-nextest 0.9.136):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` — clean after work + status commits
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings` — no warnings
- PASS: `cargo test --workspace` — every suite green
- PASS: `cargo nextest run --workspace` — **325/325 passing, 0 skipped**
- PASS: `cargo test -p terminal-commanderd --test file_ipc -- --nocapture` — 9 tests
- PASS: `cargo test -p terminal-commander-mcp --test file_tools_live_e2e -- --nocapture` — 2 tests
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — only doc/negative-assertion matches (`main.rs` docstring, `lib.rs` docstring, e2e comment)
- PASS: `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` — **no matches** (MCP crate does not touch the filesystem directly)

Evidence — explicit acceptance confirmations:

- **No `crates/daemon/src/command.rs` edits.** `git diff --stat HEAD~2..HEAD -- crates/daemon/src/command.rs` returns nothing. File-watch lifecycle lives entirely in the new `file_watch.rs` module; the `merge_active_and_inline` helper is replicated rather than imported so command.rs stays untouched.
- **MCP does not read files directly.** The legacy `ToolSurface::file_read_window` (the sole `std::fs::read` site) was removed in this commit. The verification grep over `crates/mcp/src` returns no `tokio::fs`/`std::fs`/`File::open`/`read_to_string`/`read_to_end` matches. New MCP file tools forward exclusively through `McpDaemonClient::call` (UDS).
- **No raw file/log/tail stream endpoint.** Every new IPC response carries bounded, structured fields only: `FileLine{line, byte_offset, text}` for reads, `FileSearchMatch{line, byte_offset, snippet}` for search (snippet capped to `max_snippet_bytes` ≤ 512), `FileWatchStartResponse` carries only ids + initial cursor. There is no "tail" or "stream" lane; future file content reaches the LLM only as structured `SignalEvent`s produced by scoped sifter rules.
- **File read/search/watch responses are bounded.** Caps enforced at the dispatcher: read at most `MAX_FILE_READ_LINES = 2_000` lines AND `MAX_FILE_READ_BYTES = 64 KiB`; search at most `MAX_FILE_SEARCH_MATCHES = 500` matches with each snippet capped at `MAX_FILE_SEARCH_SNIPPET_BYTES = 512` bytes, and the whole scan capped at `MAX_FILE_SEARCH_SCAN_BYTES = 16 MiB`. Watch responses are id/counter shapes only.
- **File watch emits through daemon/probes/sifters/buckets.** `WatchRuntime::start` spawns a `FileProbe` (existing `crates/probes::file`) wired to a `WatchEventSink` that calls `Router::bucket_append`. The watch never returns frame text to the LLM; only the sifter-produced `EventDraft`s reach the bucket. `file_watch_start_then_append_emits_events_when_rule_active` asserts this end-to-end through the IPC layer.
- **Denied paths are rejected and audited.** `PolicyEngine::evaluate(FileRead | FileWatch)` runs before any open. Denied paths return `IpcErrorCode::PathDenied`; the dispatcher emits the standard `ipc_<method>` audit row with decision=error, and `WatchRuntime::start` emits a `file_watch_start` audit row with decision=deny. Asserted by `file_read_window_denies_default_deny_path`, `file_watch_denies_default_deny_path`, and the MCP-level `file_read_window_denies_sensitive_path_through_mcp`.
- **Missing/binary/unknown-watch cases return typed bounded errors.** `FileNotFound` for missing or non-regular files (`file_read_window_missing_file_returns_typed_error`); `FileBinary` for non-UTF-8 (`file_read_window_rejects_binary`); `OversizedRequest` for empty queries (`file_search_rejects_empty_query`); `UnknownWatch` for stale stop ids (`file_watch_stop_unknown_id_returns_typed_error`). No raw stream content surfaces in any error path.
- **TC39 bucket/context behavior remains compatible.** The full TC39 / TC41 / TC42 / TC42b / TC42c / TC42d test suites pass unchanged — `bucket_wait`, `bucket_events_since`, `event_context`, `command_start_combed`, registry scoped activation. Nextest summary: 325/325 with 0 skipped.

Source-status:
- `file_read_window`, `file_search`, `file_watch_start`, `file_watch_stop`, `file_watch_list` (IPC + MCP): **live (TC43)**.
- `WatchRuntime`, `WatchRebindReport`, `LiveWatchIdentity`: **live (TC43)**.
- File watch backend: polling at 120ms via existing `crates/probes::file` (TC18). Native notify/inotify is explicitly deferred per the TC43 prep amendment; will be revisited as a separate goal if/when justified.
- `ToolSurface::file_read_window`: **deleted** (TC43). The MCP UDS path is the only file_read_window surface now.
- Bounded-output, pointer, and audit invariants: **unchanged**.

Commits:
- Goal file prep amendment: `b12a3c7`
- Verified work commit: `06ea2d5`
- Goal status commit: this commit

Known gaps / blockers:
- None.

Next goal:
- TC44-posix-pty-spawn-and-stdin-control.md — do NOT start until this TC43 report is reviewed.
