---
goal_id: TC31
title: Beta Packaging And Release Checklist
chain_id: terminal-commander-mvp
phase: Wave 10 - Beta readiness
status: "Superseded-by-TC48"
depends_on: ["TC26", "TC30"]
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
evidence: "RELEASE_CHECKLIST.md"
---

# TC31 - Beta Packaging And Release Checklist

> Frozen 2026-05-28 as historical record. Live status tracked in `.agent/goals/terminal-commander-runtime/` and `docs/release/MVP_EVIDENCE_REVIEW.md`. Do not edit.
> **Superseded by:** `.agent/goals/terminal-commander-runtime/TC48-beta-gate-evidence-review-and-backlog-rerank.md`.

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC31-beta-packaging-and-release-checklist.md

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
- Prepare beta packaging metadata, release checklist, versioning notes, and installation verification without publishing or deploying anything automatically.

non_goals:
- Do not publish crates, releases, containers, or packages.
- Do not run sudo install steps automatically.
- Do not claim production readiness.

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC31-beta-packaging-and-release-checklist.md
- CHANGELOG.md
- RELEASE.md
- packaging/**
- docs/install/**
- docs/release/**
- README.md

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Unrelated application behavior, unrelated documentation, or unrelated repository restructuring.

contracts_or_interfaces:
- Beta release materials must state supported platforms, unsupported platforms, known risks, install modes, uninstall steps, and verification commands.
- Versioning must be explicit and not imply API stability beyond documented status.
- Release checklist must require evidence from tests, demos, security hardening, and install dry-run.

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Create CHANGELOG.md with initial unreleased/beta entries and honest source-status notes.
- Create RELEASE.md or docs/release/BETA_CHECKLIST.md with pre-release verification steps.
- Update packaging metadata or scripts to include version, binary names, config paths, and artifacts to include/exclude.
- Document install verification and uninstall verification steps for Linux and WSL.
- Update README.md with beta status and clear warning against production/root use unless explicitly configured.

acceptance_criteria:
- Release checklist requires tests, demos, security checks, and install dry-run evidence.
- CHANGELOG.md exists and does not claim a released version unless one is actually tagged or planned.
- Packaging docs list binaries and config paths consistently.
- README.md remains honest about beta/MVP maturity.
- No publication or deployment command is executed.

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
test -f CHANGELOG.md
test -f RELEASE.md || test -f docs/release/BETA_CHECKLIST.md
```

## Task Prompt

Run TC31 only on branch `feature/terminal-commander-mvp`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Prepare beta packaging metadata, release checklist, versioning notes, and installation verification without publishing or deploying anything automatically.

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
- TC32 and TC32-evidence-review-and-backlog-refinement.md
