# WSL Cleanup Dogfood Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the eight operability/ergonomics gaps found while driving a real WSL cleanup strictly through Terminal Commander, so an LLM can use TC fluently and an operator can set it up with one self-explaining `doctor` pass.

**Architecture:** Three layers. (1) A rule-free output escape hatch + a shipped `cleanup` seed pack so exploratory and cleanup commands need no mid-flight rule authoring. (2) Operability fixes: a force restart path, a deterministic pidfile location, an env-forward allowlist, and an `update`/`restart`/`upgrade` verb split. (3) A doctor-driven setup brain that detects every prerequisite (WSL, sudoers, daemon freshness, cleanup pack) and prints the exact fix line, plus an integration doc for the SUDO discipline. No interactive wizard.

**Tech Stack:** Rust workspace (`crates/{core,sifters,store,daemon,supervisor,mcp,cli}`), `clap` CLI, `serde_json` pidfile, named-pipe (Windows) / UDS (unix) IPC, Node JS npm wrapper (`packages/terminal-commander`). Tests are `cargo test` per crate; Node tests are `node --test`.

**Spec:** `docs/superpowers/specs/2026-05-27-wsl-cleanup-dogfood-findings-design.md`

**Conventions reminder:** ASCII only. Structural edits via SymForge where possible. After cargo work in a phase, run `cargo fmt`, `cargo check`, `cargo clippy`, `cargo test` for the touched crate, then `cargo clean` at the phase boundary. Commit per task. main only (no branches).

---

## Phase ordering and gate

Phases are ordered so the LLM-facing wins land first. Each phase touches <= 5 files and ends with verification. **Stop for approval between phases.**

- Phase 1 (F1): rule-free output tail. Highest fluency win.
- Phase 2 (F2): `cleanup` seed pack + activation docs.
- Phase 3 (F8): scope-omitted-equals-global.
- Phase 4 (F4 + F3): daemon `update --force` + npm verb split.
- Phase 5 (F5): deterministic pidfile location (diagnose-first).
- Phase 6 (F6): env-forward allowlist on spawn.
- Phase 7 (doctor setup brain + SUDO integration doc; F7 docs).

---

## Phase 1 -- F1: rule-free output tail

### Task 1.1: Probe whether `event_context` already returns arbitrary tail lines

**Files:**
- Read: `crates/daemon/src/router.rs`
- Read: `crates/daemon/src/ipc/protocol.rs`
- Read: `crates/mcp/src/tools.rs`

- [ ] **Step 1: Trace the existing `event_context` path**

Run (SymForge): `get_symbol_context` on `event_context` handler in
`crates/daemon/src/router.rs`, then read `EventContext` in
`crates/daemon/src/ipc/protocol.rs`.
Expected finding to record in the task notes: does `event_context` require an
existing matched event (a `seq`/pointer) as input? If YES (it needs a matched
event to anchor), it cannot serve unmatched output, so `command_output_tail`
is required. If it can already return the last N raw frames of a job with no
matched event, STOP and convert Tasks 1.2-1.5 into a docs-only task exposing
that path.

- [ ] **Step 2: Record the decision in the task checklist**

Write one line in this plan file under Task 1.1 stating "event_context
covers/does-not-cover rule-free tail" with the file:line evidence. Commit the
plan edit.

```bash
git add docs/superpowers/plans/2026-05-27-wsl-cleanup-dogfood-fixes.md
git commit -m "docs(plan): record event_context tail-coverage finding (F1)"
```

**FINDING (recorded 2026-05-27, grounded in code):**

`event_context` DOES NOT cover rule-free tail. It requires a matched event:
- `handle_event_context` (`crates/daemon/src/ipc/server.rs:731-869`) takes
  `EventContextParams { bucket_id, event_id, .. }`, scans the bucket for that
  `event_id`, and errors `EventNotFound` if absent. The window anchor is
  `event.pointer.frame_id`; output with no rule hit carries no pointer
  (`NoPointer`/`SyntheticEvent`, empty frames). No matched event => no anchor
  => no tail. **F1 (`command_output_tail`) is REQUIRED. Tasks 1.2-1.5 proceed.**

**IMPLEMENTATION CORRECTION (supersedes Task 1.3 Step 3 "frame store" guidance):**

The raw-line source is NOT the `command_status` store. `Cmd::status`
(`crates/daemon/src/command.rs:787-813`) returns counters only
(`frames_stdout: u64`) and is documented "Never returns raw text / No raw
stream content is ever copied." The actual raw frames live in the
**per-probe context ring**, which already exposes an anchor-free bounded tail:

```rust
// crates/core/src/context.rs:485
ContextRingManager::tail_frames(probe_id, max_lines, max_bytes)
    -> Result<RingTail, ContextError>
// RingTail { lines: Vec<String> (oldest-first), evicted_frames: u64, truncated: bool }
```

This is the same primitive the no-silence `CommandReceipt.tail` already uses
(TCE-ERG-1). So the daemon handler maps: `job_id -> JobRecord (router.jobs.get)
-> probe_id -> router.rings.tail_frames(probe_id, max_lines, max_bytes)`.
Router exposes rings via the `event_context` path (`router.rings`); add a thin
`Router::command_output_tail(job_id, max_lines, max_bytes)` helper alongside
`event_context` (`crates/daemon/src/router.rs:162`) that resolves probe_id from
the job and calls `rings.tail_frames`.

Two response-shape reconciliations vs the Task 1.2 DTO draft:
1. **No `stream` filter exists.** `RingTail`/`tail()` do not filter by stream.
   Either (a) drop `stream` from the request for v1 (simplest, matches the ring
   primitive), or (b) add a stream-filtered tail to the ring. Pick (a): omit
   `stream`; document "returns both streams interleaved in capture order."
   Update the Task 1.2 DTO to remove the `stream` field and the Task 1.4 MCP
   params accordingly.
