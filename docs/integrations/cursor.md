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

## AV-Safe Direct-Exe Launch

On `setup` (and on `update` / `restart`), Terminal Commander copies the resolved
native executables into a fixed per-user directory it owns and points the MCP
config at that exe directly:

```json
{
  "mcpServers": {
    "terminal-commander": {
      "type": "stdio",
      "command": "C:\\Users\\<you>\\AppData\\Local\\terminal-commander\\bin\\terminal-commander-mcp.exe",
      "args": []
    }
  }
}
```

Why: launching the bare `terminal-commander-mcp` name makes the client run the
npm launcher shim, then `node`, then the JS shim, which finally spawns the
native exe. That script-interpreter-then-spawn chain is exactly what heuristic
antivirus reads as a loader. Pointing the config at the exe directly removes the
whole chain so the client launches one self-contained stdio MCP server.

This is entirely user-space and no-admin: copying a tool into your own
`%LOCALAPPDATA%` (or `~/.local/share` on Unix) and running it is ordinary
developer behavior. Nothing is elevated, no service is installed, and no
certificate or allowance is required.

The stable path is deliberately NOT the
`node_modules\@terminal-commander\windows-x64\bin\...` path, because npm
hoisting moves that on update and would silently break the config. The copy is
re-run whenever the installed version changes, so the stable exe always matches
the installed build (this also removes any adapter/daemon version skew).

If the copy cannot complete (for example a locked file mid-update), setup falls
back to the portable bare-name command shown above so it never hard-fails.

On macOS and Linux the same approach points the config at the resolved native
exe (no script shim exists there).

## Pre-Start the Daemon at Logon (Windows, opt-in)

By default the MCP adapter starts the daemon on first connect. If you would
rather have the daemon already running so the MCP server spawns nothing, you can
register an OPT-IN, per-user logon Scheduled Task:

```powershell
terminal-commander setup daemon-logon
```

This runs, as your own user with no elevation:

```text
schtasks.exe /Create /SC ONLOGON /TN "TerminalCommander Daemon" \
  /TR "<stable>\terminal-commanderd.exe start" /F
```

It is per-user only: there is no `/RU SYSTEM`, no `/RL HIGHEST`, no admin, and
no certificate or allowance. `schtasks` is the documented user tool for
scheduling your own tasks, and the task points at the same stable per-user exe
path used for the direct-exe config above.

It is OFF by default (nothing is registered unless you run the command),
idempotent (re-running overwrites the same task), and removable:

```powershell
terminal-commander setup daemon-logon --uninstall
```

Add `--dry-run` to either form to print the exact `schtasks` action without
registering anything.

On Linux / WSL use `terminal-commander setup daemon-autostart` instead.

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

## Per-Harness Session Isolation

Each harness (Cursor, Codex, Claude Code, ...) gets its own daemon endpoint so
two agents on one machine do not share a single daemon. This is keyed by an
opaque `TC_SESSION` token in the server `env`:

```json
"terminal-commander": {
  "type": "stdio",
  "command": "terminal-commander-mcp",
  "env": { "TC_SESSION": "tc-0a1b2c3d4e5f" }
}
```

- `terminal-commander setup harness` mints a stable per-harness token and writes
  it for you. Re-running setup does not churn the token.
- Endpoint precedence is `TC_SOCKET` (full path/pipe override) > `TC_SESSION`
  (token) > per-user default. With neither set, the endpoint is byte-identical
  to pre-session behavior (a single shared per-user daemon).
- A malformed token (`[A-Za-z0-9._-]`, 1-64 chars, at least one alphanumeric)
  is rejected and falls back to the shared default with a warning.
- **Manual escape hatch:** for a non-`setup` flow, `export TC_SESSION=<token>`
  before launching the MCP/CLI. On Windows+WSL the bridge forwards `TC_SESSION`
  into WSL automatically (via `WSLENV`); if you launch the Linux MCP yourself,
  set `TC_SESSION` inside WSL.
- `terminal-commander doctor harness` warns "shared daemon mode" when multiple
  harnesses are detected so you know to re-run setup.

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
