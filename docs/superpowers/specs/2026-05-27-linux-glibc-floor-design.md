# Spec: Linux Release Binary glibc Floor

Status: Design (brainstormed 2026-05-27). Language: ASCII only.

## Objective

Lower the glibc version required by the shipped linux release binaries
so they run on common distributions (Ubuntu 22.04, Debian 12 bookworm,
and newer), instead of demanding the build host's bleeding-edge glibc.

## The defect (verified 2026-05-27)

`.github/workflows/_build-platform-binary.yml` builds every platform on
a NATIVE runner. The linux entries (line 55) use `ubuntu-24.04`
(linux-x64) and `ubuntu-24.04-arm` (linux-arm64). Ubuntu 24.04 ships
glibc 2.39. A native `cargo build --release` links the host glibc, so
the produced binary records GLIBC_2.39 as its MINIMUM required version.

The v0.1.13 release (PR #18 merge, 2026-05-26) published successfully to
npm + crates.io, but the post-publish smoke jobs failed:

```
verify-linux-x64 / verify-linux-arm64:
  libc.so.6: version `GLIBC_2.39' not found
  (required by .../terminal-commander-mcp)
```

The verify jobs run the published npm binary inside
`node:22-bookworm-slim` (Debian 12, glibc 2.36). The binary cannot load
because it demands 2.39. Any user on glibc < 2.39 (bookworm, Ubuntu
22.04 = 2.35, RHEL 9, etc.) hits the same failure. The release shipped
broken linux binaries.

Note: this is a pre-existing pipeline defect, independent of the
agent-ergonomics work. It surfaced because the build runner's glibc
(2.39) outran the verify base image's glibc (2.36).

## Decision: build linux on ubuntu-22.04 (glibc 2.35)

Change the two linux entries in the `runs-on` platform map to
`ubuntu-22.04` and `ubuntu-22.04-arm`. Ubuntu 22.04 ships glibc 2.35, so
the binaries record a 2.35 floor and run on glibc 2.35+ (Ubuntu 22.04,
Debian 12 bookworm 2.36, RHEL 9, and everything newer). No new tooling.

`ubuntu-22.04-arm` is a generally-available GitHub-hosted runner label
(arm64 hosted runners went GA 2025-08); the repo already uses
`ubuntu-24.04-arm`, so arm hosted runners work here. The swap is
symmetric across both linux targets.

mac-x64 / mac-arm64 / windows-x64 entries are UNCHANGED: glibc does not
apply to them, and the existing comment documents why they stay on
native runners (Apple SDK is not redistributable, ruling out the
container/zigbuild path for macOS). This spec touches linux only.

### Alternatives considered and rejected

- **cargo-zigbuild with a pinned `gnu.2.XX` target.** Reaches a lower
  floor (e.g. 2.31, Ubuntu 20.04) AND is verifiable on this 2.39 WSL
  host (zig cross-links any chosen floor regardless of host glibc). The
  workflow comment notes it was the prior plan, dropped for macOS SDK
  licensing -- a mac-only concern. Rejected for THIS fix in favor of
  the minimal runner swap; the user accepts CI as the verification
  authority. Keep zigbuild on the table if 22.04 runners are later
  deprecated or a 2.31 floor becomes required.
- **manylinux container.** Same broader reach as zigbuild, more moving
  parts than a runner-label flip. Not warranted for a 2.35 floor.
- **musl static.** Runs anywhere incl. alpine, zero glibc dependency,
  but contradicts the workflow's explicit "our binaries are
  glibc-linked" design and the verify jobs' bookworm (glibc) base, and
  brings musl runtime quirks (DNS/getaddrinfo). Out of scope.

## Changes (one file)

`.github/workflows/_build-platform-binary.yml`:

1. **Runner map (line 55).** linux-x64 -> `ubuntu-22.04`; linux-arm64
   -> `ubuntu-22.04-arm`. mac/windows entries unchanged.
2. **Comment block (lines 51-54).** Add a line recording WHY linux pins
   22.04: "linux builds pin ubuntu-22.04 (glibc 2.35) so binaries run on
   glibc 2.35+; do NOT bump to 24.04 -- its glibc 2.39 re-breaks
   bookworm and older. When 22.04 hosted runners are retired, switch to
   cargo-zigbuild with a pinned gnu.2.x target rather than a newer
   native runner."
3. **glibc-floor guard (new step, linux only).** After the build,
   before staging, assert the produced binaries require no GLIBC symbol
   above 2.35. Deterministic drift guard: fails the build if a future
   change pushes the floor up, instead of waiting for the downstream
   bookworm smoke to catch a 2.36/2.37 regression that bookworm itself
   would not flag.

   Mechanism: for each built binary, run `objdump -T <bin>`, extract
   `GLIBC_X.Y` version tags, sort, take the max; if max > 2.35, print
   `::error::` with the offending symbol and exit 1. Gate the step on
   the platform being linux (`if: startsWith(inputs.platform,
   'linux')`) so mac/windows skip it. Use the binaries already staged in
   the platform package `bin/` dir (or the `target/<triple>/release`
   path) -- objdump is preinstalled on ubuntu runners.

## Verification

- **Primary (CI):** the existing `verify-linux-x64` and
  `verify-linux-arm64` jobs (run the published npm binary in
  `node:22-bookworm-slim`, glibc 2.36) must go from FAIL to PASS. They
  are the regression test for this defect.
- **In-build (CI):** the new objdump guard asserts floor <= 2.35 on
  every build, on the runner, before publish -- catching drift earlier
  than the smoke jobs.
- **Local limitation (documented):** this host's WSL is Ubuntu 24.04 /
  glibc 2.39, identical to the broken runner. A native `cargo build`
  here reproduces the DEFECT (objdump would show 2.39) but CANNOT
  produce a 2.35-floor binary (the host glibc is the floor for native
  builds). The runner-swap fix is therefore CI-verified, not
  locally-verified, by explicit choice. (cargo-zigbuild would have been
  locally verifiable; that tradeoff was weighed and the minimal swap
  chosen.)

## Out of scope / follow-ups

- Re-cutting / yanking the broken v0.1.13 linux binaries. Once this fix
  lands and a release is cut, v0.1.13's linux artifacts remain
  glibc-2.39. Decide separately whether to yank/deprecate them or just
  supersede with the next release.
- The release-please versioning gap (crates/ changes do not bump the
  shipped binary version) -- the user's SECOND requested fix, its own
  brainstorm/spec after this one.
- macOS/windows minimum-OS floors (not glibc; not in evidence here).

## Provenance

CI evidence: heaven/origin run 26450649668 (v0.1.13 release,
verify-linux-* failed on GLIBC_2.39). Build config:
`_build-platform-binary.yml:55`. Verify config:
`release-please.yml:1532-1572` (node:22-bookworm-slim). Runner label
availability: GitHub-hosted runners reference + arm64 GA changelog
(2025-08-07).
