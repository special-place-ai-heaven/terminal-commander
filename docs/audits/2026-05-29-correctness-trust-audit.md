<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 The Terminal Commander Authors -->

# Correctness & Trust Audit — 2026-05-29

Behavioral audit of the whole workspace after the session-supervisor /
ipc-handler-split / store-actor / noise-pipeline merge (`d14a126..302a05b`).
Goal axes: **correctness**, **agent trust** (an LLM keeps routing through TC
instead of falling back to raw shell), and **token economy**.

Method: six parallel read-only domain auditors (core, daemon, probes,
sifters+store, supervisor, mcp+JS), every reported finding independently
re-verified against the source before any change. `cargo clippy --workspace
--all-targets` was clean before and after.

---

## Part 1 — Fixed in this pass (verified: clippy clean + new regression tests pass)

| # | Severity | Area | Fix | Regression test |
|---|----------|------|-----|-----------------|
| F1 | High (trust) | `crates/mcp/src/tools.rs` `into_mcp_error` | Caller-fixable `IpcErrorCode`s were mapped to JSON-RPC `internal_error` (-32603 = "server broken"). Now an **exhaustive** match maps all caller-fixable codes to `invalid_params` (-32602 = "fix your input"); only `Internal`/`PeerCredentialFailure`/`UnsupportedPlatform`/`ShuttingDown` stay internal. Adding a new code now forces a deliberate classification (no wildcard). | `caller_fixable_ipc_errors_map_to_invalid_params_server_faults_stay_internal` |
| F2 | High (trust) | `crates/store/src/registry.rs` `search_rules` | Agent-supplied text was bound straight into FTS5 `MATCH`; a `"`, `:`, `(`, or bare `AND` raised `SQLITE_ERROR` → `registry_search` looked broken. New `fts5_quote_terms` tokenizes + quotes each term (term-AND), so no input can raise a syntax error; empty query → no results. | `registry_search_handles_fts5_metacharacters_without_error`, `fts5_quote_terms_escapes_metacharacters` |
| F3 | High (correctness) | `crates/probes/src/file.rs` | File probe used `BufReader::lines()/next_line()`: aborted the whole probe on the first non-UTF-8 byte, mis-split/duplicated a still-growing final line, and drifted byte offsets by 1 per CRLF line (`pos += bytes + 1`). Rewritten with a raw-byte reader (`read_file_line`) that lossily decodes, advances `pos` by exact on-disk bytes incl. terminator, and holds back an unterminated tail in follow mode (emit only in ScanOnce). | `read_file_line_splits_lf_and_reports_raw_len`, `_crlf_offset_counts_cr_byte`, `_unterminated_tail_is_flagged`, `_non_utf8_does_not_abort` |
| F4 | Medium (correctness/trust) | `crates/core/src/context.rs` `RingInner::window` | Window accumulated linearly from `want_start`; a tight `max_bytes` could exhaust the budget before reaching the anchor, returning context **without the matched line** while `anchor_missing=false`. Now builds outward from the anchor (anchor always included, like `tail`), so `event_context` never omits its own event. | `context_byte_cap_still_includes_anchor` |
| F5 | Medium (security) | `crates/daemon/src/ipc/pipe_server.rs` `accept_loop` (Windows) | On SDDL build failure the named pipe fell back to the **default** DACL (broader than the intended LocalSystem+Admins+current-user), letting a lower-priv local process connect. Now **fails closed**: refuses to bind (matching the Unix peer-credential fail-closed philosophy and the `pipe_acl` test's intent). | covered by existing `pipe_acl` ACL assertion on happy path; failure path is fail-closed return |
| F6 | Medium (security) | `packages/terminal-commander/lib/cli/restart.js` | Windows `restart` dispatched `wsl.exe` with `buildFilteredEnv(env)` but skipped `ensureSessionInWslEnv`, so an ambient `WSLENV` (e.g. `WSL_SUDO_CREDENTIAL/u`) was forwarded into WSL — the exact leak the bridge spawn path hardens. Now wraps env in `ensureSessionInWslEnv(...)` (TC-only allowlist / drop). | `restart on Windows neutralizes ambient WSLENV` |
| F7 | Low (doc) | `crates/sifters/src/noise.rs` `dedupe_key` | Doc said key was `<rule_id>|<kind>|<captures>`; code emits `<rule_id>|<version>|<kind>|<captures>`. The `version` segment is load-bearing (a version bump splits dedupe buckets). Comment corrected with a "do not drop it" note. | n/a (doc) |

---

## Part 2 — Deferred (needs a migration, an architectural decision, or is lower-confidence/latent)

### D1 — Registry FTS is a hand-written external-content table (HIGH latent, needs migration)
`rule_search` is `content='rule_versions', content_rowid='rowid'` but
`create_rule_version` inserts **without** a rowid and **never deletes** a
superseded version's FTS row (`crates/store/src/registry.rs:202`, migration
`V0002__registry.sql:47`). Two consequences:
- **Stale rows:** a rule edited N times yields N search hits (one per version),
  crowding other rules under `LIMIT`/`rank`.
