# WWS01 — Windows + WSL install UX contract for Terminal Commander

Status: WWS01 deliverable.
Branch: `main`.
Date: 2026-05-23.
Depends on: NPM02 ([`docs/release/npm-binary-packaging-contract.md`](npm-binary-packaging-contract.md)), NPM08 ([`docs/integrations/cursor.md`](../integrations/cursor.md)), NPM09 ([`docs/release/npm-distribution-final-report.md`](npm-distribution-final-report.md)), NPM10 ([`docs/release/npm-bootstrap-first-publish.md`](npm-bootstrap-first-publish.md)), TC44 runtime baseline (PTY / UDS Unix-only).

This document is the binding UX + package-contract spec the
`terminal-commander-windows-wsl-bridge` chain (WWS02–WWS09)
implements. WWS01 is documentation-only. It produces no JS code, no
Rust code, no `package.json` edit, and no workflow change. It locks
the open decisions registered in `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md`
into binding answers so WWS02 can amend NPM02 with a recorded prep
amendment instead of silently widening scope.

Language: ASCII only.

## 0. Provenance

- NPM02 commit `81f4ea1` — baseline npm packaging contract. Locked
  root `os: ["linux"]`. WWS01 amends ONE field in that contract
  (root `os`); every other NPM02 lock is preserved.
- NPM08 commit `6ab2343` — Cursor MCP integration docs + three
  copy-pasteable JSON examples (including the Windows → WSL bridge
  shape currently maintained by the operator manually).
- NPM09 commit (chain close) — recorded `Conditional Go`; all three
  npm names at E404; trusted-publisher operator preconditions
  (OP-1, OP-2) still open.
- NPM10 commit `9353fb4` — bootstrap-publish workflow committed
  but NOT dispatched.
- TC44 `non_goals` — Windows ConPTY and Windows-native PTY
  explicitly deferred. The runtime daemon and PTY surface remain
  Unix-only.
- User directive 2026-05-23: Windows host package is a
  bridge / setup surface only; the WSL / Linux platform packages
  remain the real runtime host; Cursor / harness sees one MCP
  entrypoint via the Windows shim.

## 1. Current state — what the user faces today (pre-publish)

### 1.1 Install paths recorded on `main`

| Path | Status | Where it lives |
|------|--------|----------------|
| `npm install -g terminal-commander` (published path) | NOT active. All three names at E404 on the registry. Operator preconditions OP-1, OP-2, OP-3 still open. | Future, gated on NPM10 dispatch OR NPM07 OIDC publish. |
| `scripts/smoke/verify-npm-local-install.sh` (local tarball) | Live. Linux host only. Builds release binaries, stages, packs, installs into a sandbox `--prefix`. | NPM04 verified work. |
| `cargo build --release -p terminal-commanderd -p terminal-commander-mcp -p terminal-commander-cli` (cargo-built path) | Live. Linux host only. | Always available where a Rust toolchain exists. |

### 1.2 Cursor configuration paths recorded on `main`

| File | Host topology | What it forces the operator to do |
|------|---------------|----------------------------------|
| `examples/provider-harness/cursor/mcp.global.native-linux.json` | Native Linux Cursor (or Cursor running inside WSL) | Copy file to `~/.cursor/mcp.json`. Daemon must already be running. |
| `examples/provider-harness/cursor/mcp.project.linux-wsl.json` | Workspace-scoped MCP on Linux / WSL | Copy file to `<workspace>/.cursor/mcp.json`. `TC_SOCKET` env block included for non-default `$TC_DATA`. |
| `examples/provider-harness/cursor/mcp.global.linux-wsl.json` | Windows Cursor → WSL bridge | Copy file to `%USERPROFILE%\.cursor\mcp.json`. Operator MUST substitute the distro name from `wsl --list --verbose`. Daemon MUST already be running INSIDE WSL. |

### 1.3 Friction the Windows operator hits today

| # | Friction | Source |
|---|----------|--------|
| 1 | `npm install -g terminal-commander` on Windows fails the `os: ["linux"]` filter on the root `package.json` (NPM03 layout). | `packages/terminal-commander/package.json` `os` field. |
| 2 | The Windows operator has to know they need WSL2 BEFORE installing — there is no Windows-host hint. | NPM08 `docs/integrations/cursor.md` §1, §6. |
| 3 | The Windows operator has to install Terminal Commander INSIDE a specific WSL distro (`npm install -g terminal-commander` from WSL shell). | NPM02 §3.3 "WSL2 fallback". |
| 4 | The Windows operator has to know which distro Cursor will use, hard-code that distro name into JSON, and update it if the distro is renamed. | `examples/provider-harness/cursor/mcp.global.linux-wsl.json` `args: ["-d", "Ubuntu-24.04", ...]`. |
| 5 | The Windows operator has to start the daemon manually inside WSL before opening Cursor. | NPM08 `docs/integrations/cursor.md` §3, §6. |
| 6 | The Windows operator has to edit `mcp.json` by hand, then validate JSON, then restart Cursor. | NPM08 §2, §6. |
| 7 | The Windows operator has zero diagnostic surface (`doctor`-style command). | None today. |

### 1.4 What is OK today (do not regress)

- The MCP surface is unchanged (29 tools, no raw stream lane).
- The npm shim is `child_process.spawn` with `shell: false` and
  `stdio: 'inherit'` (NPM03 contract; preserved by WWS04).
- The runtime is Unix-only (TC44). WSL distro hosts the daemon,
  the probes, the audit, the SQLite store, the PTY surface, and
  the UDS.
- Two platform packages (`@terminal-commander/linux-x64`,
  `@terminal-commander/linux-arm64`) carry the real binaries.
- No `postinstall` downloader exists.
- No `crates.io` publish exists.
- Token-surface is minimal: `NPM_TOKEN_TC` exists only inside the
  NPM10 bootstrap workflow; `CARGO_REGISTRY_TOKEN_TC` and
  `RELEASE_PLEASE_TOKEN_TC` are unused.

## 2. Desired user journey (target UX after WWS02–WWS09)

### 2.1 Windows-host happy path

**Amended by the current explicit setup posture** ([`install-bootstrap-contract.md`](install-bootstrap-contract.md)):

```powershell
npm install -g terminal-commander
terminal-commander setup harness
```

The npm install is passive. The explicit setup command merges MCP config for
detected harnesses and persists setup state. It does not rely on npm lifecycle
scripts, CMD, PowerShell, or hidden-window process options.

Legacy two-step path (deprecated; `setup cursor-wsl` delegates to bootstrap):

```powershell
npm install -g terminal-commander
terminal-commander setup cursor-wsl   # prints migration notice
```

After install, the operator can launch Cursor and ask the chat
panel to list MCP tools — the 29-tool TC45 catalogue is visible
through the Windows shim into WSL.

