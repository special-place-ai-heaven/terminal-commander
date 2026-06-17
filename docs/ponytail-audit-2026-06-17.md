# Ponytail Audit — terminal-commander

**Date:** 2026-06-17
**Scope:** repo-wide (`crates/**/*.rs`, ~51.5k source lines across 8 crates)
**Method:** 5 parallel read-only auditors, one per crate group. Complexity only —
no correctness bugs, security holes, or performance findings (those go to a normal
review pass). Lists findings; applies nothing.

Tags: `delete:` dead/speculative · `stdlib:` hand-rolled std ships · `native:`
dep/code the platform already does · `yagni:` abstraction with one caller ·
`shrink:` same logic, fewer lines.

---

## Findings — ranked, biggest cut first

| # | Tag | What to cut | Replacement | Location | ~Lines |
|---|-----|-------------|-------------|----------|--------|
| 1 | shrink | ~46 MCP tool handlers repeat the `ensure_daemon → daemon.call → match{Ok/Ok(other)/Err}` envelope verbatim | one `call_daemon_tool(daemon, req, map_resp)` helper | `crates/mcp/src/tools.rs:1000-2500` | 150 |
| 2 | shrink | `run_and_watch` hand-rolls a C-style state machine (`last_observed_state`/`exit_code`/`receipt` + manual assigns) | Option combinators / small struct | `crates/mcp/src/tools.rs:1160-1313` | 150 |
| 3 | yagni | 17 serde default-callback fns each wrap one const or `::default()` | `#[serde(default)]` + `#[derive(Default)]` | `crates/daemon/src/config.rs:131-337` | 60 |
| 4 | yagni | 3x duplicate `SuppressionCounter` impls across Process/File/Pty metrics — verbatim | macro or generic impl | `crates/probes/src/noise_pipeline.rs:41-102` | 62 |
| 5 | delete | 3 byte-identical `EventSink` impls (`emit` + `patch_dedupe_aggregate`) | one generic `RouterEventSink<M>` | `daemon/src/file_watch.rs:124`, `pty_command.rs:119`, `command.rs:235` | 50 |
| 6 | delete | 7 non-unix `session.rs` stub handlers each re-inline the same error | call existing `session_ipc_unsupported()` (L27) | `crates/daemon/src/ipc/handlers/session.rs:27-89` | 50 |
| 7 | delete | `[policy.paths]` + `[policy.probes]` config — parsed "for forward-compat," zero enforcement, no landing phase | delete structs, re-add when phase exists | `daemon/src/config.rs:264-286`, `policy.rs:26-27` | 40 |
| 8 | yagni | `regex_rule()` factory — 4 callers in the same fn, every "varying" arg hardcoded | inline, kill factory | `crates/sifters/src/universal.rs:91-126` | 35 |
| 9 | shrink | `emit_audit` hand-marshals peer-identity JSON 3x (Unix/Windows/Unknown) via `format!` | `peer.to_metadata_json()` or serde | `daemon/src/ipc/handlers/common.rs:16-58` | 25 |
| 10 | yagni | Duplicate read-limit/TTL/max-event constants in two crates (same values, divergent names + types: `usize` 10k vs `u64` 100k) | single source in core, re-export | `core/bucket.rs:50-61`, `store/lib.rs:68-77` | 15 |
| 11 | yagni | `ProbeNoisePipeline::new(policy)` dead — only `with_default_policy()` is called (5 sites) | delete `new(policy)`, rename | `probes/src/noise_pipeline.rs:112-122` | 14 |
| 12 | yagni | Reserved zero-defaulted `noise_suppressed_count`/`dedupe_collapsed_count` on `BucketSummary`/`BucketWaitResponse` "awaiting TC11" — bloat every read unused | defer | `core/bucket.rs:125-130, 278` | 10 |
| 13 | yagni | 4 `pub fn new(){ Self::default() }` delegate-only ctors | `#[derive(Default)]`, drop them | `probes/process.rs:121`, `directory.rs:83`, `pty.rs:78`, `sifters/noise.rs:105` | 8 |
| 14 | delete | Unreachable `Cmd::SelfcheckNoop` match arm (short-circuited at L107; comment admits dead) | `unreachable!()` | `daemon/src/main.rs:173-177` | 5 |
| 15 | yagni | `profile_version` ("TC36-era") parsed, never read outside tests | delete field + `default_profile_version()` | `daemon/src/config.rs:198-199` | 5 |
| 16 | yagni | `EnvironmentSpec::SshHost` "reserved, not implemented in M1" | delete or `#[cfg]`-gate | `core/src/environment.rs:17-18` | 5 |
| 17 | delete | `state_of()` "readability alias" for `pty_state()` — but `pty_state` is called directly in the same impl (L497, L531) | drop alias | `daemon/src/shell_session.rs:195-197` | 3 |
| 18 | delete | `should_replace(stale,force) => stale\|\|force`, one call site | inline | `supervisor/src/replace.rs:50-52` | 3 |

**net: ~700 lines, 0 deps possible.**

---

## Notes

**Deps are clean.** No hand-rolled code duplicates a crate already in any
`Cargo.toml`, and no dependency is removable. Every cut above is internal
dedup or dead code.

**Highest leverage is one file.** Findings #1 and #2 (~300 lines) both live in
`crates/mcp/src/tools.rs` — the single largest file in the repo (6271 lines) —
and together are ~40% of the total. The handler-envelope dedup (#1) touches ~46
call sites: a phased refactor, not a one-shot.

**Two caveats before anyone cuts.**

1. The "reserved / forward-compat" items (#7 `policy.paths`/`policy.probes`,
   #16 `SshHost`, #12 TC11 counters, #15 `profile_version`) are **deliberate
   seams**, not accidental dead code. They should be deferred or stopped from
   faking success — not blindly deleted — unless the owning milestone is
   confirmed dead. Treat as planning artifacts, not slop.

2. Findings #2 and #1 in `tools.rs` are real but large; verify behavior is
   preserved (the file carries explicit regression-guard comments) before
   touching.

**Safe mechanical subset** (zero behavior change, ~200 lines): #4
`SuppressionCounter`, #5 `EventSink`, #6 session stubs, #10 duplicate constants,
#11 dead `new(policy)`, #13 delegate ctors, #14 unreachable arm, #17 `state_of`,
#18 `should_replace`. These can be applied and verified in one phase
(`cargo check` + `cargo test`).

---

*Audit only. No edits applied. Generated by a 5-agent fan-out over the tree.*
