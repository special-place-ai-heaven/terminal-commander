# Full-Spectrum Audit: Flakiness + Fragility (2026-05-27)

Scope: whole committed codebase at `b94c876`. 8 Rust crates (~24K LOC src,
61 src + 43 test files) + npm wrapper JS (~7K LOC, 23 test files).
Method: workspace `cargo clippy --all-targets -- -D warnings` (clean) +
4 parallel read-only Explore agents reading actual source via SymForge.
Two target classes: (1) test flakiness / nondeterminism, (2) code fragility.

No fixes applied. This is a findings report awaiting triage.

## Reconciliation (2-engine: Claude agents + Cursor, blind)

Cursor independently reviewed the same HEAD b94c876 and confirmed EVERY
CRITICAL + HIGH (F1-F7) and M1-M8 with line-verified citations, same
severities, same fix directions. Two findings Cursor caught that the first
pass missed or understated:

- **F2 scope widened.** Env-race is supervisor-WIDE, not just paths.rs.
  `crates/supervisor/src/ensure.rs:379-390` (`build_forward_env_forwards_only_allowlisted_vars`)
  also mutates process-global env (`WSLENV` + a secret var) and races the same
  global table. Total env-mutating tests = **6 across 2 modules** (5 paths.rs +
  1 ensure.rs). F2 fix must cover both.
- **M9 (NEW).** `crates/mcp/src/tools.rs:1983` unit helper uses a FIXED socket
  path `temp_dir()/tc-mcp-unavailable-unit-test.sock` (no pid/nonce). Low blast
  radius (unavailable-daemon test, socket never bound, collision still yields
  "unavailable") but a latent shared-path smell. First-pass mcp agent
  overstated "all mcp tests unique." Fix: unique per-test path.

The first pass caught nothing Cursor missed on the env-race scope; Cursor's
pass is a strict superset there. Agreement across two engines = high
confidence in F1-F7.

## Headline

- **clippy `-D warnings` workspace-wide: clean.** Flakiness/fragility is in
  logic + concurrency, not lint-catchable.
- **Production panic surface: essentially clean.** Across all crates, nearly
  every `.unwrap()/.expect()/panic!` is in `#[cfg(test)]`. Only 2 production
  `expect()` exist (core/bucket.rs) and both are provably unreachable.
- **The known flake `tc_data_env_overrides_everything` root is confirmed**
  (supervisor/paths.rs global env mutation) — and is currently **unmitigated**
  (no `--test-threads=1`, no `serial_test` in the repo).
- **Zero CRITICAL in mcp/core/sifters/store/JS.** One CRITICAL in daemon
  (Windows pipe-name collision). Two HIGH production bugs (pid-reuse kill,
  leaked pipe handle).

## Severity index

| # | Sev | Area | One-line |
|---|-----|------|----------|
| F1 | CRITICAL | daemon | Fixed Windows pipe name → instance collision + hot-spin retry + squat vector |
| F2 | CRITICAL | supervisor tests | Global env mutation, no serialization = the latent flake, unmitigated |
| F3 | HIGH | supervisor | pid-reuse TOCTOU: recycled pid force-killed (no image re-verify at kill) |
| F4 | HIGH | supervisor | `pid_alive` Windows check is substring match on `tasklist` (false alive) |
| F5 | HIGH | daemon | Pipe HANDLE leaked on `from_raw_handle` error path (unbounded w/ F1 retry) |
| F6 | HIGH | daemon | Pipe-create error always treated transient → retry forever, no cap/escalation |
| F7 | HIGH | sifters/core | Runtime regex compiled with NO size/dfa limit (import path bounds it; inconsistent) → memory-DoS |
| M1 | MED | daemon tests | Pipe-name test helpers keyed on PID only → collide if a 2nd test added per file |
| M2 | MED | daemon tests | Fixed pre-assert sleeps (pty/file/bucket) race child output under load |
| M3 | MED | core tests | Wakeup/timeout tests use real short timeouts (40/60ms) → CI-load flake |
| M4 | MED | supervisor/replace | probe→kill TOCTOU on daemon identity (same family as F3) |
| M5 | MED | cli | `WaitForSingleObject`: `WAIT_FAILED` reported as clean "stopped" |
| M6 | MED | mcp | `system_discover` leaks `Debug` enum shape into user-facing error |
| M7 | MED | JS | admin shim native spawn inherits full `process.env` (inconsistent w/ secret filtering) |
| M8 | MED | cli tests | `offline_truth` integration test reads ambient `TC_SOCKET` → dev-env dependent |
| L* | LOW | various | dead `bridge_required` branch, tautological context.rs cond, swallowed eviction error, parse_status silent Draft fallback, etc. (see below) |

