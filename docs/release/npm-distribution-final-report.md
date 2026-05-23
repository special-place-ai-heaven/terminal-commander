# NPM09 — terminal-commander-npm-distribution chain final report

Status: NPM09 deliverable, terminal goal of the chain.
Branch: `main`.
Date: 2026-05-23.
Depends on: NPM01 → NPM02 → NPM03 → NPM04 → NPM05 → NPM06 → NPM07 → NPM08 → NPM08b (all `Completed`).

This document is the chain-closing evidence consolidation. It does
NOT publish, merge a release PR, create a tag, or modify runtime
behavior. The recommendation at the end of the document is
operator-actionable but operator-driven; NPM09 does not promote
the beta posture without explicit operator sign-off.

Language: ASCII only.

## 1. Chain summary (per-goal completion + commit)

| Goal  | Title | Completion commit | Status |
|-------|-------|-------------------|--------|
| NPM01 | Memory and Symforge release audit | `5dcbaa4` | Completed |
| NPM02 | npm binary packaging contract | `81f4ea1` | Completed |
| NPM03 | Wrapper package and platform package layout | `00f3ddb` | Completed |
| NPM04 | Local `npm pack` and global install smoke | `970db2c` | Completed |
| NPM05 | GitHub Actions build matrix | `2bbf2fd` | Completed |
| NPM06 | release-please manifest config | `e81eb3f` | Completed |
| NPM07 | npm trusted publishing workflow | `1b4267e` | Completed |
| NPM08 | Cursor MCP install config smoke | `6ab2343` | Completed |
| NPM08b| README project normative overhaul | `cacfef5` | Completed |
| NPM09 | Release dry-run and beta publish review | (this report) | In progress |

All artifacts referenced below are committed on `main`. Live
GitHub Actions evidence references real run IDs.

## 2. Release readiness recommendation

**Conditional Go.**

The TC48 baseline beta posture is preserved through the entire NPM
chain. The local runtime, the npm package layout, the CI build
matrix, the release-please manifest, the trusted-publishing
workflow, the local npm install smoke, the Cursor integration
docs, and the public-facing README are all `live`. The two
Conditional Go gates remain:

1. Provider live smoke (Cursor / Codex CLI / Claude Code) is
   **Not Run** on the verification host.
2. The first live npm publish is **Pending** two operator-driven
   steps:
   - npmjs.com `@terminal-commander` org claim + trusted-publisher
     configuration for all three package names with workflow
     filename `release-please.yml`.
   - A Conventional-Commits `feat:` / `fix:` commit lands on
     `main`, release-please opens a release PR, and the operator
     reviews and merges it.

`Not Run` is **not** PASS. Beta cannot promote to `Go` without at
least one provider live smoke transcript attached. NPM09 honestly
records the gap.

## 3. Per-goal acceptance

### NPM01 — Memory and Symforge release audit
- Audit `docs/release/npm-distribution-audit.md` evidence-backed
  (agentmemory + remindb + Symforge repo + Terminal Commander repo)
  and recommendations recorded per goal.
- 18 sections; recommended divergences from Symforge precedent
  locked: platform packages via `optionalDependencies`, OIDC
  trusted publishing, no `crates.io` dual publish, no macOS /
  Windows targets.

### NPM02 — npm binary packaging contract
- Contract `docs/release/npm-binary-packaging-contract.md` locks
  package names, install command, platform contract, package
  architecture (root + scoped platform packages via
  `optionalDependencies`, no postinstall, no RT compile),
  versioning contract (one shared semver, initial
  `0.1.0-beta.1`, no crates.io publish), release contract (OIDC
  trusted publishing + provenance + review-gated release PR),
  Cursor contract, safety contract.
- npm name availability verified 2026-05-23: all 4 candidate
  names returned E404 (available).

### NPM03 — Wrapper package and platform package layout
- `packages/terminal-commander/` (root wrapper) + `packages/terminal-commander-linux-x64/`
  + `packages/terminal-commander-linux-arm64/` shipped per NPM02 §5.
- Root carries the three JS bin shims + `lib/resolve-binary.js`
  resolver + 12 resolver unit tests (all pass).
