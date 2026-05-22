---
goal_id: NPM08
title: Cursor Mcp Install Config Smoke
chain_id: terminal-commander-npm-distribution
phase: Wave 5 - Provider smoke
status: "Pending"
depends_on: ["NPM07"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "Cursor MCP stdio config docs"
  - "docs/integrations/codex-cli.md (existing pattern from TC46)"
  - "docs/integrations/claude-code.md (existing pattern from TC46)"
risk_level: "low"
---

# NPM08 - Cursor Mcp Install Config Smoke

## Branch Guard

```text
main
```

## Mission Context

NPM07 produced a published npm package. NPM08 documents the Cursor MCP install path against the published package and runs the live Cursor smoke if the operator host has Cursor available. If not, this goal records `Not Run` with the exact blocker — never promoted to PASS.

## Mini-Spec

objective:
- Add `docs/integrations/cursor.md` documenting the Cursor MCP stdio install path against `npm install -g terminal-commander`. Cover both Linux/WSL2-native Cursor and Windows Cursor calling through `wsl`. Execute the live smoke if Cursor is available; otherwise record `Not Run` with the exact blocker.

non_goals:
- Do not modify `crates/**` or runtime behavior.
- Do not modify the published npm packages.
- Do not write a Cursor extension.
- Do not fake provider success.

allowed_files_or_area:
- docs/integrations/cursor.md (new)
- docs/integrations/README.md (status table refresh)
- docs/release/**
- scripts/smoke/** (only if a non-interactive Cursor harness exists; otherwise no script)
- .agent/goals/terminal-commander-npm-distribution/NPM08-*.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**, config/**
- packages/**
- .github/workflows/**
- secrets / tokens / private paths anywhere

contracts_or_interfaces:
- The Cursor MCP stdio config uses `command: "terminal-commander-mcp"` (from npm global install) and env `TC_SOCKET=${TC_DATA}/terminal-commanderd.sock`.
- The Windows-Cursor + WSL path uses `command: "wsl"` + `args: ["-d", "<distro>", "bash", "-lc", "terminal-commander-mcp"]`.
- The Cursor smoke is defined as: operator opens a Cursor session, asks Cursor to list MCP tools, confirms the 29-tool TC45 catalogue, and asks Cursor to run `command_start_combed` → `bucket_wait` → `command_status` against `echo hello`. Transcript or screenshot evidence is attached.
- If Cursor is not installed on the verification host, the goal is `Completed` with the live smoke marked `Not Run` — exact blocker recorded. The doc still ships.

invariants:
- No secrets / tokens / private paths in the doc.
- No machine-specific absolute paths; use `${TC_DATA}` / `${XDG_STATE_HOME}` style placeholders.

acceptance_criteria:
- `docs/integrations/cursor.md` exists with both the native and WSL-bridge config examples.
- `docs/integrations/README.md` lists Cursor alongside Codex and Claude Code with honest status.
- Live Cursor smoke either: (a) executed with a captured transcript reference, or (b) marked `Not Run` with exact blocker.
- No runtime / npm package changes.

evidence_required:
- Branch evidence.
- File paths changed.
- Cursor smoke transcript reference OR `Not Run` blocker text.
- Beta posture in `RELEASE_CHECKLIST.md` updated only if at least one provider live smoke transcript exists.

stop_conditions:
- Branch is not `main`.
- The Cursor doc would require runtime / npm package changes.
- The host requires interactive auth / secrets to run the smoke and the operator did not pre-authorize the smoke run.

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
# Bounded grep — no secrets / private paths in the new doc:
rg "(/home/|/Users/|C:\\\\Users\\\\|sk-|ghp_|npm_)" docs/integrations/cursor.md || true
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM08 only on branch `main`. Document the Cursor MCP install path. If Cursor is unavailable, mark the live smoke `Not Run` with the exact blocker.

## Final Report Format

Objective / Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal (NPM09).
