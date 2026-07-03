# US9 Decision Record: SKIP (with rationale)

**Date**: 2026-07-03
**Story**: US9 "Shrink the connect gap at the source" (P3, explicitly
optional per spec.md and research.md D11 — skipping with a written
rationale is a compliant outcome).
**Decision**: **SKIP.** Do not implement the N>1 pending pipe-instance
pool. Task T046 = decided (skip); T047 and T048 = void.

This record is the compliant written rationale the spec requires. Based
on a read-only investigation of the accept loop, the client retry, and
this campaign's own load evidence.

## Why skip

1. **The symptom is already fully masked, client-side.** The 0.1.72 retry
   in `crates/ipc/src/pipe_client.rs` `round_trip` (lines 111-153) retries
   exactly the two error codes the single-pending-instance gap produces —
   `ERROR_PIPE_BUSY` (231) and `ERROR_FILE_NOT_FOUND` (2) — for
   `PIPE_BUSY_RETRIES=50 x PIPE_BUSY_DELAY_MS=20ms = 1000ms`, well inside
   the outer 5 s request timeout. The retry has carried every real connect
   across two waves of heavy concurrent builds. US9 would remove
   server-side *occurrences* of a gap the client never *observes* as a
   failure — pure defense-in-depth on an already-mitigated path.

2. **P3, optional, and it must not destabilize the accept path everything
   depends on.** The Windows accept loop
   (`crates/daemon/src/ipc/pipe_server.rs` accept_loop:170-306) is
   load-bearing for every US1-US8 live-daemon test.

3. **Decisive: the one real load failure this campaign saw was the wrong
   symptom.** The Wave-1 load failures were `EndpointBindFailed` — a
   server-side bind/startup failure, NOT the connect-gap symptom US9
   addresses. The client retry does not engage for bind failures, so US9
   would not have prevented them; worse, an N-instance initial fill adds
   N-1 extra `create()` calls into exactly the fragile startup window that
   produced `EndpointBindFailed` under thrash. US9 has no surviving failure
   to fix and plausibly widens the observed-fragile surface.

## Risk assessment (why it is not free)

Three of four risk vectors sit on the fragile create/shutdown surface:

- **First-instance flag**: `FILE_FLAG_FIRST_PIPE_INSTANCE` must be set on
  exactly one create for the daemon's lifetime. A pool's near-simultaneous
  initial fill of N instances must set it on exactly one; the other N-1
  must pass `false` or fail `ACCESS_DENIED`/`ALREADY_EXISTS` — spurious
  create failures injected into the bind window.
- **Replacement timing + failure accounting**: the `transient_create_failures`
  counter and its `log_pipe_create_failure()`-to-break fatal threshold are
  owned by one loop today; N racing refill sites need a coherent shared
  counter or one starved refill tears down the whole accept loop — a strict
  regression from single-instance, where the same starvation merely retries.
- **Shutdown**: today one idle `connect()` future is cancelled by
  `shutdown.changed()`; with N, shutdown must cancel N idle accept futures
  and guarantee each pending `NamedPipeServer` is dropped (acceptance
  scenario 3: no orphaned instances). `drain_pipe_connections` drains
  handlers only, not pending accepts — a leaked instance both violates
  scenario 3 and can collide with the next boot's `first_pipe_instance(true)`.
- **Per-connection identity** (the only safe vector): `peer_identity_for`
  runs per accepted connection in `handle_pipe_connection`
  (pipe_server.rs:315), independent of pending-instance count — scenario 2
  holds structurally.

## Client-retry sufficiency

The retry is necessary AND sufficient for correctness. For a burst of
concurrency strictly greater than any pool size N the gap still exists and
the client retry is STILL the required backstop — which is why D11 keeps
the retry regardless. That makes the retry load-bearing and the pool a
non-load-bearing optimization of retry *frequency*, not a correctness fix.

## Recorded implementation shape (if ever revisited)

For the record, the minimal correct shape (N=4 per D11) mirrors the UDS
unbounded-accept loop: a `JoinSet` of N accept futures each owning one
pending `NamedPipeServer`; `first_pipe_instance(true)` exactly once for the
daemon lifetime; immediate replacement of each accepted instance to hold
pending=N; a coherent shared transient-failure counter so one starved
refill backs off rather than tearing down the loop; shutdown cancels all N
idle accept futures (dropping each pending instance) then drains handlers
under `PIPE_DRAIN_CEILING`; per-connection code untouched; keep the
client retry as the >N-concurrency backstop. Tests in
`crates/daemon/tests/pipe_*.rs` with `unique_pipe_name(tag)`: a
connect-storm test asserting near-zero client retries for N concurrent
first-connects, and a shutdown test asserting zero surviving instances.

**This shape is documented so a future decision starts from analysis, not
from scratch — but the decision today is SKIP.** If the connect gap ever
becomes a client-observed failure (not just a masked occurrence), or the
`EndpointBindFailed` startup fragility is separately root-caused and fixed,
revisit with this record as the starting point.
