# Cursor MCP config examples — Terminal Commander

This directory ships **copy-pasteable Cursor MCP config examples**
for Terminal Commander. Operators copy intentionally into their own
Cursor config scope; no active `.cursor/mcp.json` is committed in
this repository.

Authoritative walk-through:
[`docs/integrations/cursor.md`](../../../docs/integrations/cursor.md).

Language: ASCII only.

## Files

| File | Cursor scope | Host topology | When to use |
|------|--------------|---------------|-------------|
| [`mcp.global.native-linux.json`](mcp.global.native-linux.json) | Global (`~/.cursor/mcp.json`) | Linux Cursor running natively (or Cursor running inside WSL) | The simplest path. The daemon must already be running and `terminal-commander-mcp` must be on `$PATH`. |
| [`mcp.project.linux-wsl.json`](mcp.project.linux-wsl.json) | Project (`<workspace>/.cursor/mcp.json`) | Linux Cursor or inside-WSL Cursor | Scope the MCP server to one workspace. Includes the `TC_SOCKET` env block for non-default daemon `$TC_DATA`. |
| [`mcp.global.linux-wsl.json`](mcp.global.linux-wsl.json) | Global (`%USERPROFILE%\.cursor\mcp.json`) | Windows Cursor invoking the WSL-hosted adapter via `wsl.exe` | The required topology for Windows operators. Substitute your WSL distro name for `Ubuntu-24.04`. |

## How to use

1. **Pick the file** that matches your host topology from the table
   above.
2. **Copy it** to your Cursor MCP config path:
   - Linux global: `~/.cursor/mcp.json`
   - Project: `<workspace>/.cursor/mcp.json`
   - Windows global: `%USERPROFILE%\.cursor\mcp.json`
3. **Start the daemon** inside Linux / WSL (see
   [`docs/integrations/cursor.md`](../../../docs/integrations/cursor.md)
   §3). The daemon owns the UDS that `terminal-commander-mcp`
   connects to; the MCP adapter does NOT auto-start the daemon.
4. **Open / restart Cursor**, then verify MCP discovery in
   `Settings → Features → MCP`. The `terminal-commander` server
   should show "Connected".
5. **Try a tool call** from the Cursor chat panel:
   > Ask Cursor: "Call the `health` MCP tool."
   The response is a bounded JSON envelope; no raw stream text.

## Boundary statement (mirrors `docs/integrations/cursor.md` §10)

- Every example exposes ONLY `terminal-commander-mcp`. No raw shell
  is bridged.
- No HTTP / SSE transport is configured. Terminal Commander remains
  local stdio over the WSL UDS.
- No environment secrets, API keys, or credentials are in any
  example. The `env` block (when present) only carries the
  non-secret `TC_SOCKET` path.
- No auto-run permissions. The operator confirms each tool call in
  the Cursor chat panel.
- No `postinstall` downloader; no Rust compile during `npm install`;
  no Mac / Windows-native package claim; no musl / Alpine claim.

## Source status

- Files created at NPM08 (2026-05-23).
- The published-npm install path
  (`npm install -g terminal-commander`) is **pending** the first
  live publish; see
  [`docs/release/npm-trusted-publishing-contract.md`](../../../docs/release/npm-trusted-publishing-contract.md)
  §14 for the blocking operator preconditions.
- Until then, the local-tarball pre-publish path
  ([`scripts/smoke/verify-npm-local-install.sh`](../../../scripts/smoke/verify-npm-local-install.sh))
  produces the same binaries.
- Cursor provider live smoke is **operator-driven** and at NPM08
  close is recorded as **Not Run**.
