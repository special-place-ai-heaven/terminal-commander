# Terminal Commander - Architecture

Status: Baseline (TC01 wave 0 deliverable).
Scope: structural design, not implementation. Implementation details land in
TC04 through TC32 mini-specs.
Language: ASCII only.

This document is the architect's contract for the MVP. It locks the process
model, the component boundaries, the data flow, the platform notes, the
lifecycle, the storage layout, the policy posture, and the security
boundaries. Every claim that depends on external behavior cites a research
document under `docs/research/` or a README line range.

## 1. High-level diagram

```text
+----------------------------------------------------------------------+
|                       LLM / agent harness                            |
|        (Claude Code, Codex CLI, IDE agent, generic MCP client)       |
+--------------------------------|-------------------------------------+
                                 |  MCP stdio (JSON-RPC, 2025-11-25)
                                 v
+----------------------------------------------------------------------+
|   terminal-commander-mcp   (thin, per-session, unprivileged)         |
|   - rmcp 1.7.0 server, stdio transport                               |
|   - translates MCP tool calls to daemon IPC calls                    |
+--------------------------------|-------------------------------------+
                                 |  local IPC (transport: deferred to TC21)
                                 v
+----------------------------------------------------------------------+
|   terminal-commanderd       (persistent, per-user)                   |
|                                                                      |
|  +-----------+  +-----------+  +-----------+  +-----------------+    |
|  |  Probes   |  | Sifter    |  | Registry  |  | Bucket Manager  |    |
|  | (proc,    |->| Runtime   |->| (rules,   |  | (cursors,       |    |
|  |  file,    |  | (keyword, |  |  CRUD,    |  |  severity,      |    |
|  |  PTY,     |  |  regex,   |  |  FTS5)    |  |  bounded reads) |    |
|  |  dir)     |  |  dedupe,  |  +-----------+  +-----------------+    |
|  +-----------+  |  ...)     |        |               |               |
|       |         +-----------+        |               |               |
|       |              |               |               |               |
|       v              v               v               v               |
|  +------------+  +--------+   +--------------+  +--------------+     |
|  | Context    |  | Policy |   | Audit Log    |  | Store        |     |
|  | Spool/Ring |  | Engine |   | (append-only)|  | (rusqlite +  |     |
|  | (bounded)  |  | (advis.|   |              |  |  refinery +  |     |
|  +------------+  +--------+   +--------------+  |  FTS5, WAL)  |     |
|                                                 +--------------+     |
+----------------------------------------------------------------------+
                                 |
                                 v
                          Operator CLI
                  (terminal-commander-cli)
```

The diagram is a faithful elaboration of the README's intended
architecture at `README.md:213-239`.

## 2. Process model

The product runs as two processes per user.

### 2.1 `terminal-commander-mcp` (MCP server)

- Per-session lifetime. Spawned by the LLM harness as a stdio child
  per the MCP protocol (revision 2025-11-25).
- Stateless with respect to product data. The MCP server holds the
  rmcp `Server` handle, the daemon connection, and translation
  glue; it does not own probes, sifters, or storage.
- Unprivileged by construction. The README safety model
  (`README.md:289`) requires this; the MCP server must never run as
  root.
- One MCP server per harness session. Multiple sessions attaching to
  the same daemon is supported by design.

### 2.2 `terminal-commanderd` (daemon)

- Persistent, per-user lifetime. One daemon per user account per
  host. The "per-user" choice is recommended by
  `docs/research/mcp-transport-pattern.md` and remains open for
  TC26 final resolution.
- Owns all product state: probes, sifter runtime, registry, bucket
  manager, context spool, store, policy engine, audit log.
- Foreground process for the MVP. Optional systemd USER unit shipped
  alongside as a convenience per
  `docs/research/daemon-lifecycle.md`. The daemon is not a system
  unit.
- Privilege posture: unprivileged by default. A future privileged
  helper mode is documented in TC02 but is not the default.

### 2.3 Two-process is locked, IPC is deferred

