# Spec: Predicate-Routed Subscriptions + Real-Time-Active Bridge

- Date: 2026-06-02
- Status: design approved (direction); REVISED round 1 (18 blockers) then round 2 (7 fix clusters: tokio enroll idiom, off-by-one clamp, fairness cap, shutdown contract, MCP pull timeout, boot_id, runtime_state vecs) — pending focused correctness re-verify + user sign-off. One open governance decision: the IpcErrorCode amendment.
- Scope: Terminal Commander Rust workspace (daemon + core + ipc + cli + mcp), plus a documented harness-loop pattern
- Author: special-place-administrator (via Claude)

## Problem

An LLM wants to run several INDEPENDENT watches/tasks at once ("monitor all errors",
"tell me when this status completes or becomes active") and consume their matched
events in near-real-time WITHOUT juggling N opaque `(job_id, bucket_id, cursor)`
triples or polling N channels. Today the per-bucket primitives exist
(`bucket_wait`, cursor reads, `runtime_state`) but there is no multiplexed,
topic-style consumer over many sources, and no documented way to make the LLM
react in real-time.

## Grounding: what EXISTS vs what must be ADDED

The previous draft over-credited "thin reuse of existing primitives." The
adversarial review (and a read-only code-grounding pass) corrected this. The
DESIGN DIRECTION is sound — TC is already a log-structured pub/sub and Kafka-style
log+offset is the right model — but Phase 1 requires real net-new daemon machinery,
not just thin forwards. Both halves are stated honestly below so the plan cannot
over-claim again.

### EXISTS (verified, file:line) — reuse as-is

- **Bucket = bounded ring + monotonic `seq` + cursor + `dropped_count`**, backed by
  `tokio::sync::Notify` (`crates/core/src/bucket.rs`). `BucketState{head_seq,
  tail_seq, dropped_count}` (bucket.rs:101-111).
- **`bucket_wait` is event-driven**: fast-path `events_since` read, else
  `notify.notified()` raced against a timeout (bucket.rs:494-558). The producer
  signals on append.
- **Wake primitive is `notify_waiters()`** (bucket.rs:459 append, :484 patch) — NOT
  `notify_one()`. This is the single most important correction; see §3.
- **Public bucket accessors**: `events_since` (bucket.rs:565), `state` (:619),
  `summary` (:611), `drop_bucket` (:410), `list_bucket_ids` (:629),
  `has_bucket` (:416), `create_bucket` (:388). `BucketError::NotFound` (:244).
- **Per-event routing fields**: `source.probe_id` / `source.job_id`
  (core/src/source.rs) + `bucket_id`, enabling `sources:{buckets|jobs|probes}`
  filtering; `severity_min`/`kind` already honored by `events_since` via
  `BucketReadRequest`.
- **CLI is a daemon IPC client** (`crates/cli/src/ipc.rs`); `DaemonClient`
  (`crates/ipc/src/client.rs`) with an **overridable** per-call timeout —
  `with_timeout(Duration)` EXISTS (client.rs:48-53); the 5s is a default, not
  hard-coded.
- **Daemon already pipelines multiple requests on ONE connection**:
  `handle_connection` loops read→dispatch→write until EOF/shutdown
  (server.rs:301-335, unix). So request/response streaming over a persistent
  connection is client-side work — see §5 caveat for the push case and Windows.
- **Per-connection concurrency**: `accept_loop` spawns one task per connection
  (server.rs ~193-201), so a blocked pull does not block other connections.
- **Probe EXIT is event-sourced**: the command waiter task appends a synthetic
  lifecycle event to the bucket and flips `JobState` via `JobManager::finish`
  (command.rs:558-648, job.rs:154-184). Exit code lives in `JobExitInfo`.
- **Proto-ledger**: `runtime_state`/`probe_list` (`collect_probes`,
  handlers/runtime.rs:13-73).
- **Closed `IpcErrorCode` set** + exhaustive MCP classification
  (protocol.rs:359-445; tools.rs:1372-1417).
