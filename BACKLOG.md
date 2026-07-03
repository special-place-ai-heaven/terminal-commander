# Terminal Commander - Backlog

Status: TC48 beta gate snapshot.

Backlog tracks open work after the TC33-TC47 runtime chain landed.
The four historical P0 blockers (rmcp stdio adapter, PTY spawn, UDS
IPC, persistent audit writes) are now resolved on `main` and listed
in the "Resolved P0" section for traceability. Active work below is
prioritized against the current evidence.

Language: ASCII only.

## Omni completion program (as_of 2026-06-16; on review branches, NOT merged)

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

## P0 — Beta blockers (active)

The four original P0 items are resolved (see "Resolved P0" below).
Two new P0 trust defects were found by the TC trust-defects campaign
(`.planning/tc-bugfix-campaign/`); both violate the "run a command
safely" product promise at HEAD.

### P0.1 — TC-1a: client-timeout blind retry double-spawns a command

**Source:** TC trust-defects campaign, Phase 1
(`.planning/tc-bugfix-campaign/PLAN-TC1-ghost-spawn.md`).
**Evidence:** `crates/mcp/src/daemon_client.rs:191-198` re-sends a
cloned request on any transport error
(`Err(e) if e.is_transport()`), with no idempotency check. A >5s
client timeout on a `CommandStartCombed` therefore re-sends a
mutating start while the first spawn may already be running =>
DOUBLE SPAWN. The mid-call envelope compounds it:
`crates/mcp/src/tools.rs:1804-1805` remedy literally says
"retry the tool", and `tools.rs:1808` routes through
`McpError::internal_error` (numeric -32603) while the doc comment
at `tools.rs:1790-1796` falsely claims "never a raw internal_error
(-32603)".
**Impact:** A timed-out start can silently run twice; the agent is
told to retry the exact mutating op. This is a regression at HEAD
and the highest-harm item in the campaign.
**Proposed work:** Add `pub const fn is_idempotent(&self)` to
`IpcRequest` (`crates/ipc/src/protocol.rs`), gate the retry on
`request.is_idempotent()`, split self-heal from re-send, and fix
the lying doc comment + the "retry the tool" remedy (make it
operation-neutral). Pure logic; zero daemon state.
**Scope:** `crates/ipc/src/protocol.rs`,
`crates/mcp/src/daemon_client.rs`, `crates/mcp/src/tools.rs`.

### P0.2 — TC-1b: run_and_watch discards a live job handle on mid-wait IPC error

**Source:** TC trust-defects campaign, Phase 3
(`.planning/tc-bugfix-campaign/PLAN-TC1b-TC6-waitloop.md`).
**Evidence:** Once `run_and_watch` holds a job_id (the start RPC at
`crates/mcp/src/tools.rs:630-643` succeeded), any subsequent
in-loop RPC error returns `Err(into_mcp_error)` and discards the
known job_id/bucket_id/cursor/signals: the CommandStatus arm at
`tools.rs:665` and the BucketWait arm at `tools.rs:703`.
**Impact:** The agent is told "error" with no way to recover a
still-running job; the live job_id is thrown away (trust-destroying
discard).
**Proposed work:** Convert both post-job_id error arms to a
DEGRADED isError:false result that is a strict superset of the
existing payload (`{job_id, bucket_id, state (last-observed/UNKNOWN,
never silently Running), cursor, signals, complete:false,
wait_exhausted:true, degraded:true, recover_hint}`); the start-arm
error still returns Err. Co-implemented with TC-6 in one wait-loop
rewrite (shared body `tools.rs:650-709`).
**Scope:** `crates/mcp/src/tools.rs`,
`tests/fixtures/contracts/mcp-tools/run_and_watch.v1.json`.

## P1 — High priority follow-ups

### P1.0a — TC-2: no in-flight dedup guard (manual-retry double-spawn)