The two-process split is user-provided per
`docs/research/_R2-delta-summary.md` finding F2, citing
`README.md:213-239` and `README.md:354-360`. Downstream goals must
not collapse to a single process without a user decision update.

The IPC transport between MCP server and daemon is deliberately
deferred to TC21 (daemon local API and router).
`docs/research/mcp-transport-pattern.md` recommends a local socket
via `interprocess` v2 (Unix domain socket on POSIX, named pipe on
Windows-native if/when added), with JSON-RPC 2.0 as the wire
protocol. TC21 takes that recommendation as input and locks the
final choice.

## 3. Components

Each subsection describes one component: responsibility, inputs,
outputs, ownership crate, and current source-status.

### 3.1 MCP server adapter (`terminal-commander-mcp`)

- Responsibility: expose the MCP tool surface (see SPEC section 8) to
  the LLM harness over stdio.
- Inputs: rmcp `Server` plus a daemon connection.
- Outputs: forwarded daemon responses, error envelopes, MCP
  heartbeats during `bucket_wait`.
- Crate: `terminal-commander-mcp`.
- Source-status at MVP land (post-TC23/TC24): `live` for the tool
  set; any tool whose backing daemon path returns
  `not_implemented` must surface that as an explicit MCP error, not
  a fabricated success.

### 3.2 Daemon (`terminal-commanderd`)

- Responsibility: hold the long-lived components in section 3.3 -
  3.10, route local API requests, manage shutdown.
- Inputs: IPC socket from the MCP server, optional CLI commands,
  optional SIGTERM/SIGINT.
- Outputs: IPC responses, audit log writes, event-store writes,
  bucket emissions.
- Crate: `terminal-commanderd`.

### 3.3 Probes

- Responsibility: observe a single source and produce frames.
  Probe types per SPEC section 9.
- Inputs: probe definition (e.g. command + args, path + watch
  options), policy decision allowing creation.
- Outputs: typed `Frame` stream into the sifter runtime; raw frames
  into the context spool.
- Crate: `terminal-commander-probes`.
- External deps: `pty-process` 0.5.3 (`async`), `notify` 8.2,
  `notify-debouncer-full` 0.7, `tokio` 1.x.
- Source-status: `live` (process, file, PTY), `partial` (directory,
  artifact - seeds in TC20), `deferred` (journal).

### 3.4 Sifter runtime

- Responsibility: evaluate active rules against frames; emit signal
  event drafts.
- Inputs: `Frame` stream from probes, active rule set from registry.
- Outputs: `SignalEventDraft` records to the bucket manager.
- Crate: `terminal-commander-sifters`.
- Sifter types per SPEC section 10. The runtime evaluates rules in
  documented precedence; correlation rules read from a sliding
  window over recently emitted drafts.

### 3.5 Registry

- Responsibility: persistent CRUD of rules. Search, get, create,
  test, activate. Mutable at runtime via MCP tools per SPEC
  section 8.5.
- Inputs: registry tool calls from MCP, rule pack seeds from TC14.
- Outputs: active rule set fed to the sifter runtime.
- Crate: `terminal-commander-store` (persistence) +
  `terminal-commander-core` (rule type definitions).
- Storage: SQLite tables in the project DB, including an FTS5 index
  over rule text fields. See section 7.

### 3.6 Bucket manager

- Responsibility: own bucket state, allocate monotonic cursors, apply
  severity filters, enforce bounded reads, attach summaries and
  dedupe metadata.
- Inputs: `SignalEventDraft` records from the sifter runtime.
- Outputs: persisted `SignalEvent` records via the store; cursor
  responses to `bucket_events_since` and `bucket_wait`.
- Crate: `terminal-commanderd` (lives inside the daemon binary;
  type definitions in `terminal-commander-core`).

### 3.7 Context ring (context spool)

- Responsibility: maintain a bounded ring buffer of frames per
  probe so `event_context(event_id, before, after)` can return a
  small window without holding the whole stream.
