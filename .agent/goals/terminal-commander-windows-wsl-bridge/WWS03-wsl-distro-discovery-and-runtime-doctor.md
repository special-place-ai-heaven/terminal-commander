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
  - "WWS01 contract (docs/release/windows-wsl-bridge-contract.md) — §7 distro selection, §10.2 lib/wsl/detect.js + lib/wsl/doctor.js, §15 D-06 / D-07 / D-12, §12 failure-mode table"
  - "WWS02 root package widening (commit 1da40f3)"
  - "Windows wsl.exe documented CLI surface"
  - "WWS04 lib/wsl/spawn.js bridge owner (NOT this goal)"
  - "WWS06 setup cursor-wsl CLI owner (NOT this goal)"
risk_level: "medium"
---

# WWS03 - Wsl Distro Discovery And Runtime Doctor

## Branch Guard

```text
main
```

## Mission Context

Before the bridge shim (WWS04) can launch the WSL-side MCP server and before the setup CLI (WWS06) can persist a distro choice, the Windows root npm package must own a JS-only helper layer that:

1. detects whether the host is Windows at all,
2. detects whether `wsl.exe` is reachable,
3. enumerates available distros via `wsl.exe -l -v` (verbose) only,
4. identifies the WSL default distro (asterisk marker), state, and version,
5. validates operator-supplied distro names against a conservative whitelist character set,
6. produces a bounded read-only `doctor` report for a chosen distro (existence in the live whitelist + optional non-mutating runtime probe).

This is the **first environment-adapter helper layer** in the WWS chain. It does NOT install anything, does NOT write any setup-state file, does NOT invoke the bridge, does NOT touch Cursor config, does NOT expose any CLI subcommand, and does NOT add new MCP tools.

WWS03 closes WWS01 binding decisions D-06 (auto-detect via `wsl.exe -l -v` only) and the discovery half of D-12 (multi-distro handling: enumerate + safety-whitelist). The CLI half of D-07 and the persistence half of D-12 stay with WWS06. The actual `wsl.exe` bridge spawn for `terminal-commander-mcp` stays with WWS04 (`lib/wsl/spawn.js`).

No Rust change. No daemon change. No publish. All `wsl.exe` invocations go through `child_process.spawn` with `shell: false`, `windowsHide: true`, and a fixed argv shape; no shell interpolation; no operator string concatenated into argv beyond a pre-whitelisted distro name.

## Prep amendment (2026-05-23, before WWS03 implementation)

This goal file was prep-amended once before WWS03 implementation started:

- **Trigger**: the original allowed-files set covered only `lib/wsl/**`, `test/**`, `package.json` (test-script only), and the goal file itself. The directive widened the implementation scope to also touch `packages/terminal-commander/README.md`, `docs/release/windows-wsl-bridge-contract.md` (clarifying §10.2 lines), `docs/integrations/cursor.md` (WSL-discovery wording only), `GOAL_CHAIN_INDEX.md`, and `RUN_ORDER.md` for status alignment.
- **Mechanics**: per established WWS chain rule (single `chore(goals): ...` commit BEFORE implementation when scope drifts), this amendment is the prep amendment. Implementation runs only AFTER this amendment lands locally as a stand-alone commit. No code or doc edits land in the prep-amendment commit beyond this goal file.
- **Doctrine carry-forward**: a future `CAP01-capability-registry-contract.md` goal is noted but NOT started; WWS03 does NOT widen scope to add capability-registry types — `lib/wsl/detect.js` returns structured records, not capability descriptors.

## Mini-Spec

