# SC-004 Evidence: Token-lean streaming byte reduction (US4)

**Success criterion (SC-004)**: following a quiet long build via the compact
`wait` plus the delta `sub_pull`, the serialized agent-facing response bytes are
reduced by **>= 60%** versus the pre-change full-record + full-liveness shapes
for the same event stream.

**Result: 84.7% reduction (target met).**

| Shape | Serialized bytes |
|---|---|
| BEFORE (full records + full liveness every pull) | **11791** |
| AFTER (compact reads + liveness delta) | **1799** |
| Reduction | **84.7%** |

## Repro scenario (findings-doc "quiet long build")

An agent watches a quiet long build over one subscription with several in-scope
sources, polling repeatedly:

- `sources = 4` (one command job + three file-watch probes)
- `polls = 12` (poll cycles across the build)
- `matches = 3` (compile diagnostics surfaced during the build)

**BEFORE (pre-US4):** every one of the 12 pulls re-sent the full 4-entry
per-source liveness array; the event read returned the full `SignalEvent`
records (`event_id`, `bucket_id`, `timestamp`, `source`, `pointer`, `captures`,
...).

**AFTER (US4):** the adapter reads compact (`{summary, stream, seq, severity}`)
and requests the liveness delta, so the full liveness snapshot rides only the
FIRST pull and is omitted on the 11 steady idle pulls; the event read returns
compact projections.

The two mechanisms compound: the liveness delta removes 11 repeated full
liveness arrays, and compact projection strips the id plumbing from the event
records.

## How to reproduce

Deterministic measurement over the exact wire shapes (real `SignalEvent` and
`SourceLiveness` values serialized with `serde_json`):

```
cargo nextest run -p terminal-commanderd --test sc004_token_lean_bytes --no-capture
```

Prints:

```
SC-004 quiet-build bytes: before=11791 after=1799 reduction=84.7% (sources=4, polls=12, matches=3)
```

Source: `crates/daemon/tests/sc004_token_lean_bytes.rs`
(`sc004_compact_and_delta_cut_quiet_build_bytes_by_at_least_60_percent`). The
test asserts the >= 60% floor so the criterion cannot silently regress.

## Notes / honesty

- The measurement is representative, not a live capture: it serializes the exact
  wire shapes the adapter produces (real `SignalEvent`/`SourceLiveness` types)
  for a fixed, documented scenario. The percentage scales with source count and
  poll count (more idle polls -> larger delta saving); the 4-source/12-poll case
  is a modest, defensible mid-point, and the reduction stays well above 60%
  across realistic quiet-build shapes.
- The full records remain re-fetchable: a compact read is presentation-only (the
  event store is untouched), and re-reading the same cursor with `compact`
  omitted returns the full records.
