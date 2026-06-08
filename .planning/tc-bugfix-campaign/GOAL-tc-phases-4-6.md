/goal Drive Phases 4a -> 4b -> 5 -> 6a -> 6b of the in-flight TC trust-defect
bugfix campaign to completion per the three AMENDED plans, with both-OS gates green
and reviews done, each phase a separate commit on fix/tc-trust-defects, STOPPING at
every mandatory human pause, and NEVER pushing/merging -- until TC-4, TC-5, and TC-3
are implemented-verified-committed and CAMPAIGN-RESULTS.md is written, without new
deps, without MCP-guard literal leaks, and without touching the live machine daemon.

==============================================================================
NON-NEGOTIABLE HEADLINES (read these first, they override convenience)
==============================================================================
- STOP-AND-WAIT human pauses (do NOT self-approve, do NOT continue past them):
    (P1) SECURITY sign-off AFTER Phase 4a (the new argv redactor masks creds).
    (P2) SECURITY sign-off DURING Phase 6a (CommandRuntime::stop = kill +
         policy-deny ordering + audit-no-leak), BEFORE 6b.
    (P3) NEVER push / open a PR / merge / force / delete a remote branch.
         Commit to fix/tc-trust-defects ONLY and STOP at campaign end for the
         human to push. (A fix:/feat: commit triggers release-please --
         coordinate the tool-surface change with the operator.)
  A "security review" by a subagent is INPUT, not approval. Only the human gives
  P1/P2 sign-off. If you cannot get the human, you are BLOCKED -- stop, do not
  proceed to the next sub-phase.
- VERIFY-AS-REAL: a phase is done ONLY when the ACTUAL gates pass AND (for
  user-facing behavior) the running daemon/tool actually does the thing against a
  TEST socket. Never declare done off `cargo build` or code-reading. Report
  mock/blocked/unverified honestly.

==============================================================================
CONTEXT
==============================================================================
Repo: E:\project\terminal-commander (Rust workspace, local-only product:
  terminal-commanderd = daemon, terminal-commander-mcp = stdio MCP adapter,
  terminal-commander = admin CLI). Project conventions live in AGENTS.md +
  CONTRIBUTING.md + TESTING.md (this repo has NO root CLAUDE.md -- see ASSUMPTIONS).
Branch: fix/tc-trust-defects (base 725e223). HEAD at goal-authoring time: 17646b4
  ("docs(campaign): re-anchor + amend TC-4/5/3 plans for code drift"). NOTHING
  pushed. NO push/PR/merge without explicit human approval.

Phases 0-3 are DONE, committed, and form a pushed-checkpoint lineage on this branch
(do NOT redo any of these):
  - Phase 0 docs:            73b6450
  - Phase 1 retry-gate TC-1a: a012fa8  (reviewed APPROVE, both OS gates PASS)
  - Phase 2 dedup guard TC-2: cf43d5b  (reviewed APPROVE, both OS gates PASS)
  - Phase 3 wait-loop TC-1b+TC-6: cca9f06 (run_and_watch wall-clock cap + degraded
    success-shaped result; reviewed APPROVE-WITH-NITS, 0 blockers; both OS gates PASS)

Remaining defects this goal closes (in this order):
  - TC-4  anonymous probe rows / run_and_watch tag-drop   -> Phase 4a + 4b
  - TC-5  self_check false-green (no real spawn)           -> Phase 5
  - TC-3  no command_stop (cannot kill a started command)  -> Phase 6a + 6b

SOURCE OF TRUTH -- the runner MUST read and FOLLOW these per-phase; this goal
references them and does NOT restate their detail:
  - .planning\tc-bugfix-campaign\PLAN-TC4-probe-identity-AMENDED.md   (Phase 4a, 4b)
  - .planning\tc-bugfix-campaign\PLAN-TC5-selfcheck-spawn-AMENDED.md  (Phase 5)
  - .planning\tc-bugfix-campaign\PLAN-TC3-command-stop-AMENDED.md     (Phase 6a, 6b)
  - .planning\tc-bugfix-campaign\RESUME-STATE.md (campaign state, drift convention,
    standing rules)
  - scripts\windows-gate.ps1 , scripts\linux-gate.sh (the EXACT gate scripts)
  - AGENTS.md / CONTRIBUTING.md / TESTING.md (project conventions)
