# Spec: Predicate-Routed Subscriptions + Real-Time-Active Bridge

- Date: 2026-06-02
- Status: design approved (direction); pending agent review (doc-review + adversarial) then user sign-off
- Scope: Terminal Commander Rust workspace (daemon + cli + mcp), plus a documented harness-loop pattern
- Author: special-place-administrator (via Claude)

## Problem

An LLM wants to run several INDEPENDENT watches/tasks at once ("monitor all errors",
"tell me when this status completes or becomes active") and consume their matched
events in near-real-time WITHOUT juggling N opaque `(job_id, bucket_id, cursor)`
triples or polling N channels. Today the per-bucket primitives exist
(`bucket_wait`, cursor reads, `runtime_state`) but there is no multiplexed,
topic-style consumer over many sources, and no documented way to make the LLM
react in real-time.

## Grounding (what already exists — do NOT rebuild)

TC is already an in-process, log-structured pub/sub:
- **Producers** = probes (command/pty/file-watch), each streaming output.
- **Filter/routing** = sifter rules (keyword/regex/dedupe/threshold) activated by
  scope (global/bucket/job/probe). A match produces an `EventDraft`.
- **Queue/partition** = the bucket: a bounded ring of events with a monotonic
  `seq`, a `cursor`, and a `dropped_count` (lag signal). Backed by
  `tokio::sync::Notify` — `BucketManager::bucket_wait` (verified, `core/src/bucket.rs`)
  is event-driven: fast-path returns immediately if events exist, else awaits the
  Notify raced against a timeout; the producer signals Notify on append. No polling.
- **Consume** = `bucket_wait` (Notify-woken, cursor-based, lossless via ring+cursor).
- **Ledger (proto)** = `runtime_state` / `probe_list`.

This is Kafka-style (append-log + offset), NOT RabbitMQ-style (destructive queue +
ack) — and log+offset is the right model for an LLM (replayable, lossless, no ack
bookkeeping). The Rust "broker equivalent" in-process is `tokio::sync::broadcast` +
`Notify`, which the bucket already hand-rolls. **No external broker** (lapin/NATS/
kafka) — those are network brokers, wrong for a single local daemon + single
consumer, and would violate the no-sockets privilege posture.

## Goals

1. **One multiplexed consumer over many sources** — the LLM opens ONE subscription
   and reads matched events across all matching buckets through a single handle,
   with server-side offsets (no N-cursor juggling).
