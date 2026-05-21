# Runtime Tool Surface Gap — TC33

Goal: `TC33-code-reality-audit-and-runtime-pivot.md`
Branch: `main`
Audited commit: `a667010f43807dd3da53bfdc11b89ecc3b7b7825`
Purpose: map every intended MCP tool from `README.md` to its actual
implementation status, and bind each P0 backlog item to a specific
goal in the `terminal-commander-runtime` chain.

## Intended MCP tool surface (from `README.md`)

The README enumerates the tool candidates. Below: each tool, its
expected behavior, and its status on `main`.

| Tool | Intended behavior | Status | Where implemented | Notes |
|---|---|---|---|---|
| `system_discover` | Advertise version, MCP spec, policy profile, tool list. | live (in-process) | `ToolSurface::system_discover` (`crates/mcp/src/lib.rs`) | Spec `2025-11-25`. Tool list incomplete: omits `file_read_window` and the `registry_*` tools that ARE implemented. |
| `policy_status` | Report active policy profile + counters. | not implemented | n/a | CLI `policy` subcommand prints a static placeholder. |
| `command_start_combed` | Start a process, attach probes, bind rules, return bucket id. | not implemented | n/a | `ProcessProbe::spawn` exists; no MCP tool wires it. |
| `command_status` | Status of a running job. | not implemented in MCP | partial: `Router::job_get` exists | Not on `ToolSurface`. |
| `command_write_stdin` | Write to a job's stdin. | not implemented | n/a | Process probe owns child but does not expose stdin handle to MCP. |
| `command_send_signal` | Send a signal to a job. | not implemented | n/a | Process probe has `start_kill`; no MCP entry. |
| `bucket_create` | Create a bucket. | not implemented in MCP | partial: `Router::bucket_create` exists | Not on `ToolSurface`. |
| `bucket_events_since` | Cursor-based read. | live (in-process) | `ToolSurface::bucket_events_since` | Policy `BucketRead`. |
| `bucket_wait` | Realtime wait with cursor + severity_min + timeout. | live (in-process) | `ToolSurface::bucket_wait` | Backed by `tokio::sync::Notify`. Returns heartbeat on timeout. |
| `event_context` | Bounded window around an event by id. | live (in-process) | `ToolSurface::event_context` | Uses ProbeId + FrameId anchor + before/after frames + max_bytes cap. README example references `event_id`; current impl keys on `(probe_id, frame_id)` because the anchor is the frame, not the event. This is consistent with the SignalEvent pointer schema but worth noting for tool ergonomics. |
| `probe_create` | Create a probe of a given type. | not implemented in MCP | partial: `ProcessProbe`, `FileProbe`, `DirectoryProbe` exist | Not on `ToolSurface`. |
| `probe_bind_rules` | Bind registry rules to a probe. | not implemented | n/a | `SifterRuntime::build` is one-shot at construction; no dynamic rebind path. |
| `registry_search` | FTS search on rules. | live (in-process) | `ToolSurface::registry_search` | Policy `BucketRead` (note: README would suggest a registry-scoped action — current code reuses BucketRead). |
| `registry_get` | Get a rule by id. | live (in-process) | `ToolSurface::registry_get` | Returns latest version. |
| `registry_create` | Create a new rule version. | live (in-process) | `ToolSurface::registry_create` | Policy `RegistryCreate`. |
| `registry_test` | Test a rule against sample input. | not implemented | n/a | Required for safe activation; missing. |
| `registry_activate` | Activate a rule version. | live (registry-side) / not hot-binding | `ToolSurface::registry_activate` | Records activation in DB. Does NOT rebind running probes. |
| `file_read_window` | Bounded read by `offset`/`max_bytes`. | live (in-process) | `ToolSurface::file_read_window` | Cap `64 KiB`. Policy default-deny path list applied first. |
| `file_search` | Targeted search across a file or dir. | not implemented | n/a | |
| `file_watch` | Watch a path for changes. | not implemented in MCP | partial: `FileProbe`, `DirectoryProbe` exist | Not on `ToolSurface`. |

Live in MCP today (transport aside): **8 tools** out of the 20+
intended. None are reachable from a real MCP client because no rmcp
stdio adapter is bound to a binary.

## Severity-by-tool of the gap

