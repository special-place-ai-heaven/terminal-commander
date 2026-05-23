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
  - "WWS01 contract (docs/release/windows-wsl-bridge-contract.md) — §7 distro selection, §8 install policy, §9 pairing posture, §11 setup-state file layout, §15 D-07 / D-08 / D-09 / D-10 / D-12 / D-13 / D-14"
  - "WWS03 detect / doctor / distro-name helpers (commit ec8441e)"
  - "WWS04 bridge spawn helper (commit d86e73f)"
  - "WWS05 cursor config writer (commit ae37878)"
risk_level: "high"
---

# WWS06 - Wsl Runtime Install Or Pairing Flow

## Branch Guard

```text
main
```

## Mission Context

WWS06 wires the already-landed WWS03 / WWS04 / WWS05 helpers into the `terminal-commander` CLI subcommand surface. The four new subcommands are the operator-facing UX for the entire WWS chain:

- `terminal-commander setup cursor-wsl [--distro <name>] [--global|--project <path>] [--force] [--clobber-backup] [--print-config] [--dry-run] [--install-wsl-runtime]`
- `terminal-commander doctor` (Windows host) and `terminal-commander doctor wsl [--distro <name>] [--probe-runtime]`
- `terminal-commander pair create [--distro <name>]`
- `terminal-commander pair accept <code>`

