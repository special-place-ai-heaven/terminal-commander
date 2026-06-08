# Cursor code-review task: Terminal Commander Phase 3 (TC-1b + TC-6)

You are an expert Rust reviewer. Perform an INDEPENDENT, adversarial code review of
one uncommitted change set in this repository and write your report to a new file
`cursor_review.md` at the repo root. Do not modify any other file. Do not fix
anything; review only.

## Output contract (write this file)

Create `cursor_review.md` with:

1. `## Verdict` — one of `APPROVE`, `APPROVE WITH NITS`, `REQUEST CHANGES`, and one
   sentence why.
2. `## Blockers` — numbered; each: file:line, what is wrong, why it matters, the
   fix. Empty if none.
3. `## Non-blocking findings` — nits, style, naming, doc, test-strength.
4. `## Correctness analysis` — walk the wait loop and the result builder; state
   whether each invariant below holds, with reasoning (not just "looks fine").
5. `## Test adequacy` — are the added tests sufficient? what is NOT covered?
6. `## Constraint audit` — pass/fail for each hard constraint listed below.

Be concrete. Prefer "this overruns when X because Y at line Z" over generic advice.
Assume the author is competent; find the real defects, not cosmetic ones. If you
believe it is correct, say so and defend it.

## Context

- Repo: `terminal-commander` (Rust workspace). Branch: `fix/tc-trust-defects`.
- This is an MCP stdio adapter (`crates/mcp`) that forwards 1:1 to a daemon over
  Unix-socket / Windows-named-pipe IPC. The adapter NEVER spawns processes, opens
  files, or binds sockets — it only talks to the daemon via `self.daemon.call(...)`.
- The change fixes two trust defects in the `run_and_watch` tool (one-shot:
  start a command -> bounded wait draining rule "signals" -> return signals+exit):
  - **TC-1b (lost job handle):** once the start RPC succeeds and a `job_id` exists,
    ANY later in-loop IPC error returned a bare `Err`, discarding the live
    `job_id`/`bucket_id`/`cursor`/signals. The agent was told "error" with no way
    to recover the running job.
  - **TC-6 (wait cap self-violation):** `wait_ms` (max 60000) is advertised as the
    cap, but the old loop ran `ceil(wait_ms / 1000)` iterations each blocking up to
    a full 1000ms slice, so `wait_ms=60000` produced ~62-70s wall time.
- Both were deliberately fixed in ONE wait-loop rewrite (they share the loop body;
  two separate edits would collide).

## Files changed (uncommitted, vs HEAD = commit cf43d5b)

1. `crates/mcp/src/tools.rs`
   - rewrote `run_and_watch` (the wait loop + result composition)
   - new free fn `run_and_watch_result` (ONE result builder, normal + degraded)
   - new free fn `collect_rule_signals` (extracted signal-filter)
   - new const `RUN_AND_WATCH_RECOVER_HINT`
   - new inline unit tests (3) in `mod tests`
   - updated the `#[tool]` description string
2. `crates/mcp/tests/mcp_live_command_e2e.rs` — 2 new live e2e tests (cfg(unix))
3. `tests/fixtures/contracts/mcp-tools/run_and_watch.v1.json` — additive doc
   fields + invariants (no schema test gates these fields)

## Design intent / invariants to verify

- TC-1b and TC-6 are ONE wait-loop rewrite. The START-arm error still returns
  `Err` (no job_id exists yet, nothing to preserve). Only POST-job_id errors
  (the `CommandStatus` arm and the `BucketWait` arm) become a degraded result.
- The degraded result is SUCCESS-shaped (`Ok(CallToolResult)`, isError:false),
  carries the real `job_id`/`bucket_id`/`cursor`, sets `complete=false` /
  `wait_exhausted=true` / `degraded=true`, includes a `recover_hint`, and reports
  `state` as the LAST OBSERVED `JobState` or the string `"unknown"` if the daemon
  failed before the first status poll — NEVER a silent "running".
- `run_and_watch_result` is the SINGLE builder for both paths, so the normal and
  degraded payloads are a strict superset of each other: `cursor`, `degraded`
  (false on normal), and `recover_hint` (null on normal) appear on BOTH.
