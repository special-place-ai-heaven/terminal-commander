---
goal_id: WWS02
title: Root Npm Package Win32 Bridge Contract
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 1 - Package contract
status: "Pending"
depends_on: ["WWS01"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "WWS01 windows-wsl bridge contract"
  - "docs/release/npm-binary-packaging-contract.md (NPM02 §3 + §4)"
  - "packages/terminal-commander/package.json (current Linux-only os field)"
  - "packages/terminal-commander/lib/resolve-binary.js (current Linux-only resolver)"
risk_level: "high"
---

# WWS02 - Root Npm Package Win32 Bridge Contract

## Branch Guard

```text
main
```

## Mission Context

WWS01 locks the design. WWS02 amends the **root** npm package metadata so the package is installable on Windows as a bridge / setup surface, while keeping the two scoped platform packages Linux-only (NPM02 §3 unchanged). The platform-package `optionalDependencies` still target only `@terminal-commander/linux-x64` + `@terminal-commander/linux-arm64`; on Windows hosts npm correctly skips both because of `os` / `cpu` filtering, and the root bridge shim takes over without a Linux binary present.

This goal touches `packages/terminal-commander/package.json` (root only — `os` field, `bin` map, `files` whitelist) and the resolver (`packages/terminal-commander/lib/resolve-binary.js`) to recognize a `win32` branch that returns the bridge-shim resolution. The two platform packages are NOT edited. The release-please / npm-binary-build / publish workflows are NOT edited.

## Mini-Spec

objective:
- Update `packages/terminal-commander/package.json` to widen `os` to `["linux", "win32"]`, keep `version` at `0.1.0-beta.1` (or whatever release-please last bumped to), keep `optionalDependencies` exact-pinned to the same shared version. Update `lib/resolve-binary.js` to add a `win32` branch that returns a "bridge" platform package marker (no native binary path); the actual `wsl.exe` invocation lives in WWS04. Update the package README to explain the dual posture. No new MCP tools. No runtime change.

non_goals:
- Do not edit `packages/terminal-commander-linux-x64/package.json` or `-linux-arm64/package.json`.
- Do not edit `.github/workflows/**` or release-please config / manifest.
- Do not write the JS bridge shim runtime code (WWS04).
- Do not write the Cursor config writer (WWS05).
- Do not dispatch any workflow.
- Do not publish.

allowed_files_or_area:
- `packages/terminal-commander/package.json`
- `packages/terminal-commander/lib/resolve-binary.js`
- `packages/terminal-commander/test/resolve-binary.test.js` (add win32 + bridge cases)
- `packages/terminal-commander/README.md` (dual-posture explanation)
- `docs/release/**` (only to cross-link or refresh the contract reference)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS02-*.md`

forbidden_files:
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `scripts/**`
- `.github/**`
- `packages/terminal-commander-linux-x64/**`
- `packages/terminal-commander-linux-arm64/**`
- `packages/terminal-commander/bin/**` (the bridge shim files are WWS04)
- `.github/release-please-config.json` / `.release-please-manifest.json`
- secrets / tokens

contracts_or_interfaces:
- Root `package.json` `os` field becomes `["linux", "win32"]`. `cpu` field unchanged (root has none). `optionalDependencies` keys + version exact-pin unchanged.
- `lib/resolve-binary.js` returns one of:
  - `{ platformPackage, binaryPath, reason: 'ok' }` (Linux + supported arch + platform package installed)
  - `{ platformPackage: null, binaryPath: null, reason: 'bridge_required' }` (Windows host — actual bridge invocation belongs to WWS04 shim)
  - `{ platformPackage, binaryPath: null, reason: 'platform_package_missing' }` (Linux but `optionalDependencies` skipped)
  - `{ platformPackage: null, binaryPath: null, reason: 'unsupported_platform' }` (Mac, BSD, etc.)
- Resolver unit tests expanded from 12 to ~16+ cases (add `bridge_required` cases for win32 x64/arm64, leave unsupported_platform for darwin).
- README updated to describe the dual posture without claiming a Windows-native runtime.

invariants:
- No `crates/**` change.
- No runtime behavior change.
- No `.github/**` change.
- No version bump.
- Platform-package `os` / `cpu` / `bin` unchanged.
- MCP guard greps remain clean.

acceptance_criteria:
- `npm pack ./packages/terminal-commander --dry-run` still clean.
- `npm pack ./packages/terminal-commander-linux-x64 --dry-run` still clean.
- `npm pack ./packages/terminal-commander-linux-arm64 --dry-run` still clean.
- Root `package.json` `version` unchanged.
- Root `optionalDependencies` exact-pin preserved.
- Resolver unit tests pass with `npm test`.

evidence_required:
- Branch evidence.
- File paths.
- Resolver unit test results.
- Pack dry-run output.

stop_conditions:
- Branch is not `main`.
- The widening would require running real shim code on Windows at install time (postinstall is banned).

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
( cd packages/terminal-commander && npm test )
npm pack ./packages/terminal-commander --dry-run
npm pack ./packages/terminal-commander-linux-x64 --dry-run
npm pack ./packages/terminal-commander-linux-arm64 --dry-run
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run WWS02 only on branch `main`. Root package metadata + resolver only. No bridge shim runtime. No new MCP tool.

## Final Report Format

Objective / Changes / Files changed / Resolver test results / npm pack results / Verification / Evidence / Commit / Known gaps / Next goal (WWS03).