- Platform packages carry `*.placeholder` files (real binaries
  produced by NPM05 CI; never committed back).

### NPM04 — Local npm install smoke
- `scripts/smoke/verify-npm-local-install.sh` (347 lines) builds
  release binaries → stages → `npm pack` platform + root →
  installs into sandbox `--prefix` → runs daemon self-check + MCP
  stdio handshake (`initialize`, `tools/list` = 29 tools,
  `system_discover`, `health`).
- 12 PASS lines on linux-x64; explicit `arm64 cross-arch
  execution NOT covered` honesty line.

### NPM05 — GitHub Actions build matrix
- `.github/workflows/npm-binary-build.yml` (286 lines) with
  `ubuntu-24.04` (x86_64-unknown-linux-gnu, full smoke) +
  `ubuntu-24.04-arm` (aarch64-unknown-linux-gnu, build + pack).
- Live run on `b0a7839`: `26318082726`, conclusion `success`.
  Both legs PASS. Native arm64 binary `--help` smoke confirmed
  arm64 binaries are actually runnable.
- `fail-fast: false` so an arm64 outage cannot mask x64.

### NPM06 — release-please manifest config
- `.github/release-please-config.json` (47 lines) + `.github/.release-please-manifest.json` (5 lines, three packages pinned to `0.1.0-beta.1`).
- `.github/workflows/release-please.yml` (NPM06 portion: trigger on `push: main` + `workflow_dispatch`, permissions `contents:write` + `pull-requests:write`, action pinned to immutable commit SHA `5c625bfb5d1ff62eadeeb3772007f7f66fdcf071` (v4.4.1)).
- Live run on `917a66d`: `26318866625`, conclusion `success`,
  intentional no-op (no Conventional-Commits-eligible commits
  since the manifest seed).

### NPM07 — npm trusted publishing workflow
- `.github/workflows/release-please.yml` amended (NPM07 portion):
  three publish jobs (`publish-linux-x64`, `publish-linux-arm64`,
  `publish-root`) gated by `needs.release-please.outputs.releases_created
  == 'true'`. Permissions `id-token: write` + `contents: read`
  only on publish jobs. Publish order: platform packages first
  (parallel), then root. `npm publish --provenance --tag beta`
  on every package. Inline cargo builds (no `npm-binary-build`
  artifact download).
- Live run on `ae041b0`: `26319876399`, conclusion `success`,
  all three publish jobs correctly **skipped**
  (`releases_created='false'`). Token-surface log grep clean.

### NPM08 — Cursor MCP integration docs + examples
- `docs/integrations/cursor.md` (12 sections).
- `examples/provider-harness/cursor/` directory with 3
  copy-pasteable configs (`mcp.global.native-linux.json`,
  `mcp.project.linux-wsl.json`, `mcp.global.linux-wsl.json`) + a
  README.
- No active `.cursor/mcp.json` committed.
- Cursor provider live smoke recorded as `Not Run` with the
  exact blocker: Cursor 3.5.30 installed but no documented
  headless MCP entry point; `cursor-agent` CLI not installed.

### NPM08b — README normative overhaul
- `README.md` rewritten end-to-end (382 ins / 404 del) into the
  canonical public project README: product definition + value +
  architecture diagram + 24-row feature matrix + 3-install-path
  section + quickstart + Cursor MCP integration + 29-tool MCP
  catalogue + settings/config + safety posture + current beta
  status + build/verify + repo layout + license.
- Link-resolution sweep: every relative link resolves.
- Live run on `1b2da2c`: `26320821411`, conclusion `success`,
  all three publish jobs correctly **skipped**.

## 4. Release dry-run / readiness checks

### 4.1 Package names + versions

All three package names verified against npm registry on
2026-05-23 via `npm view <name> version`:

| Package | Registry response | Status |
|---------|-------------------|--------|
| `terminal-commander` | `npm error code E404` / `Not Found` | unpublished / available |
| `@terminal-commander/linux-x64` | `npm error code E404` / `Not Found` | unpublished / available |
| `@terminal-commander/linux-arm64` | `npm error code E404` / `Not Found` | unpublished / available |

No package is already published. Names remain reserved for the
first live publish.

