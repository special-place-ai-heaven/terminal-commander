![Terminal Commander](./docs/TC_logo.png)

# Terminal Commander

**Terminal Commander** is a local realtime signal channel and MCP
tool-control surface for LLM coding agents. It runs entirely on
your machine — daemon, MCP stdio adapter, and admin CLI — and
turns noisy terminal, file, and PTY streams into bounded, structured
signal that an LLM can read by cursor without ever seeing the raw
output.

It is NOT a CLI command runner.
It is NOT a shell bridge.
It is NOT a log shipper.
It is NOT a remote service.

```text
Raw terminal/file/PTY output goes in.
Only vetted, structured signal comes out.
Context remains available by pointer.
```

## Value: all signal, no raw noise

- **Probes** observe commands, files, PTY streams, and runtime state.
- **Sifters** + **registry rules** extract useful events (errors,
  package-missing, stalls, prompts, artifacts, regressions).
- **Buckets** expose bounded, cursor-based structured signal streams.
- **Context pointers** provide bounded surrounding context ONLY when
  the LLM asks — never as a raw firehose.
- The LLM speaks intent (`run this`, `watch that`, `notify me when
  X`); the daemon, probes, and sifters do the parsing toil.

## Architecture

```text
   +-----------------------------------------------+
   |   LLM / MCP client                            |
   |   (Cursor, Claude Code, Codex CLI, generic)   |
   +-----------------------------------------------+
                       |
                       |  MCP stdio (rmcp 1.7.0)
                       v
   +-----------------------------------------------+
   |  terminal-commander-mcp        (crates/mcp)   |
   |  thin adapter; NO Command::spawn;             |
   |  NO file open; NO network listener            |
   +-----------------------------------------------+
                       |
                       |  Local UDS, peer-cred checked,
                       |  length-prefixed JSON, 256 KiB cap
                       v
   +-----------------------------------------------+
   |  terminal-commanderd         (crates/daemon)  |
   |  IPC server, policy engine, job manager,      |
   |  router, persistent audit (SQLite)            |
   +-----------------------------------------------+
                       |
        +--------------+---------------+
        v              v               v
   +-----------+  +-----------+  +------------------+
   | process / |  | sifters / |  | in-memory bucket |
   | file /    |->|  registry |->| manager + ring   |
   | directory |  | runtime   |  | context + audit  |
   | / PTY     |  +-----------+  +------------------+
   | probes    |
   +-----------+
```

Data flows DOWN the stack. Signal flows UP. Only bounded JSON
envelopes cross the IPC boundary — no raw stream lane reaches the
LLM. The MCP server is a thin adapter; the daemon owns every probe,
every spawn, and every audit write.

Authoritative deeper docs:
- [`docs/runtime/REALTIME_SIGNAL_CHANNEL.md`](docs/runtime/REALTIME_SIGNAL_CHANNEL.md) — product semantics.
- [`docs/runtime/UDS_IPC.md`](docs/runtime/UDS_IPC.md) — local UDS IPC.
- [`docs/runtime/COMMAND_RUNTIME.md`](docs/runtime/COMMAND_RUNTIME.md) — command lifecycle + policy gate.
- [`docs/mcp/TOOL_CONTROL_SURFACE.md`](docs/mcp/TOOL_CONTROL_SURFACE.md) — locked MCP tool list with bounds and policy gates.
- [`docs/security/PRIVILEGE_MODEL.md`](docs/security/PRIVILEGE_MODEL.md) — security envelope.

## Feature matrix

