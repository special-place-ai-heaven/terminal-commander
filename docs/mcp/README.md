# MCP Tool Surface — Terminal Commander

`terminal-commander-mcp` is the thin LLM-facing adapter. It serves MCP over
stdio with rmcp 1.8.0 and forwards tool calls to `terminal-commanderd` over the
local IPC endpoint. The adapter does not spawn commands, open user files, or
bind a network listener.

## Surfaces

- The compact surface exposes five task-oriented facades: `command`, `files`,
  `registry`, `session`, and `status`.
- The full surface exposes the granular tools behind those facades.
- `system_discover` is the runtime authority for available methods, policy,
  host probes, ranked access routes, and the current beachhead.

Set `TC_SURFACE=compact` or `TC_SURFACE=full` before starting the adapter. The
default is documented in the root [README](../../README.md).

## Safety and output contract

Authorization is enforced by the daemon policy engine. Command, file, probe,
session, registry, and remote-target operations retain their existing policy
gates regardless of which MCP surface invokes them.

Responses are structured and bounded. Command output is sifted into signals;
raw output remains behind bounded tail/context calls. Bucket reads use cursors,
file reads and searches have byte/result caps, and long waits have explicit
deadlines.

The authoritative per-tool contract is
[TOOL_CONTROL_SURFACE.md](TOOL_CONTROL_SURFACE.md). Installation and client
configuration live in the root [README](../../README.md) and
[integration recipes](../integrations/README.md).
