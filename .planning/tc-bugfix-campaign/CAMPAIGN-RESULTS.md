# TC trust-defect campaign -- results (Phases 4-6: TC-4 / TC-5 / TC-3)

as_of 2026-06-09. Branch `fix/tc-trust-defects`. Base `5b054cb` (the GOAL docs commit).
NOTHING pushed. All work is local commits on `fix/tc-trust-defects`.

This document is the honest record of Phases 4a -> 4b -> 5 -> 6a -> 6b plus the two
human-directed security add-ons. Phases 0-3 (TC-1a/TC-2/TC-1b/TC-6) were already done
before this run; see RESUME-STATE.md for their lineage.

## Outcome

All three remaining trust defects are CLOSED, implemented-verified-committed, with
both-OS gates green per phase and both mandatory human security sign-offs obtained:

- TC-4  anonymous probe rows / run_and_watch tag-drop / raw argv in audit log -> CLOSED
- TC-5  self_check false-green (never spawned)                                 -> CLOSED
- TC-3  no command_stop (a started command could not be killed)                -> CLOSED

Campaign diff: 11 commits, 39 files, +3783/-168. `git diff 5b054cb..HEAD -- Cargo.lock`
is EMPTY -> zero new crate dependencies (a `windows-sys` feature add only).

## Commit lineage (5b054cb..HEAD, oldest first)

| SHA | Type | What |
|-----|------|------|
| 3b7e719 | fix(daemon) | TC-4 4a: ProbeListEntry +tag +argv_head; redact_argv_head + collect_probes wiring |
| 7fd0ec8 | fix(daemon) | TC-4 audit-log redaction (format_argv_metadata) -- HUMAN P1 ADD-ON |
| bb5b41d | style(mcp)  | gate-unblock: from_millis(5000)->from_secs(5) in a cfg(unix) e2e (clippy 1.95) |
| c91200e | fix(mcp)    | TC-4 4b: run_and_watch tag plumbing + CLI probe_table render + fixtures |
| 765ffe6 | docs(campaign) | drift-amend #1: re-anchor TC-5 plan post-TC-4 + RESUME-STATE |
| 6af4909 | docs(campaign) | TC-5 RE-PLAN: cached-bucket via start_combed reuse seam |
| d136e95 | fix(daemon) | TC-5: async handle_self_check real profile-gated spawn round-trip |
| f5cf4a7 | docs(campaign) | drift-amend #2: re-anchor TC-3 plan post-TC-4/TC-5 + RESUME-STATE |
| d94e1c5 | fix(daemon) | TC-3 6a: CommandRuntime::stop + CommandStop IPC (gated kill) |
| a5cdb6c | fix(probes) | TC-3 process-tree teardown (group/Job-Object) -- HUMAN P2 ADD-ON |
| c7bcfbd | feat(mcp)   | TC-3 6b: command_stop MCP tool + atomic 37->38 catalogue churn |

`feat(mcp)` (c7bcfbd) and the `fix(...)` commits will trigger release-please on push;
coordinate the tool-surface MINOR bump with the operator.

---

## Phase 4a -- argv_head + tag identity on probe rows, redacted (TC-4) -- commit 3b7e719

Files: crates/ipc/src/protocol.rs (ProbeListEntry +tag +argv_head, additive serde-default),
crates/daemon/src/command.rs (NEW redact_argv_head + mask helpers + JobBinding.argv_head +
CommandRuntime::argv_head accessor), crates/daemon/src/ipc/handlers/runtime.rs (collect_probes
wired Command/PTY/FileWatch arms), + tests.

Redactor: masks secret-value flags (-p/--password/--token/--secret/--key, -H/--header
Authorization/Bearer + custom headers, -u/--user/--proxy-user basic-auth), URL userinfo
passwords, and env KEY=VALUE secrets; defeat-proof against casing, `=` vs space,
short/long/attached flags, URL userinfo, rule ordering; program + flag names stay visible;
128B char-boundary truncation after masking. Manual string parsing, no regex, no new dep.

