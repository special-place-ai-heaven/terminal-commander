# Research index - TC01 evidence

Status: Baseline (TC01 wave 0 deliverable).
Language: ASCII only.

These documents are the TC01 research evidence used to derive
`SPEC.md`, `ARCHITECTURE.md`, `ROADMAP.md`, `CONTRIBUTING.md`, the
source map, and the assumptions register. The architect consumed
this set during the TC01 research wave; downstream goals (TC02
through TC32) should cite these files when they reuse the same
evidence.

The files are immutable historical evidence. New research lands in
new files, not in edits to existing TC01 research files.

Confidence tags are taken from the per-topic summaries
(`_R1-alpha-summary.md`, `_R1-beta-summary.md`,
`_R2-gamma-summary.md`, `_R2-delta-summary.md`).

## User decisions (locked)

- [User decisions snapshot](_USER_DECISIONS.md) - architect-binding
  user lock-in for language, license, rmcp pin, runtime, daemon
  process model, workspace, storage, file watcher, WSL handling,
  PTY, policy, platforms.

## Research summaries (per-agent)

- [R1-alpha summary](_R1-alpha-summary.md) - language, async
  runtime, MSRV, MCP SDK, transport pattern, PTY, daemon lifecycle,
  process cleanup. (high to medium-high)
- [R1-beta summary](_R1-beta-summary.md) - file watcher, WSL
  boundary, SQLite + FTS5, policy prior art. (high)
- [R2-gamma summary](_R2-gamma-summary.md) - license, workspace
  layout, tooling baseline. (high)
- [R2-delta summary](_R2-delta-summary.md) - prior-art landscape,
  user-provided evidence sweep, SOURCE_MAP reclassifications. (high)

## Language and runtime

- [language-choice](language-choice.md) - Rust, edition 2024. (high)
- [async-runtime](async-runtime.md) - tokio (forced by rmcp). (high)
- [msrv](msrv.md) - Rust 1.92 with rmcp 1.7.0 (1.90 with 0.16.0).
  (medium-high)

## MCP

- [mcp-rust-sdk](mcp-rust-sdk.md) - rmcp from
  modelcontextprotocol/rust-sdk; protocol revision 2025-11-25. (high)
- [mcp-transport-pattern](mcp-transport-pattern.md) - two-process
  split, stdio to harness, local-socket via `interprocess` v2 to
  daemon (IPC decision deferred to TC21). (medium)

## PTY and process management

- [pty-crate](pty-crate.md) - pty-process 0.5.3 with `async` for
  MVP; portable-pty for future Windows-native. (medium-high)
- [daemon-lifecycle](daemon-lifecycle.md) - foreground supervisor +
  PID file; optional systemd USER unit; `fork` crate for optional
  `--daemonize`. (medium-high)
- [process-cleanup](process-cleanup.md) - `process-wrap` + process
  groups + SIGTERM-then-SIGKILL ladder. (medium-high)

## Filesystem

- [file-watcher](file-watcher.md) - notify 8.2 +
  notify-debouncer-full 0.7 with explicit per-target transport.
  (high)
- [wsl-boundary](wsl-boundary.md) - native WSL FS works as Linux;
  `/mnt/c` 9P forces `PollWatcher` per microsoft/WSL#4739. (high)

## Storage

- [sqlite-fts5](sqlite-fts5.md) - rusqlite 0.39 bundled (FTS5
  shipped) + refinery 0.9 + WAL. (high)

## Policy and security prior art

- [policy-prior-art](policy-prior-art.md) - advisory in MVP; cap-std
  `Dir` handles; Landlock and seccomp-bpf documented as post-MVP
  hardening; Landlock is available on WSL2 since kernel 5.15.57.1.
  (high)

## Conventions

- [license-decision](license-decision.md) - Apache-2.0 with SPDX
  per-file header; NOTICE optional but recommended. (high)
- [workspace-layout](workspace-layout.md) - flat virtual manifest,
  `members = ["crates/*"]`, resolver 3, seven crates,
  `[workspace.package]` + `[workspace.dependencies]` +
  `[workspace.lints]`. (high)
- [tooling-baseline](tooling-baseline.md) - rustfmt, clippy,
  cargo-deny 0.19, cargo-machete 0.9, cargo-hack 0.6,
  cargo-nextest 0.9; seven-step CI sequence. (high)

## Prior art

- [prior-art](prior-art.md) - Warp, Cursor, container-use, honeytail,
  Datadog agent, filesystem/github/fetch MCP servers, rust-mcp-stack.
  Behavior-only survey; no copied source. (high)
- [user-provided-evidence](user-provided-evidence.md) -
  README.md-cited evidence inventory used for SOURCE_MAP
  reclassification. (high)
