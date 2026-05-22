---
goal_id: NPM06
title: Release Please Manifest Config
chain_id: terminal-commander-npm-distribution
phase: Wave 3 - CI
status: "Pending"
depends_on: ["NPM05"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "release-please manifest-releaser docs"
  - "release-please-action README"
  - "NPM02 contract (versioning section)"
risk_level: "medium"
---

# NPM06 - Release Please Manifest Config

## Branch Guard

```text
main
```

## Mission Context

NPM05 produces tarballs on every `main` push. NPM06 introduces `release-please` in manifest mode so Conventional Commits drive release PRs across the root wrapper + the platform packages, with one shared version line.

## Mini-Spec

objective:
- Add `release-please-config.json` + `.release-please-manifest.json` for the monorepo, register every npm package (root + platform packages) under one shared version, and wire a `release-please` job that produces release PRs on Conventional Commits and creates GitHub Releases when the release PR merges.

non_goals:
- Do not publish to npm (NPM07).
- Do not change Rust crate versions (the workspace stays at `0.0.0` per `RELEASE_CHECKLIST.md` until the first beta tag).
- Do not modify `crates/**`.

allowed_files_or_area:
- release-please-config.json
- .release-please-manifest.json
- .github/workflows/** (release-please job only)
- docs/release/**
- .agent/goals/terminal-commander-npm-distribution/NPM06-*.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- packages/* / package.json edits other than `version` (release-please bumps these)
- rules/**, config/**

contracts_or_interfaces:
- Manifest mode (`release-please-config.json` lists each package path; `.release-please-manifest.json` carries the version state).
- Initial version: `0.1.0-beta.1` (per `RELEASE_CHECKLIST.md`).
- Conventional Commits scope-to-package mapping: scope `wrapper`, `linux-x64`, `linux-arm64` for the npm packages (mirrors NPM02 names).
- Release-please-action pinned to a recorded SHA, not a floating major tag.
- No secrets referenced in this workflow (only `secrets.GITHUB_TOKEN`).

invariants:
- No long-lived npm tokens stored.
- No publish step in this workflow.

acceptance_criteria:
- Manifest config validates against the release-please schema.
- A dry-run of release-please (locally or via the action) recognizes the package set.
- `crates/**` untouched.

evidence_required:
- Branch evidence.
- File paths.
- Local validation output (release-please dry-run if available).
- Pinned action SHA recorded.

stop_conditions:
- Branch is not `main`.
- Multi-version-line model required (i.e., per-package independent versions) — escalate as a follow-up goal instead of expanding NPM06.

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
# Validate JSON shape:
python3 -c "import json; json.load(open('release-please-config.json')); json.load(open('.release-please-manifest.json'))"
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM06 only on branch `main`. Manifest config only — no publishing.

## Final Report Format

Objective / Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal (NPM07).
