# Ergonomics Chain Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a zero-rule Terminal Commander command return a truthful, bounded "exit receipt" instead of silence, and rewrite the MCP tool descriptions so an agent both CHOOSES TC and KEEPS trusting it.

**Architecture:** Two independent slices.
(1) TCE-ERG-1: a new `CommandReceipt` is computed in the daemon lifecycle waiter at child exit (where the pre-lifecycle `events_emitted` is still the true rule-match count), stored on the `JobBinding`, and surfaced through the existing `CommandStatusResponse` -> IPC -> MCP `command_status` path. The receipt tail is read from the already-populated context ring via a new pure read method `ContextRingManager::tail_frames`. The receipt is emitted ONLY when zero rule-driven events fired (security carve-out), and `CommandService` only ever holds process probes (PTY runs through a separate service), so the tail is structurally process-only.
(2) TCE-ERG-2: pure-text rewrite of the MCP server `instructions` and `command_start_combed` / `command_status` tool descriptions. No logic.

**Tech Stack:** Rust workspace (crates: core, probes, daemon, mcp). Tests via `cargo nextest`. Daemon IPC tests are `#![cfg(unix)]` and run under WSL2 on this host: `wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p <crate> <filter>"`. The `-lc` login shell is required for cargo to be on PATH.

---

## Background: verified code facts (do not re-derive)

All confirmed against the SymForge index on 2026-05-26:

- `crates/core/src/context.rs`
  - `RingInner` (lines 235-240): `frames: VecDeque<SourceFrame>`, `evicted_frames: u64`, `total_bytes: usize`, `config: ContextRingConfig`.
  - `SourceFrame` (56-70): has `pub text: String`, `pub line: Option<u64>`, `pub stream: SourceStream`.
  - `ContextRingManager::window` (408-415) is anchor-only; `cell(probe_id)` (383-389) returns `Result<Arc<RwLock<RingInner>>, ContextError>`. There is NO tail method today.
  - Ring creation: `create_ring(probe_id, ContextRingConfig)` (357) and `create_ring_default(probe_id)` (374). `ContextRingConfig { max_frames, max_bytes }` (129-134). Test helper `fn frame(probe_id, text, line)` (462) + `SourceFrame::new(p, stream, text).with_line(n)` builder. `ProbeId::new()` mints a fresh id.
  - `MAX_FRAME_BYTES = 8192` (line 42) is the per-frame cap.
- `crates/daemon/src/ipc/protocol.rs`
  - `MAX_RESPONSE_BYTES = MAX_FRAME_BYTES` where this crate's `MAX_FRAME_BYTES = 256 * 1024` (line 45). The transport envelope is 256 KiB, ample for a 5-line tail.
  - Re-exports `CommandStatusResponse` from `crate::command` (line 34). There is exactly ONE definition.
- `crates/daemon/src/command.rs`
  - `CommandStatusResponse` defined at 162-175 (fields incl. `frames_total`, `events_emitted`, `exit_code`, `state`, `probe_id`). `#[derive(Debug, Clone, Serialize, Deserialize)]`.
  - `CommandService::status` (723-748) builds it; `self.rings: Arc<ContextRingManager>` (261) and `self.live` are in scope.
  - `JobBinding` (212-221): `metrics`, `sifter`, `inline_rules`, `bucket_id`, `probe_id`.
  - Lifecycle waiter (553-578): `tokio::spawn` after `drive_to_exit`; has `final_metrics` (a `ProcessProbeMetrics`), `outcome`, `bucket_id`, `probe_id`, `waiter_live`, `waiter_rings` is NOT yet captured (must add). At 564-569 it conditionally appends the lifecycle draft and bumps `final_metrics.events_emitted`. At 576-578 it writes `b.metrics = final_metrics`.
  - `status()` reads `metrics` via `self.live.read().get(&job_id).map(|b| b.metrics.clone()).unwrap_or_default()`.
