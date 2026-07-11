# Release pipeline invariants

Status: current operational contract. Last updated: 2026-07-11.

Terminal Commander's release workflow supports three entry paths: a release
created by release-please, automatic gap recovery by `ensure-release`, and an
operator-triggered `force_publish` recovery. Those paths differ only before
`release-context`; every build, publish, verify, and failure-reporting job uses
the same canonical version after that point.

## Pipeline

```text
release-please ─┐
ensure-release ─┼─> release-context ─> prepublish-gate ─> platform builds
force-publish ──┘          |                    |                |
                           |                    |                v
                           |                    |         platform publishes
                           |                    |                |
                           |                    |                v
                           |                    |           root publish
                           |                    |                |
                           |                    |                v
                           |                    |          Cargo publish chain
                           |                    |                |
                           |                    |                v
                           |                    |       post-publish verification
                           |                    |
                           └──── release-verdict <───┴────> idempotent P0 reporter
```

## Locked invariants

1. **One release context.** Version and publish intent are selected once by
   `scripts/release/resolve-release-context.js`. Downstream jobs must not
   reconstruct release-please/ensure/force fallback expressions.
2. **Validate before side effects.** No npm or crates.io publish job is reachable
   unless `prepublish-gate` passes from a clean checkout.
3. **Every version anchor agrees.** The six package manifests, six
   release-please manifest entries, root package-lock anchors, Cargo workspace
   version, and root optional dependencies must equal the canonical version.
4. **Tests are checkout-independent.** Wrapper tests must not depend on ignored
   native binaries, the developer's host OS, or mutable module-loader hooks.
5. **Publishing is retry-safe.** Manual `force_publish` recovery treats an
   already-present registry version as success and continues toward missing
   artifacts and final verification.
6. **Failure reporting is independent.** The reporter does not require a Git
   checkout, always passes `--repo`, uses the canonical release identifier, and
   updates an existing open issue instead of creating duplicates.
7. **Completion is verified.** A release is not healthy merely because a tag or
   some platform packages exist; root npm, the Cargo chain, and all platform
   verification jobs must complete. Unexpected skips fail `release-verdict`.

## Executable checks

- `npm test` in `packages/terminal-commander` covers release-context selection,
  version-anchor consistency, workflow dependency closure, and the rule that
  every registry publish transitively depends on `prepublish-gate`.
- `node scripts/release/test-release-please-config.js` validates the linked
  package configuration.
- `node scripts/release/validate-release-inputs.js <version>` validates all
  committed version anchors.
- The required Linux and Windows pre-build gates run the wrapper suite from a
  clean checkout before native build jobs begin.

## Recovery

For a known version, first run:

```bash
bash scripts/release/recover-partial-publish.sh <version>
```

If only some artifacts exist, dispatch `release-please.yml` with
`force_publish: true`. Do not hand-edit version files or manufacture a competing
version source. If `release-context` fails, repair that disagreement before any
publish recovery.
