# MCP Tool Control Surface — Locked Contract

Status: Locked at TC34. Normative for TC35-TC48.
Anchored by: `docs/audits/runtime-tool-surface-gap.md`,
`docs/runtime/REALTIME_SIGNAL_CHANNEL.md`.
Language: ASCII only.

This document is the authoritative MCP tool list for Terminal
Commander beta. Every later runtime goal SHOULD bring at least one
row of this table from `deferred` or `not implemented` to `live`. A
tool that ships without bounded outputs, policy gating, and audit on
mutation is a contract violation.

## 1. Beta-required tools

These tools MUST be live (reachable from a real rmcp stdio client,
through real daemon IPC) before Terminal Commander can be called
beta. Each row binds to the goal that lands the live behavior.

| Tool | Intent | Bounded output? | Policy action | Owner goal | Live on `main` today? |
|---|---|---|---|---|---|
| `system_discover` | Advertise version, MCP spec, active profile, callable tools. | Yes (small JSON object). | None (read). | TC40 (transport), TC41 (advertised list refresh). | In-process only. Real MCP transport missing. |
| `policy_status` | Report active profile and counters. | Yes (small JSON object). | None (read). | TC41. | No. |
| `command_start_combed` | Start a policy-approved command, attach probes, bind rules, return references. | Yes (returns ids, not output). | `CommandStart { argv, cwd }`. | TC38. | No. |
| `command_status` | Status of a running job by job id. | Yes (counters + exit summary). | `BucketRead` (read-only). | TC41. | Library-side (`Router::job_get`) only. |
| `command_write_stdin` | Write a small byte slice to a job's stdin. | Yes (echoed slice id, not content dump). | `CommandStdin`. | TC44 (PTY chain). | No. |
| `command_send_signal` | Send a signal (SIGTERM, SIGKILL, SIGHUP, ...) to a job. | Yes (decision + audit). | `CommandSignal`. | TC44. | No. |
| `bucket_create` | Create a bucket. (Or implicit on `command_start_combed`; see open question Q3.) | Yes (returns bucket id). | `BucketRead` or new `BucketCreate` action. | TC41. | Library-side only. |
| `bucket_events_since` | Cursor-based bucket read with optional `severity_min`, `kind_filter`, `limit`. | Yes (`MAX_READ_LIMIT=10_000`; structured events; no raw stream). | `BucketRead`. | TC39 (daemon API), TC41 (MCP tool). | In-process only. |
| `bucket_wait` | Realtime wait; heartbeat on timeout. | Yes (`MAX_READ_LIMIT`; structured events or heartbeat). | `BucketWait`. | TC39, TC41. | In-process only. |
| `event_context` | Bounded window around an anchor frame. | Yes (frames bounded by `before/after/max_bytes`, hard cap `MAX_WINDOW_BYTES`). | `EventContext`. | TC39, TC41. | In-process only. |
| `probe_create` | Create a probe (process / file / directory / pty) bound to a bucket. | Yes (returns probe id; no stream surfaced). | `ProbeCreate { kind }`. | TC42 / TC43 / TC44 / TC45. | Library-side per probe only. |
| `probe_bind_rules` | Bind active rule ids to a probe. | Yes (returns binding summary). | `ProbeCreate` (rebinds existing probes). | TC42. | No. |
| `registry_search` | FTS search over rule registry. | Yes (`DEFAULT_SEARCH_LIMIT`, `MAX_SEARCH_LIMIT`). | `BucketRead` (read) or new `RegistryRead`; TBD with TC42. | TC41 / TC42. | In-process only. |
| `registry_get` | Get latest rule version by id. | Yes (one `RuleDefinition`). | Same as `registry_search`. | TC41 / TC42. | In-process only. |
| `registry_create` | Append a new rule version. | Yes (returns version number). | `RegistryCreate`. | TC41 / TC42. | In-process only. |
| `registry_test` | Dry-run a rule against sample input; never persists. | Yes (bounded matches + captures). | New `RegistryTest`; TC42 locks. | TC42. | No. |
| `registry_activate` | Activate a rule version; rebinds running probes. | Yes (activation record). | `RegistryActivate`. | TC42. | Library-side persistence only; no live rebind. |
| `file_read_window` | Bounded read of a file by `(offset, max_bytes)`. | Yes (`MAX_FILE_WINDOW_BYTES = 64 KiB`). | `FileRead { path }` (default-deny suffixes apply). | TC41 (MCP exposure); already live in-process. | In-process only. |
| `file_search` | Targeted search across a file or directory; bounded snippets. | Yes (bounded match count + bounded snippets). | `FileRead { path }`. | TC43. | No. |
| `file_watch` | Watch a path for changes; structured change events only. | Yes (structured events, no raw deltas). | `FileWatch { path }`. | TC43. | Library-side per probe only. |

## 2. Tools NOT exposed (out of scope at beta)

| Anti-tool | Why it must not exist |
|---|---|
| `command_read_stdout` | Would surface raw stream; contract violation. |
| `command_read_stderr` | Same. |
| `file_read_all` | Unbounded output; contract violation. |
| `stream_tail` | Raw stream tail; contract violation. |
| `shell_exec` | Unrestricted root shell; contract violation. |
| `network_listen` | No network listener allowed in the MCP-facing crate. |
| `policy_override` | Policy decisions are not bypassable. |

Any later goal that needs a capability shaped like one of these MUST
stop and propose an alternative bounded tool. Adding one of these
tools is a stop condition for the runtime chain.

## 3. Bounded-output rules per response

Every response carries explicit limits. The advertised tool list MUST
reflect this.

