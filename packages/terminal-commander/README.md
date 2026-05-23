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
| `terminal-commander-mcp` | **Refuses** with a single bounded stderr line + exit `64`. WWS04 replaces this with the real `wsl.exe` bridge shim. |
| `terminal-commander` | **Refuses** with a single bounded stderr line + exit `64`. WWS06 adds the `setup cursor-wsl` / `doctor` / `pair` subcommands. |

WWS02 is the package-contract slice of the chain. The actual
Windows -> WSL bridge invocation (`wsl.exe -d <distro> bash -lc
terminal-commander-mcp`) lands in WWS04; the Cursor MCP config
writer lands in WWS05; the setup CLI lands in WWS06. The contract
locking all of this is
[`docs/release/windows-wsl-bridge-contract.md`](https://github.com/special-place-administrator/terminal-commander/blob/main/docs/release/windows-wsl-bridge-contract.md).

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

`Conditional Go` (TC48 baseline). The npm package is **not yet
published**; all three names remain `E404` on the registry. The
publish floor recommended by WWS01 is `WWS02 + WWS04 + WWS05 +
WWS06 + WWS08`. Publishing the current root with the WWS02
amendment alone would still leave Windows operators without a
working bridge.

## Documentation

The runtime documentation, MCP tool catalogue, integration guides
(Codex CLI, Claude Code, Cursor), security model, and beta
release checklist all live in the upstream repository:

<https://github.com/special-place-administrator/terminal-commander>

## License

Apache-2.0. See `LICENSE`.
