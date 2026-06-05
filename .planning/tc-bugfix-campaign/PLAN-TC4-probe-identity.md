# PLAN-TC4 -- runtime_state probe-row identity: argv_head + tag (Phase 4)

**Source:** TC trust-defects campaign (`plan-final.json` Phase 4 / fork F6) +
`review-verdict.json` required amendment #3 (build a REAL argv redactor;
format_argv_metadata only TRUNCATES) + adopted optional improvements (drop the
always-None path lift for the Command arm; reword the invariant as CLI display
completeness, not data integrity).
**Posture:** Surface a BOUNDED argv head + tag via additive serde(default)
ProbeListEntry fields, fix the run_and_watch into_parts tag:None fake-success
path, and put crates/cli/src/render.rs in scope so the new identity columns are
not silently dropped. CRITICAL CORRECTION: a NEW argv redactor is required before
argv_head ships -- format_argv_metadata redacts NOTHING.

Language: ASCII only.

---

## Summary table

| Symptom | Location (file:line) | Fix sketch | Effort | Test impact |
|---------|----------------------|------------|--------|-------------|
| Command-arm probe rows hardcode path:None, carry no tag/argv | `crates/daemon/src/ipc/handlers/runtime.rs:13-86` (Command arm ~:37) | Read argv_head via a new redacting accessor; lift tag from state.sources.get(bucket_id) | **M** | integration (probe row carries tag + redacted argv_head) |
| PTY-arm discards _argv, path:None | `crates/daemon/src/ipc/handlers/runtime.rs` (PTY arm ~:67, ~:80) | Stop discarding _argv; project argv_head; lift tag from sources | **M** | integration (PTY row carries argv_head + tag) |
| ProbeListEntry has no tag/argv field | `crates/ipc/src/protocol.rs` (ProbeListEntry struct) | Add additive `tag: Option<String>` + `argv_head: Option<Vec<String>>` (serde default, skip_if None) | **S** | unit (absent fields decode; old payload round-trips) |
| **format_argv_metadata redacts NOTHING (only truncates)** | `crates/daemon/src/command.rs:989-1004` (128-byte truncate, verbatim) | Build a NEW argv redactor that masks values after secret-shaped flags BEFORE argv_head ships | **M** | unit (Authorization header / password flag masked to <redacted>) |
| run_and_watch into_parts hardcodes tag:None (fake-success) | `crates/mcp/src/tools.rs:2277` (tag:None); `:2219` (McpRunAndWatchParams lacks tag) | Add `#[serde(default)] tag: Option<String>`; set `tag: self.tag` in into_parts() | **S** | unit (into_parts threads tag); integration (run_and_watch tag -> probe row) |
| new fields silently dropped by the CLI | `crates/cli/src/render.rs:164` (probe_rows shared render path) | Render the new tag/argv_head columns; Option fields degrade to blank | **S** | CLI render assertion (columns appear) |

**Estimated files:** 6: `crates/ipc/src/protocol.rs`, `crates/daemon/src/command.rs`
(NEW redactor + bounded accessor), `crates/daemon/src/ipc/handlers/runtime.rs`,
`crates/mcp/src/tools.rs`, `crates/cli/src/render.rs`,
`tests/fixtures/contracts/mcp-tools/runtime_state.v1.json` (+ probe_list.v1.json
if it carries sample rows).

---

## Per-item detail

### TC-4 -- anonymous probe rows + run_and_watch tag:None

**Symptom:** runtime_state probe rows carry no identity (path:None, no tag, argv
discarded), so an operator cannot tell which job a row is. run_and_watch cannot
even tag its probe: into_parts() hardcodes tag:None.

**Citations:**

```13:86:crates/daemon/src/ipc/handlers/runtime.rs
// collect_probes: Command arm hardcodes path:None (~:37); PTY arm discards _argv (~:67), path:None (~:80)
```

```989:1004:crates/daemon/src/command.rs
// format_argv_metadata: per-item 128-byte TRUNCATE, serializes {"argv":[...]} verbatim -- NO redaction
```

```309:343:crates/sifters/src/lib.rs
// the ONLY redaction in the tree: per-rule capture redaction on command OUTPUT lines (never argv)
```

```2277:2277:crates/mcp/src/tools.rs
tag: None, // into_parts() hardcodes tag:None -- the verified fake-success path
```

**Fix:**

1. **Additive ProbeListEntry fields** (`crates/ipc/src/protocol.rs`): add
   `#[serde(default, skip_serializing_if="Option::is_none")] pub tag: Option<String>`
   and `pub argv_head: Option<Vec<String>>` (matching the frames_suppressed /
   liveness serde precedent; non-breaking, no version bump). Doc the bounded
   redacted argv shape.

