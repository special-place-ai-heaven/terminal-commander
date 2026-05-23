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
  - "WWS01 contract (docs/release/windows-wsl-bridge-contract.md) — §4.4 shim safety contract; §10.2 lib/wsl/spawn.js (WWS04); §15 D-04 / D-12; §12 failure-mode table"
  - "WWS02 root package widening (commit 1da40f3) — `bridge_required` resolver branch"
  - "WWS03 detect / doctor / distro-name helpers (commit ec8441e)"
  - "packages/terminal-commander/bin/terminal-commander-mcp.js (existing Linux shim baseline at WWS02)"
risk_level: "high"
---

# WWS04 - Windows Bridge Mcp Shim

## Branch Guard

```text
main
```

## Mission Context

The Windows `terminal-commander-mcp` shim is the most security-sensitive JS code in the chain. Cursor on Windows launches `terminal-commander-mcp`; on a Windows host the WWS02 resolver returns `bridge_required` and at WWS04 the shim must transparently `spawn` `wsl.exe -d <distro> -- bash -lc 'exec terminal-commander-mcp'`, wire `stdio: 'inherit'`, mirror the child's exit code / signal, and forward `SIGINT` / `SIGTERM`. NO shell interpolation. NO buffering. NO log mutation. NO stdout writes from the shim itself (Cursor's rmcp framing lives on stdout — any extra byte breaks the transport).

The two sibling shims (`terminal-commanderd.js`, `terminal-commander.js`) keep their WWS02 `bridge_required` + exit 64 contract at WWS04. WWS04 does NOT bridge the daemon or the admin CLI; only the MCP stdio entry-point bridges.

WWS04 closes WWS01 binding decision D-04 (bridge shape for `terminal-commander-mcp`). The CLI surface (D-13/D-14 — `setup cursor-wsl`, `doctor`, `pair`) and the Cursor config writer (D-13) remain with WWS05 / WWS06.

## Prep amendment (2026-05-23, before WWS04 implementation)

This goal file was prep-amended once before WWS04 implementation started. Two scope adjustments were locked:

1. **Allowed-files widening**. The original allowed set covered only the three bin shims, `lib/wsl/spawn.js`, `lib/config.js`, `test/**`, and the goal file. The directive widened the WWS04 implementation to also touch `packages/terminal-commander/README.md`, `docs/release/windows-wsl-bridge-contract.md` (§10.2 / §18 status clarification only), `docs/integrations/cursor.md` (§11b WWS04 status update only), `examples/provider-harness/cursor/` (operator-facing example doc only — example JSON config files stay byte-identical), `GOAL_CHAIN_INDEX.md`, and `RUN_ORDER.md` for status alignment.
2. **Distro selection source**. The original goal file proposed reading `%LOCALAPPDATA%\terminal-commander\config.json` (or `$XDG_CONFIG_HOME/terminal-commander/config.json`) for the chosen distro at WWS04. This collides with WWS01 §11 / D-10: the setup-state file is owned and written by WWS06 (`terminal-commander setup cursor-wsl`). Reading a file WWS06 has not implemented a writer for would either ship dormant-code or pre-empt WWS06's contract. **Locked replacement**: WWS04 sources the distro from a closed priority order with NO file reads:
   - **Priority 1**: `process.env.TC_WSL_DISTRO` (operator override). Validated via `assertSafeDistroName` AND membership in the live `detectWsl()` whitelist.
   - **Priority 2**: `detectWsl().default_distro` (asterisk row in `wsl.exe -l -v`).
   - **Priority 3**: Refuse with a bounded `no_default_distro` stderr line + exit 64; point operator at WWS06 `setup cursor-wsl` (which WILL write `%LOCALAPPDATA%\terminal-commander\setup.json` at WWS06 / D-10 / D-12). The `lib/config.js` introduction is **deferred to WWS06**; WWS04 ships NO file-reader.
3. **Optional runtime-presence gate**. WWS04 MAY use the WWS03 `wslDoctor({ probeRuntime: true })` once before the bridge launch to short-circuit with `runtime_missing` when `terminal-commander-mcp` is not yet installed inside the chosen distro. The gate is opt-out via `TC_WSL_SKIP_DOCTOR=1` so Cursor users who already verified the runtime can avoid the extra `wsl.exe` round-trip per launch. No install attempt — that is WWS06 / D-08.
4. **Doctrine carry-forward**. `CAP01-capability-registry-contract.md` remains a future goal. NOT started at WWS04.

## Mini-Spec

