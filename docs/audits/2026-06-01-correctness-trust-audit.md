<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The Terminal Commander Authors -->

# Correctness & Trust Audit (follow-up) — 2026-06-01

Second behavioral audit of the whole workspace, building on
`2026-05-29-correctness-trust-audit.md` (whose deferred items D1-D9 are
cross-referenced here). Same goal axes: **correctness**, **agent trust** (an
LLM keeps routing through TC instead of falling back to raw shell), and **token
economy**.

Method: a SymForge-driven fan-out of seven parallel read-only domain auditors
(mcp / probes / core+sifters / store / supervisor / daemon / js), each reading
the real code via the index (not assumptions), followed by one adversarial
**refuter per finding** that re-read the cited source and tried to break the
claim. 33 agents, 26 raw findings -> **23 confirmed, 3 refuted**. Every fix
below was then implemented and verified per crate (`cargo fmt --check`,
`cargo clippy -p <crate> --all-targets`, `cargo test -p <crate>`).

Verification caveat: this pass was executed on **Windows**, so `#[cfg(windows)]`
code was compiled and tested natively while `#[cfg(unix)]` blocks were preserved
verbatim and rely on CI-linux to compile. Each affected fix notes this.

---

## Part 0 — Pre-existing regression caught at baseline

**`cargo fmt --check` was failing** before any new work: the 2026-05-29 F2 fix
left `crates/store/src/registry.rs::search_rules` indented one level too deep
(`registry.rs:283,311`). It compiled (braces balanced) and clippy was clean, so
the prior pass — which checked clippy, not fmt — missed it; CI's fmt gate would
have rejected the branch. Fixed by `cargo fmt`. Baseline `cargo build
--workspace --all-targets` was otherwise green.

---

## Part 1 — Fixed in this pass (each verified: fmt + clippy clean + tests pass)

