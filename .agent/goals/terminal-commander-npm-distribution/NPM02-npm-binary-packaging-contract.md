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

## Final Report

Objective:
- Lock the npm packaging contract NPM03-NPM07 implement.

Changes (verified work commit `81f4ea1`):
- `docs/release/npm-binary-packaging-contract.md` (new, 14 sections, ~428 lines). Locks package names + name-availability evidence; user-facing install contract; platform contract (Linux-only initial publish); package architecture (platform packages via optionalDependencies, no postinstall); binary layout; versioning (single shared semver, 0.1.0-beta.1, no crates.io); release contract (release-please manifest + OIDC trusted publishing + provenance + review-gated PR); Cursor contract; safety contract; per-goal recommendations for NPM03-NPM07; risks (R-NPM-04..R-NPM-09); alternatives considered + rejected.

No product-code changes. No `package.json`, `packages/`, `.github/workflows/`, `release-please-config.json`, `.release-please-manifest.json`, or `scripts/**` added.

Files changed:
- `docs/release/npm-binary-packaging-contract.md` (new)
- `.agent/goals/terminal-commander-npm-distribution/NPM02-*.md` (this file)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` — clean after work + status commits
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings`
- PASS: `cargo test --workspace` — every suite green
- PASS: `cargo nextest run --workspace` — **347/347, 0 skipped**
- PASS: `bash scripts/smoke/verify-runtime-smoke.sh` — TC46 regression SUCCESS (8/8 PASS)
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — doc / negative-assertion matches only
- PASS: `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` — no matches
- PASS: `git diff HEAD -- crates/ Cargo.toml Cargo.lock rules/ config/ scripts/ .github/ packages/ package.json` — empty
- PASS: npm name-availability check on 2026-05-23 from WSL2 (`npm 10.9.7`) for `terminal-commander`, `@terminal-commander/linux-x64`, `@terminal-commander/linux-arm64`, and the fallback scope `@special-place-administrator/terminal-commander` — all four return E404 (available).

Evidence — explicit acceptance against the NPM02 mini-spec:

- **Package names locked + availability check recorded.** Four names probed via `npm view`; all E404. Operator precondition: claim the `@terminal-commander` org before NPM07. Fallback scope recorded.
- **User-facing install contract.** `npm install -g terminal-commander` yields three commands on PATH (`terminal-commanderd`, `terminal-commander-mcp`, `terminal-commander`). npm `bin` semantics cited.
- **Platform contract.** linux-x64 + linux-arm64 only at initial publish. macOS / Windows-native / musl / Alpine: explicitly rejected. WSL2 fallback for Windows operators via Cursor invoking `wsl ... terminal-commander-mcp`.
- **Package architecture.** Root wrapper + platform packages via `optionalDependencies`; JS bin shims with `process.platform`/`process.arch` resolver; no postinstall; no Rust compile at install time; no all-platforms-in-one; no long-lived `NPM_TOKEN`. `engines.npm: ">=8"` on root. Shim platform-mismatch behavior locked (exit code 64, one-line stderr).
- **Binary layout.** Three package directories under `packages/` with bounded `files` whitelists. Platform packages stub `bin/` at NPM03; NPM05 GitHub Actions populates the real binaries.
- **Versioning contract.** Single shared semver across root + both platform packages. Initial `0.1.0-beta.1`. Cargo workspace stays at `0.0.0`. No crates.io publish in this chain. Root wrapper pins platform-package versions exactly (no `^`/`~` ranges).
- **Release contract.** release-please manifest mode; review-gated PR; publishing is a separate workflow job triggered only when `release_created == true`. npm trusted publishing via GitHub Actions OIDC; `permissions: id-token: write`; `npm publish --provenance`; publish order = platform packages first, root last; first publish `--tag beta`. No long-lived `NPM_TOKEN` unless NPM07 explicitly approves the fallback with reason.
- **Cursor contract.** Both config blocks (native + WSL bridge) locked for NPM08. Smoke is operator-driven; no Cursor success claimed at NPM02.
- **Safety contract.** MCP guard greps stay green (verified at NPM02). Shim behavior locked to resolve + `child_process.spawn` only. No file I/O, no sockets, no shell interpretation, no env-var echo. No secrets / tokens / private paths in committed artifacts.

Beta-state mapping (per the user's "evidence first, then map to actual beta state" rule):
- TC48 `Conditional Go` preserved: workspace Cargo stays at `0.0.0`; no `Cargo.toml` in release-please `extra-files`.
- Provider live smoke pending: §8 keeps the Cursor smoke claim out of NPM02; the contract only specifies the config; NPM08 owns the live transcript.
- Linux/WSL2 = the real platform story: §3 rejects macOS / Windows-native; WSL2 fallback documented as the Windows path.
- TC46 + TC47 regressions stay mandatory: §10 routes them into NPM05 as pre-build gates.

Source-status:
- `docs/release/npm-binary-packaging-contract.md`: **live (NPM02)**.
- NPM01 audit: **referenced, unchanged**.
- Terminal Commander runtime + MCP surface: **unchanged**.
- Every `crates/` source file: **unchanged**.

Commits:
- Verified work commit: `81f4ea1`
- Goal status commit: `64a0b59`
- Final report patch (this commit): goal-file body update

Known gaps / blockers:
- None at NPM02. Operator preconditions (org name registration on npmjs.com, trusted-publisher configuration) remain recorded as R-NPM-04 + R-NPM-07.

Next goal:
- NPM03-wrapper-package-and-platform-package-layout.md
