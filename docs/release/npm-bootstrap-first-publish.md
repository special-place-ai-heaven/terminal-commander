# NPM10 — npm bootstrap first-publish policy exception

Status: NPM10 deliverable.
Branch: `main`.
Date: 2026-05-23.
Depends on: NPM07 ([`docs/release/npm-trusted-publishing-contract.md`](npm-trusted-publishing-contract.md)) + NPM09 ([`docs/release/npm-distribution-final-report.md`](npm-distribution-final-report.md)).

This document is the explicit, time-bounded **policy exception** to
the NPM07 OIDC-only contract. It exists to bootstrap the FIRST
publish of the three Terminal Commander npm packages so that
npmjs.com package pages exist, after which trusted-publisher
configuration becomes possible.

Language: ASCII only.

## 1. Why the exception exists

The NPM07 contract locked publishing to npm trusted publishing (OIDC
+ provenance) and explicitly rejected the long-lived `NPM_TOKEN_TC`
fallback. NPM09 closed the distribution chain with all three
package names at E404 / unpublished, gated by two npmjs.com
operator preconditions:

1. Claim the `@terminal-commander` organization.
2. Configure trusted publisher for each package page with workflow
   filename `release-please.yml`.

Operator finding (2026-05-23): npmjs.com may require the package
page to already exist before its trusted-publisher settings can be
configured. That is the classic bootstrap chicken-and-egg — the
OIDC path needs a package page; the package page needs a publish;
the publish needs OIDC.

NPM10 breaks the loop with a single, guarded, manually-dispatched
`NPM_TOKEN_TC` publish. After NPM10 succeeds, the OIDC-only path
resumes.

## 2. What changed

| Path | Change | Owner goal |
|------|--------|-----------|
| `.github/workflows/npm-bootstrap-publish.yml` | new — guarded `workflow_dispatch`-only publish workflow using `NPM_TOKEN_TC` | NPM10 |
| `docs/release/npm-bootstrap-first-publish.md` | new — this document | NPM10 |
| `BACKLOG.md` | add P1.5b — disable the bootstrap workflow after NPM10 succeeds | NPM10 |
| `RELEASE_CHECKLIST.md` | extend the npm-distribution gate to acknowledge the bootstrap workflow as the one-time exception | NPM10 |
| `.agent/goals/terminal-commander-npm-distribution/NPM10-*.md` | new goal file | NPM10 |
| `.agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md` | extend chain summary table with NPM10 row | NPM10 |
| `.agent/goals/terminal-commander-npm-distribution/RUN_ORDER.md` | append NPM10 as the explicit post-NPM09 bootstrap step | NPM10 |

NPM10 does NOT modify:

- `.github/workflows/release-please.yml` (NPM06 + NPM07).
- `.github/workflows/npm-binary-build.yml` (NPM05).
- `.github/release-please-config.json` / `.release-please-manifest.json`.
- `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`,
  `scripts/**`.
- Any `packages/*/package.json` (no version edit).
- Runtime behavior, MCP tool surface, or package architecture.

## 3. Bootstrap workflow shape

File: [`.github/workflows/npm-bootstrap-publish.yml`](../../.github/workflows/npm-bootstrap-publish.yml).

```
on: workflow_dispatch                (only — no push / release / schedule)
inputs:
  dry_run         (boolean, default true)
  confirm_publish (string,  default empty; real publish requires
                   exact value "publish-terminal-commander-beta")
permissions:
  contents: read                     (no id-token, no packages, no other)
jobs:
  resolve-gates
    outputs:
      real_publish (true iff dry_run==false AND confirm_publish==expected)
  publish-linux-x64       needs: resolve-gates
  publish-linux-arm64     needs: resolve-gates           (ubuntu-24.04-arm)
  publish-root            needs: resolve-gates +
                                  publish-linux-x64 +
                                  publish-linux-arm64
  post-publish-reminder   needs: resolve-gates + 3 publish jobs
                          if: real_publish == 'true'
```

Each platform publish job:

1. Checks out at `main` HEAD.
2. Installs Rust `1.95.0` + the matrix target.
3. Builds the three release binaries inline.
4. Stages binaries into `packages/<plat>/bin/`.
5. Runs `--help` smoke for each binary (native arch on arm64; no
   QEMU).
6. Confirms `package.json` `version == 0.1.0-beta.1`.
7. Runs the pre-publish E404 check: if the registry already hosts
   the name, the job fails LOUDLY before any publish call. This is
   defense in depth — it guards against accidental re-publish AND
   prevents a supply-chain takeover where a name was claimed after
   the workflow ran `npm view` at NPM09 time.
8. Prints a package banner (name, version, tag, mode).
9. Runs **either** `npm publish --dry-run --tag beta --access public`
   (gates closed) **or** the real `npm publish --tag beta --access public`
   with `NODE_AUTH_TOKEN` bound from `secrets.NPM_TOKEN_TC` (gates
   open).

The root job mirrors steps 1, 6, 7, 8, 9; it does not build any
binary (the root wrapper has no Rust). It also runs the NPM03
resolver unit tests (12 cases) before publish.

The `post-publish-reminder` job fires only on a real publish and
prints the required operator follow-ups.

## 4. Why provenance is NOT used at NPM10

npm's `--provenance` flag is designed to publish a SLSA attestation
that ties the tarball to a GitHub Actions workflow via OIDC. Adding
`--provenance` to a token publish does one of two things:

- npm rejects the publish (modern CLI requires OIDC for provenance).
- npm accepts it but the attestation references the OIDC token
  binding that did not actually authorize the publish — a
  misleading attestation.

Neither outcome is acceptable. NPM10 explicitly omits `--provenance`.
The honest signal is: this publish was authorized by a long-lived
token. Provenance returns at the very next publish, which flows
through NPM07's OIDC path.

## 5. Required operator actions

### 5.1 BEFORE dispatching the workflow

- [ ] Confirm `@terminal-commander` org exists on npmjs.com under
      a publisher account that owns `NPM_TOKEN_TC`. If the org does
      not exist, claim it first — the token must have publish
      authority over the org's scope or the publish will 403.
- [ ] Confirm `NPM_TOKEN_TC` has not been rotated / revoked since
      it was configured on the repo.
- [ ] Confirm all three names still return E404
      (`npm view <name> version`). If any name was claimed between
      NPM09 verification and dispatch, abort and re-plan — the
      bootstrap workflow itself will fail loudly at the
      pre-publish E404 check, but failing earlier saves a workflow
      run.
- [ ] Confirm the dispatch is happening on `main` at the NPM10
      verified-work commit or later (the workflow checks out
      whatever ref the dispatch was started from).

### 5.2 DISPATCH

1. Open the Actions tab for this repo.
2. Select the `npm-bootstrap-publish` workflow.
3. Click "Run workflow".
4. **First dispatch should be a dry-run**: leave `dry_run = true`,
   leave `confirm_publish` blank. Verify the four jobs succeed:
   - `resolve-gates` prints `real_publish=false`
   - `publish-linux-x64` runs `npm publish --dry-run --tag beta --access public`
   - `publish-linux-arm64` runs the same
   - `publish-root` runs `npm publish --dry-run --tag beta`
   - `post-publish-reminder` is `skipped`
5. **Second dispatch is the real publish**: set `dry_run = false`,
   type `publish-terminal-commander-beta` into `confirm_publish`,
   click "Run workflow". The four jobs run again; the
   `post-publish-reminder` job fires this time.

### 5.3 AFTER the real publish succeeds

- [ ] Verify each of the three names returns `0.1.0-beta.1`:
  - `npm view terminal-commander version`
  - `npm view @terminal-commander/linux-x64 version`
  - `npm view @terminal-commander/linux-arm64 version`
- [ ] Open `https://www.npmjs.com/package/<name>` for each of the
      three names and configure the trusted publisher in
      `Publishing access → Add trusted publisher`:
  - Publisher: `GitHub Actions`
  - Repository owner: `special-place-administrator`
  - Repository name: `terminal-commander`
  - Workflow filename: `release-please.yml` (exact match — NOT
    `npm-bootstrap-publish.yml`)
  - Environment: (blank)