### 4.2 Version sync

```
versions-ok 0.1.0-beta.1
manifest-ok {
  'packages/terminal-commander': '0.1.0-beta.1',
  'packages/terminal-commander-linux-x64': '0.1.0-beta.1',
  'packages/terminal-commander-linux-arm64': '0.1.0-beta.1'
}
```

Root `optionalDependencies` exact-pin both platform packages to
`0.1.0-beta.1` (no `^` / `~` ranges). Confirmed by the python
script in the NPM09 verification command.

### 4.3 Tarball dry-runs

| Package | `npm pack --dry-run` | total files | version |
|---------|----------------------|-------------|---------|
| `terminal-commander` (root wrapper) | clean | 7 | `0.1.0-beta.1` |
| `@terminal-commander/linux-x64` | clean | 5 | `0.1.0-beta.1` |
| `@terminal-commander/linux-arm64` | clean | 5 | `0.1.0-beta.1` |

The platform-package tarballs include the committed
`*.placeholder` files plus `LICENSE` + `package.json`. Real
binaries replace the placeholders at CI publish time.

### 4.4 OIDC / trusted publishing posture

- No `NPM_TOKEN` path is active. Token grep over `.github`
  `packages` `docs` `README.md` shows only negative-documentation
  matches (workflow comments + contract sections explicitly
  documenting which tokens are NOT referenced).
- The three repository secrets (`NPM_TOKEN_TC`,
  `CARGO_REGISTRY_TOKEN_TC`, `RELEASE_PLEASE_TOKEN_TC`) remain
  configured on the repo but **none are used by any workflow**.
- No `cargo publish` / `crates.io` step exists. Grep returns only
  negative-documentation matches.
- `id-token: write` is scoped strictly to the three NPM07
  publish jobs. The release-please job permissions readout from
  the latest run (`26320821411`):
  - `Contents: write`
  - `PullRequests: write`
  - (no `IdToken`, no `Packages`)
- `npm publish --provenance` is the only auth path; no `.npmrc`
  token, no `NODE_AUTH_TOKEN` env injection.

### 4.5 Operator preconditions for first live publish

1. Claim `@terminal-commander` organization on npmjs.com.
2. Reserve all three package names on the registry
   (`terminal-commander`, `@terminal-commander/linux-x64`,
   `@terminal-commander/linux-arm64`).
3. For each of the three packages, configure trusted publisher
   in npmjs.com package settings → "Publishing access" → "Add
   trusted publisher" with:
   - Publisher: `GitHub Actions`
   - Repository owner: `special-place-administrator`
   - Repository name: `terminal-commander`
   - Workflow filename: `release-please.yml` (exact match
     required)
   - Environment: blank

Until step 3 completes for all three packages, the first live
publish fails at the OIDC handshake (`403 Forbidden: trusted
publisher not configured` or `EOIDCNOTFOUND`). There is no token
fallback.

### 4.6 release-please / publish workflow output gate

- `release-please` job declares outputs
  `releases_created`, `paths_released`, `version`, `tag_name`
  from `steps.release.outputs.*`.
- Publish jobs gate on `if: needs.release-please.outputs.releases_created
  == 'true'`. Live evidence: NPM07 run `26319876399`, NPM08b run
  `26320821411` both correctly skipped the three publish jobs
  because `releases_created` was `false` on non-release pushes.
- `publish-root` declares `needs: [release-please,
  publish-linux-x64, publish-linux-arm64]` so the root wrapper
  publishes ONLY after both platform publishes succeed, matching
  NPM02 §7.

### 4.7 NPM05 npm-binary-build remains separate + non-publishing

- `.github/workflows/npm-binary-build.yml` is a separate workflow
  with its own `paths:` filter. It triggers only on
  Cargo/crates/packages/scripts changes; doc-only NPM06–NPM08b
  pushes correctly did NOT re-trigger it.
- The workflow does NOT contain `npm publish`, does NOT request
  `id-token: write`, does NOT reference any `_TOKEN`.
- Latest run is still NPM05's `26318082726`, conclusion `success`.

## 5. Local verification (NPM09)

Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, `npm 10.9.7`, `node 22.22.2`:

