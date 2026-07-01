# Scoring — LOCKED

**This file defines "better." The agent may READ it to know how to
score a round. The agent must NEVER edit this file or the assertions
inside `bench_events_since.rs` that implement it. Editing either to
inflate the score invalidates every round since.**

## The number

**Median wall-clock nanoseconds for one `events_since()` call**, over
10 runs, on a fixed synthetic fixture. Lower is better.

## Fixture (fixed, deterministic, never changes)

- One bucket, `BucketConfig::default()`.
- 5,000 events appended, `seq` 1..=5000, alternating severity
  `Low`/`Medium`/`High` (round-robin), alternating `kind` between
  `"a"` and `"b"`, each cloned from the same base fixture event
  (same shape as `event.rs`'s `fixture_event_with_pointer`).
- Query: `BucketReadRequest { cursor: 0, severity_min:
  Some(Severity::Low), kind_filter: None, limit: Some(2000) }` — this
  matches ~5,000 events but is capped at 2000 returned, so the clone
  loop does real work without being trivially small.

## Procedure

1. Build the fixture fresh (no shared/mutated state between runs).
2. Call `events_since()` once to warm up (JIT/cache, page faults) —
   discarded, not counted.
3. Call it 10 times, `Instant::now()` before/after each, record
   elapsed nanoseconds each time.
4. Score = median of the 10 durations.

## Correctness gate (required before a score counts)

- `cargo build -p terminal-commander-core` succeeds.
- `cargo test -p terminal-commander-core` passes.
- `cargo test -p terminal-commanderd --test agent_superiority_bench`
  passes.
- The benchmark's own correctness assertion passes: the returned
  events, projected to `(seq, severity, kind, count)` in order (this
  excludes `event_id`/`bucket_id`/`timestamp`, which are freshly
  minted per fixture build and never equal across builds), plus
  `next_cursor`/`has_more`/`dropped_count`, must match the baseline
  exactly. This is checked automatically by the harness — a change
  that returns a faster but different result is not a valid win, it's
  a bug.

If any gate fails, the round is an automatic revert and is logged as
such — it is never compared on speed.

## Where the harness lives

`research/asset/bench_events_since.rs` — run via:

```
cargo test -p terminal-commander-core --test bench_events_since -- --nocapture
```

The harness prints the 10 raw samples and the median in ns. That
printed median is the score for the round.
