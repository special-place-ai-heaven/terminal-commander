# MCP Tool Surface - Terminal Commander

Status: TC23 baseline.

The `terminal-commander-mcp` crate exposes the LLM-facing tool
surface. Per `docs/security/PRIVILEGE_MODEL.md`, the MCP server
contains NO command spawn, NO file open outside its own config, and
NO network listener. Every tool call is forwarded to the daemon
`Router` via in-process method dispatch.

## 1. MVP tool set (TC23)

| Tool | Status | Description |
|---|---|---|
| `system_discover` | live | Reports version, MCP spec revision, active profile, tool list. |
| `bucket_events_since` | live | Cursor-based bucket read. Bounded by `limit`. |
| `bucket_wait` | live | Realtime waiter; heartbeat-on-timeout per TC17 contract. |
| `bucket_summary` | live | Per-bucket counters (events, dropped, by severity / kind). |
| `event_context` | live | Bounded context window around a source pointer. |

Registry, probe, file, and command tools land in TC24 / TC25 / TC27.

## 2. Transport

MVP exposes the tool surface as a Rust API (`ToolSurface`). The rmcp
1.7.0 stdio adapter wrapper is deferred to a follow-up goal. Tests
exercise the surface directly so policy denials and bounded outputs
are verified before transport wiring.

## 3. Policy

Every tool call evaluates a `PolicyAction` via the daemon
`PolicyEngine`. Policy denials are ADVISORY (daemon-side; not kernel-
enforced) per TC22 framing. The denial surface is `McpError::PolicyDenied`.

## 4. Bounded outputs

All MCP responses are STRUCTURED types from `terminal_commander_core`.
There is no raw-stream lane:

- `BucketReadResponse.events: Vec<SignalEvent>` (no raw String).
- `ContextWindowResponse.frames: Vec<ContextLine>` (bounded by
  `before`/`after`/`max_bytes` and the hard `MAX_WINDOW_BYTES` cap).

## 5. Current limitations

- No real rmcp stdio adapter yet.
- Registry / probe / file / command_start_combed tools are TC24+.
- No multi-actor authorization (one MCP client per daemon).
- Audit emission is the TC21 placeholder; TC22 introduced the
  policy engine but the persistent audit log writes are deferred.

## 6. Source-status

| Component | Status |
|---|---|
| ToolSurface struct + 5 MVP tools | live (TC23) |
| Policy gate on every call | live (TC23) |
| rmcp 1.7.0 stdio adapter | reserved (post-MVP) |
| Registry / probe / file / command tools | reserved (TC24+) |
