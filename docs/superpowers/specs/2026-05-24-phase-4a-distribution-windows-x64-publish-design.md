# Phase 4a — Distribution: publish @terminal-commander/windows-x64

**Date:** 2026-05-24
**Branch:** `feature/phase-4-deferred-gaps`
**Status:** Design approved, ready for implementation plan

---

## Problem

Phase 3 + 3.5 + 3.5b (merged to main as PR #8, 40 commits) added a
native Windows-x64 daemon + MCP runtime to the source tree. The
binaries build, the tests pass on Windows, the named-pipe IPC works.

But the operator's ADR-promised contract "one command install on a
fresh Windows machine" is undelivered:

- `@terminal-commander/windows-x64` is **not on the npm registry**.
- Root package's `optionalDependencies` declares the windows-x64 dep
  as `"file:../terminal-commander-windows-x64"` — a path that only
  resolves when the maintainer has a local working copy.
- `npm install -g terminal-commander@latest` on a fresh Windows
  machine therefore: installs the root wrapper, fails to resolve the
  Windows platform package, falls through to `platform_package_missing`,
  shim exits 64. The user sees no working MCP server.

## Goal

Publish `@terminal-commander/windows-x64@0.2.0` to the npm registry
alongside a matching republish of the existing linux-x64 + linux-arm64
+ root packages, so that the next clean `npm install -g
terminal-commander` on a Windows machine actually fetches the
platform package and produces a working MCP binary on PATH.

## Non-goals

- macOS publishing (tier-3 per ADR, deferred to a future phase).
- OIDC trusted publisher per-package setup (decided against; granular
  npm token is the chosen auth path).
- Replacing the existing `release-please.yml` workflow with a new
  tag-driven `npm-publish.yml`. The existing workflow does the right
  thing for linux + root; this design extends it to cover windows-x64.
- Bumping the Cargo workspace version. Cargo workspace stays at
  `0.0.0` since no crate is published to crates.io.
- Updating `npm-binary-build.yml` matrix to include Windows. That
  workflow is a PR-time artifact-only smoke; covering it in this
  phase risks scope creep. Phase 4a.2 territory.
- Automated post-publish install-into-clean-container verification.
  Manual fresh-Windows-VM smoke remains the gold-standard check.

## Operator decisions captured during brainstorm

- **Auth:** granular npm access token (`NPM_TOKEN_TC`) scoped to
  `@terminal-commander/*` + root, read+write, max-365-day expiry.
  Already set as repo secret. No OIDC trusted publisher per-package
  setup needed.
- **Trigger:** release-please-driven (existing path). Conventional
  commits → release PR → merge → release-please tags `v<version>` →
  publish jobs fire on the same workflow run.
- **Version strategy:** monorepo lockstep. All 4 npm packages share
  one version. release-please's `linked-versions` plugin enforces.
- **Publish order:** platform packages first (linux-x64, linux-arm64,
  windows-x64 in parallel), root last. Root's `optionalDependencies`
  pin platform versions exactly, so root must not publish until all
  three platform packages are on the registry.
- **Recovery on partial failure:** existing pattern. Each publish job
  catches `E409 already-on-registry` under the `force_publish`
  workflow-dispatch path and exits success, so a re-run after the
  first 2 platforms succeeded but the third failed is idempotent.
- **macOS:** not in this phase. Reconsider in a later phase.

## What currently exists

Read evidence:

- **`.github/release-please-config.json`** — manifest-mode release-please
  config. `linked-versions` plugin groups 3 components: `terminal-commander`,
  `@terminal-commander/linux-x64`, `@terminal-commander/linux-arm64`.
  Root package has 2 `extra-files` entries to bump its
  `optionalDependencies['@terminal-commander/linux-x64']` and
  `optionalDependencies['@terminal-commander/linux-arm64']`.

- **`.github/.release-please-manifest.json`** — tracks 3 components at
  `0.1.4`.

- **`packages/terminal-commander/package.json`** — root wrapper.
  `optionalDependencies` has the two linux packages at `0.1.4` and the
  windows-x64 dep as `"file:../terminal-commander-windows-x64"` (broken).

- **`packages/terminal-commander-windows-x64/package.json`** — exists
  with correct shape: name `@terminal-commander/windows-x64`, version
  `0.1.4`, `os: ["win32"]`, `cpu: ["x64"]`, `files: ["bin/"]`,
  `engines.node >= 18`.

- **`.github/workflows/release-please.yml`** — 4 jobs:
  - `release-please` — owns the PR + tag + Release creation
  - `ensure-release` — recovery for tag/Release/publish gaps
  - `publish-version` — manual workflow_dispatch recovery
  - `publish-linux-x64` — builds + stages + publishes linux-x64
  - `publish-linux-arm64` — same for linux-arm64
  - `publish-root` — depends on the two platform jobs, publishes root

  Plus an inline Node script in `publish-root` that asserts root's
  `optionalDependencies` exact-pin matches the release version.

- **`scripts/release/sync-optional-dependencies.js`** + **`verify-optional-dependencies.js`**
  — keep root `optionalDependencies` versions synced; verify exact
  pin. Currently handle the 2 linux packages.

## Design — the delta

### Files modified (6)

1. **`.github/release-please-config.json`**
   - Add `"@terminal-commander/windows-x64"` to the `linked-versions`
     plugin `components` array (3 entries → 4).
   - Add a 3rd `extra-files` entry on the root package entry to bump
     `optionalDependencies['@terminal-commander/windows-x64']`.
   - Add a new entry under `packages`:
     ```json
     "packages/terminal-commander-windows-x64": {
       "release-type": "node",
       "component": "@terminal-commander/windows-x64",
       "package-name": "@terminal-commander/windows-x64",
       "changelog-path": "CHANGELOG.md",
       "extra-files": []
     }
     ```

2. **`.github/.release-please-manifest.json`**
   - Add: `"packages/terminal-commander-windows-x64": "0.1.4"`

3. **`packages/terminal-commander/package.json`**
   - Change `optionalDependencies['@terminal-commander/windows-x64']`
     from `"file:../terminal-commander-windows-x64"` to `"0.1.4"`.
   - release-please will bump it to `"0.2.0"` on the next release PR.

4. **`packages/terminal-commander-windows-x64/package.json`**
   - No change required. Shape already correct.

5. **`.github/workflows/release-please.yml`**
   - Add a new job `publish-windows-x64` mirroring `publish-linux-x64`,
     but:
     - `runs-on: windows-2022` (or `windows-latest`)
     - Toolchain target: `x86_64-pc-windows-msvc`
     - Stage source: `target/x86_64-pc-windows-msvc/release/`
     - Bin filenames have `.exe`: `terminal-commanderd.exe`,
       `terminal-commander-mcp.exe`, `terminal-commander.exe`
     - Working directory for npm publish:
       `packages/terminal-commander-windows-x64`
     - Same `Swatinem/rust-cache@v2` with unique `shared-key:
       release-please-publish-windows-x64`
     - Same recovery pattern: catch `npm publish` failure under
       `force_publish` workflow-dispatch and check `npm view
       <pkg>@<ver>` to surface E409 as success.
     - Same `--help` smoke step adapted for `.exe` suffix.
     - Same version-match assertion.
   - Add `publish-windows-x64` to `publish-root`'s `needs:` array
     alongside the two linux jobs.
   - Update `publish-root`'s `if:` condition to also require
     `needs.publish-windows-x64.result == 'success'`.
   - Update `publish-root`'s inline Node version-pin assertion to
     also check `@terminal-commander/windows-x64`.

6. **`scripts/release/sync-optional-dependencies.js`** + **`verify-optional-dependencies.js`**
   - Add `@terminal-commander/windows-x64` to the package list in
     each script.

### Files NOT modified (intentionally)

- **`.github/workflows/npm-binary-build.yml`** — codex review flagged
  duplication risk between this workflow and the publish workflow.
  Real concern, but Phase 4a.2 territory. This phase ships the windows
  publish; cleanup of CI/publish duplication is a follow-up.
- **`.github/workflows/npm-bootstrap-publish.yml`** — codex review
  noted this could be retired. Keep for now; it's the documented
  manual-recovery escape hatch. Retiring is also Phase 4a.2.
- **`Cargo.toml`** workspace version — stays at `0.0.0`. Not published.

## Data flow on a real Phase 4a release

```
1. Operator commits the 6 file changes above + a 'feat:' commit
   noting the new platform. Pushes to main.
2. release-please-yml opens "chore: release 0.2.0" PR. PR diff:
   - terminal-commander 0.1.4 → 0.2.0
   - @terminal-commander/linux-x64 0.1.4 → 0.2.0
   - @terminal-commander/linux-arm64 0.1.4 → 0.2.0
   - @terminal-commander/windows-x64 0.1.4 → 0.2.0  (NEW)
   - root optionalDependencies all 3 platforms pinned to "0.2.0"
   - manifest updated
3. Operator reviews PR diff. Merges.
4. release-please tags v0.2.0 + creates GitHub Release.
5. On the same workflow run, 3 platform publish jobs run in
   parallel (each on its own runner):
   - publish-linux-x64    → ubuntu-24.04
   - publish-linux-arm64  → ubuntu-24.04-arm
   - publish-windows-x64  → windows-2022      (NEW)
6. publish-root waits for all 3, then publishes root.
7. `npm view @terminal-commander/windows-x64@0.2.0` confirms.
8. Manual smoke on a fresh Windows VM:
   - `npm install -g terminal-commander@0.2.0`
   - restart Cursor
   - observe MCP connect green
```

## Error handling

- **Auth fail (token revoked, expired, or scope mismatch):** publish
  job exits 1. Linux jobs may succeed, windows-x64 fails. Operator
  rotates `NPM_TOKEN_TC`. Re-runs workflow via `workflow_dispatch
  force_publish: true`. The recovery path's `npm view <pkg>@<ver>`
  check makes re-runs idempotent for already-published packages.
- **Windows build fail (compile error, missing target):** native
  windows-2022 runner reports the failure. Linux jobs unaffected. Root
  publish does not fire (needs all 3 platforms). Operator pushes fix
  as `fix:` commit, release-please opens patch release PR (0.2.1).
- **Manifest drift (manifest version != package.json version):**
  existing `Confirm package version matches release-please output`
  step catches it before publish.
- **Re-running after partial success:** `force_publish: true` +
  `npm view` recovery branch. Already-published packages exit success
  via the existing pattern in publish-linux-x64.

## Testing plan

1. **Pre-merge static checks:**
   - YAML lint on edited workflow.
   - `node -e "JSON.parse(require('fs').readFileSync('.github/release-please-config.json'))"` — config valid.
   - `node scripts/release/verify-optional-dependencies.js` — exact-pin
     scripts handle all 3 platforms.

2. **First test = real release.** Push the 6 changes, merge resulting
   release-please PR, observe workflow. Real release-please runs are
   the only thing that exercise the full pipeline; there's no offline
   dry-run for the full release-please + publish flow.

3. **Post-publish manual smoke:**
   - Fresh Windows VM (or just `npm cache clean --force` + `npm
     uninstall -g terminal-commander`)
   - `npm install -g terminal-commander@0.2.0`
   - `terminal-commander-mcp --version` returns a version string
     without exiting 64
   - Restart Cursor, observe MCP connects green (no "Connection
     closed")

## Acceptance criteria

- `npm view @terminal-commander/windows-x64@0.2.0` returns metadata
  (package on registry).
- `npm view @terminal-commander/linux-x64@0.2.0`,
  `npm view @terminal-commander/linux-arm64@0.2.0`,
  `npm view terminal-commander@0.2.0` all return metadata.
- Root's `optionalDependencies` on the published 0.2.0 tarball
  contains all 3 platform deps pinned to `"0.2.0"`.
- Fresh `npm install -g terminal-commander@0.2.0` on Windows produces
  a working `terminal-commander-mcp.exe` on PATH that responds to
  `initialize` over stdio.

## Open questions for the implementation plan

These are scoped decisions deferred to the plan, not the design:

- Exact `Cargo.toml.lock` handling. Plan investigates whether
  release-please needs `-l` flag or `cargo update -w --workspace` step
  after version bump.
- Exact action versions for Windows-runner cargo cache + Node setup.
  Plan picks pinned SHAs matching existing linux jobs.
- Whether to add a `permissions: id-token: write` block in
  preparation for future OIDC migration. Plan decides yes/no based on
  whether the granular-token path needs it.

## Codex findings (addressed)

Codex's whole-design review caught 5 CRITICAL + 7 IMPORTANT issues
on the original design assuming a clean slate. Re-reading the
existing infrastructure revealed it is **fully built for linux + root**
and just needs windows-x64 added. The revised delta-design addresses
codex's findings as follows:

| codex finding | Severity | Resolution |
|---|---|---|
| Windows not wired into release-please | CRITICAL | Fixed by changes 1+2 above |
| Root has `file:../` for windows-x64 | CRITICAL | Fixed by change 3 above |
| No `npm-publish.yml` workflow | CRITICAL | Reframed: existing `release-please.yml` does the job; this design extends it |
| RC dry-run false-confidence | CRITICAL | Reframed: existing workflow is release-please-PR-merge-driven (not tag-driven), so manual `-rc` tag is not the trigger. Workflow already asserts version-match before publish. |
| Cargo workspace lockstep undefined | CRITICAL | Removed from scope. Cargo stays at 0.0.0; not published. |
| Use 3 native runners, not Linux cross-compile | IMPORTANT | Already done: linux-x64 + linux-arm64 + (now) windows-2022 are all native. |
| Recovery must be idempotent on E409 | IMPORTANT | Already handled in existing linux jobs; new windows-x64 job mirrors the pattern. |
| Post-publish verification | IMPORTANT | Manual fresh-VM smoke documented in Testing plan. Automated install-into-container verification deferred to Phase 4a.2. |
| Token expiry detection | IMPORTANT | Operator-process responsibility. Add to a separate release-checklist artifact, not the workflow. |
| `RELEASE_PLEASE_TOKEN_TC` scope | IMPORTANT | Existing workflow uses the PAT for label edits via `gh pr edit`; the operator confirmed the secret is already in place and working for the linux releases. No change needed. |
| `npm-binary-build.yml` drift | IMPORTANT | Acknowledged as Phase 4a.2 follow-up. Out of scope. |
| Explicit rollback semantics | IMPORTANT | Documented in this spec's Error handling section: practical recovery is patch release, not unpublish. |
