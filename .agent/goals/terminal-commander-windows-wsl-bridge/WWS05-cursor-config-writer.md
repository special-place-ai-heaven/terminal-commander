---
goal_id: WWS05
title: Cursor Config Writer
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 2 - Setup helpers
status: "Pending"
depends_on: ["WWS04"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "WWS01 contract (Cursor scope choice + write/refuse-if-present policy)"
  - "examples/provider-harness/cursor/ (NPM08 three reference JSON shapes)"
  - "docs/integrations/cursor.md (NPM08 walk-through)"
risk_level: "medium"
---

# WWS05 - Cursor Config Writer

## Branch Guard

```text
main
```

## Mission Context

Cursor reads MCP server configs from `~/.cursor/mcp.json` (global) or `<workspace>/.cursor/mcp.json` (project). WWS05 ships a JS-only writer + merger that, given a host (Windows or inside-WSL) and a target scope, produces a config that adds the `terminal-commander` server stanza WITHOUT clobbering existing MCP entries the operator already has.

Safety: never overwrite an unrelated entry. Refuse to overwrite an existing `terminal-commander` entry unless the operator passes `--force`. Always write atomically (write to `mcp.json.tmp` + `rename`). Always preserve unrelated keys. Never print the resolved path to stdout in a way that leaks the operator's home directory unnecessarily — print a relative form when possible.

## Mini-Spec

objective:
- Add `packages/terminal-commander/lib/cursor/config.js` (read + parse the target Cursor mcp.json if it exists; produce a merged JSON with the `terminal-commander` server added). Add `packages/terminal-commander/lib/cursor/write.js` (atomic write + backup of any pre-existing file to `<path>.bak`). Add tests under `packages/terminal-commander/test/`. No CLI subcommand yet (CLI surface in WWS06).

non_goals:
- No CLI subcommand.
- No `wsl.exe` invocation.
- No `crates/**` change.
- No daemon change.
- No new MCP tool.
- No workflow / publish.

allowed_files_or_area:
- `packages/terminal-commander/lib/cursor/**`
- `packages/terminal-commander/test/**`
- `packages/terminal-commander/package.json` (only if a `test` script needs adjustment)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS05-*.md`

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
- `.cursor/mcp.json` anywhere in the repo (no active Cursor config committed)
- secrets / tokens / private paths

contracts_or_interfaces:
- `config.js`: pure functions, no side effects. `load(path)`, `merge(existing, stanza)`, `validate(merged)`.
- `write.js`: atomic write + backup. `write(path, json, { force, backup_path })`.
- Stanza shape for Windows host:
  ```json
  { "terminal-commander": { "type": "stdio", "command": "terminal-commander-mcp" } }
  ```
  Stanza shape for inside-WSL host: identical.
  Stanza shape for explicit WSL-from-Windows-fallback (when the bridge shim is not on PATH but `wsl.exe` is):
  ```json
  { "terminal-commander": { "type": "stdio", "command": "wsl", "args": ["-d", "<distro>", "bash", "-lc", "terminal-commander-mcp"] } }
  ```
- Refuse to overwrite an existing `terminal-commander` entry without `force: true`.
- Always back up `mcp.json` to `mcp.json.bak` before overwrite (even with `force`).
- Never print absolute paths to stdout; use `~` / `%USERPROFILE%` placeholders in user-facing messages.

invariants:
- No raw stream endpoint added.
- No shell expansion.
- No spawn of any process (this goal is pure file IO over `mcp.json`).
- No file open outside the Cursor config directory + a temp / backup neighbor.
- MCP guard greps remain clean.

acceptance_criteria:
- New JS modules exist; `npm test` passes (resolver + WWS03 + WWS04 + new WWS05 cases).
- Pack dry-runs still clean.
- Round-trip test: load existing config with two unrelated servers → add terminal-commander → re-load → both unrelated servers preserved AND terminal-commander present.

evidence_required:
- Branch evidence.
- File paths.
- Test results.
- Pack dry-run.

stop_conditions:
- Branch is not `main`.
- The writer would need to read or modify any file outside the Cursor config directory.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
( cd packages/terminal-commander && npm test )
npm pack ./packages/terminal-commander --dry-run
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
bash scripts/smoke/verify-runtime-smoke.sh
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run WWS05 only on branch `main`. Cursor config reader + merger + atomic writer only. No CLI subcommand. No `wsl.exe` invocation. No spawn at all. No publish.

## Final Report Format

Objective / Changes / Files changed / Test results / Round-trip evidence / Verification / Evidence / Commit / Known gaps / Next goal (WWS06).