2. **Truncation flags.** `RingTail` has one `truncated: bool` (byte cap) +
   `evicted_frames: u64`. Map: `truncated_bytes = RingTail.truncated`;
   `truncated_lines = (ring frame_count > max_lines)` computed in the handler
   (the ring's `frame_count(probe_id)` gives the total); surface
   `evicted_frames` so the agent knows earlier output was lost. Keep
   `returned_lines = lines.len()`.

Caps unchanged: clamp `max_lines` to 200, `max_bytes` to 65_536 server-side.

### Task 1.2: Add `command_output_tail` IPC request/response types

**Files:**
- Modify: `crates/daemon/src/ipc/protocol.rs`
- Test: `crates/daemon/src/ipc/protocol.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test (serde round-trip of the new request)**

Add to the protocol tests module:

```rust
#[test]
fn command_output_tail_request_roundtrips() {
    let req = CommandOutputTailRequest {
        job_id: "job_abc".to_owned(),
        max_lines: 50,
        max_bytes: 65_536,
        stream: Some("stdout".to_owned()),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CommandOutputTailRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p terminal-commanderd command_output_tail_request_roundtrips`
Expected: FAIL to compile -- `CommandOutputTailRequest` not defined.

- [ ] **Step 3: Define the request/response types**

Add to `crates/daemon/src/ipc/protocol.rs` next to the other command DTOs
(match the existing derive set used by sibling requests; defaults below mirror
the spec caps):

```rust
/// Rule-free bounded read of a job's captured output (F1). Returns the
/// last `max_lines` lines (hard cap enforced server-side) of the chosen
/// stream, truncation-flagged. Does NOT change suppression defaults.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommandOutputTailRequest {
    pub job_id: String,
    #[serde(default = "default_tail_lines")]
    pub max_lines: u32,
    #[serde(default = "default_tail_bytes")]
    pub max_bytes: u32,
    #[serde(default)]
    pub stream: Option<String>,
}

fn default_tail_lines() -> u32 { 50 }
fn default_tail_bytes() -> u32 { 65_536 }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommandOutputTailResponse {
    pub job_id: String,
    pub lines: Vec<String>,
    pub returned_lines: u32,
    pub truncated_lines: bool,
    pub truncated_bytes: bool,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p terminal-commanderd command_output_tail_request_roundtrips`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/ipc/protocol.rs
git commit -m "feat(ipc): add command_output_tail request/response DTOs (F1)"
```

### Task 1.3: Implement the daemon handler with hard caps

**Files:**
- Modify: `crates/daemon/src/router.rs`
- Test: `crates/daemon/tests/command_status_lifecycle.rs`

- [ ] **Step 1: Write the failing test**

Add to `crates/daemon/tests/command_status_lifecycle.rs` (reuse the harness
that starts a short command in that file; mirror its existing setup helper):

```rust
#[tokio::test]
async fn command_output_tail_returns_bounded_lines_without_a_rule() {
    // Start a command that prints several lines and matches NO rule.
    let h = start_test_daemon().await;
    let job = h.start_command(&["printf", "L1\\nL2\\nL3\\n"]).await;
    h.wait_exit(&job).await;

    let resp = h.command_output_tail(&job, 2, 65_536, Some("stdout")).await;
    assert_eq!(resp.returned_lines, 2);
    assert_eq!(resp.lines, vec!["L2".to_owned(), "L3".to_owned()]);
    assert!(resp.truncated_lines, "3 produced, 2 requested => truncated");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p terminal-commanderd command_output_tail_returns_bounded`
Expected: FAIL -- no `command_output_tail` route / helper.

- [ ] **Step 3: Implement the handler**

In `crates/daemon/src/router.rs`, add a route arm that reads the job's
captured frames from the existing per-job frame store (the same store
`command_status` uses for `frames_stdout`/`bytes_total`), filters by `stream`
when set, takes the last `max_lines` lines while accumulating bytes up to
`max_bytes`, and sets the truncation flags. Enforce server-side caps BEFORE
allocation:

```rust
let max_lines = req.max_lines.min(200);          // hard cap per spec
let max_bytes = req.max_bytes.min(65_536);       // policy file_window ceiling
// ... collect last `max_lines` lines of the selected stream,
// stop adding once running byte total would exceed `max_bytes`,
// set truncated_lines if produced > returned, truncated_bytes if
// the byte cap stopped collection.
```

Add the matching helper to the test harness in
`crates/daemon/tests/command_status_lifecycle.rs` so the test compiles.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p terminal-commanderd command_output_tail_returns_bounded`
Expected: PASS

- [ ] **Step 5: Verify caps with a second test**

Add and run:

```rust
#[tokio::test]
async fn command_output_tail_clamps_to_200_lines() {
    let h = start_test_daemon().await;
    let job = h.start_command(&["seq", "1", "500"]).await;
    h.wait_exit(&job).await;
    let resp = h.command_output_tail(&job, 10_000, 65_536, None).await;
    assert!(resp.returned_lines <= 200);
    assert!(resp.truncated_lines);
}
```

Run: `cargo test -p terminal-commanderd command_output_tail`
Expected: PASS (both)

- [ ] **Step 6: Commit**

```bash
git add crates/daemon/src/router.rs crates/daemon/tests/command_status_lifecycle.rs
git commit -m "feat(daemon): command_output_tail handler with line/byte caps (F1)"
```

### Task 1.4: Expose `command_output_tail` in the MCP adapter

**Files:**
- Modify: `crates/mcp/src/tools.rs`
- Test: `crates/mcp/tests/mcp_live_command_e2e.rs`

- [ ] **Step 1: Write the failing e2e test**

Add to `crates/mcp/tests/mcp_live_command_e2e.rs` (follow the existing live-
daemon e2e pattern in that file):

```rust
#[tokio::test]
async fn mcp_command_output_tail_reads_unmatched_output() {
    let mcp = spawn_live_mcp().await;
    let started = mcp.command_start_combed(&["printf", "A\\nB\\nC\\n"]).await;
    mcp.wait_for_exit(&started.job_id).await;
    let tail = mcp.command_output_tail(&started.job_id, 2, 65_536, None).await;
    assert_eq!(tail.lines, vec!["B".to_owned(), "C".to_owned()]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p terminal-commander-mcp mcp_command_output_tail_reads_unmatched`
Expected: FAIL -- tool not registered.

- [ ] **Step 3: Register the tool (1:1 forward to the daemon IPC method)**

In `crates/mcp/src/tools.rs`, add the tool following the existing
`command_status` registration exactly (same params-struct + forward pattern):
params `{ job_id: String, max_lines: Option<u32>, max_bytes: Option<u32>,
stream: Option<String> }`, forwarding to the `command_output_tail` IPC method.
Tool description (agent-selfish, matches house voice):

```text
Read the last N lines of a command's captured output WITHOUT authoring a rule.
Use this for one-off/exploratory commands whose format you don't know yet
(e.g. `df -h`, `docker system df`): start the command, then read its tail here.
Bounded: max 200 lines / 64 KiB, truncation-flagged. For recurring signals you
care about, define a rule instead so you get only the matching events.
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p terminal-commander-mcp mcp_command_output_tail_reads_unmatched`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/mcp/src/tools.rs crates/mcp/tests/mcp_live_command_e2e.rs
git commit -m "feat(mcp): expose command_output_tail tool (F1)"
```

### Task 1.5: Phase 1 verification

- [ ] **Step 1: Full touched-crate verification**

Run:
```bash
cargo fmt -p terminal-commanderd -p terminal-commander-mcp
cargo check -p terminal-commanderd -p terminal-commander-mcp
cargo clippy -p terminal-commanderd -p terminal-commander-mcp -- -D warnings
cargo test -p terminal-commanderd -p terminal-commander-mcp
```
Expected: all green.

- [ ] **Step 2: cargo clean (phase boundary)**

Run: `cargo clean`

- [ ] **Step 3: STOP for approval before Phase 2.**

---

## Phase 2 -- F2: `cleanup` seed pack + activation docs

### Task 2.1: Add the `cleanup` rule pack JSON inside the store crate

**Files:**
- Create: `crates/store/rules/cleanup.json`
- Modify: `crates/store/src/import.rs:60-71` (the `SEED_PACKS` table)
- Test: `crates/store/src/import.rs` (inline tests)

- [ ] **Step 1: Write the failing test (pack resolves by name)**

Add to the `import.rs` tests module:

```rust
#[test]
fn cleanup_pack_resolves_and_has_core_rules() {
    let json = resolve_pack_json("cleanup").expect("cleanup pack present");
    let parsed: RulePackFile = serde_json::from_str(json).unwrap();
    let ids: Vec<&str> = parsed.rules.iter().map(|r| r.id.as_str()).collect();
    for want in ["cleanup.disk-usage", "cleanup.dir-size",
                 "cleanup.docker-usage", "cleanup.fstrim",
                 "cleanup.space-reclaimed"] {
        assert!(ids.contains(&want), "missing {want}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p terminal-commander-store cleanup_pack_resolves`
Expected: FAIL -- unknown pack "cleanup".

- [ ] **Step 3: Create `crates/store/rules/cleanup.json`**

Use the CORRECT `${name}` template syntax (the dogfood used bare `{name}`,
which is literal -- see spec F7). Patterns are the ones proven live this
session:

```json
{
  "_meta": { "pack": "cleanup", "version": 1,
             "description": "Disk/cache/docker cleanup signal rules." },
  "rules": [
    {
      "id": "cleanup.disk-usage", "version": 1, "kind": "regex",
      "status": "active", "severity": "info", "event_kind": "disk_usage",
      "pattern": "(?P<fs>/dev/\\S+|tmpfs|overlay|none)\\s+(?P<size>\\S+)\\s+(?P<used>\\S+)\\s+(?P<avail>\\S+)\\s+(?P<pct>\\d+%)\\s+(?P<mount>/\\S*)",
      "captures": ["fs","size","used","avail","pct","mount"],
      "summary_template": "disk ${mount}: ${used}/${size} (${pct} used), ${avail} free",
      "tags": ["cleanup","disk"]
    },
    {
      "id": "cleanup.dir-size", "version": 1, "kind": "regex",
      "status": "active", "severity": "info", "event_kind": "dir_size",
      "pattern": "^(?P<size>[0-9.]+[KMGT]?)\\s+(?P<path>\\S.*)$",
      "captures": ["size","path"],
      "summary_template": "size ${size}: ${path}",
      "tags": ["cleanup","disk"]
    },
    {
      "id": "cleanup.docker-usage", "version": 1, "kind": "regex",
      "status": "active", "severity": "info", "event_kind": "docker_usage",
      "pattern": "^(?P<type>Images|Containers|Local Volumes|Build Cache)\\s+(?P<total>\\d+)\\s+(?P<active>\\d+)\\s+(?P<size>[0-9.]+\\s*[A-Za-z]+)\\s+(?P<reclaimable>[0-9.]+\\s*[A-Za-z]+)",
      "captures": ["type","total","active","size","reclaimable"],
      "summary_template": "docker ${type}: size ${size}, reclaimable ${reclaimable}",
      "tags": ["cleanup","docker"]
    },
    {
      "id": "cleanup.fstrim", "version": 1, "kind": "regex",
      "status": "active", "severity": "info", "event_kind": "fstrim",
      "pattern": "^(?P<mount>/\\S*):\\s+(?P<human>[0-9.]+\\s*[KMGT]?i?B)\\s+\\((?P<bytes>\\d+)\\s+bytes\\)\\s+trimmed",
      "captures": ["mount","human","bytes"],
      "summary_template": "trimmed ${mount}: ${human}",
      "tags": ["cleanup","fstrim"]
    },
    {
      "id": "cleanup.space-reclaimed", "version": 1, "kind": "regex",
      "status": "active", "severity": "info", "event_kind": "space_reclaimed",
      "pattern": "(?:Total reclaimed space|freed|trimmed|Freed):?\\s+(?P<amount>[0-9.]+\\s*[KMGT]?i?B)",
      "captures": ["amount"],
      "summary_template": "reclaimed ${amount}",
      "tags": ["cleanup"]
    }
  ]
}
```

- [ ] **Step 4: Register the pack in `SEED_PACKS`**

Edit `crates/store/src/import.rs` `SEED_PACKS` (currently 7 entries,
`import.rs:60-71`) to add:

```rust
    ("cleanup", include_str!("../rules/cleanup.json")),
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p terminal-commander-store cleanup_pack_resolves`
Expected: PASS

Also update `known_pack_names_lists_all_seven` (it asserts `len() == 7`):
change to 8 and rename to `known_pack_names_lists_all_eight`. Run:
`cargo test -p terminal-commander-store known_pack_names_lists_all_eight`
Expected: PASS

- [ ] **Step 6: Verify cargo publish still packages rules (the F-from-prior-session invariant)**

Run: `cargo publish -p terminal-commander-store --dry-run`
Expected: success; `crates/store/rules/cleanup.json` is in the package file
list (include_str path stays inside the crate -- do NOT reach outside).

- [ ] **Step 7: Commit**

```bash
git add crates/store/rules/cleanup.json crates/store/src/import.rs
git commit -m "feat(store): ship cleanup seed rule pack with correct \${name} templates (F2)"
```

### Task 2.2: Import-and-render integration test (proves rendered summary, F7 closure)

**Files:**
- Test: `crates/store/src/import.rs` (inline) OR `crates/daemon/tests/registry_ipc.rs`

- [ ] **Step 1: Write the failing test**

Add to `import.rs` tests (pure-store level, no daemon needed):

```rust
#[test]
fn cleanup_pack_imports_and_renders_a_summary() {
    let mut s = EventStore::in_memory().unwrap();
    let res = s.import_rule_pack_by_name("cleanup", true).unwrap();
    assert!(res.skipped.is_empty(), "skipped: {:?}", res.skipped);
    let r = s.get_latest_rule("cleanup.fstrim").unwrap().unwrap();
    // The rule's template uses ${...} so render substitutes values.
    let mut caps = indexmap::IndexMap::new();
    caps.insert("mount".to_owned(), "/".to_owned());
    caps.insert("human".to_owned(), "2.1 GiB".to_owned());
    let rendered = r.render_summary(&caps).unwrap();
    assert_eq!(rendered.0, "trimmed /: 2.1 GiB");
}
```

- [ ] **Step 2: Run, confirm fail then pass**

Run: `cargo test -p terminal-commander-store cleanup_pack_imports_and_renders`
Expected: FAIL first (if pack absent at compile -- it is present now, so this
may pass immediately; if it passes, that is acceptable: it locks the render
contract). Confirm the assertion on the rendered string is exercised.

- [ ] **Step 3: Commit**

```bash
git add crates/store/src/import.rs
git commit -m "test(store): cleanup pack imports and renders \${...} summary (F2/F7)"
```

### Task 2.3: Document activation-is-future-only in the tool description

**Files:**
- Modify: `crates/mcp/src/tools.rs` (the `registry_activate` tool description)

- [ ] **Step 1: Amend the `registry_activate` description**

Append to the existing `registry_activate` tool description string:

```text
Activation binds NEWLY started commands only; commands already running are not
hot-rebound. Pattern: activate the rule (or import the `cleanup` pack), THEN
start the command. To read output from a command you already started without a
matching rule, use command_output_tail.
```

- [ ] **Step 2: Verify it compiles and the description renders**

Run: `cargo test -p terminal-commander-mcp` (existing tool-listing tests cover
description presence; if a snapshot test exists, update it).
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/mcp/src/tools.rs
git commit -m "docs(mcp): registry_activate is future-only; point to cleanup pack + tail (F2)"
```

### Task 2.4: Phase 2 verification

- [ ] **Step 1:** Run `cargo fmt -p terminal-commander-store -p terminal-commander-mcp && cargo check -p terminal-commander-store -p terminal-commander-mcp && cargo clippy -p terminal-commander-store -p terminal-commander-mcp -- -D warnings && cargo test -p terminal-commander-store -p terminal-commander-mcp`. Expected: green.
- [ ] **Step 2:** `cargo clean`
- [ ] **Step 3: STOP for approval before Phase 3.**

---

## Phase 3 -- F8: omitted activation scope defaults to global

### Task 3.1: Decide and lock behavior against the existing test

**Files:**
- Read: `crates/daemon/tests/registry_scope_required.rs`
- Read: `crates/daemon/src/activation.rs`

- [ ] **Step 1: Read the existing contract**

Run (SymForge): `get_file_context` on
`crates/daemon/tests/registry_scope_required.rs` and find where `ScopeInvalid`
is produced in `crates/daemon/src/activation.rs`.
Record: is "scope required" an intentional, tested contract? The spec picks
option (a) default-to-global to match the published schema. If
`registry_scope_required.rs` asserts rejection ON PURPOSE for a safety reason
(e.g. preventing accidental global activation), do NOT silently flip it --
instead implement (b): keep rejection, fix the SCHEMA/description to require
scope, and update the MCP tool schema to make scope required. Choose based on
what the test's comment says. Write the chosen option (a or b) as a one-line
note here and commit the plan edit.

### Task 3.2a (if option a): default omitted scope to global

**Files:**
- Modify: `crates/daemon/src/activation.rs`
- Test: `crates/daemon/tests/registry_scope_required.rs` (repurpose/replace)

- [ ] **Step 1: Write the failing test**

Add a test asserting an omitted scope activates as global:

```rust
#[tokio::test]
async fn activate_with_omitted_scope_defaults_to_global() {
    let h = registry_test_daemon().await;
    h.upsert_rule(minimal_active_rule("t.kw")).await;
    let res = h.activate_no_scope("t.kw").await; // sends no `scope` field
    assert_eq!(res.scope.kind, "global");
    assert!(!res.was_already_active);
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p terminal-commanderd activate_with_omitted_scope_defaults`
Expected: FAIL with `ScopeInvalid`.

- [ ] **Step 3: Implement default**

In `crates/daemon/src/activation.rs`, where an absent scope currently returns
`ScopeInvalid`, substitute `ActivationScope::Global`. Keep the explicit-global
path identical.

- [ ] **Step 4: Reconcile the old test**

If `registry_scope_required.rs` asserted rejection, replace that assertion
with the new default-to-global expectation (the contract changed
deliberately, matching the published MCP schema). Update the test name and
file-level comment to reflect the new contract.

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p terminal-commanderd activate_with_omitted_scope_defaults`
Expected: PASS. Also run the full file:
`cargo test -p terminal-commanderd --test registry_scope_required`
Expected: PASS.

- [ ] **Step 6: Mirror in the MCP e2e**

Update `crates/mcp/tests/registry_scope_required_e2e.rs` to assert the omitted-
scope-equals-global behavior end-to-end. Run:
`cargo test -p terminal-commander-mcp --test registry_scope_required_e2e`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/daemon/src/activation.rs crates/daemon/tests/registry_scope_required.rs crates/mcp/tests/registry_scope_required_e2e.rs
git commit -m "fix(daemon): omitted activation scope defaults to global to match schema (F8)"
```

### Task 3.2b (if option b): make schema require scope

**Files:**
- Modify: `crates/mcp/src/tools.rs` (registry_activate params schema + description)

- [ ] **Step 1:** Change the `scope` param from optional to required in the MCP
params struct (remove the "omit = global" wording; state scope is required and
show `{kind:'global'}`). Keep the daemon `ScopeInvalid` behavior.
- [ ] **Step 2:** Update/add a test in
`crates/mcp/tests/registry_scope_required_e2e.rs` asserting a missing scope is
a clear input error before reaching the daemon. Run it. Expected: PASS.
- [ ] **Step 3: Commit** `git commit -m "fix(mcp): registry_activate requires explicit scope (F8 option b)"`

### Task 3.3: Phase 3 verification

- [ ] **Step 1:** `cargo fmt && cargo check -p terminal-commanderd -p terminal-commander-mcp && cargo clippy -p terminal-commanderd -p terminal-commander-mcp -- -D warnings && cargo test -p terminal-commanderd -p terminal-commander-mcp`. Expected: green.
- [ ] **Step 2:** `cargo clean`
- [ ] **Step 3: STOP for approval before Phase 4.**

---

## Phase 4 -- F4 + F3: daemon `update --force` and npm verb split

### Task 4.1: Add `--force` to the daemon `update` subcommand

**Files:**
- Modify: `crates/daemon/src/main.rs` (the `Cmd::Update` variant + `run_update`)
- Modify: `crates/supervisor/src/replace.rs` (add a force-replace entry)
- Test: `crates/supervisor/src/replace.rs` (inline tests)

- [ ] **Step 1: Write the failing unit test for forced staleness**

Add to `replace.rs` tests:

```rust
#[test]
fn force_replaces_even_when_versions_match() {
    // is_stale stays version-accurate; force is a separate flag, not a
    // staleness lie. This documents the contract.
    assert!(!is_stale("0.1.18", "0.1.18"));
    assert!(should_replace(/*stale=*/false, /*force=*/true));
    assert!(should_replace(true, false));
    assert!(!should_replace(false, false));
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p terminal-commander-supervisor force_replaces_even_when`
Expected: FAIL -- `should_replace` not defined.

- [ ] **Step 3: Implement `should_replace` + thread `force` through**

In `crates/supervisor/src/replace.rs` add:

```rust
/// Replace when the running daemon is stale OR the caller forces it.
#[must_use]
pub fn should_replace(stale: bool, force: bool) -> bool { stale || force }
```

Add a `force: bool` parameter to `replace_if_stale` (or a sibling
`replace_daemon(opts, installed, force)`), and at the `UpToDate` branch, when
`force` is true, fall through to the kill path instead of returning
`UpToDate`. The kill path is unchanged (pidfile pid, else OS-query scoped to
`--data-dir`; never blind name kill).

- [ ] **Step 4: Add the CLI flag**

In `crates/daemon/src/main.rs`, change `Update` to:

```rust
    /// Replace a stale (or, with --force, any) running daemon, then ensure
    /// a current daemon is running.
    Update {
        /// Replace even when the running version equals this binary.
        #[arg(long)]
        force: bool,
    },
```

Thread `force` into `run_update(&cfg, force)` and pass to the replace call.

- [ ] **Step 5: Run unit test to verify pass**

Run: `cargo test -p terminal-commander-supervisor force_replaces_even_when`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/supervisor/src/replace.rs crates/daemon/src/main.rs
git commit -m "feat(daemon): update --force replaces a same-version daemon (F4)"
```

### Task 4.2: npm verb split -- `restart`, `upgrade`, deprecate `update`

**Files:**
- Modify: `packages/terminal-commander/bin/terminal-commander.js` (or the lib it calls)
- Test: `packages/terminal-commander/test/` (add `cli-verbs.test.js`)

- [ ] **Step 1: Find the current `update` dispatch**

Run (Grep): search `packages/terminal-commander` for the `update` verb
handler (the one that runs `npm install`). Record the file:line.

- [ ] **Step 2: Write the failing Node test**

Create `packages/terminal-commander/test/cli-verbs.test.js`:

```js
const { test } = require("node:test");
const assert = require("node:assert");
const { resolveVerb } = require("../lib/cli/verbs.js");

test("restart proxies to daemon update --force", () => {
  const v = resolveVerb("restart");
  assert.equal(v.kind, "daemon");
  assert.deepEqual(v.daemonArgs, ["update", "--force"]);
});

test("upgrade runs npm self-update", () => {
  assert.equal(resolveVerb("upgrade").kind, "npm-upgrade");
});

test("update is a deprecated alias that warns and points to restart/upgrade", () => {
  const v = resolveVerb("update");
  assert.equal(v.deprecated, true);
  assert.match(v.notice, /restart|upgrade/);
});
```

- [ ] **Step 3: Run to verify fail**

Run: `node --test packages/terminal-commander/test/cli-verbs.test.js`
Expected: FAIL -- `lib/cli/verbs.js` missing.

- [ ] **Step 4: Implement `resolveVerb`**

Create `packages/terminal-commander/lib/cli/verbs.js`:

```js
"use strict";
function resolveVerb(verb) {
  switch (verb) {
    case "restart":
      return { kind: "daemon", daemonArgs: ["update", "--force"] };
    case "upgrade":
      return { kind: "npm-upgrade" };
    case "update":
      return {
        kind: "daemon",
        daemonArgs: ["update", "--force"],
        deprecated: true,
        notice:
          "terminal-commander: `update` is deprecated. Use `restart` to " +
          "replace the running daemon, or `upgrade` to update the npm package.",
      };
    default:
      return { kind: "unknown", verb };
  }
}
module.exports = { resolveVerb };
```

Wire `bin/terminal-commander.js` to call `resolveVerb`, print `notice` when
`deprecated`, and dispatch: `daemon` -> spawn `terminal-commanderd` with
`daemonArgs`; `npm-upgrade` -> the existing npm-install path.

- [ ] **Step 5: Run to verify pass**

Run: `node --test packages/terminal-commander/test/cli-verbs.test.js`
Expected: PASS

- [ ] **Step 6: Confirm av-safe invariant unbroken**

Run: `node --test packages/terminal-commander/test/av-safe-install-runtime.test.js`
Expected: PASS (no lifecycle scripts added; we only added a lib + verb routing).

- [ ] **Step 7: Commit**

```bash
git add packages/terminal-commander/lib/cli/verbs.js packages/terminal-commander/bin/terminal-commander.js packages/terminal-commander/test/cli-verbs.test.js
git commit -m "feat(cli): split restart/upgrade; deprecate update alias (F3)"
```

### Task 4.3: Phase 4 verification

- [ ] **Step 1:** `cargo fmt && cargo check -p terminal-commanderd -p terminal-commander-supervisor && cargo clippy -p terminal-commanderd -p terminal-commander-supervisor -- -D warnings && cargo test -p terminal-commanderd -p terminal-commander-supervisor`. Expected: green.
- [ ] **Step 2:** `node --test packages/terminal-commander/test/*.test.js`. Expected: all pass.
- [ ] **Step 3:** `cargo clean`
- [ ] **Step 4: STOP for approval before Phase 5.**

---

## Phase 5 -- F5: deterministic pidfile location (diagnose first)

### Task 5.1: Diagnose where writer vs reader resolve `state_dir` on Windows install

**Files:**
- Read: `crates/daemon/src/runtime.rs` (writer: `write_daemon_pidfile` call site)
- Read: `crates/supervisor/src/replace.rs` (reader: `read_pidfile(&opts.state_dir)`)
- Read: `crates/supervisor/src/paths.rs` (`resolve_state_dir`)
- Read: `crates/daemon/src/main.rs` (`platform_default_data_dir`, `run_update`)

- [ ] **Step 1: Trace both paths**

Record in this plan the exact `state_dir` each side uses:
- Writer (daemon `start`): `cfg.daemon.data_dir` -> what does that resolve to
  on a Windows npm install with no `--config`? (`platform_default_data_dir`
  returns `USERPROFILE\.terminal-commanderd` when `HOME` is unset.)
- Reader (`run_update`): `cfg.daemon.data_dir` too -- BUT the adapter spawns
  the daemon with what `--data-dir`? Check `crates/mcp` spawn args and
  `resolve_state_dir()` in `paths.rs`.
Hypothesis to confirm: the adapter spawns the daemon WITHOUT `--data-dir`, so
the daemon writes under `platform_default_data_dir`, while the supervisor's
`EnsureDaemonOptions.state_dir` is built from `resolve_state_dir()` -- and the
two differ (e.g. `.terminal-commanderd` vs `.local/share/terminal-commanderd`
or an AppData path). Write the confirmed mismatch here. Commit the plan note.

### Task 5.2: Make writer and reader share one resolved path

**Files:**
- Modify: `crates/supervisor/src/paths.rs` (export a single `resolve_state_dir`)
- Modify: `crates/daemon/src/runtime.rs` (write pidfile under the SAME resolution)
- Test: `crates/supervisor/tests/` (add `pidfile_path_agrees.rs`) or inline in `paths.rs`

- [ ] **Step 1: Write the failing test**

Add a test asserting the daemon's pidfile dir equals the supervisor's read
dir under installed-mode defaults (no `--config`, no `--data-dir`):

```rust
#[test]
fn writer_and_reader_resolve_same_state_dir_installed_mode() {
    // Simulate installed mode: clear HOME, set USERPROFILE.
    let reader = terminal_commander_supervisor::paths::resolve_state_dir();
    let writer = daemon_default_state_dir(); // expose the daemon's resolver
    assert_eq!(reader, writer,
        "pidfile writer and reader must agree on state_dir");
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p terminal-commander-supervisor writer_and_reader_resolve_same`
Expected: FAIL (paths differ) -- this is the bug reproduced.

- [ ] **Step 3: Unify resolution**

Make the daemon's default data dir come from the SAME
`terminal_commander_supervisor::paths::resolve_state_dir()` the supervisor
uses (or move the resolver to a shared spot both depend on). The daemon's
`platform_default_data_dir` must return that exact path in installed mode.
Keep `--data-dir`/`--config` overrides working.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p terminal-commander-supervisor writer_and_reader_resolve_same`
Expected: PASS

- [ ] **Step 5: Add a regression test that update finds the pidfile**

Add an integration test (or extend an existing supervisor test) that: writes a
pidfile via the daemon resolver, then `read_pidfile(resolve_state_dir())`
returns it. Run it. Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/supervisor/src/paths.rs crates/daemon/src/runtime.rs crates/daemon/src/main.rs
git commit -m "fix(daemon): pidfile writer and reader share one resolved state_dir (F5)"
```

### Task 5.3: Phase 5 verification

- [ ] **Step 1:** `cargo fmt && cargo check -p terminal-commanderd -p terminal-commander-supervisor && cargo clippy ... -- -D warnings && cargo test -p terminal-commanderd -p terminal-commander-supervisor`. Expected: green.
- [ ] **Step 2:** `cargo clean`
- [ ] **Step 3: STOP for approval before Phase 6.**

---

## Phase 6 -- F6: env-forward allowlist on daemon spawn

### Task 6.1: Forward a fixed allowlist of host vars into the spawned daemon

**Files:**
- Modify: `crates/supervisor/src/ensure.rs` (the spawn `Command` builder)
- Test: `crates/supervisor/src/ensure.rs` (inline) or `crates/supervisor/tests/`

- [ ] **Step 1: Define the allowlist (NEVER "forward everything")**

The allowlist is operational, non-secret only. Per spec F6, NO password var.
Start with WSL-forwarding-relevant operational vars:

```rust
/// Host env vars the supervisor re-reads at spawn and forwards into the
/// daemon, so a respawn picks up freshly-set values without a full client
/// restart. Fixed allowlist; never secrets, never a password.
pub const FORWARDED_ENV_ALLOWLIST: &[&str] = &["WSLENV", "TC_WSL_DISTRO"];
```

- [ ] **Step 2: Write the failing test**

```rust
#[test]
fn spawn_env_includes_only_allowlisted_host_vars() {
    std::env::set_var("WSLENV", "FOO/u");
    std::env::set_var("SECRET_THING", "nope");
    let env = build_forward_env(); // pure helper that reads allowlist
    assert_eq!(env.get("WSLENV").map(String::as_str), Some("FOO/u"));
    assert!(!env.contains_key("SECRET_THING"));
}
```

- [ ] **Step 3: Run to verify fail**

Run: `cargo test -p terminal-commander-supervisor spawn_env_includes_only_allow`
Expected: FAIL -- `build_forward_env` missing.

- [ ] **Step 4: Implement `build_forward_env` and apply at spawn**

```rust
#[must_use]
pub fn build_forward_env() -> std::collections::BTreeMap<String, String> {
    FORWARDED_ENV_ALLOWLIST.iter().filter_map(|k| {
        std::env::var(*k).ok().map(|v| ((*k).to_owned(), v))
    }).collect()
}
```

In the spawn builder, call `.env(k, v)` for each entry of
`build_forward_env()` (in addition to inherited env). Document that this lets
a `restart` pick up a freshly-set `WSLENV` without a full client restart.

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p terminal-commander-supervisor spawn_env_includes_only_allow`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/supervisor/src/ensure.rs
git commit -m "feat(supervisor): forward allowlisted host env vars on daemon spawn (F6)"
```

### Task 6.2: Phase 6 verification

- [ ] **Step 1:** `cargo fmt && cargo check -p terminal-commander-supervisor && cargo clippy -p terminal-commander-supervisor -- -D warnings && cargo test -p terminal-commander-supervisor`. Expected: green.
- [ ] **Step 2:** `cargo clean`
- [ ] **Step 3: STOP for approval before Phase 7.**

---

## Phase 7 -- doctor setup brain + SUDO integration doc

### Task 7.1: Doctor detects setup prerequisites and prints exact fix lines

**Files:**
- Modify: `crates/cli/src/main.rs` (`doctor_checks`, `run_doctor`)
- Test: `crates/cli/src/main.rs` (inline tests; mirror the existing
  `doctor_installed_mode_*` tests)

- [ ] **Step 1: Write failing tests for the new checks**

Add to the `cli` tests module:

```rust
#[test]
fn doctor_reports_missing_sudoers_with_exact_fix_line() {
    // Pure check function takes injected facts, returns (label, ok, fix).
    let facts = SetupFacts { wsl_present: true, sudoers_ok: false,
        daemon_fresh: true, cleanup_pack_present: true };
    let checks = setup_checks(&facts);
    let sudoers = checks.iter().find(|c| c.label.contains("sudo")).unwrap();
    assert!(!sudoers.ok);
    assert!(sudoers.fix.contains("/etc/sudoers.d/tc-cleanup"));
    assert!(sudoers.fix.contains("visudo -c"));
}

#[test]
fn doctor_all_green_when_setup_complete() {
    let facts = SetupFacts { wsl_present: true, sudoers_ok: true,
        daemon_fresh: true, cleanup_pack_present: true };
    assert!(setup_checks(&facts).iter().all(|c| c.ok));
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p terminal-commander doctor_reports_missing_sudoers`
Expected: FAIL -- `SetupFacts`/`setup_checks` missing.

- [ ] **Step 3: Implement `SetupFacts`, `setup_checks`, and detection**

```rust
pub struct SetupFacts {
    pub wsl_present: bool,
    pub sudoers_ok: bool,        // `sudo -n fstrim --version` succeeds in WSL
    pub daemon_fresh: bool,      // running version == this binary
    pub cleanup_pack_present: bool,
}

pub struct SetupCheck { pub label: String, pub ok: bool, pub fix: String }

#[must_use]
pub fn setup_checks(f: &SetupFacts) -> Vec<SetupCheck> {
    let mut v = Vec::new();
    if f.wsl_present {
        v.push(SetupCheck {
            label: "WSL sudo cleanup grant (sudoers)".into(),
            ok: f.sudoers_ok,
            fix: concat!(
              "Run in WSL once: echo \"$USER ALL=(root) NOPASSWD: ",
              "/usr/bin/apt-get, /usr/bin/journalctl, /usr/sbin/fstrim\" ",
              "| sudo tee /etc/sudoers.d/tc-cleanup && sudo chmod 440 ",
              "/etc/sudoers.d/tc-cleanup && sudo visudo -c -f ",
              "/etc/sudoers.d/tc-cleanup").into(),
        });
    }
    v.push(SetupCheck {
        label: "daemon up-to-date".into(), ok: f.daemon_fresh,
        fix: "Run: terminal-commander restart".into() });
    v.push(SetupCheck {
        label: "cleanup rule pack available".into(),
        ok: f.cleanup_pack_present,
        fix: "Import: registry_import_pack name=cleanup (or it ships built-in)".into() });
    v
}
```

The detection layer (probing WSL/sudoers) lives behind a function that, in
installed mode, runs the same `sudo -n fstrim --version` probe through the
daemon path; keep it side-effect-free and fail-open (a probe error = check
shows MISSING with the fix line, never a crash). Print fix lines under each
MISSING check in `run_doctor`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p terminal-commander doctor_reports_missing_sudoers doctor_all_green_when_setup_complete`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/main.rs
git commit -m "feat(cli): doctor detects setup gaps and prints exact fix lines (doctor-as-setup-brain)"
```

### Task 7.2: SUDO discipline integration doc

**Files:**
- Create: `docs/integrations/wsl-cleanup-and-sudo.md`

- [ ] **Step 1: Write the doc**

Create `docs/integrations/wsl-cleanup-and-sudo.md` containing, verbatim, the
"SUDO setup discipline" section from the spec (the scoped NOPASSWD sudoers
one-liner, the `chmod 440` + `visudo -c` rules, the `sudo -n` usage, the
explicit rejection of the env-var/`sudo -S` approach), PLUS:
- the `${name}` template-syntax note (F7): rules use `${name}`; bare `{name}`
  is literal text.
- the "activate THEN start; or import the `cleanup` pack; or use
  command_output_tail" guidance (F1/F2).
- a note that fstrim returns blocks but the host `.vhdx` only shrinks via
  `wsl --shutdown` + `Optimize-VHD`/diskpart (out of scope, manual).

- [ ] **Step 2: Link it from the main README/docs index**

Add a one-line link in the docs index (find it via Grep for existing
`docs/integrations` links) pointing to the new file.

- [ ] **Step 3: Commit**

```bash
git add docs/integrations/wsl-cleanup-and-sudo.md
git commit -m "docs: WSL cleanup + scoped NOPASSWD sudo discipline (F6/F7/SUDO)"
```

### Task 7.3: Phase 7 verification

- [ ] **Step 1:** `cargo fmt -p terminal-commander && cargo check -p terminal-commander && cargo clippy -p terminal-commander -- -D warnings && cargo test -p terminal-commander`. Expected: green.
- [ ] **Step 2:** `cargo clean`
- [ ] **Step 3: Final acceptance pass** -- walk the spec's 8 acceptance
criteria, confirm each maps to a landed task + passing test. Record any gap.

---

## Self-review notes (author)

- Spec coverage: F1->Phase1, F2->Phase2, F3->Task4.2, F4->Task4.1, F5->Phase5,
  F6->Phase6, F7->reclassified (Task2.1/2.2 correct-syntax pack + Task7.2 doc;
  no render code change -- `sifters/src/lib.rs:386` already renders), F8->Phase3,
  SUDO discipline->Task7.2, doctor-as-setup-brain->Task7.1. All 8 acceptance
  criteria mapped.
- Placeholder scan: every code step shows code; every command shows expected
  output. The two diagnose-first tasks (1.1, 5.1) intentionally produce a
  recorded finding before edits -- that is a deliberate gate, not a placeholder.
- Type consistency: `CommandOutputTailRequest/Response`, `should_replace`,
  `build_forward_env`, `FORWARDED_ENV_ALLOWLIST`, `SetupFacts`, `setup_checks`,
  `SetupCheck`, `resolveVerb` are each defined once and reused by name in tests.
- Known unknowns flagged for the executor: Task1.1 may collapse Phase1 to
  docs-only if `event_context` already covers tail; Task3.1 picks option a vs b
  from the existing test's intent; Task5.1 confirms the exact path mismatch
  before editing. None of these block starting.
