---
goal_id: NPM07
title: Npm Trusted Publishing Workflow
chain_id: terminal-commander-npm-distribution
phase: Wave 4 - Publishing
status: "Completed"
depends_on: ["NPM06"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: "2026-05-23T03:00:00+00:00"
completed_at: "2026-05-23T04:00:00+00:00"
completion_commit: "1b4267e"
blocked_reason: ""
source_refs:
  - "npm trusted publishing docs (GitHub Actions OIDC)"
  - "npm provenance docs"
  - "release-please-action README (release publishing section)"
  - "release-please-action v4.4.1 action.yml + dist outputs (releases_created, <path>--version, <path>--tag_name)"
  - "NPM02 contract §7 (release contract: OIDC trusted publishing + provenance)"
  - "User directive 2026-05-23: same-workflow publish, output-gated, no PAT/NPM_TOKEN/CARGO_REGISTRY_TOKEN, inline binary build"
risk_level: "high"
---

# NPM07 - Npm Trusted Publishing Workflow

## Branch Guard

```text
main
```

## Mission Context

NPM06 produces release PRs and GitHub Releases. NPM07 publishes the root + platform packages to npm via npm trusted publishing (GitHub Actions OIDC) — no long-lived `NPM_TOKEN`. Provenance is enabled.

## Mini-Spec

objective:
- Add a GitHub Actions publish workflow that triggers on the release-please `release_created` output and uses npm trusted publishing (OIDC) to push the root wrapper + each platform package to the npm registry with provenance enabled. The platform packages publish first; the root wrapper publishes last so its `optionalDependencies` resolve.

non_goals:
- Do not store a long-lived `NPM_TOKEN`. If trusted publishing is impossible AND explicitly approved in this goal's final report, a fine-grained automation token is acceptable as a fallback; the fallback decision must name the reason.
- Do not publish to the `latest` dist-tag for the first beta tag. Use `--tag beta` (or `next`) until the operator promotes.
- Do not modify `crates/**`.

allowed_files_or_area:
- .github/workflows/** (publish workflow + reusable steps)
- docs/release/**
- .agent/goals/terminal-commander-npm-distribution/NPM07-*.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**, config/**
- packages/*/package.json edits beyond release-please bumps
- any secret literal in YAML

contracts_or_interfaces:
- The npm packages are configured on npmjs.com to accept trusted publishing from this GitHub repo + workflow path. This is operator-side; the goal records the steps.
- Workflow runs only on `release_created == true` from release-please.
- `permissions: id-token: write` set at the job level (required for OIDC).
- `npm publish --provenance` for each package.
- Publish order: platform packages first (`@terminal-commander/linux-x64`, `@terminal-commander/linux-arm64`), then root wrapper.
- First publish uses `--tag beta`. Promotion to `latest` is an operator action recorded in a follow-up.
- No `secrets.NPM_TOKEN` referenced in the workflow unless the fallback exception is recorded.

invariants:
- No raw stream / no network listener / no shell expansion / no runtime change.
- The publish workflow does not run the test suite — it publishes artifacts already validated by NPM05 (regression gates run pre-publish).

acceptance_criteria:
- Publish workflow file exists with OIDC permissions + provenance flags.
- Operator instructions for configuring the trusted publisher on npmjs.com live in `docs/release/`.
- Dry-run path documented (`npm publish --dry-run --provenance`).
- No long-lived token committed; fallback path explicitly recorded if used.

evidence_required:
- Branch evidence.
- Workflow YAML path.
- Operator setup notes (npmjs.com side) — bounded, no secrets.
- Dry-run output captured at goal completion.

stop_conditions:
- Branch is not `main`.
- npm trusted publishing is technically unavailable for the chosen packages AND the operator does not approve a token fallback in this goal's final report.
- The publish workflow would require any `crates/**` change.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo nextest run --workspace
bash scripts/smoke/verify-runtime-smoke.sh
yamllint .github/workflows || true
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
# Bounded grep — no committed npm tokens:
rg "NPM_TOKEN|npm_[a-zA-Z0-9]{30,}" .github docs/release packages
```

## Task Prompt

Run NPM07 only on branch `main`. Trusted publishing only; no token fallback unless explicitly approved here.

## Final Report Format

Objective / Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal (NPM08).

## Final Report

Objective:
- Add npm trusted-publishing (OIDC + provenance) for the three Terminal Commander npm packages, in-place inside `.github/workflows/release-please.yml`, gated by `release-please.outputs.releases_created == 'true'`. No long-lived tokens. No crates.io publish. No runtime change.

Token decisions (locked at user direction 2026-05-23):
- `NPM_TOKEN_TC`: **unused**. NPM07 uses npm trusted publishing only.
- `CARGO_REGISTRY_TOKEN_TC`: **unused**. No crates.io publish in this chain.
- `RELEASE_PLEASE_TOKEN_TC`: **unused**. release-please keeps `secrets.GITHUB_TOKEN`. Because GITHUB_TOKEN-created release events do not trigger downstream workflows, NPM07 publishes from the SAME workflow as release-please, not from a separate `on: release` workflow.

Changes (verified work commit `1b4267e`):
- `.github/workflows/release-please.yml` — amended. release-please job retained as-is; THREE new publish jobs appended:
  - `publish-linux-x64` (`ubuntu-24.04`, perms `id-token:write` + `contents:read`)
  - `publish-linux-arm64` (`ubuntu-24.04-arm`, perms `id-token:write` + `contents:read`)
  - `publish-root` (`ubuntu-24.04`, perms `id-token:write` + `contents:read`)
- `docs/release/npm-trusted-publishing-contract.md` — new binding contract (15 sections).
- `docs/release/release-please-contract.md` — §15 marked SUPERSEDED, points at NPM07's same-workflow design with the GITHUB_TOKEN downstream-trigger rationale.

No edits to: `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`, `scripts/**`, `packages/*/package.json`, `.github/workflows/npm-binary-build.yml`, `.github/release-please-config.json`, `.github/.release-please-manifest.json`.

Files changed (workflow path):
- `.github/workflows/release-please.yml` (amended, 340 lines diff: NPM06 release-please job kept; NPM07 publish jobs added)

Files created (docs):
- `docs/release/npm-trusted-publishing-contract.md`

Files modified (docs):
- `docs/release/release-please-contract.md` (§15 supersession note)

Files modified (goal):
- `.agent/goals/terminal-commander-npm-distribution/NPM07-npm-trusted-publishing-workflow.md` (frontmatter + Final Report)

Same-workflow / output-gated design — exact release-please output condition used:
- Top gate: `if: needs.release-please.outputs.releases_created == 'true'`
- Job outputs exposed by `release-please` step (id=`release`):
  - `releases_created`: `${{ steps.release.outputs.releases_created }}`
  - `paths_released`: `${{ steps.release.outputs.paths_released }}`
  - `version`: `${{ steps.release.outputs['packages/terminal-commander--version'] }}`
  - `tag_name`: `${{ steps.release.outputs['packages/terminal-commander--tag_name'] }}`
- Per-job version-match guards re-assert `package.json.version == release-please.outputs.version` before `npm publish`.

Publish-job dependency graph (locked):
```
release-please  (no needs, runs on every main push)
   ├─→ publish-linux-x64    (needs: release-please)
   ├─→ publish-linux-arm64  (needs: release-please)
   └─→ publish-root         (needs: [release-please, publish-linux-x64, publish-linux-arm64])
```
All three publish jobs share the same `if: needs.release-please.outputs.releases_created == 'true'`. The two platform jobs run in parallel; the root job waits for both. This preserves NPM02 §7 publish-order doctrine.

Exact publish order:
1. `@terminal-commander/linux-x64`
2. `@terminal-commander/linux-arm64`
3. `terminal-commander` (root wrapper, only after both platform publishes succeed)

Exact npm trusted-publishing preconditions (npmjs.com operator side):
- Claim `@terminal-commander` org on npmjs.com.
- Reserve all three package names.
- For each of `terminal-commander`, `@terminal-commander/linux-x64`, `@terminal-commander/linux-arm64`: configure the trusted publisher with Publisher=`GitHub Actions`, Owner=`special-place-administrator`, Repository=`terminal-commander`, Workflow filename=`release-please.yml`, Environment=(blank).
- Until all three trusted-publisher configurations are in place, the first live publish will fail at the OIDC handshake with `403 Forbidden: trusted publisher not configured` (or `EOIDCNOTFOUND`). That failure is the recorded blocker, NOT a workflow defect. There is no token fallback.

Pre-publish guards inside each job (defense in depth):
- Toolchain pinned `1.95.0` on platform jobs.
- Native arm64 build on `ubuntu-24.04-arm` (no QEMU, no x64 fake).
- Real binary `--help` returns 0 (binary is runnable on its native arch).
- `*.placeholder` files removed from `bin/` before pack so they do not end up in the tarball.
- `package.json` `version` equals `release-please.outputs.version` (catches drift).
- `optionalDependencies` exact-pin to the shared version on the root job (catches a desync that would publish a broken root wrapper).
- Resolver unit tests (`npm test`, 12 cases) on the root job.

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, `npm 10.9.7`, `node 22.22.2`):
- PASS: `git branch --show-current` → `main`
- PASS: `git status --short` → clean (post status commit)
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings` (clean, exit 0)
- PASS: `cargo test --workspace` (all 43 test-result lines `ok`)
- PASS: `cargo nextest run --workspace` — **347/347, 0 skipped**
- PASS: `bash scripts/smoke/verify-runtime-smoke.sh` — TC46 8/8 PASS
- PASS: `bash scripts/smoke/verify-npm-local-install.sh` — NPM04 SUCCESS (12 PASS, end-to-end MCP stdio against npm-installed binaries)
- PASS: `npm pack ./packages/terminal-commander --dry-run` — 7 files, 0.1.0-beta.1
- PASS: `npm pack ./packages/terminal-commander-linux-x64 --dry-run` — 5 files, 0.1.0-beta.1
- PASS: `npm pack ./packages/terminal-commander-linux-arm64 --dry-run` — 5 files, 0.1.0-beta.1
- PASS: `cd packages/terminal-commander && npm test` — resolver 12/12 PASS
- PASS: YAML parse: `.github/workflows/release-please.yml` (4 jobs: `release-please`, `publish-linux-x64`, `publish-linux-arm64`, `publish-root`) + `.github/workflows/npm-binary-build.yml` (untouched, still parses)
- PASS: Job shape sanity:
  - workflow-level `permissions: {contents: read}`
  - `release-please` job: `{contents: write, pull-requests: write}` (no id-token)
  - `publish-linux-x64`, `publish-linux-arm64`, `publish-root` jobs: `{id-token: write, contents: read}` (least-privilege for OIDC)
- PASS: Version sync — all 3 `package.json` `version` fields + all 3 `.github/.release-please-manifest.json` entries = `0.1.0-beta.1`; root `optionalDependencies` exact-pin to `0.1.0-beta.1` for both `@terminal-commander/linux-x64` and `@terminal-commander/linux-arm64` (no `^` / `~` ranges)
- PASS: `rg "NPM_TOKEN|NPM_TOKEN_TC|CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|secrets.NPM|secrets.CARGO|secrets.RELEASE|_authToken|NODE_AUTH_TOKEN" .github packages docs/release` — all matches are negative-documentation only (comments in `release-please.yml` saying "no NPM_TOKEN", contract docs describing what is forbidden, NPM01 audit's historical Symforge precedent text). No active workflow reference to any long-lived token. No `_authToken` line in any `.npmrc`. No `NODE_AUTH_TOKEN` env injection.
- PASS: `rg "cargo publish|crates\.io" .github packages docs/release` — matches are negative-documentation only (contract sections rejecting crates.io publish; NPM01 audit's Symforge note). No active `cargo publish` step.
- PASS: `rg "postinstall" packages .github docs/release` — matches are negative-documentation only (READMEs and contracts saying "no postinstall"; NPM01 audit's Symforge precedent description). No `postinstall` script in any `package.json`.
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — doc / negative-assertion matches only (unchanged from NPM06 baseline)
- PASS: `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` — no matches (unchanged)
- PASS: `git diff --ignore-cr-at-eol --shortstat HEAD -- crates/ Cargo.toml Cargo.lock rules/ config/ scripts/ packages/ .github/workflows/npm-binary-build.yml` → empty

Evidence — explicit acceptance against the NPM07 mini-spec:
- **Workflow file exists with OIDC permissions + provenance flags.** Confirmed by YAML parse + job-permission readout. `npm publish --provenance --tag beta --access public` lines present in `publish-linux-x64` and `publish-linux-arm64`; `npm publish --provenance --tag beta` in `publish-root`.
- **Operator instructions for configuring the trusted publisher live in `docs/release/`.** `docs/release/npm-trusted-publishing-contract.md` §8 lists the npmjs.com configuration steps with exact field values.
- **Dry-run path documented.** Contract §10 documents `npm audit signatures` for provenance verification. `npm publish --dry-run --provenance` is not exercised at NPM07 close because OIDC dry-run requires the trusted-publisher to be configured first; this is recorded as Pending operator setup.
- **No long-lived token committed.** Confirmed by grep above; no fallback path exists.
- **Publish order: platform packages first, root last.** Confirmed by `needs:` graph; `publish-root` waits on both platform jobs.
- **First publish uses `--tag beta`.** Confirmed in all three `npm publish` commands.
- **NPM05 binary-build workflow remains separate and non-publishing.** `.github/workflows/npm-binary-build.yml` not modified; still on `paths:` filter; no publish step.
- **Inline binary build, not artifact download.** Each platform publish job runs `cargo build --release --target <triple>` itself. No `gh run download` of NPM05 artifacts.
- **Native arm64, no QEMU.** `publish-linux-arm64` runs on `ubuntu-24.04-arm` (same label NPM05's live run proved available, run ID `26318082726`).
- **Versions + optionalDependencies remain synchronized.** Confirmed by python script + manifest cross-check.

Beta-state mapping:
- TC48 `Conditional Go` preserved. NPM07 added configuration only; no runtime / MCP surface / package layout / shim behavior changed.
- Provider live smoke ceiling still NPM08 scope.
- Linux/WSL2 still the only supported platform set; no Mac/Windows/musl claim introduced.
- TC46 + TC47 regressions still green.

Live publish status:
- **Not Run / Pending release PR merge + operator npmjs.com trusted-publisher setup.**
- No release PR currently exists (release-please's NPM06 live run was a documented no-op; no `feat:` / `fix:` commits since the manifest seed).
- The npmjs.com operator preconditions in `docs/release/npm-trusted-publishing-contract.md` §8 must complete before the first publish can succeed.
- NPM09 captures the first live publish transcript.

Source-status:
- `.github/workflows/release-please.yml`: **live (NPM06 + NPM07)**. NPM06 release-please job + NPM07 three publish jobs.
- `docs/release/npm-trusted-publishing-contract.md`: **live (NPM07)**.
- `docs/release/release-please-contract.md`: **live (NPM06)** with NPM07 supersession note in §15.
- `.github/workflows/npm-binary-build.yml`: **unchanged from NPM05**.
- `.github/release-please-config.json`: **unchanged from NPM06**.
- `.github/.release-please-manifest.json`: **unchanged from NPM06**.
- Every `packages/<pkg>/package.json`: **unchanged** (versions remain `0.1.0-beta.1`).
- Every `crates/` source file: **unchanged**.

Commits:
- Verified work commit: `1b4267e`
- Goal status commit: this commit

Known gaps / blockers:
- **Live publish: Pending operator npmjs.com setup + release PR merge.** Without the trusted publisher configured on npmjs.com for all three packages, the first publish will fail at the OIDC handshake. This is the recorded blocker.
- **No release PR exists yet.** A `feat:` / `fix:` commit (or `chore!:` breaking) must land on `main` to make release-please open the release PR. Until then, every release-please run is a documented no-op and no publish job fires.
- **arm64 runner availability** (R-NPM-02 in NPM01 §14) remains contingent on GitHub's `ubuntu-24.04-arm` allowance for this repo. NPM05's live run confirmed availability on 2026-05-23; the workflow inherits the same blocker doctrine if availability ever changes.
- **Node 20 actions deprecation** (annotated on NPM05 + NPM06 runs) applies to NPM07's `actions/checkout@v4`, `actions/setup-node@v4`, `Swatinem/rust-cache@v2`, `dtolnay/rust-toolchain@master` as well. Forcing Node 24 starts 2026-06-02. Out of NPM07 scope; track in a later amendment.

Confirmations (explicit):
- NPM_TOKEN_TC is NOT used.
- CARGO_REGISTRY_TOKEN_TC is NOT used.
- RELEASE_PLEASE_TOKEN_TC is NOT used.
- No npm publish path exists outside the OIDC-gated publish jobs.
- No crates.io / cargo publish path exists anywhere.
- Versions + optionalDependencies remain synchronized at `0.1.0-beta.1`.
- `.github/workflows/npm-binary-build.yml` was not modified at NPM07.
- `crates/`, `Cargo.toml`, `Cargo.lock`, `rules/`, `config/`, `scripts/` were not modified at NPM07.
- No new MCP tools, no runtime feature changes, no provider-harness claim.
- No release PR auto-merge.
- No Mac/Windows-native or musl/Alpine package claim.
- NPM08 was not started.

Next goal:
- NPM08-cursor-mcp-integration-and-live-smoke.md (per chain index).

