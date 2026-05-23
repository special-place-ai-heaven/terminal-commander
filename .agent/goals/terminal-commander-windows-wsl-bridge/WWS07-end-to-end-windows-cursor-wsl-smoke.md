---
goal_id: WWS07
title: End To End Windows Cursor Wsl Smoke
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 3 - Verification
status: "Completed"
depends_on: ["WWS06"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: "2026-05-23T18:00:00+00:00"
completed_at: "2026-05-23T19:10:00+00:00"
completion_commit: "785d410"
blocked_reason: ""
source_refs:
  - "WWS04 bridge shim (commit d86e73f)"
  - "WWS05 cursor config writer (commit ae37878)"
  - "WWS06 setup cursor-wsl orchestrator (commit 4936904)"
  - "docs/integrations/cursor.md (NPM08 baseline + WWS04/WWS05/WWS06 updates)"
  - "scripts/smoke/verify-runtime-smoke.sh (Linux-side smoke reference)"
  - "scripts/smoke/verify-npm-local-install.sh (NPM04 local-tarball reference)"
risk_level: "medium"
---

# WWS07 - End To End Windows Cursor Wsl Smoke

## Branch Guard

```text
main
```

## Mission Context

WWS07 is the chain's verification goal. It runs the composed Windows -> WSL MCP bridge end-to-end on the verification host and produces the structured smoke evidence that WWS08's docs + WWS09's readiness review consume.

Direct bridge smoke (scriptable):

1. Verify `wsl.exe` is reachable.
2. Run the WWS06 CLI surface (`terminal-commander doctor`, `terminal-commander doctor wsl`, `terminal-commander setup cursor-wsl --print-config`, `terminal-commander setup cursor-wsl --dry-run`). Each step asserts the expected bounded status (no spawn, no install, no Cursor config write).
3. If the WSL-side `terminal-commander-mcp` is installed in the chosen distro, drive a single MCP `initialize` + `tools/list` + `health` round-trip THROUGH the Windows `terminal-commander-mcp` shim (i.e. through the WWS04 bridge into the WSL distro) and assert: rmcp framing intact, 29 tools advertised, `health` returns ok-shape, shim writes nothing extra to stdout.
4. If the WSL-side runtime is NOT installed, record `runtime_missing` honestly. Do NOT install unless the explicit `-InstallWslRuntime` flag is supplied AND the npm package is published (currently E404; therefore `--install-wsl-runtime` is expected to return `npm_package_unpublished`).

Cursor provider smoke (operator-driven):

1. Operator launches Cursor on Windows AFTER the bridge smoke has produced a valid `mcp.json` via `terminal-commander setup cursor-wsl`.
2. Operator confirms `terminal-commander` MCP server appears in `Settings -> Features -> MCP` with the 29-tool catalogue.
3. Operator asks Cursor to call `health` from the chat panel; pastes the chat transcript into a follow-up evidence file.

Cursor's MCP discovery flow has no documented headless / scripted entry point today. There is no `cursor --list-mcp-tools` subcommand. WWS07 therefore records the Cursor provider smoke status honestly:
- PASS only if an operator transcript is attached at WWS07 evidence-collection time.
- Not Run otherwise, with the exact reason recorded.

WWS09 (readiness review) reads the WWS07 evidence and promotes the beta posture from `Conditional Go` to `Go` only if at least one provider live smoke (Cursor OR an alternate provider) is PASS.

## Prep amendment (2026-05-23, before WWS07 implementation)

This goal file was prep-amended once before WWS07 implementation started. Three scope adjustments were locked:

1. **Allowed-files widening**. The original allowed set covered only `scripts/smoke/verify-windows-bridge-smoke.ps1`, `docs/integrations/cursor.md`, `docs/release/**`, and the goal file. The directive widened to also touch `packages/terminal-commander/README.md` (new WWS07 row in the Windows command table or chain status line), `examples/provider-harness/cursor/README.md` (operator note about the PowerShell smoke), `GOAL_CHAIN_INDEX.md`, and `RUN_ORDER.md` for status alignment. The repo-level `README.md`, `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`, `.github/**`, all platform packages, all package.json files, `examples/.../*.json`, the WWS03/WWS04/WWS05/WWS06 source files under `lib/wsl/`, `lib/cursor/`, `lib/cli/`, the three bin shims, and `lib/resolve-binary.js` stay UNTOUCHED.

2. **PowerShell smoke script API locked**.
   - File: `scripts/smoke/verify-windows-bridge-smoke.ps1`
   - Shebang/comment-block: PowerShell 5.1+ compatible; `Set-StrictMode -Version Latest`; `$ErrorActionPreference = 'Stop'`.
   - Parameters (all optional; all defaulting):
     - `-DryRun` (switch): print planned actions and exit 0 without writing anything.
     - `-Distro <string>` (string): operator-supplied WSL distro override. Validated against the WWS03 safety whitelist via the CLI (`terminal-commander setup cursor-wsl --print-config --distro <name>`); rejects shell metachars.
     - `-InstallWslRuntime` (switch): forwards `--install-wsl-runtime` to the CLI. The script does NOT install anything itself.
     - `-SkipCursorWrite` (switch, default): when set, never invokes `setup cursor-wsl` without `--dry-run` / `--print-config`. The script defaults to NOT touching the operator's real `mcp.json`; only the dry-run + print-config + doctor probes run by default.
     - `-WriteCursorConfig` (switch): explicitly opt into running `terminal-commander setup cursor-wsl` (real write). Off by default; the operator must pass this AND know the implications.
     - `-TempCursorScope` (switch): instead of `--global`, run setup against a temp `--project <tempdir>/.cursor/mcp.json` so the real Cursor config is never touched. On by default whenever `-WriteCursorConfig` is supplied (to keep the smoke safe).
   - Output: bounded `PASS  <step>` / `FAIL  <step>` lines, one per check. Mirrors the NPM04 smoke style.
   - Exit codes: 0 on overall success (including honest `runtime_missing` if `-InstallWslRuntime` was NOT supplied), non-zero on any unexpected step failure.
   - No sudo. No password prompt. No env credential. No publish.

3. **`runtime_missing` is acceptable, not PASS**. The PowerShell script reports `runtime_missing` honestly when the WSL distro lacks `terminal-commander-mcp` (which is the current state until NPM07 first-publish lands). The MCP bridge round-trip is then RECORDED as Not Run with `runtime_missing` as the blocker; the script does NOT exit non-zero for this honest gap because the WWS07 acceptance criterion is honest evidence, not green-on-all-counts. WWS09 will gate the beta-posture promotion on the eventual MCP round-trip PASS.

4. **Doctrine carry-forward**. `CAP01-capability-registry-contract.md` remains a future goal. NOT started.

## Mini-Spec

objective:
- Add `scripts/smoke/verify-windows-bridge-smoke.ps1`.
- Cross-link from `docs/integrations/cursor.md` (new "Windows bridge smoke (WWS07)" subsection — operator-driven GUI checklist + non-GUI bridge smoke).
- Cross-link from `docs/release/windows-wsl-bridge-contract.md` (§10.2 / §18 mark WWS07 landed; §16 evidence policy unchanged).
- Cross-link from `packages/terminal-commander/README.md` (WWS chain status line).
- Cross-link from `examples/provider-harness/cursor/README.md` (operator note about the smoke).
- Update WWS07 frontmatter to Completed in the status commit.
- Update `GOAL_CHAIN_INDEX.md` + `RUN_ORDER.md`.

non_goals:
- NO `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`, `.github/**`.
- NO daemon change. NO IPC change. NO MCP tool change. NO Cursor config writer change. NO bridge spawn change. NO setup CLI change.
- NO publish. NO workflow dispatch. NO version bump.
- NO active `.cursor/mcp.json` committed anywhere in the repo.
- NO sudo. NO password. NO credential broker.
- NO secret / token / env value written into the smoke script.
- NO claim that the Cursor GUI smoke passed without a captured transcript.
- NO promotion of `Not Run` evidence to PASS.

allowed_files_or_area:
- `scripts/smoke/verify-windows-bridge-smoke.ps1` (new)
- `docs/integrations/cursor.md` (new "Windows bridge smoke (WWS07)" subsection)
- `docs/release/windows-wsl-bridge-contract.md` (§10.2 / §18 status only; no binding decision change)
- `packages/terminal-commander/README.md` (WWS chain status line; one-line WWS07 mention)
- `examples/provider-harness/cursor/README.md` (operator note)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS07-*.md` (this file)
- `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md`
- `.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md`

forbidden_files:
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `.github/**`
- `packages/terminal-commander-linux-x64/**`
- `packages/terminal-commander-linux-arm64/**`
- `packages/terminal-commander/package.json`
- `packages/terminal-commander/bin/**` (byte-identical to WWS06)
- `packages/terminal-commander/lib/wsl/**` (byte-identical)
- `packages/terminal-commander/lib/cursor/**` (byte-identical)
- `packages/terminal-commander/lib/cli/**` (byte-identical)
- `packages/terminal-commander/lib/resolve-binary.js` (byte-identical)
- `examples/provider-harness/cursor/*.json` (byte-identical)
- `.cursor/mcp.json` anywhere in the repo
- secrets / tokens / private paths

contracts_or_interfaces:
- PowerShell script is idempotent, non-interactive, exits 0 on overall success.
- The script's only spawn surface is `node` (for the JS CLI shim) and `wsl.exe` (indirectly, when the CLI invokes the bridge). The script itself does NOT call `child_process` / `wsl.exe` directly except via the CLI.
- The script prints structured `PASS  <step>` / `FAIL  <step>` lines.
- The script does NOT touch the operator's real `~/.cursor/mcp.json` / `%USERPROFILE%\.cursor\mcp.json` by default. `-WriteCursorConfig` is required AND `-TempCursorScope` is on by default whenever `-WriteCursorConfig` is supplied.
- The script does NOT install anything globally on the host. The CLI's `--install-wsl-runtime` is gated by the `-InstallWslRuntime` switch AND is expected to return `npm_package_unpublished` until NPM07's first publish.
- The script never echoes the contents of any env var; it only reports presence / absence by key.

invariants:
- Cursor live GUI smoke is honestly `Not Run` unless an operator transcript is attached.
- `Not Run` is NOT PASS.
- No bridge invocation that involves shell interpolation. The CLI already enforces this via WWS04 argv shape; WWS07 must not bypass.
- TC48 + NPM10 `Conditional Go` posture preserved unless a real Cursor transcript lands at WWS09.
- MCP guard greps remain clean.

acceptance_criteria:
- `scripts/smoke/verify-windows-bridge-smoke.ps1` exists, is non-interactive, prints structured PASS/FAIL lines.
- `docs/integrations/cursor.md` carries the WWS07 smoke checklist (new subsection).
- The Cursor GUI smoke status is honestly recorded.
- All existing tests stay GREEN (234/234). `npm pack` dry-runs clean. `cargo nextest` 347/347. Linux smokes unchanged.
- MCP guard greps clean. Secret-leak grep clean. `npm view` E404.
- `npm-bootstrap-publish` NOT dispatched.

evidence_required:
- Branch evidence.
- File paths.
- PowerShell smoke output (or recorded Not Run blocker if Windows shell + WSL host is unavailable).
- Cursor GUI smoke status (PASS with transcript OR Not Run with exact reason).

stop_conditions:
- Branch is not `main`.
- Working tree dirty.
- The smoke would require introducing a shell bridge.
- The smoke would require the operator's password.
- Origin/main has moved during implementation.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
test ! -e .cursor/mcp.json

( cd packages/terminal-commander && npm test )

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

CARGO_TARGET_DIR=target-wsl cargo fmt --all --check
CARGO_TARGET_DIR=target-wsl cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR=target-wsl cargo nextest run --workspace
bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh

rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
rg "NPM_TOKEN_TC|CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|sk-[A-Za-z0-9]{10}|ghp_[A-Za-z0-9]{10}|npm_[A-Za-z0-9]{20}" README.md docs examples packages .agent/goals scripts || true
rg "sudo -S|PASSWORD|PASSWD|credential broker|credential_broker|npm publish|workflow_dispatch" packages/terminal-commander/lib packages/terminal-commander/bin packages/terminal-commander/test scripts || true

# Windows-only:
# powershell -ExecutionPolicy Bypass -File scripts/smoke/verify-windows-bridge-smoke.ps1 -DryRun
# powershell -ExecutionPolicy Bypass -File scripts/smoke/verify-windows-bridge-smoke.ps1

npm view terminal-commander version || true
npm view @terminal-commander/linux-x64 version || true
npm view @terminal-commander/linux-arm64 version || true
```

## Task Prompt

Run WWS07 only on branch `main`. PowerShell smoke script + Cursor doc extension only. NO `crates/**`. NO daemon change. NO publish. The Cursor GUI smoke is operator-driven; record `Not Run` honestly when no transcript is attached.

## Final Report Format

- Pushed WWS06 range (confirmation only)
- Pushed WWS07 prep-amendment range (this file)
- Files changed by WWS07 (verified-work commit)
- Smoke script summary (parameters + PASS/FAIL line format + exit semantics)
- Windows CLI smoke result table (`doctor` / `doctor wsl` / `setup --print-config` / `setup --dry-run`)
- Windows -> WSL MCP bridge result table (initialize / tools/list / health round-trip OR `runtime_missing` honestly)
- Cursor provider smoke status: PASS (with transcript) / Not Run / Blocked with exact reason
- WSL runtime install path used: none / local tarball / existing runtime / blocked
- Whether real Cursor config was touched (default: no; only the PowerShell `-WriteCursorConfig -TempCursorScope` path can write, and that defaults to a temp scope)
- Safety evidence: no sudo, no credentials, no npm publish, no workflow dispatch, no real Cursor config touched
- Verification summary (all gates above)
- Confirmation `npm-bootstrap-publish` was not dispatched
- Confirmation no npm publish occurred
- Confirmation WWS08 not started
- Local git state (HEAD, ahead/behind, branch)

## Binding decision carry-forward (from WWS01)

| Decision | Owner at WWS07 | What WWS07 lands |
|---|---|---|
| §16 Evidence policy: `Not Run` is honest; no promotion to PASS without transcript. | WWS07 collects the evidence. | PowerShell smoke records `runtime_missing` honestly when the WSL-side runtime is absent; Cursor GUI smoke is `Not Run` unless an operator transcript is attached. |
| §14.1 Publish floor (recommended): keep `npm-bootstrap-publish` PAUSED until WWS08 + WWS09. | WWS07 reaffirms. | Smoke script never invokes publish. |
| D-15 Publish-readiness floor: WWS02 + WWS04 + WWS05 + WWS06 + WWS08 land before first publish. | WWS09 reconfirms. | WWS07's evidence is the input to WWS09's promotion decision. |
