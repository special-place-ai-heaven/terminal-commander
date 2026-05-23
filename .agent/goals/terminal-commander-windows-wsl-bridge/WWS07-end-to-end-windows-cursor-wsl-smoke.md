---
goal_id: WWS07
title: End To End Windows Cursor Wsl Smoke
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 3 - Verification
status: "Pending"
depends_on: ["WWS06"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "WWS04 bridge shim"
  - "WWS05 cursor config writer"
  - "WWS06 setup cursor-wsl orchestrator"
  - "docs/integrations/cursor.md (NPM08 baseline)"
risk_level: "medium"
---

# WWS07 - End To End Windows Cursor Wsl Smoke

## Branch Guard

```text
main
```

## Mission Context

WWS07 is the chain's verification goal. It exercises the full Windows → Cursor → WSL path end-to-end on the verification host:

1. Fresh-ish Windows shell.
2. `npm install -g <local-tarball>` (root + Linux platform packages staged via NPM04 staging).
3. `terminal-commander setup cursor-wsl --dry-run` — prints the planned actions.
4. `terminal-commander setup cursor-wsl` — writes the Cursor config + verifies the bridge shim.
5. Cursor opened by the operator; the chat panel asked to list MCP tools (expect 29) and to call `health`.
6. Operator captures transcript / screenshot.

Adds a scripted helper `scripts/smoke/verify-windows-bridge-smoke.ps1` (PowerShell, Windows-only) that performs steps 2–4 + a non-GUI bridge invocation (`echo '{}' | terminal-commander-mcp` — confirm the rmcp `initialize` handshake completes through `wsl.exe`). Cursor GUI smoke remains operator-driven; if Cursor cannot be exercised, WWS07 records `Not Run` for the Cursor leg with the exact blocker.

## Mini-Spec

objective:
- Add `scripts/smoke/verify-windows-bridge-smoke.ps1` that exercises the WWS04 bridge shim from a Windows shell into WSL, verifies the MCP `initialize` handshake completes, and `tools/list` returns 29 tools. Pair with documentation in `docs/integrations/cursor.md` for the operator-driven Cursor GUI smoke (extend the existing NPM08 doc, do not replace it).

non_goals:
- No `crates/**` change.
- No daemon change.
- No new MCP tool.
- No workflow change.
- No publish.

allowed_files_or_area:
- `scripts/smoke/verify-windows-bridge-smoke.ps1` (new)
- `docs/integrations/cursor.md` (extend with the operator-driven Cursor GUI smoke checklist)
- `docs/release/**` (cross-link)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS07-*.md`

forbidden_files:
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `.github/**`
- `packages/*/package.json` (no version edit)
- secrets / tokens / private paths

contracts_or_interfaces:
- PowerShell script: idempotent, exits non-zero on any failed step, prints structured `PASS` / `FAIL` lines (mirrors NPM04 smoke style).
- The script does NOT install anything globally on the host; it installs into a sandboxed `--prefix` analogous to NPM04.
- The script confirms WSL is reachable, the configured distro exists, and the WSL-side `terminal-commander-mcp` returns the expected `tools/list` length.
- If Cursor GUI smoke cannot run, WWS07 records `Not Run` with the exact blocker (e.g. "Cursor 3.5.30 present but no headless MCP discovery entry point on host").

invariants:
- Cursor live smoke is honestly `Not Run` unless an operator transcript is attached.
- `Not Run` is NOT PASS.
- No bridge invocation that involves shell interpolation.
- TC48 + NPM10 `Conditional Go` posture preserved unless a real Cursor transcript lands (then WWS09 promotes the posture).

acceptance_criteria:
- The PowerShell smoke script exists and is non-interactive.
- `docs/integrations/cursor.md` carries a Windows-bridge smoke checklist + the existing NPM08 walk-through.
- The Cursor GUI smoke status is honestly recorded.

evidence_required:
- Branch evidence.
- File paths.
- PowerShell smoke output (or recorded "Not Run" blocker if Windows shell + WSL host is unavailable inside the goal session).
- Cursor GUI smoke status.

stop_conditions:
- Branch is not `main`.
- The smoke would require introducing a shell bridge.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh
( cd packages/terminal-commander && npm test )
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run WWS07 only on branch `main`. Bridge smoke script + Cursor doc extension only. Cursor GUI smoke is operator-driven; record `Not Run` honestly if unattainable.

## Final Report Format

Objective / Changes / Files changed / PowerShell smoke output or Not Run reason / Cursor GUI smoke status / Verification / Evidence / Commit / Known gaps / Next goal (WWS08).
