---
goal_id: TC13
title: Registry Store And Rule Crud
chain_id: terminal-commander-mvp
phase: Wave 4 - Storage
status: "Completed"
depends_on: ["TC09", "TC12"]
target_branch: "feature/terminal-commander-mvp"
prohibited_branches: ["main", "master"]
worktree_hint: ""
created_at: "2026-05-21T00:00:00+02:00"
started_at: "2026-05-22T00:15:00+02:00"
completed_at: "2026-05-22T00:50:00+02:00"
completion_commit: "d5fe07f"
blocked_reason: ""
source_refs:
  - "User request: Terminal Commander / live terminal-stream signal-combing abstraction for LLMs, 2026-05-21"
  - "Repository: https://github.com/special-place-administrator/terminal-commander.git"
  - "User note: repository is initially empty except the generated README.md already added by user"
  - "Planning source: Terminal Commander product specification v0.1 from ChatGPT session"
risk_level: "medium"
---

# TC13 - Registry Store And Rule Crud

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC13-registry-store-and-rule-crud.md

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
- Implement persistent rule registry CRUD, versioning, search, test metadata, and activation records without live probe binding yet.

non_goals:
- Do not hot-load rules into running probes.
- Do not implement MCP registry tools.
- Do not allow unvalidated or unsafe rules to be stored as active.

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC13-registry-store-and-rule-crud.md
- crates/terminal-commander-store/src/**
- crates/terminal-commander-store/tests/**
- crates/terminal-commander-core/src/**
- docs/storage/**
- tests/fixtures/rules/**

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Unrelated application behavior, unrelated documentation, or unrelated repository restructuring.

contracts_or_interfaces:
- Rule edits must create new immutable versions, not silently mutate committed versions.
- registry_search must support text and tag-oriented lookup with bounded results.
- Stored rules must pass rule validation from TC09 before commit.
- Backend inherits the TC12 lock: `rusqlite = "0.39"` with the `bundled` feature (FTS5), `refinery = "0.9"` for migrations, WAL mode.
- `crates/terminal-commander-store/Cargo.toml` declares `license.workspace = true` (SPDX `Apache-2.0`); per-file Apache-2.0 headers per project convention.
- `registry_search` is implemented against an FTS5 virtual table over rule_id, tags, and summary; default `LIMIT=50`, max `LIMIT=500`. Out-of-range limit requests are clamped or rejected with a clear error.
- Placement of the registry DB on a `9p` mount (e.g. `/mnt/c` on WSL2) is REJECTED at startup, mirroring TC12. Detect via `/proc/self/mountinfo`.
- Advisory-policy framing: rule activation records are advisory in MVP. They reflect intent recorded by the registry but do not enforce kernel-level isolation; real enforcement (Landlock, seccomp-bpf) is roadmap and lives in TC22 / docs/security/HARDENING.md.
- Minimum tables: `rules`, `rule_versions`, `rule_tags`, `rule_activations` (plus FTS5 virtual table mirroring rule_versions).
- Schema must support import of the 6 user-locked rule-pack file names enumerated in `README.md:367-372` without further migration.
- Rule version identifier (locked 2026-05-22 at TC13): monotonically increasing `u32` per `rule_id`, starting at 1. The `rules.id` table stores the `latest_version` pointer; editing creates a new (rule_id, version+1) row in `rule_versions`. Content hashes are out of MVP scope.
- `latest` pointer (locked 2026-05-22): dedicated `rules.latest_version` column. Updated inside the same transaction as the version insert.
- Row-level validation (locked 2026-05-22): application layer only. `RuleDefinition::validate()` (TC09) is called BEFORE every insert. DB triggers add complexity without buying us anything for an in-process daemon.

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Add registry tables or storage structures for rules, versions, tags, tests, rulesets, and activations as MVP scope allows.
- Implement create_rule_version, get_rule, search_rules, list_versions, and record_rule_test operations.
- Implement activation record storage without requiring live runtime effects yet.
- Add tests for version immutability, latest lookup, invalid rule rejection, tag search, and bounded search result limits.
- Document registry schema and versioning behavior in docs/storage/REGISTRY_STORE.md.

acceptance_criteria:
- Registry can store at least keyword and regex rules with immutable versions.
- Invalid rule definitions are rejected by storage APIs.
- Search returns bounded results and can filter or rank by tags/text as implemented.
- Activation records can be written and read for future live binding goals.
- Tests prove editing creates a new version rather than mutating the old version.

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
- The registry DB file must reside on the daemon's native filesystem (mirrors TC12). Placement on `/mnt/c` (9P) is REJECTED at startup. Detect via `/proc/self/mountinfo`.

verification_command:
```bash
cargo fmt --check
cargo clippy -p terminal-commander-store --all-targets -- -D warnings
cargo nextest run -p terminal-commander-store
```

## Task Prompt

Run TC13 only on branch `feature/terminal-commander-mvp`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Implement persistent rule registry CRUD, versioning, search, test metadata, and activation records without live probe binding yet.

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
- TC14 and TC14-seed-rule-packs-and-registry-import.md
