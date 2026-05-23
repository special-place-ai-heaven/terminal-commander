---
goal_id: WWS02
title: Root Npm Package Win32 Bridge Contract
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 1 - Package contract
status: "Completed"
depends_on: ["WWS01"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: "2026-05-23T14:30:00+00:00"
completed_at: "2026-05-23T15:30:00+00:00"
completion_commit: "1da40f3"
blocked_reason: ""
source_refs:
  - "WWS01 windows-wsl bridge contract (commit 6220eb2; docs/release/windows-wsl-bridge-contract.md)"
  - "WWS01 binding decisions D-01..D-15"
  - "docs/release/npm-binary-packaging-contract.md (NPM02 §3 + §4; root os field amended at WWS02)"
  - "packages/terminal-commander/package.json (current Linux-only os field)"
  - "packages/terminal-commander/lib/resolve-binary.js (current Linux-only resolver)"
  - "packages/terminal-commander/bin/terminal-commanderd.js (current refuses on unsupported_platform)"
  - "packages/terminal-commander/bin/terminal-commander-mcp.js (current refuses on unsupported_platform)"
  - "packages/terminal-commander/bin/terminal-commander.js (current refuses on unsupported_platform)"
  - "WWS02 prep amendment 2026-05-23: scope widened to include bin/*.js shim trio, docs/integrations/**, and expanded test/acceptance gates per pivot directive (terminal-commander as MCP control plane with environment runners)"
risk_level: "high"
---

# WWS02 - Root Npm Package Win32 Bridge Contract

## Branch Guard

```text
main
```

## Mission Context

WWS01 (commit `6220eb2`) locked the design. WWS02 amends the **root**
npm package metadata + resolver + bin-shim trio so the package is
installable on Windows as a bridge / setup surface for an MCP
control plane that runs against a WSL / Linux environment runner.
The two scoped platform packages remain Linux-only (NPM02 §3
unchanged). On Windows hosts npm correctly skips both Linux platform
packages via `os` / `cpu` filtering; the root resolver returns a
new `bridge_required` reason; each of the three bin shims branches
on win32 to a bounded, **non-executing** placeholder behavior.

Critically: WWS02 does NOT implement the actual `wsl.exe` bridge
spawn. WWS01 D-04 owns the bridge invariants (`shell: false`,
argv array, distro whitelist, `windowsHide: true`); the
implementation lives in WWS04 (`lib/wsl/spawn.js`). WWS02 only
wires the resolver branch + bounded refusal / bridge_required
indicator into the three shims so a Windows `npm install -g
terminal-commander` does not 1) fail with `EBADPLATFORM` and 2)
does not produce a Windows binary claim it cannot honor.

This is a **prep-amendment-revised** goal file. The amendment
(committed in this same chore commit) widens scope versus the
original WWS02 stub to include the three bin shims, the resolver
unit tests, `docs/integrations/**` cross-links, and the expanded
acceptance / verification gate list from the WWS01 final report.

## Pivot statement (recorded for WWS02..WWS09 reference)

Terminal Commander is an **MCP control plane** that can prepare
and attach **environment runners** inside target hosts. Windows is
**bridge / setup only**. WSL / Linux is the **real runtime host**.
WWS02 is the package-contract slice of this pivot; later goals
(WWS03..WWS09) build the discovery, spawn, Cursor config write,
setup CLI, smoke, and final review on top of it.

## Mini-Spec

objective:
- Update `packages/terminal-commander/package.json`:
  - widen `os` to `["linux", "win32"]`
  - keep `name` `terminal-commander`
  - keep `version` at `0.1.0-beta.1` (no bump in WWS02)
  - keep `optionalDependencies` exact-pinned to
    `@terminal-commander/linux-x64` + `@terminal-commander/linux-arm64`
    at `0.1.0-beta.1`
  - keep `engines.npm: ">=8"` (so `os` / `cpu` filtering works on
    `optionalDependencies`)
  - keep `files` whitelist
- Update `packages/terminal-commander/lib/resolve-binary.js` to add
  the `bridge_required` reason when `process.platform === 'win32'`.
