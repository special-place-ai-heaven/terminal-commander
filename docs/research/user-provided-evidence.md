# User-Provided Evidence Sweep

Author: research agent R2-delta
Date: 2026-05-21
Scope: TC01 blind-spot H2 (user-provided design context sweep)

Purpose: every constraint the user has already authored in the
project repository or in the Obsidian vault. Anything listed here
must be reclassified from "inference" to "user-provided" in
SOURCE_MAP.

Plain ASCII. file:line citations where the source is local. URL
citations where the source is a vault note.

---

## Method

Sources searched:

1. `C:\AI_STUFF\PROGRAMMING\terminal-commander\README.md` (re-read
   carefully - it is itself user-provided design context).
2. Obsidian vault `C:\Users\poslj\claude-obsidian\primus\primus-cloud`,
   grepped for: terminal commander, terminal-commander, signal comb,
   sifter, bucket, probe, rmcp.
3. `.agent/goals/terminal-commander-mvp/SOURCE_MAP.md` and
   `ASSUMPTIONS.md` (already authored by the user / planning session;
   re-read to confirm what is user-provided vs. inferred).
4. `.agent/goals/terminal-commander-mvp/TC04-rust-workspace-and-toolchain-scaffold.md`
   (user-authored goal file - canonical crate list).

Vault matches with substantive content related to TC: zero hits on
"terminal commander," "signal comb," or "bucket" as TC concepts.
The only relevant vault note already pre-cited in this research
pass is:

- `C:\Users\poslj\claude-obsidian\primus\primus-cloud\wiki\sources\RMCP rust-sdk.md`
  - frontmatter: `url: https://github.com/modelcontextprotocol/rust-sdk`
  - claim: "rmcp v0.16.0 workspace is edition 2024" (line 9-12)
  - claim: "rmcp has a streamable HTTP session module" (line 11)
  - This note was authored before the TC project existed; it is
    user-provided REFERENCE evidence, not a TC-specific decision.

Vault note `wiki/sources/Plasmate v0.5.1 Cargo and MCP sessions.md`
also references rmcp but is about an unrelated project (Plasmate);
not TC design evidence.

---

## User-provided constraints from README.md

File: `C:\AI_STUFF\PROGRAMMING\terminal-commander\README.md`

### UP-R-01. Product identity

- Source: README.md:3 "Terminal Commander is a local, MCP-operated
  terminal and file signal-combing layer for coding agents and LLM
  harnesses."
- Constraint: TC is LOCAL, MCP-OPERATED, targets terminal+file
  signal combing, and serves coding agents / LLM harnesses.
- Implication: provider-neutral MCP server is non-negotiable. Not a
  Cursor plugin. Not a Claude-Code-only tool. Not a SaaS.

### UP-R-02. Core inversion

- Source: README.md:5-11 "Its purpose is to replace noisy, expensive,
  periodic terminal polling with continuous local observation and
  small, structured, real-time signal events." Plus the I/O diagram:
  "Raw terminal/file output goes in. Only vetted, relevant signal
  comes out. Context remains available by pointer."
- Constraint: the architecture must invert the agent-tails-output
  pattern. Continuous streaming in, structured signal out, context
  available only via bounded pointer lookups.
- Implication: no MCP tool may return raw unbounded output as a
  success path. Already reflected in ASSUMPTIONS.md:22 and in the
  invariant set across the TCxx goal files.

### UP-R-03. MVP behavioral surface

- Source: README.md:17-26 (status section MVP list).
- The MVP must:
  1. run a shell command through a local daemon,
  2. continuously process stdout/stderr line- or frame-by-frame,
  3. apply dynamic keyword, regex, and condition-based sifters,
  4. store matching signal events in cursor-based buckets,
  5. expose those buckets through an MCP server,
  6. allow an LLM to read only new signal since a cursor,
  7. allow bounded context lookup around a signal event,
  8. maintain a dynamic registry of reusable sifter rules.
