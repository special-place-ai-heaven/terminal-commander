---
goal_id: TC45
title: Parallel Probe Router And Multi Bucket Bindings
chain_id: terminal-commander-runtime
phase: Wave 7 - Parallelism and routing
status: "Pending"
depends_on: ["TC44"]
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
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/probes/src/process.rs"
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/daemon/src/router.rs"
risk_level: "high"
---

# TC45 - Parallel Probe Router And Multi Bucket Bindings

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC45-parallel-probe-router-and-multi-bucket-bindings.md

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
- Turn the runtime into a real filter/proxy/router by supporting multiple concurrent probes, multiple buckets, and dynamic routing/binding of rules to sources.

non_goals:
- Do not add distributed/cloud routing.
- Do not add network listeners.
- Do not weaken bucket limits to support fan-out.
- Do not implement every possible probe type; focus on process, PTY, file, and directory surfaces already present.

allowed_files_or_area:
- crates/daemon/src/ipc/**
- crates/daemon/src/router.rs
- crates/daemon/src/runtime.rs
- crates/daemon/src/state.rs
- crates/daemon/src/lib.rs
- crates/daemon/tests/**
- crates/mcp/src/**
- crates/mcp/tests/**
- crates/core/src/** only for narrow DTO/schema additions required by aggregate runtime state
- docs/runtime/**
- docs/mcp/**
- docs/security/**
- .agent/goals/terminal-commander-runtime/TC45-*.md
- .agent/goals/terminal-commander-runtime/GOAL_CHAIN_INDEX.md
- .agent/goals/terminal-commander-runtime/RUN_ORDER.md

Note: TC45 is deliberately the narrow / read-only aggregate runtime interpretation. The following files are intentionally NOT in the normal allowed edit set:
- `crates/daemon/src/command.rs`
- `crates/daemon/src/pty_command.rs`
- `crates/daemon/src/file_watch.rs`
- `crates/store/**`

If implementation proves any of those is required, stop and report the exact seam instead of editing silently. The same precedent TC43/TC44 used.

forbidden_files:
- journal/systemd probe unless explicitly scoped
- cloud/network transport
- privileged helper install
- unbounded queues
- raw stream endpoint
- network listener
- direct command spawn from crates/mcp
- direct file reads from crates/mcp
- shell execution feature expansion
- directory/artifact probe expansion
- TCP/HTTP/WebSocket listener
- new generic `probe_create` spawn API (would duplicate TC38/TC43/TC44 surfaces)
- true fan-out (one probe -> many buckets) bucket-model rewrite — out of scope per Scope Amendment
- session/client association identifiers — out of scope per Scope Amendment
- distributed/cloud routing

contracts_or_interfaces:
- One bucket may receive from many probes — this is already supported via parallel command / file-watch / PTY jobs that scoped registry activation can route into the same bucket. TC45 surfaces a unified view; it does NOT change the underlying bucket model.
- Routing dimensions exposed in the aggregate view: probe_id, job_id, bucket_id, rule_id. Session/client association is OUT of scope per the Scope Amendment.
- Runtime state summary must show: per-runtime live probes (command / pty / file-watch), live buckets, currently-active scoped rules, plus per-probe `frames_seen`, `events_emitted`, `last_seen_at`, `suppressed_count`, and bucket-level `dropped_count` / `head_seq` / `tail_seq` counters.
- Backpressure and dropped-event reporting must be explicit — the existing `BucketSummary.dropped_count` (TC07) and per-probe metrics already expose these; the aggregate view surfaces them in one bounded payload.
- The aggregate view is read-only. It must NOT spawn, cancel, or modify any probe / job / bucket / rule activation.

New methods (read-only, bounded):
- `runtime_state` — single bounded snapshot: counts + per-runtime live lists + active rule scopes + bucket counters.
- `probe_list` — flat list of every live probe across all runtimes; per-probe identity + counters.
- `probe_status` — bounded per-probe lookup.

Explicit deferrals (recorded per Scope Amendment):
- `probe_bind_rules` as a separate tool — DEFERRED. TC42c / TC42d scoped `registry_activate` already covers rule binding semantics (`Global` / `Bucket` / `Job` / `Probe`). Calling it a new tool would just be an alias.
- True fan-out where one probe emits into multiple buckets — DEFERRED to a later architecture goal that touches the router + bucket manager + EventDraft shape.
- Session / client association — DEFERRED. TC37 PeerCred records uid/gid/pid on connection; threading a session id end-to-end requires its own goal.
- Distributed / cloud routing — DEFERRED per `non_goals`.

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
- Model probe registry/state in daemon runtime.
- Implement probe_create/probe_stop/probe_status and probe_bind_rules over daemon API.
- Implement fan-in/fan-out routing for supported probes.
- Expose state summary and probe tools through MCP.
- Add tests with at least two concurrent probes and one bucket fan-in, plus two buckets with distinct rule filters.
- Verify no unbounded channel/queue grows without limits.

acceptance_criteria:
- Parallel probes can run and emit into separate and shared buckets.
- Runtime state summary gives the LLM realtime operational state without raw streams.
- Bucket reads remain bounded under fan-in.
- Dropped/backpressure conditions are explicit in metrics/events.

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
# targeted daemon runtime_state / probe_list / probe_status tests
cargo test -p terminal-commanderd --test runtime_state -- --nocapture
# targeted live-daemon MCP runtime_state / probe_list / probe_status e2e
cargo test -p terminal-commander-mcp --test runtime_state_live_e2e -- --nocapture
# privilege model guards on the MCP crate
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
# prove MCP does not read files directly
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Scope Amendment (TC45 prep)

This amendment aligns the original TC45 mini-spec with the actual repo layout as of TC44 and narrows the scope to the read-only aggregate runtime view. Same precedent as TC41 / TC42 / TC43 / TC44.

Interpretation chosen: **Narrow / read-only aggregate**.

- "Probe registry" = read-only aggregate view across existing runtimes (`CommandRuntime`, `WatchRuntime`, `PtyRuntime`). No new generic `probe_create` spawn API. No new `probe_stop`. No one-probe-to-many-buckets fanout. No session/client association.
- New surface: `runtime_state`, `probe_list`, `probe_status`. All bounded, all read-only.

Drift corrected:

- `crates/daemon/src/ipc.rs` does not exist on `main`; the daemon uses `crates/daemon/src/ipc/` as a module directory. Allowed area now points at `crates/daemon/src/ipc/**`.
- `tests/**/parallel*` / `tests/**/routing*` are not real paths; tests live in `crates/<crate>/tests/`. Allowed area now lists `crates/daemon/tests/**` and `crates/mcp/tests/**`.
- The original `allowed_files_or_area` omitted `crates/daemon/src/state.rs` and `crates/daemon/src/lib.rs`, both of which the new aggregate view needs (re-export the unified DTOs, wire any new IPC handlers into the dispatcher). They are now explicit.
- `crates/probes/src/**` is removed from the allowed area: TC45 does not change probe internals.
- `crates/core/src/**` retained only for narrow DTO/schema additions required by the aggregate runtime state response.

Explicit non-allowance:

- `crates/daemon/src/command.rs` — TC45 must not edit this file. Already locked since TC43.
- `crates/daemon/src/pty_command.rs` — TC45 must not edit this file. Already locked since TC44.
- `crates/daemon/src/file_watch.rs` — TC45 must not edit this file. Already locked since TC43.
- `crates/store/**` — unless a narrow, justified status-store need appears. TC45 is read-only over the in-memory runtimes; it does not persist any new state.

Explicit deferrals:

- `probe_bind_rules` as a separate tool — DEFERRED. TC42c / TC42d scoped `registry_activate` already covers rule binding semantics. A new tool would be an alias and would tempt operators to think the two paths can diverge.
- True fan-out where one probe emits into multiple buckets — DEFERRED to a later architecture goal that touches the router + bucket manager + EventDraft shape.
- Session / client association — DEFERRED.
- Distributed / cloud routing — DEFERRED per `non_goals`.

Forbidden list tightened:

- Raw stream endpoint, network listener, direct command spawn from `crates/mcp`, direct file reads from `crates/mcp`, shell execution feature expansion, directory/artifact probe expansion, TCP/HTTP/WebSocket listener, new generic `probe_create` spawn API, true fan-out bucket-model rewrite, session/client association identifiers, distributed/cloud routing.

Verification additions:

- `git branch --show-current`, `git status --short`, `cargo test --workspace`, targeted `cargo test -p terminal-commanderd --test runtime_state -- --nocapture`, targeted `cargo test -p terminal-commander-mcp --test runtime_state_live_e2e -- --nocapture`, `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp`, `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` are now part of the verification command set so the gates are explicit and reproducible.

## Task Prompt

Run TC45 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Turn the runtime into a real filter/proxy/router by supporting multiple concurrent probes, multiple buckets, and dynamic routing/binding of rules to sources.

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
- TC46-provider-harness-integration-smoke.md
