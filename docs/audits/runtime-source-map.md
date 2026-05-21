# Runtime Source Map — TC33

Goal: `TC33-code-reality-audit-and-runtime-pivot.md`
Branch: `main`
Audited commit: `a667010f43807dd3da53bfdc11b89ecc3b7b7825`
Purpose: map every product concept named in `README.md` and
`docs/runtime/**` to the actual file / module / type / function on
`main`. This is a navigation map; the live-vs-deferred status comes
from `runtime-gap-audit.md`.

## Workspace layout

`Cargo.toml` virtual workspace, seven crates under `crates/`:

| Crate dir | Cargo package | Description |
|---|---|---|
| `crates/core` | `terminal-commander-core` | Domain types, identifiers, severity, event / source / pointer models. No I/O. |
| `crates/sifters` | `terminal-commander-sifters` | Sifter runtime: keyword, regex, condition, multiline, dedupe, suppression, stall, progress, prompt, correlation, artifact parsers. |
| `crates/probes` | `terminal-commander-probes` | Probe runners: process, file, PTY (normalizer only), directory. |
| `crates/store` | `terminal-commander-store` | rusqlite + manual migration runner; event store, bucket cursors, rule registry. FTS5 search lane. |
| `crates/daemon` | `terminal-commanderd` | Long-running daemon scaffolding. Owns Router, PolicyEngine, AuditPlaceholder. Binary scaffold-only. |
| `crates/mcp` | `terminal-commander-mcp` | Thin MCP server adapter. ToolSurface struct lands; rmcp stdio binary scaffold-only. |
| `crates/cli` | `terminal-commander-cli` | Operator CLI. Subcommand parsing live; daemon attach offline. |

## Concept → code map

### LLM-facing MCP tool surface

Product concept: provider-neutral interface the LLM calls.

| Concept | Module / type | File |
|---|---|---|
| Tool surface object | `ToolSurface` | `crates/mcp/src/lib.rs` |
| Tool list advertisement | `ToolSurface::system_discover` | same |
| Bucket read by cursor | `ToolSurface::bucket_events_since` | same |
| Realtime bucket wait | `ToolSurface::bucket_wait` (async) | same |
| Bucket summary | `ToolSurface::bucket_summary` | same |
| Event context window | `ToolSurface::event_context` | same |
| Bounded file read | `ToolSurface::file_read_window` (cap `MAX_FILE_WINDOW_BYTES = 64KiB`) | same |
| Registry search | `ToolSurface::registry_search` | same |
| Registry get latest | `ToolSurface::registry_get` | same |
| Registry create new version | `ToolSurface::registry_create` | same |
| Registry activate version | `ToolSurface::registry_activate` | same |
| Error union | `enum McpError` | same |
| Discover response shape | `SystemDiscoverResponse` | same |
| File window response shape | `FileReadWindowResponse` | same |
| MCP binary entry point | `fn main` (scaffold-only exit 64) | `crates/mcp/src/main.rs` |

### Daemon runtime

Product concept: the local privileged worker.

| Concept | Module / type | File |
|---|---|---|
| Daemon library entry | `terminal_commanderd` crate root | `crates/daemon/src/lib.rs` |
| Local API router | `struct Router` | `crates/daemon/src/router.rs` |
| Audit log seam | `struct AuditPlaceholder`, `struct AuditRecord` | same |
| Bucket router methods | `bucket_create`, `bucket_append`, `bucket_events_since`, `bucket_wait`, `bucket_summary` | same |
| Event context router method | `event_context` | same |
| Job lifecycle | `job_start`, `job_finish`, `job_cancel`, `job_get` | same |
| Policy engine | `struct PolicyEngine`, `enum PolicyDecision`, `enum PolicyProfile`, `enum PolicyAction` | `crates/daemon/src/policy.rs` |
| Command deny list | `pub const COMMANDS_DENY` | same |
| Default-deny path suffixes | `pub const DEFAULT_DENY_PATH_SUFFIXES` | same |
| Daemon binary entry point | `fn main` (scaffold-only exit 64) | `crates/daemon/src/main.rs` |

### Probes

Product concept: probes do the terminal / filesystem toil.