| Area | Surface | Status |
|------|---------|--------|
| `terminal-commanderd` daemon | bootstrap / IPC server / persistent audit / sub-commands `check` / `start` / `print-config` | live (TC33–TC48) |
| Bounded UDS IPC | length-prefixed JSON over a local Unix domain socket, peer-cred checked, 256 KiB frame cap | live (TC37) |
| `terminal-commander-mcp` stdio server | rmcp 1.7.0 stdio adapter, no `Command::spawn`, no file open, no network listener | live (TC40 + ongoing) |
| Persistent audit | SQLite `audit_records` with closed-set decision labels | live (TC35) |
| `command_start_combed` | argv-only spawn with policy-gated start + shell-interpreter deny | live (TC38 / TC41) |
| `command_status` | bounded lifecycle/status JSON envelope | live (TC41 / TC45) |
| `bucket_events_since` | cursor-bounded read with severity / kind / limit filters | live (TC39 / TC41) |
| `bucket_wait` | `Notify`-backed park with heartbeat-on-timeout, never raw text | live (TC39 / TC41) |
| `bucket_summary` | per-bucket counters | live (TC39 / TC41) |
| `event_context` | pointer-anchored window, 1024 frames / 64 KiB cap, typed `unavailable_reason` on miss | live (TC39 / TC41) |
| Registry upsert / test / activate / deactivate | dynamic rule registry with scoped live rebind | live (TC42 / TC42b / TC42c / TC42d) |
| `file_read_window` / `file_search` | bounded path-policy-gated file tools | live (TC43) |
| `file_watch_start` / `_stop` / `_list` | bounded file/directory watch with sifters | live (TC43) |
| `pty_command_start` / `_write_stdin` / `_stop` / `_list` | PTY surface with **secret-prompt-deny on stdin** | live (TC44) |
| `runtime_state` / `probe_list` / `probe_status` | aggregate runtime view | live (TC45) |
| TC47 load / noise / backpressure gate | 8 stress tests | live (TC47) |
| npm wrapper + platform package layout | `terminal-commander` root + `@terminal-commander/linux-x64` + `@terminal-commander/linux-arm64` with `optionalDependencies`, no postinstall | live (NPM02 / NPM03 / NPM04) |
| GitHub Actions npm-binary-build matrix | linux-x64 (full smoke) + linux-arm64 (build + pack); both pass on `ubuntu-24.04` and `ubuntu-24.04-arm` | live (NPM05) |
| release-please manifest mode | manifest-mode config at `.github/`, single shared `0.1.0-beta.1` version, linked-versions plugin | live (NPM06) |
| npm trusted publishing (OIDC + provenance) | output-gated publish jobs inside `release-please.yml`; no `NPM_TOKEN`, no PAT | live workflow (NPM07); **first live publish pending** operator npmjs.com trusted-publisher setup + a release PR merge |
| Cursor MCP config examples | native Linux / inside-WSL + Windows → WSL bridge | live (NPM08) |
| Cursor provider live smoke transcript | requires operator GUI steps (no headless Cursor MCP entry point on host) | **Not Run** |
| Codex CLI + Claude Code provider live smokes | operator-driven | **Not Run** (TC46 / TC48 baseline) |

## Install

Terminal Commander runs on **Linux** and **WSL2**. Initial npm
platforms are **linux-x64** and **linux-arm64**. There is **no
macOS-native, no Windows-native, and no musl / Alpine package**
claim at this time — the daemon UDS is Unix-only and the runtime
chain (TC44 `non_goals`) explicitly defers those targets.

### Future published path (pending first live publish)

Once the operator completes the npmjs.com trusted-publisher
preconditions in [`docs/release/npm-trusted-publishing-contract.md`](docs/release/npm-trusted-publishing-contract.md) §8
and the first release PR merges, the install command will be:

```sh
npm install -g terminal-commander
```

After install, the following commands land on `$PATH`:

- `terminal-commanderd`        — daemon
- `terminal-commander-mcp`     — MCP stdio adapter
- `terminal-commander`         — admin CLI