- Wall-clock cap: `let deadline = Instant::now() + Duration::from_millis(wait_ms)`;
  the loop is a `loop {}` whose deadline check is at the BOTTOM (do-while) so it
  always polls status at least once (this preserves the old `.max(1)` guarantee,
  important for `wait_ms=0`). Per-iteration `BucketWait` timeout is
  `min(MAX_WAIT_SLICE_MS=1000, remaining_ms)` (0 if terminal). Total wall time is
  bounded by `wait_ms` + at most one in-flight slice + RTTs.
- `MAX_WAIT_SLICE_MS` stays 1000ms (lowering it to 500 would double RPC/sec and
  risk regressing a load-noise backpressure gate).
- On non-terminal deadline-exit, ONE final non-blocking `BucketWait`
  (`timeout_ms=Some(0)`) drains events that arrived since the last cursor before
  composing; on `wait_exhausted` the cursor is authoritative for resumption and
  signals are best-effort.
- The terminal short-circuit (`if terminal { slice=0 }` + break on terminal/cap)
  is preserved so a fast command returns immediately with all its signals.
- No fake success: the degraded result keeps the REAL job_id; signals are not
  dropped on a fast exit.

## Hard constraints (audit each: PASS/FAIL)

- ZERO new dependencies. (`std::time::Instant`/`Duration` are std, already used in
  this file via fully-qualified paths; no new `use`, no new crate.)
- `crates/mcp/src` must contain NONE of these literal substrings, even in
  comments: `Command::new`, `Command::spawn`, `TcpListener`, `UdpSocket`,
  `tokio::fs`, `std::fs`, `File::open`, `read_to_string`, `read_to_end`.
  (Pre-existing doc comments in `lib.rs`/`main.rs` that NAME the prohibition are
  grandfathered; the new code must add none.)
- `clippy -D warnings` must stay clean (already verified on Windows; the author
  fixed: a dead-store on `receipt` via deferred init, a `u128 -> u64` truncation
  via `u64::try_from(...).unwrap_or(u64::MAX)`, and two `option_if_let_else`
  rewrites to `map_or`/`map_or_else`).
- ASCII only in source.
- Additive JSON only in the fixture; no protocol/wire change; no tool added or
  removed (the catalogue still lists 37 live tools).

## Specific things to attack

1. **Deferred init of `receipt`.** `let mut receipt: Option<serde_json::Value>;`
   with no initializer. Is `receipt` provably initialized on every path that
   reaches the final non-degraded `run_and_watch_result(...)` call? The degraded
   arms `return` before reading it. Could any control-flow path read it
   uninitialized? (Rust definite-init should reject if so — but confirm the logic,
   not just "it compiled".)
2. **do-while vs `wait_ms=0`.** With `wait_ms=0`, `deadline` is already in the
   past. Trace the first iteration: status poll happens, then `slice_ms =
   min(1000, 0) = 0` (non-blocking BucketWait), then the bottom deadline check
   fires the final non-blocking drain and breaks. Is that correct and bounded? Any
   double-drain redundancy? Is it harmful (extra non-blocking RPC) or just benign?
3. **Deadline overrun bound.** Argue the maximum wall time. Can the loop ever block
   for a full 1000ms slice AFTER the deadline has nearly arrived? (slice is capped
   by `remaining_ms`.) Off-by-one on the `>=` deadline check?
4. **Degraded honesty.** If the FIRST `CommandStatus` errors, `last_observed_state`
   is `None` -> state `"unknown"`, exit_code `None`. Confirm no path reports
   `JobState::Running` without having observed it.
5. **Superset truth.** Confirm `cursor`, `degraded`, `recover_hint` are emitted on
   BOTH the normal and degraded payloads (one builder), and `receipt` is null on
   degraded.
6. **Idempotent retry interaction.** `self.daemon.call` may internally retry
   idempotent reads once on transport failure (a separate, already-reviewed gate).
   `CommandStatus`/`BucketWait` are reads. Does the degraded path trigger only
   AFTER that internal retry is exhausted (i.e. `call` returns `Err`)? Any
   double-effect risk? (Note: mutating RPCs are NOT retried; these are reads.)
