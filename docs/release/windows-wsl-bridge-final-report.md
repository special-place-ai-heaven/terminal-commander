# Windows + WSL Bridge — Final Report (WWS09 Closure)

**Chain**: `terminal-commander-windows-wsl-bridge`
**Goals**: WWS01..WWS09 (all closed at this commit)
**Branch**: `main`
**Date**: 2026-05-23
**Recommendation**: **Conditional Go preserved; ready for `npm-bootstrap-publish` workflow_dispatch in `dry_run=true` mode ONLY. Real publish requires separate explicit operator approval after dry-run evidence is captured AND the npmjs.com trusted-publisher operator preconditions land.**

This document consolidates the chain's evidence for the
operator's first-publish decision. It does NOT publish. It does
NOT dispatch any workflow. It does NOT merge a release PR. It
does NOT create a tag or GitHub release.

Language: ASCII only.

## 0. Provenance

Successor of the `terminal-commander-npm-distribution` chain
(closed at NPM10 — three-package layout, OIDC release-please
workflow live, bootstrap workflow committed but undispatched, all
three names still E404). The WWS chain shipped JS-only Windows
control-plane surfaces around the existing Linux/WSL2 runtime.

| Goal  | Title                                  | Status     | Commit    |
|-------|----------------------------------------|------------|-----------|
| WWS01 | Windows + WSL install UX contract      | Completed  | `6220eb2` |
| WWS02 | Root npm package win32 bridge contract | Completed  | `1da40f3` |
| WWS03 | WSL discovery + read-only doctor       | Completed  | `ec8441e` |
| WWS04 | Windows MCP bridge shim                | Completed  | `d86e73f` |
| WWS05 | Cursor MCP config writer               | Completed  | `ae37878` |
| WWS06 | Setup / doctor / pair CLI              | Completed  | `4936904` |
| WWS07 | Windows bridge smoke (PowerShell)      | Completed  | `785d410` |
| WWS08 | README + release-contract docs update  | Completed  | `12b47ce` |
| WWS09 | Pre-publish readiness review (this)    | Completed  | (this commit) |

Every WWS goal landed on `main` via the two-commit pattern
(verified-work commit + status commit), with prep amendments
where scope drift was identified (the chain landed prep
amendments at WWS02, WWS03, WWS04, WWS05, WWS06, WWS07, WWS08;
WWS09's scope matches the goal file verbatim — no prep amendment
needed).

## 1. Per-goal recap

### 1.1 WWS01 — Windows + WSL install UX contract

- File: `docs/release/windows-wsl-bridge-contract.md` (commit
  `6220eb2`; ~656 lines; 21 sections).
- 15 binding decisions D-01..D-15 locked.
- R-WWS-01..R-WWS-10 public risks recorded (carried into
  `RISK_REGISTER.md` at WWS08).
- §14.1 RECOMMENDED keeping `npm-bootstrap-publish.yml` PAUSED
  until at minimum WWS02 + WWS04 + WWS05 + WWS06 + WWS08 land —
  ALL of those landed.

### 1.2 WWS02 — Root npm package win32 bridge contract

- Single field change: `packages/terminal-commander/package.json`
  `os: ["linux"]` -> `os: ["linux", "win32"]` (commit `1da40f3`).
- Platform packages remain `os: ["linux"]` (`linux-x64`,
  `linux-arm64`).
- `optionalDependencies` exact-pin preserved.
- `bridge_required` resolver branch added to `lib/resolve-binary.js`
  for `win32`; the three bin shims branch to bounded refusals
  (exit `64`) on Windows.
- No Rust binary ships on Windows. No native daemon claim.
- NPM02 §13b amendment recorded the single-field change verbatim.

### 1.3 WWS03 — WSL discovery + read-only doctor

- New files (all under `packages/terminal-commander/lib/wsl/`,
  commit `ec8441e`):
  - `distro-name.js` (whitelist `^[A-Za-z0-9._-]{1,64}$`)
  - `detect.js` (`wsl.exe -l -v` only; tolerates UTF-16 LE BOM +
    NUL-padded ASCII + CRLF; closed-enum reasons)
  - `doctor.js` (read-only; `probeRuntime` opt-in; constant
    `command -v terminal-commander-mcp` probe)
