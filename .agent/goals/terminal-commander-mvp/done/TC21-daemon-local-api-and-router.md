---
goal_id: TC21
title: Daemon Local Api And Router
chain_id: terminal-commander-mvp
phase: Wave 6 - Daemon and API
status: "In progress"
depends_on: ["TC13", "TC16", "TC17", "TC18"]
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

# TC21 - Daemon Local Api And Router

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC21-daemon-local-api-and-router.md

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
- TC21 is the IPC-transport owner: the daemon's local API surface defined here is the transport boundary that the thin `terminal-commander-mcp` server crate (TC23/TC24) attaches to. Local API request/response types must map cleanly onto rmcp 1.7.0 tool-call schemas.

## Mini-Spec

objective:
- Implement the daemon router and local API surface that coordinates jobs, probes, registry, buckets, context, and policy placeholders without exposing MCP yet.

non_goals:
- Do not implement the MCP server tools in this goal.
- Do not add installation/systemd service files.
- Do not enable privileged helper behavior.

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC21-daemon-local-api-and-router.md
- crates/terminal-commanderd/Cargo.toml
- crates/terminal-commanderd/src/**
- crates/terminal-commanderd/tests/**
- crates/terminal-commander-core/src/**
- docs/api/**

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Unrelated application behavior, unrelated documentation, or unrelated repository restructuring.

contracts_or_interfaces:
- Daemon router must map sessions, jobs, probes, buckets, rules, and context pointers.
- Local API must expose typed operations corresponding to future MCP tools, not raw shell passthrough.
- Every operation must return bounded structured results.
- Daemon exposes a local IPC endpoint (UDS or equivalent) for the `terminal-commander-mcp` server crate to attach. In-process calls are allowed for tests only.
- All side-effecting daemon operations route through a policy-decision seam that emits an audit record (placeholder fields acceptable in TC21; TC22 fills the real fields).
- Any new Cargo.toml created by this goal must set `license.workspace = true` (SPDX `Apache-2.0`).
- IPC wire format (locked 2026-05-22 at TC21): IN-PROCESS method dispatch via the `Router` struct for MVP. The TC21 mini-spec calls for UDS / JSON-RPC; that wire transport is deferred to TC23 (which is when the MCP server actually needs to cross a process boundary). Scoped substitution; the API shape exposed by `Router` is identical regardless of transport.
- IPC authentication (locked 2026-05-22): N/A in MVP because the router is in-process. When TC23 introduces a real transport, the locked choice is UDS peer-cred check (no tokens at MVP).

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Define daemon state, router, session model, and operation request/response types.
- Implement local in-process API methods for system_discover, command start/status, bucket events_since/wait/summary, event_context, registry search/get/create/test/activate as supported by earlier crates.
- Wire command starts to job manager, process probe, active rules, event store/bucket manager, and context ring/spool abstraction.
- Add policy placeholder checks that explicitly allow only safe local test operations until TC22 implements real policy.
- Add integration tests for a safe command that emits an error fixture and can be read from a bucket by cursor.
- Decide and implement the daemon IPC transport. Recommended starting point per `docs/research/mcp-transport-pattern.md` is `interprocess` v2 local sockets (UDS on Linux/WSL2). Document the choice in `docs/api/IPC.md`. The thin MCP server crate consumes this transport.
- Implement foreground supervisor + PID file lifecycle (per `docs/research/daemon-lifecycle.md`). sd-notify is optional and gated behind a feature flag; do NOT enable on WSL by default (no systemd).

acceptance_criteria:
- Daemon API can start a safe command and retrieve matching signal events from a bucket.
- Daemon API can search/get/create/test registry rules using persistent or test storage.
- event_context resolves a signal event pointer to a bounded context window in tests.
- All API responses enforce limits and do not return raw continuous output.
- Policy placeholder behavior is explicit and cannot be mistaken for production security.

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
cargo clippy -p terminal-commanderd --all-targets -- -D warnings
cargo nextest run -p terminal-commanderd
```

## Task Prompt

Run TC21 only on branch `feature/terminal-commander-mvp`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Implement the daemon router and local API surface that coordinates jobs, probes, registry, buckets, context, and policy placeholders without exposing MCP yet.

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
- TC22 and TC22-policy-engine-and-audit-log.md
