# PLAN-TC3 -- command_stop: daemon kill + IPC method + MCP surface (Phase 6a/6b)

**Source:** TC trust-defects campaign (`plan-final.json` Phase 6 / fork F4) +
`review-verdict.json` required amendments #1 (command-waiter terminal-state guard
+ check-then-set stop), #6 (split into Phase 6a/6b; file-count invariant), #8
(audit-before-kill deny-path ordering) + adopted optional improvements (remove
JobBinding Clone derive; probe mutability + anchor refresh; reconcile the stale
daemon_unavailable_envelope module-doc count).
**Posture:** The ONE unavoidably multi-surface item, isolated LAST. New tool/IPC
method `command_stop` / `IpcRequest::CommandStop`. Forced-kill-only (grace
deferred). Policy-gated via the dormant `PolicyAction::CommandSignal`. The kill is
DAEMON-side (MCP forwards an IPC request; guard 1 safe). SPLIT into Phase 6a
(daemon kill capability + IPC method + re-exports + dispatch, behind no MCP tool)
and Phase 6b (MCP tool surface + fixtures + docs -- the atomic count churn).

Language: ASCII only.

---

## Summary table

| Symptom | Location (file:line) | Fix sketch | Effort | Test impact |
|---------|----------------------|------------|--------|-------------|
| No command-job stop: CommandRuntime has no stop/cancel/kill | `crates/daemon/src/command.rs` | Add `CommandRuntime::stop(job_id)`: policy -> existence -> audit -> fire cancel handle -> jobs.cancel | **L** | daemon-side + integration (job -> Cancelled, process gone) |
| JobBinding retains no cancel handle | `crates/daemon/src/command.rs:218-230` | Add `cancel: Option<oneshot::Sender<()>>`; take it from the probe BEFORE the drive_to_exit move | **M** | unit (take_cancel_handle Some-once-then-None) |
| JobBinding derives Clone; oneshot::Sender is not Clone | `crates/daemon/src/command.rs:217` | REMOVE the Clone derive (JobBinding is never whole-cloned; keep Debug) | **S** | compile |
| ProcessProbe cancel handle cannot be retained before the move | `crates/probes/src/process.rs:159` (cancel_tx), `:268` (set), `:296` (cancel) | Add `pub fn take_cancel_handle(&mut self) -> Option<oneshot::Sender<()>>` | **S** | unit (Some once then None) |
| **command waiter has NO terminal-state guard (double-emit)** | `crates/daemon/src/command.rs:668-680`; contrast PTY `pty_command.rs:391-400` | Add a mirror guard: if state already Exited/Failed/Cancelled, metrics-only return (no re-append) | **M** | integration (exactly one cancel/command_exited event) |
| stop() vs natural-exit race (mislabel) | `crates/core/src/job.rs:154-184` (finish), `:187-207` (cancel) -- both unconditional, NO terminal guard | check-then-set under the live/jobs write lock; single critical section decides stop-vs-exit | **M** | integration (Cancelled for sleep 60; terminal AND (Cancelled OR Exited) for may-self-exit) |
| PolicyAction::CommandSignal dormant | `policy.rs:62` (variant), deny matrix `:228-230`, target_path None `:429` | Evaluate it in stop(); deny ordering FIRST; audit subject = peer on deny | **M** | integration (deny under read_only_observer; no existence oracle) |
| No CommandStop IPC method | `crates/ipc/src/protocol.rs`; re-exports ipc/lib.rs:57, daemon ipc/mod.rs:62, daemon lib.rs:65 | Add Params/Response + IpcRequest/IpcResponse arms; classify NON-idempotent; 3 re-export sites | **M** | parity (system_discover_methods_match_dispatch) |
| No MCP command_stop tool; tool count 37 not 38 | `crates/mcp/src/tools.rs`; 5 name lists + 3 count assertions + fixture map + system_discover fixture + minimal_tool_args | Add the #[tool], update ALL atomic anchors in one commit | **L** | catalogue/router/stdio/live-daemon parity green at 38 |