- Inputs: raw frames from probes (parallel to the sifter feed).
- Outputs: bounded windows by source pointer.
- Crate: `terminal-commander-core` (ring data type),
  `terminal-commanderd` (per-probe instances).
- Size bound and eviction policy: documented in TC08.

### 3.8 Store

- Responsibility: durable storage of events, bucket cursors,
  registry, audit log; FTS5 search lane.
- Inputs: writes from bucket manager (events), registry (rules),
  audit log (decisions); reads from MCP tools.
- Outputs: query results, cursor pages, audit-log scans.
- Crate: `terminal-commander-store`.
- Backend: rusqlite 0.39 `bundled` (FTS5 included) + refinery 0.9 +
  WAL. See `docs/research/sqlite-fts5.md`.

### 3.9 Policy engine

- Responsibility: gate command execution, file access, registry
  edits, and probe creation based on the active policy profile.
  Advisory in MVP (in-process checks; not kernel-enforced).
- Inputs: active profile definition, requested operation context.
- Outputs: allow/deny decisions plus a decision record for the audit
  log.
- Crate: `terminal-commanderd` (engine), `terminal-commander-core`
  (decision types).
- See `docs/research/policy-prior-art.md`; the recommended framing
  is "advisory enforcement in the daemon; kernel enforcement is a
  documented future hardening goal."

### 3.10 Audit log

- Responsibility: append-only durable record of every policy
  decision, every command execution, every registry mutation, and
  every privileged operation.
- Inputs: emission from policy engine, command-execution paths,
  registry CRUD, probe creation, file access decisions.
- Outputs: stored audit rows; readable via CLI (`audit` subcommand)
  and via MCP only through policy-gated paths.
- Crate: `terminal-commander-store` (persistence) +
  `terminal-commanderd` (emitter).

## 4. Data flow

### 4.1 Source-to-event flow

```text
source ---> probe ---> Frame ---> sifter runtime ---> SignalEventDraft
                          \                              |
                           +--> context ring (raw)       v
                                                    bucket manager
                                                         |
                                                         v
                                                       store
                                                         |
                                                         v
                                              bucket cursor stream
```

Invariants on this path:

- Frames are not exposed to the LLM. Only signal events and
  bounded context windows are.
- Every signal event carries a source pointer or sets
  `pointer.context_available = false` explicitly. No fabricated
  pointers.
- Backpressure stops at the bucket manager: if the store cannot
  keep up, the bucket manager applies bounded buffering and emits
  a `bucket_backpressure` system event rather than dropping
  silently. Detailed handling in TC11 / TC28.

### 4.2 LLM-to-daemon flow

```text
LLM ---> MCP tool call (stdio) ---> terminal-commander-mcp
                                            |
                                            v
                                    daemon local API (IPC, TC21)
                                            |
                                            v
                                    terminal-commanderd
                                            |
                                            v
                       probes / registry / bucket manager / policy
                                            |
                                            v
                                     IPC response
                                            |
                                            v
                                  MCP response (JSON-RPC)
                                            |
                                            v
                                          LLM
```

Invariants on this path:

- Every operation that can read sensitive paths, execute commands,
  or mutate the registry is policy-gated.
- `bucket_wait` MUST return a heartbeat rather than a raw output
  dump when no relevant signal appears, per `README.md:281`.
- `event_context` responses are bounded by max_lines and max_bytes.

## 5. Platform notes

### 5.1 Linux native

Full feature set. inotify-backed file probes via `notify` 8.2.
Process probes via `pty-process` 0.5.3 (`async`) and tokio child
processes. SQLite WAL on local disk. Optional systemd USER unit
for the daemon.

### 5.2 WSL2 native filesystem

WSL2's native ext4 (`/home/...`) behaves as standard Linux. File
probes use the same inotify path as bare Linux. Per
`docs/research/wsl-boundary.md` section 2.1.

### 5.3 WSL2 `/mnt/c` (9P)

