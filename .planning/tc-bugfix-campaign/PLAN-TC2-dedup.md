# PLAN-TC2 -- Narrow nonce-keyed in-flight dedup (Phase 2)

**Source:** TC trust-defects campaign (`plan-final.json` Phase 2 / fork F1 dedup)
+ `review-verdict.json` required amendment #7 (plumb dedup_nonce end-to-end; new
Arc<Mutex<HashMap>> field, NOT behind the live lock) + adopted optional
improvements (peer-scoped fallback keying OR nonce-only; adapter ALWAYS generates
a per-call nonce).
**Posture:** Defense-in-depth, STATEFUL daemon behavior, deliberately SEPARATE
from the urgent pure-logic Phase 1 (unanimous scope-discipline note). Phase 1
removes the AUTOMATIC double-spawn; the residual path is a MANUAL caller/LLM
re-call of a timed-out mutating start. This phase closes it with a narrow
daemon-side in-flight dedup guard and no protocol/version change. Hardened
against the judge-flagged "collapse two legitimately-distinct rapid runs" race.

Language: ASCII only.

---

## Summary table

| Symptom | Location (file:line) | Fix sketch | Effort | Test impact |
|---------|----------------------|------------|--------|-------------|
| No idempotency/dedup guard anywhere in the daemon | `crates/daemon/src/command.rs` start_combed (no pre-spawn lookup) | Add a short-TTL in-flight fingerprint map checked at the TOP of start_combed; duplicate-in-window returns the SAME (job_id,bucket_id) | **M** | integration (two identical -> one job) |
| dedup map cannot live behind the live lock | `crates/daemon/src/command.rs:610` (`waiter_live = Arc::clone(&self.live)`) | NEW `Arc<Mutex<HashMap<u64,(JobId,BucketId,Instant)>>>` field on CommandRuntime, cloned into the waiter closure | **M** | integration (eviction on completion) |
| dedup_nonce silently dropped: start_combed receives CommandStartRequest, not CommandStartParams | `crates/daemon/src/command.rs:572-581` (builds from req: CommandStartRequest); `handle_command_start_combed` hand-builds the request | Add dedup_nonce to CommandStartRequest AND thread `params.dedup_nonce.clone()` in handle_command_start_combed | **S** | round-trip test: a nonce sent over IPC is OBSERVED by start_combed |
| no wire field for the dedup hint | `crates/ipc/src/protocol.rs` (CommandStartParams) | Add OPTIONAL `#[serde(default, skip_serializing_if="Option::is_none")] dedup_nonce: Option<String>` (additive) | **S** | unit (absent nonce decodes; old payload round-trips) |
| naive fingerprint collapses two distinct rapid runs | dedup keying choice | Nonce-preferred keying; peer-scoped OR nonce-only fallback; very short window | **M** | integration (RACE never-collapse + never-block) |
| fingerprint leaks and blocks a legitimate re-run | completion paths (exit/cancel/spawn-failure) | Evict on EVERY completion path; TTL backstop | **S** | integration (eviction-on-spawn-failure) |

