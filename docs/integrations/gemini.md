# Gemini integration (stub)

Status: INSTALL01 stub - config path not yet verified for automated writes.

Terminal Commander targets MCP stdio adapters the same way as Cursor and
Codex CLI. Google Gemini CLI / AI Studio MCP configuration paths vary by
release channel.

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
Gemini install markers are not found, the provider is skipped with no config
write. If markers are found but the config path is unverified, setup reports
`gemini: config_path_unverified` and does not modify files.

## Operator workaround

1. Install: `npm install -g terminal-commander`
2. Run `terminal-commander setup harness` and confirm Gemini remains a stub.
3. Manually add the MCP stanza to your Gemini client's documented config
   location once Google documents MCP server blocks for your channel.

Track verification in a future INSTALL01.1 goal before promoting this
page from stub to authoritative.
