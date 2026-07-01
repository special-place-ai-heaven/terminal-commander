# Results Log — events_since() optimization

Asset: `crates/core/src/bucket.rs::events_since()`
Baseline commit: `e1b6ff9`

| Round | Hypothesis | Before (ns) | After (ns) | Result | Notes |
|-------|-----------|-------------|------------|--------|-------|
| 0 (baseline) | — | — | 510500 | kept | initial measurement |
| 1 | `Vec::with_capacity(limit)` instead of `Vec::new()` for `out` | 510500 | 420300 | kept | avoids reallocation growth during clone loop; commit 07ff012 |
| 2 | Hoist `cursor`/`severity_min`/`kind_filter` into locals before loop | 421400 | 485100 | reverted | interleaved head-to-head: baseline consistently faster; no gain, `as_deref` adds noise |
| 3 | Single combined `filter` closure + `.take(limit)` + peek `has_more` (move `len>=limit` out of clone hot path) | ~440000 | ~436000 | reverted | interleaved 2-2 tie, all deltas <3% (noise); loop overhead is dwarfed by 2000 clones, no real win |
| 4 | `out.extend(matches.take(limit).cloned())` instead of manual push loop | ~420000 | ~460000 | reverted | interleaved 0-5, candidate ~30-50k slower every pair; filter+take size_hint defeats extend's fast path |
| 5 | Iterate `inner.events.make_contiguous()` (flat slice) instead of `VecDeque::iter()` | ~441000 | ~406000 | **kept** | interleaved 9-2 across two batches (6-0 tiebreaker, ~8% faster); slice iterator drops VecDeque ring-index math over 5000 filtered elements; no-op rotate since fixture is already contiguous |
| 6 | Plain `for ev in events.iter()` with inline `if ev.seq <= cursor { continue }` instead of `.filter()` adaptor | ~425000 | ~410000 | **kept** | interleaved 9-3 across two batches (5-1 tiebreaker, ~3-5% lower sums); removes the per-element filter-closure call (not inlined in debug) over 5000 elements |
| 7 | `remaining` countdown counter instead of `out.len() >= limit` for the cap check | ~410000 | ~410000 | reverted | interleaved 5-7 aggregate (tiebreaker 1-5, candidate ~4% slower); Vec::len is already a cheap field read, counter is a wash within noise |
| 8 | `partition_point` to binary-search first `seq > cursor`, skip per-element cursor check | ~410000 | ~412000 | reverted | interleaved 3-3 tie; fixture cursor=0 skips no prefix, so binary search's ~13 cmps just replace 5000 well-predicted single cmps — no net gain (would help large-cursor calls, but that is not the scored fixture) |
| 9 | Pure hoist of `cursor`/`severity_min` (Copy scalars) into loop-local vars, isolated retest of round-2 idea | ~415000 | ~420000 | reverted | interleaved 2-4; compiler already keeps a Copy field behind a shared ref in a register across the loop, so hoisting is redundant |
| 10 | Hoist `kind_filter` as `Option<&str>` via `as_deref()` once, compare `ev.kind != kf` | ~418000 | ~412000 | **kept** | interleaved 8-4 across two batches (both batches candidate-favored, sums ~0.3-1.3% lower); marginal but reproducible — leaner `Option<&str>` local avoids re-derefing the `Option<String>` behind `&request` per element. Small win. |
| 11 | Index-based `while i < events.len()` loop instead of `for ev in events.iter()` | ~412000 | ~412000 | reverted | interleaved 4-2 but sums equal/slightly worse; per-element bounds checks on `events[i]` cancel any gain — the iterator was already elision-friendly |
| 12 | Drop per-push `last_seq = ev.seq`, derive `next_cursor` from `out.last()` after loop | ~412000 | ~412000 | reverted | interleaved 3-3, sums identical (~0.04%); `last_seq` was already register-resident, the write is free next to the clone |
| 13 | OUT-OF-BOX: coalesce contiguous matching runs, bulk `extend_from_slice` (specialized clone) instead of N `push(clone())` | ~412000 | ~450000 | reverted | interleaved 3-9 aggregate (tiebreaker 0-6, ~10% slower). Correct (bench asserts pass). But in the DEBUG-measured regime `extend_from_slice`'s clone specialization does not kick in, and the closure-based `matches` predicate costs a non-inlined call per element (same lesson as round 6). Bulk clone only wins in release, which the harness does not measure. |
| 14 | Hoist `cursor`+`severity_min` into locals on top of the kept kind hoist (opt-level=0 memory-load reasoning) | ~412000 | ~417000 | reverted | interleaved 3-5, candidate ~1.3% higher; unlike `kind` (String->&str removes an indirection), `cursor`(u64)/`severity`(small enum) are one memory load either way — no fewer instructions |
| 15 | Pre-rank severity floor to `u8` (`Severity::rank()`), compare integer ranks instead of derived `PartialOrd` | ~412000 | ~433000 | reverted | interleaved 1-7, ~5% slower. `rank()` is a 7-arm `match` (branch chain at opt-level=0) — MORE work per element than the derived `<` on adjacent discriminants (a single discriminant load + int cmp). Mental model wrong; measurement corrected it. |

