# Terminal Commander - Runtime Chain Evidence Report

Status: TC48 beta gate snapshot.

This document consolidates the TC33-TC47 evidence trail. Each goal
section records the verified work commit, the goal status commit,
the live source-status (`live` / `partial` / `not run` / etc.), and
the bounded-output / pointer / audit invariants the goal touched.

Language: ASCII only.

## Chain summary

| Goal  | Title                                                            | Status     | Verified work | Goal status |
|-------|------------------------------------------------------------------|------------|---------------|-------------|
| TC35  | Persistent audit log V0003                                       | Completed  | `6c88334`     | `3212511`   |
| TC36  | Daemon runtime bootstrap + config                                | Completed  | `eee12cc`     | `c6e6774`   |
| TC37  | Daemon UDS IPC + peer identity                                   | Completed  | `b816a95`     | `405209a`   |
| TC38  | Daemon command runtime + shell-bridge guard                      | Completed  | `328b2a5`     | `c542927`   |
| TC39  | Daemon signal-retrieval API over UDS                             | Completed  | `e42c4dc`     | `3262ff2`   |
| TC40  | rmcp stdio MCP adapter + daemon UDS forwarding                   | Completed  | `a61f5ac`     | `4e179db` (+ amended `2d27f78`) |
| TC41  | MCP command + bucket tool surface over daemon UDS                | Completed  | `31f6aec`     | `8a89888`   |
| TC42  | Registry hot activation + rule binding through MCP/UDS           | Completed  | `9c8ce0e`     | `fdc2a79` / `05d8ab9` |
| TC42b | Live rule rebind for already-running command streams             | Completed  | `0974ac4`     | `7e45a41`   |
| TC42c | Scoped registry rule bindings for buckets/jobs/probes            | Completed  | `1158025`     | `7f6ba2e`   |
| TC42d | Require explicit scope on registry activate/deactivate           | Completed  | `a636697`     | `db4b502`   |
| TC43  | File probe search/watch/bounded-read tools                       | Completed  | `06ea2d5`     | `5848715`   |
| TC44  | POSIX PTY spawn + bounded stdin with secret-prompt boundary      | Completed  | `4cc8dcc`     | `9b74788`   |
| TC45  | Aggregate runtime view (runtime_state / probe_list / probe_status) | Completed | `96ddc62`     | `385052d`   |
| TC46  | Provider-harness MCP stdio smoke (local + Codex + Claude configs) | Completed | `a7e1544`     | `def548b`   |
| TC47  | Load / noise / backpressure gate (8 stress tests)                | Completed  | `003ab17`     | `726a299`   |

Tool catalogue: 29 live tools (TC45 surface, unchanged by TC46/TC47/TC48).

## Per-goal evidence

### TC35 — Persistent audit log V0003

- **Source-status:** live.
- **What it proves:** `PersistentAudit` is the production audit
  sink for every IPC accepted request and every probe lifecycle
  event. The TC35 self-check verifies the V0003 audit migration is
  applied at bootstrap and that audit emit -> store round-trip is
  observable.
- **Invariants touched:** every accepted IPC request lands one row
  in the audit table; rows are bounded structured data, never raw
  stream content.

### TC37 — Daemon UDS IPC + peer identity

- **Source-status:** live, Linux/WSL2 only.
- **What it proves:** `IpcServer` binds
  `<data_dir>/terminal-commanderd.sock` and accepts connections via
  Unix domain socket; PeerCred captures uid/gid/pid per connection
  on Linux. No TCP listener exists.
- **Invariants touched:** local-only transport; envelope size cap
  (`MAX_FRAME_BYTES`); every request is bounded; no streaming lane.
- **TC46 regression evidence:** `bash scripts/smoke/verify-runtime-smoke.sh`
  exercises this transport end-to-end as the secondary local smoke.

### TC38 — Daemon command runtime + shell-bridge guard

- **Source-status:** live.
- **What it proves:** `CommandRuntime` accepts argv-only
  (`MAX_ARGV_ITEMS = 256`, `MAX_ARGV_ITEM_BYTES = 4096`); the
  `SHELL_INTERPRETERS_DENY` list rejects bash/sh/zsh/etc. before
  spawn; the `PolicyAction::CommandStart` default-deny list
  rejects sudo/doas/su/etc. The probe pipeline normalizes lines and
  feeds the sifter runtime; only sifter-emitted EventDrafts reach
  the bucket.
- **Invariants touched:** no shell bridge; argv-only; raw stdout
  never reaches the LLM directly.

### TC39 — Daemon signal-retrieval API over UDS

- **Source-status:** live.
- **What it proves:** `bucket_events_since`, `bucket_wait` (notify-
  driven, heartbeat-aware, NOT busy-poll), `bucket_summary`,
  `event_context` (bounded by `MAX_CONTEXT_FRAMES`,
  `MAX_CONTEXT_BYTES`) are all bounded structured responses. No
  raw stream lane.
