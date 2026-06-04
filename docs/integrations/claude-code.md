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

## AV-Safe Direct-Exe Launch

On `setup` (and `update` / `restart`), Terminal Commander copies the native exe
into a stable per-user directory and writes `command` as that exe path with
`args: []` instead of the bare `terminal-commander-mcp` name:

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

This removes the npm-shim -> node -> JS-shim launch chain that heuristic
antivirus reads as a loader. It is user-space and no-admin; if the copy cannot
complete it falls back to the bare-name command. See
[`cursor.md`](cursor.md#av-safe-direct-exe-launch) for the full rationale and
the opt-in logon-task option.

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

## Subscription Notifications (best-effort, default OFF)

Terminal Commander advertises the MCP `logging` capability, so the
`subscription_pull` tool can emit a `notifications/message` nudge when a pull
returns a non-empty batch. This is OFF by default and BEST-EFFORT only:

- Opt in by launching the MCP server with `TC_MCP_NOTIFY=1` in its env block.
  With the flag unset, no notification is ever sent.
- The nudge fires ONLY on a non-empty pull, NEVER on an idle/empty pull. It
  carries a small summary (`count`, `max_severity`, `lagged`).
- It is NEVER load-bearing. The authoritative delivery of events is always the
  pull itself (`subscription_pull`, or `subscription-stream` under a `Monitor`).
  Treat the notification as a hint, not a guarantee.

Why best-effort: Claude Code currently DROPS server-originated notifications to
idle sessions (claude-code issue #36665, closed "not planned"; issue #61797, a
drop-bug). So even with the flag on, a notification may never surface in an idle
session. Harnesses that do surface server notifications can use it as a wake
hint; everything else simply ignores it. The send rides the already-open stdio
pipe in-process (no spawned process, file, or socket).

## Troubleshooting

| Symptom | Check |
| --- | --- |
| `/mcp` shows no Terminal Commander server | Confirm Claude Code loaded the config and restart the session. |
| MCP server failed to start | Confirm `terminal-commander-mcp --help` works from the same user account. |
| Daemon unavailable | Run `terminal-commander doctor daemon`; the MCP adapter normally attempts daemon auto-start on connect. |
| Non-default endpoint | Set `TC_SOCKET` explicitly in the MCP env block. |

## Real-Time-Active patterns

How to make Claude Code react to subscription events as they happen. All four
patterns sit on top of the same authoritative delivery primitive
(`subscription_pull`); pick by cadence and how persistent the watch is.

- Primary - Monitor: run
  `Monitor("terminal-commander subscription-stream <sub_id>")`. The
  `subscription-stream` CLI bridge emits one NDJSON object per matched event to
  stdout (flushed per event); the harness wakes one model turn per line. It
  exits 0 on `--max`/close and NON-ZERO on an unknown sub_id or daemon
  shutdown, so the `Monitor` terminates instead of silently idling. Best for
  persistent, session-length watches.
- One-shot - backgrounded pull: a single blocking `subscription_pull` (or one
  backgrounded `subscription-stream ... --max 1`) that returns when the awaited
  event arrives. Best for "tell me when X completes/activates".
- Cadence - `/loop` / `ScheduleWakeup` / `CronCreate`: re-invoke a
  `subscription_pull` on an interval. Best for periodic checks where you do not
  need event-driven latency.
- Optional hack - Stop-hook keep-alive (default OFF): a `Stop` hook that blocks
  the stop and injects pending events while a subscription is still active,
  bounded to N=3 consecutive keep-alives. It is the ONLY pattern that can wedge
  a session, so it is default OFF and hard-bounded. See
  [`packages/terminal-commander/hooks/`](../../packages/terminal-commander/hooks/).

Cross-harness note: the universal pattern is a background loop over
`subscription_pull`. Codex CLI / Cursor (see [`codex-cli.md`](codex-cli.md),
[`cursor.md`](cursor.md)) use their own background loop over `subscription_pull`
rather than a `Monitor`; the `subscription-stream` NDJSON bridge and the
Stop-hook are Claude-Code-specific conveniences over that same primitive.

## Smoke Evidence

A provider smoke is live only when a Claude Code session invokes one Terminal
Commander tool and the bounded response is visible in the session transcript.
The local runtime smoke script proves Terminal Commander works without Claude
Code in the loop, but it is not a provider-harness smoke by itself.