**Estimated files (amendment #6):** ~16 across 6a+6b (NOT a single <=10-file PR --
Phase 6 is explicitly EXEMPT from the <=10 rule; the atomic count-anchor set
forces a single larger atomic surface, split across 6a/6b).

---

## Phase split (amendment #6)

The Phase 6 change list enumerates ~16-17 edited files (process.rs, command.rs,
protocol.rs, ipc/lib.rs re-export, daemon ipc/mod.rs re-export, daemon lib.rs
re-export, server.rs, tools.rs, mcp_stdio.rs, mcp_live_daemon.rs,
daemon_unavailable_envelope.rs, mcp-tool-fixture-map.v1.json, command_stop.v1.json
[new], system_discover.v1.json, TOOL_CONTROL_SURFACE.md, SPEC.md,
RELEASE_CHECKLIST.md). Even excluding the 3 docs the code+test+fixture set is
~13-14, so estimated_files:10 undercounts by ~60%. SPLIT:

- **Phase 6a -- daemon kill capability + IPC method (behind no MCP tool yet):**
  process.rs (take_cancel_handle), command.rs (JobBinding cancel field + Clone
  removal + waiter terminal-state guard + CommandRuntime::stop), protocol.rs
  (CommandStopParams/Response + IpcRequest/IpcResponse + is_idempotent
  classification), the 3 re-export sites (ipc/lib.rs:57, daemon ipc/mod.rs:62,
  daemon lib.rs:65), server.rs (method_name arm, DISCOVERABLE_METHODS, dispatch
  arm, all_request_variants helper, parity cross-check). The parity test compares
  method_name vs DISCOVERABLE_METHODS, NOT vs the MCP tool catalogue, so it stays
  green with the IPC method present and no MCP tool yet.
- **Phase 6b -- MCP tool surface + fixtures + docs (the atomic count churn):**
  tools.rs (#[tool] command_stop, tool_catalogue, the ordered vec + sorted vec,
  the count assertions, module/lib doc counts), mcp_stdio.rs, mcp_live_daemon.rs,
  daemon_unavailable_envelope.rs, mcp-tool-fixture-map.v1.json,
  command_stop.v1.json [new], system_discover.v1.json, TOOL_CONTROL_SURFACE.md,
  SPEC.md, RELEASE_CHECKLIST.md. ALL count anchors update ATOMICALLY in 6b.

---

## Per-item detail

### TC-3 -- no command-job stop

**Symptom:** A started command job cannot be stopped from the MCP surface (only
PTY has `pty_command_stop`). CommandRuntime has no stop/cancel/kill; JobBinding
retains no cancel handle; PolicyAction::CommandSignal is dormant.

**Citations:**

```218:230:crates/daemon/src/command.rs
// JobBinding -- currently retains NO cancel handle; derives Clone (command.rs:217)
```

```159:159:crates/probes/src/process.rs
// cancel_tx: Option<oneshot::Sender<()>> (set :268; pub fn cancel() :296 does cancel_tx.take())
```

```391:400:crates/daemon/src/pty_command.rs
// PTY waiter DOES guard: returns early if state is Exited/Failed/Cancelled (the precedent to MIRROR)
```

```668:680:crates/daemon/src/command.rs
// command waiter unconditionally calls finish/cancel then bucket_append -- NO skip-on-terminal guard
```

```154:207:crates/core/src/job.rs
// finish (:154-184) and cancel (:187-207): unconditional rec.state set + Some(draft), NO terminal guard
```

```62:62:crates/daemon/src/policy.rs
// PolicyAction::CommandSignal dormant (deny matrix :228-230; target_path None :429; no production evaluate() caller)
```

**Fix (Phase 6a -- daemon kill capability):**

1. **take_cancel_handle** (`crates/probes/src/process.rs`): add
   `pub fn take_cancel_handle(&mut self) -> Option<oneshot::Sender<()>>` that
   takes cancel_tx (set :268) so the handle can be retained before the probe
   moves; the existing `pub fn cancel()` at :296 stays for the in-probe forced
   kill.

