# Claude Code integration

Connect Claude Code to Terminal Commander through MCP stdio.

Install and write the MCP config:

```powershell
npm install -g terminal-commander@latest
terminal-commander setup harness --provider claude-code
```

The npm install is passive. The setup command is the explicit step that merges
Terminal Commander into Claude Code's MCP config.

## Config Shape

Claude Code accepts MCP servers in a JSON `mcpServers` block:

```json
{
  "mcpServers": {
    "terminal_commander": {
      "command": "terminal-commander-mcp",
      "args": []
    }
  }
}
```

Only add an env block when you intentionally use a non-default daemon endpoint:

```json
{
  "mcpServers": {
    "terminal_commander": {
      "command": "terminal-commander-mcp",
      "args": [],
      "env": {
        "TC_SOCKET": "/path/to/terminal-commanderd.sock"
      }
    }
  }
}
```

On Windows, the default endpoint is a local named pipe and normally does not
need `TC_SOCKET`. On Unix, the default endpoint is a Unix domain socket.

## Verify

1. Run `terminal-commander doctor harness`.
2. Restart Claude Code.
3. Run `/mcp` and confirm `terminal_commander` is connected.

Expected Terminal Commander tools include `system_discover`, `health`,
`policy_status`, `command_start_combed`, `bucket_wait`,
`bucket_events_since`, `command_status`, `file_read_window`, `file_search`,
`file_watch_start`, `file_watch_stop`, `file_watch_list`, `pty_command_start`,
`pty_command_write_stdin`, `pty_command_stop`, `pty_command_list`,
`registry_*`, `runtime_state`, `probe_list`, and `probe_status`.

## Minimal Flow

Ask Claude Code to:

1. Call `system_discover`.
2. Call `command_start_combed` with argv `["echo", "hello"]`.
3. Call `bucket_wait` with the returned `bucket_id` and `cursor: 0`.
4. Call `command_status` with the returned `job_id`.

Every response is bounded JSON.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| `/mcp` shows no Terminal Commander server | Confirm Claude Code loaded the config and restart the session. |
| MCP server failed to start | Confirm `terminal-commander-mcp --help` works from the same user account. |
| Daemon unavailable | Run `terminal-commander doctor daemon`; the MCP adapter normally attempts daemon auto-start on connect. |
| Non-default endpoint | Set `TC_SOCKET` explicitly in the MCP env block. |

## Smoke Evidence

A provider smoke is live only when a Claude Code session invokes one Terminal
Commander tool and the bounded response is visible in the session transcript.
The local runtime smoke script proves Terminal Commander works without Claude
Code in the loop, but it is not a provider-harness smoke by itself.
