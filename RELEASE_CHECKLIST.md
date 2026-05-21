# Beta Release Checklist - Terminal Commander

Status: TC31 baseline. NEVER auto-publishes; this is a manual
operator gate.

Language: ASCII only.

## Pre-flight (must pass on the release branch)

- [ ] `git branch --show-current` == `feature/terminal-commander-mvp`
- [ ] `git status --short` clean
- [ ] `cargo fmt --all --check` PASS
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` PASS
- [ ] `cargo nextest run --workspace` PASS
- [ ] `cargo deny check licenses` PASS
- [ ] `bash scripts/dev/verify-baseline.sh` PASS
- [ ] All `tests/fixtures/contracts/*.json` parse via
      `python3 -m json.tool`
- [ ] All seven rule packs import without skipping
      (`cargo nextest run -p terminal-commander-store
      --filter-expr 'test(import)'`)

## Versioning

- Workspace version (`Cargo.toml [workspace.package].version`) is
  `0.0.0` during MVP. First beta tag = `v0.1.0-beta.1`.
- Bump `version` before tagging; commit the bump as its own commit.
- Tag format: `vMAJOR.MINOR.PATCH[-PRERELEASE]`.

## Beta artifact

Beta does NOT publish to crates.io. Operators install via
`cargo install --path crates/{daemon,mcp,cli}` (see
`docs/install/README.md`).

A future release goal may add `cargo package` + `cargo publish`
wiring; that is OUT of TC31 scope.

## Cargo-deny gate (release-only stricter pass)

For release tags, the cargo-deny gate runs with `--all-features`:

```bash
cargo deny --all-features check
```

The standard `cargo deny check licenses` is the MVP minimum.

## Known beta limitations

- rmcp stdio adapter is deferred (TC23 follow-up).
- pty-process spawn path is deferred (TC15 follow-up; ANSI
  normalization and prompt detection are live).
- process-wrap process-group integration is deferred (TC15
  follow-up).
- Daemon IPC transport is in-process only (TC21 follow-up).
- Audit log persistence to SQLite is deferred (TC22 introduced
  the policy engine; persistent audit log writes are post-MVP).
- macOS and Windows-native targets are deferred (Linux + WSL2 only).
- notify 8.2 inotify path is deferred (file/directory probes use
  polling in MVP).

## Doctrine snapshot (locked decisions)

- License: Apache-2.0.
- Rust toolchain: 1.95.0 active (rmcp 1.7.0 MSRV floor 1.92).
- Storage: rusqlite 0.39 bundled + FTS5; manual migration runner
  (refinery 0.9 pinned rusqlite <=0.38; conflict resolved by
  manual runner — see TC12 commit message).
- Severity enum: 7-value union (trace/debug/info/low/medium/high/
  critical).
- Policy enforcement: advisory at MVP (in-process + cap-std);
  Landlock + seccomp-bpf are roadmap.
- Default-deny path list: 14 suffixes (SECURITY.md section 5).
- Bucket retention: 24h TTL + 100_000 events; FIFO eviction with
  dropped_count counter.
- Per-frame size cap: 8192 bytes (MAX_FRAME_BYTES).
- Bucket wait timeout: tokio Notify-based; heartbeat on timeout
  with next_cursor=max(tail, request.cursor).

## Sign-off

- Author commits the bump + tag.
- Author runs `git push origin <tag>` only when every checkbox
  above is checked.
- This file is the authoritative checklist. Out-of-band release
  steps are not permitted.
