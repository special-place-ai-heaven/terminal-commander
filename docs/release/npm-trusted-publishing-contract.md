# NPM07 — npm trusted publishing contract

Status: NPM07 deliverable.
Branch: `main`.
Date: 2026-05-23.
Depends on: [`docs/release/release-please-contract.md`](release-please-contract.md) (NPM06), [`docs/release/npm-binary-packaging-contract.md`](npm-binary-packaging-contract.md) (NPM02).

This document is the binding NPM07 contract for the chain. It locks
how the three Terminal Commander npm packages are published from
GitHub Actions via npm trusted publishing (OIDC) + provenance, with
no long-lived secrets.

Language: ASCII only.

## 1. Scope

NPM07 amends `.github/workflows/release-please.yml` (added by NPM06)
to add THREE downstream publish jobs that run only when release-please
reports `releases_created == 'true'` on the same workflow invocation.
No new workflow file is created at NPM07; no separate `on: release`
workflow exists.

NPM07 does NOT:
- store or reference any long-lived secret (`NPM_TOKEN`,
  `NPM_TOKEN_TC`, `CARGO_REGISTRY_TOKEN_TC`, `RELEASE_PLEASE_TOKEN_TC`,
  or any other PAT),
- publish to crates.io,
- run `cargo publish`,
- introduce a postinstall downloader,
- add macOS / Windows / musl support,
- change any `crates/`, `Cargo.toml`, `Cargo.lock`, `rules/`,
  `config/`, or `scripts/` file,
- touch `.github/workflows/npm-binary-build.yml` (NPM05; kept as a
  non-publishing CI gate),
- auto-merge the release PR,
- change the package layout or the resolver shims (NPM03 layout
  remains live).

## 2. Why "same workflow, output-gated"

The user directive on 2026-05-23 locked the publish design to "same
workflow as release-please, gated by release-please outputs." The
reasoning, recorded here so the contract is auditable:

- release-please currently uses `secrets.GITHUB_TOKEN` to commit the
  release PR. GitHub Actions' "GITHUB_TOKEN does not trigger
  downstream workflow runs" rule means a `release.published` event
  fired by release-please via the default token will NOT trigger a
  separate `on: release` workflow. A separate publish workflow
  would silently never run on the first beta release.
- The only workarounds are (a) use a PAT (`RELEASE_PLEASE_TOKEN_TC`)
  so release-please's events trigger downstream runs, or (b) move
  publishing into the SAME workflow run, gated by release-please's
  step outputs. Option (a) is explicitly rejected by the
  user-locked secret policy.
- Therefore: same workflow, output-gated. Each publish job declares
  `needs: release-please` + `if: needs.release-please.outputs.releases_created == 'true'`.
  When the merge of the release PR triggers `push: main`, the same
  workflow run also runs the publish jobs.

This decision supersedes the NPM06 contract §15 hand-off note that
mentioned `on: release` triggers. NPM06 itself is not reopened;
the supersedure is recorded here and in the workflow file's header
comment.

## 3. Binary-source decision (inline build)

The publish jobs build the three Rust binaries INLINE on the runner
that runs them, from the checkout of the merge-commit on `main`
(the same SHA release-please tagged). The publish workflow does
NOT download artifacts from the `npm-binary-build` workflow.

Reasons (recorded so the choice is auditable):

- **Provenance fidelity.** `npm publish --provenance` attests to the
  workflow that executes the publish. Building inline means the
  attestation's `buildConfig` references the actual `cargo build`
  steps. Downloading prebuilt tarballs from another workflow run
  would leave provenance attesting to a copy step, not the build,
  which is the worst kind of half-truth.
- **No cross-workflow coupling.** Artifact-retention windows
  (`actions/upload-artifact` retention: 14 days at NPM05) could
  silently expire before a release PR is merged, breaking the
  release in a hard-to-debug way. Inline builds carry no such
  dependency.
- **One source of truth.** The same checkout that release-please
  tagged is the input to the build. No risk of "the tarball was
  built from an older SHA than the tag."

`.github/workflows/npm-binary-build.yml` remains independent: it
builds + packs on every push (NPM05 contract) and never publishes.
It exists as the non-publishing CI gate; the publish workflow does
not depend on its artifacts.

