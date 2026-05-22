# Claude Code integration

Connect [Claude Code](https://docs.claude.com/en/docs/claude-code) to
Terminal Commander over MCP stdio. Claude Code supports MCP servers
either through `~/.claude/settings.json` / `claude_desktop_config.json`
style configuration, or by passing `--mcp-config <path>` when
launching `claude`. Either path lets you point Claude Code at the
locally-built `terminal-commander-mcp` adapter, which forwards every
tool call to the local `terminal-commanderd` daemon over its Unix
domain socket.

No secrets, no tokens, no machine-specific absolute paths.

## Prerequisites

- Linux or WSL2 (Terminal Commander's daemon UDS is Unix-only).
- A built workspace.

```sh
cargo build -p terminal-commanderd -p terminal-commander-mcp --bins
```

## Step 1 — start the daemon

```sh
export TC_DATA="${XDG_STATE_HOME:-$HOME/.local/state}/terminal-commander"
mkdir -p "$TC_DATA"

terminal-commanderd --data-dir "$TC_DATA" start --mode ipc-server
```

Leave the daemon running.

## Step 2a — `--mcp-config` flag (recommended for first try)

Save the following as `terminal-commander.mcp.json`:

```json
{
  "mcpServers": {
    "terminal_commander": {
      "command": "terminal-commander-mcp",
      "args": [],
      "env": {
        "TC_SOCKET": "${TC_DATA}/terminal-commanderd.sock"
      }
    }
  }
}
```

Launch Claude Code with the config:

```sh
claude --mcp-config terminal-commander.mcp.json
```

## Step 2b — persistent settings.json entry

To make Terminal Commander always available, add the same block to
your Claude Code settings file (`~/.claude/settings.json` on Linux,
`~/Library/Application Support/Claude/claude_desktop_config.json` on
macOS, or wherever Claude Code documents the canonical location for
your install):

```jsonc
{
  // ...existing settings...
  "mcpServers": {
    "terminal_commander": {
      "command": "terminal-commander-mcp",
      "args": [],
      "env": {
        "TC_SOCKET": "${TC_DATA}/terminal-commanderd.sock"
      }
    }
  }
}
```

Restart Claude Code.

## Step 3 — discover the tools

Inside Claude Code use the `/mcp` slash command to confirm Terminal
Commander is connected. You should see the 29-tool TC45 catalogue
(see [`codex-cli.md`](codex-cli.md) for the full list).

## Step 4 — minimal flow

Ask the assistant to:

1. Call `command_start_combed` with argv `["echo", "hello"]`.
2. Call `bucket_wait` with the returned `bucket_id` and `cursor: 0`.
3. Call `command_status` with the returned `job_id`.

Every response is a bounded JSON envelope.

## Troubleshooting

- **`/mcp` shows no servers.** Confirm Claude Code loaded the
  config file (`--mcp-config` path is absolute, settings file is the
  right one for your platform). Re-launch.
- **`terminal-commander-mcp` exits immediately.** It is Unix-only.
  On native Windows it returns exit code 64 with a stderr message;
  use WSL2.
- **Requests time out.** The daemon is not running or `TC_SOCKET`
  does not match the daemon socket path.

## Smoke evidence requirements

The Claude Code provider-harness smoke is considered "live" only if
a Claude Code session actually invokes one of the Terminal Commander
tools and the response is observed in the session transcript. If
your environment lacks a usable Claude Code CLI (no `claude` binary,
no auth, sandboxed CI, etc.), mark the provider smoke as `Not Run`
or `Blocked` in your report and cite the exact reason.

A local daemon + MCP stdio smoke without Claude Code in the loop is
available via `scripts/smoke/verify-runtime-smoke.sh`; it is
secondary evidence, not provider-harness success.
