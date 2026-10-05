# Beta Release Checklist - Terminal Commander

Status: TC48 beta gate, **refreshed at NPM09** (terminal-commander-npm-distribution chain close, 2026-05-23). NEVER auto-publishes; this is a manual operator gate.

Language: ASCII only.

> Update 2026-10-05: this file is a beta-era (TC48 / NPM09) snapshot that
> was only partly kept current. Several gates below were rewritten on
> 2026-10-05 because they contradicted the live pipeline: the package is
> published (0.3.11, five platform packages), releases flow through
> `release-please.yml` + `release-pr-sync.yml` (auto-merged release PR)
> with npm OIDC trusted publishing + `--provenance`, crates.io is
> published by the same workflow, and Windows-native and macOS packages
> ship. Lines marked "(historical)" are the original beta record. See
> `docs/release/release-pipeline-invariants.md` for the current rules.

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
- (historical) First live npm publish was **Pending** two operator-driven
  steps: npmjs.com `@terminal-commander` org claim + trusted-publisher
  config (workflow filename `release-please.yml`), AND a Conventional-
  Commits `feat:` / `fix:` commit followed by a merged release PR.
  Update 2026-10-05: DONE; the first publish landed 2026-07-17 (see
  `BACKLOG.md` P1.5) and the current version is 0.3.11.
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
      status commit `726a299`; historical count, the workspace has
      far more tests now)
- [ ] `cargo test -p terminal-commanderd --test load_noise_backpressure
      -- --nocapture` PASS (TC47 regression, 8/8)
- [ ] `bash scripts/smoke/verify-runtime-smoke.sh` PASS (TC46
      regression)
- [ ] `rg "Command::new|Command::spawn|TcpListener|UdpSocket"
      crates/mcp` returns only doc / negative-assertion matches
- [ ] `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end"
      crates/mcp/src` returns no matches
- [ ] `cargo deny check` PASS (all four checks; CI also enforces it in
      `pre-build-gates (linux-x64)`)

## Provider-harness gate (out of CI; operator-driven)

- [ ] Codex CLI: real smoke run against
      `docs/integrations/codex-cli.md`. Transcript MUST show
      `tools/list` (60 tools on the full surface; six facade tools
      on the compact surface) + a tool call (e.g.
      `command_start_combed` -> `bucket_wait` -> `command_status`).
- [ ] Claude Code: real smoke run against
      `docs/integrations/claude-code.md` (either `--mcp-config` or
      persistent settings form). Transcript MUST show `/mcp`
      discovery + a tool call.
- [ ] Cursor: real smoke run against `docs/integrations/cursor.md`
      using one of the `examples/provider-harness/cursor/*.json`
      configs (native Linux / inside-WSL OR Windows-to-WSL bridge).
      Transcript / screenshot MUST show Cursor `Settings -> MCP`
      reporting `terminal-commander` connected with the full tool
      catalogue (60 tools; 38 at the time this gate was written) + a tool call (e.g. `health` or `command_start_combed`
      -> `bucket_wait` -> `command_status`).

Until ALL THREE provider boxes are checked AND the transcripts are
attached to a follow-up artifact, the beta posture stays
`Conditional Go`.

## npm distribution gate (added at NPM09 close)