- Implication: each item 1-8 is a hard MVP acceptance criterion.

### UP-R-04. Probe taxonomy

- Source: README.md:124-130.
- User-provided probe types:
  - `process_probe`
  - `terminal_probe` (PTY, shell, tmux pane, interactive stream)
  - `file_probe`
  - `directory_probe`
  - `journal_probe` (systemd/journal where allowed)
  - `artifact_probe` (JUnit XML, coverage JSON)
- Implication: identifier names are user-fixed. Code should not
  silently rename them. Implementation order is goal-defined
  (TC15/18/19/20).

### UP-R-05. Sifter taxonomy

- Source: README.md:136-148.
- User-provided sifter types:
  - keyword
  - regex
  - numeric condition
  - multiline block
  - progress detector
  - prompt detector
  - stall detector
  - dedupe rule
  - suppression rule
  - correlation rule
  - artifact parser
- Constraint: README.md:150 "Sifters are stored in a dynamic
  registry so an LLM can search, select, create, test, version,
  and activate them at runtime."
- Implication: TC09/TC10/TC11/TC13/TC14 cover this; runtime
  mutability is a hard requirement.

### UP-R-06. Signal bucket schema

- Source: README.md:152-197 (bucket description and example event
  shape).
- A bucket contains:
  - monotonic sequence numbers
  - timestamps
  - severity
  - event kind
  - summaries
  - extracted fields
  - source pointers
  - context references
  - dedupe metadata
  - noise-suppression statistics
- Example event shape (README.md:173-197) names fields:
  `event_id`, `bucket_id`, `seq`, `timestamp`, `severity` (sample
  value "high"), `kind` (sample value "missing_package"), `summary`,
  `captures` (object), `source` (`probe_id`, `source_type`,
  `stream`, `job_id`), `pointer` (`frame_id`, `line`,
  `context_available`).
- Implication: TC05 / TC06 schema work has a near-canonical
  user-provided shape to honor. Field names and event_id ULID-style
  prefix are user-defined.

### UP-R-07. Context-by-pointer primitive

- Source: README.md:200-209.
- User-named primitive: `event_context(event_id, before=3, after=5)`
- Implication: bounded context lookup is a first-class tool. TC08
  covers it.

### UP-R-08. Intended architecture diagram

- Source: README.md:213-239.
- Topology: LLM/agent -> MCP -> terminal-commander-mcp -> local API
  -> terminal-commanderd -> probes (process/terminal/file/directory)
  -> Live Signal Comber (sifter registry + sifter runtime +
  context spool + policy engine) -> signal buckets.
- Implication: two-process split (MCP server + daemon) is
  USER-PROVIDED, not inferred. R1-alpha's open question on
  "Two-process vs single-process architecture" was identified as a
  decision to escalate - per this README citation, the user has
  already DECIDED two-process. This is a SOURCE_MAP reclassification.

### UP-R-09. MCP tool surface (initial candidates)

- Source: README.md:243-266 (Planned MCP tool surface).
- User-provided tool name candidates:
  - `system_discover`
  - `policy_status`
  - `command_start_combed`
  - `command_status`
  - `command_write_stdin`
  - `command_send_signal`
  - `bucket_create`
  - `bucket_events_since`
  - `bucket_wait`
  - `event_context`
  - `probe_create`
  - `probe_bind_rules`
  - `registry_search`
  - `registry_get`
  - `registry_create`
  - `registry_test`
  - `registry_activate`
  - `file_read_window`
  - `file_search`
  - `file_watch`
- Source: README.md:268 "The most important tool is `bucket_wait`."
- Source: README.md:272-281 - sample `bucket_wait` request payload:
  `{ "bucket": "build_42", "cursor": 1842, "severity_min": "medium",
  "timeout_ms": 30000 }` and the rule "If no relevant signal appears,
  the response should be a heartbeat, not a raw output dump."
