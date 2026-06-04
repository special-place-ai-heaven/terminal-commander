# Codex CLI integration

Connect Codex CLI to Terminal Commander through MCP stdio.

Install and write the MCP config:

```powershell
npm install -g terminal-commander@latest
terminal-commander setup harness --provider codex-cli
```

The npm install is passive. The setup command is the explicit step that merges
Terminal Commander into `~/.codex/config.toml`.

## Config Shape

Codex CLI reads MCP servers from `~/.codex/config.toml`:

```toml
[mcp_servers.terminal_commander]
command = "terminal-commander-mcp"
args = []
```

Only add an env block when you intentionally use a non-default daemon endpoint:

```toml
[mcp_servers.terminal_commander.env]
TC_SOCKET = "/path/to/terminal-commanderd.sock"
```

On Windows, the default endpoint is a local named pipe and normally does not
need `TC_SOCKET`. On Unix, the default endpoint is a Unix domain socket.

## AV-Safe Direct-Exe Launch

On `setup` (and `update` / `restart`), Terminal Commander copies the native exe
into a stable per-user directory and writes `command` as that exe path with
`args = []` instead of the bare `terminal-commander-mcp` name:

```toml
[mcp_servers.terminal_commander]
command = "C:\\Users\\<you>\\AppData\\Local\\terminal-commander\\bin\\terminal-commander-mcp.exe"
args = []
```

This removes the npm-shim -> node -> JS-shim launch chain that heuristic
antivirus reads as a loader. It is user-space and no-admin; if the copy cannot
complete it falls back to the bare-name command. See
[`cursor.md`](cursor.md#av-safe-direct-exe-launch) for the full rationale and
the opt-in logon-task option.

## Verify

1. Run `terminal-commander doctor harness`.
2. Start a new Codex CLI session.
3. Ask Codex to list available MCP tools.

Expected Terminal Commander tools include `system_discover`, `health`,
`policy_status`, `command_start_combed`, `bucket_wait`,
`bucket_events_since`, `command_status`, `file_read_window`, `file_search`,
`file_watch_start`, `file_watch_stop`, `file_watch_list`, `pty_command_start`,
`pty_command_write_stdin`, `pty_command_stop`, `pty_command_list`,
`registry_*`, `runtime_state`, `probe_list`, and `probe_status`.

## Minimal Flow

Ask the assistant to:

1. Call `system_discover`.
2. Call `command_start_combed` with argv `["echo", "hello"]`.
3. Call `bucket_wait` with the returned `bucket_id` and `cursor: 0`.
4. Call `command_status` with the returned `job_id`.

Every response is bounded JSON. Raw stdout/stderr should not be pasted into the
conversation.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Codex reports the MCP server failed to start | Confirm `terminal-commander-mcp --help` works from the same user account. |
| No tools listed | Restart Codex CLI or rename the server key to refresh the catalogue. |
| Daemon unavailable | Run `terminal-commander doctor daemon`; the MCP adapter normally attempts daemon auto-start on connect. |
| Non-default endpoint | Set `TC_SOCKET` explicitly in the MCP env block. |

## Smoke Evidence

A provider smoke is live only when a Codex CLI session invokes one Terminal
Commander tool and the bounded response is visible in the session transcript.
The local runtime smoke script proves Terminal Commander works without Codex in
the loop, but it is not a provider-harness smoke by itself.