2. **Build a REAL argv redactor (amendment #3 -- DO NOT rely on
   format_argv_metadata):** format_argv_metadata (command.rs:989-1004) only
   truncates each element to 128 bytes and serializes verbatim; it redacts
   nothing, and the ONLY redaction in the tree is on OUTPUT lines
   (sifters/src/lib.rs:309-343). Because argv_head is argv[0..2] (program + first
   two tokens), bounding the head does NOT bound away secrets -- it keeps exactly
   the window where secrets sit (`psql postgres://user:pass@host`,
   `mysql -ppassword`, `curl -H "Authorization: Bearer <tok>"`), all under 128
   bytes so truncation never trips. Choose ONE:
   - **(a)** add a dedicated argv redactor that masks values following
     secret-shaped flags (`-H`/`--header`, `-p`/`--password`, `--token`,
     `--secret`, `Authorization:`, and `key=value` where key matches
     `*_TOKEN`/`*_SECRET`/`*_PASSWORD`/`*_KEY`) and replaces the value with
     `<redacted>`; OR
   - **(b)** surface ONLY argv[0] (program basename) plus tag and drop
     argv[1..2].
   Add a bounded redacting accessor on CommandRuntime (e.g.
   `argv_head(job_id) -> Option<Vec<String>>` returning the redacted argv[0..2])
   so collect_probes need not re-implement redaction or hold extra locks. Update
   every reference that previously said "format_argv_metadata redaction" to read
   "format_argv_metadata only truncates; a NEW argv redactor is required."

3. **collect_probes wiring** (`crates/daemon/src/ipc/handlers/runtime.rs:13-86`):
   - **Command arm:** read argv_head via the new accessor and lift tag from
     `state.sources.get(bucket_id)` (BucketSource). **Drop the `(+ path)` lift
     for the Command arm (adopted optional):** start_combed records `path: None`
     for command buckets (command.rs:509), so lifting path is a guaranteed no-op;
     tag IS recorded (command.rs:510) and is the real win.
   - **PTY arm:** stop discarding _argv and project argv_head; lift tag from
     sources.
   - **FileWatch arm:** synthesize `argv_head=['file_watch:<path>']` and keep its
     existing path.

4. **Fix run_and_watch tag plumbing** (`crates/mcp/src/tools.rs`): add
   `#[serde(default)] pub tag: Option<String>` to McpRunAndWatchParams (struct at
   :2219, mirror McpCommandStartParams.tag); set `tag: self.tag` in into_parts()
   (replace the hardcoded tag:None at :2277).

5. **CLI render in scope** (`crates/cli/src/render.rs:164`, the single shared
   probe_rows render path covering jobs/probes/runtime-state): display the new
   tag/argv_head columns; Option fields degrade to blank so an omitted column is
   no regression.

**Effort:** M. **Test:**
- unit (crates/ipc): ProbeListEntry with absent tag/argv_head decodes (serde
  default); old payload round-trips. source-status: test-only.
- unit (crates/mcp): McpRunAndWatchParams::into_parts threads tag (regression for
  the silent-drop). source-status: test-only.
- unit (crates/daemon, amendment #3): an argv with a secret-looking token (an
  Authorization header / a password flag, NOT just a long string) surfaces
  `<redacted>` in argv_head -- assert against a REAL secret pattern, not length.
  source-status: test-only.
- integration through daemon IPC (TEST socket): start a tagged command, call
  runtime_state, assert the probe row carries the tag and a bounded redacted
  argv_head. source-status: live.
- integration: run_and_watch with tag=X -> runtime_state row shows tag X (proves
  end-to-end into_parts plumbing). source-status: live.
- CLI render assertion that tag/argv_head columns appear. source-status: live.

---

## Invariants (Phase 4)

- TC-4 argv is surfaced as a BOUNDED head (argv[0..2] or argv[0] only), redacted
  by a NEW argv redactor (format_argv_metadata only truncates), consistent with
  R-06 (argv is operator-input-safe, already verbatim in command_exited summary)
  -- neither over- nor under-redacted. Cross-link R-10 (new read-surface widening).
- Additive wire fields only: ProbeListEntry.tag/argv_head use
  `#[serde(default, skip_serializing_if="Option::is_none")]` -- non-breaking, no
  version bump; old client ignores unknown, old daemon omits, new client sees None.
- ProbeListEntry is constructed only in collect_probes -- the additive fields fix
  all three consumers (runtime_state/probe_list/probe_status) without touching
  other struct literals.
- Reworded invariant (adopted optional): render.rs is in scope so the CLI table
  SURFACES the new identity columns (wire fields round-trip regardless; this is
  CLI display completeness, not data integrity).
- Up to MAX_LIST_LIMIT=500 sources.get() RwLock reads per runtime_state -- verify
  no lock inversion with the live/jobs locks held in collect_probes (read locks,
  no held-across-await; likely fine).
- No fake success: into_parts() threads the REAL tag.

## Verification (Phase 4)

- `wsl bash scripts/linux-gate.sh` (the PTY arm is cfg(unix) -- ensure the
  windows build still compiles collect_probes with the cfg-gated PTY block;
  render.rs cross-platform).
- `pwsh -File scripts/windows-gate.ps1`.
- `cargo nextest run -p terminal-commander-ipc -p terminal-commander-daemon -p terminal-commander-mcp -p terminal-commander-cli`.
- manual: runtime_state rows are no longer anonymous (tag + redacted argv_head +
  path where applicable); confirm the argv redactor masks a real secret pattern;
  confirm no lock-ordering inversion.