| Check | Result |
|-------|--------|
| `git branch --show-current` | `main` |
| `git status --short` | clean (pre-status-commit) |
| `git diff --check` | clean |
| `cargo metadata --no-deps` | PASS |
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS (clean) |
| `cargo test --workspace` | all 43 test-result lines `ok` |
| `cargo nextest run --workspace` | **347 / 347 PASS, 0 skipped** |
| `bash scripts/smoke/verify-runtime-smoke.sh` | TC46 8/8 PASS |
| `bash scripts/smoke/verify-npm-local-install.sh` | NPM04 SUCCESS, 12 PASS lines |
| `npm pack ./packages/terminal-commander --dry-run` | 7 files, `0.1.0-beta.1` |
| `npm pack ./packages/terminal-commander-linux-x64 --dry-run` | 5 files, `0.1.0-beta.1` |
| `npm pack ./packages/terminal-commander-linux-arm64 --dry-run` | 5 files, `0.1.0-beta.1` |
| YAML parse | both workflows OK |
| Version sync (script) | `0.1.0-beta.1` across 3 package.json + 3 manifest entries; root `optionalDependencies` exact-pin |
| `rg "Command::new\|Command::spawn\|TcpListener\|UdpSocket" crates/mcp` | doc / negative-assertion matches only |
| `rg "tokio::fs\|std::fs\|File::open\|read_to_string\|read_to_end" crates/mcp/src` | no matches |
| `rg "sk-[A-Za-z0-9]{10}\|ghp_[A-Za-z0-9]{10}\|npm_[A-Za-z0-9]{20}" README.md docs examples packages` | no matches |
| Forbidden-paths diff `--ignore-cr-at-eol` over `crates/ Cargo.toml Cargo.lock rules/ config/ scripts/ packages/ .github/` | empty |

## 6. GitHub Actions latest status

| Workflow | Latest run | SHA | Trigger | Conclusion | Notes |
|----------|------------|-----|---------|------------|-------|
| `release-please` | `26320821411` | `1b2da2c` (NPM08b push) | `push: main` | **success** | release-please `success` (no-op); all 3 publish jobs `skipped` (`releases_created='false'`). Token-surface log clean. |
| `release-please` | `26320460098` | `0d0e530` (NPM08+chain-insertion push) | `push: main` | success | same shape as above |
| `release-please` | `26319876399` | `ae041b0` (NPM07 push) | `push: main` | success | first run after NPM07 publish-jobs landed; all 3 publish jobs `skipped`; token-surface log clean |
| `release-please` | `26318866625` | `917a66d` (NPM06 push) | `push: main` | success | first NPM06 live run; no-op |
| `npm-binary-build` | `26318082726` | `b0a7839` (NPM05 push) | `push: main` | success | linux-x64 full smoke PASS; linux-arm64 build + pack PASS (native arm64 runner) |

No PR is open. No GitHub release exists. No tag exists on the
remote (`git ls-remote --tags origin` empty).

## 7. Active blockers / operator preconditions (recap)

| ID | Item | Required action | Gates |
|----|------|-----------------|-------|
| OP-1 | `@terminal-commander` npm org not yet claimed | An npmjs.com account with org-create permission must register the organization | first live publish |
| OP-2 | Trusted publisher not configured for any of the three package names | npmjs.com side: add trusted publisher (GitHub Actions, this repo, workflow filename `release-please.yml`) per package | first live publish; without it the OIDC handshake fails with no token fallback |
| OP-3 | No `feat:` / `fix:` commit since the manifest seed | A Conventional-Commits release-eligible commit must land on `main` for release-please to open a release PR | release PR + tag + GitHub Release |
| OP-4 | Cursor provider live smoke not captured | Operator opens Cursor with one of the example configs, asks chat to list MCP tools + call `health`, captures transcript / screenshot | promoting beta to `Go` |
| OP-5 | Codex CLI / Claude Code provider live smokes not captured | TC46 baseline; operator-driven, same shape as OP-4 | promoting beta to `Go` |

None of these block the chain's "implementation complete" state.
All five are operator-side preconditions for the operational
events (publish, release, beta promotion) NPM09 explicitly does
NOT perform.