## 4. Workflow shape (final)

`.github/workflows/release-please.yml` contains FOUR jobs:

| Job | Runner | Permissions | When it runs |
|-----|--------|-------------|--------------|
| `release-please` | `ubuntu-24.04` | `contents: write` + `pull-requests: write` | every push to `main` + `workflow_dispatch` |
| `publish-linux-x64` | `ubuntu-24.04` | `id-token: write` + `contents: read` | only if `release-please.outputs.releases_created == 'true'` |
| `publish-linux-arm64` | `ubuntu-24.04-arm` | `id-token: write` + `contents: read` | only if `release-please.outputs.releases_created == 'true'` |
| `publish-root` | `ubuntu-24.04` | `id-token: write` + `contents: read` | only if `release-please.outputs.releases_created == 'true'` AND both platform jobs succeeded |

Dependency graph:

```
release-please
   ├─→ publish-linux-x64
   ├─→ publish-linux-arm64
   └─→ publish-root  (depends on the two platform jobs above)
```

Workflow-level `permissions: contents: read` declares the implicit
default; each job overrides with its own minimal claim.

The release-please job retains `permissions: contents: write` +
`pull-requests: write` ONLY at the job level (release-please needs
those to commit version bumps + open the release PR). The release-
please job does NOT request `id-token: write`; OIDC permission is
scoped strictly to the three publish jobs.

## 5. Trigger logic — when publishing actually happens

