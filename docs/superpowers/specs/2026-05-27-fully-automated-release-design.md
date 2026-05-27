# Spec: Fully Automated Release (push crates/ -> npm binary)

Status: Design (decisions made 2026-05-27, no open questions). Language: ASCII only.

## Objective

Make `git push` of a `feat:`/`fix:` commit anywhere in the repo --
including `crates/` (the Rust binary source) -- automatically cut a
versioned release and publish installable binaries to npm, with NO
human step, and WITHOUT shipping a binary that fails to load on the
supported glibc floor.

## What already works (verified 2026-05-27)

The pipeline is ALREADY designed for zero-human release:

- `release-please.yml` (push:main) runs release-please in manifest mode,
  opens a release PR, and on the merge commit builds + publishes all 5
  platform npm packages + the root wrapper + 7 cargo crates.
- `release-pr-sync.yml` (pull_request) syncs versions and ENABLES
  AUTO-MERGE on the release PR (`gh pr merge --auto`, no human).
- `ensure-release` + `release_retry` recover the known post-auto-merge
  "release-please skips the tag" race.
- linked-versions plugin keeps all 6 npm components on one shared
  version; this 6-component linked group is already load-bearing in
  production (v0.1.13 released through it).

The chain is intact. It just never FIRES for `crates/` changes.

## Root cause (single)

release-please attributes commits to a package BY PATH PREFIX. All six
components live under `packages/`. A commit touching only `crates/`,
`docs/`, etc. yields "No commits for path: packages/..., skipping" ->
no release PR -> auto-merge has nothing to merge -> no publish. Every
feature commit this session (receipt, registry_import_pack, cargo pack
-- all `crates/`) hit exactly this and never released.

## Secondary gap (safety, for full-auto)

`publish-linux-x64 needs: build-linux-x64` only. The bookworm install
smoke (`verify-linux-*`) runs AFTER publish (`needs: publish-root`,
installs from npm). v0.1.13 shipped GLIBC_2.39 binaries that this
post-publish smoke caught too late. With no human merge gate, a broken
binary goes public before anyone sees red. The just-landed build-time
objdump glibc guard blocks the >2.35 class pre-publish, but a
load/runtime failure on the floor distro is only caught post-publish.

## Design

### Part 1 -- Trigger: root version-driver component

Add a `"."` root component so a `feat:`/`fix:` commit anywhere bumps the
shared version.

`release-please-config.json`:
- New `packages` entry keyed `"."`:
  - `release-type: simple` (no package.json; repo root is a cargo
    workspace; `simple` tracks the version via the manifest + an
    optional version file -- per release-please manifest-releaser docs).
  - `component: terminal-commander-root` (a non-published version
    DRIVER; distinct from the `terminal-commander` npm component so the
    tag/changelog surfaces do not collide).
  - `changelog-path: CHANGELOG.md` (root CHANGELOG).
  - `extra-files`: `version.txt` (the root anchor `simple` stamps).
- Add `"terminal-commander-root"` to `plugins[linked-versions].components`
  so a root-triggered bump propagates to all 6 npm components in
  lockstep (one shared version, one PR). Safe because the linked group
  is already multi-component in production; this adds a 7th linked
  member of the same shape -- the `version` output stays the single
  shared version the publish jobs already consume.

`.release-please-manifest.json`:
- Add `".": "0.1.13"`.

Repo root:
- New `version.txt` containing `0.1.13` wrapped in release-please
  version markers:
  ```
  0.1.13
  ```
  with `extra-files` of `{ "type": "generic", "path": "version.txt" }`
  on the root component (the `generic` updater rewrites the version via
  `x-release-please-*` markers OR a bare version line; the plan picks
  the exact marker form release-please requires for `generic`).

Semantics: `feat:`/`fix:` (and `feat!:` breaking) anywhere -> bump;
`docs:`/`chore:`/`test:`/`refactor:` -> no bump (release-please
default). `"."` path breadth is harmless: commit TYPE gates the bump,
not path. This session's `crates/` `feat:` commits would have released;
the `docs:` ones correctly would not.