## 8. Negative-surface confirmations (final)

- No `npm publish` occurred during NPM09 (or any prior NPM goal).
- No release PR was merged.
- No tag or GitHub release was created manually.
- No NPM07 / NPM05 workflow file edits at NPM09.
- No `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`,
  `config/**`, `scripts/**` change at NPM09.
- No `packages/*/package.json` version edit at NPM09.
- No new MCP tools, no new IPC surface, no runtime behavior
  change at NPM09.
- No use of `NPM_TOKEN_TC`, `CARGO_REGISTRY_TOKEN_TC`, or
  `RELEASE_PLEASE_TOKEN_TC` anywhere.
- No `Not Run` evidence promoted to PASS.

## 9. Chain terminal state

The `terminal-commander-npm-distribution` chain closes with NPM09.
The 10 goals (NPM01 → NPM02 → NPM03 → NPM04 → NPM05 → NPM06 →
NPM07 → NPM08 → NPM08b → NPM09) executed in declared order with
the documented two-commit landing pattern (verified work + status)
and prep-amendment-first discipline.

Beta posture exit: **Conditional Go** (unchanged from TC48 entry).
Promotion to `Go` requires operator-driven steps recorded in §7.

No new chain is opened at NPM09 close.

## 10. Acceptance against NPM09 mini-spec

- [x] All four artifacts inspected:
      `RELEASE_CHECKLIST.md`, `BACKLOG.md`, `RISK_REGISTER.md`,
      `EVIDENCE_REPORT_RUNTIME.md`. Conditional Go posture remains
      consistent with all four.
- [x] Release dry-run output recorded: §4 covers package
      names + versions, root `optionalDependencies` exact-pin,
      tarball dry-runs, OIDC posture, operator preconditions,
      output-gate evidence, and NPM05 / NPM07 workflow
      separation.
- [x] Beta recommendation locked with rationale: §2 (Conditional
      Go preserved; gates documented).
- [x] Chain is closed: §9 (no new chain opened).
- [x] No publish occurred during NPM09.
- [x] No release PR was merged during NPM09.
- [x] No manual tag / release created.
- [x] No runtime / package version / release workflow change at
      NPM09 outside scope.

## 11. WWS chain follow-up (added at WWS08)

Added by WWS08 to acknowledge the successor chain
`terminal-commander-windows-wsl-bridge` (WWS01–WWS09). The WWS
chain shipped JS-only Windows control-plane surfaces that wrap
the existing Linux/WSL2 runtime. None of the NPM06–NPM10
publish gates or workflow contracts changed.

- WWS01 contract: `docs/release/windows-wsl-bridge-contract.md`
  (commit `6220eb2`); 15 binding decisions D-01..D-15.
- WWS02 root npm package `os: ["linux", "win32"]` widening
  (commit `1da40f3`); recorded as §13b amendment in
  `docs/release/npm-binary-packaging-contract.md`. Platform
  packages remain `os: ["linux"]`; `optionalDependencies`
  exact-pin unchanged.
- WWS03–WWS06 JS-only Windows control-plane modules under
  `packages/terminal-commander/lib/{wsl,cursor,cli}/`;
  `packages/terminal-commander/bin/terminal-commander*.js` shims
  extended for the Windows branch. Linux behaviour byte-identical.
- WWS07 Windows bridge smoke
  (`scripts/smoke/verify-windows-bridge-smoke.ps1`, commit
  `785d410`); records `runtime_missing` honestly when the WSL
  distro lacks `terminal-commander-mcp`. The Cursor provider GUI
  smoke remains `Not Run`.
- WWS08 (this commit) ships public README + RELEASE_CHECKLIST +
  BACKLOG + RISK_REGISTER + ROADMAP updates only; NO code, NO
  workflow, NO version bump, NO publish.

Publish posture: unchanged from NPM10. `npm-bootstrap-publish.yml`
remains committed-but-undispatched. The first live publish still
requires (i) operator npmjs.com trusted-publisher setup per
`docs/release/npm-trusted-publishing-contract.md` §8, AND (ii) a
Conventional-Commits `feat:`/`fix:` commit merging through a
release-please PR. WWS09 is the final pre-publish readiness
review.
