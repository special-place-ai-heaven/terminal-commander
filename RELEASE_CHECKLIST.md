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
      reporting `terminal-commander` connected with the 38-tool
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
- [ ] No `preinstall`, `install`, or `postinstall` lifecycle script in any npm package.

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
publish via NPM07's OIDC path remains `Pending`. **NPM10 adds a
one-time bootstrap exception** (see below) for the case where
npmjs.com requires the package page to exist before the
trusted-publisher UI can be configured.

## NPM10 bootstrap exception (one-time NPM_TOKEN_TC path)

Use this path **only if** the operator determines that
npmjs.com's trusted-publisher UI requires the package page to
exist before configuration. Full policy:
[`docs/release/npm-bootstrap-first-publish.md`](docs/release/npm-bootstrap-first-publish.md).

Workflow: `.github/workflows/npm-bootstrap-publish.yml`. Trigger:
`workflow_dispatch` only. Two-gate confirm:

- `dry_run` (boolean, default `true`)
- `confirm_publish` (string, must equal
  `publish-terminal-commander-beta` for real publish)

Default execution is `npm publish --dry-run`. Real publish
requires both gates flipped. Auth: `secrets.NPM_TOKEN_TC` via
`NODE_AUTH_TOKEN`. **No provenance** on token publish (intentional).

Post-NPM10-success operator steps (required before any further
publish):

- [ ] Configure trusted publisher on every package page on
      npmjs.com (workflow filename `release-please.yml`).
- [ ] Disable or remove `.github/workflows/npm-bootstrap-publish.yml`.
- [ ] Rotate / invalidate `NPM_TOKEN_TC`.
- [ ] Confirm next release flows entirely through `release-please.yml`
      (OIDC + provenance).

Until ALL post-success steps complete, the chain is NOT considered
to have closed the bootstrap exception cleanly. The OIDC contract
remains the standing capability; `NPM_TOKEN_TC` is a single-use
key.

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

- License: PolyForm-Noncommercial-1.0.0.
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

## Windows + WSL bridge chain (WWS01–WWS07, WWS08 current)

Status added by WWS08 (docs-only); does NOT modify any publish
gate locked at NPM09. The WWS chain shipped JS-only Windows
control-plane surfaces that wrap the existing Linux/WSL2 runtime;
no version bump and no workflow change at WWS08.

- [x] WWS01 Windows + WSL install UX contract live
      (`docs/release/windows-wsl-bridge-contract.md`, commit
      `6220eb2`; 15 binding decisions D-01..D-15).
- [x] WWS02 root npm package `os: ["linux", "win32"]` widened;
      `bridge_required` resolver branch; bounded shim refusals
      on Windows (commit `1da40f3`).
- [x] WWS03 WSL discovery + read-only doctor helpers shipped
      (`lib/wsl/{distro-name,detect,doctor}.js`, commit
      `ec8441e`).
- [x] WWS04 Windows → WSL `terminal-commander-mcp` bridge shim
      shipped (`lib/wsl/spawn.js`, commit `d86e73f`).
- [x] WWS05 Cursor MCP config writer shipped
      (`lib/cursor/{config,write,index}.js`, commit `ae37878`).
- [x] WWS06 setup / doctor / pair CLI shipped (`lib/cli/**`,
      commit `4936904`). Five subcommands locked; 21-status
      enum; `--install-wsl-runtime` opt-in; no sudo; no
      password.
- [x] WWS07 Windows bridge smoke script shipped
      (`scripts/smoke/verify-windows-bridge-smoke.ps1`, commit
      `785d410`). Windows CLI / config-writer / doctor PASS on
      the verification host.
- [ ] WWS07 Windows → WSL MCP bridge round-trip (`initialize` +
      `tools/list` + `tools/call(health)` through the WWS04
      bridge): **Not Run** = `runtime_missing` because the WSL
      distro lacks `terminal-commander-mcp` until the first npm
      publish lands.
- [ ] Cursor provider live smoke transcript: **Not Run** (no
      headless Cursor MCP discovery entry point; operator GUI
      steps required).
- [ ] WWS09 pre-publish readiness review: Pending.

Inherited / preserved from earlier chains (NOT modified by WWS08):

- [ ] First live npm publish: pending operator npmjs.com
      trusted-publisher setup + release PR merge (see
      `docs/release/npm-trusted-publishing-contract.md` §8).
- [ ] `npm-bootstrap-publish.yml` disable / rotate after first
      publish (BACKLOG P1.5b, inherited from NPM10).

The WWS chain did NOT modify any of:

- `crates/**` (Rust workspace untouched; 347/347 nextest PASS
  preserved).
- `Cargo.toml` / `Cargo.lock`.
- `.github/**` (no workflow change; `release-please.yml`,
  `npm-binary-build.yml`, `npm-bootstrap-publish.yml`,
  trusted-publishing surfaces all untouched).
- `packages/*/package.json` (root + both platform packages
  byte-identical; `0.1.0-beta.1` preserved).
- `packages/terminal-commander-linux-x64/**` /
  `packages/terminal-commander-linux-arm64/**` (platform
  packages byte-identical).
- `scripts/` other than the new WWS07 PowerShell smoke
  (existing `verify-runtime-smoke.sh`,
  `verify-npm-local-install.sh` byte-identical).
- `rules/**` / `config/**` (rule packs + daemon config example
  byte-identical).
- No new MCP tool added; 29-tool TC45 catalogue unchanged.
- No daemon change. No IPC change. No raw stream endpoint
  added. No network listener added. No postinstall downloader.
