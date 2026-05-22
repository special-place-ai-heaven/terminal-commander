---
goal_id: NPM02
title: Npm Binary Packaging Contract
chain_id: terminal-commander-npm-distribution
phase: Wave 1 - Distribution audit
status: "Completed"
depends_on: ["NPM01"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: "2026-05-23T11:00:00+00:00"
completed_at: "2026-05-23T11:40:00+00:00"
completion_commit: "81f4ea1"
blocked_reason: ""
source_refs:
  - "NPM01 audit output: docs/release/npm-distribution-audit.md"
  - "npm package.json bin field docs"
  - "npm optionalDependencies docs"
  - "npm trusted publishing docs"
risk_level: "low"
---

# NPM02 - Npm Binary Packaging Contract

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-npm-distribution/NPM02-npm-binary-packaging-contract.md

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

If the current branch is one of the prohibited branches, or anything other than `main`, do not edit there.

## Mission Context

NPM01 produced the audit. NPM02 turns the recommendations into a binding contract: package names, version policy, binary list, target triples, distribution mechanism. No code or `package.json` written yet; this is the spec NPM03 implements.

## Mini-Spec

objective:
- Author `docs/release/npm-packaging-contract.md` locking the npm distribution contract for Terminal Commander: package names, semver policy, exposed `bin` commands, supported target triples, distribution mechanism, and provenance / trusted-publishing posture.

non_goals:
- Do not write `package.json`.
- Do not write platform-package directories.
- Do not write GitHub Actions workflows.
- Do not write release-please config.
- Do not change `crates/**` or runtime behavior.

allowed_files_or_area:
- docs/release/** (the contract document lives here)
- docs/install/** (cross-reference only if needed)
- .agent/goals/terminal-commander-npm-distribution/NPM02-*.md
- .agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**
- config/**
- scripts/**
- .github/workflows/**
- package.json
- packages/**
- release-please-config.json
- .release-please-manifest.json

contracts_or_interfaces:
- Lock the package names (default: root = `terminal-commander`; platform = `@terminal-commander/linux-x64`, `@terminal-commander/linux-arm64`).
- Lock the `bin` commands the root package exposes (default: `terminal-commanderd`, `terminal-commander-mcp`, `terminal-commander`).
- Lock the supported target triples for the initial publish (default: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`).
- Lock the distribution mechanism: platform binary packages pulled via `optionalDependencies` from the root. No `postinstall` binary download.
- Lock the semver policy and the initial version (`0.1.0-beta.1` matches `RELEASE_CHECKLIST.md`).
- Lock the provenance / trusted-publishing posture (default: npm trusted publishing via GitHub Actions OIDC; provenance enabled). No long-lived `NPM_TOKEN` unless trusted publishing is impossible AND explicitly approved here.
- Document the WSL2 fallback for Windows operators using Cursor.

invariants:
- The runtime chain invariants (Unix-only, no Windows-native, no shell expansion, no raw stream lane) carry over verbatim.
- The contract MUST refuse a platform package for an unsupported runtime target (no macOS or Windows package shipped here).

scope_substitution_policy:
- If NPM01 recommended a divergent layout (e.g., platform packages flat under the `@terminal-commander/` org), record the substitution + reason in this contract; do not silently override NPM01.

acceptance_criteria:
- `docs/release/npm-packaging-contract.md` exists and locks the items in `contracts_or_interfaces`.
- Contract is self-contained: NPM03 can implement the layout without re-reading NPM01.
- No code / CI / `package.json` files touched.

evidence_required:
- Branch evidence: `git branch --show-current` output exactly `main`.
- File paths changed.
- Verification command output summary.
- Explicit reference to NPM01 audit conclusions; deltas recorded.

stop_conditions:
- Branch is not `main`.
- The goal would touch any forbidden file.
- NPM01 audit is missing or contradicts the recommended contract without resolution.

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
bash scripts/smoke/verify-runtime-smoke.sh
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM02 only on branch `main`. Lock the contract; do not implement.

## Final Report Format

Objective:
- Lock the npm packaging contract for NPM03+.

Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal (NPM03).
