# terminal-commander

Local MCP control plane with environment runners. Raw terminal /
file / PTY output goes in; only vetted, bounded signal comes out;
context remains available by pointer.

This npm package is the **root wrapper**. It runs on:

- **Linux + WSL2** as the full runtime host (daemon, MCP stdio
  adapter, admin CLI; backed by the platform-binary
  `optionalDependency` matching your host).
- **Windows** as a **bridge / setup surface only** — no native
  daemon, no native PTY, no native UDS. The actual runtime still
  lives inside a WSL distro. The Windows shims branch to bounded
  refusals / setup-pending messages until WWS04 (WSL bridge shim)
  and WWS06 (setup / doctor CLI) land.

Platform binaries arrive via `optionalDependencies`:

- `@terminal-commander/linux-x64`
- `@terminal-commander/linux-arm64`

(No Windows binary package exists. No macOS package exists. No
musl / Alpine package exists. The runtime is Unix-only by design;
see TC44 `non_goals`.)

## Install

### Linux / WSL2

```sh
npm install -g terminal-commander
```

`npm install` pulls only the platform package matching your host
(`process.platform === 'linux'` + `process.arch` in `x64` /
`arm64`). The shims `child_process.spawn` the resolved Rust binary
with `shell: false` and `stdio: 'inherit'`.

### Windows (bridge / setup surface)

`npm install -g terminal-commander` runs INSTALL01 bootstrap: WSL runtime ensure +
harness MCP auto-config. See [`docs/release/install-bootstrap-contract.md`](../../docs/release/install-bootstrap-contract.md).

```powershell
npm install -g terminal-commander
```

On Windows, npm correctly skips both Linux platform packages
(`os: ["linux"]` filter), so no Rust binary is installed. The
root package itself is now `os: ["linux", "win32"]` (widened at
WWS02), so the install succeeds. Behavior of the three commands
at this WWS02 milestone:

| Command | Windows behavior at WWS02 |
|---------|--------------------------|
| `terminal-commanderd` | **Refuses** with a single bounded stderr line + exit `64`. Daemon is Unix-only; run it inside a WSL distro. |
| `terminal-commander-mcp` | **Bridges** to the WSL distro via `wsl.exe`. Resolves the distro from `TC_WSL_DISTRO` (operator override) then `detectWsl().default_distro`; refuses with `no_default_distro` if neither yields a safe + whitelisted name. Optionally probes WSL-side runtime presence via WWS03 `wslDoctor` (default ON; opt-out via `TC_WSL_SKIP_DOCTOR=1`). Spawns `wsl.exe -d <distro> -- bash -lc 'exec terminal-commander-mcp'` with `shell: false`, `windowsHide: true`, `stdio: 'inherit'`. The shim writes nothing to stdout — Cursor's rmcp framing passes through the WSL pipe transparently. Token-shaped env vars are stripped from the child's env. |
| `terminal-commander` | **CLI surface** for the WWS chain. WWS06 added `doctor`, `doctor wsl`, `setup cursor-wsl`, `pair create`, `pair accept <code>`. Run `terminal-commander --help` for the full panel. Every `wsl.exe` invocation flows through the WWS04 bridge helper (NO sudo, NO password handling, NO credential broker). The `--install-wsl-runtime` flag attempts ONE constant `npm install -g terminal-commander` invocation inside the chosen distro; it returns `npm_package_unpublished` honestly until NPM07's first publish lands. |

