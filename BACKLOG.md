# Terminal Commander - Backlog

Status: TC48 beta gate snapshot.

Backlog tracks open work after the TC33-TC47 runtime chain landed.
The four historical P0 blockers (rmcp stdio adapter, PTY spawn, UDS
IPC, persistent audit writes) are now resolved on `main` and listed
in the "Resolved P0" section for traceability. Active work below is
prioritized against the current evidence.

Language: ASCII only.

## P0 — Beta blockers (active)

None. The four original P0 items are resolved (see "Resolved P0"
below). The remaining open items are P1/P2/P3.

## P1 — High priority follow-ups

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

Resolved items remain listed so reviewers can map current code to
the P0 backlog that drove the chain. Move new items into P1/P2/P3
only after the work is shown live in the daemon + MCP surface and
matched by tests.