- Update the three bin shims
  (`packages/terminal-commander/bin/terminal-commanderd.js`,
  `terminal-commander-mcp.js`, `terminal-commander.js`) so each
  handles the new `bridge_required` reason without performing any
  actual `wsl.exe` invocation (deferred to WWS04). Specifically:
  - `terminal-commanderd.js`: on `bridge_required`, refuse with a
    single bounded stderr line + exit code 64. Message matches
    WWS01 §4.1 / §12.
  - `terminal-commander-mcp.js`: on `bridge_required`, exit 64
    with a bounded "bridge not yet implemented; pending WWS04"
    stderr line. WWS04 replaces this stub with the real
    `lib/wsl/spawn.js` dispatch.
  - `terminal-commander.js` (admin CLI): on `bridge_required`,
    exit 64 with a bounded "Windows host: run 'terminal-commander
    setup cursor-wsl' (pending WWS06)" hint. WWS06 replaces this
    stub with the real setup/doctor subcommands.
- Update `packages/terminal-commander/test/resolve-binary.test.js`
  (expand from 12 to ~16+ cases; add win32 x64 + win32 arm64
  `bridge_required` cases; keep linux-x64 / linux-arm64 unchanged;
  keep darwin / unsupported still `unsupported_platform`).
- Update `packages/terminal-commander/README.md` to describe the
  dual posture honestly (Linux runtime + Windows bridge/setup
  surface). No claim of Windows-native daemon. Cross-link
  `docs/release/windows-wsl-bridge-contract.md` (WWS01).
- Update `docs/release/npm-binary-packaging-contract.md` (NPM02)
  with a clearly-labeled "WWS02 amendment" section that records
  the single field change (root `os` widening) without rewriting
  any other NPM02 lock.
- Cross-link from `docs/integrations/cursor.md` to the new dual
  posture (NO config-writer mention — that is WWS05). A single
  sentence + link to `docs/release/windows-wsl-bridge-contract.md`
  is sufficient.

non_goals:
- Do NOT implement the actual `wsl.exe` bridge spawn
  (`lib/wsl/spawn.js` belongs to WWS04).
- Do NOT implement WSL distro detection (`lib/wsl/detect.js`
  belongs to WWS03).
- Do NOT implement the WSL runtime doctor (`lib/wsl/doctor.js`
  belongs to WWS03).
- Do NOT implement the Cursor config writer (`lib/cursor/**`
  belongs to WWS05).
- Do NOT implement the setup/doctor/pair CLI subcommands
  (`lib/cli/**` belongs to WWS06).
- Do NOT implement automatic WSL install. Default behavior on
  Windows in WWS02 is exit-64 with bounded hints; nothing
  executes inside WSL during WWS02 install / invocation.
- Do NOT implement pairing.
- Do NOT bump `version`. The shared `0.1.0-beta.1` semver stays
  pinned across root + two platform packages.
- Do NOT edit `packages/terminal-commander-linux-x64/package.json`
  or `packages/terminal-commander-linux-arm64/package.json`.
- Do NOT edit `.github/workflows/**` or
  `.github/release-please-config.json` /
  `.release-please-manifest.json`.
- Do NOT add `postinstall` to any `package.json`.
- Do NOT add a Rust compile step during `npm install`.
- Do NOT claim macOS support.
- Do NOT claim Windows-native runtime / daemon / PTY / UDS.
- Do NOT add new MCP tools (29-tool TC45 catalogue unchanged).
- Do NOT change the daemon, the MCP adapter, the admin CLI Rust
  binary, the IPC surface, or any probe.
- Do NOT dispatch `npm-bootstrap-publish.yml`.
- Do NOT publish to npm.
- Do NOT touch `NPM_TOKEN_TC`, `CARGO_REGISTRY_TOKEN_TC`, or
  `RELEASE_PLEASE_TOKEN_TC`.

allowed_files_or_area:
- `packages/terminal-commander/package.json`
- `packages/terminal-commander/lib/resolve-binary.js`
- `packages/terminal-commander/bin/terminal-commanderd.js`
- `packages/terminal-commander/bin/terminal-commander-mcp.js`
- `packages/terminal-commander/bin/terminal-commander.js`
- `packages/terminal-commander/test/**`
- `packages/terminal-commander/README.md`
- `docs/release/**` (refresh NPM02 amendment section + cross-link
  to WWS01 contract; do NOT edit
  `npm-bootstrap-first-publish.md` or
  `npm-trusted-publishing-contract.md`)
