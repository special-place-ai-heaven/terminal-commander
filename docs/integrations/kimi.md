# Kimi integration (stub)

Status: INSTALL01 stub — config path not yet verified for automated writes.

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

On Windows hosts, `terminal-commander-mcp` is the WWS04 bridge shim.

## Bootstrap behavior today

`npm install -g terminal-commander` on Windows runs harness detection.
If Kimi install markers are not found, the provider is skipped. If markers
are found but the config path is unverified, bootstrap logs
`kimi: config_path_unverified` on stderr and does not modify files.

## Operator workaround

1. Complete Windows install: `npm install -g terminal-commander`
2. Start the daemon inside WSL (see [`cursor.md`](cursor.md) §3).
3. Manually add the MCP stanza when Kimi documents MCP server configuration
   for your install channel.

Track verification in a future INSTALL01.1 goal before promoting this
page from stub to authoritative.