- Implication: TC17 (`bucket_wait`) is the keystone MCP tool. The
  user has named the priority and the response semantics
  (heartbeat over raw output).

### UP-R-10. Safety model

- Source: README.md:284-297.
- User-provided rules:
  - MCP server should be unprivileged where possible.
  - Daemon/helper may be privileged only when configured.
  - All command execution must pass policy checks.
  - File access must respect allowed and denied paths.
  - Risky operations must be auditable.
  - Raw secret-bearing files must be denied by default.
  - No LLM-facing interface should become an unrestricted root shell.
  - Default-denied sensitive areas should include: private keys,
    password files, credential stores, token caches (unless
    explicitly allowed by policy).
- Implication: TC02 doctrine is grounded in these user-provided
  rules. Privilege separation between MCP server and daemon is a
  user-stated invariant - this aligns with and confirms the
  two-process split in UP-R-08.

### UP-R-11. Development discipline

- Source: README.md:301-311.
- Goal files must be: branch-safe, evidence-driven, narrowly scoped,
  independently verifiable, small enough for one autonomous agent
  run, explicit about allowed and forbidden files, clear about stop
  conditions and acceptance criteria.
- Implication: confirms the goal-file template already used in
  `.agent/goals/terminal-commander-mvp/`.

### UP-R-12. Goal chain ordering (user-named)

- Source: README.md:312-331.
- User has explicitly ordered the expected chain start:
  1. repository bootstrap
  2. architecture and security specification
  3. test methodology
  4. core schemas
  5. event store
  6. bucket manager
  7. context ring
  8. sifter runtime
  9. registry database
  10. process probe
  11. MCP server
  12. `bucket_wait`
  13. file probe
  14. rule packs
  15. policy engine
  16. installer and WSL support
  17. integration validation
- Implication: the existing TC01..TC30 chain implements this
  ordering (with extra splitting). User has implicitly authorized
  the expanded breakdown.

### UP-R-13. Repository layout (planned)

- Source: README.md:347-378.
- User-planned layout names these directories:
  - `.agent/goals/terminal-commander-mvp/`
  - `crates/terminal-commander-core/`
  - `crates/terminal-commanderd/`
  - `crates/terminal-commander-mcp/`
  - `crates/terminal-commander-probes/`
  - `crates/terminal-commander-sifters/`
  - `crates/terminal-commander-cli/`
  - `config/terminal-commander.example.toml`
  - `config/policy.example.toml`
  - `rules/generic.terminal.json`, `apt.json`, `cargo.json`,
    `npm.json`, `pytest.json`, `gcc.json`
  - `tests/fixtures/`, `tests/integration/`, `tests/load/`
- Source caveat: README.md:380 "The exact layout should be confirmed
  by the initial bootstrap and architecture goals before
  implementation begins."
- **CRATE COUNT CONTRADICTION**: README lists SIX crates
  (README.md:354-360). TC04 goal file
  (`TC04-rust-workspace-and-toolchain-scaffold.md`:87-93) lists
  SEVEN crates - it adds `crates/terminal-commander-store/` which
  is NOT in README.md. This is a discrepancy that R2-delta
  surfaces, per task instruction. See contradiction section below.

### UP-R-14. License posture

- Source: README.md:384-386 "License is not selected yet. Choose and
  add a license through an explicit goal before publishing
  implementation code for wider reuse."
- Task pre-confirmed context says "License: Apache-2.0." This is
  the parent agent's pre-confirmed decision, not a README claim.
  R2-delta flags this as a known divergence: README still says "not
  selected." Treat Apache-2.0 as the active orchestration decision,
  but a `LICENSE` file + README update should occur in a license
  goal before public publication.

---

## User-provided constraints from `.agent/goals/`

### UP-G-01. Repository identity

- Source: `SOURCE_MAP.md`:5
  "Repository URL: `https://github.com/special-place-administrator/terminal-commander.git`"
