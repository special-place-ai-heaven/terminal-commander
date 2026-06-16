---
description: "Task list for the Omni Completion Program"
---

# Tasks: Omni Completion Program

**Input**: Design documents from `/specs/001-omni-completion/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/mcp-tools.md, quickstart.md

**Tests**: REQUIRED. The constitution (VI No-Mock + Verification Gate) mandates
through-the-daemon integration tests per new MCP tool and source-status labels.
Tests are written before implementation per principle VI (TDD).

**Organization**: Grouped by user story (P1-P6). Each story is an independently
shippable slice; after each, run the verification gate and commit to
`feature/omni-<slice>`, pausing before merge/push.

**Verification gate (after every code task group)**:
`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace`

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different files, no incomplete-task dependency)
- **[Story]**: US1=P1 sessions, US2=P2 parse, US3=P3 platform, US4=P4 privileged
  (plan-only), US5=P5 remote, US6=P6 certify

---

## Phase 1: Setup (Shared Infrastructure)

- [ ] T001 Confirm baseline green: run the verification gate on current `main` and record the current tool count (grep `39` anchors in `crates/mcp/tests/` and `crates/mcp/src/main.rs`).
- [ ] T002 [P] Create `crates/daemon/src/util/ansi.rs` shared ANSI/CSI/OSC stripper (UTF-8-safe, `vte`-backed) with unit tests; export from `crates/daemon/src/lib.rs`. (Foundation for TC-B1; reused by process + summary paths.)
- [ ] T003 [P] Add config blocks scaffolding in `crates/daemon/src/config.rs`: `[shell_session]` (max_sessions, idle_ttl_secs), placeholders for `[privileged_helper]` and `[[targets]]` (parsed, defaults safe/disabled).

**Checkpoint**: baseline proven, shared stripper + config slots exist.

---

## Phase 2: Foundational (Blocking Prerequisites)

**CRITICAL**: blocks all user stories.

- [X] T004 Extend `PolicyCaps` in `crates/daemon/src/policy.rs` with `allow_session`, `allow_privileged`, `allow_remote` (default false); add `caps_allow_session` accessor mirroring `caps_allow_shell`. NOTE: the three cap fields already existed (commit 8308842); this task added the `caps_allow_session` accessor.
- [X] T005 Add `PolicyAction::SessionStart { shell, cwd }` variant in `crates/daemon/src/policy.rs`; extend `evaluate` with default-deny + cap-gated AllowWithAudit (deny-first gate mirroring the shell lane); add the exhaustive `action_path_subject` arm. (`PrivilegedExec`/`RemoteTargetUse` deferred to P4/P5 per slice.)
- [X] T006 [P] Policy tests in `crates/daemon/src/policy.rs` mod tests: `session_start_denied_by_default`, `session_start_allowed_with_audit_when_cap_on`, `session_start_denied_in_repo_only_even_with_cap`, `session_start_denied_in_read_only_observer_even_with_cap`, `session_cap_independent_of_shell_cap` -- all PASS (fmt+clippy clean, Windows cargo).
- [ ] T007 Add the persisted receipt table + accessor (TC-B3) in the daemon store layer (`crates/daemon/src/ipc/server.rs` + store): `JobReceipt { job_id, bucket_id, terminal_state, exit_code, final_signal_counts, restarted_at, created_at }`; write on every terminal transition.

**Checkpoint**: caps, gated actions, audit, and receipt persistence exist for all stories to build on.

---

## Phase 3: User Story 1 - Persistent shell sessions + workspace + ledger fixes (Priority: P1) MVP

**Goal**: Sticky-cwd/env sessions, workspace snapshots, and the five folded
ledger fixes. Gates O-02; closes SC-001/008/009/010/011.

**Independent Test**: start session, `cd /tmp`, `pwd` -> `/tmp` without re-passing cwd.

### Tests for User Story 1 (write first, must fail)

- [X] T008 [P] [US1] Integration test `crates/daemon/tests/shell_session_ipc.rs`: start -> exec `cd /tmp` -> exec `pwd` -> signal contains `/tmp`; status returns cwd/env; stop is graceful. (O-02 PASS live via IPC.)
- [X] T009 [P] [US1] Live e2e `crates/mcp/tests/shell_session_live_e2e.rs`: full session flow through the MCP adapter; default-deny denial when cap off. (O-02 PASS live via adapter.)
- [X] T010 [P] [US1] Test `crates/probes/tests/ansi_strip.rs` (TC-B1) + CRLF-normalizer tests: anchored rule matches stripped output; no escape bytes in summary; raw bytes retrievable; UTF-8 safe.
- [X] T011 [P] [US1] `crates/mcp/tests/ledger_compact_wait_restart.rs`: compact projection (4 fields only) + single canonical capture field.
- [X] T012 [P] [US1] Same file: TC-E2 wait_until:"exit" wall-time <= cap (1.6s vs 30s sleep); TC-B3 restart -> restart-marked terminal result.

### Implementation for User Story 1

- [X] T013 [US1] `crates/daemon/src/shell_session.rs`: `ShellSessionRuntime` over PtyRuntime; long-lived login shell; sticky cwd/env; terminal-state guard; idle reaper; `max_sessions` cap.
- [X] T014 [US1] IPC protocol variants in `crates/ipc/src/protocol.rs`: ShellSession Start/Exec/Status/Stop/List, WorkspaceSnapshot Create/Apply + error codes.
- [X] T015 [US1] Handlers `crates/daemon/src/ipc/handlers/session.rs` + dispatch in `ipc/server.rs`; SessionStart policy-check + audit before spawn.
- [X] T016 [US1] Wired `ShellSessionRuntime` into `state.rs` bootstrap + `runtime.rs` + `config.rs` `[shell_session]`.
- [X] T017 [US1] Workspace snapshot persistence (SQLite migration V0006 + `store/src/workspace.rs`); create/apply handlers.
- [X] T018 [US1] 7 MCP forwarder tools in `crates/mcp/src/tools.rs` (adapter no-spawn guard PASS).
- [X] T019 [US1] TC-B1: `crates/probes/src/ansi.rs` strip before sift + in summaries on process path; raw kept in ring; `strip_ansi` default true; AnsiNormalizer made CRLF-aware.
- [X] T020 [US1] TC-E1 compact projection in MCP layer + TC-E4 canonical capture key at sifter emit site.
- [X] T021 [US1] TC-E2 wait_until:"exit" + poll_hint_ms (cap honored to the wire) + TC-B3 SQLite job-receipt (migration V0007) restart-marked status fallback.
- [X] T022 [US1] Tool-count anchors 39 -> 46 across all anchor files + 7 contract fixtures + system_discover fixture.
- [ ] T023 [US1] Docs: `docs/runtime/SHELL_SESSION.md` + `POLICY.md` SessionStart algorithm. DEFERRED to the US6 documentation pass (T058) to batch all doc realignment; tracked, not dropped.
- [X] T024 [US1] Verified in WSL (fmt + clippy -D + nextest 651 passed/1 skipped); 3 commits on `feature/omni-p1-sessions` (d9b1c75 policy, 673e0bd sessions, eec1e38 ledger). PAUSED before merge/push.

**Checkpoint**: O-02 passes; ledger fixes verified; MVP shippable.

---

## Phase 4: User Story 2 - Parse omni (Priority: P2) [parallelizable with US3]

**Goal**: suggest-from-samples, universal extractors, 25+ packs, pack hints. Gates O-05; SC-002/003.

**Independent Test**: unknown tool -> suggest -> test -> activate -> signals; nothing auto-activated.

### Tests for User Story 2 (write first)

- [ ] T025 [P] [US2] Unit tests for suggestion heuristics in `crates/sifters/src/` (or new `suggest.rs`): error/warning/FAILED/path detection; empty-proposal case.
- [ ] T026 [P] [US2] Closed-loop e2e `crates/mcp/tests/suggest_loop_e2e.rs`: suggest -> assert NOT activated -> test -> upsert -> activate -> re-run -> signals.
- [ ] T027 [P] [US2] Pack import tests: docker/kubectl/git packs load and match representative fixtures under `tests/fixtures/terminal/`.

### Implementation for User Story 2

- [ ] T028 [US2] Implement `registry_suggest_from_samples` heuristics (pure Rust) returning `{proposed_rules, confidence, next_steps}`; NEVER activates.
- [ ] T029 [US2] IPC variant + daemon handler for suggest in `crates/ipc/src/protocol.rs` + `crates/daemon/src/ipc/handlers/registry.rs`.
- [ ] T030 [US2] MCP tool `registry_suggest_from_samples` in `crates/mcp/src/tools.rs`.
- [ ] T031 [US2] Universal extractors: config-gated always-on low-severity sifters (stderr/warning/exit/progress) in `crates/sifters/src/`; gate on `sifters.universal_extractors`.
- [ ] T032 [P] [US2] Add rule packs JSON in `crates/store/rules/`: docker, kubectl, git (P0), then pip, uv, go, systemd/journal, msbuild, winget, choco, terraform, ansible to reach >=25; register in `crates/store/src/import.rs`.
- [ ] T033 [US2] Pack-available hint in command-start responses (`{kind:"pack_available", pack, action}`) when a recognized tool runs without its pack.
- [ ] T034 [US2] Update tool-count anchors (+1 suggest tool) + contract fixture `registry_suggest_from_samples.v1.json`.
- [ ] T035 [US2] Run verification gate + e2e; record source-status; commit to `feature/omni-p2-parse`; PAUSE.

**Checkpoint**: O-05 closed loop works; 25+ packs available.

---

## Phase 5: User Story 3 - Platform parity (Priority: P3) [parallelizable with US2]

**Goal**: Windows ConPTY, macOS tier-1, notify file backend, grace-ladder cancel. Gates O-03/O-07/O-08; SC-004.

**Independent Test**: native Windows REPL via PTY tools; macOS smoke; prompt file-change signal.

### Tests for User Story 3 (write first)

- [ ] T036 [P] [US3] Windows PTY live e2e (cfg(windows)) driving a REPL with bounded combed output.
- [ ] T037 [P] [US3] File backend test: native-FS event-driven signal latency + WSL 9P poll-fallback selection (reuse `tests/fixtures/probes/wsl-mountinfo/`).
- [ ] T038 [P] [US3] Cancel-ladder test: SIGTERM-then-SIGKILL across command/PTY/session; terminal state reported.

### Implementation for User Story 3

- [ ] T039 [US3] Add `portable-pty` and implement Windows ConPTY path behind `pty_command_*` in `crates/daemon/src/pty_command.rs` (feature/cfg-gated); update `system_discover` PTY/platform fields.
- [ ] T040 [US3] macOS tier-1: ensure POSIX PTY path + daemon/MCP smoke; add `scripts/smoke/verify-runtime-smoke` macOS coverage.
- [ ] T041 [US3] Add `notify` event-driven file backend in `crates/probes/src/file.rs`; keep 9P poll fallback (mountinfo detection); wire in `crates/daemon/src/file_watch.rs`.
- [ ] T042 [US3] Process-group SIGTERM-then-SIGKILL grace ladder in `crates/probes/src/process.rs`; align command/PTY/session stop contracts.
- [ ] T043 [US3] Run verification gate (+ platform-specific where runnable); record source-status incl `blocked` for any platform not runnable on the dev host; commit to `feature/omni-p3-platform`; PAUSE.

**Checkpoint**: PTY parity on tier-1 platforms; responsive file signals.

---

## Phase 6: User Story 4 - Privileged helper (Priority: P4) PLAN-ONLY (threat-review gated)

**Goal**: Specify, do NOT implement. Gates O-06 (deferred). Per clarify decision,
no privileged code lands until a dedicated threat review completes.

- [ ] T044 [US4] Write `docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md`: attack surface, allow-list rationale, approval-token threat model, audit requirements; mark BLOCKED-on-review.
- [ ] T045 [US4] Update `docs/security/PRIVILEGE_MODEL.md` section 5 with the helper architecture (closed allow-list, separate binary, no shell line, no generic sudo).
- [ ] T046 [US4] Record the P4 contract (privileged_exec/list_ops/approve) in `contracts/mcp-tools.md` (done) and add a deferred-tasks note; NO code, NO new crate yet.

**Checkpoint**: P4 fully specified and threat-review-queued; zero privileged code.

---

## Phase 7: User Story 5 - Remote federation (Priority: P5)

**Goal**: SSH-forward remote daemon, `target_id` on tools, target_list/probe. Gates O-09/O-10; SC-006.

**Independent Test**: register target -> probe -> run tool with target_id -> remote combed signals; no public TCP.

### Tests for User Story 5 (write first)

- [ ] T047 [P] [US5] Target router unit tests: default-local when `target_id` unset; SSH-forward transport selection; no TCP listener opened.
- [ ] T048 [P] [US5] Remote e2e (loopback SSH or mock-forward to a second local socket): command -> combed signals via tunnel.

### Implementation for User Story 5

- [ ] T049 [US5] `targets.toml` parsing in `crates/daemon/src/config.rs`; `RemoteTarget` model.
- [ ] T050 [US5] Target router in the MCP adapter / daemon client: optional `target_id` on every daemon-backed tool; SSH `-L` local-forward to remote UDS; default local.
- [ ] T051 [US5] MCP tools `target_list` + `target_probe`; gate remote use on `allow_remote` + audit.
- [ ] T052 [US5] Update tool-count anchors (+2) + contract fixtures `target_list.v1.json`, `target_probe.v1.json`.
- [ ] T053 [US5] Run verification gate + remote e2e; assert no public TCP; commit to `feature/omni-p5-remote`; PAUSE.

**Checkpoint**: remote combed commands work over tunnel only.

---

## Phase 8: User Story 6 - Certification + release (Priority: P6)

**Goal**: automated omni smokes, omni_status, provider smokes, docs, release. Gates O-14; SC-007.

- [ ] T054 [P] [US6] Create `scripts/smoke/verify-omni-linux.sh` running O-01..O-14, non-zero on failure.
- [ ] T055 [P] [US6] Create `verify-omni-wsl.sh`, `verify-omni-windows.ps1`, `verify-omni-macos.sh` (same sequence).
- [ ] T056 [US6] Add `system_discover.omni_status` capability matrix payload in `crates/mcp/src/tools.rs` + daemon assembly; update fixture.
- [ ] T057 [P] [US6] Provider trust smokes for Cursor/Codex/Claude (extend `examples/provider-harness/` + docs/integrations).
- [ ] T058 [P] [US6] Create `docs/mcp/OMNI_PLAYBOOK.md` agent decision tree; realign `README.md`, `SPEC.md`, `ROADMAP.md` to omni identity.
- [ ] T059 [US6] Version bump to 1.0.0 once all O-* gates green (release-please-driven; operator-gated publish); commit to `feature/omni-p6-certify`; PAUSE.

**Checkpoint**: omni promise provable per platform; release-ready.

---

## Phase 9: Polish & Cross-Cutting Concerns

- [ ] T060 [P] Full stale-doc tool-count sweep (BACKLOG TCD-7): reconcile every non-gated count reference across README/SPEC/RELEASE_CHECKLIST/CONTRIBUTING.
- [ ] T061 [P] Update `BACKLOG.md`: mark closed items; move ledger fixes + waves to Resolved with commit refs.
- [ ] T062 Run `quickstart.md` validation end-to-end on Linux + WSL; capture evidence.
- [ ] T063 `cargo deny`, `cargo machete`, MSRV gate, doctests (full TESTING.md seven-step pipeline) before any release tag.

---

## Dependencies & Execution Order

- **Setup (P1 tasks T001-T003)**: first.
- **Foundational (T004-T007)**: blocks ALL user stories.
- **US1 (P1)**: after Foundational. MVP. Folds in all ledger fixes.
- **US2 (P2) and US3 (P3)**: after Foundational; independent of each other and of
  US1 (can run in parallel by separate agents).
- **US4 (P4)**: plan-only; can be authored any time after spec; no code dependency.
- **US5 (P5)**: after US1 (reuses session/tool patterns) and a stable local surface.
- **US6 (P6)**: last; gates on US1-US3 + US5 landing (US4 deferred is reflected in
  omni_status as `available:false, reason:"threat_review_pending"`).
- **Polish (P9)**: after desired stories complete.

### Within each story

- Tests written first and FAIL before implementation (constitution VI).
- Protocol/types -> daemon runtime -> handlers/dispatch -> MCP tool -> anchors/fixtures -> docs -> verify.
- Story complete (verify gate green + review branch) before next priority.

### Parallel opportunities

- T002, T003 in Setup.
- All `[P]` test tasks within a story.
- US2 and US3 across separate agents (different crates: sifters/store vs probes/pty).
- Rule-pack JSON authoring (T032) parallel across packs.

---

## Implementation Strategy

### MVP first

Setup -> Foundational -> US1 (sessions + ledger fixes) -> STOP, validate O-02 +
SC-008/009/010/011 live -> review branch.

### Incremental delivery

Each subsequent slice (US2, US3, US5, then US6) adds value, verifies, and lands on
its own review branch without breaking prior slices. US4 stays plan-only until
threat review.

### Parallel team / agent strategy

After Foundational: agent A -> US2 (parse), agent B -> US3 (platform), main ->
US1. Throttle compile-heavy agents (cap ~2 concurrent cargo builds) per the
resource-contention rule.

---

## Notes

- [P] = different files, no dependency. [Story] maps to spec.md priorities.
- Every tool-adding task MUST update all count anchors + discovery fixture in the
  same commit (CI count assertions are the gate).
- Record a source-status label for every behavior; `unknown` is a commit-time fail.
- Commit each slice to `feature/omni-<slice>`; PAUSE before merge/push (operator approval).