- [ ] **Disable or remove the bootstrap workflow.** Choose one:
  - Delete `.github/workflows/npm-bootstrap-publish.yml` outright.
  - Rename it to `npm-bootstrap-publish.yml.disabled` so GitHub
    Actions stops picking it up.
  - Add a `disabled: true` annotation (GitHub does not natively
    support that; rename is the recommended path).
- [ ] **Rotate / invalidate `NPM_TOKEN_TC`** on npmjs.com so a
      future accidental dispatch cannot succeed.
- [ ] Verify that the next release flows entirely through
      `release-please.yml`:
  - Land a Conventional-Commits `feat:` or `fix:` commit on `main`.
  - release-please opens a release PR.
  - Operator reviews + merges the release PR.
  - The merge push triggers `release-please.yml`; release-please
    sets `releases_created='true'`; the OIDC-gated publish jobs
    fire with `--provenance`.

## 6. Rollback story (if the first publish goes wrong)

| Failure mode | Recovery |
|--------------|----------|
| One platform publish succeeds, the other fails | npm's `publish-root` job is gated on `needs: [publish-linux-x64, publish-linux-arm64]` and runs only after BOTH platform jobs succeed. If one platform fails, the root is not published; the orphan platform package is unusable on its own but the root install command (`npm install -g terminal-commander`) cannot have been published yet, so users cannot resolve the broken state. Recovery: fix the failing platform job (or its underlying cause), bump the version (e.g. `0.1.0-beta.2`), re-cut the bootstrap dispatch, OR pivot to NPM07's OIDC path once the trusted publisher is configurable on the partially-published name. |
| Both platforms publish, root fails | Both `@terminal-commander/linux-*` are public at `0.1.0-beta.1`; `terminal-commander` is unpublished. Recovery: fix the root job, dispatch again (the pre-publish E404 check will only fail for the names already published, so a partial recovery dispatch needs the root job to be re-runnable in isolation — that is a future refinement; the simpler path is to bump every package to `0.1.0-beta.2` and re-cut). |
| All three publish but trusted publisher cannot be configured | Operator has options: contact npm support; ship NPM_TOKEN_TC publishes for subsequent versions UNTIL trusted publisher works (which is exactly what NPM10 was designed to avoid); or accept token publishes as the long-term posture (which requires a contract amendment to NPM02 / NPM07). Document whichever path is taken. |
| `NPM_TOKEN_TC` is invalid / rotated | Workflow fails at the `npm publish` step with a 401. Recovery: regenerate the token on npmjs.com, update the repo secret, re-dispatch. |

## 7. Negative-surface confirmations (NPM10 implementation time)

- No `crates/`, `Cargo.toml`, `Cargo.lock`, `rules/`, `config/`,
  `scripts/` edit.
- No `.github/workflows/release-please.yml` edit.
- No `.github/workflows/npm-binary-build.yml` edit.
- No `.github/release-please-config.json` / `.release-please-manifest.json` edit.
- No `packages/*/package.json` edit (no version change).
- No `CARGO_REGISTRY_TOKEN_TC` reference anywhere.
- No `RELEASE_PLEASE_TOKEN_TC` reference anywhere.
- No `cargo publish` step.
- No crates.io reference (outside negative-documentation).
- No postinstall added to any package.json.
- No MCP tool change. MCP guard greps unchanged.
- No `Not Run` promoted to PASS. Beta posture remains `Conditional Go`
  (TC48 baseline preserved).

## 8. NPM11 hand-off

After the first publish succeeds AND the trusted publisher is
configured AND the bootstrap workflow is disabled / removed, open a
follow-up goal (NPM11) to:

- Delete `.github/workflows/npm-bootstrap-publish.yml`.
- Confirm `release-please.yml` is the only publish path active.
- Mark `NPM_TOKEN_TC` as decommissioned in `docs/release/`.
- Resolve BACKLOG P1.5 + P1.5b.

NPM10 explicitly does NOT pre-create NPM11.