**Source:** TC trust-defects campaign, Phase 2
(`.planning/tc-bugfix-campaign/PLAN-TC2-dedup.md`).
**Evidence:** No idempotency/dedup guard exists anywhere in the
daemon: `crates/daemon/src/command.rs` `start_combed` (builds from
`CommandStartRequest` at `command.rs:572-581`) has no pre-spawn
lookup, and `RequestEnvelope` has no key. After P0.1 removes the
AUTOMATIC double-spawn, the residual path is a MANUAL caller/LLM
re-call of a timed-out mutating start.
**Impact:** A deliberate re-call still spawns a second identical
job; trust defense-in-depth is missing.
**Proposed work:** Add a NEW `Arc<Mutex<HashMap<u64,(JobId,BucketId,
Instant)>>>` field on `CommandRuntime` (NOT behind the live lock),
checked at the top of `start_combed`, keyed preferentially on a
client nonce (`dedup_nonce`, adapter always generates one) with a
short peer-scoped argv+cwd+tag fallback; thread `dedup_nonce`
through `CommandStartParams` -> `CommandStartRequest` ->
`handle_command_start_combed` (`crates/daemon/src/ipc/handlers/
command.rs`) or it is silently dropped; evict on EVERY completion
path (exit/cancel/spawn-failure). Never collapses two
legitimately-distinct rapid runs.
**Scope:** `crates/daemon/src/command.rs`,
`crates/ipc/src/protocol.rs`,
`crates/daemon/src/ipc/handlers/command.rs`,
`crates/mcp/src/tools.rs`.

### P1.0b — TC-3: no command-job stop tool on the MCP surface

**Source:** TC trust-defects campaign, Phase 6a/6b
(`.planning/tc-bugfix-campaign/PLAN-TC3-command-stop.md`).
**Evidence:** A started command job cannot be stopped from the MCP
surface; only PTY has `pty_command_stop`. `CommandRuntime` has no
stop/cancel/kill; `JobBinding` (`crates/daemon/src/command.rs:218-230`)
retains no cancel handle and derives `Clone`; no `CommandStop` IPC
method; `PolicyAction::CommandSignal` is dormant
(`crates/daemon/src/policy.rs:62`); the command waiter
(`command.rs:668-680`) has NO terminal-state guard (contrast PTY
`crates/daemon/src/pty_command.rs:391-400`).
**Impact:** Advertised command-lifecycle control is incomplete; a
long-running command job is unstoppable through the tool surface.
**Proposed work:** Add `command_stop` / `IpcRequest::CommandStop`
(forced-kill-only). Phase 6a: `take_cancel_handle()`
(`crates/probes/src/process.rs`), a `cancel` field on `JobBinding`
(remove the `Clone` derive), a command-waiter terminal-state guard,
`CommandRuntime::stop` (policy-deny FIRST with peer-subject audit;
check-then-set Cancelled), the IPC method + 3 re-exports + dispatch.
Phase 6b: the MCP tool + ALL atomic count anchors (5 name lists + 3
count assertions + fixture map + system_discover fixture +
minimal_tool_args) bumped 37->38 + docs.
**Scope:** ~16 files across 6a/6b (EXEMPT from the <=10-file rule;
atomic count-anchor set).

### P1.0c — TC-4: anonymous runtime_state probe rows + run_and_watch tag:None

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

### P1.0d — TC-5: self_check is false-green (never spawns a command)

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

### P1.0e — TC-6: run_and_watch wait_ms cap self-violation

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

### P1.0f — adapter transport misclassifies load-induced failures

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

### P1.1 — Explicit daemon-side `frames_suppressed` counter

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
`crates/daemon/src/ipc/protocol.rs` re-export.

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
configs, confirms the 29-tool catalogue in `Settings -> MCP`, and
captures a real tool-call transcript or screenshot.
**Proposed work:** Operator copies one of
`examples/provider-harness/cursor/mcp.global.native-linux.json`,
`mcp.project.linux-wsl.json`, or `mcp.global.linux-wsl.json` into
their Cursor MCP config path; starts the daemon; asks Cursor chat
to call `health` and `command_start_combed` -> `bucket_wait` ->
`command_status`; captures evidence.

### P1.5 — First live npm publish (operator preconditions)

**Source:** NPM07 + NPM09 final reports + NPM10 policy exception.
**Evidence:** `.github/workflows/release-please.yml` carries an
output-gated, OIDC-only publish path. All three package names
return `E404` from `npm view` on 2026-05-23 — unpublished /
available. Live publish jobs were correctly `skipped` on the
NPM06 / NPM07 / NPM08 / NPM08b / NPM09 live runs because
`releases_created='false'` (no Conventional-Commits-eligible
commits since the manifest seed).
**Impact:** Until the FIRST publish lands, npmjs.com cannot offer
the trusted-publisher configuration UI on a package page that does
not yet exist. NPM10 added a one-time bootstrap workflow
(`.github/workflows/npm-bootstrap-publish.yml`,
`workflow_dispatch` only, two-gate confirm, NPM_TOKEN_TC) so the
first publish can land via token; every subsequent publish goes
through NPM07's OIDC path.
**Proposed work (post-NPM10):**
1. Operator dispatches `npm-bootstrap-publish` in dry-run mode and
   verifies the three platform / root publish jobs succeed.
