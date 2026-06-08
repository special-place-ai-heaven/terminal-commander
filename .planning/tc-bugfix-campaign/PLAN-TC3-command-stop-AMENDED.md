# PLAN-TC3 (AMENDED 2026-06-08) -- command_stop (Phase 6a/6b)

**Supersedes** `PLAN-TC3-command-stop.md` (adversarially-reviewed original; kept for
lineage). Amendment scope: (1) **anchor on SYMBOLS, not lines** (Phase 2 shifted
command.rs ~+90-100; Phase 3 shifted tools.rs tests +316; Phases 4/5 WILL shift
more before this phase runs); (2) correct the terminal-guard insertion point;
(3) record the SECOND tools.rs count string + precise current count values. No
change to the original design intent or amendments #1/#6/#8.

## Drift convention (READ FIRST)

This is the LAST phase -- by the time it runs, Phases 4, 5, 6a will have shifted
command.rs / protocol.rs / server.rs / tools.rs AGAIN. Line numbers below are
`as_of e76ebdc` HINTS. **Anchor on the named symbol; re-resolve with SymForge at
phase start** and re-verify each claim before editing. If a count baseline changed
(e.g. Phase 4 or 5 added a tool), RE-COUNT before the 37->38 churn.

## Re-anchor table (verified as_of e76ebdc)

