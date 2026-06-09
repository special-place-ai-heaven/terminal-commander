# Cursor code-review request — TC trust-defect campaign (Phases 4-6: TC-4 / TC-5 / TC-3)

You are doing an independent, adversarial **second-opinion code review** of a finished
multi-phase bugfix campaign on a Rust workspace. Another model implemented it, reviewed it
with sub-agents (code + security), and ran both-OS gates. Your job is to find what they
MISSED — correctness bugs, security holes, race conditions, incomplete churn, convention
violations, and anything that would fail in production. Be skeptical; reproduce reasoning
from the code, do not trust the commit messages.

## Repo / scope

- Repo: Terminal Commander — a LOCAL-ONLY product. `terminal-commanderd` (daemon),
  `terminal-commander-mcp` (stdio MCP adapter), `terminal-commander` (admin CLI), plus
  `crates/probes`, `crates/ipc`, `crates/core`, `crates/supervisor`, `crates/sifters`.
- Branch: `fix/tc-trust-defects`. Review the campaign diff against its base:

  ```
  git --no-pager log --oneline 5b054cb..HEAD
  git --no-pager diff 5b054cb..HEAD
  ```

  Base `5b054cb` is the GOAL-file docs commit; everything after it is this campaign.

- The campaign closed three trust defects. Impl commits (newest last):
  - **TC-4** (probe-row + audit-log argv identity, redacted):
    `3b7e719` argv_head + tag identity on probe rows, redacted (daemon);
    `7fd0ec8` redact credentials in the command audit-log argv metadata (daemon);
    `c91200e` thread run_and_watch tag + surface probe identity in the CLI (mcp/cli).
    (`bb5b41d` is an unrelated clippy `from_secs` lint fix.)
  - **TC-5** (self_check false-green eliminated):
    `d136e95` self_check runs a real profile-gated command-spawn round-trip (daemon).
  - **TC-3** (command_stop — a started command could not be killed):
    `d94e1c5` command_stop force-kills a running command via gated IPC (daemon, 6a);
    `a5cdb6c` tear down the whole process tree on a command kill (probes);
    the last commit adds the `command_stop` MCP tool + the 37->38 tool-catalogue churn (6b).
  - `docs(campaign): ...` commits are planning docs; skip them.

## Hard constraints the change MUST hold (flag any violation)

- **Zero new crate dependencies.** Only a `windows-sys` *feature* add is allowed. Confirm
  `git --no-pager diff 5b054cb..HEAD -- Cargo.lock` shows NO new package.
- **MCP guard:** `crates/mcp/src/**` must contain NONE of these literal substrings (even in
  comments): `Command::new`, `Command::spawn`, `TcpListener`, `UdpSocket`, `tokio::fs`,
  `std::fs`, `File::open`, `read_to_string`, `read_to_end`.
- Local single-tenant trust model (one policy profile per daemon; `peer` = OS socket creds,
  used for the audit subject only, not for authorization).

## Where to look hardest (highest-risk surfaces)

### 1. TC-4 argv credential redactor (SECURITY-CRITICAL) — `crates/daemon/src/command.rs`
`redact_argv` / `redact_argv_head` + helpers `mask_token_inline`, `mask_url_userinfo`,
`mask_header_credential`, `env_key_is_secret`. Used by `collect_probes`
(`crates/daemon/src/ipc/handlers/runtime.rs`) for the OPERATOR-FACING probe listing and by
`format_argv_metadata` for the AUDIT LOG. Additive wire fields `tag` + `argv_head` on
`ProbeListEntry` (`crates/ipc/src/protocol.rs`).
- Hunt for ANY bypass that leaks a real credential into the probe listing or audit metadata:
  casing, `=` vs space, short/long/attached flags, URL userinfo (`scheme://user:PASS@host`),
  env `KEY=VALUE` secrets, `Authorization`/`Bearer`/custom headers, basic-auth `-u user:pass`.
  Does the bounded head (3 items) surface a secret it should drop, or drop a flag whose value
  it should mask? Is the 128-byte truncation char-boundary safe (no multibyte panic)?
- Confirm the audit path (`format_argv_metadata` -> `redact_argv(argv, None)`) redacts the
  FULL argv so a secret PAST index 3 is still masked.
- Over-redaction is acceptable (safe); UNDER-redaction (a leaked secret) is a finding.

### 2. TC-3 command_stop kill ordering (SECURITY-CRITICAL) — `crates/daemon/src/command.rs` `CommandRuntime::stop`
Required order: (1) evaluate `PolicyAction::CommandSignal` FIRST; on Deny emit a deny audit
row whose SUBJECT is the peer identity (NEVER the job_id) and return PolicyDenied WITHOUT
touching the live map (no existence oracle); (2) only after Allow, live-map lookup (UnknownJob
if absent), check-then-set Cancelled iff not terminal under the live write lock;
(3) job-id allow audit BEFORE firing the kill.
- Can a denied caller distinguish a real job from a nonexistent one (timing / error / audit)?
- Can the deny audit ever contain the job_id or argv?
- Trace the stop-vs-natural-exit race: the waiter terminal guard in `start_combed_inner`
  (between receipt-publish and the draft match), and the TC-2 dedup evict on the guard path.
  Worst case — double-finish? leaked dedup entry? stuck non-terminal job?
- Lock ordering: `stop` nests `live.write` -> `jobs.write`. Any reverse nesting that deadlocks?

