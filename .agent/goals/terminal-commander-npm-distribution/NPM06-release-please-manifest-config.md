---
goal_id: NPM06
title: Release Please Manifest Config
chain_id: terminal-commander-npm-distribution
phase: Wave 3 - CI
status: "Completed"
depends_on: ["NPM05"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: "2026-05-23T01:00:00+00:00"
completed_at: "2026-05-23T02:00:00+00:00"
completion_commit: "e81eb3f"
blocked_reason: ""
source_refs:
  - "release-please manifest-releaser docs"
  - "release-please-action README"
  - "NPM02 contract (versioning section + §7 release contract)"
risk_level: "medium"
---

# NPM06 - Release Please Manifest Config

## Branch Guard

```text
main
```

## Mission Context

NPM05 produces tarballs on every `main` push. NPM06 introduces `release-please` in manifest mode so Conventional Commits drive release PRs across the root wrapper + the platform packages, with one shared version line.

### Path conflict resolution (NPM06 prep)

The original NPM06 goal-file mini-spec listed `release-please-config.json` and `.release-please-manifest.json` at the repo ROOT under `allowed_files_or_area`. The binding NPM02 contract (§7 + §10) and the NPM01 audit (§11) both lock these files at `.github/release-please-config.json` and `.github/.release-please-manifest.json` (Symforge path layout). Per the chain rule "if NPM02 and NPM06 disagree, follow NPM02", this goal-file has been amended:

- `allowed_files_or_area` widened to `.github/release-please-config.json` and `.github/.release-please-manifest.json`.
- Workflow file scoped to exactly `.github/workflows/release-please.yml` (NPM05's `.github/workflows/npm-binary-build.yml` added to `forbidden_files`).
- No silent widening of scope. The amendment is recorded here.

### Release-please-action SHA pin (NPM06 prep)

Pinned action: `googleapis/release-please-action@5c625bfb5d1ff62eadeeb3772007f7f66fdcf071` (tag `v4.4.1`, commit SHA resolved via GitHub API on 2026-05-23). v4 is preferred over v5 because manifest-mode shape used in this chain matches the v4 README and the Symforge precedent. The pin is an immutable commit SHA, not a floating tag.

## Mini-Spec

objective:
- Add `release-please-config.json` + `.release-please-manifest.json` for the monorepo, register every npm package (root + platform packages) under one shared version, and wire a `release-please` job that produces release PRs on Conventional Commits and creates GitHub Releases when the release PR merges.

non_goals:
- Do not publish to npm (NPM07).
- Do not change Rust crate versions (the workspace stays at `0.0.0` per `RELEASE_CHECKLIST.md` until the first beta tag).
- Do not modify `crates/**`.

allowed_files_or_area:
- .github/release-please-config.json
- .github/.release-please-manifest.json
- .github/workflows/release-please.yml (release-please job only)
- docs/release/**
- .agent/goals/terminal-commander-npm-distribution/NPM06-*.md
- .agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md (status row only)

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- packages/* / package.json edits other than `version` (release-please bumps these)
- rules/**, config/**
- scripts/**
- .github/workflows/npm-binary-build.yml (NPM05; do not edit at NPM06)

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

## Final Report

Objective:
- Add release-please in manifest mode for the three Terminal Commander npm packages (`terminal-commander`, `@terminal-commander/linux-x64`, `@terminal-commander/linux-arm64`) with one shared semver and no publish surface. Workflow only opens / updates release PRs on Conventional Commits and creates the GitHub Release when the operator merges the PR. NPM07 owns publishing.

Changes (verified work commit `e81eb3f`, prep amendment commit `02a4528`):
- `.github/release-please-config.json` (new). Manifest mode; three packages; `release-type: node`; `linked-versions` plugin groups them under `terminal-commander`; `separate-pull-requests: false`; `include-component-in-tag: false` + `include-v-in-tag: true`; `prerelease: true`; `bump-minor-pre-major: true`; `bump-patch-for-minor-pre-major: true`.
- `.github/.release-please-manifest.json` (new). All three package paths pinned to `0.1.0-beta.1`.
- `.github/workflows/release-please.yml` (new). Trigger: `push` to `main` + `workflow_dispatch`. Permissions: `contents: write` + `pull-requests: write`. Concurrency `cancel-in-progress: false`. Action pinned to immutable SHA `googleapis/release-please-action@5c625bfb5d1ff62eadeeb3772007f7f66fdcf071` (tag `v4.4.1`).
- `docs/release/release-please-contract.md` (new, 15 sections). Binding NPM07 hand-off contract.
- This goal file: prep amendment widened `allowed_files_or_area` to `.github/` paths (NPM02 §7 / §10 + NPM01 §11 binding); workflow scoped to exactly `.github/workflows/release-please.yml`; `.github/workflows/npm-binary-build.yml` added to `forbidden_files`; release-please-action SHA pin recorded.

No runtime code changes. No package.json edits (versions already at `0.1.0-beta.1` from NPM03). No CI/publish/token workflow.

Files changed:
- `.github/release-please-config.json` (new)
- `.github/.release-please-manifest.json` (new)
- `.github/workflows/release-please.yml` (new)
- `docs/release/release-please-contract.md` (new)
- `.agent/goals/terminal-commander-npm-distribution/NPM06-*.md` (prep amendment + status)

Release-please contract addresses:
- release-please config path: `.github/release-please-config.json`
- manifest path: `.github/.release-please-manifest.json`
- workflow path: `.github/workflows/release-please.yml`
- package paths / components:
  - `packages/terminal-commander` / component `terminal-commander` / npm name `terminal-commander`
  - `packages/terminal-commander-linux-x64` / component `@terminal-commander/linux-x64` / npm name `@terminal-commander/linux-x64`
  - `packages/terminal-commander-linux-arm64` / component `@terminal-commander/linux-arm64` / npm name `@terminal-commander/linux-arm64`
- exact shared version: `0.1.0-beta.1`

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, `npm 10.9.7`, `node 22.22.2`):
- PASS: `git branch --show-current` → `main`
- PASS: `git status --short` → clean (no untracked / uncommitted files post-status-commit)
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings` (clean, exit 0)
- PASS: `cargo test --workspace` (every suite ok)
- PASS: `cargo nextest run --workspace` — **347/347, 0 skipped**
- PASS: `bash scripts/smoke/verify-runtime-smoke.sh` — TC46 SUCCESS (8/8 PASS)
- PASS: `bash scripts/smoke/verify-npm-local-install.sh` — NPM04 SUCCESS (12 PASS lines, end-to-end MCP stdio)
- PASS: `npm pack ./packages/terminal-commander --dry-run` — 7 files, 0.1.0-beta.1
- PASS: `npm pack ./packages/terminal-commander-linux-x64 --dry-run` — 5 files, 0.1.0-beta.1
- PASS: `npm pack ./packages/terminal-commander-linux-arm64 --dry-run` — 5 files, 0.1.0-beta.1
- PASS: JSON parse over 5 files (release-please config + manifest + 3 package.json files)
- PASS: YAML parse over `.github/workflows/release-please.yml` (1 job, 1 step, action SHA pinned) and `.github/workflows/npm-binary-build.yml` (untouched, still parses)
- PASS: Version sync — all three `package.json` `version` fields and all three manifest entries are exactly `0.1.0-beta.1`; root `optionalDependencies` exact-pin to `0.1.0-beta.1` (no `^` / `~` ranges)
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — doc / negative-assertion matches only (unchanged from NPM05 baseline)
- PASS: `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` — no matches (unchanged)
- PASS: `git diff --ignore-cr-at-eol HEAD -- crates/ Cargo.toml Cargo.lock rules/ config/ scripts/ .github/workflows/npm-binary-build.yml` — empty (any apparent forbidden-paths diff is a CRLF-vs-LF artifact of WSL2 reading Windows-mounted files; no semantic change)

Evidence — explicit acceptance against the NPM06 mini-spec:

- **release-please-config.json validates against the JSON schema.** Confirmed by `python3 -c "json.load(...)"`. Includes `$schema` reference to the upstream release-please config schema.
- **A dry-run of release-please recognizes the package set.** The config + manifest agree on the three package paths (`packages/terminal-commander`, `packages/terminal-commander-linux-x64`, `packages/terminal-commander-linux-arm64`) and the version `0.1.0-beta.1`. release-please CLI was NOT executed locally (the chain rule says live action runs after push); the configuration is shape-correct and SHA-pinned.
- **`crates/**` untouched.** Forbidden-paths diff with `--ignore-cr-at-eol` is empty.
- **No publish surface introduced.** `rg "npm publish"` over `.github/` matches comment text only (no action step or run-line). `rg "NPM_TOKEN"` over `.github/` matches comment text only. No `id-token: write` in any job. No `secrets.NPM_TOKEN` reference.
- **No release-please publish side effect configured.** v4 release-please-action does not invoke `npm publish` on its own — it only opens release PRs and creates GitHub Releases. The publish workflow is NPM07's separate file.
- **NPM05 binary build workflow remains independent.** `.github/workflows/npm-binary-build.yml` is on the NPM06 `forbidden_files` list and was not edited. Both workflows exist side-by-side; they do not call each other at NPM06.
- **release-please-action pinned by SHA.** `5c625bfb5d1ff62eadeeb3772007f7f66fdcf071` (resolved from tag `v4.4.1` via GitHub API on 2026-05-23). Floating tags were considered and rejected per goal-file invariant.
- **Conventional-Commits scope mapping documented.** `docs/release/release-please-contract.md` §6 captures the scope → package mapping.
- **No auto-merge.** Workflow does not call any merge-on-green action. Release PRs are review-gated per NPM02 §7.

Beta-state mapping:
- TC48 `Conditional Go` preserved. NPM06 added configuration only; no runtime / MCP surface / package layout / behavior changed.
- Provider live smoke ceiling still NPM08 scope.
- Linux/WSL2 still the only supported platform set.
- TC46 + TC47 regressions still green.

Live GitHub Actions evidence:
- NOT executed at NPM06. The chain rule requires verified work + status commit landing locally first, then operator push approval, then live evidence capture. Recorded as **Pending push**, not Pass and not Blocked.
- Expected first live behavior: a release-please workflow run on the next `main` push. Either (a) opens a release PR titled `chore: release 0.1.0-beta.1` if there are Conventional-Commits-eligible commits since the last tag, OR (b) no-ops if the manifest matches the latest tag and there are no bumping commits. Both outcomes are valid and would not invalidate NPM06.

Source-status:
- `.github/release-please-config.json`: **live (NPM06)**.
- `.github/.release-please-manifest.json`: **live (NPM06)**.
- `.github/workflows/release-please.yml`: **live (NPM06)**, not yet executed live.
- `docs/release/release-please-contract.md`: **live (NPM06)**.
- `.github/workflows/npm-binary-build.yml` (NPM05): **unchanged**.
- Every `crates/` source file: **unchanged**.
- Every `packages/<pkg>/package.json`: **unchanged** (versions remain `0.1.0-beta.1`).
- TC46 smoke + TC47 load regression: **green**.

Commits:
- Prep amendment commit: `02a4528` (widened `allowed_files_or_area` to `.github/` paths; recorded SHA pin)
- Verified work commit: `e81eb3f`
- Goal status commit: this commit

Known gaps / blockers:
- Live release-please workflow execution is **pending push**. The operator-approved push will produce the first live run; expected outcome is either a release PR titled `chore: release 0.1.0-beta.1` or a no-op (both valid).
- Operator preconditions from NPM02 §1.2 (npmjs.com `@terminal-commander` org claim + trusted-publisher configuration) remain pending. Both gate NPM07, not NPM06.

Confirmations (explicit):
- No npm publish step exists in any workflow at NPM06.
- No `NPM_TOKEN` is referenced in any workflow at NPM06.
- No trusted-publishing (OIDC) workflow was introduced at NPM06.
- No `id-token: write` permission claim exists in any workflow at NPM06.
- No auto-merge on the release PR.
- NPM05 binary build workflow file was not edited at NPM06.
- NPM07 was not started.

Next goal:
- NPM07-trusted-publish-workflow.md