**Estimated files:** 5 (amendment #7 bumps from 4): `crates/daemon/src/command.rs`
(dedup map field + CommandStartRequest field + eviction), `crates/ipc/src/protocol.rs`
(CommandStartParams.dedup_nonce), `crates/daemon/src/ipc/handlers/command.rs`
(handle_command_start_combed plumbing), `crates/mcp/src/tools.rs` (adapter nonce
generation), plus the test module.

---

## Per-item detail

### TC-2 -- no in-flight dedup; the manual-retry double-spawn window

**Symptom:** After Phase 1, a deliberate caller/LLM re-call of a timed-out
mutating start still spawns a second identical job; nothing on the daemon side
collapses a duplicate in-flight start. `RequestEnvelope` has no key;
`start_combed` has no pre-spawn lookup.

**Citations:**

```572:581:crates/daemon/src/command.rs
// start_combed builds JobConfig from req: CommandStartRequest (the INTERNAL struct)
```

```584:592:crates/daemon/src/command.rs
// JobBinding insert; start_combed operates on internal types, NOT CommandStartParams
```

```610:610:crates/daemon/src/command.rs
let waiter_live = Arc::clone(&self.live); // the established capture pattern the new dedup map must mirror
```

```541:544:crates/daemon/src/subscriptions/pull.rs
// (unrelated reference: pull offset commit -- cited in Phase 1 for SubscriptionPull)
```

**Fix:**

1. **New dedup map field (amendment #7):** drop the "behind the existing live
   lock" option -- the live lock is `Arc<RwLock<HashMap<JobId, JobBinding>>>` with
   a different key/value type. Add a NEW
   `Arc<Mutex<HashMap<u64,(JobId,BucketId,Instant)>>>` field on CommandRuntime,
   cloned into the lifecycle waiter closure (`let waiter_dedup = Arc::clone(&self.dedup);`)
   alongside `waiter_live` so eviction can run on the completion paths. Uses std
   hashing (zero new dep).

2. **Check at the TOP of start_combed:** before the id mint, look up the
   fingerprint. A duplicate within the window returns the SAME (job_id,bucket_id)
   instead of spawning (the REAL existing job_id -- no fake success).

3. **Keying (amendment + adopted optional):**
   - PREFER a client-supplied request nonce (`dedup_nonce`). Distinct nonces
     never collapse.
   - **Resolve the load-bearing mechanism (adopted optional):** the adapter
     ALWAYS generates a per-call nonce (so a genuine LLM re-issue that copies
     nothing gets a NEW nonce => distinct jobs; only an exact retry that reuses
     the nonce collapses). Default `None` from an old client still decodes.
   - **Peer-scoped fallback (adopted optional, policy-security major):** if a
     low-entropy argv+cwd+tag fallback is kept, key it on
     (peer_uid/peer_sid, argv, cwd, tag), NOT (argv, cwd, tag) alone -- so a
     sibling local client cannot guess another client's about-to-run command and
     receive the SAME live (job_id,bucket_id) (a cross-client live-handle
     disclosure it could command_stop or subscribe to). dispatch() already has
     PeerIdentity in scope (server.rs:825). If threading peer identity is too
     invasive, DROP the argv+cwd+tag fallback and dedup ONLY on the explicit
     client nonce (which a client only knows for its own request).

4. **Thread dedup_nonce end-to-end (amendment #7 -- else silently dropped):**
   - add `dedup_nonce: Option<String>` to `CommandStartRequest`
     (`crates/daemon/src/command.rs`);
   - thread `dedup_nonce: params.dedup_nonce.clone()` in
     `handle_command_start_combed` (`crates/daemon/src/ipc/handlers/command.rs`)
     where the request is hand-built field-by-field;
   - add `#[serde(default, skip_serializing_if="Option::is_none")] dedup_nonce: Option<String>`
     to `CommandStartParams` (`crates/ipc/src/protocol.rs`, additive,
     non-breaking).

5. **Evict on EVERY completion path:** normal exit (lifecycle waiter), cancel,
   AND the spawn-failure early-return. A leaked entry must never block a
   legitimate re-run; the TTL is the backstop, eviction-on-completion is primary.

**Effort:** M. **Test:**
- integration (crates/daemon, own socket+data dir): two identical
  CommandStartCombed within the TTL window (same nonce, or same peer-scoped
  argv+cwd+tag) return the SAME job_id (deduped, one process). source-status: live.
- integration (RACE / never-collapse, the judge-flagged correctness guard): two
  legitimately-DISTINCT rapid identical-argv runs with DISTINCT nonces spawn TWO
  distinct jobs; a third run AFTER the first completes (fingerprint evicted)
  spawns a fresh job (never blocked). source-status: live.
- integration (never-collapse, NO-NONCE case -- adopted optional): if the
  fallback window is retained, prove two distinct same-signature runs from the
  SAME peer within the window behave per the documented mechanism (and that the
  adapter's always-generated nonce makes production distinct). source-status: live.
- integration (eviction-on-spawn-failure): a start that fails to spawn evicts its
  fingerprint so an immediate legitimate retry is not blocked. source-status: live.
- round-trip (amendment #7): a nonce sent over IPC is OBSERVED by start_combed's
  dedup path (not merely that CommandStartParams decodes it). source-status: live.
- unit (crates/ipc): CommandStartParams with absent dedup_nonce decodes (serde
  default); old payload round-trips. source-status: test-only.

---

## Invariants (Phase 2)

- TC-2 dedup NEVER silently collapses two legitimately-distinct rapid identical
  runs: nonce-preferred keying (distinct nonces => distinct jobs), a very short
  peer-scoped argv+cwd+tag fallback window (or nonce-only), eviction on EVERY
  completion path, and explicit never-collapse + never-block tests.
- The dedup map is a NEW Arc<Mutex<HashMap>> field captured into the waiter
  closure, NOT the live RwLock (amendment #7).
- dedup_nonce is plumbed CommandStartParams -> CommandStartRequest ->
  handle_command_start_combed -> start_combed; a missing link silently drops it
  (the fake-success class this campaign guards against).
- dedup_nonce is additive serde(default) -- no wire break, no protocol version
  bump.
- This is NOT a server-honored idempotency-key protocol (no TTL store, no
  envelope change) -- just an in-flight collapse hint. The full envelope key is
  DEFERRED (R-07, BACKLOG).
- No fake success: a deduped duplicate returns the REAL existing job_id.

## Verification (Phase 2)

- `wsl bash scripts/linux-gate.sh` (fmt + clippy -D + nextest + TC47 load gate +
  MCP guards; dedup is daemon-side, mcp/src untouched except the nonce
  passthrough which adds no spawn/fs/socket).
- `pwsh -File scripts/windows-gate.ps1` (start_combed spawn path is cfg-split;
  windows_spawn_site_coverage stays >=1).
- `cargo nextest run -p terminal-commander-daemon -p terminal-commander-ipc`.
- manual: on a TEST socket, fire two identical starts within the window and
  confirm one job in runtime_state; fire two with distinct nonces and confirm two
  jobs.
