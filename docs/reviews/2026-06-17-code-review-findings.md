# Terminal Commander — Full Code Review Findings

Date: 2026-06-17  
Scope: omni program (`c3adafd..HEAD`, 14 commits) + surrounding seams  
Method: source-grounded review (P0→P2 hotspots, six invariants, cross-cuts)  
Baseline: 49 live MCP tools, 25 rule packs, 45/49 contract fixtures, 943 tests passed / 1 skipped (per `TESTING.md`; not re-run in this review)

This document is the **action artifact** for a follow-up agent: findings are ordered by severity, each with evidence and a minimal fix seam. Known disclosed gaps (O-07, O-09, partial cwd, etc.) are verified/bounded here—not duplicated as fresh discoveries.

---

## Part B — Summary verdict

### Top 5 to fix before merge/release

1. **F-001** — Windows ConPTY writer-thread secret denial surfaces as `Internal` IPC error (not `SecretInputDenied`); metrics undercount.
2. **F-002** — `max_sessions` cap has a TOCTOU race; concurrent `shell_session_start` can exceed the configured cap.
3. **F-003** — Caller-supplied session `env` is persisted to SQLite workspace snapshots without secret redaction.
4. **F-010** — ConPTY live child-output + secret-gate path remains **test-unverified** on real Windows (O-07); treat as release gate for Windows PTY/session surfaces.
5. **F-006** — Four subscription tools lack contract fixtures; drift risk on the 49-tool contract surface.

### Per-invariant assessment (section 3 rubric)

| Invariant | Verdict | Supporting findings |
|-----------|---------|---------------------|
| I Two-process boundary (MCP no command spawn) | **UPHELD** | `crates/mcp/src` grep guard + tools forward IPC only; daemon bootstrap via `supervisor::ensure_daemon` in `main.rs` is intentional (spawns `terminal-commanderd`, not user commands). |
| II Policy-before-spawn, default-deny, caps | **UPHELD** | `PolicyEngine::evaluate` deny-first tower; `SessionStart`/`CommandShellStart` gates before profile arms; `full_access` does not bypass `evaluate`. Residual: argv[0]-only `COMMANDS_DENY` for shell lines (documented). |
| III Combed, bounded output | **AT RISK** | F-001 wrong error on Windows secret deny may cause agent retry storms (bounded but misleading). Compact mode by design drops ids (F-011). |
| IV Local-only privilege boundary | **UPHELD** | `target_router.rs` dials local socket paths only; no TCP bind in MCP path. |
| V Audit + redaction | **AT RISK** | argv/shell_line redaction is strong; session env snapshots bypass redaction (F-003). |
| VI No-mock + honest degradation | **UPHELD** | Job receipt fallback, degraded `run_and_watch`, suggest never activates; receipt write failure is log-and-drop (honest). |
| VII Suggest never auto-activates | **UPHELD** | `handle_registry_suggest_from_samples` has no `state` handle; returns drafts only. |

### Test-trust assessment

| Area | Trust |
|------|-------|
| Unix PTY secret gate (`pty_core` + `process_line`) | **Strong** — platform-neutral unit tests + unix live M1/M2/grace tests. |
| Windows ConPTY (`runtime_win`) | **Weak** — structurally mirrors unix; live e2e gated `TC_CONPTY_E2E=1`, blocked on dev host (O-07). Writer-thread denial path has **no** test. |
| Shell sessions | **Moderate** — `parse_cd_target` unit tests; live e2e on unix; cap race untested. |
| Job receipts / restart status | **Moderate** — store round-trips; live restart e2e exists; collision/receipt ordering not stress-tested. |
| Policy engine | **Strong** — extensive `policy.rs` unit tests for caps/profiles/session gate. |
| Remote federation | **Moderate** — simulated second local socket (O-09); not real SSH. |
| Rule packs / ReDoS | **Moderate** — CI budget in `TESTING.md` sec 7; manual spot-check of new packs shows anchored patterns (no catastrophic `.*` seen in docker pack). |
| IPC malformed input | **Strong** — frame size limits + decode errors return `IpcError`, no panic path found in production handlers. |

### Highest-leverage refactor (non-blocking)

Unify Windows `write_stdin` error mapping: treat writer-thread secret denial identically to `WriteStdinError::SecretInputActive` (F-001 fix pattern could extend to a shared helper used by both PTY backends).

