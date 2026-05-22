---
goal_id: NPM04
title: Local Pack And Global Install Smoke
chain_id: terminal-commander-npm-distribution
phase: Wave 2 - Layout
status: "Pending"
depends_on: ["NPM03"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "NPM03 layout"
  - "scripts/smoke/verify-runtime-smoke.sh (TC46 regression)"
risk_level: "medium"
---

# NPM04 - Local Pack And Global Install Smoke

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-npm-distribution/NPM04-local-pack-and-global-install-smoke.md

## Goal File Workflow

0. Branch Guard.
1. Mark `In progress`.
2. Execute only the mini-spec.
3. On pass: commit + status commit.

## Branch Guard

```text
main
```

## Mission Context

NPM03 stood up the layout. NPM04 proves a clean `npm install -g <local-tarball>` on a Linux host produces working binaries on PATH and a working MCP stdio handshake. Local binaries (from `cargo build`) feed the platform package's `bin/` directory; no CI involvement yet.

## Mini-Spec

objective:
- Build the three Rust binaries for the host platform, drop them into the matching platform package, run `npm pack` for the root + platform packages, install the resulting tarballs globally into a sandboxed npm prefix, and verify the three commands work end-to-end including the TC46 local smoke run against the npm-installed binaries.

non_goals:
- Do not publish to the public npm registry.
- Do not modify `crates/**` source.
- Do not add macOS or Windows targets.
- Do not write GitHub Actions.

allowed_files_or_area:
- scripts/smoke/** (new local-install smoke script)
- packages/**
- docs/release/**
- .agent/goals/terminal-commander-npm-distribution/NPM04-*.md
- .agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**
- config/**
- .github/workflows/**

contracts_or_interfaces:
- The smoke script installs into a sandboxed `--prefix` (temp dir) — never the user's global npm prefix.
- After install, `${prefix}/bin/terminal-commanderd`, `${prefix}/bin/terminal-commander-mcp`, `${prefix}/bin/terminal-commander` exist and are executable.
- The script then runs the TC46 `verify-runtime-smoke.sh` flow but using the npm-installed binaries (via `PATH=${prefix}/bin:$PATH`).
- Script tears down the sandbox on exit, even on failure.
- No secrets / tokens written.

invariants:
- Runtime / MCP / audit invariants unchanged.
- Smoke script must not require root.

acceptance_criteria:
- `scripts/smoke/verify-npm-local-install.sh` exits 0 on a clean Linux x64 host.
- The three commands run from the sandboxed prefix.
- The bundled TC46 smoke (or its npm-installed equivalent) passes 8/8 assertions.
- `crates/**` and `.github/workflows/**` untouched.

evidence_required:
- Branch evidence.
- Smoke script output.
- File paths changed.

stop_conditions:
- Branch is not `main`.
- The host architecture is not one of the NPM02-locked targets.
- The smoke requires running as root or modifying the user's global npm prefix.

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
bash scripts/smoke/verify-npm-local-install.sh
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM04 only on branch `main`. Prove local install works before any CI work begins.

## Final Report Format

Objective / Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal (NPM05).