Review: code review APPROVE-WITH-NITS, 0 blockers. Security review found 3 HIGH + 1 MED
under-redaction leaks (bare env keys, -u user:pass, --password=<url> rule-ordering, custom
headers); ALL fixed; security re-review verdict P1-READY (46 functional + 19 panic vectors,
no new bypass, idempotent, panic-safe).

Gates: Windows fmt CLEAN, clippy -D CLEAN, ipc 22 + daemon-lib 121 (incl 26 redact_tests),
windows-gate.ps1 PASSED (probes 4 + daemon spawn-site 3). WSL nextest 313/313 (the cfg(unix)
PTY arm compiled + linted on Linux).

Live verify (TEST socket, daemon-only): runtime_state_command_probe_carries_tag_and_redacted_argv_head
-- a tagged `env SECRET_TOKEN=topsecret123 sleep 1` probe row shows tag + `SECRET_TOKEN=<redacted>`,
secret absent.

### >>> P1 SECURITY SIGN-OFF: GRANTED ("Approve, but fix audit-log too") <<<
The human approved the probe-listing redactor AND directed extending redaction to the
pre-existing raw-argv audit-log surface (out of the original TC-4 plan scope).

## TC-4 audit-log redaction (P1-directed add-on) -- commit 7fd0ec8

format_argv_metadata wrote raw (length-truncated) argv into the audit log on every
command_start allow/deny/spawn-fail/exit path. Extracted a shared redact_argv(argv,
max_items): redact_argv_head delegates with Some(3) (byte-identical -- all 26 prior tests
green); format_argv_metadata uses redact_argv(argv, None) (FULL argv, unbounded, so a secret
past index 3 is masked) and the char-boundary truncation also removed a latent multibyte panic.
Security-confirmed (masking core untouched, all 5 audit sites covered incl. both deny paths,
no new bypass). Live: command_stop... (sic) command_start_audit_metadata_redacts_argv_secret --
the persisted allow audit row masks a --password value at argv index 3. Gates green (Windows
lib 30 redact_tests; WSL clippy + nextest 31 incl the live audit test).

## Phase 4b -- run_and_watch tag + CLI probe identity (TC-4) -- commit c91200e

Files: crates/mcp/src/tools.rs (McpRunAndWatchParams +tag; into_parts threads the REAL tag,
was hardcoded None; chain into_ipc -> CommandStartParams.tag verified), crates/cli/src/render.rs
(probe_rows -> testable probe_table + TAG/ARGV_HEAD columns, Option degrades to blank,
width-clamped char-boundary-safe), fixtures runtime_state.v1.json + probe_list.v1.json
(additive + invariant reconcile), crates/mcp/tests/runtime_state_live_e2e.rs (live e2e).

Review: APPROVE-WITH-NITS, 0 blockers; both nits applied (clamp the TAG column; multibyte
truncation test). Gates: Windows fmt/clippy + mcp-lib 65 + cli 32. WSL clippy + nextest mcp +
cli 46 (session_reap drvfs wedge excluded per the Phase 2-3 exemption). MCP guards clean.
Live e2e: run_and_watch_threads_tag_and_redacts_argv_head_through_mcp -- a tagged MCP
run_and_watch surfaces tag `e2e-4b-tag` + a redacted argv_head in runtime_state.

---

## Drift-amend #1 -- TC-5 plan (765ffe6) + TC-5 RE-PLAN (6af4909)

After TC-4 landed: re-anchored the TC-5 plan. Verdict: ZERO line drift to the TC-5-anchored
symbols (TC-4 did not touch server.rs/main.rs/state.rs/source.rs; protocol.rs/tools.rs edits
were below the TC-5 symbols). Recorded the two new facts (JobBinding gained argv_head;
command.rs grew ~700 lines; Phase-2 dedup intact).

RE-PLAN (6af4909): a SymForge investigation found the TC-5 plan's "reuse ONE cached immortal
bucket via the normal CommandRuntime path" was NOT achievable -- start_combed always mints a
fresh BucketId and bucket_create rejects duplicates; a per-call fresh bucket would LEAK one
immortal bucket per self-check. Re-planned (correctness over the leaky path) to add an optional
bucket-reuse seam. This re-plan commit is an EXTRA docs commit beyond the goal's "exactly two
drift-amends" -- a documented deviation forced by the broken plan claim.