| Event | release-please runs? | Publish runs? | Why |
|-------|----------------------|---------------|-----|
| `feat:` / `fix:` push to `main` | yes | no | release-please opens / updates the release PR; `releases_created` is `'false'` because the merge has not happened yet |
| `chore:` / docs / refactor push to `main` | yes | no | release-please runs but finds no user-facing commits; outputs `releases_created == 'false'` (this is the documented no-op observed in NPM06 live run) |
| `workflow_dispatch` (no eligible commits) | yes | no | same as above |
| Merge of the release PR | yes | **yes** | The release PR merge produces a push to `main`; release-please runs, creates the GitHub Release + tag, sets `releases_created == 'true'`, and the publish jobs fire |
| Push that does NOT match `paths:` (NPM05's filter; not present on this workflow) | yes | no | release-please.yml has no `paths:` filter — it runs on every `main` push, which is correct because release-please is the source of truth for whether to publish |

Note: `release-please.yml` deliberately omits a `paths:` filter so
release-please always evaluates state. The cost (one no-op run per
unrelated push) is small; the benefit (no risk of missing a
release-PR-merge push) is large.

## 6. Publish order (locked)

1. `@terminal-commander/linux-x64`  (job `publish-linux-x64`)
2. `@terminal-commander/linux-arm64` (job `publish-linux-arm64`)
3. `terminal-commander`              (job `publish-root`)

Jobs 1 and 2 run in parallel (no `needs:` between them). Job 3
declares `needs: [release-please, publish-linux-x64,
publish-linux-arm64]`, so the root wrapper publishes ONLY after
both platform packages succeeded. This matches NPM02 §7
("Publish order: platform packages first, root wrapper last").

If either platform job fails, `publish-root` is skipped (npm leaves
the root unpinned to non-existent platform deps; the operator
investigates the platform-job failure and either reruns or cuts a
new release). The root's `optionalDependencies` exact-pin to the
shared version is the safety net.

## 7. Auth — npm trusted publishing (OIDC)

- Each publish job sets `permissions: id-token: write` + `contents: read`.
- `actions/setup-node@v4` is invoked with `registry-url: "https://registry.npmjs.org"`.
- `npm publish --provenance` automatically uses the OIDC token via
  the npm trusted-publishing flow (npm CLI ≥ 9.5 / ≥ 11 detects the
  GitHub Actions OIDC environment and exchanges the ID token for a
  short-lived publish token).
- The shell environment does NOT contain `NODE_AUTH_TOKEN`,
  `NPM_TOKEN`, or any `.npmrc` token entry. There is no
  `_authToken` line anywhere. Provenance + OIDC is the only auth
  path.

Secrets explicitly NOT referenced (verified by grep over `.github`):

- `secrets.NPM_TOKEN`
- `secrets.NPM_TOKEN_TC`
- `secrets.CARGO_REGISTRY_TOKEN_TC`
- `secrets.RELEASE_PLEASE_TOKEN_TC`
- Any `NODE_AUTH_TOKEN` env injection

If npm rejects the publish for any reason (org not claimed, trusted
publisher not configured, package name already taken, etc.), the
job fails LOUDLY. There is no token fallback. NPM07's stop-condition
explicitly forbids silently switching to `NPM_TOKEN_TC`.

## 8. Operator preconditions on npmjs.com

Before the FIRST publish run can succeed, an npmjs.com account with
org-owner / package-owner permissions MUST complete these steps. NPM07
does not automate them; NPM07 documents them.

| # | Action | Notes |
|---|--------|-------|
| 1 | Claim the `@terminal-commander` organization on npmjs.com | NPM02 §1.2 already flagged this. |
| 2 | Reserve all three package names on the registry | `terminal-commander`, `@terminal-commander/linux-x64`, `@terminal-commander/linux-arm64`. The reservation can be done by publishing a stub 0.0.0 version with the `--access public` flag, OR by the npmjs.com `Add Package` flow for the scope. If a name is taken when the operator gets there, follow NPM02 §1.3 (stop and amend, do not silently rename). |
| 3 | For each of the three packages, configure a trusted publisher in the npmjs.com package settings → "Publishing access" → "Add trusted publisher" with: <br>· Publisher: GitHub Actions <br>· Repository owner: `special-place-administrator` <br>· Repository name: `terminal-commander` <br>· Workflow filename: `release-please.yml` <br>· Environment: (leave blank — no environment is used) | Workflow filename MUST match the actual workflow that publishes. NPM07 publishes from `.github/workflows/release-please.yml`. If the operator selects a different filename, npm will reject the OIDC token. |
| 4 | (Optional) Pre-register the npm dist-tags | Not required. The first `npm publish --tag beta` creates the `beta` dist-tag implicitly. |
| 5 | Document the steps so a future operator can mirror them | Recorded in this file + `docs/release/npm-binary-packaging-contract.md` §11. |

Until step 3 is complete for all three packages, the FIRST live
publish attempt will fail at the OIDC handshake with an npm error
along the lines of `EOIDCNOTFOUND` or `403 Forbidden: trusted
publisher not configured`. That failure is the recorded blocker,
NOT a workflow defect.

Live npm publication is therefore marked **Pending operator setup**
at NPM07 close. NPM09 captures the first real publish transcript.

## 9. Pre-publish guards inside each job

Each publish job runs these checks BEFORE `npm publish`:

| Guard | Job | What it asserts |
|-------|-----|-----------------|
| Toolchain pinned `1.95.0` | platform jobs | Build matches NPM05 baseline |
| Native arm64 runner (not QEMU) | `publish-linux-arm64` | NPM05 honesty rule preserved |
| `cargo build --release` succeeds for the matrix target | platform jobs | Real binaries produced |
| Real binary `--help` returns 0 | platform jobs | Binary is runnable on its native arch |
| `.placeholder` files removed from `bin/` before pack | platform jobs | The committed placeholders do not end up in the tarball alongside the real binaries (both would match `files: ["bin/"]`) |
| `package.json` `version` equals `release-please.outputs.version` | all publish jobs | Defense in depth: catches drift between the committed package.json and the release-please-computed version |
| `optionalDependencies` exact-pin to the shared version | `publish-root` | Catches a manifest desync that would publish a broken root wrapper |
| Resolver unit tests (`npm test`) | `publish-root` | The NPM03 resolver test suite (12 cases) runs against the published code |

A guard failure aborts the job with a clear `::error::` line; no
fallback path masks it.

## 10. Provenance attestation surface

When `npm publish --provenance` succeeds, npm publishes a SLSA
provenance attestation alongside the tarball. The attestation
records:

- The GitHub Actions workflow file path (`.github/workflows/release-please.yml`)
- The git SHA being published (the merge-commit on `main`)
- The runner image, the action versions used, and the steps that
  produced the tarball
- The OIDC `subject` claim that authorized the publish

Operators / consumers can verify provenance with:

```bash
npm install --foreground-scripts=false @terminal-commander/linux-x64
npm audit signatures
```

…or via the npm provenance UI at
`https://www.npmjs.com/package/<pkg>/v/<version>` once the publish
completes.

## 11. Failure modes + recovery

| Failure | Recovery |
|---------|----------|
| `ubuntu-24.04-arm` runner label not allocated | Same blocker doctrine as NPM05. Workflow fails LOUDLY; operator either acquires the allowance or amends NPM07 to use QEMU `cross` (a follow-up goal, not a silent fix). Do NOT fake arm64 with QEMU at NPM07. |
| Trusted publisher not configured on npmjs.com | Job fails at the `npm publish` step. Operator completes §8 step 3 and re-runs the publish job (or cuts a fresh release PR). No `NPM_TOKEN` fallback. |
| One platform job fails after the other succeeded | `publish-root` is skipped (because `needs:` is unsatisfied). The successful platform package is now published at a version the root cannot resolve. Recovery: fix the failing platform job, then either (a) `gh run rerun <id> --failed` so the missing platform + root publish, or (b) cut a new patch release that supersedes the broken state. NPM09 owns the recovery rehearsal. |
| Root job fails after both platforms succeeded | Both platform packages are at `0.x.y`; root wrapper is stuck at the previous version. Recovery: fix the root job (or its underlying cause), `gh run rerun <id> --failed`. The platform-package republish on rerun is a no-op (npm rejects re-publish of the same version), so only the root publishes. |
| `npm test` (resolver tests) fails inside `publish-root` | Job aborts before any publish. No tarball reaches the registry. |

## 12. Why a separate `on: release` workflow was rejected

| Option considered | Why rejected |
|-------------------|-------------|
| Separate workflow at `.github/workflows/npm-publish.yml` triggered by `on: release: types: [published]` | GitHub Actions does not fire `release.published` workflows when the release is created with `GITHUB_TOKEN`. release-please uses `GITHUB_TOKEN` (NPM06 contract). Result: the workflow would never fire on the first beta release. |
| Same separate workflow but with release-please switched to `RELEASE_PLEASE_TOKEN_TC` | User locked `RELEASE_PLEASE_TOKEN_TC` as **unused** on 2026-05-23. Reopening release-please's token plumbing is outside NPM07 scope. |
| Same separate workflow but triggered by `on: workflow_run` of release-please | Couples two workflows by name + adds extra layers of "did the right thing happen?" logic. Strictly less safe than reading release-please outputs directly inside the same workflow. |
| Inline download from `npm-binary-build` artifacts | Provenance attestation ends up describing a `gh run download` step, not the build. Breaks the SLSA attestation chain semantics. |

## 13. Acceptance against NPM07 mini-spec

- [x] OIDC `id-token: write` set only on publish jobs.
- [x] `npm publish --provenance` on every package.
- [x] `--tag beta` on the first publish.
- [x] Platform packages publish first, root wrapper last.
- [x] No `secrets.NPM_TOKEN` / `_TC` reference anywhere in the
      workflow.
- [x] No `cargo publish` / crates.io step.
- [x] Operator setup notes documented in this file §8.
- [x] Workflow YAML parses.
- [x] No `crates/**` / `Cargo.toml` / `Cargo.lock` / `rules/**` /
      `config/**` / `scripts/**` change.
- [x] NPM05's `npm-binary-build.yml` untouched.
- [x] Resolver unit tests still pass.
- [x] `npm pack --dry-run` clean for all three packages.
- [x] Versions + optionalDependencies still synchronized.

## 14. Live publish status

**Not Run / Pending release PR merge.** No live publish has been
attempted at NPM07 close. Reasons:

- No release PR exists at NPM07 close (release-please's NPM06 run
  was a documented no-op because no `feat:` / `fix:` commits since
  the manifest seed).
- npmjs.com operator preconditions in §8 must complete before the
  first publish can succeed. Until step 3 finishes for all three
  packages, the OIDC handshake will fail.

NPM07 captures the workflow change + the contract. The first live
publish transcript is NPM09's deliverable.

## 15. NPM08 hand-off

NPM08 (Cursor MCP install + provider live smoke) consumes the
published packages once §8 + the first release PR merge complete.
NPM08 does NOT publish anything. NPM07 does not pre-create NPM08
artifacts.