objective:
- Add `packages/terminal-commander/lib/wsl/detect.js` exporting `detectWsl(opts)`. Returns a typed result describing the host platform, whether `wsl.exe` is callable, the list of installed distros parsed from `wsl.exe -l -v` (with state + WSL version + default-marker), and any failure reason from a closed enum.
- Add `packages/terminal-commander/lib/wsl/distro-name.js` exporting `isSafeDistroName(name)` + `assertSafeDistroName(name)` so future bridge / setup callers cannot pass an unvetted distro string into `wsl.exe` argv.
- Add `packages/terminal-commander/lib/wsl/doctor.js` exporting `wslDoctor(opts)`. Gated by `detectWsl`. Validates an operator-supplied distro name against the live whitelist returned by detect, optionally runs ONE non-mutating probe inside the distro (`wsl.exe -d <distro> -- bash -lc 'command -v terminal-commander-mcp'`), and returns a bounded `{ status, reason, distro, runtime_present, hint }` record. The probe is opt-in via `opts.probeRuntime === true`; default is OFF.
- Add unit tests under `packages/terminal-commander/test/` exercising parser fixtures, name safety, mocked executor behavior, doctor result enum, and the static guards listed below.
- Cross-link the new helpers from `packages/terminal-commander/README.md` (one short subsection), `docs/release/windows-wsl-bridge-contract.md` §10.2 / §17 (clarify "WWS03 lands detect + doctor, WWS04 still owns spawn"), and `docs/integrations/cursor.md` §11b (one-line update naming WWS03 helpers as the discovery layer).
- Update `.agent/goals/terminal-commander-windows-wsl-bridge/WWS03-*.md` frontmatter to Completed in the status commit.
- Update `GOAL_CHAIN_INDEX.md` + `RUN_ORDER.md` to mark WWS03 Completed.