- Source: `ASSUMPTIONS.md`:5 same URL, confirmed by user.
- Implication: canonical remote. Already pre-confirmed in the
  parent task brief.

### UP-G-02. Branch policy

- Source: `ASSUMPTIONS.md`:7-10
  - `target_branch: feature/terminal-commander-mvp`
  - `prohibited_branches: ["main", "master"]`
- Source: every TCxx goal file repeats this in frontmatter and the
  Branch Guard block.
- Implication: hard policy. R2-delta does not touch git.

### UP-G-03. Repository state at start

- Source: `SOURCE_MAP.md`:16-17 "User reports the GitHub repository
  has been created and the initial README.md has been added."
- Source: `ASSUMPTIONS.md`:6 same claim.
- Implication: anything beyond `README.md` plus the `.agent/`
  scaffold plus prior `docs/research/` deliverables was generated
  during the goal chain or by research agents.

### UP-G-04. Goal-run mode

- Source: `ASSUMPTIONS.md`:11 "Goal files should be run linearly
  with `/goal`."
- Source: `ASSUMPTIONS.md`:12 "Work mode: mixed planning and
  implementation goals."
- Implication: TC01 is planning, TCxx code goals come later.

### UP-G-05. Stack assumptions (architect, not yet user-confirmed)

- Source: `ASSUMPTIONS.md`:14-22 lists architect assumptions to
  verify during goals. These are NOT user-provided. They are the
  starting hypothesis set:
  - Rust workspace.
  - SQLite or equivalent embedded store.
  - Provider-neutral local MCP server.
  - No privileged helper until policy allows.
  - No destructive migrations.
  - Linux + WSL primary targets.
  - No unbounded raw output by default.
- Note: items now substantially evidence-backed by `_R1-alpha-summary.md`
  and `_R1-beta-summary.md` (Rust + tokio + rmcp + rusqlite +
  notify + Linux/WSL). They graduate from "inference" to
  "evidence-backed" but not to "user-provided." The user did NOT
  literally specify Rust in the README.

### UP-G-06. Crate names (canonical, per TC04)

- Source: `TC04-rust-workspace-and-toolchain-scaffold.md`:87-93 and
  contracts at line 102.
- Canonical SEVEN crate names:
  - terminal-commander-core
  - terminal-commander-sifters
  - terminal-commander-probes
  - terminal-commander-store
  - terminal-commanderd
  - terminal-commander-mcp
  - terminal-commander-cli
- Discrepancy: README.md:354-360 names only SIX crates - omits
  terminal-commander-store. The store crate exists in TC04's
  intent but not in README. R2-delta flags this for resolution.
  See SOURCE_MAP reclassifications below.

---

## User-provided REFERENCE notes from the Obsidian vault

### UP-V-01. RMCP rust-sdk source

- Source: `C:\Users\poslj\claude-obsidian\primus\primus-cloud\wiki\sources\RMCP rust-sdk.md`
- Frontmatter URL: `https://github.com/modelcontextprotocol/rust-sdk`
- Confidence: `high`
- Key claims (line 9-12):
  - rmcp v0.16.0 workspace is edition 2024
  - rmcp has a streamable HTTP session module
  - No rmcp ICE issue match was found for the search terms used
- Status: not authored for TC; pre-existing vault evidence.
  R1-alpha already cross-referenced this and found that rmcp 1.7.0
  is the current crates.io latest, also edition 2024. This vault
  note pins TC's evidence to rmcp 0.16.0 specifically.
- Implication: rmcp version pin is an open architect decision per
  `_R1-alpha-summary.md` "Items REQUIRES USER DECISION" #1. The
  vault note constitutes pre-existing user-provided evidence that
  the architect was working from 0.16.0 at the moment of TC
  authoring.

### Other vault hits (not substantive)