| # | Sev | Axis | Area | Fix | Regression test | Def |
|---|-----|------|------|-----|-----------------|-----|
| H4 | High | token | `store/registry.rs` `search_rules` | `rule_search` kept an FTS row per rule version and never deleted superseded ones, so a rule edited N times returned N identical hits that crowded distinct rules out of the `LIMIT`/`rank` budget. The query now joins `rules.latest_version` so search returns exactly one current hit per rule (no schema migration required). | `registry_search_returns_only_latest_version_no_duplicate_hits` | D1 |
| M4 | Med | correct | `store/registry.rs` `create_rule_version` | The manual FTS insert supplied no rowid, so FTS5 auto-assigned its own sequence; the `rv.rowid = rs.rowid` join in `search_rules` was correct only by coincidental 1:1 insert order, and any future delete/VACUUM would map a hit onto the wrong rule. The insert now supplies `rowid = tx.last_insert_rowid()` (captured before the `rule_tags` inserts advance it), making the external-content join correct by construction. | (covered by the H4 test + existing registry tests) | D1 |
| H8 | High | robust | `daemon/store_actor.rs` `actor_loop`/`execute` | The single store-writer thread had no `catch_unwind`; one panic in any `EventStore` op killed the thread and turned every later `StoreClient::call` into a closed-channel error — silently disabling ALL persistence + audit for the daemon's life. `execute` now runs inside `guard_panic` (`catch_unwind(AssertUnwindSafe(..))`), converting a panic into a typed `EventStoreError::Unavailable` (logged) so the loop survives. | `panicking_op_is_isolated_as_unavailable_not_thread_death` | D7 |
| H7 | High | trust | `daemon/ipc/handlers/common.rs` `map_store_error` (+ store error taxonomy) | Every `EventStoreError::InvalidPayload` mapped to `IpcErrorCode::RuleInvalid`, so an internal fault (dead/panicked store actor) told the agent "your rule is invalid" — driving it to edit a valid rule forever instead of learning the store is down. Added `EventStoreError::Unavailable` for non-caller-fixable backend faults; routed the actor-dead / dropped-channel / unexpected-reply / spawn-fail / op-panic sites to it; `map_store_error` maps `Unavailable -> Internal`. Genuine validation still maps `InvalidPayload -> RuleInvalid`. | updated `call_after_shutdown_errors_cleanly`; the 3 `RuleInvalid` IPC tests stay green | D7 |
| H5 | High | trust | `supervisor/replace.rs` `cmdline_is_our_daemon` / `pid_belongs_to_daemon` / `find_daemon_pid_os` | The kill-time identity gate was a bare substring test: a seeded session's dir (`<base>/agent-1`) contains the default session's dir (`<base>`), so the gate confirmed one session's daemon as another's and could force-kill a different live session. New whole-argument matcher (`contains_path_arg`) requires a token boundary on both sides — a path separator is explicitly NOT a boundary — so a prefix never matches. Windows blocks were refactored to fetch `CommandLine` and post-filter in Rust (path no longer enters the PowerShell query). | `cmdline_match_rejects_path_prefix_of_another_session`, `cmdline_match_handles_apostrophe_and_equals_forms` | new |
| M5 | Med | trust | `supervisor/replace.rs` (Windows) | The PowerShell `-like '*{path}*'` interpolation escaped only `[`/`]`, so a `'` in the state-dir path (e.g. user `O'Brien`) closed the single-quoted literal and the identity/find query silently returned nothing (a real stale daemon was never replaced). Subsumed by the H5 Windows refactor: the path is no longer interpolated into PowerShell at all, so an apostrophe can no longer break it. | (covered by `cmdline_match_handles_apostrophe_and_equals_forms`) | D4 |
| H3 | High | correct | `sifters/noise.rs` `Dedupe::apply_at` | A `DedupeEntry` persists across `apply_at` batches but stored `representative_index`, an index into the batch's `emit` vec. On a cross-batch repeat, `emit.get_mut(stale_index)` wrote one event's aggregate count/timestamps onto an UNRELATED draft at the same index in the new batch, which was then emitted corrupted. Replaced the persisted field with a per-batch `key -> emit-index` map, so the in-`emit` collapse patch applies only to same-batch representatives; cross-batch recurrence flows solely through the `representative_seq` bucket patch. | `cross_batch_repeat_does_not_corrupt_an_unrelated_draft` | new |
| L1 | Low | trust | `mcp/tools.rs` `unexpected_variant` | On a daemon request/response variant mismatch, 30 handlers built their error via `format!("...{resp:?}")`, interpolating the full `IpcResponse` Debug — leaking internal variant layout and any payload it carried (captured stream text, file-window bytes, rule definitions) into the client-facing error as unbounded tokens. Now returns a bounded, payload-free message (mirrors the `system_discover` precedent). | (compile + clippy; behavior is a fixed string) | new |

---

## Part 2 — Confirmed but deferred (each needs a bigger/standalone change; specs are
## verifier-blessed and ready to execute)