- **Latent wrong-rule join:** the `rv.rowid = rs.rowid` join is correct only while
  the two tables' rowid sequences stay in lockstep. They do today (1:1 inserts,
  no deletes), but `rules` has `ON DELETE CASCADE` to `rule_versions`; any future
  delete/VACUUM diverges the sequences and maps a match onto the wrong rule's
  definition (severity/status/template).

**Fix (needs new migration `V0005`):** manage `rule_search` via AFTER
INSERT/UPDATE/DELETE triggers on `rule_versions` writing `(new.rowid, …)` and
`'delete' old.rowid` — the exact pattern `events_fts` already uses
(`V0001:55-73`) — then `INSERT INTO rule_search(rule_search) VALUES('rebuild')`.
Not shipped here because it requires a schema migration + FTS rebuild on existing
DBs, which warrants its own reviewed change. Add a regression test inserting ≥2
rules × ≥2 versions and asserting search returns the correct latest definition.

### D2 — Supervisor: no cross-process single-flight around probe→kill→spawn (MEDIUM)
`replace_if_stale` and `ensure_daemon` have no inter-process lock
(`crates/supervisor/src/replace.rs:269`, `ensure.rs:128`). Concurrent triggers
(MCP adapter auto-check + `terminal-commander update` + npm postinstall — all
wired per the session-supervisor plan) can double-kill, or kill a just-spawned
replacement, or (Windows, where a second `CreateNamedPipe` instance succeeds)
run two daemons against one SQLite dir. Single-process gates are correct; the
cross-process case is unguarded.
**Fix:** a `state_dir/replace.lock` (`create_new` / `LockFileEx` / `flock`)
covering the whole probe→kill→clear→spawn window, ideally paired with a
daemon-side singleton instance lock at startup. Architectural — deferred for a
focused change with its own multi-process test.

### D3 — Supervisor: `sessions::enumerate` liveness is not identity-gated (LOW/MEDIUM)
`alive` is set from bare `pid_alive(rec.pid)` (`crates/supervisor/src/sessions.rs`)
with no `pid_belongs_to_daemon`/`proc_cmdline` check, so PID reuse (or a not-yet-
reaped zombie) makes `session list` report a dead session as alive. No wrong kill
(the reap force-path re-gates), but the displayed liveness and reap *selection*
can be wrong. **Fix:** make `entry_for` identity-aware using the crate's existing
`pid_belongs_to_daemon`/`proc_cmdline`. Low-risk; deferred only to keep this pass
verifiable.