## Phase 5 -- self_check real command-spawn round-trip (TC-5) -- commit d136e95

Files: crates/daemon/src/command.rs (start_combed split into private start_combed_inner(req,
reuse_bucket: Option<BucketId>); pub start_combed delegates None -- byte-identical, all callers
untouched; new start_combed_reusing for the self-check), crates/daemon/src/state.rs
(selfcheck_bucket: parking_lot::Mutex<Option<BucketId>>), crates/daemon/src/ipc/server.rs
(async handle_self_check + selfcheck_spawn_probe + fresh_selfcheck_nonce; dispatch .await),
crates/daemon/src/main.rs (hidden Cmd::SelfcheckNoop short-circuit BEFORE resolve_config), +
3 test files.

Behavior: handle_self_check now spawns this binary's hidden selfcheck-noop leaf through the
normal CommandRuntime path (validate_argv + shell guard + policy.evaluate + a command_start
audit row), polls command_status to terminal within ~2s, sets failures>0 ONLY on genuine
breakage (spawn error / never-terminal / Failed / nonzero exit); a policy Deny or an
unresolvable current_exe SKIPS with a reason (NEVER false-RED). Fresh dedup_nonce per call
defeats the TC-2 dedup. The cached-bucket parking_lot Mutex is never held across the poll
.await. runtime.rs run_self_check is untouched.

Review: APPROVE-WITH-NITS, 0 blockers; the dead `| None` arm in the healthy gate tightened to
`exit_code == Some(0)`. Gates: Windows fmt/clippy + lib 125 + subcommand test; WSL clippy +
nextest 305/305. Live (TEST socket, WSL): self_check_developer_local_spawns_and_reports_healthy
(failures==0, command_start allow row), self_check_reuses_one_bucket_across_calls (+1 bucket
over lifetime), self_check_read_only_observer_skips_without_failure (never false-RED),
selfcheck_spawn_probe_reports_failure_on_nonexistent_binary (never false-GREEN),
self_check_back_to_back_spawns_distinct_jobs (dedup-defeating), selfcheck_noop_subcommand_exits_zero;
+ 2 reuse-seam unit tests.

---

## Drift-amend #2 -- TC-3 plan (f5cf4a7)

After TC-5 landed: re-anchored the TC-3 plan. Every claim HOLDS, but command.rs/server.rs
anchors drifted hard: start_combed RENAMED to start_combed_inner (the spawn match, live-map
insert, lifecycle waiter, audit sites now inside it); JobBinding is 7 fields (argv_head added),
still Clone; the parity test system_discover_methods_match_dispatch shifted ~+189 (now
1236-1267); the 37-tool baseline confirmed UNCHANGED (Phase 4/5 added no MCP tool).

## Phase 6a -- command_stop force-kill via gated IPC (TC-3) -- commit d94e1c5

Files: crates/probes/src/process.rs (take_cancel_handle, metrics_handle), crates/daemon/src/command.rs
(JobBinding +cancel +metrics_live, Clone removed; mut probe takes the handle; lifecycle waiter
terminal-state guard between receipt-publish and the draft match, with the TC-2 dedup evict on
the guard path; CommandRuntime::stop), crates/ipc/src/protocol.rs (CommandStop Params/Response
+ req/resp variants + is_idempotent NON-idempotent), 3 re-exports, crates/daemon/src/ipc/handlers/command.rs
(handle_command_stop), crates/daemon/src/ipc/server.rs (method_name/DISCOVERABLE_METHODS/dispatch/
all_request_variants/parity), + a live test file.

CommandRuntime::stop ordering (security-critical): (1) evaluate PolicyAction::CommandSignal
FIRST; on Deny emit a deny audit row whose subject is the PEER identity (never the job_id) and
return PolicyDenied WITHOUT touching the live map (no existence oracle); (2) after Allow,
live-map lookup (UnknownJob if absent) and check-then-set Cancelled iff not terminal under the
live write lock (single critical section vs the natural-exit waiter); (3) job-id allow audit
BEFORE firing the kill. stop snapshots LIVE probe metrics (the MED code-review finding fixed to
true PTY parity). Lock order live -> jobs, no reverse nesting.