### H1 — mcp daemon availability is sampled once and never refreshed (HIGH, trust, new)
`crates/mcp/src/daemon_client.rs` + `tools.rs`. `EnsureDaemonStatus` is captured
once at MCP startup; if the daemon was slow to bind (a transient
`StartupTimeout`, 10s) every daemon-backed tool returns `daemon_unavailable` for
the **whole process life**, even after the socket goes live — the agent cannot
restart the MCP process, so it permanently falls back to raw shell.
**Fix (spec):** make `ensure_daemon_available` self-healing — when the cached
status is `Unavailable`, attempt a lightweight liveness re-probe (a `Health`
IPC with a short timeout); on success, clear the flag through the existing
`Arc<Mutex<EnsureDaemonStatus>>` (add a setter) and proceed. Prefer
clear-on-observed-success over a blind time-window clear; serialize the re-probe
so concurrent handlers don't each fire one.
**Why deferred:** the re-probe is async, so `ensure_daemon_available` must become
`async` and ~31 `self.ensure_daemon_available()?` call sites become
`.await?` — a mechanical but broad refactor of the trust surface that warrants
its own focused, well-tested change rather than a session-tail edit. The
existing `daemon_unavailable_envelope` test still passes under the fix (a down
daemon's re-probe fails and still yields the envelope).

### H6 — supervisor: no cross-process single-flight around ensure/replace (HIGH, trust, D2)
`crates/supervisor/src/ensure.rs` + `daemon/ipc/server.rs`. `ensure_daemon` does
probe-miss -> spawn with no inter-process lock; two adapters cold-starting in the
same window each spawn a daemon, and the daemon's bind unconditionally
`remove_file`s + rebinds the socket, orphaning the first daemon (which keeps the
DB open) while clients reach only the second.
**Fix (spec):** a non-blocking OS advisory lock on
`<state_dir>/terminal-commanderd.lock` around the probe->spawn (and
probe->kill->spawn in `replace_if_stale`) critical section; on contention,
re-probe and treat a freshly-bound endpoint as `AlreadyRunning`. Belt-and-
suspenders: the daemon itself takes the same lock (or checks `read_pidfile` for a
live, endpoint-matching daemon) before the unconditional `remove_file`+`bind`.
**Why deferred:** architectural; needs a multi-process test harness.

### M1 — PTY secret prompt counted twice (MED, trust, D9)
`crates/probes/src/pty.rs`. The peek-detection path and `process_line` both
increment `secret_prompts_total`/`prompts_total` for one prompt (peek + later
completion). **Fix:** gate the `process_line` secret bump on "not already counted
via peek for the current generation"; keep counting non-secret prompts there.

### M2 — PTY CR-collapse wipes a newline-less secret prompt before detection (MED, security, D9)
`crates/probes/src/pty.rs`. `AnsiNormalizer` clears the line buffer on any `\r`;
a secret prompt ending in `\r` (no `\n`) in one read chunk leaves `peek_pending()`
empty, so `secret_prompt_active` is never set and an LLM `write_stdin` in the same
window could be written into the prompt. **Fix:** classify the pre-CR line content
for prompt detection before `self.line.clear()` (capture it into a
`last_overwritten` field; only ever SET the flag, never clear it). Cross-platform-
and timing-sensitive; warrants its own change with a targeted test.

### M3 — progress pre-filter drops frames before the sifter, masking matching rules (MED, correct, D9)
`crates/probes/src/noise_pipeline.rs` `process_frame:135-138`. A frame whose whole
text is `%`/`n-over-m`/spinner/blank is dropped BEFORE `runtime.evaluate`, so a
rule (even High/Critical) keyed to such a line never fires. **Fix:** evaluate
first; suppress as progress only when the sifter produced zero drafts. Note: the
existing `progress_line_skips_evaluate` test asserts the OLD behavior and must be
updated (intended contract change). Pure logic, fully testable — a good next pick.

### M6 — system_discover.methods hand-list has drifted from the dispatcher (MED, trust, D7)
`crates/daemon/src/ipc/server.rs` `handle_system_discover`. The advertised method
list omits `registry_import_pack` and `shutdown`, both live dispatch arms, so an
agent that builds its tool surface from `system_discover` never offers them.
**Fix:** add the two names + a parity test asserting every dispatch-arm name
appears in `methods` (ideally derive the list from a shared
`fn method_name(&IpcRequest) -> &'static str`). Trivial; pure logic.

### M7 — registry_import_pack activate=true has no rollback on mid-loop failure (MED, trust, D7)
`crates/daemon/src/ipc/handlers/registry.rs` `handle_registry_import_pack`. Imports
+ promotes all pack rules, then activates them in a loop with `?`; a mid-loop
failure leaves rules 0..N-1 active + persisted and the pack imported, but the
caller gets only an error. **Fix:** collect per-rule outcomes and return a partial-
success response (`activated[]` + a new additive `failed[]`) instead of `?`-bailing.
**Why deferred:** changes the `RegistryImportPackResponse` wire shape (additive, but
touches the protocol + any TS/JSON clients) — a deliberate API change.

### M8 — Windows named-pipe shutdown does not drain in-flight connection tasks (MED, robust, new)
`crates/daemon/src/ipc/pipe_server.rs`. Unlike the Unix server (JoinSet + bounded
`drain_connections`), Windows connection handlers are fully detached;
`PipeServerHandle::shutdown` joins only the accept loop, so a request between
read and store-call can still be running when `shutdown_store` joins the actor —
returning a bogus failure for an accepted request during shutdown. **Fix:** mirror
the Unix path — track handlers in a `tokio::task::JoinSet` drained (bounded) before
`shutdown_store`.

### M9 — WSLENV credential-forwarding defense missing from 5 of 7 wsl.exe spawn sites (MED, security, new)
`packages/terminal-commander/lib/`: `bootstrap/ensure_wsl_runtime.js`,
`bootstrap/ensure_daemon_autostart.js`, `cli/setup_cursor_wsl.js`,
`cli/doctor_daemon.js`. The `ensureSessionInWslEnv` defense that `restart.js` +
`wsl/spawn.js` apply is missing here, so these spawns forward an operator's ambient
`WSLENV` (and any var it names, e.g. `WSL_SUDO_CREDENTIAL`) into a TC-controlled
`bash -lc` / `npm install` inside WSL. **Fix:** wrap each env with
`ensureSessionInWslEnv(buildFilteredEnv(env))` (already exported) + a regression
test per path. Mechanical; JS (no Rust compile) — a good next pick.

### M10 — wsl detect/doctor probes spawn with no env block at all (MED, security, new)
`packages/terminal-commander/lib/wsl/detect.js` `defaultExec`,
`wsl/doctor.js` `defaultProbeExec`. These `spawn(wsl.exe, ...)` with no `env`, so
the child inherits the FULL ambient `process.env` (every secret-shaped var, plus
`WSLENV`). **Fix:** set `env: ensureSessionInWslEnv(buildFilteredEnv(process.env))`.

> **Cycle gotcha (applies to M9 + M10).** `ensureSessionInWslEnv` currently lives
> in `wsl/spawn.js`, but `spawn.js` itself `require`s `./detect.js` and
> `./doctor.js` — so having detect.js/doctor.js `require("./spawn.js")` back would
> form a circular dependency (one module sees a partial `{}` export at load and
> the helper is `undefined` at call time). The clean fix is to **relocate
> `ensureSessionInWslEnv` to the leaf module `wsl/filtered_env.js`** (where
> `buildFilteredEnv` already lives; the function is pure — it only manipulates an
> env object), and re-export it from `spawn.js` for back-compat (`restart.js`
> imports it from there). Then every site — the 4 in M9 and the 2 here — imports
> it from `filtered_env.js` with no cycle. The bootstrap M9 sites alone do not
> cycle (spawn.js does not require them), so they could import from `spawn.js`
> directly, but routing all six through `filtered_env.js` keeps it uniform.

### L2 — file probe mislabels a non-zero shrink as a rotation (LOW, trust, D9)
`crates/probes/src/file.rs:207-216`. Truncation is recorded only when post-shrink
size is exactly 0; a truncate-then-write that leaves a small non-zero size counts
as a rotation. Reset is still correct (`pos=0`); only the metric misleads. **Fix
(interim, safe):** count any in-place shrink (`size < prev`) as a truncation.
(Folds naturally into H2 below if file identity is added.)

### H2 — file rotation with same-or-larger size is never detected (HIGH, correct, D9)
`crates/probes/src/file.rs` `run`. Rotation is inferred only from `size < prev`;
a logrotate `create` / atomic-rename replacement whose new file already reached or
passed the old `pos` is not detected — the probe seeks into the middle of the NEW
file, drops its leading bytes, and emits a corrupt mid-line fragment with a wrong
offset. **Fix:** track file identity — `(dev, ino)` on Unix
(`std::os::unix::fs::MetadataExt`), file-index + volume-serial on Windows
(`std::os::windows::fs::MetadataExt`) — captured from the open handle; reset
`pos=0` on identity-change OR `size < prev`.
**Why deferred:** cross-platform identity APIs (one half uncompilable/untestable on
the Windows dev box this pass ran on) — warrants a change validated on both OSes.

### L3 — cross-batch dedupe undercount when the representative has no seq (LOW, trust, new)
`crates/sifters/src/noise.rs` `Dedupe::apply_at`. When a representative's bucket
append returned `None` (sink failure), `representative_seq` stays `None`; a later
within-window repeat increments the private count but produces no patch and no
emit, so the bucket row undercounts the recurrence. **Fix:** when a cross-batch
repeat finds `representative_seq == None`, re-emit a refreshed representative draft
(so the seq gets registered) instead of dropping it. (Adjacent to H3, but a
distinct behavior change with its own test.)

### L4 — events_fts is maintained on every append but never searched (LOW, token, new)
`crates/store/migrations/V0001`. `events_fts` + its three triggers add an FTS
insert per append and an FTS delete per eviction, but no code path ever issues an
`events_fts MATCH`. **Decision required:** either wire an events full-text search
tool (guarded by the existing `fts5_quote_terms` sanitizer) or drop `events_fts`
+ all three triggers in a forward migration. Carrying dormant write-amplification
on the hot path is a token-economy cost; pick intentionally.

### L5 — sessions liveness is not identity-gated (LOW, trust, D3)
`crates/supervisor/src/sessions.rs` `entry_for`. `alive = pid_alive(rec.pid)` with
no identity check, so a recycled PID leaves a dead session's stale pidfile
uncleaned (`reap_one` only cleans on `!alive`). **Fix:** `alive = pid_alive(pid)
&& pid_belongs_to_daemon(pid, state_dir)` (now prefix-safe after H5). **Why
deferred:** breaks `enumerate_finds_default_and_seeded_...` (it uses the test
runner's PID as a stand-in for a live daemon); a clean fix wants an injectable
identity check for testability, and adds a per-entry subprocess spawn.

---

## Part 3 — Refuted by the adversarial pass (recorded so they are not re-investigated)

- **Dedupe GC clock-base mix (D9):** REFUTED. Every production `EventDraft` is
  stamped `OffsetDateTime::now_utc()` at construction (`sifters/lib.rs::build_draft`,
  `SourceFrame::new`), on the SAME clock as the GC cutoff. No frame/draft ever
  carries a historical timestamp (no `with_timestamp`, no log-time parsing), so the
  premature-eviction scenario cannot arise. Latent only if a future replay feature
  attaches historical timestamps.
- **Detached command lifecycle waiters lose `command_exited` across restart (D7):**
  REFUTED. The headline symptom is impossible: bucket events are in-memory only
  (`BucketManager` is an `RwLock<HashMap>`; `StoreOp` has no bucket variant; buckets
  are never persisted or rehydrated), so nothing "shows Running after restart." The
  only real, much narrower loss is a best-effort `command_exit` AUDIT row in a precise
  exit-at-shutdown race — a low audit-completeness item, not a trust/high defect.
- **TC_SESSION not forwarded by autostart WSL spawns:** REFUTED. By design and
  documented (`autostart.js:170-177`): the autostart daemon serves the legacy DEFAULT
  endpoint as a pre-warm; per-session daemons are spawned separately by the bridge,
  which DOES forward `TC_SESSION`. The install/start bash never reads `TC_SESSION`.

---

## Verification
- Per-crate `cargo fmt --check` (0 diffs), `cargo clippy -p <crate> --all-targets`
  (0 warnings), and `cargo test -p <crate>` green for every crate touched: store,
  daemon (`terminal-commanderd`), supervisor, mcp, sifters.
- Integrated `cargo fmt --check` / `cargo clippy --workspace --all-targets` /
  `cargo test --workspace` — see `outputs/`.
- Executed on Windows: `#[cfg(unix)]` blocks in `supervisor/replace.rs` were
  preserved verbatim (only comments + the shared cross-platform matcher changed)
  and rely on CI-linux to compile.
