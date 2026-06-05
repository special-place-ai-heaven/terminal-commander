# PLAN-TC5 -- self_check real command-spawn round-trip (Phase 5)

**Source:** TC trust-defects campaign (`plan-final.json` Phase 5 / fork F5) +
`review-verdict.json` required amendment #2 (hidden clap SUBCOMMAND
SelfcheckNoop, NOT a --flag; positive exits-0 test) + required amendment #5
(DELETE the phantom dispatch_envelope second await site -- single dispatch await
at server.rs:541).
**Posture:** Host the real spawn round-trip in the IPC `handle_self_check` (the
surface whose false-green misleads LIVE clients). Make it `async`, propagate
`.await` to EXACTLY ONE call site. Reuse ONE cached immortal self-check bucket.
Profile-gate the spawn to skip-or-assert-deny (NEVER false-RED). Spawn
current_exe() as a VALID hidden subcommand. Add a NEGATIVE test (failures>0 on
real breakage) AND a positive subcommand exits-0 test.

Language: ASCII only.

---

## Summary table

| Symptom | Location (file:line) | Fix sketch | Effort | Test impact |
|---------|----------------------|------------|--------|-------------|
| handle_self_check hardcodes failures:0, never spawns | `crates/daemon/src/ipc/server.rs:801-818` | Make async; add a profile-gated bounded real round-trip via the normal CommandRuntime path | **M** | integration (spawn-ok line, failures==0 healthy); negative (failures>0 on breakage) |
| dispatch arm is sync | `crates/daemon/src/ipc/server.rs:540-543` | Add `.await` at THIS ONE site (server.rs:541) | **S** | compile/build (both OS) |
| (PHANTOM) dispatch_envelope needs a second .await | `crates/daemon/src/ipc/server.rs:821-828` | **DELETED** -- dispatch_envelope is a one-line delegate; NO SelfCheck arm; zero change | **0** | verify Windows build compiles (expect zero source change in pipe_server/dispatch_envelope) |
| buckets are immortal; a per-call bucket would litter | `crates/daemon/src/.../source.rs:12-17` (no drop_bucket) | Cache ONE self-check bucket on DaemonState (Mutex<Option<BucketId>> / OnceCell), reuse every call | **S** | integration (bucket_count grows by exactly 1 over daemon lifetime) |
| **a bare --selfcheck-noop flag is rejected by clap** | `crates/daemon/src/main.rs:28-75` (required closed subcommand set) | Add hidden `Cmd::SelfcheckNoop` subcommand; spawn `[exe, "selfcheck-noop"]` | **M** | positive test ("selfcheck-noop" exits 0) |

**Estimated files:** 5: `crates/daemon/src/ipc/server.rs`,
`crates/daemon/src/state.rs` (cached bucket holder),
`crates/daemon/src/main.rs` (hidden subcommand), `crates/ipc/src/protocol.rs`
(SelfCheckResponse report string only -- no wire change), plus the test module.

---

## Per-item detail

### TC-5 -- self_check never exercises a real spawn (false-green)

**Symptom:** `handle_self_check` hardcodes `failures:0` and never spawns a
command, so a live client polling self_check during a real outage (e.g. the
TC-1/TC-6 window) gets a false GREEN. The dispatch arm is sync; buckets are
immortal.

**Citations:**

```801:818:crates/daemon/src/ipc/server.rs
// handle_self_check: static report, hardcoded failures:0, no spawn
```

```540:543:crates/daemon/src/ipc/server.rs
// the SOLE handle_self_check call, inside dispatch()
```

```821:828:crates/daemon/src/ipc/server.rs
// dispatch_envelope body = dispatch(state, boot, req_env, peer).await -- NO SelfCheck arm of its own
```

```28:75:crates/daemon/src/main.rs
// #[derive(Parser)] Cli with a REQUIRED #[command(subcommand)] cmd: Cmd over a CLOSED set {Check, Start, PrintConfig, Update}
```

**Fix:**

1. **Make handle_self_check `async fn`** (server.rs:801-818). Keep the static
   lines (data_dir / policy_profile / audit / audit_count) and ADD a
   profile-gated bounded real round-trip: if policy allows CommandStart for the
   active profile AND command_allow_roots does not exclude the daemon binary AND
   (repo_only has a resolvable root OR the profile is not repo_only) -> spawn
   `std::env::current_exe()` in the hidden self-check subcommand via the normal
   CommandRuntime path into the CACHED self-check bucket, poll command_status to
   terminal within a ~2s budget, set `failures>0` + an explanatory report line
   ONLY on genuine breakage (spawn error / never terminal / nonzero exit); ELSE
   SKIP and emit `spawn probe skipped: <profile reason>` with failures unchanged
   (NEVER false-RED).

