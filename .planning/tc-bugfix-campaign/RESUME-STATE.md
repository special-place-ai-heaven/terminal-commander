# Campaign resume state (as_of 2026-06-08)

Branch: fix/tc-trust-defects (base 725e223). NOTHING pushed. NO push/PR/merge
without explicit human approval.

## Done (committed)
- Phase 0 docs: commit 73b6450
- Phase 1 retry-gate (TC-1a): commit a012fa8 -- reviewed APPROVE, both OS gates PASS
- Phase 2 dedup guard (TC-2): commit cf43d5b -- reviewed APPROVE, both OS gates PASS
- Phase 3 wait-loop rewrite (TC-1b + TC-6): cca9f06 -- APPROVE-WITH-NITS, both OS gates PASS
- Phase 4a probe-row identity (TC-4): 3b7e719 -- code review APPROVE-WITH-NITS;
  security review found 3 HIGH + 1 MED under-redaction leaks, all fixed, re-review
  P1-READY; HUMAN P1 SIGN-OFF GRANTED. Both OS gates PASS; live runtime_state IPC
  round-trip proves tag + redacted argv_head on a TEST daemon.
- Phase 4a audit-log redaction (TC-4, P1-directed add-on): 7fd0ec8 -- the human P1
  owner directed extending redaction to the raw-argv audit-log surface
  (format_argv_metadata). Security-confirmed; live test proves the persisted
  command_start audit row masks a --password value at argv index 3.
- Phase 4b run_and_watch tag + CLI render (TC-4): c91200e -- code review APPROVE
  (0 blockers); both OS gates PASS; live MCP e2e proves a tagged run_and_watch
  surfaces the tag + redacted argv_head in runtime_state. TC-4 COMPLETE.
- TC-5 re-plan (cached-bucket): 6af4909 -- start_combed split into a private
  start_combed_inner(req, reuse_bucket) + start_combed (None) + start_combed_reusing,
  the only mechanism that gives "+1 bucket over lifetime" without leaking an
  immortal bucket per call (start_combed always minted a fresh bucket).
- Phase 5 self_check real spawn (TC-5): d136e95 -- async handle_self_check does a
  profile-gated REAL spawn of the hidden selfcheck-noop leaf through the normal
  CommandRuntime path into the cached bucket, polls to terminal, failures>0 only on
  real breakage, SKIPS (never false-RED) on policy Deny; fresh dedup_nonce per call;
  SelfcheckNoop short-circuits before resolve_config. Code review APPROVE-WITH-NITS
  (0 blockers). Both OS gates PASS (Windows lib 125 + subcommand; WSL nextest
  305/305). Live: healthy spawn-ok, +1 bucket reuse, read_only SKIP, forced-broken
  failures>0, selfcheck-noop exits 0, back-to-back distinct job_ids. TC-5 COMPLETE.
- Gate-hygiene lint fix: bb5b41d -- pre-existing from_millis(5000)->from_secs(5) in
  a cfg(unix) mcp e2e test (clippy duration_suboptimal_units), surfaced only by the
  WSL clippy of the mcp crate (Windows clippy compiles cfg(unix) test bodies away).

GATE-DISCIPLINE LESSON (campaign-wide): the Windows clippy gate is BLIND to
`#![cfg(unix)]` test files (their bodies compile to nothing on Windows). The WSL
clippy run on the touched crate is the AUTHORITATIVE linter for cfg(unix) code. Any
phase touching a crate with cfg(unix) e2e tests MUST run WSL `clippy -p <crate>
--all-targets -D warnings` and treat it as the real gate. Also: `nextest -p
terminal-commander-cli` pulls in the `session_reap` drvfs-wedge test -- exclude it
with `-E "not binary(session_reap)"` (the Phase 2-3 exemption extended to cli).

## Phase 3 summary (TC-1b lost-job-handle + TC-6 wait-cap self-violation)
ONE wait-loop rewrite in crates/mcp/src/tools.rs::run_and_watch:
- TC-6: wall-clock `Instant` deadline; per-slice bucket_wait timeout =
  min(MAX_WAIT_SLICE_MS=1000, remaining); do-while loop preserves the old
  `.max(1)` >=1-poll guarantee; final non-blocking drain on non-terminal
  deadline-exit. MAX_WAIT_SLICE_MS stays 1000ms (load-gate safe).
