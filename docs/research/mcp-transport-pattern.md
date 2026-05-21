# MCP Transport Pattern: Two-Process Architecture

Topic: B3
Author: research agent R1-alpha
Date: 2026-05-21
Confidence: medium

## Recommendation

For MVP: two-process pattern, with IPC over a Unix domain socket on
Linux/macOS/WSL and a Windows named pipe on Windows native (Windows
native is deferred per platform decision, but the abstraction should
be designed to accommodate it later). MCP transport between
`terminal-commander-mcp` and the LLM harness is stdio. Daemon
transport is `interprocess` v2 local sockets.

## Design choice

```text
LLM harness  <-- stdio MCP --> terminal-commander-mcp
                                       |
                                       | local-socket JSON-RPC (or HTTP)
                                       v
                              terminal-commanderd
                                       |
                                       +--> probes, sifters, store
```

The split exists for three reasons:

1. The MCP server is short-lived per agent session (started by the
   harness with `command stdio`); the daemon is long-lived and owns
   probes, file watchers, sifter rules, and the event store.
2. Multiple agents can attach to the same daemon. A single per-host
   daemon enables one source of truth.
3. The daemon can run under tighter privileges than the MCP server,
   per README safety model: "MCP server should be unprivileged where
   possible. Daemon/helper may be privileged only when configured."

Source: README "Safety model" section,
`C:\AI_STUFF\PROGRAMMING\terminal-commander\README.md`, lines 284-298.

## Prior-art comparison: other local-daemon MCP servers

| Project | Language | Process model | Transport |
|---|---|---|---|
| filesystem MCP server | TypeScript / Node | single | stdio |
| github-mcp-server | Go | single process (also remote HTTP variant) | stdio (local) or HTTP |
| container-use | Go | single process | stdio |

Sources:

- https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem
- https://github.com/github/github-mcp-server
- https://github.com/dagger/container-use

All three reference servers are single-process. The two-process split
in Terminal Commander is intentional and not standard MCP practice;
it is justified by long-lived probes and the privilege-separation
goal. This is a project-specific choice, not an industry convention.

REQUIRES USER DECISION: confirm two-process split is desired. If a
single-process variant is acceptable for MVP (skipping multi-agent
attach and privilege separation), the architecture simplifies
substantially.

## IPC options for MCP-to-daemon

### Option 1 (recommended): local sockets via `interprocess` v2

- Crate: `interprocess = "2.4.2"`.
- MSRV: 1.75 (lower than the rmcp MSRV floor, so non-binding).
- License: 0BSD OR Apache-2.0.
- Tokio support: yes via `tokio` feature.
- Linux / macOS / WSL: Unix domain socket at e.g.
  `${XDG_RUNTIME_DIR}/terminal-commander/daemon.sock`
  or `~/.local/state/terminal-commander/daemon.sock` if XDG is unset.
- Windows native: named pipe at e.g.
  `\\.\pipe\terminal-commander-<user>`.

Source: https://lib.rs/crates/interprocess

Wire protocol over the socket: JSON-RPC 2.0 (mirrors MCP). The
`terminal-commander-mcp` process acts as both an MCP server (stdio,
to the LLM harness) and a JSON-RPC client (local socket, to the
daemon).

Trade-off: simpler than HTTP, no port collision, supports local
filesystem ACLs for access control.

### Option 2: HTTP/1.1 over loopback

- Use axum/hyper to expose a JSON-RPC or REST endpoint on
  `127.0.0.1:<ephemeral_port>`.
- Pros: easy to inspect with curl; easy to attach health endpoints;
  the same transport works on every platform without conditional code.
- Cons: needs a port; needs an explicit auth token because TCP cannot
  rely on filesystem ACLs; risk that other local processes connect.

Rejected as primary. Consider only if `interprocess` shows a
platform-specific bug or feature gap.

### Option 3: HTTP over Unix domain socket

- Hyper supports UDS via the `hyper-util` or `hyperlocal` adapters.
- Pros: HTTP semantics on a UDS file.
- Cons: heavier than raw JSON-RPC for a single internal RPC channel;
  more deps; no clear benefit for the MVP.

Rejected as primary.

## Local socket path and permissions

Path:

- Linux: `${XDG_RUNTIME_DIR}/terminal-commander/daemon.sock` if
  `XDG_RUNTIME_DIR` is set (typical on modern desktops); otherwise
  `${HOME}/.local/state/terminal-commander/daemon.sock`.
- macOS: `${HOME}/Library/Application Support/terminal-commander/daemon.sock`.
- WSL2: same as Linux. WSL `XDG_RUNTIME_DIR` is usually `/run/user/$UID`
  when systemd is enabled, otherwise unset and fallback applies.

Permissions: socket file mode `0700` (owner-only). The directory
containing the socket should also be `0700`. No additional auth token
is required when the socket lives in a per-user directory with these
permissions.

REQUIRES USER DECISION: whether to keep a per-user daemon
(simpler, the recommendation) vs a per-machine daemon
(needs a shared path and explicit auth).

## MCP transport between mcp-process and harness

For MVP: stdio only. This matches every reference MCP server and is
how harnesses (Claude Code, Codex CLI, etc.) attach.

Streamable HTTP is supported by rmcp 0.16.0 and useful later for
remote/multi-tenant scenarios. Out of MVP scope.

## File layout implications

The split implies two binary crates:

- `terminal-commander-mcp` - thin client that opens an MCP stdio
  session, forwards calls to the daemon over the local socket, and
  bridges streaming responses (especially `bucket_wait`).
- `terminal-commanderd` - the daemon proper.

Both consume `terminal-commander-core` for shared types. This matches
the crate layout already in the README.

## Confidence

Medium. The split is sound and justified by privilege separation +
multi-agent attach, but it is a project-specific decision rather than
an industry pattern (all three prior-art MCP servers reviewed are
single-process). The IPC choice (`interprocess` v2 local sockets)
is high confidence; the architectural split itself REQUIRES USER
DECISION.

## HALT-worthy findings

None. The two-process pattern is implementable today; no blocker
identified.

## SOURCE_MAP reclassification

- Two-process split is project-specific design, not external evidence;
  remains inference.
- IPC via local sockets (UDS / named pipes) using `interprocess` v2
  with tokio: evidence-backed via
  https://lib.rs/crates/interprocess
  ("Currently, the only supported async runtime is Tokio.").