### 2.2 WSL-host happy path (also covers Linux-native)

```sh
# Step 1 — install the same root package inside the WSL distro
# (or on a Linux host). npm picks the matching platform package
# (linux-x64 / linux-arm64) via optionalDependencies.
npm install -g terminal-commander

# Step 2 — start the daemon (existing TC46 flow).
export TC_DATA="${XDG_STATE_HOME:-$HOME/.local/state}/terminal-commander"
mkdir -p "$TC_DATA"
terminal-commanderd --data-dir "$TC_DATA" start --mode ipc-server

# Step 3 — structured diagnostic.
terminal-commander doctor
```

### 2.3 Cursor MCP config after `setup cursor-wsl`

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

Identical to the native-Linux example. The Windows operator never
edits `wsl.exe` argv themselves.

### 2.4 What the Cursor / LLM harness sees

ONE MCP server, ONE entrypoint command (`terminal-commander-mcp`),
ONE bounded JSON envelope per tool call. No knowledge of WSL, no
knowledge of distro names, no raw stream lane. The 29 tools from
TC45 remain the only surface. The bridge is invisible to the LLM.

## 3. Harness / LLM-facing model

| Property | Behavior |
|----------|----------|
| Tool catalogue | 29 tools (TC45), unchanged. |
| Transport | MCP stdio (rmcp 1.7.0). |
| Server name | `terminal-commander`. |
| Server command | `terminal-commander-mcp`. |
| Argv | none. `setup cursor-wsl` writes a no-args stanza by default. |
| Env | optional `TC_SOCKET` only (non-secret transport variable). |
| Auto-execute | NEVER. Operator confirms each tool call in Cursor. |
| Raw stream lane | NONE. Every response remains a bounded JSON envelope. |

The LLM never learns whether it is on Windows or Linux. The shim is
a transport detail.

## 4. Windows host package responsibilities (new at WWS01)

The root `terminal-commander` npm package, when installed on Windows
(`process.platform === 'win32'`), MUST behave as follows.

### 4.1 What ships on Windows

| Component | Behavior |
|-----------|----------|
| `bin/terminal-commanderd` (shim) | **Refuses** with a bounded one-line stderr message: `terminal-commander: terminal-commanderd runs only inside Linux / WSL. Run from a WSL distro, or use 'terminal-commander setup cursor-wsl' on Windows to configure the bridge.` Exit code 64 (matches NPM03 unsupported-platform exit). |
| `bin/terminal-commander-mcp` (shim) | Bridge shim. Spawns `wsl.exe -d <distro> bash -lc terminal-commander-mcp` with `shell: false`, inherits stdio, mirrors exit code. Distro chosen from setup-state file written at `setup cursor-wsl` time, OR from `--distro` argv override, OR from default WSL distro detected via `wsl.exe -l -v`. Fails LOUDLY (exit 64) with an actionable message if no distro is reachable. |
| `bin/terminal-commander` (admin CLI shim) | On Windows, exposes the new setup subcommands (`setup cursor-wsl`, `doctor`, optional `pair create` / `pair accept`). Does NOT bridge arbitrary admin CLI subcommands into WSL; the Windows admin CLI is a Windows-side surface, not a remote shell. |

### 4.2 What does NOT ship on Windows

- NO Rust binary.
- NO Windows-native daemon claim.
- NO Windows-native PTY.
- NO ConPTY adapter.
- NO `terminal-commander.exe` direct invocation.
- NO Windows-native UDS or named-pipe transport.

### 4.3 Platform-package layout on Windows

- The two existing platform packages remain Linux-only
  (`os: ["linux"]`, `cpu: ["x64"|"arm64"]`). npm honors `os` /
  `cpu` filters on `optionalDependencies`, so an npm install on
  Windows simply SKIPS both platform packages.
- The Windows install lands the root wrapper alone: three JS shims
  + `lib/resolve-binary.js` (existing, unchanged) + new
  `lib/wsl/**` helpers (WWS03 / WWS04) + new `lib/cursor/**`
  helpers (WWS05) + new `lib/cli/**` CLI subcommands (WWS06).
- The resolver returns `{ reason: "bridge_required" }` on Windows;
  the bridge shim takes over from there.

### 4.4 Bridge invariants

| Invariant | Why |
|-----------|-----|
| `child_process.spawn` with `shell: false` only. | No shell interpolation. No quoting bugs. Matches NPM03 shim discipline. |
| Argv passed as an array (`["-d", distro, "bash", "-lc", "terminal-commander-mcp"]`). | No string concatenation; no operator input is interpolated into the command line. |
| Distro names validated against a whitelist of the strings returned by `wsl.exe -l -v`. | Refuses arbitrary operator-supplied distro names that did not appear in the live `wsl.exe -l -v` output. Prevents `; calc.exe` style injection if the setup file is tampered with. |
| `stdio: 'inherit'` on stdin / stdout / stderr. | rmcp framing flows transparently through the `wsl.exe` pipe. |
| No environment secrets passed via argv. | Same NPM03 boundary. |
| No hidden-window spawn option (`windowsHide`, `CREATE_NO_WINDOW`, `SW_HIDE`). | Avoids stealth-like process behavior that endpoint tools reasonably flag. |
| Exit code mirroring. | Operator sees the WSL-side exit code. |

#### Daemon runtime spawn rule (Windows)

**Split by site:** The §4.4 “no hidden-window” invariant applies **only** to the
JS bridge spawn path (`packages/terminal-commander/lib/wsl/spawn.js`, WWS04
setup-time hosting). Bridge visibility is load-bearing EDR legitimacy — the
operator sees `wsl.exe` start the MCP session.

**Daemon runtime** children (`command_start_combed` via `ProcessProbe::spawn` in
`crates/probes/src/process.rs`, and the one-shot `wsl.exe` bootstrap in
`wsl_username` in `crates/daemon/src/environment/wsl.rs`) **must** use
`CREATE_NO_WINDOW` (`0x08000000`) through the shared `windows_silent()` helper
in `crates/core/src/platform.rs`. Those processes are LLM payload children with
no operator console expectation; allocating a visible console is outward-filter
leakage (the operator sees UI the LLM did not intend).

EDR still sees the signed GUI-subsystem daemon as parent (single hop, declared
service). This does not add a hidden bridge spawn or a LOLBin chain — it only
suppresses console allocation for daemon-initiated payload children.

## 5. WSL / Linux runtime package responsibilities (unchanged)

The WSL runtime package is the SAME root + platform packages NPM02
locked. WWS01 does NOT widen runtime claims.

- `terminal-commanderd` still owns the daemon, the IPC server, the
  job manager, the policy engine, the persistent audit, and every
  probe.
