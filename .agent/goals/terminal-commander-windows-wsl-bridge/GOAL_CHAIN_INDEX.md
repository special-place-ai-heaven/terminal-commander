# Goal Chain Index — terminal-commander-windows-wsl-bridge

Status: WWS01 + WWS02 + WWS03 Completed; WWS04..WWS09 Pending.
Branch: `main`.
Successor of: `terminal-commander-npm-distribution` (NPM10 = Completed, bootstrap workflow committed but NOT dispatched; all three packages remain E404 / unpublished).

This chain makes Terminal Commander easy to install and use from Windows 11 + Cursor while keeping the real runtime inside WSL / Linux. It is a **user-experience and install/setup chain**, NOT a runtime feature chain. The TC33–TC48 runtime surface and the NPM01–NPM10 package layout remain the input contracts; this chain does not change runtime behavior, does not add MCP tools, and does not run any publish.

Language: ASCII only.

## Chain summary

| Goal   | Title                                                               | Status   |
|--------|---------------------------------------------------------------------|----------|
| WWS01  | Windows / WSL install UX contract                                   | Completed (commit `6220eb2`; contract at `docs/release/windows-wsl-bridge-contract.md`) |
| WWS02  | Root npm package win32 bridge contract                              | Completed (commit `1da40f3`; resolver bridge_required branch + bounded shim refusals + NPM02 §13b amendment) |
| WWS03  | WSL distro discovery and runtime doctor                             | Completed (commit `ec8441e`; lib/wsl/distro-name.js + detect.js + doctor.js + 50 new Node tests; live Windows doctor PASS) |
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

LOCKED at WWS01 in
[`docs/release/windows-wsl-bridge-contract.md`](../../docs/release/windows-wsl-bridge-contract.md)
(commit `6220eb2`). The 15 binding answers are recorded in §15 of
that contract and reproduced here in summary form:

- D-01 Root npm package `os` widened to `["linux", "win32"]`. Owner: WWS02.
- D-02 Linux platform packages remain `os: ["linux"]` (NPM02 §4.1 unchanged). Owner: WWS02.
- D-03 Windows install ships JS only (no Rust binary). Owner: WWS02.
- D-04 `terminal-commander-mcp` on Windows is a bridge shim
  (`wsl.exe -d <distro> bash -lc terminal-commander-mcp`,
  `shell: false`, argv array, whitelist-validated distro).
  Owner: WWS04.
- D-05 `terminal-commanderd` on Windows refuses with exit 64 +
  one-line "use WSL" message. Owner: WWS02 / WWS04.
- D-06 `setup cursor-wsl` auto-detects WSL distros via `wsl.exe
  -l -v` only. Owner: WWS03.
- D-07 Default-distro pick (the asterisk in `wsl -l -v`); ask
  once on multi-distro hosts; refuse on `--no-interactive`.
  Owner: WWS06.
- D-08 Automatic install inside WSL requires explicit
  `--install-wsl-runtime`. Default prints the exact one-line
  install command and exits non-zero. Owner: WWS06.
- D-09 Pairing is OPTIONAL; pair codes are operator confirmation,
  never security secrets. Owner: WWS06.
- D-10 Windows-side state at `%LOCALAPPDATA%\terminal-commander\setup.json`
  (+ optional `pair.json`). Schema in contract §11. Owner: WWS06.
- D-11 WSL-side runtime config unchanged
  (`$XDG_STATE_HOME/terminal-commander/`). Owner: (unchanged).
- D-12 Multi-distro handling via persisted choice +
  `--distro` override. Owner: WWS03 / WWS06.
- D-13 Cursor config default `--global`; `--project` opts into
  workspace scope; refuse-existing-terminal-commander-entry
  without `--force`; always `.bak` backup. Owner: WWS05 / WWS06.
- D-14 Rollback / uninstall via `setup cursor-wsl --uninstall`
  (restores `mcp.json.bak`) BEFORE `npm uninstall`. Owner: WWS06
  + WWS08.
- D-15 Publish-readiness floor: WWS02 + WWS04 + WWS05 + WWS06 +
  WWS08 must land before the first npm publish. WWS01
  RECOMMENDS keeping `npm-bootstrap-publish.yml` PAUSED until
  WWS08 (at minimum). WWS09 reconfirms. Owner: WWS09.
