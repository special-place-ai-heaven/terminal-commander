# Phase 1 Contracts: New / Changed MCP Tool Surfaces

Each new tool gets a fixture under `tests/fixtures/contracts/mcp-tools/<tool>.v1.json`
and at least one through-the-daemon integration test (constitution VI). Request
shapes are the contract; responses are bounded combed signals + receipt. Tool
count moves 39 -> ~46-53 across the program; every add updates all count anchors
in the same change.

## P1 -- Sessions and workspace

### shell_session_start
- Request: `{ shell?: string, cwd?: string, env?: {k:v}, rules?: [..] }`
- Response: `{ session_id, bucket_id, state }`
- Gate: `PolicyAction::SessionStart`, cap `allow_session` (default deny), audited.

### shell_session_exec
- Request: `{ session_id, line: string (<= max bytes), wait_ms?, compact? }`
- Response: combed signals in the session bucket + `{ cursor, complete, wait_exhausted }`.

### shell_session_status
- Request: `{ session_id }`
- Response: `{ state, cwd, env_snapshot (bounded), last_active_at }`

### shell_session_stop
- Request: `{ session_id }`
- Response: `{ state: Exited, terminal_reason }` (graceful then forced).

### shell_session_list
- Request: `{}`
- Response: `{ sessions: [{ session_id, state, cwd, last_active_at }] }`

### workspace_snapshot_create
- Request: `{ session_id, name? }`  Response: `{ snapshot_id }`

### workspace_snapshot_apply
- Request: `{ snapshot_id, session_id }`  Response: `{ applied: true, cwd }`

### Changed (P1 cross-cutting)
- All signal-returning tools gain `compact?: bool` (default false) -> TC-E1
  projection `{summary,stream,seq,severity}`.
- Wait-capable tools gain `wait_until?: "exit"` + server-cap; running responses
  add `poll_hint_ms` -> TC-E2.
- `command_start_combed` / `run_and_watch` gain `strip_ansi?: bool` (default
  true) -> TC-B1.
- `command_status` returns a restart-marked terminal result from a persisted
  receipt when the in-memory job is gone -> TC-B3.

## P2 -- Parse omni

### registry_suggest_from_samples
- Request: `{ samples: [string], intent?: string, max_rules?: number }`
- Response: `{ proposed_rules: [..], confidence: "heuristic", next_steps:
  ["registry_test","registry_upsert","registry_activate"] }`
- Invariant: NEVER activates (FR-008).

### Changed
- Command-start responses gain an optional `hint: { kind:"pack_available",
  pack, action:"registry_import_pack" }` when a known tool runs without its pack.
- Config `sifters.universal_extractors=true` enables always-on low-severity
  extractors (no new tool; behavior change).
- Built-in pack set grows to >=25 (data under `crates/store/rules/`).

## P3 -- Platform parity (no new tools; availability + behavior)

- `pty_command_*` reported `available:true` on Windows (ConPTY) and macOS.
- `system_discover` PTY/platform fields updated.
- File-watch backend event-driven on native FS; tool surface unchanged.
- `command_stop` / `pty_command_stop` / `shell_session_stop` share a
  graceful-then-forced terminate contract.

## P4 -- Privileged helper (CONTRACT ONLY; no code this run)

### privileged_exec
- Request: `{ op: allow-list-member, params: {..}, approval_token?: string }`
- Response: `{ state: "pending_approval", approval_id }` OR combed signals when
  approved.
- Gate: cap `allow_privileged` (default deny); off-list op refused; audit before exec.

### privileged_list_ops
- Request: `{}`  Response: `{ ops: [allow-list], require_human_approve: bool }`

### privileged_approve (admin CLI only, not MCP)
- `terminal-commander privileged approve <approval_id>`

## P5 -- Remote federation

### target_list
- Request: `{}`  Response: `{ targets: [{ target_id, host, reachable }] }`

### target_probe
- Request: `{ target_id }`  Response: `{ reachable, daemon_version? }`

### Changed
- Every daemon-backed tool gains optional `target_id` (default = local).
- Transport is SSH local-forward to the remote UDS; no public TCP.

## P6 -- Certification (discovery payload)

### system_discover.omni_status (payload addition)
- `{ omni_status: { program_version, matrix: { shell_exec:{available},
  sessions:{available}, pty:{available,platform}, privileged_helper:{available,
  reason}, remote_targets:{count,reachable} } } }`
