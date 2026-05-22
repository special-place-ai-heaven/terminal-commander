# Beta Release Checklist - Terminal Commander

Status: TC48 beta gate. NEVER auto-publishes; this is a manual
operator gate.

Language: ASCII only.

## Beta recommendation

**Conditional Go.**

Rationale:
- The TC33-TC47 runtime chain is complete. Every TC35-TC45 source-
  status is `live` (see `EVIDENCE_REPORT_RUNTIME.md`).
- The TC47 load / noise / backpressure gate passes 8/8 stress tests
  with the bounded-output / no-raw-stream / drop-counter invariants
  asserted.
- Provider-harness LIVE smokes for Codex CLI and Claude Code are
  `Not Run` on the verification host (TC46). Config-only examples
  ship in `docs/integrations/`. Operators must run their provider
  CLI against the shipped configs before calling the beta fully
  provider-validated.
- The Conditional Go ceiling reflects the provider gap, not a
  Terminal Commander defect. The local daemon + MCP stdio smoke
  passes end-to-end.

## Pre-flight (must pass on `main`)

- [ ] `git branch --show-current` == `main`
- [ ] `git status --short` clean
- [ ] `git diff --check` PASS
- [ ] `cargo metadata --no-deps` PASS
- [ ] `cargo fmt --all --check` PASS
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` PASS
- [ ] `cargo test --workspace` every suite green
- [ ] `cargo nextest run --workspace` PASS (347/347 at the TC47
      status commit `726a299`)
- [ ] `cargo test -p terminal-commanderd --test load_noise_backpressure
      -- --nocapture` PASS (TC47 regression, 8/8)
- [ ] `bash scripts/smoke/verify-runtime-smoke.sh` PASS (TC46
      regression)
- [ ] `rg "Command::new|Command::spawn|TcpListener|UdpSocket"
      crates/mcp` returns only doc / negative-assertion matches
- [ ] `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end"
      crates/mcp/src` returns no matches
- [ ] `cargo deny check licenses` PASS (legacy gate, kept)

## Provider-harness gate (out of CI; operator-driven)

- [ ] Codex CLI: real smoke run against
      `docs/integrations/codex-cli.md`. Transcript MUST show
      `tools/list` (>=29 tools) + a tool call (e.g.
      `command_start_combed` -> `bucket_wait` -> `command_status`).
- [ ] Claude Code: real smoke run against
      `docs/integrations/claude-code.md` (either `--mcp-config` or
      persistent settings form). Transcript MUST show `/mcp`
      discovery + a tool call.

Until BOTH provider boxes are checked AND the transcripts are
attached to a follow-up artifact, the beta posture stays
`Conditional Go`.

## Versioning

- Workspace version (`Cargo.toml [workspace.package].version`) is
  `0.0.0` during the runtime chain. First beta tag =
  `v0.1.0-beta.1`.
- Bump `version` before tagging; commit the bump as its own commit.
- Tag format: `vMAJOR.MINOR.PATCH[-PRERELEASE]`.

## Beta artifact

Beta does NOT publish to crates.io. Operators install via
`cargo install --path crates/{daemon,mcp,cli}` (see
`docs/install/README.md`).

A future release goal may add `cargo package` + `cargo publish`
wiring; that is OUT of TC48 scope.

## Cargo-deny gate (release-only stricter pass)

For release tags, the cargo-deny gate runs with `--all-features`:

```bash
cargo deny --all-features check
```

The standard `cargo deny check licenses` is the MVP minimum.

## Beta limitations (current, recorded honestly)

The TC31 baseline list is superseded. The following remain TRUE as
of TC47:

- Linux + WSL2 only. Windows-native targets are NOT supported; the
  MCP adapter and daemon refuse to start (Unix-only UDS + PTY).
  WSL2 is the supported Windows path.
- File-watch backend is poll-based at 120 ms (see TC43 prep
  amendment). Native notify/inotify is out of scope.
- Windows ConPTY is out of scope per TC44 `non_goals`.
- `frames_suppressed` daemon-side counter does NOT exist. Tests
  derive noise reduction from `frames_total / events_emitted`.
  Tracked in `BACKLOG.md` as P1.1.
- Dedicated file-watch and PTY megabyte-scale load tests are
  `Not Run` (TC47 final report). Existing TC43 / TC44 + TC47
  process load coverage is the proxy. Tracked in `BACKLOG.md` as
  P2.1 / P2.2.
- Codex CLI and Claude Code provider live smokes were `Not Run` on
  the verification host. Tracked in `BACKLOG.md` as P1.2 / P1.3,
  and in `RISK_REGISTER.md` as R-01.

## Doctrine snapshot (locked decisions, refreshed at TC48)

- License: Apache-2.0.
- Rust toolchain: 1.95.0 active (rmcp 1.7.0 MSRV floor 1.92).
- Storage: rusqlite 0.39 bundled + FTS5; manual migration runner
  (refinery 0.9 pinned rusqlite <=0.38; conflict resolved by
  manual runner — see TC12 commit message).
- Severity enum: 7-value union (trace/debug/info/low/medium/high/
  critical).
- Policy enforcement: advisory at beta (in-process + cap-std);
  Landlock + seccomp-bpf are roadmap.
- Default-deny path list: 14 suffixes (SECURITY.md section 5).
- Bucket retention: 24h TTL + 100_000 events; FIFO eviction with
  `dropped_count` counter.
- Per-frame size cap: 8192 bytes (`MAX_FRAME_BYTES`).
- Bucket-read limit: `MAX_BUCKET_READ_LIMIT = 10_000` events per
  call.
- Bucket wait timeout: tokio Notify-based; heartbeat on timeout
  with `next_cursor = max(tail, request.cursor)`.
- Context window caps: `MAX_CONTEXT_FRAMES = 1024`,
  `MAX_CONTEXT_BYTES = 64 KiB`.
- File read caps: `MAX_FILE_READ_LINES = 2000`,
  `MAX_FILE_READ_BYTES = 64 KiB`.
- File search caps: `MAX_FILE_SEARCH_MATCHES = 500`,
  `MAX_FILE_SEARCH_SNIPPET_BYTES = 512`,
  `MAX_FILE_SEARCH_SCAN_BYTES = 16 MiB`.
- PTY stdin cap: `MAX_PTY_STDIN_BYTES = 4096`.
- PTY dependency: `pty-process = "=0.5.3"` (MIT, async feature),
  Linux/WSL2 only.

## Sign-off

- Author commits the version bump + tag.
- Author runs `git push origin <tag>` only when EVERY checkbox in
  the Pre-flight section is checked AND the provider-harness gate
  ceiling is documented (either `Conditional Go` with Codex /
  Claude Code transcripts pending, or `Go` with both transcripts
  attached).
- This file is the authoritative checklist. Out-of-band release
  steps are not permitted.
