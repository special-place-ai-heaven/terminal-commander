# NPM06 — release-please manifest-mode contract

Status: NPM06 deliverable.
Branch: `main`.
Date: 2026-05-23.
Depends on: [`docs/release/npm-binary-packaging-contract.md`](npm-binary-packaging-contract.md) (NPM02).

This document records the release-please manifest-mode configuration
that drives release PRs / version bumps / changelogs for the three
Terminal Commander npm packages. It is the binding NPM06 contract for
NPM07 and later goals.

Language: ASCII only.

## 1. Scope

NPM06 configures:

- `.github/release-please-config.json` — release-please manifest-mode config.
- `.github/.release-please-manifest.json` — version state file.
- `.github/workflows/release-please.yml` — release-please action runner.
- This document.

NPM06 does NOT touch publishing, OIDC, npm tokens, the NPM05 binary
build workflow, runtime code, or the package layout. Publishing is
NPM07's job.

## 2. Packages registered

| `packages/` path | npm `name` | release-please `component` | Initial version |
|------------------|------------|----------------------------|-----------------|
| `packages/terminal-commander` | `terminal-commander` | `terminal-commander` | `0.1.0-beta.1` |
| `packages/terminal-commander-linux-x64` | `@terminal-commander/linux-x64` | `@terminal-commander/linux-x64` | `0.1.0-beta.1` |
| `packages/terminal-commander-linux-arm64` | `@terminal-commander/linux-arm64` | `@terminal-commander/linux-arm64` | `0.1.0-beta.1` |

The on-disk directory names are the unscoped form
(`terminal-commander-linux-x64`, `terminal-commander-linux-arm64`);
the published npm names are the scoped form
(`@terminal-commander/linux-x64`, `@terminal-commander/linux-arm64`).
This split is documented in NPM02 §5 ("Notes").

## 3. Single shared version (locked)

All three packages move in lockstep. release-please enforces this via
the `linked-versions` plugin:

```json
{
  "plugins": [
    {
      "type": "linked-versions",
      "groupName": "terminal-commander",
      "components": [
        "terminal-commander",
        "@terminal-commander/linux-x64",
        "@terminal-commander/linux-arm64"
      ]
    }
  ]
}
```

Plus `separate-pull-requests: false` so release-please opens ONE
release PR (not three) per release cycle. Plus
`group-pull-request-title-pattern: "chore: release ${version}"` and
`pull-request-title-pattern: "chore: release ${version}"` so the
title carries the shared version, not a per-package component name.

