# Adversarial review: Agent-Ergonomics Chain spec

**Spec:** `docs/specs/2026-05-26-agent-ergonomics-chain.md`  
**Repo:** `C:\Users\poslj\terminal-commander`  
**Date:** 2026-05-26  
**Method:** Spec + code trace (`process.rs`, `context.rs`, `command.rs`, MCP tools, TC47 tests)

---

## Verdict

**Phase 1 is not safe to build exactly as written.**

The diagnosis (silence kills trust) is right, and **frame data is already retained** in the context ring. Phase 1 is **underspecified** on:

1. **Where** the receipt is surfaced (which MCP/IPC tool).
2. **How** to read a tail (no tail API on `ContextRingManager` today).
3. **Contract conflict** with the live “never raw output” guarantee (TC47 / tool descriptions).
4. **PTY / secret-prompt** tail leakage.

**Before implementation:** amend the spec with a bounded-tail security carve-out, a single delivery surface (prefer `command_status`), `ContextRingManager::tail_frames()`, and receipt semantics for zero-frame runs and ring eviction.

Phase 2 (`run_and_watch`) does **not** block Phase 1’s tail. TCE-ERG-2 (text-only) can land in parallel.

---

## Critical

### 1. Spec contradicts the live “never raw output” contract

MCP tools explicitly promise no stdout/stderr text:

- `crates/mcp/src/tools.rs:364-366` — `command_start_combed`: “No stdout/stderr text is returned.”
- `crates/mcp/src/tools.rs:395-397` — `command_status`: “never returns raw output.”

TC47 encodes the same invariant:

- `crates/daemon/tests/load_noise_backpressure.rs:11-13` — only structured sifter events reach the bucket; lifecycle events carry argv metadata, not stdout body.

Phase 1 introduces **bounded raw tail** without amending TC29/TC47 text or tests. That is the spec’s biggest internal risk: not byte volume (tail can fit `MAX_RESPONSE_BYTES`), but **semantic** — TC becomes “sometimes streams raw lines.”

**Required:** named carve-out (e.g. receipt only when zero rule-driven events), cap (lines + bytes), redaction pass, and TC47/MCP e2e updates.

### 2. PTY / secret-prompt path: tail can leak credentials

All probe paths append every normalized line to the ring before sifting:

- `crates/probes/src/process.rs:268-269` — `rings.append_frame(probe_id, frame.clone())`
- `crates/probes/src/pty.rs:612-613` — same for PTY

A “last 5 lines” receipt after a secret prompt can include password input. The spec does not mention `SecretInputDenied`, redaction, or withholding tail for PTY jobs.

**Required:** Phase 1 scope = process probe only, or suppress tail when secret prompt active, or run tail through rule `redact` machinery.

---

## High

### 3. “Naive last-N from existing ring” — data exists, read API does not

**Frames are not discarded when no rule matches.** They are always appended; sifter only controls `events_emitted`:

```rust
// crates/probes/src/process.rs:268-294
let _ = rings.append_frame(probe_id, frame.clone());
// ...
for draft in runtime.evaluate(&frame, bucket_id) {
    m.events_emitted = m.events_emitted.saturating_add(1);
    sink.emit(draft);
}
```

So `wsl uname -a` **does** retain output in the ring; silence is an **exposure** problem, not retention.

**Gap:** `ContextRingManager` only exposes anchor-based `window()` (`crates/core/src/context.rs:404-415`). No `tail_frames(probe_id, n, max_bytes)`. Lifecycle events have `pointer: None` (`crates/core/src/job.rs:271-272`), so `event_context` cannot supply the receipt without an anchor.

**Phase 1 needs:** e.g. `ContextRingManager::tail_frames` called from the lifecycle waiter (`crates/daemon/src/command.rs:553-574`). The spec’s “no ring buffer required” is wrong: the buffer exists; a **tail read path** is missing.

### 4. Delivery surface unspecified — agents may stay on the 10-call path

Current no-rule UX:

| Tool | What the agent gets |
|------|---------------------|
| `bucket_wait` | Lifecycle `command_exited` (argv in summary, no command body) |
| `command_status` | `frames_total`, `events_emitted`, exit — **no text** (`command.rs:717-743`) |
| `event_context` on lifecycle | Empty frames (`mcp_live_command_e2e.rs:221-224`) |

Spec says “through the MCP surface” but not **which tool**. Receipt-only-in-bucket still forces `bucket_wait` parsing.

**Recommend:** receipt on **`command_status`** (and MCP JSON) and/or lifecycle captures; one golden path documented.

### 5. `frames_suppressed` does not exist; metric is ambiguous

- Spec: “N lines suppressed.”
- `events_emitted` = rule matches only (`process.rs`), not lifecycle bucket events.
- For zero-rule runs: **`suppressed ≈ frames_total`** is workable.
- `BucketSummary.noise_suppressed_count` is always 0 today (`bucket.rs`) — different concept.
- **BACKLOG P1.1** — explicit `frames_suppressed` (`BACKLOG.md:20-35`, `load_noise_backpressure.rs:29-33`).

Define receipt counter as `frames_total - events_emitted` for Phase 1, or block on P1.1.

### 6. Ring eviction makes “last 5 lines” potentially dishonest

