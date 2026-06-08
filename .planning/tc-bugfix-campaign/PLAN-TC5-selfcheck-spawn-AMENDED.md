# PLAN-TC5 (AMENDED 2026-06-08) -- self_check real command-spawn round-trip

**Supersedes** `PLAN-TC5-selfcheck-spawn.md` (the adversarially-reviewed original;
kept for lineage). Amendment scope: (1) **anchor on SYMBOLS, not line numbers** --
because each preceding phase that lands re-shifts absolute lines; (2) record two
NEW findings from the current-code re-anchor (the `resolve_config` placement trap;
the Mutex-crate pin); (3) disambiguate the sibling `run_self_check`. No change to
the original design intent or amendments #2/#5.

## Post-TC-4 re-anchor (as_of HEAD c91200e, 2026-06-08)

TC-4 (Phase 4a probe-redact `3b7e719`, audit-log redact `7fd0ec8`, Phase 4b
mcp/cli `c91200e`, plus a lint-fix `bb5b41d`) is now landed. Re-anchor verdict
for this plan: **ZERO line drift to every TC-5-anchored symbol.** TC-4 touched
`crates/daemon/src/command.rs`, `crates/daemon/src/ipc/handlers/runtime.rs`,
`crates/ipc/src/protocol.rs` (only at `ProbeListEntry` ~1527, AFTER
`SelfCheckResponse`), `crates/mcp/src/tools.rs` (only at the `Mcp*Params` structs
~2329+, AFTER the `self_check` relay), `crates/cli/src/render.rs`, and two fixtures
-- NONE of the TC-5 anchor files (`server.rs`, `main.rs`, `state.rs`, `source.rs`)
were touched, and the two shared files were edited only below the TC-5 symbols.
Verified current ranges (all match the e76ebdc hints below):
- `handle_self_check` server.rs **801-818** (unchanged; still SYNC, `failures:0`).
- `dispatch_envelope` server.rs **821-828** (unchanged; one-line delegate).
- `fn main` main.rs **86-151**; `resolve_config` called at :88 BEFORE `match cli.cmd`
  at :93 (the placement trap from amendment finding still holds); `resolve_config`
  **212-234** (unchanged; loads `--config`, can return `ExitCode::from(1)`).
- `struct Cli` **39-48**; `enum Cmd` **51-75** (1-line nudge; CLOSED set
  {Check, Start, PrintConfig, Update} -- still NO `SelfcheckNoop`).
- `DaemonState` state.rs **54-130** (unchanged; still NO self-check bucket field;
  `parking_lot::Mutex` import intact; `sources: Arc<BucketSourceTable>` precedent).
- `BucketSourceTable` source.rs **53-56** (unchanged).
- `SelfCheckResponse` protocol.rs **493-496** (unchanged; report-line add needs no
  wire change).
- mcp `self_check` relay tools.rs **570-582** (unchanged; only destructures
  `{report, failures}`).

NEW post-TC-4 facts the Phase-5 implementer MUST honor:
- **`JobBinding` gained a `argv_head: Vec<String>` field (TC-4 4a)**, populated in
  `start_combed` via `redact_argv_head(&argv_for_meta)`. The self-check spawn going
  through `CommandRuntime` will therefore ALSO store a redacted argv_head for
  `[exe, "selfcheck-noop"]` -- harmless (no secret in that argv), but note that
  `JobBinding` no longer derives nothing-but-the-old-fields. `command.rs` grew
  ~+700 lines (redactor + `format_argv_metadata` now redacts), so any residual
  command.rs line numbers are MORE stale than the e76ebdc hints -- re-resolve with
  SymForge at exec, do not trust command.rs line hints.