- 50 new Node tests; no spawn outside `wsl.exe`; no file write.

### 1.4 WWS04 — Windows MCP bridge shim

- New file: `packages/terminal-commander/lib/wsl/spawn.js`
  (commit `d86e73f`).
- `spawnWslBridge()` orchestrates: distro priority chain
  (`TC_WSL_DISTRO` env -> `detectWsl().default_distro` ->
  `no_default_distro` refusal) -> double validation
  (`assertSafeDistroName` + live whitelist) -> optional
  `wslDoctor` runtime gate (opt-out: `TC_WSL_SKIP_DOCTOR=1`) ->
  token-shaped env strip via `buildFilteredEnv` -> `spawn(wsl.exe,
  ['-d', distro, '--', 'bash', '-lc', 'exec terminal-commander-mcp',
  ...userArgv], { shell:false, stdio:'inherit', env:filteredEnv })`.
- Wired into `bin/terminal-commander-mcp.js` Windows branch.
- 21 new tests + static guards. The two sibling shims
  (`terminal-commanderd.js`, `terminal-commander.js`) stay
  byte-identical to the WWS02 contract.

### 1.5 WWS05 — Cursor MCP config writer

- New files (under `packages/terminal-commander/lib/cursor/`,
  commit `ae37878`):
  - `config.js` (pure stanza-builder + path resolver + JSON parse
    + merge + validate; `MAX_CONFIG_BYTES = 256 KiB`)
  - `write.js` (atomic write via `<path>.tmp.<random>` +
    `renameSync` in the same directory; `.bak` backup; 12-status
    closed enum; stdout-silent)
  - `index.js` (barrel)
- Default stanza: `{ "type": "stdio", "command":
  "terminal-commander-mcp" }`. Optional `env.TC_WSL_DISTRO` only
  when operator passed a safe distro.
- 62 new tests. NO `child_process`, NO spawn, NO network. The
  ONLY env key the writer ever emits is `TC_WSL_DISTRO`.

### 1.6 WWS06 — Setup / doctor / pair CLI

- New files (under `packages/terminal-commander/lib/cli/`, commit
  `4936904`):
  - `parser.js` (dependency-free argv parser)
  - `doctor.js`, `setup_cursor_wsl.js`, `pair_create.js`,
    `pair_accept.js`, `setup_state.js`, `run.js`, `index.js`
- Five subcommands locked: `doctor`, `doctor wsl`, `setup
  cursor-wsl`, `pair create`, `pair accept <code>`.
- 21-status closed enum (`setup_ready` through
  `credential_required`).
- `--install-wsl-runtime` opt-in. ONE constant `npm install -g
  terminal-commander` invocation through the WWS04 bridge argv
  shape. NO sudo. NO `sudo -S`. NO password prompt. NO env
  credential. EACCES -> `install_permission_required` (no retry
  under sudo). E404 -> `npm_package_unpublished` honestly.
- `bin/terminal-commander.js` Windows branch delegates to
  `lib/cli/run.js`. Linux branch byte-identical to the WWS02 / WWS03 / WWS04
  baseline.
- 78 new tests + static guards.

### 1.7 WWS07 — Windows bridge smoke

- New file: `scripts/smoke/verify-windows-bridge-smoke.ps1`
  (commit `785d410`).
- PowerShell 5.1+ compatible (`Set-StrictMode -Version Latest`;
  `ErrorActionPreference = Stop`).
- Flags: `-DryRun`, `-Distro <name>`, `-InstallWslRuntime`,
  `-WriteCursorConfig`, `-TempCursorScope` (default-on when
  `-WriteCursorConfig` supplied).
- Output: bounded `PASS <step>` / `FAIL <step>` / `NOTE <text>`
  / `INFO <text>` lines.
- Only spawn surface: `node` (the JS CLI shim). NO direct
  `wsl.exe` / `child_process` calls from PowerShell.
- Live evidence on the verification host:
  - Windows CLI (doctor / doctor wsl / setup --print-config /
    setup --dry-run): **PASS**.
  - Windows -> WSL MCP bridge round-trip: **Not Run** (WSL distro
    lacks `terminal-commander-mcp` because the npm package is
    still E404).
  - Cursor GUI provider smoke: **Not Run** (no headless Cursor
    MCP discovery entry point on host).
  - Real Cursor config: NOT touched.
  - WSL runtime install path: none.

