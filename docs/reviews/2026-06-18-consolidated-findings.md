# Terminal Commander — Consolidated Review & Cleanup Backlog

Date: 2026-06-18
Branch: `feature/omni-review-fixes`
Supersedes (folded in here, originals removed):
`docs/reviews/2026-06-17-code-review-findings.md`,
`docs/reviews/2026-06-17-code-review-remediation-results.md`,
`docs/ponytail-audit-2026-06-17.md` (origin PR #100 -- folded into Part 3)

Two passes live in this one doc:

- **Part 1 — External code review (2026-06-17)**: correctness / security / contract.
  Status: **REMEDIATED** (5 fixed+verified, 7 documented, 1 CI-gated, 1 no-action).
  Carried here as the record of record; nothing left to do except the F-010 CI gate.
- **Part 2 — Ponytail over-engineering audit (2026-06-18)**: complexity only.
  Status: **TO FIX**. ~730 lines cuttable, 0 deps. This is the active backlog.

Correctness, security, and performance were explicitly **out of scope** for Part 2
(that is Part 1's job). Part 2 only hunts over-engineering.

---

## Part 1 — External code review 2026-06-17 (REMEDIATED)

Scope: omni program (`c3adafd..HEAD`, 14 commits) + surrounding seams.
Method: source-grounded review; remediation via 4 file-disjoint worktrees, TDD per
fix, WSL verification gate, integrated via `--no-ff` merges (zero conflicts).

### Capstone verification (integrated branch, WSL)

```
cargo fmt --all -- --check                            -> clean
cargo clippy --workspace --all-targets -- -D warnings -> clean
cargo nextest run --workspace                          -> 955 passed, 1 skipped
```

(955 vs 943 baseline = ~12 new TDD tests. The 1 skip is the pre-existing
`command_status_lifecycle::helper_child_emit_then_linger` helper.) F-001's
`cfg(windows)` ConPTY edits additionally validated by a Windows-native
`cargo clippy -p terminal-commander-probes --all-targets -- -D warnings` (exit 0).

### Per-finding disposition

| ID | Sev | Status | Note |
|----|-----|--------|------|
| F-001 | HIGH | FIXED + VERIFIED | Windows ConPTY writer-thread secret deny now maps to `SecretInputActive` -> IPC `SecretInputDenied` + `stdin_writes_denied_secret`. Typed `WriterReply{Written,SecretDenied,Io}`. unix/win reply shapes unified. Commit `21de53f`. Invariant III restored. |
| F-002 | MED | FIXED + VERIFIED | Atomic `SessionTable{entries,pending}` under one `RwLock`; `reserve_slot()` checks `live+pending>=max` in one critical section; release on spawn failure. 64-thread no-overshoot test. Commit `0183193`. Invariant II restored. |
| F-003 | MED | FIXED + VERIFIED | `command::redact_env_pairs` reuses `mask_token_inline`; env masked before snapshot persist and in `shell_session_status`. e2e proves `TOKEN=` value absent. Commit `0183193`. Invariant V restored. |
| F-008 | LOW | FIXED + VERIFIED | `LimitReached{live,max}`; Display reports both. Commit `0183193`. |
| F-006 | MED | FIXED + VERIFIED | 4 new `subscription_{open,pull,list,close}.v1.json` fixtures; `covered_live 45->49`, `missing_fixture 4->0`. Commit `68ffea0`. |
| F-004 | LOW | DOCUMENTED | sessions unix-only; `omni_status.sessions.available=false` on Windows. `TOOL_CONTROL_SURFACE.md`. `8de3d2c`. |
| F-005 | LOW | DOCUMENTED | Windows ConPTY forced-kill-only vs unix grace ladder. `SHELL_SESSION.md`. `8de3d2c`. |
| F-007 | LOW | DOCUMENTED | `status.cwd` advisory/best-effort. `SHELL_SESSION.md`. `8de3d2c`. |
| F-009 | LOW | DOCUMENTED | CRLF split-read overwrite gap + `stty -onlcr` rationale. `SHELL_SESSION.md`. `8de3d2c`. |
| F-011 | NIT | DOCUMENTED | `compact:true` drops ids; re-fetch note. `OMNI_PLAYBOOK.md`. `8de3d2c`. |
| F-012 | LOW | DOCUMENTED | `target_id` command-lane-only. `TOOL_CONTROL_SURFACE.md` / `OMNI_PLAYBOOK.md`. `8de3d2c`. |
| F-013 | LOW | DOCUMENTED | Universal extractors fallback-only (no baseline alongside an active pack). `OMNI_PLAYBOOK.md` + config comment. `8de3d2c`. |
| F-010 | MED | CI GATE (closes on green) | `scripts/windows-gate.ps1` runs `conpty_*` with `TC_CONPTY_E2E=1` under `GITHUB_ACTIONS`. **Closes when the PR's `pre-build-gates (windows-x64)` job is green.** |
| F-014 | NIT | NO ACTION | Job-receipt design sound (UUIDv7 job_id; live jobs read in-memory first; receipt only on `UnknownJob`). |

### Per-invariant outcome

- II default-deny caps: RESTORED under concurrency (F-002).
- III honest/typed/bounded refusal: RESTORED on Windows writer path (F-001).
- V audit + redaction: RESTORED for session env (F-003); false comments corrected.
- I, IV, VI, VII: held (unchanged).

### Remaining (human-gated)

- **F-010** is the only open item: it closes when CI's `pre-build-gates (windows-x64)`
  job passes (ConPTY live e2e). Land fixes + gate together on merge.

### Honesty notes

- F-006 contract test validates fixture presence/meta/counts, not serde round-trip of
  the example payloads (grounded by hand against the structs).
- F-002 concurrency test exercises the `try_reserve` critical section under the real
  lock; it is not a real-PTY parallel-spawn stress test.
- F-001 live ConPTY path remains unverified on the dev host (= F-010).

---

## Part 2 — Ponytail over-engineering audit 2026-06-18 (TO FIX)

Scope: all 9 crates (~52k Rust LOC) + npm packages + `.claude/*.mjs`.
Method: 5 file-disjoint read-only auditors; every "dead"/"one-caller" claim
cross-verified by repo-wide ripgrep; low-confidence items downgraded explicitly.
Tags: `delete` (dead/speculative), `stdlib` (std ships it), `native` (platform/dep
ships it), `yagni` (one-impl abstraction), `shrink` (same logic, fewer lines).

**Estimated total: ~730 lines removed, 0 dependencies removed.**

> [!IMPORTANT]
> Two items are **forward-compat seams**, not accidental dead code. Per the repo's
> "vision-aligned seam" policy they need **owner sign-off before deletion** — do not
> cut blind: rows **#6** (`PolicyPaths/ProbesSection`) and **#14** (`RemoteTarget`
> fields). Everything else is plain over-engineering and safe to cut.

### Ranked findings (biggest cut first)

| # | Tag | What to cut | Replacement | Location | Conf |
|---|-----|-------------|-------------|----------|------|
| 1 | delete | Entire `directory.rs` module (`DirectoryProbe`, `DirectoryEvent/Kind`, `DirectoryProbeConfig/Error`, `DirectorySink`, `InMemoryDirectorySink`, `JunitSummary`, `extract_attr`, `DEFAULT_DIR_POLL_INTERVAL`) + its `mod` decl | nothing — daemon watches files via `FileProbe`; zero callers repo-wide (~360 lines) | `crates/probes/src/directory.rs:1-359`; `lib.rs:33-37` | HIGH |
| 2 | yagni | Legacy in-process `ToolSurface` facade + `SystemDiscoverResponse` + `McpError::{Bucket,Context,Io}` variants | move to a test helper or delete; only consumer is `tests/e2e.rs`, it lives in `src/` and duplicates `TerminalCommanderMcpServer` | `crates/mcp/src/lib.rs:42-237,56-60` | HIGH |
| 3 | delete | `cmdline_is_our_daemon` + `contains_path_arg` + `is_arg_boundary` (+3 tests) | nothing — `#[allow(dead_code)]`, doc admits "no production caller" | `crates/supervisor/src/replace.rs:236-293,915-979` | HIGH |
| 4 | delete | `EventStore` unused queries: `list_activations`+`ActivationRecord`, `backup_to`, `list_rule_versions`, `get_event`, `vacuum` | nothing — no production/daemon/mcp callers (some have store self-tests only) | `crates/store/src/registry.rs:452-485,51-59,296-309`; `lib.rs:593-601,542-554,587-591` | HIGH |
| 5 | delete | `proc_cmdline` + `join_proc_cmdline` (Linux) + tests | nothing — `replace.rs` uses its own `read_proc_cmdline` | `crates/supervisor/src/pidfile.rs:143-168,280-309` | HIGH |
| 6 | delete | `PolicyPathsSection` + `PolicyProbesSection` structs + config fields | nothing — comments admit "not yet enforced", zero field reads | `crates/daemon/src/config.rs:264-286` | **SIGN-OFF** |
| 7 | stdlib | Hand-rolled 5-arm `match policy.profile` -> snake_case label, duplicated in 3 runtimes | one `PolicyProfile::as_str(&self)->&'static str` (serde already derives `rename_all="snake_case"`) reused by all 3 | `file_watch.rs:177`, `command.rs:427`, `pty_command.rs:191` | HIGH |
| 8 | delete | mcp `ToolStatus::NotImplemented` variant + dead `discovered_tools` `!implemented` branch | drop enum + skip the always-true check — all 49 catalogue entries are `Live` | `crates/mcp/src/tools.rs:96-98,511-512` | HIGH |
| 9 | delete | Daemon zero-caller items: `caps_allow_session`, `BucketSourceTable::snapshot`, `DaemonState::audit_is_persistent` (tautology), `InMemoryAudit::snapshot`, `ActivationRegistryHandle` alias | nothing | `policy.rs:276`, `subscriptions/source.rs:83`, `state.rs:344`, `audit.rs:130`, `activation.rs:205` | HIGH |
| 10 | delete | Core zero-caller items: `RuleHandle`, `EnvironmentSpec::label`, `EnvironmentSpec::from_optional_wsl_distro`, `TypedId::as_uuid` | nothing | `core/src/rule.rs:316`, `environment.rs:24,34`, `ids.rs:85` | HIGH |
| 11 | shrink | mcp `opt_uint_narrow` + 4 near-identical `de_opt_u{16,32,64,usize}_lenient` | one generic `de_opt_uint_lenient<T: TryFrom<u64>>`; collapses the u16/u32/usize trio | `crates/mcp/src/tools.rs:3115-3156` | HIGH |
| 12 | delete | ipc dead: `framing::read_request` (non-classified), `MAX_REQUEST_BYTES` const | use `read_request_classified`; only `MAX_FRAME_BYTES` is enforced | `ipc/src/framing.rs:36-41`, `protocol.rs:178` | HIGH |
| 13 | delete | Daemon `ReadOutcome` enum redefined (Ok/Err/Eof) | import `crate::ipc::ReadOutcome` (already exported) | `crates/daemon/src/ipc/server.rs:340-344` | HIGH |
| 14 | delete | `RemoteTarget.identity_file` + `remote_socket` fields | nothing — read only by config.rs's own tests | `crates/daemon/src/config.rs:601-606` | **SIGN-OFF** (LOW) |
| 15 | yagni | store non-scoped wrappers: `import_rule_pack` file-path variant, `deactivate_rule`, `list_active_rule_defs` | keep `_scoped`/`_str` variants — `store_actor` calls only those | `store/src/import.rs:112`, `registry.rs:490,573` | HIGH |
| 16 | yagni | mcp `RemoteTransport`/`client_for` one-arm match; `TargetRouter::target()` accessor | inline single socket dial; `resolve()` already does the lookup | `mcp/src/target_router.rs:122-130,85-87` | MED |
| 17 | shrink | cli `daemon_is_fresh()` const fn always returns `true`; threads a fix-line that can never print | drop the no-op fact + its `setup_checks` branch | `cli/src/main.rs:1273-1278,1349-1353` | HIGH |
| 18 | mixed | Daemon nits: `PeerCred::to_audit_string` (test-only), `default_shell_session`/`default_sifters` wrappers, `now_epoch` open-coded 3x, `fresh_selfcheck_nonce` (1 caller), `format_os_error_code` (1 caller), `state_of` thin alias | delete / inline / dedupe to existing helper | `peer.rs:32`, `config.rs:167,186`, `shell_session.rs:244,252`, `server.rs:915`, `pipe_server.rs:92` | HIGH |
| 19 | mixed | store/probes nits: `kind_s` JSON-then-trim, `events_table_has_no_blob` Vec, `audit_count` `.optional().unwrap_or(0)`, `evict_expired` unused `created_at` col, `forwarded_wslenv_value` (1 caller), `decode_file_line` dup in `process.rs` | direct `match` / fold into closure / `query_row` / drop col / inline / share helper | `store/registry.rs:205`, `store/lib.rs:620,436`, `store/audit.rs:292`, `supervisor/ensure.rs:63`, `probes/file.rs:644` vs `process.rs:695` | HIGH |
| 20 | delete | `secret_prompt_generation` getter on both PTY backends | nothing — daemon reads `is_secret_prompt_active` only | `crates/probes/src/pty.rs:887-889,1554-1557` | HIGH |
| 21 | shrink | core nits: `SourceFrame::with_byte_offset` (1 caller), `let _ = def.severity == Severity::Trace;` no-op, `SourcePointer::with_stream`, `BySeverity::{bump,unbump}` | inline into call sites / delete dead line / verify-then-inline | `core/context.rs:115`, `sifters/lib.rs:543`, `core/pointer.rs:75`, `core/bucket.rs:158` | MED |
| 22 | yagni | `ActivationRegistry::{len,is_empty}` + `SubscriptionRegistry::is_empty` | flag only — conventional collection helpers, test-only callers; low priority | `activation.rs:82`, `subscriptions/registry.rs:96` | LOW |

### Verified NOT over-engineering (left in place)

`command.rs` redaction/policy-gating, `ShellRuntime` (`Arc` in state, sync-exec
contract), `SpawnProbeOutcome` (6 construction sites), `pipe_acl.rs` Win32 SDDL/FFI,
the 49 `#[tool(...)]` forwarding methods (rmcp macro requires one per tool with its
JSON Schema + teaching description), lenient deserializers (real MCP-client behavior,
well-tested), Windows native kill FFI (deliberate EDR-hardening), `ansi.rs` reusing
`vte`, `paths.rs` byte-matching daemon defaults, `proc_lock.rs` std `try_lock`, the
6 `package.json` platform packages (standard npm boilerplate), the 4 `.claude/*.mjs`
(git-ignored local scratch, never shipped), and documented cross-platform/cross-process
`cfg`-gated seams (`RUNNER_SOCK_SUFFIX`, stub handlers).

### Verification commands (after each batch of cuts)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings   # dead-code cuts must keep this green
cargo nextest run --workspace
```

Suggested cut order: rows 1-5 first (biggest, independent, pure deletes), then the
zero-caller batches 9/10/12/13/20, then the shrinks. Hold 6 and 14 for owner sign-off.

---

## Part 3 — Earlier ponytail audit 2026-06-17 (folded in from origin #100)

A separate, earlier ponytail pass landed on `main` via PR #100
(`docs/ponytail-audit-2026-06-17.md`, now removed and folded here so the consolidated
doc stays the single source of truth). It used a **dedup/shrink lens** where Part 2
used a **dead-code-deletion lens**, so the two lists are largely complementary rather
than duplicates. Findings are preserved verbatim below; the few that overlap Part 2 or
were already applied by the `refactor: remove confirmed-dead code` cleanup
(`cbea00c`) are flagged in the right-hand column.

Tags (same scheme): `delete` · `stdlib` · `native` · `yagni` · `shrink`.

**Audit estimate: ~700 lines, 0 deps.**

| # | Tag | What to cut | Replacement | Location | ~Lines | Cross-ref |
|---|-----|-------------|-------------|----------|--------|-----------|
| P3-1 | shrink | ~46 MCP tool handlers repeat the `ensure_daemon -> daemon.call -> match{Ok/Ok(other)/Err}` envelope verbatim | one `call_daemon_tool(daemon, req, map_resp)` helper | `crates/mcp/src/tools.rs:1000-2500` | 150 | not in Part 2 |
| P3-2 | shrink | `run_and_watch` hand-rolls a C-style state machine (`last_observed_state`/`exit_code`/`receipt` + manual assigns) | Option combinators / small struct | `crates/mcp/src/tools.rs:1160-1313` | 150 | not in Part 2 |
| P3-3 | yagni | 17 serde default-callback fns each wrap one const or `::default()` | `#[serde(default)]` + `#[derive(Default)]` | `crates/daemon/src/config.rs:131-337` | 60 | not in Part 2 |
| P3-4 | yagni | 3x duplicate `SuppressionCounter` impls across Process/File/Pty metrics -- verbatim | macro or generic impl | `crates/probes/src/noise_pipeline.rs:41-102` | 62 | not in Part 2 |
| P3-5 | delete | 3 byte-identical `EventSink` impls (`emit` + `patch_dedupe_aggregate`) | one generic `RouterEventSink<M>` | `daemon/src/file_watch.rs:124`, `pty_command.rs:119`, `command.rs:235` | 50 | not in Part 2 |
| P3-6 | delete | 7 non-unix `session.rs` stub handlers each re-inline the same error | call existing `session_ipc_unsupported()` (L27) | `crates/daemon/src/ipc/handlers/session.rs:27-89` | 50 | not in Part 2 |
| P3-7 | delete | `[policy.paths]` + `[policy.probes]` config -- parsed "for forward-compat," zero enforcement | delete structs, re-add when phase exists | `daemon/src/config.rs:264-286`, `policy.rs:26-27` | 40 | **overlaps Part 2 #6 (SIGN-OFF); Feature A now ENFORCES paths -- re-evaluate** |
| P3-8 | yagni | `regex_rule()` factory -- 4 callers in the same fn, every "varying" arg hardcoded | inline, kill factory | `crates/sifters/src/universal.rs:91-126` | 35 | not in Part 2 |
| P3-9 | shrink | `emit_audit` hand-marshals peer-identity JSON 3x (Unix/Windows/Unknown) via `format!` | `peer.to_metadata_json()` or serde | `daemon/src/ipc/handlers/common.rs:16-58` | 25 | not in Part 2 |
| P3-10 | yagni | Duplicate read-limit/TTL/max-event constants in two crates (same values, divergent names + types) | single source in core, re-export | `core/bucket.rs:50-61`, `store/lib.rs:68-77` | 15 | not in Part 2 |
| P3-11 | yagni | `ProbeNoisePipeline::new(policy)` dead -- only `with_default_policy()` is called (5 sites) | delete `new(policy)`, rename | `probes/src/noise_pipeline.rs:112-122` | 14 | not in Part 2 |
| P3-12 | yagni | Reserved zero-defaulted `noise_suppressed_count`/`dedupe_collapsed_count` "awaiting TC11" | defer | `core/bucket.rs:125-130, 278` | 10 | not in Part 2 (forward-compat seam) |
| P3-13 | yagni | 4 `pub fn new(){ Self::default() }` delegate-only ctors | `#[derive(Default)]`, drop them | `probes/process.rs:121`, `directory.rs:83`, `pty.rs:78`, `sifters/noise.rs:105` | 8 | `directory.rs` ctor MOOT -- module deleted by `cbea00c` (Part 2 #1) |
| P3-14 | delete | Unreachable `Cmd::SelfcheckNoop` match arm (short-circuited at L107) | `unreachable!()` | `daemon/src/main.rs:173-177` | 5 | not in Part 2 |
| P3-15 | yagni | `profile_version` ("TC36-era") parsed, never read outside tests | delete field + `default_profile_version()` | `daemon/src/config.rs:198-199` | 5 | not in Part 2 (forward-compat seam) |
| P3-16 | yagni | `EnvironmentSpec::SshHost` "reserved, not implemented in M1" | delete or `#[cfg]`-gate | `core/src/environment.rs:17-18` | 5 | not in Part 2 (forward-compat seam) |
| P3-17 | delete | `state_of()` "readability alias" for `pty_state()` -- called directly in the same impl | drop alias | `daemon/src/shell_session.rs:195-197` | 3 | DUP of Part 2 #18 (`state_of` thin alias) |
| P3-18 | delete | `should_replace(stale,force) => stale\|\|force`, one call site | inline | `supervisor/src/replace.rs:50-52` | 3 | not in Part 2 |

### Folding caveats (carried from origin's audit notes)

- **Highest leverage is `tools.rs`.** P3-1 and P3-2 (~300 lines) both live in
  `crates/mcp/src/tools.rs` (the repo's largest file). The handler-envelope dedup (P3-1)
  touches ~46 call sites: a phased refactor, not a one-shot. Verify behavior is preserved
  (the file carries explicit regression-guard comments) before touching.
- **Reserved/forward-compat items are deliberate seams**, not accidental dead code:
  P3-7 (`policy.paths`/`policy.probes`), P3-12 (TC11 counters), P3-15 (`profile_version`),
  P3-16 (`SshHost`). Defer or stop-faking-success rather than blind-delete unless the
  owning milestone is confirmed dead.
- **P3-7 needs re-evaluation after Feature A.** Feature A (`b7a1de6`) now ENFORCES path
  allow-lists and gates probe kinds, so `[policy.paths]` is no longer a zero-enforcement
  seam. Part 2 #6 still flags `PolicyPathsSection`/`PolicyProbesSection` for owner
  sign-off; this is the one finding where the two audits and Feature A intersect -- owner
  must reconcile before any cut.
