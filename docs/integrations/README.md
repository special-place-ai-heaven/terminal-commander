# Provider Integration Examples - Terminal Commander

Status: current public integration index for the six-facade compact and
60-tool full MCP surfaces. Compact facades: `command`, `files`, `recipe`,
`registry`, `session`, `status`.

Provider-neutral MCP integration recipes. NO secrets, NO machine-
specific paths. The MCP server is launched by the LLM harness as a
child process over rmcp stdio (per `docs/security/PRIVILEGE_MODEL.md`
section 4) and forwards every tool call through the daemon's local IPC endpoint
(Unix domain socket on Unix, named pipe on Windows).

Per-provider walk-throughs:
- [`codex-cli.md`](codex-cli.md) - Codex CLI MCP stdio config (`~/.codex/config.toml`).
- [`claude-code.md`](claude-code.md) - Claude Code MCP stdio config
  (`--mcp-config` flag + persistent settings).
- [`cursor.md`](cursor.md) - Cursor MCP stdio config (native Linux,
  inside-WSL, and legacy Windows-Cursor-to-WSL bridge). Copy-pasteable
  configs in
  [`examples/provider-harness/cursor/`](../../examples/provider-harness/cursor/).
- [`gemini.md`](gemini.md) - Gemini stub (INSTALL01; path unverified).
- [`kimi.md`](kimi.md) - Kimi stub (INSTALL01; path unverified).
- [`omp.md`](omp.md) - OMP (oh-my-pi) MCP stdio config
  (`~/.omp/agent/mcp.json`); setup manages its enabled list.

## MCP protocol floor

The adapter prefers **2026-07-28** and also accepts legacy `initialize`
with **2025-11-25** or **2025-06-18**. Other revisions receive JSON-RPC
`-32022` Unsupported protocol version. A client that sends no handshake at
all (for example a harness reconnecting after an adapter restart) is served
as if it had initialized; stateless 2026-07-28 clients must send `_meta`
on every request.

- **Codex CLI** still sends `2025-06-18` until you opt in
  (`mcp_2026_07_28 = true` plus `CODEX_MCP_PROTOCOL_VERSION = "2026-07-28"`).
  See [`codex-cli.md`](codex-cli.md).
- **Cursor and OMP** use the supported legacy `initialize` path.
- **Claude Code** finishes the 2026-07-28 handshake. Tip `tools/list`
  emits SEP-2549 cache hints `ttlMs: 0` and `cacheScope: "public"`.
  Claude Code uses the preferred **2026-07-28** path. See
  [`claude-code.md`](claude-code.md).
- **OMP** uses its **2025-11-25** handshake; setup writes its server entry.
  See [`omp.md`](omp.md).

Real-Time-Active patterns (react to subscription events as they happen): see the
"Real-Time-Active patterns" section in [`claude-code.md`](claude-code.md) -
`Monitor` over `subscription-stream`, a one-shot backgrounded `subscription_pull`,
`/loop`/`ScheduleWakeup` cadence, and the default-OFF Stop-hook keep-alive
([`packages/terminal-commander/hooks/`](../../packages/terminal-commander/hooks/)).
The universal cross-harness pattern is a background loop over `subscription_pull`.

Operational guides (not provider-specific):
- [`recipe-registry.md`](recipe-registry.md) - argv recipes versus signal
  rules. Happy path: search, operator activate, then `recipe_run`. Prefer
  `recipe_run` or argv over shell. Teach may return `retry_with_recipe`.
  The MCP protocol floor above is unchanged.
- [`omni-harness-smoke.md`](omni-harness-smoke.md) - the operator-gated
  provider-harness smoke procedure for the omni surface (command -> wait ->
  status, a persistent shell-session check, and the suggest-loop check)
  across Cursor / Codex CLI / Claude Code. This is the O-14 provider-trust
  pass, separate from the local daemon+adapter smoke.
- [`wsl-cleanup-and-sudo.md`](wsl-cleanup-and-sudo.md) - run WSL disk/cache
  cleanup through TC; the scoped NOPASSWD sudoers discipline (no password ever
  leaks); the `cleanup` rule pack; `${name}` template syntax; and why fstrim
  frees blocks but the `.vhdx` only shrinks via a manual host-side step.

