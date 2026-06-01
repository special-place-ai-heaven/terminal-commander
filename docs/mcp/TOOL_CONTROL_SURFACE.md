# MCP Tool Control Surface - Locked Contract

Status: current MCP-facing contract as of 2026-05-25.
Anchored by: `crates/mcp/src/tools.rs`, `docs/runtime/REALTIME_SIGNAL_CHANNEL.md`.
Language: ASCII only.

This document is the authoritative MCP tool contract for Terminal
Commander clients. A tool that ships without bounded outputs, honest
availability metadata, policy gating where applicable, and audit on
mutation is a contract violation.

## 1. Discovery and availability

`system_discover` is always callable. It returns adapter metadata, daemon
reachability, and the live tool catalogue:

```json
{
  "adapter_version": "0.1.10",
  "mcp_spec": "2025-11-25",
  "daemon_available": false,
  "daemon": null,
  "daemon_error": "daemon ipc error [...]: ...",
  "tools": [
    {
      "name": "system_discover",
      "status": "live",
      "requires_daemon": false,
      "available": true,
      "unavailable_reason": null
    },
    {
      "name": "health",
      "status": "live",
      "requires_daemon": true,
      "available": false,
      "unavailable_reason": "daemon_unavailable"
    }
  ]
}
```

Availability rules:

- `system_discover` does not require the daemon.
- Every other MCP tool requires the daemon.
- When the daemon is unavailable, daemon-backed tools report
  `available: false` and `unavailable_reason: "daemon_unavailable"`.
- Daemon-backed calls must return a structured `daemon_unavailable`
  error when startup status says the daemon is unavailable. They must
  not leak raw pipe/socket errors as the primary client contract.
- The advertised list and the registered rmcp router are tested to stay
  aligned (`catalogue_lists_twenty_nine_live_tools_at_tc45` and
  `tool_router_exposes_all_live_tools`).

Machine-readable fixture:
`tests/fixtures/contracts/mcp-tools/system_discover.v1.json`.

## 2. Live tool catalogue

The current rmcp stdio adapter exposes 32 live tools.

| Group | Tools |
|---|---|
| Discovery and health | `system_discover`, `health`, `policy_status`, `self_check` |
| Commands and buckets | `command_start_combed`, `run_and_watch`, `command_status`, `command_output_tail`, `bucket_events_since`, `bucket_wait`, `bucket_summary`, `event_context` |
| Rule registry | `registry_search`, `registry_get`, `registry_upsert`, `registry_test`, `registry_activate`, `registry_import_pack`, `registry_deactivate`, `registry_list_active` |
| Files | `file_read_window`, `file_search`, `file_watch_start`, `file_watch_stop`, `file_watch_list` |
| PTY | `pty_command_start`, `pty_command_write_stdin`, `pty_command_stop`, `pty_command_list` |
| Runtime | `runtime_state`, `probe_list`, `probe_status` |

Each catalogue entry returned by `system_discover.tools[]` includes:

- `name`
- `status`
- `description`
- `requires_daemon`
- `available`
- `unavailable_reason`

## 3. Tools not exposed

| Anti-tool | Why it must not exist |
|---|---|
| `command_read_stdout` | Would surface raw stream text. |
| `command_read_stderr` | Would surface raw stream text. |
| `file_read_all` | Unbounded file output. |
| `stream_tail` | Raw stream tail. |
| `shell_exec` | Generic shell bridge instead of argv-first controlled commands. |
| `network_listen` | No network listener is allowed in the MCP-facing crate. |
| `policy_override` | Policy decisions are not bypassable by clients. |

Any later goal that needs a capability shaped like one of these must
stop and propose a bounded alternative.

## 4. Bounded-output rules

Every response carries explicit limits or returns references/cursors
instead of raw streams.

| Surface | Required behavior |
|---|---|
| Command start | Returns ids and metadata, not stdout/stderr dumps. |
| Bucket reads | Cursor-based and capped by daemon/store limits. |
| Bucket wait | Returns events or a heartbeat, not an unbounded tail. |
| Event context | Returns a bounded window around a pointer. |
| File reads | Windowed by line/byte limits; no whole-file dump tool. |
| File search | Bounded matches and capped snippets. |
| Registry search/test | Bounded hit/sample counts. |
| Runtime/probe status | Bounded JSON snapshots. |

`bucket_wait` must return one of two response shapes:

```text
{ heartbeat: true,  events: [],         next_cursor: <input> }
{ heartbeat: false, events: [...non-empty], next_cursor: <max(seq)> }
```

A response with raw stream text in `events` is invalid. The forbidden
fixture `tests/fixtures/contracts/forbidden/raw-stream-as-events.v1.json`
is the structural test oracle.

## 5. Policy and audit

Tools that read or mutate daemon state must route through the daemon-side
policy/audit boundary. The MCP adapter is a transport and validation
layer; it must not become an alternate command executor, policy bypass,
or hidden shell bridge.

Known policy action families include command start/stdin/signal,
file read/watch, probe create, registry create/activate, bucket wait/read,
and event context. A new tool that needs a new policy action must add the
closed-set variant in the same goal that adds the tool.

## 6. Forbidden expansions

A future goal must stop and surface a blocker rather than:

- Add a tool that returns raw stream text.
- Add a tool that bypasses policy evaluation.
- Add a tool that opens a TCP listener.
- Add a tool that spawns commands from the MCP crate.
- Replace `bucket_wait` heartbeat with a partial raw dump.
- Replace `event_context` bounded windows with an unbounded mode.
- Expose command or PTY mutation without audit.

## 7. References

- `crates/mcp/src/tools.rs` - live rmcp tool registration and discovery.
- `docs/runtime/REALTIME_SIGNAL_CHANNEL.md` - product contract.
- `docs/mcp/README.md` - adapter overview.
- `docs/security/PRIVILEGE_MODEL.md` - privilege boundaries.
- `docs/contracts/README.md` - wire-shape fixtures.