| # | anchor SYMBOL | file | as_of line | claim |
|---|---------------|------|-----------|-------|
| 1 | `struct JobBinding` + Clone derive | daemon/command.rs | derive 230, struct 231-244 | derives `Debug, Clone`; fields metrics/sifter/inline_rules/bucket_id/probe_id/receipt; NO cancel handle; stored `Arc<RwLock<HashMap<JobId,JobBinding>>>`; never whole-cloned -> Clone removable |
| 2 | `ProcessProbe::spawn` match in `start_combed` | daemon/command.rs | 636 (`let probe = match`) | immutable `let probe`; change to `let mut probe` |
| 3 | live-map insert (JobBinding literal) | daemon/command.rs | 676 (literal 677-684) | insert site for the cancel field |
| 4 | `drive_to_exit(probe).await` | daemon/command.rs | 718 (in lifecycle `async move` closure ~717-815) | the move; take cancel handle BEFORE this |
| 5 | job-id mint / spawn-failure return | daemon/command.rs | mint 540; spawn-fail `return Err(CommandError::Spawn(e))` 651 (Phase 2 dedup evict at 645) | |
| 6 | command lifecycle WAITER | daemon/command.rs | draft `let draft = match outcome{finish/cancel}` 766-769; bucket_append 781-782; exit-audit 807 | **NO terminal-state guard** (unconditional). Phase 2 dedup evict at 777 is BETWEEN draft and append -- unrelated. **Guard goes BEFORE 766** (after receipt-publish ~761-763) |
| 7 | command audit subject = job_id.to_wire_string() | daemon/command.rs | start-allow audit 687-693; exit audit 807 | two sites embed job_id wire string -> deny must not leak it |
| 8 | `cancel_tx` / set / `pub fn cancel()` | probes/process.rs | field 159; set 268; `cancel()` 296-300 (`take()` 297) | NO drift; no `take_cancel_handle` yet |
| 9 | PTY waiter terminal guard (PRECEDENT) | daemon/pty_command.rs | 391-400 (in `fn start` 270-468) | `if waiter_jobs.get(job_id).is_some_and(|r| matches!(r.state, Exited\|Failed\|Cancelled)) { return; }` -- mirror this |
| 10 | `fn finish` / `fn cancel` | core/job.rs | finish 154-184; cancel 187-207 | NO drift; both unconditional state set + Some(draft), no guard |
| 11 | `PolicyAction::CommandSignal` | daemon/policy.rs | variant 62; deny arm 230 (ReadOnlyObserver matches 226-233); path-subject arm 429 (action_path_subject 424-437) | NO drift; DORMANT (find_references = 3, all in policy.rs, no evaluate() caller) |
| 12 | PtyCommandStop shapes + is_idempotent | ipc/protocol.rs | Params 1414-1416; Response 1419-1427; IpcRequest::PtyCommandStop 265; IpcResponse 449; `is_idempotent` 326-393 (PtyCommandStop NON-idempotent at 334) | mirror for CommandStop; classify NON-idempotent. No CommandStop exists |
| 13 | 3 re-export sites | ipc/lib.rs 57; daemon ipc/mod.rs 62; daemon lib.rs 65 | NO drift | CommandStop needs all three |
| 14 | server.rs parity surfaces | daemon/ipc/server.rs | method_name 414-455 (Pty arm 442); DISCOVERABLE_METHODS 463-502 (490); dispatch 505-771 (Pty arm 682); all_request_variants 878-1038 (999); parity test `system_discover_methods_match_dispatch` 1047-1078 | parity compares method_name vs DISCOVERABLE_METHODS (NOT MCP catalogue) -> 6a stays green with IPC method + no MCP tool |
| 15 | catalogue test | mcp/tools.rs | `catalogue_lists_thirty_seven_live_tools` 3548-3605; ordered vec 3556-3594 (37) | +316 drift. command family: command_start_combed/run_and_watch/command_status/command_output_tail |
| 16 | router test | mcp/tools.rs | `tool_router_exposes_all_live_tools` 3608-3658; sorted vec 3618-3656 (37) | +316. **command_stop sorts BETWEEN command_status and event_context** |
| 17 | module/lib doc counts | mcp/lib.rs 12; mcp/tools.rs **20 AND 29** | "37 live tools" | **TWO strings in tools.rs (20 + 29)** -- plan flagged only 29; bump BOTH 37->38 |
| 18 | `tool_catalogue()` | mcp/tools.rs | 109-297 | live count = 37 (verified 4 ways) |
| 19 | mcp_stdio sorted list | mcp/tests/mcp_stdio.rs | vec 73-111 (37) | no numeric assert; slot between command_status(82) and event_context(83) |
| 20 | mcp_live_daemon list + count | mcp/tests/mcp_live_daemon.rs | sorted vec 100-138; `assert_eq!(live_count,37)` 215 (comment 216) | two edit sites |
| 21 | daemon_unavailable_envelope | mcp/tests/daemon_unavailable_envelope.rs | `assert_eq!(checked,36)` 215-217; minimal_tool_args arm 134 (`command_status\|pty_command_stop\|command_output_tail`); module-doc 12 says **"30"** (stale) | checked 36->37; reuse arm 134; module-doc 30->37 |
| 22 | fixture map counts | tests/fixtures/contracts/mcp-tool-fixture-map.v1.json | live_tools 37 (247); covered_live 33 (248); missing_fixture 4 (250); daemon_unavailable_shapes 24-60 (37) | live 37->38; covered_live 33->34; add command_stop to live_tools[] + daemon_unavailable_shapes |
| 23 | system_discover fixture | tests/fixtures/contracts/mcp-tools/system_discover.v1.json | tools[] 28-65 (37); pty_command_stop template at 56 | catalogue-ordered; add command_stop near command_status |

## Phase 6a -- daemon kill + IPC method (behind NO mcp tool)

Files: probes/process.rs, daemon/command.rs, ipc/protocol.rs, 3 re-exports
(ipc/lib.rs, daemon ipc/mod.rs, daemon lib.rs), daemon/ipc/server.rs.

