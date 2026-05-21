---
goal_id: TC12
title: Persistent Event Store And Bucket Cursors
chain_id: terminal-commander-mvp
phase: Wave 4 - Storage
status: "Pending"
depends_on: ["TC07"]
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

# TC12 - Persistent Event Store And Bucket Cursors

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC12-persistent-event-store-and-bucket-cursors.md

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
- Implement persistent event storage and bucket cursor queries using the chosen storage backend while preserving bounded reads.

non_goals:
- Do not implement registry persistence in this goal.
- Do not implement daemon process management or MCP server tools.
- Do not persist raw unbounded terminal output in the event store.

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC12-persistent-event-store-and-bucket-cursors.md
- crates/terminal-commander-store/Cargo.toml
- crates/terminal-commander-store/src/**
- crates/terminal-commander-store/tests/**
- crates/terminal-commander-core/src/**
- docs/storage/**

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Unrelated application behavior, unrelated documentation, or unrelated repository restructuring.

contracts_or_interfaces:
- Persistent bucket reads must be cursor-based and bounded like the in-memory bucket manager.
- Events must be stored append-only per bucket with monotonic seq values.
- Storage schema and migrations must be documented before being treated as live.
- Backend is locked: `rusqlite = "0.39"` with the `bundled` feature (ships FTS5), `refinery = "0.9"` for migrations, `PRAGMA journal_mode=WAL`, batched single-writer transactions per `docs/research/sqlite-fts5.md`.
- Single-writer invariant: the event-store DB has exactly one writer (the daemon process). MCP server and other readers MUST open read-only connections (`OpenFlags::SQLITE_OPEN_READ_ONLY`). Pragmas: `journal_mode=WAL`, `busy_timeout=5000` (ms), `synchronous=NORMAL`.
- Event row schema MUST NOT have BLOB columns (no `raw_bytes` / `payload_blob`). Only `summary TEXT`, `captures JSON`, `pointer JSON`. A schema test asserts no BLOB columns exist in the events table.
- `crates/terminal-commander-store/Cargo.toml` declares `license.workspace = true` (SPDX `Apache-2.0`).
- <<DECISION REQUIRED: FTS5 virtual-table column schema for summary+kind search (which columns are indexed, tokenizer choice)>>
- <<DECISION REQUIRED: retention and compaction policy (time-based, count-based, per-bucket cap, VACUUM cadence)>>
- <<DECISION REQUIRED: backup/snapshot strategy (`VACUUM INTO` vs file-level copy with WAL checkpoint)>>

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Implement against the locked backend: `rusqlite = "0.39"` with the `bundled` feature (ships FTS5), `refinery = "0.9"` for migrations, WAL mode and batched single-writer transactions per `docs/research/sqlite-fts5.md`. Document the lock in `docs/storage/EVENT_STORE.md` and cite the research source.
- Implement event store initialization and refinery migrations for bucket metadata and event rows (no BLOB columns; `summary TEXT`, `captures JSON`, `pointer JSON`).
- Implement append_event, events_since, get_event, and bucket_summary operations against a single-writer connection; expose read-only connection factory for downstream readers.
- Detect placement of the DB file at startup via `/proc/self/mountinfo`; REJECT a DB path resolving to a `9p` filesystem (i.e. `/mnt/c` on WSL2). Document the rejection in `docs/storage/EVENT_STORE.md`.
- Add tests using a temporary database or isolated test directory on the daemon's native filesystem; assert WAL pragma is active and no BLOB columns exist.
- Verify that serialized event fields do not include unbounded raw output.

acceptance_criteria:
- Persistent store can append and read events by bucket cursor.
- events_since enforces a maximum result limit.
- get_event can retrieve a stored event by event_id.
- Storage tests run without external services and clean up temporary files.
- docs/storage/EVENT_STORE.md documents schema, retention assumptions, and limitations.
- Schema test asserts the events table has no BLOB columns; only `summary TEXT`, `captures JSON`, `pointer JSON`.
- Startup test demonstrates that a DB path resolving to a `9p` mount (per `/proc/self/mountinfo`) is rejected with a clear diagnostic.
- WAL pragmas (`journal_mode=WAL`, `busy_timeout=5000`, `synchronous=NORMAL`) are asserted active in tests.

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
- The event-store DB file must reside on the daemon's native filesystem (Linux ext4, or WSL2 native `/home/...`). Placing the DB on `/mnt/c` is REJECTED at startup because SQLite WAL on 9P is unreliable. Detect via `/proc/self/mountinfo`. Stop if tests or implementation attempt to relax this constraint.

verification_command:
```bash
cargo fmt --check
cargo clippy -p terminal-commander-store --all-targets -- -D warnings
cargo nextest run -p terminal-commander-store
```

## Task Prompt

Run TC12 only on branch `feature/terminal-commander-mvp`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Implement persistent event storage and bucket cursor queries using the chosen storage backend while preserving bounded reads.

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
- TC13 and TC13-registry-store-and-rule-crud.md
