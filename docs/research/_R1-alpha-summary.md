# R1-alpha Research Summary

Author: research agent R1-alpha
Date: 2026-05-21
Goal: TC01 (research product baseline)
Scope: A1, A2, A3, B1, B2, B3, C1, C2, C3

This summary collects one-line recommendations and confidence per
topic. Detailed evidence and citations are in the per-topic files.

## One-line recommendations

| Topic | File | Recommendation | Confidence |
|---|---|---|---|
| A1 language | `language-choice.md` | Rust, edition 2024. | high |
| A2 runtime | `async-runtime.md` | tokio (rmcp forces it). | high |
| A3 MSRV | `msrv.md` | Rust 1.90 (rmcp 0.16.0) or 1.92 (rmcp 1.7.0). Edition 2024. | medium-high |
| B1/B2 MCP SDK | `mcp-rust-sdk.md` | rmcp crate from modelcontextprotocol/rust-sdk. MCP spec 2025-11-25. | high |
| B3 transport | `mcp-transport-pattern.md` | Two-process split; MCP stdio to harness, local-socket (interprocess v2) to daemon. | medium |
| C1 PTY | `pty-crate.md` | pty-process 0.5.3 with `async` feature for MVP; portable-pty for future Windows. | medium-high |
| C2 daemon | `daemon-lifecycle.md` | Foreground supervisor + PID file; optional systemd user unit via sd-notify. | medium-high |
| C3 cleanup | `process-cleanup.md` | process-wrap (tokio1) + process groups + SIGTERM-then-SIGKILL ladder. | medium-high |

## Files written

All paths absolute:

- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\language-choice.md`
- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\async-runtime.md`
- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\msrv.md`
- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\mcp-rust-sdk.md`
- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\mcp-transport-pattern.md`
- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\pty-crate.md`
- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\daemon-lifecycle.md`
- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\process-cleanup.md`
- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\_R1-alpha-summary.md`

## Top 3 findings

1. rmcp 0.16.0 is NOT the current crates.io latest. Current latest
   is rmcp 1.7.0 (released 2026-05-13). The README at HEAD still
   has 0.16.0 in install snippets, but releases page shows 1.x line
   active since at least 2026-03 (1.3.0). User-pinned evidence
   said 0.16.0; downstream architect must explicitly decide whether
   to pin 0.16.0 (MSRV 1.90, edition 2024) or upgrade to 1.7.0
   (MSRV 1.92, edition 2024). Both are viable. The protocol revision
   is the same (2025-11-25).
   Sources:
   https://github.com/modelcontextprotocol/rust-sdk/releases
   and
   https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/README.md
2. The two-process pattern (thin MCP server + persistent daemon) is
   NOT the industry default for local MCP servers. All three
   reference servers reviewed (filesystem MCP server in TypeScript,
   github-mcp-server in Go, container-use in Go) are single-process.
   The split in Terminal Commander is justified by privilege
   separation and multi-agent attach, both stated in the README
   safety section, but it is a project-specific choice. Architect
   should make this design choice explicit, not implicit.
3. PTY crate ecosystem requires a deliberate choice. `pty-process`
   0.5.3 (edition 2024, MIT, tokio under `async` feature) is the
   clean MVP fit for Linux+WSL but is POSIX-only. `portable-pty`
   0.9.0 (MIT) is cross-platform including Windows ConPTY but is
   blocking and has zero tokio integration (`tokio` is not a
   dependency at all). For Windows native later, an explicit
   blocking-to-async bridge will be needed; this is a known cost.

## Items REQUIRES USER DECISION

These are open choices the architect must escalate to the user, not
guess at:

1. rmcp pin: 0.16.0 (user-pinned evidence) vs 1.7.0 (current
   crates.io latest). Decides MSRV (1.90 vs 1.92). See `msrv.md`
   and `mcp-rust-sdk.md`.
2. Two-process vs single-process architecture. The README safety
   model implies two-process, but it is not strictly required for
   MVP. See `mcp-transport-pattern.md`.
3. Per-user vs per-machine daemon. Per-user is recommended. See
   `mcp-transport-pattern.md`.
4. SQLite client: rusqlite + tokio-rusqlite vs sqlx (sqlite). Not
   needed at the architecture step; defer to the store goal.
   See `msrv.md` MSRV table.
5. Daemonize crate: skip entirely (foreground only) vs add
   `--daemonize` via the maintained `fork` crate (BSD-3-Clause).
   See `daemon-lifecycle.md`.

## HALT-worthy findings

None. Every dependency that the architecture depends on (rmcp,
tokio, pty-process, portable-pty, notify, interprocess, process-wrap,
sd-notify, rusqlite, sqlx, fork) exists, is published on crates.io
or maintained on GitHub, and supports the target platforms (Linux
native + WSL2). The project is buildable today on the recommended
toolchain.

## SOURCE_MAP reclassifications (inference -> evidence-backed)

- Language choice = Rust: evidence-backed via README crate naming
  and user-pinned rmcp 0.16.0.
- Runtime = tokio: evidence-backed via rmcp Cargo.toml direct
  dependency on `tokio = "1"`.
- MSRV >= 1.90 (rmcp 0.16.0) or 1.92 (rmcp 1.7.0): evidence-backed
  via rmcp `rust-toolchain.toml` at both tags.
- Edition 2024: evidence-backed via rmcp workspace package and
  pty-process Cargo.toml.
- MCP protocol revision 2025-11-25: evidence-backed via
  modelcontextprotocol.io specification page and rmcp README.
- rmcp transport set (stdio + streamable HTTP server + child
  process etc.): evidence-backed via
  `crates/rmcp/src/transport.rs`.
- WSL systemd is opt-in (requires WSL 0.67.6 + `/etc/wsl.conf`):
  evidence-backed via Microsoft learn docs.
- `daemonize` 0.5.0 unmaintained; `fork` 0.7.0 maintained:
  evidence-backed via lib.rs pages.
- `process-wrap` 9.1.0 supersedes `command-group`: evidence-backed
  via watchexec/command-group README.
- watchexec uses process groups for child cleanup: evidence-backed
  via watchexec README.
- `portable-pty` has no tokio dependency: evidence-backed via
  wezterm `pty/Cargo.toml`.
- `pty-process` 0.5.3 supports tokio via `async` feature with
  `tokio = "1.46.1"`: evidence-backed via doy/pty-process
  `Cargo.toml`.

## Out-of-scope notes

- macOS launchd and Windows SCM integration are mentioned only as
  future parity; not part of MVP.
- Specific sifter implementation language, registry storage schema,
  policy engine design, and event-store schema are NOT in scope for
  this research pass.
- License selection for the project itself remains a separate user
  decision (README explicitly says "License is not selected yet").

## Branch / repo hygiene

- No source code was touched. Only files under
  `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\` were
  created.
- Branch guard respected: no git operations performed.