2. **Single dispatch await (amendment #5 -- delete the phantom):** add `.await`
   at EXACTLY ONE place, the SOLE call site server.rs:541. dispatch_envelope
   (server.rs:821-828) is a one-line delegate -- its entire body is
   `dispatch(state, boot, req_env, peer).await` -- with NO SelfCheck arm; the
   Windows named-pipe server reaches SelfCheck transitively through dispatch,
   which already awaits the (now-async) dispatch. So ZERO changes to
   dispatch_envelope / pipe_server. Verify the Windows build compiles, but expect
   zero source change there. The "BOTH dispatch sites" graft was a misread of the
   delegation chain and is removed.

3. **Hidden clap SUBCOMMAND, not a flag (amendment #2):** spawning current_exe()
   with a bare `--selfcheck-noop` flag would make clap error on the unknown arg
   AND the missing required subcommand -> the child exits nonzero on a HEALTHY
   daemon -> self_check false-REDs (violating the never-false-RED invariant). On
   Windows it is worse: `windows_subsystem=windows` (main.rs:4) means no console
   for the clap stderr, but it still exits nonzero. Instead:
   - add `Cmd::SelfcheckNoop` with `#[command(hide = true)]` to the Cmd enum
     (`crates/daemon/src/main.rs`), handled at the TOP of main() to return
     `ExitCode::SUCCESS` BEFORE any further work; ignore all other argv/env (an
     inert leaf mode, no socket/fs/policy work -- the no-bypass invariant the
     rejected policy-security finding confirms is already satisfied because the
     handler hardcodes the argv);
   - spawn current_exe() with argv `[exe, "selfcheck-noop"]` (a VALID
     subcommand), NOT `--selfcheck-noop`;
   - reconcile the main.rs module-doc / long_about (main.rs:11-18, 36-37 "Does
     NOT spawn child commands by itself") with the new internal self-probe
     target.

4. **Cached immortal bucket:** add a cached self-check bucket holder
   (`Mutex<Option<BucketId>>` or OnceCell) to DaemonState (`crates/daemon/src/state.rs`),
   lazily created on the first spawn-running self_check and reused every call
   (honors the immortal-bucket invariant source.rs:12-17; BucketSourceTable has
   no remove). A drop_bucket seam is DEFERRED (larger blast radius).

5. **SelfCheckResponse** (`crates/ipc/src/protocol.rs`) is structurally unchanged
   `{report, failures}`; the report string gains a spawn-probe line (no wire
   change).

**Effort:** M. **Test:**
- integration through daemon IPC (TEST socket, DeveloperLocal profile): self_check
  spawns the noop round-trip, report shows the spawn-ok line, failures==0 on a
  healthy daemon; a SECOND self_check reuses the SAME bucket (bucket_count grows
  by at most 1 over the daemon lifetime). source-status: live.
- integration: under read_only_observer profile, self_check SKIPS the spawn and
  stays failures==0 (NOT RED) with `spawn probe skipped: <reason>`. source-status:
  live.
- negative (TC-5 acceptance, the judge-graft proof it is no longer a hardcoded
  green): force the round-trip to fail (a test build where the noop exits nonzero,
  or an impossible argv on a dev profile) and assert failures>0 with an
  explanatory line. source-status: live.
- positive (amendment #2): assert `terminal-commanderd selfcheck-noop` exits 0,
  so a future clap refactor that breaks the subcommand is caught instead of
  silently false-REDing the whole daemon. source-status: live.

---

## Invariants (Phase 5)

- TC-5 self_check NEVER false-REDs a healthy daemon: profile-gated to
  skip-or-assert-deny under read_only_observer / restrictive command_allow_roots /
  repo_only-no-root; reuses ONE cached immortal self-check bucket (no
  drop_bucket); bucket_count grows by exactly 1 over the daemon lifetime; a forced
  round-trip failure DOES yield failures>0 (negative test).
- The self-check spawn target is a VALID hidden clap subcommand (SelfcheckNoop),
  an inert leaf mode exiting 0 before any arg/env/socket work; spawned as
  `[exe, "selfcheck-noop"]`, NOT a bare flag.
- handle_self_check has a single call site (server.rs:541); making it async adds
  `.await` there and ZERO change to dispatch_envelope/pipe_server.
- The spawn routes through the normal CommandRuntime path so it inherits
  validate_argv + the shell guard + policy.evaluate and emits a command_start
  audit row (no policy bypass).
- The LIVE machine daemon is NEVER restarted/killed: all tests use their own
  TC_SOCKET + explicit data dir.
- No fake success: self_check returns failures>0 on REAL breakage.

## Verification (Phase 5)

- `wsl bash scripts/linux-gate.sh` (spawn path is cfg(unix)/cfg(windows);
  current_exe + CREATE_NO_WINDOW on Windows; windows_spawn_site_coverage stays
  >=1).
- `pwsh -File scripts/windows-gate.ps1`.
- `cargo nextest run -p terminal-commander-daemon -p terminal-commander-ipc`.
- manual: run self_check against a TEST daemon socket (NEVER the live machine
  daemon) and confirm a real command_start audit row is emitted; confirm
  bucket_count grows by exactly 1 across the daemon lifetime, not per call;
  confirm `terminal-commanderd selfcheck-noop` exits 0.