- `terminal-commander-mcp` (the real one, INSIDE WSL) is still the
  thin rmcp 1.7.0 stdio adapter. The two MCP guard greps remain
  clean.
- `terminal-commander` (the real admin CLI, INSIDE WSL) is
  unchanged.
- The PTY surface remains Unix-only (TC44).
- The daemon UDS lives at `$TC_DATA/terminal-commanderd.sock`.
- The 29-tool catalogue is the only IPC surface (TC45).

## 6. Cursor MCP config model (locked)

### 6.1 Stanza shape written by `setup cursor-wsl`

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

Identical to `examples/provider-harness/cursor/mcp.global.native-linux.json`.
On Windows, `terminal-commander-mcp` is the bridge shim from §4.1.
On Linux / inside-WSL Cursor, it is the real adapter.

### 6.2 Scope choice

Default: **global** (`%USERPROFILE%\.cursor\mcp.json` on Windows;
`~/.cursor/mcp.json` on Linux). Operators who want workspace
scope pass `--project` to `setup cursor-wsl`; the writer drops
the stanza into `<workspace>/.cursor/mcp.json` and informs the
operator the file lives next to their workspace.

### 6.3 Existing-file policy

- If the target `mcp.json` does NOT exist: write fresh.
- If the target `mcp.json` exists and contains a `terminal-commander`
  entry: refuse without `--force`.
- If the target `mcp.json` exists and does NOT contain a
  `terminal-commander` entry: merge the new stanza in, preserving
  every unrelated entry verbatim.
- Always back up the pre-existing file to `<path>.bak` BEFORE
  any overwrite (even with `--force`).
- Always write atomically (write to `<path>.tmp`, fsync, rename).

### 6.4 What the writer must NOT do

- MUST NOT print the operator's full home directory to stdout.
  Use `~` / `%USERPROFILE%` placeholders in user-facing messages.
- MUST NOT include API keys, tokens, or credentials in the
  `env` block. Only the non-secret `TC_SOCKET` is allowed, and
  only when the operator passes `--socket-override` or
  `setup cursor-wsl` discovers a non-default `$TC_DATA` in WSL.
- MUST NOT commit `.cursor/mcp.json` anywhere in the repo. The
  WWS05 forbidden_files list enforces this at goal level.

## 7. Distro discovery model (locked)

### 7.1 Discovery flow at setup time

1. Run `wsl.exe -l -v` with `shell: false`. Parse output strictly
   (allow UTF-16 LE BOM that `wsl.exe` writes on Windows).