### 3. TC-3 process-tree kill (SECURITY-CRITICAL) — `crates/probes/src/process.rs`
`ProcessProbe::spawn` (unix `process_group(0)`; windows Job Object via `create_job_for_child`),
`kill_process_tree`, `JobHandle` + its `Drop`.
- UNIX: the kill uses `kill -s KILL -- -<pgid>` via the `kill(1)` tool. CONFIRM it can NEVER
  signal the daemon's own process group or any unrelated group (a prior iteration used
  `kill -KILL -<pgid>`, which procps-ng mis-parsed and SIGKILLed the CALLER's group — confirm
  that bug is fully gone and no other mis-target path exists, incl. pid-reuse windows).
- WINDOWS: Job Object handle lifetime — every Win32 return checked? `CloseHandle` exactly once
  (no double-close / use-after-close)? Is `TerminateJobObject` safe vs a concurrent Drop
  (Arc-shared)? Does `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` ever kill a still-wanted tree on a
  NORMAL (non-cancel) completion?
- Both: is the degraded fallback (job-create fails / `kill` absent -> single-process kill)
  acceptable, and can it ever escalate to a wrong target? EDR: the windows PRODUCT path must
  spawn NO external process (no taskkill/powershell/cmd) — test-only `wmic`/`tasklist` is fine.

### 4. TC-5 self_check real spawn — `crates/daemon/src/ipc/server.rs` `handle_self_check` / `selfcheck_spawn_probe`
Plus the `start_combed` reuse seam in `command.rs` (`start_combed_inner(req, reuse_bucket)`),
the `selfcheck_bucket` cache on `DaemonState`, and `Cmd::SelfcheckNoop` in
`crates/daemon/src/main.rs`.
- NEVER false-RED a healthy daemon (policy Deny / repo_only-no-root / unresolvable current_exe
  must SKIP, not fail). NEVER false-GREEN (spawn error / nonzero exit / never-terminal must set
  failures>0). Is `healthy = Exited && exit_code==Some(0)` correct?
- The cached-bucket parking_lot Mutex must NEVER be held across the poll `.await`.
- A fresh `dedup_nonce` per call must defeat the TC-2 in-flight dedup (else two self_checks
  collapse and the 2nd never spawns). Confirm.
- The `SelfcheckNoop` short-circuit MUST precede `resolve_config` in `main()` (a child
  inheriting a broken `--config`/env must not false-RED).
- The reuse seam: `start_combed(req)` must be behaviorally identical to before (it delegates
  `None`); the `Some(bucket)` path skips bucket_create + source.record and reuses the id with
  NO AlreadyExists error, so `bucket_count` grows by exactly 1 over the daemon lifetime.

### 5. TC-3 6b atomic 37->38 tool churn (COMPLETENESS) — `crates/mcp/**` + `tests/fixtures/contracts/**`
A `command_stop` MCP tool was added and the live-tool catalogue went 37 -> 38. Verify NO site
was left at the old count and the new tool sorts correctly (catalogue order: after
`command_status`; alpha-sort: between `command_status` and `event_context`). Sites:
`tool_catalogue`, the catalogue + router unit tests in `tools.rs`, the doc-count strings
(`tools.rs` x2, `lib.rs`, `main.rs`), `mcp_stdio.rs`, `mcp_live_daemon.rs` (vec + `live_count`
assert + comment + module-doc), `daemon_unavailable_envelope.rs` (`checked` 36->37 + the
`minimal_tool_args` arm + module-doc), `mcp-tool-fixture-map.v1.json` (counts + arrays),
`system_discover.v1.json`, and the NEW `command_stop.v1.json`. The contract test
`fixture_catalogue_contract.rs` gates the fixtures. Check the `command_stop` tool body
(`tools.rs`) is pure IPC forwarding (no MCP-guard literals) and NON-idempotent
(`into_mcp_error_for(false, ...)`).

## What has ALREADY been verified (so you can focus elsewhere)

- Both-OS gates green: Windows (`scripts/windows-gate.ps1`, fmt, clippy `-D warnings`, lib +
  integration tests) and WSL (clippy `-D warnings`, `cargo nextest` on the touched crates; the
  workspace-wide `session_reap` CLI test is a known drvfs wedge, excluded). 26 redactor unit
  tests; live TEST-socket round-trips per defect; the unix grandchild tree-kill; the
  catalogue/router/fixture-contract tests; and `cargo nextest -p terminal-commander-mcp`
  (125/125), `-p terminal-commanderd` (378/378), `-p terminal-commander-probes` (49/49) all
  pass on WSL.
- Note: the Windows clippy gate does NOT lint `#![cfg(unix)]` test bodies (it compiles them to
  nothing); the WSL clippy run is authoritative for those.

## Output

Write your review to **`.planning/tc-bugfix-campaign/cursor_review_tc456.md`** with:
- A findings list, each: `[SEVERITY] file:line — issue — why it matters — suggested fix`.
  Severities: BLOCKER / HIGH / MEDIUM / LOW / NIT.
- Per the 5 surfaces above, an explicit CONFIRMED-SAFE or ISSUE.
- A one-paragraph overall verdict: is this campaign safe to push (it triggers `release-please`
  on the `fix:`/`feat:` commits), or what must change first.
- If you find NO blockers, say so plainly.

Be concise and concrete. Prefer a real reproduction or exact trace over speculation. Thanks.
