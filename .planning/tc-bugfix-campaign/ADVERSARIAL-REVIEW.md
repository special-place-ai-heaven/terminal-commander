# Adversarial review summary -- TC trust-defects campaign

**Source:** `review-verdict.json` (adversarial review of `plan-final.json`).
**Verdict:** **approve-with-amendments.**
8 REQUIRED amendments + 16 optional improvements (ALL adopted) + 5 rejected
findings. Every required amendment and adopted optional has been folded into the
per-defect PLAN files; this document is the human-readable index of what changed
and why.

Language: ASCII only.

---

## Required amendments (8)

### A1 -- Phase 6 (TC-3): command-waiter terminal-state guard + race-safe kill

**What changed in the plan:** The plan's claim that command_stop can "coordinate
with the lifecycle waiter via the existing terminal-state idempotency (PTY
checked-then-skipped pattern)" is FALSE for the command path. That guard exists
ONLY in the PTY waiter; the command waiter has none, and jobs.finish/cancel are
themselves non-idempotent. As written, command_stop + the waiter both call
jobs.cancel/finish + bucket_append => doubled bucket event + doubled audit row,
and the "exactly one event" test FAILS. Fix: (1) add a terminal-state skip guard
to the command waiter mirroring pty_command.rs:391-400; (2) add command.rs to
Phase 6 for the waiter guard (not only the cancel-field); (3) check-then-set
Cancelled under the live/jobs write lock so a job self-exiting microseconds before
the kill is not mislabeled; (4) make the integration test self-exit-race tolerant
(terminal AND (Cancelled OR Exited) for a may-self-exit job; STRICT Cancelled only
for sleep 60). command_stop on an already-terminal job is a no-op returning the
terminal state, not an error.

**Evidence:** crates/core/src/job.rs:154-184 (finish, no terminal guard),
:187-207 (cancel, no terminal guard); crates/daemon/src/command.rs:668-680
(command waiter unconditional); crates/daemon/src/pty_command.rs:391-400 (PTY
waiter DOES guard); plan-final.json:24, :246, :256.

### A2 -- Phase 5 (TC-5): --selfcheck-noop must be a hidden clap SUBCOMMAND

**What changed in the plan:** The "hidden --selfcheck-noop flag/mode" would be
rejected by clap -- the daemon Cli has a REQUIRED subcommand over a CLOSED set
{Check, Start, PrintConfig, Update}, Cli::parse() runs before dispatch, so a bare
`--selfcheck-noop` errors on the unknown arg AND the missing subcommand => child
exits nonzero on a HEALTHY daemon => self_check false-REDs (worse on Windows:
windows_subsystem=windows, no console). Fix: (1) add `Cmd::SelfcheckNoop` with
`#[command(hide = true)]`, handled at the top of main() to return SUCCESS as an
inert leaf mode; (2) spawn `[exe, "selfcheck-noop"]` (a VALID subcommand), NOT a
flag; (3) re-describe the Phase 5 main.rs change + reconcile the module-doc "Does
NOT spawn child commands by itself"; (4) add a positive test asserting
`terminal-commanderd selfcheck-noop` exits 0.

**Evidence:** crates/daemon/src/main.rs:28-48 (required cmd subcommand), :50-75
(closed Cmd enum), :86-87 (parse before dispatch), :4 (windows_subsystem);
plan-final.json:30, :216, :303.

### A3 -- Phase 4 (TC-4): build a REAL argv redactor (format_argv_metadata only truncates)