Ring caps: 4096 frames / 1 MiB (`context.rs:35-38`). Long runs evict from head (`context.rs:258-266`). Tail may miss early errors. `evicted_frames` exists (`context.rs:191-194`) but is not in the receipt spec.

**Recommend:** receipt flags `evicted_frames > 0` or “tail may be incomplete.”

---

## Medium

### 7. Bet #1 (no-silence): correct problem, wrong “no buffer” claim

Council claim that Phase 1 needs no ring buffer is **misleading**: buffer exists and is populated; need tail API + wire surfacing.

Zero-output (`true`): `frames_total == 0`; receipt must say `exited 0; 0 lines suppressed; tail: (empty)` explicitly.

### 8. Bet #2 (agent-selfish description): right direction, weak falsifiability

- `get_info().with_instructions` still TC40 boilerplate (`tools.rs:1102-1104`).
- Tool descriptions are the real selection surface — agree with council.

**Testability:**

- A/B: old vs new descriptions, N runs, first-tool choice + Bash fallback rate.
- Cheaper CI: static lint on instructions; scripted fixed-prompt agent; human rubric on traces.

Description alone does not fix trust without TCE-ERG-1 — spec correctly couples them.

### 9. Bet #3 (reject “refuse small commands”): **Agree**

Use TCE-ERG-2 routing line (“Bash for tiny/interactive/one-off”) instead of error-driven refusal.

### 10. Bet #4 (TCE-ERG-5 behavioral eval): right north star, poor CI gate

LLM selection-rate over N runs = flaky in CI (model drift, temperature).

**Cheaper CI proxies:**

| Proxy | Measures |
|-------|----------|
| `agent_superiority_bench` floor | Token reduction (committed) |
| Receipt e2e | `uname` → `command_status` has exit + suppressed + non-empty tail |
| Teaching-error test (Phase 2) | One error with full valid example |
| Call-count budget (Phase 2) | Noisy task ≤ K MCP calls |
| Nightly LLM eval (optional) | Selection rate, not merge-blocking until stable |

### 11. Phase ordering

| Item | Depends on |
|------|------------|
| TCE-ERG-1 | `tail_frames` + wire (core + daemon) |
| TCE-ERG-2 | Nothing (mcp text) |
| TCE-ERG-3 | ERG-1 receipt on zero-match |
| TCE-ERG-4 | Independent |
| TCE-ERG-5 | Phases 1-2 shipped |

ERG-1 does not depend on ERG-3/4. ERG-2 parallel.

### 12. Spec gaps (council / author blind spots)

- **Audit / persistence:** tail in bucket/DB = stored raw bytes; retention policy line needed.
- **Policy engine:** no profile flag for “allow no-silence tail.”
- **File watch / directory probes:** same ring pattern; scope boundary needed.
- **WSL:** same daemon; `uname` failure not Windows-specific.
- **`registry_test` force-active** (`server.rs:1040-1042`): test pass ≠ prod activate Draft.
- **TCE-ERG-6:** consolidate `merge_active_and_inline` — good; not blocking ERG-1.
- **`MAX_RESPONSE_BYTES` = 8192** (`protocol.rs:54-55`): tight for 5 long lines + JSON.
- **Shared bucket:** receipt in summaries visible to all bucket readers.

---

## Low

### 13. `registry_test` force-active

No interaction with no-silence (read-only). Minor: “test passed” ≠ “activate Draft in prod.”

### 14. Wire-compat / TC05

ERG-2 text-only. ERG-1 new JSON fields need contract fixtures if IPC shapes version.

---

## Central bets (challenge summary)

| Bet | Assessment |
|-----|------------|
| **1. No-silence default** | **Correct priority.** Data in ring; need tail API + surface + security/PTY amendments. “No ring buffer” wording is wrong. |
| **2. Agent-selfish description** | **High leverage, low cost.** Partially testable; insufficient without receipt. |
| **3. Reject refuse-small-commands** | **Agree**; use routing prose. |
| **4. Trust = behavioral** | **Right goal**; deterministic CI proxies + optional nightly LLM eval. |

---

## Phase 1 build checklist (spec amendments)

1. **Security:** Bounded tail only when no rule-driven events; redaction; PTY/secret exclusion; update TC47 + MCP e2e.
2. **API:** `ContextRingManager::tail_frames(probe_id, max_lines, max_bytes) -> { lines, evicted_frames, truncated }`.
3. **Surface:** Enrich `CommandStatusResponse` + MCP `command_status` with `receipt: { exit, lines_suppressed, tail_lines }`.
4. **Metrics:** `lines_suppressed = frames_total - events_emitted` until P1.1; document lifecycle excluded from `events_emitted`.
5. **Edge cases:** Empty tail for `true`; `evicted_frames` warning.
6. **Tests:** No-rule `uname` asserts non-empty bounded receipt; update `mcp_live_command_e2e`.

---

## Bottom line

Council reorder is sound: **silence first, pitch second, moat later.** Phase 1 is the right wedge, but the spec overstates “no buffer needed,” understates API + contract work, and ignores raw-output invariant conflict and PTY secret risk.

**Amend the spec, then build** — do not implement TCE-ERG-1 verbatim without those changes.

---

## Related

- Spec: `docs/specs/2026-05-26-agent-ergonomics-chain.md`
- Draft-poison fix review: `docs/audits/2026-05-26-draft-poison-fix-review.md`
- BACKLOG P1.1: `frames_suppressed` counter