Review: code review APPROVE-WITH-NITS 0 blockers (the MED metrics-zero fixed). Security review:
SAFE for P2 -- all 8 adversarial threats CONFIRMED-SAFE (no oracle, no job_id leak on deny,
kill reaps the child, the stop-vs-exit race worst-case is the documented cosmetic
Exited-vs-Cancelled label, no deadlock, exactly-once dedup/audit). Gates: Windows fmt/clippy +
lib 125 + probes 38; WSL clippy + nextest 378/378. Live (TEST socket, WSL):
command_stop_kills_running_command_and_audits_allow,
command_stop_read_only_observer_is_denied_with_peer_subject_no_oracle,
command_stop_second_stop_on_terminal_job_is_noop,
command_stop_unknown_job_under_allowed_profile_returns_unknown_job (4/4).

### >>> P2 SECURITY SIGN-OFF: GRANTED ("Approve, add process-tree kill") <<<
The human approved the kill/policy/audit ordering AND directed upgrading the single-process
kill to a process-tree teardown.

## TC-3 process-tree teardown (P2-directed add-on) -- commit a5cdb6c

ProcessProbe's cancel killed only the direct child -> grandchildren orphaned. Upgraded (no new
crate): UNIX spawns the child in its own process group (process_group(0)) and the cancel
SIGKILLs the whole group via the kill(1) tool as `kill -s KILL -- -<pgid>`. NOTE: a prior
iteration used `kill -KILL -<pgid>`, which procps-ng (verified WSL2 4.0.4) mis-parses and
delivers SIGKILL to the CALLER's group -- it would have killed the daemon itself; the
`-s KILL --` form is parse-unambiguous and was verified to reap the target group's
grandchildren while leaving the caller alive. WINDOWS uses a Job Object (CreateJobObjectW +
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE + AssignProcessToJobObject; TerminateJobObject on cancel;
RAII JobHandle CloseHandles exactly once) -- native Win32 only, no taskkill/powershell (EDR
hardening). Cargo.toml adds only windows-sys features (Cargo.lock unchanged). CommandRuntime::stop
and all daemon code untouched.

Security re-review: SAFE -- all 6 threats CONFIRMED-SAFE (self-kill closed; child_pid can never
equal the daemon's pgid; Windows handle lifetime sound, no double-close/UAF; no policy
regression; EDR-safe). Gates: Windows fmt/clippy + tree-kill 2/2; WSL clippy + probes nextest
49/49 (incl unix_grandchild_is_killed_on_cancel) + command_stop_ipc 4/4 still green. Live:
a parent's grandchild (sh -c "sleep 300 &" on unix; cmd /c ping on windows) is GONE after the
cancel; the caller is left alive.

## Phase 6b -- command_stop MCP tool + atomic 37->38 churn (TC-3) -- commit c7bcfbd

The 6a daemon kill is surfaced as an MCP tool (pure IPC forwarding, NON-idempotent, MCP-guard
clean). The live-tool catalogue went 37 -> 38 ATOMICALLY across every site (see "Tool count"
below). Review: code review (via the implementing agent + my own re-grep, which caught two
EXTRA stale counts -- main.rs and mcp_live_daemon module-doc -- bumped to 38). Gates: Windows
fmt/clippy + lib 65 (incl catalogue_lists_thirty_eight_live_tools + tool_router) +
fixture_catalogue_contract; WSL clippy + nextest -p terminal-commander-mcp 125/125 (incl
fixture_catalogue_contract, mcp_stdio full-set, mcp_live_daemon live_count==38,
daemon_unavailable_envelope checked==37). MCP guard greps clean.

---

## Tool count: 37 -> 38 (sites updated)

