---
goal_id: WWS04
title: Windows Bridge Mcp Shim
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 2 - Bridge shim
status: "Pending"
depends_on: ["WWS03"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "WWS02 root package widening (`bridge_required` resolver branch)"
  - "WWS03 detect / doctor helpers"
  - "packages/terminal-commander/bin/terminal-commander-mcp.js (existing Linux shim baseline)"
risk_level: "high"
---

# WWS04 - Windows Bridge Mcp Shim

## Branch Guard

```text
main
```

## Mission Context

The Windows shim is the most security-sensitive JS code in the chain. Cursor on Windows launches `terminal-commander-mcp`; on a Windows host the resolver returns `bridge_required` and the shim must transparently `spawn` `wsl.exe -d <distro> bash -lc 'terminal-commander-mcp'`, forward `stdio: 'inherit'`, and mirror the child's exit code / signal. No shell interpolation. No buffering. No log mutation. Stdin / stdout flow rmcp framing transparently across the WSL pipe.

This goal also updates the two sibling shims (`terminal-commanderd.js`, `terminal-commander.js`) so they print a clear error on Windows ("daemon and admin CLI must run inside WSL — use `terminal-commander-mcp` instead, or open a WSL shell"). They do NOT bridge those commands across `wsl.exe`; the daemon and admin CLI are out of scope for a Cursor stdio entrypoint.

## Mini-Spec

objective:
- Amend `packages/terminal-commander/bin/terminal-commander-mcp.js` to handle the `bridge_required` resolver case by spawning `wsl.exe` with a fixed argv that runs `terminal-commander-mcp` inside the configured distro. Resolve the distro via a small JSON config file under `%LOCALAPPDATA%\terminal-commander\config.json` (or `$XDG_CONFIG_HOME/terminal-commander/config.json` on Linux for symmetry); fall back to `wsl.exe --status` default distro if no config file. The other two shims print an unsupported-on-Windows message and exit 64. Tests update accordingly.

non_goals:
- No automatic WSL install attempt.
- No CLI subcommand surface (`setup` / `doctor` belong to WWS06).
- No new MCP tool.
- No `crates/**` change.
- No workflow change.

allowed_files_or_area:
- `packages/terminal-commander/bin/terminal-commander-mcp.js`
- `packages/terminal-commander/bin/terminal-commanderd.js`
- `packages/terminal-commander/bin/terminal-commander.js`
- `packages/terminal-commander/lib/wsl/spawn.js` (new — bridge invocation helper)
- `packages/terminal-commander/lib/config.js` (new — read / locate config file)
- `packages/terminal-commander/test/**`
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS04-*.md`

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
- `bin/terminal-commander-mcp.js` on Windows: resolves the configured distro, prints nothing to stdout (rmcp framing!), spawns `wsl.exe -d <distro> bash -lc 'terminal-commander-mcp'` with `stdio: 'inherit'`, mirrors exit code / signal, forwards SIGINT / SIGTERM.
- On Linux: existing behavior unchanged.
- On macOS / other: existing `unsupported_platform` exit 64.
- `bin/terminal-commanderd.js` and `bin/terminal-commander.js` on Windows: print one stderr line ("daemon / admin CLI run inside WSL; use `wsl -d <distro> terminal-commanderd ...`") and exit 64.
- `lib/wsl/spawn.js`: pure helper, validates distro name against the WWS03 detect list (whitelist; rejects shell metachars).
- `lib/config.js`: locates config file using only standard `process.env` paths (`LOCALAPPDATA`, `APPDATA`, `XDG_CONFIG_HOME`, `HOME`); never writes the config file (writer lives in WWS06).

invariants:
- `spawn(wsl_path, [...argv], { shell: false, stdio: 'inherit', windowsHide: true })` — no shell interpolation, ever.
- No raw stream endpoint added.
- No file open outside the package directory + the config file path.
- No network listener.
- MCP guard greps remain clean (no `crates/**` change).
- Cursor's rmcp framing on stdout / stdin is preserved bit-for-bit.

acceptance_criteria:
- Resolver + shim unit tests pass on `npm test`.
- Pack dry-runs still clean.
- The shim refuses operator-supplied distro names that contain shell metachars.
- On a non-Windows host running the WWS04 test suite, the bridge path is gated off; on Windows the bridge path executes (mocked `wsl.exe` for test cases is acceptable).

evidence_required:
- Branch evidence.
- File paths.
- Test results.
- Pack dry-run.

stop_conditions:
- Branch is not `main`.
- The bridge would require `shell: true` or any shell escape.
- The shim would require writing to stdout outside the rmcp framing (any stdout write would break Cursor's MCP transport).

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

Run WWS04 only on branch `main`. Shim runtime + spawn helper + config locator only. No CLI subcommand. No shell interpolation. No publish. No workflow change.

## Final Report Format

Objective / Changes / Files changed / Test results / Stdout-cleanliness verification / Verification / Evidence / Commit / Known gaps / Next goal (WWS05).