7. **Signal cap.** `collect_rule_signals` keeps only `ev.rule.is_some()` events and
   stops at `max_signals`. The `BucketWait` `limit` is
   `max_signals.saturating_sub(signals.len()).max(1)`. Any way to exceed
   `max_signals`, or to spin without progress?
8. **Test adequacy.** The unit tests cover the BUILDER shape (degraded/unknown,
   degraded/last-observed, normal/terminal-superset). The live tests cover the
   wall-clock cap (non-terminating `sleep 5`, `wait_ms=1500`, assert wall <1900ms +
   `wait_exhausted`) and a fast command. There is NO live fault-injection test that
   forces a real mid-wait IPC error to exercise the degraded WIRING end-to-end
   (would require a stateful protocol-speaking fake daemon). Is the builder unit
   test + the wiring-by-inspection + the existing transport-error retry-gate tests
   adequate, or is a fault-injection integration test a blocker?

## The actual changed code (ground truth — review THIS)

### `crates/mcp/src/tools.rs` :: `run_and_watch` (rewritten)

```rust
async fn run_and_watch(
    &self,
    Parameters(params): Parameters<McpRunAndWatchParams>,
) -> Result<CallToolResult, McpError> {
    use terminal_commander_core::JobState;

    self.ensure_daemon_available().await?;
    let (start_params, wait_ms, max_signals) = params.into_parts();
    let start_ipc = start_params.into_ipc()?;

    // 1. Start.
    let (job_id, bucket_id, mut cursor) = match self
        .daemon
        .call(IpcRequest::CommandStartCombed(start_ipc))
        .await
    {
        Ok(IpcResponse::CommandStartCombed(CommandStartResponse {
            job_id, bucket_id, cursor, ..
        })) => (job_id, bucket_id, cursor),
        Ok(other) => return Err(unexpected_variant(&other)),
        Err(e) => return Err(into_mcp_error_for(false, &e)),
    };

    // 2. Wait loop (wall-clock budget, TC-6).
    let mut signals: Vec<terminal_commander_core::SignalEvent> = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
    let mut last_observed_state: Option<JobState> = None;  // TC-1b: never silent Running
    let mut exit_code: Option<i32> = None;
    let mut receipt: Option<serde_json::Value>;  // deferred init (avoid dead store)

    loop {
        let status = match self
            .daemon
            .call(IpcRequest::CommandStatus(CommandStatusParams { job_id }))
            .await
        {
            Ok(IpcResponse::CommandStatus(s)) => s,
            Ok(other) => return Err(unexpected_variant(&other)),
            // TC-1b: preserve the job handle on a post-job_id transport error.
            Err(_) => {
                return run_and_watch_result(
                    job_id, bucket_id, cursor, last_observed_state, exit_code,
                    &signals, None, true, Some(RUN_AND_WATCH_RECOVER_HINT),
                );
            }
        };
        last_observed_state = Some(status.state);
        exit_code = status.exit_code;
        receipt = status.receipt.as_ref().map(|r| serde_json::json!(r));

        let terminal = matches!(
            status.state,
            JobState::Exited | JobState::Cancelled | JobState::Failed
        );

        let remaining_ms = u64::try_from(
            deadline.saturating_duration_since(std::time::Instant::now()).as_millis(),
        ).unwrap_or(u64::MAX);
        let slice_ms = if terminal { 0 } else { MAX_WAIT_SLICE_MS.min(remaining_ms) };

        let wait = BucketWaitParams {
            bucket_id, cursor,
            severity_min: None, kind_filter: None,
            limit: Some(max_signals.saturating_sub(signals.len()).max(1)),
            timeout_ms: Some(slice_ms),
        };
        match self.daemon.call(IpcRequest::BucketWait(wait)).await {
            Ok(IpcResponse::BucketWait(r)) => {
                cursor = r.next_cursor;
                collect_rule_signals(r.events, &mut signals, max_signals);
            }
            Ok(other) => return Err(unexpected_variant(&other)),
            // TC-1b: same as the status arm -- preserve the job handle.
            Err(_) => {
                return run_and_watch_result(
                    job_id, bucket_id, cursor, last_observed_state, exit_code,
                    &signals, None, true, Some(RUN_AND_WATCH_RECOVER_HINT),
                );
            }
        }

        if terminal || signals.len() >= max_signals {
            break;
        }
        // TC-6: budget spent + still running -> one final non-blocking drain.
        if std::time::Instant::now() >= deadline {
            let drain = BucketWaitParams {
                bucket_id, cursor,
                severity_min: None, kind_filter: None,
                limit: Some(max_signals.saturating_sub(signals.len()).max(1)),
                timeout_ms: Some(0),
            };
            if let Ok(IpcResponse::BucketWait(r)) =
                self.daemon.call(IpcRequest::BucketWait(drain)).await
            {
                cursor = r.next_cursor;
                collect_rule_signals(r.events, &mut signals, max_signals);
            }
            break;
        }
    }

    // 3. Non-degraded result via the shared builder (strict superset).
    run_and_watch_result(
        job_id, bucket_id, cursor, last_observed_state, exit_code,
        &signals, receipt, false, None,
    )
}
```