Baseline RE-COUNTED at 6b start = 37 (Phase 4/5 added no MCP tool). command_stop sorts after
command_status (catalogue order) / between command_status and event_context (alpha). Sites:
tool_catalogue() entry; the new #[tool] command_stop + McpCommandStopParams + the protocol
import; catalogue test (renamed _thirty_seven_ -> _thirty_eight_) + router test; FOUR doc
strings (tools.rs:20, tools.rs:29, lib.rs:12, main.rs:12); mcp_stdio.rs sorted vec;
mcp_live_daemon.rs (sorted vec + live_count 37->38 + comment + module-doc 30->38);
daemon_unavailable_envelope.rs (checked 36->37 + minimal_tool_args arm + module-doc 30->37);
mcp-tool-fixture-map.v1.json (live_tools 37->38, covered_live 33->34, arrays);
system_discover.v1.json; NEW command_stop.v1.json; docs TOOL_CONTROL_SURFACE.md / SPEC.md /
RELEASE_CHECKLIST.md. No site left at the old count (verified by the contract/catalogue/router
tests + a final grep).

## Human security sign-offs

- P1 (after 4a): GRANTED with the added directive "fix the audit-log too" -> commit 7fd0ec8.
- P2 (after 6a): GRANTED with the added directive "add process-tree kill" -> commit a5cdb6c.
Both add-ons were implemented, security-reviewed, gated, and verified.

## Known gaps / Unverified / Deferred

- DEFERRED (per the TC-3 plan): README stale tool-count refs not updated this campaign.
- DEFERRED (pre-existing, acknowledged): RELEASE_CHECKLIST.md L312 ("29-tool TC45 catalogue
  unchanged") sits in a per-release no-change manifest for a DIFFERENT (docs-only) release;
  editing it would falsify that release's historical assertion, so it was left. Only the L71
  smoke-instruction literal was reconciled.
- EXEMPTION (pre-existing, Phases 2-3): the workspace-wide `cargo nextest --workspace` is NOT
  run -- the `terminal-commander-cli::session_reap` integration test wedges on /mnt/e drvfs
  under WSL. The targeted per-crate WSL nextest was run instead; `-p terminal-commander-cli`
  was run with `-E "not binary(session_reap)"`.
- GATE-DISCIPLINE LEARNING: the Windows clippy gate is BLIND to `#![cfg(unix)]` test bodies
  (it compiles them to nothing); the WSL clippy run is the authoritative linter for cfg(unix)
  code. Two pre-existing clippy violations in cfg(unix) mcp test files surfaced only on the
  WSL gate (one fixed in bb5b41d; the 6b churn's cfg(unix) edits were WSL-verified).
- KILL SEMANTICS (documented, in scope as best-effort): a child that calls setsid/setpgid into
  its own session (unix) or is spawned with CREATE_BREAKAWAY_FROM_JOB (windows) can escape the
  tree teardown; and if Job-Object creation fails / `kill(1)` is absent, the kill degrades to a
  single-process kill (grandchildren may orphan). The Windows degradation is silent (probes has
  no `tracing` dep, so a warn would need a new dep -- not added); documented in-code.
- COMMIT-COUNT DEVIATIONS from the goal's S6 expectation (5 impl + 2 drift-amend + 1 GOAL):
  +1 fix(daemon) audit-log (P1-directed), +1 fix(probes) tree-kill (P2-directed), +1 style(mcp)
  lint gate-unblock, +1 docs(campaign) TC-5 re-plan. All documented above and traceable.
- METRICS race note (cosmetic): a stop racing a natural exit can leave the job labeled
  Exited-instead-of-Cancelled (or vice versa) in a microscopic window -- identical to the
  existing PTY design; no corruption, no security impact.

## Second opinion (optional, pending)

A Cursor code-review prompt is prepared at .planning/tc-bugfix-campaign/CURSOR-REVIEW-PROMPT.md
(covers the full campaign diff 5b054cb..HEAD, emphasis on the security-critical surfaces). When
the operator runs it, the report lands at .planning/tc-bugfix-campaign/cursor_review_tc456.md;
any findings will be folded as follow-up commits before push.

## Closing

NOTHING is pushed. Branch `fix/tc-trust-defects` (HEAD c7bcfbd) awaits the human to push.
The `fix:`/`feat:` commits will fire release-please (the command_stop tool surface is a MINOR
bump) -- coordinate with the operator. No push / PR / merge was performed.