### Could not verify (not "clean")

- Live ConPTY byte pipeline + secret detection through a real Windows child (O-07).
- macOS notify/smoke paths (O-08).
- Real SSH `-L` tunnel remote ops (O-09/O-10).
- Provider live smokes (O-14).
- Full `cargo nextest run --workspace` on this host during review (read-only code review).

---

## Part A — Findings

### F-001

```
ID:           F-001
Title:        Windows ConPTY writer-thread secret deny returns Io error → Internal IPC, not SecretInputDenied
Severity:     HIGH
Confidence:   HIGH
Category:     security | error-handling | contract
Location:     crates/probes/src/pty.rs:1639-1647, 1435-1436; crates/daemon/src/pty_command.rs:599; crates/daemon/src/ipc/handlers/pty.rs:174-177
What:         On Windows, `write_stdin` checks the secret gate on the caller thread, then queues bytes to the writer thread. The writer re-checks the gate and denies with `std::io::ErrorKind::PermissionDenied` sent through the reply channel. `write_stdin` maps that via `result?` to `WriteStdinError::Io`, not `SecretInputActive`. The daemon maps `Io` to `PtyRuntimeError::Io`, and the IPC handler maps that to `IpcErrorCode::Internal` with message `pty_command_write_stdin: ...`.
Why it matters: Bytes are not written (good), but agents receive a generic Internal error instead of the typed `SecretInputDenied` contract. Audit rows show `deny` only for `SecretInputActive`; writer-path denials skip `stdin_writes_denied_secret` increment and may omit the secret audit shape. Violates invariant III honesty and breaks client recovery logic that keys off `SecretInputDenied`.
Evidence:     Writer denial: `reply.send(Err(std::io::Error::new(PermissionDenied, ...)))`. Caller: `let written = result?` → `WriteStdinError::Io`. Handler: `Err(other) => Internal`.
Repro / trigger: Windows host; start PTY job; trigger secret prompt; call `pty_command_write_stdin` in the window where caller-thread gate is false but reader thread has published secret before writer runs.
Suggested fix: In `runtime_win::write_stdin`, map writer `PermissionDenied` (or a dedicated reply enum) to `WriteStdinError::SecretInputActive` and increment `stdin_writes_denied_secret`. Alternatively, writer thread sends a typed denial enum instead of `io::Error`. Add Windows unit/integration test for writer-thread denial path.
```

### F-002

```
ID:           F-002
Title:        shell_session_start max_sessions cap is not atomic (TOCTOU race)
Severity:     MEDIUM
Confidence:   HIGH
Category:     concurrency | correctness
Location:     crates/daemon/src/shell_session.rs:217-252
What:         `start` calls `reap_terminal()`, reads `live_count()` under a read lock, compares to `max_sessions`, then spawns the PTY (slow) and only afterwards inserts into `sessions` under write lock. Two concurrent `start` calls can both observe `live < max` and both spawn, temporarily exceeding the cap.
Why it matters: Constitution II promises bounded, explanatory refusal at the cap. A burst of parallel session starts can overshoot `max_sessions`, increasing PTY/shell resource use beyond operator intent.
Evidence:     No lock held between `live_count()` at line 218 and `self.sessions.write().insert` at line 259; `pty.start_session` at line 252 is outside the session map lock.
Repro / trigger: Two harness threads call `shell_session_start` when `live_count == max_sessions - 1`.
Suggested fix: Reserve a slot under write lock (e.g. increment pending counter or insert placeholder) before spawn, or serialize starts through a mutex; release slot on spawn failure. Add concurrent start test.
```

### F-003

```
ID:           F-003
Title:        Session env overlay persisted to workspace snapshots without redaction
Severity:     MEDIUM
Confidence:   HIGH
Category:     security
Location:     crates/daemon/src/shell_session.rs:103-106, 255-256; crates/store/src/workspace.rs:31-33, 70-79
What:         `env_snapshot` stores caller-supplied `(key, value)` pairs bounded by `MAX_SESSION_ENV_ITEMS` but not redacted. `workspace_snapshot_create` persists this JSON to SQLite. Comment in `workspace.rs` claims "no unredacted host secrets" because daemon bounds before persistence—but caller/agent can supply `API_KEY=...` in session start env.
Why it matters: Invariant V (audit/redaction) and snapshot durability can leak secrets-shaped values to disk and later `status`/`workspace_snapshot_apply` responses. Unlike argv redaction, env overlay has no `redact_argv` pass.
Evidence:     `env_snapshot: Vec<(String, String)>` from `req.env`; `record_workspace_snapshot` stores `env_json` verbatim.
Repro / trigger: `shell_session_start` with `env: [{key: "TOKEN", value: "secret"}]`; create workspace snapshot; read store row or `shell_session_status`.
Suggested fix: Apply same redaction heuristics as argv (or deny well-known secret keys); or strip env from persisted snapshots and only store keys; document that env overlay is operator-trusted. Add test that `TOKEN=` values are redacted in snapshot rows.
```

