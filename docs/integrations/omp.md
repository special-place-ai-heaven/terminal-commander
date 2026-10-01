# OMP integration

Connect OMP (oh-my-pi) to Terminal Commander through MCP stdio.

Install the package, then configure OMP explicitly:

```powershell
npm install -g terminal-commander@latest
terminal-commander setup harness --provider omp
```

Setup writes `~/.omp/agent/mcp.json`, points it at the same stable native
adapter as the other harnesses, and preserves unrelated settings.

The adapter accepts OMP's **2025-11-25** handshake. See
[MCP protocol floor](README.md#mcp-protocol-floor).

## Config shape

User scope is `~/.omp/agent/mcp.json`. Setup merges a
`terminal-commander` entry beside servers you already have.

```json
{
  "mcpServers": {
    "terminal-commander": {
      "command": "/home/<user>/.local/share/terminal-commander/bin/terminal-commander-mcp",
      "args": []
    }
  }
}
```

`stdio` is OMP's default when `type` is omitted, and `command` is required.
Optional `args` and `env` use the same stdio shape as OMP's other local
servers. Explicit setup adds `terminal-commander` to `enabledServers` when
that list exists and removes it from `disabledServers`. Automatic installation
refreshes preserve those lists, including an intentionally disabled entry.
A project file `.omp/mcp.json` is also read and, for that working
directory, precedes the user file. Use the user file when you want Terminal
Commander in every OMP session. OMP's own MCP config guide
(<https://github.com/can1357/oh-my-pi/blob/main/docs/mcp-config.md>) covers the optional `$schema` line and named
profiles; those are OMP file layout, not a Terminal Commander handshake.

On Windows, prefer the AV-safe direct exe the other harnesses use after
`terminal-commander setup` (or `update` / `restart`) has copied it:

```json
{
  "mcpServers": {
    "terminal-commander": {
      "command": "C:\\Users\\<you>\\AppData\\Local\\terminal-commander\\bin\\terminal-commander-mcp.exe",
      "args": []
    }
  }
}
```

That path drops the npm-shim launch chain. If the stable copy cannot be made,
setup uses a resolved installed native adapter. If no usable native adapter
resolves, registration is deferred without changing the configuration; install
the platform package and rerun setup. Setup does not write a bare command that
depends on OMP's `PATH`. See [harness configuration](harnesses.md) for discovery
and refresh rules. Add `env.TC_SOCKET` only for a non-default daemon endpoint. On
Windows the default endpoint is a local named pipe and normally does not
need `TC_SOCKET`.

`allow_shell` is on in the default `full_access` profile (TC inherits the
harness's trust); a hardened config sets `[policy.caps] allow_shell = false`. If a shell
call is PolicyDenied, follow `recover_hint` `retry_with_argv` (argv
`run_and_watch` / `command_start_combed`) first; the deny names the
profile and the config key that changes it.

## Setup scope

The provider is `omp`. Setup writes the user config by default. To configure
a project, supply its root:

```sh
terminal-commander setup harness --provider omp --project /absolute/project/path
```

This writes `<project>/.omp/mcp.json`. Use an absolute Windows path on Windows.
An explicit provider selection can create the configuration without existing
OMP markers. `terminal-commander doctor harness` reports whether the OMP entry
is enabled in the user config.

## Protocol compatibility

OMP **18.3.4** opens `initialize` with `protocolVersion` **2025-11-25**.
The adapter accepts that revision. OMP has no modern-protocol opt-in
analogous to Codex's `mcp_2026_07_28`.

## Verify

1. Confirm `terminal-commander-mcp --help` works from the same user account
   (or that the Windows exe path above exists).
2. Run `/mcp reload` or start a new OMP session so it reloads MCP config.
3. Ask OMP to list MCP tools and call `system_discover`.

Expected Terminal Commander tools include `system_discover`, `health`,
`policy_status`, `command_start_combed`, `bucket_wait`,
`bucket_events_since`, `command_status`, `file_read_window`, `file_search`,
`file_watch_start`, `file_watch_stop`, `file_watch_list`, `pty_command_start`,
`pty_command_write_stdin`, `pty_command_stop`, `pty_command_list`,
`registry_*`, `runtime_state`, `probe_list`, and `probe_status`.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Connect fails with `-32022` (`requested` `2025-11-25`) | Check that OMP launches an adapter build with legacy support, not the previous modern-only binary. |
| OMP has no `terminal-commander` server | Confirm the stanza is in `~/.omp/agent/mcp.json` (or in `.omp/mcp.json` for this project) and start a new session. |
| MCP server failed to start | Confirm `terminal-commander-mcp --help` works from the same user account. |
| Daemon unavailable | Run `terminal-commander doctor daemon`; the MCP adapter normally attempts daemon auto-start on connect. |
| Non-default endpoint | Set `TC_SOCKET` in the server `env` object. |

## Smoke evidence

A provider smoke is live only when an OMP session invokes one Terminal
Commander tool and the bounded response is visible in the session transcript.
Writing `mcp.json` is the wiring step; a provider smoke requires a live tool call.
