# Kimi integration (stub)

Status: INSTALL01 stub - config path not yet verified for automated writes.

Terminal Commander targets MCP stdio adapters for Moonshot / Kimi coding
agents when those products expose a local MCP configuration file.

## Expected shape (unverified)

When a stable global config file is confirmed, the harness writer will add
a stanza equivalent to:

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

On Windows hosts, `terminal-commander-mcp` uses the native Windows platform
package by default. The legacy WSL bridge is opt-in with
`TC_USE_LEGACY_WSL_BRIDGE=1`.

## Bootstrap behavior today

`npm install -g terminal-commander` is passive and does not run harness
detection. `terminal-commander setup harness` detects providers explicitly. If
Kimi install markers are not found, the provider is skipped. If markers are
found but the config path is unverified, setup reports
`kimi: config_path_unverified` and does not modify files.

## Operator workaround

1. Install: `npm install -g terminal-commander`
2. Run `terminal-commander setup harness` and confirm Kimi remains a stub.
3. Manually add the MCP stanza when Kimi documents MCP server configuration
   for your install channel.

Track verification in a future INSTALL01.1 goal before promoting this
page from stub to authoritative.