### F-004

```
ID:           F-004
Title:        Persistent shell sessions are unix-only; Windows agents have no session lane
Severity:     MEDIUM
Confidence:   HIGH
Category:     contract | docs-vs-code
Location:     crates/daemon/src/shell_session.rs:44-45 (#![cfg(unix)]); ipc handlers gated unix
What:         Entire `ShellSessionRuntime` is `#[cfg(unix)]`. Windows builds omit session IPC handlers. Omni matrix may still advertise session tools on Windows via MCP catalogue while daemon cannot serve them.
Why it matters: Agents on Windows get unavailable/deny paths for session tools; platform parity gap for omni program P1 slice.
Evidence:     Module header: "Unix-only: the whole module is `#[cfg(unix)]`".
Repro / trigger: Windows daemon + `shell_session_start` IPC.
Suggested fix: Confirm `system_discover.omni_status` marks sessions unavailable on Windows with explicit reason; or document in TOOL_CONTROL_SURFACE. Long-term: ConPTY session lane (out of scope unless spec'd).
```

### F-005

```
ID:           F-005
Title:        Windows ConPTY cancel ignores grace ladder (forced kill only)
Severity:     LOW
Confidence:   HIGH
Category:     correctness | docs-vs-code
Location:     crates/probes/src/pty.rs:1445-1452, 1453-1466
What:         `runtime_win::cancel` documents that `config.grace` is not honored; kill + drop master only. Unix PTY uses `terminate_process_tree_graceful` SIGTERM→wait→SIGKILL.
Why it matters: Documented asymmetry; cooperative shutdown on Windows PTY/session stop is not attempted. Not a silent lie if docs match—operators may expect grace on all platforms.
Evidence:     Comment block at 1445-1452; `cancel` calls `killer.kill()` directly.
Repro / trigger: Windows PTY child handling SIGTERM; observe immediate kill behavior.
Suggested fix: Document in SHELL_SESSION.md / PLATFORM notes; optional future graceful ConPTY terminate if API exists. No code change required if docs are explicit everywhere.
```

### F-006

```
ID:           F-006
Title:        Four live subscription tools lack per-tool contract fixtures (45/49)
Severity:     MEDIUM
Confidence:   HIGH
Category:     contract | tests
Location:     tests/fixtures/contracts/mcp-tool-fixture-map.v1.json:283-300, 320-323
What:         `subscription_open`, `subscription_pull`, `subscription_list`, `subscription_close` are `missing_fixture`. Catalogue and router tests enforce 49 tools but contract fixture set is incomplete.
Why it matters: MCP contract drift for subscription surface; external clients relying on fixtures miss shape/defaults for 4 tools.
Evidence:     `counts.missing_fixture: 4` in fixture map.
Repro / trigger: Run fixture catalogue contract; compare to tool list.
Suggested fix: Add four `mcp-tools/*.v1.json` fixtures; update fixture map counts. Follow `docs/contracts/README.md` convention.
```

### F-007

```
ID:           F-007
Title:        status.cwd is best-effort parse only; documented but easy to misuse
Severity:     LOW
Confidence:   HIGH
Category:     correctness | docs-vs-code
Location:     crates/daemon/src/shell_session.rs:547-565, 368-370; docs/runtime/SHELL_SESSION.md
What:         `parse_cd_target` accepts only plain `cd <single-token>`; rejects compounds, `cd -`, `$VAR`, quoted paths with spaces. `status.cwd` updates only on matching exec lines.
Why it matters: Not a bug if labeled advisory; becomes correctness issue if future policy/containment reads `status.cwd` instead of `pwd` signal.
Evidence:     Tests explicitly reject `cd /tmp && ls`, `cd '/tmp/a b'`.
Repro / trigger: `shell_session_exec` `cd $HOME`; status.cwd stale.
Suggested fix: Ensure API/docs/schema label `cwd` as approximate; optional `confidence: partial` field in status response. No code change unless policy starts using cwd.
```

### F-008

```
ID:           F-008
Title:        LimitReached error message misreports live session count
Severity:     LOW
Confidence:   HIGH
Category:     error-handling
Location:     crates/daemon/src/shell_session.rs:68-70, 219-220
What:         `LimitReached(usize)` error template uses `{0}` twice: "session limit reached: {0} live sessions (cap {0})". Only `max_sessions` is passed, not current live count—message reads as if current live equals cap always.
Why it matters: Operators debugging cap refusals get misleading telemetry.
Evidence:     `return Err(SessionError::LimitReached(self.max_sessions))`.
Suggested fix: Pass `(live, max)` or format with explicit "cap {max} reached (live {live})".
```

### F-009

```
ID:           F-009
Title:        CRLF `\r` deferred across PTY read boundaries still uses overwrite semantics
Severity:     LOW
Confidence:   HIGH
Category:     correctness
Location:     crates/probes/src/pty.rs:55-61, 104-107; AnsiNormalizer pending_cr
What:         Documented: `\r` at end of one read with `\n` in the next read is still treated as overwrite (M2 secret test depends on this). Can drop/merge legitimate interactive lines in rare split case.
Why it matters: Combed signal may miss or distort line; session priming retains `stty -onlcr` as belt-and-suspenders.
Evidence:     Module comment lines 55-60.
Repro / trigger: Split `pwd\r` and `\n` across two PTY reads without `-onlcr`.
Suggested fix: Accept as bounded known gap OR extend normalizer cross-feed CRLF join; add session e2e for split-read path if priming is removed.
```

### F-010

```
ID:           F-010
Title:        ConPTY live e2e unverified — highest-risk path lacks host proof
Severity:     MEDIUM (process/release gate)
Confidence:   HIGH
Category:     tests
Location:     crates/probes/src/pty.rs:1727-1730; BACKLOG O-07; scripts gated TC_CONPTY_E2E
What:         Windows ConPTY tests compile only on Windows and require `TC_CONPTY_E2E=1`. Disclosed blocked on dev host (DLL init). Reader/writer/waiter thread orchestration + secret gate on live child untested.
Why it matters: P0 omni slice (US3a) ships production Windows PTY path without live verification; F-001-class bugs may hide here.
Evidence:     O-07 disclosure; `conpty_e2e_tests` module comment.
Repro / trigger: Run on Windows desktop CI with TC_CONPTY_E2E=1.
Suggested fix: Unblock O-07 on CI; add e2e for writer-thread secret denial after F-001 fix. Block Windows PTY release until green.
```

### F-011

```
ID:           F-011
Title:        compact:true drops event_id and rule metadata (by design)
Severity:     NIT
Confidence:   HIGH
Category:     contract
Location:     crates/mcp/src/tools.rs:2741-2753, 2795-2803
What:         `project_signal_compact` keeps only summary, stream, seq, severity. Agents using compact cannot call `event_context` without re-fetching full events.
Why it matters: Documented presentation-only; store untouched. Risk if agents use compact for automated actions without `bucket_events_since`.
Evidence:     Comment "PRESENTATION ONLY" at 2745-2746.
Suggested fix: Doc-only: OMNI_PLAYBOOK note that compact requires bucket re-fetch for ids. Optional: include `event_id` in compact.
```

### F-012

```
ID:           F-012
Title:        target_id wired on command lane only — remote agents can silently hit local daemon on other tools
Severity:     LOW
Confidence:   HIGH
Category:     contract | security
Location:     crates/mcp/src/tools.rs (CommandStart/Status/Stop/run_and_watch params); P5 docs
What:         `target_id` exists on command start/status/stop/run_and_watch and target_* tools, not on shell_exec, pty_*, file_*, registry_* (except probe/list). Agent passing target_id on unsupported tools either fails serde or ignores field.
Why it matters: Remote federation UX trap; not a bypass of allow_remote (local path lacks remote gate) but operator may think all tools route.
Evidence:     `McpCommandStartParams.target_id` vs tools without field in schema.
Suggested fix: Document command-path-only in TOOL_CONTROL_SURFACE; or add ignored-field warning in system_discover; long-term uniform optional target_id on all daemon-backed tools.
```

### F-013

```
ID:           F-013
Title:        Universal extractors merge only when zero scoped+inline rules (intentional conservative gap)
Severity:     LOW
Confidence:   HIGH
Category:     correctness | docs-vs-code
Location:     crates/daemon/src/command.rs:827-835
What:         When `universal_extractors` is true but an unrelated active pack applies, universals are NOT merged. Baseline LOW signals absent unless pack matches.
Why it matters: Disclosed in tasks.md; operators may expect universal baseline always-on when flag is true.
Evidence:     `if self.universal_extractors && merged_rules.is_empty()`.
Suggested fix: Doc clarification in OMNI_PLAYBOOK / config comment. Product change only if spec requires universals alongside packs.
```

### F-014

```
ID:           F-014
Title:        Job receipt uses INSERT OR REPLACE on job_id — safe given UUIDv7; stale receipt only after restart
Severity:     NIT
Confidence:   HIGH
Category:     correctness
Location:     crates/store/src/job_receipt.rs:88-101; crates/core/src/ids.rs:66-70; handlers/command.rs:173-246
What:         Post-restart `command_status` falls back to receipt on `UnknownJob`. JobId is UUIDv7 (collision negligible). Live jobs never read receipt while in memory map. Receipt write failure is log-and-drop (no false terminal in memory).
Why it matters: Confirms TC-B3 design holds; no false "exited" on live job found in code path review.
Evidence:     Handler only uses receipt on `UnknownJob`; `status()` reads live `jobs` first.
Suggested fix: None required; optional stress test for receipt ordering after `stop()` early-finalize path (command.rs:1098-1111).
```

---

## Action queue for implementer agent

Priority order:

| Priority | ID | Effort | Files (primary) |
|----------|-----|--------|-----------------|
| P0 | F-001 | S | `crates/probes/src/pty.rs` (`runtime_win`), tests on Windows or mock writer reply |
| P0 | F-010 | CI | Windows runner + `TC_CONPTY_E2E=1`, BACKLOG O-07 |
| P1 | F-002 | S | `crates/daemon/src/shell_session.rs`, concurrent test |
| P1 | F-003 | M | `shell_session.rs`, `workspace.rs`, redaction helper reuse from `command.rs` |
| P1 | F-006 | S | `tests/fixtures/contracts/mcp-tools/*.v1.json`, fixture map |
| P2 | F-004, F-008, F-012 | S | docs + `system_discover` matrix |
| P2 | F-005, F-009, F-011, F-013 | S | docs only unless product change requested |

### Verification commands (post-fix)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace -E 'not test(session_reap_token_shuts_down_the_daemon)'
# Windows host additionally:
# set TC_CONPTY_E2E=1 && cargo nextest run -p terminal_commander_probes -- conpty
bash scripts/smoke/verify-runtime-smoke.sh
```

### Hotspots reviewed — no additional HIGH found

- **Policy `evaluate` ordering**: Shell/session gates return before profile `Allow` arms; `repo_only` containment runs inside profile match; `read_only_observer` denies mutations. **OK.**
- **MCP no-spawn guard**: `scripts/linux-gate.sh` greps `crates/mcp/src`; production adapter forwards IPC. **OK** (daemon ensure is supervisor, not user command).
- **Redaction argv/shell_line**: `redact_shell_line` / `redact_argv_head` in `command.rs` — **OK** for command/shell audit subjects.
- **IPC panic on malformed input**: length cap + decode errors; no production `unwrap` on request path in handlers. **OK.**
- **Capture canonicalization (TC-E4)**: named captures win over reserved keys in `inject_full_match` — **OK.**
- **notify 9p backend**: component-aware prefix in `path_has_mount_prefix` — **OK.**
- **Suggest never activates**: handler has no store/registry mutation — **OK.**
- **P4 privileged**: plan-only in omni matrix; no privileged spawn code in crates — **OK.**

---

## Reviewer notes

- SymForge was not indexed for this repo during review; inspection used direct source reads and grep.
- Disclosed gaps O-07/O-08/O-09/O-10/O-14 treated as verification debt (F-010), not rediscovered.
- The prior prompt file `2026-06-17-external-codebase-review-prompt.md` is superseded by this findings artifact and should be removed from the tree.