2. Operator dispatches `npm-bootstrap-publish` with
   `dry_run = false` AND
   `confirm_publish = "publish-terminal-commander-beta"`.
3. After the real publish, operator configures trusted publisher
   for each of the three package pages on npmjs.com with workflow
   filename `release-please.yml`.
4. Operator lands a Conventional-Commits `feat:` / `fix:` commit;
   release-please opens a release PR; operator reviews + merges
   it. The OIDC publish jobs fire on the merge push.

After step 3 + step 4 succeed, P1.5b (below) fires.

### P1.5b — Disable the NPM10 bootstrap workflow + rotate NPM_TOKEN_TC

**Source:** NPM10 goal file +
`docs/release/npm-bootstrap-first-publish.md` §5.3.
**Evidence:** `.github/workflows/npm-bootstrap-publish.yml` is the
ONE-TIME `NPM_TOKEN_TC` path; standing capability is OIDC trusted
publishing via `release-please.yml`. The bootstrap workflow must
not remain dispatchable after the first publish succeeds, otherwise
an accidental dispatch could publish a token-authorized version
that bypasses the OIDC + provenance contract.
**Proposed work (post-first-publish, post-trusted-publisher-config):**
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
prep amendment).
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

Today: Codex CLI + Claude Code. Adding templates for additional
MCP-capable clients (Continue, Cursor, Cline, etc.) is opportunistic.

## DEFERRED — TC trust-defects campaign (tracked, not dropped)

Items the TC trust-defects campaign explicitly DEFERRED. The
campaign closes the realistic double-spawn windows (P0.1 + P1.0a)
without these; each is tracked here with file:line evidence so it
is not lost.

| ID | Item | Why deferred | Evidence (file:line) |
|----|------|--------------|----------------------|
| TCD-1 | Server-honored idempotency key on `RequestEnvelope` (F1-b) | With the blind retry removed (P0.1) and the in-flight dedup landed (P1.0a), both the automatic and realistic manual double-spawn windows close without a wire/protocol change. The full key protocol has unresolved design (client-id vs argv/cwd/window fingerprint, TTL, persistence). Tracked in RISK_REGISTER R-07. | `crates/ipc/src/protocol.rs` (RequestEnvelope has no key); `crates/daemon/src/command.rs:572-581` (start_combed) |
| TCD-2 | Pre-spawn async ack handshake (F1-c) | Rewrites the spawn-failure contract (`command.rs:548-560` must flip an already-acked Starting job to Failed) and exposes a new Starting liveness at the wire boundary; research confirms it STILL double-spawns under blind retry unless paired with a dedup guard, so it is redundant once P0.1 + P1.0a exist. Highest-blast-radius machinery. Tracked in R-07. | `crates/daemon/src/command.rs:548-560` (spawn-failure early return) |
| TCD-3 | `command_stop` graceful grace window (F4) | `ProcessProbeConfig.grace` exists but is "advisory; cancellation in MVP is forced kill only"; wiring a SIGTERM-then-SIGKILL window is net-new probe work, not tool exposure. `command_stop` ships forced-kill-only (parity with `PtyRuntime::stop`). | `crates/probes/src/process.rs:47` (grace advisory) |
| TCD-4 | Change the numeric JSON-RPC code of `transport_unavailable_error` away from -32603 | Phase 1 fixes the BEHAVIOR (no retry of mutating ops) and the misleading remedy text; whether to also change the wire numeric code for clients keying off -32603 is a separate decision with client-compat implications. Tracked in R-07. | `crates/mcp/src/tools.rs:1808` (`McpError::internal_error`) |
| TCD-5 | Retrofit policy-gating onto the ungated `pty_command_stop` | Out of TC-1..TC-6 scope. `command_stop` is gated correctly via the dormant `CommandSignal`; PTY symmetry is a separate conformance item. | `crates/daemon/src/pty_command.rs:526-571` (`PtyRuntime::stop` ungated) |
| TCD-6 | A real `drop_bucket` seam (`BucketManager` + `BucketSourceTable.remove`) | TC-5 reuses ONE cached immortal self-check bucket instead, honoring the existing immortal-bucket invariant. A reclamation seam is larger blast radius and not required. | `crates/.../source.rs:12-17` (immortal-bucket invariant; no remove) |
| TCD-7 | Full stale-doc tool-count sweep (29 in RELEASE_CHECKLIST/BACKLOG, 31 in README, 32 in TOOL_CONTROL_SURFACE) | Only the CI-gated assertions + the normative TOOL_CONTROL_SURFACE table + the lines `command_stop` must touch are reconciled (37->38). A full sweep of non-gated stale references (incl. the dead CONTRIBUTING branch/goal-file doctrine and SPEC internal Tier-1 drift) is a separate chore. | RELEASE_CHECKLIST.md:61,71,312; BACKLOG.md:78; README.md:201,292; docs/mcp/TOOL_CONTROL_SURFACE.md:61; CONTRIBUTING.md:12-26 |

