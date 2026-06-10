# Field Review Findings — 2026-06-10

Reviewer: Claude (Cowork session, Windows host, daemon 0.1.47, repo HEAD `4f2d9e4`).
Method: every seed finding from the 2026-06-10 field report was reproduced live
against the running daemon AND located in source at HEAD; an independent pass
covered the v0.1.46..v0.1.47 diff, the TC49 unreleased work, the core engine,
and an LLM-ergonomics audit of the tool surface. Verification baseline below.

## Verification baseline (main @ 4f2d9e4, Windows host)

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --all --check` | **FAIL** — diffs in TC49 shell-exec param code (~198 diff lines) |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | PASS (52.9 s) |
| Tests | `cargo nextest run --workspace --no-fail-fast` | **639 run: 637 pass (1 leaky), 2 FAIL** — both `read_subcommands` (S7 pipe collision); 6/6 pass with `--test-threads 1` |
| Windows gate | `windows_no_console_spawn` (incl. ignored) | PASS (exit 0) |
| Windows gate | `windows_spawn_site_coverage` | PASS (3 passed) |
| npm wrapper | `node --test` in `packages/terminal-commander` | **360 run: 359 pass, 1 FAIL** — `av-safe-install-runtime.test.js:146` (S9 host-WSL leak) |
| TC47 load gate | linux-gate only (python3 path probe is unix-specific) | NOT RUN locally — runs in CI |
| Doctests | `cargo test --workspace --doc` | NOT RUN locally this session |

CI-Linux is presumably green on the same SHA; every local failure above is
either repo hygiene (S8) or host-environment-sensitive test design (S7, S9).

## Severity-ordered findings

### S1 — `command_status` counters are zero until exit (HIGH, trust) — seed #4 CONFIRMED
`CommandRuntime::status` (crates/daemon/src/command.rs:1275-1304) reads
`JobBinding.metrics`, which the code's own doc comment says "is only populated
at exit, so it would read zero for a still-running job"
(crates/daemon/src/command.rs:253-281). TC-3 added `metrics_live`
(`Arc<Mutex<ProcessProbeMetrics>>`, updated per line by
`read_stream`, crates/probes/src/process.rs:563-644) so `stop()` could report
real counts — but `status()` was never switched over.
**Live repro:** mid-run `command_status` returned `bytes_total: 0,
frames_total: 0` while `command_output_tail` on the same job returned 50+
captured lines (`truncated_lines: true`).
**Fix:** `status()` snapshots `metrics_live` when the job is not terminal
(mirror `stop()`); regression test polls status mid-run and asserts
`frames_total > 0`.

### S2 — inline `rules_json` rules silently discarded without `"status":"active"` (HIGH, trust)
`RuleDefinition`'s serde default status is `Draft`. The adapter parses
`rules_json` directly as `Vec<RuleDefinition>` (`parse_bucket_and_rules`,
crates/mcp/src/tools.rs:2360-2400), so a definition without an explicit
`status` field arrives Draft. `merge_active_and_inline`
(crates/daemon/src/command.rs:1353-1377, mirrored in file_watch.rs:477-495 and
pty_command.rs) then *silently skips* non-runtime-eligible inline rules — a
draft-poison defense that is correct for stale registry entries but wrong for
rules the caller passed in this very call. The command runs, zero signals
fire, and the receipt claims "zero rules matched". No tool description
mentions `status`.
**Live repro:** `rules_json` `[{"id":"xmarker",...,"pattern":"XMARKER"}]` →
0 signals, receipt `lines_suppressed: 2`; identical call with
`"status":"active"` → 2 signals.
**Fix:** parse `rules_json` through `Vec<RuleInput>` + `finalize()` (the typed
path), which ships `Active`, applies defaults, and returns teaching errors.
`full_rule_definition_json_still_parses_as_rule_input` already proves
compatibility. Keep the daemon-side draft filter for registry-sourced rules.

### S3 — Option-typed schemas don't survive real MCP clients (HIGH, interop) — seeds #1/#3/#8a CONFIRMED
Every optional numeric/array param (`wait_ms`, `max_signals`, `grace_ms`,
`max_lines`, `max_bytes`, `start_line`, `rules`, …) derives a schema of the
form `{"type":["integer","null"]}` / `{"type":["array","null"],
"items":{"$ref":"#/$defs/RuleInput"}}` (verified by driving the HEAD-built
adapter's `tools/list` directly). Real clients (this session's included)
normalize away union types and `$defs` refs, leaving the param **untyped** —
the client then stringifies numbers and JSON-encodes arrays, and adapter-side
serde rejects them: `invalid type: string "45000", expected u64`;
`invalid type: string "[...]", expected a sequence`.
**Live repro:** multiple times this session, on `run_and_watch.wait_ms`,
`command_output_tail.max_lines`, `file_read_window.start_line`, and `rules`.
**Fix (both layers):**
(a) schema post-transform: optional fields emit plain `"type":"integer"` /
`"type":"array"` (the field is already optional via `required`); inline item
schemas rather than `$defs` where feasible;
(b) lenient deserializers: accept numeric strings for numeric options and a
JSON-encoded string for `rules` (the `deserialize_argv` teaching pattern,
crates/mcp/src/tools.rs:2187-2208, already establishes the idiom).
Regression: a schema test asserting every property of every tool schema has a
plain string `"type"`, plus serde tests for `"45000"` → `45000` and
stringified `rules`.

### S4 — idle self-reaper fires with live jobs/waits (HIGH, reliability) — seed #5 ADJUDICATED
The observed daemon restarts are **by-design idle eviction**: TTL 1800 s
(crates/daemon/src/config.rs:60), `spawn_idle_reaper`
(crates/daemon/src/runtime.rs:446-474) fires `trigger_shutdown` when
`idle_secs() >= ttl`. Daemon log (state/logs/terminal-commanderd.log) shows 27
idle-reaps since 2026-05-29, including 09:01:26 and 09:48:06 today — matching
the field report's `daemon_unavailable` + `uptime_secs: 0`.
Defects within the design:
1. `idle_secs` counts only IPC dispatch recency (state.rs:308-317). A
   30-minute quiet stretch with a **still-running command** reaps the daemon;
   `drain_lifecycle_tasks` (command.rs:1135-1153) aborts waiters after
   `LIFECYCLE_DRAIN_CEILING` (10 s) **without killing the children** → the
   process is orphaned and its receipt/events/audit row are lost.
2. An in-flight long-poll (`bucket_wait`/`run_and_watch` slice) at reap time
   gets a dropped pipe rather than a clean shutdown reply → the client's
   "IPC error interrupted the wait" degraded receipts.
3. The adapter does not transparently respawn-and-retry idempotent calls after
   eviction, so the first post-reap call fails `daemon_unavailable`.
**Fix (minimal):** the reaper consults the runtime before firing — live
probes/PTY jobs/file watches/open subscriptions count as activity. Follow-ups
(separate findings, not bundled): graceful long-poll completion on shutdown;
adapter-side single respawn-retry for idempotent RPCs.

### S5 — two versions of one rule active in one scope fire twice per frame (MEDIUM) — seed #7 CONFIRMED
`ActivationRegistry` is keyed `(rule_id, version, scope)`; the documented
rationale covers the same `(id, version)` under *disjoint scopes*
(crates/daemon/src/activation.rs:40-49), but nothing supersedes v1 when v2 of
the same id is activated under the *same* scope. `snapshot_for_job`
(activation.rs:183-199) returns both; the sifter fires one event per rule
entry per frame (crates/sifters/src/lib.rs:282-352).
**Live repro:** one stderr line → two identical events, `rule.version` 1 and 2
(`cargo.warning`, globally active twice on this daemon).
**Fix:** `registry_activate` deactivates other versions of the same rule id
under the same scope (activate-supersedes), with the response disclosing what
was superseded. Regression test: activate v1 then v2, one frame, assert one
event.

### S6 — pack rules brand foreign output (MEDIUM, classification honesty) — seed #6 CONFIRMED
`cargo.warning` (crates/store/rules/cargo.json:43-60) matches
`^warning: (?P<message>.+)$` on stderr — git's "warning: LF will be replaced
by CRLF" (and any tool's `warning:` line) is emitted as
`kind: "compile_warning"`, `tags: ["build","rust","cargo"]`. The regex crate
has no lookahead, so the pattern cannot be narrowed reliably.
**Live repro:** `node -e "console.error('warning: something happened')"` →
`compile_warning` tagged rust/cargo.
**Fix:** relabel to honest generality — `event_kind: "warning"`, drop the
language-claim tags on this one rule (keep "cargo" pack identity in the rule
id), and document in `registry_import_pack` that global activation applies
pack rules to every command's streams (scoped activation is the precise
tool).

### S7 — Windows pipe collision in CLI live-daemon tests (MEDIUM, tests)
Every test in crates/cli/tests/read_subcommands.rs (and friends) uses the
constant `TC_SESSION` token `"readtest"`. On Windows the endpoint is derived
*solely from the token* (`\\.\pipe\terminal-commander-readtest`,
crates/supervisor/src/paths.rs:133-160); the unique-per-test `TC_DATA` only
isolates the state dir. Parallel nextest runs collide: one test's daemon owns
the pipe, another test's CLI probes after the owner died → exit 69
`EndpointBindFailed`. On Unix the socket path derives from the state dir, so
CI-Linux never sees it.
**Evidence:** full parallel run: 2 FAIL; 6-test parallel run: 3 FAIL;
`--test-threads 1`: 6/6 PASS; manual single harness reproduction: CLI exit 0.
Also: `rules_show_missing_rule_exits_typed_error` asserts only
non-zero + stderr-contains-"failed", which an unavailability exit can satisfy
vacuously.
**Fix:** unique token per test (the existing `tag` is already unique);
tighten the missing-rule assertion to require the typed `RuleNotFound`.

### S8 — main is not fmt-clean (LOW, hygiene)
`cargo fmt --all --check` fails on TC49 shell-exec param code. Fix: run fmt.

### S9 — npm test depends on host WSL state (LOW, tests)
`av-safe-install-runtime.test.js:146` expects the "native Windows MCP path
selected; WSL bootstrap skipped." diagnostic but real host detection answered
"WSL runtime already present." — the test exercises real WSL detection
instead of a stubbed one and fails on any WSL-equipped Windows host.
**Fix:** stub/force the detection branch in the test.

### S10 — shell-bridge guard launderable via `wsl.exe` (LOW, hardening/docs)
`SHELL_INTERPRETERS_DENY` (crates/daemon/src/command.rs:78-95) checks only
`argv[0]`'s basename; `["wsl","bash","-c", …]` runs a shell with argv[0]=wsl.
The policy engine remains the actual boundary, but the README/POLICY contract
language ("shell interpreters denied") overstates the guard.
**Fix:** add `wsl`/`wsl.exe` to the deny list AND/OR document the guard as
best-effort interpreter hygiene, not a security boundary.

### S11 — common severity aliases rejected (LOW, ergonomics) — seed #2 residual
Seed #2's "documented minimal form does not work" is **fixed at HEAD** (typed
`RuleInput` shorthand: pattern-only rules, defaults, single teaching errors —
crates/mcp/src/tools.rs:2220-2346 with tests). Residual: `severity: "error"`
(the single most likely LLM guess) is still rejected.
**Fix:** alias map in `finalize`: error→high, warn/warning→medium,
fatal→critical; keep the teaching error for genuinely unknown values.

### S12 — `ShellInterpreterDenied` doesn't teach the remedy (LOW, ergonomics) — seed #8b CONFIRMED
crates/daemon/src/ipc/handlers/common.rs:98-103 says what was denied but not
what to do.
**Fix:** append "invoke the program directly as argv (e.g.
[\"cargo\",\"build\"]); shell_exec exists behind the allow_shell policy cap."

### S13 — stale global activations silently poison later sessions (INFO/docs)
Durable global activations (by design) meant a previous session's
`cargo.warning` v1+v2 fired on every command in *this* session, mislabeling
output (S6) in duplicate (S5). With S5's supersede fix and S6's relabel the
sting is reduced; additionally document `registry_list_active` as a
session-start hygiene check and prefer scoped activation in pack docs.

## Seed-finding adjudication summary

| Seed | Verdict | Finding |
|---|---|---|
| 1. untyped numeric params | CONFIRMED at HEAD (union-type stripping) | S3 |
| 2. minimal rules form broken | FIXED at HEAD (RuleInput); residuals | S11, S2 |
| 3. `rules` array rejected | CONFIRMED (same root cause) | S3 |
| 4. counters lag until exit | CONFIRMED (code + live) | S1 |
| 5. wait interruptions / restarts | EXPLAINED (idle reap by design) + 3 defects | S4 |
| 6. git stderr as compile_warning | CONFIRMED | S6 |
| 7. duplicate events v1+v2 | CONFIRMED | S5 |
| 8. tail params + denial message | CONFIRMED | S3, S12 |

## Fix plan (branch per group, regression test per fix)

1. `fix/fmt` — S8 (hygiene, lands first).
2. `fix/mcp-client-interop` — S3 (schema transform + lenient coercion + schema/serde regression tests).
3. `fix/inline-rules-trust` — S2 + S11 + S12.
4. `fix/status-live-counters` — S1.
5. `fix/idle-reaper-live-work` — S4 (minimal: live work counts as activity).
6. `fix/activation-supersede` — S5.
7. `fix/test-isolation` — S7 + S9 (test-only).
8. `fix/cargo-pack-honest-labels` — S6 (+ S10 docs note).

Deliberately left open: graceful long-poll completion on daemon shutdown,
adapter respawn-retry for idempotent calls post-eviction, `wsl` deny-list
decision (operator call), compact response mode for heavy event objects.