The vault grep also returned `wiki/log.md`, `wiki/index.md`,
`wiki/sources/_index.md`, `wiki/entities/SymForge.md`,
`wiki/entities/Agent-Army-Professionals.md`, and
`wiki/questions/Research - Plasmate ICE check_mod_deathness.md`.
Spot reading confirms these reference "rmcp," "sifter" (in an
unrelated AAP / Plasmate context), or "probe" only incidentally
and contain no TC-specific design constraints. They are NOT
user-provided TC evidence.

---

## SOURCE_MAP reclassifications (R2-delta finding)

The current `SOURCE_MAP.md` separates "User-provided source
material" (lines 1-13) from "Inferences to verify in TC01"
(lines 19-25).

R2-delta proposes the following reclassifications to be applied
in TC01 by the implementing agent (R2-delta does not edit
SOURCE_MAP directly, per scope):

1. **Two-process architecture (MCP server + daemon)**: move from
   inference to USER-PROVIDED. Evidence: README.md:213-239
   architecture diagram and README.md:284-297 safety model
   privilege separation. The architecture diagram literally
   labels both processes (`terminal-commander-mcp` and
   `terminal-commanderd`). R1-alpha had flagged this as an open
   choice; it is not - the user drew it.

2. **MCP tool surface (20 tools by name)**: move from inference
   to USER-PROVIDED. Evidence: README.md:243-266.

3. **Event/bucket schema field names**: move from inference to
   USER-PROVIDED. Evidence: README.md:173-197 sample event.
   Specifically `event_id`, `bucket_id`, `seq`, `timestamp`,
   `severity`, `kind`, `summary`, `captures`, `source.probe_id`,
   `source.source_type`, `source.stream`, `source.job_id`,
   `pointer.frame_id`, `pointer.line`, `pointer.context_available`.

4. **Probe taxonomy (6 named probe types)**: move from inference
   to USER-PROVIDED. Evidence: README.md:124-130. Names are
   fixed.

5. **Sifter taxonomy (11 named sifter types)**: move from
   inference to USER-PROVIDED. Evidence: README.md:136-148.

6. **`bucket_wait` keystone status and request shape**: move from
   inference to USER-PROVIDED. Evidence: README.md:268-281.
   Quote `The most important tool is bucket_wait` and the
   sample request payload. Heartbeat-over-raw-output semantics
   stated explicitly.

7. **Context-by-pointer primitive shape**: move from inference
   to USER-PROVIDED. Evidence: README.md:204-209,
   `event_context(event_id, before=3, after=5)`.

8. **Safety/security default-deny list**: move from inference to
   USER-PROVIDED. Evidence: README.md:294-297 lists private
   keys, password files, credential stores, token caches.

9. **Rule pack file names**: move from inference to
   USER-PROVIDED. Evidence: README.md:367-372 lists
   `generic.terminal.json`, `apt.json`, `cargo.json`,
   `npm.json`, `pytest.json`, `gcc.json`.

10. **Rust stack itself**: REMAINS inference / evidence-backed.
    The user did NOT literally write "Rust" in the README.
    `_R1-alpha-summary.md` established the evidence via rmcp
    Cargo metadata, crate name conventions, and edition-2024
    expectations. This is architect-confirmed, not user-stated.

---

## Contradictions found

### CONTRADICTION-01. Crate count: README says 6, TC04 says 7

- README.md:354-360 lists six crates: terminal-commander-core,
  terminal-commanderd, terminal-commander-mcp,
  terminal-commander-probes, terminal-commander-sifters,
  terminal-commander-cli.
- TC04 (`.agent/goals/terminal-commander-mvp/TC04-rust-workspace-and-toolchain-scaffold.md`:87-93)
  lists seven crates - it adds `terminal-commander-store`.
- Probable resolution: TC04 is more recent and reflects an
  architect-level decision to separate persistent storage
  concerns (event store, registry store) into a dedicated
  crate. This aligns with subsequent goals TC12 (persistent event
  store) and TC13 (registry store), which assume a store layer
  with its own crate boundary.