- **Constants**: `MAX_FRAME_BYTES = 256 KiB` (protocol.rs:104),
  `MAX_BUCKET_WAIT_MS = 30_000` / `DEFAULT_BUCKET_WAIT_MS = 5_000`
  (protocol.rs:139-141), `DRAIN_CEILING = 10s` (server.rs:151, unix).

### MUST-ADD (net-new — owned by Phase 1 unless noted)

1. **`BucketManager::bucket_notify(bucket_id: BucketId) -> Result<Arc<Notify>,
   BucketError>`** — the `notify` field is private (bucket.rs:273) with no public
   accessor today. The multiplexed pull needs to arm N notifiers; this is the
   minimal accessor (mirrors the existing private `bucket()` lock pattern, no
   deadlock). The multi-Notify `select!` itself is NEW logic, not a wrapper.
2. **Per-bucket source-metadata side-table** — buckets carry NO source identity
   today (`BucketState` has none; `Router::bucket_create` stores none). Add a
   `RwLock<HashMap<BucketId, BucketSource>>` populated at the 3 existing
   `bucket_create` call sites (command.rs:444, file_watch.rs:274,
   pty_command.rs:280). `BucketSource{kind, job_id?, probe_id?, path?, argv?,
   tag?}`. This is the routing substrate (§1). There is NO probe-lifecycle event
   bus to hook — start is a synchronous state mutation; the side-table + a
   `collect_probes`-style snapshot is the truthful mechanism.
3. **`ProbeListEntry` liveness field + per-kind derivation in `collect_probes`** —
   liveness is NOT available today: command bindings LINGER in the `live` map after
   exit (the waiter never `remove`s them — verified zero matches), so presence ≠
   running; file-watch/pty have no exit-code concept (removal is `stop()`-driven).
   Derive: command → `running|exited{code}|failed{code,signal}|cancelled` from
   `JobState`+`JobExitInfo`; file-watch → `running` while present, else `stopped`
   (no exit code); pty → present + job-ledger exit if wired. `dropped{count}` from
   `BucketState.dropped_count`.
4. **Persistent/streaming `DaemonClient` mode** (Phase 2) — only `call`
   (connect-per-call) exists. The §5 stream bridge needs either a reconnect-per-pull
   loop or a connection-reusing client; server needs no change for
   request/response pipelining, but does for any push (§5). On Windows the named
   pipe is single-pending-instance with a short `ERROR_PIPE_BUSY` retry budget
   (pipe_client.rs:18-26; pipe_server.rs:232-250), so a long-lived persistent
   connection contends with other clients — PREFER reconnect-per-pull on Windows.
5. **Two new `IpcErrorCode` variants — REQUIRES A GOAL-FILE AMENDMENT** (the set is
   closed by governance, protocol.rs:359-360, and the MCP match is exhaustive):
   `UnknownSubscription` (→ `invalid_params`/-32602, matching the
   `UnknownJob/UnknownWatch/UnknownProbe` pattern) and `SubscriptionLimitExceeded`
   (→ `invalid_params`/-32602; caller can free a slot). No existing variant fits
   (`UnknownJob`/`OversizedRequest` are semantic mismatches). Both force an edit to
   the exhaustive `into_mcp_error` match (tools.rs:1389-1416). **This is a decision
   for the user** — flagged in Open Decisions.
6. **`boot_id` on `DaemonState` + `SubscriptionOpenResponse`** — net-new wire state.
   DaemonState holds only a `std::time::Instant` (state.rs:54-114), not a stable
   serializable ID, and no `boot_id` is on any IpcResponse today. Mint a `Uuid` (or
   `OffsetDateTime`) at `bootstrap` and surface it on `subscription_open` so a
   looping agent detects a restart (registry/offsets/buckets all reset together).
7. **MCP `subscription_pull` per-call timeout override** — the MCP daemon client is
   built with the 5s default (tools.rs:3124 / main.rs:190). The pull path MUST issue
   its daemon call with `with_timeout(> pull server cap)` (EXISTS, client.rs:50) so
   an idle ~8s pull returns SUCCESS empty+liveness, NOT a -32603 client timeout (a
   trust-thesis hole). Timeout hierarchy: `pull_server_cap (~8s) < DRAIN_CEILING
   (10s) < MCP pull client timeout (~12s)`.

## Goals

