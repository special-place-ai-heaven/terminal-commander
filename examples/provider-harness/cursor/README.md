# Cursor MCP config examples

These files are copy-paste Cursor MCP examples for Terminal Commander. They are
not active configs and should not be committed into another project without
review.

Authoritative walk-through:
[`docs/integrations/cursor.md`](../../../docs/integrations/cursor.md).

## Files

| File | Scope | Host topology | When to use |
| --- | --- | --- | --- |
| [`mcp.global.native-linux.json`](mcp.global.native-linux.json) | Global | Native Terminal Commander on Linux, Windows, macOS, or WSL | Recommended shape. The filename is historical; the JSON is the same native stdio config used on all supported platforms. |
| [`mcp.project.linux-wsl.json`](mcp.project.linux-wsl.json) | Project | Native Terminal Commander with explicit `TC_SOCKET` | Workspace-local config when the daemon endpoint is non-default. |
| [`mcp.global.linux-wsl.json`](mcp.global.linux-wsl.json) | Global | Manual Windows-to-WSL bridge | Legacy reference only. Use native Windows unless you intentionally installed Terminal Commander inside WSL. |

## Recommended Config

```json
{
  "mcpServers": {
    "terminal-commander": {
      "command": "terminal-commander-mcp",
      "type": "stdio"
    }
  }
}
```

Prefer letting the CLI write this for you:

```powershell
terminal-commander setup harness --provider cursor
```

## Boundary

- No secrets or API keys are included.
- No HTTP or SSE transport is configured.
- npm install is passive; setup writes config only when explicitly run.
- The native Windows path does not require WSL.
- The legacy WSL bridge example is retained for compatibility only.
