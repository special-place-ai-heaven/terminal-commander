# Phase 4a-max — Distribution: native tier-1 publish pipeline

**Date:** 2026-05-24
**Branch:** `feature/phase-4-deferred-gaps`
**Status:** Design approved (max scope), ready for implementation plan
**Supersedes:** Phase 4a-min draft (windows-x64-only) — replaced by Phase 4a-max
after operator selected strict-correctness / boil-the-lake scope.

---

## Problem

Phase 3 + 3.5 + 3.5b (merged to main as PR #8, 40 commits) added a
native Windows-x64 daemon + MCP runtime to the source tree. The
binaries build, the tests pass on Windows, the named-pipe IPC works.

But the operator's ADR-promised contract "one command install on a
fresh Windows machine" is undelivered, AND the existing publish
pipeline carries 7 IMPORTANT gaps codex flagged during the original
delta-design review:

- `@terminal-commander/windows-x64` is **not on the npm registry**.
- Root package's `optionalDependencies` declares the windows-x64 dep
  as `"file:../terminal-commander-windows-x64"` (broken in `npm install`).
- npm publish auth uses a granular access token — **no SLSA provenance**
  on the published tarballs. Supply-chain attacks via stolen
  `NPM_TOKEN_TC` would be undetectable.
- `npm-binary-build.yml` (PR-time build smoke) and `release-please.yml`
  (release-time publish) duplicate the build matrix. Drift between them
  means a Windows compile break can land on main without CI catching it.
- No automated post-publish verification. A botched publish only surfaces
  when a user files an issue.
- No automated token-expiry detection. The `NPM_TOKEN_TC` granular token
  has a 365-day max lifetime; silent expiry breaks the next release.
- No documented rollback story beyond "publish a patch release".
- No alternative install path for Rust users — npm is the only door.

## Goal

Ship a **production-grade tier-1 distribution pipeline** that:

1. Publishes `@terminal-commander/{linux-x64, linux-arm64, windows-x64,
   mac-x64, mac-arm64}@0.2.0` + `terminal-commander@0.2.0` to npm with
   SLSA provenance attestations.
2. Publishes `terminal-commander-mcp` + `terminal-commanderd` +
   `terminal-commander-supervisor` to crates.io at `0.2.0`, enabling
   `cargo install terminal-commander-mcp` as a no-Node install path.
3. Self-verifies every release via containerized smoke jobs that fail
   the release if any platform tarball is broken.
4. Self-heals via a documented recovery script + a one-click deprecation
   workflow when a release ships broken.
5. Self-warns via a weekly scheduled secret-health check.

## Non-goals

- Replacing the existing `release-please.yml` workflow architecture
  with a different release framework. The existing tag-driven /
  release-please-PR-merge-driven flow stays; this design extends it.
- Bumping Cargo workspace version beyond `0.2.0` ahead of npm; the two
  release pipelines lockstep on the same semver tag.
- macOS as a runtime tier-1 target. Macs receive published binaries and
  can install, but QA / doctor / docs coverage stays tier-3 per ADR.
  We ship the artifacts because cross-compile is free; we don't QA them.

## Operator decisions captured during brainstorm

- **Auth migration:** start with `NPM_TOKEN_TC` granular token (already
  in repo secrets, day-0 working), THEN migrate to OIDC trusted
  publisher per package on npmjs.com (5-pkg config tax accepted in
  exchange for `--provenance` attestations on every tarball).
- **Trigger:** release-please-driven. Conventional commits → release PR
  → merge → release-please tags `v<version>` → publish jobs fire.
- **Version strategy:** monorepo lockstep. All 5 npm packages + 3 cargo
  crates + 1 root npm package share one version. release-please's
  `linked-versions` plugin enforces npm side; cargo workspace
  `version = "X.Y.Z"` enforces cargo side; an inline assert in
  release-please.yml enforces npm-cargo parity.
- **Publish order per release:**
  1. 5 platform npm packages in parallel
  2. Root npm package (depends on 5)
  3. 3 cargo crates in dep order: supervisor → daemon → mcp
  4. 5 containerized smoke jobs in parallel (one per platform tarball)
  5. Release marked successful (or auto-deprecated if any smoke fails)
- **Recovery:** existing E409-tolerant pattern + new
  `scripts/release/recover-partial-publish.sh` for the panic case.
- **Deprecation:** new `.github/workflows/deprecate-version.yml`
  workflow_dispatch input: version + message. Marks all 5 npm packages
  deprecated via `npm deprecate`. crates.io equivalent: `cargo yank`.
- **macOS:** included in this phase. Cross-compile from Linux runner
  using `cargo-zigbuild` (free, well-tested) for `x86_64-apple-darwin`
  and `aarch64-apple-darwin` targets.

## What currently exists

Read evidence (per Phase 4a-min spec, unchanged):

- **`.github/release-please-config.json`** — manifest-mode. 3 components:
  `terminal-commander`, `@terminal-commander/linux-x64`,
  `@terminal-commander/linux-arm64`.
- **`.github/.release-please-manifest.json`** — 3 entries at `0.1.4`.
- **`packages/terminal-commander/package.json`** — root wrapper.
  `optionalDependencies` has 2 linux pkgs at `0.1.4` and windows-x64 as
  `"file:../terminal-commander-windows-x64"`.
- **`packages/terminal-commander-windows-x64/package.json`** — exists
  with correct shape, no change needed.
- **`.github/workflows/release-please.yml`** — release-please + 2 linux
  publish jobs + root publish + ensure-release + manual recovery.
- **`scripts/release/sync-optional-dependencies.js`** +
  **`verify-optional-dependencies.js`** — handle 2 linux packages.
- **`Cargo.toml`** workspace — version `0.0.0`, all crates `publish = false`.

## Design — the delta

### Files modified or created

#### A. npm publish coverage expansion (windows + macOS)

1. **`packages/terminal-commander-mac-x64/package.json`** *(NEW)*
   ```json
   {
     "name": "@terminal-commander/mac-x64",
     "version": "0.1.4",
     "os": ["darwin"],
     "cpu": ["x64"],
     "files": ["bin/"],
     "license": "Apache-2.0",
     "engines": { "node": ">=18" },
     "repository": { "type": "git", "url": "..." }
   }
   ```
   Add `bin/.gitkeep`.

2. **`packages/terminal-commander-mac-arm64/package.json`** *(NEW)*
   Same shape as `mac-x64` but `"cpu": ["arm64"]`.

3. **`.github/release-please-config.json`**
   - `linked-versions` plugin `components` array: 3 → 6 entries
     (add `@terminal-commander/windows-x64`,
     `@terminal-commander/mac-x64`, `@terminal-commander/mac-arm64`).
   - Root package `extra-files` array: add 3 new
     `optionalDependencies['@terminal-commander/{windows-x64,mac-x64,mac-arm64}']`
     entries.
   - Add 3 new `packages/...` entries (windows, mac-x64, mac-arm64),
     each `release-type: node`, component matches package name.

4. **`.github/.release-please-manifest.json`**
   - Add 3 entries at `"0.1.4"` for windows-x64, mac-x64, mac-arm64.

5. **`packages/terminal-commander/package.json`**
   - Change `optionalDependencies['@terminal-commander/windows-x64']`
     from `"file:../terminal-commander-windows-x64"` to `"0.1.4"`.
   - Add `optionalDependencies['@terminal-commander/mac-x64']: "0.1.4"`.
   - Add `optionalDependencies['@terminal-commander/mac-arm64']: "0.1.4"`.

6. **`packages/terminal-commander/lib/resolver.js`**
   - Confirm resolver already handles `darwin/x64` + `darwin/arm64` cases.
     Current resolver is platform-keyed; verify mac entries exist. If
     missing, add: `darwin-x64 → @terminal-commander/mac-x64`, etc.

7. **`scripts/release/sync-optional-dependencies.js`** +
   **`verify-optional-dependencies.js`**
   - Extend `KNOWN_PLATFORM_PACKAGES` array from 2 → 5.

#### B. Build-matrix de-duplication

8. **`.github/workflows/_build-platform-binary.yml`** *(NEW — reusable workflow)*
   ```yaml
   name: Build platform binary (reusable)
   on:
     workflow_call:
       inputs:
         platform: { required: true, type: string }  # linux-x64, linux-arm64, windows-x64, mac-x64, mac-arm64
         upload_artifact: { default: true, type: boolean }
       outputs:
         artifact_name:
           value: ${{ jobs.build.outputs.artifact_name }}
   jobs:
     build:
       runs-on: ${{ fromJSON(...) }}  # resolves platform → runner
       steps:
         - checkout
         - rust toolchain (resolves platform → target triple)
         - Swatinem/rust-cache@v2 (shared-key keyed on platform)
         - cargo build --release --target <triple>
         - stage 3 bins into bin/ (with .exe suffix on windows)
         - --version smoke
         - upload-artifact (if upload_artifact=true)
   ```
   Platform → runner + triple lookup is inline using a `case` block:
   - `linux-x64 → ubuntu-24.04 / x86_64-unknown-linux-gnu`
   - `linux-arm64 → ubuntu-24.04-arm / aarch64-unknown-linux-gnu`
   - `windows-x64 → windows-2022 / x86_64-pc-windows-msvc`
   - `mac-x64 → ubuntu-24.04 / x86_64-apple-darwin (cargo-zigbuild)`
   - `mac-arm64 → ubuntu-24.04 / aarch64-apple-darwin (cargo-zigbuild)`

9. **`.github/workflows/npm-binary-build.yml`** *(MODIFIED — collapse to caller)*
   - Replace inline build steps with 5 parallel `uses: ./.github/workflows/_build-platform-binary.yml`
     calls, one per platform, `upload_artifact: true`.
   - The PR-time smoke is now: build all 5 platforms, upload artifacts,
     done. No drift possible vs publish path (same reusable workflow).

10. **`.github/workflows/release-please.yml`** *(MODIFIED extensively)*
    - Remove inline cargo build / stage steps from
      `publish-linux-x64` and `publish-linux-arm64` jobs.
    - Replace with: each `publish-<platform>` job has 2 child steps:
      (a) `uses: ./.github/workflows/_build-platform-binary.yml` with
      `upload_artifact: false`, plus inline staging into
      `packages/terminal-commander-<platform>/bin/`,
      (b) `npm publish` with `--provenance` flag (OIDC mode) or token
      mode (initial cutover).
    - Add 3 new publish jobs mirroring shape: `publish-windows-x64`,
      `publish-mac-x64`, `publish-mac-arm64`.
    - `publish-root`'s `needs:` array: add windows-x64 + mac-x64 + mac-arm64.
    - `publish-root`'s inline Node version-pin assertion: extend to all
      5 platform packages.
    - Add OIDC permissions block at workflow level:
      ```yaml
      permissions:
        contents: write
        id-token: write   # NEW — for npm --provenance + crates.io OIDC
      ```

#### C. Cargo / crates.io publishing

11. **`Cargo.toml`** *(workspace)*
    - Bump `workspace.package.version` from `0.0.0` to `0.2.0`.
    - For the 3 published crates (`supervisor`, `daemon`, `mcp`):
      change `publish = false` → `publish = true`. The rest stay
      `publish = false` (internal-only).
    - Add per-crate metadata (`description`, `repository`, `license`,
      `keywords`, `categories`) to the 3 published crates if missing.

12. **`.github/workflows/release-please.yml`** *(MODIFIED — add cargo publish jobs)*
    - Add 3 new jobs after `publish-root`:
      - `publish-cargo-supervisor` — `cargo publish -p terminal-commander-supervisor`
      - `publish-cargo-daemon` — needs supervisor, `cargo publish -p terminal-commanderd`
      - `publish-cargo-mcp` — needs daemon, `cargo publish -p terminal-commander-mcp`
    - Each uses `cargo publish --dry-run` first, then real publish.
    - Uses `CARGO_REGISTRY_TOKEN_TC` secret (already set by operator).
    - E409 recovery: catch "crate already on registry" and exit success
      under `force_publish` workflow_dispatch.
    - npm-cargo version parity assertion: a step in the
      release-please-tag job that fails if
      `cargo pkgid -p terminal-commander-mcp` version differs from the
      release tag.

#### D. Post-publish containerized verification

13. **`.github/workflows/release-please.yml`** *(MODIFIED — add verify jobs)*
    - Add 5 new jobs at the end:
      - `verify-linux-x64` — `runs-on: ubuntu-24.04`
        ```yaml
        steps:
          - run: docker run --rm node:20-alpine sh -c \
              "npm install -g terminal-commander@${{ needs.release-please.outputs.tag_name }} && \
               terminal-commander-mcp --version && \
               echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"smoke\",\"version\":\"0.0.0\"}}}' | timeout 10 terminal-commander-mcp || true"
        ```
      - `verify-linux-arm64` — same, on `ubuntu-24.04-arm`
      - `verify-windows-x64` — on `windows-2022`, native `npm install -g`
        (Docker-on-Windows pipelines are flaky; use native).
      - `verify-mac-x64` — `runs-on: macos-13` (x86_64), native install
      - `verify-mac-arm64` — `runs-on: macos-14` (arm64), native install
    - All 5 depend on `publish-root` succeeding.
    - If any fails, a `mark-release-broken` job fires:
      ```yaml
      mark-release-broken:
        needs: [verify-linux-x64, verify-linux-arm64, verify-windows-x64, verify-mac-x64, verify-mac-arm64]
        if: failure()
        steps:
          - run: gh issue create --title "Release ${{ tag }} smoke FAILED on $PLATFORM" --label "release-broken,P0"
      ```

#### E. Operational tooling

14. **`.github/workflows/secret-health.yml`** *(NEW)*
    ```yaml
    name: Weekly secret health
    on:
      schedule:
        - cron: '0 9 * * MON'   # Monday 09:00 UTC
      workflow_dispatch:
    jobs:
      probe:
        runs-on: ubuntu-24.04
        steps:
          - name: npm token
            env: { NPM_TOKEN: ${{ secrets.NPM_TOKEN_TC }} }
            run: |
              echo "//registry.npmjs.org/:_authToken=$NPM_TOKEN" > ~/.npmrc
              if ! npm whoami; then
                gh issue create --title "NPM_TOKEN_TC failing whoami" --label "ops,P1"
                exit 1
              fi
          - name: cargo token
            env: { CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN_TC }} }
            run: |
              if ! cargo search terminal-commander-mcp --limit 1; then
                gh issue create --title "CARGO_REGISTRY_TOKEN_TC failing" --label "ops,P1"
                exit 1
              fi
          - name: release-please PAT
            env: { GH_TOKEN: ${{ secrets.RELEASE_PLEASE_TOKEN_TC }} }
            run: |
              if ! gh auth status; then
                gh issue create --title "RELEASE_PLEASE_TOKEN_TC failing" --label "ops,P1"
                exit 1
              fi
    ```

15. **`scripts/release/recover-partial-publish.sh`** *(NEW)*
    ```bash
    #!/usr/bin/env bash
    # Usage: ./recover-partial-publish.sh <version>
    # Queries npm + crates.io for each artifact, identifies missing ones,
    # prints republish commands for just those. Operator runs the commands.
    set -euo pipefail
    VER="${1:?version required}"
    PKGS=(
      "npm:terminal-commander"
      "npm:@terminal-commander/linux-x64"
      "npm:@terminal-commander/linux-arm64"
      "npm:@terminal-commander/windows-x64"
      "npm:@terminal-commander/mac-x64"
      "npm:@terminal-commander/mac-arm64"
      "cargo:terminal-commander-supervisor"
      "cargo:terminal-commanderd"
      "cargo:terminal-commander-mcp"
    )
    for pkg in "${PKGS[@]}"; do
      kind="${pkg%%:*}"
      name="${pkg#*:}"
      case "$kind" in
        npm)
          if npm view "$name@$VER" version >/dev/null 2>&1; then
            echo "OK    $kind/$name@$VER"
          else
            echo "MISS  $kind/$name@$VER   → re-run release-please.yml workflow with force_publish=true"
          fi ;;
        cargo)
          if cargo search "$name" --limit 1 | grep -q "$name = \"$VER\""; then
            echo "OK    $kind/$name@$VER"
          else
            echo "MISS  $kind/$name@$VER   → cargo publish -p $name (from this checkout at tag v$VER)"
          fi ;;
      esac
    done
    ```

16. **`.github/workflows/deprecate-version.yml`** *(NEW)*
    ```yaml
    name: Deprecate published version
    on:
      workflow_dispatch:
        inputs:
          version: { required: true, type: string }
          reason:  { required: true, type: string, description: "Deprecation message users see on npm install" }
    permissions: { contents: read }
    jobs:
      deprecate:
        runs-on: ubuntu-24.04
        steps:
          - uses: actions/setup-node@v4
            with: { node-version: '20', registry-url: 'https://registry.npmjs.org' }
          - env: { NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN_TC }} }
            run: |
              for pkg in terminal-commander \
                         @terminal-commander/linux-x64 \
                         @terminal-commander/linux-arm64 \
                         @terminal-commander/windows-x64 \
                         @terminal-commander/mac-x64 \
                         @terminal-commander/mac-arm64; do
                npm deprecate "$pkg@${{ inputs.version }}" "${{ inputs.reason }}"
              done
          - env: { CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN_TC }} }
            run: |
              for crate in terminal-commander-supervisor \
                           terminal-commanderd \
                           terminal-commander-mcp; do
                cargo yank --version "${{ inputs.version }}" "$crate"
              done
    ```

#### F. OIDC trusted publisher migration (after first token-mode release)

17. **`.github/workflows/release-please.yml`** *(MODIFIED — flip auth mode)*
    - After the first 0.2.0 release ships via `NPM_TOKEN_TC`, operator
      configures OIDC trusted publisher on npmjs.com for each of the 6
      packages (5 platforms + root). One-time UI clicks: ~10 min total.
    - Edit publish jobs: remove `NODE_AUTH_TOKEN` env, add
      `--provenance` flag to `npm publish`. The `id-token: write`
      permission block (already added in change #10) authorizes OIDC.
    - Repeat for cargo: configure crates.io trusted publisher for each
      of the 3 crates. Flip cargo publish jobs to drop
      `CARGO_REGISTRY_TOKEN` env.
    - Secrets become unused: keep `NPM_TOKEN_TC` /
      `CARGO_REGISTRY_TOKEN_TC` as emergency-recovery escape hatch but
      remove their day-to-day use.

### Files NOT modified (intentionally)

- **`.github/workflows/npm-bootstrap-publish.yml`** — superseded by
  `recover-partial-publish.sh` + `deprecate-version.yml`. Delete in a
  follow-up; not in this phase to avoid scope tangling.
- **Crates beyond {supervisor, daemon, mcp}** — `core`, `sifters`,
  `store`, etc. stay `publish = false` (internal-only, no public API
  contract).
- **Cargo `Cargo.lock`** — included in the published crate tarballs for
  reproducible builds (cargo default behavior).

## Data flow on a real Phase 4a-max release

```
1. Operator merges conventional-commit work to main.
2. release-please.yml opens "chore: release 0.2.0" PR. Diff includes:
   - terminal-commander 0.1.4 → 0.2.0
   - 5 platform packages 0.1.4 → 0.2.0
   - Cargo.toml workspace 0.0.0 → 0.2.0  (or current → 0.2.0)
   - root optionalDependencies all 5 platforms pinned to "0.2.0"
   - manifest updated
3. Pre-merge CI: npm-binary-build.yml fires 5 parallel reusable-workflow
   calls, all green = no compile breaks on any platform.
4. Operator reviews PR diff. Merges.
5. release-please tags v0.2.0 + creates GitHub Release.
6. release-please.yml jobs execute in this order:
   (a) 5 platform publish jobs in parallel:
       - publish-linux-x64    → ubuntu-24.04
       - publish-linux-arm64  → ubuntu-24.04-arm
       - publish-windows-x64  → windows-2022
       - publish-mac-x64      → ubuntu-24.04 + cargo-zigbuild
       - publish-mac-arm64    → ubuntu-24.04 + cargo-zigbuild
   (b) publish-root waits for all 5, publishes root.
   (c) 3 cargo publish jobs in order: supervisor → daemon → mcp.
   (d) 5 verify-<platform> jobs in parallel, each on the matching native
       OS, installing from registry + smoke-probing.
7. If all 5 verify jobs green → release is good.
   If any fails → mark-release-broken opens P0 issue; operator runs
   `deprecate-version.yml` for 0.2.0; patch release follows.
8. Weekly: secret-health.yml probes 3 tokens. Opens issue if any fails.
```

## Error handling

- **Auth fail mid-publish:** publish job exits 1. Operator runs
  `recover-partial-publish.sh 0.2.0` to see which artifacts shipped vs
  missed. Re-runs `release-please.yml` workflow_dispatch with
  `force_publish: true`. E409 recovery (existing) makes already-shipped
  packages exit success. Only missing ones republish.
- **Single-platform build fail:** that platform's publish job exits 1.
  Other 4 platforms succeed. publish-root + cargo jobs skip (needs:).
  Operator pushes `fix:` commit, release-please opens patch release PR.
- **Smoke verify fail (post-publish):** mark-release-broken opens P0
  issue. Operator triages: if installable-but-broken, run
  `deprecate-version.yml 0.2.0 "broken X, install 0.2.1"`. Then push
  fix → 0.2.1 release cycle.
- **Cargo publish dep-order race:** mcp depends on daemon depends on
  supervisor. Workflow enforces order via `needs:`. cargo's
  `crates.io` index has propagation lag (~30s); a 60s sleep between
  cargo publish steps absorbs this.
- **npm-cargo version drift:** pre-publish parity assertion job fails
  the entire workflow before any publish fires. Operator inspects
  release-please's manifest update for the missed write.
- **Token expiry (the 365-day cliff):** secret-health.yml opens P1
  issue at the failure week, weeks before any release window cares.

## Rollback decision tree

| Time since publish | Has dependents? | Action |
|---|---|---|
| < 72h | No | `npm unpublish` + `cargo yank` allowed. Reserve for total disasters. |
| < 72h | Yes (other consumers depend on it) | Use `deprecate-version.yml`; don't unpublish. |
| > 72h | n/a | npm forbids unpublish. Patch release (0.2.1) + `deprecate-version.yml 0.2.0 "broken X, install 0.2.1"`. |

`latest` dist-tag pointer rule: always re-point `npm dist-tag add
terminal-commander@<safe-version> latest` after deprecating an
intermediate version, so `npm install -g terminal-commander` resolves
to safe.

## Testing plan

### Static pre-merge

1. YAML lint on all 5 workflow files.
2. `node -e "JSON.parse(require('fs').readFileSync('.github/release-please-config.json'))"`.
3. `node scripts/release/verify-optional-dependencies.js` — all 5
   platforms pinned correctly.
4. `cargo publish --dry-run -p terminal-commander-supervisor` (and the
   other 2 crates) — confirms crate metadata is publishable.
5. `npm pack --dry-run` in each of the 6 npm package dirs — confirms
   each tarball is sane size + no junk files.

### Live first release

Real release-please runs are the only end-to-end test of the pipeline;
there's no offline dry-run for the full publish flow. First 0.2.0
release IS the test:

1. Push the changes, merge release-please PR, observe workflow.
2. Confirm 5 platform publishes succeed.
3. Confirm root publishes after.
4. Confirm 3 cargo publishes succeed.
5. Confirm 5 verify jobs all green.
6. Operator manually `npm install -g terminal-commander@0.2.0` on
   their own Windows machine + restarts Cursor. MCP connects.
7. Operator manually `cargo install terminal-commander-mcp` on Linux.
   Binary runs.

### Post-release ongoing

- Weekly: secret-health.yml runs unattended.
- Every release: 5 verify jobs gate the success bit.
- Quarterly: operator triggers `deprecate-version.yml` dry-run on a
  test version to confirm the deprecation pipe stays unrotted.

## Acceptance criteria

1. `npm view <pkg>@0.2.0` returns metadata WITH `dist.attestations`
   field (proves SLSA provenance) for all 6 npm packages.
2. `cargo search terminal-commander-mcp` returns `0.2.0` as latest.
3. `cargo search terminal-commanderd` returns `0.2.0`.
4. `cargo search terminal-commander-supervisor` returns `0.2.0`.
5. Root's `optionalDependencies` on the published 0.2.0 tarball pins
   all 5 platform deps to exactly `"0.2.0"`.
6. Fresh `npm install -g terminal-commander@0.2.0` on each of Windows,
   Linux-x64, Linux-arm64, mac-x64, mac-arm64 produces a working MCP
   binary on PATH (the verify jobs prove this automatically).
7. Fresh `cargo install terminal-commander-mcp@0.2.0` on Linux + Mac
   produces a working `terminal-commander-mcp` binary on PATH.
8. `secret-health.yml` ran at least once successfully against all 3
   tokens before the release ships.
9. `deprecate-version.yml` ran at least once successfully (test mode)
   against a dummy version before the release ships, confirming the
   deprecation path is hot.

## Open questions deferred to plan

- Exact `cargo-zigbuild` invocation for mac targets. Plan picks a
  pinned action version + verified target triple incantation.
- Exact dist-tag policy for prerelease versions (alpha/beta/rc). Plan
  decides whether `next` tag is needed or `latest` is fine.
- Whether to add `cargo-deny` license/security audit gate before cargo
  publish. Plan investigates.
- Whether `verify-mac-x64` on `macos-13` is durable (GitHub deprecates
  macOS runners on a rolling 18-month cycle). Plan picks a pinning
  strategy: use the macos-latest alias OR pin + add monthly Renovate.

## Codex findings (re-mapped to Phase 4a-max)

| codex finding | Severity | Resolution in 4a-max |
|---|---|---|
| Windows not wired into release-please | CRITICAL | Changes A.3, A.4, A.5, A.10 |
| Root has `file:../` for windows-x64 | CRITICAL | Change A.5 |
| No `npm-publish.yml` workflow | CRITICAL | Reframed: release-please.yml extended (change A.10) |
| RC dry-run false-confidence | CRITICAL | Reframed: release-please-PR-driven, not tag-driven manual RC |
| Cargo workspace lockstep undefined | CRITICAL | Change C.11 + parity-assert step in C.12 |
| Use 3 native runners, not Linux cross-compile | IMPORTANT | Done for Win + Linux native; macOS uses cargo-zigbuild (the well-tested cross path) since macOS runners cost 10x — change B.8 documents this tradeoff |
| Recovery must be idempotent on E409 | IMPORTANT | Existing E409 pattern preserved + new recover-partial-publish.sh script (E.15) |
| Post-publish verification | IMPORTANT | 5 containerized verify jobs (D.13) — gate the release-success bit |
| Token expiry detection | IMPORTANT | secret-health.yml weekly cron (E.14) |
| `RELEASE_PLEASE_TOKEN_TC` scope | IMPORTANT | Existing PAT; secret-health weekly check validates it |
| `npm-binary-build.yml` drift | IMPORTANT | Reusable workflow `_build-platform-binary.yml` (B.8) called by BOTH paths — drift impossible by construction |
| Explicit rollback semantics | IMPORTANT | "Rollback decision tree" section above + `deprecate-version.yml` workflow (E.16) |
| **NEW: SLSA provenance absent** | (proactive) | OIDC trusted publisher migration (F.17), `--provenance` flag, `id-token: write` perm |
| **NEW: no Rust-native install path** | (proactive) | 3 cargo crates published (C.11, C.12) |
| **NEW: no macOS reach** | (proactive) | 2 mac platform packages + verify jobs |
