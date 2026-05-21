---
goal_id: TC01a
title: Readme License Contributing Reconcile
chain_id: terminal-commander-mvp
phase: Wave 0 - Research and discipline
status: "Completed"
depends_on: ["TC01"]
target_branch: "feature/terminal-commander-mvp"
prohibited_branches: ["main", "master"]
worktree_hint: ""
created_at: "2026-05-21T00:00:00+02:00"
started_at: "2026-05-21T15:00:00+02:00"
completed_at: "2026-05-21T15:30:00+02:00"
completion_commit: "e9a9858"
blocked_reason: ""
source_refs:
  - "TC01 research findings; user-locked Apache-2.0 license; rmcp =1.7.0 pin"
  - "Architect deliverables: SPEC.md, ARCHITECTURE.md, ROADMAP.md, CONTRIBUTING.md"
  - "Crate gap: README.md lists 6 crates; SPEC.md/TC04 list 7 (adds terminal-commander-store)"
  - "License gap: README.md says 'License is not selected yet'; user decision = Apache-2.0"
  - "Research evidence: docs/research/_USER_DECISIONS.md, docs/research/license-decision.md, docs/research/_R2-gamma-summary.md"
risk_level: "low"
---

# TC01a - Readme License Contributing Reconcile

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-mvp/TC01a-readme-license-contributing-reconcile.md

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

If the current branch is one of the prohibited branches, or anything other than `feature/terminal-commander-mvp`, do not edit there. Switch to or create the correct worktree/branch, then rerun this Branch Guard.

## Mission Context

- TC01 produced SPEC.md, ARCHITECTURE.md, ROADMAP.md, CONTRIBUTING.md, and the research baseline under `docs/research/`.
- TC01 mini-spec did NOT allow editing `README.md` or creating `LICENSE`. The user-decided license (Apache-2.0) and the architect-extended 7-crate list cannot be reflected in the README from inside TC01.
- TC04 (Rust workspace scaffold) needs a present `LICENSE` file at repo root and SPDX `license = "Apache-2.0"` in Cargo.toml. Without TC01a, TC04 violates scope or ships inconsistent metadata.
- This goal is the minimum reconcile pass that brings README, LICENSE, NOTICE, and CONTRIBUTING into alignment with the locked decisions and the architect spec.

## Mini-Spec

objective:
- Reconcile the user-facing project doctrine (README, LICENSE, NOTICE, CONTRIBUTING) with the locked decisions from TC01 and the architect spec so TC04 and downstream goals can rely on a consistent foundation.

non_goals:
- Do not change SPEC.md, ARCHITECTURE.md, ROADMAP.md, or any TC mini-spec.
- Do not add code, scaffolding, or Cargo manifests.
- Do not change branch policy, security posture, or product scope.
- Do not edit `.agent/goals/terminal-commander-mvp/*` other than this goal file's own status.

allowed_files_or_area:
- .agent/goals/terminal-commander-mvp/TC01a-readme-license-contributing-reconcile.md
- README.md
- LICENSE
- NOTICE
- CONTRIBUTING.md

forbidden_files:
- Any path outside `allowed_files_or_area` except this goal file status update if not already listed.
- Secrets, credentials, private keys, token caches, or environment files containing secrets.
- Generated binaries, build outputs, vendored dependencies, or large log artifacts.
- Any source code, Cargo.toml/.lock, CI files, or `.agent/goals/...` files other than this one.

contracts_or_interfaces:
- README.md must list all 7 canonical crates from SPEC.md (terminal-commander-core, terminal-commander-sifters, terminal-commander-probes, terminal-commander-store, terminal-commanderd, terminal-commander-mcp, terminal-commander-cli).
- README.md license section must state: "Apache-2.0; see LICENSE."
- LICENSE must contain the verbatim, official Apache License 2.0 full text as published by the Apache Software Foundation.
- NOTICE (if added) must follow Apache-2.0 appendix conventions and cite the rmcp Apache-2.0 relicensing transition (cargo-deny allowlist covers both `MIT OR Apache-2.0` and `Apache-2.0`).
- CONTRIBUTING.md (already authored by TC01 architect) may be touched only to fix concrete inconsistencies surfaced against the now-updated README/LICENSE. Do not rewrite.