non_goals:
- NO CLI subcommand surface (`terminal-commander setup` / `terminal-commander doctor`) — that is WWS06.
- NO `lib/wsl/spawn.js` introduction — that is WWS04.
- NO Cursor config writer (`lib/cursor/**`) — that is WWS05.
- NO `setup.json` / `pair.json` writer — that is WWS06.
- NO automatic install inside WSL (`npm install -g terminal-commander` via `wsl.exe`) — that is WWS06 behind the explicit `--install-wsl-runtime` flag.
- NO pairing implementation — that is WWS06.
- NO sudo handling. NO password handling. NO credential broker. NO env-var passthrough beyond what `child_process.spawn` defaults to.
- NO `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`, `scripts/**`, `.github/**` change.
- NO MCP tool addition. NO daemon change. NO IPC change.
- NO publish. NO workflow dispatch. NO version bump. NO release-please change.
- NO macOS code paths. NO Windows-native runtime claim. NO postinstall downloader.
- NO new bin shim. The three bin/* shims from WWS02 stay UNCHANGED in this goal.
- NO `lib/wsl/detect.js` or `lib/wsl/doctor.js` import inside `bin/*.js` (deferred to WWS04 / WWS06; the helpers ship as library-only at WWS03).

allowed_files_or_area:
- `packages/terminal-commander/lib/wsl/**`
  (new files: `detect.js`, `doctor.js`, `distro-name.js`; possibly `index.js` if a single import surface is wanted)
- `packages/terminal-commander/test/**`
  (new test files; existing tests stay unchanged except for cross-imports if needed)
- `packages/terminal-commander/package.json` (ONLY if the `test` script needs to glob the new test files — no other field change)
- `packages/terminal-commander/README.md` (one short "Windows discovery helpers (WWS03)" subsection; no claim that the bridge ships yet)
- `docs/release/windows-wsl-bridge-contract.md` (clarification edits in §10.2 / §17 only — record which helpers land at WWS03; no binding decisions changed)
- `docs/integrations/cursor.md` (one-line update inside §11b only — name the WWS03 helpers as the discovery layer)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS03-*.md` (this file)
- `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md` (status row + footer)
- `.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md` (header status line)

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
- `packages/terminal-commander/lib/resolve-binary.js`
- secrets / tokens / private paths

contracts_or_interfaces:
- `detectWsl(opts)` is an async function. `opts` (all optional, all defaulting): `platform` (default `process.platform`), `exec` (a mocked executor for tests; defaults to a thin wrapper around `child_process.spawn('wsl.exe', argv, { shell: false, windowsHide: true })`), `wslPath` (default `'wsl.exe'`), `timeoutMs` (default 5000), `now` (clock injection for tests).
- Returns `{ host_platform, wsl_callable, default_distro, distros: [{ name, state, wsl_version, is_default }], reason, raw_excerpt_for_debug? }`. `reason` is one of: `ok`, `unsupported_host`, `wsl_not_found`, `no_distros`, `wsl_command_failed`, `check_timeout`.
- `isSafeDistroName(name)` returns `true` iff `name` matches `^[A-Za-z0-9._-]{1,64}$`. `assertSafeDistroName(name)` throws `Error` with `.code = 'UNSAFE_DISTRO_NAME'` otherwise. No whitespace, no quote, no semicolon, no pipe, no dollar, no backtick, no slash, no backslash, no shell metacharacter.
- `wslDoctor(opts)` is an async function. `opts.distro` (required, validated via `assertSafeDistroName` before any further work). Optional `opts.probeRuntime` (default `false`). Returns `{ status, reason, distro, runtime_present, hint, raw_excerpt_for_debug? }`. `status` is one of: `ok`, `unsupported_host`, `wsl_not_found`, `no_distros`, `distro_not_found`, `unsafe_distro_name`, `wsl_command_failed`, `runtime_missing`, `runtime_present`, `doctor_not_run`, `check_timeout`. `hint` is a bounded single-line human-readable string that maps to §12 of the WWS01 contract.
- Every `wsl.exe` invocation uses `spawn(wslPath, argvArray, { stdio: ['ignore', 'pipe', 'pipe'], shell: false, windowsHide: true })`.
- Argv arrays are constant beyond the distro name. The ONLY operator-supplied value passed to `wsl.exe` is the distro string, AFTER it has been (a) `assertSafeDistroName`-validated AND (b) found in the live `detectWsl()` distro list.
- Functions degrade gracefully on non-Windows hosts: `detectWsl()` returns `{ host_platform, wsl_callable: false, default_distro: null, distros: [], reason: 'unsupported_host' }`; `wslDoctor()` returns `{ status: 'unsupported_host', ... }` without invoking the executor.
- Parsers tolerate UTF-16 LE BOM, NUL bytes from `wsl.exe` legacy output, and CRLF line endings. The detect parser strips UTF-16 NULs before splitting lines.

invariants:
- No raw stream lane added.
- No shell expansion. No `shell: true`. No `bash -lc` string concatenating operator input.
- No spawn of anything other than `wsl.exe`.
- No file open outside the package directory. No file write at all.
- No network. No socket. No HTTP. No subprocess outside `wsl.exe`.
- No environment-variable echo. No env-var leak in any returned record.
- No credentials in any code path. No `sudo`. No password prompt.
- No new MCP tool. The 29-tool TC45 catalogue is unchanged.
- No bin shim edit. No resolver edit.
- No platform-package edit.
- No version bump.
- No release-please / publish workflow change.
- `npm-bootstrap-publish.yml` stays committed-but-undispatched.
- All long-lived tokens (`NPM_TOKEN_TC`, `CARGO_REGISTRY_TOKEN_TC`, `RELEASE_PLEASE_TOKEN_TC`) stay UNUSED.
- TC48 + NPM10 `Conditional Go` posture preserved.

acceptance_criteria:
- `packages/terminal-commander/lib/wsl/detect.js` exists, exports `detectWsl` (async) + named constants (e.g. `DETECT_REASONS`).
- `packages/terminal-commander/lib/wsl/distro-name.js` exists, exports `isSafeDistroName`, `assertSafeDistroName`, and a frozen `UNSAFE_DISTRO_NAME` code constant.
- `packages/terminal-commander/lib/wsl/doctor.js` exists, exports `wslDoctor` (async) + named `DOCTOR_STATUSES`.
- Each helper is pure with respect to the injected `exec` (no global state, no top-level spawn).
- `npm test` in `packages/terminal-commander` passes ALL the following:
  - existing 20 WWS02 tests (resolver + shim win32 branch) UNCHANGED + GREEN
  - new detect tests: verbose parse single distro, multi-distro, default-marker (`*`), state (`Running` / `Stopped`), version (1 / 2), names-only fallback if implemented, NUL/UTF-16 LE BOM normalization, CRLF normalization, mocked `wsl_not_found`, mocked `no_distros`, mocked `wsl_command_failed`, mocked `check_timeout`, `unsupported_host` on non-win32 platform injection
  - new distro-name tests: accepts conservative names (`Ubuntu`, `Ubuntu-24.04`, `Debian_12`, `arch.linux`), rejects whitespace, quotes, semicolons, pipes, dollars, backticks, slashes, backslashes, empty string, > 64 chars, NUL byte
  - new doctor tests: `unsafe_distro_name` rejected BEFORE executor invoked, `distro_not_found` for whitelist miss, `ok` + `runtime_present` when probe enabled and mocked executor returns `command -v` hit, `runtime_missing` when probe enabled and executor returns empty, `doctor_not_run` when `probeRuntime` is false, `check_timeout` on injected timeout, `wsl_command_failed` on injected non-zero exit
  - new static-guard tests: helper code contains NO `sudo`, NO `npm install`, NO `apt-get`, NO `pacman`, NO `--install`, NO `pair`, NO `mcp.json`, NO file-write API (`fs.writeFile`, `fs.writeFileSync`, `fs.appendFile`, `createWriteStream`); helper code spawns ONLY `wsl.exe` (the only `spawn(` first argument literal token is either `wslPath` / `wsl_path` parameter or the constant `'wsl.exe'`)
  - new static-guard tests: helper code does NOT invoke `wsl.exe` from any `bin/*.js` shim (resolver, shims untouched)
- `npm pack --dry-run` clean for all THREE packages, file counts unchanged for the two platform packages, file count for the root package equals the prior baseline plus only the new `lib/wsl/**` files actually shipped.
- Resolver tests from WWS02 still pass (regression guard for resolve-binary.test.js).
- `crates/**` untouched; `cargo nextest run --workspace` PASS 347/347.
- runtime-smoke + npm-local-install PASS unchanged from WWS02 baseline.
- MCP guard greps clean (no new Command::new / Command::spawn / TcpListener / UdpSocket / fs in crates/mcp).
- secret-leak grep clean.
- `npm view` for all three names still returns E404 (no publish).
- `.github/**` diff empty.
- `npm-bootstrap-publish` NOT dispatched.

evidence_required:
- Branch evidence: `git branch --show-current` reports `main`.
- File paths of every added / modified file in the WWS03 verified-work commit.
- `npm test` PASS line count + total tests.
- `npm pack --dry-run` summary for root + linux-x64 + linux-arm64.
- `cargo nextest run --workspace` PASS count (run inside WSL with `CARGO_TARGET_DIR=target-wsl`).
- `bash scripts/smoke/verify-runtime-smoke.sh` PASS line count.
- `bash scripts/smoke/verify-npm-local-install.sh` PASS line count.
- MCP guard grep outputs (doc / negative matches only).
- Secret-leak grep output (0 matches expected).
- Live Windows / WSL doctor evidence (run from a Windows host with `wsl.exe` if available); otherwise mark "Not Run" with the exact blocker per the WWS01 evidence policy.

stop_conditions:
- Branch is not `main`.
- `git status --short` is not clean before commit.
- The detect / doctor helpers would require dispatching `wsl.exe` with operator-supplied shell strings (i.e. anything beyond an `assertSafeDistroName`-validated distro AS A SINGLE ARGV ELEMENT after `-d`).
- A new top-level spawn target (`bash`, `cmd`, `powershell`, anything other than the parameterized `wslPath`) would be introduced.
- Any file-write API call would be added to the helper layer.
- Any credential / token / env-var passthrough would be added.
- The bin/* shims would be edited (deferred to WWS04 / WWS06).
- Any platform-package or version field would change.
- Origin/main has moved during implementation (re-check before commit).

verification_command:
```bash
# Branch + worktree state
git branch --show-current
git status --short
git diff --check

# npm pack contract (3 packages)
npm pack ./packages/terminal-commander --dry-run
npm pack ./packages/terminal-commander-linux-x64 --dry-run
npm pack ./packages/terminal-commander-linux-arm64 --dry-run

# Package contract assertions
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

# JS test suite (resolver + shim + WSL helpers)
( cd packages/terminal-commander && npm test )

# Rust workspace gauntlet (WSL host)
CARGO_TARGET_DIR=target-wsl cargo metadata --no-deps
CARGO_TARGET_DIR=target-wsl cargo fmt --all --check
CARGO_TARGET_DIR=target-wsl cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR=target-wsl cargo nextest run --workspace

# Runtime + npm-local-install smokes (WSL host)
bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh

# MCP guard greps + secret-leak grep
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
rg "NPM_TOKEN_TC|CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|sk-[A-Za-z0-9]{10}|ghp_[A-Za-z0-9]{10}|npm_[A-Za-z0-9]{20}" README.md docs examples packages .agent/goals || true

# Publish negative checks
npm view terminal-commander version || true
npm view @terminal-commander/linux-x64 version || true
npm view @terminal-commander/linux-arm64 version || true
```

## Task Prompt

Run WWS03 only on branch `main`. JS detect + distro-name + doctor helpers only, under `packages/terminal-commander/lib/wsl/`. No CLI subcommand. No `lib/wsl/spawn.js`. No `wsl.exe` shell interpolation. No file writes. No publish. No workflow change. No bin/* edit. The three bin/* shims from WWS02 stay byte-identical at WWS03.

## Final Report Format

- Pushed WWS02 range (confirmation only)
- Pushed WWS03 prep-amendment range (this file)
- Files changed by WWS03 (verified-work commit)
- WSL helper API summary (detect / distro-name / doctor signatures + return shapes)
- Distro parser behavior table (input fixture -> parsed distros)
- Doctor result / error table (reason -> meaning -> caller action)
- Safety evidence: no install, no sudo, no credentials, no bridge launch, no file writes, no env passthrough, only `wsl.exe` is spawned
- Static-guard test summary
- Live Windows / WSL doctor status: PASS / Not Run / Blocked with exact reason
- Verification summary (every gate listed above)
- Confirmation `npm-bootstrap-publish` was not dispatched
- Confirmation no npm publish occurred
- Confirmation WWS04 not started
- Local git state (HEAD, ahead/behind, branch)

## Binding decision carry-forward (from WWS01)

| Decision | Owner at WWS03 | What WWS03 lands |
|---|---|---|
| D-06 `setup cursor-wsl` auto-detects WSL distros via `wsl.exe -l -v` only. | WWS03 owns the discovery layer. WWS06 owns the CLI surface. | `detectWsl()` issues only `wsl.exe -l -v` (and optionally `wsl.exe --status` for "is wsl callable"). Parses verbose output. Identifies default + state + version. |
| D-07 Pick default distro automatically; ask once on multi-distro hosts; refuse on `--no-interactive`. | WWS06 owns the asking. WWS03 only EXPOSES the default + the multi-distro list so WWS06 can decide. | `detectWsl()` returns `default_distro` + full `distros` list. No prompting code at WWS03. |
| D-12 Multi-distro handling via persisted choice + `--distro` override. | WWS06 owns persistence. WWS03 owns enumeration + whitelist validation. | `detectWsl()` enumerates; `assertSafeDistroName()` + caller-side membership check enforce the whitelist boundary. |

WWS03 does NOT decide D-07's prompting UX, does NOT persist any state file, and does NOT touch Cursor config (D-13).