## Resolved P0 (historical context)

| P0 ID                   | Resolved by | Notes |
|-------------------------|-------------|-------|
| persistent audit writes | TC35        | `PersistentAudit` is the production audit path; the IPC server writes one audit row per accepted request. |
| local UDS IPC           | TC37        | `IpcServer` binds `<data_dir>/terminal-commanderd.sock`; PeerCred records uid/gid/pid on connect; no network listener. |
| rmcp stdio adapter      | TC40        | `terminal-commander-mcp` serves an rmcp 1.7.0 stdio adapter that forwards every tool call through the daemon UDS. |
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
| WWS-B1 | First live npm publish | Pending operator npmjs.com trusted-publisher setup + release PR merge (`docs/release/npm-trusted-publishing-contract.md` §8). Until then `terminal-commander setup cursor-wsl --install-wsl-runtime` returns `npm_package_unpublished` honestly. |
| WWS-B2 | Windows → WSL MCP bridge round-trip live evidence | WWS07 PowerShell smoke records `runtime_missing` honestly. Re-run after WWS-B1 to capture an MCP `initialize` + `tools/list` + `tools/call(health)` transcript through the WWS04 bridge. |
| WWS-B3 | Cursor provider GUI live smoke transcript | No headless Cursor MCP discovery entry point. Operator opens Cursor → confirms `terminal-commander` in Settings → asks for `health` from chat → attaches transcript. Required before beta posture can promote `Conditional Go` → `Go`. |
| WWS-B4 | `terminal-commander setup cursor-wsl --uninstall` | D-14 rollback (partial at WWS06). The WWS05 writer already produces `<mcp.json>.bak`; the uninstall flow restores it. NOT implemented at WWS06. |
| WWS-B5 | Multi-distro interactive ask-once prompt | D-07 future enhancement. At WWS06 operators must pass `--distro <name>` or set `TC_WSL_DISTRO` when no default distro is available; the CLI emits `no_default_distro_ambiguous` with the candidate list. A future `--interactive` flag may add a prompt. |
| WWS-B6 | Full WSL-side `pair accept` handshake | At WWS06 `pair create` persists `pair.json`; `pair accept` validates the 6-digit shape + persisted-code match → `pair_accepted` or `pair_deferred`. The WSL-side daemon session token exchange is deferred. |
| WWS-B7 | Credential broker for `--install-wsl-runtime` permission failures | At WWS06 the install probe returns `install_permission_required` honestly when the inside-WSL npm install hits EACCES; Terminal Commander does NOT prompt for passwords or run sudo. Future work may add a safe broker that does NOT forward LLM-supplied credentials through MCP / chat / bucket / log / audit / env / Cursor config. |
| WWS-B8 | `npm-bootstrap-publish.yml` disable / rotate after first publish | Inherited from NPM10 (BACKLOG P1.5b). The workflow exists but stays committed-but-undispatched. |
| WWS-B9 | CAP01 capability-registry contract (future doctrine) | Recorded as doctrine carry-forward through the WWS chain. The registry would formalize the "tentacle = programmable probe = policy-gated capability executor" model. NOT started; NOT scheduled. |
