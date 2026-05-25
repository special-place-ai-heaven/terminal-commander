# Cursor MCP integration

Connect Cursor to Terminal Commander through MCP stdio.

Cursor reads MCP servers from `mcp.json`. Terminal Commander can write that
file for you:

```powershell
npm install -g terminal-commander@latest
terminal-commander setup harness --provider cursor
```

The npm install is passive. The setup command is the explicit step that merges
the Cursor MCP stanza.

## Config Locations

| Scope | Path | When to use |
| --- | --- | --- |
| Global | `~/.cursor/mcp.json` on Unix, `%USERPROFILE%\.cursor\mcp.json` on Windows | Available in every Cursor workspace |
| Project | `.cursor/mcp.json` at the workspace root | Available only for that workspace |

Do not commit a personal `.cursor/mcp.json` without review. MCP servers run
with your user permissions.

## Recommended Config

Use this shape on Linux, Windows, macOS, WSL, and project scopes:

```json
{
  "mcpServers": {
    "terminal-commander": {
      "type": "stdio",
      "command": "terminal-commander-mcp",
      "args": []
    }
  }
}
```

`terminal-commander setup harness --provider cursor` writes this server entry
and preserves unrelated MCP servers.

## Windows

Windows uses the native `@terminal-commander/windows-x64` package by default.
Cursor launches `terminal-commander-mcp` on the Windows host. The adapter talks
to the daemon over a local named pipe and can start the sibling
`terminal-commanderd.exe` when needed.

No WSL bridge is required for the default path.

The legacy Windows-to-WSL path is still available for operators who explicitly
want it:

```json
{
  "mcpServers": {
    "terminal-commander": {
      "type": "stdio",
      "command": "terminal-commander-mcp",
      "args": [],
      "env": {
        "TC_USE_LEGACY_WSL_BRIDGE": "1",
        "TC_WSL_DISTRO": "Ubuntu"
      }
    }
  }
}
```

Use the legacy path only when the Terminal Commander runtime is intentionally
installed inside WSL and the Windows host should bridge into that distro.

## Verify In Cursor

1. Run `terminal-commander doctor harness`.
2. Reload Cursor.
3. Open Cursor Settings -> Tools & MCP.
4. Confirm `terminal-commander` is enabled and connected.
5. In a new agent chat, ask:

```text
List every MCP server prefix and Terminal Commander tool you can call.
```

Expected Terminal Commander tools include `system_discover`, `health`,
`command_start_combed`, `bucket_wait`, `bucket_events_since`,
`command_status`, `file_read_window`, `file_search`, `file_watch_start`,
`file_watch_stop`, `file_watch_list`, `pty_command_start`,
`pty_command_write_stdin`, `pty_command_stop`, `pty_command_list`,
`registry_*`, `runtime_state`, `probe_list`, and `probe_status`.

## Minimal Agent Flow

Ask Cursor to use Terminal Commander:

```text
Call system_discover.
Start argv ["echo","hello"] with command_start_combed.
Wait on the returned bucket with bucket_wait.
Read command_status for the returned job.
```

The response should be bounded JSON. Cursor should not paste raw terminal
scrollback into the chat.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Cursor shows server start error | Run `terminal-commander-mcp --help` and `terminal-commander doctor daemon` from the same user account. |
| Cursor cannot find the command | Confirm the npm global bin directory is on the Windows or Unix `PATH` visible to Cursor. |
| Tools are missing after a config change | Reload Cursor or disable/enable the server in Tools & MCP. |
| Daemon is unavailable | Run `terminal-commander doctor daemon`; the MCP adapter normally attempts daemon auto-start on connect. |
| You intentionally use WSL | Set `TC_USE_LEGACY_WSL_BRIDGE=1` and `TC_WSL_DISTRO=<name>` in the server env. |

## Security Notes

- The Cursor config exposes only `terminal-commander-mcp`.
- No HTTP or SSE transport is configured.
- Do not put API keys or secrets in `mcp.json`.
- Terminal Commander tools return bounded JSON and pointer-based context.
- Command execution is argv-first and policy-gated by the daemon.

## Manual Examples

Example files live in
[`examples/provider-harness/cursor/`](../../examples/provider-harness/cursor/).
The native example is the recommended shape. The `linux-wsl` example is kept as
a legacy manual bridge reference for operators who still need it.
