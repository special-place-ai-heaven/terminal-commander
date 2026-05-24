# Terminal Commander - Product Specification

Status: Baseline (TC01 wave 0 deliverable).
Audience: implementers of TC02 through TC32.
Language: ASCII only.

## 1. Name and pitch

Product name: Terminal Commander.

One-line pitch: a local, MCP-operated signal-combing layer for LLM coding
agents that converts continuous terminal streams and file activity into
small, structured, severity-tagged events accessible by cursor.

## 2. Objective

Replace the noisy "agent runs a command, tails output, greps chunks,
misses signal, burns tokens, repeats" loop with a local daemon that
observes the source continuously and exposes only vetted signal events
plus bounded context windows.

The product target, per `README.md:17-27` and the MVP target list at
`README.md:333-343`, is a system where an LLM can: start a command, attach
continuous probes, activate registry-backed sifters, receive only
structured signal events, wait for new bucket events by cursor, fetch
bounded context around an event, and never need to read raw terminal
output to make progress.

## 3. Non-goals

These are explicitly out of scope for the MVP and must not be silently
expanded. Defer or document them as future work.

Tier-1 targets are Windows-x64 native and Linux-x64 native (the
latter also covers WSL Ubuntu through the Linux artifact). See
`docs/adr/ADR-native-tier1-runtime.md`.

- Hosted SaaS, remote ingestion, or cross-host federation. Terminal
  Commander runs locally on a single machine.
- Replacing the LLM harness or the MCP client. Terminal Commander is
  a tool surface exposed via MCP, not an agent runtime.
- A general-purpose log shipper (Datadog/Fluent Bit/honeytail
  replacement). The output target is one local LLM session, not a
  metrics pipeline. See `docs/research/prior-art.md`.
- Kernel-enforced sandboxing in the MVP. Policy is advisory in-process
  with an audit log; Landlock and seccomp-bpf are post-MVP hardening.
  See `docs/research/policy-prior-art.md` and
  `docs/research/_USER_DECISIONS.md` (row "Policy enforcement (MVP)").
- macOS support beyond build-only artifacts. macOS is tier-3 per
  `docs/adr/ADR-native-tier1-runtime.md`.
- A full TUI or GUI. The operator surface is a CLI; the LLM surface is
  MCP.
- Privileged installation as the default. Privilege escalation paths
  are gated by explicit policy goals (TC02, TC22) and not part of
  default MVP installation.
- Encryption at rest, secret vaulting, and credential management.
  Deferred per `docs/research/_R1-beta-summary.md`.

## 4. MVP boundaries

In scope for the MVP (TC01 - TC30 outcomes):

- Two-process architecture: `terminal-commander-mcp` (thin MCP server)
  plus `terminal-commanderd` (persistent daemon).
- Process probes (start/observe a command) and file probes (follow,
  rotate, create-after-watch).
- Sifter runtime with keyword, regex, basic numeric condition,
  multiline block, dedupe, suppression, stall, progress, and prompt
  detectors. Full list per section 9.
- In-memory and persistent signal buckets with monotonic cursors,
  severity filtering, and bounded reads.
- Bounded context windows referenced by source pointer.
- Dynamic rule registry with search, get, create, test, and activate
  paths exposed over MCP.
- Audit log of command execution, file access, registry edits, and
  policy denials.
- Advisory policy enforcement (path allow/deny lists, default-deny of
  sensitive files per `README.md:294-297`).
- CLI for operators (`status`, `doctor`, `rules`, `buckets`, `jobs`,
  `probes`, `policy`, `audit` subcommands).
- Linux native + WSL2 support. WSL2 `/mnt/c` (9P) is handled by forced
  polling on file probes per `docs/research/wsl-boundary.md`.

Deferred to post-MVP (or to specific later goals):

- Directory probes and artifact probes beyond the seed implementation
  in TC20.
- Terminal/PTY probe (TC19) is in MVP scope but interactive multi-pane
  attachment and tmux integration are not.
- Journal probe (`journal_probe` per `README.md:128`) is not in MVP.
  Add only when a goal explicitly scopes it.
- Encryption at rest, sqlcipher integration, cross-host audit
  federation, kernel-enforced policy.
- Daemonize (`--daemonize` flag via the `fork` crate) is optional and
  not required for the MVP per
  `docs/research/daemon-lifecycle.md`.

## 5. Terminology

These terms are normative. Goals downstream of TC01 must use them
exactly. The set is derived from `README.md:86-209` and from the
research-locked vocabulary in `docs/research/_USER_DECISIONS.md`.

- **Probe**: a long-running observer attached to a single source.
  Sources are processes, terminals/PTYs, files, directories, journals,
  or structured artifacts. Probes produce raw frames.
