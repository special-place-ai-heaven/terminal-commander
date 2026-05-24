# Phase 4a — Release Procedure Runbook

Owner: terminal-commander maintainers
Created: 2026-05-24

## Pre-flight (one-time setup, before first release)

OIDC trusted publishers MUST be configured before the first release fires.
This was Task 0 of the Phase 4a-max plan; it is reproduced here as the
canonical procedure for any future package or crate added to the pipeline.

For each of the 6 npm packages:
- Pkg → Settings → Publishing access → Trusted Publishers → GitHub Actions
- Organization: `special-place-administrator`
- Repository: `terminal-commander`
- Workflow filename: `release-please.yml`
- Environment: (leave blank)

For each of the 7 cargo crates: same on crates.io.

Verify: re-run a small workflow_dispatch test. Confirm published tarball
shows `dist.attestations` (npm) or signed provenance metadata (crates.io).

## Normal release flow

1. Land conventional-commit work on `main`.
2. `release-please` opens "chore: release X.Y.Z" PR. Review the diff.
   - Confirm all 6 manifest entries bump.
   - Confirm `Cargo.toml` workspace version bumps + every `x-release-please-version`-marked line bumps.
   - Confirm root `optionalDependencies` all 5 platforms at new version.
3. Merge the PR.
4. Same workflow run: 5 platform builds (parallel) → 5 publish-platform
   (parallel, OIDC + provenance) → publish-root (OIDC + provenance) →
   7 cargo publish chain (OIDC, in strict dep order: core → sifters →
   probes → store → supervisor → daemon → mcp) → 5 verify jobs.
5. If all 5 verify jobs green: release is good. Done.
6. If any verify fails: `mark-release-broken` opens P0 issue. Go to
   "Broken release" below.

## Broken release

1. Triage the P0 issue. Determine which platform broke + why.
2. Run `.github/workflows/deprecate-version.yml`:
   - version: the broken version
   - reason: "broken {symptom}, install {next safe version}"
3. Push a `fix:` commit. release-please opens patch release PR (X.Y.Z+1).
4. Re-point `latest` dist-tag (npm only, cargo yank handles cargo side):
   ```
   npm dist-tag add terminal-commander@X.Y.Z+1 latest
   ```

## Partial-publish recovery

If the workflow died mid-publish (some packages on registry, others
missing):

1. Run: `bash scripts/release/recover-partial-publish.sh X.Y.Z`
2. The script reports missing artifacts + republish guidance (using
   crates.io HTTP API + npm view, both deterministic).
3. Re-run the release-please.yml workflow with `force_publish: true`
   for npm side. The E409-tolerant pattern republishes only missing.
4. For cargo side: from a checkout at tag `vX.Y.Z`, run the script's
   suggested `cargo publish -p <crate>` commands in dep order. Wait
   60s between each so crates.io index propagates.

## Secret rotation (annual)

`RELEASE_PLEASE_TOKEN_TC` (PAT for opening release PRs) has 365-day
max lifetime. Weekly `secret-health.yml` files P1 issue when probe fails.

Trusted-publisher OIDC auth does NOT have an expiry, so `NPM_TOKEN_TC`
and `CARGO_REGISTRY_TOKEN_TC` are now only emergency-recovery escape
hatches; they should not rotate on a schedule, but ARE still probed
weekly for liveness in case we need them for a `force_publish` recovery.

1. Rotate the failing token at its registry UI.
2. Update repo secret at https://github.com/special-place-administrator/terminal-commander/settings/secrets/actions
3. Manually re-run `secret-health.yml` to confirm green.

## macOS runner deprecation

GitHub deprecates macOS runners on roughly 18-month cycles. We pin
`macos-13` (Intel) + `macos-14` (Apple Silicon) for `_build-platform-binary.yml`
and `verify-mac-*` jobs. When deprecation warnings appear in workflow runs:

1. Pick the next-newest available macOS runners (e.g. `macos-14` Intel
   replacement, `macos-15` arm).
2. Update both `_build-platform-binary.yml` (runs-on map) and the two
   `verify-mac-*` jobs in `release-please.yml` in a single PR.
3. CI green = good. Old version users keep working; only the publish
   path changes.