---

## CRITICAL

### F1 — `crates/daemon/src/config.rs:261-268` — fixed Windows pipe name
`pipe_name()` = `\\.\pipe\terminal-commander-{USERNAME}`, no PID/nonce. Two
daemons for one user collide; second's `CreateNamedPipeW` with
`FILE_FLAG_FIRST_PIPE_INSTANCE` (pipe_acl.rs:121-123) fails →
`accept_loop` (pipe_server.rs:104-120) busy-retries every 100ms forever,
logging "transient error" instead of failing loud. A process squatting that
exact name also defeats FIRST_PIPE_INSTANCE.
**Fix:** include PID (and/or random nonce) in default pipe name; treat
non-first-instance bind failure as fatal, not transient. (Couples to F5/F6.)

### F2 — `crates/supervisor/src/paths.rs:122-240` — env-race flake, unmitigated
Tests `tc_data_env_overrides_everything`, `empty_tc_data_is_ignored`,
`empty_tc_socket_is_ignored`, `xdg_state_home_is_ignored_on_unix`,
`windows_socket_path_has_no_default_suffix` each do
`unsafe { set_var/remove_var("TC_DATA"/"TC_SOCKET"/"HOME"/...) }`. The
resolvers (`resolve_state_dir`/`resolve_socket_path`) read those same
process-global vars. Env is per-process → any co-scheduled test races the
set/remove window. **Repo has no `--test-threads=1`, no `serial_test`** — so
the session-note mitigation is NOT actually in the repo; tests pass today only
because cargo happens not to co-schedule the colliders. paths.rs:163 is worst:
`remove_var("TC_SOCKET")` with **no restore** → contaminates later tests.
**Fix (structural, preferred):** give resolvers a config-injection seam
(`resolve_state_dir_from(env: &EnvLike)`) so tests pass values.
**Fix (tactical):** serialize all env-touching tests behind a shared
`Mutex<()>` (or `serial_test`), and restore every var.

---

## HIGH

### F3 — `crates/supervisor/src/replace.rs:155-189` — pid-reuse TOCTOU
Between `read_pidfile` (155) and `hard_kill(pid)` (185) the original daemon can
exit and the OS recycle the pid. Windows kill is `taskkill /PID <pid> /F`
(85-87), no image guard → a reused pid (unrelated process) gets force-killed.
**Fix:** re-verify the pid's image is `terminal-commanderd` at kill time
(`find_daemon_pid_os` already filters by image+cmdline — reuse that guard).

### F4 — `crates/supervisor/src/pidfile.rs:59-79` — substring `pid_alive`
Windows liveness = `tasklist ... .contains(&pid.to_string())`, matches pid
digits in any column (mem usage, session id, superstring pid) → false "alive".
Feeds F3.
**Fix:** parse `tasklist /FO CSV` and compare the PID column exactly, or
`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION) + GetExitCodeProcess == STILL_ACTIVE`.

### F5 — `crates/daemon/src/ipc/pipe_acl.rs:128-149` — leaked pipe handle
`CreateNamedPipeW` returns a valid handle; if `NamedPipeServer::from_raw_handle`
(tail expr, 149) errors, the OS handle is never `CloseHandle`d. With F1's retry
loop this is unbounded kernel-handle leak.
**Fix:** `CloseHandle(handle)` on the `from_raw_handle` error path.

