# Wave 1 (P1) Integration Evidence

**Date**: 2026-07-02
**Integrated tree**: `d1db68e` on `002-dogfood-remediation`
(merges: US1 `d891f46`, US2 `2290b4d`, US3 `55d8055`)
**Compared against**: T001 baseline (`baseline.md`, all green at `edb54ca`)

## Both-platform gate on the merged tree

| Platform | Command | Result | Delta vs baseline |
|---|---|---|---|
| Windows | cargo fmt --all --check | PASS | - |
| Windows | cargo clippy --workspace --all-targets -- -D warnings | PASS | - |
| Windows | cargo nextest run --workspace | PASS — 860 run: 860 passed (1 leaky), 1 skipped | +11 tests, 0 new failures |
| WSL | cargo fmt --all --check | PASS | - |
| WSL | cargo clippy --workspace --all-targets -- -D warnings | PASS | - |
| WSL | cargo nextest run --workspace | PASS — 1106 run: 1106 passed, 1 skipped | +23 tests, 0 new failures |

## Notes

- The "1 leaky" Windows test is the same pre-existing one recorded at
  baseline — unchanged, passing.
- The four `terminal-commander-cli` live-daemon tests that flaked in the
  US3 worktree under triple-build load (`EndpointBindFailed` — the
  accept/recreate class US9 targets) PASSED in this integration run on a
  quiet machine. Recorded as load-sensitive flakiness, not a regression;
  US9's decision checkpoint (T046) should weigh this observation.
- Merge conflict resolution was confined to four re-export/import lists
  (`crates/daemon/src/ipc/mod.rs`, `crates/daemon/src/ipc/server.rs`,
  `crates/daemon/src/lib.rs`, `crates/mcp/src/tools.rs`), union-resolved;
  no semantic conflicts. US1's schema-derived validator picked up US2's
  selector changes and US3's new `list` action with zero edits — the
  self-updating design working as intended.
- Story-level red->green evidence (per FR) lives in the Wave 1 agent
  reports; per-story Windows gates ran green in each worktree before merge.

**Verdict**: US1, US2, US3 integrated and verified. SC-008 holds: zero new
failures attributable to the wave on either platform.