- **Frame**: one normalized unit of probe output (typically a line,
  but PTY frames may be a screen update). Frames carry a stream tag
  (`stdout`, `stderr`, `file`, etc.) and a monotonically increasing
  `frame_id` per probe.
- **Sifter**: a rule that consumes frames and emits zero or more
  signal events. Sifters are typed (keyword, regex, condition,
  multiline, etc.) and live in the registry.
- **Rule**: the persistent definition of a sifter, including its kind,
  patterns, capture groups, summary template, severity, and metadata.
  Stored in the registry; mutable at runtime through MCP tools.
- **Signal event**: the structured output of a sifter match. Fields
  are normative per section 10.
- **Signal bucket** (or just "bucket"): a cursor-addressable ordered
  stream of signal events. A bucket is not a log; it is a curated view.
- **Source pointer**: the bounded handle that allows context retrieval
  around a signal event. Identifies the probe, the frame_id, and the
  line number where applicable. Never embeds raw payload.
- **Context window**: a bounded slice of frames around a source
  pointer, retrievable through `event_context(event_id, before, after)`
  (per `README.md:204-209`).
- **Registry**: the persistent CRUD store of rules. Supports search,
  versioning, testing, and activation.
- **Bucket manager**: the in-process component that owns bucket state,
  cursor allocation, severity filters, and bounded reads.
- **Context spool / context ring**: the bounded ring buffer that
  stores frames so context windows can be served without holding the
  whole stream in memory.
- **Daemon (`terminal-commanderd`)**: the persistent process that owns
  probes, sifters, registry, bucket manager, context spool, store,
  policy engine, and audit log.
- **MCP server (`terminal-commander-mcp`)**: the thin per-session
  process that exposes the MCP tool surface over stdio and forwards
  every operation to the daemon.
- **Policy profile**: a named bundle of allow/deny rules that gates
  command execution, file access, registry edits, and probe creation.
  Profiles are defined in TC02 (`developer_local`, `repo_only`,
  `read_only_observer`, `admin_debug` per
  `docs/research/policy-prior-art.md` section scope).
- **Audit log**: the append-only record of every policy decision,
  every command execution, every registry edit, and every privileged
  operation. Source of truth for "what happened."
- **Comb / combing**: the verb form for sifter-mediated extraction of
  signal from a stream. "Live Signal Comber" is the README's name for
  the sifter runtime + bucket manager pair.

## 6. Locked stack decisions

These are locked in `docs/research/_USER_DECISIONS.md` and must not be
re-litigated by downstream goals without an explicit user decision
update.

- Language: Rust, edition 2024.
- Async runtime: tokio (forced by rmcp 1.7.0's `tokio = "1"` dep, per
  `docs/research/mcp-rust-sdk.md`).
- MCP SDK: rmcp `=1.7.0` exact pin. MSRV Rust 1.92.
- Storage: rusqlite 0.39 with `bundled` feature (FTS5 included) +
  refinery 0.9 migrations + WAL mode. See
  `docs/research/sqlite-fts5.md`.
- File watcher: notify 8.2 + notify-debouncer-full 0.7 with explicit
  per-target transport (inotify on Linux/WSL native; `PollWatcher`
  forced on WSL `/mnt/c` 9P). See `docs/research/file-watcher.md` and
  `docs/research/wsl-boundary.md`.
- PTY (MVP): pty-process 0.5.3 with `async` feature. POSIX only;
  portable-pty bridge deferred for Windows native. See
  `docs/research/pty-crate.md`.
- License: Apache-2.0 (SPDX `Apache-2.0`). See
  `docs/research/license-decision.md`.
- Process model: two-process (thin MCP + persistent daemon). IPC
  transport between the two is deferred to TC21 (local Unix domain
  socket via `interprocess` v2 is the leading candidate per
  `docs/research/mcp-transport-pattern.md` but not locked yet).
- Platforms: Linux native + WSL2 primary. macOS / Windows-native
  deferred.

## 7. Crate list (canonical)

Seven crates per `docs/research/_USER_DECISIONS.md` and per TC04's
locked layout. All crates live under `crates/<short>/` in a flat
workspace; package names use hyphens.

