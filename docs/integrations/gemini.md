# Gemini integration (stub)

Status: INSTALL01 stub — config path not yet verified for automated writes.

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

On Windows hosts, `terminal-commander-mcp` is the WWS04 bridge shim; the
daemon runs inside WSL.

## Bootstrap behavior today

`npm install -g terminal-commander` on Windows runs harness detection.
If Gemini install markers are not found, the provider is skipped with no
config write. If markers are found but the config path is unverified,
bootstrap logs `gemini: config_path_unverified` on stderr and does not
modify files.

## Operator workaround

1. Complete Windows install: `npm install -g terminal-commander`
2. Start the daemon inside WSL (see [`cursor.md`](cursor.md) §3).
3. Manually add the MCP stanza to your Gemini client's documented config
   location once Google documents MCP server blocks for your channel.

Track verification in a future INSTALL01.1 goal before promoting this
page from stub to authoritative.
