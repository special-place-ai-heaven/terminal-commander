# Stale-Branch Supersession Audit — 2026-07-11

**Repository:** `C:\AI_STUFF\PROGRAMMING\terminal-commander`
**Baseline:** `main` @ `a26dc49` (tag `v0.1.79`) — verified clean before and after.
**Method:** read-only. `git log -S`, `git patch-id --stable`, `git cherry`, direct struct/function inspection, SymForge index (646 files), three parallel `code-reviewer` agents whose findings were spot-checked against the actual main tree. No branch checked out, no file modified, no history touched.

> [!IMPORTANT]
> `git cherry` / `git patch-id` proved the two harness branches are **byte-identical in code** to commits already on main (only their commit *messages* were reworded on re-land). The canary branch is the opposite: `git cherry` marks it `+` (not patch-equivalent), and semantic tracing confirms **six of its seven findings are genuinely absent from main**. "Unmerged" was not the deciding factor either way — the code was.

---

## 1. Executive verdict

| Branch | Tip | Ahead/Behind | Code vs main | Verdict | Deletion recommendation |
|---|---|---|---|---|---|
| `fix/harness-auto-registration` | `3d02960` | +1 / −128 | **Byte-identical** to main `393f89c` (patch-id `e62c5d6…`) | **FULLY_SUPERSEDED** | **SAFE_TO_DELETE** |
| `fix/harness-windows-shell-false` | `06571bf` | +1 / −124 | **Byte-identical** to main `c0b695b` (patch-id `de2208d…`) | **FULLY_SUPERSEDED** | **SAFE_TO_DELETE** |
| `fix/canary-trust-fixes-f1-f7` | `4aed3df` | +1 / −128 | **Not** in main; F1, F2, F6 + Windows/pipeline tool-docs unique; F4 minor; only F7 superseded | **PARTIALLY_SUPERSEDED / STILL_UNIQUE** | **KEEP_FOR_PORT** |

Both `origin/*` upstreams for the two harness branches are already **`[gone]`** (deleted on the remote) — corroborating that their work landed and the remote branches were pruned.

---

## 2. Per-branch claim-by-claim evidence

### 2.1 `fix/harness-auto-registration` (`3d02960`, 19 files, JS only)

Merge-base `35aa3c2`. `git cherry -v main` → `-` (patch-equivalent present in main). `git patch-id --stable` of `3d02960` and main `393f89c` are **identical** (`e62c5d6e792710571afb89b8d25e69865dd0bc0c`). `diff <(git show 3d02960) <(git show 393f89c)` differs **only** in the commit header/prose (author email, date, reworded body); every `+/-` code hunk is identical.

