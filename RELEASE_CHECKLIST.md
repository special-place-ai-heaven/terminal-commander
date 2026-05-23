# Beta Release Checklist - Terminal Commander

Status: TC48 beta gate, **refreshed at NPM09** (terminal-commander-npm-distribution chain close, 2026-05-23). NEVER auto-publishes; this is a manual operator gate.

Language: ASCII only.

## Beta recommendation

**Conditional Go.** (TC48 baseline, preserved through NPM01-NPM09.)

Rationale:
- The TC33-TC47 runtime chain is complete. Every TC35-TC45 source-
  status is `live` (see `EVIDENCE_REPORT_RUNTIME.md`).
- The TC47 load / noise / backpressure gate passes 8/8 stress tests
  with the bounded-output / no-raw-stream / drop-counter invariants
  asserted.
- The NPM01-NPM08b distribution chain landed: npm wrapper + platform
  packages (NPM02-NPM03), local install smoke (NPM04), CI build
  matrix linux-x64 + linux-arm64 (NPM05), release-please manifest
  mode (NPM06), npm trusted-publishing workflow OIDC-gated (NPM07),
  Cursor MCP docs + examples (NPM08), canonical public README
  (NPM08b). See `docs/release/npm-distribution-final-report.md`
  for the chain close evidence and the per-goal completion commits.
- Provider-harness LIVE smokes for Codex CLI, Claude Code, AND
  Cursor are `Not Run` on the verification host. Config-only
  examples ship in `docs/integrations/` + `examples/provider-harness/cursor/`.
- First live npm publish is **Pending** two operator-driven steps:
  npmjs.com `@terminal-commander` org claim + trusted-publisher
  config (workflow filename `release-please.yml`), AND a Conventional-
  Commits `feat:` / `fix:` commit followed by a merged release PR.
- The Conditional Go ceiling reflects the provider gap + the
  operator-preconditioned first publish, not a Terminal Commander
  defect. The local daemon + MCP stdio + npm-install smokes pass
  end-to-end.

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
- [ ] Cursor: real smoke run against `docs/integrations/cursor.md`
      using one of the `examples/provider-harness/cursor/*.json`
      configs (native Linux / inside-WSL OR Windows-to-WSL bridge).
      Transcript / screenshot MUST show Cursor `Settings -> MCP`
      reporting `terminal-commander` connected with the 29-tool
      catalogue + a tool call (e.g. `health` or `command_start_combed`
      -> `bucket_wait` -> `command_status`).

Until ALL THREE provider boxes are checked AND the transcripts are
attached to a follow-up artifact, the beta posture stays
`Conditional Go`.

## npm distribution gate (added at NPM09 close)

- [ ] `npm view terminal-commander version` returns the published
      beta version (currently E404 / unpublished).
- [ ] `npm view @terminal-commander/linux-x64 version` returns the
      same version.
- [ ] `npm view @terminal-commander/linux-arm64 version` returns the
      same version.
- [ ] `npm pack --dry-run` clean for all three packages (root +
      both platform packages). Last NPM09 local verification:
      7 / 5 / 5 files at `0.1.0-beta.1`.
- [ ] Root `optionalDependencies` exact-pin both platform packages
      to the shared version (no `^` / `~` ranges).
- [ ] `.github/.release-please-manifest.json` agrees with all three
      `package.json` version fields.
- [ ] `npm-binary-build` workflow latest run on `main` is `success`
      on both `ubuntu-24.04` (full smoke) and `ubuntu-24.04-arm`
      (build + pack).
- [ ] `release-please` workflow latest run on `main` is `success`
      and the three publish jobs were correctly `skipped` if no
      release PR was merged on that push (gate
      `releases_created='true'`).
- [ ] No `NPM_TOKEN`, `NPM_TOKEN_TC`, `CARGO_REGISTRY_TOKEN_TC`, or
      `RELEASE_PLEASE_TOKEN_TC` reference exists in any active
      workflow path.
- [ ] No `cargo publish` / `crates.io` step in any workflow.
- [ ] No `postinstall` script in any `package.json`.

## Operator preconditions for first live npm publish (NPM07/NPM09)

- [ ] `@terminal-commander` organization claimed on npmjs.com.
- [ ] All three names reserved on npmjs.com.
- [ ] Trusted publisher configured for each of the three packages
      with Publisher=`GitHub Actions`, Owner=`special-place-administrator`,
      Repository=`terminal-commander`, Workflow filename=
      `release-please.yml`, Environment=blank.
- [ ] A Conventional-Commits `feat:` or `fix:` commit lands on
      `main`, release-please opens a release PR, operator reviews
      and merges it. Only then do the publish jobs fire.

Until ALL operator preconditions complete, the first live npm
publish remains `Pending`. No token fallback is permitted.

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
