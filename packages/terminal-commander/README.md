# terminal-commander

npm root wrapper for [Terminal Commander](https://github.com/special-place-ai-heaven/terminal-commander), a local MCP control plane for coding agents.

## Install

```powershell
npm install -g terminal-commander@latest
```

Install is passive: no lifecycle bootstrap, no automatic MCP config writes, no
daemon start, no WSL install, and no hidden helper process.

Configure harnesses explicitly:

```powershell
terminal-commander setup harness
```

Or target one provider:

```powershell
terminal-commander setup harness --provider cursor
terminal-commander setup harness --provider codex-cli
terminal-commander setup harness --provider claude-code
terminal-commander setup harness --provider claude-desktop
```

## Update

```powershell
terminal-commander update
```

This runs `npm install -g terminal-commander@latest`.

On Windows, update first runs a native scoped lock preflight. It terminates only
Terminal Commander binaries currently running from the installed npm platform
package `bin` directory. It does not invoke `cmd.exe`, PowerShell, `taskkill`, or
downloaded scripts.

## Commands

| Binary | Role |
| --- | --- |
| `terminal-commander` | Admin CLI: version, update, setup, doctor, native diagnostics |
| `terminal-commander-mcp` | MCP stdio adapter launched by the LLM harness |
| `terminal-commanderd` | Local daemon for IPC, probes, policy, buckets, and audit |

## Platform Packages

Optional platform dependencies:

- `@terminal-commander/linux-x64`
- `@terminal-commander/linux-arm64`
- `@terminal-commander/windows-x64`
- `@terminal-commander/mac-x64`
- `@terminal-commander/mac-arm64`

Windows uses the native `@terminal-commander/windows-x64` package by default.
The legacy Windows-to-WSL bridge is opt-in with `TC_USE_LEGACY_WSL_BRIDGE=1`.

## Documentation

Full README, architecture diagrams, and integration guides:

<https://github.com/special-place-ai-heaven/terminal-commander/blob/main/README.md>

## License

Apache-2.0
