# Provider Integration Examples - Terminal Commander

Status: TC46 — updated for the TC45 29-tool surface.

Provider-neutral MCP integration recipes. NO secrets, NO machine-
specific paths. The MCP server is launched by the LLM harness as a
child process over rmcp stdio (per `docs/security/PRIVILEGE_MODEL.md`
section 4) and forwards every tool call through the daemon UDS.

Per-provider walk-throughs:
- [`codex-cli.md`](codex-cli.md) — Codex CLI MCP stdio config.
- [`claude-code.md`](claude-code.md) — Claude Code MCP stdio config
  (`--mcp-config` flag + persistent settings).
- [`cursor.md`](cursor.md) — Cursor MCP stdio config (native Linux,
  inside-WSL, and Windows-Cursor-to-WSL bridge). Copy-pasteable
  configs in
  [`examples/provider-harness/cursor/`](../../examples/provider-harness/cursor/).

A local daemon + MCP stdio smoke (no provider in the loop) lives at
[`scripts/smoke/verify-runtime-smoke.sh`](../../scripts/smoke/verify-runtime-smoke.sh).
It is secondary evidence: it proves Terminal Commander's local
transport surface works without a provider. Provider-harness success
requires actually running the provider against one of the configs
above and observing tool calls in the session transcript.

The rest of this page is the TC27 baseline kept for historical
context; the modern TC45 surface advertises 29 tools and the
per-provider walk-throughs above are the authoritative source.

Language: ASCII only.

## 1. Claude Code

Add a stanza to your `~/.config/claude-code/config.json` (or the
project-local `.claude/config.json`):

```json
{
  "mcpServers": {
    "terminal-commander": {
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
# Should list terminal-commander with the 5 MVP tools.
```

Sample prompt (uses bucket_wait):

```text
Start a build via terminal-commander, then wait on the build bucket
for any signal events at severity medium or above. If you see a
compile_error, retrieve event_context around it.
```

## 2. Codex CLI

Codex CLI reads MCP servers from `~/.codex/mcp_servers.json`:

```json
{
  "terminal-commander": {
    "command": "terminal-commander-mcp",
    "args": []
  }
}
```

Codex's tool discovery surfaces the five MVP tools the same way.

## 3. Generic MCP client

Any MCP client that speaks rmcp 1.7.0 stdio should work. Launch the
binary as a child process; the server emits the MCP initialize
handshake on stdout and reads requests on stdin.

```bash
terminal-commander-mcp 2>terminal-commander-mcp.log
```

stderr carries log lines; stdout is the rmcp transport. Do NOT
pipe stdout through any pretty-printer.

## 4. The five MVP tools (quick reference)

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
| Claude Code stanza | live (TC27) |
| Codex CLI stanza | live (TC27) |
| Cursor MCP stanza | live (NPM08, 2026-05-23) — see [`cursor.md`](cursor.md) + [`examples/provider-harness/cursor/`](../../examples/provider-harness/cursor/) |
| Cursor provider smoke transcript | Not Run (operator-driven; no scripted MCP entry point in Cursor today) |
| Generic MCP-client recipe | live (TC27) |
| examples/*.md walk-throughs | live (TC27) |
| examples/provider-harness/cursor/*.json | live (NPM08) |
| rmcp stdio adapter wiring | reserved (TC23 follow-up) |