1. **One multiplexed consumer over many sources** — the LLM opens ONE subscription
   and reads matched events across all matching buckets through a single handle,
   with server-side offsets (no N-cursor juggling).
2. **Topic/predicate routing** — subscribe by WHAT you care about (severity, kind,
   source set / all; tags in Phase 3), not by enumerating bucket_ids; matching
   FUTURE probes auto-join (so "monitor all errors" covers tomorrow's command).
3. **Real-time-active, honestly framed** — two distinct latencies: (a) INTERNAL
   wake latency = sub-ms (existing Notify); (b) END-TO-END agent-reaction latency =
   internal wake + IPC round trip + harness turn cost (harness-dependent, NOT
   absolute real-time). The pure-pull core is "real-time relative to harness turn
   cadence"; the CLI stream/Monitor path (Phase 2) is what approaches stream
   latency (one connection, NDJSON per event, no per-event model turn). The
   subscription response models idle as empty `events[]` + per-source liveness
   rather than a boolean heartbeat flag; the underlying `bucket_wait` retains its
   `heartbeat` field unchanged.
4. **Bounded + honest** — every read is capped + truncation/lag flagged; no
   fake-success; thin MCP facade (logic in the daemon).

## Non-goals

- No external message broker; no new network surface.
- No persistence of subscriptions across daemon restart. On restart, registry +
  buckets + offsets reset TOGETHER (consistent, not partial). `subscription_open`
  returns a daemon `boot_id` so a looping agent detects a restart; a pull against an
  unknown/expired `sub_id` returns the typed `UnknownSubscription` error, NEVER
  empty+liveness (so "registry lost" is never mistaken for "no events").
- No true server→model push (impossible over MCP — see Real-Time-Active). We make
  TC the ideal SOURCE for a harness loop, not a pusher into a dormant model.
- No separate queueing/admission layer beyond the existing per-probe policy caps
  (dedup falls out of the subscription hash; concurrency is the existing caps).

## Decision

Add **predicate-routed Subscriptions** over the existing event log, an in-memory
**SubscriptionRegistry**, an instantly-woken multiplexed **pull** with an explicit
lossless discipline, a **CLI stream bridge** for harness loops, and an **optional
MCP notification nudge**. Reframe "heartbeat" as liveness-on-empty.

## Portability (any harness, any OS) — the load-bearing property

The **universal solution** is the Subscriptions tools (`subscription_open/pull/
list/close`) + the cross-platform daemon. It works on EVERY MCP harness (Claude
Code, Codex, Cursor, Agent SDK, ...) and every OS, because:
- The surface is **standard request/response MCP tools** — no harness-specific API,
  no server push required.
- **Real-time-active is achieved portably by the agent LOOPING `subscription_pull`:**
  the call is instantly Notify-woken, so a loop of pulls delivers events with NO
  busy-poll (each pull blocks until an event or the bounded timeout, then the agent
  calls again). Every harness can loop tool calls. (Honest caveat per Goal 3: the
  per-pull cadence still includes the harness turn cost; "real-time" is relative to
  that cadence.)
- The daemon is **cross-platform Rust**; the UDS-vs-named-pipe transport fork is
  internal. NOTE: the Windows `pipe_server` is a SEPARATE impl from the unix
  `server.rs` quoted above — the request loop, persistent-connection, and
  shutdown-race behavior MUST be verified on the pipe path too (dual-OS AC).

Everything in "Real-Time-Active" BEYOND "agent loops `subscription_pull`" — the CLI
`subscription-stream`, `Monitor`/Stop-hook wakes, MCP `notifications/message` — is an
**OPTIONAL, additive, per-harness ACCELERATOR**. None is required for correctness;
the core behaves identically everywhere without them. **Phase 1 (the universal core)
is the whole solution; Phases 2-3 are sugar.**

---

## Components

### 1. Subscription model (daemon, `crates/daemon`)

A `Subscription` = `{ sub_id, predicate, offsets: Map<BucketId, seq>, created_at,
last_pull_at, rr_start: usize }` (`rr_start` = round-robin rotation cursor, §3).
- `sub_id` = a stable content hash of the normalized predicate (so identical
  predicates DEDUP to one subscription — the user's "hashing").
- **Predicate grammar** (all fields optional; AND semantics; a match = an event
  whose source bucket is in-scope AND the event satisfies the field filters):
  - `severity_min` (trace..critical) — **per-EVENT** filter (events_since honors it).
  - `kind` (event_kind allowlist, e.g. ["error","panic"]) — **per-EVENT** filter.
  - `sources`: one of `all` | `{ jobs: [...] }` | `{ buckets: [...] }` |
    `{ probes: [...] }` — **per-BUCKET** routing (resolved against the source
    side-table, MUST-ADD #2).
  - `tag` — **per-BUCKET** routing; DEFERRED to Phase 3 (rides the same side-table).
- **Routing index (truthful mechanism)**: there is no lifecycle event bus. The
  daemon maintains the bucket source side-table (MUST-ADD #2), written at
  `bucket_create`. On each `subscription_pull`, the in-scope bucket set is a LAZY
  REBUILD: `list_bucket_ids()` intersected with the predicate evaluated over the
  side-table. `sources: all` (and Phase-3 `tag`) auto-include new matching buckets
  because the rebuild re-reads the table; `sources:{buckets:[...]}` is a fixed set.
  A cheap dirty-flag bumped at bucket create/drop MAY short-circuit the rebuild
  (optimization, not correctness).
- **Join-offset semantics (lossless for auto-join)**: when a new bucket enters a
  subscription's scope, its offset initializes to the bucket's **head** (`seq=0` /
  creation), NOT its current tail. The side-table entry is written at `bucket_create`
  AND bumps a dirty-flag that forces the routing rebuild on the next pull, so a
  `sources: all` subscription picks up the new probe. Losslessness holds AS LONG AS
  the rebuild observes the bucket before its ring FIFO-evicts the first events; a
  probe that floods + evicts a whole ring BETWEEN two pulls falls back to the
  standard lagged/dropped path (§ Leave/eviction) — never silent. (A late
  `subscription_open` over a pre-existing bucket starts at that bucket's current tail
  at open time; only AUTO-JOINED future buckets start at head, and they have no
  backlog yet, so no full-ring replay.)
- **Leave / eviction reconciliation (per pull)**: if `stored_offset <
  bucket.head_seq` (events FIFO-evicted under us), clamp offset to
  `head_seq.saturating_sub(1)` — NOT `head_seq` — because `events_since` reads
  strictly `e.seq > cursor` (bucket.rs:582) and `head_seq` is the oldest SURVIVING
  event's own seq (bucket.rs:339-340); clamping to `head_seq` would skip that
  survivor. Surface `lagged` + dropped delta = `head_seq - stored_offset` (correct
  log-semantics loss, never silent). If a fixed-source bucket is `drop_bucket`'d,
  emit a final `exited/gone` liveness entry and remove it from the offsets map.
- **Routing scan is bounded**: a hard cap of max buckets-per-subscription
  (default 200) with a `truncated` flag; the `list_bucket_ids()` ∩ side-table scan
  per pull is O(live buckets), acceptable under the cap.

### 2. SubscriptionRegistry (daemon, in-memory, ephemeral)
Holds all open subscriptions for the daemon session. Bounded (a max-subscriptions
cap; opening beyond it is `SubscriptionLimitExceeded`, MUST-ADD #5). Cleaned up on
`subscription_close` and when its last fixed source exits (configurable; predicate
subs stay open for late-joiners). This IS the in-memory ledger the user asked for.
Reset wholesale on daemon restart (see Non-goals).

### 3. Multiplexed pull (the in-protocol realtime read) — LOSSLESS DISCIPLINE

`subscription_pull(sub_id, max?, timeout_ms?)`. Correctness rests on an explicit
**enroll-before-recheck** ordering, because the bucket signals with
`notify_waiters()` (no stored permit; only ENROLLED waiters wake — bucket.rs:459,
484). The cursor/seq is the SOURCE OF TRUTH (lossless); Notify is a latency hint
only. THE INVARIANT: within a pull pass, no `events_since` read for a bucket may
precede that bucket's waiter ENROLLMENT. In tokio (1.52) a `Notified` future
enrolls into the waiter list only when FIRST POLLED or via `Notified::enable()` —
NOT when created or pinned; "create + pin then read" still drops a wake. Algorithm:

1. Resolve the subscription. If `sub_id` is unknown/expired → return
   `UnknownSubscription` (typed error, never empty).
2. Snapshot the in-scope bucket set (routing rebuild, §1) and clone each bucket's
   `Arc<Notify>` via `bucket_notify` (MUST-ADD #1). `N` = in-scope bucket count;
   if `N == 0`, skip straight to step 7 (timeout/liveness — no `ceil(max/0)`).
3. **Enroll**: for each in-scope bucket, create `notify.notified()`, pin it, and
   call `Notified::enable()` (or poll-to-`Pending`) to ENROLL the waiter NOW —
   before any read. Each loop iteration constructs FRESH `notified()` futures (they
   are single-use); a prior iteration's future is never reused.
4. **Fast-path recheck (after enroll)**: scan ALL in-scope offsets via
   `events_since`. If any bucket has events past its offset, drain (fair, step 6)
   and return immediately — the enrolled futures drop harmlessly.
5. **Slow path**: `select!` over the enrolled `FuturesUnordered` raced against
   `timeout_ms`. On ANY wake, **re-scan ALL in-scope offsets** (not just the woken
   bucket) and drain. A SPURIOUS wake (Notify fired but no in-scope event passes the
   predicate) RE-ENTERS the loop — and re-entry MUST re-enroll fresh waiters (step 3)
   BEFORE the re-scan, so an append landing in the re-arm window is not lost. A
   bucket born during the `select!` is picked up on the next iteration's rebuild
   (latency-bounded by the next wake/timeout, never lost — its events sit in the
   ring at offset=head).
6. **Fairness (v1 = deterministic round-robin, capped at `max`)**: per-bucket share
   = `max(1, max / N)`; drain buckets starting from `rr_start`, STOPPING the moment
   the running total reaches `max` (so `N > max` still returns ≤ `max` — buckets
   past the cut-off get priority next pull via the rotated `rr_start`). This both
   honors the hard cap AND prevents a flooding bucket from starving a quiet bucket's
   single high-sev event within one pull. (Proportional-to-lag deferred to Phase 3.)
7. **On timeout (or `N == 0`)**: return `events: []` + `liveness[]` (per in-scope
   source: `running | exited{code} | failed{code,signal} | stopped | dropped{count}`,
   per-kind per MUST-ADD #3) + a global `lagged` flag if any bucket dropped. No
   `heartbeat` field.
8. **Bounded**: `max` (default 50, hard cap), per-event byte cap, `truncated` flag;
   the `liveness[]` array is ALSO bounded (shares the §6 limit+truncated shape); the
   COMBINED response (events + liveness) is asserted under `MAX_FRAME_BYTES`
   (256 KiB), independent of `N`.

**Timeout reconciliation (Phase 1: timeout-below-ceiling, no shutdown-race)**:
`timeout_ms` is hard-capped strictly below `DRAIN_CEILING` (cap ~8s < 10s) so a
blocked pull RETURNS its normal empty+liveness at its own timeout before the unix
drain would abort it; the loop-of-pulls reaches the 30s `MAX_BUCKET_WAIT_MS` watch
duration by issuing successive ≤8s pulls. The MCP pull path raises its client
timeout via `with_timeout(> pull cap)` (MUST-ADD #7) so an idle pull is SUCCESS, not
-32603. Phase 1 does NOT race the connection's shutdown receiver server-side:
`dispatch(state, boot, req_env, peer)` (server.rs:406; pipe_server dispatch_envelope)
is NOT given the shutdown `watch::Receiver`, and threading it touches the shared
signature used by every method (large blast radius). On graceful shutdown the
in-flight pull simply returns at its short timeout; on a hard kill the looping agent
re-detects via a changed `boot_id`. (A future Phase-2 enhancement MAY thread the
shutdown receiver to emit a SUCCESS `next_state: draining` field — never a
`ShuttingDown`/-32603 error. Out of Phase 1.) **Windows**: `pipe_server`'s
`handle_pipe_connection` checks shutdown only at loop-top (no `select!` race, no
`JoinSet`/`DRAIN_CEILING` — pipe_server.rs:263-290), so an in-flight pull returns at
its own ≤8s timeout and the loop then breaks; a hard kill mid-pull is best-effort
abort + `boot_id` re-detect. This asymmetry is documented, not silently assumed.

Offsets are server-advanced (log semantics, replayable in principle); a
`subscription_seek(sub_id, bucket_id, seq)` for explicit re-read is DEFERRED to
Phase 3.

### 4. Subscription lifecycle tools (MCP facade → daemon IPC, 1:1)
- `subscription_open(predicate) -> { sub_id, boot_id, created_at, matched_sources }`
- `subscription_pull(sub_id, max?, timeout_ms?) -> { events[], liveness[], lagged, truncated }` (Phase 1 has no `next_state`; a SUCCESS `next_state: draining` is a possible Phase-2 add — see §3 Timeout reconciliation)
- `subscription_list() -> [{ sub_id, predicate, source_count, created_at, last_pull_at }]` (bounded — §6)
- `subscription_close(sub_id) -> { closed: bool }`
- (Deferred/optional, Phase 3) `subscription_add`/`remove` for fixed-set edits;
  `subscription_seek` for replay.
All bounded, structured-only, closed IpcErrorCode set (with MUST-ADD #5 amendment).
The MCP crate stays free of spawn/fs/socket — thin forwards to daemon IPC.

### 5. CLI stream bridge (Phase 2 — NET-NEW client machinery)
`terminal-commander subscription-stream <sub_id> [--max N]`. This is NOT thin reuse
of the one-shot client. Two honest options (decide in the plan):
- (a) **reconnect-per-pull loop** (simplest, honest): a loop of one-shot
  `subscription_pull` calls, each a fresh connect; label the per-pull connect cost
  (on Windows, up to the pipe-busy retry budget).
- (b) **persistent streaming client**: hold ONE connection and pipeline successive
  pull requests on it (the daemon loop already supports multi-request connections,
  server.rs:301-335 unix — VERIFY the Windows pipe_server has the same loop), using
  `with_timeout(> pull cap)`.
Either way it writes **one newline-delimited JSON object per matched event** to
stdout, flushes per event, and exits NON-ZERO on `UnknownSubscription` (so a harness
`Monitor`/loop terminates rather than silently idling) and on daemon shutdown. This
is the channel that actually approaches stream latency (no per-event model turn).
NOTE: true server-initiated PUSH is NOT supported (the server only writes in
response to a read); the bridge is still client-driven pulls, just pipelined.

### 6. Bounded ledger surface (folds in the deferred round-2 item)
`subscription_list`, `probe_list`, `registry_list_active` (single-vec responses)
share one bounded shape: a `limit` (default + hard cap), a `next_cursor`/`offset`
for pagination, and a `truncated` flag. `runtime_state` is the exception — it
carries THREE independent vecs in one response (`probes`, `buckets`, `active_rules`
— protocol.rs:1292-1294), so each vec is bounded SEPARATELY with its own
`limit`+`truncated` (a single cursor cannot page three lists). The pull `liveness[]`
array reuses the single-vec bounding. (This closes the deferred "list/snapshot tools
are unbounded" finding coherently with the new ledger.)

### 7. Optional MCP notification nudge (Phase 2, best-effort)
On new events for an open subscription, the MCP server MAY emit
`notifications/message` ("sub <id>: N new, max severity X"). PREREQUISITE (not free
today): capture the rmcp `Peer<RoleServer>` from the served instance and advertise
the capability in `get_info` (currently only `enable_tools()`); the handler retains
no peer handle for out-of-band sends today. Harnesses that surface notifications get
a hint; others ignore it. NEVER authoritative — delivery is always the pull.
Additive, off the critical path.

## Real-Time-Active: harness wake mechanisms

TC cannot push into a dormant model; the wake comes from the harness loop watching
TC's stream. Documented patterns (also captured in CONTRIBUTING/AGENTS):
- **Primary — Claude Code `Monitor`**: `Monitor("terminal-commander subscription-stream <sub_id>")`
  → one model turn per matched event line. Persistent for session-length watches.
- **One-shot — backgrounded `subscription_pull`**: a blocking pull that returns on
  the awaited event → single completion wake ("tell me when X activates/completes").
- **Cadence — `/loop` / `ScheduleWakeup` / `CronCreate`**: interval re-invocation.
- **Optional hack — Stop-hook keep-alive** (Phase 2, **default OFF**): a
  `settings.json` `Stop` hook that, on model stop, checks TC for pending
  subscription events and (if any) blocks the stop + injects them. This is the ONLY
  mechanism here that can WEDGE a session, so it is bounded HARD: **max 3
  consecutive keep-alives** AND a wall-clock escape; on exhaustion it force-allows
  stop and injects a loud message ("keep-alive budget exhausted, M events pending,
  resume via subscription_pull"). ANTI-GOAL: it is for low-rate "wake me when X
  completes" watches, NOT high-rate "all errors" streams (use Monitor over
  subscription-stream for those). Only works in a live interactive session;
  silently no-ops in headless runs.
- **Cross-harness**: Codex/Cursor use their own background/loop over
  `subscription_pull`; an Agent-SDK agent can be a pure event loop on it.

## Error handling
- Closed `IpcErrorCode` set + the MUST-ADD #5 amendment: unknown `sub_id` →
  `UnknownSubscription` (caller-fixable, -32602); registry full →
  `SubscriptionLimitExceeded` (-32602); invalid predicate → `RuleInvalid`
  (existing). Both new variants edit the exhaustive `into_mcp_error` match.
- Idle/empty pull is SUCCESS (empty + liveness), never an error; lag is surfaced
  (`lagged`/`dropped_count`), never silent. Unknown/expired `sub_id` is the ONE
  case that returns an error instead of empty (so registry-loss ≠ no-events).
- No raw output; events are structured signals only.

## Testing / acceptance
- AC1: `subscription_open` with `{severity_min: high, sources: all}` then start two
  noisy commands → `subscription_pull` returns high-sev events from BOTH, tagged by
  source, bounded, first try.
- AC2 (auto-join + join-offset): a future-started command matching the predicate
  auto-joins; its events are delivered to the pre-existing `sources: all`
  subscription with NO LOST events (completeness), within one wake/timeout cycle
  (latency), and with NO full-ring replay.
- AC3 (lossless enroll-before-recheck): with a bucket enrolled via
  `Notified::enable()` BEFORE the fast-path read, an append landing in the gap
  between the read and the await is delivered on the SAME pull (not lost to timeout)
  — a test must drive this exact ordering. A spurious Notify wake with no in-scope
  match re-enrolls fresh waiters, re-scans, and does NOT return a premature empty;
  an append in the re-arm window of a spurious wake is delivered on the same pull.
- AC4 (fairness + hard cap): one flooding bucket does not starve a quiet one — the
  quiet bucket's event appears in the SAME pull as the flood, flood capped to its
  share `max(1, max/N)`. With `N > max` (e.g. 100 buckets, max=50) a pull returns
  ≤ `max` events and the response stays under `MAX_FRAME_BYTES`; `N == 0` returns
  empty+liveness with no division.
- AC5 (liveness, no heartbeat): idle pull returns `events:[]` + per-source liveness
  within `timeout_ms`; command exited reports `exited{code}`/`failed{code,signal}`
  derived from `JobState` (NOT presence in the live map); file-watch reports
  `running|stopped` (no exit code); a dropped/lagged bucket sets
  `lagged`/`dropped_count`. No `heartbeat` field in the subscription response.
- AC6 (shutdown, Phase 1 timeout-below-ceiling): `timeout_ms` is capped below
  `DRAIN_CEILING`, so a pull in flight during graceful shutdown returns its normal
  `events:[]`+liveness at its own timeout (no -32603, no torn connection on the
  graceful path) — verified on unix; on Windows the in-flight pull returns at its
  ≤8s timeout then the loop breaks, and a hard kill is best-effort-abort with
  `boot_id` re-detect (documented asymmetry). No server-side shutdown-race in Phase 1.
- AC7 (restart honesty): pull against an unknown/expired `sub_id` returns
  `UnknownSubscription`, never empty+liveness; `subscription_open` returns a
  `boot_id` that changes across restart.
- AC8: identical predicates → same `sub_id` (hash dedup); registry cap enforced via
  `SubscriptionLimitExceeded`.
- AC9 (stream): `subscription-stream` emits NDJSON, one line per event, flushed per
  event; exits non-zero on `UnknownSubscription`; a `Monitor` over it wakes one turn
  per line (verified by driving it).
- AC10 (privilege + bounds): MCP crate stays free of spawn/fs/socket (the existing
  grep guards still pass); all new tools bounded; the two new IpcErrorCode variants
  are classified in the exhaustive `into_mcp_error` match.
- AC11: `subscription_list`/`probe_list`/`registry_list_active` bounded with `limit`
  + `truncated`; `runtime_state`'s three vecs each bounded independently; the pull
  `liveness[]` array bounded; combined pull response asserted under `MAX_FRAME_BYTES`.
- AC12 (eviction off-by-one): after FIFO eviction past a subscription's offset, the
  next pull clamps to `head_seq.saturating_sub(1)` and delivers the event AT the
  post-eviction head EXACTLY ONCE, with `lagged` + dropped delta set (no skipped
  survivor, no silent loss).
- AC13 (MCP trust path): an idle `subscription_pull` over the MCP facade returns
  SUCCESS `events:[]`+liveness within `timeout_ms`, NEVER a -32603 (the MCP client
  uses `with_timeout(> pull cap)`).
- Dual-OS: daemon logic cross-platform; verify via the linux gate (WSL) + windows
  gate; the CLI stream on both. The Windows `pipe_server` shutdown behavior is
  best-effort-abort (documented in §3), NOT drain-parity with unix — AC6 asserts the
  timeout-below-ceiling behavior, not a graceful-drain signal.

## Open decisions for review / user sign-off
- **IpcErrorCode amendment (REQUIRES USER APPROVAL)**: add `UnknownSubscription` +
  `SubscriptionLimitExceeded` to the closed set + exhaustive match? (Recommended —
  no existing variant fits; alternative is a strained reuse of `OversizedRequest`
  for full + a generic not-found, which the codebase's per-entity pattern argues
  against.)
- Tags (§1, Phase 3) — confirm deferral (`sources: all` + severity/kind covers
  "monitor all errors").
- Stream bridge: reconnect-per-pull (a) vs persistent client (b) for Phase 2 —
  Windows leans (a) (single-instance pipe + `ERROR_PIPE_BUSY`).
- Subscription lifetime: predicate subs stay open for late-joiners; fixed-source
  subs auto-close when all sources exit — confirm.
- Caps (proposed, confirm): max subscriptions; max buckets-per-subscription = 200
  (+`truncated`); pull `timeout_ms` hard cap ~8s (< `DRAIN_CEILING` 10s); MCP pull
  client timeout ~12s; `max` default 50.

## Phasing
- **Phase 1 (universal core)**: `bucket_notify` accessor (MUST-ADD #1); bucket
  source side-table + 3 call-site writes (MUST-ADD #2); `ProbeListEntry` liveness +
  per-kind derivation (MUST-ADD #3); SubscriptionRegistry; predicate
  (severity/kind/sources); multiplexed pull with enroll-before-recheck (`enable()`)
  + capped round-robin fairness + timeout-below-ceiling (no server-side
  shutdown-race); the 2 IpcErrorCode variants (MUST-ADD #5, pending approval);
  `boot_id` (MUST-ADD #6); MCP pull timeout override (MUST-ADD #7);
  `subscription_open/pull/list/close`; bounded ledger surface (§6); dual-OS tests
  (Windows shutdown = best-effort-abort, documented). (No tags, no seek, no stream
  bridge.)
- **Phase 2 (accelerators)**: CLI `subscription-stream` bridge (NET-NEW client mode,
  MUST-ADD #4) + the documented harness-loop patterns (Monitor/loop/Stop-hook,
  default OFF) + optional MCP notification (Peer<RoleServer> prereq).
- **Phase 3 (optional)**: tags, replay/`subscription_seek`, proportional fairness.
