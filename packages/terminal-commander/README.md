# terminal-commander

npm root wrapper for [Terminal Commander](https://github.com/special-place-ai-heaven/terminal-commander) — local MCP control plane for coding agents.

## Install

```powershell
# Windows
npm install -g terminal-commander@latest
```

```sh
# Linux / WSL
npm install -g terminal-commander@latest
```

Install is passive: no lifecycle bootstrap, no automatic MCP config writes, no
daemon start, and no hidden WSL install.

Update explicitly:

```sh
terminal-commander update
```

This runs `npm install -g terminal-commander@latest`.

Configure harnesses explicitly after install or update:

```sh
terminal-commander setup harness --provider cursor
terminal-commander setup harness --provider codex-cli
terminal-commander setup harness --provider claude-code
```

## Commands

| Binary | Role |
|--------|------|
| `terminal-commander-mcp` | MCP stdio adapter |
| `terminal-commanderd` | Daemon |
| `terminal-commander` | `doctor harness`, `doctor wsl`, `doctor daemon`, `setup harness` |

## Windows

On `win32`, the default path uses the `@terminal-commander/windows-x64`
platform package. The legacy WSL bridge is still available only when
`TC_USE_LEGACY_WSL_BRIDGE=1`:

- Linux-first `PATH` in `bash -lc` (avoids `/mnt/c/.../nodejs` shim).
- Sources `~/.config/terminal-commander/autostart.sh` before MCP.
- Re-execs native Linux MCP if the Windows shim is still invoked under `/mnt/c`.

## Platform packages

Optional platform dependencies:

- `@terminal-commander/linux-x64`
- `@terminal-commander/linux-arm64`
- `@terminal-commander/windows-x64`
- `@terminal-commander/mac-x64`
- `@terminal-commander/mac-arm64`

## Documentation

Full README, architecture diagrams, and integration guides:

<https://github.com/special-place-ai-heaven/terminal-commander/blob/main/README.md>

## License

Apache-2.0
