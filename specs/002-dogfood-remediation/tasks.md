# Tasks: Dogfood Remediation Batch

**Input**: Design documents from `specs/002-dogfood-remediation/`

**Prerequisites**: plan.md, spec.md, research.md (D1-D11), data-model.md
(E1-E9), contracts/ (ipc-wire.md W1-W7, mcp-facade.md F1-F9, policy-wsl.md),
quickstart.md (gate commands + per-story test names)

**Tests**: REQUIRED. SC-008 mandates at least one red -> green test per FR:
write the test, prove it fails against pre-change behavior (run it before
the fix, or `git stash` the fix), then show it passing. Test tasks precede
implementation tasks inside every story.

**Organization**: one phase per user story, priority order
(P1: US1/US2/US3 -> P2: US4/US5/US8/US6 -> P3: US7/US9-optional). Stories
are independent; within a story the order is wire-first
(protocol.rs -> daemon handler -> MCP -> contract fixtures -> gate).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- **[Story]**: US1..US9 traceability label
- Every gate task means BOTH platforms (quickstart.md "verification gate"):
  Windows `cargo fmt --all --check && cargo clippy --workspace --all-targets
  -- -D warnings && cargo nextest run --workspace`, then the same gate-exact
  commands in WSL (`CARGO_TARGET_DIR=$HOME/tc-linux-target ~/.cargo/bin/cargo ...`).

## Path Conventions

Rust workspace at repo root: `crates/{mcp,ipc,daemon,store,sifters,core}`,
docs in `docs/`, MCP contract fixtures in
`crates/mcp/tests/fixtures/contracts/mcp-tools/`.

---

## Phase 1: Setup

**Purpose**: honest baseline so later changes cannot be blamed for
pre-existing failures, and a stable branch state.

- [x] T001 Confirm branch `002-dogfood-remediation` is current and clean; run the full verification gate on Windows AND WSL against unmodified code; record any pre-existing failures (with test names) in `specs/002-dogfood-remediation/baseline.md`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: none required — this batch is deliberately additive on existing
seams. There is no shared model, migration, or framework work that blocks
stories. US1's validator derives from the schemars schema at runtime, so it
needs no knowledge of fields other stories add later (research.md D1).

*(no tasks — proceed directly to user stories)*

**Checkpoint**: baseline recorded; any story phase may start.

---

## Phase 3: User Story 1 - Facade strictness (Priority: P1) - MVP

**Goal**: one failed call teaches everything — all missing required fields
reported at once; unknown-for-action fields rejected with a counterpart
remedy. Valid calls byte-identical. (FR-001/002/003; research D1; contract
mcp-facade.md F1.)

**Independent Test**: `registry {action:"deactivate", rule_id:"x"}` -> ONE
error naming `scope`; `command {action:"sub_pull", sub_id:..., wait_ms:30000}`
-> error naming `wait_ms` unknown + `timeout_ms` remedy; a correct call
behaves exactly as before.

### Tests (write first, prove red)

