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

## WWS04: Windows bridge shim landed (alternative to `mcp.global.linux-wsl.json`)

With WWS04 landed, Windows operators have two working paths to point
Cursor at Terminal Commander:

1. **Manual `wsl.exe` invocation in `mcp.json`** (this directory's
   `mcp.global.linux-wsl.json`). The operator hard-codes the WSL
   distro into the `args` array. This path was the only one
   available before WWS04 and remains valid.
2. **Native `terminal-commander-mcp` invocation** (post-WWS04). After
   `npm install -g terminal-commander` on Windows, `mcp.json` can
   simply call the command name — the WWS04 bridge shim transparently
   spawns `wsl.exe -d <distro> -- bash -lc 'exec terminal-commander-mcp'`
   inside the WSL distro selected by `TC_WSL_DISTRO` env or
   `wsl.exe -l -v` default. Distro names are double-validated (safety
   whitelist + live distro membership) before any spawn; token-shaped
   env vars are stripped from the child env; the shim writes nothing
   to stdout so rmcp framing passes through transparently:

   ```json
   {
     "mcpServers": {
       "terminal-commander": {
         "type": "stdio",
         "command": "terminal-commander-mcp"
       }
     }
   }
   ```

The WWS04 path requires that the WSL-side runtime is installed (`npm
install -g terminal-commander` from within the distro). If it is not,
the bridge short-circuits with a single bounded stderr line + exit
64 (`runtime_missing`). WWS06 will add `terminal-commander setup
cursor-wsl --install-wsl-runtime` to automate that step; until then,
operators install inside WSL by hand.

The Cursor config writer is available as a library at WWS05
(`packages/terminal-commander/lib/cursor/`). It produces the same
stanza as `mcp.global.linux-wsl.json` for the WWS04 bridge path
and merges it into the operator's existing Cursor `mcp.json`
without clobbering unrelated MCP servers.

The WWS06 CLI now wraps it: run
`terminal-commander setup cursor-wsl --print-config` to preview,
`terminal-commander setup cursor-wsl --dry-run` to see the plan,
and `terminal-commander setup cursor-wsl` to apply (use `--force`
to overwrite an existing `terminal-commander` entry). See
`docs/integrations/cursor.md` §11a for the WWS05 writer API
contract and §11c for the WWS06 CLI surface.

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