- `crates/probes/src/process.rs`
  - Frame appended to ring ALWAYS (268-269), regardless of rule match.
  - `events_emitted += 1` ONLY per sifter-matched draft (288-294). So at child exit, BEFORE the lifecycle bump, `final_metrics.events_emitted` is the exact count of rule-driven events.
  - `ProcessProbeMetrics` (58-64): `frames_total, frames_stdout, frames_stderr, bytes_total, events_emitted`. No secret/PTY fields (process-only by construction).
- `crates/daemon/src/ipc/server.rs`
  - `handle_command_status` (920-929) calls `state.command.status(job_id)`. `state.command` is the process `CommandService`; PTY jobs are NOT in it (they hit `UnknownJob`). A2 (process-only) holds structurally.
- `crates/mcp/src/tools.rs`
  - `command_status_payload` (1366-1381) hand-builds the JSON from `CommandStatusResponse` fields. Adding a field to the struct is NOT auto-surfaced here; this fn must add the `"receipt"` key.
  - `command_status` tool wrapper at 472-481; description at ~395-397. `command_start_combed` description at ~364-366.
  - MCP `instructions` string built in `get_info().with_instructions` (~1102-1104).

## File Structure

- **Create:** none.
- **Modify:**
  - `crates/core/src/context.rs` — add `RingTail` struct + `RingInner::tail` + `ContextRingManager::tail_frames`.
  - `crates/daemon/src/command.rs` — add `CommandReceipt` struct; add `receipt: Option<CommandReceipt>` to `CommandStatusResponse`; add `receipt` field to `JobBinding`; compute receipt in lifecycle waiter; return it from `status()`.
  - `crates/mcp/src/tools.rs` — surface `receipt` in `command_status_payload`; rewrite descriptions + instructions (ERG-2).
  - `crates/mcp/src/tools.rs` description text + TC47 test text (A1 contract update).
- **Test:**
  - `crates/core/src/context.rs` `#[cfg(test)] mod tests` — `tail_frames` unit tests.
  - `crates/daemon/tests/mcp_live_command_e2e.rs` — receipt e2e (no-rule -> receipt; rule-match -> no receipt).
  - `crates/daemon/tests/load_noise_backpressure.rs` (TC47) — update invariant comment + add carve-out assertion.

---

## Task 1: `ContextRingManager::tail_frames` read path (A3)

**Files:**
- Modify: `crates/core/src/context.rs` (add `RingTail`, `RingInner::tail`, `ContextRingManager::tail_frames`)
- Test: `crates/core/src/context.rs` `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `crates/core/src/context.rs`:

Use the existing test idioms confirmed in this file's `mod tests`
(lines 457-496): `ProbeId::new()`, the local helper
`fn frame(probe_id, text, line) -> SourceFrame` (462-464, builds a
`SourceStream::Stderr` frame via `SourceFrame::new(...).with_line(...)`),
`mgr.create_ring(pid, ContextRingConfig { .. })` for a custom-capped
ring, and `mgr.create_ring_default(pid)` for the default. Reuse the
`frame` helper — do NOT hand-roll `SourceFrame` field assignment.

```rust
#[test]
fn tail_frames_returns_last_n_in_order() {
    let mgr = ContextRingManager::new();
    let pid = ProbeId::new();
    mgr.create_ring(pid, ContextRingConfig { max_frames: 100, max_bytes: 1_000_000 })
        .unwrap();
    for i in 0..10u64 {
        mgr.append_frame(pid, frame(pid, &format!("line {i}"), i + 1)).unwrap();
    }
    let tail = mgr.tail_frames(pid, 3, 1_000_000).unwrap();
    assert_eq!(tail.lines, vec!["line 7", "line 8", "line 9"]);
    assert_eq!(tail.evicted_frames, 0);
    assert!(!tail.truncated);
}

