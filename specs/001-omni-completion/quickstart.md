# Quickstart / Validation Guide: Omni Completion Program

This guide proves each slice end-to-end. It references contracts in
`contracts/mcp-tools.md` and entities in `data-model.md`; it does not duplicate
implementation. The omni acceptance gates (O-01..O-14) live in
`docs/plans/LLM-HANDOFF-tc-omni-program.md`.

## Prerequisites

- Rust toolchain at MSRV 1.92.0; `cargo-nextest` installed.
- Build: `cargo build --workspace`.
- Verification gate (run after every slice):
  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo nextest run --workspace
  ```

## P1 -- Sessions + workspace (gate O-02)

1. Enable `allow_session` in a `developer_local` test config.
2. `shell_session_start` -> get `session_id`.
3. `shell_session_exec` `cd /tmp`; then `shell_session_exec` `pwd`.
4. **Expect**: the second call's signal reports `/tmp` without re-passing cwd.
5. `workspace_snapshot_create`; start a fresh session; `workspace_snapshot_apply`;
   confirm cwd restored.
6. **Negative**: default config (cap off) -> `shell_session_start` denied + audited.
7. Ledger checks in this slice:
   - TC-B1: run a command with colored output + an anchored rule -> rule matches;
     summaries contain no escape bytes; raw bytes still retrievable.
   - TC-E1: same call with `compact:true` -> response carries only
     `{summary,stream,seq,severity}`; measure byte reduction (SC-008).
   - TC-E2: `wait_until:"exit"` -> wall time <= advertised cap (SC-011).
   - TC-B3: start a job, restart the daemon, `command_status(job_id)` ->
     restart-marked terminal result, not an error (SC-009).
   - TC-E4: a simple-pattern signal has one canonical capture field, no triple echo.
- **Tests**: `crates/daemon/tests/shell_session_ipc.rs`,
  `crates/mcp/tests/shell_session_live_e2e.rs`, policy tests mirroring
  `shell_start_*`, and the ledger-fix unit/integration tests.

## P2 -- Parse omni (gate O-05)

1. Run an unknown tool via `run_and_watch`; capture bounded tail.
2. `registry_suggest_from_samples` with sampled lines -> proposals + confidence +
   next_steps; **assert nothing activated**.
3. `registry_test` -> `registry_upsert` -> `registry_activate` one rule; re-run ->
   signals appear.
4. Enable universal extractors -> a no-pack command still emits low-severity
   error/warning/exit signals.
5. `registry list` shows >=25 packs incl docker/kubectl/git (SC-003).
- **Tests**: suggest heuristics unit tests; a closed-loop e2e; pack import tests.

## P3 -- Platform parity (gates O-03, O-07, O-08)

1. Native Windows: open a Python REPL via `pty_command_*`; drive it; bounded combed
   output; `system_discover` shows PTY available.
2. macOS: run `scripts/smoke/verify-runtime-smoke.sh` equivalent -> command ->
   wait -> status passes.
3. Native FS: change a watched file -> signal arrives promptly (event-driven);
   WSL `/mnt/c` still works via poll fallback.
4. Stop a running command/PTY/session -> graceful-then-forced; terminal state reported.

## P4 -- Privileged helper (gate O-06) -- VALIDATION DEFERRED

Plan/spec only this run. Validation steps recorded for after the threat review:
default-deny denial; pending-approval flow; off-list refusal; audit-before-exec.

## P5 -- Remote federation (gates O-09, O-10)

1. Register a target in `targets.toml`; `target_probe` -> reachable.
2. Run a daemon-backed tool with `target_id` set -> combed signals from remote.
3. Confirm no public TCP port opened on either host (netstat check in smoke).

## P6 -- Certification (gate O-14)

1. Run `scripts/smoke/verify-omni-linux.sh` (and wsl/windows/macos) -> executes
   O-01..O-14; exits non-zero on any gap.
2. `system_discover` -> `omni_status` capability matrix present and honest.
3. Provider trust smokes (Cursor/Codex/Claude) -> each does command->wait->status.
4. README/SPEC/ROADMAP present omni identity; version -> 1.0.0 when all gates green.

## Done signal per slice

A slice is "done" when: its quickstart steps pass live, the verification gate is
green, tool-count anchors + discovery fixture are updated, source-status labels
are recorded, and the slice is committed to its `feature/omni-<slice>` review
branch (paused before merge/push).