2. If no distros are listed: print one-line actionable error
   ("WSL is installed but no distro is registered; run `wsl
   --install -d Ubuntu-24.04`") and exit non-zero.
3. If exactly one distro is listed: use it as the default.
4. If multiple distros are listed AND `--distro <name>` is passed:
   use that distro after whitelist validation.
5. If multiple distros are listed AND no `--distro` is passed:
   if a previous setup-state file records a choice, reuse it; else
   pick the WSL default distro (the one without an asterisk; the
   asterisk in `wsl -l -v` marks the default) and ASK the operator
   ONCE to confirm via `--yes` flag OR via interactive Y/n prompt
   (Y default). On `--no-interactive`, refuse and print the exact
   `--distro` flag the operator needs.
6. Persist the chosen distro into `%LOCALAPPDATA%\terminal-commander\setup.json`
   (see §11) so subsequent shim invocations are silent.

### 7.2 What discovery must NOT do

- MUST NOT run `wsl --install` on its own. WSL distro install
  remains an operator action.
- MUST NOT pick a distro that does not appear in `wsl.exe -l -v`.
- MUST NOT pick a distro whose state is `Installing` or `Stopped`
  without informing the operator.
- MUST NOT trust operator-supplied distro names without comparing
  them against the live `wsl.exe -l -v` output (whitelist check).

## 8. WSL-side runtime install / update model (locked)

### 8.1 Default behavior (amended INSTALL01)

On **Windows global `npm install -g`**, the `install` lifecycle script
and `lib/bootstrap/ensure_wsl_runtime.js` **default to installing**
the runtime inside the resolved WSL distro. The same path runs when
`spawnWslBridge` sees `runtime_missing` (lazy bootstrap) unless
`TC_SKIP_BOOTSTRAP=1`.

`setup cursor-wsl` and `setup harness` call the shared ensure module.
`--install-wsl-runtime` remains accepted as an explicit alias.

If WSL is missing or install fails, bootstrap **fail-soft** for npm
(exit 0 from the install script with stderr guidance) unless the
operator invoked `setup` directly (non-zero).

### 8.2 Install command (locked)

Inside WSL, the orchestrator runs ONE constant `bash -lc` string (see
`lib/bootstrap/constants.js`). NO operator value is interpolated.
`PATH` is prefixed with Linux system paths before `npm` to avoid
Windows `nodejs` shims shadowing the in-distro install.

### 8.3 Opt-out

- `TC_SKIP_BOOTSTRAP=1`
- `npm install -g terminal-commander --ignore-scripts`
- Manual per-user npm prefix inside WSL (operator may pre-install;
  doctor still verifies `command -v terminal-commander-mcp`)

## 9. Optional pairing model (locked, not the primary path)

Pairing is OPTIONAL. The default install relies on `wsl.exe`'s
automatic mapping from `wsl.exe -d <distro>` to the correct
distro VM. The six-digit pairing flow exists only as an
operator-confirmation / anti-misconfiguration aid:

| Subcommand | Where | Behavior |
|------------|-------|----------|
| `terminal-commander pair create` | Windows | Generate a 6-digit code via `crypto.randomInt(100000, 999999)`. Store under `%LOCALAPPDATA%\terminal-commander\pair.json` with `pair_id`, `code`, `created_at`, `distro` fields. Print the code to operator on stdout. |
| `terminal-commander pair accept <code>` | WSL | Read the code from operator argv. Compare against the Windows-side file if reachable (`/mnt/c/Users/<user>/AppData/Local/...`); else accept after operator confirms `Y` interactively. Persist a `paired_at` field. |

Pair codes are NEVER treated as cryptographic secrets in any
decision. They are human confirmation that "the right distro is
talking to the right Windows install". Setup MUST work without
the pair commands ever being run.

## 10. Security model (locked)

### 10.1 Boundary inherits from TC44 + NPM02 + NPM07 + NPM10

| Surface | Locked |
|---------|--------|
| MCP root shell | No. `terminal-commander-mcp` is a thin adapter on both sides. The Windows shim is `spawn` only. |
| Network listener | No. Daemon + adapter + bridge shim all stay local. |
| Raw stream endpoint | No. The 29 TC45 tools remain the only IPC. |
| Shell bridge | No. `command_start_combed` argv-only + shell-interpreter deny list (TC38 / TC41). |
| Auto password entry | No. `pty_command_write_stdin` rejects secret-prompt responses (TC44). |
| Bridge shim shell interpolation | No. `wsl.exe` argv passed as an array; distro name whitelist-validated. |
| Postinstall downloader | No. NPM02 §4.2 unchanged. |
| Bridge shim secrets | No. Bridge passes no env vars, no operator credentials, no tokens. |
| Setup state file | Plain JSON under `%LOCALAPPDATA%\terminal-commander\setup.json`. Contains chosen distro + Cursor scope only. NEVER contains tokens, passwords, or absolute paths outside `%LOCALAPPDATA%`. |

### 10.2 What WWS chain adds (Windows side)

- `lib/wsl/spawn.js` (WWS04, **landed**): exports
  `spawnWslBridge(opts)`. Resolves the bridge distro from a closed
  priority chain (`TC_WSL_DISTRO` env -> `detectWsl().default_distro`
  -> bounded `no_default_distro` refusal), double-validates it via
  `assertSafeDistroName` + live `detectWsl()` whitelist membership,
  optionally runs the WWS03 `wslDoctor({ probeRuntime: true })` gate
  (opt-out via `TC_WSL_SKIP_DOCTOR=1`), strips token-shaped env vars
  via `buildFilteredEnv`, and spawns `wsl.exe -d <distro> -- bash
  -lc 'exec terminal-commander-mcp' [...userArgv]` with EXACTLY
  `{ shell: false, stdio: 'inherit', env: filteredEnv }` and no hidden-window
  option.
  Forwards `SIGINT` / `SIGTERM` from the parent to the child, mirrors
  the child's exit code / signal back into the parent.
  `BRIDGE_PROBE_CMD` is a literal constant; no operator value is
  interpolated into the `bash -lc` argument. The shim writes nothing
  to stdout; rmcp framing passes through the WSL pipe transparently.
  This is the single bridge spawn site in the entire wrapper.
- `lib/wsl/distro-name.js` (WWS03, **landed**): exports
  `isSafeDistroName` + `assertSafeDistroName`. Conservative
  whitelist `^[A-Za-z0-9._-]{1,64}$`; rejects whitespace, quote,
  semicolon, pipe, dollar, backtick, slash, backslash, NUL, and
  every other shell metacharacter. The only operator-supplied
  string that the WWS chain ever passes to `wsl.exe`.
- `lib/wsl/detect.js` (WWS03, **landed**): runs `wsl.exe -l -v`
  only. Parses verbose output (default-marker `*`, state, WSL
  version) and tolerates UTF-16 LE BOM + NUL-padded ASCII + CRLF.
  Executor-injectable for tests. Returns a typed
  `{ host_platform, wsl_callable, default_distro, distros, reason }`
  record with `reason` in the closed enum
  `{ ok, unsupported_host, wsl_not_found, no_distros,
  wsl_command_failed, check_timeout }`.
- `lib/wsl/doctor.js` (WWS03, **landed**): read-only WSL doctor.
  Validates the operator-supplied distro twice (character
  whitelist + live `detectWsl()` membership) before passing it as
  a single argv element after `-d`. Optional inside-distro probe
  (`opts.probeRuntime === true`) runs exactly one constant
  command — `command -v terminal-commander-mcp` — and never
  concatenates operator input into the `bash -lc` argument.
  Returns a typed `{ status, reason, distro, runtime_present,
  hint }` record with `status` covering
  `{ ok, unsupported_host, wsl_not_found, no_distros,
  distro_not_found, unsafe_distro_name, wsl_command_failed,
  runtime_missing, runtime_present, doctor_not_run, check_timeout }`.
- `lib/cursor/config.js` (WWS05, **landed**): pure stanza-builder
  + path resolver + JSON parse / merge / validate. Default
  generated Windows stanza is the WWS04 bridge form
  `{ "type": "stdio", "command": "terminal-commander-mcp" }`;
  optional `env.TC_WSL_DISTRO` is added when the operator passed
  a safe distro (WWS03 whitelist) and, if `requireKnownDistro:
  true`, the distro was present in `detectWsl().distros`. No
  other env key is ever emitted. No spawn. No network. No file
  I/O. `MAX_CONFIG_BYTES = 256 * 1024` size cap.
- `lib/cursor/write.js` (WWS05, **landed**): atomic write +
  `.bak` backup orchestrator. Refuses existing `terminal-commander`
  entry without `force: true`. Refuses pre-existing `.bak` without
  `clobber_backup: true`. Atomic-write via
  `<path>.tmp.<random>` in the SAME directory + `renameSync`. Every
  path touched (final, tmp, bak) stays inside the resolved Cursor
  scope dir (`path_not_allowed` otherwise). No spawn. No network.
  No `child_process` import. Stdout-silent; status returns via a
  typed `{ status, path, backup_path, server, was_present, hint }`
  record. 12-status closed enum.
- `lib/cli/setup_cursor_wsl.js` + friends (WWS06, **landed**):
  orchestrate the above helpers. The CLI shipped at WWS06 is:
  `terminal-commander doctor`, `terminal-commander doctor wsl
  [--distro <name>] [--probe-runtime]`, `terminal-commander setup
  cursor-wsl [--distro] [--global|--project <path>] [--force]
  [--clobber-backup] [--print-config] [--dry-run]
  [--install-wsl-runtime]`, `terminal-commander pair create
  [--distro]`, `terminal-commander pair accept <code>`. Every
  `wsl.exe` invocation flows through the WWS04 spawn argv shape;
  the `--install-wsl-runtime` flag issues exactly one constant
  `wsl.exe -d <distro> -- bash -lc 'npm install -g
  terminal-commander'` command. NO sudo. NO password prompt. NO
  credential broker (deferred). Permission failures return
  `install_permission_required`; npm E404 returns
  `npm_package_unpublished` honestly. `pair create` persists a
  bounded `pair.json` (uuid v7 + 6-digit code + timestamps; no
  secrets, no tokens, no env values). `pair accept` validates the
  6-digit shape AND the persisted-code match; the full WSL-side
  handshake is deferred to a future enhancement (`pair_deferred`
  on no match). Windows-side `setup.json` written under
  `%LOCALAPPDATA%\terminal-commander\` (bounded schema; no
  secrets).

### 10.3 What the WWS chain DOES NOT add

- No Windows-native daemon.
- No Windows-native MCP tool.
- No new IPC surface.
- No new socket / network listener.
- No new file-read primitive.
- No new probe.
- No new admin CLI shell out.
- No `Not Run` evidence promoted to PASS.

## 11. Setup-state file layout (locked)

| Side | Path | Purpose |
|------|------|---------|
| Windows | `%LOCALAPPDATA%\terminal-commander\setup.json` | Persists distro choice + Cursor scope + last `doctor` summary. Never contains secrets, tokens, or absolute paths outside `%LOCALAPPDATA%`. |
| Windows | `%LOCALAPPDATA%\terminal-commander\pair.json` | Optional pairing record only when `pair create` was run. |
| WSL | `$XDG_STATE_HOME/terminal-commander/` (existing TC36 location) | Daemon `$TC_DATA`. Unchanged from runtime baseline. |

The `setup.json` schema (locked at WWS01, implemented by WWS06):

```json
{
  "schema_version": 1,
  "distro": "Ubuntu-24.04",
  "cursor_scope": "global",
  "cursor_config_path_redacted": "%USERPROFILE%\\.cursor\\mcp.json",
  "last_doctor_at": "2026-05-23T12:00:00Z",
  "last_doctor_status": "ok"
}
```

No fields hold credentials. `cursor_config_path_redacted` keeps
the placeholder; the resolved absolute path is computed on demand,
never persisted.

## 12. Failure modes + user-facing error messages (locked)

| Failure | Behavior | Operator message |
|---------|----------|------------------|
| `wsl.exe` not on PATH | Setup exits 64 | `terminal-commander: wsl.exe not found on PATH. Install WSL2: run 'wsl --install' from an elevated PowerShell, restart, then re-run 'terminal-commander setup cursor-wsl'.` |
| `wsl.exe -l -v` returns no distros | Setup exits 64 | `terminal-commander: WSL is present but no distro is registered. Run 'wsl --install -d Ubuntu-24.04' or pick another distro, then re-run setup.` |
| Operator-supplied `--distro` not in whitelist | Setup exits 64 | `terminal-commander: distro '<name>' not found in 'wsl -l -v'. Available distros: <list>.` |
| Distro listed but `terminal-commanderd` missing inside it | Setup exits non-zero, prints install hint | `terminal-commander: runtime not installed in distro '<distro>'. Run 'wsl -d <distro> -- bash -lc \"npm install -g terminal-commander\"' OR re-run 'terminal-commander setup cursor-wsl --install-wsl-runtime' to do it automatically.` |
| Cursor `mcp.json` already has a `terminal-commander` entry | Setup exits non-zero unless `--force` | `terminal-commander: '<path>' already contains a 'terminal-commander' MCP entry. Re-run with --force to overwrite (a backup will be written to <path>.bak).` |
| Cursor `mcp.json` exists with unrelated entries | Setup merges in the new stanza, preserves the rest | `terminal-commander: wrote '<path>' (backup at '<path>.bak'); preserved N existing MCP server entry/entries.` |
| Bridge shim invoked on Windows but `wsl.exe` failed to launch | Shim exits 64 | `terminal-commander: failed to invoke wsl.exe -d '<distro>' bash -lc 'terminal-commander-mcp': <wsl exit code>. Re-run 'terminal-commander doctor'.` |
| `terminal-commanderd` invoked on Windows | Refuses with the §4.1 message; exit 64 | `terminal-commander: terminal-commanderd runs only inside Linux / WSL. Run from a WSL distro, or use 'terminal-commander setup cursor-wsl' on Windows to configure the bridge.` |
| Invocation on macOS / unsupported platform | Same as today (NPM03 resolver `unsupported_platform`) | `terminal-commander: unsupported platform darwin-arm64; supported: linux-x64, linux-arm64, win32-x64 (bridge), win32-arm64 (bridge)` |

Every message above is a single bounded stderr line; no stack
traces; no environment dumps.

## 13. NPM02 conflict list + amendment proposal

### 13.1 NPM02 fields that conflict with this UX

| NPM02 section | NPM02 lock | WWS01 change | Owner goal |
|---------------|-----------|--------------|-----------|
| §3.1 "Supported npm binary platforms" | linux-x64 + linux-arm64 only | Unchanged. Windows is BRIDGE-only, NOT a binary platform. | none (this is a clarification, not a change) |
| §3.2 "Unsupported (rejected at NPM02)" | "Windows-native (any arch)" | Restated: Windows-native runtime remains rejected. Windows host gets a JS bridge package only. | WWS08 (docs) |
| §3.3 "WSL2 fallback for Windows operators" | "Install Terminal Commander INSIDE WSL" | Extended: operator MAY also install the root npm package on Windows for the bridge / setup surface. WSL install is still required for runtime. | WWS02 |
| §4.1 "Preferred (LOCKED)" — `optionalDependencies` shape | Both platform packages `0.1.0-beta.1` exact-pin | Unchanged. | none |
| §4.3 "`optionalDependencies` semantics" | `engines.npm: ">=8"` so `os` / `cpu` filters work | Unchanged; this is exactly what allows the Windows install to skip both Linux platform packages cleanly. | none |
| §4.4 "Shim behavior contract" | resolver returns `{ binaryPath: null }` on unsupported platform; shim exits 64 | **Amended**: on `win32`, the resolver returns `{ reason: "bridge_required" }` instead of `unsupported_platform`; the `terminal-commander-mcp` shim dispatches to `lib/wsl/spawn.js`; the `terminal-commanderd` shim refuses with the §4.1 message; the admin CLI shim exposes the new setup subcommands. | WWS02 |
| §9 "Safety contract" | bounded shim invariants | Extended with the bridge invariants in §4.4 of THIS contract (whitelist-validated distro, argv-array spawn, `shell: false`, no hidden-window option). | WWS04 |
| Root `package.json` `os: ["linux"]` | Locked at NPM02 / NPM03 | **Amended**: widen to `os: ["linux", "win32"]`. Platform packages remain `os: ["linux"]`. | WWS02 |

### 13.2 NPM02 fields that DO NOT change

- §1 Package names — unchanged.
- §2 Install command — unchanged. `npm install -g terminal-commander`
  remains the public-facing command.
- §6 Versioning contract — unchanged. Shared semver across the
  three packages; `0.1.0-beta.1` initial.
- §7 Release contract — unchanged. release-please manifest mode +
  OIDC trusted publishing. NPM10 bootstrap exception unchanged.
- §8 Cursor contract — unchanged stanza shape; WWS05 now writes
  the stanza for the operator.
- §10 NPM03–NPM07 per-goal recommendations — unchanged.
- §11 Risks — extended (see WWS01 risks below).
- §12 Alternatives considered — unchanged.

### 13.3 NPM02 prep amendment requirement

WWS02 MUST land a `chore(goals): NPM02 prep amendment — widen
root os to include win32 (delegated to WWS02)` commit (or
equivalent) on the WWS02 goal file BEFORE editing
`packages/terminal-commander/package.json`. The prep amendment
commit is documentation-only and points at this WWS01 contract.

## 14. Package-contract changes required before the first npm publish

The first live publish (either through NPM10 bootstrap dispatch
OR through NPM07 OIDC) MUST land the WWS02 amendments first, or
the public-facing UX will be broken:

| Change | Owner | Why publishing before this is wrong |
|--------|-------|------------------------------------|
| Widen root `os` to `["linux", "win32"]` | WWS02 | Windows users running `npm install -g terminal-commander` today (with the published name) would hit `EBADPLATFORM` from npm; the very target audience the chain serves cannot install. |
| Add `lib/wsl/**` (detect / doctor / spawn) | WWS03 / WWS04 | Without these, the published Windows install is a `terminal-commander-mcp` that does not actually launch anything inside WSL. |
| Add `lib/cursor/**` (config / write) | WWS05 | Without these, `setup cursor-wsl` cannot write the Cursor config; the public-facing one-command UX is not delivered. |
| Add `lib/cli/**` (setup, doctor, pair) | WWS06 | Without these, the new CLI surface promised in this contract does not exist; operators are back to manual JSON editing. |
| Update `docs/integrations/cursor.md` + README to describe the new flow | WWS08 | Without these, the published docs contradict the published binary behavior. |
| Pre-publish smoke covers Windows-bridge handshake | WWS07 | Without these, the bridge could ship broken on the very platform the chain exists to serve. |

### 14.1 Recommendation on the NPM10 bootstrap dispatch

**KEEP THE NPM10 BOOTSTRAP WORKFLOW PAUSED** until WWS02 +
(at minimum) WWS04 + WWS05 + WWS06 + WWS08 land. Reasons:

- The bootstrap workflow currently would publish a root package
  with `os: ["linux"]` that REJECTS Windows installs at the npm
  `EBADPLATFORM` filter. The first thing Windows operators
  would try is `npm install -g terminal-commander` on Windows;
  that would 1× their first impression with a broken install.
- A second bootstrap dispatch with a corrected root package is
  possible only if every package is bumped to `0.1.0-beta.2`
  (because the bootstrap workflow refuses to re-publish names
  that already exist; see `.github/workflows/npm-bootstrap-publish.yml`
  pre-publish E404 check). Publishing a known-broken `beta.1`
  burns one beta number.
- The bootstrap workflow's `NPM_TOKEN_TC` is a one-time policy
  exception. Burning it on a publish that immediately needs a
  follow-up publish is operationally wasteful.

Therefore: WWS01 RECOMMENDS that operator does NOT dispatch
`npm-bootstrap-publish.yml` until the WWS chain reaches at least
WWS08 (`Completed`). WWS09 reconfirms or amends this
recommendation based on whatever real smoke evidence WWS07
produces.

## 15. Open decisions resolved (binding answers)

Maps every open decision from `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md`
to a binding answer. WWS02–WWS09 implement these.

| # | Open decision | Binding answer | Owner |
|---|---------------|----------------|-------|
| D-01 | Should the root npm package support `win32` as bridge / setup only? | YES. Root `os: ["linux", "win32"]`. Platform packages stay `os: ["linux"]`. | WWS02 |
| D-02 | Should Linux platform packages remain Linux-only `optionalDependencies`? | YES. NPM02 §4.1 and §4.3 unchanged. | WWS02 |
| D-03 | Should Windows install include only JS bridge / setup shims and no Rust binaries? | YES. No Rust binary ships in the root wrapper. Windows install resolves to JS only. | WWS02 |
| D-04 | Should `terminal-commander-mcp` on Windows be a bridge shim to WSL? | YES. `child_process.spawn` of `wsl.exe -d <distro> bash -lc terminal-commander-mcp`, `shell: false`, distro whitelist-validated, argv array only. | WWS04 |
| D-05 | Should `terminal-commanderd` on Windows refuse with a clear "use WSL" message? | YES. Exit 64. Single bounded stderr line per §12. | WWS02 / WWS04 |
| D-06 | Should `setup cursor-wsl` auto-detect WSL distros? | YES. Via `wsl.exe -l -v` only. | WWS03 |
| D-07 | Should setup pick the default distro or ask once? | Pick the WSL default distro (asterisk in `wsl -l -v`). Ask once via interactive Y/n if multiple distros AND no `--distro` flag AND no persisted choice. Refuse on `--no-interactive`. | WWS06 |
| D-08 | Should setup auto-install the WSL runtime package, or require `--install-wsl-runtime`? | **Superseded by INSTALL01:** default ON for Windows global bootstrap + lazy MCP bridge; opt-out via `TC_SKIP_BOOTSTRAP` / `--ignore-scripts`. `--install-wsl-runtime` is an alias. | INSTALL01 |
| D-09 | Should pairing codes exist, and if yes, optional or mandatory? | Optional. Pair codes are operator-confirmation only, never security secrets. Default install works without ever running `pair create`. | WWS06 |
| D-10 | Where to store Windows-side bridge / setup state? | `%LOCALAPPDATA%\terminal-commander\setup.json` (chosen distro, Cursor scope) + `%LOCALAPPDATA%\terminal-commander\pair.json` (optional). Schema in §11. | WWS06 |
| D-11 | Where to store WSL-side runtime config? | Unchanged. `$XDG_STATE_HOME/terminal-commander/` (TC36 baseline). | none (unchanged) |
| D-12 | How to handle multiple WSL distros? | `wsl.exe -l -v`, prefer default, allow operator override via `--distro`, persist choice in `setup.json`. | WWS03 / WWS06 |
| D-13 | Cursor global vs project config handling? | Default `--global`. Operator opts into `--project` for workspace-scoped. Existing-entry policy in §6.3. | WWS05 / WWS06 |
| D-14 | Rollback / uninstall story? | `npm uninstall -g terminal-commander` removes the root package + (on Linux/WSL) the matching platform package; on Windows the operator runs `terminal-commander setup cursor-wsl --uninstall` BEFORE `npm uninstall` to restore the `mcp.json` backup. | WWS06 (documented), WWS08 (README) |
| D-15 | Exact UX required before first npm publish? | At minimum: WWS02 (root os widen), WWS04 (bridge shim), WWS05 (cursor writer), WWS06 (`setup cursor-wsl` CLI), WWS08 (docs). WWS09 reconfirms readiness. | gating condition recorded in §14 |

## 16. Risks and blockers

| ID | Risk | Mitigation | Where tracked |
|----|------|-----------|---------------|
| R-WWS-01 | `wsl.exe -l -v` output format may differ between Windows builds (UTF-16 BOM, trailing whitespace, column widths). | WWS03 parser must handle UTF-16 LE BOM + arbitrary column widths. Tested in `packages/terminal-commander/test/wsl-detect.test.js`. | WWS03 |
| R-WWS-02 | Cursor MCP config schema may evolve. | WWS05 writes ONLY the documented stanza shape (`type: "stdio"`, `command`). Refuses unknown top-level keys in existing config without `--force`. | WWS05 |
| R-WWS-03 | Operator may run setup on a Windows host with no WSL2 installed at all. | §12 row 1 covers this: `wsl.exe not found` → exit 64 with the actionable install instruction. | WWS06 |
| R-WWS-04 | A malicious actor may supply a crafted distro name via `--distro` to attempt injection. | WWS04 distro whitelist validated against the live `wsl.exe -l -v` output before spawning. Argv array only; no shell. | WWS04 |
| R-WWS-05 | Operator may have an existing `.cursor/mcp.json` with the `terminal-commander` entry pointing at a different command (e.g. an old `wsl -d ... bash -lc ...` line). | WWS05 refuses to overwrite without `--force` and ALWAYS writes a `.bak` first. | WWS05 |
| R-WWS-06 | Windows operator may run setup before installing the runtime inside WSL, then later install the runtime; the bridge shim must work on the next invocation. | WWS06 `doctor` is the canonical re-check entry point; `setup cursor-wsl` is re-runnable. | WWS06 |
| R-WWS-07 | TC44 / runtime constraints make Windows-native daemon impossible; future Windows native support would need a deeper runtime overhaul. | DEFERRED. The hard product boundary in `GOAL_CHAIN_INDEX.md` records this. | future chain |
| R-WWS-08 | Publishing `0.1.0-beta.1` with the current root `os: ["linux"]` would break the very Windows users this chain serves. | §14.1: keep `npm-bootstrap-publish.yml` paused until WWS08 lands at minimum. | WWS09 |
| R-WWS-09 | Cursor live smoke from a Windows host still requires GUI steps (no headless MCP entry point). | WWS07 records the GUI smoke status honestly; `Not Run` if unattainable; no promotion to PASS. | WWS07 |
| R-WWS-10 | WSL2 9P filesystem performance on `/mnt/c` could mislead the operator if the daemon is run from there. | The daemon already rejects `/mnt/c` `data_dir` at writer-open (TC36); the bridge does not change this. WWS06 `doctor` surfaces the existing rejection. | WWS06 |

## 17. Explicit deferrals (out of WWS chain)

- macOS-native bridge or Mac WSL2 equivalent (no Mac WSL today).
- Native Windows daemon (TC44 follow-up required, not this chain).
- Cursor extension / plugin auto-install (this chain writes JSON
  only).
- Codex CLI + Claude Code Windows-side bridges (WWS08 may
  document if the pattern generalizes; default is the existing
  Linux / WSL provider walk-throughs at TC46).
- crates.io / cargo publish (TC31 / TC48 baseline preserved).
- Standing `NPM_TOKEN_TC` use beyond the NPM10 single dispatch.
- musl / Alpine target (no runtime evidence yet).
- Auto-elevation / `sudo` inside WSL during install.
- Removing or moving the existing `wsl.exe`-direct example
  config (`examples/provider-harness/cursor/mcp.global.linux-wsl.json`).
  WWS08 keeps it as a documented fallback for operators who do
  not want the bridge shim path.

## 18. Decisions locked for WWS02–WWS09 (binding)

This subsection is the single source of truth WWS02 onward
implement. Any deviation requires a prep amendment that cites this
WWS01 contract by document path AND date.

- **WWS02**: widen root `os` to `["linux", "win32"]`; platform
  packages unchanged; resolver returns `{ reason: "bridge_required" }`
  on `win32`; the three shims branch on Windows per §4.1.
- **WWS03** (**landed**): `lib/wsl/distro-name.js` +
  `lib/wsl/detect.js` + `lib/wsl/doctor.js`; runs `wsl.exe` only;
  distro whitelist `^[A-Za-z0-9._-]{1,64}$`; UTF-16 LE BOM +
  NUL-padded ASCII + CRLF tolerated; read-only; default
  `probeRuntime` is `false` (doctor returns the `OK` /
  `doctor_not_run` pair until probing is explicitly opted in);
  helpers are library-only (the three `bin/*` shims stay
  byte-identical to the WWS02 baseline).
- **WWS04** (**landed**): `lib/wsl/spawn.js` — single entry-point
  bridge spawn helper. `shell: false`, argv array only, no hidden-window
  option, `stdio: 'inherit'`, distro double-validated
  (`assertSafeDistroName` + live whitelist), `BRIDGE_PROBE_CMD =
  'exec terminal-commander-mcp'` is a literal constant (no
  interpolation), distro priority chain
  `TC_WSL_DISTRO` -> `detectWsl().default_distro` -> bounded
  `no_default_distro` refusal, optional runtime-presence gate via
  WWS03 `wslDoctor` (opt-out: `TC_WSL_SKIP_DOCTOR=1`), token-shaped
  env vars stripped via `buildFilteredEnv`. The
  `terminal-commander-mcp` shim dispatches through this helper on
  Windows; the daemon + admin-CLI shims stay byte-identical to the
  WWS02 contract (refuse with bounded stderr + exit 64). The shim
  writes nothing to stdout (rmcp framing).
- **WWS05** (**landed**): `lib/cursor/config.js` + `lib/cursor/write.js`.
  Pure stanza-builder + path resolver + JSON parse / merge /
  validate + atomic write (`<path>.tmp.<random>` in SAME dir +
  `renameSync`) + `.bak` backup. Default stanza is the WWS04
  bridge form (`{ "command": "terminal-commander-mcp" }`); optional
  `env.TC_WSL_DISTRO` when operator supplied safe distro.
  Refuses existing `terminal-commander` entry without `force:
  true`; refuses pre-existing `.bak` without `clobber_backup:
  true`. `MAX_CONFIG_BYTES = 256 KiB`. 12-status closed enum.
  NO `child_process`. NO spawn. NO network. NO env key written
  beyond `TC_WSL_DISTRO`. Stdout-silent. Library-only; CLI surface
  remains with WWS06.
- **WWS06** (**landed**): `lib/cli/parser.js` +
  `lib/cli/setup_cursor_wsl.js` + `lib/cli/doctor.js` +
  `lib/cli/pair_create.js` + `lib/cli/pair_accept.js` +
  `lib/cli/setup_state.js` + `lib/cli/run.js`.
  `terminal-commander.js` Windows branch delegates to
  `lib/cli/run.js` (Linux branch byte-identical). 21-status closed
  setup enum; locked subcommand surface (`doctor`, `doctor wsl`,
  `setup cursor-wsl`, `pair create`, `pair accept <code>`).
  `--install-wsl-runtime` triggers ONE constant `npm install -g
  terminal-commander` invocation via the WWS04 spawn argv shape.
  NO sudo. NO password prompt. NO credential broker (deferred).
  Permission failures -> `install_permission_required`. NPM E404
  -> `npm_package_unpublished`. Distro priority chain
  `--distro` -> `TC_WSL_DISTRO` -> `detectWsl().default_distro` ->
  `no_default_distro_ambiguous` refusal. State files
  (`setup.json` + `pair.json`) under
  `%LOCALAPPDATA%\terminal-commander\` per §11 — bounded schemas;
  no secrets / tokens / env values / command history. `pair create`
  + `pair accept` deferred handshake (`pair_deferred` until the
  WSL-side daemon session protocol lands).
- **WWS07** (**landed**): PowerShell smoke script
  (`scripts/smoke/verify-windows-bridge-smoke.ps1`). PS 5.1+;
  `Set-StrictMode -Version Latest`; `ErrorActionPreference =
  Stop`. Flags: `-DryRun`, `-Distro <name>`, `-InstallWslRuntime`,
  `-WriteCursorConfig`, `-TempCursorScope` (default-on when
  `-WriteCursorConfig` supplied). Output: bounded `PASS  <step>`
  / `FAIL  <step>` / `NOTE  <text>` / `INFO  <text>` lines.
  Drives the WWS06 CLI (doctor, doctor wsl, setup --print-config,
  setup --dry-run, --probe-runtime) and, when the WSL-side
  runtime is present, drives an MCP initialize + tools/list +
  health round-trip through the WWS04 bridge. The script never
  calls `child_process` or `wsl.exe` directly; the only spawn
  surface is `node` (the JS CLI shim). NO sudo. NO password. NO
  publish. Real Cursor config NOT touched unless
  `-WriteCursorConfig` AND `-TempCursorScope` (default-on); the
  operator's `%USERPROFILE%\.cursor\mcp.json` is never written
  without explicit opt-out. `runtime_missing` is RECORDED, not
  promoted to FAIL — the script exits 0 on overall honest
  evidence even when the WSL-side runtime is absent. Cursor GUI
  provider smoke is operator-driven and `Not Run` until a
  transcript is attached.
- **WWS08** (**landed**): docs-only — `README.md` (feature
  matrix WWS rows + Windows host install subsection + Cursor
  bridge note + WWS01..WWS09 state table + Not Run items
  preserved), `docs/release/npm-binary-packaging-contract.md`
  §13b cross-link to the WWS chain (no field change to NPM02
  baseline), `docs/release/npm-distribution-final-report.md` §11
  WWS chain follow-up section, `docs/integrations/cursor.md`
  cross-links to §11a/§11c/§11d, `examples/provider-harness/cursor/README.md`
  operator note, `RELEASE_CHECKLIST.md` Windows + WSL bridge
  chain section, `BACKLOG.md` P2 WWS-B1..WWS-B9 follow-ups,
  `RISK_REGISTER.md` R-WWS-01..R-WWS-10 entries, `ROADMAP.md`
  WWS chain table. No `package.json` edit. No workflow edit. No
  `crates/**` edit. No `scripts/**` edit. No `packages/*/lib/**`
  / `bin/**` / `test/**` edit. No version bump. No publish. No
  workflow dispatch.
- **WWS09**: review + final report. Recommendation either
  preserves `Conditional Go` or, if WWS07 Cursor live smoke
  transcript landed AND every other gate green, promotes to `Go`.

## 19. Acceptance against the WWS01 mini-spec

- [x] Root `os` field decision locked (§13.1, §15 D-01).
- [x] Platform-package contract decision locked (§13.1, §15 D-02).
- [x] Windows package responsibilities locked (§4, §15 D-03 / D-05).
- [x] Bridge shim shape locked (§4.4, §15 D-04).
- [x] Distro discovery model locked (§7, §15 D-06 / D-07 / D-12).
- [x] Runtime install opt-in locked (§8, §15 D-08).
- [x] Pairing model locked (§9, §15 D-09).
- [x] Setup-state file layout locked (§11, §15 D-10 / D-11).
- [x] Cursor config model locked (§6, §15 D-13).
- [x] Failure modes + error messages locked (§12).
- [x] NPM02 conflict list + amendment proposal locked (§13).
- [x] Package-contract changes required before publish locked (§14).
- [x] Recommendation on `npm-bootstrap-publish.yml` pause locked (§14.1).
- [x] Risks and blockers locked (§16).
- [x] Deferrals listed (§17).
- [x] Per-goal binding decisions for WWS02–WWS09 locked (§18).
- [x] No `crates/**` change.
- [x] No `packages/*/package.json` change.
- [x] No `.github/**` change.
- [x] No `Not Run` evidence promoted to PASS.

## 20. Evidence

- Inputs read at WWS01: `.agent/goals/terminal-commander-windows-wsl-bridge/WWS01-windows-wsl-install-ux-contract.md`, `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md`, `.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md`, `README.md`, `docs/integrations/cursor.md`, `examples/provider-harness/cursor/{mcp.global.native-linux.json, mcp.global.linux-wsl.json, mcp.project.linux-wsl.json, README.md}`, `docs/release/npm-binary-packaging-contract.md`, `docs/release/npm-distribution-final-report.md`, `docs/release/npm-bootstrap-first-publish.md`, `packages/terminal-commander/package.json`, `packages/terminal-commander-linux-x64/package.json`, `packages/terminal-commander-linux-arm64/package.json`, `packages/terminal-commander/bin/{terminal-commanderd.js, terminal-commander-mcp.js, terminal-commander.js}`, `packages/terminal-commander/lib/resolve-binary.js`, `scripts/smoke/verify-npm-local-install.sh`, `.github/workflows/npm-bootstrap-publish.yml`.
- Token-surface: no `NPM_TOKEN_TC`, `CARGO_REGISTRY_TOKEN_TC`, or
  `RELEASE_PLEASE_TOKEN_TC` reference added by WWS01.
- Forbidden-paths diff (`--ignore-cr-at-eol`) over `crates/ Cargo.toml Cargo.lock rules/ config/ scripts/ packages/ .github/`: WWS01 writes nothing under any of these paths.
- MCP guard greps unchanged (WWS01 touches no Rust source).
- No `Not Run` smoke promoted to PASS.

## 21. Cross-links

- NPM02 binding spec: [`docs/release/npm-binary-packaging-contract.md`](npm-binary-packaging-contract.md).
- NPM07 OIDC publish contract: [`docs/release/npm-trusted-publishing-contract.md`](npm-trusted-publishing-contract.md).
- NPM08 Cursor walk-through: [`docs/integrations/cursor.md`](../integrations/cursor.md).
- NPM09 final report: [`docs/release/npm-distribution-final-report.md`](npm-distribution-final-report.md).
- NPM10 bootstrap policy exception: [`docs/release/npm-bootstrap-first-publish.md`](npm-bootstrap-first-publish.md).
- Chain index: [`.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md`](../../.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md).
- Chain run order: [`.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md`](../../.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md).
