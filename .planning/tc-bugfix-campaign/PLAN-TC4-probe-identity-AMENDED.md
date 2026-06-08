# PLAN-TC4 (AMENDED 2026-06-08) -- runtime_state probe-row identity

**Supersedes** `PLAN-TC4-probe-identity.md` (the adversarially-reviewed original;
kept for lineage). This amendment ONLY: (1) re-anchors every line citation against
the current tree (Phases 1-3 shifted `crates/mcp/src/tools.rs` by ~+400 lines and
`crates/daemon/src/command.rs`), (2) splits the 6-file phase into **4a** (wire +
redactor + daemon) and **4b** (mcp + cli + fixtures) to respect the <=5-files-per-
phase rule, (3) adds an explicit **security review** gate on the new argv redactor.
No design change to the original's intent or amendment #3.

Language: ASCII only.

## Re-anchor table (verified against working tree at commit e76ebdc)

| Target | Original ref | CURRENT ref | Notes (verified) |
|--------|--------------|-------------|------------------|
| `ProbeListEntry` struct | "struct" | `crates/ipc/src/protocol.rs:1527-1556` | no `tag`/`argv_head`; `path:Option<PathBuf>` + `liveness` use `#[serde(default)]` -- the additive precedent to mirror |
| `format_argv_metadata` | `command.rs:989-1004` | `crates/daemon/src/command.rs:1133-1149` | DRIFT. Confirmed: per-item 128B truncate, `json!({"argv":v})` verbatim -- **redacts NOTHING** (amendment #3 stands) |
| `collect_probes` | `runtime.rs:13-86` | `crates/daemon/src/ipc/handlers/runtime.rs:13-86` | no drift. Command arm `path:None` no tag; FileWatch `path:Some`; PTY `#[cfg(unix)]` destructures `_argv` (discarded), `path:None` |
| `into_parts` (`tag:None`) | `tools.rs:2277` | `crates/mcp/src/tools.rs:2475-2495` (tag:None ~2493) | DRIFT. `McpCommandStartParams.tag` exists; run_and_watch hardcodes `tag:None` |
| `McpRunAndWatchParams` struct | `tools.rs:2219` | `crates/mcp/src/tools.rs:2436-2471` | DRIFT. Confirmed NO `tag` field present |
| CLI probe render | `render.rs:164` | `crates/cli/src/render.rs:164-179` (`fn probe_rows`) | no drift |
| tag source for lift | `state.sources.get(bucket_id)` | `crates/daemon/src/subscriptions/source.rs:77` `fn get(BucketId)->Option<BucketSource>` | CONFIRMED `BucketSource.tag: Option<String>` (source.rs:45) + `path` |
| redaction prior art | `sifters/src/lib.rs:309-343` | (unchanged; output-line capture redaction only, never argv) | evidence only |

Net: the original plan's DESIGN is intact; only the 3 drifted line refs above
need the new numbers. `collect_probes` and `render.rs` did not move.

---

## Phase 4a -- wire fields + argv redactor + daemon collect_probes

**Files (3 prod + tests):** `crates/ipc/src/protocol.rs`,
`crates/daemon/src/command.rs`, `crates/daemon/src/ipc/handlers/runtime.rs`.

1. **Additive `ProbeListEntry` fields** (`protocol.rs:1527-1556`): add
   `#[serde(default, skip_serializing_if="Option::is_none")] pub tag: Option<String>`
   and `#[serde(default, skip_serializing_if="Option::is_none")] pub argv_head: Option<Vec<String>>`,
   mirroring the existing `path`/`liveness` serde-default precedent. Non-breaking,
   no version bump. Doc the "bounded, redacted argv head" shape.

2. **NEW argv redactor + bounded accessor** (`command.rs`; do NOT reuse
   `format_argv_metadata` at :1133-1149 -- it only truncates):
   - Add `fn redact_argv_head(argv: &[String]) -> Vec<String>` that takes
     `argv[0..N]` (N=3: program + 2 tokens) and masks a value to `<redacted>` when
     it follows a secret-shaped flag (`-H`/`--header` with `Authorization:`/`Bearer`,
     `-p`/`--password`, `--token`, `--secret`, `--key`) or matches an inline secret
     (`key=value` where key ~ `*_TOKEN|*_SECRET|*_PASSWORD|*_KEY`, or a URL
     userinfo `scheme://user:PASS@host`). Mask the SECRET span only; keep program +
     flag names visible. Per-item 128B truncate AFTER masking.
   - Add a bounded accessor on `CommandRuntime` (e.g.
     `pub fn argv_head(&self, job_id: JobId) -> Option<Vec<String>>`) returning the
     redacted head, so `collect_probes` neither re-implements redaction nor holds
     extra locks across await.

3. **`collect_probes` wiring** (`runtime.rs:13-86`):
   - Command arm: `argv_head = state.command.argv_head(j.job_id)`;
     `tag = state.sources.get(j.bucket_id).and_then(|s| s.tag)`. DROP the path lift
     (command buckets record `path:None` -- guaranteed no-op; tag is the real win).
   - PTY arm (`#[cfg(unix)]`): stop discarding `_argv` -> redact + project
     `argv_head`; `tag` from `sources.get(bid)`.
   - FileWatch arm: synthesize `argv_head = Some(vec!["file_watch:<path>"])`; keep
     existing `path`.

**4a tests:**
- ipc unit: `ProbeListEntry` with absent `tag`/`argv_head` decodes (serde default);
  a pre-existing payload round-trips. (test-only)
- daemon unit (**amendment #3, security-critical**): argv carrying a REAL secret --
  `["curl","-H","Authorization: Bearer abc123","https://x"]`,
  `["psql","postgres://u:pw@h/db"]`, `["mysql","-ppassw0rd"]` -- yields
  `<redacted>` in `argv_head`; assert against the secret PATTERN, not length; assert
  the program + flag name remain visible. (test-only)

**4a gates + review:** SymForge/external code review + **security review of
`redact_argv_head`** (does it mask every listed pattern; any bypass via casing,
`=` vs space, short/long flag, URL userinfo?) + both OS gates + separate commit
`fix(daemon): ...argv_head + tag identity on probe rows, redacted (TC-4)`.

---

## Phase 4b -- run_and_watch tag plumbing + CLI render + fixtures

**Depends on 4a** (the wire fields must exist first).
**Files (2 prod + fixtures + tests):** `crates/mcp/src/tools.rs`,
`crates/cli/src/render.rs`, `tests/fixtures/contracts/mcp-tools/runtime_state.v1.json`
(+ `probe_list.v1.json` if it carries sample rows).

1. **run_and_watch tag** (`tools.rs`): add `#[serde(default)] pub tag: Option<String>`
   to `McpRunAndWatchParams` (struct `2436-2471`, mirror `McpCommandStartParams.tag`);
   in `into_parts` (`2475-2495`) replace the hardcoded `tag: None` (~:2493) with
   `tag: self.tag`.

2. **CLI render** (`render.rs:164-179`, `fn probe_rows` -- the shared path for
   jobs/probes/runtime-state): add `tag` and `argv_head` columns; `Option` fields
   degrade to blank so an omitted column is no regression.

3. **Fixtures**: add `tag`/`argv_head` to the runtime_state probe-row example +
   invariants (additive; R3 -- no schema test gates these fields).

**4b tests:**
- mcp unit: `McpRunAndWatchParams::into_parts` threads `tag` (regression for the
  silent `tag:None` drop). (test-only)
- live integration (TEST socket): start a tagged command -> `runtime_state` row
  carries the tag + a bounded redacted `argv_head`; `run_and_watch{tag:X}` ->
  `runtime_state` row shows tag X (end-to-end into_parts plumbing). (live)
- CLI render assertion that the tag/argv_head columns appear. (live)

**4b gates + review:** code review + both OS gates + separate commit
`fix(mcp): thread run_and_watch tag + surface probe identity in CLI (TC-4)`.

---

## Invariants (unchanged from original; restated)

- argv surfaced as a BOUNDED, REDACTED head (new redactor; `format_argv_metadata`
  only truncates). Neither over- nor under-redacted. Cross-link R-10 (new read
  surface).
- Additive wire fields only (`#[serde(default, skip_serializing_if)]`) -- no version
  bump; old client ignores unknown, old daemon omits, new client sees None.
- `ProbeListEntry` is constructed ONLY in `collect_probes` -- the additive fields
  reach all three consumers (runtime_state/probe_list/probe_status) with no other
  struct-literal edits.
- render.rs in scope = CLI display completeness (wire round-trips regardless).
- Up to MAX_LIST_LIMIT=500 `sources.get()` RwLock reads per runtime_state -- read
  locks, none held across await; verify no inversion with the live/jobs locks in
  collect_probes.
- No fake success: `into_parts` threads the REAL tag.

## Verification (per phase: 4a, then 4b)

- `pwsh -File scripts/windows-gate.ps1` (PTY arm is `#[cfg(unix)]` -- ensure the
  Windows build still compiles `collect_probes` with the cfg-gated PTY block;
  render.rs cross-platform).
- WSL: `cargo nextest run -p terminal-commander-ipc -p terminal-commander-daemon -p terminal-commander-mcp -p terminal-commander-cli`
  (targeted multi-package; avoids the full-workspace `session_reap` drvfs wedge).
  `cargo fmt --all --check` + `clippy -D` on touched crates. MCP guards (4b touches
  `crates/mcp/src` -- keep it free of the CI-guard literals; the tag plumbing is
  pure data, no fs/spawn/socket).
- manual: runtime_state rows are no longer anonymous (tag + redacted argv_head +
  path where applicable); the redactor masks a REAL secret pattern; no lock
  inversion.
