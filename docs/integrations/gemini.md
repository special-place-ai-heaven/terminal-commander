# Gemini CLI integration

Install Terminal Commander, then register it for Gemini CLI:

```sh
npm install -g terminal-commander@latest
terminal-commander setup harness --provider gemini
```

Setup merges `mcpServers.terminal_commander` into `~/.gemini/settings.json`.
An explicit provider selection also works before Gemini creates that file.
For a project configuration, supply its root:

```sh
terminal-commander setup harness --provider gemini --project /absolute/project/path
```

This writes `<project>/.gemini/settings.json`. On Windows, use an absolute
Windows project path; the global home comes from `%USERPROFILE%`.

The generated stdio entry uses an absolute native adapter path with `args: []`.
If no usable native adapter resolves, bootstrap defers registration and leaves
the configuration untouched; install the platform package and rerun setup.
It preserves unrelated Gemini
settings, other servers, and existing Terminal Commander policy fields and
custom environment variables. Re-registering replaces `command` and `args`
together.

After setup, run `/mcp reload` in Gemini or restart its session. List MCP servers
and call Terminal Commander's `system_discover` to verify the connection;
successful configuration writing alone does not prove a live tool call.

See [harness configuration](harnesses.md) for discovery, backups, and shared
repair behavior. Gemini's [official MCP reference](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md)
describes `mcpServers`, policy settings, and reload commands. This integration
covers Gemini CLI; it does not configure AI Studio.
