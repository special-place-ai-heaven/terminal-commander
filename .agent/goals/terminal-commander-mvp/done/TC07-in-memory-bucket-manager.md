---
goal_id: TC07
title: In Memory Bucket Manager
chain_id: terminal-commander-mvp
phase: Wave 2 - Core model
status: "Completed"
depends_on: ["TC06"]
target_branch: "feature/terminal-commander-mvp"
prohibited_branches: ["main", "master"]
worktree_hint: ""
created_at: "2026-05-21T00:00:00+02:00"
started_at: "2026-05-21T19:15:00+02:00"
completed_at: "2026-05-21T20:00:00+02:00"
completion_commit: "ba39140"
blocked_reason: ""
source_refs:
  - "User request: Terminal Commander / live terminal-stream signal-combing abstraction for LLMs, 2026-05-21"
  - "Repository: https://github.com/special-place-administrator/terminal-commander.git"
  - "User note: repository is initially empty except the generated README.md already added by user"
  - "Planning source: Terminal Commander product specification v0.1 from ChatGPT session"
risk_level: "medium"
---

# TC07 - In Memory Bucket Manager

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC07-in-memory-bucket-manager.md

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
- Implement an in-memory signal bucket manager with monotonic cursors, bounded reads, severity filtering, summaries, and tests.

non_goals:
- Do not implement persistent storage, MCP tools, daemon APIs, or command execution.
- Do not add probe input sources.
- Do not return raw context through bucket reads.

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC07-in-memory-bucket-manager.md
- crates/terminal-commander-core/src/**
- crates/terminal-commander-core/tests/**

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Unrelated application behavior, unrelated documentation, or unrelated repository restructuring.

contracts_or_interfaces:
- Bucket reads must use a caller-provided cursor and return next_cursor.
- Bucket responses must be bounded by a caller-provided or default limit.
- Bucket summaries must expose event counts, severity counts, latest sequence, and noise-removal counters if available.
- Per-bucket seq pinned to `u64` (mirrors TC12 SQLite INTEGER / TC06 SignalEvent.seq); document conversion site to SQLite i64.
- BucketSummary MUST reserve fields `noise_suppressed_count: u64` and `dedupe_collapsed_count: u64` (zero-default until TC11 wires noise/dedupe).
- Concurrency: BucketManager MUST be `Send + Sync`; use `parking_lot::RwLock` or `tokio::sync::RwLock` (no std `Mutex` for the public surface).
- tokio 1 is the locked async runtime; if any read API becomes async it MUST use `tokio::sync::RwLock`.
- In-memory cap (locked 2026-05-21 by operator): per-bucket max events AND per-bucket TTL, both operator-tunable via `BucketConfig`. Defaults: `max_events = 10_000`, `ttl = Duration::from_secs(24 * 3600)` (24h). Eviction is FIFO when `max_events` is reached; events older than `ttl` are evicted on read or on a periodic sweep.
- Backpressure (locked 2026-05-21 by operator): drop-oldest with `dropped_count` counter. On `append` when the bucket is full, the head event is evicted and `dropped_count` is incremented. The counter is surfaced in `BucketReadResponse` and `BucketSummary` so consumers can detect loss. Producers never block.

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Add BucketId, BucketConfig, BucketState, BucketSummary, BucketReadRequest, and BucketReadResponse types if not already present.
- Implement an in-memory BucketManager that appends SignalEvent records with monotonic seq values per bucket.
- Implement events_since with cursor, severity_min, event kind filter if straightforward, and limit.
- Implement summary calculation with event totals and severity counts.
- Add tests for empty bucket reads, cursor advancement, severity filtering, limit behavior, and duplicate sequence prevention.

acceptance_criteria:
- BucketManager can create a bucket, append events, read since cursor, and summarize state.
- events_since never returns more than the configured or requested limit.
- Severity filtering works according to the Severity ordering from TC06.
- Read responses do not include raw terminal/file output.
- Tests cover cursor behavior before, at, and after latest sequence.

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
git diff --check
cargo fmt --check
cargo clippy -p terminal-commander-core --all-targets -- -D warnings
cargo nextest run -p terminal-commander-core -E 'test(bucket)'
```

## Task Prompt

Run TC07 only on branch `feature/terminal-commander-mvp`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Implement an in-memory signal bucket manager with monotonic cursors, bounded reads, severity filtering, summaries, and tests.

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
- TC08 and TC08-context-ring-and-bounded-context-windows.md
