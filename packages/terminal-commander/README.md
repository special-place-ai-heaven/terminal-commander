# terminal-commander

npm root wrapper for [Terminal Commander](https://github.com/special-place-administrator/terminal-commander) — local MCP control plane for coding agents.

**Install once (zero-touch):**

```powershell
# Windows
npm install -g terminal-commander@latest
```

```sh
# Linux / WSL
npm install -g terminal-commander@latest
```

That configures detected harnesses (Cursor, Codex, Claude, …), installs the WSL runtime on Windows, and sets up daemon autostart. Restart your harness MCP after install.

## Commands

| Binary | Role |
|--------|------|
| `terminal-commander-mcp` | MCP stdio (Windows → WSL bridge) |
| `terminal-commanderd` | Daemon (Linux/WSL only) |
| `terminal-commander` | `doctor harness`, `doctor wsl`, `doctor daemon`, `setup harness` |

## Windows bridge

On `win32`, `terminal-commander-mcp` delegates to WSL via `lib/wsl/spawn.js`:

- Linux-first `PATH` in `bash -lc` (avoids `/mnt/c/.../nodejs` shim).
- Sources `~/.config/terminal-commander/autostart.sh` before MCP.
- Re-execs native Linux MCP if the Windows shim is still invoked under `/mnt/c`.

## Platform packages

Optional dependencies (Linux only):

- `@terminal-commander/linux-x64`
- `@terminal-commander/linux-arm64`

## Documentation

Full README, architecture diagrams, and integration guides:

<https://github.com/special-place-administrator/terminal-commander/blob/main/README.md>

## License

Apache-2.0
