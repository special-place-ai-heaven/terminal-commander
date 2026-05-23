# INSTALL01 — Install bootstrap and harness auto-config contract

Status: INSTALL01 deliverable.
Branch: `main`.
Date: 2026-05-23.
Supersedes: WWS01 §2.1 (two-step happy path), WWS01 §8 / D-08 (opt-in WSL install only).
Preserves: NPM02 no postinstall **downloader**; TC44 Unix-only runtime; WWS04 bridge shim rules.

Language: ASCII only.

## 1. Operator contract (Windows)

After this chain lands, the primary Windows operator path is:

```powershell
npm install -g terminal-commander
```

That single command MUST (zero-touch — no `setup` subcommands required):

1. Run the package `install` lifecycle script (local-only; see §3).
2. Detect WSL, resolve a distro, and run `npm install -g terminal-commander` **inside** the distro with a Linux-first `PATH` (see §5).
3. Install **daemon autostart** (systemd user unit or profile hook) and **start the daemon** if not already running.
4. For each **detected** harness, **merge or update** the Terminal Commander MCP stanza (`--force` semantics on install; always creates `.bak` before overwrite).
5. Persist bootstrap metadata in `%LOCALAPPDATA%\terminal-commander\setup.json`.
6. Exit **0** from npm even when WSL is missing (fail-soft with one stderr line).

**First MCP connect** (lazy bootstrap): if anything above was skipped (e.g. lock contention), the Windows→WSL bridge runs the same bootstrap once before spawning `terminal-commander-mcp`.

Opt-out:

- `TC_SKIP_BOOTSTRAP=1` before install.
- `TC_SKIP_DAEMON_AUTOSTART=1` to skip daemon service/profile install only.
- `npm install -g terminal-commander --ignore-scripts` (manual recovery only).

## 2. Operator contract (Linux / WSL)

`npm install -g terminal-commander` on Linux or inside WSL:

- Runs harness detect + config merge only (no WSL nested install).
- After harness merge, installs **daemon autostart** inside WSL (systemd
  user unit when available, else profile hook). Opt-out:
  `TC_SKIP_DAEMON_AUTOSTART=1` or `TC_BOOTSTRAP_START_DAEMON=0`.
- On Linux-native global install, same autostart install runs locally.

## 3. npm lifecycle (NPM02 amendment)

| Rule | Locked |
|------|--------|
| `postinstall` downloader | Still **forbidden** (GitHub Releases fetch). |
| `install` script | **Allowed** when it performs only: local filesystem writes, bounded `wsl.exe` spawn for in-distro `npm install -g`, harness detection, stderr logging. |
| Network from install script | Only the in-distro `npm install -g` (registry.npmjs.org), not artifact download from GitHub Releases. |
| stdout from install script | **Forbidden** (stderr only). |

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