- [x] T002 [US1] Write failing strictness tests per quickstart US1 list (`facade_missing_fields_are_reported_all_at_once_with_action`, `facade_unknown_field_is_rejected_with_counterpart_remedy`, `facade_sub_pull_wait_ms_names_timeout_ms_as_remedy`, `facade_unknown_action_lists_valid_actions`) in the `#[cfg(test)]` mod of `crates/mcp/src/facade_strict.rs` (new file; tests target the validator's public fn) plus facade-level cases in `crates/mcp/src/facades.rs` test mod; record the red run

### Implementation

- [x] T003 [US1] Implement the schema-driven validator in `crates/mcp/src/facade_strict.rs`: derive per-action `required`/`properties` from the SAME schemars schema served by tools/list (cache per facade); alias allow-list (`samples` -> `sample_lines` on `suggest_from_samples` only); static counterpart table (`wait_ms` <-> `timeout_ms` pairs per mcp-facade.md F1); aggregate ALL missing + unknown violations into one `invalid_params` error naming the action; counterpart suggestions only when the counterpart exists in the chosen action's live schema `properties`; the ADVERTISED tools/list schema remains the `*FacadeCall` schemars schema (decouple runtime raw-object acceptance from schema generation); register the module in `crates/mcp/src/lib.rs`
- [x] T004 [US1] Rewire the five facade handlers (`command_facade` tools.rs:2788, `session_facade` :2820, `files_facade` :2845, `registry_facade` :2869, `status_facade` :2894) in `crates/mcp/src/tools.rs` to accept the raw JSON object, run the validator, THEN deserialize into the existing `*FacadeCall` enums and dispatch unchanged
- [x] T005 [US1] Byte-identical + compat guards in `crates/mcp/src/tools.rs` / `crates/mcp/src/facade_strict.rs` test mods: `facade_valid_calls_are_byte_identical_after_validation`, `facade_samples_alias_still_accepted_on_suggest`, `facade_validator_derives_from_advertised_schema` (drift guard); verify the existing alias/lenient suites still pass (`parse_legacy_rules_json_string_still_works`, `tb12_*`, `registry_call_deserializes_sample_lines_and_alias`)
- [x] T006 [US1] Run affected MCP e2e suites (`crates/mcp/tests/compact_surface.rs`, `e2e.rs`, `mcp_stdio.rs`) and update any test pinned to the old first-missing-field message; then full verification gate on Windows + WSL

**Checkpoint**: US1 shippable alone — every facade teaches completely in one
error.

---

## Phase 4: User Story 2 - Registry lifecycle (Priority: P1)

**Goal**: identical pack re-import reports `skipped` and mints nothing; one
call deactivates a whole pack or a rule list with per-rule outcomes.
(FR-010/011; research D2/D3; contracts ipc-wire.md W2/W7, mcp-facade.md F3.)

**Independent Test**: import `cargo` pack twice -> second response
all-`skipped`, one version per rule in the store; then
`{action:"deactivate", pack:"cargo", scope:{kind:"global"}}` -> one call,
`registry_list_active` empty.

### Tests (write first, prove red)

- [x] T007 [P] [US2] Write failing store tests in `crates/store/src/import.rs` test mod: `reimport_identical_pack_skips_all_rules_and_creates_no_versions`, `reimport_with_two_changed_rules_imports_exactly_those_two`, `reimport_skip_ignores_version_and_status_differences`; record the red run (current code mints a new version per rule)
- [x] T008 [P] [US2] Write failing daemon integration test `import_pack_twice_reports_skipped_and_store_holds_one_version_per_rule` in `crates/daemon/tests/registry_ipc.rs`; record the red run

### Implementation — import idempotency

- [x] T009 [US2] Implement skip-on-identical in `import_parsed_pack` (`crates/store/src/import.rs:162-182`): fetch `get_latest_rule(id)`, normalize BOTH sides (`version`, `status` excluded per data-model.md E2), compare via derived `PartialEq`; identical -> `skipped` with NO `failed` entry (W7 semantics); confirm `registry_immutable_versions_no_mutation` (`crates/store/src/registry.rs:930`) still passes untouched

### Implementation — bulk deactivate (wire-first)

- [x] T010 [US2] Add `RegistryDeactivateBulk` request/response variants + `RegistryDeactivateBulkParams`/`RegistryDeactivateBulkResponse`/`BulkDeactivateOutcome`/`BulkOutcomeKind` per ipc-wire.md W2 (exact serde posture) in `crates/ipc/src/protocol.rs`, including the `is_idempotent` classification (mutating)
- [x] T011 [US2] Write failing daemon tests in NEW `crates/daemon/tests/registry_deactivate_bulk.rs` per quickstart US2 list (`deactivate_bulk_pack_scope_deactivates_all_members_in_one_call`, `deactivate_bulk_reports_per_rule_outcomes_with_unknown_named`, `deactivate_bulk_requires_exactly_one_selector`, `deactivate_bulk_rebinds_live_jobs_once`); harness pattern from `registry_ipc.rs`
- [x] T012 [US2] Implement `handle_registry_deactivate_bulk` in `crates/daemon/src/ipc/handlers/registry.rs` (exactly-one-selector validation; pack membership via `resolve_pack_json` `crates/store/src/import.rs:99`; per rule: `active_versions_for_scope` -> durable `deactivate_rule_scoped` -> in-memory `activation.deactivate`; ONE rebind pass after the loop; per-rule audit rows) and route the method in `crates/daemon/src/ipc/server.rs`
- [x] T013 [US2] MCP facade: add optional `rule_ids`/`pack` selectors to `McpRegistryDeactivateParams` in `crates/mcp/src/tools.rs` AND relax `rule_id` to `Option` in the schema (all three selectors optional at schema level; the exactly-one-of validator owns required-ness, `scope` stays schema-required); enforce exactly-one-of `{rule_id, rule_ids, pack}` (+ `version` only valid with `rule_id`) with a teaching error; route single -> `RegistryDeactivate` (unchanged), bulk -> `RegistryDeactivateBulk`; surface `outcomes` + `jobs_rebound` in the payload
- [x] T014 [US2] Update contract fixtures `crates/mcp/tests/fixtures/contracts/mcp-tools/registry_deactivate.v1.json` + `registry_import_pack.v1.json` + `mcp-tool-fixture-map.v1.json`; add MCP e2e `deactivate_pack_through_mcp_empties_active_list` in `crates/mcp/tests/registry_live_e2e.rs`; full gate Windows + WSL

**Checkpoint**: pack lifecycle costs one call each way; re-import is a no-op.

---

## Phase 5: User Story 3 - Directory listing (Priority: P1)

**Goal**: bounded, policy-gated single-level listing through the files
facade — file discovery on default-deny profiles. (FR-020/021; research D4;
contracts ipc-wire.md W1, mcp-facade.md F2.)

**Independent Test**: default-deny profile, `files {action:"list",
path:"E:/project"}` returns entries; listing a policy-denied path returns
the exact `file_read` denial shape.

### Implementation (wire-first; tests precede each layer)

- [x] T015 [US3] Add `FileListDir` request/response variants, `FileListDirParams`/`FileListDirResponse`/`DirEntry`/`DirEntryKind`, and constants `MAX_FILE_LIST_ENTRIES = 500` / `DEFAULT_FILE_LIST_ENTRIES = 200` per ipc-wire.md W1 in `crates/ipc/src/protocol.rs` (idempotent-read classification)
- [x] T016 [US3] Write failing daemon tests in `crates/daemon/tests/file_ipc.rs` per quickstart US3 list (`file_list_dir_returns_sorted_bounded_entries`, `file_list_dir_truncates_with_total_count_over_cap`, `file_list_dir_denies_policy_path_same_shape_as_read`, `file_list_dir_on_file_teaches_read_action`, `file_list_dir_rejects_relative_path`; `#[cfg(unix)]` only where symlink/POSIX proofs need it)
- [x] T017 [US3] Implement `handle_file_list_dir` in `crates/daemon/src/ipc/handlers/file.rs`: `resolve_and_authorize_file(state, path, false)` (common.rs:212) for the identical read gate; `std::fs::read_dir` + `symlink_metadata` (never follow); dirs-first lexicographic sort; clamp `.unwrap_or(DEFAULT).min(MAX)`; `total_entries` + `truncated`; stat races -> omit or partial entry; route method + dispatch-audit `ipc_file_list_dir` in `crates/daemon/src/ipc/server.rs`
- [x] T018 [US3] MCP facade: `FilesFacadeCall::List` arm in `crates/mcp/src/facades.rs`; `McpFileListDirParams` + `file_list_dir` handler + dispatch arm in `crates/mcp/src/tools.rs` (payload mirrors wire response per mcp-facade.md F2)
- [x] T019 [US3] Add NEW fixture `crates/mcp/tests/fixtures/contracts/mcp-tools/file_list.v1.json` + update `mcp-tool-fixture-map.v1.json`; check the `system_discover` fixture enumerates the new action; MCP e2e `file_list_dir_through_mcp_on_default_deny_profile` in `crates/mcp/tests/file_tools_live_e2e.rs`; full gate Windows + WSL

**Checkpoint**: P1 trio complete — TC-exclusive operation has discovery,
teaching errors, and a sane pack lifecycle.

---

## Phase 6: User Story 4 - Token-lean streaming (Priority: P2)

**Goal**: `compact` on `wait`/`events` (adapter-side projection reuse) and
liveness sent only on change on `sub_pull` (wire-opt-in delta).
(FR-030/031; research D5/D6; contracts ipc-wire.md W3, mcp-facade.md F4/F5.)

**Independent Test**: `wait {compact:true}` returns only
summary/stream/seq/severity per signal; two idle consecutive `sub_pull`s —
second carries no liveness; a running->exited transition appears in exactly
the next pull.

### Tests (write first, prove red)

- [ ] T020 [P] [US4] Write failing MCP compact tests (`bucket_wait_compact_projects_only_load_bearing_fields`, `bucket_events_compact_full_records_refetchable_by_cursor`, `compact_never_changes_which_events_match`) following the `crates/mcp/tests/ledger_compact_wait_restart.rs` pattern
- [ ] T021 [P] [US4] Write failing daemon delta tests in `crates/daemon/tests/subscription_pull_lossless.rs` per quickstart US4 list (`pull_delta_first_pull_sends_full_liveness_baseline`, `pull_delta_idle_second_pull_sends_no_liveness`, `pull_delta_transition_appears_in_exactly_next_pull`, `pull_delta_seek_resets_baseline_to_full_snapshot`, `pull_without_flag_sends_full_liveness_unchanged`)

### Implementation

- [ ] T022 [US4] MCP compact: add `compact: bool #[serde(default)]` to `McpBucketWaitParams` (tools.rs:4509) and `McpBucketEventsSinceParams` (tools.rs:4472); project through the EXISTING `project_signal_compact` (tools.rs:3254) in `bucket_wait` (:1629) and `bucket_events_since` (:1612); echo `"compact": true` in the payload — all in `crates/mcp/src/tools.rs`, zero daemon change (research D5)
- [ ] T023 [US4] Wire: add `liveness_delta: bool #[serde(default)]` to `SubscriptionPullParams` per ipc-wire.md W3 in `crates/ipc/src/protocol.rs`
- [ ] T024 [US4] Daemon delta: add `last_liveness: HashMap<BucketId, Liveness>` to `Subscription` in `crates/daemon/src/subscriptions/model.rs`; diff full snapshot vs map and commit the new snapshot at the same point pull offsets advance in `crates/daemon/src/subscriptions/pull.rs` + `crates/daemon/src/ipc/handlers/subscription.rs`; clear the map in `handle_subscription_seek` (subscription.rs:224-257)
- [ ] T025 [US4] MCP `sub_pull` (tools.rs:2639): always send `liveness_delta: true`; omit the `liveness` payload section when the delta is empty; update `crates/mcp/tests/mcp_subscription_notify.rs` / `mcp_subscriptions_e2e.rs` expectations if pinned to full liveness
- [ ] T026 [US4] SC-004 evidence: replay the findings-doc repro shapes (quiet long build via compact `wait` + delta `sub_pull`), record before/after serialized response byte counts (target >= 60% reduction) in `specs/002-dogfood-remediation/evidence-sc004.md`; update subscription_pull + bucket wait/events fixtures under `crates/mcp/tests/fixtures/contracts/mcp-tools/`; full gate Windows + WSL

**Checkpoint**: streaming surfaces stop leaking tokens; wire stays
compatible for non-adapter clients.

---

## Phase 7: User Story 5 - Fewer round-trips (Priority: P2)

**Goal**: `event_context` by `event_id` alone; `pty_stdin` with bounded
`wait_ms` returning the combed signals the write provoked. (FR-040/041;
research D7/D8; contracts ipc-wire.md W4/W5, mcp-facade.md F6/F7.)

**Independent Test**: `event_context {event_id:"evt_..."}` returns the
window; node REPL `pty_stdin {bytes:"1+1\n", wait_ms:3000}` returns echo +
result signals in one call.

### Tests (write first, prove red)

- [ ] T027 [P] [US5] Write failing daemon tests in `crates/daemon/tests/ipc_bucket.rs` (`event_context_resolves_by_event_id_alone`, `event_context_mismatched_bucket_id_is_error_not_ignored`, `event_context_unknown_event_errors_identically_in_both_modes`, `event_context_by_id_after_bucket_eviction_is_honest_not_found`) next to `event_context_unknown_event_returns_typed_error`
- [ ] T028 [P] [US5] Write failing daemon tests in `crates/daemon/tests/pty_ipc.rs` (`pty_stdin_wait_ms_returns_combed_signals_with_cursor`, `pty_stdin_without_wait_is_byte_identical_to_today`, `pty_stdin_secret_prompt_denial_unchanged_by_wait`)

### Implementation (wire-first)

- [ ] T029 [US5] Wire: `EventContextParams.bucket_id` -> `Option<BucketId>` (W4) and `PtyCommandWriteStdinParams` + `cursor`/`wait_ms`, response + optional settle fields (W5), exact serde posture, in `crates/ipc/src/protocol.rs`
- [ ] T030 [US5] Daemon event_context: in `handle_event_context` (`crates/daemon/src/ipc/handlers/bucket.rs:103-241`) keep the supplied-bucket path byte-identical; when `bucket_id` absent iterate bucket ids with the same bounded page scan; not-found-anywhere -> `EventNotFound` without a bucket name
- [ ] T031 [US5] Daemon pty settle: in the pty stdin handler + `crates/daemon/src/pty_command.rs`, after the write (secret-prompt denial untouched, BEFORE write) run the same bounded settle-window bucket read `ShellSessionExec` uses from `cursor` (default 0), daemon-clamped; populate the optional response fields only when `wait_ms` present
- [ ] T032 [US5] MCP: `McpEventContextParams.bucket_id` -> `Option<String>` (tools.rs:4558, `into_ipc` :4576); `McpPtyCommandWriteStdinParams` + `cursor`/`wait_ms` (tools.rs:5118) and combed-batch payload fields in `pty_command_write_stdin` (:2240) in `crates/mcp/src/tools.rs`
- [ ] T033 [US5] Update fixtures (`event_context`, pty stdin) under `crates/mcp/tests/fixtures/contracts/mcp-tools/` + map; MCP e2e one-call REPL test in `crates/mcp/tests/pty_tools_live_e2e.rs`; full gate Windows + WSL

**Checkpoint**: one REPL interaction = one call; event inspection needs no
bucket ceremony.

---

## Phase 8: User Story 8 - WSL nested-shell gate (Priority: P2)

**Goal**: the shell capability follows the shell across the WSL boundary —
nested interpreters denied under `allow_shell=false`, fail-closed on unknown
constructions, non-shell WSL usage untouched. (FR-060/061; research D10;
contract policy-wsl.md — the full classification table is normative.)

**Independent Test**: default profile: `["wsl.exe","-e","bash","-lc","..."]`
-> teaching denial; `["wsl.exe","-e","cargo","build"]` and
`["wsl.exe","--list","--verbose"]` -> run; with `allow_shell=true` the
nested form runs and the audit row carries the classification.

### Tests (write first, prove red — cross-platform pure-argv logic)

- [ ] T034 [US8] Write failing tests per quickstart US8 list in `crates/daemon/tests/command_runtime.rs` (`wsl_nested_shell_denied_under_allow_shell_false`, `wsl_nested_shell_all_spellings_classified_identically`, `wsl_exec_introduced_non_shell_payload_and_management_flags_run_unchanged`, `wsl_bare_payload_without_exec_is_shell_interpreted_and_denied`, `wsl_tilde_shorthand_is_selector_not_payload`, `wsl_unknown_construction_fails_closed`, `wsl_bare_invocation_is_default_shell_and_denied`), `crates/daemon/tests/ipc_command.rs` (`wsl_nested_shell_allowed_and_audit_tagged_under_allow_shell_true`), `crates/daemon/tests/pty_ipc.rs` (`pty_wsl_nested_shell_denied_like_argv_lane`); spellings matrix from policy-wsl.md (incl. the no-`-e` shell-interpretation rows and `~`); record the red run (today `wsl.exe -e bash` passes)

### Implementation

- [ ] T035 [US8] Implement `WslArgvClass` + `classify_wsl_nested_shell(argv: &[String])` in `crates/daemon/src/command.rs` next to `shell_interpreter_basename` (:500-522), exactly per policy-wsl.md steps 1-4 (carrier detection incl. absolute paths; management-flag list; selector skipping; payload vs `SHELL_INTERPRETERS_DENY`; empty payload = default shell; unknown flag = `UnknownConstruction`)
- [ ] T036 [US8] Enforce at both lanes: argv guard block (`crates/daemon/src/command.rs:732-746`) and PTY lane (`crates/daemon/src/pty_command.rs:267`); deny path reuses `IpcErrorCode::ShellInterpreterDenied` with the extended carrier-aware teaching message (policy-wsl.md enforcement contract) + existing `command_rejected` audit row; allow path adds `"nested_shell"` / `"wsl_construction"` to audit `metadata_json` and notes it in `reason`
- [ ] T037 [US8] FR-061: document the stance (argv-only inspection, fail-closed rule, enforcement matrix, boundary rationale) in `docs/security/POLICY.md` alongside the `SHELL_INTERPRETERS_DENY` section
- [ ] T038 [US8] Full gate Windows + WSL, PLUS `cargo nextest run -p terminal-commanderd --test security` (Constitution: policy work runs the security profile); live Windows proof of the Independent Test transcript

**Checkpoint**: zero shell interpreters reachable through the argv lane,
WSL carriers included (SC-006).

---

## Phase 9: User Story 6 - File append (Priority: P2)

**Goal**: policy-gated, bounded, atomic append on `file_write`. (FR-022;
research D4-adjacent; contracts ipc-wire.md W6, mcp-facade.md F8.)

**Independent Test**: append one line to an allowed file -> prior bytes
intact + line at end + `bytes_written` = appended bytes; append to a denied
path -> the write-policy denial.

### Implementation (wire-first; tests precede the handler)

- [ ] T039 [US6] Wire: `append: bool #[serde(default)]` on `FileWriteParams` per ipc-wire.md W6 in `crates/ipc/src/protocol.rs`
- [ ] T040 [US6] Write failing daemon tests in `crates/daemon/tests/file_ipc.rs` per quickstart US6 list (`file_write_append_preserves_prefix_and_reports_bytes_appended`, `file_write_append_missing_file_creates_it`, `file_write_append_denied_path_same_policy_error_as_write`, `file_write_append_oversize_bounded_error`)
- [ ] T041 [US6] Implement the append branch in `handle_file_write` (`crates/daemon/src/ipc/handlers/file.rs:289`): same size-cap-first + `resolve_and_authorize_file_write` gate; append mode = open append + single `write_all` + `sync_all` (OS append-offset atomicity serializes racing appenders per spec edge case); domain audit metadata gains `"append": true`
- [ ] T042 [US6] MCP: `append` field on `McpFileWriteParams` (tools.rs:4986) in `crates/mcp/src/tools.rs`; update `file_write.v1.json` fixture + map; full gate Windows + WSL

**Checkpoint**: log-like workflows stop rewriting whole files.

---

## Phase 10: User Story 7 - Suggest heuristics (Priority: P3)

**Goal**: `suggest_from_samples` proposes draft rules for `npm ERR!` and
`error TS\d+:` shapes; never activates, no stream filter without evidence.
(FR-050; research D9.)

**Independent Test**: feed the six-line npm/tsc sample from the findings doc
-> proposals covering both shapes; `registry_test` passes them; nothing
persists or activates.

### Tests (write first, prove red)

- [ ] T043 [US7] Write failing sifter tests in `crates/sifters/src/suggest.rs` test mod (`detects_npm_err_prefix`, `detects_ts_error_code`, `new_heuristics_set_no_stream_filter_without_evidence`); record the red run (six-line sample currently yields only `error-prefix`)

### Implementation

- [ ] T044 [US7] In `crates/sifters/src/suggest.rs`: change `Heuristic.stream` to `Option<StreamKind>` (existing six keep `Some(...)` — zero behavior change; `build_proposal` :262 propagates the Option); insert `npm-error` (`^npm ERR! (?P<message>.+)$`) and `ts-error` (`^error TS(?P<code>[0-9]+): (?P<message>.+)$`) after `coded-error`, before `error-prefix`, both `stream: None` (research D9)
- [ ] T045 [US7] Verify the existing invariants stay green (`every_proposed_rule_is_draft_and_validates`, `one_proposal_per_heuristic_not_per_line`, `proposed_patterns_compile_under_bounded_regex`) and the e2e guard `crates/mcp/tests/suggest_loop_e2e.rs`; live check: findings-doc sample -> both proposals survive `registry_test` (SC-007); full gate Windows + WSL

**Checkpoint**: mainstream JS/TS output is recognized out of the box.

---

## Phase 11: User Story 9 - Pipe instance pool (Priority: P3, OPTIONAL)

**Goal**: N>1 pending named-pipe instances shrink the accept/recreate gap at
the source (client retry from 0.1.72 stays as backstop). (FR-070; research
D11.) **Skipping this story with a written rationale is a compliant
outcome.**

**Independent Test**: synthetic connect storm — N concurrent first connects
all succeed without entering the client retry loop; clean shutdown leaves no
orphaned instances.

- [ ] T046 [US9] Decision checkpoint: implement or skip. If skipping, write the rationale into `specs/002-dogfood-remediation/evidence-us9.md` (why, what the client-side retry already covers, what would trigger revisiting) and mark T047-T048 as void
- [ ] T047 [US9] Restructure `accept_loop` in `crates/daemon/src/ipc/pipe_server.rs:170-306`: fixed pool of N=4 pending instances (accept futures in a `JoinSet`; `first_pipe_instance(true)` only on the very first instance ever; replace each accepted instance immediately); shutdown cancels the idle pending accepts then drains in-flight handlers under the existing `PIPE_DRAIN_CEILING`; per-connection identity/policy/audit untouched (research D11)
- [ ] T048 [US9] Windows-gated tests in `crates/daemon/tests/pipe_accept_loop.rs` + `pipe_shutdown_drain.rs` (`n_concurrent_first_connects_all_succeed_without_retry`, `pool_per_connection_identity_and_policy_unchanged`, `shutdown_closes_all_pending_instances_cleanly`) plus a connect-storm retry-rate comparison vs the single-instance baseline recorded in `specs/002-dogfood-remediation/evidence-us9.md`; full gate Windows + WSL (Linux gate proves the cfg-gated code still compiles clean)

**Checkpoint**: all nine stories resolved (implemented or compliantly
skipped).

---

## Phase 12: Polish & Cross-Cutting

**Purpose**: batch-wide consistency, honest evidence, and the finishing
checklist.

- [ ] T049 [P] Fixture + discovery sweep: every touched tool fixture under `crates/mcp/tests/fixtures/contracts/mcp-tools/` consistent with shipped schemas; `mcp-tool-fixture-map.v1.json` complete; `system_discover` fixture enumerates all new actions/fields; facade description constants in sync (`facade_consts_match_tool_attribute_descriptions`)
- [ ] T050 [P] Update `BACKLOG.md` (mark the ten dogfood improvement items resolved with commit refs; P1.0g resolution note) and `docs/dogfood/2026-07-02-tc-0.1.70-dogfood-findings.md` (status column per finding)
- [ ] T051 Final full verification gate on Windows AND WSL (fmt, clippy `-D warnings`, nextest workspace) + `-p terminal-commanderd --test security`; compare against `baseline.md` — zero new failures
- [ ] T052 Live dogfood validation pass: drive every story's Independent Test through the running daemon + MCP facades as the agent-user (quickstart per-story transcripts); write the evidence report per quickstart's report shape into `specs/002-dogfood-remediation/evidence-report.md` — honest per-SC status (SC-001..SC-008), mock/blocked/unverified named as such

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: none — start immediately.
- **Foundational (Phase 2)**: empty — stories are independent by design.
- **User Stories (Phases 3-11)**: each depends only on Phase 1. Priority
  order for sequential delivery: US1 -> US2 -> US3 (P1), then US4 -> US5 ->
  US8 -> US6 (P2), then US7 -> US9 (P3). US9 ships LAST regardless (it
  touches the accept path every other story's tests ride on).
- **Polish (Phase 12)**: after all desired stories.

### Cross-story notes (independence caveats)

- No story depends on another's code. US1's validator derives
  required/allowed fields from the live schemars schema, so fields added by
  US2-US6 are picked up automatically regardless of landing order.
- SAME-FILE CONTENTION (not logical dependency): `crates/ipc/src/protocol.rs`
  is touched by US2/US3/US4/US5/US6, and `crates/mcp/src/tools.rs` by almost
  every story. On a single branch, execute stories sequentially (the default
  here). If parallelizing across agents, isolate per-story worktrees and
  merge in priority order.
- Each story phase ends with the both-platform gate; per project rules a
  phase touches at most ~5 files and verifies before the next begins.

### Within Each User Story

- Red tests FIRST (record the failing run — SC-008 proof), then wire types
  (protocol.rs), then daemon handler, then MCP layer, then contract
  fixtures, then the both-platform gate.

### Parallel Opportunities

- T007 + T008 (US2 red tests: store vs daemon files) in parallel.
- T020 + T021 (US4 red tests: MCP vs daemon files) in parallel.
- T027 + T028 (US5 red tests: bucket vs pty files) in parallel.
- T049 + T050 (polish: fixtures vs docs) in parallel.
- Whole stories can fan out to parallel agents ONLY with per-story worktrees
  (see contention note); compile-heavy agents capped at ~2 concurrent.

---

## Parallel Example: User Story 2

```text
# The two red-test tasks touch different crates and can run together:
Task T007: failing store tests in crates/store/src/import.rs test mod
Task T008: failing daemon test in crates/daemon/tests/registry_ipc.rs

# Then strictly sequential (shared files / layering):
T009 (store fix) -> T010 (protocol.rs) -> T011 (bulk red tests)
  -> T012 (daemon handler) -> T013 (MCP) -> T014 (fixtures + gate)
```

---

## Implementation Strategy

### MVP First (User Story 1)

1. Phase 1 baseline, then Phase 3 (US1) alone.
2. Validate: the two dogfood repro calls teach completely in one error;
   valid calls byte-identical; both gates green.
3. US1 is the highest-leverage single story — every later story's errors
   become self-teaching once it lands.

### Incremental Delivery

Each story phase is a shippable increment ending in a green both-platform
gate: P1 trio first (strictness, registry lifecycle, discovery — the
observed-cost leaders), then P2 (token economy, round-trips, WSL boundary,
append), then P3 (heuristics, optional pool). Stop at any checkpoint;
nothing later breaks anything earlier (additive-only wire evolution).

### Single-agent default, parallel option

Default: one agent, sequential phases, commit per task or logical group
(Conventional Commits, no push without explicit human approval). Parallel
variant: per-story worktrees for US4/US5/US6 after P1 lands, merged in
priority order by an integrator — respect the ~2 concurrent compile-heavy
agent cap.

---

## Notes

- [P] = different files, no dependency on an incomplete task.
- Every gate = BOTH platforms; cfg-gated code (US8/US9) has burned the Linux
  clippy gate twice this cycle — run WSL before every push.
- Constitution v1.0.0 binds every task: adapter spawns nothing, default-deny
  intact, bounded output, audit gated actions, no-mock production paths,
  honest degradation. NON-NEGOTIABLE principles are not waivable.
- Out of scope (spec assumptions): P1.0a-P1.0e campaign items, omni review
  branches, any wire-protocol breaking change. Finding yourself editing
  those areas means you have left this feature's scope.
