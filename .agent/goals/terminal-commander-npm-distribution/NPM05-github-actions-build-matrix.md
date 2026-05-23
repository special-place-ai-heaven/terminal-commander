---
goal_id: NPM05
title: Github Actions Build Matrix
chain_id: terminal-commander-npm-distribution
phase: Wave 3 - CI
status: "Completed"
depends_on: ["NPM04"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: "2026-05-23T14:00:00+00:00"
completed_at: "2026-05-23T14:50:00+00:00"
completion_commit: "2bbf2fd"
blocked_reason: ""
source_refs:
  - "NPM04 local smoke"
  - "GitHub Actions runners doc (ubuntu-latest, ubuntu-24.04-arm)"
  - "actions-rs / cargo, rust-cache action options"
risk_level: "medium"
---

# NPM05 - Github Actions Build Matrix

## Branch Guard

```text
main
```

## Mission Context

NPM04 proved a local Linux x64 install works. NPM05 stands up the CI build matrix that compiles the three binaries for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, drops them into the matching platform package, runs `npm pack`, and uploads the tarballs as workflow artifacts. No publishing yet (NPM07 owns publishing).

## Mini-Spec

objective:
- Add a GitHub Actions workflow that, on pushes to `main` and on tag candidates, builds the three Rust binaries for the locked target triples, populates the platform package `bin/` directories, runs `npm pack`, uploads the tarballs + checksums as workflow artifacts, and runs the TC46 + TC47 regression gates before any publish.

non_goals:
- Do not add `release-please` config (NPM06).
- Do not configure trusted publishing (NPM07).
- Do not push tarballs to the npm registry.
- Do not change `crates/**` source.

allowed_files_or_area:
- .github/workflows/**
- packages/** (only the platform `bin/` placeholders if NPM03 left stubs)
- scripts/smoke/** (re-used as the CI smoke gate)
- docs/release/**
- .agent/goals/terminal-commander-npm-distribution/NPM05-*.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**
- config/**
- release-please-config.json
- .release-please-manifest.json
- any secret in committed YAML

contracts_or_interfaces:
- Matrix targets locked at NPM02 (initial: linux-x64, linux-arm64).
- Cargo caching via the standard `Swatinem/rust-cache` (or equivalent — recorded in the goal final report).
- Workflow uses `actions/checkout@v4`, `actions/setup-node@v4`, and a documented Rust toolchain pin (`1.95.0`).
- Workflow runs the TC46 smoke (`scripts/smoke/verify-runtime-smoke.sh`) and the TC47 load regression (`cargo test -p terminal-commanderd --test load_noise_backpressure`) before the pack step.
- Tarballs uploaded to artifacts with deterministic names: `terminal-commander-${version}.tgz`, `terminal-commander-linux-x64-${version}.tgz`, `terminal-commander-linux-arm64-${version}.tgz`.
- Checksums emitted via `sha256sum` per tarball, also uploaded.
- No `secrets.*` referenced for publish credentials in this workflow (NPM07 owns that).

invariants:
- No long-lived credentials in YAML.
- No raw stream / no network listener / no shell-execution surface added.

acceptance_criteria:
- Workflow runs green on `main` for the matrix targets.
- Artifacts include three tarballs + their checksums.
- TC46 + TC47 regression gates run inside CI.
- `crates/**` untouched.

evidence_required:
- Branch evidence.
- Workflow file path.
- Successful workflow run id + artifact list (operator records this on first execution).
- Toolchain pin + cache key strategy documented.

stop_conditions:
- Branch is not `main`.
- A matrix target requires changing `crates/**` build flags beyond a tiny documented compatibility fix.
- The workflow would require uploading secrets.

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
# Local YAML lint:
yamllint .github/workflows || true
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM05 only on branch `main`. Build matrix only — no publishing.

## Final Report

Objective:
- Add a GitHub Actions workflow that builds the three Rust binaries for linux-x64 and linux-arm64, stages them into the matching platform package, runs `npm pack` for the root + platform packages, runs the TC46/TC47 regressions on the x64 leg, and uploads the resulting tarballs as workflow artifacts. No publishing.

Changes (verified work commit `2bbf2fd`):
- `.github/workflows/npm-binary-build.yml` (new, 286 lines, 188 YAML keys, 1 job, 20 steps). Two matrix legs (linux-x64 full smoke, linux-arm64 narrow). `permissions: contents: read`. No `id-token: write`. No `secrets.*` for publish. `actions/upload-artifact@v4` for tarballs.

No product-code changes. No release-please config. No publish workflow. No runtime changes. No `crates/**` edits. No `Cargo.toml` / `Cargo.lock` edits.

Files changed:
- `.github/workflows/npm-binary-build.yml` (new)
- `.agent/goals/terminal-commander-npm-distribution/NPM05-*.md` (this file)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, `npm 10.9.7`, `node 22.22.2`):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` — clean after work + status commits
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings`
- PASS: `cargo test --workspace` — every suite green
- PASS: `cargo nextest run --workspace` — **347/347, 0 skipped**
- PASS: `bash scripts/smoke/verify-runtime-smoke.sh` — TC46 regression SUCCESS (8/8 PASS)
- PASS: `bash scripts/smoke/verify-npm-local-install.sh` — NPM04 regression SUCCESS (12 PASS lines)
- PASS: `npm pack ./packages/terminal-commander --dry-run` — 7 files
- PASS: `npm pack ./packages/terminal-commander-linux-x64 --dry-run` — 5 files
- PASS: `npm pack ./packages/terminal-commander-linux-arm64 --dry-run` — 5 files
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — doc / negative-assertion matches only
- PASS: `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` — no matches
- PASS: `git diff HEAD -- crates/ Cargo.toml Cargo.lock rules/ config/ release-please-config.json .release-please-manifest.json` — empty (0 lines)
- PASS: `python3 -c "import yaml; yaml.safe_load(...)"` — workflow YAML parses; 1 job; 20 steps

Workflow design (key boundary properties):
- Two matrix legs with `fail-fast: false`. arm64 outage does not mask x64 success.
- linux-x64 leg = `ubuntu-24.04`, target `x86_64-unknown-linux-gnu`, FULL smoke (fmt + clippy + nextest + TC47 load + MCP grep guards + daemon self-check + full NPM04 install smoke).
- linux-arm64 leg = `ubuntu-24.04-arm`, target `aarch64-unknown-linux-gnu`, NARROW smoke (build binaries + `--help` + `npm pack` only).
- Pre-build gate (x64) runs the workspace fmt / clippy / nextest / TC47 load regression + both MCP grep guards.
- Cargo cache via `Swatinem/rust-cache@v2` with leg-specific `shared-key`.
- Rust toolchain pinned to `1.95.0` via `dtolnay/rust-toolchain@master`.
- Node 20 LTS via `actions/setup-node@v4`.
- `taiki-e/install-action@nextest` installs cargo-nextest before the workspace test step.
- `actions/upload-artifact@v4` uploads tarballs as `npm-tarballs-${matrix.name}`, `retention-days: 14`, `if-no-files-found: error`.
- `concurrency: cancel-in-progress: false` so an in-flight build on `main` is not lost.
- `permissions: contents: read` at workflow level; no `id-token`, no `actions: write`, no `contents: write`. (NPM07 adds the OIDC claim.)
- No `secrets.NPM_TOKEN` referenced. No `npm publish`. No `release-please-action`.
- The workflow does NOT commit real binaries back to the repository. Real binaries land in `target/<triple>/release/` on the runner and are copied into `packages/<plat>/bin/` on the runner only — never `git add`'d.

Honest cross-arch posture:
- `ubuntu-24.04-arm` runner availability depends on the repository's runner allowance. If unavailable, the arm64 leg fails with "No runner matching the specified labels was found". The workflow's header comment names this as the recorded blocker and the operator's two recovery paths: (a) acquire arm64 runner allowance, or (b) replace the leg with QEMU `cross` in a follow-up amendment. No fake arm64 execution from x64.
- arm64 leg runs binary-level checks ONLY (`cargo build` + `--help` + `npm pack`); full nextest / TC47 / TC46 regressions are intentionally skipped on arm64 because the runtime gate is enforced by the x64 leg.

Evidence — explicit acceptance against the NPM05 mini-spec:
- **GitHub Actions workflow exists for npm binary build matrix.** Yes — `.github/workflows/npm-binary-build.yml`.
- **x64 and arm64 jobs are represented honestly.** Two matrix legs with distinct runner labels, targets, and gate scopes. arm64 caveats documented in the workflow header.
- **linux-x64 package build path is validated locally through WSL.** NPM04 regression (re-run during this goal) confirms the full install + MCP stdio path on linux-x64.
- **linux-arm64 package execution is only claimed if actually run on arm64.** Workflow header explicitly states the leg's narrow scope; smoke script's final lines state `arm64 cross-arch execution NOT covered; only linux-x64 was actually run.` Live arm64 evidence requires the GitHub Actions run.
- **Workflow does not publish.** No `npm publish` step anywhere.
- **Workflow does not use NPM_TOKEN.** No `secrets.NPM_TOKEN` referenced.
- **Workflow does not add release-please.** No `googleapis/release-please-action` step.
- **Workflow artifacts are package tarballs or staged package outputs only.** `tarballs/*.tgz` uploaded; no source / debug-symbol artifacts.
- **Existing NPM04 local install smoke still passes.** Confirmed (re-run during this goal).
- **Existing TC46 runtime smoke still passes.** Confirmed.
- **Existing TC47 load gate remains green.** Confirmed (8/8 stress tests, embedded in nextest 347/347).
- **No runtime / product code changed.** `git diff HEAD -- crates/ Cargo.toml Cargo.lock rules/ config/ release-please-config.json .release-please-manifest.json` returns empty.

Beta-state mapping:
- TC48 `Conditional Go` preserved. NPM05 adds CI build infrastructure; it does not change the runtime, the MCP surface, or the published artifact contents.
- Provider live smoke pending: still NPM08 scope. Cursor smoke is operator-driven.
- Linux/WSL2 = the real platform story: matrix targets only Linux (x64 + arm64). arm64 is honestly conditional on the GitHub-hosted runner allowance.
- TC46 + TC47 regressions stay mandatory gates: the x64 matrix leg runs both inline.

Source-status:
- `.github/workflows/npm-binary-build.yml`: **live (NPM05)** — YAML parses, gates inlined are all green locally. **Live CI run NOT YET EXECUTED** because it requires a push to `main`; the workflow will trigger on the next push and the operator records the run id + arm64 leg outcome in a follow-up artifact (or this goal's status commit if they choose to push together).
- `scripts/smoke/verify-npm-local-install.sh`: **unchanged** — used as-is from NPM04.
- Terminal Commander runtime + MCP surface: **unchanged**.
- Every `crates/` source file: **unchanged**.

Commits:
- Verified work commit: `2bbf2fd`
- Goal status commit: this commit

Known gaps / blockers:
- Live GitHub Actions validation is pending until the next push to `main`. The operator records the run id + arm64 leg outcome (PASS / BLOCKED) after the push lands. This goal does NOT claim CI passed; it only claims the workflow YAML parses, the gates inlined match the local NPM04 evidence, and the workflow obeys the locked boundaries.
- `ubuntu-24.04-arm` GitHub-hosted runner availability depends on the repository's allowance. If unavailable on first run, the arm64 leg's blocker is recorded honestly per the workflow header.
- Operator preconditions from NPM02 (npmjs.com `@terminal-commander` org claim + trusted-publisher config) remain pending; both gate NPM07, not NPM06.

Next goal:
- NPM06-release-please-manifest-config.md
