# PLAN-TC1b/TC6 -- Wait-loop rewrite: degraded result + wall-clock cap (Phase 3)

**Source:** TC trust-defects campaign (`plan-final.json` Phase 3 / forks F2+F3) +
`review-verdict.json` adopted optional improvements (degraded state honesty:
UNKNOWN/last-observed not silently Running, recover_hint confirms daemon liveness
first; final non-blocking drain on deadline-exit; cursor superset coherence:
cursor on the NORMAL payload too -- one builder).
**Posture:** TC-1b and TC-6 MUST be co-designed in ONE wait-loop rewrite (shared
body `tools.rs:650-709`; two separate edits collide -- research-gaps explicit
warning). Localized arithmetic + result-shape fix. ZERO protocol/surface change.
Instant via tokio::time already in-tree (zero new dep). MAX_WAIT_SLICE_MS stays
1000ms (no load-gate RPC-doubling risk).

Language: ASCII only.

---

## Summary table

| Symptom | Location (file:line) | Fix sketch | Effort | Test impact |
|---------|----------------------|------------|--------|-------------|
| Mid-loop RPC error discards the known job_id/bucket_id/cursor/signals, returns a bare Err | `crates/mcp/src/tools.rs:665` (CommandStatus arm), `:703` (BucketWait arm) | Both post-job_id error arms build a DEGRADED isError:false result (strict superset) via one shared helper | **M** | unit (degraded carries job_id+cursor+degraded:true); integration (inject mid-loop error) |
| wait_ms=60000 cap self-violates (~62-70s wall) | `crates/mcp/src/tools.rs:650` (`for _ in 0..deadline_slices`), `:683` (hardcoded MAX_WAIT_SLICE_MS) | Wall-clock `Instant` deadline; per-slice timeout = min(slice, remaining) | **M** | integration (wall <= ~61s at wait_ms=60000) |
| degraded state silently defaults to Running | `crates/mcp/src/tools.rs:651` (`JobState::Running` default) | Mark state UNKNOWN/last-observed; do not read "Running" as "confirmed running" | **S** | unit (degraded state is last-observed/UNKNOWN, not forced Running) |
| events arriving after the last cursor before deadline-exit are lost | post-loop on non-terminal deadline exit | ONE final non-blocking BucketWait (timeout_ms 0) drain before building degraded | **S** | integration (signals best-effort, cursor authoritative) |
| degraded "strict superset" is not actually a superset (cursor only on degraded) | `crates/mcp/src/tools.rs:724-736` (normal payload), `:147` change adds only degraded:false+recover_hint:null | Add cursor to the NORMAL payload too; genuinely ONE builder | **S** | unit (normal builder sets degraded:false, recover_hint:null, cursor present) |

**Estimated files:** 4 (`crates/mcp/src/tools.rs`,
`tests/fixtures/contracts/mcp-tools/run_and_watch.v1.json`, plus the test
module/timing assertions).

---

## Per-item detail

### TC-1b -- lost job handle on mid-wait IPC error

**Symptom:** Once run_and_watch holds a job_id (the start RPC at tools.rs:630-643
succeeded), any subsequent in-loop RPC error returns `Err(into_mcp_error)`,
discarding the known job_id/bucket_id/cursor/signals. The agent is told "error"
with no way to recover the live job.

**Citations:**

```665:665:crates/mcp/src/tools.rs
Err(e) => return Err(into_mcp_error(&e)), // CommandStatus arm -- discards job_id
```

```703:703:crates/mcp/src/tools.rs
Err(e) => return Err(into_mcp_error(&e)), // BucketWait arm -- discards job_id
```

```724:736:crates/mcp/src/tools.rs
// the existing run_and_watch normal success payload (degraded result must be a strict superset)
```

**Fix:**

1. **Convert both post-job_id error arms** (tools.rs:665, :703) from
   `Err(into_mcp_error(&e))` to a DEGRADED partial payload via a shared helper
   `run_and_watch_degraded_result(job_id, bucket_id, cursor, signals, last_state, last_exit_code)`
   returning isError:false with:
   `{job_id, bucket_id, state, exit_code, signals, signal_count, cursor,
   receipt:null, complete:false, wait_exhausted:true, degraded:true, recover_hint}`.
2. **Degraded state honesty (adopted optional):** `state` is the LAST OBSERVED /
   UNKNOWN value, NOT a silent default to `JobState::Running` (tools.rs:651). The
   agent must not read "Running" as "confirmed still running" when the daemon may
   have died.
3. **recover_hint confirms liveness first (adopted optional):** the hint tells
   the agent to FIRST confirm daemon liveness (health) before polling
   command_status in a loop, so a permanently-dead daemon is detected rather than
   polled forever. Example: "IPC error mid-wait; the job is still tracked. First
   confirm daemon health, then poll command_status with job_id for the final
   state/signals; do not re-run."
4. **The start-arm error at tools.rs:643 STILL returns Err** (no job_id exists
   yet, nothing to preserve).

**Effort:** M. **Test:**
- unit (crates/mcp): the degraded helper carries job_id + bucket_id + cursor +
  degraded:true + recover_hint and is isError:false-shaped; state is
  last-observed/UNKNOWN, not forced Running. source-status: test-only.
- integration: inject a mid-loop IPC error AFTER job_id is known (mock daemon
  errors on the 2nd CommandStatus) and assert isError:false + degraded:true + the
  correct preserved job_id. source-status: live (partial/mock for the injection).