- **Phase-2 dedup is intact and unchanged** (point #6 still applies): `dedup_nonce`
  on `CommandStartRequest` (139-174) and `fn dedup_key` (now ~1126-1147) are
  present. The self-check spawn MUST mint a FRESH `dedup_nonce` per call (or be
  dedup-exempt), else two self_checks within 3s collapse and the second never
  spawns -- re-introducing the false-green. TEST: two back-to-back self_checks
  within 3s each yield a DISTINCT job_id.

## Drift convention (READ FIRST)

Line numbers below are `as_of commit e76ebdc` HINTS only. They WILL drift after
Phase 4 lands (Phase 4 does not touch these files much, but Phases are sequential
and any edit shifts lines). **Anchor on the named symbol; re-resolve with SymForge
at phase start.** Mandatory first execution step:

> Re-anchor: for each symbol in the table below, `search_symbols`/`get_symbol` to
> get its CURRENT range; confirm the CLAIM still holds before editing. If a claim
> no longer holds (e.g. a prior phase already made handle_self_check async),
> STOP and re-plan that item.

## Re-anchor table (verified as_of e76ebdc)

| Item (anchor SYMBOL) | file | as_of line | claim (verify at exec) |
|----------------------|------|-----------|------------------------|
| `fn handle_self_check` | crates/daemon/src/ipc/server.rs | 801-818 | SYNC `fn`; returns `SelfCheckResponse{report, failures:0}` hardcoded (literal `failures:0`); never spawns; 3-4 static lines |
| `fn dispatch` SelfCheck arm | crates/daemon/src/ipc/server.rs | arm ~541 (dispatch 505-771) | dispatch is ALREADY `async`; the SOLE `handle_self_check` caller is this arm (find_references = 1). Add `.await` here only |
| `fn dispatch_envelope` | crates/daemon/src/ipc/server.rs | 821-828 | one-line delegate `dispatch(...).await`; NO SelfCheck arm; already async -> ZERO change (amendment #5) |
| `struct Cli` / `enum Cmd` | crates/daemon/src/main.rs | Cli 39-48, Cmd 50-75 | required `#[command(subcommand)] cmd: Cmd`; CLOSED set {Check 53, Start 56-60, PrintConfig 62, Update 68-74}; NO SelfcheckNoop exists |
| `fn main` + `resolve_config` | crates/daemon/src/main.rs | main 86-151 (parse 87, resolve_config 88, match 93-150); resolve_config 212-234 | **NEW (see below):** config is resolved BEFORE command dispatch |
| windows_subsystem attr | crates/daemon/src/main.rs | line 4 | `#![cfg_attr(windows, windows_subsystem="windows")]` (no console for clap stderr) |
| module-doc "Does NOT spawn child commands" | crates/daemon/src/main.rs | line 37 (in long_about 33-37); "Subcommands:" block 11-18 | plan's `36-37` -> single line **37** |
| `struct DaemonState` | crates/daemon/src/state.rs | 54-130 (impl 145) | NO self-check bucket field today; uses `parking_lot::Mutex` (import line 19); `sources: Arc<...BucketSourceTable>` is the threading precedent |
| `struct BucketSourceTable` + module doc | crates/daemon/src/subscriptions/source.rs | doc 12-17; struct 53-56; impl 58-97 | buckets IMMORTAL; impl exposes new/record/get/snapshot/dirty_epoch -- NO remove/drop |
| `struct SelfCheckResponse` | crates/ipc/src/protocol.rs | 493-496 | `{report:String, failures:u32}` -- a report-line add needs NO wire change |
| `fn self_check` (MCP relay) | crates/mcp/src/tools.rs | ~570-582 | only destructures `{report, failures}` and relays -> zero MCP change |

## Fix (symbol-anchored)

1. **`handle_self_check` -> `async fn`** + profile-gated bounded real round-trip.
   Keep the static lines; ADD: if policy allows CommandStart for the active profile
   AND command_allow_roots does not exclude the daemon binary AND (repo_only has a
   resolvable root OR profile != repo_only) -> spawn `std::env::current_exe()` as
   the hidden subcommand via the normal CommandRuntime path into the CACHED
   self-check bucket; poll command_status to terminal within ~2s; set `failures>0`
   + explanatory line ONLY on genuine breakage (spawn error / never terminal /
   nonzero exit); ELSE SKIP with `spawn probe skipped: <profile reason>`,
   failures unchanged (NEVER false-RED).

2. **Single dispatch await (amendment #5):** add `.await` at the SOLE
   `handle_self_check` call inside `dispatch` only. `dispatch_envelope` /
   pipe_server reach it transitively through the already-async `dispatch` -> ZERO
   change there. Verify the Windows build still compiles.

3. **Hidden clap SUBCOMMAND, not a flag (amendment #2) -- with the NEW placement
   requirement:** add `Cmd::SelfcheckNoop` with `#[command(hide = true)]` to `Cmd`.
   **NEW FINDING (must honor):** `main()` calls `resolve_config(&cli)` (as_of :88)
   BEFORE the `match cli.cmd` block (as_of 93-150), and `resolve_config` loads a
   `--config` file from disk + applies env overrides and can return `ExitCode::from(1)`.
   A `SelfcheckNoop` arm placed inside `match cli.cmd` would run AFTER config
   resolution -- so a child that inherits a broken `--config`/env would exit nonzero
   in inert mode, FALSE-REDing a healthy daemon. Therefore the short-circuit MUST be
   between `Cli::parse()` and `resolve_config`, e.g. immediately after parse:
   `if matches!(cli.cmd, Cmd::SelfcheckNoop) { return ExitCode::SUCCESS; }` -- an
   inert leaf that does NO arg/env/config/socket/fs/policy work. Spawn argv =
   `[exe, "selfcheck-noop"]` (a VALID subcommand), NEVER `--selfcheck-noop`.
   Reconcile the main.rs module-doc/long_about ("Does NOT spawn child commands",
   line 37) with the new internal self-probe.

4. **Cached immortal bucket -- pin the type (NEW):** add a cached holder to
   `DaemonState`. Use **`parking_lot::Mutex<Option<BucketId>>`** (the crate already
   imported at state.rs:19; mirrors the existing `Arc<Mutex<Instant>> last_activity`)
   or `once_cell::OnceCell` for write-once-reuse. Do NOT use `tokio::sync::Mutex`
   held across the `.await` spawn-poll. Lazily create on the first spawn-running
   self_check, reuse every call (honors the immortal-bucket invariant; no
   drop_bucket; bucket_count grows by exactly 1 over the daemon lifetime).

5. **`SelfCheckResponse`** structurally unchanged; report string gains a
   spawn-probe line (no wire change).

6. **Phase-2 dedup interaction (NEW -- must honor, verify at re-anchor):** the
   self-check spawn routes through `CommandRuntime`, which since Phase 2 (cf43d5b)
   holds an in-flight dedup guard (nonce-preferred; else peer-scoped
   `(uid/sid, argv, cwd, tag)`; 3s TTL). The self-check argv is constant
   (`[exe, "selfcheck-noop"]`), so two self_checks within 3s would hash-collide and
   the SECOND would COLLAPSE into the first job -- it would NOT actually spawn,
   silently re-introducing the false-green this phase exists to kill. The self-check
   spawn MUST mint a FRESH `dedup_nonce` per call (the daemon-internal start path can
   set `dedup_nonce: Some(<fresh>)` the same way the MCP adapter does), or be
   explicitly dedup-exempt. TEST: two back-to-back self_checks within 3s each yield a
   DISTINCT job_id (both really spawned).

**Disambiguation (NEW):** do NOT edit the heavyweight `run_self_check` /
`SelfCheckReport` in `crates/daemon/src/runtime.rs` (used by `Cmd::Check` and
`RuntimeMode::SelfCheck`). TC-5 targets ONLY the IPC `handle_self_check` (the
false-green LIVE-client surface).

## Tests (unchanged intent)
- live (DeveloperLocal): self_check spawns the noop, report shows spawn-ok,
  failures==0 healthy; a SECOND self_check reuses the SAME bucket (bucket_count +1
  max over lifetime).
- live (read_only_observer): SKIPS the spawn, stays failures==0 with
  `spawn probe skipped: <reason>`.
- negative (TC-5 acceptance): force the round-trip to fail -> failures>0 + line.
- positive (amendment #2): `terminal-commanderd selfcheck-noop` exits 0.

## Invariants / Verification: unchanged from the original (see PLAN-TC5-selfcheck-spawn.md sections "Invariants (Phase 5)" / "Verification (Phase 5)"); plus:
- the SelfcheckNoop short-circuit precedes `resolve_config` (never false-RED via a
  broken inherited config);
- the cached-bucket Mutex is parking_lot/OnceCell, never held across await.