inotify is **silently non-functional** on `/mnt/c` paths.
`inotify_add_watch` succeeds, no events are delivered, the daemon
appears live but probes never fire. Microsoft tracks this as
`microsoft/WSL#4739`; the issue is open since 2019.

The daemon MUST detect 9P mounts at probe-construction time via
`/proc/self/mountinfo` and force `PollWatcher` (notify's polling
backend) for those paths. The detection is a one-time check at
probe creation, not a runtime back-off after observed silence.
Per `docs/research/wsl-boundary.md` sections 2.2 and 2.3.

TC18 takes this as a hard acceptance criterion per
`docs/research/_R1-beta-summary.md`.

### 5.4 macOS / Windows-native

Deferred per `docs/research/_USER_DECISIONS.md`. Goal files in this
chain must not introduce macOS-only or Windows-native code paths
unless an explicit goal scopes them. The codebase should keep PTY
selection (`pty-process` vs `portable-pty`) behind a feature flag
so a Windows port becomes a goal, not a rewrite.

## 6. Lifecycle

### 6.1 Daemon lifecycle

- Primary mode: foreground supervised process. Logs to stderr or to
  a configured file. Shuts down on SIGTERM/SIGINT.
- PID file: `${XDG_RUNTIME_DIR}/terminal-commander/daemon.pid` if
  set, else `${HOME}/.local/state/terminal-commander/daemon.pid`.
  Atomic write via temp-file + rename.
- Liveness check on start: refuses to start if PID file points at a
  live process running the same binary.
- Optional systemd USER unit shipped under `dist/` or equivalent.
  Per `docs/research/daemon-lifecycle.md`. **Not a system unit.**
- Does not assume systemd is available. WSL2 systemd is opt-in
  per `docs/research/daemon-lifecycle.md` and must be detected
  rather than assumed.

### 6.2 MCP server lifecycle

- Spawned per session by the LLM harness as a stdio child.
- Connects to the daemon at start; refuses to operate if the daemon
  is not reachable (returns explicit `daemon_unavailable` MCP error).
- Exits when the harness closes stdin.

### 6.3 CLI lifecycle

- Short-lived process per invocation. Connects to the daemon, runs
  one operation, exits. The CLI must not bypass policy.

## 7. Storage layout

One SQLite database per daemon, opened with WAL mode and configured
via refinery migrations. Per `docs/research/sqlite-fts5.md`.

Logical schemas (final DDL lives in TC12, TC13, TC22):

- `events` - one row per signal event. Indexed by `(bucket_id, seq)`
  for cursor reads. FTS5 mirror on `summary` and `captures_text`
  for the search lane.
- `buckets` - one row per bucket: id, label, created_at,
  binding metadata, retention policy.
- `registry_rules` - one row per rule version: id, name, kind,
  pattern, capture map, summary template, severity, active flag,
  metadata. FTS5 mirror on rule text fields.
- `audit_log` - append-only decisions, command executions, registry
  edits, probe creations. Strictly chronological insert order.

Context spool path strategy: the context ring is in-memory per
probe (size and eviction in TC08). Persistent spill is deferred;
if introduced later, it spills to a per-probe file under
`${state_dir}/spool/<probe_id>/` and is policy-gated.

Database path defaults:

- Linux: `${XDG_DATA_HOME}/terminal-commander/data.db` if set, else
  `${HOME}/.local/share/terminal-commander/data.db`.
- WSL2: same as Linux. Database lives inside the WSL filesystem,
  never on `/mnt/c`.

Backup, retention, and rotation policies are defined in TC22.

## 8. Policy posture

Per `docs/research/_USER_DECISIONS.md` and
`docs/research/policy-prior-art.md`:

- MVP enforcement is **advisory in-process**: the daemon checks the
  active policy profile in code before performing the operation,
  emits a decision record to the audit log, and proceeds or denies.
- The recommended framing for documentation is exactly the one
  quoted in `docs/research/_R1-beta-summary.md`:

  > advisory enforcement in the daemon; kernel enforcement is a
  > documented future hardening goal.

