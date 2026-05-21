# Terminal Commander

**Terminal Commander** is a local realtime signal-channel and
tool-control abstraction layer for LLM coding agents.

It is NOT a CLI command runner.
It is NOT a shell bridge.
It is NOT a log shipper.

Its job: convert noisy continuous terminal and filesystem streams
into small structured signal events that an LLM can read by cursor,
plus bounded context windows when the LLM needs to look closer.

```text
Raw terminal/file output goes in.
Only vetted, relevant signal comes out.
Context remains available by pointer.
```

## Problem

Current coding-agent terminal interaction is inefficient:

```text
agent runs command
command emits huge output
agent periodically tails output
agent greps or scans chunks
agent misses signal between probes
agent burns tokens on noise
agent repeats
```

This is especially bad for package installation, large builds, test
suites, compilers, long-running shell commands, WSL workflows,
generated logs and reports, and interactive command prompts.

Terminal Commander moves that parsing burden out of the LLM and
into a local streaming system.

## How an LLM uses it

The LLM never reads a noisy terminal stream directly. The LLM
says, in effect:

```text
Run this command.
Watch stdout, stderr, and these files.
Use these registry rules.
Notify me only when useful signal appears.
Give me context around event X if needed.
```

Terminal Commander handles the stream continuously and exposes
structured events such as:

- command failed,
- test failed,
- package missing,
- compiler error,
- permission denied,
- sudo/password prompt,
- stalled command,
- file changed,
- artifact generated,
- repeated warning collapsed,
- numeric threshold crossed.

The LLM consumes those events by cursor. When it wants to look at
the raw lines around an event, it asks for a bounded context window
by pointer — it never receives the whole stream.

## Why the design is shaped this way

- **No raw stream lane to the LLM.** Every LLM-visible response is
  bounded and structured. A "give me the full stdout" tool would
  defeat the purpose.
- **Bounded outputs everywhere.** Bucket reads cap at 10 000
  events. File reads cap at 64 KiB. Context windows cap at 1024
  frames / 64 KiB.
- **Pointer-or-reason invariant.** Every event with
  `severity >= medium` carries a `SourcePointer` OR a typed
  `pointer_unavailable_reason`. The LLM is never left guessing
  whether context exists.
- **No MCP root shell.** The MCP server is a thin adapter. It
  contains no `Command::spawn`, no file open outside its own
  config, no network listener.
- **No network listener at all.** The daemon is reachable only
  over a local Unix domain socket with peer-credential checks.
- **Policy gate before every spawn.** Four closed-set profiles
  (`developer_local`, `repo_only`, `read_only_observer`,
  `admin_debug`) plus a shell-interpreter deny list that prevents
  `command_start_combed` from degrading into a shell bridge.
- **Persistent audit log.** Every policy-relevant action lands in
  SQLite with a closed-set decision label. No in-memory audit on
  the production path.
- **Bucket waits don't busy-poll.** `bucket_wait` parks on a tokio
  `Notify`. On timeout it returns a heartbeat, never raw text.

## Architecture

