# INSTALL01 — Explicit setup harness contract

Status: Superseded by AV-safe explicit setup posture.
Branch: `main`.
Date: 2026-05-23.
Supersedes: WWS01 §2.1 (two-step happy path), WWS01 §8 / D-08 (opt-in WSL install only).
Preserves: NPM02 no install/postinstall bootstrap, TC44 native runtime, WWS04 bridge shim rules without hidden-window options.

Language: ASCII only.

## 1. Operator contract (Windows)

The primary Windows operator path is intentionally two explicit operator steps:

```powershell
npm install -g terminal-commander
terminal-commander setup harness
```

`npm install -g terminal-commander` MUST be passive:

1. No `install`, `preinstall`, or `postinstall` lifecycle script.
2. No MCP config writes.
3. No WSL probing, nested `npm install`, daemon autostart, or process spawning.
4. No CMD, PowerShell, hidden-window, taskkill, downloaded helper, or broad process-control behavior.

`terminal-commander setup harness` is the explicit config-writing command. It writes only detected provider MCP config files, creates backups before overwrite, and reports structured status. Current generated MCP stanzas use an executable `node` command plus a package `.js` shim in `args`; they do not use npm, CMD, or PowerShell as the MCP command.

First MCP connect MUST NOT perform hidden lazy bootstrap. If setup was not run, the harness reports the missing configuration/runtime state honestly.

Opt-out:

- `TC_SKIP_BOOTSTRAP=1` is retained only as a compatibility no-op for older call sites.
- `TC_SKIP_DAEMON_AUTOSTART=1` to skip daemon service/profile install only.
- `npm install -g terminal-commander --ignore-scripts` is safe but unnecessary because the package has no lifecycle scripts.

## 2. Operator contract (Linux / WSL)

`npm install -g terminal-commander` on Linux or inside WSL is also passive. Run `terminal-commander setup harness` explicitly to write MCP provider config.

Daemon startup is a runtime concern, not npm install work. Setup commands may report what they would do, but they must not hide daemon/profile/service installation inside npm lifecycle hooks.

## 3. npm lifecycle (NPM02 amendment)

| Rule | Locked |
|------|--------|
| `preinstall` / `install` / `postinstall` lifecycle script | **Forbidden**. |
| Postinstall downloader | **Forbidden** (GitHub Releases fetch or any other hidden network fetch). |
| Network from install script | **Forbidden** because install scripts are forbidden. |
| stdout/stderr from install script | **Forbidden** because install scripts are forbidden. |

## 4. Harness registry (INSTALL01 scope)

Full registry at `packages/terminal-commander/lib/harness/registry.js`.

| Provider | Config | Format |
|----------|--------|--------|
| `cursor` | global `.cursor/mcp.json` | JSON |
| `codex-cli` | `~/.codex/config.toml` | TOML `[mcp_servers.terminal_commander]` |
| `claude-code` | `~/.claude/settings.json` | JSON `mcpServers` |
| `claude-desktop` | App Support `claude_desktop_config.json` | JSON |
| `gemini` | stub until path verified | — |
| `kimi` | stub until path verified | — |

`cursor-cli` is reserved; not written at INSTALL01.

## 5. WSL runtime ensure (supersedes D-08 default)

On Windows global bootstrap, WSL runtime install is **ON by default**.

- Command: locked constant `npm install -g terminal-commander` inside `bash -lc`, with `PATH` stripped of Windows `nodejs` / `npm` shims before Linux paths.
- NO `sudo`. NO password prompts. NO operator argv interpolation into `bash -lc`.
- After install: verify `command -v terminal-commander-mcp` and optional platform package resolution inside WSL.
- `--install-wsl-runtime` on `setup cursor-wsl` remains an alias for the same ensure path.

## 6. Lazy bootstrap (MCP bridge)

When `spawnWslBridge` sees `runtime_missing` and `TC_SKIP_BOOTSTRAP !== "1"`:

- Acquire `%LOCALAPPDATA%\terminal-commander\bootstrap.lock`.
- Run the same `ensureWslRuntime` once.
- Retry doctor; then existing refusal paths.

## 7. Deprecation

- `terminal-commander setup cursor-wsl` prints a migration notice and delegates to harness bootstrap (Cursor included).
- Preferred: `terminal-commander setup` or `terminal-commander setup harness`.

## 8. Cross-links

- [`windows-wsl-bridge-contract.md`](windows-wsl-bridge-contract.md) — bridge shim (unchanged).
- [`npm-binary-packaging-contract.md`](npm-binary-packaging-contract.md) — optionalDependencies layout.
- [`../integrations/README.md`](../integrations/README.md) — per-provider stanzas.