### D4 — Supervisor: PowerShell `-like` interpolation of the state-dir path (LOW)
`pid_belongs_to_daemon` / `find_daemon_pid_os` (Windows) escape only `[`/`]` of
the state-dir before interpolating into a single-quoted PS string + `-like '*…*'`
(`crates/supervisor/src/replace.rs:174,230`). A `'` in the path truncates the
literal (identity check fails → a real daemon is never confirmed/replaced).
`*?` are NTFS-illegal so the over-match is theoretical; the `'` case is real.
**Fix:** double `'`→`''` (and escape `` ` ``), or filter by `ProcessId` and
compare `CommandLine` in Rust rather than in `-like`.

### D5 — Supervisor: daemon-side pidfile-write failure is swallowed (LOW/MEDIUM)
`write_pidfile` failure is `warn`-and-continue (`crates/daemon/src/runtime.rs`
~L369). A reachable daemon with no pidfile is then classified "pre-pidfile /
stale" and becomes a kill candidate. **Fix:** retry once / fail loud on the
daemon side; don't treat reachable-but-no-pidfile as automatically stale.

### D6 — Supervisor: spawned daemon child dropped without reaping (LOW, unix)
`ensure_daemon` drops the `Child` without `wait`; a fast-failing daemon becomes a
zombie, and `pid_alive` reports a zombie as alive (compounds D3). **Fix:** the
daemon should `setsid`+double-fork so it reparents to init.

### D7 — Daemon robustness/visibility (LOW)
- `store_actor` has no `catch_unwind` around `execute` — a future panic in any
  `EventStore` op kills the actor and breaks all persistence for the daemon's
  life (`crates/daemon/src/store_actor.rs`). Wrap + reply with an error.
- Shutdown drains IPC tasks then the store, but detached per-command lifecycle
  waiters aren't drained, so a command exiting during shutdown loses its final
  `command_exited` event + audit row (`runtime.rs` vs `command.rs`). Track waiters
  in a `JoinSet` and drain before `shutdown_store`.
- `system_discover.methods` omits `registry_import_pack` and `shutdown`
  (`server.rs` ~L647) — advisory only, but derive the list from the dispatcher to
  prevent drift.
- `registry_import_pack` with `activate=true` commits pack rows + activates rules
  in a loop; a mid-loop activation error leaves a partially-active pack with no
  rollback (`handlers/registry.rs`). Validate scope up front or report partial
  success.

### D8 — Core bucket edge cases (LOW, mostly latent/internal)
`Bucket::append` accepts a caller-supplied `seq` into an empty bucket (breaks
cursor monotonicity if it saturates), uses `saturating_add` for seq assignment
(silent duplicate at `u64::MAX`), and reports `head_seq = tail_seq` for a fully
drained bucket (`head_seq` then misrepresents an empty window)
(`crates/core/src/bucket.rs`). Reachable only via the pre-set-seq path / extreme
counts. **Fix:** reject nonzero pre-set seq except on restore; `checked_add` →
typed `SeqExhausted`; sentinel for empty-bucket head.

### D9 — Sifter / probe latent issues (LOW)
- **Progress pre-filter precedence:** `noise_pipeline` drops a frame matching
  `is_progress_line` (`n/m` or pure-`%`) **before** the sifter runs, rule-agnostic
  — so a (hypothetical) rule targeting a pure fraction/percentage could never
  fire. No shipped seed pack trips it, but it violates "never drop a matched
  signal." Prefer: run evaluate first, only progress-drop when nothing matched.
- **Dedupe clock-base mix:** GC compares draft timestamps against an injected
  `now`; replay/file probes with historical timestamps can purge dedupe entries
  immediately, degrading collapse (token economy, not data loss).
- **File rotation identity:** rotation is inferred from `size < prev` only; a
  same-size-or-larger replacement isn't detected (data loss/skip). Track
  dev+ino (unix) / file-index+volume-serial (windows).
- **PTY:** trailing-`\r` clears the line buffer before flush (a trailing-CR secret
  prompt is wiped → never detected); peek-path + flush-path can double-count
  `secret_prompts_total`.
- **`process.rs`:** `child.id().expect(...)` can panic for a microsecond-lived
  child whose PID is already reaped — make `child_pid` an `Option`.

---

## Verification
- `cargo clippy --workspace --all-targets` — clean (0 warnings) before and after.
- New + existing unit tests pass for: core, store, mcp, probes, sifters; JS
  `cli-restart` / `wsl-spawn`.
- Full `cargo test --workspace` — see CI / `outputs/test_all.txt`.
