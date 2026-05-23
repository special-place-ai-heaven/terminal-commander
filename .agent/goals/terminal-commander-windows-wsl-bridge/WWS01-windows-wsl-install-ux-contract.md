---
goal_id: WWS01
title: Windows Wsl Install Ux Contract
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 0 - Contract
status: "Completed"
depends_on: []
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: "2026-05-23T13:00:00+00:00"
completed_at: "2026-05-23T14:00:00+00:00"
completion_commit: "6220eb2"
blocked_reason: ""
source_refs:
  - "docs/release/npm-binary-packaging-contract.md (NPM02 Linux-only assumption that WWS01 must amend)"
  - "docs/integrations/cursor.md (NPM08 existing config examples)"
  - "examples/provider-harness/cursor/ (NPM08 three JSON examples)"
  - "TC44 non_goals (Windows ConPTY deferred)"
  - "User directive 2026-05-23: Windows package is bridge/setup only; WSL package is the real runtime"
risk_level: "high"
---

# WWS01 - Windows Wsl Install Ux Contract

## Branch Guard

```text
main
```

## Mission Context

NPM02 locked the npm package contract to Linux-only (`os: ["linux"]` on root + scoped platform packages for `linux-x64` / `linux-arm64`). NPM08 shipped Cursor MCP docs + three copy-pasteable JSON configs but kept setup manual. The user-locked target UX is `npm install -g terminal-commander` + `terminal-commander setup cursor-wsl` on Windows producing a working Cursor MCP entrypoint with zero manual JSON editing.

WWS01 is the contract goal that audits the gap between the existing NPM02 contract and the target UX, locks the binding decisions (root package widened to include `win32`, platform packages stay Linux-only, Windows shim invokes WSL, Cursor config writer is JS-side, pairing optional), and writes the binding `docs/release/windows-wsl-bridge-contract.md`. No implementation. No `package.json` edit. No workflow change.

## Mini-Spec

objective:
- Produce `docs/release/windows-wsl-bridge-contract.md` (or equivalent under `docs/`) that locks: (a) which hosts the root npm package supports (`linux` + `win32`), (b) what binaries / shims each host receives, (c) how the Windows shim resolves the WSL invocation, (d) what `terminal-commander setup cursor-wsl` MUST and MUST NOT do, (e) the safety boundary (no Windows-native daemon claim, no raw shell exposure, no network listener, no postinstall downloader), (f) the open decisions registry from `GOAL_CHAIN_INDEX.md` resolved to binding answers.

non_goals:
- Do not modify `crates/**`, `Cargo.toml`, or `Cargo.lock`.
- Do not modify any `packages/*/package.json` at WWS01.
- Do not modify `.github/workflows/**`.
- Do not modify `.github/release-please-config.json` / `.release-please-manifest.json`.
- Do not write JS shim code (WWS02 / WWS04).
- Do not write the Cursor config writer (WWS05).
- Do not dispatch any workflow.
- Do not publish anything.

allowed_files_or_area:
- `docs/release/**`
- `docs/install/**` (only for cross-linking)
- `docs/integrations/**` (only for cross-linking; WWS05 owns the writer)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS01-*.md`
- `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md` (status / open-decisions row updates only)
- `.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md` (status row only if needed)

forbidden_files:
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `scripts/**`
- `.github/**`
- `packages/*/package.json` (all `version` / `os` / `cpu` / `optionalDependencies` edits are WWS02 scope, NOT WWS01)
- `packages/terminal-commander/lib/**`
- `packages/terminal-commander/bin/**`
- secrets / tokens / private usernames / private absolute paths

contracts_or_interfaces:
- The contract document MUST identify every conflict with NPM02 (root `os: ["linux"]`) and propose the exact amendment WWS02 will implement.
- The contract document MUST resolve every open decision listed in `GOAL_CHAIN_INDEX.md` (whether Windows setup may install inside WSL, whether pairing is required, where state files live, WSL distro selection, Cursor scope choice).
- The contract document MUST cite the existing TC44 runtime boundary (Unix-only PTY + UDS) so the win32 widening is package-shape only, not runtime claim.
- The contract document MUST link `examples/provider-harness/cursor/*.json` so operators have a reference shape even before WWS05's writer lands.

invariants:
- TC48 + NPM10 `Conditional Go` posture preserved (WWS01 is doc-only).
- MCP guard greps remain clean (WWS01 touches no Rust source).
- No `Not Run` evidence promoted to PASS.

acceptance_criteria:
- The contract document exists, links the right NPM02 / NPM08 / TC44 prior work, and lists every open decision resolved to a binding answer.
- `crates/**` and `packages/*/package.json` untouched.
- `.github/**` untouched.
- The contract clearly separates Windows-bridge / setup surface from WSL runtime surface.

evidence_required:
- Branch evidence.
- File paths.
- Open-decision table with binding answers.
- Conflict list against NPM02 + NPM08.

stop_conditions:
- Branch is not `main`.
- A binding answer requires modifying `crates/**` or runtime behavior to be honest.
- The Cursor MCP discovery contract is not stable enough to commit a single shim shape.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run WWS01 only on branch `main`. Documentation-only contract goal. Stop on any forbidden-file diff. Do not implement.

## Final Report Format

Objective / Changes / Files changed / Open decisions resolved / Conflicts identified with NPM02 / Verification / Evidence / Commit / Known gaps / Next goal (WWS02).
