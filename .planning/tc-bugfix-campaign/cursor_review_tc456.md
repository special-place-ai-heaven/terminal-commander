# Cursor adversarial review — TC campaign Phases 4–6 (TC-4 / TC-5 / TC-3)

Reviewed diff `5b054cb..HEAD` on `fix/tc-trust-defects` (impl commits through
`c7bcfbd`). Planning-only `docs(campaign):` commits skipped. Code read directly;
commit messages not trusted.

## Overall verdict

**No blockers.** The campaign fixes the three advertised trust defects with
coherent implementations, strong test coverage on the high-risk paths, and
constraint compliance (zero new Cargo packages, MCP guard clean, `CommandStop`
correctly non-idempotent at MCP and IPC). The argv redactor and `command_stop`
deny-first ordering match the security intent; process-tree kill uses the
documented safe `kill -s KILL -- -<pgid>` form on Unix and Job Objects on
Windows without product-path shell-outs.

Safe to push from a correctness/security standpoint. Optional follow-ups below
are hardening nits (extended flag coverage, forced-stop lifecycle observability,
docs drift in planning artifacts) — none block merge or release-please.

---

## Findings

### BLOCKER

(none)

### HIGH

(none)

### MEDIUM

1. **[MEDIUM] `crates/daemon/src/command.rs:1394-1416,1530-1537` — attached
   `--api-key=` / `--passwd=` / `--pass=` not in curated secret-flag lists —
   under-redaction for common CLI spellings — extend Layer A look-ahead and Rule
   (1) attached prefixes (e.g. `--api-key=`, `--api_key=`, `--passwd=`, `--pass=`)
   or document as accepted gap with env-style `KEY=VALUE` as the supported path.**

   Verified covered: `-p`/`-ppass`, `--password`, `--token`, `--secret`, `--key`
   (space and `=`), `-u`/`--user` with `:`, `-H`/`--header`, URL userinfo, env
   `*_PASSWORD` / `*_TOKEN` / `*_KEY` suffixes, deep-index audit via
   `format_argv_metadata` (`redact_tests::format_argv_metadata_redacts_secret_at_deep_index`).

   Repro trace: token `--api-key=AKIA...` — `lower.starts_with("--key=")` is
   false; Layer A has no `--api-key`; env rule requires `=`-split key matching
   `env_key_is_secret` (`api_key` exact, not `api-key`). Raw secret survives in
   both `argv_head` and audit JSON. Same for `--passwd=secret` (exact env key
   `passwd` does not apply to flag form). Over-redaction elsewhere is intentional;
   this is the main under-redaction gap found.

2. **[MEDIUM] `crates/daemon/src/command.rs:856-875,1024-1026` — forced
   `command_stop` skips bucket `command_exited` append and exit audit waiter
   path — operators/agents polling the bucket for lifecycle may not see a stop
   event — consider emitting a synthetic lifecycle draft on stop (mirroring
   natural exit) or document that `command_stop` + `command_status` are the
   authoritative stop signals.**

   Trace: `stop()` sets `Cancelled` under `live.write`, fires cancel, returns.
   Waiter in `start_combed_inner` sees terminal `Cancelled`, evicts dedup, returns
   before `finish`/`bucket_append`/exit audit (`856-875`). Allow audit with
   job_id fires at `1014-1023`. Functionally correct for kill + status; bucket
   signal stream gap only.

### LOW

3. **[LOW] `crates/daemon/src/command.rs:1024-1026` — cancel one-shot send
   errors ignored (`let _ = tx.send(())`) — if the lifecycle task died before
   receiving, job is `Cancelled` in ledger but OS process may still run — add
   debug audit or treat `send` failure as internal error after allow audit.**

   Requires a pre-existing task failure; `cancel_tx` is taken once at spawn into
   `JobBinding` and should remain valid for normal lifetimes.

4. **[LOW] `.planning/tc-bugfix-campaign/00-INDEX.md:118` — still claims 37 live
   tools — planning doc drift only; production `TOOL_CONTROL_SURFACE.md` and MCP
   sources say 38.**

