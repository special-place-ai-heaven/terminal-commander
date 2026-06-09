# Wave 2 — Platform Parity

**Goal:** Feature parity across Linux, WSL, macOS, and native Windows so no LLM harness is a second-class citizen.

**Depends on:** Wave 1 shell/session (Linux path); ConPTY can start in parallel with TC50.

**Acceptance:** O-03, O-07, O-08.

---

## Workstreams

### WS-2A — Windows ConPTY (TC53)

**Problem:** `pty_command_*` reports `available:false` on native Windows.

**Approach:**

1. Add `portable-pty` dependency (already researched in `docs/research/async-runtime.md`).
2. Create `crates/daemon/src/pty_command_win.rs` or cfg-unify existing module.
3. Implement ConPTY spawn path mirroring Unix `pty_command.rs` API.
4. Secret-prompt boundary on Windows (password input detection).
5. Update `system_discover` availability logic.
6. Live e2e on Windows CI agent (no WSL).

**Files:** `crates/probes/src/pty.rs`, `crates/daemon/src/pty_command.rs`, platform tests.

**Effort:** 2–3 weeks.

---

### WS-2B — macOS tier-1 (TC54)

**Problem:** macOS is tier-3 per ADR; agents on Mac lack supported path.

**Deliverables:**

1. Daemon IPC on macOS (UDS — same as Linux).
2. Policy paths for macOS homebrew/dev layouts.
3. Full smoke: `scripts/smoke/verify-runtime-smoke.sh` equivalent for macOS.
4. npm platform package verification if applicable.

**Effort:** 2–3 weeks (may overlap ConPTY if same engineer knows both).

---

### WS-2C — Native filesystem backends (TC55)

**Problem:** File/directory probes poll at 250ms; 9P/WSL drvfs forced poll.

**Deliverables:**

1. Integrate `notify` + debouncer for Linux/macOS native FS.
2. Keep poll fallback for 9P with explicit `system_discover` flag `file_watch_backend: poll|notify`.
3. Move-event detection (BACKLOG P1).

**Effort:** 1–2 weeks.

---

### WS-2D — Process lifecycle hardening (TC56)

**Deliverables:**

1. `process-wrap` integration — SIGTERM to group + grace ladder.
2. Orphan reaping on session stop.
3. Align command + PTY + shell session cancel paths.

**Effort:** 1 week.

---

## Sequencing

```text
TC53 ConPTY          ─────────────────────────►
TC54 macOS tier-1    ──────────────►
TC55 notify          ────────►
TC56 process-wrap    ──►
```

**Total:** 4–6 weeks with 1–2 engineers.

---

## Platform matrix (target end state)

| Capability | Linux | WSL | macOS | Win native |
|---|---|---|---|---|
| command_start_combed | yes | yes | yes | yes |
| run_and_watch | yes | yes | yes | yes |
| pty_command_* | yes | yes | yes | yes |
| shell_exec | yes | yes | yes | yes |
| shell_session_* | yes | yes | yes | yes (after ConPTY) |
| file_watch notify | yes | poll on /mnt/c | yes | poll/N/A |

---

## Risks

| Risk | Mitigation |
|---|---|
| ConPTY edge cases (Windows Server, headless) | CI matrix + graceful degrade in discover |
| macOS seatbelt/sandbox | Document dev profile; no kernel sandbox in v1 |
| notify + WSL 9P | Keep poll; document in playbook |