- Recommendation: do NOT touch README in this research pass
  (R2-delta scope forbids it). Flag for TC01 implementer to
  reconcile in `SPEC.md` / `ARCHITECTURE.md` and to update README
  in a later goal that explicitly allows README edits.

### CONTRADICTION-02. License: README says "not selected," parent task says Apache-2.0

- README.md:384-386: "License is not selected yet."
- Parent task pre-confirmed context: "License: Apache-2.0."
- Probable resolution: orchestration-level decision made after
  README was authored. README has not been updated.
- Recommendation: a future goal should add a `LICENSE` file
  carrying Apache-2.0 text and update README's License section.
  Not in TC01 scope per allowed-files lists.

### CONTRADICTION-03. rmcp version pin

- Vault note `RMCP rust-sdk.md`:9 claims `rmcp v0.16.0`.
- `_R1-alpha-summary.md`:40-46 finds rmcp 1.7.0 is current
  crates.io latest as of 2026-05-13.
- This is documented as an open architect decision in
  `_R1-alpha-summary.md` "Items REQUIRES USER DECISION" #1.
- Recommendation: escalate to user. R2-delta does not resolve.

### Non-contradiction: planned chain length (17 in README, 32 in goals)

- README.md:312-331 lists 17 ordered chain items.
- `.agent/goals/terminal-commander-mvp/` contains 32 TCxx goal
  files (TC01-TC32).
- Inspecting the chain: the 32-file expansion subdivides the 17
  user-named items into smaller goal-file-sized units (e.g.,
  README item 11 "MCP server" becomes TC23 + TC24; README item 15
  "policy engine" becomes TC22; etc.). The expansion is
  consistent with the user's instruction in README.md:301-311
  to keep goals "small enough for one autonomous agent run."
- This is NOT a contradiction; it is a faithful expansion.

---

## Summary table - what is user-provided vs. inference

| Item | Authority | Citation |
|---|---|---|
| Product name "Terminal Commander" | User | README.md:1-3 |
| "Local, MCP-operated" identity | User | README.md:3 |
| Continuous streaming inversion | User | README.md:5-11 |
| MVP behavior list (8 items) | User | README.md:17-26 |
| 6 probe types by name | User | README.md:124-130 |
| 11 sifter types by name | User | README.md:136-148 |
| Bucket schema + sample event shape | User | README.md:152-197 |
| `event_context` primitive | User | README.md:204-209 |
| Two-process architecture (MCP server + daemon) | User | README.md:213-239 |
| 20 MCP tools by name | User | README.md:243-266 |
| `bucket_wait` keystone status | User | README.md:268-281 |
| Safety doctrine + default-deny list | User | README.md:284-297 |
| Goal-file discipline rules | User | README.md:301-311 |
| Numbered chain ordering (17 items) | User | README.md:312-331 |
| Six crate names | User | README.md:354-360 |
| Seventh `store` crate | Architect/TC04 | TC04:87-93 |
| Rule pack file names | User | README.md:367-372 |
| Tests directory layout | User | README.md:374-378 |
| Repository URL | User | SOURCE_MAP.md:5 |
| Branch policy | User | ASSUMPTIONS.md:7-10 |
| Rust toolchain | Architect (evidence-backed by R1-alpha) | _R1-alpha-summary.md |
| tokio runtime | Architect (evidence-backed by R1-alpha) | _R1-alpha-summary.md |
| SQLite via rusqlite | Architect (evidence-backed by R1-beta) | _R1-beta-summary.md |
| notify file watcher | Architect (evidence-backed by R1-beta) | _R1-beta-summary.md |
| WSL 9P polling requirement | Architect (evidence-backed by R1-beta) | _R1-beta-summary.md |
| Apache-2.0 license | Orchestration (parent task brief) | task pre-confirmed |
| rmcp 0.16.0 vs 1.7.0 pin | Open architect decision | _R1-alpha-summary.md |
