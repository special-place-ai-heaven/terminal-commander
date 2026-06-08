# Code review: Phase 3 TC-1b + TC-6 (`run_and_watch` wait-loop rewrite)

Reviewed against uncommitted working tree vs HEAD `cf43d5b`. Embedded code in
`CURSOR-REVIEW-PROMPT.md` matches the repo; no discrepancies found.

## Verdict

**APPROVE WITH NITS** — The wait-loop rewrite correctly fixes TC-1b (job handle
preservation on post-start transport errors) and TC-6 (wall-clock `wait_ms`
budget with slice capping), with a single shared result builder keeping normal
and degraded payloads aligned; no correctness blockers found.

## Blockers

(none)

## Non-blocking findings

1. **`tools.rs:2419-2421` — stale `MAX_WAIT_SLICE_MS` doc comment.** Still says
   "The loop runs `ceil(wait_ms / slice)` iterations"; the loop is now a
   deadline-driven `loop {}`. Misleading for future readers; update the comment
   to describe per-iteration slice capping against `remaining_ms`.

2. **`tools.rs:769-774` — final drain swallows transport errors.** The main
   `BucketWait` arm degrades on `Err(_)`, but the deadline-exit final drain uses
   `if let Ok(...)`, ignoring transport failure. Low risk (cursor and state were
   updated in the same iteration’s successful main wait), but it is a small
   asymmetry with TC-1b’s “preserve honesty on IPC failure” spirit. Acceptable
   as best-effort drain; document or unify only if you want strict symmetry.

3. **`tools.rs:676,735` — `Ok(other)` still returns bare `Err`.** Only transport
   `Err(_)` degrades; a daemon protocol mismatch mid-wait still discards
   `job_id`. Matches stated design intent (degrade transport, not protocol
   faults); worth noting as a known scope boundary, not a regression.

4. **Tests — no `wait_ms=0` coverage.** The do-while / at-least-one-poll
   behavior is reasoned in comments but not exercised by unit or live tests.
   Low risk given the straight-line control flow.

5. **Tests — no live fault-injection for degraded wiring.** See Test adequacy
   section; acceptable gap, not a merge blocker.

6. **Tests — TC-6 live assertions only run under `#![cfg(unix)]`.** Consistent
   with the rest of `mcp_live_command_e2e.rs`; Windows relies on unit tests +
   clippy. Pre-existing pattern.

7. **Unit tests omit `include_receipt` / non-empty signals paths.** Builder
   shape for terminal+exited is covered; receipt gating (`!degraded &&
   signals.is_empty()`) is untested but trivial.

## Correctness analysis

### Wait loop (`run_and_watch`, `tools.rs:667-777`)

**Invariant: START-arm errors stay `Err`.** `CommandStartCombed` failure at
`642` returns `into_mcp_error_for(false, &e)` before any `job_id` exists. PASS.

**Invariant: POST-`job_id` transport errors degrade.** Both `CommandStatus`
(`680-691`) and main `BucketWait` (`737-748`) match `Err(_)` and return
`run_and_watch_result(..., degraded: true, recover_hint: Some(...))` without
discarding `job_id`, `bucket_id`, or `cursor`. PASS.

**Invariant: do-while / `wait_ms=0` polls at least once.** The `loop {}` has no
guard before the first `CommandStatus`; deadline is `now + 0` (already elapsed)
but the first iteration always runs a status poll, then `remaining_ms = 0`,
`slice_ms = 0` (non-blocking `BucketWait`), then the bottom `>= deadline` check
fires the final non-blocking drain and breaks. Bounded (≤2 non-blocking
`BucketWait`s when non-terminal). Redundant second drain is benign extra RPC,
not harmful. PASS.

**Invariant: terminal short-circuit.** `terminal` forces `slice_ms = 0`; break
on `terminal || signals.len() >= max_signals` before deadline drain when
terminal. Fast commands do not sleep. PASS.

**Invariant: wall-clock cap (TC-6).** `slice_ms = MAX_WAIT_SLICE_MS.min(remaining_ms)`
where `remaining_ms` is derived from `deadline.saturating_duration_since(now)`.
A blocking slice cannot exceed time left at measurement; after it returns, the
deadline check stops the loop. Maximum overrun vs `wait_ms`: one in-flight slice
(up to 1000ms) started while `remaining_ms > 0`, plus per-iteration
`CommandStatus` RTTs (not subtracted from the budget — see nit below), plus the
final drain RTT. This fixes the old `ceil(wait_ms/1000) * 1000` bucket-wait
overrun (the primary TC-6 defect). PASS for the advertised bucket-wait bound;
CommandStatus latency stacks per iteration outside the deadline (pre-existing
structural pattern, negligible at local UDS speeds, validated by live test).