- **Invariants touched:** every LLM-visible response is bounded;
  cursor-based reads stay under `MAX_BUCKET_READ_LIMIT = 10_000`.
- **TC47 regression evidence:** stress tests assert the limit
  clamps + the heartbeat block (>=700 ms for 800 ms timeout).

### TC40 — rmcp stdio MCP adapter + daemon UDS forwarding

- **Source-status:** live, Linux/WSL2 only.
- **What it proves:** `terminal-commander-mcp` serves an rmcp 1.7.0
  stdio MCP server and forwards every tool call through the daemon
  UDS. The MCP crate does not spawn commands, does not open files,
  does not bind a network socket.
- **Grep evidence on every commit since TC40:**
  - `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp`
    returns only doc / negative-assertion comments.
  - `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src`
    returns no matches.

### TC41 — MCP command + bucket tool surface over daemon UDS

- **Source-status:** live.
- **What it proves:** MCP tools `command_start_combed`,
  `command_status`, `bucket_events_since`, `bucket_wait`,
  `bucket_summary`, `event_context` forward through the daemon UDS
  with bounded JSON envelopes.

### TC42 / TC42b / TC42c / TC42d — Registry activation, scoped binding, live rebind, explicit scope

- **Source-status:** live.
- **What it proves:**
  - TC42: `registry_activate` / `registry_deactivate` mutate the
    daemon-side `ActivationRegistry`; new commands pick up the rule
    set at spawn time.
  - TC42b: a live command-stream's `SifterRuntime` is atomically
    rebuilt (no draft loss) when the active rule set changes
    mid-stream.
  - TC42c: scoped activation supports `Global` / `Bucket` / `Job` /
    `Probe`; the daemon validator resolves the scope id against
    every live runtime (command + file watch + PTY) so a scoped
    activation can target any of them.
  - TC42d: missing-scope is a typed rejection
    (`IpcErrorCode::ScopeInvalid`); cargo-nextest is the first-class
    workspace gate.
- **TC47 regression evidence:**
  `registry_activate_during_active_stream_rebinds_without_raw_leak`
  asserts the TC42b live-rebind path under noise.

### TC43 — File probe search/watch/bounded-read tools

- **Source-status:** live.
- **What it proves:** `file_read_window` (capped at
  `MAX_FILE_READ_LINES = 2000` and `MAX_FILE_READ_BYTES = 64 KiB`),
  `file_search` (capped at `MAX_FILE_SEARCH_MATCHES = 500`,
  `MAX_FILE_SEARCH_SNIPPET_BYTES = 512`,
  `MAX_FILE_SEARCH_SCAN_BYTES = 16 MiB`), `file_watch_start/stop/list`.
  Default-deny path list rejects sensitive paths
  (`IpcErrorCode::PathDenied`).
- **Invariants touched:** MCP never touches the filesystem; file
  watch emits only sifter-produced EventDrafts.

### TC44 — POSIX PTY spawn + bounded stdin + secret-prompt boundary

- **Source-status:** live, Linux/WSL2 only.
- **What it proves:** `pty-process = "=0.5.3"` (MIT, async feature)
  drives the POSIX PTY spawn. ANSI/CR normalization via
  `AnsiNormalizer`; secret-prompt detection via `PromptDetector`
  (now also inspects the pending partial-line buffer so
  `[sudo] password: ` prompts trip the secret flag without a
  newline). `command_write_stdin` returns
  `IpcErrorCode::SecretInputDenied` while a secret prompt is
  active; audit metadata is bounded
  `(job_id, byte_count, prompt_kind, decision, reason)` and NEVER
  the stdin bytes.
- **Invariants touched:** no automatic password entry; no
  LLM-supplied password forwarding; no raw PTY screen buffer
  endpoint.

### TC45 — Aggregate runtime view

- **Source-status:** live.
- **What it proves:** `runtime_state`, `probe_list`, `probe_status`
  surface the union of `command.live_jobs()`, `watch.list()`,
  `pty.list()` plus per-bucket counters and the scoped activation
  snapshot. Read-only; no new spawn API; no multi-bucket fan-out.
  Unknown probe id yields `IpcErrorCode::UnknownProbe`.
- **Invariants touched:** bounded JSON; no raw stream content; the
  TC42c/TC43/TC44 runtimes are unchanged.

### TC46 — Provider-harness MCP stdio smoke

- **Source-status:** secondary local smoke is live; provider live
  smoke is **Not Run** for both Codex CLI and Claude Code.
- **What it proves locally:** `scripts/smoke/verify-runtime-smoke.sh`
  builds the binaries, spawns daemon + MCP stdio, runs `initialize`
  + `tools/list` (>=29 tools) + `system_discover` + `health` +
  `command_start_combed` + `bucket_wait` + `command_status`. All 8
  assertions PASS including the raw-stream leak check (the
  literal echo argv string never appears outside argv/summary
  metadata).