### F6 — `crates/daemon/src/ipc/pipe_server.rs:104-120` — retry forever
Any pipe-create error treated transient, looped at 100ms indefinitely with only
`eprintln!`. Misconfigured ACL/name-collision → silent hot-spin, never serves,
never exits (except shutdown).
**Fix:** distinguish fatal (`ERROR_ACCESS_DENIED`, first-instance
`ERROR_PIPE_BUSY`) from transient; bound retries; escalate fatal.

### F7 — `crates/sifters/src/lib.rs:224` + `crates/core/src/rule.rs:324` — unbounded regex compile
Runtime rule regex uses bare `Regex::new`/`RegexSet::new` with no
`size_limit`/`dfa_size_limit`, while `crates/store/src/import.rs:151-154`
correctly bounds both to 65536. A pattern under the 4096-byte length cap but
compiling to a large DFA is bounded only by the crate default (10MB) —
contradicts the `rule.rs:22-23` comment claiming the byte cap bounds DFA size.
(Note: NOT classic ReDoS — Rust `regex` is a finite automaton, backrefs/
lookaround rejected at rule.rs:412. This is a memory-DoS bound mismatch.)
**Fix:** compile via `RegexBuilder::size_limit().dfa_size_limit()` in
`build_inner`+`validate`, reusing import.rs constants.

---

## MEDIUM

- **M1** `crates/daemon/tests/pipe_accept_loop.rs:43`, `pipe_peer_identity.rs:46`
  — pipe names keyed on `process::id()` only. Distinct prefixes today; adding a
  2nd test to either file (shared binary PID) collides. **Fix:** per-test atomic
  counter/nanos, like the data-dir helpers already do.
- **M2** daemon tests `pty_ipc.rs:106,327,388`, `file_ipc.rs:364,369`,
  `ipc_bucket.rs:122,254,366`, `load_noise_backpressure.rs:1065` — fixed
  pre-assert `sleep(400/500/800ms)` before asserting on emitted child output;
  flakes under CI load / slow PTY spawn. (Poll-loop tests are fine.)
  **Fix:** poll-until-condition via bucket cursor / metrics readiness.
- **M3** `crates/core/src/bucket.rs:879-956` — wakeup/timeout tests use real
  40/60ms timeouts → fire before assertion under load. **Fix:**
  `tokio::time::pause()/advance()` for virtual time.
- **M4** `crates/supervisor/src/replace.rs:144-151` — probe→kill TOCTOU on
  daemon identity (same family as F3). **Fix:** re-verify at kill.
- **M5** `crates/cli/src/update_locks.rs:205-206` — `Ok(wait != WAIT_TIMEOUT)`
  reports `WAIT_FAILED`/`WAIT_ABANDONED` as clean "stopped pid N". Diagnostic
  only (TerminateProcess already succeeded). **Fix:** match `WAIT_OBJECT_0`.
- **M6** `crates/mcp/src/tools.rs:374-378` — `system_discover` puts
  `format!("unexpected response variant: {other:?}")` (Debug enum shape) into a
  user-facing `daemon_error`. **Fix:** stable `unexpected_variant` code.
- **M7** `packages/terminal-commander/bin/terminal-commander.js:270-273` — final
  native spawn omits `env`, inherits full `process.env` (incl. secrets), unlike
  the MCP shim (mcp.js:44) and update path (171). **Fix:** decide intentionally
  + document (same-host native inherit may be acceptable).
- **M8** `crates/cli/tests/offline_truth.rs:24-27` — spawns real CLI which
  resolves endpoint from inherited ambient `TC_SOCKET`/`TC_DATA`/`HOME`; a dev
  with a live daemon flips the "unavailable" (exit 69) assertions. **Fix:**
  `Command::env` an isolated unbound endpoint for the child.

---

## LOW

- `crates/daemon/src/server.rs:1444-1455` — `to_string().unwrap_or("null")` in
  audit metadata; fallback correct, swallow is benign.
- `crates/daemon/src/command.rs:602,639-642` (+ `pty_command.rs:397`) — final
  metrics/receipt dropped if stop races exit; `status()` returns zeroed metrics.
  Reconcile via persisted JobManager record or add comment/assert.