- `docs/integrations/**` (single cross-link sentence + reference
  only; no config-writer behavior described)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS02-*.md`
- `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md`
  (status row only)
- `.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md`
  (status row only)

allowed_for_inspection_only:
- `packages/terminal-commander-linux-x64/package.json` (verify
  unchanged after WWS02 lands)
- `packages/terminal-commander-linux-arm64/package.json` (verify
  unchanged after WWS02 lands)

forbidden_files:
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `scripts/**`
- `.github/**` (workflows + release-please config + manifest)
- `packages/terminal-commander-linux-x64/**` (no edits; inspection only)
- `packages/terminal-commander-linux-arm64/**` (no edits; inspection only)
- `packages/terminal-commander/lib/wsl/**` (WWS03 / WWS04)
- `packages/terminal-commander/lib/cursor/**` (WWS05)
- `packages/terminal-commander/lib/cli/**` (WWS06)
- `.github/release-please-config.json` /
  `.github/.release-please-manifest.json`
- `.github/workflows/npm-bootstrap-publish.yml` (NPM10; remains
  pending and undispatched)
- `.github/workflows/release-please.yml` (NPM06 + NPM07)
- `.github/workflows/npm-binary-build.yml` (NPM05)
- secrets / tokens / private usernames / private absolute paths

contracts_or_interfaces:

### Root `package.json`
- `name: "terminal-commander"` (unchanged)
- `version: "0.1.0-beta.1"` (unchanged — no bump in WWS02)
- `os: ["linux", "win32"]` (widened from `["linux"]`; sole NPM02
  field change)
- `bin` map unchanged (three entries: `terminal-commanderd`,
  `terminal-commander-mcp`, `terminal-commander`)
- `files` whitelist preserved (`bin/`, `lib/`, `README.md`,
  `LICENSE`)
- `optionalDependencies` exact-pin to `@terminal-commander/linux-x64`
  + `@terminal-commander/linux-arm64` at `0.1.0-beta.1` (unchanged)
- `engines.node >= 18`, `engines.npm >= 8` (unchanged)
- NO `postinstall` script (NPM02 §4.2 unchanged)
- NO `cpu` field on root (unchanged)

### Platform packages (read-only inspection)
- `@terminal-commander/linux-x64` and
  `@terminal-commander/linux-arm64` remain
  `os: ["linux"]`, `cpu: ["x64"|"arm64"]`, no Windows artifact,
  no postinstall, no Cargo. WWS02 verification confirms these
  files are unchanged.

### `lib/resolve-binary.js`
Returns one of these results, NEVER spawns, NEVER opens sockets,
NEVER reads files outside `require.resolve` of the platform
package's own `package.json`:

| Reason | Trigger | shim must |
|--------|---------|-----------|
| `ok` | `linux` + (`x64` OR `arm64`) AND platform package resolves | spawn `binaryPath` with `shell: false`, `stdio: 'inherit'` |
| `bridge_required` | `win32` (any arch) | print bounded bridge-stub stderr + exit 64 (no `wsl.exe` invocation in WWS02; full impl in WWS04) |
| `platform_package_missing` | `linux` + supported arch but `require.resolve` of platform `package.json` throws | print bounded stderr citing `optionalDependencies` skip + exit 64 |
| `unsupported_platform` | any platform / arch combination not covered above (notably `darwin`, `freebsd`, etc.) | print bounded stderr citing supported targets + exit 64 |
| `invalid_binary` | resolver invoked with binary name not in `ALLOWED_BINARIES` | resolver-internal error; shims do not pass arbitrary names |

The resolver's `SUPPORTED_TARGETS` table MAY be extended with
metadata documenting the win32 bridge entry, but the field MUST
NOT claim a binary path on Windows.

### Bin shims (win32 branches added; Linux branches unchanged)

#### `bin/terminal-commanderd.js`
On `bridge_required`:
```
terminal-commander: terminal-commanderd runs only inside Linux / WSL. Run from a WSL distro, or use 'terminal-commander setup cursor-wsl' on Windows to configure the bridge.
```
Exit 64. No `child_process.spawn` call. No `wsl.exe` reference in
code (the actual bridge belongs to `terminal-commander-mcp`, not
to the daemon shim).

#### `bin/terminal-commander-mcp.js`
On `bridge_required`:
```
terminal-commander: Windows host bridge mode is not yet implemented (pending WWS04). Run 'terminal-commander-mcp' from inside a WSL distro, or wait for the WWS04 release.
```
Exit 64. WWS04 replaces this stub with the real
`lib/wsl/spawn.js` dispatch.

#### `bin/terminal-commander.js`
On `bridge_required`:
```
terminal-commander: Windows host detected. Setup / doctor subcommands are pending WWS06. To use Terminal Commander today, run 'npm install -g terminal-commander' from inside a WSL distro.
```
Exit 64. WWS06 replaces this stub with the real setup / doctor /
pair subcommands.

All three messages are SINGLE bounded stderr lines. No stack
traces. No environment dumps. No absolute paths to stdout. No
secrets.

### `test/resolve-binary.test.js`
Expand the existing 12 cases to ~16+ to cover:
- `linux/x64` `ok` (existing)
- `linux/arm64` `ok` (existing)
- `linux/x64` `platform_package_missing` (existing)
- `linux/arm64` `platform_package_missing` (existing)
- `linux/x64` with invalid binary name (existing `invalid_binary`)
- `darwin/x64` `unsupported_platform` (existing)
- `darwin/arm64` `unsupported_platform` (existing)
- (and any other existing cases)
- NEW: `win32/x64` `bridge_required`
- NEW: `win32/arm64` `bridge_required`
- NEW: `win32/x64` with invalid binary name still returns
  `invalid_binary` (resolver validates binary name before
  platform branching)
- NEW: `freebsd/x64` still `unsupported_platform` (regression
  guard — adding win32 does not accidentally bridge unknown
  platforms)
- NEW: `formatResolveError` returns bounded one-line message for
  `bridge_required` and the message ASCII-only and includes the
  word `bridge`.

### `README.md` (package-level)
Update to describe the dual posture honestly:
- Linux + WSL: full runtime — daemon, MCP stdio adapter, admin
  CLI all native.
- Windows: bridge / setup surface only (no Rust binary, no
  daemon, no PTY).
- Cross-link `docs/release/windows-wsl-bridge-contract.md` (WWS01).
- NO claim of Windows-native runtime.

### `docs/release/npm-binary-packaging-contract.md` (NPM02)
Add a clearly-labeled "WWS02 amendment (2026-05-23)" subsection
under the existing risk / amendment narrative. Record:
- Single field change: root `os: ["linux"]` -> `["linux", "win32"]`.
- Every other NPM02 lock (package names, version, install
  command, platform packages, `optionalDependencies` shape,
  release contract, OIDC trusted publishing, Cursor stanza,
  safety contract) explicitly preserved.
- Cross-link to `docs/release/windows-wsl-bridge-contract.md`
  (WWS01).

### `docs/integrations/cursor.md`
Add a single sentence + link pointing at the new dual posture.
No config-writer description (WWS05). No `setup cursor-wsl`
description (WWS06). The Windows operator path remains the
manual `wsl.exe`-direct stanza for the duration of WWS02.

invariants:
- No `crates/**` change.
- No `Cargo.toml` / `Cargo.lock` change.
- No `.github/**` change.
- No runtime behavior change.
- No daemon / MCP adapter / probe / IPC surface change.
- No new MCP tool.
- No version bump (`0.1.0-beta.1` remains pinned).
- Platform-package `os` / `cpu` / `bin` / `files` unchanged.
- Root `optionalDependencies` exact-pin preserved.
- Root has no `postinstall` script.
- Root has no Rust compile step at `npm install`.
- No `wsl.exe` invocation in WWS02 (deferred to WWS04).
- No JSON file written by any WWS02 code on Windows install
  (Cursor config writer is WWS05).
- No `Not Run` evidence promoted to PASS.
- WWS01 binding decisions D-01..D-15 preserved verbatim.
- TC48 + NPM09 + NPM10 `Conditional Go` beta posture preserved.
- MCP guard greps remain clean (WWS02 touches no Rust source).
- Token-surface unchanged: `NPM_TOKEN_TC`, `CARGO_REGISTRY_TOKEN_TC`,
  `RELEASE_PLEASE_TOKEN_TC` all remain UNUSED.

acceptance_criteria:
- Resolver returns `ok` for `linux/x64` and `linux/arm64` with the
  matching platform package installed (existing behavior preserved).
- Resolver returns `bridge_required` for `win32/x64` and
  `win32/arm64`, NOT `unsupported_platform`.
- Resolver returns `unsupported_platform` for `darwin/x64`,
  `darwin/arm64`, `freebsd/*`, and all other platform/arch
  combinations.
- `terminal-commanderd` shim, when given a `bridge_required`
  resolver result, refuses with the single bounded stderr line in
  `contracts_or_interfaces` §"bin/terminal-commanderd.js" and
  exits with code 64. No `child_process.spawn` call occurs.
- `terminal-commander-mcp` shim, when given a `bridge_required`
  resolver result, refuses with the single bounded stderr line in
  §"bin/terminal-commander-mcp.js" and exits with code 64. No
  `wsl.exe` invocation occurs (deferred to WWS04).
- `terminal-commander` admin CLI shim, when given a
  `bridge_required` resolver result, refuses with the single
  bounded stderr line in §"bin/terminal-commander.js" and exits
  with code 64. No setup / doctor / pair subcommand executes
  (deferred to WWS06).
- Platform packages remain `os: ["linux"]`; their `package.json`
  diffs after WWS02 are empty.
- `npm pack ./packages/terminal-commander --dry-run` is clean and
  carries the same file count / version (`0.1.0-beta.1`) as
  before WWS02.
- `npm pack ./packages/terminal-commander-linux-x64 --dry-run` and
  `npm pack ./packages/terminal-commander-linux-arm64 --dry-run`
  are clean and unchanged in file count and version.
- `(cd packages/terminal-commander && npm test)` passes with
  ~16+ resolver tests (12 existing + 4+ new win32 / bridge /
  formatResolveError cases) — count >= 16.
- `bash scripts/smoke/verify-runtime-smoke.sh` 8/8 PASS unchanged.
- `bash scripts/smoke/verify-npm-local-install.sh` NPM04 SUCCESS,
  12 PASS lines (Linux host smoke; the new win32 branch is unit-
  tested only, since the verification host is Linux/WSL).
- Forbidden-paths diff `--ignore-cr-at-eol` over `crates/`,
  `Cargo.toml`, `Cargo.lock`, `rules/`, `config/`, `scripts/`,
  `.github/`, and `packages/terminal-commander-linux-*` is empty.
- MCP guard greps remain clean.
- Secret-leak grep over committed paths returns 0 matches.
- Beta posture remains `Conditional Go`.

evidence_required:
- Branch evidence (`main`).
- File paths.
- Resolver test results (count + pass/fail).
- `npm pack` dry-run output for all three packages (file count +
  version line).
- `verify-runtime-smoke.sh` tail.
- `verify-npm-local-install.sh` tail.
- Forbidden-paths diff (`git diff --ignore-cr-at-eol --shortstat`
  over the forbidden-paths list above) — must be empty.
- MCP guard grep results.
- Secret-leak grep results.
- Token-surface check (the three `*_TC` secrets remain UNUSED in
  any new/edited code).

stop_conditions:
- Branch is not `main`.
- Implementing the win32 branch would require running real
  `wsl.exe` code on the install path (postinstall is banned;
  bridge spawn deferred to WWS04).
- Any change would require editing a `forbidden_files` path —
  surface a prep amendment and stop before continuing.
- A resolver test case would require shipping a Windows binary
  in any platform package.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo nextest run --workspace
bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh
( cd packages/terminal-commander && npm test )
npm pack ./packages/terminal-commander --dry-run
npm pack ./packages/terminal-commander-linux-x64 --dry-run
npm pack ./packages/terminal-commander-linux-arm64 --dry-run
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
rg "NPM_TOKEN_TC|CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|sk-[A-Za-z0-9]{10}|ghp_[A-Za-z0-9]{10}|npm_[A-Za-z0-9]{20}" README.md docs examples packages .agent/goals || true
git diff --ignore-cr-at-eol --shortstat -- crates/ Cargo.toml Cargo.lock rules/ config/ scripts/ .github/ packages/terminal-commander-linux-x64/ packages/terminal-commander-linux-arm64/
```

## Task Prompt

Run WWS02 only on branch `main`. Root package metadata + resolver +
three bin shims (win32 branches only) + resolver tests + package
README + NPM02 amendment section + single docs/integrations
cross-link. No bridge shim spawn implementation (deferred to
WWS04). No Cursor config writer (deferred to WWS05). No setup CLI
(deferred to WWS06). No new MCP tool. No version bump. No publish.
No workflow dispatch. No `crates/**` change.

The bin-shim win32 branches MUST exit-64 with a bounded one-line
stderr message and MUST NOT invoke `wsl.exe`. WWS04 owns the real
bridge invocation; WWS02 wires only the resolver branch and the
honest "not yet implemented" refusal.

## Final Report Format

Objective / Changes / Files changed / Resolver test count + results /
Bin-shim win32-branch evidence (the three exact stderr lines + exit
codes from a unit test or scripted run) / npm pack results for all
three packages / verify-runtime-smoke.sh tail / verify-npm-local-install.sh
tail / Forbidden-paths diff result / MCP guard grep results /
Secret-leak grep result / Token-surface confirmation / Verification /
Evidence / Verified work commit hash / Status commit hash / Known
gaps / Next goal (WWS03).