| Concept | Module / type | File |
|---|---|---|
| Process probe | `struct ProcessProbe`, `struct ProcessProbeConfig`, `struct ProcessProbeMetrics`, `enum ProcessProbeError`, `trait EventSink`, `struct InMemorySink`, `const DEFAULT_GRACE = 10s` | `crates/probes/src/process.rs` |
| File probe | `struct FileProbe`, `struct FileProbeConfig`, `struct FileProbeMetrics`, `enum FileProbeMode { ScanOnce, FollowEnd, FollowBeginning }`, `const DEFAULT_POLL_INTERVAL = 250ms` | `crates/probes/src/file.rs` |
| Directory probe | `struct DirectoryProbe`, `struct DirectoryProbeConfig`, `enum DirectoryEventKind`, `struct DirectoryEvent`, `struct JunitSummary`, `trait DirectorySink`, `struct InMemoryDirectorySink`, `const DEFAULT_DIR_POLL_INTERVAL = 500ms` | `crates/probes/src/directory.rs` |
| PTY normalization (no spawn) | `struct AnsiNormalizer`, `struct PromptDetector`, `enum PromptKind` | `crates/probes/src/pty.rs` |
| Probes crate root | `terminal_commander_probes` lib | `crates/probes/src/lib.rs` |

### Sifters

| Concept | Module / type | File |
|---|---|---|
| Sifter runtime | `struct SifterRuntime`, `SifterRuntime::build` | `crates/sifters/src/lib.rs` |

### Buckets + context

| Concept | Module / type | File |
|---|---|---|
| Bucket identity | `struct BucketId`, wire format `bkt_<32-hex>` | `crates/core/src/ids.rs` |
| Bucket config | `struct BucketConfig`, count cap, TTL | `crates/core/src/bucket.rs` |
| Bucket manager | `struct BucketManager` | same |
| Per-bucket Notify | `BucketInner::notify : Arc<Notify>` | same |
| Bucket wait | `BucketManager::bucket_wait` async, races Notify vs timeout | same |
| Bucket read | `BucketManager::events_since` | same |
| Bucket summary | `BucketManager::summary`, `struct BucketSummary` | same |
| Bucket wait response | `struct BucketWaitResponse { events, heartbeat, dropped_count, ... }` | same |
| Context ring | `struct ContextRingManager`, `struct ContextWindowRequest`, `struct ContextWindowResponse` | `crates/core/src/context.rs` |
| Source frame | `struct SourceFrame`, `FrameId::new()`, `with_line()` | `crates/core/src/source.rs` |
| Source pointer | `struct SourcePointer::new(FrameId)`, `with_line(u64)` | `crates/core/src/pointer.rs` |

### Events + signal

| Concept | Module / type | File |
|---|---|---|
| Severity union | `enum Severity { Trace, Debug, Info, Low, Medium, High, Critical }` | `crates/core/src/severity.rs` |
| Signal event | `struct SignalEvent`, `SignalEvent::validate` enforces pointer-or-reason for sev>=Medium | `crates/core/src/event.rs` |
| Event draft | `struct EventDraft`, `EventDraft::into_signal_event(seq)` | same |
| Event source | `struct EventSource`, `enum SourceType`, `enum SourceStream` | `crates/core/src/source.rs` |
| Captures map | `type Captures = IndexMap<String, String>` | same |
| Rule reference | `struct RuleRef` | `crates/core/src/rule.rs` |
| Rule definition | `struct RuleDefinition`, `enum RuleType`, `enum RuleStatus`, `struct ContextHint` | same |
| Errors | `enum BucketError`, `enum ContextError` | `crates/core/src/error.rs` |

### Identifiers

| Concept | Type | File |
|---|---|---|
| Event id | `EventId` (UUIDv7, wire `evt_<32-hex>`) | `crates/core/src/ids.rs` |
| Bucket id | `BucketId` (wire `bkt_<32-hex>`) | same |
| Probe id | `ProbeId` (wire `probe_<32-hex>`) | same |
| Job id | `JobId` (wire `job_<32-hex>`) | same |
| Frame id | `FrameId` (wire `frame_<32-hex>`) | same |

### Persistence