| Path | Package name | One-line role |
|---|---|---|
| `crates/core/` | `terminal-commander-core` | Domain types, identifiers, severity, event/source/pointer models, errors, traits. No I/O. |
| `crates/sifters/` | `terminal-commander-sifters` | Sifter runtime: keyword, regex, condition, multiline, dedupe, suppression, stall, progress, prompt, correlation, artifact parsers. |
| `crates/probes/` | `terminal-commander-probes` | Probe runners: process probe, file probe, PTY probe, directory probe, future journal/artifact probes. Owns notify integration and PTY bridging. |
| `crates/store/` | `terminal-commander-store` | rusqlite + refinery persistence: event store, bucket cursors, rule registry, audit log. FTS5 search lane. |
| `crates/daemon/` | `terminal-commanderd` | Long-running daemon binary. Owns bucket manager, context spool, policy engine, audit emitter, and the local API. |
| `crates/mcp/` | `terminal-commander-mcp` | Thin MCP server adapter (rmcp 1.7.0 stdio). Forwards every tool call to the daemon over IPC. |
| `crates/cli/` | `terminal-commander-cli` | Operator CLI. Talks to the daemon. Subcommands per TC25 (status, doctor, rules, buckets, jobs, probes, policy, audit). |

### README reconciliation

`README.md:354-360` lists six crates and omits `terminal-commander-store`.
The seven-crate list above is the canonical architect intent and is
referenced by TC04 (workspace scaffold), TC12 (persistent event store),
and TC13 (registry store), all of which assume a dedicated persistence
crate. The discrepancy is recorded in
`.agent/goals/terminal-commander-mvp/SOURCE_MAP.md` under
"Open decisions / contradictions"; a later goal (post-TC04) is expected
to update the README to match. Until then, the seven-crate list in this
SPEC is authoritative for implementation.

## 8. MCP tool surface

The MCP server exposes the tools below. The set originates from
`README.md:243-266` (verbatim) and is grouped here by category. The
exact wire schema, ownership crate, and policy gating are defined in
TC23, TC24, and TC22 respectively.

### 8.1 Discovery and policy (TC23)

- `system_discover` - return host, platform (linux / wsl), shell,
  policy profile name, daemon version, MCP server version, configured
  paths.
- `policy_status` - return active policy profile, allow/deny paths,
  command allowlist, and any flagged denials in the current session.

### 8.2 Command execution and process control (TC23)

- `command_start_combed` - start a command under a process probe,
  bind one or more bucket and rule selections, return a job id.
- `command_status` - return current status of a job (running,
  exited, failed, signaled).
- `command_write_stdin` - bounded stdin write to a running job.
  Subject to policy.
- `command_send_signal` - send POSIX signal to a job. Subject to
  policy.

### 8.3 Buckets and events (TC23)

- `bucket_create` - create a named bucket bound to one or more
  probes and rule sets.
- `bucket_events_since` - read events strictly after a cursor.
  Bounded by max_count and max_bytes.
- `bucket_wait` - block until new matching signal appears, or the
  timeout elapses. Returns a heartbeat, never a raw output dump.
  Sample payload at `README.md:272-280`. This is the keystone tool
  per `README.md:268-269`.
- `event_context` - return a bounded `before`/`after` context window
  around an event by source pointer. Per `README.md:204-209`.

### 8.4 Probes (TC24)

- `probe_create` - create a probe (process, file, directory) without
  starting a command. Subject to policy.
- `probe_bind_rules` - bind registry rules to an existing probe.

### 8.5 Registry (TC24)

- `registry_search` - search the rule registry by text, kind, tags,
  language.
- `registry_get` - fetch a single rule by id.
- `registry_create` - create a new rule. Subject to validation and
  policy.
- `registry_test` - test a rule against a sample input set without
  persisting any output. Returns matches, captures, summaries, and
  validation diagnostics.
- `registry_activate` - mark a rule as active and eligible for
  default binding.

### 8.6 File observation (TC24)

- `file_read_window` - read a bounded line/byte window from a file.
  Subject to policy.
- `file_search` - text search within a file or directory, bounded
  by max_matches and max_bytes.
- `file_watch` - create a file probe (equivalent path; final naming
  resolved during TC18).

The complete set above is the planned MVP MCP surface. The exact tool
count and naming may shift by a handful as TC23/TC24 finalize schemas;
any change requires a goal-file mini-spec update.

## 9. Probe types

Six probe types per `README.md:124-130`. Three ship in MVP wave; the
other three are scoped to follow-on goals.

| Probe | Source | MVP status | Owning goal |
|---|---|---|---|
| `process_probe` | child process, stdout/stderr | MVP | TC15 |
| `file_probe` | file (follow, rotate, create-after-watch) | MVP | TC18 |
| `terminal_probe` | PTY-backed interactive command | MVP | TC19 |
| `directory_probe` | directory tree, generated artifacts | MVP seed | TC20 |
| `artifact_probe` | structured reports (JUnit XML, coverage JSON) | MVP seed | TC20 |
| `journal_probe` | systemd/journald or equivalent | Post-MVP | not yet assigned |

Per the no-mock invariant, a probe type whose implementation is not
present must not appear in `system_discover` output or in any
configuration default.

