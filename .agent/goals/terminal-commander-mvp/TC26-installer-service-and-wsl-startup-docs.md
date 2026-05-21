---
goal_id: TC26
title: Installer Service And Wsl Startup Docs
chain_id: terminal-commander-mvp
phase: Wave 8 - Operator tooling
status: "Pending"
depends_on: ["TC22", "TC23", "TC25"]
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

# TC26 - Installer Service And Wsl Startup Docs

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC26-installer-service-and-wsl-startup-docs.md

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
- Create safe installer, service, configuration, and WSL startup artifacts for local development use without enabling unintended privileged behavior.

non_goals:
- Do not install automatically during tests.
- Do not create a privileged helper unless explicitly designed and blocked behind policy.
- Do not modify system files outside documented install scripts.

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC26-installer-service-and-wsl-startup-docs.md
- install/**
- packaging/**
- config/**
- docs/install/**
- scripts/dev/**
- SECURITY.md
- POLICY.md

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Unrelated application behavior, unrelated documentation, or unrelated repository restructuring.

contracts_or_interfaces:
- Installer artifacts must support dry-run or documented manual review before sudo actions.
- systemd service definitions must run with least privilege by default.
- WSL setup must document limitations and not assume systemd availability unless detected.
- Default: foreground daemon + PID file (per `docs/research/daemon-lifecycle.md`). Optional systemd USER unit (not system unit). NEVER assume systemd on WSL by default.

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Create docs/install/README.md with Linux and WSL installation modes, prerequisites, config paths, data paths, and uninstall steps.
- Install docs document WSL systemd opt-in prerequisite: WSL 0.67.6+ with `/etc/wsl.conf` `[boot] systemd=true`. Document this as opt-in only; default daemon mode does NOT require systemd.
- Create example config files for daemon and policy under config/.
- Create packaging/systemd user-mode and optional system service examples with clear privilege notes.
- Create install scripts with dry-run support and no destructive default behavior.
- Add script tests or shellcheck-style checks if available; otherwise document manual verification.

acceptance_criteria:
- Install docs distinguish user-mode, systemd user service, system service, and WSL modes.
- Install docs clearly distinguish: user-mode foreground, systemd user service (Linux + WSL with systemd opt-in), system service (out of MVP), WSL without systemd.
- Example service files do not run as unrestricted root unless explicitly marked unsupported/deferred/unsafe for MVP.
- Install scripts support dry-run or require explicit confirmation before privileged actions.
- Config examples include limits, policy profile, registry path, event store path, and context spool path.
- No installation command is executed as part of this goal.

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
bash -n install/*.sh || true
test -f docs/install/README.md
test -f config/terminal-commander.example.toml
test -f config/policy.example.toml
```

## Task Prompt

Run TC26 only on branch `feature/terminal-commander-mvp`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Create safe installer, service, configuration, and WSL startup artifacts for local development use without enabling unintended privileged behavior.

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
- TC27 and TC27-provider-harness-integration-examples.md