### 1.8 WWS08 — README + release-contract docs update

- 8 files modified (commit `12b47ce`):
  - `README.md` (feature matrix WWS rows; Install Windows host
    subsection; Cursor section bridge note; Current beta status
    WWS chain state table; Repository layout)
  - `docs/release/windows-wsl-bridge-contract.md` (§18 WWS08
    row expanded)
  - `docs/release/npm-binary-packaging-contract.md` (§13b
    closing follow-up paragraph)
  - `docs/release/npm-distribution-final-report.md` (new §11
    WWS chain follow-up)
  - `RELEASE_CHECKLIST.md` (new Windows + WSL bridge chain
    section)
  - `BACKLOG.md` (new P2 WWS-B1..WWS-B9 section)
  - `RISK_REGISTER.md` (new R-WWS-01..R-WWS-10 entries)
  - `ROADMAP.md` (new WWS01..WWS09 table + out-of-chain
    deferrals)
- No version bump. No workflow change. No code change.

### 1.9 WWS09 — Pre-publish readiness review (this commit)

- New file: `docs/release/windows-wsl-bridge-final-report.md`
  (this document).
- WWS09 frontmatter -> Completed.
- `GOAL_CHAIN_INDEX.md` chain summary header flipped to
  "WWS01..WWS09 Completed; chain CLOSED".
- `RUN_ORDER.md` header flipped to match.

## 2. npm registry probe

`npm view <name> version` invoked for all three names at
WWS09 close. Expected outcome: E404 for every name (no publish
has occurred from this chain or before).

```text
npm view terminal-commander version             -> E404
npm view @terminal-commander/linux-x64 version  -> E404
npm view @terminal-commander/linux-arm64 version -> E404
```

All three names remain `E404`. The first live publish has NOT
occurred. The publish floor recommended by WWS01 §14.1
(WWS02 + WWS04 + WWS05 + WWS06 + WWS08) is met; the additional
operator preconditions in
`docs/release/npm-trusted-publishing-contract.md` §8 are still
pending operator action (npmjs.com org claim + trusted-publisher
configuration for all three names with workflow filename
`release-please.yml`).

## 3. Local verification gate snapshot

| Gate | Result |
|---|---|
| `git branch --show-current` | `main` |
| `git status --short` (pre-commit) | clean |
| `git diff --check` | clean |
| `cargo metadata --no-deps` (WSL, `CARGO_TARGET_DIR=target-wsl`) | OK |
| `cargo fmt --all --check` (WSL) | `FMT_OK` |
| `cargo clippy --workspace --all-targets -- -D warnings` (WSL) | clean |
| `cargo nextest run --workspace` (WSL) | **347/347 PASS, 0 skipped** |
| `bash scripts/smoke/verify-runtime-smoke.sh` (WSL) | SUCCESS |
| `bash scripts/smoke/verify-npm-local-install.sh` (WSL) | SUCCESS |
| `(cd packages/terminal-commander && npm test)` | **234/234 PASS** (12 NPM03 + 9 WWS02 + 50 WWS03 + 21 WWS04 + 62 WWS05 + 78 WWS06; +2 cli-pair tests not double-counted) |
| `npm pack ./packages/terminal-commander --dry-run` | 23 files, `0.1.0-beta.1` |
| `npm pack ./packages/terminal-commander-linux-x64 --dry-run` | 5 files, `0.1.0-beta.1` |
| `npm pack ./packages/terminal-commander-linux-arm64 --dry-run` | 5 files, `0.1.0-beta.1` |
| Python package-contract assertions | `package-contract-ok 0.1.0-beta.1` |
| `test ! -e .cursor/mcp.json` | PASS |
| MCP guard #1 (`Command::new|spawn|TcpListener|UdpSocket` in `crates/mcp`) | 4 doc/negative matches (unchanged baseline) |
| MCP guard #2 (`tokio::fs|std::fs|File::open|...` in `crates/mcp/src`) | 0 matches |

## 4. Bridge invariants confirmed

