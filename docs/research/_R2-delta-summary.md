# R2-delta Research Summary

Author: research agent R2-delta
Date: 2026-05-21
Goal: TC01 (research product baseline)
Scope: H1 (prior art landscape survey) and H2 (user-provided
evidence sweep with SOURCE_MAP reclassification)

Files written (absolute paths):

- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\prior-art.md`
- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\user-provided-evidence.md`
- `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\_R2-delta-summary.md`

No files touched outside `docs/research/`. No git operations. No
source-code or .agent/ edits.

---

## Top findings

### F1. Terminal Commander's closest direct analog is Honeycomb's honeytail, not any agent/terminal product

Surprising result. Honeycomb's `honeytail` agent
(https://github.com/honeycombio/honeytail) shares more architecture
with TC than Warp, Cursor, container-use, or Claude Code's Bash tool
do. honeytail is a daemon that tails files, applies a pluggable
parser stack (JSON, regex with named groups, logfmt, nginx, MySQL,
syslog, MongoDB, ArangoDB, CSV), handles rotation, resumes from
saved progress after interrupt, and emits structured events.

The DELTAS that make TC distinct:

1. Local consumer (LLM via MCP) instead of remote SaaS.
2. Wider probe surface (also processes, PTY, directories, future
   journal and artifact probes; honeytail is file-only).
3. Bounded-context-by-pointer primitive
   (`event_context(event_id, before, after)`) - honeytail has no
   analog.
4. LLM-runtime-mutable rule registry (search/test/activate via MCP).
   honeytail rules are static config.
5. Typed event schema with severity, kind enum, source pointer,
   captures, dedupe metadata - honeytail produces flat records.

Implication: TC's pitch is "Datadog/Honeycomb agent shape, but for
coding-agent loops, exposed via MCP." This positions TC clearly
and honestly. Confidence: high.

### F2. The two-process architecture is user-provided, not an open architect decision

R1-alpha's `_R1-alpha-summary.md` listed "Two-process vs
single-process architecture" as an open user decision (#2 in
"Items REQUIRES USER DECISION"). On re-reading README.md, this is
incorrect. The user already specified two processes:

- README.md:213-239 (architecture diagram) draws
  `terminal-commander-mcp` and `terminal-commanderd` as separate
  boxes.
- README.md:354-360 lists them as two separate crates.
- README.md:284-297 (safety model) makes privilege separation
  between MCP server and daemon a constraint: "MCP server should
  be unprivileged where possible. Daemon/helper may be privileged
  only when configured."

R2-delta closes that R1-alpha open question: two-process is
USER-PROVIDED, not inferred. Confidence: high.

### F3. Crate count discrepancy between README and TC04

README.md:354-360 names SIX crates. TC04 goal file at
`.agent/goals/terminal-commander-mvp/TC04-rust-workspace-and-toolchain-scaffold.md`:87-93
names SEVEN, adding `terminal-commander-store`.

The store crate is consistent with TC12 (persistent event store)
and TC13 (registry store), so the seventh crate is an architect
extension of the user-provided six. R2-delta does not touch either
file. Resolution path: TC01 implementer should document the
seven-crate canonical list in `SPEC.md` / `ARCHITECTURE.md` and
let a later goal that allows README edits update the README. Flag
also in `SOURCE_MAP.md` under contradictions. Confidence: high.

### F4. No surveyed product combines all three of {MCP, streaming daemon, runtime-mutable rule registry}

After surveying Warp, Cursor, Fig/Amazon Q, Claude Code Bash tool,
container-use, Dagger, just, cargo-watch, direnv, asciinema,
filesystem MCP server, github-mcp-server, fetch MCP server,
rust-mcp-stack, honeytail, and Datadog agent: NONE combines all
three TC differentiators. The closest combinations are:

- honeytail: streaming daemon + parser stack, but no MCP, no
  runtime mutability.
- Claude Code background monitor: streaming + agent-facing, but no
  daemon, no rule registry, harness-specific.
- container-use: MCP + isolation, but no streaming signal
  extraction, no rule registry.
- Cursor terminal tool: MCP-adjacent + agent-facing, but no
  streaming sifter and not provider-neutral.

This is the novelty argument. TC is not a clone of an existing
product; it is a new combination. Confidence: high.

### F5. Claude Code's existing "wait for stdout signal" pattern validates the `bucket_wait` design

Documented Claude Code patterns (cited by community sources) state:
"Instead of running status checks on a timer, Claude waits for a
process to emit a signal (a file write, a log entry, or a stdout
stream match) and acts only when that signal arrives." The
background monitor pattern uses 200ms-batched stdout lines as chat
notifications.

This is independent confirmation that the
"wait-for-signal-not-poll" loop is the right primitive. TC's
`bucket_wait` formalizes this pattern: an MCP-callable, cursor-
addressable, severity-filterable, heartbeat-emitting wait, instead
of a per-script grep/awk wrapper. Confidence: high.

---

## SOURCE_MAP reclassifications (R2-delta finding, for TC01 implementer)

Move these items from "Inferences to verify in TC01" to "User-
provided source material" in
`C:\AI_STUFF\PROGRAMMING\terminal-commander\.agent\goals\terminal-commander-mvp\SOURCE_MAP.md`:

1. Two-process architecture (MCP server + daemon) - cite
   README.md:213-239 + 284-297.