```text
                +-----------------------------------+
                |   LLM / MCP client                |
                |   (Claude Code, Codex CLI, ...)   |
                +-----------------------------------+
                          |
                          |  MCP (rmcp stdio)
                          v
                +-----------------------------------+
                |  terminal-commander-mcp           |  crates/mcp
                |  - thin adapter                   |
                |  - NO Command::spawn              |
                |  - NO network listener            |
                |  - forwards every call to daemon  |
                +-----------------------------------+
                          |
                          |  Local UDS (length-prefixed JSON,
                          |  peer-cred checked, 256 KiB frame cap)
                          v
+---------------------------------------------------------------+
|  terminal-commanderd  (crates/daemon)                         |
|                                                               |
|  IPC server                                                   |
|    system_discover  health  policy_status  self_check         |
|    bucket_events_since  bucket_wait  bucket_summary           |
|    event_context  (command_* surfaced via the MCP layer)      |
|         every accepted request -> PersistentAudit row         |
|                                                               |
|  Command runtime  (argv-only; NO shell bridge)                |
|    1. validate argv (bounded item count and size)             |
|    2. shell-interpreter deny list                             |
|    3. PolicyEngine::evaluate(CommandStart)                    |
|    4. spawn ProcessProbe (tokio::process, stdin = null)       |
|    5. JobManager start + lifecycle waiter                     |
|                                                               |
|  Router                                                       |
|    bucket_create  bucket_append  bucket_events_since          |
|    bucket_wait (Notify-backed)  event_context                 |
|    every action -> PersistentAudit                            |
|                                                               |
|  Policy engine                                                |
|    4 profiles, sudo/doas/su/pkexec/kexec/polkit deny doctrine |
|    14 default-deny sensitive path suffixes                    |
|                                                               |
|  Persistent audit                                             |
|    SQLite audit_records table, closed-set decisions,          |
|    bounded subject / reason / metadata_json                   |
+---------------------------------------------------------------+
                          |
                          v
+---------------------------------------------------------------+
|  Probes  (crates/probes)                                      |
|  +---------------------------+  +--------------------------+  |
|  | process_probe             |  | file_probe               |  |
|  | tokio::process::Command   |  | follow / create-after /  |  |
|  | non-interactive           |  | truncate / rotate        |  |
|  +---------------------------+  +--------------------------+  |
|  +---------------------------+  +--------------------------+  |
|  | directory_probe           |  | terminal / PTY probe     |  |
|  | create / modify / delete  |  | ANSI normalization +     |  |
|  |                           |  | prompt detection         |  |
|  +---------------------------+  +--------------------------+  |
+---------------------------------------------------------------+
                          |
                          v
+---------------------------------------------------------------+
|  Sifter runtime  (crates/sifters)                             |
|  keyword (aho-corasick), regex (RegexSet), multiline,         |
|  dedupe, suppression, progress, prompt, stall, condition,     |
|  correlation, artifact parsers.                               |
|  Probes hand SourceFrames in. Sifters emit one EventDraft     |
|  per match.                                                   |
+---------------------------------------------------------------+
                          |
                          v
+---------------------------------------------------------------+
|  In-memory bucket manager  (crates/core::bucket)              |
|    per-bucket Notify wakeup -> bucket_wait, no busy poll      |
|    count-cap + TTL retention, drop-oldest + dropped_count     |
|                                                               |
|  Context ring  (crates/core::context)                         |
|    per-probe SourceFrame ring, anchored windows by FrameId    |
|                                                               |
|  Job manager  (crates/core::job)                              |
|    Starting / Running / Exited / Failed / Cancelled           |
|    synthesizes command_exited / command_failed lifecycle      |
|                                                               |
|  Event store + registry  (crates/store)                       |
|    SQLite (rusqlite + bundled SQLite + FTS5),                 |
|    manual migration runner, audit log table                   |
+---------------------------------------------------------------+
```

Data flows DOWN this stack. Signal flows UP. Only structured
events and bounded context cross the IPC boundary.

## Core concepts

### Daemon

`terminal-commanderd` is the long-running local process. It owns
the SQLite event store, the audit log, the in-memory bucket
manager, the context ring, the job manager, the policy engine, the
sifter runtime, and the local UDS server.

The daemon is unprivileged by default. There is no setuid binary,
no polkit rule, no system-level systemd unit. A user-level
systemd-unit example ships under `config/`.

Subcommands:

```text
terminal-commanderd check          # bootstrap + self-check report, exit
terminal-commanderd start          # bootstrap + bind UDS, idle until SIGTERM
terminal-commanderd print-config   # render the resolved config back to TOML
```

### MCP server

`terminal-commander-mcp` is the provider-neutral interface used by
Claude Code, Codex CLI, IDE agents, or other LLM harnesses.

It is a thin adapter. It contains no `Command::spawn`, no file open
outside its own config, and no network listener. Every tool call is
forwarded to the daemon over the local UDS.

### Probes

A probe observes a source and produces normalized `SourceFrame`s.
It does NOT decide significance.

- `process_probe` — spawns and watches a command (non-interactive).
- `file_probe` — follows a file, handles create-after-start,
  truncation, and rotation.
- `directory_probe` — watches a directory for create / modify /
  delete events; surfaces basic artifact summaries.
- `terminal_probe` / PTY — attaches to an interactive PTY stream
  (ANSI normalization + prompt detection ship today; PTY spawn is
  a follow-up).