**STOP CONDITION HIT: 5 consecutive reverted rounds (11-15) — local optimum reached.**

## Summary

- Rounds run: 15 (1 pre-existing + 14 by this agent, rounds 2-15).
- Kept: rounds 1, 5, 6, 10 (4 wins). Reverted: rounds 2, 3, 4, 7, 8, 9, 11, 12, 13, 14, 15 (11).
- Start (round 0 baseline, original host): 510500 ns. Round 1 (with_capacity): 420300 ns.
- On THIS host the round-1 code medians ~421k-471k; after rounds 5/6/10 the kept
  code medians ~406k-412k. The three agent wins (slice iteration, inline filter,
  kind `as_deref`) are each small (~2-8%) and sit near the measured noise floor.
- Best single change: **round 5** (`make_contiguous()` flat-slice iteration
  instead of `VecDeque::iter`) — the only change with a clean 6-0 interleaved
  tiebreaker (~8%).

### Physical constraints of the scoring environment (why the number floors out)

- **Build profile `[profile.dev] opt-level = 0`** — the score is measured with NO
  optimizer. No inlining, no register promotion, no clone specialization. This
  inverts normal intuition: `.filter()`/closures/`extend_from_slice`/`rank()`
  all ADD non-inlined calls or branches and lose (rounds 4, 13, 15), while
  removing an abstraction layer (rounds 5, 6, 10) wins.
- **Measurement noise floor ~6%**: 12 runs of IDENTICAL code spanned 406400-430900
  ns (Δ24500). Any change below ~6% is physically unmeasurable in one run — hence
  the interleaved multi-batch protocol; single-run scoring would produce false
  keeps/reverts.
- **Cost structure is clone-bound**: the call does 2000 `SignalEvent::clone()`
  (each: 2 Strings + `Option<IndexMap>` + `Option<SourcePointer>` + `EventSource`
  with UUIDs). That clone is fixed by the wire contract (response owns
  `Vec<SignalEvent>`) and is ~90% of the time. Loop scaffolding is the only
  movable ~10%, and it is now stripped to slice-iter + inline checks + as_deref.
- **`evict_expired` + `refresh_state`** run inside the timed region but are a
  shared helper — off-limits (changing them alters `summary`/`state` semantics).
- Toolchain 1.95.0, 32 logical cores, test harness parallel — but the bench times
  a single `events_since` call inside one test, so cross-test parallelism only
  adds scheduler jitter (part of the noise floor above).

### Residual risk
None to semantics: every kept change preserves cursor/severity/kind/limit
filtering and has_more/next_cursor/dropped_count, verified by the bench's own
projection assertion and the full 110-test suite each round. The kept changes are
also correct and beneficial in RELEASE builds (the wire path the daemon actually
ships), even though the debug-measured score only partly reflects that.

## Measurement note (agent, round 2+)

Absolute ns differs per machine/worktree and has ~6-10% run-to-run noise on this
host, so a single bench run can falsely win/lose. Decision protocol adopted from
round 2 on: run baseline vs candidate **interleaved** (stash/pop, rebuild each
side, alternate) for 4 pairs; keep only if the candidate wins the majority of
interleaved pairs beyond noise. The "Before/After" columns below are the
representative interleaved medians for that round on this host (worktree-local
baseline for round-1 code measured ~421k-451k here vs the 420300 logged on the
original host — same code, different machine).