## 10. Sifter types

Eleven sifter types per `README.md:136-148`. The MVP wave (TC10, TC11)
ships keyword, regex, dedupe, suppression, multiline block, progress,
stall, and prompt detectors. Numeric condition, correlation, and
artifact parser are post-MVP seeds (TC11 may include a minimum-viable
condition evaluator; correlation and artifact-parser are explicitly
post-MVP unless a later goal pulls them in).

1. keyword
2. regex
3. numeric condition
4. multiline block
5. progress detector
6. prompt detector
7. stall detector
8. dedupe rule
9. suppression rule
10. correlation rule
11. artifact parser

Rule packs (TC14) seed the registry with curated bundles for at least:
`generic.terminal`, `apt`, `cargo`, `npm`, `pytest`, `gcc` per
`README.md:367-372`. Additional bundles (`make`, etc.) may land if
TC14 scope allows.

## 11. Signal event schema

Normative shape per `README.md:171-197`. All fields are required unless
marked optional; downstream goals must not add fields without
extending this section first.

```json
{
  "event_id": "evt_01HX...",
  "bucket_id": "build_42",
  "seq": 1842,
  "timestamp": "2026-05-20T20:11:34.218+02:00",
  "severity": "high",
  "kind": "missing_package",
  "summary": "APT could not locate package libssl-dev",
  "captures": { "package": "libssl-dev" },
  "source": {
    "probe_id": "probe_apt_7",
    "source_type": "terminal",
    "stream": "stderr",
    "job_id": "job_install_1"
  },
  "pointer": {
    "frame_id": 9821,
    "line": 318,
    "context_available": true
  }
}
```

Field constraints:

- `event_id`: ULID-style typed identifier owned by TC06.
- `bucket_id`: stable string per bucket; bucket lifecycle in TC07/TC12.
- `seq`: monotonic per bucket. Cursor semantics defined in TC07/TC12.
- `timestamp`: ISO-8601 with timezone offset.
- `severity`: enum. Initial values: `info`, `low`, `medium`, `high`,
  `critical`. Finalized in TC06.
- `kind`: stable string token identifying the signal class
  (e.g. `missing_package`, `test_failed`, `permission_denied`,
  `compiler_error`). Open-ended; not a closed enum.
- `summary`: rendered from the rule's summary template.
- `captures`: object of named capture groups from the rule.
- `source`: descriptor of the producing probe and stream.
- `pointer`: bounded handle for context retrieval; `context_available`
  is the explicit invariant that some events may have no retrievable
  context (e.g. event derived from a structured artifact summary).
  Stub events that exist but lack a real pointer must set this to
  `false`; they must not silently fabricate a frame_id.

## 12. Source-status notes

Per the no-mock invariant, every shipping component must declare its
current state. The following statuses are defined for use across goals:

| Status | Meaning |
|---|---|
| `live` | Implemented, tested, and exposed by default. |
| `partial` | Implemented for the documented subset; out-of-subset calls return an explicit "not implemented" error rather than silent success. |
| `degraded` | Implementation exists but is currently disabled or running below specified guarantees; the daemon's `doctor` must surface this. |
| `disabled` | Wired but not enabled by default; activation requires explicit configuration. |
| `test_only` | Present in the codebase but only reachable in test or benchmark profiles. |
| `mock` | Test scaffold only. Must not be reachable from a production build path. |
| `deferred` | Defined in this SPEC but not implemented in MVP. Must not appear in `system_discover`. |
| `unknown` | Reserved. If used at runtime, the daemon must log a warning; treat as a bug. |

The `system_discover` MCP tool must report the status of every probe
type, every sifter type, the policy posture, and the audit log target.
A `mock` or `unknown` status must never be reported as `live` to the
MCP client. The advertised tool list MUST equal the actually-
dispatched tool list; a hard-coded mismatch is a defect (TC34 + TC40).

## 13. Runtime contract anchor (TC34)

The realtime signal channel contract and the locked MCP tool surface
are normative for the `terminal-commander-runtime` chain (TC35-TC48).
This SPEC remains the baseline; the runtime contract refines it.

- Product semantics: `docs/runtime/REALTIME_SIGNAL_CHANNEL.md`.
- Tool surface lock: `docs/mcp/TOOL_CONTROL_SURFACE.md`.
- TC33 audit evidence: `docs/audits/runtime-gap-audit.md`,
  `docs/audits/runtime-source-map.md`,
  `docs/audits/runtime-tool-surface-gap.md`.

When this SPEC and the runtime contract conflict, the runtime
contract wins for TC35-TC48; the conflict is recorded as a SPEC
amendment in the goal that observes it.