5. **[LOW] `crates/mcp/tests/daemon_unavailable_envelope.rs:12` — module doc says
   "37 daemon-backed tools" while assert at `:215` correctly expects `37`
   (= 38 catalogue − `system_discover`). Comment is accurate math but easy to
   misread; optional clarify "37 daemon-backed of 38 live".**

### NIT

6. **[NIT] `crates/daemon/src/command.rs:1391-1412` — `-h` treated as `--header`
   not `--help` — intentional over-redaction; benign `curl -h` help text may show
   redacted next token if present. Safe per campaign trade-off.**

7. **[NIT] `crates/probes/src/process.rs:452-456` — Unix tree kill shells out to
   `kill(1)` from the daemon — acceptable (mirrors supervisor); not an MCP/EDR
   product-path violation (daemon is privileged).**

---

## Surface verdicts

### 1. TC-4 argv credential redactor — **ISSUE (medium, flag coverage gap)**

- **CONFIRMED-SAFE** for the patterns explicitly tested (26 unit tests): Bearer
  header, URL userinfo including embedded `@` in password, `-ppass`, space-separated
  `--password`, `--token=`, env assignments, attached `--header=`, custom
  `X-Vault-Token:`, deep audit index, multibyte 128-byte truncation (char-boundary
  loop at `1427-1434` — no panic).
- **CONFIRMED-SAFE** audit vs head split: `format_argv_metadata` →
  `redact_argv(argv, None)` masks full argv; `redact_argv_head` → `Some(3)` drops
  items beyond index 2 (by design, not a leak).
- **CONFIRMED-SAFE** wiring: `collect_probes` uses `redact_argv_head` for command
  (stored at bind) and PTY (read-time); `format_argv_metadata` on allow/deny
  start audits (`778-783`).