2. **Topic/predicate routing** — subscribe by WHAT you care about (severity, kind,
   tag, source set / all), not by enumerating bucket_ids; matching FUTURE probes
   auto-join (so "monitor all errors" covers tomorrow's command).
3. **Real-time-active** — events reach the consumer with sub-ms internal wake
   (existing Notify), and the LLM can be driven turn-by-turn via a harness loop
   over a stream bridge. No "heartbeat" concept: the data path is push
   (filter→bucket→Notify); an idle read returns empty + per-source liveness.
4. **Bounded + honest** — every read is capped + truncation/lag flagged; no
   fake-success; thin MCP facade (logic in the daemon).

## Non-goals

- No external message broker; no new network surface.
- No persistence of subscriptions across daemon restart (the registry is
  in-memory/ephemeral; buckets/rules remain the durable layer — re-open on restart).
- No true server→model push (impossible over MCP — see Real-Time-Active). We make
  TC the ideal SOURCE for a harness loop, not a pusher into a dormant model.
- No separate queueing/admission layer beyond the existing per-probe policy caps
  (dedup falls out of the subscription hash; concurrency is the existing caps).

## Decision

Add **predicate-routed Subscriptions** over the existing event log, an in-memory
**SubscriptionRegistry**, an instantly-woken multiplexed **pull**, a **CLI stream
bridge** for harness loops, and an **optional MCP notification nudge**. Reframe
"heartbeat" as liveness-on-empty.

## Portability (any harness, any OS) — the load-bearing property

The **universal solution** is the Subscriptions tools (`subscription_open/pull/
list/close`) + the cross-platform daemon. It works on EVERY MCP harness (Claude
Code, Codex, Cursor, Agent SDK, ...) and every OS, because:
- The surface is **standard request/response MCP tools** — no harness-specific API,
  no server push required.
- **Real-time-active is achieved portably by the agent LOOPING `subscription_pull`:**
  the call is instantly Notify-woken, so a loop of pulls delivers events in
  real-time with NO busy-poll (each pull blocks until an event or the bounded
  timeout, then the agent calls again). Every harness can loop tool calls — this is
  the universal active mechanism (a no-socket consumer's equivalent of a live
  stream).
- The daemon is **cross-platform Rust**; the UDS-vs-named-pipe transport fork is
  internal and already handled.

Everything in "Real-Time-Active" BEYOND "agent loops `subscription_pull`" — the CLI
`subscription-stream`, `Monitor`/Stop-hook wakes, MCP `notifications/message` — is an
**OPTIONAL, additive, per-harness ACCELERATOR**. None is required for correctness;
the core behaves identically everywhere without them. The spec phases reflect this:
**Phase 1 (the universal core) is the whole solution; Phases 2-3 are sugar.**

---

## Components

### 1. Subscription model (daemon, `crates/daemon`)

A `Subscription` = `{ sub_id, predicate, offsets: Map<BucketId, seq>, created_at, last_pull_at }`.
- `sub_id` = a stable content hash of the normalized predicate (so identical
  predicates DEDUP to one subscription — the user's "hashing").
- **Predicate grammar** (all fields optional; AND semantics; a match = an event
  whose source bucket is in-scope AND the event satisfies the field filters):
  - `severity_min` (trace..critical)
  - `kind` (event_kind allowlist, e.g. ["error","panic"])
  - `sources`: one of `all` | `{ jobs: [...] }` | `{ buckets: [...] }` | `{ probes: [...] }`
  - `tag` (optional grouping label — see §1a)
  - Reuses the existing `BucketReadRequest` filter fields where possible.
- **Routing index**: an in-memory map the daemon maintains of which live buckets
  match each subscription's predicate, updated on probe **start/stop**
  (hook the existing probe-lifecycle events). `sources: all` and `tag`-based
  subscriptions auto-include new matching buckets; `sources: {buckets:[...]}` is a
  fixed set. The index lets `pull` know which Notifies to select over.

#### 1a. (Optional, flag-gated) `tag` on probe start
To support `tag`-routed subscriptions, command/pty/file-watch start params gain an
optional `tag: Option<String>` stored on the bucket. If we descope tags in v1,
predicates use `severity_min`/`kind`/`sources` only and §1a is deferred. (Decide in
review — `sources: all` + severity/kind already covers "monitor all errors".)

### 2. SubscriptionRegistry (daemon, in-memory, ephemeral)
Holds all open subscriptions for the daemon session. Bounded (a max-subscriptions
cap; opening beyond it is a typed `ResourceExhausted`-class error, reusing the
closed IpcErrorCode set). Cleaned up on `subscription_close` and when its last
fixed source exits (configurable). This IS the in-memory ledger the user asked for.

### 3. Multiplexed pull (the in-protocol realtime read)
`subscription_pull(sub_id, max, timeout_ms)`:
- Resolve the subscription + its routing-index bucket set.
- **Fast path**: if any in-scope bucket has events past its offset, return them
  immediately (bounded by `max`, fair across buckets — see Fairness).
- **Slow path**: `select!` over the in-scope buckets' `Notify` handles raced
  against `timeout_ms`. On any wake, drain matched events across buckets since
  their offsets, advance offsets, return (tagged by source: `{bucket_id, job_id?,
  seq, event}`). On timeout: return `events: []` + `liveness` (per in-scope source:
  running | exited{code} | dropped{count}) + a global `lagged` flag if any bucket
  dropped. **No heartbeat field as a concept** — empty + liveness is the idle return.
- **Bounded**: `max` (default 50, hard cap), per-event byte cap, `truncated` flag,
  `lagged`/`dropped_count` surfaced (no silent loss).
- **Fairness**: round-robin / proportional drain across in-scope buckets so one
  noisy probe cannot starve the others within a single `max`-bounded pull (cite
  this as an explicit test).
- **Offsets are replayable**: a `subscription_pull(..., from_cursor?)` or a
  `subscription_seek(sub_id, bucket_id, seq)` allows re-reading (log semantics, not
  destructive) — v1 may keep it simple (server-advanced offsets only) and defer
  seek; decide in review.

### 4. Subscription lifecycle tools (MCP facade → daemon IPC, 1:1)
- `subscription_open(predicate) -> { sub_id, created_at, matched_sources }`
- `subscription_pull(sub_id, max?, timeout_ms?) -> { events[], liveness[], lagged, truncated, next_state }`
- `subscription_list() -> [{ sub_id, predicate, source_count, created_at, last_pull_at }]` (bounded — see §6)
- `subscription_close(sub_id) -> { closed: bool }`
- (Deferred/optional) `subscription_add`/`remove` for fixed-set edits;
  `subscription_seek` for replay.
All bounded, structured-only, closed IpcErrorCode set. The MCP crate stays free of
spawn/fs/socket — these are thin forwards to daemon IPC.

### 5. CLI stream bridge (the real-time-active channel)
`terminal-commander subscription-stream <sub_id> [--max N]` (cli crate = existing
daemon IPC client): internally loops `subscription_pull` (blocking, instantly
woken) and writes **one newline-delimited JSON object per matched event** to
stdout; flushes per event; exits on `subscription_close`/daemon shutdown. This is
NOT an MCP tool (MCP can't stream a single call) — it's a stdout stream a harness
loop consumes. Each line is the "firing symbol": the harness turns each into a
model wake.

### 6. Bounded ledger surface (folds in the deferred round-2 item)
`subscription_list`, `runtime_state`, `probe_list`, `registry_list_active` share
one bounded shape: a `limit` (default + hard cap), a `next_cursor`/`offset` for
pagination, and a `truncated` flag. (This closes the deferred "list/snapshot tools
are unbounded" finding coherently with the new ledger.)

### 7. Optional MCP notification nudge (best-effort)
On new events for an open subscription, the MCP server MAY emit
`notifications/message` ("sub <id>: N new, max severity X"). Harnesses that surface
notifications get a hint; others ignore it. NEVER authoritative — delivery is
always the pull. Additive, off the critical path.

## Real-Time-Active: harness wake mechanisms

TC cannot push into a dormant model; the wake comes from the harness loop watching
TC's stream. Documented patterns (also captured in CONTRIBUTING/AGENTS):
- **Primary — Claude Code `Monitor`**: `Monitor("terminal-commander subscription-stream <sub_id>")`
  → one model turn per matched event line. Persistent for session-length watches.
- **One-shot — backgrounded `subscription_pull`**: a blocking pull that returns on
  the awaited event → single completion wake ("tell me when X activates/completes").
- **Cadence — `/loop` / `ScheduleWakeup` / `CronCreate`**: interval re-invocation.
- **Optional hack — Stop-hook keep-alive**: a `settings.json` `Stop` hook that, on
  model stop, checks TC for pending subscription events and (if any) blocks the
  stop + injects them → TC effectively drives the model while traffic flows.
  MUST be bounded (max consecutive keep-alives / an escape) to avoid infinite
  loops; only works within a live session.
- **Cross-harness**: Codex/Cursor use their own background/loop over
  `subscription_pull`; an Agent-SDK agent can be a pure event loop on it.

## Error handling
- Closed `IpcErrorCode` set: unknown `sub_id` → a typed not-found; registry full →
  resource-exhausted-class; invalid predicate → caller-fixable (RuleInvalid-style).
- Idle/empty pull is SUCCESS (empty + liveness), never an error; lag is surfaced
  (`lagged`/`dropped_count`), never silent.
- No raw output; events are structured signals only.

## Testing / acceptance
- AC1: `subscription_open` with `{severity_min: high, sources: all}` then start two
  noisy commands → `subscription_pull` returns high-sev events from BOTH, tagged by
  source, bounded, first try.
- AC2: a future-started command matching the predicate auto-joins (routing index)
  and its events appear without re-opening.
- AC3: idle pull returns `events:[]` + per-source liveness (running/exited) within
  `timeout_ms`; a dropped/lagged bucket sets `lagged`/`dropped_count` (no silent
  loss). No `heartbeat` field.
- AC4: fairness — one flooding bucket does not starve a quiet one within a single
  `max`-bounded pull.
- AC5: `subscription-stream` emits NDJSON, one line per event, flushed per event;
  a `Monitor` over it wakes one turn per line (verified by driving it).
- AC6: identical predicates → same `sub_id` (hash dedup); registry cap enforced.
- AC7: MCP crate stays free of spawn/fs/socket (the existing grep guards still
  pass); all new tools bounded + closed IpcErrorCode.
- AC8: `subscription_list`/`runtime_state`/`probe_list`/`registry_list_active`
  bounded with `limit` + `truncated`.
- Dual-OS: daemon logic cross-platform; verify via the linux gate (WSL) + windows
  gate; the CLI stream on both.

## Open decisions for review (call these out to the adversarial pass)
- Tags (§1a) in v1 or deferred? (`sources: all` + severity/kind may suffice.)
- Replay/`subscription_seek` in v1 or deferred (server-advanced offsets only)?
- Subscription lifetime: auto-close when all fixed sources exit, or keep open for
  late-joiners (predicate subs)?
- Fan-in fairness policy (round-robin vs proportional-to-lag).
- Routing-index cost under heavy probe churn; cap on subscriptions + on
  buckets-per-subscription.

## Phasing
- **Phase 1**: SubscriptionRegistry + predicate (severity/kind/sources) + routing
  index + `subscription_open/pull/list/close` + bounded ledger surface (§6) +
  dual-OS tests. (No tags, no seek.)
- **Phase 2**: CLI `subscription-stream` bridge + the documented harness-loop
  patterns (Monitor/loop/Stop-hook) + optional MCP notification.
- **Phase 3 (optional)**: tags, replay/seek, proportional fairness.