#[test]
fn tail_frames_empty_ring_returns_empty() {
    let mgr = ContextRingManager::new();
    let pid = ProbeId::new();
    mgr.create_ring(pid, ContextRingConfig { max_frames: 100, max_bytes: 1_000_000 })
        .unwrap();
    let tail = mgr.tail_frames(pid, 5, 1_000_000).unwrap();
    assert!(tail.lines.is_empty());
    assert_eq!(tail.evicted_frames, 0);
}

#[test]
fn tail_frames_reports_eviction() {
    let mgr = ContextRingManager::new();
    let pid = ProbeId::new();
    mgr.create_ring(pid, ContextRingConfig { max_frames: 3, max_bytes: 1_000_000 })
        .unwrap();
    for i in 0..6u64 {
        mgr.append_frame(pid, frame(pid, &format!("l{i}"), i + 1)).unwrap();
    }
    let tail = mgr.tail_frames(pid, 5, 1_000_000).unwrap();
    // ring capped at 3 frames; 3 evicted
    assert_eq!(tail.lines, vec!["l3", "l4", "l5"]);
    assert_eq!(tail.evicted_frames, 3);
}

#[test]
fn tail_frames_byte_cap_truncates_from_front() {
    let mgr = ContextRingManager::new();
    let pid = ProbeId::new();
    mgr.create_ring(pid, ContextRingConfig { max_frames: 100, max_bytes: 1_000_000 })
        .unwrap();
    for i in 0..5u64 {
        // each line "xxxx" = 4 bytes
        mgr.append_frame(pid, frame(pid, "xxxx", i + 1)).unwrap();
    }
    // ask for 5 lines but only 9 bytes: fits 2 lines (8 bytes), drops oldest
    let tail = mgr.tail_frames(pid, 5, 9).unwrap();
    assert_eq!(tail.lines.len(), 2);
    assert!(tail.truncated);
}

#[test]
fn tail_frames_unknown_probe_is_error() {
    let mgr = ContextRingManager::new();
    let pid = ProbeId::new();
    assert!(mgr.tail_frames(pid, 5, 1000).is_err());
}
```

Note: the `frame` helper builds `Stderr` frames; `tail_frames` is
stream-agnostic (it returns `text` regardless of stream), so the
assertions hold. `ContextRingConfig` fields are `max_frames` and
`max_bytes` (confirmed lines 129-134).

- [ ] **Step 2: Run tests to verify they fail**

Run (WSL not required for the core crate, but use it for parity):
```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-core tail_frames"
```
Expected: FAIL — `no method named tail_frames`.

- [ ] **Step 3: Implement `RingTail` + `RingInner::tail` + `ContextRingManager::tail_frames`**

Add the public return type near `ContextWindowResponse` in `crates/core/src/context.rs`:

```rust
/// Bounded tail of a probe ring. Used by the no-silence exit receipt
/// (TCE-ERG-1) when a command finished with zero rule-driven events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingTail {
    /// Last N frame texts in chronological order (oldest first).
    pub lines: Vec<String>,
    /// Frames evicted from the ring since creation. When > 0 the tail
    /// may not include the earliest output; callers should flag this.
    pub evicted_frames: u64,
    /// True when the byte cap dropped one or more of the requested
    /// trailing frames.
    pub truncated: bool,
}
```

Add to `impl RingInner` (after `window`):

```rust
/// Return the last `max_lines` frame texts, oldest first, bounded by
/// `max_bytes` (newest frames win when the byte budget is tight).
fn tail(&self, max_lines: usize, max_bytes: usize) -> RingTail {
    let mut chosen: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    for frame in self.frames.iter().rev().take(max_lines) {
        let len = frame.text.len();
        if !chosen.is_empty() && bytes + len > max_bytes {
            truncated = true;
            break;
        }
        // Always include at least one line even if it alone exceeds the cap.
        if chosen.is_empty() && len > max_bytes {
            truncated = true;
        }
        bytes += len;
        chosen.push_front(frame.text.clone());
    }
    RingTail {
        lines: chosen.into_iter().collect(),
        evicted_frames: self.evicted_frames,
        truncated,
    }
}
```

Add to `impl ContextRingManager` (after `window`):

```rust
/// Read a bounded tail of a probe's ring without an anchor. Pure
/// read; never mutates. Returns `NotFound` when the ring is absent.
pub fn tail_frames(
    &self,
    probe_id: ProbeId,
    max_lines: usize,
    max_bytes: usize,
) -> Result<RingTail, ContextError> {
    let cell = self.cell(probe_id)?;
    let inner = cell.read();
    Ok(inner.tail(max_lines, max_bytes))
}
```

- [ ] **Step 4: Run tests to verify they pass**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-core tail_frames"
```
Expected: PASS (5 tests).