This path is **not active yet**; no npm publish has executed against
the registry. See [§ Current beta status](#current-beta-status).

### Current pre-publish path

Build + pack + install the same packages locally without touching
npmjs.com:

```sh
bash scripts/smoke/verify-npm-local-install.sh
```

The smoke script produces real release binaries via `cargo build
--release`, stages them into the matching platform package's `bin/`,
runs `npm pack`, installs the resulting tarballs into a sandboxed
`--prefix`, and exercises the three commands end-to-end (daemon
self-check + MCP stdio handshake + `tools/list` returning 29 tools).
Documented in [`docs/release/npm-binary-packaging-contract.md`](docs/release/npm-binary-packaging-contract.md).

### Cargo-built path (always available)

```sh
cargo build --release \
  -p terminal-commanderd \
  -p terminal-commander-mcp \
  -p terminal-commander-cli
```

The CLI's Cargo package name is `terminal-commander-cli`; the binary
name is `terminal-commander` (per the `[[bin]]` section in
`crates/cli/Cargo.toml`).

## Quickstart

```sh
# 1. Start the daemon (replace TC_DATA with any writable directory)
export TC_DATA="${XDG_STATE_HOME:-$HOME/.local/state}/terminal-commander"
mkdir -p "$TC_DATA"
terminal-commanderd --data-dir "$TC_DATA" start --mode ipc-server
```

Leave the daemon running in a separate terminal or under a process
supervisor.

```sh
# 2. Configure Cursor MCP. Copy ONE of these into ~/.cursor/mcp.json
#    (or your workspace's .cursor/mcp.json):
#
#    examples/provider-harness/cursor/mcp.global.native-linux.json
#    examples/provider-harness/cursor/mcp.project.linux-wsl.json
#    examples/provider-harness/cursor/mcp.global.linux-wsl.json   (Windows → WSL bridge)
#
# See: docs/integrations/cursor.md
```

```sh
# 3. Inside Cursor (or any MCP client), ask for the tool list and
#    verify the 29 TC45 tools are visible, then call:
#
#      health
#      system_discover
```

```sh
# 4. Run a real flow (works against any MCP client):
#
#    command_start_combed argv=["echo","hello"]
#    bucket_wait        bucket_id=<from_combed>  cursor=0  timeout_ms=2000
#    command_status     job_id=<from_combed>
```

Every response is a bounded JSON envelope. No raw stdout or stderr
appears in the LLM transcript.

## Cursor MCP integration

Cursor reads MCP server configs from `~/.cursor/mcp.json` (global)
or `<workspace>/.cursor/mcp.json` (project). This repo does NOT
ship an active `.cursor/mcp.json` — operators copy intentionally
from `examples/provider-harness/cursor/` into their own scope.

Full walk-through: [`docs/integrations/cursor.md`](docs/integrations/cursor.md).

### Native Linux / inside-WSL Cursor

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

[`examples/provider-harness/cursor/mcp.global.native-linux.json`](examples/provider-harness/cursor/mcp.global.native-linux.json)

### Windows Cursor → WSL bridge

```json
{
  "mcpServers": {
    "terminal-commander": {
      "command": "wsl",
      "type": "stdio",
      "args": ["-d", "Ubuntu-24.04", "bash", "-lc", "terminal-commander-mcp"]
    }
  }
}
```

[`examples/provider-harness/cursor/mcp.global.linux-wsl.json`](examples/provider-harness/cursor/mcp.global.linux-wsl.json)

Substitute your WSL distribution name from `wsl --list --verbose`.

### Cursor live smoke status

**Not Run.** Cursor 3.5.30 is installed on the verification host,
but Cursor today has no documented non-interactive MCP discovery /
tool-call entry point — there is no `cursor --list-mcp-tools`
subcommand, and the `cursor-agent` headless CLI is not installed.
Live smoke requires operator-driven GUI steps (open Cursor → place
config → start daemon → ask Cursor chat to list MCP tools and call
`health` → capture transcript). Not scriptable in this session;
not promoted to PASS.

The local daemon + MCP stdio smoke
([`scripts/smoke/verify-runtime-smoke.sh`](scripts/smoke/verify-runtime-smoke.sh))
is secondary evidence only — it proves the local transport surface
without a provider in the loop.

## MCP tool catalogue (29 tools)

```text
system_discover            health                    policy_status
self_check
command_start_combed       command_status
bucket_events_since        bucket_wait               bucket_summary
event_context
registry_search            registry_get              registry_upsert
registry_test              registry_activate         registry_deactivate
registry_list_active
file_read_window           file_search
file_watch_start           file_watch_stop           file_watch_list
pty_command_start          pty_command_write_stdin   pty_command_stop
pty_command_list
runtime_state              probe_list                probe_status
```

The most important tool is `bucket_wait`: the LLM parks on a
`Notify`-backed channel and receives a bounded JSON envelope when
matching signal arrives — or `heartbeat = true` on timeout, never
raw text:

```json
{
  "method": "bucket_wait",
  "params": {
    "bucket_id": "bkt_01HX...",
    "cursor": 1842,
    "severity_min": "medium",
    "timeout_ms": 30000
  }
}
```

Full per-tool bounds, policy gates, and shapes:
[`docs/mcp/TOOL_CONTROL_SURFACE.md`](docs/mcp/TOOL_CONTROL_SURFACE.md).

## Settings and configuration

Operator-tunable settings live in `terminal-commanderd.toml`. A
safe-to-commit example ships at
[`config/terminal-commanderd.example.toml`](config/terminal-commanderd.example.toml).

Selected knobs:

| Key | Purpose | Default / constraint |
|---|---|---|
| `daemon.data_dir` | SQLite DB + audit log location | MUST be a native Linux filesystem; WSL `/mnt/c` is rejected at writer open |
| `daemon.socket_path` | local UDS path | `<data_dir>/terminal-commanderd.sock` |
| `daemon.runtime_mode` | how `start` runs | `self_check` / `foreground_idle` / `ipc_server` |
| `policy.profile` | policy profile | `developer_local` / `repo_only` / `read_only_observer` / `admin_debug` |
| `retention.max_events` | per-bucket cap | clamped to 100 000 |
| `retention.ttl_seconds` | per-bucket TTL | 24 h default |
| `audit.retention_days` | audit log retention | operator-controlled |
| `limits.file_window_bytes` | file-read window cap | clamped at config load to 64 KiB |
| `limits.bucket_read_limit` | bucket-read cap | clamped at config load to 10 000 |

Environment variables used by the MCP adapter:

| Var | Purpose |
|---|---|
| `TC_SOCKET` | optional override for the daemon UDS path the adapter connects to; defaults to `<data_dir>/terminal-commanderd.sock`. Used in the `mcp.project.linux-wsl.json` Cursor example via `${TC_DATA}/terminal-commanderd.sock`. |

No other config keys are advertised here. Path-policy / bounded-output
caveats: every command spawn passes through the policy engine BEFORE
it runs; every file read passes through the path-suffix deny list;
every audit record lands in SQLite with a closed-set decision label.

## Safety posture

Terminal Commander runs locally with a deliberately narrow security
envelope:

- **No MCP root shell.** `terminal-commander-mcp` is a thin adapter
  with no `Command::spawn` of its own.
- **No network listener.** Neither the daemon nor the MCP crate
  opens a TCP / UDP / HTTP / SSE socket. Reachability is local UDS
  only, peer-cred checked.
- **No raw stream endpoint.** No tool returns raw stdout / stderr.
  Every response is a bounded JSON envelope (`bucket_wait` returns
  events or a heartbeat; `event_context` returns a bounded pointer-
  anchored window with `unavailable_reason` on miss).
- **No MCP-side command spawn.** Structural grep guard:
  `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp`
  yields only doc / negative-assertion matches.
- **No MCP-side file reads.** Structural grep guard:
  `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src`
  yields no matches.
- **No shell bridge.** `command_start_combed` is argv-only and runs
  a shell-interpreter deny list (`sh`, `bash`, `dash`, `zsh`,
  `fish`, `ksh`, `csh`, `tcsh`, `ash`, `busybox`, `powershell`,
  `pwsh`, `cmd`, plus `.exe` variants) BEFORE the policy engine.
- **No automatic password entry.** `pty_command_write_stdin`
  rejects writes that look like secret-prompt responses
  (sudo/SSH/GPG prompt patterns) per TC44.
- **Persistent audit.** Every policy-relevant action lands in
  SQLite's `audit_records` table with a closed-set decision label.
  No in-memory audit on a production path.
- **Bounded outputs.** Bucket reads cap at 10 000 events. File
  reads cap at 64 KiB. Context windows cap at 1024 frames / 64 KiB.
- **Pointer-or-reason invariant.** Every `severity >= medium` event
  carries either a `SourcePointer` or a typed `pointer_unavailable_reason`.
  Context-by-pointer is always bounded.
- **Policy gate before every spawn.** Four closed-set profiles
  (`developer_local`, `repo_only`, `read_only_observer`,
  `admin_debug`). Default-deny suffix list covers private keys,
  credential stores, sudoers, and token caches.

Full security boundary: [`docs/security/PRIVILEGE_MODEL.md`](docs/security/PRIVILEGE_MODEL.md).
Threat model + structural enforcement: [`SECURITY.md`](SECURITY.md).

## Current beta status

**Conditional Go** (TC48 baseline, preserved through NPM01–NPM08).

What is green:

- Local daemon + MCP stdio + npm-install smoke all pass.
  - TC46 runtime smoke (`scripts/smoke/verify-runtime-smoke.sh`): 8/8 PASS.
  - NPM04 npm-install smoke (`scripts/smoke/verify-npm-local-install.sh`): 12 PASS (end-to-end MCP stdio against npm-installed binaries).
  - `cargo nextest run --workspace`: 347/347 PASS, 0 skipped.
- TC47 load / noise / backpressure gate: 8/8 stress tests passing.
- NPM05 GitHub Actions npm-binary-build matrix: PASS on both
  `ubuntu-24.04` (full smoke) and `ubuntu-24.04-arm` (build + pack).
- NPM06 release-please live workflow runs cleanly on every push;
  no-ops when no `feat:` / `fix:` commits since the last release.
- NPM07 trusted-publishing workflow runs cleanly: on a normal push
  the three publish jobs are correctly **skipped** because
  `releases_created` evaluates to `false`. No `NPM_TOKEN`, no
  `CARGO_REGISTRY_TOKEN_TC`, no `RELEASE_PLEASE_TOKEN_TC`
  referenced. `npm-binary-build.yml` remains a separate
  non-publishing CI gate.

What is **Not Run / pending** (and why beta is `Conditional Go`,
not `Go`):

- **Cursor provider live smoke**: Not Run. No documented
  non-interactive entry point on the verification host. Requires
  operator GUI steps.
- **Codex CLI + Claude Code provider live smokes**: Not Run on the
  verification host (TC46 / TC48 baseline).
- **First live npm publish**: pending two operator-driven steps:
  1. Claim `@terminal-commander` org on npmjs.com and configure the
     trusted publisher for all three package names
     (`terminal-commander`, `@terminal-commander/linux-x64`,
     `@terminal-commander/linux-arm64`) with workflow filename
     `release-please.yml`. See
     [`docs/release/npm-trusted-publishing-contract.md`](docs/release/npm-trusted-publishing-contract.md) §8.
  2. A Conventional-Commits `feat:` / `fix:` commit lands on `main`,
     release-please opens a release PR, and the operator merges it.

Beta cannot promote to `Go` until at least one provider live smoke
transcript is attached. `Not Run` is **not** PASS.

Authoritative beta artifacts:
[`RELEASE_CHECKLIST.md`](RELEASE_CHECKLIST.md),
[`EVIDENCE_REPORT_RUNTIME.md`](EVIDENCE_REPORT_RUNTIME.md),
[`RISK_REGISTER.md`](RISK_REGISTER.md),
[`BACKLOG.md`](BACKLOG.md).

## Build and verify locally

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo nextest run --workspace
bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh
```

UDS IPC and command-runtime integration tests are Unix-only. Native
Windows compiles the workspace but skips those tests; use WSL2 to
exercise the full surface. Testing doctrine: [`TESTING.md`](TESTING.md).

## Repository layout

```text
.agent/
  goals/
    terminal-commander-mvp/             # library + scaffold goals
    terminal-commander-runtime/         # daemon runtime + IPC goals (TC33–TC48)
    terminal-commander-npm-distribution/# npm packaging + Cursor (NPM01–NPM09)

crates/
  core/      sifters/   probes/   store/   daemon/   mcp/   cli/

packages/
  terminal-commander/                   # npm root wrapper (JS shims)
  terminal-commander-linux-x64/         # platform binary package (linux/x64)
  terminal-commander-linux-arm64/       # platform binary package (linux/arm64)

examples/
  provider-harness/cursor/              # copy-pasteable Cursor MCP configs
  bucket_wait_demo.md
  dynamic_rule_demo.md

config/
  terminal-commanderd.example.toml
  terminal-commanderd.service.example

rules/
  apt.json   cargo.json   gcc.json   generic.terminal.json
  make.json  npm.json     pytest.json

docs/
  runtime/   mcp/        install/   integrations/
  release/   security/   audits/    contracts/
  storage/   research/   rules/
```

## Development approach

The project advances through small, sequential `/goal` files. Each
goal is:

- branch-safe,
- evidence-driven,
- narrowly scoped,
- independently verifiable,
- small enough for one autonomous agent run,
- explicit about allowed and forbidden files,
- clear about stop conditions and acceptance criteria.

The two-commit landing pattern (verified work commit + status
commit) and the prep-amendment-first rule for any scope drift come
from the TC43+ runtime chain and are preserved across the NPM
chain.

## License

Apache-2.0; see [`LICENSE`](LICENSE).

SPDX identifier: `Apache-2.0`. The full Apache License 2.0 text is
in the `LICENSE` file at the repository root, and `NOTICE` records
the rmcp relicensing transition relevant to the supply-chain
(`cargo-deny`) license allowlist. See
[`docs/research/license-decision.md`](docs/research/license-decision.md)
for the decision rationale and [`CONTRIBUTING.md`](CONTRIBUTING.md)
for the per-file SPDX header expectation.
