# Codex CLI integration

Connect Codex CLI to Terminal Commander through MCP stdio.

Install and write the MCP config:

```powershell
npm install -g terminal-commander@latest
terminal-commander setup harness --provider codex-cli
```

npm install outside CI attempts setup; the explicit command configures or
repairs the server block and protocol marker in `~/.codex/config.toml`. It leaves
Codex's optional MCP 2026-07-28 feature flag to you.

## MCP 2026-07-28 opt-in

The adapter accepts Codex's default **2025-06-18** handshake and prefers
**2026-07-28**. See
[MCP protocol floor](README.md#mcp-protocol-floor).

Codex CLI's legacy default (no opt-in) still opens `initialize` with
**2025-06-18**, which the adapter now accepts. Dogfood on **0.157.1**
speaks Discover when the opt-in is on. Prefer that path when available.

For the modern path, set both of these:

1. In `~/.codex/config.toml`, enable the feature:

```toml
[features]
mcp_2026_07_28 = true
```

2. Setup writes the protocol marker in the Terminal Commander server env block:

```toml
[mcp_servers.terminal_commander.env]
CODEX_MCP_PROTOCOL_VERSION = "2026-07-28"
```

`terminal-commander setup harness --provider codex-cli` writes
`[mcp_servers.terminal_commander]` and its env table with `TC_SESSION`,
`CODEX_MCP_PROTOCOL_VERSION`, and requested optional settings. A force
refresh writes the marker again. `[features]` sits outside the server
table, so setup leaves your feature choice alone.

Codex warns that the under-development feature `mcp_2026_07_28` is enabled.
`suppress_unstable_features_warning = true` in the same file hides that
warning.

One-shot probes can enable the feature on the command line. The env marker
on the server block is still required:

```text
codex exec --enable mcp_2026_07_28 ...
```

### Version pin

| Item | Value |
| --- | --- |
| Package | `@openai/codex` |
| Floor that can speak Discover | `>= 0.147.0`, with the opt-in above |
| Dogfood proof | **0.157.1** (`npm i -g @openai/codex@0.157.1`) |

## Config Shape

Codex CLI reads MCP servers from `~/.codex/config.toml`. The feature flag,
the server block, and the protocol env belong together:

```toml
[features]
mcp_2026_07_28 = true

[mcp_servers.terminal_commander]
command = "terminal-commander-mcp"
args = []

[mcp_servers.terminal_commander.env]
CODEX_MCP_PROTOCOL_VERSION = "2026-07-28"
```

Add `TC_SOCKET = "/path/to/terminal-commanderd.sock"` in that same env table
only when you intentionally use a non-default daemon endpoint. On Windows,
the default endpoint is a local named pipe and normally does not need
`TC_SOCKET`. On Unix, the default endpoint is a Unix domain socket.

## AV-Safe Direct-Exe Launch

On `setup` (and `update` / `restart`), Terminal Commander copies the native exe
into a stable per-user directory and writes `command` as that exe path with
`args = []` instead of the bare `terminal-commander-mcp` name:

```toml
[features]
mcp_2026_07_28 = true

[mcp_servers.terminal_commander]
command = "C:\\Users\\<you>\\AppData\\Local\\terminal-commander\\bin\\terminal-commander-mcp.exe"
args = []

[mcp_servers.terminal_commander.env]
CODEX_MCP_PROTOCOL_VERSION = "2026-07-28"
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

`allow_shell` is on in the default `full_access` profile (TC inherits the
harness's trust); a hardened config sets `[policy.caps] allow_shell = false`. If
`system_discover` shows `shell_exec` `available: false`, or a shell call
is PolicyDenied, follow `recover_hint` `retry_with_argv` (argv
`run_and_watch` / `command_start_combed`) first; the deny names the
profile and the config key that changes it.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Handshake fails with `-32022` Unsupported protocol version (`requested` `2025-06-18`) | Legacy default. Set `[features] mcp_2026_07_28 = true` and `CODEX_MCP_PROTOCOL_VERSION = "2026-07-28"`. Confirm Codex is `>= 0.147.0` (dogfood pin `0.157.1`). |
| Codex reports the MCP server failed to start | Confirm `terminal-commander-mcp --help` works from the same user account. |
| No tools listed | Restart Codex CLI or rename the server key to refresh the catalogue. |
| Daemon unavailable | Run `terminal-commander doctor daemon`; the MCP adapter normally attempts daemon auto-start on connect. |
| Non-default endpoint | Set `TC_SOCKET` explicitly in the MCP env block. |

## Smoke Evidence

A provider smoke is live only when a Codex CLI session invokes one Terminal
Commander tool and the bounded response is visible in the session transcript.
The local runtime smoke script proves Terminal Commander works without Codex in
the loop, but it is not a provider-harness smoke by itself.