- [ ] **Step 5: fmt + clippy the core crate**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo fmt -p terminal-commander-core && cargo clippy -p terminal-commander-core --all-targets -- -D warnings"
```
Expected: clean.

- [ ] **Step 6: Commit**

```
git add crates/core/src/context.rs
git commit -F <msg-file>
```
Message subject: `feat(core): add ContextRingManager::tail_frames bounded read path`
Body: one line per: pure read, VecDeque rev/take, reports evicted_frames + truncated, no new buffer (frames already retained). Co-Authored-By trailer.

---

## Task 2: `CommandReceipt` type + struct field (no logic yet)

**Files:**
- Modify: `crates/daemon/src/command.rs` (add `CommandReceipt`, add field to `CommandStatusResponse` and `JobBinding`)

- [ ] **Step 1: Add the `CommandReceipt` struct**

In `crates/daemon/src/command.rs`, immediately before `CommandStatusResponse` (line ~160), add:

```rust
/// No-silence exit receipt (TCE-ERG-1). Present ONLY when a finished
/// process command produced ZERO rule-driven events. This is the one
/// sanctioned exception to "TC never returns raw output": a bounded,
/// truthful tail so a zero-rule command does not read as breakage.
///
/// PTY/file-watch jobs never reach this path (`CommandService` holds
/// process probes only), so a tail cannot include secret-prompt input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub exit_code: Option<i32>,
    /// Frames produced by the command that no rule matched, i.e. lines
    /// the agent would have had to scroll. `frames_total` for a
    /// zero-rule run.
    pub lines_suppressed: u64,
    /// Last N frame texts (oldest first), byte-capped.
    pub tail: Vec<String>,
    /// True when the ring evicted earlier frames; the tail may omit
    /// the start of output.
    pub tail_incomplete: bool,
}
```

- [ ] **Step 2: Add `receipt` to `CommandStatusResponse`**

Append a field (keep existing fields unchanged):

```rust
pub struct CommandStatusResponse {
    pub job_id: JobId,
    pub bucket_id: BucketId,
    pub probe_id: ProbeId,
    pub state: terminal_commander_core::JobState,
    pub frames_total: u64,
    pub frames_stdout: u64,
    pub frames_stderr: u64,
    pub bytes_total: u64,
    pub events_emitted: u64,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub duration_ms: Option<u64>,
    /// No-silence receipt; `Some` only for a finished process command
    /// with zero rule-driven events. See `CommandReceipt`.
    pub receipt: Option<CommandReceipt>,
}
```

- [ ] **Step 3: Add `receipt` storage to `JobBinding`**

```rust
struct JobBinding {
    metrics: ProcessProbeMetrics,
    sifter: Arc<terminal_commander_sifters::SifterRuntime>,
    inline_rules: Vec<terminal_commander_core::RuleDefinition>,
    bucket_id: BucketId,
    probe_id: ProbeId,
    /// Computed by the lifecycle waiter at child exit; read by
    /// `status()`. `None` until exit or when any rule matched.
    receipt: Option<CommandReceipt>,
}
```

- [ ] **Step 4: Fix the two `JobBinding` constructions + `status()` builder to compile**

At the `JobBinding { ... }` literal (~527-535) add `receipt: None,`.
Search for ALL `JobBinding {` constructions (there may be one in a rebind path):
```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && grep -rn 'JobBinding {' crates/daemon/src"
```
Add `receipt: None,` to every literal.

In `status()` builder (~734-747) add `receipt: None,` for now (logic lands in Task 3).

- [ ] **Step 5: Compile-check the daemon crate**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo check -p terminal-commanderd"
```
Expected: clean (no behavior change yet).

- [ ] **Step 6: Commit**

```
git add crates/daemon/src/command.rs
git commit -F <msg-file>
```
Subject: `feat(daemon): add CommandReceipt type + plumbing fields (no logic)`

---

## Task 3: Compute the receipt in the lifecycle waiter + return from status (A4, A5, A6, A7)

**Files:**
- Modify: `crates/daemon/src/command.rs` (lifecycle waiter ~553-578; `status()` ~734-747)

- [ ] **Step 1: Capture the ring handle into the waiter**

In `start` (before `tokio::spawn` at ~553), alongside `let waiter_live = Arc::clone(&self.live);`, add:
```rust
let waiter_rings = Arc::clone(&self.rings);
```

- [ ] **Step 2: Compute the receipt BEFORE the lifecycle bump**

Inside the waiter, the value `final_metrics.events_emitted` at this point (right after `drive_to_exit`, BEFORE the 564-569 lifecycle append) is the exact rule-match count. Capture it first:

```rust
let (mut final_metrics, outcome) = drive_to_exit(probe).await;

// TCE-ERG-1: compute the no-silence receipt while events_emitted
// still reflects ONLY rule-driven events (the lifecycle bump below
// would otherwise inflate it by one).
let rule_driven_events = final_metrics.events_emitted;
let exit_code = match &outcome {
    ProbeOutcome::Exited { code, .. } => *code,
    ProbeOutcome::Cancelled => None,
};
let receipt = if rule_driven_events == 0 {
    // Zero rules matched: surface a bounded tail so the command
    // does not read as silent breakage. Tail of last 5 frames,
    // capped to a safe slice of the response envelope.
    let tail = waiter_rings
        .tail_frames(probe_id, 5, 4096)
        .unwrap_or(terminal_commander_core::RingTail {
            lines: Vec::new(),
            evicted_frames: 0,
            truncated: false,
        });
    Some(CommandReceipt {
        exit_code,
        lines_suppressed: final_metrics.frames_total,
        tail: tail.lines,
        tail_incomplete: tail.evicted_frames > 0 || tail.truncated,
    })
} else {
    None
};
```

(`RingTail` import: add `RingTail` to the `terminal_commander_core::{...}` use at the top of command.rs, or reference fully-qualified as above.)

- [ ] **Step 3: Store the receipt on the binding**

At the existing `if let Some(b) = waiter_live.write().get_mut(&job_id) { b.metrics = final_metrics; }` (~576-578), extend:

```rust
if let Some(b) = waiter_live.write().get_mut(&job_id) {
    b.metrics = final_metrics;
    b.receipt = receipt;
}
```

- [ ] **Step 4: Return the receipt from `status()`**

In the `status()` builder (~734-747), read the receipt from the binding alongside metrics. Replace the `metrics` read + builder so it captures the receipt:

```rust
let (metrics, receipt) = self
    .live
    .read()
    .get(&job_id)
    .map(|b| (b.metrics.clone(), b.receipt.clone()))
    .unwrap_or_default();
```
and set `receipt,` instead of `receipt: None,` in the returned `CommandStatusResponse`.

Note: `unwrap_or_default()` requires `(ProcessProbeMetrics, Option<CommandReceipt>)` to be `Default`; tuples of `Default` types are `Default`, and `Option` is `Default` (None). `ProcessProbeMetrics` already derives `Default` (used at command.rs:530 via `::default()`). Verify the derive; if missing, use `.unwrap_or((ProcessProbeMetrics::default(), None))`.

- [ ] **Step 5: Compile-check**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo check -p terminal-commanderd"
```
Expected: clean.

- [ ] **Step 6: fmt + clippy**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo fmt -p terminal-commanderd && cargo clippy -p terminal-commanderd --all-targets -- -D warnings"
```
Expected: clean.

- [ ] **Step 7: Commit**

```
git add crates/daemon/src/command.rs
git commit -F <msg-file>
```
Subject: `feat(daemon): compute no-silence exit receipt in lifecycle waiter`
Body: receipt computed pre-lifecycle-bump so rule-match count is exact; emitted only when rule_driven_events == 0 (A1 carve-out); process-only; tail_incomplete on eviction/byte-cap (A6); empty tail -> empty Vec (A7).

---

## Task 4: Surface `receipt` in the MCP `command_status` payload (A4)

**Files:**
- Modify: `crates/mcp/src/tools.rs` (`command_status_payload` ~1366-1381)

- [ ] **Step 1: Add the receipt key to the JSON payload**

In `command_status_payload`, add a `"receipt"` key. `CommandReceipt` is `Serialize`, so `serde_json::to_value` handles `Option` (null when None):

```rust
fn command_status_payload(s: &CommandStatusResponse) -> serde_json::Value {
    serde_json::json!({
        "job_id": s.job_id,
        "bucket_id": s.bucket_id,
        "probe_id": s.probe_id,
        "state": s.state,
        "frames_total": s.frames_total,
        "frames_stdout": s.frames_stdout,
        "frames_stderr": s.frames_stderr,
        "bytes_total": s.bytes_total,
        "events_emitted": s.events_emitted,
        "exit_code": s.exit_code,
        "signal": s.signal,
        "duration_ms": s.duration_ms,
        "receipt": s.receipt,
    })
}
```

- [ ] **Step 2: Compile-check the mcp crate**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo check -p terminal-commander-mcp"
```
Expected: clean. (If the crate name differs, use the name from `crates/mcp/Cargo.toml` `[package] name`.)

- [ ] **Step 3: Commit**

```
git add crates/mcp/src/tools.rs
git commit -F <msg-file>
```
Subject: `feat(mcp): surface no-silence receipt on command_status`

---

## Task 5: Receipt e2e test + TC47 carve-out (A1 contract)

**Files:**
- Test: `crates/daemon/tests/mcp_live_command_e2e.rs` (add receipt tests)
- Modify: `crates/daemon/tests/load_noise_backpressure.rs` (TC47 invariant text + carve-out assertion)

- [ ] **Step 1: Read the existing e2e harness to match its idiom**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && sed -n '1,60p' crates/daemon/tests/mcp_live_command_e2e.rs"
```
Identify: how a command is started (the helper that wraps `command_start_combed`), how it waits for exit (`bucket_wait` / poll on `command_status` until `state` is `Exited`), and how `command_status` is called. Reuse those helpers verbatim — do not invent new ones.

- [ ] **Step 2: Write the failing receipt tests**

Add two tests (mirror the existing harness's start/wait helpers; the snippet below shows intent, not new infrastructure):

```rust
// A no-rule command must return a non-empty, truthful receipt.
#[tokio::test]
async fn no_rule_command_returns_exit_receipt() {
    let h = harness().await; // existing helper
    let job = h.start_combed(&["/bin/sh", "-c", "echo hello; echo world"], &[]).await;
    h.wait_exited(job).await; // existing poll-to-Exited helper
    let status = h.command_status(job).await;
    let receipt = status.receipt.expect("zero-rule run must carry a receipt");
    assert_eq!(receipt.exit_code, Some(0));
    assert_eq!(receipt.lines_suppressed, 2);
    assert_eq!(receipt.tail, vec!["hello", "world"]);
    assert!(!receipt.tail_incomplete);
}

// A command whose output a rule matches must NOT carry a tail
// (A1 carve-out: the "never raw output" contract still holds when
// any rule fires).
#[tokio::test]
async fn rule_match_command_has_no_receipt() {
    let h = harness().await;
    let rule = inline_rule_matching("world"); // existing inline-rule builder
    let job = h.start_combed(&["/bin/sh", "-c", "echo hello; echo world"], &[rule]).await;
    h.wait_exited(job).await;
    let status = h.command_status(job).await;
    assert!(status.receipt.is_none(), "rule match must suppress the tail");
}
```

If the harness exposes only JSON (not typed `CommandStatusResponse`), assert on the JSON shape instead: `status["receipt"]["tail"]` array and `status["receipt"].is_null()`. Match whatever the existing tests in this file do.

- [ ] **Step 3: Run to verify the rule-match test passes and (if logic is right) both pass**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commanderd --test mcp_live_command_e2e receipt"
```
Expected: both PASS. If `no_rule_command_returns_exit_receipt` fails on `lines_suppressed` or tail ordering, inspect actual vs expected — `/bin/sh -c "echo hello; echo world"` yields exactly two stdout frames; adjust ONLY if the probe coalesces lines differently (then assert the real, truthful values).

- [ ] **Step 4: Update the TC47 invariant (A1)**

In `crates/daemon/tests/load_noise_backpressure.rs`, the header comment (~11-13) states only structured sifter events reach the bucket. Amend it to name the carve-out explicitly (text change, plus one assertion that a NOISY no-rule run still does not push raw frames into the BUCKET — the receipt rides on `command_status`, not the bucket):

Comment becomes (preserve surrounding lines):
```rust
// TC47 invariant: only structured sifter events reach the BUCKET.
// Lifecycle events carry argv metadata, not stdout body. The
// TCE-ERG-1 no-silence receipt is the one sanctioned exception and
// it rides on `command_status` (NOT the bucket): a bounded tail is
// returned ONLY when zero rules matched. The bucket itself never
// carries raw stdout.
```

Add an assertion in the existing noisy-no-rule test (or a new small one) that after a noisy no-rule run, the BUCKET contains only the lifecycle event (no raw stdout frames), proving the receipt did not leak raw output into the bucket path.

- [ ] **Step 5: Run TC47**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commanderd --test load_noise_backpressure"
```
Expected: all PASS.

- [ ] **Step 6: Commit**

```
git add crates/daemon/tests/mcp_live_command_e2e.rs crates/daemon/tests/load_noise_backpressure.rs
git commit -F <msg-file>
```
Subject: `test(daemon): receipt e2e + TC47 no-silence carve-out`

---

## Task 6: TCE-ERG-2 agent-selfish descriptions + routing line (text only)

**Files:**
- Modify: `crates/mcp/src/tools.rs` (`command_start_combed` desc ~364-366; `command_status` desc ~395-397; `get_info().with_instructions` ~1102-1104)

- [ ] **Step 1: Read the current instructions + descriptions verbatim**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && sed -n '360,400p;1098,1110p' crates/mcp/src/tools.rs"
```
Capture exact current strings so the edit is surgical.

- [ ] **Step 2: Rewrite the `command_start_combed` description**

Lead with the signal model + agent-selfish pitch + the carve-out. Example target string (adapt to the existing `#[tool(description = "...")]` attribute form):

```
Run a command and return ONLY the signals your rules match, not the
raw stream. You get the matching line(s) plus exit code instead of the
4,800 lines you would otherwise scroll; this lets you run commands
whose output is too big to fit in your context. If zero rules match,
you still get a bounded exit receipt (exit code + suppressed-line
count + a short tail) so a quiet command never reads as broken. No
other stdout/stderr text is returned.
```

- [ ] **Step 3: Rewrite the `command_status` description**

```
Bounded counters + final exit state for a job. Never returns raw
stream text, with one exception: when the command finished and ZERO
rules matched, a bounded exit receipt (exit code, suppressed-line
count, short tail) is included so a no-rule command is never silent.
```

- [ ] **Step 4: Rewrite the server `instructions` (routing line)**

First sentence MUST name the no-output-by-default model; include the agent-selfish pitch and the Bash routing rule. Target:

```
Terminal Commander runs commands and returns STRUCTURED SIGNALS, not
raw output: you define keyword/regex rules and get back only the
matching events plus exit state, so you can run noisy or long-running
commands without flooding your context. This saves you tokens and
scrolling, and lets you run commands too large to read. If no rule
matches, you get a bounded receipt (exit + suppressed count + short
tail), never silence. Use plain Bash instead for tiny, interactive,
or one-off commands where the full output is small and you want it
verbatim; reach for Terminal Commander when output is large, noisy,
long-running, or you only care about specific signals.
```

- [ ] **Step 5: Compile-check**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo check -p terminal-commander-mcp"
```
Expected: clean.

- [ ] **Step 6: Assert the instructions contract in a test**

If `crates/mcp` has a unit test module or an e2e that reads `get_info()`, add a cheap static assertion (A1/ERG-2 falsifiability per review Medium #8):

```rust
#[test]
fn instructions_name_signal_model_and_routing() {
    let info = ToolSurface::test_instance().get_info(); // use existing constructor idiom
    let instr = info.instructions.unwrap();
    assert!(instr.to_lowercase().contains("structured signal"));
    assert!(instr.to_lowercase().contains("bash"));
    assert!(instr.contains("receipt"));
}
```
If no such constructor exists, skip the test and instead grep-assert in CI is out of scope; note it and move on (do not invent a constructor).

- [ ] **Step 7: Commit**

```
git add crates/mcp/src/tools.rs
git commit -F <msg-file>
```
Subject: `feat(mcp): agent-selfish tool descriptions + Bash routing line (TCE-ERG-2)`

---

## Task 7: Full verification pass

- [ ] **Step 1: Workspace fmt + clippy**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings"
```
Expected: clean.

- [ ] **Step 2: Run the touched test suites**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-core -p terminal-commanderd"
```
Expected: all PASS. Watch for the known pre-existing flake `file_search_rejects_empty_query` (120ms file-watch poll) — re-run in isolation if it fails; it is not caused by this work.

- [ ] **Step 3: cargo clean (per global rule, task boundary)**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo clean"
```

- [ ] **Step 4: Report**

Produce the DSPIVR report: objective, changes, files changed, verification commands + results, evidence (a sample receipt JSON for `uname -a`), known gaps (PTY/file-watch receipts deferred; behavioral eval is Phase 3).

---

## Spec coverage check

- TCE-ERG-1 no-silence default -> Tasks 1-5.
- A1 security carve-out -> Task 3 (gate on `rule_driven_events == 0`) + Task 5 (TC47 text + rule-match-no-receipt test) + Task 6 (description text).
- A2 PTY/secret exclusion -> structural (`CommandService` is process-only; documented on `CommandReceipt` and verified by `handle_command_status` routing). No PTY receipt code path is added.
- A3 tail read path -> Task 1.
- A4 delivery surface = `command_status` -> Tasks 3-4.
- A5 metric `lines_suppressed = frames_total - rule_driven_events` (zero-rule => `frames_total`) -> Task 3 (computed pre-lifecycle-bump).
- A6 eviction honesty -> Task 1 (`evicted_frames`, `truncated`) + Task 3 (`tail_incomplete`).
- A7 empty-output -> Task 1 (empty ring -> empty `lines`) + Task 3 (empty `tail` Vec).
- TCE-ERG-2 -> Task 6.

Out of Phase 1 (deferred): TCE-ERG-3 run_and_watch, TCE-ERG-4 teaching errors, TCE-ERG-5 behavioral eval, TCE-ERG-6 merge consolidation.