USE THE *-AMENDED.md plans only. The pre-amend originals (PLAN-TC4/5/3-*.md without
-AMENDED) are kept for lineage ONLY -- do not execute them.

DRIFT REALITY (why re-anchoring is mandatory): the AMENDED plans cite absolute line
numbers `as_of commit e76ebdc`. Current HEAD (17646b4) is the AMENDMENT commit that
re-anchored against e76ebdc, plus Phase 0-3 landed earlier. Every absolute line
number is a HINT that WILL be stale; each plan is SYMBOL-anchored precisely so it
survives drift. Re-anchor every symbol via SymForge at each phase start.

Crate package names (for exact targeted gate commands):
  terminal-commander-ipc | terminal-commanderd | terminal-commander-mcp |
  terminal-commander-cli | terminal-commander-probes | terminal-commander-core

==============================================================================
SUCCESS CRITERIA (MEASURABLE -- all must be TRUE; "done" is a checkable outcome,
not "it compiles" / "tests pass" / "looks right")
==============================================================================
[S1] TC-4 implemented exactly per PLAN-TC4-...-AMENDED.md:
      - 4a: ProbeListEntry gains additive serde-default tag + argv_head; a NEW
        redact_argv_head masks the listed secret patterns; collect_probes wires
        tag + redacted argv_head on Command/PTY/FileWatch arms. SECURITY-reviewed
        (subagent) AND human P1 sign-off obtained. Separate commit.
      - 4b: McpRunAndWatchParams gains tag; into_parts threads the REAL tag (no
        more hardcoded tag:None); CLI render shows tag/argv_head; fixtures updated.
        Separate commit.
      EVIDENCE (4a, daemon-only -- NO MCP yet): unit tests prove a REAL secret
      pattern (curl Authorization Bearer, postgres://u:pw@h, mysql -ppass) becomes
      <redacted> while program+flag names stay visible; a DIRECT runtime_state IPC
      call against a TEST daemon shows probe rows carrying tag + redacted argv_head.
      EVIDENCE (4b, end-to-end): a live MCP tagged run_and_watch -> runtime_state row
      shows the tag; the CLI render shows the tag/argv_head columns.
[S2] TC-5 implemented exactly per PLAN-TC5-...-AMENDED.md:
      handle_self_check becomes async + does a profile-gated bounded REAL
      command-spawn round-trip into a CACHED immortal bucket; SelfcheckNoop hidden
      subcommand short-circuits BEFORE resolve_config in main(); cached bucket uses
      parking_lot/OnceCell (never tokio Mutex across .await); runtime.rs
      run_self_check is NOT touched. Separate commit.
      EVIDENCE (live, TEST socket): DeveloperLocal self_check spawns the noop,
      report shows spawn-ok, failures==0 healthy; a 2nd self_check reuses the SAME
      bucket (bucket_count +1 max over lifetime); read_only_observer SKIPS with a
      reason and stays failures==0 (never false-RED); a forced-broken round-trip
      yields failures>0 + line (never false-GREEN); `terminal-commanderd
      selfcheck-noop` exits 0.
[S3] TC-3 implemented exactly per PLAN-TC3-...-AMENDED.md:
      - 6a: ProcessProbe::take_cancel_handle; JobBinding cancel field + Clone
        removed; command-waiter terminal-state guard inserted AFTER receipt-publish
        and BEFORE the draft match; CommandRuntime::stop with the exact deny-first /
        no-job_id-leak / check-then-set ordering; CommandStop IPC + 3 re-exports +
        dispatch/parity. SECURITY-reviewed (subagent) AND human P2 sign-off
        obtained. Separate commit.
      - 6b: command_stop MCP tool + ATOMIC tool-count churn + fixtures + docs.
        Separate commit.
      EVIDENCE (live, TEST socket): start a long command -> command_stop kills it,
      returns terminal state; a ReadOnlyObserver stop is PolicyDenied with the deny
      audit subject = peer identity (NOT job_id), live map untouched; a 2nd stop on
      an already-terminal job is a no-op returning the terminal state.
[S4] TOOL-COUNT ANCHORS CONSISTENT: at Phase 6b start, RE-COUNT the live-tool
      baseline (Phase 4/5 added NO MCP tool, but verify -- do not trust the 37
      literal). TC-3 takes the catalogue 37->38 ATOMICALLY across ALL sites the plan
      lists (catalogue test, router test, BOTH tools.rs count strings, mcp/lib.rs,
      mcp_stdio.rs, mcp_live_daemon.rs vec+assert+comment, daemon_unavailable_
      envelope.rs checked+arm+module-doc, fixture-map, system_discover fixture).
      No site left at the old count.
[S5] BOTH-OS GATES GREEN per phase (4a, 4b, 5, 6a, 6b):
      - Windows: `pwsh -File scripts\windows-gate.ps1` PASSES (asserts >=1 test ran).
      - WSL (Linux): the TARGETED equivalent passes for the touched crates:
        `cargo nextest run -p <touched crates>` + `cargo fmt --all --check` +
        `cargo clippy ... -D warnings` on touched crates + the MCP-guard greps.
        Do NOT run full `nextest --workspace` (session_reap CLI test wedges on
        /mnt/e drvfs -- pre-existing/unrelated; exemption established Phases 2-3).
[S6] EACH PHASE A SEPARATE conventional commit on fix/tc-trust-defects with the
      footer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
      Five impl commits (4a, 4b, 5, 6a, 6b) + EXACTLY TWO drift-amend docs commits
      (amend TC-5 after TC-4 lands; amend TC-3 after TC-5 lands) = 7 commits, plus the
      one-time GOAL-file docs commit. Likely prefixes: 4a fix(daemon), 4b fix(mcp),
      5 fix(daemon), 6a fix(daemon), 6b feat(mcp) (new command_stop tool ->
      release-please MINOR). All on fix/tc-trust-defects; none pushed.
[S7] DRIFT-AMEND CHAIN (cross-PLAN only): after each PLAN completes (TC-4 = 4a+4b;
      TC-5; TC-3), re-anchor + amend the NEXT plan's AMENDED file for the new drift
      and COMMIT that amendment BEFORE starting it. INTRA-plan sub-phase transitions
      (4a->4b and 6a->6b live in ONE plan file) need only a fresh SymForge re-anchor
      at the sub-phase start (loop step 1) -- NOT a docs commit (a plan is not amended
      against its own file mid-phase). Net: EXACTLY TWO drift-amend commits -- amend
      TC-5 after TC-4 lands, amend TC-3 after TC-5 lands. TC-3 (last) has no successor.
[S8] NOTHING pushed/merged. Final honest results doc written at
      .planning\tc-bugfix-campaign\CAMPAIGN-RESULTS.md with PER-PHASE evidence
      (gate outputs / test counts / commit SHAs) and an explicit Known-Gaps section.
[S9] HARD CONSTRAINTS HELD (see Constraints): zero new deps; crates/mcp/src free of
      the forbidden literal substrings (incl. comments); live machine daemon never
      touched; target/ kept warm between phases.

==============================================================================
CONSTRAINTS (rules the runner MUST follow)
==============================================================================
[security] P1 pause after 4a; P2 pause during 6a. Human sign-off only -- a subagent
  security review is advice, not approval. BLOCKED if no human is reachable.
[git] NO push / PR / merge / force / remote-branch-delete -- EVER, without explicit
  human approval. Commit to fix/tc-trust-defects only; STOP at campaign end.
  Always use `git -C E:\project\terminal-commander ...` (the runner may be in HOME).
[deps] ZERO new dependencies (no new Cargo.toml entries; reuse crates already
  present -- e.g. parking_lot/OnceCell already imported per the TC-5 plan).
[mcp-guard] crates/mcp/src must contain NONE of these literal substrings, even in
  comments: Command::new , Command::spawn , TcpListener , UdpSocket , tokio::fs ,
  std::fs , File::open , read_to_string , read_to_end . Use `std::time`,
  "file system", etc. The linux-gate greps enforce this (guard 1 + guard 2).
[live-daemon] NEVER touch / restart / kill the live machine daemon. ALL tests use
  their own TC_SOCKET + an explicit data dir. The live e2e harness spawning an
  in-process daemon in a temp dir is FINE.
[target-warm] Keep target/ WARM between phases. The cache was just cleaned (Phase 4a
  is a COLD build) -- do NOT `cargo clean` between phases. `cargo clean` only at
  CAMPAIGN END. linux-gate uses CARGO_TARGET_DIR=~/tc-linux-target (TC_LINUX_TARGET
  override honored).
[wsl-wedge] WSL is invoked via `wsl bash -lc "..."` with /mnt/e/... paths. If a test
  is idle >3 min at 0% CPU, abort and kill ONLY PIDs whose cwd is THIS repo -- never
  other agents' processes, never the live machine daemon.
[shell] In this environment a leading `! ` runs bash (Git Bash), not PowerShell;
  PowerShell syntax otherwise. WSL paths are /mnt/e/...
[scope] Touch ONLY the files each AMENDED plan names for that sub-phase (each
  sub-phase is sized <=5 files by design). Do not expand scope. Do NOT touch
  runtime.rs run_self_check (TC-5 targets the IPC handle_self_check only).
[anchor] RE-ANCHOR every symbol via SymForge at phase start before editing; absolute
  line numbers in the plans are `as_of e76ebdc` HINTS and WILL be stale. If a plan
  CLAIM no longer holds (e.g. a prior phase already changed a symbol), STOP and
  re-plan that item -- do not edit blind. If SymForge is unavailable, FALL BACK to
  ripgrep + `git grep` + cargo-test-scoped navigation to resolve symbols -- never
  edit against a stale line number blind.
[verify] "Verified" = actual gates pass + live TEST-socket behavior for user-facing
  changes. Code existing / rendering / green build is NOT verified.

==============================================================================
OPERATING RULES -- THE PER-PHASE LOOP (non-negotiable; run for 4a, 4b, 5, 6a, 6b)
==============================================================================
For EACH sub-phase, in order: 4a -> [P1 pause] -> 4b -> 5 -> 6a -> [P2 pause] -> 6b.

  1. RE-ANCHOR. Open the phase's AMENDED plan. For EVERY symbol in that plan's
     re-anchor table, use SymForge (search_symbols / get_symbol /
     get_symbol_context / find_references) to resolve its CURRENT range and
     RE-VERIFY each stated CLAIM before touching anything. If any claim is false
     now, STOP and re-plan that item (note it; do not guess).
  2. IMPLEMENT with the right specialist. Dispatch rust-pro for the Rust edits
     (prefer SymForge structural edits -- replace_symbol_body / edit_within_symbol /
     insert_symbol). database-architect is NOT needed. Stay within the plan's named
     files for that sub-phase.
  3. REVIEW. SymForge-first code review (code-reviewer, read-only). FOR
     SECURITY-TOUCHING SUB-PHASES (4a redactor; 6a stop/kill/policy/audit) ALSO run
     security-reviewer (read-only) and produce its findings -- then HALT for the
     human pause (P1 after 4a; P2 within 6a before 6b). Resolve review blockers
     before gating.
  4. GATE -- both OS, EXACT commands (SYMMETRIC: run the same fmt/clippy/unit
     coverage on BOTH OSes so "both gates green" means the same thing; clippy on each
     OS also compiles that OS's cfg-gated paths -- cfg(unix) PTY arm vs cfg(windows)
     spawn -- so a cfg-gated break is caught. This mirrors what Phase 3 actually did):
       Windows:  pwsh -File scripts\windows-gate.ps1
                 cargo fmt --all --check
                 cargo clippy -p <touched-crates> --all-targets -- -D warnings
                 cargo test  -p <touched-crates>   (portable unit/integration tests; nextest is not installed on Windows)
       WSL:      wsl bash -lc "cd /mnt/e/project/terminal-commander && \
                   export CARGO_TARGET_DIR=$HOME/tc-linux-target && \
                   cargo fmt --all --check && \
                   cargo clippy -p <touched-crates> --all-targets -- -D warnings && \
                   cargo nextest run -p <touched-crates> && \
                   <run the linux-gate MCP guard 1 + guard 2 greps over crates/mcp/src>"
     Map <touched-crates> to the crates the sub-phase edited (e.g. 4a:
     terminal-commander-ipc + terminal-commanderd; 4b: terminal-commander-mcp +
     terminal-commander-cli; 5: terminal-commanderd (+ipc if SelfCheckResponse
     line changes); 6a: terminal-commander-probes + terminal-commanderd +
     terminal-commander-ipc; 6b: terminal-commander-mcp). Do NOT run
     `nextest --workspace` (drvfs session_reap wedge exemption). The MCP-guard greps
     are MANDATORY for any sub-phase touching crates/mcp/src (4b, 6b). Honor the
     wsl-wedge protocol.
  5. LIVE VERIFY (user-facing changes). Stand up a TEST daemon (own TC_SOCKET +
     temp data dir) and prove the phase's EVIDENCE bullet from [S1]/[S2]/[S3]
     actually happens. Never the live machine daemon. Capture the output as
     evidence for CAMPAIGN-RESULTS.md.
  6. COMMIT. ONE conventional commit for the sub-phase (commit message prefix per
     each plan, e.g. 4a `fix(daemon): ...`, 4b `fix(mcp): ...`) with the
     `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` footer.
     `git -C E:\project\terminal-commander`. NO push.
  7. DRIFT-AMEND THE NEXT PLAN. Re-anchor the NEXT subsequent AMENDED plan against
     the now-shifted tree, update its as_of line hints / claims for the new drift,
     and COMMIT that amendment (docs(campaign): ...) BEFORE starting the next phase.
     (6b has no successor.)

Phase-specific reminders the plans flag (do not lose these):
  - 4a SECURITY: redact_argv_head must mask EVERY listed pattern with no bypass via
    casing / `=` vs space / short-vs-long flag / URL userinfo; mask the SECRET span
    only, keep program + flag name visible; 128B truncate AFTER masking. -> P1.
  - 5: SelfcheckNoop short-circuit MUST precede resolve_config in main() (a child
    inheriting a broken --config/env must NOT false-RED); cached bucket is
    parking_lot/OnceCell, never tokio Mutex across .await; spawn argv is the VALID
    subcommand `[exe, "selfcheck-noop"]`, never `--selfcheck-noop`; never false-RED
    on a profile that legitimately skips, never false-GREEN on real breakage.
    DEDUP INTERACTION (verify at re-anchor): the self_check spawn goes through
    CommandRuntime, which since Phase 2 (cf43d5b) has an in-flight dedup guard
    (nonce-preferred, else peer-scoped argv+cwd+tag, 3s TTL). A repeated identical
    self-check spawn would COLLAPSE into the prior job and NEVER actually run --
    silently re-breaking the false-green. The self_check spawn MUST pass a FRESH
    dedup_nonce each call (or be explicitly dedup-exempt). Cover with a test: two
    back-to-back self_checks within 3s each actually spawn (distinct job_ids).
  - 6a: command-waiter terminal-guard goes AFTER receipt-publish, BEFORE the draft
    match (Phase 2's dedup-evict is unrelated -- do not conflate). CommandRuntime::
    stop deny-first ordering must NOT leak job_id on a denied stop. -> P2.
  - 6b: RE-COUNT live tools first; bump BOTH tools.rs count strings; command_stop
    sorts BETWEEN command_status and event_context in router/stdio/live lists; the
    37->38 churn is ATOMIC across all sites the plan enumerates.

Agent routing: rust-pro (implementation), code-reviewer (read-only review),
security-reviewer (read-only, 4a + 6a), test-runner (run/repair the targeted gate
tests if they break), debugger (only if a live round-trip misbehaves), git-master
(commits to fix/tc-trust-defects ONLY -- HARD-GATED, never push). Throttle heavy
cargo to <=2 concurrent; these phases are sequential anyway (shared files + ordered
drift), so prefer serial execution.

==============================================================================
FINAL DELIVERABLE (campaign end)
==============================================================================
Write .planning\tc-bugfix-campaign\CAMPAIGN-RESULTS.md (honest, ASCII), containing:
  - Per phase (4a, 4b, 5, 6a, 6b): what changed, files touched, the EVIDENCE bullet
    proven, both-OS gate outputs (test counts), the live TEST-socket result, the
    commit SHA, and the review verdict (incl. the human P1/P2 sign-off record).
  - The drift-amend commit SHAs.
  - The final live-tool count (37 -> 38) and the sites updated.
  - A Known-Gaps / Unverified / Deferred section (label mock-as-mock, blocked-as-
    blocked; e.g. the workspace-nextest drvfs exemption; README stale refs deferred
    per the TC-3 plan).
  - A closing line: NOTHING pushed; branch fix/tc-trust-defects awaits the human to
    push (release-please will fire on the fix/feat commits -- coordinate with the
    operator).
Then STOP. Do not push. Do not merge. Do not open a PR.

Checklist (running log -- tick as you go):
  [ ] 4a re-anchor   [ ] 4a implement  [ ] 4a code+security review  [ ] 4a both-OS gates
  [ ] 4a live verify [ ] 4a commit ___  >>> P1 SECURITY PAUSE (human sign-off) <<<
  [ ] 4b re-anchor (intra-plan: re-anchor only, NO amend commit)
  [ ] 4b implement   [ ] 4b review  [ ] 4b both-OS gates (+MCP guards)
  [ ] 4b live verify [ ] 4b commit ___   << TC-4 plan COMPLETE
  [ ] amend TC-5 re-anchor + commit ___   (cross-plan: TC-4 done -> amend TC-5)
  [ ] 5 re-anchor    [ ] 5 implement   [ ] 5 review   [ ] 5 both-OS gates
  [ ] 5 live verify  [ ] 5 commit ___    << TC-5 plan COMPLETE
  [ ] amend TC-3 re-anchor + commit ___   (cross-plan: TC-5 done -> amend TC-3)
  [ ] 6a re-anchor   [ ] 6a implement  [ ] 6a code+security review  [ ] 6a both-OS gates
  [ ] 6a live verify [ ] 6a commit ___ >>> P2 SECURITY PAUSE (human sign-off) <<<
  [ ] 6b re-anchor (intra-plan: re-anchor only, NO amend commit)
  [ ] 6b RE-COUNT tools  [ ] 6b implement (atomic 37->38)
  [ ] 6b review      [ ] 6b both-OS gates (+MCP guards)  [ ] 6b live verify  [ ] 6b commit ___
  [ ] CAMPAIGN-RESULTS.md written  [ ] STOP (nothing pushed)

----------------------------------------------------------------------------
ASSUMPTIONS (resolve before launch if any is wrong)
----------------------------------------------------------------------------
A1. The task said "root CLAUDE.md (project conventions)" but this repo has NO root
    CLAUDE.md. The project conventions live in AGENTS.md + CONTRIBUTING.md +
    TESTING.md; this goal references those instead. Confirm that is the intended
    convention source.
A2. HEAD is 17646b4 (the amendment commit), not the e76ebdc the plans cite as their
    as_of anchor -- 17646b4 IS the commit that re-anchored against e76ebdc plus the
    earlier Phase 0-3 work. Treated as expected; the mandatory per-phase re-anchor
    covers any residual drift. Confirm 17646b4 is the correct campaign HEAD.
A3. The Phase 3 commit SHA is recorded as cca9f06 (from the git log) where
    RESUME-STATE says "see the fix(mcp) commit"; used cca9f06 as TC-1b+TC-6.
A4. "Both OS gates" on WSL = the TARGETED commands above (not the full linux-gate.sh,
    which runs `nextest --workspace` and would hit the documented session_reap drvfs
    wedge). windows-gate.ps1 is run verbatim. Confirm the targeted-WSL substitution
    is the accepted gate for these phases (it matches the Phase 2-3 exemption).
A5. P1/P2 "human sign-off" means a real human approves; an autonomous overnight run
    must BLOCK and wait at those pauses rather than self-approve. Confirm a human
    will be available, or accept that the run halts at P1.