This matches NPM02 §6 ("one shared semver across the root + the two
platform packages") and §12 ("release-please monorepo mode with
independent versions per package: rejected").

## 4. Pre-release posture

The manifest opens on `0.1.0-beta.1` (NPM02 §6, mirrors
`RELEASE_CHECKLIST.md`). The release-please config sets
`prerelease: true` so version bumps stay on the `-beta.N` track until
the operator promotes to a stable line. `bump-minor-pre-major: true`
+ `bump-patch-for-minor-pre-major: true` ensures feature-level
commits drive the `0.1.x` pre-release line (no 1.0.0 jump on the
first `feat:` commit).

## 5. Tag shape

- `include-component-in-tag: false` and
  `include-v-in-tag: true` together produce tags shaped like
  `v0.1.0-beta.2`, NOT `terminal-commander-v0.1.0-beta.2`.
- Single shared tag, single shared GitHub Release per cycle.
- Consequence: the GitHub Release covers all three packages at once.
  NPM07's publish workflow consumes that single release event and
  publishes the three packages in order (platform packages first,
  root wrapper last) per NPM02 §7.

## 6. Conventional Commits → version bump

release-please uses Conventional Commits to decide the bump:

| Commit prefix | Bump |
|---------------|------|
| `feat:` / `feat(...)`: | minor (pre-major, with `bump-minor-pre-major: true`) — for `0.1.x` track this is interpreted as the next `0.x.y` step |
| `fix:` / `fix(...)`: | patch |
| `chore:`, `docs:`, `refactor:`, `test:`, `style:`, `ci:`, `build:`, `perf:` | no bump unless `!:` (breaking change) is appended |
| `<type>!:` or footer `BREAKING CHANGE:` | major (held at minor pre-major) |

Scope conventions used inside the chain (per NPM02 §10):

- `wrapper` → root `terminal-commander` package
- `linux-x64` → `@terminal-commander/linux-x64`
- `linux-arm64` → `@terminal-commander/linux-arm64`

Because the three packages share one version line, a `feat:` in any
scope bumps the shared version; the linked-versions plugin then
synchronizes all three `package.json` files in the release PR.

## 7. Workflow behavior

- Trigger: `push` to `main` plus manual `workflow_dispatch`.
- Permissions: `contents: write` (commit bumps + changelog into the
  release PR), `pull-requests: write` (open/update the release PR).
  NOTHING ELSE.
- No `id-token: write` — that is NPM07's flag.
- No secrets other than the implicit `secrets.GITHUB_TOKEN` granted
  by the runner. No `NPM_TOKEN` referenced anywhere.
- Action pinned to immutable commit SHA, not a floating tag:
  - `googleapis/release-please-action@5c625bfb5d1ff62eadeeb3772007f7f66fdcf071`
  - Resolves to tag `v4.4.1` (verified via GitHub API on 2026-05-23).
- Concurrency: serialized per ref, `cancel-in-progress: false` so a
  follow-up push does not abort an in-flight release-please run that
  is updating the release PR.
- The workflow does NOT install Node, does NOT call `npm`, does NOT
  execute any package code. release-please-action handles the
  manifest update internally and pushes commits via the GitHub API.

## 8. Release PR review-gating

Release PRs are NOT auto-merged. The operator reviews + merges the
release PR manually for the first beta cuts. This matches NPM02 §7
("Release PR is review-gated; no auto-merge through the first beta
cuts").

On merge:

1. release-please creates the GitHub Release and the `v<version>` tag.
2. The release-please workflow updates `.github/.release-please-manifest.json`
   to the new version on the default branch.
3. NPM07's publish workflow (to be added in NPM07) listens to the
   `release.published` event and runs `npm publish --provenance`
   against the platform packages first, then the root wrapper.

NPM06 stops at step 1. Steps 2 and 3 are described here only so the
contract is auditable end-to-end; NPM06 does not implement step 3.

## 9. Changelog behavior

- Each package writes its own `CHANGELOG.md` (under its `packages/`
  directory) via `changelog-path: "CHANGELOG.md"` in the per-package
  config block. Since the three changelogs share one version line,
  the root wrapper's changelog is the authoritative human-readable
  log; the platform packages' changelogs are minimal mirrors.
- release-please groups commits by Conventional-Commits type
  (`Features`, `Bug Fixes`, etc.) using its default sections.
- Changelogs are written by release-please into the release PR;
  NPM06 does NOT pre-populate them.

## 10. Safety invariants

- No `npm publish` step anywhere in the workflow.
- No `NPM_TOKEN` reference.
- No `id-token: write`.
- No `postinstall` script added to any `package.json`.
- No edits to `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`,
  `config/**`, `scripts/**`.
- No new MCP tool, no new IPC surface, no new network listener.
- No edits to NPM05's `.github/workflows/npm-binary-build.yml`.
- The NPM05 binary build workflow remains independent of release-please;
  it builds + packs on every `main` push regardless of release-please
  state. The two workflows do not call each other at NPM06.

## 11. Validation done at NPM06

| Check | Result |
|-------|--------|
| `python3 -c "json.load(open('.github/release-please-config.json'))"` | parses |
| `python3 -c "json.load(open('.github/.release-please-manifest.json'))"` | parses |
| `python3 -c "yaml.safe_load(open('.github/workflows/release-please.yml'))"` | parses |
| All three `packages/*/package.json` versions equal | `0.1.0-beta.1` x 3 |
| Root `optionalDependencies` exact-pin to platform versions | `0.1.0-beta.1` exact |
| `rg "npm publish|NPM_TOKEN|secrets.NPM_TOKEN|postinstall"` over `.github packages docs/release` | only negative-documentation matches (this file + the audit + the contract describe what was rejected) |
| MCP grep guards | unchanged (no `crates/` edits) |
| Forbidden paths diff | empty |

## 12. Live GitHub Actions evidence

- Live release-please run: NOT executed at NPM06 because the chain
  rule says NPM06 verified work commit + status commit must land
  before push. Live evidence will be captured after the operator
  approves the push.
- Expected first live behavior: release-please opens a release PR
  titled `chore: release 0.1.0-beta.1` (no version bump unless a
  `feat:` / `fix:` lands first) OR no release PR if the manifest
  state matches the latest tag and there are no bumping commits
  since the last release. Both outcomes are valid.

## 13. Risks

| ID | Risk | Mitigation |
|----|------|-----------|
| R-NPM-10 | release-please-action v4 enters maintenance mode while NPM06 still depends on it | SHA pin is immutable; the chain can re-pin to v5 in a follow-up amendment if the API surface migrates. |
| R-NPM-11 | linked-versions plugin shape changes between v4 minor versions | SHA pin protects against silent shape drift; any re-pin re-validates the manifest. |
| R-NPM-12 | Pre-release shape (`0.1.0-beta.N`) misinterpreted by release-please as a major-release candidate | `prerelease: true` + `bump-minor-pre-major: true` keep the bumps inside the `0.1.x` line. |

## 14. Acceptance against NPM06 mini-spec

- [x] Manifest config validates as JSON.
- [x] Manifest state file validates as JSON.
- [x] Workflow YAML parses.
- [x] Three packages registered; one shared version; one shared tag.
- [x] release-please-action pinned to immutable commit SHA.
- [x] No publish step; no `NPM_TOKEN`; no `id-token: write`.
- [x] `crates/**` untouched.
- [x] NPM05 workflow file untouched.
- [x] Path layout matches NPM02 §7 (Symforge precedent).
- [x] Operator preconditions from NPM02 §1.2 still apply (org claim
      gates NPM07, not NPM06).

## 15. NPM07 hand-off (superseded by NPM07; see note)

> **Superseded 2026-05-23 by NPM07.** The NPM06 hand-off note below
> originally described a SEPARATE `on: release.published` workflow.
> NPM07's user-locked design moved publishing INTO this same
> `release-please.yml` workflow as downstream jobs gated by
> `release-please.outputs.releases_created == 'true'`. The reason
> recorded in `docs/release/npm-trusted-publishing-contract.md` §2
> is the GitHub Actions rule that `GITHUB_TOKEN`-created release
> events do not trigger downstream `on: release` workflows; and the
> user explicitly forbade switching release-please to a PAT.
> The NPM06 release-please job itself is unchanged — only the
> "where does publish live" decision was deferred to NPM07.

Historical NPM06 hand-off note (kept for audit, not authoritative):

NPM07 was originally going to add a SEPARATE workflow file
(path TBD; not prescribed at NPM06) that:

- Triggers on `release.published`.
- Sets `permissions: id-token: write` + `contents: read`.
- Calls `npm publish --provenance` per package.
- Honors the publish order: platform packages first, root wrapper
  last.
- Uses the first dist-tag `beta`.
- Has no `NPM_TOKEN` unless NPM07 records an explicit fallback
  approval.

NPM06 did NOT pre-create that workflow. NPM07's actual implementation
lives inside this same `release-please.yml`; see
`docs/release/npm-trusted-publishing-contract.md` for the binding
contract.
