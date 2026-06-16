# Phase 0 Research: Omni Completion Program

All Technical Context unknowns resolved below. Each decision is grounded in the
existing codebase (verified via SymForge) and the project constitution.

## R-1 (P1) Shell session backend

- **Decision**: Implement `ShellSessionRuntime` over the existing `pty_command.rs`
  PTY infrastructure -- a long-lived login shell per session; each
  `shell_session_exec` writes a line + `\n` to the PTY and the sifter reads the
  merged PTY stream into the session bucket.
- **Rationale**: Sticky cwd/env require a persistent process; the PTY runtime
  already merges streams, normalizes ANSI/CR, detects prompts, and enforces
  secret-prompt denial. `shell.rs` (`ShellRuntime::exec`) already proves the
  policy-gated shell lane over the command core; sessions are the long-lived
  analogue. Reuse > reinvention.
- **Alternatives**: (a) pipe-backed persistent process -- rejected: cannot
  preserve interactive cwd/env semantics or prompt detection. (b) re-exec with
  serialized cwd/env each call -- rejected: not a real session; breaks REPLs.

## R-2 (P1) Session policy + caps

- **Decision**: Add `PolicyAction::SessionStart { shell, cwd }` gated by a new
  `allow_session` cap on `PolicyCaps`; default deny; `developer_local` +
  `allow_session=true` -> AllowWithAudit. Mirror the `caps_allow_shell` /
  `shell_start_*` pattern already in `policy.rs`.
- **Rationale**: The cap scaffold and the exact gating + audit pattern already
  exist for shell; sessions follow it 1:1, keeping the policy surface uniform and
  testable (the existing `shell_start_denied_*` tests are the template).
- **Alternatives**: Reuse `allow_shell` -- rejected: sessions are a longer-lived,
  higher-blast-radius capability and deserve an independent operator switch.

## R-3 (P1) Session limits + lifecycle

- **Decision**: `[shell_session] max_sessions=4, idle_ttl_secs=3600` in config;
  an idle reaper (reuse the existing daemon reaper pattern) tears down sessions
  past TTL; a terminal-state guard (mirroring the PTY waiter guard) prevents
  sends to a dead shell.
- **Rationale**: Bounded resource use is a constitution-adjacent safety property;
  the daemon already has an idle reaper and a PTY terminal-state guard to reuse.
- **Alternatives**: Unbounded sessions -- rejected (resource-exhaustion risk).

## R-4 (P1, ledger) TC-B1 ANSI stripping

- **Decision**: Strip ANSI/CSI/OSC before sifter rule matching and in emitted
  summaries on the non-PTY process path (`probes/src/process.rs`), keeping raw
  bytes in the frame store; expose `strip_ansi: bool` default true. Reuse `vte`
  (already locked for the PTY/test corpus) for a UTF-8-safe stripper in a shared
  helper.
- **Rationale**: Colored output silently defeats anchored rules and pollutes
  summaries (observed live). PTY already normalizes; the process path does not.
  A shared stripper keeps both paths consistent.
- **Alternatives**: Strip only in summaries -- rejected: anchored rules still
  fail to match. Strip destructively in the frame store -- rejected: raw bytes
  must remain retrievable.

## R-5 (P1, ledger) TC-E1 compact mode + TC-E4 capture canonicalization

- **Decision**: Add `compact: bool` (default false) to signal-returning tool
  params; when true, project each signal to `{summary, stream, seq, severity}`
  in the MCP layer (`mcp/src/tools.rs`) -- a presentation projection, not a wire
  change to the event store. Separately, collapse the duplicate capture echo
  (`0`/`line`/`match`) to one canonical field plus named captures at the sifter
  emit site.
- **Rationale**: Id plumbing dominates token cost for the common case; a
  projection is non-invasive and reversible. Capture dedup is a pure emit-shape
  cleanup. Both honor "bounded output".
- **Alternatives**: New compact event type in the store -- rejected: larger blast
  radius for a presentation concern.

## R-6 (P1, ledger) TC-E2 honest waiting + TC-B3 receipt persistence

- **Decision**: TC-E2 -- add `wait_until: "exit"` to the wait loop with a
  server-side hard cap and a `poll_hint_ms` in running responses; reuse the
  wall-clock `Instant` deadline pattern from the TC-6 run_and_watch rewrite so the
  advertised cap is honored to the wire. TC-B3 -- persist a compact job/bucket
  receipt (`{job_id, terminal_state, exit_code, restarted_at}`) to SQLite on
  every completion path; a post-restart `command_status` reads the receipt and
  returns a restart-marked terminal result instead of an error.
