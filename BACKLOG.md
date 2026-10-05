# Terminal Commander - Backlog

Status: TC48 beta gate snapshot.

Backlog tracks open work after the TC33-TC47 runtime chain landed.
The four historical P0 blockers (rmcp stdio adapter, PTY spawn, UDS
IPC, persistent audit writes) are now resolved on `main` and listed
in the "Resolved P0" section for traceability. Active work below is
prioritized against the current evidence.

Language: ASCII only.

## Omni completion program (as_of 2026-06-16; on review branches, NOT merged)

> Update 2026-10-05: MERGED. No `feature/omni-*` ref exists any more and
> the slice code is on `main` (e.g. `crates/daemon/tests/shell_session_ipc.rs`,
> the `portable-pty` / `notify` dependencies in `crates/probes`, 25 packs in
> `crates/store/rules/`; omni PRs such as #99 and #182). The live MCP
> catalogue is now 60 tools (`docs/mcp/TOOL_CONTROL_SURFACE.md`), not 49.
> The text below is the 2026-06-16 snapshot and its "open before 1.0.0" list
> is otherwise unchanged (O-07/O-08/O-09/O-10/O-14 and the P4 threat review
> were not re-verified).

The omni-completion program (`specs/001-omni-completion/`) landed on stacked
per-slice review branches (paused before merge/push for human review):

- P1 `feature/omni-p1-sessions` (d9b1c75, 673e0bd, eec1e38): persistent shell
  sessions + workspace snapshots (tools 39->46) + folded ledger fixes TC-B1
  (ANSI strip + CRLF-aware normalizer), TC-E1 (compact), TC-E4 (capture canon),
  TC-E2 (honest wait cap), TC-B3 (job-receipt restart status). O-02 live.
- P2 `feature/omni-p2-parse` (78f9188): registry_suggest_from_samples (never
  auto-activates), universal extractors, 8->25 rule packs, pack hints
  (tools 46->47). O-05 live.
- P3 `feature/omni-p3-platform` (16fd537, 2e636b5): Windows ConPTY dual-backend
  (lifecycle live; child-output e2e blocked-on-host TC_CONPTY_E2E), notify file
  backend (+9P poll fallback), SIGTERM->SIGKILL grace ladder. macOS = code-only
  (no host).
- P5 `feature/omni-p5-remote` (b1a3dd6): target_list/target_probe + target_id
  routing on the command path via operator-forwarded local socket, no public TCP
  (tools 47->49). Sim-verified (second local socket); real-SSH untested (no sshd).
- P6 `feature/omni-p6-certify` (1311cb4, 59d70fc): system_discover.omni_status
  matrix, verify-omni-* smokes, OMNI_PLAYBOOK + README/SPEC/ROADMAP realign.
- P4 privileged helper: PLAN-ONLY (docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md);
  BLOCKED-ON-REVIEW, zero code.

Open before a 1.0.0: merge the review branches (human-gated); close O-07
(ConPTY child-output on a real Windows desktop/CI), O-08 (macOS host), O-09/O-10
(SSH/container), O-14 (provider trust smokes); complete the P4 threat review.

Update 2026-10-05, status of each gate from code/CI (a CI run result itself is
not visible from the repo):
O-07 DONE in CI wiring: `scripts/windows-gate.ps1` runs the live ConPTY
child-output e2e with `TC_CONPTY_E2E=1` on GitHub Actions and refuses a
vacuous pass; local hosts without ConPTY still need the opt-in.
O-08 STILL OPEN: `omni-o-runner.py` only runs it on macOS and no workflow runs
`verify-omni-macos.sh` (the `verify-mac-*` release jobs only smoke the published
package). O-09/O-10 STILL OPEN: the runner skips both ("needs-ssh-remote-host",
"needs-container-runtime"); `remote_federation_e2e.rs` covers a second local
socket only. O-14 NOT DETERMINABLE from the repo: a human must run a real
provider session (`docs/integrations/omni-harness-smoke.md`). P4 threat review
STILL OPEN: `docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md` is
BLOCKED-ON-REVIEW, and `system_discover` reports `threat_review_pending`.

## P0 — Beta blockers (none active)

All P0 items are resolved (see "Resolved P0" below). The four original
items closed with the TC33-TC47 runtime chain; the two P0 trust defects
found by the TC trust-defects campaign (`.planning/tc-bugfix-campaign/`)
are both fixed at HEAD and were re-verified against source on
2026-08-07 while assessing release readiness.

### P0.1 — TC-1a: client-timeout blind retry double-spawns (RESOLVED)

**Was:** `crates/mcp/src/daemon_client.rs` re-sent a cloned request on any
transport error, so a >5s client timeout on a `CommandStartCombed` could
re-send a mutating start while the first spawn was still running.
**Resolved by:** `IpcRequest::is_idempotent()`
(`crates/ipc/src/protocol.rs`) now gates the re-send. Self-heal is split
from re-send: the heal always runs, but only idempotent RPCs are retried
(`crates/mcp/src/daemon_client.rs:433-445`). Mutating RPCs -- including
`CommandStartCombed` and every registry write -- return the transport
error instead of double-spawning.

### P0.2 — TC-1b: run_and_watch discards a live job handle (RESOLVED)

**Was:** once `run_and_watch` held a `job_id`, any in-loop RPC error
returned `Err` and threw the live handle away.
**Resolved by:** both post-`job_id` error arms now return a DEGRADED
`isError:false` result that is a strict superset of the normal payload
(`crates/mcp/src/tools.rs:1380` CommandStatus arm, `:1447` BucketWait
arm), carrying `job_id`, `bucket_id`, `cursor`, `signals`,
`last_observed_state`, `complete:false`, `degraded:true` and a
`recover_hint`. The start-arm error still returns `Err`, as designed.

## P1 — High priority follow-ups

### P1.0a — TC-2: in-flight dedup guard (RESOLVED)

**Was:** no idempotency/dedup guard anywhere in the daemon, so a MANUAL
caller/LLM re-call of a timed-out mutating start spawned a second
identical job.
**Resolved by:** `CommandRuntime.dedup` — an `Arc<Mutex<HashMap<...>>>`
checked at the top of `start_combed` BEFORE the id mint
(`crates/daemon/src/command.rs:863-875`), keyed preferentially on the
client nonce with a short peer-scoped argv+cwd+tag fallback
(`dedup_key`, `command.rs:2471`). A duplicate returns the REAL ids of
the in-flight job (never a fake success). Registered pre-spawn
(`command.rs:1204`) so the slow-spawn window is covered, and evicted on
every completion path including the `stop()` early return
(`command.rs:1438-1442`).

### P1.0b — TC-3: command-job stop tool on the MCP surface (RESOLVED)

**Was:** a started command job could not be stopped from the MCP
surface; only PTY had `pty_command_stop`.
**Resolved by:** `command_stop` is live on the MCP surface
(`crates/mcp/src/tools.rs:1551`) over `IpcRequest::CommandStop`,
forced-kill-only, returning final bounded counters and never raw
output. The tool count moved 37->38 with the atomic count-anchor set.

### P1.0c — TC-4: anonymous runtime_state probe rows + run_and_watch tag:None (RESOLVED)

> Update 2026-10-05: RESOLVED by commit `3b7e719` (`ProbeListEntry` carries
> `tag` + a redacted `argv_head`; `collect_probes` in
> `crates/daemon/src/ipc/handlers/runtime.rs` fills both). The
> evidence and proposal below are the original, pre-fix text.

**Source:** TC trust-defects campaign, Phase 4
(`.planning/tc-bugfix-campaign/PLAN-TC4-probe-identity.md`).
**Evidence:** `collect_probes`
(`crates/daemon/src/ipc/handlers/runtime.rs:13-86`) hardcodes
`path:None` on the Command arm (~:37) and PTY arm (~:80) and
discards PTY `_argv` (~:67); `ProbeListEntry` has no tag/argv
field; `McpRunAndWatchParams::into_parts` hardcodes `tag:None`
(`crates/mcp/src/tools.rs:2277`) and the struct (`tools.rs:2219`)
lacks a tag field. NOTE: `format_argv_metadata`
(`crates/daemon/src/command.rs:989-1004`) only TRUNCATES; it
redacts nothing, so a NEW argv redactor is required before
argv_head ships.
**Impact:** Operators cannot tell which job a probe row is;
run_and_watch cannot tag its probe (a verified fake-success path).
**Proposed work:** Add additive `tag` + `argv_head` (serde default)
to `ProbeListEntry`; build a NEW argv redactor (mask values after
secret-shaped flags); lift tag in `collect_probes`; fix
`into_parts` to thread the real tag; render the columns in
`crates/cli/src/render.rs:164`.
**Scope:** `crates/ipc/src/protocol.rs`,
`crates/daemon/src/command.rs`,
`crates/daemon/src/ipc/handlers/runtime.rs`,
`crates/mcp/src/tools.rs`, `crates/cli/src/render.rs`,
`tests/fixtures/contracts/mcp-tools/runtime_state.v1.json`.

### P1.0d — TC-5: self_check is false-green (never spawns a command) (RESOLVED)

> Update 2026-10-05: RESOLVED by commit `d136e95` (`handle_self_check` is
> async and runs a real round-trip via the hidden `selfcheck-noop`
> subcommand; test `crates/daemon/tests/selfcheck_noop_subcommand.rs`).
> The text below is the original, pre-fix text.

**Source:** TC trust-defects campaign, Phase 5
(`.planning/tc-bugfix-campaign/PLAN-TC5-selfcheck-spawn.md`).
**Evidence:** `handle_self_check`
(`crates/daemon/src/ipc/server.rs:801-818`) hardcodes
`failures:0` and never spawns a command; the dispatch arm
(`server.rs:540-543`) is sync; buckets are immortal (no
drop_bucket). A live client polling self_check during a real
outage gets a false GREEN.
**Impact:** self_check lied to live clients during the TC-1/TC-6
window; it is not a real health probe.
**Proposed work:** Make `handle_self_check` async (add `.await` at
the SOLE call site `server.rs:541`); add a profile-gated bounded
real round-trip that spawns `current_exe()` as a hidden clap
SUBCOMMAND `[exe, "selfcheck-noop"]` (NOT a flag) through the normal
CommandRuntime path into ONE cached immortal bucket; skip-or-
assert-deny so a healthy daemon is NEVER false-RED; failures>0 only
on real breakage (negative test). Add a positive `selfcheck-noop`
exits-0 test.
**Scope:** `crates/daemon/src/ipc/server.rs`,
`crates/daemon/src/state.rs`, `crates/daemon/src/main.rs`,
`crates/ipc/src/protocol.rs`.

### P1.0e — TC-6: run_and_watch wait_ms cap self-violation (RESOLVED)

> Update 2026-10-05: RESOLVED by commit `cca9f06` (wall-clock `Instant`
> deadline in the `run_and_watch` wait loop, `crates/mcp/src/tools.rs`).
> The text below is the original, pre-fix text.

**Source:** TC trust-defects campaign, Phase 3
(`.planning/tc-bugfix-campaign/PLAN-TC1b-TC6-waitloop.md`).
**Evidence:** `run_and_watch` advertises a `wait_ms` cap (max
60000) but `for _ in 0..deadline_slices`
(`crates/mcp/src/tools.rs:650`) x per-slice BucketWait blocking up
to `MAX_WAIT_SLICE_MS=1000` (`tools.rs:683`) + RTTs yields ~62-70s
wall vs 60000ms advertised.
**Impact:** Dishonest timeout: the cap the tool promises is
exceeded.
**Proposed work:** Rewrite the wait loop once (shared body
`tools.rs:650-709`): wall-clock `Instant` deadline
(`Instant::now() + Duration::from_millis(wait_ms)`); per-slice
timeout `min(MAX_WAIT_SLICE_MS, remaining)`; keep
`MAX_WAIT_SLICE_MS=1000` (no load-gate RPC-doubling risk);
preserve the terminal short-circuit; final non-blocking drain on
deadline-exit. Co-implemented with TC-1b.
**Scope:** `crates/mcp/src/tools.rs`,
`tests/fixtures/contracts/mcp-tools/run_and_watch.v1.json`.

### P1.0f — adapter transport misclassifies load-induced failures (RESOLVED, commit `f61411d`)

**Source:** dogfood round 2026-07-02
(`docs/dogfood/2026-07-02-tc-0.1.70-dogfood-findings.md`, bug 5).
**Evidence:** Three live occurrences under heavy compile load against
a healthy daemon (uptime continuous, `health` succeeding between
failures): run_and_watch degraded twice with the canned "IPC error
interrupted the wait" hint, and `bucket_wait` returned
`daemon_unavailable` twice in a row. Static trace disproves any
timeout-constant mismatch on the run_and_watch path (1s slices).
**Impact:** A transient transport hiccup to a live daemon surfaces as
"daemon unavailable" / opaque degradation; agents draw the wrong
conclusion and abandon recoverable jobs.
**Proposed work:** The swallowed-error half is fixed (degraded hint now
carries the underlying code+message, commit c64652e); remaining:
retry-once-on-transient before classifying `daemon_unavailable`, and
align the IPC client connect/read deadline with the longest blocking
daemon wait it forwards (bucket_wait 30s). Reproduce under load with
the surfaced error string before choosing the constant.
**Scope:** `crates/mcp/src/daemon_client.rs`, `crates/ipc/src/client.rs`,
`crates/ipc/src/pipe_client.rs`.
**Resolution (as_of 2026-07-02, second dogfood round):** the surfaced
error string pinned the mechanism live: `pipe connect: The system
cannot find the file specified. (os error 2)` — connects landing in
the single-pending-instance accept/recreate gap, which under CPU
starvation widens to whole scheduler quanta; ERROR_FILE_NOT_FOUND was
not in the ERROR_PIPE_BUSY retry loop so it failed instantly (and the
immediate idempotent retry landed in the same gap). Fixed three ways:
(1) ERROR_FILE_NOT_FOUND joins the bounded connect-retry loop;
(2) per-request transport deadlines — bucket_wait / session-exec get
their clamped daemon-side budget + 4 s margin instead of the flat 5 s
client timeout (which also deterministically killed ANY quiet wait
over ~5 s, load or not); (3) the daemon_unavailable envelope carries
`details.transport_detail`. OPTIONAL follow-up: server-side, keep N>1
pending pipe instances to shrink the gap at the source.

### P1.0g — dogfood 2026-07-02 ergonomics batch

**Source:** dogfood round 2026-07-02
(`docs/dogfood/2026-07-02-tc-0.1.70-dogfood-findings.md`, improvements
1-10 + bug 7).
**Evidence:** Live session friction, each item reproduced: no
files-list/glob primitive (Windows + allow_shell=false = no directory
discovery at all); sub_pull silently ignores `timeout_ms` (honors
`wait_ms`); no compact projection on wait/events; sub_pull resends full
liveness[] every pull; event_context requires bucket_id though evt_ ids
are unique; pty_stdin cannot wait for the signals it provokes; no bulk
deactivate; file_write has no append; import_pack re-import mints
identical new versions instead of `skipped`; suggest_from_samples
misses `npm ERR!`/`TS\d+` shapes; wsl.exe -e bash smuggles a shell
through the argv denylist (policy stance needed).
**Impact:** Each is small; together they are the dominant tax on
agent-driven TC use.
**Proposed work:** Batch by surface: files facade (list + append),
command facade (per-action unknown-field rejection, compact on
wait/events, event_context by event_id alone), subscriptions (liveness
delta), registry (idempotent import, bulk deactivate, richer suggest
heuristics), policy (WSL stance doc or gate).
**Scope:** `crates/mcp/src/tools.rs`, `crates/daemon/src/ipc/handlers/`,
`crates/daemon/src/registry*`, docs.
**Resolution (as_of 2026-07-03):** RESOLVED by spec
`specs/002-dogfood-remediation` (branch `002-dogfood-remediation`, not yet
pushed). All eleven items shipped as nine user stories, each red->green
tested, integrated, and gated on Windows + WSL (final gate: Win 868/868,
WSL 1139/1139 nextest, security 11/11, clean fmt/clippy):
US1 facade strictness (all-missing-fields-at-once + unknown-for-action
rejection); US2 registry idempotent import + bulk/pack deactivate; US3
files-facade directory listing; US4 compact wait/events + sub_pull liveness
delta (measured 84.7% byte reduction, SC-004); US5 event_context by
event_id alone + pty_stdin bounded wait; US6 file_write append; US7 npm/TS
suggest heuristics; US8 WSL nested-shell gate (fail-closed, both argv
lanes); US9 (optional pipe-instance pool) SKIPPED with rationale
(`specs/002-dogfood-remediation/evidence-us9.md`). Correction of record:
the friction was `sub_pull` silently dropping `wait_ms` while honoring
`timeout_ms` (this entry had the two reversed); US1's strict validator now
rejects `wait_ms` on `sub_pull` and names `timeout_ms` as the remedy.
Evidence: `specs/002-dogfood-remediation/evidence-wave1.md`,
`evidence-wave2.md`, `evidence-sc004.md`.

## P1 — Pre-existing high priority follow-ups

### P1.1 — Explicit daemon-side `frames_suppressed` counter (RESOLVED)

> Update 2026-10-05: RESOLVED. `frames_suppressed`,
> `frames_suppressed_progress` and `frames_suppressed_dedupe` exist in the
> command metrics (`crates/daemon/src/command.rs`; introduced with the TC11
> noise wiring, commit `590addf`). The text below is the TC47-era text.

**Source:** TC47 final report.
**Evidence:** `crates/probes/src/process.rs`, `crates/probes/src/file.rs`,
`crates/probes/src/pty.rs` track `frames_total`, `events_emitted`,
`bytes_total`, and `secret_prompts_total` (PTY only). They do NOT
track a dedicated `frames_suppressed` counter today.
**Impact:** Tests that own both input volume AND the matching rule
can derive noise reduction from `frames_total / events_emitted`. A
real beta operator inspecting `runtime_state` or `bucket_summary`
cannot see how many frames were suppressed by sifter
dedupe/rate-limit logic versus emitted as signal.
**Proposed work:** Add a `frames_suppressed: u64` counter to each
probe's `*Metrics` struct, increment it where the sifter runtime
rejects a frame via `Dedupe` or `NoisePolicy`, and surface it in
`runtime_state` / `probe_list` / `probe_status` `ProbeListEntry`.
**Scope:** narrow product-code change touching
`crates/probes/src/*.rs` + `crates/sifters/src/*.rs` +
`crates/daemon/src/ipc/mod.rs` re-export.

### P1.2 — Codex CLI provider-harness live smoke

**Source:** TC46 final report.
**Evidence:** `codex --help` on the verification host fails with
`Error: Missing optional dependency @openai/codex-linux-x64`. The
config-only example ships in `docs/integrations/codex-cli.md` and
is correct against the documented Codex MCP schema.
**Impact:** Beta cannot be called fully provider-validated against
Codex until an operator runs `codex` end-to-end against the shipped
config and confirms `tools/list` + a `command_start_combed` -> 
`bucket_wait` -> `command_status` flow in a real session.
**Proposed work:** Operator with a working Codex CLI install runs
the smoke from `docs/integrations/codex-cli.md` and attaches the
transcript evidence to a follow-up goal.

### P1.3 — Claude Code provider-harness live smoke

**Source:** TC46 final report.
**Evidence:** `which claude` returns no result on the verification
host. The config-only examples (both `--mcp-config` and persistent
settings form) ship in `docs/integrations/claude-code.md`.
**Impact:** Same as P1.2 but for Claude Code.
**Proposed work:** Operator with a working Claude Code install
runs `claude --mcp-config <path>` or uses persistent settings,
issues `/mcp` and a tool call, captures the transcript.

### P1.4 — Cursor provider-harness live smoke

**Source:** NPM08 final report.
**Evidence:** Cursor 3.5.30 is installed on the verification host,
but Cursor has no documented non-interactive MCP discovery / tool-call
entry point — no `cursor --list-mcp-tools` subcommand, no
`cursor-agent` headless CLI on this host. Docs +
copy-pasteable configs ship at `docs/integrations/cursor.md` and
`examples/provider-harness/cursor/`.
**Impact:** Beta cannot be called fully provider-validated against
Cursor until an operator opens Cursor with one of the example
configs, confirms the tool catalogue (60 tools at 2026-10-05) in `Settings -> MCP`, and
captures a real tool-call transcript or screenshot.
**Proposed work:** Operator copies one of
`examples/provider-harness/cursor/mcp.global.native-linux.json`,
`mcp.project.linux-wsl.json`, or `mcp.global.linux-wsl.json` into
their Cursor MCP config path; starts the daemon; asks Cursor chat
to call `health` and `command_start_combed` -> `bucket_wait` ->
`command_status`; captures evidence.

### P1.5 — First live npm publish (RESOLVED)

**Source:** NPM07 + NPM09 final reports + NPM10 policy exception.
**Was:** all three package names returned `E404` from `npm view` on
2026-05-23, and npmjs.com could not offer the trusted-publisher UI for
a package page that did not yet exist.
**Resolved (first publish only).** Verified against the live registry on
2026-08-07: `terminal-commander` resolves with `dist-tags.latest = 0.1.86`,
and the platform packages `@terminal-commander/{linux-x64,linux-arm64,
windows-x64,mac-x64,mac-arm64}` all resolve (HTTP 200). Release PR #163
merged 2026-07-17 and its `release-please` run published in 16m29s.

**Correction (2026-08-07, same day).** An earlier revision of this entry
claimed "the OIDC path is exercised, not merely configured". That was
WRONG and is retracted. `release-please.yml` publishes with
`NODE_AUTH_TOKEN: secrets.NPM_TOKEN_TC` at every publish job
(`:534`, `:663`, `:748`, `:833`, `:918`, `:1009`), and its own header says
"trusted-publisher OIDC not yet configured on npmjs.com for all package
names". #163 therefore published by TOKEN. Trusted publishing remains
unconfigured and Task 23 still owns the cutover.

> Update 2026-10-05: the Task 23 cutover LANDED (commit `23a407a`,
> 2026-08-07). `release-please.yml` now publishes npm packages with OIDC
> trusted publishing and `npm publish --provenance`, requests `id-token:
> write` on the publish jobs, and has no `NODE_AUTH_TOKEN` on them (its
> header says `NPM_TOKEN_TC` was removed at Task 23; crates.io still uses
> `CARGO_REGISTRY_TOKEN_TC`). The paragraph above describes the state on
> 2026-08-07 before that cutover. Current version is 0.3.11.

Publishing is a normal two-gate flow, NOT automatic on a feature merge:
merging a Conventional-Commits `feat:`/`fix:` PR to `main` makes
release-please open/update a release PR; merging THAT release PR bumps the
version and fires the publish jobs.

### P1.5b — Disable the NPM10 bootstrap workflow + rotate NPM_TOKEN_TC

**Source:** NPM10 goal file +
`docs/release/npm-bootstrap-first-publish.md` §5.3.
**Evidence:** `.github/workflows/npm-bootstrap-publish.yml` is the
ONE-TIME `NPM_TOKEN_TC` path; the intended standing capability is OIDC
trusted publishing via `release-please.yml`.
**Status (2026-08-07): BLOCKED, not actionable — precondition unmet.**
An earlier revision of this entry called it "ACTIONABLE, and now overdue"
on the strength of the first publish having landed. That was wrong, and is
retracted: step 2 (rotate `NPM_TOKEN_TC`) would BREAK releases outright,
because `release-please.yml` — the standing publish path — still
authenticates with that same token on every job. Rotation is safe only
AFTER the Task 23 OIDC cutover, which is the real precondition.

What is still true, at lower severity than previously recorded: the
bootstrap workflow remains `workflow_dispatch`-able and is a redundant
second manual publish route. It does NOT expose a new credential class,
since the standing workflow uses the same token.
> Update 2026-10-05: the stated precondition (Task 23 OIDC cutover) is now
> MET (commit `23a407a`; `release-please.yml` no longer uses `NPM_TOKEN_TC`).
> `.github/workflows/npm-bootstrap-publish.yml` is still present and still
> reads `secrets.NPM_TOKEN_TC`; so does `.github/workflows/deprecate-version.yml`
> (its `npm deprecate` step). Owner-reported facts, 2026-10-05 (not
> checkable from the repo): `NPM_TOKEN_TC` was replaced that day with a
> granular, stage-only token scoped to `terminal-commander` and the
> `@terminal-commander` scope (it can deprecate versions but cannot publish
> directly), publishing uses OIDC, and the weekly secret-health probe passes
> again. So step 2 (rotate) is DONE; step 1 (disable or delete the
> bootstrap workflow) is STILL OPEN, as are steps 3 and 4.

**Proposed work (post-Task-23 OIDC cutover, in this order):**
1. Delete `.github/workflows/npm-bootstrap-publish.yml` OR rename
   it to `.disabled` so GitHub Actions stops indexing it.
2. Rotate / invalidate `NPM_TOKEN_TC` on npmjs.com.
3. Update `docs/release/` to record that `NPM_TOKEN_TC` is
   decommissioned and OIDC trusted publishing is the only
   publish path.
4. Open a follow-up goal (NPM11) to make the change auditable in
   one commit pair.

## P2 — Medium priority

### P2.1 — Dedicated file-watch load test

**Source:** TC47 final report.
**Evidence:** TC47 covers file-watch in steady-state via TC43
tests; under sustained megabyte/s append rate the file-watch path
is dominated by the 120 ms polling backend (`crates/probes/src/file.rs`),
NOT Terminal Commander's signal pipeline.
**Impact:** A dedicated load test would primarily measure the
polling boundary. Useful only after the polling backend is replaced
with native notify/inotify (currently out of scope per the TC43
prep amendment; update 2026-10-05: a `notify` backend has since shipped in
omni P3, with the poll fallback kept for WSL `/mnt/c`).
**Proposed work:** Either (a) accept the polling boundary and skip
the dedicated test, or (b) land native notify/inotify under a new
goal, then add the load test.

### P2.2 — Dedicated PTY load test

**Source:** TC47 final report.
**Evidence:** TC44 already exercises ANSI/CR normalization and
secret-prompt detection under `pty_ipc.rs` and
`pty_tools_live_e2e.rs`. The sifter pipeline downstream of the
PTY merged stream is identical to the process probe pipeline that
TC47 already stresses at ~1 MB.
**Impact:** A dedicated PTY load test would primarily measure WSL
`pty-process` throughput, not Terminal Commander's bounded-output
contract.
**Proposed work:** Optional — accept the existing coverage.

### P2.3 — Wire the `system_discover` payload to include the TC45 +
TC47 stress evidence summary

**Source:** TC48 review.
**Evidence:** `system_discover` advertises adapter_version, MCP
spec, and the live tool catalogue. It does not summarize stress
gate status or load-evidence ids. Operators currently learn the
beta posture only from `EVIDENCE_REPORT_RUNTIME.md`.
**Proposed work:** Add a `beta_evidence_ref: "<git sha>"` field
to the `system_discover` payload pointing at the verified beta
commit. Narrow protocol addition; covered by an existing TC45-style
read-only addition pattern.

## P3 — Low priority / opportunistic

### P3.1 — `bash scripts/smoke/verify-runtime-smoke.sh` Windows-host wrapper

The smoke script requires WSL2 today. A thin PowerShell wrapper
would let Windows operators run the smoke without manual `wsl -e`
invocation. Not a beta blocker; convenience only.

### P3.2 — `verify-load-gate.sh` shell harness

The TC47 prep amendment marks this as optional; pure Rust tests
were sufficient. Re-evaluate after `frames_suppressed` lands —
shell-driven repeatability might earn its keep.

### P3.3 — Provider config templates for additional MCP clients

Today (2026-10-05): Codex CLI, Claude Code, Cursor, Gemini, Kimi and OMP
have pages under `docs/integrations/`. Adding templates for further
MCP-capable clients (Continue, Cline, etc.) is opportunistic.

## DEFERRED — TC trust-defects campaign (tracked, not dropped)

Items the TC trust-defects campaign explicitly DEFERRED. The
campaign closes the realistic double-spawn windows (P0.1 + P1.0a)
without these; each is tracked here with file:line evidence so it
is not lost.

| ID | Item | Why deferred | Evidence (file:line) |
|----|------|--------------|----------------------|
| TCD-1 | Server-honored idempotency key on `RequestEnvelope` (F1-b) | With the blind retry removed (P0.1) and the in-flight dedup landed (P1.0a), both the automatic and realistic manual double-spawn windows close without a wire/protocol change. The full key protocol has unresolved design (client-id vs argv/cwd/window fingerprint, TTL, persistence). Tracked in RISK_REGISTER R-07. | `crates/ipc/src/protocol.rs` (RequestEnvelope has no key); `crates/daemon/src/command.rs:572-581` (start_combed) |
| TCD-2 | Pre-spawn async ack handshake (F1-c) | Rewrites the spawn-failure contract (`command.rs:548-560` must flip an already-acked Starting job to Failed) and exposes a new Starting liveness at the wire boundary; research confirms it STILL double-spawns under blind retry unless paired with a dedup guard, so it is redundant once P0.1 + P1.0a exist. Highest-blast-radius machinery. Tracked in R-07. | `crates/daemon/src/command.rs:548-560` (spawn-failure early return) |
| TCD-3 | `command_stop` graceful grace window (F4) (update 2026-10-05: omni P3 TC58 added a SIGTERM->SIGKILL terminate ladder shared by command/PTY/session stop per ROADMAP.md; not re-verified for `command_stop` here) | `ProcessProbeConfig.grace` exists but is "advisory; cancellation in MVP is forced kill only"; wiring a SIGTERM-then-SIGKILL window is net-new probe work, not tool exposure. `command_stop` ships forced-kill-only (parity with `PtyRuntime::stop`). | `crates/probes/src/process.rs:47` (grace advisory) |
| TCD-4 | Change the numeric JSON-RPC code of `transport_unavailable_error` away from -32603 | Phase 1 fixes the BEHAVIOR (no retry of mutating ops) and the misleading remedy text; whether to also change the wire numeric code for clients keying off -32603 is a separate decision with client-compat implications. Tracked in R-07. | `crates/mcp/src/tools.rs:1808` (`McpError::internal_error`) |
| TCD-5 | Retrofit policy-gating onto the ungated `pty_command_stop` | Out of TC-1..TC-6 scope. `command_stop` is gated correctly via the dormant `CommandSignal`; PTY symmetry is a separate conformance item. | `crates/daemon/src/pty_command.rs:526-571` (`PtyRuntime::stop` ungated) |
| TCD-6 | A real `drop_bucket` seam (`BucketManager` + `BucketSourceTable.remove`) | TC-5 reuses ONE cached immortal self-check bucket instead, honoring the existing immortal-bucket invariant. A reclamation seam is larger blast radius and not required. | `crates/.../source.rs:12-17` (immortal-bucket invariant; no remove) |
| TCD-7 | Full stale-doc tool-count sweep (29 in RELEASE_CHECKLIST/BACKLOG, 31 in README, 32 in TOOL_CONTROL_SURFACE) | Only the CI-gated assertions + the normative TOOL_CONTROL_SURFACE table + the lines `command_stop` must touch are reconciled (37->38). A full sweep of non-gated stale references (incl. the dead CONTRIBUTING branch/goal-file doctrine and SPEC internal Tier-1 drift) is a separate chore. | RELEASE_CHECKLIST.md:61,71,312; BACKLOG.md:78; README.md:201,292; docs/mcp/TOOL_CONTROL_SURFACE.md:61; CONTRIBUTING.md:12-26 |
| TCD-8 | Evict terminal jobs from `CommandRuntime.live` (unbounded growth) | Found while implementing spec 004. No `remove` on that map exists, so a binding is inserted per combed job and NEVER dropped; `ipc/handlers/runtime.rs:20` independently documents "bindings linger after exit". A long-lived daemon therefore grows one binding per command forever (each holding a sifter `Arc`, inline rules, and an argv head). Deliberately NOT fixed in spec 004: the lingering binding is currently LOAD-BEARING -- `status()` reads the terminal metrics aggregate from it, and `collect_probes` reports terminal state from it, so eviction needs a replacement source for both before it is safe. Slow leak, not a correctness defect. | `crates/daemon/src/command.rs` (no `live.write().remove`); `crates/daemon/src/ipc/handlers/runtime.rs:20` (bindings linger after exit) |

## Resolved P0 (historical context)

| P0 ID                   | Resolved by | Notes |
|-------------------------|-------------|-------|
| persistent audit writes | TC35        | `PersistentAudit` is the production audit path; the IPC server writes one audit row per accepted request. |
| local UDS IPC           | TC37        | `IpcServer` binds `<data_dir>/terminal-commanderd.sock`; PeerCred records uid/gid/pid on connect; no network listener. |
| rmcp stdio adapter      | TC40        | `terminal-commander-mcp` serves an rmcp 1.7.0 stdio adapter (now `rmcp =3.4.1`, workspace `Cargo.toml`) that forwards every tool call through the daemon UDS. |
| PTY spawn               | TC44        | `pty-process = "=0.5.3"` drives the POSIX PTY spawn; secret-prompt boundary enforced via `IpcErrorCode::SecretInputDenied`. |
| MCP command + bucket    | TC41        | `command_start_combed`, `bucket_events_since`, `bucket_wait`, `bucket_summary`, `command_status` all live through MCP. |
| File read/search/watch  | TC43        | `file_read_window`, `file_search`, `file_watch_start/stop/list` all live and bounded. |
| Dynamic rule activation | TC42 / TC42b / TC42c / TC42d | Persisted activation registry, scoped binding (Global/Bucket/Job/Probe), live rebind for running jobs, explicit-scope requirement. |
| Aggregate runtime view  | TC45        | `runtime_state`, `probe_list`, `probe_status` aggregate read-only across the three runtimes. |
| Local smoke harness     | TC46        | `scripts/smoke/verify-runtime-smoke.sh` proves the daemon + MCP stdio path end-to-end. |
| Load / noise / backpressure gate | TC47 | 8 stress tests covering megabyte-scale stdout, bucket caps, drop counters, cross-talk isolation, mid-stream rebind. |
| Windows native IPC (Phases 0-3) | feature/native-tier1-phases-0-3 | Named-pipe ACL (SDDL), peer-SID resolution via Win32 FFI, pipe server accept loop; all 258 tests pass; clippy -D warnings clean. |

Resolved items remain listed so reviewers can map current code to
the P0 backlog that drove the chain. Move new items into P1/P2/P3
only after the work is shown live in the daemon + MCP surface and
matched by tests.

## P2 — Windows + WSL bridge follow-ups (WWS08 docs-only)

Added by WWS08 to record known gaps from the WWS01–WWS07 chain.
None of these are publish-blockers (the publish floor recommended
by WWS01 §14.1 was WWS02 + WWS04 + WWS05 + WWS06 + WWS08, all
landed); they are post-publish enhancements.

| ID    | Item | Reason |
|-------|------|--------|
| WWS-B1 | First live npm publish | **DONE 2026-07-17.** `terminal-commander@0.1.86` and all five `@terminal-commander/*` platform packages resolve on the live registry (verified 2026-08-07). See P1.5. |
| WWS-B2 | Windows → WSL MCP bridge round-trip live evidence | WWS07 PowerShell smoke records `runtime_missing` honestly. Re-run after WWS-B1 to capture an MCP `initialize` + `tools/list` + `tools/call(health)` transcript through the WWS04 bridge. |
| WWS-B3 | Cursor provider GUI live smoke transcript | No headless Cursor MCP discovery entry point. Operator opens Cursor → confirms `terminal-commander` in Settings → asks for `health` from chat → attaches transcript. Required before beta posture can promote `Conditional Go` → `Go`. |
| WWS-B4 | `terminal-commander setup cursor-wsl --uninstall` | D-14 rollback (partial at WWS06). The WWS05 writer already produces `<mcp.json>.bak`; the uninstall flow restores it. NOT implemented at WWS06. |
| WWS-B5 | Multi-distro interactive ask-once prompt | D-07 future enhancement. At WWS06 operators must pass `--distro <name>` or set `TC_WSL_DISTRO` when no default distro is available; the CLI emits `no_default_distro_ambiguous` with the candidate list. A future `--interactive` flag may add a prompt. |
| WWS-B6 | Full WSL-side `pair accept` handshake | At WWS06 `pair create` persists `pair.json`; `pair accept` validates the 6-digit shape + persisted-code match → `pair_accepted` or `pair_deferred`. The WSL-side daemon session token exchange is deferred. |
| WWS-B7 | Credential broker for `--install-wsl-runtime` permission failures (update 2026-10-05: a separate owner-only credential elicitation path now exists, MCP `credential_request` plus `crates/daemon/tests/credential_ipc.rs`; whether it covers the WSL install probe was not verified) | At WWS06 the install probe returns `install_permission_required` honestly when the inside-WSL npm install hits EACCES; Terminal Commander does NOT prompt for passwords or run sudo. Future work may add a safe broker that does NOT forward LLM-supplied credentials through MCP / chat / bucket / log / audit / env / Cursor config. |
| WWS-B8 | `npm-bootstrap-publish.yml` disable / rotate after first publish | Inherited from NPM10 (BACKLOG P1.5b). The workflow exists but stays committed-but-undispatched. |
| WWS-B9 | CAP01 capability-registry contract (future doctrine) | Recorded as doctrine carry-forward through the WWS chain. The registry would formalize the "tentacle = programmable probe = policy-gated capability executor" model. NOT started; NOT scheduled. |
