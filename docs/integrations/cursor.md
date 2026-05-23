# Cursor MCP integration

Connect [Cursor](https://cursor.com) to Terminal Commander over MCP
stdio. Cursor supports MCP servers via a `mcp.json` configuration
file at either of two scopes (global, project) — this doc ships
copy-pasteable configs for the three supported host shapes:

1. Native Linux Cursor (or Cursor running inside WSL).
2. Project-scoped MCP on Linux / WSL.
3. Windows Cursor invoking the WSL-hosted `terminal-commander-mcp`
   over the `wsl.exe` bridge.

No secrets, no tokens, no machine-specific absolute paths.

Language: ASCII only.

## 1. Prerequisites

- Linux or WSL2 host for the daemon. Terminal Commander's daemon UDS
  is Unix-only; native Windows is unsupported (TC44 `non_goals`).
- One of the install paths below provides the binaries:
  - **Future (post-NPM07 live publish):** `npm install -g
    terminal-commander` produces `terminal-commanderd`,
    `terminal-commander-mcp`, and `terminal-commander` on `$PATH`.
    This path is **pending** the first live publish; see
    `docs/release/npm-trusted-publishing-contract.md` §14 for the
    blocking operator preconditions.
  - **Current pre-publish:** the NPM04 local-tarball smoke
    (`scripts/smoke/verify-npm-local-install.sh`) builds + packs +
    installs into a sandbox prefix, exactly the same binaries the
    published package will ship. Operators who want to try Cursor
    against pre-publish binaries can install the local tarballs into
    a non-sandboxed `--prefix` and add that `bin/` to `$PATH`.
  - **Cargo-built path:** `cargo build -p terminal-commanderd -p
    terminal-commander-mcp -p terminal-commander-cli --bins` and
    point Cursor at the absolute or workspace-relative paths.

## 2. Where Cursor reads MCP config

Cursor reads MCP server configs from two locations (current
documented behavior at the time of this writing):

| Scope | Path | When to use |
|-------|------|-------------|
| Global | `~/.cursor/mcp.json` | The MCP server is available in every Cursor workspace. |
| Project | `.cursor/mcp.json` at the workspace root | The MCP server is available only when this workspace is open. |

NEVER commit your local `.cursor/mcp.json` into a shared repo
without reviewing it — it is part of your security boundary
(MCP servers run with your user permissions). This Terminal
Commander repo intentionally does NOT ship an active
`.cursor/mcp.json`; copy from `examples/provider-harness/cursor/`
into your own scope.

## 3. Start the daemon

The daemon must be running before Cursor (or any MCP client) spawns
the adapter.

```sh
# Replace TC_DATA with any writable directory the daemon will own.
# The daemon creates `$TC_DATA/terminal-commanderd.sock`.
export TC_DATA="${XDG_STATE_HOME:-$HOME/.local/state}/terminal-commander"
mkdir -p "$TC_DATA"

terminal-commanderd --data-dir "$TC_DATA" start --mode ipc-server
```

Leave the daemon running in a separate terminal (or under a process
supervisor). Cursor's MCP layer spawns `terminal-commander-mcp` as a
child process; the adapter forwards every tool call to the daemon
UDS.

## 4. Native Linux / inside-WSL Cursor

If Cursor is running on Linux (or inside a WSL distribution), the
simplest config is:

```json
{
  "mcpServers": {
    "terminal-commander": {
      "command": "terminal-commander-mcp",
      "type": "stdio"
    }
  }
}
```

Copy from
[`examples/provider-harness/cursor/mcp.global.native-linux.json`](../../examples/provider-harness/cursor/mcp.global.native-linux.json)
into `~/.cursor/mcp.json`.

If your daemon is running in a non-default `$TC_DATA`, add the
`env` block:

```json
{
  "mcpServers": {
    "terminal-commander": {
      "command": "terminal-commander-mcp",
      "type": "stdio",
      "env": {
        "TC_SOCKET": "${TC_DATA}/terminal-commanderd.sock"
      }
    }
  }
}
```

`TC_DATA` must match the value the daemon was started with. The
adapter does NOT spawn a daemon on its own; it only connects to an
existing UDS path.

## 5. Project-scoped Cursor MCP

For a workspace-local config, place
[`examples/provider-harness/cursor/mcp.project.linux-wsl.json`](../../examples/provider-harness/cursor/mcp.project.linux-wsl.json)
at `<workspace-root>/.cursor/mcp.json`. The shape is identical
to the global config; only the file location changes.

This is the right choice when:

- multiple Cursor projects need different MCP server sets,
- the MCP server should only be visible inside a specific repo,
- you want the config to live in version control alongside the
  workspace (in which case carefully review the file every time —
  MCP servers run with your user permissions).

## 6. Windows Cursor → WSL bridge

When Cursor runs natively on Windows but the Terminal Commander
daemon runs inside WSL (the supported host topology, since the
daemon UDS is Unix-only), Cursor must launch the adapter via
`wsl.exe`.

```json
{
  "mcpServers": {
    "terminal-commander": {
      "command": "wsl",
      "type": "stdio",
      "args": [
        "-d",
        "Ubuntu-24.04",
        "bash",
        "-lc",
        "terminal-commander-mcp"
      ]
    }
  }
}
```

Copy from
[`examples/provider-harness/cursor/mcp.global.linux-wsl.json`](../../examples/provider-harness/cursor/mcp.global.linux-wsl.json)
into `%USERPROFILE%\.cursor\mcp.json` on Windows. Substitute your
WSL distribution name (`Ubuntu-24.04` in the example) for the one
returned by `wsl --list --verbose`.

Notes on this bridge:

- `bash -lc` ensures `$PATH` is set from the login shell so the
  `terminal-commander-mcp` shim resolves correctly.
- The daemon must already be running INSIDE the WSL distro. The
  Windows side does not start it. A common pattern is a systemd
  user service or a `tmux` / `screen` session inside WSL.
- stdin / stdout flow over the `wsl.exe` pipe transparently;
  rmcp framing is preserved.

## 7. Verify tool discovery in Cursor

Open Cursor and:

1. **Settings → Features → MCP**. The `terminal-commander` server
   should appear and report "Connected" once the daemon is up.
2. In the Cursor chat panel, ask:
   > List the MCP tools you have available.
   You should see the 29 TC45 tools (`system_discover`, `health`,
   `command_start_combed`, `bucket_wait`, `bucket_events_since`,
   `command_status`, `file_read_window`, `file_search`,
   `file_watch_start` / `_stop` / `_list`, `pty_command_start` /
   `_write_stdin` / `_stop` / `_list`, `registry_*`, `runtime_state`,
   `probe_list`, `probe_status`, etc.).
3. Ask Cursor to call `health`. The response is a bounded JSON
   envelope; no raw stream text appears in the chat.

## 8. Minimal real flow

Ask Cursor to:

1. Call `command_start_combed` with argv `["echo", "hello"]`.
2. Call `bucket_wait` with the returned `bucket_id` and `cursor: 0`,
   `timeout_ms: 2000`.
3. Call `command_status` with the returned `job_id`.

Every response is a bounded JSON envelope. No raw stdout / stderr
appears in the chat transcript. This is the same flow documented in
`docs/integrations/codex-cli.md` § "Step 4 — minimal flow".

## 9. Smoke evidence requirements

Per the TC46 acceptance criteria, the Cursor provider-harness smoke
is considered "live" only if a Cursor session actually invokes one
of the Terminal Commander tools AND the response is observed in
the session transcript. If your environment lacks a usable Cursor
install / auth / config, mark the provider smoke as `Not Run` or
`Blocked` in your report and cite the exact reason.

Cursor's MCP discovery + tool invocation flow today does NOT have
a documented headless / scripted entry point — the operator must
open Cursor, configure the MCP server, observe the chat tool
catalogue, and capture a transcript or screenshot. There is no
`cursor --list-mcp-tools` subcommand at present. A local daemon +
MCP stdio smoke (without Cursor in the loop) is available via
[`scripts/smoke/verify-runtime-smoke.sh`](../../scripts/smoke/verify-runtime-smoke.sh);
it is secondary evidence, not provider-harness success.

## 10. Security notes

- MCP servers execute local code under your user permissions.
  Treat every `mcp.json` entry the same way you treat a shell
  alias — review before adopting.
- This config exposes ONLY `terminal-commander-mcp`. No raw shell
  is bridged. No HTTP/SSE transport is configured. No environment
  secrets are passed.
- Terminal Commander's MCP surface (TC45) is bounded: 29 tools
  with JSON envelopes, no raw stream lane. The MCP guard greps
  remain green:
  - `rg "Command::new|Command::spawn|TcpListener|UdpSocket"
    crates/mcp` — doc / negative-assertion matches only.
  - `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end"
    crates/mcp/src` — no matches.
- Do NOT add auto-run / auto-execute permissions to the
  `terminal-commander` MCP entry. The operator confirms each tool
  call in the Cursor chat panel.
- Do NOT include credentials, API keys, or env secrets in
  `mcp.json`. The `env` block is only for non-secret transport
  variables (e.g. `TC_SOCKET`).

## 11. Troubleshooting

- **Cursor reports "MCP server failed to start".** Confirm
  `terminal-commander-mcp` runs from your shell with the same env.
  The adapter is Unix-only; on native Windows it refuses to start
  and exits with code 64. Use the WSL bridge configuration in §6.
- **`daemon ipc error [Internal]: request timed out`.** The daemon
  is not running or `TC_SOCKET` does not match the daemon's socket
  path. Confirm `$TC_DATA/terminal-commanderd.sock` exists and is
  readable from the user that launched Cursor.
- **No tools listed in Cursor MCP settings.** Cursor caches tool
  catalogues per server name; rename `terminal-commander` to
  invalidate the cache, or restart Cursor.
- **WSL bridge prints `wsl.exe: no such command` or hangs.**
  Confirm the distro name in `args` matches `wsl --list --verbose`
  output exactly. Some installations rename Ubuntu (e.g.
  `Ubuntu-22.04`, `Ubuntu`); the example uses `Ubuntu-24.04`.

## 11a. Auto-generated config (WWS05)

WWS05 ships a JS-only writer under
`packages/terminal-commander/lib/cursor/` that produces the
correct Cursor `mcp.json` stanza for the WWS04 bridge path. It is
a library API at WWS05; the CLI subcommand
(`terminal-commander setup cursor-wsl`) that calls it arrives at
WWS06. Operators who want to invoke the writer directly today can
do so via `node`:

```js
const { writeCursorMcpConfig } = require("terminal-commander/lib/cursor/write.js");
const r = writeCursorMcpConfig({
  scope: "global",            // or "project" with explicit projectRoot
  distro: "Ubuntu-24.04",     // optional; double-validated before emit
  force: false,               // refuse-existing-terminal-commander default
  clobber_backup: false,      // refuse pre-existing .bak default
});
// r.status is one of: config_created, config_updated, already_exists,
//   invalid_json, config_too_large, path_not_allowed,
//   project_root_required, unsafe_distro_name, distro_not_found,
//   backup_failed, write_failed, unsupported_host.
// r.path / r.backup_path / r.server / r.was_present / r.hint follow.
```

Safety guarantees:

- Preserves every unrelated `mcpServers` entry untouched.
- Refuses to overwrite an existing `terminal-commander` entry
  unless `force: true` is passed. Always creates `<mcp.json>.bak`
  before overwrite. Refuses if `.bak` already exists unless
  `clobber_backup: true`.
- Reads at most 256 KiB of existing `mcp.json`
  (`config_too_large` refusal above that).
- Refuses `invalid_json` without modifying the original file.
- Atomic write via a same-directory `mcp.json.tmp.<random>` +
  `renameSync`. Every path the writer touches stays inside the
  resolved Cursor scope dir (`path_not_allowed` otherwise).
- NO `child_process`, NO spawn, NO `wsl.exe`, NO network, NO
  secrets/tokens/credentials/passwords. The only env key the
  writer ever emits is `TC_WSL_DISTRO`.
- Stdout-silent — all status text returns via the typed result
  record.

## 11b. Windows bridge roadmap (WWS chain)

The Windows config block in §6 is the **current manual path**:
operator copies the JSON, substitutes their distro, restarts
Cursor. A future release will replace it with a single setup
command (`terminal-commander setup cursor-wsl`) that auto-detects
WSL, picks a distro, writes `~/.cursor/mcp.json`, and verifies
the bridge end-to-end.

The full design is locked in
[`docs/release/windows-wsl-bridge-contract.md`](../release/windows-wsl-bridge-contract.md)
(WWS01). The implementation is staged across WWS02 (root npm
package widened to install on Windows), WWS03 (the read-only
discovery layer: `lib/wsl/distro-name.js` distro safety
whitelist, `lib/wsl/detect.js` `wsl.exe -l -v` parser, and
`lib/wsl/doctor.js` read-only `terminal-commander-mcp`-presence
probe), WWS04 (the `terminal-commander-mcp` bridge shim
`lib/wsl/spawn.js` that actually invokes `wsl.exe -d <distro> --
bash -lc 'exec terminal-commander-mcp'` with double-validated
distro, `stdio: 'inherit'`, and token-shaped env vars stripped),
WWS05 (the Cursor config writer — `lib/cursor/config.js` +
`lib/cursor/write.js` — now landed as library-only; auto-generates
the bridge stanza, refuses to clobber existing `terminal-commander`
entries without `force`, always creates `.bak` before overwrite,
atomic-writes via a same-directory tmp file + rename), and WWS06
(the setup / doctor / pair CLI). With WWS04 + WWS05 landed, Cursor
on Windows that invokes `terminal-commander-mcp` will now
transparently bridge into the WSL distro chosen by
`TC_WSL_DISTRO` or the WSL default — provided the WSL-side
`terminal-commander-mcp` is installed (the bridge short-circuits
with `runtime_missing` until WWS06 lands the optional
`--install-wsl-runtime` flag). The manual `wsl.exe` config in §6
remains a valid alternative for operators who prefer to invoke
`wsl.exe` directly from `mcp.json`.

## 12. Source status

| Component | Status |
|---|---|
| `docs/integrations/cursor.md` (this file) | live (NPM08, 2026-05-23) |
| `examples/provider-harness/cursor/*.json` | live (NPM08, 2026-05-23) — copy intentionally |
| `npm install -g terminal-commander` install path | pending NPM07 live publish + operator preconditions |
| Local-tarball pre-publish install path | live (NPM04) |
| Cursor provider smoke transcript | **Not Run** (operator-driven; see §9) |
| MCP guard greps | green (unchanged from TC45 / NPM02 baseline) |