- **Codex CLI Not Run reason (verbatim):**
  `Error: Missing optional dependency @openai/codex-linux-x64.
   Reinstall Codex: npm install -g @openai/codex@latest`. The
  `codex` shim under Windows nvm does not include the Linux x64
  native binary required to run under WSL2. Config-only example
  ships in `docs/integrations/codex-cli.md`.
- **Claude Code Not Run reason (verbatim):** `which claude`
  returns no result on the verification host; no `claude` binary
  in `$PATH` or in npm-global. Config-only example ships in
  `docs/integrations/claude-code.md` (both `--mcp-config` flag
  form and persistent settings form).
- **Invariants touched:** no provider-side secrets, tokens, or
  private paths in committed artifacts; integration docs use
  `${TC_DATA}` env var, not hardcoded user paths.

### TC47 — Load / noise / backpressure gate

- **Source-status:** live for all 8 stress tests in
  `crates/daemon/tests/load_noise_backpressure.rs`. Dedicated
  file-watch and PTY load tests are **Not Run**; reasons
  documented below.
- **What it proves:**
  - `megabyte_scale_noisy_stdout_emits_signal_without_raw_leak` —
    ~1 MB stdout (10_000 lines x ~100 bytes/line, 7 needles)
    through `command_start_combed`. >=1 `needle_match` emitted; the
    noise marker `noise-00009999` (only in raw stdout, NOT in argv)
    never appears in any bucket payload.
  - `bucket_wait_heartbeat_respects_timeout_without_busy_poll` —
    `bucket_wait` with `timeout_ms = 800` blocks >=700 ms.
  - `bucket_events_since_limit_clamps_to_max` — request for
    `MAX_BUCKET_READ_LIMIT * 10` clamps; payload stays under
    `MAX_RESPONSE_BYTES`.
  - `concurrent_probes_buckets_do_not_cross_talk` — bucket A has
    only probe A events (10 needles); bucket B has only probe B
    events (0 needles by construction).
  - `runtime_state_stays_bounded_under_live_load` — 3 concurrent
    jobs; `runtime_state` payload under `MAX_RESPONSE_BYTES` and
    `command_jobs == 3`.
  - `event_context_window_stays_bounded` —
    `MAX_CONTEXT_FRAMES + 100` requested; clamped.
  - `bucket_dropped_count_visible_when_retention_evicts` —
    `max_events: 8` vs 100 needles -> `dropped_count > 0`.
  - `registry_activate_during_active_stream_rebinds_without_raw_leak`
    — mid-stream activation produces >=1 `needle_match` from frames
    emitted AFTER the rebind.
- **Not Run — dedicated file-watch load test.** TC43 polling
  backend (120 ms) bounds push-rate; a dedicated test would measure
  the polling boundary, not the signal pipeline. Tracked as
  BACKLOG P2.1.
- **Not Run — dedicated PTY load test.** TC44 already covers
  ANSI/CR/secret path; a dedicated megabyte-rate PTY test would
  measure WSL `pty-process` throughput rather than Terminal
  Commander's bounded-output contract. Tracked as BACKLOG P2.2.
- **`frames_suppressed`:** the daemon does NOT surface a dedicated
  `frames_suppressed` counter today. TC47 derives noise reduction
  from `frames_total / events_emitted` only where the test owns
  both. Tracked as BACKLOG P1.1.

## Verification snapshot (latest gates)

Run on `main` at the TC47 status commit `726a299`, Linux WSL2,
`CARGO_TARGET_DIR=target-wsl`:

| Gate                                                           | Result |
|----------------------------------------------------------------|--------|
| `git branch --show-current`                                    | `main` |
| `git status --short`                                           | clean  |
| `git diff --check`                                             | PASS   |
| `cargo metadata --no-deps`                                     | PASS   |
| `cargo fmt --all --check`                                      | PASS   |
| `cargo clippy --workspace --all-targets -- -D warnings`        | PASS   |
| `cargo test --workspace`                                       | every suite green |
| `cargo nextest run --workspace`                                | **347/347 passing, 0 skipped** |
| `cargo test -p terminal-commanderd --test load_noise_backpressure` | 8/8 in ~3.8s |
| `bash scripts/smoke/verify-runtime-smoke.sh`                   | SUCCESS, 8/8 PASS assertions |
| `rg "Command::new\|Command::spawn\|TcpListener\|UdpSocket" crates/mcp` | doc/negative matches only |
| `rg "tokio::fs\|std::fs\|File::open\|read_to_string\|read_to_end" crates/mcp/src` | no matches |

## Beta posture cross-reference

The TC48 beta recommendation is `Conditional Go`. The rationale,
provider blockers, and `Not Run` areas are documented in
`RELEASE_CHECKLIST.md`. Open risks live in `RISK_REGISTER.md`.
Active follow-ups (P1/P2/P3) live in `BACKLOG.md`.