- [ ] `npm view terminal-commander version` returns the published
      version (published; 0.3.11 at 2026-10-05; the old "currently
      E404" note is obsolete).
- [ ] `npm view @terminal-commander/<platform> version` returns the
      same version for each of `linux-x64`, `linux-arm64`,
      `windows-x64`, `mac-x64`, `mac-arm64`.
- [ ] `npm pack --dry-run` clean for all six packages (root + five
      platform packages). (historical: 7 / 5 / 5 files at
      `0.1.0-beta.1` for the then three packages.)
- [ ] Root `optionalDependencies` exact-pin all five platform packages
      to the shared version (no `^` / `~` ranges); verified by
      `scripts/release/verify-optional-dependencies.js`.
- [ ] `.github/.release-please-manifest.json` agrees with all six
      `package.json` version fields.
- [ ] `npm-binary-build` workflow latest run on `main` is `success`:
      both `pre-build-gates` jobs (linux-x64, windows-x64), all five
      `build-*` legs, and `npm-pack`.
- [ ] `release-please` workflow latest run on `main` is `success`
      and the publish jobs were correctly `skipped` if no
      release PR was merged on that push (gate
      `releases_created='true'`).
- [ ] No npm publish job in `release-please.yml` authenticates with a
      token: npm publishing uses OIDC trusted publishing +
      `--provenance` (no `NODE_AUTH_TOKEN`). (The original gate "no
      `NPM_TOKEN_TC` / `CARGO_REGISTRY_TOKEN_TC` / `RELEASE_PLEASE_TOKEN_TC`
      anywhere" is obsolete: `RELEASE_PLEASE_TOKEN_TC` and
      `CARGO_REGISTRY_TOKEN_TC` are in use by design, and `NPM_TOKEN_TC` is
      still read by `deprecate-version.yml`;
      per the owner it is now a granular stage-only token that cannot publish
      directly.)
- [ ] crates.io publishing happens only in the `release-please.yml`
      cargo-publish chain (token auth, `CARGO_REGISTRY_TOKEN_TC`; crates.io
      has no trusted-publishing cutover yet). (The original gate "no
      `cargo publish` / `crates.io` step in any workflow" is obsolete.)
- [ ] The root npm package has exactly one lifecycle script,
      `postinstall` (`node scripts/postinstall.js` in `packages/terminal-commander/`, a guarded, fail-soft,
      CI/opt-out-aware bootstrap: harness auto-setup, and on Linux/WSL daemon
      autostart via `packages/terminal-commander/lib/daemon/autostart.js`); no
      `preinstall` / `install` script, and none in the platform
      packages. Pinned by
      `packages/terminal-commander/test/av-safe-install-runtime.test.js`.
      (The original gate "no postinstall in any npm package" was reversed
      deliberately by commit `393f89c`, 2026-06-25.)

## Operator preconditions for first live npm publish (NPM07/NPM09)

(historical; satisfied. Update 2026-10-05: npm publishes through
`release-please.yml` OIDC trusted publishing succeed, commit `23a407a`.)

- [x] `@terminal-commander` organization claimed on npmjs.com (the
      scoped platform packages are published; `.github/.release-please-manifest.json`
      lists them at 0.3.11).
- [x] All names reserved on npmjs.com (six packages now, not three).
- [x] Trusted publisher configured for each package with Publisher=`GitHub
      Actions`, Owner=`special-place-administrator`,
      Repository=`terminal-commander`, Workflow filename=
      `release-please.yml`, Environment=blank. Evidence:
      `release-please.yml` publishes with `--provenance` and no token, so a
      misconfigured publisher would fail the release. (The npmjs.com
      settings page itself is not visible from the repo.)
- [x] A Conventional-Commits `feat:` or `fix:` commit lands on
      `main`, release-please opens a release PR, and it is merged. Now
      automatic: `release-pr-sync.yml` auto-merges after required checks.

Until ALL operator preconditions complete, the first live npm
publish via NPM07's OIDC path remains `Pending`. **NPM10 adds a
one-time bootstrap exception** (see below) for the case where
npmjs.com requires the package page to exist before the
trusted-publisher UI can be configured.

## NPM10 bootstrap exception (one-time NPM_TOKEN_TC path; RETIRED)

This exception covered the case where npmjs.com's trusted-publisher UI
required the package page to exist before configuration. The first publish
has landed, and the bootstrap workflow `npm-bootstrap-publish.yml`
(`workflow_dispatch` only, `secrets.NPM_TOKEN_TC`, no provenance) was
deleted on 2026-10-05. `release-please.yml` (OIDC trusted publishing) is the
only publish path. Historical policy:
[`docs/release/npm-bootstrap-first-publish.md`](docs/release/npm-bootstrap-first-publish.md).

Post-NPM10-success operator steps (required before any further
publish):

- [x] Configure trusted publisher on every package page on
      npmjs.com (workflow filename `release-please.yml`); see the evidence
      above.
- [x] Disable or remove `.github/workflows/npm-bootstrap-publish.yml`:
      done 2026-10-05, the file is deleted.
- [x] Rotate / invalidate `NPM_TOKEN_TC`: done 2026-10-05 per the owner (the
      old token was replaced with a granular, stage-only token scoped to
      `terminal-commander` and `@terminal-commander`; it can deprecate but
      not publish). Not checkable from the repo.
- [x] Confirm next release flows entirely through `release-please.yml`
      (OIDC + provenance): the workflow has no npm token on publish jobs.

(Update 2026-10-05: `NPM_TOKEN_TC` is still read by
`deprecate-version.yml` (it needs it
to deprecate versions); it is now a granular stage-only token that cannot
publish directly, and the weekly secret-health probe passes again, per the
owner. See `BACKLOG.md` P1.5b.)

All post-success steps are complete. The OIDC contract is the standing
capability.

## Versioning

- Versions are owned by release-please (manifest
  `.github/.release-please-manifest.json`, config
  `.github/release-please-config.json`); the workspace version in
  `Cargo.toml` is 0.3.11 at 2026-10-05 and carries an
  `x-release-please-version` marker. Do NOT hand-bump or hand-tag: merging
  the release PR bumps versions and creates the tag. (historical: `0.0.0`
  during the runtime chain, first beta tag `v0.1.0-beta.1`.)
- Tag format: `vMAJOR.MINOR.PATCH[-PRERELEASE]`.
- README `<!-- release-status -->` line (under "Recent improvements") is
  stamped "released in vX.Y.Z" by the release-pr-sync job; check the release PR
  diff shows it. The prepublish gate fails the release if it still says
  "not yet in a tagged release".

## Beta artifact

(historical: beta did NOT publish to crates.io.) Update 2026-10-05: the
crates are published to crates.io by the `release-please.yml` cargo-publish
chain (`scripts/release/publish-cargo-crate.js`); the primary install is
`npm install -g terminal-commander@latest` (see `docs/install/README.md`).
`cargo install --path crates/{daemon,mcp,cli}` still works from a checkout.

## Cargo-deny gate

CI runs `cargo deny check` on every PR and push to `main`
(`pre-build-gates (linux-x64)` in `npm-binary-build.yml`); `deny.toml` sets
`all-features = true`, so no extra flag is needed:

```bash
cargo deny check
```

## Beta limitations (current, recorded honestly)

The TC31 baseline list is superseded. The following were TRUE as
of TC47 (items marked "Update 2026-10-05" have since changed):

- Linux + WSL2 only. Windows-native targets are NOT supported; the
  MCP adapter and daemon refuse to start (Unix-only UDS + PTY).
  WSL2 is the supported Windows path. Update 2026-10-05: no longer
  true. Native Windows is supported (named-pipe IPC, ConPTY backend,
  `@terminal-commander/windows-x64` is the default Windows package; the
  WSL bridge is opt-in via `TC_USE_LEGACY_WSL_BRIDGE=1`) and macOS
  platform packages ship (live macOS verification still open).
- File-watch backend is poll-based at 120 ms (see TC43 prep
  amendment). Native notify/inotify is out of scope. Update 2026-10-05:
  a `notify` backend shipped (omni P3); polling remains for WSL `/mnt/c`.
- Windows ConPTY is out of scope per TC44 `non_goals`. Update
  2026-10-05: shipped (omni P3, `portable-pty` in `crates/probes`).
- `frames_suppressed` daemon-side counter does NOT exist. Tests
  derive noise reduction from `frames_total / events_emitted`.
  Tracked in `BACKLOG.md` as P1.1. Update 2026-10-05: the counter exists
  (see `BACKLOG.md` P1.1, RESOLVED).
- Dedicated file-watch and PTY megabyte-scale load tests are
  `Not Run` (TC47 final report). Existing TC43 / TC44 + TC47
  process load coverage is the proxy. Tracked in `BACKLOG.md` as
  P2.1 / P2.2.
- Codex CLI and Claude Code provider live smokes were `Not Run` on
  the verification host. Tracked in `BACKLOG.md` as P1.2 / P1.3,
  and in `RISK_REGISTER.md` as R-01.

## Doctrine snapshot (locked decisions, refreshed at TC48)

- License: PolyForm-Noncommercial-1.0.0.
- Rust toolchain: 1.97.1 active (rmcp =3.4.1; MSRV floor 1.92).
- Storage: rusqlite 0.40.2 bundled + FTS5 (`crates/store/Cargo.toml`); manual migration runner
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
- PTY dependency: `pty-process = "=0.5.3"` (MIT, async feature) on
  unix; `portable-pty` (ConPTY) on Windows.

## Sign-off

- (historical beta process) Author commits the version bump + tag and
  runs `git push origin <tag>` once the Pre-flight section is checked.
  Update 2026-10-05: not how releases happen now. release-please opens
  the release PR, `release-pr-sync.yml` auto-merges it after required
  checks, and the workflow tags and publishes (`.github/workflows/`).
  The Pre-flight list is a manual cross-check, not a gate that blocks
  that automation; `release-pipeline-invariants.md` describes what the
  pipeline enforces.

## Windows + WSL bridge chain (WWS01–WWS07, WWS08 current)

(historical record as of WWS08. Update 2026-10-05: the root package `os`
list is now `linux`, `win32`, `darwin`; native Windows is the default and
the WSL bridge is opt-in, see `docs/install/README.md`; version is 0.3.11.
The unchecked WWS items below were not re-verified.)

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
      (`packages/terminal-commander/lib/wsl/{distro-name,detect,doctor}.js`, commit
      `ec8441e`).
- [x] WWS04 Windows → WSL `terminal-commander-mcp` bridge shim
      shipped (`packages/terminal-commander/lib/wsl/spawn.js`, commit `d86e73f`).
- [x] WWS05 Cursor MCP config writer shipped
      (`packages/terminal-commander/lib/cursor/{config,write,index}.js`, commit `ae37878`).
- [x] WWS06 setup / doctor / pair CLI shipped (`packages/terminal-commander/lib/cli/**`,
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
      publish lands. Update 2026-10-05: the publish blocker is gone, but the
      bridge is now opt-in (`TC_USE_LEGACY_WSL_BRIDGE=1`) and no workflow runs
      `verify-windows-bridge-smoke.ps1`; NOT DETERMINABLE from the repo (needs
      a real Windows + WSL machine).
- [ ] Cursor provider live smoke transcript: **Not Run** (no
      headless Cursor MCP discovery entry point; operator GUI
      steps required). NOT DETERMINABLE from the repo (human GUI check).
- [x] WWS09 pre-publish readiness review: closed 2026-05-23
      (`docs/release/windows-wsl-bridge-final-report.md`).

Inherited / preserved from earlier chains (NOT modified by WWS08):

- [x] First live npm publish: DONE 2026-07-17 (`BACKLOG.md` P1.5); current
      version 0.3.11.
- [x] `npm-bootstrap-publish.yml` disable / rotate after first
      publish (BACKLOG P1.5b, inherited from NPM10): rotate DONE
      2026-10-05 per the owner; the workflow was deleted 2026-10-05.

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
