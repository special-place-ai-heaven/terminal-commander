# Spec 002 Dogfood Remediation — Final Evidence Report

**Date**: 2026-07-03
**Branch**: `002-dogfood-remediation` (not pushed — no mandate)
**Final gate**: Windows 868/868 nextest (1 leaky, 1 skipped) + clean
fmt/clippy; WSL 1139/1139 nextest (1 skipped) + clean fmt/clippy; daemon
security profile 11/11. Zero new failures vs the T001 baseline.

## Story disposition

| Story | FRs | State | Merge |
|---|---|---|---|
| US1 facade strictness | 001/002/003 | done | d891f46 |
| US2 registry lifecycle | 010/011 | done | 2290b4d |
| US3 directory listing | 020/021 | done | 55d8055 |
| US4 token-lean streaming | 030/031 | done | 2163fe8 |
| US5 fewer round-trips | 040/041 | done | 012d6d0 |
| US6 file append | 022 | done | a9aef11 |
| US7 suggest heuristics | 050 | done | f5bd864 |
| US8 WSL nested-shell gate | 060/061 | done | ccc8ee0 |
| US9 pipe-instance pool | 070 | SKIPPED (rationale) | evidence-us9.md |

## Success criteria (SC-008: red->green per FR, both platforms)

Every criterion below is proven by AUTOMATED tests that drive a REAL
daemon over REAL IPC with the REAL policy engine (in-process integration
+ live-daemon e2e), not by code reading. What is NOT yet done is a pass
driving the tools through the INSTALLED stdio MCP adapter as the
agent-user — see "Deferred" below.

- **SC-001** (learn any call shape in <=1 failed call; was 3 for
  deactivate): `facade_missing_fields_are_reported_all_at_once_with_action`,
  `facade_unknown_field_is_rejected_with_counterpart_remedy`,
  `facade_sub_pull_wait_ms_names_timeout_ms_as_remedy`,
  `facade_unknown_action_lists_valid_actions`. **MET.**
- **SC-002** (re-import creates zero new rows; pack deactivate is one call):
  `reimport_identical_pack_skips_all_rules_and_creates_no_versions`,
  `import_pack_twice_reports_skipped_and_store_holds_one_version_per_rule`,
  `deactivate_bulk_pack_scope_deactivates_all_members_in_one_call`. **MET.**
- **SC-003** (default-deny profile enumerates a directory in one call; was
  impossible on Windows): `file_list_dir_*` (daemon) +
  `file_list_dir_through_mcp_on_default_deny_profile` (live-daemon e2e).
  **MET.**
- **SC-004** (>=60% fewer response tokens): measured **84.7%**
  (before=11791, after=1799 bytes) in `sc004_token_lean_bytes.rs`,
  asserted >=60% so it cannot silently regress. See `evidence-sc004.md`.
  **MET, exceeded.**
- **SC-005** (one REPL interaction = one call; was two):
  `pty_stdin_wait_ms_returns_combed_signals_with_cursor` +
  `pty_stdin_wait_ms_returns_signals_in_one_call_through_mcp` (e2e).
  **MET.**
- **SC-006** (allow_shell=false -> zero shell interpreters reachable via
  argv incl. WSL carriers; non-shell WSL unchanged): the nine `wsl_*`
  classifier tests across command_runtime/ipc_command/pty_ipc + daemon
  security profile 11/11. **MET.**
- **SC-007** (six-line JS/TS sample yields npm ERR! + TS proposals; was
  0/2): `detects_npm_err_prefix`, `detects_ts_error_code`,
  `new_heuristics_set_no_stream_filter_without_evidence`. **MET.**
- **SC-008** (all existing tests green both platforms; every FR has a
  red->green test): baseline all-green (`baseline.md`); per-wave evidence
  (`evidence-wave1.md`, `evidence-wave2.md`); final gate green both
  platforms. Each story's agent report records the failing pre-change run
  and the passing post-change run per FR. **MET.**

## Honesty contracts preserved (constitution)

- Adapter spawns nothing (`mcp_crate_contains_no_command_spawn` green).
- Default-deny intact; US8 CLOSES an argv-smuggling gap and defaults
  nothing open; directory listing + append reuse the existing FileRead /
  FileWrite policy actions.
- Bounded + truthful: listing caps + `truncated`; compact/delta reduce
  tokens without hiding data (full records cursor-refetchable); append
  contract is honest (no interleave; partial-on-I/O-failure is an error,
  not a false all-or-nothing).
- Suggest never auto-activates (stateless handler, untouched).
- Tool COUNT unchanged (`fixture_map_matches_live_tool_catalogue`,
  `all_facade_action_enums_are_exact`, `facade_validator_derives_from_advertised_schema`
  all green).

## Deferred (honest gap)

**Live agent-perspective pass through the INSTALLED MCP adapter is NOT
done in this session.** The running MCP is the installed v0.1.72, which
does not contain these changes; driving the actual
`terminal-commander` MCP tools against the new behavior requires building
and installing this (unpushed) branch, which bounces the running MCP
connection. Per the established workflow the operator performs the
install + reconnect. That post-install session — driving every story's
Independent Test as the agent-user — is the natural next dogfood round
and the seed for the follow-up spec. The automated evidence above is the
strongest verification achievable without that install; it is real
daemon + real IPC, not code reading, but it is not the installed-adapter
path.

## Not pushed

Branch `002-dogfood-remediation` is committed locally only. No push,
no release, no publish — awaiting explicit operator mandate.