2. The 20 MCP tool names - cite README.md:243-266.
3. The event/bucket schema field names and sample event shape -
   cite README.md:152-197.
4. The 6 probe type names - cite README.md:124-130.
5. The 11 sifter type names - cite README.md:136-148.
6. The `bucket_wait` keystone status, sample request payload, and
   heartbeat-over-raw-output semantics - cite README.md:268-281.
7. The `event_context(event_id, before, after)` primitive shape -
   cite README.md:204-209.
8. The safety default-deny list (private keys, password files,
   credential stores, token caches) - cite README.md:294-297.
9. The rule pack file names (generic.terminal.json, apt.json,
   cargo.json, npm.json, pytest.json, gcc.json) - cite
   README.md:367-372.
10. The expected ordered chain (17 items) - cite README.md:312-331.

Items that STAY in inference/evidence-backed (not user-provided):

- Rust as the language. Architect-decided, evidence-backed by
  `_R1-alpha-summary.md`.
- tokio as the async runtime. Architect-decided, evidence-backed
  by rmcp's Cargo.toml.
- SQLite via rusqlite. Architect-decided, evidence-backed by
  `_R1-beta-summary.md`.
- notify file watcher. Architect-decided, evidence-backed by
  `_R1-beta-summary.md`.
- WSL 9P polling requirement. Architect-decided, evidence-backed
  by `_R1-beta-summary.md`.

Items that REMAIN open architect/user decisions:

- rmcp 0.16.0 vs 1.7.0 pin (per `_R1-alpha-summary.md` open
  decisions #1). Vault note `RMCP rust-sdk.md` cites 0.16.0.
- License file content: parent task says Apache-2.0 but README
  still says "not selected." Reconcile in a license goal.

New user-provided entry that is currently missing from SOURCE_MAP:

- The seven-crate canonical list (TC04). Even though only six
  appear in README, the user authorized TC04 and the seven-crate
  list is the active architect intent. Document the contradiction
  explicitly rather than papering over it.

---

## Contradictions found between README and goal files

1. **Crate count: README says 6, TC04 says 7.** README.md:354-360
   vs TC04:87-93. TC04 adds `terminal-commander-store`. This is
   the most concrete contradiction R2-delta found. See F3.

2. **License: README says "not selected," parent task pre-confirms
   Apache-2.0.** README.md:384-386 vs task brief. Resolution path:
   add a `LICENSE` file and update README in a license goal.

3. **Chain length: README says 17 ordered items, goals folder has
   32 TCxx files.** Inspection confirms the 32 is a faithful
   expansion of the 17 (e.g., README item 11 "MCP server" becomes
   TC23 + TC24; item 16 "installer and WSL support" becomes TC26;
   etc.). NOT a contradiction - a permitted expansion per
   README.md:308 "small enough for one autonomous agent run."

4. **rmcp version pin: vault says 0.16.0, current crates.io is
   1.7.0.** Already flagged by R1-alpha as an open user decision.

---

## Confidence summary

| Finding | Confidence | Notes |
|---|---|---|
| Prior-art landscape inventory (Warp, Cursor, etc.) | High | All URLs fetched; all attributions live. |
| honeytail is closest direct analog | High | README + parser list match. |
| Two-process is user-provided | High | README diagram and crate list are explicit. |
| Crate count contradiction (6 vs 7) | High | Direct comparison of two repo files. |
| 20 MCP tool names are user-provided | High | README.md:243-266 lists them verbatim. |
| Event schema is user-provided | High | README.md:173-197 sample. |
| TC novelty: MCP + streaming + runtime-mutable rules | High | Confirmed by exhaustive prior-art sweep. |
| License posture | Medium | Two sources disagree; await reconcile. |
| rmcp version pin | Medium | Open decision per R1-alpha. |

---

## Blockers / open questions for the architect

R2-delta reached no HALT-worthy blocker. Three items require
follow-up by the TC01 implementer (or by a later goal):

1. **Reconcile crate count.** README should be updated to seven
   crates, or TC04 should drop the store crate. Strong recommend
   the README update because TC12/TC13 already reference
   `terminal-commander-store` semantics.
2. **Add a `LICENSE` file** carrying Apache-2.0 and update the
   README license section. Not in any current TCxx allowed-files
   list - needs a small dedicated goal.
3. **rmcp version pin** (carried forward from R1-alpha) - the
   architect should choose 0.16.0 or 1.7.0 before TC04 starts.

None of the three blocks TC01's mini-spec (which only requires
producing SPEC.md, ARCHITECTURE.md, SOURCE_MAP.md, ASSUMPTIONS.md,
and docs/research/). All three should be surfaced in the TC01
final report's "Known gaps / blockers" section.

---

## Branch / repo hygiene

- No source code or .agent/ files touched.
- All writes confined to
  `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\`.
- No git operations performed.
- No vault edits performed - the vault was read-only context.

---

## Cross-references to prior research in this folder

- `_R1-alpha-summary.md` (R1-alpha): language, async runtime, MSRV,
  MCP SDK, transport, PTY, daemon lifecycle, process cleanup.
- `_R1-beta-summary.md` (R1-beta): file watcher, WSL boundary,
  SQLite + FTS5, policy prior art.
- R2-delta (this file): prior art landscape + user-provided
  evidence sweep + contradiction surfacing.

R2-delta does not duplicate any topic R1-alpha or R1-beta covered.
The deliverables are additive.