`release-pr-sync.yml` reads the version from
`packages/terminal-commander/package.json`; linked-versions keeps that
equal to the root bump, so the read and the Cargo-sync still work
unchanged.

### Part 2 -- Safety: pre-publish glibc smoke gating publish

Add a linux-only pre-publish smoke that proves the BUILT binary loads
on the floor distro before it is published, and gate `publish-*` on it.

- New jobs `presmoke-linux-x64` / `presmoke-linux-arm64`:
  - `needs: build-linux-x64` / `build-linux-arm64` (consume the
    `tc-bin-<platform>` artifact already uploaded by the build).
  - Download + untar `bin.tar` (restores the +x bit).
  - `docker run --rm node:22-bookworm-slim` (Debian 12, glibc 2.36) and
    exec the STAGED binary directly:
    `./bin/terminal-commander-mcp --version` plus the same MCP
    initialize stdio probe the post-publish verify uses (10s timeout;
    rc 0 or 124 = pass, else fail). No `npm install`, no registry --
    this isolates the glibc-load class the v0.1.13 failure was.
  - arm64 presmoke runs on `ubuntu-24.04-arm` (docker can run the
    arm64 image natively there).
- `publish-linux-x64` gains `needs: presmoke-linux-x64`;
  `publish-linux-arm64` gains `needs: presmoke-linux-arm64` (in
  addition to the existing `build-*`). A failed presmoke -> no publish
  -> nothing broken goes public.
- The existing post-publish `verify-linux-*` jobs STAY as a final
  registry/packaging backstop.

Scope: presmoke is LINUX ONLY (the glibc-load class is linux-specific).
mac/windows keep their build-time native `--version` smoke + the
post-publish verify; no cheap floor-distro container exists for them
and they have no glibc dependency.

## Net flow after this spec

```
push feat: (crates/ or anywhere)
  -> release-please opens release PR (root component fired)        [Part 1]
  -> release-pr-sync enables auto-merge (no human)                 [existing]
  -> PR checks pass -> auto-merge -> merge commit re-triggers
  -> build-linux-* (+ objdump glibc guard, pre-publish)            [shipped]
  -> presmoke-linux-* (bookworm load test, pre-publish)            [Part 2]
  -> publish-* (npm + cargo)                                       [existing]
  -> verify-* (post-publish backstop)                              [existing]
  => `npm install -g terminal-commander` gets the new binary
```

## Verification

release-please + Actions cannot run locally. Proof is the next push:

1. A `feat:`-bearing commit (this spec's implementation will include a
   real `feat:` so the pipeline fires) opens a release PR bumping `.`
   + all 6 npm packages + Cargo to 0.1.14; auto-merge fires; presmoke
   passes on bookworm; publish + cargo publish run; post-publish verify
   green; `npm install -g terminal-commander@0.1.14` works on a clean
   box.
2. A later `docs:`-only push opens NO release PR (correct).

Pre-push static checks (local, in WSL):
- `node scripts/release/test-release-please-config.js` validates the
  config parses with the new root component.
- JSON validity of config + manifest.
- The release-please dry-run is not locally reproducible; CI is the
  authority (consistent with the glibc-fix decision).

## Out of scope

- Re-cutting / yanking v0.1.13's broken linux binaries (superseded by
  the next release; known-issue note "linux: use >= 0.1.14").
- macOS/windows pre-publish floor smoke (no cheap container; no glibc
  class).
- The cargo-crate version stream (already synced by
  `sync-cargo-versions.py` in release-pr-sync).
- Branch-protection / "Allow auto-merge" repo setting -- already
  configured (release-pr-sync depends on it; documented there).

## Provenance

Pipeline trace this session: release-please.yml (release-please ->
build -> publish -> verify), release-pr-sync.yml (auto-merge),
_build-platform-binary.yml (build + glibc guard, commit 538bba0). Root
cause: run 26482547975 log "No commits for path: packages/..., skipping".
release-please root-component + simple release-type: manifest-releaser
docs. Multi-component linked-versions already proven by v0.1.13.