### `crates/mcp/src/tools.rs` :: `run_and_watch_result` (new shared builder)

```rust
fn run_and_watch_result(
    job_id: terminal_commander_core::JobId,
    bucket_id: terminal_commander_core::BucketId,
    cursor: u64,
    last_observed_state: Option<terminal_commander_core::JobState>,
    exit_code: Option<i32>,
    signals: &[terminal_commander_core::SignalEvent],
    receipt: Option<serde_json::Value>,
    degraded: bool,
    recover_hint: Option<&str>,
) -> Result<CallToolResult, McpError> {
    let (complete, wait_exhausted) = if degraded {
        (false, true)
    } else {
        last_observed_state.map_or((false, true), run_and_watch_completion)
    };
    let state_json = last_observed_state
        .map_or_else(|| serde_json::json!("unknown"), |state| serde_json::json!(state));
    let include_receipt = !degraded && signals.is_empty();
    json_tool_result(&serde_json::json!({
        "job_id": job_id,
        "bucket_id": bucket_id,
        "state": state_json,
        "exit_code": exit_code,
        "signals": signals,
        "signal_count": signals.len(),
        "receipt": if include_receipt { receipt } else { None },
        "complete": complete,
        "wait_exhausted": wait_exhausted,
        "cursor": cursor,
        "degraded": degraded,
        "recover_hint": recover_hint,
    }))
}
```

`run_and_watch_completion(state: JobState) -> (bool /*complete*/, bool /*wait_exhausted*/)`
returns `(terminal, !terminal)` where terminal = Exited|Cancelled|Failed.

### `crates/mcp/src/tools.rs` :: `collect_rule_signals` (new)

```rust
fn collect_rule_signals(
    events: Vec<terminal_commander_core::SignalEvent>,
    signals: &mut Vec<terminal_commander_core::SignalEvent>,
    max_signals: usize,
) {
    for ev in events {
        if ev.rule.is_some() && signals.len() < max_signals {
            signals.push(ev);
        }
    }
}
```

### `crates/mcp/src/tools.rs` :: const (new)

```rust
const RUN_AND_WATCH_RECOVER_HINT: &str = "IPC error interrupted the wait, but the job is still tracked. First confirm daemon liveness with the `health` tool, then poll command_status with this job_id for the final state and signals. Do not re-run the command.";
```

`JobState` = `{ Starting, Running, Exited, Cancelled, Failed }` (serializes lowercase:
"running", "exited", ...). `JobId`/`BucketId` are `Copy` typed-UUID newtypes that
serialize to `job_<hex>` / `bkt_<hex>`. `cursor` is a `u64`.

## How to do the review

Read the real files in the repo (the embedded code above is the source of truth if
they differ — flag any discrepancy). Reason about the control flow yourself. Then
write `cursor_review.md` per the output contract. Keep it tight and concrete.