**Install behavior:** `npm install -g terminal-commander` outside CI attempts
guarded auto-setup for detected harnesses. CI and `TC_NO_AUTO_SETUP=1` skip it.
Run `terminal-commander setup harness` to configure or repair detected
harnesses, or add `--provider cursor`, `--provider codex-cli`,
`--provider omp`, `--provider claude-code`, or `--provider claude-desktop`
to target one harness. Codex setup writes its protocol marker; enabling
Codex's modern feature flag remains optional because its legacy default works.

A local daemon + MCP stdio smoke (no provider in the loop) lives at
[`scripts/smoke/verify-runtime-smoke.sh`](../../scripts/smoke/verify-runtime-smoke.sh).
It is secondary evidence: it proves Terminal Commander's local
transport surface works without a provider. Provider-harness success
requires actually running the provider against one of the configs
above and observing tool calls in the session transcript.

The rest of this page is the older provider-neutral baseline kept for historical
context; the modern full surface advertises 60 tools and the per-provider
walk-throughs above are the authoritative source.

Language: ASCII only.

## 1. Claude Code

See [`claude-code.md`](claude-code.md). Prefer `~/.claude/settings.json`
with server key `terminal_commander`:

```json
{
  "mcpServers": {
    "terminal_commander": {
      "command": "terminal-commander-mcp",
      "args": []
    }
  }
}
```

Verify discovery:

```bash
# Inside Claude Code:
/mcp
# Should list terminal-commander with the live tool surface.
```

Sample prompt (uses bucket_wait):

```text
Start a build via terminal-commander, then wait on the build bucket
for any signal events at severity medium or above. If you see a
compile_error, retrieve event_context around it.
```

## 2. Codex CLI

Codex CLI reads MCP servers from `~/.codex/config.toml` (authoritative
shape in [`codex-cli.md`](codex-cli.md)):

```toml
[mcp_servers.terminal_commander]
command = "terminal-commander-mcp"
args = []
```

That stanza alone is not enough on this tip. Codex still needs the
2026-07-28 opt-in in [`codex-cli.md`](codex-cli.md).

## 3. Generic MCP client

A stdio MCP client works when it negotiates protocol **2026-07-28**. rmcp is
the server implementation, not a promise that every older client handshake
succeeds. Clients that initialize with `2025-11-25` or `2025-06-18` are
accepted; see
[MCP protocol floor](#mcp-protocol-floor). Launch
the binary as a child process; the server speaks MCP on stdout and reads
requests on stdin.

```bash
terminal-commander-mcp 2>terminal-commander-mcp.log
```

stderr carries log lines; stdout is the rmcp transport. Do NOT
pipe stdout through any pretty-printer.

## 4. Core tools (quick reference)

| Tool | Bounded shape | Use |
|---|---|---|
| `system_discover` | small JSON | Probe version / spec / available tools. |
| `bucket_events_since(bucket_id, cursor, severity_min?, kind?, limit?)` | `BucketReadResponse` | Read recent events past a cursor. |
| `bucket_wait(bucket_id, cursor, ..., timeout)` | `BucketWaitResponse` (events OR heartbeat) | Block for matching events; heartbeat on timeout. |
| `bucket_summary(bucket_id)` | `BucketSummary` | Per-bucket counters. |
| `event_context(probe_id, anchor, before, after, max_bytes?)` | `ContextWindowResponse` (frames bounded) | Pull bounded raw frame text around an event. |

## 5. Examples directory

`examples/` ships harness-portable scripts:

- `examples/bucket_wait_demo.md`: walk-through showing how an LLM
  should use bucket_wait to avoid polling.
- `examples/dynamic_rule_demo.md`: walk-through showing the
  registry_create / registry_test / registry_activate flow (TC24
  tools).

These are markdown narratives, not runnable code. They document the
LLM-side prompt + tool-call sequence and the expected response
shapes.

## 6. Source-status

| Component | Status |
|---|---|
| Claude Code stanza | live |
| Codex CLI stanza | live |
| Cursor MCP stanza | live - see [`cursor.md`](cursor.md) + [`examples/provider-harness/cursor/`](../../examples/provider-harness/cursor/) |
| Cursor provider smoke transcript | Not Run (operator-driven; no scripted MCP entry point in Cursor today) |
| Generic MCP-client recipe | live (2026-07-28, 2025-11-25, 2025-06-18) |
| OMP stanza | setup provider; 2025-11-25 handshake supported - see [`omp.md`](omp.md) |
| examples/*.md walk-throughs | live |
| examples/provider-harness/cursor/*.json | live; includes legacy WSL bridge example |
| rmcp stdio adapter wiring | live |
