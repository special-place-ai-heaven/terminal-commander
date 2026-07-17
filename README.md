<div align="center">

![Terminal Commander](./docs/logo.jpg)

# Terminal Commander

**Structured terminal signals for AI coding agents — bounded receipts, never silence, local-first.**

[![npm](https://img.shields.io/npm/v/terminal-commander?label=npm&color=cb3837)](https://www.npmjs.com/package/terminal-commander)
[![CI](https://github.com/special-place-ai-heaven/terminal-commander/actions/workflows/npm-binary-build.yml/badge.svg)](https://github.com/special-place-ai-heaven/terminal-commander/actions/workflows/npm-binary-build.yml)
[![License: PolyForm Noncommercial](https://img.shields.io/badge/license-PolyForm%20Noncommercial%201.0.0-blue)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95-orange?logo=rust)](./rust-toolchain.toml)
[![MCP](https://img.shields.io/badge/protocol-MCP-8A2BE2)](https://modelcontextprotocol.io)
[![Platforms](https://img.shields.io/badge/platforms-win--x64%20%7C%20linux--x64%20%7C%20linux--arm64%20%7C%20mac--arm64%20%7C%20mac--x64-555)](#platform-support)

[Install](#quick-start) · [Why](#why-terminal-commander) · [How It Works](#architecture) · [Tools](#mcp-tool-surface) · [Innovations](#innovations)

</div>

---

Terminal Commander is a local MCP control plane for coding agents. It gives
Cursor, Codex CLI, Claude Code, Claude Desktop, and other MCP clients a bounded
tool surface for commands, files, PTYs, persistent shell sessions, runtime
state, and signal context.

The goal is **omni**: an agent should never need a separate raw terminal tool.
The same runtime is available through either five compact, action-dispatched
facades (`command`, `session`, `files`, `registry`, `status`) or the full
51-tool granular surface. It covers one-shot commands and shell pipelines,
persistent stateful sessions, interactive PTYs (unix and Windows ConPTY),
unknown-output rule suggestion, and operator-gated remote hosts. The agent's
lane-selection map is [`docs/mcp/OMNI_PLAYBOOK.md`](docs/mcp/OMNI_PLAYBOOK.md).

Raw terminal output stays out of the model transcript. The agent defines
keyword/regex **rules**, runs the command, and receives only the matching
**signal events** plus the exit state. A quiet command (zero matches) returns a
bounded **receipt** — exit code, suppressed-line count, short tail — so no
result is ever silent or misleading.

> [!IMPORTANT]
> The default command lane is **argv-only**: `argv[0]` is the program, shell
> interpreters (`sh`, `bash`, `cmd`, `powershell`, …) are denied, and there is
> no string-concatenated shell anywhere in the path. Pipelines and redirects
> live behind the separate `shell_exec` tool, which is disabled unless the
> operator enables the `allow_shell` policy capability. When that lane is
> enabled and no shell override is supplied, Terminal Commander follows the
> highest-ranked interpreter route proven by `system_discover`.

## Contents

- [Why Terminal Commander](#why-terminal-commander)
- [Innovations](#innovations)
- [Quick Start](#quick-start)
- [Architecture](#architecture)
- [The Life Of A Command](#the-life-of-a-command)
- [How LLMs Should Use It](#how-llms-should-use-it)
- [MCP Tool Surface](#mcp-tool-surface)
- [Per-Harness Sessions](#per-harness-sessions)
- [Harness Configuration](#harness-configuration)
- [Platform Support](#platform-support)
- [Admin CLI](#admin-cli)
- [Doctor And Repair](#doctor-and-repair)
- [Update](#update)
- [Environment](#environment)
- [Local State](#local-state)
- [Safety Posture](#safety-posture)
- [Develop From Source](#develop-from-source)
- [Repository Layout](#repository-layout)
- [License](#license)

## Why Terminal Commander

Coding agents run terminal commands constantly, and raw terminal output is
hostile to them: unbounded, noisy, and token-expensive. A test suite that
prints 2,802 lines costs a context-window fortune to scroll — when the only
information the agent needed was "exit 0" and five matched lines. That is the
product: in real use, Terminal Commander condensed exactly such a run into a
five-line receipt with the exit code.

| Instead of | Use | Effect |
|---|---|---|
| Reading 2,800 lines of test scrollback | `command_start_combed` + rules + `command_status` | Matched signals + a bounded exit receipt |
| Polling raw stdout in a loop | `bucket_wait` (long-poll with cursor) | Wake on signal, heartbeat on quiet |
| Re-running a command to "see that error again" | `event_context` on the event pointer | Bounded context window around the line |
| `tail -f build.log` in a terminal you can't see | `file_watch_start` + rules | Structured events as lines append |
| One watcher loop per running job | `subscription_open` over many sources | One multiplexed pull for everything |
| Pasting a whole file for one section | `file_read_window` / `file_search` | Bounded windows and match pointers |

Three properties rank above everything else, in this order:

1. **Trust** — every response is honest about state (`running`/`exited`/
   `failed`/degraded), counters are near-real-time and never lie, receipts are
   accurate, and errors teach the caller how to recover.
2. **Reliability** — the daemon self-manages (idle self-reap with live-work
   veto), processes are torn down as whole trees on stop, and degraded states
   are disclosed loudly with a recovery path.
3. **LLM ergonomics** — a fresh model with only the tool schemas succeeds on
   the first call: minimal required fields, defaults that work, lenient
   parameter coercion for real-world MCP clients, and one complete teaching
   error instead of a guessing game.

## Innovations

The engineering decisions that distinguish Terminal Commander from "run a
command over MCP".

### Structured signals over raw streams

Every captured line flows through the **sifter runtime**: an Aho-Corasick pass
for keyword rules and a compiled regex-set pass for pattern rules, one event
per matching rule per frame, with named captures, severity, tags, and a
summary template. The agent reads events, not scrollback. Rules can be passed
inline per command (minimal: `[{"pattern": "ERROR"}]` — everything else has a
sane default), or persisted in a versioned **registry** and activated
globally or scoped to one bucket/job/probe. Activating a new version of a rule
supersedes the old one in that scope, so one line never fires twice.

### Bounded receipts that never go silent

A command whose output matched zero rules is not an error and not an empty
success — it returns a **receipt**: exit code, how many lines were suppressed,
and a short tail. The same no-silence rule runs through the whole surface:
`command_status` for a finished quiet job carries the receipt; a stopped job
reports its real counters (snapshotted from the live probe metrics, not
zeroes); truncation is always flagged (`truncated_lines`, `truncated_bytes`,
`evicted_frames`).

### Degraded-state disclosure with recovery hints

If an IPC error interrupts a `run_and_watch` wait, the response does not
become a bare error — the job handle is preserved and the result arrives with
`degraded: true`, the last observed state, and a `recover_hint` that tells the
agent exactly how to re-attach (`command_status` with the returned `job_id`).
Mutating RPCs are never auto-retried; idempotent ones may be. A daemon restart
is detectable via `boot_id` on subscriptions.

### Evidence-backed environment beachheads

`system_discover` probes the host under hard deadlines instead of inferring a
route from the OS name. It reports terminal evidence, shell and PowerShell
paths/versions, WSL state, common tools, and confirmed execution status.
Confirmed programs become `direct_argv` routes, confirmed WSL adds a
`wsl_argv` route, and successful interpreter sentinels become shell routes. The
daemon filters that ranked set through the active command-probe, shell, and
command policy before returning `access_routes`; `repo_only` uses its configured
repository root for this discovery check, never the daemon's incidental cwd.
`beachhead` repeats the best surviving route with the exact argv template an LLM
can follow. A path that merely exists is not advertised as executable, and
failed or timed-out probes remain explicit evidence. Every concrete call is
still rechecked with its real argv and cwd.

In-process embedders construct `DaemonState` with `DaemonState::bootstrap` and
call `DaemonState::discover_environment`; IPC discovery uses that same method,
so embedded and daemon clients receive identical capability filtering without
opening a socket.

### Self-healing daemon transport

The adapter re-probes stale availability with a bounded Health call and
single-flights concurrent recovery. If the daemon disappeared, it can reuse the
original supervisor plan to restore the local session. Read-only/idempotent
requests may retry once after recovery; mutating requests are never replayed
automatically. Version skew is refreshed through Health, and a mid-call loss
returns a structured recovery contract instead of an opaque pipe error.

### Buckets, context rings, and subscriptions

Signals land in per-job **buckets** read by cursor (`bucket_events_since`,
`bucket_wait`) so nothing is lost between polls and nothing is re-sent.
Each probe keeps a bounded **context ring** of raw frames so `event_context`
can resolve a pointer into surrounding lines on demand — context is fetched
when needed, never pushed. **Subscriptions** multiplex many sources behind one
predicate (severity floor, kind allowlist, tag, source set) with per-source
liveness in every pull, and `sources: all` auto-joins future probes.

### A thin-facade adapter that cannot surprise you

`terminal-commander-mcp` is a stdio adapter in which every tool forwards 1:1
to a daemon IPC method. CI guards assert the adapter source contains no
process spawn, no network socket, and no direct filesystem access — the policy
gate in the daemon is the single choke point. Tool schemas are typed plainly
(no `["integer","null"]` unions that real MCP clients strip), and parameters
arriving as stringified numbers or JSON-encoded arrays are coerced with
teaching errors — schema honesty for clients that aren't.

### Per-session daemons with disciplined lifecycles

Each harness gets its own daemon keyed by a deterministic `TC_SESSION` token —
two harnesses never share state. An idle daemon self-reaps after
`TC_IDLE_TTL_SECS` (default 1800 s) of no real IPC, but **live work vetoes the
reap**: a still-running command, file watch, or PTY job keeps the daemon up so
children are never orphaned and receipts never lost. `command_stop` kills the
whole process tree, identity-gated so a recycled PID is never signalled.

### Policy gate, audit trail, and the argv-only contract

Every command start passes a policy engine (profile-based: deny lists, path
suffix guards, per-call caps) and emits a durable audit row with
credential-redacted argv. The shell lane (`shell_exec`) is a separate policy
action (`allow_shell`, default off) — enabling it is an explicit operator
decision, never an agent's.

### Rule packs: expert signal extraction in one call

`registry_import_pack` ships **25 curated packs** so an agent gets expert
rules without authoring JSON: `ansible`, `apt`, `bundler`, `cargo`, `choco`,
`cleanup`, `docker`, `dotnet`, `gcc`, `generic.terminal`, `git`, `go`,
`kubectl`, `make`, `msbuild`, `npm`, `pip`, `pnpm`, `pytest`, `ssh`,
`systemd`, `terraform`, `uv`, `winget`, `yarn`. Pack rules label honestly: a
generic `warning:` matcher claims no language it cannot verify. When a known
tool runs without its pack, a command-start response can carry a
`pack_available` hint pointing at `registry_import_pack`.

For output whose format you do not know yet, `registry_suggest_from_samples`
proposes DRAFT rules from raw samples (pure-Rust heuristics). It NEVER
auto-activates: the loop is always suggest -> `registry_test` ->
`registry_upsert` -> `registry_activate`.

> [!TIP]
> Prefer **scoped** activation (`{"kind":"job", "job_id": …}`) or per-command
> inline rules over global activation. Globally-activated rules see every
> command's streams — a `warning:` pattern meant for cargo will also fire on
> git's CRLF notices.

## Quick Start

Install from npm:

```powershell
npm install -g terminal-commander@latest
```

The npm install is intentionally passive: no `postinstall` bootstrap, no MCP
config writes, no daemon start, no WSL install, no shell wrapper, no
hidden-window helper spawn.

Configure detected harnesses explicitly:

```powershell
terminal-commander setup harness
```

Choose the MCP surface explicitly when the harness benefits from a smaller tool
list:

```powershell
terminal-commander setup harness --surface compact
```

`compact` exposes five action-dispatched facade tools. `full` exposes all 51
granular tools and is the server default when `TC_SURFACE` is unset. Both views
reach the same daemon operations and enforce the same policy.

Or target one harness:

```powershell
terminal-commander setup harness --provider cursor
terminal-commander setup harness --provider codex-cli
terminal-commander setup harness --provider claude-code
terminal-commander setup harness --provider claude-desktop
```

Verify:

```powershell
terminal-commander doctor harness
terminal-commander doctor daemon
terminal-commander session list
terminal-commander --version
```

When a harness starts `terminal-commander-mcp`, the adapter resolves the
endpoint from the inherited `TC_SESSION` (or `TC_SOCKET` override) and talks
to `terminal-commanderd` over local IPC. If the daemon is not already running,
the adapter spawns its own session daemon and reports the result on stderr.

## Architecture

```mermaid
flowchart LR
  subgraph harness["MCP harnesses (each env.TC_SESSION)"]
    Cursor["Cursor"]
    Codex["Codex CLI"]
    Claude["Claude Code / Desktop"]
    Other["Other MCP clients"]
  end

  subgraph mcp["terminal-commander-mcp"]
    Stdio["rmcp stdio server\n5 compact facades or 51 full tools\n1:1 facade over IPC"]
  end

  subgraph sup["terminal-commander-supervisor (shared lib)"]
    Ensure["ensure_daemon\nprobe + spawn-if-absent"]
    Replace["replace_if_stale\nversion-gated swap"]
    Session["session tokens\n+ endpoint resolution"]
  end

  subgraph daemon["terminal-commanderd (one per session)"]
    IPC["IPC server\nUDS or named pipe"]
    Policy["policy gate\nargv deny · allow_shell cap"]
    Dispatch["request dispatch"]
    Router["router"]
    Sift["sifter runtime\nkeyword AC + regex set"]
    Store["SQLite store\nevents · registry · audit"]
    Buckets["buckets + context rings\n+ subscriptions"]
    Idle["idle self-reap\nTC_IDLE_TTL_SECS\n(live work vetoes)"]
  end

  subgraph runtimes["Probe runtimes"]
    Cmd["argv commands"]
    Files["file read · search · watch"]
    Pty["PTY sessions"]
  end

  Cursor --> Stdio
  Codex --> Stdio
  Claude --> Stdio
  Other --> Stdio
  Stdio --> Ensure
  Ensure --> Replace
  Replace -. "health handshake\n(probe_endpoint)" .-> IPC
  Replace -. "spawn terminal-commanderd\nif endpoint absent" .-> IPC
  Stdio <--> IPC
  IPC --> Policy
  Policy --> Dispatch
  Dispatch --> Router
  Router --> Cmd
  Router --> Files
  Router --> Pty
  Cmd --> Sift
  Files --> Sift
  Pty --> Sift
  Sift --> Buckets
  Buckets --> Store
  Dispatch --> Store
  Idle --> IPC
  Session -. "TC_SESSION / TC_SOCKET" .-> IPC
```

The MCP adapter does not spawn arbitrary commands and does not open network
sockets — CI guards enforce this on the adapter source. It forwards tool calls
to the daemon over local IPC; the daemon applies policy before starting argv
commands or returning bounded file/context data.

`probe_endpoint` performs a bounded `health` IPC handshake, not a bare
connect. A pre-bound or stale socket that does not answer with our protocol is
rejected; `ensure_daemon` may spawn a fresh session daemon instead.

### In-process embedding

Host applications can depend on the `terminal-commanderd` library and build the
full engine with `DaemonState::bootstrap` without starting IPC, MCP, or the CLI.
The [embedding guide](docs/EMBEDDING.md) documents the exact boundary, the
capability-filtered discovery API, direct command path, OS restrictions, and
revision-pinning requirement.

## The Life Of A Command

```mermaid
sequenceDiagram
    participant A as Agent
    participant M as terminal-commander-mcp
    participant D as terminal-commanderd
    participant P as Process probe

    A->>M: run_and_watch argv=["cargo","test"] rules=[{"pattern":"FAIL"}]
    M->>D: command_start_combed (IPC)
    D->>D: shell-interpreter guard, policy gate, audit row
    D->>P: spawn child (argv, no shell)
    P-->>D: stdout/stderr frames (line-bounded)
    D->>D: sifter combs each frame against active ∪ inline rules
    D->>D: matches → signal events in the job's bucket
    D->>D: non-matches → suppressed counters + context ring
    M->>D: bucket_wait slices (cursor, bounded by wait_ms)
    P-->>D: child exit → lifecycle event + final counters + audit
    alt rules matched
        M-->>A: signals[] + exit_code + state (complete:true)
    else zero matches (quiet command)
        M-->>A: receipt: exit code, lines_suppressed, short tail
    else wait budget spent, still running
        M-->>A: wait_exhausted:true + cursor → poll command_status
    else IPC interrupted mid-wait
        M-->>A: degraded:true + last observed state + recover_hint
    end
```

> [!NOTE]
> Every branch of that `alt` returns something useful. There is no path where
> a started command yields a bare error that loses the job handle, and no path
> where output disappears without a count of what was suppressed.

## How LLMs Should Use It

Start by discovering the host. Follow the returned beachhead rather than
assuming Bash, PowerShell, WSL, or a particular executable is available:

```text
status action=system_discover
→ environment.access_routes[] + environment.beachhead.argv_template
→ direct_argv/wsl_argv/wsl_shell: command action=run|run_and_watch with argv_template
→ shell: command action=exec with shell=<executable> and shell_line=<command>
```

Use Terminal Commander whenever raw terminal scrollback would waste context or
hide the signal. Compact-surface examples are shown below; full-surface callers
can use the granular action names documented in the linked tool contract.

One-shot (most common):

```text
command action=run_and_watch argv=["npm","test"] rules=[{"pattern":"FAIL"}]
→ signals + exit_code in one call; quiet runs return a receipt
```

Long-running, with live monitoring:

```text
command action=run argv=["cargo","nextest","run"] rules=[{"pattern":"^\\s+FAIL"}]
command action=wait bucket_id=<returned> cursor=0 timeout_ms=10000 max_signals=50
command action=status job_id=<returned>          # near-real-time counters
command action=event_context bucket_id=… event_id=…
command action=output_tail job_id=<returned> strip_ansi=true  # bounded clean tail
command action=stop job_id=<returned>            # kills the whole process tree
```

Agent rules:

- Prefer `command action=run_and_watch` for commands that finish within a
  minute. Compact callers may also send `command action=run` with `wait_ms`;
  TC honors that request through the same `run_and_watch` contract. Plain
  `run` remains immediate; pair it with `command action=wait` for longer jobs.
- A minimal rule is just `{"pattern": "ERROR"}` — id, version, matcher,
  severity, and summary default sanely. `kind` may be the matcher override
  (`regex`/`keyword`) or a natural emitted event label such as `test_result`;
  explicit `event_kind` wins. Severity accepts `error`/`warn`/`fatal` aliases.
- Use `command action=output_tail` for exploratory commands where you don't know what
  to match yet — bounded to 200 lines / 64 KiB, truncation-flagged. Optional
  `strip_ansi` cleans only the returned rendering; stored frames stay raw.
- `files action=search` accepts an absolute file or directory. Directory searches
  recurse deterministically with per-file policy checks, never follow links, and
  remain bounded by match, byte, and entry caps.
- Use `command action=sub_open` + `command action=sub_pull` instead of N polling loops
  when watching several jobs at once.
- `wait_exhausted: true` means STILL RUNNING — call `command action=status`;
  do not treat it as finished. `degraded: true` means follow the `recover_hint`.
- The `cursor` returned by `run_and_watch` is a resume cursor. If `max_signals`
  caps the response, it remains before omitted matches so a later `wait` from
  that cursor recovers them instead of silently skipping evidence.
- Keep interpreters out of the argv lane. For a pipeline or compound command,
  use `command action=exec` only when `status action=policy_status` confirms
  `allow_shell`; otherwise follow a returned `direct_argv`/`wsl_argv` route and
  run each program directly as argv.
- Do not pipe into shell-side `tail`, `head`, or `grep` when Terminal Commander
  needs full evidence. Use rules, `command action=output_tail`, or
  `files action=search` so discarded lines remain observable.
- Do not ask for unbounded output. Every response is intentionally capped.

## MCP Tool Surface

Terminal Commander offers two schema views over the same runtime:

| Surface | Tools advertised | Intended use |
| --- | --- | --- |
| `compact` | `command`, `session`, `files`, `registry`, `status` | Small, stable action-dispatched surface for LLM harnesses. |
| `full` | 51 granular tools | Explicit per-operation names for clients that prefer a broad schema. |

Set the view with `TC_SURFACE=compact|full` or `setup harness --surface ...`.
Unset or unrecognized values select `full`. Compact calls are validated against
the chosen action before they reach the same handlers used by the full surface.

| Compact facade | Responsibilities |
| --- | --- |
| `status` | Discovery, health, policy, audit, runtime/probe state, and targets. |
| `command` | Start/watch/status/stop, shell execution, buckets/context, and subscriptions. |
| `registry` | Search, test, version, activate, deactivate, import, and suggest rules. |
| `files` | Bounded read/search/list/write, file watches, and workspace snapshots. |
| `session` | PTY commands and persistent shell sessions. |

`status action=system_discover` advertises the full live capability catalogue
and per-operation availability. All daemon-backed operations return a structured
`daemon_unavailable` error when the daemon is down instead of leaking raw
pipe/socket errors, and `system_discover` itself remains callable to explain
per-tool availability (`requires_daemon`, `available`, `unavailable_reason`).
When the daemon is reachable it also probes the execution environment with
hard time bounds: OS/architecture, terminal evidence, shell and PowerShell
paths/versions, WSL execution, and core tools. Confirmed interpreters become
ranked `access_routes`; `beachhead` is the highest-ranked route and includes the
exact argv template an LLM can follow. Unavailable or timed-out candidates stay
truthful evidence, never inferred availability. Discovery also carries the
honest `omni_status` capability matrix (see below).

Full contract: [`docs/mcp/TOOL_CONTROL_SURFACE.md`](docs/mcp/TOOL_CONTROL_SURFACE.md).
Agent lane-selection map: [`docs/mcp/OMNI_PLAYBOOK.md`](docs/mcp/OMNI_PLAYBOOK.md).

**Omni capability matrix.** `system_discover.omni_status` reports, honestly
from live state, which omni capabilities are wired on THIS host: `shell_exec`,
`sessions` (unix-only), `pty` (with a `platform` of `posix` / `windows_conpty`
/ `unavailable`), `remote_targets` (count + reachable), and `privileged_helper`
-- which is always `{ available: false, reason: "threat_review_pending" }`
because the privileged helper is plan-only (no code shipped; blocked on a
threat review). The matrix never claims a capability that is not actually
wired.

`health` is a non-bumping, audit-free **peek**: it returns `uptime_secs` plus
optional `idle_secs` and never resets the daemon's idle timer or writes an
audit row. All other IPC requests bump the idle clock and audit normally.

> [!WARNING]
> `shell_exec` exists for pipelines/compounds/redirects, but it is gated by
> the `allow_shell` policy capability, which is **off by default** and lives
> in the operator's config TOML — it is not an MCP-flippable parameter. On the
> default profile, `shell_exec` returns `PolicyDenied`.

> [!CAUTION]
> PTY tools are a dual backend: unix `pty-process` and Windows ConPTY
> (`portable-pty`). `system_discover.omni_status.pty.platform` reports the
> live backend (`posix`, `windows_conpty`, or `unavailable`) per host before
> you call. Honest caveat: ConPTY lifecycle is live-verified on Windows, but
> full live ConPTY child-output end-to-end remains gated behind
> `TC_CONPTY_E2E=1` and is not yet closed on every dev host -- check
> `system_discover` on native Windows before relying on it.

> [!TIP]
> Persistent shell sessions (`shell_session_*`) and workspace snapshots
> (`workspace_snapshot_*`) let an agent run multi-step work that shares
> cwd/env. Availability requires all three gates: a reachable daemon, a UNIX
> session runtime, and `allow_session` enabled (default off). On a non-unix
> daemon they return `UnsupportedPlatform`. See
> [`docs/runtime/SHELL_SESSION.md`](docs/runtime/SHELL_SESSION.md).

## Per-Harness Sessions

Each harness gets a distinct daemon, keyed by an opaque token. The token is
minted by `setup harness` (deterministic per harness id + machine) and emitted
as `env.TC_SESSION` in the harness's MCP stanza.

**Endpoint resolution precedence** (in both the daemon at bind time and every
client at connect time):

1. `TC_SOCKET` (full path/pipe override — operator escape hatch)
2. `TC_SESSION` (opaque token; ASCII `[A-Za-z0-9._-]`, 1–64 chars, ≥1 alphanumeric, `default` reserved)
3. Per-user default (one shared daemon)

Malformed `TC_SESSION` falls back to the per-user default with a stderr
warning — it never names a kernel object.

**On idle, the daemon self-reaps.** Each daemon tracks last-IPC time in memory
and exits gracefully after `TC_IDLE_TTL_SECS` of no real IPC (default 1800
seconds; `0` disables). Health/probe peeks never reset the idle clock.
**Live work defers the reap**: a still-running command, file watch, or PTY job
keeps the daemon alive so children are never orphaned and their receipts,
exit events, and audit rows are never lost. The shutdown path stops accepting
new connections, drains in-flight requests, then exits 0 and removes the
pidfile.

**Inspect and reap sessions:**

```powershell
terminal-commander session list
terminal-commander session reap <token>
terminal-commander session reap --all
```

`session reap` sends a graceful `Shutdown` over IPC and waits for the endpoint
to go unreachable. If a daemon is wedged, the force path is identity-gated by
`pid_belongs_to_daemon` (daemon image + session `state_dir` in the live
cmdline) before any kill signal, and on Unix is re-checked again immediately
before the SIGKILL leg — a PID recycled mid-grace is never signalled.

## Harness Configuration

`terminal-commander setup harness` detects installed harnesses and writes MCP
config for supported providers, minting a per-harness `TC_SESSION`. Use
`--provider` to restrict the write.

| Harness | Server key | Config style | Status |
| --- | --- | --- | --- |
| Cursor | `terminal-commander` | JSON `mcpServers` | Live |
| Codex CLI | `terminal_commander` | TOML `[mcp_servers.terminal_commander]` | Live |
| Claude Code | `terminal_commander` | JSON `mcpServers` | Live |
| Claude Desktop | `terminal_commander` | JSON `mcpServers` | Live |
| Gemini | `terminal_commander` | Stub | Path verification pending |
| Kimi | `terminal_commander` | Stub | Path verification pending |

Generated Cursor stanza (with per-harness session token):

```json
{
  "mcpServers": {
    "terminal-commander": {
      "type": "stdio",
      "command": "terminal-commander-mcp",
      "args": [],
      "env": {
        "TC_SESSION": "tc-<12 hex chars>"
      }
    }
  }
}
```

Re-running `setup harness` for the same provider produces the same token; your
daemon is not churned. A malformed token in the stanza is rejected at write
time by both the JS validator and the Rust resolver.

Guides: [`docs/integrations/cursor.md`](docs/integrations/cursor.md) ·
[`docs/integrations/codex-cli.md`](docs/integrations/codex-cli.md) ·
[`docs/integrations/claude-code.md`](docs/integrations/claude-code.md) ·
[`docs/integrations/README.md`](docs/integrations/README.md)

## Platform Support

| Platform | Package | IPC | Notes |
| --- | --- | --- | --- |
| Linux x64 | `@terminal-commander/linux-x64` | Unix domain socket | Native daemon and MCP adapter |
| Linux arm64 | `@terminal-commander/linux-arm64` | Unix domain socket | Native daemon and MCP adapter |
| Windows x64 | `@terminal-commander/windows-x64` | Named pipe | Native by default; PTY via ConPTY (full child-output e2e gated by `TC_CONPTY_E2E=1`); shell sessions are unix-only |
| macOS x64 | `@terminal-commander/mac-x64` | Unix domain socket | Native package published |
| macOS arm64 | `@terminal-commander/mac-arm64` | Unix domain socket | Native package published |

macOS native packages are published, but the omni platform-parity work for
macOS is code plus a smoke script only -- it is NOT live-verified on a Mac
host (no Mac host available to the program), so treat the macOS runtime as
unverified until a Mac smoke run lands. Likewise, native-Windows ConPTY
child-output e2e is gated behind `TC_CONPTY_E2E=1` and must be run on
CI/desktop to fully close it.

The legacy Windows-to-WSL bridge is still available for operators who
explicitly set `TC_USE_LEGACY_WSL_BRIDGE=1`. It is not the default Windows
path. When the bridge is used, only `TC_SESSION/u` crosses into WSL via
`WSLENV`; the ambient operator `WSLENV` is dropped so credential-shaped vars
cannot cross the trust boundary.

## Admin CLI

| Command | Role |
| --- | --- |
| `terminal-commander` | Admin CLI: status, doctor, setup, session, rules, jobs, probes, policy, audit, update |
| `terminal-commander-mcp` | MCP stdio adapter launched by Cursor/Codex/Claude |
| `terminal-commanderd` | Local daemon for IPC, probes, policy, buckets, audit, and graceful shutdown |

Admin CLI subcommands (`terminal-commander <cmd>`):

| Subcommand | Purpose |
| --- | --- |
| `status` | High-level daemon status (reachable / unavailable). |
| `doctor harness` | Per-provider detection + configuration audit (warns on shared-daemon mode). |
| `doctor daemon` | Native daemon diagnostics (binary, pidfile, endpoint). |
| `doctor wsl` | WSL distro + runtime diagnostics. |
| `setup harness [--provider <id>] [--force]` | Write MCP stanzas (mint + emit `env.TC_SESSION`). |
| `setup daemon-autostart` | Install Linux/WSL daemon autostart (systemd/profile). |
| `session list` | Enumerate sessions (default + seeded), columns: SESSION/PID/STATE/IDLE/ENDPOINT. |
| `session reap [<token>] [--all] [--idle --idle-secs N]` | Graceful Shutdown-IPC; identity-gated force fallback. |
| `rules { list \| show <id> }`, `jobs`, `probes`, `policy`, `audit [--limit N]` | Daemon-backed inspection (exit 69 when daemon unavailable; no fake data). |
| `update` | Run `npm install -g terminal-commander@latest` after a scoped Windows lock preflight. |

The Rust admin CLI does not synthesize fake daemon data. Daemon-backed
inspection commands exit `69` with an `unavailable` message rather than
returning empty or not-found success.

## Doctor And Repair

```powershell
terminal-commander doctor harness
terminal-commander doctor daemon
terminal-commander doctor wsl
terminal-commander session list
```

`doctor harness` warns "shared daemon mode" when multiple harnesses are
present and at least one is not yet configured. Repair is explicit — there is
no hidden auto-repair during npm install:

```powershell
terminal-commander setup harness --force
terminal-commander setup daemon-autostart
terminal-commander session reap --all
```

## Update

```powershell
terminal-commander update
```

`update` runs the same public npm command (`npm install -g
terminal-commander@latest`). On Windows it first runs a native preflight that
terminates only Terminal Commander binaries whose executable path is inside
the current npm platform package `bin` directory — no `cmd.exe`, PowerShell,
`taskkill`, hidden windows, broad process-name matches, or downloaded helper
scripts.

On startup the adapter calls `ensure_daemon`, then `replace_if_stale` when
spawn is allowed — a running daemon older than the installed adapter is
swapped (identity-gated) before tool calls proceed.

## Environment

| Variable | Effect |
| --- | --- |
| `TC_SESSION` | Opaque per-harness session token; selects the endpoint and state subdir. |
| `TC_SOCKET` | Full endpoint override (pipe name / socket path). Wins over `TC_SESSION`. |
| `TC_DATA` | State-dir base override (default: `%LOCALAPPDATA%\terminal-commanderd\state` on Windows, `~/.local/share/terminal-commanderd` on Unix). |
| `TC_IDLE_TTL_SECS` | Idle self-reap TTL in seconds (default 1800; `0` disables). |
| `TC_SURFACE` | MCP schema view: `compact` (five facades) or `full` (51 granular tools; default). |
| `TC_USE_LEGACY_WSL_BRIDGE` | `1` opts into the legacy Windows→WSL bridge. |
| `TC_WSL_DISTRO` | Selects the WSL distro for the legacy bridge. |
| `TC_SKIP_DAEMON_AUTOSTART` | `1` skips daemon autostart during `setup harness`. |

## Local State

Everything lives under the per-session state dir
(`<TC_DATA>/<TC_SESSION>` when a session token is set):

| Path | Contents |
| --- | --- |
| `terminal-commander.toml` | Optional conventional daemon/policy config, auto-loaded when `--config` is omitted. |
| `terminal-commander.db` | SQLite store: events, rule registry (versioned, FTS5), durable activations, audit rows, workspace snapshots. |
| `logs/terminal-commanderd.log` | Daemon log (bind, self-checks, idle-reap decisions). |
| `terminal-commanderd.pid` | Pidfile: pid, version, endpoint (the probe cross-checks it). |
| `terminal-commanderd.lock` | Bring-up single-flight lock. |

## Safety Posture

- npm install is passive; wrapper scripts use direct process spawn with
  `shell:false`; no hidden subprocess windows.
- The MCP adapter speaks stdio and local IPC only — CI guards assert no
  spawn/socket/fs calls in the adapter source.
- Command execution is argv-first and policy-gated; the shell lane is a
  separate, default-off capability with its own audit labels.
- The omni opt-in capabilities are all default-DENY and config-only (never
  MCP-flippable): `allow_shell` (shell_exec), `allow_session` (persistent
  sessions, unix-only), `allow_remote` (remote targets via an operator
  `ssh -L` forward, no public TCP). `allow_privileged` is wired but gates a
  PLAN-ONLY helper -- no privileged code ships (blocked on a threat review;
  see [`docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md`](docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md)).
- Tool responses are bounded JSON, not raw stream dumps; credential-shaped
  argv values are redacted in audit metadata and probe rows.
- `ensure_daemon` requires a real Health handshake — a connectable but
  non-Terminal-Commander socket/pipe (squatter, stale bind, wrong process) is
  rejected, not silently accepted.
- Force-kill on reap/replace is identity-gated at both signal legs; a PID
  recycled mid-grace is never signalled.
- Win→WSL forwarding is a TC-only allowlist (`TC_SESSION/u`); ambient `WSLENV`
  is dropped.
- Daemon idle self-reap reclaims abandoned daemons without an external
  watcher; live work (running commands, watches, PTYs) defers it.
- Stale daemon availability and version-skew state are refreshed through
  bounded Health probes; recovery is single-flight, and mutating calls are
  never replayed automatically.

Security model: [`docs/security/PRIVILEGE_MODEL.md`](docs/security/PRIVILEGE_MODEL.md) and [`SECURITY.md`](SECURITY.md).

## Develop From Source

```powershell
git clone https://github.com/special-place-ai-heaven/terminal-commander.git
cd terminal-commander
```

The PR gate CI runs is `scripts/linux-gate.sh` (plus
`scripts/windows-gate.ps1` for Windows-only regressions) — running them
locally is the same check that gates your PR:

```bash
bash scripts/linux-gate.sh        # linux/mac (or via WSL on Windows)
pwsh scripts/windows-gate.ps1     # Windows-only regression gate
```

Fast inner loop:

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
npm --prefix packages/terminal-commander test
```

Local package testing:

```powershell
cd packages/terminal-commander
npm link
terminal-commander --version
terminal-commander setup --help
terminal-commander session list
```

Testing doctrine: [`TESTING.md`](TESTING.md). Contributor guide:
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## Repository Layout

```text
crates/                                  Rust workspace (9 crates; 8 published to crates.io)
  core/                                  ids, buckets, context rings, events, activation
  sifters/                               rule evaluation + noise dedupe
  probes/                                process / file / PTY probe runtimes
  store/                                 SQLite (events, registry, audit) + FTS5 + rule packs
  supervisor/                            ensure_daemon, replace_if_stale, session tokens, pidfile
  ipc/                                   wire protocol + framing + clients (UDS / named pipe)
  daemon/                                terminal-commanderd — IPC, policy, router, runtimes
  mcp/                                   terminal-commander-mcp — 5-facade compact / 51-tool full surface
  cli/                                   terminal-commander admin CLI (local only, not on crates.io)
packages/
  terminal-commander/                    npm root wrapper (@latest)
  terminal-commander-{linux-x64,linux-arm64,windows-x64,mac-x64,mac-arm64}/
docs/                                    architecture, integrations, audits, release docs
examples/provider-harness/               copy-paste MCP config examples
scripts/                                 CI, release, and smoke helpers
```

## License

Licensed under the [PolyForm Noncommercial License 1.0.0](LICENSE)
(`SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0`). You may inspect,
study, and use the source code for noncommercial purposes. Commercial use
requires a separate license — contact the licensor for commercial terms.
