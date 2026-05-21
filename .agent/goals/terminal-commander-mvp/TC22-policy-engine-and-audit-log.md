---
goal_id: TC22
title: Policy Engine And Audit Log
chain_id: terminal-commander-mvp
phase: Wave 6 - Daemon and API
status: "Pending"
depends_on: ["TC02", "TC21"]
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

# TC22 - Policy Engine And Audit Log

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC22-policy-engine-and-audit-log.md

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
- MVP policy is ADVISORY: enforced inside the daemon via cap-std `Dir` handles + audit log. Not kernel-enforced. Landlock (kernel 5.13+; WSL2 supports it since 5.15.57.1) and seccomp-bpf (`seccompiler`) are documented in a roadmap section but OUT OF SCOPE for TC22. Documentation MUST qualify enforcement as advisory.

## Mini-Spec

objective:
- Implement the first real policy engine and audit log for command execution, file access, registry edits, probe creation, and privileged-operation denial.

non_goals:
- Do not implement a privileged helper or sudo execution.
- Do not weaken tests to bypass policy.
- Do not allow denied paths or forbidden commands under a permissive default.

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC22-policy-engine-and-audit-log.md
- crates/terminal-commanderd/src/**
- crates/terminal-commanderd/tests/**
- crates/terminal-commander-core/src/**
- config/**
- docs/security/**
- POLICY.md

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Unrelated application behavior, unrelated documentation, or unrelated repository restructuring.

contracts_or_interfaces:
- Policy decisions must be explicit allow, deny, require_confirmation, or blocked/unsupported as applicable.
- Audit log records must include timestamp, session/client, operation, sanitized arguments hash or summary, decision, and related job/probe/bucket IDs where available.
- Default policy must deny known sensitive paths and privileged operations.
- Default policy MUST deny: SSH private keys (~/.ssh/id_*), password files (/etc/shadow, ~/.pgpass), credential stores (~/.aws/credentials, ~/.config/gcloud, ~/.kube/config), token caches (~/.cache/* tokens). Document the exact list in `POLICY.md`. Cite README.md:294-297.
- Audit log persists in rusqlite (WAL, refinery-managed migration) with batched single-writer transactions per the event-store lane (cite TC12). Schema: timestamp, session_id, client_id, operation, args_hash, decision, related_job_id, related_probe_id, related_bucket_id.
- Output limits live in probe/event-store enforcement; policy provides configured limits + emits audit record on hit.
- Any new Cargo.toml created by this goal must set `license.workspace = true` (SPDX `Apache-2.0`).
- <<DECISION REQUIRED: policy config format (TOML vs JSON vs YAML)>>
- <<DECISION REQUIRED: seccompiler stub-now-or-defer>>
- <<DECISION REQUIRED: audit-log retention/rotation>>

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Implement policy profile structures and config loading from a safe MVP config format.
- Implement checks for command cwd, forbidden command patterns, file read/watch paths, registry writes, probe creation, max runtime, and max output limits.
- Integrate policy checks into daemon API operations from TC21.
- Implement audit record creation for side-effecting operations and policy denials.
- Add tests for allowed repo command, denied sensitive path, denied forbidden command, registry write audit, and privileged unsupported denial.
- Use `cap-std` `Dir` handles for all probe-side file open operations. Probe code receives a pre-opened `Dir` from the policy engine rather than raw paths.

acceptance_criteria:
- Policy checks execute before side-effecting daemon operations.
- Denied operations do not start jobs, create probes, or read files.
- Audit records are produced for allowed and denied side-effecting operations.
- Default config denies sensitive paths and unsupported privileged actions.
- Tests prove policy cannot be silently bypassed in daemon API calls.

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

Run TC22 only on branch `feature/terminal-commander-mvp`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Implement the first real policy engine and audit log for command execution, file access, registry edits, probe creation, and privileged-operation denial.

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
- TC23 and TC23-mcp-server-discovery-jobs-and-buckets.md
