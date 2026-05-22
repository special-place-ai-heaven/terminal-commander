---
goal_id: NPM03
title: Wrapper Package And Platform Package Layout
chain_id: terminal-commander-npm-distribution
phase: Wave 2 - Layout
status: "Pending"
depends_on: ["NPM02"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "NPM02 contract: docs/release/npm-packaging-contract.md"
  - "npm package.json docs (bin / optionalDependencies / files)"
risk_level: "medium"
---

# NPM03 - Wrapper Package And Platform Package Layout

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-npm-distribution/NPM03-wrapper-package-and-platform-package-layout.md

## Goal File Workflow

0. Branch Guard.
1. Mark `In progress`, set `started_at`.
2. Execute only the mini-spec.
3. On pass: commit verified work, mark Completed, set `completed_at` + `completion_commit`.
4. Goal status as separate commit.
5. On block: mark Blocked with `blocked_reason`.

## Branch Guard

```text
main
```

```bash
git branch --show-current
git status --short
```

## Mission Context

NPM02 locked the contract. NPM03 lays the package directories down: root wrapper + per-platform binary packages, with Node shims, `optionalDependencies`, and a resolver that picks the right platform binary at runtime. No GitHub Actions yet, no `release-please` yet.

## Mini-Spec

objective:
- Create the npm package directory layout under `packages/` (or the chosen root per NPM02) so a local `npm pack` of each package succeeds and the root wrapper's Node shims resolve a real binary from the matching platform package. No publishing.

non_goals:
- Do not write GitHub Actions.
- Do not write release-please config.
- Do not publish.
- Do not run a `cargo build` cross-compile yet (NPM05 handles the matrix).
- Do not modify `crates/**` or `Cargo.toml`.

allowed_files_or_area:
- packages/terminal-commander/** (root wrapper)
- packages/terminal-commander-linux-x64/** (platform package, scoped namespace name per NPM02)
- packages/terminal-commander-linux-arm64/** (platform package)
- packages/.gitignore / packages/README.md
- docs/release/**
- .agent/goals/terminal-commander-npm-distribution/NPM03-*.md
- .agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**
- config/**
- scripts/**
- .github/workflows/**
- release-please-config.json
- .release-please-manifest.json
- secrets / tokens / private paths anywhere

contracts_or_interfaces:
- Root wrapper package matches NPM02 name + version.
- Root `bin` field exposes the three commands (`terminal-commanderd`, `terminal-commander-mcp`, `terminal-commander`) as Node shims.
- Each shim resolves `${platform_package}/bin/${command}` via the matching `optionalDependencies` entry, picking by `process.platform` + `process.arch`.
- If no matching platform package is installed, the shim exits non-zero with a clear error citing the supported targets — never prints a stack trace.
- Platform packages contain ONLY a `bin/` directory with the three binaries (placeholders allowed at NPM03; real binaries come from NPM05). Each platform `package.json` declares `"os"` and `"cpu"` so npm refuses to install on the wrong host.
- `files` field set so only `bin/` ships.
- License + repository fields populated.
- `engines` field declares the Node minimum used to run the shims.

invariants:
- The runtime chain invariants carry over.
- No `postinstall` script.
- No download from GitHub Releases at install time.

scope_substitution_policy:
- If NPM01 recommended a flat (non-scoped) platform package name, use that and record the substitution here.

acceptance_criteria:
- `npm pack` succeeds in `packages/terminal-commander/`, `packages/terminal-commander-linux-x64/`, and `packages/terminal-commander-linux-arm64/`.
- `npm pack --dry-run` for the root shows the three `bin` shims in the tarball file list.
- Root wrapper's Node shim can be unit-tested for the resolver fallback (no platform package installed → clear non-zero exit).
- `crates/**`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/**` untouched.

evidence_required:
- Branch evidence.
- File paths created.
- `npm pack` output for each package.
- Resolver fallback evidence (test or scripted run).

stop_conditions:
- Branch is not `main`.
- Layout would require runtime code changes.
- A platform package is needed for an unsupported target.

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
( cd packages/terminal-commander && npm pack --dry-run )
( cd packages/terminal-commander-linux-x64 && npm pack --dry-run )
( cd packages/terminal-commander-linux-arm64 && npm pack --dry-run )
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM03 only on branch `main`. Create the package layout per the NPM02 contract.

## Final Report Format

Objective / Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal (NPM04).
