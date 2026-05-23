---
goal_id: WWS03
title: Wsl Distro Discovery And Runtime Doctor
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 1 - Setup helpers
status: "Pending"
depends_on: ["WWS02"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "WWS01 contract (open decision on distro selection)"
  - "WWS02 root package widening"
  - "Windows wsl.exe documented CLI surface"
risk_level: "medium"
---

# WWS03 - Wsl Distro Discovery And Runtime Doctor

## Branch Guard

```text
main
```

## Mission Context

Before the bridge shim can launch the WSL-side MCP server, the Windows root package must be able to (a) detect that WSL is installed at all, (b) enumerate available distros via `wsl.exe --list --verbose`, (c) pick a default distro (or ask the operator once and persist the choice), and (d) `doctor`-check whether `terminal-commanderd` is installed and runnable inside the chosen distro. This is the JS-only helper layer that `setup cursor-wsl` (WWS06) and the bridge shim (WWS04) consume.

No Rust change. No daemon change. No publish. All `wsl.exe` invocations go through `child_process.spawn` with `shell: false` and a fixed argv shape; no shell interpolation.

## Mini-Spec

objective:
- Add `packages/terminal-commander/lib/wsl/detect.js` (probe `wsl.exe --status`, return structured `{ installed: bool, default_distro: string|null, distros: [...] }` from `wsl.exe --list --verbose --quiet`). Add `packages/terminal-commander/lib/wsl/doctor.js` (run `wsl.exe -d <distro> bash -lc 'command -v terminal-commander-mcp && terminal-commander-mcp --help'` and return structured `{ installed_in_wsl: bool, mcp_help_ok: bool, distro: string }`). Add tests under `packages/terminal-commander/test/`. No CLI surface yet (`terminal-commander setup` and `terminal-commander doctor` subcommands are WWS06).

non_goals:
- No CLI subcommand surface yet.
- No automatic install inside WSL (WWS06).
- No `crates/**` change.
- No daemon change.
- No new MCP tool.
- No workflow / publish.

allowed_files_or_area:
- `packages/terminal-commander/lib/wsl/**`
- `packages/terminal-commander/test/**`
- `packages/terminal-commander/package.json` (only if a `test` script needs adjustment)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS03-*.md`

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
- `packages/terminal-commander/bin/**`
- secrets / tokens / private paths

contracts_or_interfaces:
- `detect.js` exports a pure async function that returns the structured shape above.
- `doctor.js` exports a pure async function gated by the detect result.
- Every `wsl.exe` invocation uses `spawn(wsl_path, [...argv], { stdio: ['ignore', 'pipe', 'pipe'], shell: false, windowsHide: true })`.
- No argument interpolated from user input; the only operator-supplied value is the distro name, which is whitelisted against the `detect.js` distro list.
- Functions degrade gracefully on non-Windows hosts: return `{ installed: false, reason: 'not_windows' }`.

invariants:
- No raw stream lane added.
- No shell expansion.
- No spawn of anything other than `wsl.exe`.
- No file open outside the package directory.
- TC48 + NPM10 `Conditional Go` posture preserved.

acceptance_criteria:
- New JS modules under `packages/terminal-commander/lib/wsl/` exist and are exercised by Node built-in test runner.
- `npm test` inside `packages/terminal-commander` passes (resolver tests + new wsl tests).
- `npm pack --dry-run` still clean for all three packages.
- Resolver tests from WWS02 still pass.
- `crates/**` untouched; runtime smoke unchanged.

evidence_required:
- Branch evidence.
- File paths.
- Test results.
- Pack dry-run.

stop_conditions:
- Branch is not `main`.
- The detect / doctor helpers would require dispatching `wsl.exe` with operator-supplied shell strings.

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
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run WWS03 only on branch `main`. JS detect + doctor helpers only. No CLI subcommand. No `wsl.exe` shell interpolation. No publish. No workflow change.

## Final Report Format

Objective / Changes / Files changed / Test results / Verification / Evidence / Commit / Known gaps / Next goal (WWS04).