objective:
- Add `packages/terminal-commander/lib/wsl/spawn.js` exporting `spawnWslBridge(opts)`. Resolves the bridge distro (priority: `TC_WSL_DISTRO` env -> `detectWsl().default_distro`), double-validates it (`assertSafeDistroName` + live whitelist), optionally runs the WWS03 `wslDoctor` runtime-presence probe (default ON; off via `TC_WSL_SKIP_DOCTOR=1`), and `spawn`s `wsl.exe -d <distro> -- bash -lc 'exec terminal-commander-mcp'` with `shell: false`, `windowsHide: true`, and `stdio: 'inherit'`. Wires `SIGINT` / `SIGTERM` forwarding and exit-code / signal mirroring.
- Amend `packages/terminal-commander/bin/terminal-commander-mcp.js` so the `bridge_required` resolver branch delegates to `spawnWslBridge()` on Windows. Linux behaviour stays byte-identical (resolver returns `ok`, shim spawns the resolved Linux binary).
- Add unit + static-guard tests for `spawnWslBridge()` and the Windows shim wiring under `packages/terminal-commander/test/`.
- The two sibling shims (`bin/terminal-commanderd.js`, `bin/terminal-commander.js`) stay BYTE-IDENTICAL at WWS04. Their WWS02 `bridge_required` + exit 64 contract is preserved; static-guard tests enforce this.
- Cross-link the new behaviour from `packages/terminal-commander/README.md` (Windows section), `docs/release/windows-wsl-bridge-contract.md` (§10.2 / §18 mark WWS04 landed), `docs/integrations/cursor.md` (§11b WWS04 status), and `examples/provider-harness/cursor/README.md` (operator note: the WWS06 setup CLI is still pending; manual `wsl.exe -d ...` config remains valid; the WWS04 shim is an additional working path once the WSL runtime is installed).
- Update `WWS04-*.md` frontmatter to Completed in the status commit.
- Update `GOAL_CHAIN_INDEX.md` + `RUN_ORDER.md` to mark WWS04 Completed.