- TC-1b: both post-job_id error arms (CommandStatus, BucketWait) now return a
  degraded, success-shaped, job-identified result (job_id+bucket_id+cursor
  preserved, degraded:true, recover_hint, state=last-observed or "unknown" --
  never silent Running) via ONE shared builder run_and_watch_result. Start-arm
  still returns Err (no job_id yet).
- Shared builder makes normal + degraded payloads a strict superset
  (cursor/degraded/recover_hint on both). collect_rule_signals extracted.
- New const RUN_AND_WATCH_RECOVER_HINT. #[tool] description updated.
- Tests: 3 inline unit tests (builder shape/honesty/superset); 2 live e2e
  (wall-clock cap at sleep5/wait_ms=1500 returns <1900ms wait_exhausted; fast
  command complete+not-degraded). Fixture run_and_watch.v1.json: additive
  cursor/degraded/recover_hint examples + invariants.

### Phase 3 verification (all green)
- Windows: cargo fmt --all --check CLEAN; clippy -p mcp --all-targets -D CLEAN;
  cargo test -p mcp --lib 64/64 PASS; windows-gate.ps1 PASSED (probes
  windows_no_console_spawn + daemon windows_spawn_site_coverage 3/3).
- WSL (Linux): cargo nextest -p terminal-commander-mcp 123/123 PASS (incl 3 new
  unit + 2 new e2e + stdin_eof_survives once daemon bin built + existing
  run_and_watch e2e). Daemon bin builds clean.
- MCP guards (linux-gate greps): crates/mcp/src has only doc-comment matches for
  guard-1 set and ZERO guard-2 matches; new code uses std::time, not std::fs.
- External review (Cursor): APPROVE WITH NITS, 0 blockers. Nit #1 (stale
  MAX_WAIT_SLICE_MS comment) FIXED. Nits #2 (final-drain swallows transport err,
  best-effort by design) and #3 (no live fault-injection for degraded wiring;
  covered by builder unit tests + retry_gate transport tests + inspection)
  ACCEPTED. Report: .planning/tc-bugfix-campaign/cursor_review.md.
- NOT run: full `nextest --workspace` (the session_reap CLI test wedges on /mnt/e
  drvfs -- pre-existing, unrelated; Phase 2 took the same exemption). This diff is
  mcp-only (leaf crate); other crates unaffected (diff_symbols confirms only
  crates/mcp + the fixture changed).

## Disk
- 2026-06-08: cargo clean run. Removed shared /mnt/e target (20.7 GiB) +
  ~/tc-linux-target (55 GiB). Next build (Phase 4) is COLD. AAP target left
  alone (in use by another project).

## Next steps on resume
USE THE *-AMENDED.md plans (re-anchored 2026-06-08 vs e76ebdc; symbol-anchored so
they survive line drift). The pre-amend originals are kept for lineage only.
1. Phase 4a/4b (TC-4): per PLAN-TC4-probe-identity-AMENDED.md. 4a = wire fields +
   NEW argv redactor + collect_probes (SECURITY review on the redactor); 4b = mcp
   tag plumbing + CLI render + fixtures.
2. Phase 5 (TC-5): per PLAN-TC5-selfcheck-spawn-AMENDED.md. Note: SelfcheckNoop
   short-circuit MUST precede resolve_config in main(); cached bucket =
   parking_lot/OnceCell (not tokio); do NOT touch runtime.rs run_self_check.
3. Phase 6a/6b (TC-3): per PLAN-TC3-command-stop-AMENDED.md. Note: waiter guard
   goes after receipt-publish, before the draft match; tools.rs has TWO count
   strings; RE-COUNT the live-tool baseline before the 37->38 churn.

## Drift convention (campaign-wide)
Absolute line numbers re-drift after EVERY phase lands. Plans anchor on SYMBOLS;
line numbers are `as_of <ref>` hints. MANDATORY per-phase step: at phase start,
re-anchor every symbol via SymForge and re-verify each claim BEFORE editing; after
a phase is coded+tested+committed, re-anchor + amend the NEXT plan (commit the
amendment) before starting it.

## Standing rules
- Each phase: code -> SymForge/external review -> both OS gates -> separate commit.
- NEVER touch the live machine daemon (tests use own TC_SOCKET + data dir).
- Zero new deps; crates/mcp/src free of CI-guard literals (incl. comments).
- WSL gate wedge protocol: if idle >3 min zero CPU, kill only PIDs whose cwd =
  this repo. linux-gate uses CARGO_TARGET_DIR=~/tc-linux-target.
- NO push/PR/merge without explicit human approval.