---

### TC-6 -- run_and_watch wait_ms cap self-violation

**Symptom:** `wait_ms=60000` is advertised as the cap but `for _ in 0..deadline_slices`
x per-slice BucketWait blocking up to MAX_WAIT_SLICE_MS=1000 + RTTs yields
~62-70s wall.

**Citations:**

```650:650:crates/mcp/src/tools.rs
let deadline_slices = wait_ms.div_ceil(MAX_WAIT_SLICE_MS).max(1); // for _ in 0..deadline_slices
```

```683:683:crates/mcp/src/tools.rs
// per-iteration BucketWait timeout_ms hardcoded MAX_WAIT_SLICE_MS
```

**Fix:**

1. **Rewrite the wait loop ONCE** (shared body tools.rs:650-709): replace
   `let deadline_slices = wait_ms.div_ceil(MAX_WAIT_SLICE_MS).max(1)` +
   `for _ in 0..deadline_slices` with
   `let deadline = Instant::now() + Duration::from_millis(wait_ms)`; loop
   `while Instant::now() < deadline`.
2. **Per-iteration BucketWait timeout** = `Some(if terminal {0} else { min(MAX_WAIT_SLICE_MS, remaining_ms) })`
   (was hardcoded MAX_WAIT_SLICE_MS at :683); break when remaining hits zero.
3. **PRESERVE the terminal short-circuit** (tools.rs:705-707) so a fast command
   returns immediately with all signals.
4. **Final non-blocking drain (adopted optional):** after the while loop exits due
   to deadline while NON-terminal, do ONE final non-blocking BucketWait
   (timeout_ms: Some(0)) to drain events that arrived since the last cursor before
   building the degraded result, mirroring the existing terminal short-circuit
   drain (tools.rs:683). Document that on wait_exhausted, signals is best-effort
   and cursor is authoritative for resumption.
5. **Keep MAX_WAIT_SLICE_MS=1000** (no slice-size change): the judge graft warns
   lowering it to 500 doubles RPC/sec and risks regressing the TC47 load gate
   (load_noise_backpressure); the wall-clock deadline alone makes the advertised
   cap honest (total wall <= wait_ms + at most one in-flight slice + RTTs).

**Effort:** M. **Test:**
- unit (crates/mcp): at wait_ms=60000 the computed per-slice timeout never
  exceeds remaining and the loop cannot exceed deadline + one slice. source-status:
  test-only.
- integration through daemon IPC (TEST socket): (a) a command that exits within
  budget returns complete:true, degraded:false with all signals; (b) a
  non-terminating command with wait_ms=2000 returns wait_exhausted:true +
  complete:false + job_id present and measured wall time <= ~2.5s; (c) a
  long-running command with wait_ms=60000 returns measured wall <= 61s (TC-6
  acceptance). source-status: live.

---

### shared -- one builder + normal-path superset

**Fix:** The NORMAL success payload (tools.rs:724-736) always sets
`degraded:false` AND `recover_hint:null` AND `cursor` (adopted optional --
genuinely ONE shared builder so the normal and degraded paths cannot drift and the
degraded result is a TRUE strict superset). Low risk: R3 verified no schema test
gates run_and_watch result fields. Update the run_and_watch `#[tool]` description
(tools.rs:617): the wait_ms cap is a WALL-CLOCK budget honored within one slice +
RTT; on a mid-wait interruption it returns a degraded job-identified result (poll
command_status after confirming health), never a bare error once a job_id exists.
Add invariants to `tests/fixtures/contracts/mcp-tools/run_and_watch.v1.json`
documenting degraded/recover_hint/cursor (additive, not a hard gate).

---

## Invariants (Phase 3)

- TC-1b and TC-6 are ONE wait-loop rewrite (shared body tools.rs:650-709): the
  start-arm error still returns Err (no job_id yet); only post-job_id errors
  become the degraded partial result. MAX_WAIT_SLICE_MS stays 1000ms.
- Degraded state is last-observed/UNKNOWN, never a silent "Running"; recover_hint
  tells the agent to confirm daemon health BEFORE polling.
- A final non-blocking drain (timeout_ms 0) runs on non-terminal deadline-exit;
  cursor is authoritative for resumption, signals best-effort.
- cursor is present on BOTH the normal and degraded payloads (true strict
  superset, one builder).
- degraded/recover_hint/cursor are additive JSON; no run_and_watch result-schema
  test exists (R3), so no gate breaks, but the crates/mcp/tests e2e bucket flow
  must still pass.
- No fake success: the degraded result keeps the REAL job_id; the loop must NOT
  drop the final signals on a fast exit (preserve the terminal short-circuit and
  drain semantics).

## Verification (Phase 3)

- `wsl bash scripts/linux-gate.sh` (fmt + clippy -D + nextest + TC47 load gate
  UNCHANGED since slice stays 1000ms + MCP guards; pure crates/mcp/src edit,
  Instant via tokio::time already in-tree, no fs/socket).
- `pwsh -File scripts/windows-gate.ps1`.
- `cargo nextest run -p terminal-commander-mcp` (incl. the e2e bucket flow in
  crates/mcp/tests must still pass).
- timing assertion in the long-running integration test proves wall <= advertised
  cap + bounded margin (TC-6 acceptance); assert the mid-loop-error case never
  returns isError:true once job_id is known.