All ten WWS01 §15 binding decisions D-01..D-15 are honored on
`main` at WWS09 close. Spot checks executed at WWS09:

- D-01 root `os` widened to `["linux", "win32"]`: verified via
  `python package-contract` assertion + npm pack root tarball
  showing the unchanged 23 files.
- D-02 platform packages remain `os: ["linux"]`: verified by
  python contract; pack file counts 5/5 unchanged.
- D-03 Windows install ships JS only: root tarball at WWS09
  contains `lib/cli/**` + `lib/cursor/**` + `lib/wsl/**` +
  `bin/*` + `lib/resolve-binary.js`. No Rust binary. Both
  platform packages skipped on Windows via `os` / `cpu` filter.
- D-04 `terminal-commander-mcp` on Windows bridges via
  `lib/wsl/spawn.js` argv-only spawn: verified by
  `wsl-spawn.test.js` + `cli-static-guards.test.js` static
  guards.
- D-05 `terminal-commanderd` on Windows refuses with exit 64:
  the shim is byte-identical to the WWS02 baseline; the static
  guard in `cli-static-guards.test.js` enforces no
  `lib/wsl/spawn.js` import in the daemon shim.
- D-06 `setup cursor-wsl` auto-detect via `wsl.exe -l -v` only:
  verified by `lib/wsl/detect.js` source; only `wsl.exe -l -v`
  is invoked at WWS03.
- D-07 default-distro pick + ask-once: partial — WWS06 picks
  default + accepts `--distro`; interactive prompting deferred
  to a future enhancement (`no_default_distro_ambiguous` refusal
  + candidate list when ambiguous). Recorded in BACKLOG WWS-B5.
- D-08 automatic install requires explicit `--install-wsl-runtime`:
  verified by `cli-setup.test.js` argv shape assertion.
- D-09 pairing optional, code is operator confirmation: verified
  by `pair_create.js` / `pair_accept.js` doc + tests; `pair
  accept` returns `pair_deferred` until the full handshake lands
  (BACKLOG WWS-B6).