- **Rationale**: Both are honesty guarantees (constitution VII). The cap pattern
  already exists; the receipt store is a small additive table keyed by job_id.
- **Alternatives**: In-memory-only receipts -- rejected: lost on the exact restart
  TC-B3 targets.

## R-7 (P2) Rule suggestion + universal extractors + packs

- **Decision**: `registry_suggest_from_samples` = pure-Rust heuristics
  (error/warning/FAILED prefixes, path shapes, exit summaries) returning proposals
  with a `confidence` label and `next_steps`; NEVER auto-activates (FR-008).
  Universal extractors = always-on low-severity sifters gated by
  `sifters.universal_extractors`. Packs: add JSON under `crates/store/rules/`
  (docker, kubectl, git first; then pip/uv/go/systemd/...) registered in
  `import.rs` to reach 25+.
- **Rationale**: Heuristics are deterministic and testable; the suggest->test->
  activate loop is the constitution's suggest-never-auto-activate rule made
  concrete. Packs are additive data.
- **Alternatives**: ML suggestion -- explicitly out of scope (non-deterministic,
  unverifiable in CI).

## R-8 (P3) Platform parity

- **Decision**: Windows ConPTY via `portable-pty` behind the existing
  `pty_command` tool surface (unify the runtime, feature-gated per OS); macOS
  reuses the POSIX PTY path and gains a tier-1 smoke; file backend swaps the
  120ms poller for `notify` on native FS while keeping the poll fallback for WSL
  9P `/mnt/c` (detected via mountinfo, fixtures already exist); a process-group
  SIGTERM-then-SIGKILL grace ladder unifies command/PTY/session cancel.
- **Rationale**: `portable-pty` and `notify` are the established crates for these
  jobs (per `docs/research/async-runtime.md` / file-watcher research); the WSL 9P
  fallback is a known constraint with existing detection fixtures.
- **Alternatives**: Native ConPTY FFI by hand -- rejected: `portable-pty` is the
  vetted abstraction. Drop poll fallback -- rejected: breaks WSL `/mnt/c`.

## R-9 (P4) Privileged helper -- PLAN ONLY this run

- **Decision**: Specify a separate `terminal-commander-privileged` binary with a
  closed allow-list (`apt_install`/`apt_update`/`systemctl_*`/
  `journal_read_window`/`winget_install`/`sc_*`), `privileged_exec` +
  `privileged_list_ops` MCP tools, admin-CLI-only `privileged_approve`, and a
  human-approval token flow gated by `allow_privileged`. NO code lands until a
  dedicated threat review completes (clarify decision 2026-06-16).
- **Rationale**: Highest blast radius; the constitution forbids generic sudo and
  mandates a closed allow-list + audit; a threat review must precede code.
- **Alternatives**: Generic `sudo -c` -- forbidden by constitution II.

## R-10 (P5) Remote federation

- **Decision**: A remote TC daemon per host reached via SSH `-L` local-forward to
  the remote daemon's UDS; optional `target_id` on every daemon-backed tool
  (default local); `target_list` + `target_probe` tools; `targets.toml` config.
  No public TCP listener.
- **Rationale**: Combing must happen on the remote host (not SSH-exec without
  combing); tunnelling to an existing local socket preserves constitution IV.
- **Alternatives**: Public daemon TCP port -- forbidden. SSH-exec passthrough --
  rejected: no combing, violates principle III.

## R-11 (P6) Certification

- **Decision**: `scripts/smoke/verify-omni-{linux,wsl,windows,macos}` each run the
  O-01..O-14 sequence and exit non-zero on any gap; `system_discover.omni_status`
  returns a capability matrix with reasons; provider trust smokes for
  Cursor/Codex/Claude; `docs/mcp/OMNI_PLAYBOOK.md` decision tree; README/SPEC/
  ROADMAP realign; version -> 1.0.0 when all gates green.
- **Rationale**: A single automated proof per platform is the only honest way to
  claim "never needs a separate terminal tool".
- **Alternatives**: Manual checklists only -- rejected: not repeatable.

## Cross-cutting: tool-count anchors

- **Decision**: Every tool-adding task updates all count anchors (the `39` ->
  N+1 references in `crates/mcp/tests/`, `mcp/src/main.rs` header, the
  `system_discover` fixture, `minimal_tool_args`, and docs) in the same change;
  CI count assertions are the gate.
- **Rationale**: Established convention (TC49 bumped 38->39 this way); drift is a
  known recurring chore (BACKLOG TCD-7).