**What changed in the plan:** The central TC-4 safety claim -- argv_head is
"routed through the existing format_argv_metadata-style redaction" -- is FALSE.
format_argv_metadata only TRUNCATES each element to 128 bytes; it redacts nothing.
The ONLY redaction in the tree is per-rule capture redaction on command OUTPUT
lines, never argv. argv_head is argv[0..2], the exact window where secrets sit
(`psql postgres://user:pass@host`, `mysql -ppassword`,
`curl -H "Authorization: Bearer <tok>"`), all under 128 bytes so truncation never
trips. Fix: either (a) add a dedicated argv redactor that masks values after
secret-shaped flags (-H/--header, -p/--password, --token, --secret,
Authorization:, and key=value where key matches *_TOKEN/*_SECRET/*_PASSWORD/*_KEY)
=> `<redacted>`; OR (b) surface ONLY argv[0] + tag. Every section claiming
format_argv_metadata redacts is corrected to "only truncates; a NEW argv redactor
is required." The redaction test must assert against a real secret pattern, not
length.

**Evidence:** crates/daemon/src/command.rs:989-1004 (truncate, no redaction);
crates/sifters/src/lib.rs:309-343 (only output-line redaction); grep
redact|scrub|sanitize|mask: zero argv redactor; plan-final.json:36, :178, :199,
:301.

### A4 -- Phase 1 (TC-1a): fix is_idempotent classification (SubscriptionPull + omitted variants)

**What changed in the plan:** (1) RECLASSIFY SubscriptionPull as NON-idempotent
(false), NOT idempotent -- unlike BucketWait (client-cursor-driven, replayable),
SubscriptionPull advances per-consumer offsets and COMMITS them server-side INSIDE
the pull before serializing the response, so a lost-then-retried pull restarts from
the advanced offset and the drained events are gone (the documented "lossless
pull" becomes lossy on the exact retry path the gate preserves). (2) State the
governing RULE at the top of the helper and classify the omitted variants:
SubscriptionOpen=false (mints sub_id + slot; a blind retry LEAKS a slot),
SubscriptionClose=false (frees a slot), AuditSince=true (read),
CommandOutputTail=true (read). Add table-driven exhaustiveness assertions
including a SubscriptionOpen-retry-leaks-a-slot case.

**Evidence:** crates/daemon/src/subscriptions/pull.rs:541,544 (offset advance +
commit server-side during the pull); crates/ipc/src/protocol.rs:289-294
("lossless pull"); crates/daemon/src/ipc/handlers/bucket.rs:54-65 (BucketWait IS
cursor-driven, the safe contrast); protocol.rs:283-302 (omitted variants);
plan-final.json:82.

### A5 -- Phase 5 (TC-5): delete the phantom dispatch_envelope second await site

**What changed in the plan:** The "judge graft" propagating .await to a second
dispatch site corrects a NON-EXISTENT call site. handle_self_check has exactly ONE
caller: dispatch() at server.rs:541. dispatch_envelope (server.rs:821-828) is a
one-line delegate (`dispatch(state, boot, req_env, peer).await`) with NO SelfCheck
arm; the named-pipe server reaches SelfCheck transitively. Making handle_self_check
async needs `.await` at EXACTLY ONE place and ZERO changes to
dispatch_envelope/pipe_server. Fix: delete the phantom change item, its risk line,
and the "BOTH dispatch sites" framing; replace with "single call site
(server.rs:541); verify the Windows build compiles, expect zero source change in
pipe_server/dispatch_envelope."

**Evidence:** crates/daemon/src/ipc/server.rs:540-543 (sole call), :821-828
(delegate, no SelfCheck arm); grep: only one handle_self_check reference;
plan-final.json:2, :30-31, :214, :234.

### A6 -- Phase 6 (TC-3): file-count invariant violation -> split 6a/6b

**What changed in the plan:** Phase 6's change list enumerates ~16-17 edited files
(the 3 re-export sites confirmed real: ipc/lib.rs:57, daemon ipc/mod.rs:62, daemon
lib.rs:65), so the declared estimated_files:10 undercounts by ~60% and the phase
cannot be a single <=10-file PR. Fix (option b adopted): SPLIT Phase 6 into 6a
(daemon kill capability + IPC method + re-exports + dispatch, behind no MCP tool
yet -- the parity test compares method_name vs DISCOVERABLE_METHODS, not the MCP
catalogue, so it stays green) and 6b (MCP tool surface + fixtures + docs).
Document Phase 6 as EXEMPT from the <=10 rule (the atomic count-anchor set forces
a single larger atomic surface).

**Evidence:** plan-final.json:2, :42, :295 (the <=10 invariant), :245-253 (the
~16-17-file change list), :278 (estimated_files:10); crates/ipc/src/lib.rs:57,
crates/daemon/src/ipc/mod.rs:62, crates/daemon/src/lib.rs:65 (re-export sites).

### A7 -- Phase 2 (TC-2): plumb dedup_nonce end-to-end

**What changed in the plan:** Phase 2 adds dedup_nonce to the WIRE struct
CommandStartParams and the dedup check inside start_combed, but start_combed does
NOT receive CommandStartParams -- it receives the internal CommandStartRequest, and
handle_command_start_combed hand-builds that field-by-field, so an unlisted
dedup_nonce is silently DROPPED (the exact fake-success class the plan guards
against). Fix: (a) add dedup_nonce to CommandStartRequest; (b) thread
`dedup_nonce: params.dedup_nonce.clone()` in handle_command_start_combed; bump
estimated_files 4->5; add a round-trip test that a nonce sent over IPC is OBSERVED
by start_combed's dedup path. Also drop the "behind the existing live lock" option
-- the live lock is a different key/value type, so the dedup map must be a NEW
Arc<Mutex<HashMap<u64,(JobId,BucketId,Instant)>>> field on CommandRuntime, cloned
into the waiter closure.

**Evidence:** crates/daemon/src/command.rs:572-581 (start_combed builds from
CommandStartRequest), :584-592 (internal types), :610 (waiter_live capture
pattern); plan-final.json:113-116.

### A8 -- Phase 6 (TC-3): audit-before-kill ordering on the deny path

**What changed in the plan:** The plan says "evaluate policy ... emit the audit
row BEFORE the kill" but leaves the DENY ordering unspecified, and the cited PTY
precedent does the opposite of what a deny path needs (PTY checks existence first,
audits job_id only after kill, and is not policy-gated). Fix -- required ordering:
(1) evaluate CommandSignal FIRST; on Deny, emit a deny audit row whose SUBJECT is
the peer identity (NOT the job_id) and return PolicyDenied WITHOUT touching the
live map (no existence oracle, no job_id leak into a sibling-readable audit row);
(2) only AFTER Allow, do the live-map lookup (UnknownJob if absent); (3) emit the
job-id-bearing allow audit BEFORE firing the cancel handle. Tighten the deny test:
the deny audit subject is the peer and does NOT contain the job_id wire string;
stop() returns the SAME error for a live and a bogus job_id under
read_only_observer.

**Evidence:** crates/daemon/src/pty_command.rs:391-400 (PTY not a deny precedent);
crates/daemon/src/command.rs:701-708 (audit rows embed job_id.to_wire_string());
plan-final.json:246, :257.

---

## Adopted optional improvements (16, all adopted)

1. **Phase 2 peer-scoped dedup:** key the argv+cwd+tag fallback on
   (peer_uid/peer_sid, argv, cwd, tag), not (argv, cwd, tag) alone, to prevent a
   sibling local client receiving another client's live (job_id,bucket_id); or
   dedup ONLY on an explicit client nonce. [PLAN-TC2]
2. **Phase 1 split self-heal from re-send:** call try_self_heal() on a transport
   error for BOTH idempotent and mutating RPCs, but only RE-SEND when idempotent.
   [PLAN-TC1]
3. **Phase 3 degraded state honesty:** mark state UNKNOWN/last-observed, not
   silently Running; recover_hint tells the agent to confirm daemon liveness
   (health) FIRST before polling command_status. [PLAN-TC1b/TC6]
4. **Phase 3 final non-blocking drain:** after the wall-clock loop exits
   non-terminal, do ONE final non-blocking BucketWait (timeout_ms 0) to drain
   late events; signals best-effort, cursor authoritative. [PLAN-TC1b/TC6]
5. **Phase 6 JobBinding Clone derive:** remove the Clone derive (oneshot::Sender
   is not Clone; JobBinding is never whole-cloned; keep Debug). [PLAN-TC3]
6. **Phase 6 probe mutability + anchor refresh:** `let mut probe`; take the handle
   after spawn-success; corrected line anchors (id mint :474, spawn-failure
   :551-561, JobBinding insert :584, drive_to_exit move :620, eviction :668-671).
   [PLAN-TC3]
7. **Phase 1 operation-neutral mutating remedy:** "this mutating operation may or
   may not have taken effect; call command_status/runtime_state to confirm before
   re-issuing" (honest for start, stop, shutdown alike). [PLAN-TC1]
8. **Phase 1 correct the stale doc comment:** fix tools.rs:1790-1796 ("never a raw
   internal_error (-32603)") -- the very falsehood the TC-1a sub-defect is about.
   [PLAN-TC1]
9. **Phase 2 resolve the load-bearing dedup mechanism:** the adapter ALWAYS
   generates a per-call nonce (so a genuine LLM re-issue gets a new nonce =>
   distinct jobs); add a never-collapse test for the no-nonce identical-signature
   case. [PLAN-TC2]
10. **Phase 3 cursor superset coherence:** add cursor to the NORMAL payload too,
    so the degraded result is a genuine strict superset built by ONE shared
    builder. [PLAN-TC1b/TC6]
11. **Phase 0 RISK_REGISTER widening row:** record that argv_head/tag surfaces the
    program + bounded redacted head of every live job to ANY local IPC client
    (read handlers take no peer authz), accepted under the single-tenant trust
    model, mitigated by the new argv redactor; cross-link R-06. [RISK_REGISTER
    R-10]
12. **Phase 6 stale module-doc count:** reconcile
    daemon_unavailable_envelope.rs:12 ("all 30 daemon-backed tools") to the real
    count and re-label "gated + known prose". [PLAN-TC3 6b]
13. **Phase 4 drop the always-None path lift:** for the Command arm, `(+ path)` is
    a no-op (start_combed records path:None at command.rs:509); drop it, keep
    tag-lifting (tag IS recorded at command.rs:510). Reword the invariant as CLI
    display completeness, not data integrity (probe_rows is the single shared
    render path render.rs:164). [PLAN-TC4]
14. **Cross-cutting MCP guard-2 substring trap:** any new comment/string literal
    in crates/mcp/src must avoid the exact guard-2 literals (write "file system",
    not "std::fs"). [PLAN-TC1 invariants]
15. **Phase 1 lock in PTY-path coverage:** add an integration test asserting
    pty_command_start (not only run_and_watch) produces exactly ONE PTY job under
    an induced transport timeout. [PLAN-TC1]
16. **Phase 5 positive subcommand exits-0 test** (folded with A2): assert
    `terminal-commanderd selfcheck-noop` exits 0 so a future clap refactor that
    breaks the subcommand is caught. [PLAN-TC5]

---

## Rejected findings (5)

1. **REJECTED -- policy-security-reviewer minor "self_check noop could be abused as
   arbitrary-exec / must not bypass policy gates":** a confirmation, not a defect.
   The handler hardcodes the argv and routes the spawn through the normal
   CommandRuntime path (validate_argv + shell guard + policy.evaluate). The
   substantive part (an inert leaf mode exiting 0 before any work) is already
   folded into the required Phase 5 CLI-shape amendment (A2). No separate change.
2. **REJECTED (positive confirmation, no amendment) -- scope-guardian "Phase 1
   DOES fix pty_command_start + command_start_combed" and
   "transport_unavailable_error/-32603/into_mcp_error are exactly as claimed":**
   both are VERIFIED confirmations that the plan's premises hold; no amendment
   assigned. The doc-comment-fix sub-suggestion is carried as adopted optional #8.
3. **REJECTED (already-handled) -- adversarial-breaker "F1 retry-gate removes
   reconnect self-heal for mutating RPCs; acceptable trade":** the reviewer's own
   verdict is that fail-closed is the CORRECT posture, so not a defect. The
   actionable sub-point (try_self_heal bundled into the gated branch) is preserved
   as adopted optional #2.
4. **REJECTED (redundant framing) -- coherence-auditor "verified anchor values
   accurate at HEAD" and policy-security "TC-4 broadens read surface":** the first
   is positive confirmation (re-verified: command_output_tail <
   command_start_combed < command_status < command_stop holds); the second is
   captured as the RISK_REGISTER-row improvement (R-10), an accepted-risk doc item
   under the single-tenant trust model, not a correctness blocker.
5. **PARTIALLY REJECTED -- scope-guardian minor "F1-b deferral rationale
   self-contradictory with Phase 2 dedup_nonce":** rated minor; design is coherent
   enough to ship with defensible distinctions (method-local field vs universal
   RequestEnvelope; in-flight map vs persistent TTL store). The contradiction is
   in PROSE clarity, not design; folded into the dedup-mechanism clarification
   (adopted optional #9), not a required amendment.