WWS02 is the package-contract slice of the chain. The actual
Windows -> WSL bridge invocation (`wsl.exe -d <distro> -- bash -lc
'exec terminal-commander-mcp'`) lands in WWS04 (already landed).
The Cursor MCP config writer is shipped at WWS05 (already landed
as `lib/cursor/`). The setup CLI shipped at WWS06 (`lib/cli/`).
The Windows bridge smoke ships at WWS07
(`scripts/smoke/verify-windows-bridge-smoke.ps1`). The contract
locking all of this is
[`docs/release/windows-wsl-bridge-contract.md`](https://github.com/special-place-administrator/terminal-commander/blob/main/docs/release/windows-wsl-bridge-contract.md).

### Cursor config writer (WWS05)

The root package also ships a JS-only Cursor `mcp.json` writer under
`lib/cursor/`. It is a library-only surface at WWS05; the CLI
subcommand that exposes it (`terminal-commander setup cursor-wsl`)
arrives at WWS06.

Default generated stanza (Linux native / inside-WSL OR Windows
bridge, both forms identical to Cursor):

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

When the operator wants to pin a specific WSL distro instead of
letting the WWS04 bridge resolve via `wsl.exe -l -v` default, an
optional `env.TC_WSL_DISTRO` is added (distro name double-validated
via the WWS03 safety whitelist before it lands in the config):

```json
{
  "mcpServers": {
    "terminal-commander": {
      "type": "stdio",
      "command": "terminal-commander-mcp",
      "env": {
        "TC_WSL_DISTRO": "Ubuntu-24.04"
      }
    }
  }
}
```

Safety invariants:

- Preserves every unrelated `mcpServers` entry untouched.
- Refuses to overwrite an existing `terminal-commander` entry
  unless `force: true`. Always creates `<mcp.json>.bak` before
  overwrite. Refuses if `.bak` already exists unless
  `clobber_backup: true`.
- Reads at most 256 KiB of existing `mcp.json` (`config_too_large`
  refusal above that).
- Refuses `invalid_json` without modifying the original file.
- Atomic write via a same-directory `mcp.json.tmp.<random>` +
  `renameSync`. Every path the writer touches stays inside the
  resolved Cursor scope dir.
- NO `child_process`, NO spawn, NO `wsl.exe`, NO network, NO
  secrets/tokens/credentials/passwords. The only env key the
  writer ever emits is `TC_WSL_DISTRO`.
- Stdout-silent — all status text returns via the typed result
  record.

### Windows discovery helpers (WWS03)

WWS03 adds a JS-only, read-only Windows discovery helper layer
under `lib/wsl/`:

- `lib/wsl/distro-name.js` exports `isSafeDistroName(name)` and
  `assertSafeDistroName(name)`. Distro names accepted by future
  bridge / setup callers must match `^[A-Za-z0-9._-]{1,64}$` —
  whitespace, quotes, semicolons, pipes, dollars, backticks,
  slashes, backslashes, and NUL bytes are all rejected.
- `lib/wsl/detect.js` exports `detectWsl(opts)`. Runs only
  `wsl.exe -l -v` (`shell: false`, `windowsHide: true`, argv
  array). Parses verbose output (default-marker `*`, state, WSL
  version). Tolerates UTF-16 LE BOM + NUL-padded ASCII + CRLF.
  Returns `{ host_platform, wsl_callable, default_distro,
  distros, reason }` with `reason` in `{ ok, unsupported_host,
  wsl_not_found, no_distros, wsl_command_failed, check_timeout
  }`.
- `lib/wsl/doctor.js` exports `wslDoctor(opts)`. Validates the
  operator-supplied distro twice (character whitelist + live
  `detectWsl()` membership) before passing it as a single argv
  element after `-d`. The optional inside-distro runtime probe is
  off by default (`probeRuntime: false`); when enabled it runs
  exactly one constant command — `command -v
  terminal-commander-mcp` — and never concatenates operator input
  into the `bash -lc` argument. Returns `{ status, reason, distro,
  runtime_present, hint }` with `status` covering `{ ok,
  unsupported_host, wsl_not_found, no_distros, distro_not_found,
  unsafe_distro_name, wsl_command_failed, runtime_missing,
  runtime_present, doctor_not_run, check_timeout }`.

WWS03 ships these helpers as library-only modules — the three
`bin/` shims still refuse on Windows with the WWS02 bounded-stderr
+ exit 64 behaviour. WWS04 (`lib/wsl/spawn.js`) owns the actual
bridge spawn; WWS06 (`terminal-commander setup cursor-wsl` /
`doctor` / `pair`) owns the CLI surface and the optional
`%LOCALAPPDATA%\terminal-commander\setup.json` writer. WWS03 itself
performs no file writes, no install, no sudo, no credential
handling, no Cursor configuration, no pairing, and no network
calls.

## Commands

| Command | Purpose |
|---------|---------|
| `terminal-commanderd` | The daemon. Holds the UDS, the bucket manager, the sifter runtime, the audit sink. Unix-only. |
| `terminal-commander-mcp` | The rmcp stdio MCP adapter. Forwards every tool call through the daemon UDS. Unix-only at runtime; the Windows bridge shim arrives in WWS04. |
| `terminal-commander` | Admin CLI. The setup / doctor / pair subcommands arrive in WWS06. |

## Safety model

- The shims `require()` only the bundled resolver and (on Linux)
  `child_process.spawn` the resolved Rust binary with
  `shell: false` and `stdio: 'inherit'`.
- On Windows the shims do **not** invoke `wsl.exe`. Bridge spawn
  is the exclusive responsibility of WWS04's
  `lib/wsl/spawn.js` helper, which constructs the argv array,
  validates the distro against the live `wsl.exe -l -v`
  whitelist, and uses `shell: false` + `windowsHide: true`.
- No postinstall script.
- No network calls at install time.
- No file reads beyond resolving the platform package via
  `require.resolve()`.
- No environment-variable echo.
- The MCP crate inside the Rust binaries refuses to spawn child
  commands, open network listeners, or read files — verified at
  every release by the project guard greps.

## Beta status

`Conditional Go` (TC48 baseline). First npm beta publish is
release-please driven on `main` (operator-approved). Until the
registry lists a version, all three names may still return `E404`.

## Release marker (first npm beta)

- **Root wrapper** (`terminal-commander`): bridge and setup surface on
  Windows; full Linux/WSL2 runtime via platform `optionalDependencies`.
- **Platform packages** (`@terminal-commander/linux-x64`,
  `@terminal-commander/linux-arm64`): carry prebuilt Linux binaries only.
- Supported install surfaces: **linux x64**, **linux arm64**, and
  **win32** for the root package contract (runtime remains in WSL on
  Windows). No macOS package. No musl / Alpine package.

## Documentation

The runtime documentation, MCP tool catalogue, integration guides
(Codex CLI, Claude Code, Cursor), security model, and beta
release checklist all live in the upstream repository:

<https://github.com/special-place-administrator/terminal-commander>

## License

Apache-2.0. See `LICENSE`.
