# terminal-commander

npm root wrapper for [Terminal Commander](https://github.com/special-place-ai-heaven/terminal-commander), a local MCP control plane for coding agents.

## Install

```powershell
npm install -g terminal-commander@latest
```

npm install outside CI attempts guarded setup for detected harnesses.
CI and `TC_NO_AUTO_SETUP=1` skip auto-setup.

Configure or repair harnesses explicitly:

```powershell
terminal-commander setup harness
```

Or target one provider:

```powershell
terminal-commander setup harness --provider cursor
terminal-commander setup harness --provider codex-cli
terminal-commander setup harness --provider omp
terminal-commander setup harness --provider claude-code
terminal-commander setup harness --provider claude-desktop
terminal-commander setup harness --provider gemini
terminal-commander setup harness --provider grok
terminal-commander setup harness --provider kilo-code
```

Explicit provider selection also works before the harness creates a config
file. Add `--project <absolute-project-path>` for project scope; Claude Desktop
supports global configuration only. Grok Bot installations using Cursor should
select `cursor`; `grok` selects Grok CLI.

Setup refreshes the complete launch command and arguments while retaining
existing policy fields, custom environment values, and unrelated servers.
Native launches use an absolute executable path and empty arguments. If no
usable native adapter resolves, bootstrap defers registration without changing
the configuration. Changed configs receive
timestamped backups. Kilo uses its modern `mcp`/argv-array format; JSONC writes
normalize formatting and remove comments.

See the [harness configuration reference](https://github.com/special-place-ai-heaven/terminal-commander/blob/main/docs/integrations/harnesses.md)
for Windows, Linux, WSL, and cloud-host paths, discovery, and reload steps.

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

## LLM Trust Contract

`system_discover` is the source of truth for the live MCP tool catalogue. It is
callable even when the daemon is down and reports `daemon_available` plus
per-tool `requires_daemon`, `available`, and `unavailable_reason` fields. Tools
that require the daemon report `daemon_unavailable` instead of forcing clients
to infer reachability from raw pipe or socket errors.

The admin CLI also refuses fake offline success. Daemon-backed inspection
commands that are not wired to live daemon IPC exit `69` with an unavailable
message instead of returning empty or not-found success.

## Platform Packages

Optional platform dependencies:

- `@terminal-commander/linux-x64`
- `@terminal-commander/linux-arm64`
- `@terminal-commander/windows-x64`
- `@terminal-commander/mac-x64`
- `@terminal-commander/mac-arm64`

Windows uses the native `@terminal-commander/windows-x64` package by default.
The legacy Windows-to-WSL bridge is opt-in with `TC_USE_LEGACY_WSL_BRIDGE=1`.

## Build Provenance

Current native packages are compiled with Rust 1.97.1. Installing Terminal
Commander does not require Rust; the workspace's supported MSRV remains a
separate source-development contract.

## Documentation

Full README, architecture diagrams, and integration guides:

<https://github.com/special-place-ai-heaven/terminal-commander/blob/main/README.md>

## License

PolyForm-Noncommercial-1.0.0
