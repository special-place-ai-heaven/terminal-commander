<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 The Terminal Commander Authors -->

# Scoop manifest (skeleton)

`terminal-commander.json` is a [Scoop](https://scoop.sh) app manifest skeleton
for installing Terminal Commander on Windows **per-user, with no admin rights**.

Scoop installs into the current user's profile (`~/scoop`) with no elevation,
which matches Terminal Commander's user-space-only model. A developer installing
their own tools with Scoop is ordinary, no-admin behavior — there is no service,
no certificate, and no allowance involved.

## What ships

The manifest exposes the three native Windows binaries on the user's Scoop
shim PATH:

- `terminal-commander.exe` (admin/inspection CLI)
- `terminal-commander-mcp.exe` (stdio MCP server)
- `terminal-commanderd.exe` (daemon)

After install, configure an MCP client with `terminal-commander setup harness`,
and optionally pre-start the daemon at logon (per-user, no-admin) with
`terminal-commander setup daemon-logon` (remove with `--uninstall`).

## Placeholders / follow-up (NOT wired yet)

This is the manifest skeleton only. `version`, `url`, and `hash` are
placeholders. Publishing it is a separate CI/infra task that needs:

1. **A GitHub-release zip asset** of the Windows binaries at the templated
   `url` (`.../releases/download/v<version>/terminal-commander-windows-x64.zip`),
   so Scoop can download and SHA256-verify it. The release pipeline does not yet
   produce that zip asset.
2. **A Scoop bucket repo** to host this manifest, so users can
   `scoop bucket add <bucket>` then `scoop install terminal-commander`.

`checkver` + `autoupdate` are pre-wired so that, once the release zip exists,
`scoop update` can pick up new tags automatically.

To finish a release manually before CI lands: set `version`, point `url` at the
published zip, and replace `hash` with its SHA256.