- `journal_probe` — watches systemd/journal output where allowed.
- `artifact_probe` — summarizes structured reports such as JUnit
  XML or coverage JSON.

### Sifters

A sifter extracts signal from noise. Rule types:

- keyword,
- regex,
- numeric condition,
- multiline block,
- progress detector,
- prompt detector,
- stall detector,
- dedupe rule,
- suppression rule,
- correlation rule,
- artifact parser.

Sifters live in a dynamic registry so an LLM can search, select,
create, test, version, and activate them at runtime.

### Signal buckets

A bucket is a cursor-based stream of structured `SignalEvent`s.
**A bucket is not a raw log.**

Per-bucket retention: count cap (default 100 000) AND TTL (default
24 h). Drop-oldest, with `dropped_count` exposed so the LLM knows
when retention discarded older signal.

Each event carries:

- monotonic per-bucket `seq` number,
- timestamp (RFC 3339),
- severity (`trace` / `debug` / `info` / `low` / `medium` / `high`
  / `critical`),
- open-string `kind`,
- short single-line summary,
- optional captures map (string → string, insertion-ordered),
- optional source pointer or typed `pointer_unavailable_reason`,
- optional rule reference and tags,
- dedupe / suppression metadata.

Example shape (informal):

```json
{
  "event_id": "evt_01HX...",
  "bucket_id": "bkt_01HX...",
  "seq": 1842,
  "timestamp": "2026-05-20T20:11:34.218+02:00",
  "severity": "high",
  "kind": "missing_package",
  "summary": "APT could not locate package libssl-dev",
  "captures": { "package": "libssl-dev" },
  "source": {
    "probe_id": "probe_01HX...",
    "source_type": "process",
    "stream": "stderr",
    "job_id": "job_01HX..."
  },
  "pointer": { "frame_id": "frame_01HX...", "line": 318 }
}
```

### Context by pointer

Raw stream data is available **only** through bounded windows. The
LLM asks:

```text
event_context(bucket_id, event_id, before, after, max_bytes)
```

The daemon resolves the event's `SourcePointer` against the
per-probe context ring and returns a bounded window. If the anchor
frame has already been evicted from the ring, the response carries
a typed `unavailable_reason` — it never invents raw text. Hard
caps: 1024 frames, 64 KiB.

## MCP tool surface

The provider-neutral interface used by an LLM harness:

```text
system_discover            policy_status
command_start_combed       command_status
command_write_stdin        command_send_signal
bucket_create              bucket_events_since
bucket_wait                bucket_summary
event_context
probe_create               probe_bind_rules
registry_search            registry_get
registry_create            registry_test
registry_activate
file_read_window           file_search           file_watch
```

The most important tool is `bucket_wait`. It lets the LLM wait for
meaningful signal without polling raw output:

```json
{
  "method": "bucket_wait",
  "params": {
    "bucket_id": "bkt_01HX...",
    "cursor": 1842,
    "severity_min": "medium",
    "timeout_ms": 30000
  }
}
```

If nothing matching arrives in `timeout_ms`, the response is
`heartbeat = true` with an empty `events` array. Never a raw
output dump.

The locked tool list with per-tool bounds and policy gates lives in
`docs/mcp/TOOL_CONTROL_SURFACE.md`.

## Safety model

Terminal Commander runs locally. The security envelope is explicit:

- The MCP server is unprivileged. A structural test in
  `crates/daemon/tests/security.rs` enforces no `Command::spawn`,
  no `TcpListener`, and no `UdpSocket` in the MCP crate.
- The daemon is unprivileged by default. No setuid binary, no
  polkit rule, no installed system service.
- All command execution passes the policy engine BEFORE spawn.
- The command runtime applies a shell-interpreter deny list
  BEFORE policy: `sh`, `bash`, `dash`, `zsh`, `fish`, `ksh`,
  `csh`, `tcsh`, `ash`, `busybox`, `powershell`, `powershell.exe`,
  `pwsh`, `pwsh.exe`, `cmd`, `cmd.exe`. argv whose basename
  matches any of these is rejected. `command_start_combed` is
  argv-only and is not a shell bridge.
- The policy `COMMANDS_DENY` set (sudo, doas, su, pkexec, kexec,
  polkit-agent, polkit-auth-agent-1) is rejected by every profile.
