# Terminal Commander

**Terminal Commander** is a local, MCP-operated terminal and file signal-combing layer for coding agents and LLM harnesses.

Its purpose is to replace noisy, expensive, periodic terminal polling with continuous local observation and small, structured, real-time signal events.

```text
Raw terminal/file output goes in.
Only vetted, relevant signal comes out.
Context remains available by pointer.
```

## Status

This repository is at project bootstrap stage.

The intended first milestone is an MVP that can:

- run a shell command through a local daemon,
- continuously process stdout and stderr line-by-line or frame-by-frame,
- apply dynamic keyword, regex, and condition-based sifters,
- store matching signal events in cursor-based buckets,
- expose those buckets through an MCP server,
- allow an LLM to read only new signal since a cursor,
- allow bounded context lookup around a signal event,
- maintain a dynamic registry of reusable sifter rules.

No production implementation is implied until the corresponding goal files are completed and verified.

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

This is especially bad for:

- package installation,
- large builds,
- test suites,
- compilers,
- long-running shell commands,
- WSL workflows,
- generated logs and reports,
- interactive command prompts.

Terminal Commander is designed to move that parsing burden out of the LLM and into a local streaming system.

## Product idea

The LLM should not need to read a noisy terminal stream directly.

Instead, it should be able to say:

```text
Run this command.
Watch stdout, stderr, and these files.
Use these registry rules.
Notify me only when useful signal appears.
Give me context around event X if needed.
```

Terminal Commander then handles the stream continuously and exposes structured events such as:

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

## Core concepts

### MCP server

The MCP server is the provider-neutral interface used by Claude Code, Codex CLI, IDE agents, or other LLM harnesses.

It exposes tools for:

- starting commands,
- creating probes,
- reading signal buckets,
- waiting for new bucket events,
- reading event context,
- searching the sifter registry,
- creating and activating new rules.

### Local daemon

The daemon performs the actual work:

- command execution,
- terminal stream capture,
- file watching,
- directory watching,
- stream normalization,
- rule execution,
- event storage,
- bucket management,
- context spooling,
- policy enforcement,
- audit logging.

The LLM-facing MCP server should not be treated as an unrestricted root shell.

### Probes

A probe observes a source.

Planned probe types:

- `process_probe` — starts and watches a command,
- `terminal_probe` — attaches to a PTY, shell, tmux pane, or interactive stream,
- `file_probe` — watches an existing or future file,
- `directory_probe` — watches generated artifacts and changed files,
- `journal_probe` — watches systemd/journal output where allowed,
- `artifact_probe` — summarizes structured reports such as JUnit XML or coverage JSON.

### Sifters

A sifter extracts signal from noise.

Planned sifter types:

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

Sifters are stored in a dynamic registry so an LLM can search, select, create, test, version, and activate them at runtime.

### Signal buckets

A bucket is a cursor-based stream of structured signal events.

A bucket is not a raw log.

A bucket contains:

- monotonic sequence numbers,
- timestamps,
- severity,
- event kind,
- summaries,
- extracted fields,
- source pointers,
- context references,
- dedupe metadata,
- noise-suppression statistics.

Example event shape:

```json
{
  "event_id": "evt_01HX...",
  "bucket_id": "build_42",
  "seq": 1842,
  "timestamp": "2026-05-20T20:11:34.218+02:00",
  "severity": "high",
  "kind": "missing_package",
  "summary": "APT could not locate package libssl-dev",
  "captures": {
    "package": "libssl-dev"
  },
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

### Context by pointer

Raw stream data should remain available only through bounded context tools.

For example:

```text
event_context(event_id, before=3, after=5)
```

This allows an agent to inspect nearby source lines without reading the entire terminal stream or log file.

## Intended architecture

```text
LLM / agent harness
        |
        | MCP
        v
terminal-commander-mcp
        |
        | local API
        v
terminal-commanderd
        |
        +--> process probes
        +--> terminal probes
        +--> file probes
        +--> directory probes
        |
        v
Live Signal Comber
        |
        +--> sifter registry
        +--> sifter runtime
        +--> context spool
        +--> policy engine
        |
        v
signal buckets
```

## Planned MCP tool surface

Initial tool candidates:

```text
system_discover
policy_status
command_start_combed
command_status
command_write_stdin
command_send_signal
bucket_create
bucket_events_since
bucket_wait
event_context
probe_create
probe_bind_rules
registry_search
registry_get
registry_create
registry_test
registry_activate
file_read_window
file_search
file_watch
```

The most important tool is `bucket_wait`.

It should allow a client to wait for meaningful signal without polling raw output:

```json
{
  "bucket": "build_42",
  "cursor": 1842,
  "severity_min": "medium",
  "timeout_ms": 30000
}
```

If no relevant signal appears, the response should be a heartbeat, not a raw output dump.

## Safety model

Terminal Commander is intended to run locally and may eventually support privileged installation.

The security model must therefore be explicit:

- MCP server should be unprivileged where possible.
- Daemon/helper may be privileged only when configured.
- All command execution must pass policy checks.
- File access must respect allowed and denied paths.
- Risky operations must be auditable.
- Raw secret-bearing files must be denied by default.
- No LLM-facing interface should become an unrestricted root shell.

Default-denied sensitive areas should include private keys, password files, credential stores, and token caches unless explicitly allowed by policy.

## Development approach

This project should be developed through small, sequential `/goal` files.

Each goal should be:

- branch-safe,
- evidence-driven,
- narrowly scoped,
- independently verifiable,
- small enough for one autonomous agent run,
- explicit about allowed and forbidden files,
- clear about stop conditions and acceptance criteria.

The expected goal chain starts with:

1. repository bootstrap,
2. architecture and security specification,
3. test methodology,
4. core schemas,
5. event store,
6. bucket manager,
7. context ring,
8. sifter runtime,
9. registry database,
10. process probe,
11. MCP server,
12. `bucket_wait`,
13. file probe,
14. rule packs,
15. policy engine,
16. installer and WSL support,
17. integration validation.

## MVP target

The MVP is considered useful when an LLM can:

1. start a command through MCP,
2. attach continuous stdout/stderr probes,
3. activate registry-backed sifters,
4. receive only structured signal events,
5. wait for new bucket events by cursor,
6. request bounded context around an event,
7. avoid reading large raw terminal output.

## Repository conventions to establish

Planned layout:

```text
.agent/
  goals/
    terminal-commander-mvp/

crates/
  terminal-commander-core/
  terminal-commander-sifters/
  terminal-commander-probes/
  terminal-commander-store/
  terminal-commanderd/
  terminal-commander-mcp/
  terminal-commander-cli/

config/
  terminal-commander.example.toml
  policy.example.toml

rules/
  generic.terminal.json
  apt.json
  cargo.json
  npm.json
  pytest.json
  gcc.json

tests/
  fixtures/
  integration/
  load/
```

The exact layout should be confirmed by the initial bootstrap and architecture goals before implementation begins.

## License

Apache-2.0; see LICENSE.

SPDX identifier: `Apache-2.0`. The full Apache License 2.0 text is in the
`LICENSE` file at the repository root, and `NOTICE` records the rmcp
relicensing transition relevant to the supply-chain (`cargo-deny`)
license allowlist. See `docs/research/license-decision.md` for the
decision rationale, and `CONTRIBUTING.md` for the per-file SPDX header
expectation.
