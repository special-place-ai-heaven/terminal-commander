# Auto Research Engineer — Instructions

**This file is locked to the human. The agent reads it, never edits it.**

## Goal

We are optimizing `BucketManager::events_since()` in
`crates/core/src/bucket.rs` (the ASSET). This is the hot path hit by
every `BucketWait` and `BucketEventsSince` IPC call from the daemon —
it filters a bucket's event log by cursor/severity/kind and clones
matching events into a response `Vec<SignalEvent>`. Line 638 clones
every matched event; for large buckets under load this is a lot of
allocator churn on a path that runs on every agent poll.

**Why:** lower per-call latency here means faster `command_status` /
`run_and_watch` signal delivery across the whole daemon, with zero
change to the wire contract or semantics.

## Rules

1. The ONLY file you may change is the ASSET
   (`crates/core/src/bucket.rs`), plus the benchmark harness itself
   (`research/asset/bench_events_since.rs`) if a change to the
   benchmark is needed to measure fairly — but never to make scoring
   easier, only to keep it accurate as the function's shape changes.
2. You must NEVER edit `research/score.md` or the scoring logic
   embedded in the benchmark's assertions/measurement method. Read it
   to know how you'll be scored. Don't move the goalposts.
3. A change only counts as scored if it (a) compiles
   (`cargo build -p terminal-commander-core`) and (b) passes existing
   tests, specifically `cargo test -p terminal-commander-core` and
   the daemon's `agent_superiority_bench` test. A change that fails
   either is an automatic revert — never scored, never kept.
4. Semantics must not change: same filtering behavior (cursor,
   severity_min, kind_filter, limit, has_more, next_cursor,
   dropped_count). The output Vec of SignalEvents for a given input
   must be identical to the baseline's output. This is a performance
   optimization, not a behavior change.
5. Each round = one hypothesis, one change, one score. If it beats
   baseline, keep it (new baseline). If not, revert to the prior
   commit and try something else.
6. Run in ~5-minute loops, overnight, indefinitely, until the goal is
   hit or the human stops you.

## Stop conditions

- Stop after **5 consecutive reverted rounds** (local optimum reached), OR
- Stop after **40 rounds**, OR
- Stop after **3 hours** wall-clock,
  whichever comes first.

## Safety

- Each round is a commit on branch `research/events-since-optimize`.
  Baseline is tagged/committed before round 1. A losing round is
  reverted via `git checkout -- crates/core/src/bucket.rs` (or
  `git reset --hard` to the last winning commit) before starting the
  next hypothesis. `main` is never touched.
- Log every round in `research/results-log.md`: round #, hypothesis,
  before → after score, kept/reverted, and why.

## Definition of done

Report back in the morning with the results log and a summary:
starting score, final score, % improvement, and how many rounds it
took. Offer to open a PR from `research/events-since-optimize` if a
net win was found; do not merge or push without asking.