| Surface | Hard cap | Where it lives |
|---|---|---|
| Bucket read events count | `MAX_READ_LIMIT = 10_000` | `crates/store::MAX_READ_LIMIT` |
| File read window bytes | `MAX_FILE_WINDOW_BYTES = 64 KiB` | `crates/mcp::MAX_FILE_WINDOW_BYTES` |
| Context window bytes | `MAX_WINDOW_BYTES` (TBD in TC39 — currently a per-ring `max_bytes` cap on `ContextWindowRequest`). | `crates/core::context` |
| Frame size cap | sifter-side oversize-text capture | `crates/sifters` |
| Registry search hits | `DEFAULT_SEARCH_LIMIT`, clamped to `MAX_SEARCH_LIMIT` | `crates/store::registry` |
| Audit batch (post-TC35) | TBD | TC35 will lock |

## 4. Policy gate per tool (locked)

Every tool MUST evaluate a `PolicyAction` BEFORE doing work. The
current `PolicyAction` variants are:

```text
CommandStart { argv, cwd }
CommandStdin
CommandSignal
FileRead { path }
FileWatch { path }
ProbeCreate { kind }
RegistryCreate
RegistryActivate
BucketWait
BucketRead
EventContext
```

`registry_search` and `registry_get` reuse `BucketRead` today; TC42
will decide whether to introduce a dedicated `RegistryRead` action.
That decision is recorded in TC42, not here.

A new tool that needs a new `PolicyAction` MUST add the variant in
the goal that adds the tool; the closed-set enum is not amended in
isolation.

## 5. `system_discover` advertised tool list

`system_discover.tools[]` MUST be a function of the dispatcher's
registered handler set. A static hard-coded list is a defect: the
advertised list and the callable list MUST be identical.

TC40 owns this binding when the rmcp stdio adapter lands. On `main`
today the in-process `system_discover` advertises 5 tools but the
`ToolSurface` actually implements 8 (the registry / file tools are
not advertised). TC34 records this drift; TC40 fixes it.

## 6. Heartbeat contract on `bucket_wait`

`bucket_wait` MUST return one of two response shapes:

```text
{ heartbeat: true,  events: [],         next_cursor: <input> }
{ heartbeat: false, events: [...non-empty], next_cursor: <max(seq)> }
```

A response with `heartbeat = true` AND non-empty `events` is invalid.
A response with `heartbeat = false` AND empty `events` is invalid.
A response with raw stream text in `events` is invalid. The forbidden
fixture `tests/fixtures/contracts/forbidden/raw-stream-as-events.v1.json`
is the structural test oracle.

## 7. Source-status table at beta gate

The beta gate (TC48) checks every row below. A row that is not at the
status it claims fails the gate.

| Tool | Required status at beta gate | Notes |
|---|---|---|
| `system_discover` | live over real MCP transport | TC40 |
| `policy_status` | live | TC41 |
| `command_start_combed` | live with daemon-side execution | TC38 |
| `command_status` | live | TC41 |
| `command_write_stdin` | live for PTY-backed commands | TC44 |
| `command_send_signal` | live with audit | TC44 |
| `bucket_create` | live (explicit or implicit decided by TC41) | TC41 |
| `bucket_events_since` | live over real MCP | TC41 |
| `bucket_wait` | live with heartbeat | TC39, TC41 |
| `event_context` | live with bounded window | TC39, TC41 |
| `probe_create` | live for process / file / directory | TC42, TC43, TC45 |
| `probe_bind_rules` | live with hot rebind | TC42 |
| `registry_search` | live | TC41, TC42 |
| `registry_get` | live | TC41, TC42 |
| `registry_create` | live | TC41, TC42 |
| `registry_test` | live | TC42 |
| `registry_activate` | live with hot rebind | TC42 |
| `file_read_window` | live | TC41 |
| `file_search` | live | TC43 |
| `file_watch` | live | TC43 |
| PTY spawn (used by command_* over PTY) | live on POSIX/WSL | TC44 |

## 8. Unresolved contract questions (mirrored from REALTIME_SIGNAL_CHANNEL §13)

These are open and MUST be answered in the owning goal file when it
lands. TC34 does not answer them.

1. `event_context` anchor key: `event_id` (README example) or
   `(probe_id, frame_id)` (current code). Decide in TC39.
2. `command_start_combed` return shape: bucket id, job id, both, or
   a `command_started` event in a bucket. Decide in TC38.
3. `bucket_create`: explicit tool or implicit on
   `command_start_combed` / `probe_create`. Decide in TC41.
4. `policy_status`: standalone tool or section of `system_discover`.
   Decide in TC41.
5. `command_write_stdin` and `command_send_signal` profile gating:
   `developer_local` + `admin_debug`, or `admin_debug` only. Decide
   in TC44.

## 9. Forbidden expansions

A future goal MUST stop and surface a blocker rather than:

- Add a tool that returns raw stream text.
- Add a tool that bypasses `PolicyEngine::evaluate`.
- Add a tool that opens a TCP listener.
- Add a tool that spawns commands from the `mcp` crate.
- Replace `bucket_wait` heartbeat with a partial raw dump.
- Replace `event_context` bounded window with a "give me everything
  in the ring" mode.
- Expose `command_*` tools without an audit record on execute.

## 10. References

- `docs/runtime/REALTIME_SIGNAL_CHANNEL.md` — product contract.
- `docs/audits/runtime-tool-surface-gap.md` — current vs intended.
- `docs/mcp/README.md` — adapter-level overview.
- `docs/security/PRIVILEGE_MODEL.md` — privilege boundaries.
- `docs/contracts/README.md` — wire-shape fixtures (including
  `mcp-tools/*.v1.json`).