| Concept | Module / type | File |
|---|---|---|
| Event store | `struct EventStore` | `crates/store/src/lib.rs` |
| Store reader open | `EventStore::open_reader` | same |
| Store writer open | `EventStore::open_writer` (rejects 9P drvfs at open) | same |
| In-memory open | `EventStore::in_memory` | same |
| Migration runner | manual; tracks `schema_migrations` table | same |
| V0001 schema | `migrations/V0001__initial_schema.sql` — `buckets`, `events` | same dir |
| V0002 schema | `migrations/V0002__registry.sql` — `rules`, `rule_versions`, `rule_tags`, `rule_activations` | same dir |
| Audit table (V0003) | **not present on `main`** | n/a — deferred (BACKLOG P0) |
| Rule pack ingest | `import::RulePackFile`, `RulePackMeta`, `ImportResult`, `RULE_PACK_REGEX_SIZE_LIMIT`, `RULE_PACK_DFA_SIZE_LIMIT` | `crates/store/src/import.rs` |
| Registry persistence | `registry::*` | `crates/store/src/registry.rs` |
| Activation log | `registry::ActivationRecord`, `record_activation()` | same |
| Rule search hit | `registry::RuleSearchHit`, `search_rules()` | same |

### CLI

| Concept | Module / type | File |
|---|---|---|
| CLI binary entry | `fn main`, `struct Cli`, `enum Command`, `enum RulesOp`, `enum BucketsOp` | `crates/cli/src/main.rs` |
| Doctor subcommand | `run_doctor` | same |

## Tests on `main`

189 tests across 7 crates. Notable:

| Test file | What it covers |
|---|---|
| `crates/core/tests/load.rs` | Load behavior of bucket manager / ring. |
| `crates/daemon/tests/security.rs` | Structural deny tests: sudo in all profiles, sensitive-path suffixes, no Command::spawn in mcp crate, no TCP listener in mcp crate, RegexSet ReDoS guard. |
| `crates/mcp/tests/e2e.rs` | In-process MVP demo: discover, bucket create+append+read, dynamic rule create + search + activate, async bucket_wait awakened by append, event_context around emitted event, file_read_window cap on large file. |
| Per-crate `#[cfg(test)]` modules | Unit coverage for bucket manager, context ring, sifter runtime, store, registry, probes (file create/truncation/follow; directory create/modify/delete; process stdout/stderr/no-match metrics). |

## Documents anchoring contracts

| Document | Anchors |
|---|---|
| `README.md` | Product purpose, intended architecture, planned MCP tool surface (lines that name `command_start_combed`, `bucket_wait`, etc.), safety model (lines 286-297). |
| `SPEC.md` | Locked contracts. |
| `ARCHITECTURE.md` | Two-process architecture diagram. |
| `SECURITY.md`, `POLICY.md`, `docs/security/PRIVILEGE_MODEL.md` | Sudo deny, default-deny path suffixes, no network listener, no setuid. |
| `docs/contracts/enums/severity.md`, `docs/contracts/enums/policy-decision.md` | Closed-set enums. |
| `docs/storage/EVENT_STORE.md` | Store design. |
| `docs/research/_USER_DECISIONS.md` | Doctrine amendments — toolchain pin, severity union, crate path shorthand. |
| `docs/runtime/**` | Listed by chain SOURCE_MAP as contract / status docs; their detailed audit is out of scope for TC33. |
| `BACKLOG.md` | P0 / P1 / P2 / P3 deferrals. |
| `EVIDENCE_REPORT.md`, `FINAL_REPORT.md` | Per-goal commit hashes and chain-close notes. |

## What is missing from this map

These product concepts have **no module on `main`**:

- rmcp transport adapter — no `rmcp` dependency wired, no stdio loop.
- Daemon runtime bootstrap (config load, tokio runtime, listener bind).
- UDS / named-pipe IPC server.
- Peer-identity check (SO_PEERCRED / equivalent).
- `command_start_combed` MCP tool that wires policy + ProcessProbe + sifter + bucket end-to-end.
- Probe registry inside the daemon (multi-probe routing).
- Rule rebind on registry activate (hot activation in live probes).
- File search tool (different from `file_read_window`).
- File watch MCP tool.
- Audit log writer + reader (no module, no table, no migration).
- Provider-neutral smoke harness.
- Load / backpressure gate.

Each of these has a goal in `terminal-commander-runtime` TC34-TC48.