Pairing is OPTIONAL. The default WSL setup relies on `wsl.exe -l -v` discovery + the WWS04 bridge spawn — the 6-digit pairing flow exists only as an operator confirmation / anti-misconfiguration aid, not a security secret. At WWS06 `pair create` persists a bounded `pair.json` under `%LOCALAPPDATA%\terminal-commander\` (no secret values, only the 6-digit code + timestamp); `pair accept` validates the code shape and returns `pair_deferred` — the true handshake is out of scope at WWS06 and is recorded as a future enhancement.

WWS06 closes WWS01 binding decisions D-07 (default-distro pick / ask-once / refuse on `--no-interactive`), D-08 (automatic WSL install requires explicit `--install-wsl-runtime`), D-09 (pairing optional, codes are operator confirmation), D-10 (Windows-side state at `%LOCALAPPDATA%\terminal-commander\setup.json`), D-12 (multi-distro persisted choice + `--distro` override), D-14 (rollback via the `.bak` written by WWS05).

## Prep amendment (2026-05-23, before WWS06 implementation)

This goal file was prep-amended once before WWS06 implementation started. Four scope adjustments were locked:

1. **Allowed-files widening**. The original allowed set covered `lib/cli/**`, `bin/terminal-commander.js`, `test/**`, `package.json`, `docs/install/**`, and the goal file. The `docs/install/**` directory does not exist in the repo; the directive replaces it with `docs/integrations/cursor.md` (the canonical Cursor walk-through), `docs/release/windows-wsl-bridge-contract.md` (§10.2 / §18 / §15 status clarification only — no binding decision change), `packages/terminal-commander/README.md` (new WWS06 row in the Windows command table), `examples/provider-harness/cursor/README.md` (operator note about the new CLI path), `GOAL_CHAIN_INDEX.md`, and `RUN_ORDER.md`. The repo-level `README.md` stays untouched.

2. **CLI surface locked**. The four subcommands above, plus their `--help` panels. Unknown flags exit non-zero with bounded usage. `--global` and `--project <path>` are mutually exclusive (defaulting to `--global`). `--dry-run` short-circuits before any file I/O or `wsl.exe` invocation beyond `detectWsl`. `--print-config` prints the JSON stanza that would be written and exits 0 without touching the filesystem.

3. **`--install-wsl-runtime` safety boundary**. The flag opts into ONE constant install command issued through the WWS04 `spawnWslBridge` argv shape:
   ```
   wsl.exe -d <distro> -- bash -lc 'npm install -g terminal-commander'
   ```
   The `bash -lc` argument is a literal constant; the distro name is the only operator-supplied value passed to `wsl.exe`, and it goes through `assertSafeDistroName` + live whitelist membership BEFORE the spawn. **NO sudo. NO `sudo -S`. NO password prompt. NO password env var. NO LLM-supplied credential. NO automatic privilege escalation.** If the inside-WSL `npm install -g` fails with a permission error (EACCES / `please use sudo` text / non-zero exit while running as non-root user), WWS06 reports `install_permission_required` + a bounded operator-readable hint and exits 64. It does NOT retry under sudo, does NOT pass a password by env, does NOT prompt for credentials in any channel. If the npm package is still unpublished (E404), the install path returns `npm_package_unpublished` honestly. NPM07's first publish unblocks this code path; until then, `--install-wsl-runtime` is implemented but expected to return `npm_package_unpublished` in practice.

4. **Pairing implemented as deferred handshake**. `pair create` generates a 6-digit code via `crypto.randomInt(100000, 999999)`, persists it to `%LOCALAPPDATA%\terminal-commander\pair.json` with `{ schema_version, pair_id (uuid v7), code, created_at, distro? }`, and prints the code to the operator. `pair accept <code>` validates the input is a 6-digit string AND that the code matches the persisted `pair.json`; on match, returns status `pair_accepted` and updates `pair.json` with `accepted_at`. The full WSL-side handshake (mounting the Windows pair file via `wsl.exe` UNC paths, exchanging a session token with the daemon) is **DEFERRED** — at WWS06, the Windows-side `pair accept` returns `pair_deferred` whenever the operator runs it from a non-Windows host, because the canonical handshake requires the WSL daemon to be running and a true session protocol that does not yet exist. The acceptance code stays on disk as audit/replay evidence. No network. No credential. No environment variable. The `pair.json` file contains ONLY the bounded metadata listed above; no token, no env, no command history.

5. **Doctrine carry-forward**. `CAP01-capability-registry-contract.md` remains a future goal. NOT started at WWS06.

## Mini-Spec

objective:
- Add `packages/terminal-commander/lib/cli/parser.js` (small dependency-free argv parser; rejects unknown flags; emits usage text).
- Add `packages/terminal-commander/lib/cli/setup_cursor_wsl.js` (orchestrator).
- Add `packages/terminal-commander/lib/cli/doctor.js` (structured diagnostics).
- Add `packages/terminal-commander/lib/cli/pair_create.js` + `pair_accept.js` (deferred handshake).
- Add `packages/terminal-commander/lib/cli/setup_state.js` (bounded `setup.json` read/write under `%LOCALAPPDATA%\terminal-commander\`; reuses WWS05 `atomicWrite` patterns).
- Add `packages/terminal-commander/lib/cli/run.js` (entry-point that the bin shim delegates to on Windows).
- Extend `packages/terminal-commander/bin/terminal-commander.js` Windows branch ONLY to delegate to `lib/cli/run.js`. Linux branch stays byte-identical (the resolver still returns `ok` on Linux and the shim spawns the resolved Rust admin CLI).
- Add tests under `packages/terminal-commander/test/`.
- Cross-link the new CLI from `packages/terminal-commander/README.md`, `docs/release/windows-wsl-bridge-contract.md` (§10.2 / §18), `docs/integrations/cursor.md` (new "WWS06 setup CLI" subsection), and `examples/provider-harness/cursor/README.md`.
- Update WWS06 frontmatter to Completed in the status commit.
- Update `GOAL_CHAIN_INDEX.md` + `RUN_ORDER.md`.

non_goals:
- NO automatic install of WSL itself. Operator must run `wsl --install` separately.
- NO sudo. NO password prompt. NO credential broker. NO password env passthrough. NO LLM-supplied credential. NO automatic privilege escalation.
- NO PowerShell scripts. JS only.
- NO `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`, `scripts/**`, `.github/**`.
- NO platform-package edit.
- NO MCP tool. NO daemon change. NO IPC change.
- NO publish. NO workflow dispatch. NO version bump. NO release-please change.
- NO new spawn target beyond `wsl.exe` (already owned by `lib/wsl/spawn.js`).
- NO active `.cursor/mcp.json` committed anywhere in the repo.
- NO secret / token / password / API key written into `setup.json` or `pair.json`.
- NO full WSL-side `pair accept` handshake (deferred).
- NO end-to-end MCP smoke through the bridge — that is WWS07.
- NO CAP01 implementation.

allowed_files_or_area:
- `packages/terminal-commander/lib/cli/**`
  (new: `parser.js`, `setup_cursor_wsl.js`, `doctor.js`, `pair_create.js`, `pair_accept.js`, `setup_state.js`, `run.js`, optionally `index.js`)
- `packages/terminal-commander/bin/terminal-commander.js`
  (Windows branch extension only — delegates to `lib/cli/run.js`; Linux branch byte-identical)
- `packages/terminal-commander/test/**`
- `packages/terminal-commander/README.md` (new WWS06 row in the Windows command table + post-table prose)
- `docs/release/windows-wsl-bridge-contract.md` (§10.2 / §18 / §15 status clarification only; no binding decision change)
- `docs/integrations/cursor.md` (new "WWS06 setup CLI" subsection + §11b status update)
- `examples/provider-harness/cursor/README.md` (operator note about the new CLI path)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS06-*.md` (this file)
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
- `packages/terminal-commander/bin/terminal-commanderd.js` (byte-identical)
- `packages/terminal-commander/bin/terminal-commander-mcp.js` (byte-identical — WWS04 contract preserved)
- `packages/terminal-commander/lib/resolve-binary.js` (byte-identical)
- `packages/terminal-commander/lib/wsl/**` (byte-identical — consumed via `require()`)
- `packages/terminal-commander/lib/cursor/**` (byte-identical — consumed via `require()`)
- `packages/terminal-commander/package.json`
- `examples/provider-harness/cursor/*.json` (byte-identical)
- `.cursor/mcp.json` anywhere in the repo
- secrets / tokens / private paths

contracts_or_interfaces:
- `lib/cli/parser.js`: pure argv parser. `parseArgv(argv, spec)` -> `{ ok, command, flags, positional, error?, usage? }`. Spec declares valid commands / subcommands / flags + their kinds (boolean / string / required-arg). Rejects unknown flags. NO `child_process`. NO file I/O.
- `lib/cli/setup_cursor_wsl.js`: orchestrator. Calls `lib/wsl/detect.js`, `lib/wsl/doctor.js`, `lib/cursor/config.js`, `lib/cursor/write.js`, `lib/wsl/spawn.js` (ONLY when `--install-wsl-runtime` is set). NEVER calls `child_process` directly; every WSL invocation goes through the WWS04 spawn helper. Returns a typed `{ status, distro?, config_path?, backup_path?, hint }`.
- `lib/cli/doctor.js`: structured diagnostics. Calls `lib/wsl/detect.js` + optionally `lib/wsl/doctor.js({ probeRuntime })`. Pure JSON-printable result; no spawn beyond `detectWsl`.
- `lib/cli/pair_create.js`: `crypto.randomInt(100000, 999999)` + UUID v7. Persists to `%LOCALAPPDATA%\terminal-commander\pair.json` via `lib/cli/setup_state.js`. Prints the code to operator.
- `lib/cli/pair_accept.js`: validates `code` is a 6-digit string. Reads `pair.json`; if matched, updates `accepted_at` and returns `pair_accepted`. Otherwise returns `pair_deferred` (full handshake out of scope).
- `lib/cli/setup_state.js`: bounded JSON read/write under `%LOCALAPPDATA%\terminal-commander\`. Uses the same `atomicWrite` pattern as WWS05 (tmp-in-same-dir + rename; refuses paths outside scope dir). Schema:
  ```json
  {
    "schema_version": 1,
    "distro": "Ubuntu-24.04",
    "cursor_scope": "global",
    "created_at": "<iso8601>",
    "updated_at": "<iso8601>"
  }
  ```
  `pair.json`:
  ```json
  {
    "schema_version": 1,
    "pair_id": "<uuid v7>",
    "code": "<6-digit string>",
    "created_at": "<iso8601>",
    "accepted_at": "<iso8601|null>",
    "distro": "<safe distro name|null>"
  }
  ```
  NO token. NO env. NO command history. NO secret.
- `lib/cli/run.js`: dispatches `terminal-commander <subcommand>`. On Windows, reads `process.argv` and routes to the appropriate handler. Returns a typed result that the bin shim turns into stderr text + exit code.
- Every `wsl.exe` invocation goes through `lib/wsl/spawn.js` (WWS04 owner). NO direct `child_process.spawn(wsl.exe, ...)` from `lib/cli/**`.
- `bin/terminal-commander.js` Windows branch ONLY changes: instead of unconditional bridge-required refusal, it now calls `require('../lib/cli/run.js').run()` and exits with the returned code. Linux branch is byte-identical.

invariants:
- No new MCP tool, no IPC surface change.
- No `--no-confirm` flag for the automatic WSL install path. Operator MUST opt in via `--install-wsl-runtime`.
- `setup cursor-wsl` NEVER overwrites a non-terminal-commander MCP entry (WWS05 merge preserves them); refuses to overwrite an existing terminal-commander entry without `--force` (WWS05 contract preserved).
- Pair codes are NOT treated as cryptographic secrets in any decision. They are operator confirmation.
- NO sudo. NO password prompt. NO credential capture. NO password env var. NO LLM-supplied secret forwarded through MCP / chat / bucket / log / audit / env / Cursor config / setup.json / pair.json.
- NO network listener. NO raw stream endpoint. NO new postinstall behaviour.
- NO file write outside `%LOCALAPPDATA%\terminal-commander\` (Windows) / `$XDG_STATE_HOME/terminal-commander/` (Linux test fallback) / the resolved Cursor scope dir (delegated to WWS05).
- TC48 + NPM10 `Conditional Go` posture preserved.

acceptance_criteria:
- `npm test` passes ALL of:
  - existing 154 WWS02-WWS05 tests UNCHANGED + GREEN
  - new parser tests: --help routing, unknown-flag refusal, subcommand routing for `doctor` / `doctor wsl` / `setup cursor-wsl` / `pair create` / `pair accept <code>`, mutual exclusion of `--global` / `--project`
  - new doctor tests: `doctor wsl` returns structured JSON / human text; reads detectWsl + optionally wslDoctor; never writes files; never invokes spawn beyond `lib/wsl/spawn.js`
  - new setup tests: `--dry-run` writes nothing and returns a planned-actions summary; `--print-config` prints exactly the WWS05 stanza JSON and exits without writing; `--distro` overrides default; unsafe distro rejected before any spawn; runtime missing returns `runtime_missing` with the inside-WSL install hint; runtime present invokes WWS05 writer (mock); `--force` and `--clobber-backup` pass through to WWS05 writer; `--install-wsl-runtime` constructs the constant `wsl.exe -d <distro> -- bash -lc 'npm install -g terminal-commander'` argv (verified by mocked spawn capture); install permission failure maps to `install_permission_required`; npm E404 maps to `npm_package_unpublished`
  - new pair tests: `pair create` generates a 6-digit code, persists `pair.json` with the bounded schema, prints the code; `pair accept <code>` rejects non-6-digit input; mismatched code returns `pair_deferred`; matching code returns `pair_accepted` and updates `accepted_at`
  - new state tests: `setup_state.js` atomicWrite + refuse-outside-scope, refuses non-JSON / over-size, never serializes a key matching `*_TOKEN` / `*_SECRET` / `*_PASSWORD` / `*_API_KEY` / `*_PASS` / `credential`
  - new static-guard tests:
    - `lib/cli/**` does NOT require `child_process` (every spawn flows through `lib/wsl/spawn.js`)
    - `lib/cli/**` does NOT reference `sudo`, `sudo -S`, `password`, `PASSWORD`, `PASSWD`, `npm publish`, `workflow_dispatch`, `credential broker` in executable code
    - `lib/cli/**` does NOT read token-shaped env vars
    - `lib/cli/**` does NOT write any file outside `%LOCALAPPDATA%\terminal-commander\` (Windows) or test scope dir
    - `bin/terminal-commander-mcp.js` + `bin/terminal-commanderd.js` are BYTE-IDENTICAL to the WWS04 baseline
    - `lib/wsl/**` + `lib/cursor/**` + `lib/resolve-binary.js` are BYTE-IDENTICAL to the WWS05 baseline
    - no `.cursor/mcp.json` anywhere in the repo
- `npm pack --dry-run` clean for all three packages; root file count = WWS05 baseline + new `lib/cli/**` files.
- `crates/**` untouched; `cargo nextest run --workspace` PASS 347/347.
- runtime-smoke + npm-local-install PASS unchanged.
- MCP guard greps clean.
- secret-leak grep clean.
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
- `--help` output for `terminal-commander setup cursor-wsl --help` + `terminal-commander doctor wsl --help`.
- Live Windows evidence:
  - `terminal-commander doctor wsl` against the verification machine (record actual distro list).
  - `terminal-commander setup cursor-wsl --dry-run --print-config` (no writes; prints the planned config).
  - `terminal-commander setup cursor-wsl --dry-run` (no writes; prints planned actions).
  - `terminal-commander setup cursor-wsl` against a temp HOME (uses WWS05 writer end-to-end) — runtime probe will return `runtime_missing` honestly (NPM07 publish has not happened).
  - `terminal-commander pair create` (writes pair.json under a TEMP `LOCALAPPDATA`, never the operator's real one).
  - `terminal-commander pair accept <bogus>` -> `pair_deferred`.
  - `terminal-commander setup cursor-wsl --install-wsl-runtime` is NOT exercised live; tests use a mocked spawn capture.

stop_conditions:
- Branch is not `main`.
- Working tree dirty.
- A CLI subcommand would require shell interpolation.
- A subcommand would call `child_process` directly (must go through `lib/wsl/spawn.js`).
- A subcommand would require running `sudo`, `sudo -S`, or any privilege escalation.
- A subcommand would store a token / password / credential in `setup.json` or `pair.json`.
- A `.cursor/mcp.json` would be committed anywhere in the repo.
- The Linux behaviour of `bin/terminal-commander.js` would change.
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
rg "NPM_TOKEN_TC|CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|sk-[A-Za-z0-9]{10}|ghp_[A-Za-z0-9]{10}|npm_[A-Za-z0-9]{20}" README.md docs examples packages .agent/goals || true
rg "sudo -S|PASSWORD|PASSWD|credential broker|credential_broker|npm publish|workflow_dispatch" packages/terminal-commander/lib packages/terminal-commander/bin packages/terminal-commander/test || true

npm view terminal-commander version || true
npm view @terminal-commander/linux-x64 version || true
npm view @terminal-commander/linux-arm64 version || true
```

## Task Prompt

Run WWS06 only on branch `main`. JS CLI subcommands only under `lib/cli/`. NO PowerShell. NO sudo. NO password handling. NO publish. Every `wsl.exe` invocation goes through the WWS04 `lib/wsl/spawn.js` helper. The bin shim's Windows branch delegates to `lib/cli/run.js`; the Linux branch stays byte-identical.

## Final Report Format

- Pushed WWS05 range (confirmation only)
- Pushed WWS06 prep-amendment range (this file)
- Files changed by WWS06 (verified-work commit)
- CLI command surface summary (`--help` panels)
- Doctor behavior table
- `setup cursor-wsl` behavior table (without and with `--install-wsl-runtime`)
- Pairing behavior table
- Install behavior and credential / sudo boundary evidence
- Cursor config writer integration evidence
- Live Windows setup / doctor status (per the evidence list above)
- Verification summary
- Confirmation `npm-bootstrap-publish` was not dispatched
- Confirmation no npm publish occurred
- Confirmation WWS07 not started
- Local git state (HEAD, ahead/behind, branch)

## Binding decision carry-forward (from WWS01)

| Decision | Owner at WWS06 | What WWS06 lands |
|---|---|---|
| D-07 Pick default distro automatically; ask once on multi-distro hosts; refuse on `--no-interactive`. | WWS06 owns the asking. | `setup cursor-wsl` picks default; with multi-distro and no `--distro`/`TC_WSL_DISTRO`/persisted choice, prints the candidate list + exit 64 (`no_default_distro_ambiguous`) UNLESS the operator runs with `--distro <name>`. No interactive prompt at WWS06 (deferred); operator must pass `--distro` or set `TC_WSL_DISTRO`. |
| D-08 Automatic install inside WSL requires explicit `--install-wsl-runtime`. | WWS06 owns the flag. | `--install-wsl-runtime` triggers ONE constant install command via `lib/wsl/spawn.js`. No sudo. No password. `install_permission_required` on failure. |
| D-09 Pairing is OPTIONAL; pair codes are operator confirmation, never security secrets. | WWS06 owns the implementation. | `pair create` persists `pair.json`; `pair accept` validates the 6-digit shape + `pair_deferred` for the full handshake (future enhancement). |
| D-10 Windows-side state at `%LOCALAPPDATA%\terminal-commander\setup.json`. | WWS06 owns the writer. | `lib/cli/setup_state.js` atomic-write per WWS05 patterns. Bounded schema. |
| D-12 Multi-distro persisted choice + `--distro` override. | WWS06 owns persistence. | `setup_state.json.distro` is the persisted choice. `--distro` overrides for one invocation. |
| D-13 Cursor config default `--global`; `--project` opts into workspace. | WWS06 owns the flag surface; WWS05 owns the writer. | `setup cursor-wsl` flags wire through to WWS05 `writeCursorMcpConfig`. |
| D-14 Rollback via `setup cursor-wsl --uninstall` (restores `mcp.json.bak`). | Partial — uninstall flow is a future enhancement at WWS06. | At WWS06 the `.bak` is written by WWS05; `--uninstall` is documented as future work, not implemented here (recorded as known gap). |
