# T001 Baseline Gate Record

**Date**: 2026-07-02
**Commit**: edb54ca (branch `002-dogfood-remediation`; docs-only delta on a
CI-green release ancestor)
**Status**: GREEN — zero pre-existing failures on either platform.

| Platform | Command | Result |
|---|---|---|
| Windows | cargo fmt --all --check | PASS (exit 0) |
| Windows | cargo clippy --workspace --all-targets -- -D warnings | PASS (exit 0) |
| Windows | cargo nextest run --workspace | PASS — 849 tests run: 849 passed (1 leaky), 1 skipped |
| WSL (Linux) | cargo fmt --all --check | PASS (exit 0) |
| WSL (Linux) | cargo clippy --workspace --all-targets -- -D warnings | PASS (exit 0) |
| WSL (Linux) | cargo nextest run --workspace | PASS — 1083 tests run: 1083 passed, 1 skipped |

Notes:

- WSL gate run gate-exact: `CARGO_TARGET_DIR=$HOME/tc-linux-target`,
  `~/.cargo/bin/cargo` invoked directly.
- One test reported "leaky" by nextest on Windows (passed; process left a
  handle behind). Pre-existing characteristic, not a failure — recorded so
  it is not attributed to spec-002 changes later.
- Test-count difference between platforms is expected (`#[cfg(unix)]` /
  Windows-gated suites).

Acceptance: any wave-integration gate result is compared against this
record; only NEW failures are attributable to spec-002 work (SC-008).
