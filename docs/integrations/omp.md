# OMP integration

Connect OMP (oh-my-pi) to Terminal Commander through MCP stdio.

`terminal-commander setup` has **no OMP provider**. It will not detect OMP
and it will not write `~/.omp/agent/mcp.json`. Add the server by hand. This
wire stays required: point OMP at the same `terminal-commander-mcp` binary
the other harnesses already launch. The stanza is `command` and `args` (plus
`env` only for a non-default `TC_SOCKET`). On dogfood OMP **18.3.4** that
wire does not connect. See
[After the wire](#after-the-wire-protocol-2026-07-28).

Tip accepts MCP **2026-07-28** only. See
[MCP protocol floor](README.md#mcp-protocol-floor).

## Config shape

User scope is `~/.omp/agent/mcp.json`. Merge a `terminal_commander` entry
beside servers you already have. Do not replace the file.

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

`stdio` is OMP's default when `type` is omitted, and `command` is required.
Optional `args` and `env` use the same stdio shape as OMP's other local
servers. A project file `.omp/mcp.json` is also read and, for that working
directory, precedes the user file. Use the user file when you want Terminal
Commander in every OMP session. OMP's own MCP config guide
(<https://omp.sh/docs/mcp>) covers the optional `$schema` line and named
profiles; those are OMP file layout, not a Terminal Commander handshake.

On Windows, prefer the AV-safe direct exe the other harnesses use after
`terminal-commander setup` (or `update` / `restart`) has copied it:

```json
{
  "mcpServers": {
    "terminal_commander": {
      "command": "C:\\Users\\<you>\\AppData\\Local\\terminal-commander\\bin\\terminal-commander-mcp.exe",
      "args": []
    }
  }
}
```

That path drops the npm-shim launch chain. If the copy never ran, the bare
`terminal-commander-mcp` command is the fallback. See
[`cursor.md`](cursor.md#av-safe-direct-exe-launch) for why the stable exe
exists. Add `env.TC_SOCKET` only for a non-default daemon endpoint. On
Windows the default endpoint is a local named pipe and normally does not
need `TC_SOCKET`.

`allow_shell` is off unless an operator sets it in config TOML. If a shell
call is PolicyDenied, follow `recover_hint` `retry_with_argv` (argv
`run_and_watch` / `command_start_combed`). Do not ask to enable shell.

## What setup does not do

There is no `--provider omp`. The harness registry today is `cursor`,
`codex-cli`, `claude-code`, `claude-desktop`, `gemini`, and `kimi`.
`terminal-commander doctor harness` reports those providers. It does not
confirm that `~/.omp/agent/mcp.json` contains `terminal_commander`.

## After the wire: protocol 2026-07-28

The manual `mcp.json` entry is necessary and, on current dogfood OMP, not
sufficient. After `terminal_commander` is wired, OMP still has to speak MCP
**2026-07-28**. Dogfood OMP **18.3.4** opens `initialize` with
`protocolVersion` **2025-11-25**. Tip rejects that handshake with JSON-RPC
`-32022` Unsupported protocol version, and `supported` is only
`["2026-07-28"]`.

OMP's session warning:

```text
Warning: MCP server "terminal_commander" failed to connect: MCP error -32022: Unsupported protocol version; its tools are unavailable for this run.
```

The adapter log pins the requested version:

```text
terminal-commander-mcp: stdio serve failed: initialize failed: -32022: Unsupported protocol version({"requested":"2025-11-25","supported":["2026-07-28"]})
```

The live OMP matrix is **banked** as **client-bump / out-of-support**. Tools
never load, so the teach, argv, and transport runs cannot start. Tip stays
on **2026-07-28** only. OMP's mcp-schema has no opt-in flag analogous to
Codex `mcp_2026_07_28`. Keep the manual wire. It remains the right config,
and it connects only when OMP speaks **2026-07-28**.

## Verify

1. Confirm `terminal-commander-mcp --help` works from the same user account
   (or that the Windows exe path above exists).
2. Start a new OMP session so it reloads MCP config.
3. Ask OMP to list MCP tools and call `system_discover`. On OMP **18.3.4**
   this stops at the handshake in the protocol section above.
   `system_discover` does not run until OMP speaks **2026-07-28**.

Expected Terminal Commander tools include `system_discover`, `health`,
`policy_status`, `command_start_combed`, `bucket_wait`,
`bucket_events_since`, `command_status`, `file_read_window`, `file_search`,
`file_watch_start`, `file_watch_stop`, `file_watch_list`, `pty_command_start`,
`pty_command_write_stdin`, `pty_command_stop`, `pty_command_list`,
`registry_*`, `runtime_state`, `probe_list`, and `probe_status`.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Connect fails with `-32022` (`requested` `2025-11-25`) | The wire can be correct. OMP **18.3.4** is client-bump / out-of-support for this tip. See the protocol section above. mcp-schema has no opt-in flag for **2026-07-28**. |
| OMP has no `terminal_commander` server | Confirm the stanza is in `~/.omp/agent/mcp.json` (or in `.omp/mcp.json` for this project) and start a new session. |
| MCP server failed to start | Confirm `terminal-commander-mcp --help` works from the same user account. |
| Daemon unavailable | Run `terminal-commander doctor daemon`; the MCP adapter normally attempts daemon auto-start on connect. |
| Non-default endpoint | Set `TC_SOCKET` in the server `env` object. |

## Smoke evidence

A provider smoke is live only when an OMP session invokes one Terminal
Commander tool and the bounded response is visible in the session transcript.
Writing `mcp.json` is the wiring step. On OMP **18.3.4** the session still
fails connect before any tool call, so that smoke stays banked.