invariants:
- No unbounded raw terminal or file output may be exposed as a success path.
- Every signal event design or implementation must preserve a bounded source pointer or explain why no pointer can exist.
- Every public contract must be documented or tested before it is treated as live.
- No mock, stub, placeholder, TODO-only, disabled, degraded, or unknown behavior may be reported as completed functionality.
- Security-sensitive operations must be policy-gated and auditable when they are introduced.

implementation_steps:
- Read SPEC.md to confirm the canonical 7-crate list and Apache-2.0 license decision.
- Edit README.md `Repository conventions to establish` section: replace 6-crate list with 7-crate list (add `crates/terminal-commander-store/`). Cite SPEC.md as authority.
- Edit README.md `License` section: replace "License is not selected yet" with "Apache-2.0; see LICENSE." Remove the obsolete "Choose and add a license..." paragraph.
- Create LICENSE at repo root containing the official Apache License 2.0 full text (downloaded from https://www.apache.org/licenses/LICENSE-2.0.txt). Verify SHA256 against the official file.
- Create NOTICE at repo root with: project name (Terminal Commander), copyright line, and a section noting "Upstream dependency rmcp is in Apache-2.0 relicensing transition; the cargo-deny license allowlist covers `MIT OR Apache-2.0` (legacy) and `Apache-2.0` (current)."
- Read CONTRIBUTING.md; if any sentence still refers to the README's 6-crate list or to an undecided license, patch surgically.

acceptance_criteria:
- README.md lists all 7 crates by exact name.
- README.md License section reads exactly: "Apache-2.0; see LICENSE." (or includes that line as the canonical statement).
- LICENSE exists at repo root and matches the official Apache-2.0 text byte-for-byte except for trailing newline normalization.
- NOTICE exists at repo root and includes the rmcp relicensing note.
- CONTRIBUTING.md does not contradict README crate list or the Apache-2.0 license.
- No files outside the allowed list were touched.

evidence_required:
- Branch evidence: `git branch --show-current` output exactly `feature/terminal-commander-mvp`.
- File paths changed.
- Verification command output summary.
- SHA256 of LICENSE compared against the official ASF file at https://www.apache.org/licenses/LICENSE-2.0.txt.
- Explicit source-status notes for any partial work.

stop_conditions:
- Current branch is not exactly `feature/terminal-commander-mvp`.
- The goal requires touching forbidden files.
- LICENSE SHA256 does not match the official ASF Apache-2.0 text.
- A license-compatibility question appears that contradicts SPEC.md's Apache-2.0 decision.
- The README cannot be made consistent with SPEC.md without exceeding the allowed-files set.

verification_command:
```bash
git diff --check
test -f LICENSE
test -f NOTICE
test -f README.md
grep -q "terminal-commander-store" README.md
grep -q "Apache-2.0" README.md
grep -q "Apache License" LICENSE
sha256sum LICENSE
```

## Task Prompt

Run TC01a only on branch `feature/terminal-commander-mvp`. Update README.md crate list (6 -> 7) and license section (-> Apache-2.0). Create LICENSE (official Apache-2.0 text) and NOTICE at repo root. Patch CONTRIBUTING.md only if it contradicts the updated README. Commit verified work, update this file's status fields, report blockers instead of guessing.

## Final Report Format

Objective:
- Reconcile README, LICENSE, NOTICE, and CONTRIBUTING with TC01 locked decisions so TC04 has a consistent foundation.

Changes:
- <focused list>

Files changed:
- <paths>

Verification:
- PASS/FAIL: `<command>` - <summary>

Evidence:
- LICENSE SHA256: <hash> (matches/does-not-match ASF official)
- README crate count: 7 (verified)
- README license line: "Apache-2.0; see LICENSE." (verified)

Commit:
- Verified work commit: `<hash or none>`
- Goal status commit: `<hash or none>`

Known gaps / blockers:
- <none or explicit blocker>

Next goal:
- TC02 and TC02-security-privilege-and-policy-doctrine.md
