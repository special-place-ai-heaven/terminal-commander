# Campaign resume state (as_of 2026-06-08)

Branch: fix/tc-trust-defects (base 725e223). NOTHING pushed. NO push/PR/merge
without explicit human approval.

## Done (committed)
- Phase 0 docs: commit 73b6450
- Phase 1 retry-gate (TC-1a): commit a012fa8 -- reviewed APPROVE, both OS gates PASS
- Phase 2 dedup guard (TC-2): commit cf43d5b -- reviewed APPROVE, both OS gates PASS
- Phase 3 wait-loop rewrite (TC-1b + TC-6): committed -- see the fix(mcp) commit.

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
1. Phase 4 (TC-4): probe identity -- ProbeListEntry additive argv_head/tag,
   collect_probes enrichment, into_parts tag fix, NEW argv redactor (mask
   -H Authorization etc; format_argv_metadata only truncates), CLI render.rs.
   Per PLAN-TC4-probe-identity.md.
2. Phase 5 (TC-5): self_check spawn -- async handle_self_check, hidden clap
   Cmd::SelfcheckNoop subcommand, one cached immortal bucket, profile-gated
   skip-not-fail, negative test. Per PLAN-TC5-selfcheck-spawn.md.
3. Phase 6a/6b (TC-3): command_stop -- retain cancel handle, IpcRequest::
   CommandStop, CommandRuntime::stop with policy gate, command waiter terminal
   guard; then command_stop MCP tool + all tool-count fixtures (37->38).
   Per PLAN-TC3-command-stop.md.

## Standing rules
- Each phase: code -> SymForge/external review -> both OS gates -> separate commit.
- NEVER touch the live machine daemon (tests use own TC_SOCKET + data dir).
- Zero new deps; crates/mcp/src free of CI-guard literals (incl. comments).
- WSL gate wedge protocol: if idle >3 min zero CPU, kill only PIDs whose cwd =
  this repo. linux-gate uses CARGO_TARGET_DIR=~/tc-linux-target.
- NO push/PR/merge without explicit human approval.
