# Harness configuration

Terminal Commander registers its local stdio adapter with these providers:

| `--provider` | Global configuration | Project configuration with `--project` | Managed key / format |
| --- | --- | --- | --- |
| `cursor` | `~/.cursor/mcp.json` | `.cursor/mcp.json` | `mcpServers.terminal-commander`, JSON |
| `codex-cli` | `$CODEX_HOME/config.toml`, otherwise `~/.codex/config.toml` | `.codex/config.toml` | `[mcp_servers.terminal_commander]`, TOML |
| `omp` | `~/.omp/agent/mcp.json` | `.omp/mcp.json` | `mcpServers.terminal-commander`, JSON |
| `claude-code` | `~/.claude.json` | `.mcp.json` | `mcpServers.terminal_commander`, JSON |
| `claude-desktop` | OS-specific path below | Global only | `mcpServers.terminal_commander`, JSON |
| `gemini` | `~/.gemini/settings.json` | `.gemini/settings.json` | `mcpServers.terminal_commander`, JSON |
| `grok` | `~/.grok/config.toml` | `.grok/config.toml` | `[mcp_servers.terminal_commander]`, TOML |
| `kilo-code` | `$XDG_CONFIG_HOME/kilo/kilo.jsonc`, otherwise `~/.config/kilo/kilo.jsonc` | Existing `kilo.jsonc`, `kilo.json`, `.kilo/kilo.jsonc`, or `.kilo/kilo.json`; otherwise `.kilo/kilo.jsonc` | `mcp.terminal_commander`, JSON/JSONC |

Kilo global discovery also accepts an existing `kilo.json`. Project discovery
falls back to an existing `.kilocode/mcp.json` after the modern candidates;
that legacy file uses `mcpServers` and the classic stdio shape. Setup does not
create a new legacy file. Kilo JSONC writes preserve parsed values but normalize
formatting and remove comments.

Claude Desktop uses `%APPDATA%\Claude\claude_desktop_config.json` on Windows
(with a user-profile AppData fallback) and
`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS.
The implementation also recognizes `~/.config/Claude/claude_desktop_config.json`
on Linux. This Linux path is a compatibility convention; the supplied primary
research did not verify it as an Anthropic-supported MCP path.

`grok` means Grok CLI. A Grok Bot orchestrator using Cursor uses `cursor` and
`~/.cursor/mcp.json`; there is no separate Grok Bot configuration target.

## Configure or repair

```sh
terminal-commander setup harness
terminal-commander setup harness --provider cursor
terminal-commander setup harness --provider gemini --project /absolute/project/path
```

Without a provider filter, setup discovers harnesses from their configuration
files or containing directories. It does not scan every executable on `PATH`
or register every harness whose schema looks compatible. An explicit supported
`--provider` can create its configuration without existing discovery markers.
`--project` selects project scope for all supported project writers; Claude
Desktop is skipped because it has no project configuration target.

Windows uses the current process's `%USERPROFILE%`; Linux and macOS use `HOME`.
WSL runs resolve Linux paths inside the active distribution. Headless Linux VMs
and cloud bots follow the same Linux rules. Run setup as the account and inside
the OS environment that launches the harness. A Windows invocation does not
discover every WSL user's configurations, and a Linux invocation does not
configure another user's home.

Installation outside CI attempts setup for detected harnesses, even without an
interactive terminal. CI and `TC_NO_AUTO_SETUP=1` skip automatic setup; explicit
`setup harness` remains available. Automatic bootstrap preserves OMP's existing
`enabledServers` and `disabledServers` lists. Explicit setup activates its
managed OMP entry in those lists when they exist.

## What a refresh changes

Setup prefers the stable per-user native adapter, then a resolved installed
native adapter. A native launch has an absolute `command` and `args: []`.
If no usable native adapter resolves, bootstrap defers registration without
modifying the harness configuration. Install the platform package and rerun
setup. It does not register a bare command that depends on the harness's
`PATH`, or a disposable npm/npx cache binary. The command builder also supports
explicit Node-plus-shim launches, keeping the shim argument when those are
requested; automatic bootstrap requires a usable native adapter.

All writers replace the complete launch pair together. Kilo's equivalent is
`type: "local"`, an argv array in `command`, and `environment` instead of `env`.
They retain other servers, unrelated settings, custom environment values, and
existing managed-entry policy fields such as tool restrictions or enablement.
Generated session information is refreshed; stale `TC_WSL_DISTRO` is removed
when the new launch does not use it. An existing `TC_SURFACE` is retained unless
setup explicitly selects another surface.

Codex adds `required = true` and `startup_timeout_sec = 60` when these fields
are missing. Explicit existing values, including `required = false`, remain.
Grok CLI does not receive Codex-specific defaults or protocol environment
settings.

Changed files receive timestamped `.bak` backups beside the original and are
replaced through an atomic write. Malformed JSON/JSONC, recognized malformed
TOML, unsupported managed TOML layouts, or an existing remote server under the
managed name are refused instead of converted to a local launch. The TOML
editor is scoped, not a complete TOML validator. Review a refusal and repair
the affected entry explicitly; forcing setup does not bypass these checks.

Kimi remains an unverified stub. Cline, Roo, Windsurf, VS Code, Zed, Continue,
and Amazon Q are not automatically registered by this implementation.

## Reload and verify

| Harness | After changing configuration |
| --- | --- |
| Cursor, including Cursor-based Grok Bot | Restart Cursor / its host session. |
| Codex CLI | Start a new session; project configuration also requires a trusted project. |
| Claude Code | Restart the session and inspect `/mcp`. |
| Claude Desktop | Fully quit and restart the application. |
| Gemini CLI | Run `/mcp reload` or restart the session. |
| OMP | Run `/mcp reload` or start a new session. |
| Kilo Code | Refresh/restart its MCP connection through the client UI. |
| Grok CLI | Restart the client session so it reads the updated configuration. |

Confirm the managed server appears, then call `system_discover` and `health`.
These live calls verify the client launches the adapter and reaches the daemon;
writing a configuration file alone does not.

## Primary references

- [Cursor MCP](https://cursor.com/docs/mcp)
- [Codex MCP](https://learn.chatgpt.com/docs/extend/mcp.md)
- [Claude Code MCP](https://code.claude.com/docs/en/mcp)
- [Claude Desktop local-server setup](https://modelcontextprotocol.io/docs/2026-07-28/develop/connect-local-servers)
- [Gemini CLI MCP](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md)
- [OMP MCP configuration](https://github.com/can1357/oh-my-pi/blob/main/docs/mcp-config.md)
- [Kilo Code MCP](https://kilo.ai/docs/automate/mcp/using-in-kilo-code)
- [Grok CLI MCP](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/07-mcp-servers.md)
