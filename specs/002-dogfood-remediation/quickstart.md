# Quickstart: Validating the Dogfood Remediation Batch

**Feature**: `specs/002-dogfood-remediation` | validation guide only —
implementation detail lives in [research.md](./research.md) and
[contracts/](./contracts/).

## Prerequisites

- Windows 11 host with the repo at `E:\project\terminal-commander`, and a
  WSL Ubuntu with the Rust toolchain (the Linux gate runs there).
- `cargo nextest` installed on both sides.
- For live end-to-end proof: the freshly built daemon + MCP adapter
  (`cargo build --workspace`), driven through the MCP facades — the same
  way the dogfood rounds were run.

## The verification gate (Constitution VI — every story, both platforms)

```bash
# Windows (repo root)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace

# Linux gate (inside WSL; gate-exact, pre-push)
CARGO_TARGET_DIR=$HOME/tc-linux-target ~/.cargo/bin/cargo fmt --all --check
CARGO_TARGET_DIR=$HOME/tc-linux-target ~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR=$HOME/tc-linux-target ~/.cargo/bin/cargo nextest run --workspace
```

cfg-gated code (US8, US9) has produced Linux-gate clippy failures twice this
cycle — run the WSL gate BEFORE every push, not after CI complains.

## Red -> green discipline (SC-008)

Every FR lands with at least one test that FAILS on pre-change behavior.
Prove it: write the test, run it against unmodified code (or `git stash`
the fix), record the failure, then show it passing. Suggested test names
below follow the house `behavior_condition_expectation` convention; homes
are the files each area's existing tests live in.

## Per-story validation

### US1 — facade strictness (P1)

Unit (in `crates/mcp/src/tools.rs` / `facades.rs` test mods):

```text
facade_missing_fields_are_reported_all_at_once_with_action
facade_unknown_field_is_rejected_with_counterpart_remedy
facade_sub_pull_wait_ms_names_timeout_ms_as_remedy
facade_valid_calls_are_byte_identical_after_validation
facade_samples_alias_still_accepted_on_suggest
facade_unknown_action_lists_valid_actions
facade_validator_derives_from_advertised_schema   // drift guard
```

Live (agent-perspective): call `registry` `{action:"deactivate",
rule_id:"cargo.compile-error"}` -> ONE error naming `scope`; call `command`
`{action:"sub_pull", sub_id:..., wait_ms:30000}` -> error naming `wait_ms`
unknown + `timeout_ms` remedy. A correct call immediately after must behave
exactly as before the change.

### US2 — import idempotency + bulk deactivate (P1)

Store unit (`crates/store/src/import.rs` mod tests):

```text
reimport_identical_pack_skips_all_rules_and_creates_no_versions
reimport_with_two_changed_rules_imports_exactly_those_two
reimport_skip_ignores_version_and_status_differences
```

Daemon integration (`crates/daemon/tests/registry_ipc.rs` +
new `registry_deactivate_bulk.rs`):

```text
import_pack_twice_reports_skipped_and_store_holds_one_version_per_rule
deactivate_bulk_pack_scope_deactivates_all_members_in_one_call
deactivate_bulk_reports_per_rule_outcomes_with_unknown_named
deactivate_bulk_requires_exactly_one_selector
deactivate_bulk_rebinds_live_jobs_once
```

Live: `registry import_pack cargo` twice -> second response all-`skipped`,
`registry_list_active` unchanged; then `{action:"deactivate", pack:"cargo",
scope:{kind:"global"}}` -> one call, list_active empty.

### US3 — directory listing (P1)

Daemon integration (`crates/daemon/tests/file_ipc.rs`):

```text
file_list_dir_returns_sorted_bounded_entries
file_list_dir_truncates_with_total_count_over_cap
file_list_dir_denies_policy_path_same_shape_as_read
file_list_dir_on_file_teaches_read_action
file_list_dir_rejects_relative_path
```

(`#[cfg(unix)]` where the proof needs symlinks/POSIX paths, mirroring the
existing split.) Live: on a default-deny profile, `files
{action:"list", path:"E:/project"}` returns entries — the exact operation
that was impossible in the dogfood round.

### US4 — compact wait/events + liveness delta (P2)

MCP (`crates/mcp` unit + `ledger_compact_wait_restart.rs` pattern):

```text
bucket_wait_compact_projects_only_load_bearing_fields
bucket_events_compact_full_records_refetchable_by_cursor
compact_never_changes_which_events_match
```

Daemon (`crates/daemon/tests/subscription_pull_lossless.rs` +
`subscription_ipc.rs`):

