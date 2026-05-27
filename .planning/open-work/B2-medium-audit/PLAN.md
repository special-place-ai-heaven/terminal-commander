# B2 — Medium audit (M1–M8)

**Source:** `docs/audits/2026-05-27-full-spectrum-flakiness-fragility-audit.md` (MEDIUM section)  
**Posture:** Opportunistic — fix when touching related code; batch test hygiene in one PR if desired.

---

## Summary table

| ID | Symptom (verified) | Location (actual lines) | Fix sketch | Effort | Test impact |
|----|-------------------|-------------------------|------------|--------|-------------|
| M1 | Pipe test names use `process::id()` only | `pipe_accept_loop.rs:43`, `pipe_peer_identity.rs:46` | Add atomic counter / `test::name()` suffix to pipe name format | **S** | Prevents future same-file collision |
| M2 | Fixed sleeps before assert on child output | `pty_ipc.rs:106,327`, `file_ipc.rs:369`, `ipc_command.rs:114,214`, `load_noise_backpressure.rs:1065`; audit also cited `ipc_bucket.rs` **40/50ms** at `:122,254,366` (not 400ms) | Poll bucket cursor / `command_status` / metrics until ready or timeout | **M** | Reduces CI flake under load |
| M3 | Real 40–60ms wall-clock bucket waits | `bucket.rs:888-904`, `944-945` (+ 20ms park `927`) | `tokio::time::pause()` / `advance()` in tests | **M** | Virtual time; no host timing |
| M4 | probe→kill TOCTOU on daemon identity | **CLOSED (verified 2026-05-28):** `replace.rs:234` `pid_belongs_to_daemon(pid,&state_dir)` before `hard_kill` | **Close.** Add one recycled-pid-refused regression test only; NO new kill logic | **0 / S** | Regression test for F3 fix |
| M5 | `WAIT_FAILED` treated as success | `update_locks.rs:205-206` `Ok(wait != WAIT_TIMEOUT)` | Match `WAIT_OBJECT_0` only; log other codes | **S** | Windows unit test with mock if feasible |
| M6 | `Debug` variant in user-facing daemon error | `tools.rs:376-378` `format!("unexpected response variant: {other:?}")` | Stable code e.g. `unexpected_variant` + structured field | **S** | MCP unit test on malformed response |
| M7 | Native spawn inherits full `process.env` | `terminal-commander.js:270-273` no `env` key | Align with `filtered_env` / document intentional inherit | **S–M** | JS test spawn env keys |
| M8 | CLI integration inherits ambient `TC_SOCKET` | `offline_truth.rs:24-27` no env scrub | `Command::env_clear()` + minimal env or set bogus `TC_SOCKET` | **S** | Stable exit 69 in dev envs |

---

## Per-item detail

### M1 — Pipe-name helpers PID-only collision

**Symptom:** `format!(r"\\.\pipe\tc-test-accept-{}", std::process::id())` — second test in same process could collide if prefixes match.

**Citations:**

```43:43:crates/daemon/tests/pipe_accept_loop.rs
    let pipe_name = format!(r"\\.\pipe\tc-test-accept-{}", std::process::id());
```

```46:46:crates/daemon/tests/pipe_peer_identity.rs
    let pipe_name = format!(r"\\.\pipe\tc-test-peer-id-{}", std::process::id());
```

**Fix:** `static NEXT: AtomicU32` or include `line!()` / test name hash in suffix (pattern used elsewhere for data dirs per audit).

**Effort:** S · **Test:** add second test in file to prove no collision.

---

### M2 — Fixed pre-assert sleeps

**Symptom:** Tests sleep 400–800ms then assert; under loaded CI, child output may not be ready.

**Verified locations:**

| File | Lines | Sleep |
|------|-------|-------|
| `pty_ipc.rs` | 106, 327 | 800ms, 500ms |
| `file_ipc.rs` | 369 | 800ms |
| `ipc_command.rs` | 114, 214 | 400ms |
| `load_noise_backpressure.rs` | 1065 | 400ms |
| `ipc_bucket.rs` | 122, 254, 366 | 40–50ms (lower risk but same pattern) |

**Fix sketch:** Loop with deadline: `command_status`, `bucket_events_since`, or probe metrics until predicate or 5s timeout.

**Effort:** M · **Test impact:** Tests may run slightly longer worst-case but more reliable.

**Ledger drift:** Audit listed `pty_ipc.rs:388` — **no 388ms sleep at :388** in current tree (UNKNOWN line; issue at 327).

---

### M3 — Core bucket real timeouts

**Symptom:** `Duration::from_millis(40|50|60)` in async tests without time control.

**Citations:** `crates/core/src/bucket.rs:888-904`, `944-945`.

**Fix:** Enable paused clock in `rt()` test helper; `advance` past timeout boundaries.

**Effort:** M · **Test impact:** Faster, deterministic CI.

---

### M4 — replace probe→kill TOCTOU

**Ledger:** Open TOCTOU same family as F3.

**Current code:** Re-verify immediately before kill:

```230:242:crates/supervisor/src/replace.rs
    // Re-verify at kill time that `pid` is still OUR daemon. Closes the
    // pid-reuse TOCTOU (F3): ...
    if !pid_belongs_to_daemon(pid, &opts.state_dir) {
        return ReplaceOutcome::Skipped { ... };
    }
```

Audit status table marks F3 **SHIPPED** (`a1465b5`). **Recommend:** close M4 with regression test, not new kill logic.

**Effort:** S (test only) or **0** (close).

---

### M5 — WaitForSingleObject WAIT_FAILED

**Symptom:** After `TerminateProcess`, any wait result except `WAIT_TIMEOUT` returns `Ok(true)`.

```205:206:crates/cli/src/update_locks.rs
        let wait = unsafe { WaitForSingleObject(proc.0, 2000) };
        Ok(wait != WAIT_TIMEOUT)
```

**Fix:** `Ok(wait == WAIT_OBJECT_0)` and map failures to `Err` or `Ok(false)` with log.

**Effort:** S · Diagnostic path only (post-terminate).

---

### M6 — system_discover Debug leak

```374:378:crates/mcp/src/tools.rs
            Ok(other) => (
                None,
                Some(format!("unexpected response variant: {other:?}")),
            ),
```

**Fix:** `unexpected_ipc_response` stable string; log full debug at `tracing::warn!`.

**Effort:** S.

---

### M7 — admin shim full env

```270:273:packages/terminal-commander/bin/terminal-commander.js
    const child = spawn(result.binaryPath, args, {
      stdio: "inherit",
      shell: false,
    });
```

Contrast: `terminal-commander-mcp.js:41-44` passes `env: process.env` explicitly (same effect).

**Fix options:** (a) use `buildFilteredEnv()` from `lib/wsl/filtered_env.js` if appropriate for native; (b) document that native CLI intentionally inherits for operator tooling.

**Effort:** S–M depending on policy.

---

### M8 — offline_truth ambient env

```24:27:crates/cli/tests/offline_truth.rs
        let output = terminal_commander()
            .args(args)
            .output()
```

Child inherits `TC_SOCKET` / live daemon → false negatives on exit code 69.

**Fix:**

```rust
terminal_commander()
    .env_remove("TC_SOCKET")
    .env_remove("TC_SESSION")
    .env("TC_SOCKET", "/nonexistent/terminal-commander-offline-test.sock")
```

**Effort:** S.

---

## Suggested batching

1. **PR A (tests):** M8 + M3 + M1  
2. **PR B (tests):** M2 (file-by-file)  
3. **PR C (product):** M6 + M5 + M7  
4. **M4:** close with test or audit doc update only
