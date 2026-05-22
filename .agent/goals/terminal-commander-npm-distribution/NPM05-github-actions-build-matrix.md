---
goal_id: NPM05
title: Github Actions Build Matrix
chain_id: terminal-commander-npm-distribution
phase: Wave 3 - CI
status: "Pending"
depends_on: ["NPM04"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
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

## Final Report Format

Objective / Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal (NPM06).