- **Critical (blocks the product story):** rmcp stdio adapter,
  `command_start_combed`, daemon UDS IPC, persistent audit log,
  `bucket_wait` over real MCP, `event_context` over real MCP.
- **High (breaks named tool surface):** `policy_status`,
  `bucket_create` MCP entry, `probe_create`, `probe_bind_rules`,
  `registry_test`, `file_search`, `file_watch`, hot rule activation,
  `command_status`/`command_write_stdin`/`command_send_signal`.
- **Medium (ergonomics or polish):** `system_discover` tool list
  honesty, audit hash chain, native inotify, native PTY spawn,
  process-wrap process-group cancellation.

## P0 → goal map

Source: `BACKLOG.md` P0 list. Each row maps to exactly one goal in
`terminal-commander-runtime`. No P0 is being deferred.

| P0 item | Goal | Notes |
|---|---|---|
| rmcp 1.7.0 stdio adapter | **TC40** — `TC40-rmcp-stdio-adapter-and-tool-discovery.md` | Brings real MCP transport to the MCP binary. |
| pty-process spawn path | **TC44** — `TC44-posix-pty-spawn-and-stdin-control.md` | Adds POSIX/WSL PTY spawn + stdin steering on top of existing normalizer. |
| daemon IPC transport (UDS) | **TC37** — `TC37-daemon-uds-ipc-and-peer-identity.md` | UDS server + peer-cred check. |
| Persistent audit log writes | **TC35** — `TC35-persistent-audit-log-v0003.md` | V0003 migration + audit writer replacing AuditPlaceholder. |

## P0-adjacent gaps → goal map

These are not in the current P0 list but are required for a real
realtime signal channel. They are already pointed at by goals in
the new chain:

| Adjacent gap | Goal |
|---|---|
| `command_start_combed` MCP runtime wiring | TC38 |
| `bucket_wait` + `event_context` reachable from daemon (server side) | TC39 |
| Real MCP command + bucket tool set | TC41 |
| Hot rule activation rebind in live probes | TC42 |
| File `search` / `watch` / bounded read MCP tools | TC43 |
| Parallel probe router + multi-bucket bindings | TC45 |
| Provider-neutral smoke harness | TC46 |
| Load / noise / backpressure gate | TC47 |
| Beta-gate evidence review | TC48 |

## Tools to add to `system_discover` once they exist

When the runtime chain lands, `system_discover.tools[]` must reflect
the real callable set. The advertised list today (`system_discover`,
`bucket_events_since`, `bucket_wait`, `bucket_summary`,
`event_context`) is incomplete and will drift further unless TC40
ties this list directly to the actual rmcp tool dispatcher.

## Implementation invariants to preserve in every later goal

These invariants are live on `main` and must not regress when the
real runtime lands:

- Bounded outputs: `MAX_FILE_WINDOW_BYTES = 64KiB`,
  `MAX_READ_LIMIT = 10_000`, context window byte cap.
- Pointer-or-reason for severity >= Medium.
- No `Command::spawn` in the `mcp` crate (TC29 structural test).
- No TCP listener in the `mcp` crate (TC29 structural test).
- Sudo / doas / su / pkexec / kexec / polkit-agent /
  polkit-auth-agent-1 denied across every profile.
- 14 default-deny path suffixes (TC29 test covers all of them).
- Closed-set enums: `Severity`, `PolicyDecision`, `PolicyProfile`,
  `PolicyAction`, `RuleType`, `RuleStatus`, `SourceStream`,
  `SourceType`.

## Tool surface "honesty" checklist for TC34

TC34 is the contract goal. When it locks the realtime signal channel
contract it should answer:

1. Does `event_context` key on `event_id` (per README example) or on
   `(probe_id, frame_id)` (current impl)? Pick one and document.
2. Should `command_start_combed` return a bucket id, a job id, both,
   or a structured `command_started` event in a bucket? Decide.
3. Is `bucket_create` an explicit tool or implicit on
   `command_start_combed`?
4. Should `policy_status` exist as a tool, or only as a section of
   `system_discover`?
5. Are `command_write_stdin` and `command_send_signal` allowed under
   `developer_local`, or only under `admin_debug`?

These are contract-level questions, not implementation. TC33 does
not answer them.