2. **Probe mutability + anchor refresh (adopted optional):** change
   `let probe = match ProcessProbe::spawn(...)` (command.rs:547) to
   `let mut probe = ...` so `take_cancel_handle(&mut self)` is callable; take the
   handle immediately after the spawn-success arm and store it in the JobBinding
   inserted at command.rs:584. Corrected line anchors: id mint at command.rs:474
   (not :472), spawn-failure return at :551-561 (not :548-560), JobBinding insert
   at :584 (not ~:589), drive_to_exit move at :620 (not ~:606), eviction at the
   cancel/finish branch ~:668-671 (not ~:687, which is the second metrics write).

3. **JobBinding cancel field + Clone removal (adopted optional):** add
   `cancel: Option<oneshot::Sender<()>>` to JobBinding (command.rs:218-230).
   oneshot::Sender is not Clone, so REMOVE the `Clone` derive (command.rs:217) --
   JobBinding is never whole-cloned (rebind and live_jobs read individual
   fields); keep Debug. This pre-empts an implementer reaching for the
   Arc<Mutex<Option<...>>> pattern the plan explicitly avoids (it sidesteps the
   PTY lock-across-await footgun, R2).

4. **command waiter terminal-state guard (amendment #1 -- BLOCKER):** the claim
   that command_stop can reuse "the existing terminal-state idempotency (PTY
   checked-then-skipped pattern)" is FALSE for the command path -- that guard
   exists ONLY in the PTY waiter (pty_command.rs:391-400), and jobs.finish/cancel
   (job.rs:154-207) are themselves non-idempotent (unconditional state set +
   Some(draft) every call). Without a guard, command_stop calling jobs.cancel
   while the command waiter ALSO calls jobs.cancel/finish + bucket_append produces
   TWO lifecycle drafts (doubled bucket event + doubled audit row) and the
   exactly-one-event test FAILS. Required: BEFORE the command waiter builds the
   draft and calls finish/cancel/bucket_append (~command.rs:667-680), add a guard
   mirroring pty_command.rs:391-400 -- if `waiter_jobs.get(job_id)` reports state
   already Exited/Failed/Cancelled, update metrics only and return without
   re-appending. Add crates/daemon/src/command.rs to Phase 6a FOR THE WAITER
   GUARD (not only the cancel-handle field).