- **ISSUE**: attached `--api-key=` / `--passwd=` / `--pass=` bypass (finding #1).

### 2. TC-3 `CommandRuntime::stop` ordering — **CONFIRMED-SAFE**

Trace of `stop` (`962-1027`):

1. `PolicyAction::CommandSignal` evaluated first (`970-981`). Deny → audit
   `command_stop` / `peer_subject` / `deny`, **no** `live` touch, **no** job_id
   in audit metadata (`977` uses `None` metadata).
2. Allow → `live.write()`, `get_mut` → `UnknownJob` only here (`988-989`).
3. Terminal check via `jobs.get` (`998-1004`) → no-op return without second allow
   audit (integration test `command_stop_second_stop_on_terminal_job_is_noop`).
4. `cancel.take()`, `jobs.cancel` under held `live` lock (`1006-1009`).
5. Allow audit with job_id wire string (`1014-1023`), then kill send.

**Deny oracle**: `command_stop_ipc.rs::command_stop_read_only_observer_is_denied_with_peer_subject_no_oracle`
asserts `PolicyDenied`, not `UnknownJob`, and deny audit subject is peer uid —
matches code.

**Waiter / dedup race**: Guard after receipt-publish (`856-875`) evicts `dedup_k`
on early return when `stop()` already set terminal — fixes the documented leak
where nonce-keyed dedup would otherwise never TTL-release. No double-finish path
found: `stop` sets `Cancelled` before waiter can `finish`.

**Lock ordering**: `stop` nests `live.write` → `jobs.cancel` (internal
`jobs.write`). Waiter publishes receipt under `live.write`, drops lock, then
`jobs.finish` — sequential, not reverse-nested. No deadlock cycle identified.

### 3. TC-3 process-tree kill — **CONFIRMED-SAFE**

**Unix** (`427-458`): Uses `kill -s KILL -- -{pgid}` with explicit comment on
procps `-KILL -<pgid>` mis-parse killing caller's group. Child uses
`process_group(0)` (`264`) so `pgid == child_pid`, not daemon's group.
Integration test `unix_grandchild_is_killed_on_cancel` observes grandchild death.

**Windows** (`496-559`, `460-477`): Job Object with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; `CloseHandle` once on failure paths and in
`JobHandle::Drop` (`171-181`); `Arc<JobHandle>` shared with lifecycle task.
`TerminateJobObject` on cancel; normal exit → `wait` → probe drop → job close
kills straggler descendants (intended). No `taskkill`/PowerShell in product path.
Degraded path: `start_kill` only if job creation fails (`474-476`).

**Pid reuse**: Kill runs while child still in `select!` cancel arm; not post-reap.

### 4. TC-5 self_check real spawn — **CONFIRMED-SAFE**

- `SelfcheckNoop` short-circuit before `resolve_config` (`main.rs:107-108`).
- `selfcheck_spawn_probe`: policy Deny → SKIP (`failed: false`, `862-866`);
  `current_exe` Err → SKIP (`1004-1007`); spawn/status/timeout failures →
  `failed: true`.
- Healthy = `JobState::Exited && exit_code == Some(0)` (`929`) — consistent with
  `JobManager::finish` mapping nonzero to `Failed`.
- `parking_lot::Mutex` on `selfcheck_bucket`: read/store in scoped blocks only;
  poll loop awaits `sleep` with no lock held (`872-961`).
- Fresh `dedup_nonce` per probe (`fresh_selfcheck_nonce`, `885`) defeats TC-2
  collapse.
- Reuse seam: `start_combed` → `start_combed_inner(req, None)`; reuse path skips
  `bucket_create`/`source.record` when `Some(bid)` (`636-637` guard). Bucket id
  cached after first spawn (`904-908`).

### 5. TC-3 6b atomic 37→38 tool churn — **CONFIRMED-SAFE**

Verified in production/test code (not stale planning docs):

| Site | Status |
|------|--------|
| `tool_catalogue()` + `catalogue_lists_thirty_eight_live_tools` | 38, `command_stop` after `command_status` |
| `tool_router_exposes_all_live_tools` sorted vec | `command_stop` between `command_status` and `event_context` |
| `tools.rs` doc strings lines 20, 29 | "38 live" / "38 tools" |
| `lib.rs:12`, `main.rs:12` | 38 |
| `mcp_stdio.rs`, `mcp_live_daemon.rs` vec + `assert_eq!(live_count, 38)` | OK |
| `daemon_unavailable_envelope.rs` `checked == 37` + `command_stop` in `minimal_tool_args` | OK (38−1) |
| `mcp-tool-fixture-map.v1.json` | `live_tools: 38`, `covered_live: 34`, `command_stop` entry |
| `system_discover.v1.json`, `command_stop.v1.json` | present |

**`command_stop` MCP tool** (`tools.rs:822-853`): pure
`IpcRequest::CommandStop` forward; `into_mcp_error_for(false, ...)` — correct
non-idempotent. No MCP-guard literals in new code.

---

## Hard constraint audit

| Constraint | Result |
|------------|--------|
| Zero new crate dependencies | **PASS** — `git diff 5b054cb..HEAD -- Cargo.lock` empty; only `windows-sys` feature additions in `crates/probes/Cargo.toml` |
| MCP guard literals in `crates/mcp/src/**` | **PASS** — only pre-existing prohibition doc comments in `lib.rs` / `main.rs` |
| Local single-tenant trust model | **PASS** — deny audit uses `identity_audit_subject(peer)`; policy profile is daemon-wide |

---

## What prior verification likely caught (not re-litigated)

- WSL nextest counts cited in prompt (mcp 125, daemon 378, probes 49).
- Live TEST-socket round-trips per defect (campaign results).
- Windows gate + targeted clippy/tests.
- `command_stop_ipc.rs` integration suite (kill, deny/no-oracle, second stop no-op,
  unknown job under allowed profile).

---

## Recommended pre-push (optional, non-blocking)

1. Add redactor tests + rules for `--api-key=` / `--passwd=` (finding #1).
2. Document or emit bucket lifecycle on forced stop (finding #2).
3. Refresh `.planning/tc-bugfix-campaign/00-INDEX.md` tool count (finding #4).