1. **`take_cancel_handle`** on ProcessProbe: `pub fn take_cancel_handle(&mut self) -> Option<oneshot::Sender<()>>` taking `cancel_tx`; keep `cancel()` for in-probe kill.
2. **Probe mutability:** `let mut probe = match ProcessProbe::spawn(...)` (#2); take the handle in the spawn-success arm; store it in the JobBinding inserted at #3.
3. **JobBinding cancel field + Clone removal:** add `cancel: Option<oneshot::Sender<()>>` (#1); REMOVE `Clone` derive (oneshot::Sender not Clone; never whole-cloned); keep Debug.
4. **Command waiter terminal-state guard (amendment #1, BLOCKER -- CORRECTED SITE):**
   insert a guard mirroring pty_command.rs:391-400 **immediately after the
   receipt-publish block (~command.rs:761-763) and BEFORE `let draft = match outcome`
   (~:766)** -- if `waiter_jobs.get(job_id)` is already Exited/Failed/Cancelled,
   update metrics only and return without building a draft / bucket_append. NOTE:
   Phase 2's `waiter_dedup.lock().remove()` at ~:777 is unrelated (dedup cleanup,
   not a lifecycle guard) -- do not conflate.
5. **`CommandRuntime::stop` (amendments #1+#8):**
   `pub fn stop(&self, job_id) -> Result<(BucketId, ProcessProbeMetrics), CommandError>`.
   ORDER: (1) evaluate `PolicyAction::CommandSignal` FIRST; on Deny emit a deny
   audit row whose SUBJECT is the peer identity (NOT job_id) and return PolicyDenied
   WITHOUT touching the live map (no existence oracle, no job_id leak); (2) only
   after Allow, live-map lookup (UnknownJob if absent); (3) emit the job-id-bearing
   allow audit BEFORE firing the cancel handle. check-then-set Cancelled only if not
   already terminal, under the live/jobs write lock (single critical section decides
   stop-vs-natural-exit). Already-terminal = no-op returning the terminal state.
6. **CommandStop IPC** (protocol.rs): `CommandStopParams{job_id}` +
   `CommandStopResponse{job_id,bucket_id,counters}` (mirror PtyCommandStop);
   IpcRequest::CommandStop + IpcResponse::CommandStop; classify NON-idempotent in
   is_idempotent (#12). Re-export via all 3 sites (#13).
7. **dispatch + parity** (server.rs, #14): method_name arm, DISCOVERABLE_METHODS,
   dispatch arm (handle_command_stop), all_request_variants helper, parity test --
   ALL atomic. (Parity is method_name vs DISCOVERABLE_METHODS, so 6a is green with no
   MCP tool yet.)

## Phase 6b -- MCP tool + fixtures + docs (atomic count churn)

8. **MCP tool** (tools.rs): `#[tool] async fn command_stop` forwarding
   IpcRequest::CommandStop (ensure_daemon_available; daemon owns the kill -> MCP
   guard 1 safe); McpCommandStopParams{job_id}; add to tool_catalogue (Live).
9. **ATOMIC 37->38 (RE-COUNT first if Phase 4/5 added a tool):**
   - catalogue test (#15): rename `_thirty_seven_`->`_thirty_eight_`; add command_stop
     to ordered vec (after command_status, family grouping);
   - router test (#16): add command_stop to sorted vec BETWEEN command_status and
     event_context;
   - doc counts: mcp/lib.rs:12 AND **mcp/tools.rs:20 AND :29** (TWO strings);
   - mcp_stdio.rs (#19) + mcp_live_daemon.rs (#20: vec + `37`->`38` assert + comment);
   - daemon_unavailable_envelope.rs (#21): `checked` 36->37 + comment; add command_stop
     minimal_tool_args via the :134 arm; module-doc :12 "30"->"37";
10. **Fixtures** (#22, #23): fixture-map live_tools 37->38, covered_live 33->34, add
    command_stop to live_tools[] + daemon_unavailable_shapes; new
    mcp-tools/command_stop.v1.json (copy pty_command_stop shape); add command_stop to
    system_discover.v1.json tools[].
11. **Docs:** TOOL_CONTROL_SURFACE.md (add command_stop + forced-kill-only +
    CommandSignal gating; fix stale "32 live tools"), SPEC.md (record command_stop,
    SPEC-8/SPEC-13 xref), RELEASE_CHECKLIST.md (reconcile stale tool-count lines).
    README stale refs DEFERRED.

## Invariants / Tests / Verification: as the original PLAN-TC3-command-stop.md
("Invariants (Phase 6a/6b)" / "Effort/Test" / "Verification"), PLUS:
- guard insertion point is after receipt-publish (~763), before the draft match
  (~766); Phase 2 dedup-evict (~777) is unrelated;
- bump BOTH tools.rs count strings (20 + 29);
- RE-COUNT the live-tool baseline at phase start (Phase 4/5 may have changed 37);
- NO push/PR/merge without human approval; release-please side-effect of the
  tool-surface change coordinated with the operator.
