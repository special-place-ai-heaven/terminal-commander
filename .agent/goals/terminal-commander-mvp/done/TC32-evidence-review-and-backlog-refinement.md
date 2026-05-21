---
goal_id: TC32
title: Evidence Review And Backlog Refinement
chain_id: terminal-commander-mvp
phase: Wave 10 - Beta readiness
status: "Pending"
depends_on: ["TC31"]
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
risk_level: "low"
---

# TC32 - Evidence Review And Backlog Refinement

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC32-evidence-review-and-backlog-refinement.md

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
- Review all completed goals, consolidate evidence, identify unresolved gaps, and produce the next backlog without implementing new features.

non_goals:
- Do not implement new product behavior.
- Do not rewrite completed goals to hide blockers.
- Do not mark unknown, partial, degraded, or blocked behavior as complete.

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC32-evidence-review-and-backlog-refinement.md
- .agent/goals/terminal-commander-mvp/GOAL_CHAIN_INDEX.md
- .agent/goals/terminal-commander-mvp/RUN_ORDER.md
- .agent/goals/terminal-commander-mvp/RISK_REGISTER.md
- .agent/goals/terminal-commander-mvp/ASSUMPTIONS.md
- docs/release/**
- ROADMAP.md
- BACKLOG.md

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Unrelated application behavior, unrelated documentation, or unrelated repository restructuring.

contracts_or_interfaces:
- Backlog items must be evidence-backed and separated into defects, hardening, feature work, documentation, and integration tasks.
- Final review must list live, partial, blocked, deferred, mock/test-only, and unknown behavior separately.
- Goal chain index must reflect actual status evidence rather than desired status.

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Review all goal files in the chain and collect completion commits, blockers, verification evidence, and known gaps.
- Update GOAL_CHAIN_INDEX.md and RUN_ORDER.md statuses if repository policy permits status maintenance.
- Update RISK_REGISTER.md and ASSUMPTIONS.md with resolved and unresolved items.
- Create BACKLOG.md with evidence-backed next goals split by priority and risk.
- BACKLOG.md MUST inherit the deferred-items list from `docs/research/_R1-alpha-summary.md`, `docs/research/_R1-beta-summary.md`, and `docs/research/_USER_DECISIONS.md`: IPC transport (resolved at TC21 or carried), kernel-level enforcement via Landlock/seccomp-bpf, macOS FSEvents probe design, encryption-at-rest for audit store, cross-host federated audit, daemonize crate choice, portable-pty Windows-native bridge.
- Create docs/release/MVP_EVIDENCE_REVIEW.md summarizing what is live, partial, blocked, deferred, and unknown.
- Record the most recent green run of the 7-step CI sequence (fmt -> clippy -> deny -> hack --each-feature -> hack --rust-version -> nextest+doc -> machete) per `docs/research/_R2-gamma-summary.md` in `MVP_EVIDENCE_REVIEW.md`.

acceptance_criteria:
- BACKLOG.md exists and every item cites evidence or a source-status note.
- MVP_EVIDENCE_REVIEW.md distinguishes live, partial, blocked, deferred, test-only, and unknown behavior.
- RISK_REGISTER.md and ASSUMPTIONS.md are updated with final MVP review notes.
- No new implementation behavior is added.
- Next development chain recommendations are clear and dependency-aware.

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
test -f BACKLOG.md
test -f docs/release/MVP_EVIDENCE_REVIEW.md
```

## Task Prompt

Run TC32 only on branch `feature/terminal-commander-mvp`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Review all completed goals, consolidate evidence, identify unresolved gaps, and produce the next backlog without implementing new features.

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
- none