non_goals:
- NO Cursor config writer (`lib/cursor/**`) — that is WWS05.
- NO `setup cursor-wsl` / `doctor` / `pair` CLI subcommands — that is WWS06.
- NO `setup.json` / `pair.json` writer — that is WWS06.
- NO `lib/config.js` reader at WWS04 (deferred to WWS06's writer).
- NO automatic WSL runtime install — that is WWS06 behind explicit `--install-wsl-runtime`.
- NO pairing implementation — WWS06.
- NO `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`, `scripts/**`, `.github/**` change.
- NO new MCP tool. NO daemon change. NO IPC change.
- NO publish. NO workflow dispatch. NO version bump. NO release-please change.
- NO `terminal-commanderd.js` / `terminal-commander.js` edits beyond verifying their WWS02 baseline (static guard only — bytes unchanged).
- NO macOS code paths. NO Windows-native runtime claim. NO postinstall downloader.
- NO new top-level spawn target beyond `wsl.exe` (and only inside `lib/wsl/spawn.js`).
- NO sudo. NO password. NO credential broker. NO env-var-secret echo.

allowed_files_or_area:
- `packages/terminal-commander/bin/terminal-commander-mcp.js`
  (Windows branch only — Linux branch stays byte-identical)
- `packages/terminal-commander/lib/wsl/spawn.js`
  (new — bridge invocation helper; the only WWS04 production-code addition)
- `packages/terminal-commander/test/**`
  (new test files; existing tests must stay GREEN)
- `packages/terminal-commander/README.md`
  (Windows section: replace WWS02 "Refuses" row for `terminal-commander-mcp` with WWS04 "Bridges to WSL via wsl.exe" description; preserve other rows)
- `docs/release/windows-wsl-bridge-contract.md`
  (§10.2 / §18 WWS04 row marked landed — no binding decision change)
- `docs/integrations/cursor.md`
  (§11b status update naming WWS04 as the bridge-shim layer)
- `examples/provider-harness/cursor/README.md`
  (operator note about the new bridge path; example JSON files stay byte-identical)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS04-*.md` (this file)
- `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md`
- `.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md`

forbidden_files:
- `packages/terminal-commander/bin/terminal-commanderd.js` (byte-identical at WWS04)
- `packages/terminal-commander/bin/terminal-commander.js` (byte-identical at WWS04)
- `packages/terminal-commander/lib/resolve-binary.js`
- `packages/terminal-commander/lib/wsl/detect.js` (WWS03 contract — unchanged)
- `packages/terminal-commander/lib/wsl/doctor.js` (WWS03 contract — unchanged; consumed via require())
- `packages/terminal-commander/lib/wsl/distro-name.js` (WWS03 contract — unchanged; consumed via require())
- `packages/terminal-commander/lib/wsl/index.js`
- `packages/terminal-commander/package.json`
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `scripts/**`
- `.github/**`
- `packages/terminal-commander-linux-x64/**`
- `packages/terminal-commander-linux-arm64/**`
- `examples/provider-harness/cursor/*.json` (byte-identical at WWS04; the WWS04 path is an alternative — the manual `wsl.exe -d ...` JSON config in §6 of cursor.md remains valid)
- secrets / tokens / private paths

contracts_or_interfaces:
- `spawnWslBridge(opts)` is an async function. Default `opts` (all optional, all defaulting): `platform` (default `process.platform`), `env` (default `process.env`), `argv` (default `process.argv.slice(2)`), `exec` (a mocked executor for tests; defaults to a thin wrapper around `child_process.spawn('wsl.exe', argv, { shell: false, windowsHide: true, stdio: 'inherit' })`), `detect` (a mocked WWS03 `detectWsl` for tests), `doctor` (a mocked WWS03 `wslDoctor` for tests), `wslPath` (default `'wsl.exe'`), `timeoutMs` (default 5000).
- Resolves the distro:
  1. If `env.TC_WSL_DISTRO` is set: validate via `assertSafeDistroName` + live whitelist; refuse if either check fails.
  2. Else use `detectWsl().default_distro`; refuse with `no_default_distro` if null.
- Optionally runs `wslDoctor({ distro, probeRuntime: true })` (skipped iff `env.TC_WSL_SKIP_DOCTOR === '1'`). Refuses with `runtime_missing` when the probe says so. No install attempt.
- Spawns `wsl.exe` with EXACTLY `argv = ['-d', distro, '--', 'bash', '-lc', BRIDGE_PROBE_CMD, ...userArgv]` where `BRIDGE_PROBE_CMD` is the literal constant `'exec terminal-commander-mcp'`. `userArgv` (extra MCP arguments forwarded from Cursor) is passed AFTER the `--` separator so `bash -lc` receives them in `$0..$N`. No user value is interpolated into `BRIDGE_PROBE_CMD` itself.
- spawn options are EXACTLY `{ shell: false, windowsHide: true, stdio: 'inherit', env: filteredEnv }` where `filteredEnv` is a defensive copy of `process.env` with token-shaped variables removed (see `SECRET_ENV_PATTERNS` in spawn.js — `NPM_TOKEN`, `CARGO_REGISTRY_TOKEN`, `RELEASE_PLEASE_TOKEN`, `GITHUB_TOKEN`, `GH_TOKEN`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `SLACK_TOKEN`, and any var matching `/_(TOKEN|SECRET|PASSWORD|API_KEY|PASS)$/i`). The filter is a defensive belt-and-braces measure; the bridge does not REQUIRE any of these to function.
- Mirrors child exit code / signal into the parent process. Forwards `SIGINT` / `SIGTERM` from parent to child.
- Returns a typed `{ status, distro, exit_code, signal, hint }` shape only when `opts.returnInsteadOfMirror === true` (test-only path). The production path calls `process.exit()` on the parent so Cursor sees the right exit code.
- `status` enum: `ok`, `unsupported_host`, `unsafe_distro_name`, `no_default_distro`, `distro_not_found`, `wsl_not_found`, `no_distros`, `wsl_command_failed`, `check_timeout`, `runtime_missing`, `bridge_spawn_failed`, `bridge_child_exit`.

invariants:
- No raw stream lane added.
- No shell expansion. No `shell: true`. The `bash -lc` argument is a single constant literal; no operator input is concatenated into it.
- No `spawn(` target other than `wsl.exe` (parameterised as `opts.wslPath`).
- The shim writes NOTHING to stdout. All status messages go to stderr.
- No new file open or write.
- No network. No socket. No HTTP. No subprocess outside `wsl.exe`.
- No environment-variable echo. Token-shaped env vars are filtered out of the child's env.
- No credentials in any code path. No `sudo`. No password prompt.
- No new MCP tool. The 29-tool TC45 catalogue is unchanged.
- The two sibling shims (`terminal-commanderd.js`, `terminal-commander.js`) are byte-identical to the WWS02 / WWS03 baseline.
- No platform-package edit.
- No version bump.
- No release-please / publish workflow change.
- `npm-bootstrap-publish.yml` stays committed-but-undispatched.
- All long-lived tokens (`NPM_TOKEN_TC`, `CARGO_REGISTRY_TOKEN_TC`, `RELEASE_PLEASE_TOKEN_TC`) stay UNUSED.
- TC48 + NPM10 `Conditional Go` posture preserved.

acceptance_criteria:
- `packages/terminal-commander/lib/wsl/spawn.js` exists, exports `spawnWslBridge` (async) + named `BRIDGE_STATUSES`, `BRIDGE_PROBE_CMD`, `SECRET_ENV_PATTERNS`.
- `packages/terminal-commander/bin/terminal-commander-mcp.js` on Windows delegates to `spawnWslBridge` when the resolver returns `bridge_required`. Linux behaviour byte-identical to the WWS02 / WWS03 baseline.
- `npm test` in `packages/terminal-commander` passes ALL the following:
  - existing 70 WWS02 + WWS03 tests UNCHANGED + GREEN
  - new spawn helper tests: env-resolves-distro (priority 1), detect-default fallback (priority 2), `no_default_distro` refusal (priority 3), `assertSafeDistroName` rejection, whitelist-miss `distro_not_found`, runtime-missing gate (probeRuntime=true by default), `TC_WSL_SKIP_DOCTOR=1` bypasses the gate, argv shape (`-d <distro> -- bash -lc 'exec terminal-commander-mcp'` + forwarded userArgv), spawn options shape (`shell:false`, `windowsHide:true`, `stdio:'inherit'`), env filter strips token-shaped variables, exit-code mirroring, signal mirroring, generic spawn error -> `bridge_spawn_failed`
  - new bridge-wiring tests: terminal-commander-mcp.js under forced `win32` invokes `spawnWslBridge()` (mocked); shim writes NOTHING to stdout; bridge_required branch is reached without spawning wsl.exe in tests (mocked exec); Linux behaviour unchanged
  - new static-guard tests: spawn.js contains NO `sudo`, NO `npm install`, NO `apt-get`, NO `pacman`, NO `--install`, NO `pair`, NO `mcp.json`, NO file-write API, NO network API; spawn first argument is `wslPath` / `'wsl.exe'`; `bash -lc` second argument is `BRIDGE_PROBE_CMD` byte-for-byte; the terminal-commanderd.js + terminal-commander.js shim files are BYTE-IDENTICAL to the WWS02 / WWS03 baseline (SHA-256 lock pinned in the test)
- `npm pack --dry-run` clean for all THREE packages, file counts: root += 1 (lib/wsl/spawn.js); both platform packages unchanged.
- Resolver tests from WWS02 still pass. WSL helper tests from WWS03 still pass. Existing shim-win32-branch tests for terminal-commanderd.js + terminal-commander.js stay GREEN; the existing shim-win32-branch test for terminal-commander-mcp.js is REPLACED by the new bridge-wiring test (the old "exit 64 + pending WWS04" assertion is obsoleted by the WWS04 bridge landing).
- `crates/**` untouched; `cargo nextest run --workspace` PASS 347/347.
- runtime-smoke + npm-local-install PASS unchanged.
- MCP guard greps clean.
- secret-leak grep clean.
- `npm view` for all three names still returns E404.
- `.github/**` diff empty.
- `npm-bootstrap-publish` NOT dispatched.

evidence_required:
- Branch evidence.
- File paths.
- `npm test` PASS line count + total tests.
- `npm pack --dry-run` summary for root + linux-x64 + linux-arm64.
- `cargo nextest run --workspace` PASS count (WSL).
- `bash scripts/smoke/verify-runtime-smoke.sh` + `verify-npm-local-install.sh` PASS counts.
- MCP guard greps + secret-leak grep outputs.
- Live Windows bridge evidence (run on a Windows host with `wsl.exe` available); record `runtime_missing` honestly if the WSL runtime is not yet installed (NPM07 publish has not happened).

stop_conditions:
- Branch is not `main`.
- Working tree dirty.
- Bridge spawn would require `shell: true` or any shell escape.
- The shim would require writing to stdout outside the child's transparent passthrough.
- A new spawn target other than `wsl.exe` would be introduced.
- The two sibling shims would need to change.
- Origin/main has moved during implementation.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check

npm pack ./packages/terminal-commander --dry-run
npm pack ./packages/terminal-commander-linux-x64 --dry-run
npm pack ./packages/terminal-commander-linux-arm64 --dry-run

python3 - <<'PY'
import json
from pathlib import Path
root = json.loads(Path("packages/terminal-commander/package.json").read_text())
x64 = json.loads(Path("packages/terminal-commander-linux-x64/package.json").read_text())
arm64 = json.loads(Path("packages/terminal-commander-linux-arm64/package.json").read_text())
assert root["os"] == ["linux", "win32"], root["os"]
assert x64["os"] == ["linux"], x64["os"]
assert arm64["os"] == ["linux"], arm64["os"]
assert x64["cpu"] == ["x64"], x64["cpu"]
assert arm64["cpu"] == ["arm64"], arm64["cpu"]
versions = {root["version"], x64["version"], arm64["version"]}
assert len(versions) == 1, versions
deps = root.get("optionalDependencies", {})
assert deps.get("@terminal-commander/linux-x64") == root["version"], deps
assert deps.get("@terminal-commander/linux-arm64") == root["version"], deps
print("package-contract-ok", root["version"])
PY

( cd packages/terminal-commander && npm test )

CARGO_TARGET_DIR=target-wsl cargo metadata --no-deps
CARGO_TARGET_DIR=target-wsl cargo fmt --all --check
CARGO_TARGET_DIR=target-wsl cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR=target-wsl cargo nextest run --workspace

bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh

rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
rg "NPM_TOKEN_TC|CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|sk-[A-Za-z0-9]{10}|ghp_[A-Za-z0-9]{10}|npm_[A-Za-z0-9]{20}" README.md docs examples packages .agent/goals || true
rg "sudo|npm install|apt-get|pacman|--install|pair|mcp\.json" packages/terminal-commander/lib/wsl packages/terminal-commander/test || true

npm view terminal-commander version || true
npm view @terminal-commander/linux-x64 version || true
npm view @terminal-commander/linux-arm64 version || true
```

## Task Prompt

Run WWS04 only on branch `main`. Add `lib/wsl/spawn.js` and wire the `bridge_required` branch in `bin/terminal-commander-mcp.js`. NO `lib/config.js` (deferred to WWS06). NO `lib/cursor/*`. NO setup CLI. NO publish. NO workflow change. The two sibling shims (`terminal-commanderd.js`, `terminal-commander.js`) stay byte-identical.

## Final Report Format

- Pushed WWS03 range (confirmation only)
- Pushed WWS04 prep-amendment range (this file)
- Files changed by WWS04 (verified-work commit)
- Bridge helper API summary (`spawnWslBridge` signature + status enum)
- Windows bridge behavior table (env / detect default / failure cases)
- Linux behavior unchanged evidence
- Distro selection + safety evidence (double validation chain)
- Runtime_missing behavior evidence
- Stdio proxy evidence (`stdio: 'inherit'`, no stdout writes)
- Safety evidence: no install, no sudo, no credentials, no env-secret echo, no Cursor write
- Static-guard test summary
- Live Windows bridge smoke status: PASS / Not Run / Blocked with exact reason
- Verification summary (every gate above)
- Confirmation `npm-bootstrap-publish` was not dispatched
- Confirmation no npm publish occurred
- Confirmation WWS05 not started
- Local git state (HEAD, ahead/behind, branch)

## Binding decision carry-forward (from WWS01)

| Decision | Owner at WWS04 | What WWS04 lands |
|---|---|---|
| D-04 `terminal-commander-mcp` on Windows is a bridge shim. `spawn(wsl.exe, [-d, distro, --, bash, -lc, "exec terminal-commander-mcp"], { shell:false, windowsHide:true, stdio:'inherit' })`. Distro whitelist-validated. Argv array only. | WWS04 owns the bridge spawn. | `lib/wsl/spawn.js` `spawnWslBridge()`; wired into `bin/terminal-commander-mcp.js` Windows branch. |
| D-05 `terminal-commanderd` on Windows refuses with exit 64. | WWS02 already landed. WWS04 preserves byte-identical shim. | No edit; static-guard test pins the shim contents. |
| D-12 Multi-distro handling via persisted choice + `--distro` override. | WWS06 owns persistence. WWS04 honors operator override via `TC_WSL_DISTRO` env var. | Priority chain `TC_WSL_DISTRO` -> `detectWsl().default_distro`; refuses with `no_default_distro` if neither yields a safe + whitelisted name. |

WWS04 does NOT introduce `lib/config.js` (deferred to WWS06's `setup.json` writer), does NOT write Cursor config (D-13 belongs to WWS05), and does NOT add CLI subcommands (D-13/D-14 belong to WWS06).