```text
pull_delta_first_pull_sends_full_liveness_baseline
pull_delta_idle_second_pull_sends_no_liveness
pull_delta_transition_appears_in_exactly_next_pull
pull_delta_seek_resets_baseline_to_full_snapshot
pull_without_flag_sends_full_liveness_unchanged   // wire-compat guard
```

SC-004 measurement: replay the findings-doc repro shapes (quiet long build:
compact `wait` + delta `sub_pull`) and compare serialized response bytes
against the pre-change shapes; the target is >= 60% reduction. Record the
two byte counts in the evidence report.

### US5 — event_context by id + pty_stdin wait (P2)

Daemon (`crates/daemon/tests/ipc_bucket.rs`, `pty_ipc.rs`):

```text
event_context_resolves_by_event_id_alone
event_context_mismatched_bucket_id_is_error_not_ignored
event_context_unknown_event_errors_identically_in_both_modes
event_context_by_id_after_bucket_eviction_is_honest_not_found
pty_stdin_wait_ms_returns_combed_signals_with_cursor
pty_stdin_without_wait_is_byte_identical_to_today
pty_stdin_secret_prompt_denial_unchanged_by_wait
```

Live: node REPL via PTY — `pty_stdin {bytes:"1+1\n", wait_ms:3000}` returns
the echo + `2` signals in ONE call (was two).

### US6 — file append (P2)

Daemon (`crates/daemon/tests/file_ipc.rs`):

```text
file_write_append_preserves_prefix_and_reports_bytes_appended
file_write_append_missing_file_creates_it
file_write_append_denied_path_same_policy_error_as_write
file_write_append_oversize_bounded_error
```

### US7 — suggest heuristics (P3)

Sifters unit (`crates/sifters/src/suggest.rs` mod tests):

```text
detects_npm_err_prefix
detects_ts_error_code
new_heuristics_set_no_stream_filter_without_evidence
proposed_patterns_compile_under_bounded_regex     // existing, must stay green
every_proposed_rule_is_draft_and_validates        // existing, must stay green
```

Live: feed the six-line npm/tsc sample from the findings doc -> proposals
covering both shapes; `registry_test` passes them; nothing activates.

### US8 — WSL nested-shell gate (P2)

Daemon (`crates/daemon/tests/command_runtime.rs`, `ipc_command.rs`,
`pty_ipc.rs`):

```text
wsl_nested_shell_denied_under_allow_shell_false
wsl_nested_shell_all_spellings_classified_identically
wsl_exec_introduced_non_shell_payload_and_management_flags_run_unchanged
wsl_bare_payload_without_exec_is_shell_interpreted_and_denied
wsl_tilde_shorthand_is_selector_not_payload
wsl_unknown_construction_fails_closed
wsl_bare_invocation_is_default_shell_and_denied
wsl_nested_shell_allowed_and_audit_tagged_under_allow_shell_true
pty_wsl_nested_shell_denied_like_argv_lane
```

The classifier is pure argv logic — these tests are cross-platform (no WSL
needed to run them); the live Windows proof is `command start` with
`["wsl.exe","-e","bash","-lc","echo hi"]` on a default profile -> teaching
denial, and `["wsl.exe","-e","uname","-a"]` -> runs. FR-061: confirm
`docs/security/POLICY.md` carries the stance.

### US9 — pipe instance pool (P3, optional)

Windows-gated (`crates/daemon/tests/pipe_accept_loop.rs` +
`pipe_shutdown_drain.rs`):

```text
n_concurrent_first_connects_all_succeed_without_retry
pool_per_connection_identity_and_policy_unchanged
shutdown_closes_all_pending_instances_cleanly
```

Plus a synthetic connect storm comparing retry-rate against the
single-instance baseline. SKIPPING this story with a written rationale in
the evidence report is a compliant outcome.

## Cross-cutting checks (every story)

- Contract fixtures: `cargo nextest run -p terminal-commander-mcp` covers
  the fixture-map tests; update `crates/mcp/tests/fixtures/contracts/` in
  the same change as any schema change.
- Security posture: `cargo nextest run -p terminal-commanderd --test
  security` (grep-tests: adapter spawns nothing, no TCP listener).
- Wire compat: old-shape requests (no new fields) against the new daemon
  decode and behave byte-identically — the `*_unchanged` /
  `*_byte_identical_*` tests above are the proof set.

## Evidence report shape (per TESTING.md section 10)

```text
Objective:      <user story / FRs>
Changes:        <what + why>
Files changed:  <list>
Verification:   <per-command PASS/FAIL, both platforms>
Evidence:       <red->green proof, live transcript excerpts, SC measurements>
Known gaps:     <honest list; "none" only if true>
```