- File access respects 14 default-deny path suffixes covering
  private keys, credential stores, sudoers, and token caches.
- Every policy-relevant action is audited to a persistent SQLite
  audit log with a closed-set decision label. No in-memory audit
  ever appears on a production path.
- The local UDS server fails closed on Linux / WSL when peer
  credentials cannot be obtained.
- No network listener exists in the daemon or in the MCP crate.

Default-denied sensitive areas include private keys, password
files, credential stores, and token caches unless explicitly
allowed by policy.

## Configuration

Operator-tunable settings live in `terminal-commanderd.toml`. A
safe-to-commit example ships at
`config/terminal-commanderd.example.toml`. Notable knobs:

- `daemon.data_dir` — where the SQLite DB and audit log live. MUST
  be a native Linux filesystem; WSL `/mnt/c` is rejected at writer
  open.
- `daemon.socket_path` — local UDS path; defaults to
  `<data_dir>/terminal-commanderd.sock`.
- `daemon.runtime_mode` — `self_check` / `foreground_idle` /
  `ipc_server`.
- `policy.profile` — `developer_local` / `repo_only` /
  `read_only_observer` / `admin_debug`.
- `retention.max_events` / `retention.ttl_seconds` — per-bucket
  caps.
- `audit.retention_days` — audit log retention.
- `limits.file_window_bytes` — clamped at config load to 64 KiB.
- `limits.bucket_read_limit` — clamped at config load to 10 000.

## Repository layout

```text
.agent/
  goals/
    terminal-commander-mvp/        # library + scaffold goals
    terminal-commander-runtime/    # daemon runtime + IPC goals

crates/
  core/                            # terminal-commander-core
  sifters/                         # terminal-commander-sifters
  probes/                          # terminal-commander-probes
  store/                           # terminal-commander-store
  daemon/                          # terminal-commanderd
  mcp/                             # terminal-commander-mcp
  cli/                             # terminal-commander-cli

config/
  terminal-commanderd.example.toml
  terminal-commanderd.service.example

rules/
  apt.json  cargo.json  gcc.json  generic.terminal.json
  make.json  npm.json  pytest.json

tests/
  fixtures/                        # contracts, terminal, files, ...

docs/
  runtime/                         # REALTIME_SIGNAL_CHANNEL.md,
                                   # UDS_IPC.md, COMMAND_RUNTIME.md
  mcp/                             # TOOL_CONTROL_SURFACE.md
  audits/                          # runtime reality audit
  contracts/                       # wire-shape fixtures + enums
  storage/  security/  install/  research/
```

## Building and verifying

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo nextest run --workspace
```

UDS IPC and command-runtime integration tests are Unix-only.
Windows native compiles the workspace but skips those tests; use
WSL2 to exercise the full surface.

## Runtime contract

The runtime chain (`.agent/goals/terminal-commander-runtime/`) is
the realtime-signal-channel lock-in for this codebase. Two
documents are normative:

- `docs/runtime/REALTIME_SIGNAL_CHANNEL.md` — product semantics:
  Terminal Commander is a realtime signal channel and tool-control
  abstraction layer for LLM agents, not a CLI command runner.
- `docs/mcp/TOOL_CONTROL_SURFACE.md` — locked MCP tool list with
  per-tool bounds and policy gates.

If a planned feature would create a raw-stream lane, bypass the
policy engine, open a network listener, or hard-code an MCP tool
list that diverges from the live dispatcher, it is a contract
violation and the relevant goal stops and amends the runtime
contract.

## Development approach

The project develops through small, sequential `/goal` files. Each
goal is:

- branch-safe,
- evidence-driven,
- narrowly scoped,
- independently verifiable,
- small enough for one autonomous agent run,
- explicit about allowed and forbidden files,
- clear about stop conditions and acceptance criteria.

## License

Apache-2.0; see LICENSE.

SPDX identifier: `Apache-2.0`. The full Apache License 2.0 text is
in the `LICENSE` file at the repository root, and `NOTICE` records
the rmcp relicensing transition relevant to the supply-chain
(`cargo-deny`) license allowlist. See
`docs/research/license-decision.md` for the decision rationale, and
`CONTRIBUTING.md` for the per-file SPDX header expectation.
