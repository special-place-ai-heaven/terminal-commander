# Goal Chain Index — terminal-commander-windows-wsl-bridge

Status: skeleton (WWS00 init).
Branch: `main`.
Successor of: `terminal-commander-npm-distribution` (NPM10 = Completed, bootstrap workflow committed but NOT dispatched; all three packages remain E404 / unpublished).

This chain makes Terminal Commander easy to install and use from Windows 11 + Cursor while keeping the real runtime inside WSL / Linux. It is a **user-experience and install/setup chain**, NOT a runtime feature chain. The TC33–TC48 runtime surface and the NPM01–NPM10 package layout remain the input contracts; this chain does not change runtime behavior, does not add MCP tools, and does not run any publish.

Language: ASCII only.

## Chain summary

| Goal   | Title                                                               | Status   |
|--------|---------------------------------------------------------------------|----------|
| WWS01  | Windows / WSL install UX contract                                   | Pending  |
| WWS02  | Root npm package win32 bridge contract                              | Pending  |
| WWS03  | WSL distro discovery and runtime doctor                             | Pending  |
| WWS04  | Windows bridge MCP shim                                             | Pending  |
| WWS05  | Cursor config writer                                                | Pending  |
| WWS06  | WSL runtime install or pairing flow                                 | Pending  |
| WWS07  | End-to-end Windows + Cursor + WSL smoke                             | Pending  |
| WWS08  | README and release contract update                                  | Pending  |
| WWS09  | Pre-publish readiness review                                        | Pending  |

## Target user experience (assumption — must be locked at WWS01)

```powershell
# Windows
npm install -g terminal-commander
terminal-commander setup cursor-wsl
```

```sh
# WSL (one-time, or automated by Windows setup with explicit consent)
npm install -g terminal-commander
terminal-commander doctor
```

Cursor MCP config (written by `setup cursor-wsl`, or copied from
`examples/provider-harness/cursor/`):

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

On Windows, `terminal-commander-mcp` is a **bridge shim**. The shim
internally invokes:

```text
wsl.exe -d <distro> bash -lc "terminal-commander-mcp"
```

Inside WSL, the REAL `terminal-commander-mcp` talks to the
`terminal-commanderd` daemon over the local UDS. The LLM / Cursor
sees one MCP server; the setup complexity is hidden.

## Hard product boundary (must be honored or amended)

- **No Windows-native runtime claim.** The daemon and probes are
  Unix-only; the runtime chain (TC44 `non_goals`) explicitly
  defers Windows ConPTY + Windows-native PTY work.
- **Windows package is a bridge / setup surface only.** It ships
  JS shims + setup helpers; it does NOT ship a Rust daemon for
  win32.
- **WSL / Linux package remains the real daemon / probe / runtime
  host.** The two existing platform packages
  (`@terminal-commander/linux-x64`, `@terminal-commander/linux-arm64`)
  carry the real binaries; that contract is unchanged.
- **Cursor / LLM sees exactly ONE MCP entrypoint.** `terminal-commander-mcp`.
  No raw shell exposed; no second tool; no manual command-string maintenance.
- **No network listener.** Every transport remains local stdio +
  local UDS.
- **No raw stream endpoint.** The 29-tool TC45 catalogue stays
  bounded.
- **No root shell.** The shim only `spawn`s `wsl.exe` with a
  fixed argv shape; no shell interpolation.
- **No secrets / tokens / private usernames / private absolute
  paths** in any committed artifact.
- **No `Not Run` promotion to PASS.**
- **Pairing code is OPTIONAL.** Default install relies on
  `wsl.exe` automatic invocation; the six-digit pairing flow is a
  manual fallback / anti-misconfiguration aid, not a security
  secret.

## Out of chain (deferred)

- macOS-native bridge or Mac WSL2 equivalent (no Mac WSL today).
- Native Windows daemon — DEFERRED until TC44 follow-up replaces
  the UDS / PTY assumptions.
- Cursor extension / plugin auto-install — out of scope; this
  chain writes JSON config only.
- Codex CLI and Claude Code Windows-side bridge — only documented
  if the Cursor bridge pattern generalizes cleanly. Defaults to
  Linux/WSL provider walk-throughs already shipped at TC46.
- crates.io / cargo publish — still out of scope (TC31 / TC48
  baseline).
- Standing `NPM_TOKEN_TC` use — still rejected. The NPM10 bootstrap
  workflow remains a one-time fallback.

## Cross-chain invariants (inherited)

- TC48 + NPM09 + NPM10 `Conditional Go` beta posture preserved
  through this chain. Promotion to `Go` still requires at least
  one provider live smoke transcript.
- MCP must not spawn commands directly inside `crates/mcp` (guard
  greps remain clean).
- MCP must not read files directly inside `crates/mcp/src`.
- No `postinstall` downloader.
- No release-please / publish workflow change at WWS01–WWS08
  unless WWS02 explicitly amends NPM02 package contract under a
  recorded prep amendment.
- All long-lived tokens (`NPM_TOKEN_TC`, `CARGO_REGISTRY_TOKEN_TC`,
  `RELEASE_PLEASE_TOKEN_TC`) stay unused.

## Open decisions to lock at WWS01

- Whether Windows setup may install Terminal Commander inside WSL
  automatically, or only print copy-pasteable commands.
- Whether the setup command must ask before running `npm install`
  inside WSL.
- Whether a pairing manifest is needed (both sides), or whether
  `wsl.exe` invocation is enough on its own.
- Where to store the Windows-side pairing / setup state file
  (e.g. `%LOCALAPPDATA%\terminal-commander\` or `%APPDATA%\`).
- Where to store the WSL-side runtime config (existing
  `$XDG_STATE_HOME/terminal-commander/` placeholder).
- Whether the root `package.json` `os` field must be widened to
  include `"win32"`, and whether `optionalDependencies` remain
  Linux-only.
- How to handle hosts with multiple WSL distros (pick default,
  ask once, persist choice).
- Whether Cursor config should be written to the global path
  (`%USERPROFILE%\.cursor\mcp.json`) or to a workspace-scoped
  `.cursor/mcp.json`, and what to do if a file already exists.