- `crates/store/src/registry.rs:486-493` — `parse_status` silently maps unknown
  status → `Draft` (sibling `parse_scope` errors). Corrupt column silently
  deactivates a rule. **Fix:** error like parse_scope.
- `crates/store/src/lib.rs:427` — `let _ = self.evict_expired(...).ok()` on read
  path swallows DB error. **Fix:** log at least once.
- `crates/core/src/context.rs:340-341` — tautological `truncated_before` first
  operand (always false); net result happens correct. **Fix:** delete operand.
- `crates/core/tests/load.rs:56-65` — `elapsed < 5.0s` wall-clock assertion;
  generous but host-contention flakeable. **Fix:** drop time assert (counts
  cover correctness).
- `crates/sifters/src/noise.rs:92` — `Dedupe::apply` reads `now_utc()` for GC
  cutoff while tests inject `ts`; latent flake if a test builds drafts older
  than the window. **Fix:** inject clock.
- `bin/terminal-commander.js:264`, `bin/terminal-commander-mcp.js:34` — dead
  `bridge_required` branch (resolver never returns it; asserted by tests).
  **Fix:** delete branch or restore intent.
- `lib/daemon/autostart.js:243` — `runAutostartOnce` return ignored; launch
  failure silent. **Fix:** log non-zero exit.

---

## Clean (verified, not assumed)

- **clippy** `--workspace --all-targets -- -D warnings`: zero warnings (WSL,
  stable 1.95).
- **Production panic surface:** all `.unwrap()/.expect()/panic!` in daemon,
  supervisor, cli, probes, core, sifters, store, mcp src are in `#[cfg(test)]`
  except 2 provably-unreachable `expect()` in core/bucket.rs:302,314.
- **IPC framing:** `framing.rs:20-27` + `server.rs:333-340` check
  `len > MAX_FRAME_BYTES` (256KiB) before allocating; no unbounded alloc from
  attacker length prefix.
- **Win32 FFI:** `cli/update_locks.rs` handles all RAII-wrapped (`OwnedHandle`
  + Drop→CloseHandle), no leak on any path; the `expect`s are infallible
  conversions. daemon `peer_windows.rs`/`pipe_acl.rs` SID buffer aligned via
  `Vec<u64>`, CloseHandle/LocalFree paired on every early-return — only defect
  is F5.
- **mcp daemon→MCP error mapping:** errors surfaced via `into_mcp_error`, never
  swallowed; response sizes bounded daemon-side.
- **mcp `#![cfg(unix)]` e2e:** unique per-test `tc-mcp-live-{tag}-{pid}-{nanos}`
  data dir + derived socket; no collision; no env mutation.
- **JS security:** WSL distro name double-whitelisted (`^[A-Za-z0-9._-]{1,64}$`),
  all spawns `shell:false` argv arrays, `bash -lc` arg is literal constant,
  secret-env filtered across WSL boundary, writes scope-guarded
  (`isPathInsideScope`), `resolve-binary.js` validates against
  `ALLOWED_BINARIES` within platform-package roots (no traversal).
- **JS tests:** no `process.env` mutation, time injected via `now:` callbacks,
  temp dirs `mkdtempSync` unique + cleaned, spawns wire `on("error")`, top-level
  async has `.catch()`.
- **No HashMap iteration-order assertions** (Captures is IndexMap; dedupe_key
  sorts).
- **No integer-overflow panics in ring/index math** (saturating + try_from
  unwrap_or MAX; SeqOverflow error).

---

## Recommended fix order

1. **F2** — kill the latent flake. Highest value, the literal "codebase is
   flaky" complaint. Structural injection seam preferred; test mutex as stopgap.
2. **F1 + F5 + F6** — Windows pipe trio (one coherent fix: unique name + fatal
   classification + handle close). Multi-instance correctness + leak + hot-spin.
3. **F3 + F4** — pid-reuse kill safety (image re-verify + exact pid match).
4. **F7** — bound runtime regex compile (reuse import.rs constants).
5. MEDIUM test-determinism cluster (M1/M2/M3/M8) — convert sleeps/ambient-env to
   poll-until / injected env / virtual time.
6. LOW nits opportunistically.