**Invariant: final non-blocking drain on budget exhaustion.** Only on non-terminal,
non-max-signals exit via `now >= deadline`; one `timeout_ms: Some(0)` drain
updates cursor/signals best-effort before composing a non-degraded
`wait_exhausted` result. PASS.

**Deferred init of `receipt` (`663`, used at `792`).** Paths reaching the final
`run_and_watch_result(..., receipt, false, None)` must have exited the loop via
`break` at `752-753` or `775`, both of which require completing a successful
`CommandStatus` in that iteration (`696`: `receipt = ...`). Degraded arms and
`unexpected_variant`/`Err` returns happen before `792`. Rust definite-assignment
rejects any hole; logic confirms initialization. PASS.

**Signal cap.** `collect_rule_signals` gates on `signals.len() < max_signals`.
`BucketWait` `limit` is `max(1, max_signals - len)` so the daemon always gets a
positive limit; excess rule events from the daemon are dropped client-side.
Cursor advances every successful wait, so no spin without progress. Loop also
breaks on `signals.len() >= max_signals`. Cannot exceed `max_signals`. PASS.

### Result builder (`run_and_watch_result`, `tools.rs:1885-1924`)

**Degraded honesty.** `last_observed_state: None` → `state: "unknown"` via
`map_or_else`; never emits `"running"` without observation. Degraded forces
`(complete=false, wait_exhausted=true)`. PASS.

**No fake success on degraded.** Real `job_id`/`bucket_id`/`cursor` passed
through; `receipt` suppressed via `include_receipt = !degraded &&
signals.is_empty()`. PASS.

**Superset / single builder.** Both paths emit `cursor`, `degraded`, `recover_hint`
(normal: `false` / `null`). Normal completion derived from
`run_and_watch_completion`; degraded overrides to incomplete + exhausted. PASS.

**Normal `wait_exhausted` vs degraded.** Clean budget exit uses `degraded: false`
with `wait_exhausted` from observed non-terminal state; degraded uses
`degraded: true` regardless of last state. Distinction is correct. PASS.

### Idempotent retry interaction

`CommandStatus` and `BucketWait` are `is_idempotent() == true`
(`protocol.rs:366-368`). `McpDaemonClient::call` re-sends once only after
transport failure and returns `Err` only if both attempts fail (`daemon_client.rs:211-215`).
Degraded arms trigger on that final `Err`, not on the first failure. Reads have
no double-effect risk. PASS.

## Test adequacy

**What is covered**

| Layer | Coverage |
|-------|----------|
| Unit | Degraded + no status → `"unknown"`, job ids, cursor, recover_hint, no receipt |
| Unit | Degraded + last observed `Running` |
| Unit | Normal terminal superset keys (`cursor`, `degraded`, `recover_hint`) |
| Live (unix) | Wall-clock cap: `sleep 5`, `wait_ms=1500`, elapsed ∈ [1400ms, 1900ms), `wait_exhausted`, not degraded |
| Live (unix) | Fast `true` command: complete, not degraded, superset keys |
| Existing | `retry_gate.rs` idempotent re-send; `run_and_watch_completion` unit tests |

**What is NOT covered**

- End-to-end degraded wiring (mid-wait transport failure after `job_id` minted).
  Would need a stateful fake daemon or fault injection hook.
- `wait_ms=0` do-while path.
- Normal `wait_exhausted` builder path (non-degraded, still running) at unit level.
- `include_receipt` when `signals.is_empty()` on normal path.
- Windows live wall-clock (file is `#![cfg(unix)]` throughout).

**Adequacy judgment:** Sufficient for merge. The highest-risk logic (degraded
payload honesty, no silent `"running"`, superset keys) is unit-tested at the
single builder that both paths use; TC-6’s primary failure mode (multi-second
slice overrun) has a live regression test; transport retry semantics are covered
elsewhere. Missing fault-injection e2e is an accepted gap — the wiring is
two `Err(_)` early returns calling the tested builder, not complex state
machine logic. A fake-daemon test would be valuable hardening but is not a
blocker given the builder tests + inspection + retry-gate coverage.

## Constraint audit

| Constraint | Result |
|------------|--------|
| Zero new dependencies | **PASS** — `std::time::Instant`/`Duration` only; no new `use` or crates |
| No forbidden literals in `crates/mcp/src` new code | **PASS** — grep shows only pre-existing `lib.rs`/`main.rs` prohibition docs |
| `clippy -D warnings` clean | **PASS** (author-verified; deferred `receipt`, `u64::try_from`, `map_or` patterns address known lints) |
| ASCII only in source | **PASS** |
| Additive JSON in fixture; no protocol/wire change; 37 live tools | **PASS** — fixture adds fields + `response_example_degraded` + invariants; catalogue test unchanged at 37 |