- D-10 Windows-side state at `%LOCALAPPDATA%\terminal-commander\`:
  verified by `lib/cli/setup_state.js` `getStateDir` + tests.
- D-11 WSL-side runtime config unchanged: `crates/**` and
  `crates/daemon/src/**` byte-identical (forbidden-paths diff
  empty across the entire chain).
- D-12 multi-distro persisted choice + `--distro` override:
  verified at WWS06 (env override + `setup_state.json`).
- D-13 Cursor config default `--global`; `--project` for
  workspace: verified by `cli-setup.test.js` flag tests.
- D-14 rollback via `setup cursor-wsl --uninstall`: partial.
  WWS05 writes `.bak`; `--uninstall` flag NOT implemented.
  Recorded in BACKLOG WWS-B4.
- D-15 publish-readiness floor (WWS02 + WWS04 + WWS05 + WWS06 +
  WWS08): MET. WWS07 + WWS09 also complete (additional
  evidence).

## 5. Cursor live smoke status

**Not Run.**

- Direct PowerShell smoke
  (`scripts/smoke/verify-windows-bridge-smoke.ps1`) on the
  verification host: CLI / config / doctor PASS; MCP round-trip
  Not Run = `runtime_missing`.
- Cursor GUI provider smoke: Not Run because Cursor has no
  documented headless / scripted MCP discovery entry point (no
  `cursor --list-mcp-tools` subcommand; no `cursor-agent`
  headless CLI on host).

`Not Run` is honestly recorded everywhere it appears (README
feature matrix; README Current beta status; RELEASE_CHECKLIST
WWS section; BACKLOG WWS-B3; RISK_REGISTER R-WWS-09). No
promotion to PASS.

To promote `Not Run` -> PASS, an operator must (per WWS07 §11d
of `docs/integrations/cursor.md` and BACKLOG WWS-B3):

1. Install the WSL-side runtime via local-tarball install OR
   wait for NPM07 first publish + run
   `terminal-commander setup cursor-wsl --install-wsl-runtime`.
2. Run `terminal-commander setup cursor-wsl` (or `--project
   <repo>` for workspace scope) so the operator's real
   `mcp.json` points at the WWS04 bridge.
3. Open Cursor on Windows.
4. Confirm `terminal-commander` appears in `Settings -> Features
   -> MCP`.
5. Ask Cursor's chat to call `health`. Verify the response is a
   bounded JSON envelope.
6. Capture the chat transcript + timestamps and attach to a
   follow-up evidence file.

## 6. GitHub Actions status

| Workflow | Last status | Head SHA | Notes |
|---|---|---|---|
| `release-please.yml` | success | `ff8812c` (WWS08 status commit) | Latest run; publish jobs correctly **skipped** because `releases_created` evaluates to `false` (no `feat:` / `fix:` commit; WWS08 was docs-only). |
| `npm-binary-build.yml` | success | `23208db` (WWS07 status commit) | Last run that exercised the workflow; later docs-only commits do not trigger the path filter. |
| `npm-bootstrap-publish.yml` | (never dispatched) | — | Workflow file present; `gh workflow run` has NEVER been invoked. |

Two prior `release-please` runs (on `333552b` WWS07 prep and
`32d40fb` WWS06 status) failed transiently with `Bad
credentials - https://docs.github.com/rest` (release-please
action could not fetch from the GitHub API; operator-environment
token rotation issue, NOT a code or contract issue). All
subsequent runs after token rotation are green. This does NOT
block the publish recommendation because no publish was
attempted on those failed runs; release-please was probing
whether to OPEN a release PR (which it would not have, because
no `feat:` / `fix:` commit was pending).

## 7. Active blockers / operator preconditions

For first live npm publish via the standard OIDC
`release-please.yml` path:

- **Operator precondition 1**: Claim the `@terminal-commander`
  org on npmjs.com (the unscoped `terminal-commander` name is
  also pre-reserved). Configure the trusted publisher for all
  three package names (`terminal-commander`,
  `@terminal-commander/linux-x64`,
  `@terminal-commander/linux-arm64`) with workflow filename
  `release-please.yml`. Reference:
  `docs/release/npm-trusted-publishing-contract.md` §8.
- **Operator precondition 2**: A Conventional-Commits `feat:` /
  `fix:` commit lands on `main`. release-please opens a release
  PR. The operator merges it. The publish jobs run (OIDC token
  exchange; provenance attestation; no `NPM_TOKEN`; no PAT).

For first live npm publish via the NPM10 bootstrap workflow
(`npm-bootstrap-publish.yml`):

- **Operator precondition 1**: Decide whether to use the
  bootstrap path at all. Per WWS01 §14.1 + NPM10 contract, the
  bootstrap workflow is a one-time fallback that uses
  `NPM_TOKEN_TC` (a short-lived narrowly-scoped npm token,
  manually rotated). The recommendation is to attempt the OIDC
  path FIRST.
- **Operator precondition 2**: If bootstrap is needed, dispatch
  with `dry_run=true` first; review the dry-run evidence;
  separately dispatch with `dry_run=false` only after explicit
  owner approval. Then immediately disable / rotate per BACKLOG
  WWS-B8 / inherited NPM10 P1.5b.

For Cursor live provider smoke promotion to PASS:

- See §5 above; six operator-driven steps.

For Windows -> WSL MCP bridge round-trip PASS:

- Install the WSL-side runtime (local-tarball OR post-publish
  registry install).
- Re-run `scripts/smoke/verify-windows-bridge-smoke.ps1`. The
  script will then drive an MCP `initialize` + `tools/list` +
  `tools/call(health)` through the WWS04 bridge.

For BACKLOG WWS-B4..WWS-B9 (uninstall flow, multi-distro
prompt, full pair-accept handshake, credential broker, bootstrap
disable, CAP01): future enhancements; NOT publish-blockers.

## 8. Safety posture

All structural invariants from WWS01 §9 + §17 + the WWS04 /
WWS05 / WWS06 / WWS07 hard safety boundaries hold at WWS09
close:

- No MCP root shell.
- No Windows-native daemon / runtime claim.
- No macOS support claim.
- No musl / Alpine target claim.
- No network listener (daemon + MCP both local-only).
- No raw stream endpoint (every tool returns a bounded JSON
  envelope).
- No MCP-side command spawn
  (`rg "Command::new|Command::spawn|TcpListener|UdpSocket"
  crates/mcp` yields 4 doc / negative-assertion matches only).
- No MCP-side file reads
  (`rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end"
  crates/mcp/src` yields no matches).
- No shell bridge (`command_start_combed` argv-only +
  shell-interpreter deny list).
- No automatic password entry (`pty_command_write_stdin`
  rejects secret-prompt-shaped writes per TC44).
- No sudo / `sudo -S` / password prompt anywhere in the WWS
  chain. `cli-static-guards.test.js` static guard enforces no
  `spawn('sudo', ...)`, no `process.env.PASSWORD` reads, no
  `require('credential_broker')` import.
- No credentials passed through LLM chat / MCP args / MCP
  responses / buckets / logs / audit metadata / env vars /
  Cursor config / `setup.json` / `pair.json`.
- No active `.cursor/mcp.json` committed anywhere in the repo
  (recursive scan in `cli-static-guards.test.js` and
  `cursor-static-guards.test.js`).
- `npm-bootstrap-publish.yml` committed but NEVER dispatched.
- `release-please.yml` publish jobs correctly skipped on every
  push because no `feat:` / `fix:` commit has landed.
- Token surface remains ZERO during normal operation:
  `NPM_TOKEN_TC` is referenced only inside the bootstrap
  workflow YAML (guarded behind `inputs.dry_run` semantics);
  `CARGO_REGISTRY_TOKEN_TC` and `RELEASE_PLEASE_TOKEN_TC`
  remain UNUSED.
- No postinstall downloader. No `cargo install -P registry`. No
  crates.io publish.

## 9. Recommendation

**Conditional Go preserved.** Ready to dispatch
`npm-bootstrap-publish.yml` with `workflow_dispatch` in
`dry_run=true` mode ONLY, after the operator confirms the
npmjs.com precondition (org claim + trusted-publisher
configuration; see §7).

Real publish (`dry_run=false` OR a release-please PR merge)
requires SEPARATE explicit operator approval AFTER:

1. Dry-run evidence is captured and reviewed (workflow logs +
   pack tarball SHAs + version sync confirmation).
2. The operator explicitly accepts that:
   - The Cursor GUI provider live smoke remains `Not Run` AND
     the beta posture therefore stays `Conditional Go` post-publish.
   - The Windows -> WSL MCP bridge round-trip will remain
     `Not Run` until an operator installs the WSL-side runtime
     (via `terminal-commander setup cursor-wsl
     --install-wsl-runtime` OR manual `npm install -g
     terminal-commander` inside WSL) and re-runs the WWS07
     smoke.
   - Beta-posture promotion `Conditional Go` -> `Go` is gated on
     at least one provider live smoke transcript (Cursor /
     Codex CLI / Claude Code) — none of these are PASS yet.

**Do NOT attempt real publish (dry_run=false OR release-please
PR merge) at this WWS09 close.** The current commit is suitable
for a bootstrap dry-run only.

## 10. Exact next operator action

1. Confirm npmjs.com precondition: claim
   `@terminal-commander` org; configure trusted publisher per
   `docs/release/npm-trusted-publishing-contract.md` §8 for all
   three names; record screenshots in a follow-up evidence file.
2. (Optional, if choosing bootstrap path) Dispatch
   `npm-bootstrap-publish.yml` with `dry_run=true`:
   ```text
   gh workflow run npm-bootstrap-publish.yml \
     --ref main \
     -f dry_run=true \
     -f release_tag=v0.1.0-beta.1
   ```
   Wait for completion. Capture the workflow run URL + pack
   tarball SHAs + version-sync log lines. Attach to a follow-up
   evidence file.
3. (Optional, if choosing OIDC path) Land a Conventional-Commits
   `feat:` or `fix:` commit on `main`. Let release-please open
   a release PR. Review the PR. Merge it. The publish jobs run
   automatically with OIDC + provenance + no `NPM_TOKEN`.
4. After first publish (whichever path), capture three
   `npm view <name> version` lines that return `0.1.0-beta.1`
   (or whatever release-please bumped to). Attach to the
   evidence file.
5. Promote the WWS07 PowerShell smoke from `runtime_missing` to
   PASS by either running `terminal-commander setup cursor-wsl
   --install-wsl-runtime` (which will now succeed) OR doing the
   manual `npm install -g terminal-commander` inside WSL, then
   re-running the smoke and capturing the MCP `initialize` +
   `tools/list` + `tools/call(health)` lines.
6. Run the Cursor GUI provider smoke per §5; capture the chat
   transcript. ONLY THEN may the beta posture promote
   `Conditional Go` -> `Go`.
7. Address BACKLOG WWS-B8 (disable / rotate bootstrap workflow)
   if the bootstrap path was used in step 2.

## 11. Negative-surface confirmations at WWS09 close

- No `crates/**` change at WWS09. No `Cargo.toml` / `Cargo.lock`
  change.
- No `.github/**` change. No workflow dispatch.
- No `scripts/**` change (the WWS07 PowerShell smoke is
  byte-identical).
- No `packages/*/package.json` change.
- No `packages/terminal-commander/{bin,lib,test}/**` change.
- No platform-package change.
- No `examples/provider-harness/cursor/*.json` change.
- No active `.cursor/mcp.json` anywhere in the repo.
- No new MCP tool. The 29-tool TC45 catalogue is unchanged.
- No daemon change. No IPC change. No new spawn target.
- No file write outside the allowed docs.
- No sudo. No password. No credential broker. No env-var-secret
  echo.
- No CAP01 / capability-registry implementation. Doctrine
  carried forward only; not started.
- No version bump. All three packages remain `0.1.0-beta.1`.
- No npm publish. All three names remain `E404`.
- No `npm-bootstrap-publish.yml` dispatch.
- TC48 + NPM09 + NPM10 + WWS01..WWS08 `Conditional Go` posture
  preserved.

## 12. Chain terminal state

The `terminal-commander-windows-wsl-bridge` chain is **CLOSED**
at WWS09. No follow-up goal opens automatically.

The next operator-driven milestone is the first live npm
publish (see §10). After that publish lands and the Cursor GUI
provider smoke transcript is captured, the beta posture may
promote `Conditional Go` -> `Go` and the project can be
considered out of beta.

The doctrine carry-forward (`CAP01-capability-registry-contract.md`
— tentacles = programmable probes = policy-gated capability
executors) is recorded in BACKLOG WWS-B9 and ROADMAP. CAP01 is
NOT scheduled at this WWS09 close.

## 13. Acceptance against the WWS09 mini-spec

- [x] Final report exists at `docs/release/windows-wsl-bridge-final-report.md`
      (this file).
- [x] Per-goal chain summary §1.
- [x] npm registry probe result §2 (all three names E404).
- [x] Rust + smoke + pack gate snapshot §3.
- [x] Bridge invariants confirmed §4 (D-01..D-15).
- [x] Cursor live smoke status §5 (Not Run, honestly).
- [x] Active blockers / operator preconditions §7.
- [x] Recommendation locked with rationale §9
      (Conditional Go; dry-run-ready; real publish gated on
      operator approval after dry-run evidence + npmjs.com
      precondition).
- [x] No `Not Run` evidence promoted to PASS.
- [x] No publish.
- [x] TC48 + NPM09 + NPM10 token-policy invariants preserved.
- [x] Chain CLOSED at WWS09.

## 14. Cross-links

- `docs/release/windows-wsl-bridge-contract.md` — WWS01 contract
  (commit `6220eb2`).
- `docs/release/npm-binary-packaging-contract.md` — NPM02 +
  WWS02 §13b amendment.
- `docs/release/npm-trusted-publishing-contract.md` — NPM07
  OIDC + operator preconditions §8.
- `docs/release/npm-distribution-final-report.md` — NPM09
  closure + WWS chain follow-up §11.
- `RELEASE_CHECKLIST.md` — beta gates + Windows + WSL bridge
  section.
- `BACKLOG.md` — P2 WWS-B1..WWS-B9 follow-ups.
- `RISK_REGISTER.md` — R-WWS-01..R-WWS-10 active risks.
- `ROADMAP.md` — WWS01..WWS09 table.
- `docs/integrations/cursor.md` §11a / §11c / §11d — auto-generated
  config + CLI surface + WWS07 smoke checklist.
- `scripts/smoke/verify-windows-bridge-smoke.ps1` — WWS07 smoke
  script.
