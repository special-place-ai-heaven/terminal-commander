# Async Runtime Choice for Terminal Commander

Topic: A2
Author: research agent R1-alpha
Date: 2026-05-21
Confidence: high

## Recommendation

Tokio, single-runtime project-wide. All async crates in the workspace
must declare features that target tokio; do not mix tokio with smol
or async-std in the same process.

## Cross-reference: what does rmcp 0.16.0 require?

rmcp depends directly on `tokio = "1"` with features
`["sync", "macros", "rt", "time"]`. The crate also exposes
transport modules whose names presume a tokio runtime
(`TokioChildProcess`, `StreamableHttpService`, etc.).

There is no smol or async-std support in rmcp. Choosing rmcp locks
the project to tokio.

Sources:

- rmcp Cargo.toml at
  https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/Cargo.toml
- rmcp README at
  https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/README.md
  states: "Built on tokio's async runtime."

## Runtime comparison

### tokio (recommended)

- Latest version: 1.52.3, released 2026-05-08.
- MSRV: 1.71 (per current tokio README).
- License: MIT.
- Status: actively maintained, dominant Rust async runtime.
- Used by rmcp directly.

Source: https://lib.rs/crates/tokio

### async-std

- Latest version: 1.13.2, released 2025-08-15.
- License: Apache-2.0 OR MIT.
- Status: DISCONTINUED. The maintainers explicitly recommend
  switching to smol.
- Quote from project: "async-std has been discontinued; use smol
  instead."

Source: https://lib.rs/crates/async-std

Trade-off: not viable for a new project in 2026.

### smol

- Latest version: 2.0.2, released 2024-09-07.
- MSRV: 1.63.
- License: Apache-2.0 OR MIT.
- Status: maintained, lighter weight than tokio.

Source: https://lib.rs/crates/smol

Trade-off: rmcp does not support smol. Picking smol would force a
hand-rolled MCP server stack on top of raw JSON-RPC, losing the official
SDK and the conformance harness. Rejected.

## PTY crate runtime compatibility

This is a deciding factor because Terminal Commander needs PTY support
for terminal probes.

### portable-pty (wezterm/pty)

- Latest version: 0.9.0, released 2025-02-11.
- License: MIT.
- Async: NO direct tokio support. Dev-dependencies include `smol`,
  no `tokio` runtime dependency. The crate exposes a blocking
  `Reader`/`Writer` interface; callers must move IO onto a runtime
  manually (e.g. `tokio::task::spawn_blocking` or a thread).

Source: https://raw.githubusercontent.com/wez/wezterm/main/pty/Cargo.toml
(quoted: "There is no tokio dependency.")

Trade-off: usable with tokio but only via blocking-thread bridge.
Lowest-common-denominator cross-platform PTY (Linux + macOS + Windows
ConPTY). See `pty-crate.md` for full PTY comparison.

### pty-process (doy/pty-process)

- Latest version: 0.5.3, released 2025-07-12.
- Edition: 2024.
- License: MIT.
- Async: YES, via optional `async` feature that pulls
  `tokio = "1.46.1"` with `["fs", "process", "net"]`.
- Built on `rustix` for the POSIX PTY syscalls.

Sources:

- https://raw.githubusercontent.com/doy/pty-process/main/Cargo.toml
- https://github.com/doy/pty-process

Trade-off: clean tokio integration but POSIX-only (uses rustix `pty`
module). Windows is not supported. Acceptable given the platform
decision (Linux native + WSL2 primary; macOS/Windows-native deferred).

### tokio-pty-process

Not separately verified in this research pass. Older, low-maintenance
in the ecosystem; superseded in practice by `pty-process` with the
`async` feature. Treat as not recommended unless future evidence
contradicts.

## File watcher runtime compatibility

### notify

- Latest version: 9.0.0-rc.4, released 2026-05-02.
- MSRV: 1.88.
- License: notify itself is CC Zero 1.0; notify-types is MIT OR
  Apache-2.0.
- Runtime: runtime-agnostic. Optional integrations available with
  tokio, crossbeam-channel, flume, or futures channels (selectable
  via feature flags).
- Platforms: Linux/Android via inotify, macOS via FSEvents or kqueue,
  Windows via ReadDirectoryChangesW, BSDs via kqueue, polling fallback
  everywhere.

Source: https://lib.rs/crates/notify

Trade-off: works well with tokio. Use the tokio-aware adapter feature
and forward events to tokio channels in the daemon.

## Recommendation summary

| Component | Choice | Why |
|---|---|---|
| Runtime | tokio | rmcp requires it; ecosystem default. |
| PTY (MVP) | pty-process with `async` feature | Clean tokio fit, POSIX. |
| PTY (cross-platform later) | portable-pty + spawn_blocking | Adds Windows ConPTY when needed. |
| File watch | notify with tokio adapter | Runtime-agnostic, mature. |
| SQLite | rusqlite + tokio-rusqlite OR sqlx (sqlite) | Decision deferred to store goal. |

## Confidence

High. tokio is forced by rmcp; smol/async-std elimination is hard
evidence (smol unsupported by rmcp; async-std discontinued).

## SOURCE_MAP reclassification

Runtime = tokio: was inferred, now evidence-backed via rmcp Cargo.toml
direct dependency on `tokio = "1"`.
