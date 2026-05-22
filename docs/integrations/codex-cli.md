# Codex CLI integration

Connect [Codex CLI](https://developers.openai.com/codex) to Terminal Commander
over MCP stdio. Codex supports MCP servers via `~/.codex/config.toml` —
this doc ships a copy-pasteable config that points Codex at the
locally-built `terminal-commander-mcp` adapter, which forwards every
tool call to the local `terminal-commanderd` daemon over its Unix
domain socket.

No secrets, no tokens, no machine-specific absolute paths.

## Prerequisites

- Linux or WSL2 (Terminal Commander's daemon UDS is Unix-only).
- A built workspace. The smoke script and these examples assume
  `CARGO_TARGET_DIR=target-wsl`, but any target dir works as long as
  `terminal-commanderd` and `terminal-commander-mcp` are on `$PATH`
  or referenced by absolute path in `config.toml`.

Build the binaries once:

```sh
cargo build -p terminal-commanderd -p terminal-commander-mcp --bins
```

## Step 1 — start the daemon

The daemon must be running before Codex spawns the MCP adapter.

```sh
# Replace TC_DATA with any writable directory. The daemon creates
# `$TC_DATA/terminal-commanderd.sock`.
export TC_DATA="${XDG_STATE_HOME:-$HOME/.local/state}/terminal-commander"
mkdir -p "$TC_DATA"

terminal-commanderd --data-dir "$TC_DATA" start --mode ipc-server
```

Leave the daemon running in a separate terminal (or under your
process supervisor of choice).

## Step 2 — point Codex at the MCP adapter

Add the following block to `~/.codex/config.toml`:

```toml
# Terminal Commander MCP stdio adapter.
#
# - Codex spawns this binary; the binary serves an rmcp stdio MCP
#   server and forwards every tool call to the daemon UDS at
#   `$TC_DATA/terminal-commanderd.sock`.
# - `TC_DATA` MUST match the value the daemon was started with.
# - No credentials. No network. No raw stream lane.
[mcp_servers.terminal_commander]
command = "terminal-commander-mcp"
# Optional: list of CLI args. The adapter supports `--socket <path>`
# but we prefer the env-var path so the same config works regardless
# of where the daemon is started from.
args = []
# Optional: env block. Codex passes this verbatim to the child process.
[mcp_servers.terminal_commander.env]
TC_SOCKET = "${TC_DATA}/terminal-commanderd.sock"
```

If `terminal-commander-mcp` is not on your `$PATH`, replace
`command` with an absolute or workspace-relative path. Avoid
hardcoding `/home/<your-user>/...` in committed configs.

## Step 3 — discover the tools

In a Codex session, ask for the list of available MCP tools. You
should see at least the 29 tools shipped by Terminal Commander
TC45, e.g.:

- `system_discover`
- `health`
- `policy_status`
- `command_start_combed`
- `bucket_wait`
- `bucket_events_since`
- `command_status`
- `file_read_window`
- `file_search`
- `file_watch_start`
- `file_watch_stop`
- `file_watch_list`
- `pty_command_start`
- `pty_command_write_stdin`
- `pty_command_stop`
- `pty_command_list`
- `registry_*`
- `runtime_state`
- `probe_list`
- `probe_status`

## Step 4 — minimal flow

Ask the assistant to:

1. Call `command_start_combed` with argv `["echo", "hello"]`.
2. Call `bucket_wait` with the returned `bucket_id` and `cursor: 0`.
3. Call `command_status` with the returned `job_id`.

Every response is a bounded JSON envelope. No raw stdout / stderr
appears anywhere in the conversation.

## Troubleshooting

- **Codex reports the MCP server failed to start.** Confirm
  `terminal-commander-mcp` runs from your shell with the same env. The
  adapter is Unix-only; on native Windows it refuses to start and
  exits with code 64.
- **`daemon ipc error [Internal]: request timed out`.** The daemon is
  not running or `TC_SOCKET` does not match the daemon's socket path.
  Re-check `$TC_DATA/terminal-commanderd.sock` exists.
- **No tools listed.** Codex caches tool catalogues per server name;
  rename `terminal_commander` to invalidate the cache, or restart
  Codex.

## Smoke evidence requirements

Per the TC46 acceptance criteria, the Codex provider-harness smoke is
considered "live" only if a Codex session actually invokes one of the
Terminal Commander tools and the response is observed in the session
transcript. If your environment lacks a usable Codex CLI / auth /
config, mark the provider smoke as `Not Run` or `Blocked` in your
report and cite the exact reason.

A local daemon + MCP stdio smoke (without Codex in the loop) is
available via `scripts/smoke/verify-runtime-smoke.sh`; it is
secondary evidence, not provider-harness success.