- Policy profiles (locked by TC02): `developer_local`, `repo_only`,
  `read_only_observer`, `admin_debug`.
- Default-deny paths: private keys, password files, credential
  stores, token caches. Per `README.md:294-297`.
- The MCP server itself does NOT enforce policy. The MCP server
  forwards. The daemon decides. This preserves the
  privilege-separation invariant.

Post-MVP hardening roadmap (not implemented in TC01-TC32 unless
later goals add them):

- Landlock LSM for filesystem self-sandbox. Available on WSL2
  since kernel 5.15.57.1 per
  `docs/research/_R1-beta-summary.md`.
- seccomp-bpf via `seccompiler` for syscall narrowing.
- Optional sqlcipher feature for encryption at rest.

## 9. Security boundaries

- **MCP server**: unprivileged. Must not run as root. Must not hold
  file system handles beyond what `rmcp` and the daemon IPC require.
  Per `README.md:289`.
- **Daemon**: unprivileged by default. May be configured to run
  with elevated privileges only when a future explicit goal scopes
  privileged operations. Per `README.md:290`.
- **Default-deny**: sensitive paths (`README.md:294-297`) are denied
  by default in every policy profile; explicit allow rules are
  required.
- **No unbounded raw output**: invariant from TC01's mini-spec and
  from `README.md:8-12`. Frames are not first-class outputs of any
  MCP tool. The only path from raw stream to LLM is via
  `event_context`, which is bounded.
- **Audit-log integrity**: append-only. Any operation that
  could subvert the audit log (truncate, replace path) is
  policy-gated and itself audited. Final mechanism in TC22.
- **Registry safety**: rule creation through MCP is policy-gated.
  Regex rules must pass a complexity / time-budget check before
  activation per TC09 / TC29. No silent compilation of
  attacker-supplied regex.

## 10. Open architectural decisions

Tracked here so downstream goals can find them. Each entry names
the deciding goal.

| Decision | Owner | Default behavior until decided |
|---|---|---|
| IPC transport between MCP server and daemon | TC21 | Plan-of-record: `interprocess` v2 local socket + JSON-RPC 2.0 (per `docs/research/mcp-transport-pattern.md`). Not locked. |
| Daemonize flag (`--daemonize` via `fork` crate) | TC25 or skip | Skip. Foreground only is acceptable for MVP per `docs/research/daemon-lifecycle.md`. |
| Per-user vs per-machine daemon | TC26 | Per-user. |
| Encryption at rest (sqlcipher) | post-MVP | Off. |
| Kernel-enforced policy (Landlock / seccomp) | post-MVP | Advisory only. |
| macOS / Windows-native port | post-MVP | Not built. PTY abstraction kept feature-flagged. |

## 11. Runtime contract anchor (TC34)

The `terminal-commander-runtime` chain (TC33-TC48) lands the live
runtime on top of this architecture. The normative product contract
and tool-surface lock for that chain are:

- `docs/runtime/REALTIME_SIGNAL_CHANNEL.md` — product semantics.
- `docs/mcp/TOOL_CONTROL_SURFACE.md` — locked MCP tool list.

The TC33 reality audit (`docs/audits/runtime-gap-audit.md`,
`runtime-source-map.md`, `runtime-tool-surface-gap.md`) records the
gap between this architecture and `main` as of commit `a667010`.
Notable scaffold-only or deferred surfaces:

- `terminal-commanderd` binary entry point (`crates/daemon/src/main.rs`).
- `terminal-commander-mcp` binary entry point (`crates/mcp/src/main.rs`).
- rmcp 1.7.0 stdio adapter (deferred — lands in TC40).
- Daemon UDS / named-pipe IPC (deferred — lands in TC37).
- Persistent audit log (`AuditPlaceholder` in memory — replaced by
  TC35 / V0003 migration).
- POSIX PTY spawn (`crates/probes/src/pty.rs` is normalizer-only —
  spawn lands in TC44).

The runtime chain brings these to live status without changing the
process topology, privilege boundary, or invariants set out in
sections 1-10.
