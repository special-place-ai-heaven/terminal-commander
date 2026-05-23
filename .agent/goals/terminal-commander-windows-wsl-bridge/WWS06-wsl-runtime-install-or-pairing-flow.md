---
goal_id: WWS06
title: Wsl Runtime Install Or Pairing Flow
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 3 - CLI assembly
status: "Pending"
depends_on: ["WWS05"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "WWS01 contract (open decision on auto-install + pairing)"
  - "WWS03 detect / doctor helpers"
  - "WWS04 bridge shim + config locator"
  - "WWS05 cursor config writer"
risk_level: "high"
---

# WWS06 - Wsl Runtime Install Or Pairing Flow

## Branch Guard

```text
main
```

## Mission Context

WWS06 wires WWS03 + WWS04 + WWS05 helpers into the `terminal-commander` CLI subcommand surface. The new commands:

- `terminal-commander setup cursor-wsl [--distro <name>] [--force]` — Windows-side end-to-end setup: detect WSL → pick distro (operator-supplied OR default) → confirm runtime is installed inside WSL (or print the exact one-line install command — automatic install requires `--install-wsl-runtime`) → write Cursor MCP config (default global; `--project` for workspace) → run `health` smoke through the bridge → print ready banner.
- `terminal-commander doctor` — both Windows and WSL host: print structured diagnostics (WSL installed / distros / configured distro / Cursor config presence / runtime install status / daemon reachability).
- `terminal-commander pair create` — Windows: optional manual pairing (generates a 6-digit code stored in `%LOCALAPPDATA%\terminal-commander\pair.json`; prints code to operator).
- `terminal-commander pair accept <code>` — WSL: optional manual pairing (verifies code matches Windows-side manifest accessible via `wsl.exe --status` UNC-style or simply by asking the operator to copy/paste; defaults to no-op if `--no-pair` is set).

The pairing flow is OPTIONAL. Default install relies on `wsl.exe` automatic invocation. The 6-digit pairing is operator confirmation / anti-misconfiguration, NOT a security secret.

## Mini-Spec

objective:
- Add `packages/terminal-commander/lib/cli/**` with the four subcommands above. Wire them through `packages/terminal-commander/bin/terminal-commander.js` (extended from the current Linux-only admin CLI shim). Add tests. No publish.

non_goals:
- No automatic install of WSL itself (operator must run `wsl --install` separately).
- No automatic npm install inside WSL without `--install-wsl-runtime` flag.
- No `crates/**` change.
- No new MCP tool.
- No workflow / publish.
- No PowerShell scripts. JS only.

allowed_files_or_area:
- `packages/terminal-commander/lib/cli/**`
- `packages/terminal-commander/bin/terminal-commander.js` (extend Windows branch only; Linux behavior unchanged)
- `packages/terminal-commander/test/**`
- `packages/terminal-commander/package.json` (only if a `bin` entry or `test` script needs adjustment)
- `docs/install/**` (cross-link the new subcommands)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS06-*.md`

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
- secrets / tokens / private paths

contracts_or_interfaces:
- `lib/cli/setup_cursor_wsl.js`: orchestrator. Calls `lib/wsl/detect.js`, `lib/wsl/doctor.js`, `lib/cursor/config.js`, `lib/cursor/write.js`. Prints a structured human banner; never echoes secrets.
- `lib/cli/doctor.js`: structured diagnostics with closed-set status fields.
- `lib/cli/pair_create.js` / `pair_accept.js`: optional helpers. Generate 6-digit code (cryptographic random or `crypto.randomInt(100000, 999999)`); store under `%LOCALAPPDATA%\terminal-commander\pair.json` with `pair_id`, `code`, `created_at`, `distro` fields.
- Every WSL invocation goes through the WWS04 `lib/wsl/spawn.js` whitelist-validated helper.
- All operator-supplied flags are validated up front; unknown flags exit non-zero with usage.

invariants:
- No new MCP tool, no IPC surface change.
- No `--no-confirm` flag for the automatic WSL install path (operator MUST opt in via `--install-wsl-runtime`).
- `setup cursor-wsl` never overwrites a non-terminal-commander MCP entry; refuses to overwrite an existing terminal-commander entry without `--force`.
- Pair codes are NOT treated as cryptographic secrets in any decision; they are human confirmation.
- TC48 + NPM10 `Conditional Go` posture preserved.

acceptance_criteria:
- New JS modules exist; CLI subcommands documented in `--help`.
- `npm test` passes.
- Pack dry-runs clean.
- Manual happy-path on a Windows host (or noted as Not Run with exact blocker): `setup cursor-wsl --dry-run` prints the planned actions without performing them.

evidence_required:
- Branch evidence.
- File paths.
- Test results.
- `--help` output for the new subcommands.

stop_conditions:
- Branch is not `main`.
- A CLI subcommand would require shell interpolation.
- A subcommand would require running `npm install` outside the WSL distro the operator explicitly named.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
( cd packages/terminal-commander && npm test )
npm pack ./packages/terminal-commander --dry-run
npm pack ./packages/terminal-commander-linux-x64 --dry-run
npm pack ./packages/terminal-commander-linux-arm64 --dry-run
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run WWS06 only on branch `main`. JS CLI subcommands only. No shell interpolation. No PowerShell. No publish. No `crates/**` change.

## Final Report Format

Objective / Changes / Files changed / Test results / `--help` output / Verification / Evidence / Commit / Known gaps / Next goal (WWS07).