5. **CommandRuntime::stop with check-then-set + deny ordering (amendments #1+#8):**
   add `pub fn stop(&self, job_id) -> Result<(BucketId, ProcessProbeMetrics), CommandError>`.
   Required ORDERING (amendment #8 -- no existence oracle, no job_id leak on deny):
   1. **evaluate `PolicyAction::CommandSignal` FIRST;** on Deny, emit a deny audit
      row whose SUBJECT is the peer identity (identity_audit_subject), NOT the
      job_id, and return PolicyDenied WITHOUT touching the live map -- so a denied
      caller learns nothing about whether the job exists and no job_id leaks into
      a sibling-readable audit row (command audit rows embed
      job_id.to_wire_string() into the subject, locally readable via audit_since,
      command.rs:701-708);
   2. only AFTER an Allow verdict, do the live-map lookup (UnknownJob if absent);
   3. emit the job-id-bearing "allow" audit row BEFORE firing the cancel handle.
   **Authoritative terminal-state ownership (amendment #1):** stop() sets
   Cancelled ONLY if the job is not already terminal (check-then-set under the
   live/jobs write lock); a single critical section decides stop-vs-natural-exit
   so a job that self-exits microseconds before the kill is not mislabeled. Then
   take the cancel handle, fire it (forced kill, grace unused), and jobs.cancel
   synchronously, coordinating with the now-guarded lifecycle waiter.
   command_stop on an already-terminal job is a no-op returning the existing
   terminal state, NOT an error.

6. **CommandStop IPC method** (`crates/ipc/src/protocol.rs`): add
   `CommandStopParams{job_id}` + `CommandStopResponse{job_id,bucket_id,counters}`
   (mirror PtyCommandStop), `IpcRequest::CommandStop` + `IpcResponse::CommandStop`;
   classify CommandStop as NON-idempotent in `is_idempotent()` (the Phase 1
   helper) -- mutating; a stop on an already-stopped job is harmless but we do not
   auto-retry mutating RPCs. Re-export the new types through ALL THREE sites:
   crates/ipc/src/lib.rs:57, crates/daemon/src/ipc/mod.rs:62,
   crates/daemon/src/lib.rs:65 (PtyCommandStop re-export sites confirmed ->
   CommandStop needs all three).

7. **dispatch + parity** (`crates/daemon/src/ipc/server.rs`): method_name()
   exhaustive arm (+CommandStop="command_stop"), DISCOVERABLE_METHODS const
   (+"command_stop"), dispatch() arm (handle_command_stop),
   all_request_variants() test helper line, and the
   system_discover_methods_match_dispatch parity cross-check -- ALL atomic or the
   parity test fails both directions.

**Fix (Phase 6b -- MCP surface + fixtures + docs):**

8. **MCP tool** (`crates/mcp/src/tools.rs`): new `#[tool] async fn command_stop`
   forwarding IpcRequest::CommandStop (ensure_daemon_available first; NEVER
   spawns/kills from the mcp crate -- daemon owns the kill, guard 1 safe);
   McpCommandStopParams{job_id}; ADD command_stop to tool_catalogue()
   (status:Live).

9. **ATOMIC count/list updates (the single biggest half-fix trap; all VERIFIED at
   HEAD, 37 -> 38):**
   - `catalogue_lists_thirty_seven_live_tools` -> `_thirty_eight_` (tools.rs:3232)
     and add command_stop in the ORDERED vec (after run_and_watch/command_status,
     family grouping);
   - `tool_router_exposes_all_live_tools` (tools.rs:3292) add command_stop in the
     SORTED vec at its alphabetical slot (VERIFIED sort order:
     command_output_tail < command_start_combed < command_status < command_stop);
   - module/lib doc tool counts (lib.rs:12 "all 37 live tools", tools.rs:29);
   - mcp_stdio.rs + mcp_live_daemon.rs: insert command_stop in the SORTED name
     lists (after command_status) and bump the count string 37->38
     (mcp_live_daemon.rs:213);
   - daemon_unavailable_envelope.rs: bump `assert_eq!(checked, 36 -> 37)` and its
     comment ("expected 37 daemon-backed tools (38 catalogue entries minus
     system_discover)"); add a minimal_tool_args arm for command_stop (job_id-only;
     may reuse the existing command_status|pty_command_stop arm). **Adopted
     optional:** also reconcile the stale module-doc count at
     daemon_unavailable_envelope.rs:12 ("exercises all 30 daemon-backed tools") --
     a //! doc comment (won't fail CI) but a SECOND stale count in the very file
     6b edits; re-label "gated + known prose", not "all".

10. **Fixtures** (`tests/fixtures/contracts/`): add command_stop to
    mcp-tool-fixture-map.v1.json live_tools[] and daemon_unavailable_shapes; bump
    counts live_tools 37->38 and covered_live 33->34 (VERIFIED current values);
    add mcp-tools/command_stop.v1.json (copy the pty_command_stop.v1.json shape);
    add command_stop to mcp-tools/system_discover.v1.json tools[].

11. **Docs:** docs/mcp/TOOL_CONTROL_SURFACE.md -- add command_stop to the Commands
    group + note forced-kill-only semantics + CommandSignal policy gating; bump
    the stale "32 live tools" line (TOOL_CONTROL_SURFACE.md:61) to the real count
    (38). SPEC.md amendment recording command_stop with a SPEC-8/SPEC-13
    cross-reference (the runtime contract per SPEC-13 governs). RELEASE_CHECKLIST.md
    -- reconcile the stale "29-tool unchanged" line (:312, and the >=29/29-tool
    references at :61, :71). Do NOT chase the non-gated README stale references (a
    full doc-count sweep is DEFERRED).

**Effort:** L. **Test:**
- integration through daemon IPC (MANDATORY for a new tool, TEST socket): start a
  long-running command, command_stop it, assert the job transitions to Cancelled,
  command_status reflects it, the process is actually killed, and exactly one
  cancel/command_exited event (waiter does not double-emit). **Self-exit-race
  tolerance (amendment #1):** assert terminal AND (Cancelled OR Exited) when
  stopping a may-self-exit job; assert STRICT Cancelled only for a
  guaranteed-long-running job (e.g. sleep 60). source-status: live.
- integration (amendment #8): command_stop under read_only_observer is DENIED
  (PolicyDenied) with an audit row emitted BEFORE any kill; the deny audit subject
  is the PEER and does NOT contain the job_id wire string; stop() returns the SAME
  error for a live job_id and a bogus job_id (no existence oracle). source-status:
  live.
- daemon-side: CommandRuntime::stop fires the retained cancel handle and
  jobs.cancel sets Cancelled; the lifecycle waiter does not double-cancel (race
  test). source-status: live.
- unit (crates/probes): take_cancel_handle returns Some once then None.
  source-status: test-only.
- contract: fixture_catalogue_contract passes (set + dir equality);
  catalogue/router/stdio/live-daemon parity green at 38;
  system_discover_methods_match_dispatch parity green; daemon_unavailable_envelope
  checked==37. source-status: live/test-only.
- Windows + unix: kill path under both cfg gates. source-status: live.

---

## Invariants (Phase 6a/6b)

- Adding command_stop updates ALL atomic anchors in one commit (6b): 5 name lists
  (tools.rs ordered vec, tools.rs sorted vec with command_stop after
  command_status, mcp_stdio.rs sorted, mcp_live_daemon.rs sorted,
  system_discover.v1.json tools[]) + 3 count assertions
  (daemon_unavailable_envelope checked 36->37 + comment, catalogue test name
  thirty_seven->thirty_eight, mcp_live_daemon count string) + fixture map
  (live_tools 37->38, covered_live 33->34) + system_discover fixture +
  minimal_tool_args. Half-fix = CI red.
- The new IPC method keeps the 6+ sync surfaces mutually consistent
  (IpcRequest/IpcResponse + Params/Response, method_name exhaustive match,
  DISCOVERABLE_METHODS, dispatch arm, all_request_variants helper, 3 re-exports)
  or the parity test fails both directions.
- The command waiter has a terminal-state guard mirroring pty_command.rs:391-400;
  stop() uses check-then-set under the live/jobs write lock; a single critical
  section decides stop-vs-natural-exit.
- Deny ordering: policy FIRST; deny audit subject = peer (not job_id); live-map
  lookup only AFTER Allow; no existence oracle.
- The kill is DAEMON-side; MCP guard 1 forbids Command/spawn/TcpListener/UdpSocket
  in crates/mcp/src -- command_stop forwards an IPC request.
- CommandStop is classified NON-idempotent in the Phase 1 is_idempotent() helper.
- Phase 6 is EXEMPT from the <=10-file invariant (amendment #6); the atomic
  count-anchor set forces a single larger atomic surface split across 6a/6b.
- Grace window deferred (forced-kill-only, process.rs:47) -- tracked in BACKLOG.
  Retrofitting policy-gating onto the ungated pty_command_stop -- DEFERRED.
- Release side-effect: a feat:/fix: commit triggers release-please; the
  tool-surface change breaks the stale "29-tool unchanged" release-doc invariant
  -- reconcile in 6b and coordinate the merge with the operator (NO push/merge
  without approval).
- No fake success: command_stop performs a REAL kill verified via runtime_state.

## Verification (Phase 6a/6b)

- `wsl bash scripts/linux-gate.sh` (MCP guard 1: command_stop forwards to the
  daemon; guard 2: no fs; the system_discover parity test; nextest --workspace
  green with ALL 5 name lists + 3 count assertions consistent at 38).
- `pwsh -File scripts/windows-gate.ps1` (kill path cfg-split;
  windows_spawn_site_coverage + windows_no_console_spawn stay >=1).
- `cargo nextest run --workspace` (parity + catalogue + fixture_catalogue_contract
  + daemon_unavailable_envelope all green).
- manual: command_stop a live TEST-socket job and confirm via runtime_state it is
  Cancelled and the process is gone; confirm ALL atomic anchors updated together
  (half-fix = CI red).
