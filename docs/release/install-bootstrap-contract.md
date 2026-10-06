# INSTALL01 — Explicit setup harness contract

Status: Current. The passive-install rule was replaced by guarded postinstall auto-setup (section 1).
Branch: `main`.
Date: 2026-05-23.
Supersedes: WWS01 §2.1 (two-step happy path), WWS01 §8 / D-08 (opt-in WSL install only).
Preserves: TC44 native runtime, WWS04 bridge shim rules without hidden-window options.

Language: ASCII only.

## 1. Operator contract (all platforms)

```powershell
npm install -g terminal-commander
terminal-commander setup harness
```

The package has one lifecycle script, `postinstall` (`packages/terminal-commander/scripts/postinstall.js`). It is fail-soft: it always exits 0, so it can never fail `npm install`.

History: this contract first required a passive install with no lifecycle script (2026-05-26). 393f89c (2026-06-25) replaced that rule with guarded, fail-soft auto-setup. The install may run the same bootstrap as `setup harness`, but only through that guarded `postinstall`. The `postinstall` script itself spawns no process and only delegates. Under CI or an opt-out it is a no-op that exits 0. Like all runtime JS, it never requests a hidden window and never invokes CMD or PowerShell. `packages/terminal-commander/test/av-safe-install-runtime.test.js` enforces these constraints.

It does nothing when any of these hold:

- `TC_NO_AUTO_SETUP=1` or `TC_SKIP_BOOTSTRAP=1`.
- A CI environment (`CI=true` or `CI=1`, or a provider variable such as `GITHUB_ACTIONS`).
- It is not running as npm's `postinstall` lifecycle event.
- Another bootstrap holds the bootstrap lock.

Otherwise it runs the same bootstrap as `terminal-commander setup harness`, in fail-soft mode:

1. Stops running Terminal Commander processes of this install and removes npm leftover folders, so an upgrade is not blocked (`packages/terminal-commander/lib/bootstrap/release_instances.js`).
2. Windows with the WSL runtime selected (`TC_WSL_DISTRO` or `TC_USE_LEGACY_WSL_BRIDGE=1`): runs the WSL steps in section 5. Otherwise the native Windows path is used and WSL is not touched.
3. Linux / WSL: installs daemon autostart (a systemd user unit, or a profile hook plus a background start), unless `TC_SKIP_DAEMON_AUTOSTART=1` or `TC_BOOTSTRAP_START_DAEMON=0`. The profile hook runs only in interactive shells. `autostart.sh` starts nothing when `TC_DAEMON_CHILD`, `TC_SOCKET` or `TC_SESSION` is set (a Terminal Commander process tree), when `$TC_DATA/terminal-commanderd.sock` exists, or when `$TC_DATA/terminal-commanderd.pid` names a live `terminal-commanderd`. `TC_DATA` defaults to `~/.local/share/terminal-commanderd`.
4. Writes the MCP config of every detected harness, pointing at the native MCP executable (a stable per-user copy where possible), with backups before overwrite.

npm does not run `postinstall` with `--ignore-scripts` or `ignore-scripts=true`, or when npm's install-script policy (`allow-scripts` / `allowScripts`, npm 11) does not allow `terminal-commander`. The install still succeeds. Nothing is configured or repaired until the operator runs:

- `terminal-commander setup` (or `setup harness`): the same steps, reporting failures instead of swallowing them, without the process release.
- `terminal-commander update`: runs `npm install -g terminal-commander@latest`, then re-runs `setup harness` with the new launcher.

`setup daemon-autostart` reinstalls only the daemon autostart.

Generated MCP stanzas never use npm, CMD, or PowerShell as the MCP command. No step opens a CMD, PowerShell, or hidden window, downloads a helper, or uses taskkill.

## 2. First MCP connect

The MCP shims and the legacy WSL bridge never run bootstrap, install, or config writes. A missing runtime or configuration is reported, for example as `runtime_missing`, and `terminal-commander setup` repairs it.

## 3. npm lifecycle

| Rule | Current |
|------|--------|
| `preinstall` / `install` lifecycle script | None. |
| `postinstall` lifecycle script | Present, fail-soft, exits 0 (section 1). |
| Postinstall downloader | **Forbidden** (no GitHub Releases or other binary fetch; native binaries come from `optionalDependencies`). |
| Network from install script | Only the WSL runtime `npm install -g` of section 5 (Windows with the WSL runtime selected). |
| stdout/stderr from install script | Status lines only. |

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

On Windows, when the WSL runtime is selected, bootstrap runs these steps in this order:

1. Repair: if `~/.config/terminal-commander/autostart.sh` or its profile snippet exists, rewrite both. This step uses non-login `bash -c`, because a <= 0.3.11 snippet exits every login shell once the daemon socket exists. It writes nothing on a fresh machine.
2. Runtime ensure: probe the runtime version; on skew, run the locked constant `npm install -g terminal-commander`, verify `terminal-commander-mcp` and the platform package, and swap the live daemon. These steps run in `bash -lc`, with `PATH` stripped of Windows `nodejs` / `npm` shims before Linux paths.
3. Daemon autostart: the full install. It runs after the runtime because it chooses systemd only when the daemon binary already exists.
4. Start the daemon. When `TC_DAEMON_CHILD`, `TC_SOCKET` or `TC_SESSION` is set in the bootstrap's own environment (it runs inside a Terminal Commander session), this step does not run. It reports `daemon_start: { status: "skipped", reason }` and an informational line, not a warning. If `autostart.sh` declines anyway, it prints `terminal-commander: autostart skipped (inside a Terminal Commander session)`, which is also reported as `skipped`, never `ok`. The Linux install and `setup daemon-autostart` report the same skip.

Every `bash -lc` step prints a shell-ran sentinel before its command. Exit 0 without the sentinel means a startup file ended the shell early. Such a step fails with `shell_exited_early` and names the likely cause; it is never reported as success.

NO `sudo`. NO password prompts. NO operator argv interpolation into `bash -lc`. `--install-wsl-runtime` is still accepted (for example on `setup cursor-wsl`), but it changes nothing: the ensure path always runs.

## 6. Lazy bootstrap

None. See section 2.

## 7. Deprecation

- `terminal-commander setup cursor-wsl` prints a migration notice and runs the same bootstrap, writing only the Cursor harness config.
- Preferred: `terminal-commander setup` or `terminal-commander setup harness`.

## 8. Cross-links

- [`windows-wsl-bridge-contract.md`](windows-wsl-bridge-contract.md) — bridge shim (unchanged).
- [`npm-binary-packaging-contract.md`](npm-binary-packaging-contract.md) — optionalDependencies layout.
- [`../integrations/README.md`](../integrations/README.md) — per-provider stanzas.