| Old behavior (branch) | Current-main successor | Evidence | Classification | Residual risk |
|---|---|---|---|---|
| Setup force-refreshes `terminal_commander` key (no `ALREADY_EXISTS` skip), preserves siblings | Same code | main `393f89c` (PR #137 line) — identical hunks | FULLY_SUPERSEDED | None |
| Absolute exe via `resolveDirectExePath()`; refuse transient npx/temp binaries; bare only as warned last resort | Same code | identical patch-id | FULLY_SUPERSEDED | None |
| Atomic (temp+fsync+rename) + timestamped `.bak` + malformed-config-safe + BOM-strip writers | Same code | identical patch-id; reinforced by later `122f7f6` (atomic fsync + scope-check on every writer) | FULLY_SUPERSEDED | None |
| npm postinstall runs `runBootstrap` install-mode, fail-soft, CI/`TC_NO_AUTO_SETUP` no-op | Same code | identical patch-id | FULLY_SUPERSEDED | None |
| 411 JS tests | Same tests | identical patch-id | FULLY_SUPERSEDED | None |

**Bottom line:** the branch diff and the main commit are the same bytes of code. Nothing is lost by deleting it.

### 2.2 `fix/harness-windows-shell-false` (`06571bf`, 4 files, JS only)

Merge-base `fd0623b`. `git cherry -v main` → `-`. `git patch-id --stable` of `06571bf` and main `c0b695b` are **identical** (`de2208d727605622b9aa55262cc9c627bf75d5da`). Patch bodies differ only in commit-message prose.

| Old behavior (branch) | Current-main successor | Evidence | Classification | Residual risk |
|---|---|---|---|---|
| Never write a bare MCP command on Windows under `shell:false` (ENOENT); resolve absolute `.exe`; bare only last-resort non-Windows | Same code | main `c0b695b`; further hardened by `48a699b` (runtime detection + daemon recovery) and `d1ec657` (version-aware WSL gate) | FULLY_SUPERSEDED | None |
| Tests: bare-on-win32 forbidden when a path resolves; exePath precedence; `update` re-registers only on npm success | Same tests | identical patch-id (`test/harness-direct-exe.test.js`, `test/update-reregister.test.js`) | FULLY_SUPERSEDED | None |

**Bottom line:** identical code already on main, plus main added *further* Windows/WSL hardening on top. Nothing is lost by deleting it.

### 2.3 `fix/canary-trust-fixes-f1-f7` (`4aed3df`, 14 files, Rust `crates/` + `.agent/` goal docs)

Merge-base `35aa3c2`. `git cherry -v main` → **`+`** (NOT patch-equivalent — nothing like it in main). Traced each finding into main by symbol, struct field, `git log -S`, and function-body inspection; agent findings independently re-verified against `git show main:…`.

| Finding | Old behavior (branch) | Current-main successor | Evidence | Classification | Residual risk if deleted |
|---|---|---|---|---|---|
| **F1** pipefail wrap | `apply_pipefail_wrap` prepends `set -o pipefail;` to `-c`/`-lc` scripts containing an unquoted `\|`; quote-aware `shell_script_has_pipeline` scanner | **None** | `git log -S pipefail -- crates/` empty; `git grep pipefail\|apply_pipefail_wrap main -- crates/` = 0 | **STILL_UNIQUE** | Failing upstream pipe stage in a shell script still reports `exit 0`; no pipefail injected |
| **F1** masked-exit tracking | `JobBinding.pipeline_in_argv` + `CommandStatusResponse.pipeline_exit_masked: Option<bool>` warning | **None** | `git show main:crates/ipc/src/protocol.rs` — `CommandStatusResponse` ends at `restarted: bool`, **no masked-exit field under any name**; `git grep pipeline_exit_masked main -- crates/` = 0 | **STILL_UNIQUE** | A pipeline that exits 0 via masking is reported as a clean success with no warning |
| **F1** tests | `pipeline_failure_reports_nonzero_exit_with_pipefail` + 3 unit tests (incl. `bracket_test_survives_pipefail_wrap`, `echo 'a\|b'` must not wrap) | **None** | test names = 0 hits on main | **STILL_UNIQUE** | No regression guard; the tasteful quote-aware edge-case discipline is gone |
| **F2** argv guard | `CommandError::PosixPathOnWindows` + `#[cfg(windows)] validate_windows_posix_argv_paths` rejecting `/…` argv on a Windows daemon | **None** | `CommandError` enum on main has no such variant; `validate_argv` checks only empty/too-long; `git log -S PosixPathOnWindows main` = 0 | **STILL_UNIQUE** | Windows daemon mis-resolves `/home/…` argv → `C:\home\…`; no pre-spawn rejection |
| **F2** file-op guard | `looks_like_posix_absolute` + `posix_path_on_windows_error` at TOP of `resolve_and_authorize_file`/`_write`, before the "must be absolute" gate | **None** | main's first gate is `if !path.is_absolute()` → generic "must be absolute" (`common.rs:235-247`, `302-315`); repo-wide grep for both helpers = 0 files | **STILL_UNIQUE** | `/home/…` file path on Windows daemon yields the **misleading** "must be absolute" error instead of "use `wsl`" guidance |
| **F2** error map + test | `map_command_error` arm → PathDenied w/ wsl guidance; `#[cfg(windows)]` test asserting `Linux/WSL` and NOT `must be absolute` | **None** | no arm in main `map_command_error`; test name = 0 hits | **STILL_UNIQUE** | No mapping, no regression guard |
| **F4** idempotent retry | 2-attempt post-heal retry loop (`for attempt in 0..2`) with re-heal between attempts, gated behind `is_idempotent` | Main retries **once**, no inter-attempt re-heal | `daemon_client.rs:317` "retried once"; `retry_gate.rs` asserts exactly 2 connections; `48a699b` touched the file but only added version-skew probing | **STILL_UNIQUE** (behavioral delta, not a bug) | Minor: a transient transport blip on the *first* post-heal retry (e.g. mid `bucket_wait` across a daemon replace) surfaces a spurious `daemon_unavailable` that a 2nd heal would absorb |
| **F6** live-only filter | `ListLimitParams.live_only` + handler `retain(Starting\|Running)` + MCP `runtime_state`/`probe_list` wiring & docs | **None** | `git show main:crates/ipc/src/protocol.rs` — `ListLimitParams` has only `limit`; both handlers apply only `limit`/`take`; `git grep live_only main -- crates/` = 0 | **STILL_UNIQUE** (recoverable) | Convenience only: main still emits `ProbeListEntry.liveness` (full union), so an agent can filter to non-terminal probes **client-side** |
| **F7** wait_hint | `run_and_watch_result` adds `wait_hint` field on `wait_exhausted && !complete && !degraded` | **Present, better, different field** | main folds identical trigger into `recover_hint` via `RUN_AND_WATCH_WAIT_EXHAUSTED_HINT` (`tools.rs:3441`, `4435`), landed `96a3f3d`/PR #148; wording is *richer* (points at `bucket_wait` signal-resume path the branch omits) | **FULLY_SUPERSEDED** | None material — only the literal field name `wait_hint` differs |
| **Tool-desc** hardening | `command_start_combed`/`run_and_watch`/`pty_command_start` docs gain Windows-buffering, pipefail, bare-`/home` argv, `pipeline_exit_masked` guidance | **None** | `git grep 'piped children\|buffering stdout\|bare /home\|pipefail\|pipeline_exit_masked' main -- crates/mcp/src/tools.rs` = 0 | **STILL_UNIQUE** (partly inert) | Windows-buffering + posix-path advisory prose is portable; the pipefail/`pipeline_exit_masked` half references F1 runtime behavior that also isn't on main |

**Bottom line:** F7 is the only finding main independently reproduced (and improved). F1 and F2 (both halves + tests), F6, and the Windows/posix tool-doc prose are **entirely absent** from main — no differently-named equivalent exists (verified by `git log -S` + struct inspection, not just grep). F4 is a deliberate single-retry on main; the branch loop is a minor optional resilience upgrade. **Deleting this branch permanently loses F1, F2, F6, and the advisory docs.**

---

## 3. Deletion recommendations

- **`fix/harness-auto-registration` → SAFE_TO_DELETE.** Its code is byte-identical (verified patch-id) to main `393f89c`, which main then reinforced with `122f7f6`. Deleting loses **no behavior, no test, no doc** — only a duplicate commit with an older commit message. Remote upstream already `[gone]`.
- **`fix/harness-windows-shell-false` → SAFE_TO_DELETE.** Code byte-identical (verified patch-id) to main `c0b695b`; main added *further* Windows/WSL hardening (`48a699b`, `d1ec657`) on top. Deleting loses nothing. Remote upstream already `[gone]`.
- **`fix/canary-trust-fixes-f1-f7` → KEEP_FOR_PORT.** It is the **sole location** of F1 (pipeline-exit masking), F2 (POSIX-path-on-Windows detection + guidance + tests), F6 (server-side `live_only` filter), and the Windows/posix tool-doc prose. It is 128 commits behind and will **not** merge cleanly — port the wanted findings onto a fresh branch off `main`, then delete.

> [!WARNING]
> Do not delete `fix/canary-trust-fixes-f1-f7` on the strength of its similar-sounding "F1–F7" subject or its `[gone]`-looking siblings. Its siblings landed; **it did not.** `git cherry` marks it `+`, and the code is provably absent from main.

---

## 4. Follow-up porting work (prioritized)

**P1 — F1 pipeline-exit masking (trust/correctness).**
A `-c`/`-lc` script like `failing-cmd | tail` reports `exit 0` on main today; the caller gets no signal the success is a lie. Re-apply `apply_pipefail_wrap` + the quote-aware scanner, the `JobBinding.pipeline_in_argv` flag, the additive `CommandStatusResponse.pipeline_exit_masked: Option<bool>` field, and the four tests, against main's current `JobBinding`/`CommandStatusResponse`. The `pipeline_exit_masked` field is `#[serde(default, skip_serializing_if = "Option::is_none")]` — additive and wire-compatible.

**P2 — F2 POSIX-path-on-Windows detection + guidance (Windows-daemon UX/trust).**
Small, self-contained: the `PosixPathOnWindows` variant slots cleanly into main's existing well-shaped `map_command_error` enum-dispatch; the two `common.rs` pre-checks + `#[cfg(windows)]` test port directly. **Priority caveat:** main's production target is Linux/WSL and the Windows-native daemon is explicitly deferred (`policy.rs`), so this hardens a deferred surface — but the branch is the only place the fix and its test exist, and re-deriving later costs more than salvaging now.

**P3 — F6 `live_only` filter (ergonomics) + Windows/posix tool-doc prose.**
F6 is a pure convenience (agents can already filter on the emitted `liveness` field client-side); port only if server-side filtering for cleanup audits is wanted. The clean design — one optional bool threaded through `ListLimitParams` + a `retain` in the shared handler, counts still reflecting true totals — is worth preserving verbatim. Port the Windows-buffering / bare-`/home` advisory prose alongside; **omit** the pipefail/`pipeline_exit_masked` doc lines unless P1 lands first (otherwise they reference a non-existent response field).

**Do NOT port — F7** (`wait_hint`): superseded by main's richer `recover_hint`/`RUN_AND_WATCH_WAIT_EXHAUSTED_HINT`. **F4** (2-attempt retry): optional; if ported, keep it gated behind `is_idempotent` and add a test that actually exercises the re-heal-on-attempt-0 path (main's current test fake can't — `try_self_heal` is a no-op without a status handle).

---

## 5. Commands / tests run

Read-only throughout; no test suite executed (the audit is a code-supersession trace, not a behavioral regression run).

| Command | Result |
|---|---|
| `git status` (start & end) | clean, `main` up-to-date with `origin/main` |
| `git rev-parse HEAD` (start & end) | `a26dc49b2cebb573f63d65f455336db5bcdc2b8a` (unchanged) |
| `git merge-base main <branch>` | canary/auto-reg `35aa3c2`; win-shell `fd0623b` |
| `git rev-list --count <mb>..<branch>` / `..main` | each branch +1 / main −128 (win-shell −124) |
| `git cherry -v main <branch>` | harness branches `-` (equiv present); canary `+` (not present) |
| `git patch-id --stable` (branch tip vs main successor) | `3d02960`≡`393f89c` (`e62c5d6…`); `06571bf`≡`c0b695b` (`de2208d…`) — identical |
| `diff <(git show branch) <(git show main-successor)` | differ **only** in commit-message header/prose; code hunks identical |
| `git log -S <symbol> main -- crates/` | `pipefail`, `apply_pipefail_wrap`, `pipeline_in_argv`, `pipeline_exit_masked`, `PosixPathOnWindows`, `looks_like_posix_absolute`, `live_only` → **all empty** |
| `git grep <symbol> main -- crates/` | `pipefail\|pipeline_exit_masked\|PosixPathOnWindows` = 0; `live_only` = 0 |
| `git show main:crates/ipc/src/protocol.rs` (struct inspect) | `CommandStatusResponse` has no masked-exit field; `ListLimitParams` has only `limit` — confirmed |
| `git show main:crates/mcp/src/tools.rs` (F7) | `recover_hint` + `RUN_AND_WATCH_WAIT_EXHAUSTED_HINT` at `:3441`/`:4435` — confirmed |
| `git show main:crates/mcp/src/daemon_client.rs` (F4) | "retried once" at `:317`; single retry, no loop — confirmed |
| SymForge `status` | index Ready, 646 files — used for navigation/impact |

---

## 6. Final repository state

- Branch: `main` — unchanged.
- HEAD: `a26dc49b2cebb573f63d65f455336db5bcdc2b8a` (tag `v0.1.79`) — identical before and after.
- `git status --porcelain`: empty (working tree clean).
- No branch deleted, no history rewritten, no source modified, no commit or push. This report is the only file added.
