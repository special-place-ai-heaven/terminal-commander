# Results Log — events_since() optimization

Asset: `crates/core/src/bucket.rs::events_since()`
Baseline commit: `e1b6ff9`

| Round | Hypothesis | Before (ns) | After (ns) | Result | Notes |
|-------|-----------|-------------|------------|--------|-------|
| 0 (baseline) | — | — | 510500 | kept | initial measurement |
| 1 | `Vec::with_capacity(limit)` instead of `Vec::new()` for `out` | 510500 | 420300 | kept | avoids reallocation growth during clone loop; commit 07ff012 |
| 2 | Hoist `cursor`/`severity_min`/`kind_filter` into locals before loop | 421400 | 485100 | reverted | interleaved head-to-head: baseline consistently faster; no gain, `as_deref` adds noise |

## Measurement note (agent, round 2+)

Absolute ns differs per machine/worktree and has ~6-10% run-to-run noise on this
host, so a single bench run can falsely win/lose. Decision protocol adopted from
round 2 on: run baseline vs candidate **interleaved** (stash/pop, rebuild each
side, alternate) for 4 pairs; keep only if the candidate wins the majority of
interleaved pairs beyond noise. The "Before/After" columns below are the
representative interleaved medians for that round on this host (worktree-local
baseline for round-1 code measured ~421k-451k here vs the 420300 logged on the
original host — same code, different machine).
