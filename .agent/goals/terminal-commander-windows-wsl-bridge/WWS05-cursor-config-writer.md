---
goal_id: WWS05
title: Cursor Config Writer
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 2 - Setup helpers
status: "Completed"
depends_on: ["WWS04"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: "2026-05-23T15:00:00+00:00"
completed_at: "2026-05-23T16:10:00+00:00"
completion_commit: "ae37878"
blocked_reason: ""
source_refs:
  - "WWS01 contract (docs/release/windows-wsl-bridge-contract.md) — §11 Cursor scope + write/refuse-if-present policy; D-13 Cursor config default --global + refuse-existing + .bak backup"
  - "WWS04 bridge spawn (commit d86e73f) — Windows entry point that operator's mcp.json should call"
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

Cursor reads MCP server configs from `~/.cursor/mcp.json` (global scope) or `<workspace>/.cursor/mcp.json` (project scope). With WWS04 landed, the simple Windows config that Cursor needs is:

```json
{
  "mcpServers": {
    "terminal-commander": {
      "type": "stdio",
      "command": "terminal-commander-mcp"
    }
  }
}
```

When the operator wants to pin a specific WSL distro (instead of letting the bridge resolve via `wsl.exe -l -v` default), an OPTIONAL `env.TC_WSL_DISTRO` is added. Distro names go through the WWS03 safety whitelist before they enter the config file.

WWS05 ships the JS-only writer + merger that produces that config WITHOUT clobbering unrelated MCP entries. Refuses to overwrite an existing `terminal-commander` entry without `force: true`. Always atomic-writes (`mcp.json.tmp` -> `rename`) and always creates a `.bak` backup before overwrite. Always preserves unrelated keys. Never prints absolute paths to stdout in a way that leaks the operator's home directory unnecessarily.

WWS05 closes WWS01 binding decision D-13 (Cursor config default `--global`, refuse-existing-terminal-commander-entry without `--force`, always `.bak` backup). The CLI surface that calls this writer (`terminal-commander setup cursor-wsl`) remains with WWS06.

## Prep amendment (2026-05-23, before WWS05 implementation)

This goal file was prep-amended once before WWS05 implementation started. Three scope adjustments were locked:

1. **Allowed-files widening**. The original allowed set covered only `lib/cursor/**`, `test/**`, `package.json` (test-script only), and the goal file. The directive widened the WWS05 implementation to also touch `packages/terminal-commander/README.md` (new WWS05 row in the Windows command table), `docs/release/windows-wsl-bridge-contract.md` (§10.2 / §18 status clarification only), `docs/integrations/cursor.md` (§11b + new "auto-generated config" subsection), `examples/provider-harness/cursor/README.md` (operator note about the auto-generated config; example JSON files stay byte-identical), `GOAL_CHAIN_INDEX.md`, and `RUN_ORDER.md` for status alignment.

2. **Stanza shape locked to the WWS04 bridge form**. The default Windows stanza is `{ "terminal-commander": { "type": "stdio", "command": "terminal-commander-mcp" } }` — i.e. Cursor invokes the WWS04 bridge shim by name on Windows. The optional `env.TC_WSL_DISTRO` field is added when the operator passed `distro` to the writer AND the name passed `assertSafeDistroName` (and, if `opts.requireKnownDistro === true`, was in the live `detectWsl().distros` whitelist). The WWS01 §4.4 / D-13 "wsl.exe direct" stanza (`{ "command": "wsl", "args": ["-d", "<distro>", "bash", "-lc", "terminal-commander-mcp"] }`) is preserved in `docs/integrations/cursor.md` §6 + `examples/provider-harness/cursor/mcp.global.linux-wsl.json` as the documented FALLBACK path for operators who do not want the bridge shim; WWS05 does NOT generate that stanza.

3. **Status enum locked**. WWS05 uses a closed enum:
   - `config_created` — new file written.
   - `config_updated` — existing file merged + written.
   - `already_exists` — `terminal-commander` entry present and `force: false`.
   - `invalid_json` — existing file failed JSON parse; not overwritten.
   - `config_too_large` — existing file > `MAX_CONFIG_BYTES` (256 KiB).
   - `path_not_allowed` — refusing to write outside the resolved Cursor scope directory.
   - `project_root_required` — project scope requested but no `project_root` provided.
   - `unsafe_distro_name` — operator-supplied distro failed the WWS03 whitelist.
   - `distro_not_found` — `requireKnownDistro: true` AND name not in `detectWsl().distros`.
   - `backup_failed` — atomic `.bak` step failed (e.g. EACCES).
   - `write_failed` — atomic rename / mkdir step failed.
   - `unsupported_host` — used for future Mac-specific guard; not currently triggered.

4. **Doctrine carry-forward**. `CAP01-capability-registry-contract.md` remains a future goal. NOT started.

## Mini-Spec

objective:
- Add `packages/terminal-commander/lib/cursor/config.js` exporting pure helpers:
  - `getCursorGlobalConfigPath(opts)` — derives `~/.cursor/mcp.json` (Linux/macOS) or `%USERPROFILE%\.cursor\mcp.json` (Windows) from injected `opts.platform` + `opts.env`. Never hardcodes a username.
  - `getCursorProjectConfigPath(projectRoot)` — `<projectRoot>/.cursor/mcp.json`; refuses missing/non-string `projectRoot`.
  - `buildTerminalCommanderServerConfig(opts)` — returns `{ type: "stdio", command: "terminal-commander-mcp", env? }`. Adds `env.TC_WSL_DISTRO` only when `opts.distro` passes safety (and optional `opts.requireKnownDistro` whitelist check).
  - `mergeCursorMcpConfig(existing, server, opts)` — merges into `existing.mcpServers["terminal-commander"]`. Refuses if `terminal-commander` already present AND `!opts.force`. Preserves every other `mcpServers` entry untouched.
  - `parseExistingCursorConfig(buffer)` — parses bytes, returns typed `{ ok: true, value }` or `{ ok: false, reason }` where reason is `invalid_json` / `config_too_large` / `bad_shape`.
  - `validateCursorConfigShape(obj)` — sanity-checks the merged config is `{ mcpServers: { [name]: stanza } }` only.
- Add `packages/terminal-commander/lib/cursor/write.js` exporting:
  - `writeCursorMcpConfig(opts)` — orchestrates load -> parse -> merge -> backup -> atomic-write. Returns a typed `{ status, path, server, hint }` record.
  - `backupCursorConfig(path, opts)` — copy `path` to `path.bak` if `path` exists; bounded.
  - Atomic write via `tmpfile in same directory + rename`.
- Add tests under `packages/terminal-commander/test/`.
- Cross-link the new writer from `packages/terminal-commander/README.md`, `docs/release/windows-wsl-bridge-contract.md` (§10.2 / §18), `docs/integrations/cursor.md` (new subsection), and `examples/provider-harness/cursor/README.md`.
- Update WWS05 frontmatter to Completed in the status commit.
- Update `GOAL_CHAIN_INDEX.md` + `RUN_ORDER.md`.

non_goals:
- NO CLI subcommand (`terminal-commander setup cursor-wsl` is WWS06).
- NO `wsl.exe` invocation. NO `child_process.spawn` AT ALL. Pure file I/O over `mcp.json`.
- NO automatic WSL runtime install. NO pairing.
- NO Cursor extension write. NO Cursor restart attempt.
- NO active `.cursor/mcp.json` committed anywhere in the repo.
- NO `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`, `scripts/**`, `.github/**`.
- NO platform-package edit.
- NO bin shim edit.
- NO MCP tool. NO daemon change. NO IPC change.
- NO publish. NO workflow dispatch. NO version bump. NO release-please change.
- NO secrets / tokens written into any generated config or any test/doc/example. The only env key the writer ever emits is `TC_WSL_DISTRO`.
- NO CAP01 implementation.

allowed_files_or_area:
- `packages/terminal-commander/lib/cursor/**`
  (new: `config.js`, `write.js`, optionally `index.js`)
- `packages/terminal-commander/test/**`
- `packages/terminal-commander/README.md` (new WWS05 entry in the Windows command table or post-table prose)
- `docs/release/windows-wsl-bridge-contract.md` (§10.2 lib/cursor entry marked landed; §18 WWS05 row)
- `docs/integrations/cursor.md` (new "auto-generated config (WWS05)" subsection; §11b WWS05 status update)
- `examples/provider-harness/cursor/README.md` (operator note about the auto-generated config; example JSON files stay byte-identical)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS05-*.md` (this file)
- `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md`
- `.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md`

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
- `packages/terminal-commander/bin/**` (byte-identical; WWS04 left them as-is)
- `packages/terminal-commander/lib/resolve-binary.js`
- `packages/terminal-commander/lib/wsl/**` (WWS03 + WWS04 contract — byte-identical; consumed via `require()` for distro safety only)
- `packages/terminal-commander/package.json`
- `examples/provider-harness/cursor/*.json` (byte-identical)
- `.cursor/mcp.json` anywhere in the repo (no active Cursor config committed)
- secrets / tokens / private paths

contracts_or_interfaces:
- `lib/cursor/config.js`: pure functions. No I/O. No spawn. No network. Imports only Node built-ins (`path`) and the WWS03 `distro-name` module for `assertSafeDistroName` / `isSafeDistroName`. MAY require `./detect.js` from `lib/wsl/` ONLY when the caller passes `opts.requireKnownDistro === true`; otherwise no live `detectWsl()` call.
- `lib/cursor/write.js`: side-effectful (file I/O only). Imports Node `fs` and `path`. NEVER imports `child_process`. NEVER calls `spawn` / `exec` / `execFile`. NEVER invokes `wsl.exe`. NEVER reads files outside the resolved Cursor scope directory.
- Atomic write: write `<path>.tmp.<random>` in the SAME directory as the final file, fsync the tmp file, then `fs.renameSync(tmp, path)`.
- Backup: if `path` exists, copy to `<path>.bak` BEFORE atomic write. Refuses if `path.bak` already exists AND the operator did not opt into `clobber_backup: true`.
- Size cap: refuse to read files > `MAX_CONFIG_BYTES = 256 * 1024` (256 KiB). Cursor's real `mcp.json` is kilobytes at most; 256 KiB is a generous defensive cap.
- Path safety: every file path the writer touches (final, tmp, bak) must be inside the resolved Cursor scope directory. Refuse otherwise with `path_not_allowed`.
- Distro safety: every `distro` argument MUST pass `assertSafeDistroName`. `opts.requireKnownDistro === true` ALSO requires membership in `detectWsl().distros`.
- The writer NEVER writes any env var beyond `TC_WSL_DISTRO`. NEVER writes secrets / tokens / API keys / passwords / credentials of any kind.
- The writer NEVER prints the resolved absolute path to stdout — only to stderr if at all; the typed result record contains `path` for callers but the writer itself does no console.log.

invariants:
- No raw stream lane added.
- No shell expansion. No `bash -lc`. No template-literal interpolation of operator input into any string that would land in `mcp.json`.
- No spawn. No exec. No network. No HTTP. No DNS.
- No file open outside the Cursor scope directory + its `.tmp.<random>` + its `.bak` sibling.
- MCP guard greps remain clean.
- TC48 + NPM10 `Conditional Go` posture preserved.
- WWS04 shim contract unchanged; the two sibling shims stay byte-identical.

acceptance_criteria:
- New JS modules under `packages/terminal-commander/lib/cursor/` exist; `npm test` passes ALL of:
  - existing 92 WWS02 + WWS03 + WWS04 tests UNCHANGED + GREEN
  - new config-builder tests: linux/native stanza; windows-bridge stanza without env; windows-bridge stanza with safe `TC_WSL_DISTRO`; rejects unsafe distro; rejects `distro_not_found` when `requireKnownDistro: true`
  - new path-resolution tests: global on Linux (XDG/HOME), global on Windows (USERPROFILE), project requires explicit `projectRoot`; refuses paths outside the resolved scope dir
  - new parse tests: invalid JSON -> `invalid_json`; over-size -> `config_too_large`; empty-buffer -> treated as new file
  - new merge tests: preserves two unrelated `mcpServers` entries; refuses existing `terminal-commander` without `force`; updates with `force: true`; round-trip load -> merge -> write -> reload yields the merged shape
  - new write tests: creates parent `.cursor/` directory if missing; backs up to `.bak` before overwrite; atomic rename uses same-directory tmp file; the writer's stdout is empty; refuses to write to a target outside the scope dir
  - new static-guard tests: `lib/cursor/**` code never `require()`s `child_process`; never calls `spawn`/`exec`/`execFile`; never references `sudo`/`npm install`/`apt-get`/`pacman`/`--install`/`pair` in executable code; never reads token-shaped env vars
  - new repo-shape test: `.cursor/mcp.json` does NOT exist anywhere in the repo (recursive check)
- `npm pack --dry-run` clean for all three packages; root file count increases by exactly the new `lib/cursor/**` files; both platform packages unchanged.
- `crates/**` untouched; `cargo nextest run --workspace` PASS 347/347.
- runtime-smoke + npm-local-install PASS unchanged.
- MCP guard greps clean.
- secret-leak grep clean (writer code names no token values).
- `npm view` E404 for all three names.
- `.github/**` diff empty.
- `npm-bootstrap-publish` NOT dispatched.

evidence_required:
- Branch evidence.
- File paths of every added / modified file.
- `npm test` PASS count + total tests.
- `npm pack --dry-run` summaries.
- `cargo nextest run --workspace` PASS count.
- runtime-smoke + npm-local-install PASS counts.
- MCP guard grep + secret-leak grep outputs.
- Round-trip evidence: load existing config with two unrelated servers -> add `terminal-commander` -> reload -> all three stanzas present.
- Live evidence: `writeCursorMcpConfig` against a temp dir (NOT the real Cursor user config). Mark "Not Run" on the operator's real config write — that step is WWS06's CLI surface concern.

stop_conditions:
- Branch is not `main`.
- Working tree dirty.
- The writer would need to `require('child_process')` or spawn anything.
- The writer would need to read or modify any file outside the Cursor scope directory.
- The writer would need to write any env key other than `TC_WSL_DISTRO`.
- The writer would need to delete the operator's existing `mcp.json.bak` without explicit opt-in.
- An active `.cursor/mcp.json` would be committed anywhere in the repo.
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

CARGO_TARGET_DIR=target-wsl cargo metadata --no-deps
CARGO_TARGET_DIR=target-wsl cargo fmt --all --check
CARGO_TARGET_DIR=target-wsl cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR=target-wsl cargo nextest run --workspace

bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh

rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
rg "NPM_TOKEN_TC|CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|sk-[A-Za-z0-9]{10}|ghp_[A-Za-z0-9]{10}|npm_[A-Za-z0-9]{20}" README.md docs examples packages .agent/goals || true
rg "sudo|npm install|apt-get|pacman|--install|pair|credential|password" packages/terminal-commander/lib/cursor packages/terminal-commander/test || true

npm view terminal-commander version || true
npm view @terminal-commander/linux-x64 version || true
npm view @terminal-commander/linux-arm64 version || true
```

## Task Prompt

Run WWS05 only on branch `main`. Cursor config reader + merger + atomic writer + backup helper under `lib/cursor/`. NO `child_process` import. NO spawn. NO wsl.exe. NO network. Default stanza targets the WWS04 bridge shim (`command: terminal-commander-mcp`); the `wsl.exe`-direct stanza stays in `docs/integrations/cursor.md` §6 + `examples/provider-harness/cursor/mcp.global.linux-wsl.json` as a manual fallback ONLY.

## Final Report Format

- Pushed WWS04 range (confirmation only)
- Pushed WWS05 prep-amendment range (this file)
- Files changed by WWS05 (verified-work commit)
- Cursor config writer API summary
- Config shape table (Linux native; Windows bridge default; Windows bridge with TC_WSL_DISTRO)
- Merge/backup/force behavior evidence
- Global/project path behavior
- Distro env validation evidence
- Round-trip preserve-unrelated test evidence
- Safety evidence: no active `.cursor/mcp.json`, no secrets, no install, no sudo, no pairing, no bridge spawn changes
- Live Windows config-write status: temp-dir PASS; real Cursor config NOT modified (deferred to WWS06 CLI surface)
- Verification summary
- Confirmation `npm-bootstrap-publish` was not dispatched
- Confirmation no npm publish occurred
- Confirmation WWS06 not started
- Local git state (HEAD, ahead/behind, branch)

## Binding decision carry-forward (from WWS01)

| Decision | Owner at WWS05 | What WWS05 lands |
|---|---|---|
| D-13 Cursor config default `--global`; `--project` opts into workspace scope; refuse-existing-terminal-commander-entry without `--force`; always `.bak` backup. | WWS05 owns the pure writer + path helpers. WWS06 owns the `--global` / `--project` / `--force` flag surface. | `getCursorGlobalConfigPath`, `getCursorProjectConfigPath`, `mergeCursorMcpConfig` refuse-existing default, `backupCursorConfig` always-runs-before-overwrite. |
| D-14 Rollback / uninstall via `setup cursor-wsl --uninstall` (restores `mcp.json.bak`) BEFORE `npm uninstall`. | WWS06 owns the uninstall flow. WWS05 only guarantees the `.bak` exists; restore is a separate operation. | `backupCursorConfig` produces `.bak`; no uninstall path at WWS05. |

WWS05 does NOT decide D-07 (prompting UX), does NOT touch `setup.json` (WWS06), and does NOT run any `wsl.exe` probe (WWS04 bridge spawn is consumed by the OPERATOR's Cursor instance, not by WWS05 directly).
