# External Code Review - Remediation Results

Date: 2026-06-17
Source review: `docs/reviews/2026-06-17-code-review-findings.md`
Branch: `feature/omni-review-fixes` @ `019ddcd` (local; NOT pushed, NOT merged to main)
Method: 4 file-disjoint git worktrees, TDD per fix, SymForge for navigation, WSL
verification gate; integrated via 4 `--no-ff` merges (zero conflicts).

## Capstone verification (integrated branch, WSL)

```
cargo fmt --all -- --check                          -> clean
cargo clippy --workspace --all-targets -- -D warnings -> clean
cargo nextest run --workspace                        -> 955 passed, 1 skipped
```

(955 vs the 943 baseline = ~12 new TDD tests added by these fixes. The 1 skip is
the pre-existing `command_status_lifecycle::helper_child_emit_then_linger`
helper, not introduced here.) F-001's Windows-only `cfg(windows)` ConPTY edits
were additionally validated by a Windows-native `cargo clippy -p
terminal-commander-probes --all-targets -- -D warnings` (exit 0), since the WSL
run does not compile that module.

## Per-finding disposition

| ID | Sev | Status | Evidence |
|----|-----|--------|----------|
| F-001 | HIGH | FIXED + VERIFIED | Typed `WriterReply{Written,SecretDenied,Io}` + pure `map_writer_reply`; writer-path secret deny now -> `WriteStdinError::SecretInputActive` (-> IPC `SecretInputDenied`) + increments `stdin_writes_denied_secret`. unix+win reply shapes unified. 3 cross-platform mapping tests (TDD red proven) + Windows-native compile. Commit `21de53f`. **Invariant III restored.** |
| F-002 | MED | FIXED + VERIFIED | Atomic `SessionTable{entries,pending}` under one `RwLock`; `reserve_slot()` checks `live+pending>=max` and bumps `pending` in one critical section; release-on-spawn-failure. 64-thread no-overshoot test (TDD red proven). Commit `0183193`. **Invariant II restored.** |
| F-003 | MED | FIXED + VERIFIED | `command::redact_env_pairs` reuses the existing `mask_token_inline` heuristic; env masked before snapshot persist AND in `shell_session_status`. e2e: `TOKEN=supersecretvalue` appears nowhere in the snapshot row nor status (TDD red proven). False "no secrets leak" comments corrected. Commit `0183193`. **Invariant V restored.** |
| F-008 | LOW | FIXED + VERIFIED | `LimitReached{live,max}`; Display reports both honestly. Test asserts live distinct from cap. Commit `0183193`. |
| F-006 | MED | FIXED + VERIFIED | 4 new `mcp-tools/subscription_{open,pull,list,close}.v1.json` fixtures grounded in the real protocol/tool/handler structs; fixture-map `covered_live 45->49`, `missing_fixture 4->0`. Contract test green; tool count still 49. Commit `68ffea0`. |
| F-004 | LOW | DOCUMENTED | Confirmed in code: `session_runtime_available()=cfg!(unix)` -> `omni_status.sessions.available=false` on Windows; reason string rides per-tool catalogue entries. Noted in `TOOL_CONTROL_SURFACE.md`. Commit `8de3d2c`. |
| F-005 | LOW | DOCUMENTED | Windows ConPTY forced-kill-only vs unix grace ladder noted in `SHELL_SESSION.md`. `8de3d2c`. |
| F-007 | LOW | DOCUMENTED | `status.cwd` labeled advisory/best-effort in `SHELL_SESSION.md`. `8de3d2c`. |
| F-009 | LOW | DOCUMENTED | CRLF split-read overwrite known gap + `stty -onlcr` rationale in `SHELL_SESSION.md`. `8de3d2c`. |
| F-011 | NIT | DOCUMENTED | `compact:true` drops ids; re-fetch note in `OMNI_PLAYBOOK.md`. `8de3d2c`. |
| F-012 | LOW | DOCUMENTED | `target_id` command-lane-only noted in `TOOL_CONTROL_SURFACE.md` / `OMNI_PLAYBOOK.md`. `8de3d2c`. |
| F-013 | LOW | DOCUMENTED | Universal extractors fallback-only (no baseline alongside an active pack) noted in `OMNI_PLAYBOOK.md` + config comment. `8de3d2c`. |
| F-010 | MED | CI GATE ADDED (closes on green) | `scripts/windows-gate.ps1` runs `conpty_*` with `TC_CONPTY_E2E=1` when `GITHUB_ACTIONS=true` (the existing required `pre-build-gates (windows-x64)` job). Refuses self-skip. Local `windows-gate.ps1` skips unless `TC_CONPTY_E2E=1` is set manually. F-010 closes when that CI job goes green on `windows-2022`. |
| F-014 | NIT | NO ACTION | Job-receipt design confirmed sound (UUIDv7 job_id, live jobs read in-memory first, receipt only on `UnknownJob`). |

## Per-invariant outcome

- II default-deny caps: RESTORED under concurrency (F-002).
- III honest/typed/bounded refusal: RESTORED on the Windows writer path (F-001).
- V audit + redaction: RESTORED for session env (F-003); false comments corrected.
- I, IV, VI, VII: held (unchanged; not touched by these fixes).

## Integration state and remaining steps (human-gated)

- Branch `feature/omni-review-fixes` rebased onto `origin/main` (v0.1.50 / `ddbe964`).
  Local only — not pushed. Recovery anchor `_anchor/pre-omni-integration` -> `9c87923`.
- F-010 closes when the PR's `pre-build-gates (windows-x64)` job passes (ConPTY e2e
  in `windows-gate.ps1`). Merge the PR to land fixes + the gate together.

## Honesty notes

- F-006 fixtures: the contract test validates fixture presence/meta/counts, not
  serde-deserialization of the example payloads (grounded by hand against the
  structs).
- F-002 concurrency test is deterministic (exercises the `try_reserve` critical
  section under the real lock), not a real-PTY parallel-spawn stress test.
- F-001 live ConPTY path remains unverified on this host (= F-010).
