# Feature Specification: Dogfood Remediation Batch

**Feature Branch**: `002-dogfood-remediation`

**Created**: 2026-07-02

**Status**: Draft

**Input**: User description: "TC dogfood remediation batch: fix all remaining
defects and ergonomics gaps found in the 2026-07-02 agent dogfood rounds
(docs/dogfood/2026-07-02-tc-0.1.70-dogfood-findings.md, BACKLOG.md P1.0f
resolution + P1.0g). Eleven live-reproduced items spanning registry lifecycle,
facade parameter strictness, token-lean responses, round-trip reduction, file
operations, suggestion heuristics, the WSL shell boundary, and optional pipe
server hardening. Every item must carry acceptance criteria an LLM coding
agent can verify mechanically, and preserve TC's honesty contracts (bounded,
truthful, never-silent)."

Every item below was reproduced live by an agent operating TC exclusively on
Windows 11 + WSL against daemon v0.1.70-v0.1.72. The findings document
(`docs/dogfood/2026-07-02-tc-0.1.70-dogfood-findings.md`) is the evidence
record; this spec is the remediation contract. The primary user of every
surface here is an LLM coding agent driving TC through its MCP tools; "user"
below means that agent (and transitively the human whose tokens and time it
spends).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Learn any call shape in one failed attempt (Priority: P1)

An agent calling a TC tool with wrong or incomplete parameters learns
EVERYTHING wrong with its call from a single error: all missing required
fields for the chosen action are named in one response, and a field that the
chosen action does not consume is rejected by name instead of being silently
ignored.

**Why this priority**: This was the single largest observed waste. Live
repro: `registry deactivate` revealed missing fields one per call (three
round-trips to learn one call shape), and `sub_pull` silently accepted
`wait_ms` while honoring only `timeout_ms` — the agent believed it had set a
30 s wait when it had set nothing, and misdiagnosed the resulting behavior as
a daemon bug. Silent parameter acceptance corrupts agent reasoning; every
other item in this batch is cheaper once errors teach completely.

**Independent Test**: Call `registry` action=`deactivate` with only
`rule_id`; the single error names every other required field. Call `command`
action=`sub_pull` with `wait_ms`; the error names `wait_ms` as unknown
for the action and names `timeout_ms` as the field that action consumes.

**Acceptance Scenarios**:

1. **Given** a facade call whose action requires fields A and B, **When** the
   call omits both, **Then** one single error names both A and B (not just
   the first) and names the action it validated against.
2. **Given** a facade call carrying a field that exists in the facade schema
   but is not consumed by the chosen action, **When** the call is made,
   **Then** it is rejected with an error naming the unknown-for-action field
   and, when an obvious counterpart exists (e.g. `timeout_ms` vs `wait_ms`),
   naming that counterpart as the remedy.
3. **Given** a well-formed call, **When** it is made, **Then** behavior is
   byte-identical to today (strictness adds no cost to correct calls).

---

### User Story 2 - Registry lifecycle without waste or ceremony (Priority: P1)

An agent importing a rule pack twice gets the same registry state twice —
identical definitions are reported as `skipped`, not re-minted as new
versions. An agent finished with a pack (or any set of rules) deactivates it
in ONE call.

**Why this priority**: Live repro: re-importing the `cargo` pack minted six
identical v2 rows (registry pollution that compounds forever, since rows are
immutable), and deactivating the pack afterwards took six separate calls.
Pack lifecycle is the recommended workflow — it must not punish its users.

**Independent Test**: Import the `cargo` pack twice; the second response
lists all six rules under `skipped` and the store holds one version per
rule. Then deactivate the whole pack with a single call and verify
`registry_list_active` returns empty.

**Acceptance Scenarios**:

1. **Given** a pack already imported, **When** the identical pack is imported
   again, **Then** every unchanged rule appears in `skipped`, no new
   versions exist, and `imported` is empty.
2. **Given** a pack whose definition changed for two of six rules, **When**
   it is re-imported, **Then** exactly those two appear in `imported` (new
   versions) and four appear in `skipped`.
3. **Given** six active rules from one pack, **When** a single pack-level
   deactivate call is made with an explicit scope, **Then** all six are
   deactivated and the response lists each (rule_id, version) acted on.
4. **Given** a bulk deactivate naming three rule_ids of which one is
   unknown, **Then** the response deactivates the two known rules and names
   the unknown one explicitly — partial success is reported per-rule, never
   silently.

---

### User Story 3 - Discover files without a shell (Priority: P1)

An agent on a default-deny profile (no shell) lists the contents of one
directory through the files facade: names, kind (file/dir/symlink), size,
and modification time, bounded and policy-gated exactly like file reads.

**Why this priority**: Live repro: with `allow_shell=false` and `cmd` denied
on the argv path (both correct defaults), there is NO way to discover what
exists on disk through TC on Windows — the agent had to guess paths and got
`FileNotFound`. This is the single largest capability gap for TC-exclusive
operation.

**Independent Test**: With a default-deny profile, list a project directory
and receive its entries; attempt to list a policy-denied path and receive
the same policy denial a `file_read` of that path produces.

**Acceptance Scenarios**:

1. **Given** a readable directory, **When** the agent lists it, **Then**
   entries carry name, kind, size (files), and mtime, sorted
   deterministically (dirs first, then files, each lexicographic).
2. **Given** a directory with more entries than the response cap, **When**
   listed, **Then** the response is truncation-flagged with the total count
   — bounded and truthful, never silently partial.
3. **Given** a path under the policy deny list, **When** listed, **Then**
   the call is denied with the same policy error shape as `file_read`.
4. **Given** a path that is a file, not a directory, **Then** the error says
   so and names the `read` action as the remedy.

---

### User Story 4 - Token-lean streaming reads (Priority: P2)

An agent following a long-running command through `wait`/`events` (and
subscriptions through `sub_pull`) receives the same load-bearing facts at a
fraction of today's token cost: an opt-in compact projection on bucket
reads, and per-source liveness that is sent only when it changes.

**Why this priority**: Live repro: one PTY line of interest arrived wrapped
in ~15 lines of id plumbing via `wait` (run_and_watch already solved this
with `compact`), and every `sub_pull` re-sent the full liveness array for
all nine sources even when nothing changed. TC's product IS token economy;
its own streaming surfaces should not be the leak.

**Independent Test**: Issue `wait` with `compact: true` and verify each
signal carries only summary/stream/seq/severity; issue two consecutive
`sub_pull`s with no source state change and verify the second carries no
(or empty) liveness payload while a state change reappears in the next pull.

**Acceptance Scenarios**:

1. **Given** `compact: true` on `wait` or `events`, **When** signals return,
   **Then** each carries exactly the compact field set already established
   by `run_and_watch` (summary, stream, seq, severity), and the full records
   remain re-fetchable by cursor with `compact` omitted.
2. **Given** two consecutive pulls on one subscription with no liveness
   transitions between them, **Then** the second pull's liveness section is
   empty/omitted; **Given** a transition (running -> exited), **Then** the
   next pull carries that source's new liveness exactly once.
3. **Given** a fresh subscription's first pull, **Then** liveness for all
   in-scope sources is sent in full (the baseline snapshot).

---

### User Story 5 - Fewer round-trips on interactive surfaces (Priority: P2)

An agent inspecting an event's context addresses it by `event_id` alone;
an agent driving a REPL writes stdin and receives the combed signals that
write provoked in the same call.

**Why this priority**: Live repro: `event_context` rejected a call carrying
a globally-unique `evt_` id because `bucket_id` was missing (pure ceremony),
and every REPL interaction cost two calls (`pty_stdin`, then `wait`).
Round-trips are the second currency after tokens.

**Independent Test**: Call `event_context` with only `event_id` and receive
the context window. Call `pty_stdin` with `wait_ms: 3000` against a node
REPL and receive the echo + result signals in the response.

**Acceptance Scenarios**:

1. **Given** a valid `event_id` and no `bucket_id`, **When** `event_context`
   is called, **Then** the context window returns exactly as if the correct
   `bucket_id` had been supplied; an unknown `event_id` errors identically
   in both addressing modes. A supplied `bucket_id` that contradicts the
   event's actual bucket is an error, not silently ignored.
2. **Given** a running PTY job, **When** `pty_stdin` is called with
   `wait_ms`, **Then** the response contains the signals emitted after the
   write (same bounded-wait semantics as `sh_exec`: cursor in, signals +
   next cursor out); with `wait_ms` omitted, behavior is byte-identical to
   today (immediate return).
3. **Given** a PTY holding an active secret prompt, **Then** the existing
   denial behavior is unchanged by the new parameter.

---

### User Story 6 - Append without rewriting (Priority: P2)

An agent appends bounded content to an existing file (logs, checklists,
running notes) without resending the whole file, gated by the exact same
write policy as `file_write`.

**Why this priority**: Live repro: triggering a file-watch test required
rewriting the entire file just to add one line. Append is the natural shape
for every log-like workflow; whole-file rewrite also races concurrent
writers.

**Independent Test**: Append a line to an existing allowed file and verify
the prior content is intact with the new line at the end; append to a
denied path and receive the write-policy denial.

**Acceptance Scenarios**:

1. **Given** an existing file of N bytes, **When** append is called with M
   bytes, **Then** the file is N+M bytes with the original prefix untouched,
   and the response reports bytes appended.
2. **Given** a missing file and append mode, **Then** the file is created
   (parity with write + `create_dirs` rules unchanged).
3. **Given** a path outside `paths.write_allow`, **Then** append is denied
   with the same policy error as write; the size bound applies to append
   payloads identically.

---

### User Story 7 - Suggestions that recognize the mainstream stacks (Priority: P3)

An agent feeding raw npm/TypeScript output to `suggest_from_samples`
receives draft rules for the shapes that dominate real JS/TS builds:
`npm ERR!` lines and `error TS<digits>:` diagnostics.

**Why this priority**: Live repro: six realistic npm/tsc lines yielded only
a generic `error-prefix` proposal; the two most common JS-ecosystem shapes
were missed entirely. Lower priority because inline rules and packs cover
the gap manually.

**Independent Test**: Feed the six-line sample from the findings doc;
receive draft proposals matching `npm ERR!` and `error TS\d+` lines, each
still marked draft/never-activated.

**Acceptance Scenarios**:

1. **Given** samples containing `npm ERR! code ERESOLVE`, **Then** a draft
   proposal matches `npm ERR!`-prefixed lines and survives `registry_test`
   against those samples.
2. **Given** samples containing `error TS2345: ...`, **Then** a draft
   proposal captures the TS error code and message and survives
   `registry_test`.
3. **Given** any input, **Then** the tool still NEVER activates or persists
   a rule (existing contract intact), and a proposal's stream filter is only
   set when the samples carried stream evidence.

---

### User Story 8 - The WSL boundary is a policy decision, not an accident (Priority: P2)

An agent (or operator) can state exactly what `wsl.exe` may carry through
the argv lane. Nested shell payloads (`wsl.exe -e bash -lc "..."`,
`wsl.exe bash`, `wsl.exe -- sh -c ...`) are governed by the same
`allow_shell` capability that governs `shell_exec` — not smuggled past the
interpreter denylist because the denylist only inspected `argv[0]`.

**Why this priority**: Live repro: with `allow_shell=false`, the argv lane
correctly denied `cmd` but happily ran `wsl.exe -e bash -lc <arbitrary
shell>`. The constitution (Principle II) is explicit: no argv smuggling,
and the interpreter deny must remain intact — today it is intact in letter
and bypassed in spirit. This item implements the constitutional default:
the shell capability follows the shell, whichever side of the WSL boundary
it runs on.

**Independent Test**: With `allow_shell=false`, `wsl.exe -e bash -lc "..."`
is denied with the same teaching error shape as a bare `bash`; `wsl.exe -e
cargo build` and `wsl.exe --list --verbose` still run. With
`allow_shell=true`, the nested-shell form runs.

**Acceptance Scenarios**:

1. **Given** `allow_shell=false`, **When** argv is `wsl.exe` (or `wsl`)
   carrying a recognized shell interpreter as its command payload (with or
   without `-e`/`--`, with or without distro selectors like `-d <distro>`),
   **Then** the call is denied with a teaching error naming the nested
   interpreter and the `allow_shell` gate.
2. **Given** `allow_shell=false`, **When** argv is `wsl.exe` carrying a
   non-shell program via the direct-exec introducer (`-e`/`--exec`, e.g.
   `wsl.exe -e cargo build`) or a WSL management flag (`--list`,
   `--status`), **Then** it runs exactly as today. A payload NOT
   introduced by `-e`/`--exec` is handed to the distro's default shell by
   WSL itself (shell-interpreted) and is therefore governed by
   `allow_shell` like any other shell line.
3. **Given** `allow_shell=true`, **Then** nested shell payloads run, and the
   audit record notes the nested-shell classification.
4. The stance (what is inspected, what is not, and why WSL is treated as
   this host's boundary rather than a remote machine) is documented in the
   policy documentation alongside `SHELL_INTERPRETERS_DENY`.

---

### User Story 9 - Shrink the connect gap at the source (Priority: P3, optional)

The daemon's local endpoint keeps more than one pending pipe instance on
Windows so that concurrent or starvation-delayed connects land on a waiting
instance instead of a gap. Client-side retry (shipped in 0.1.72) already
masks the symptom; this removes most occurrences of the gap itself.

**Why this priority**: Defense in depth on an already-mitigated failure
mode; strictly optional, ships last, and must not destabilize the accept
path that everything else depends on.

**Independent Test**: Under a synthetic connect storm (many concurrent
clients in a loop), the rate of connect attempts that need any retry drops
to near zero compared to the single-instance baseline.

**Acceptance Scenarios**:

1. **Given** N concurrent first-connect attempts (N <= pending pool size),
   **Then** all succeed without entering the client retry loop.
2. **Given** the new pool, **Then** peer-identity recording, policy
   evaluation, and per-connection behavior are byte-identical per
   connection (the pool changes only WHEN a connect succeeds, nothing about
   what happens after).
3. **Given** daemon shutdown, **Then** all pending instances close cleanly
   (no orphaned pipe instances after exit).

---

### Edge Cases

- Strictness vs. compatibility: a client sending yesterday's exact calls
  (all documented fields, correct actions) must see zero behavior change;
  strictness applies only to calls that were already broken or silently
  misinterpreted. `rules_json` and other documented-deprecated fields stay
  accepted where currently accepted.
- Pack idempotency vs. drafts: a stored rule locally edited (new version
  upserted by the operator) must NOT be clobbered or "skipped" into
  ambiguity by a pack re-import — identity comparison is against the
  latest stored version of the same rule id (an operator edit differs in
  content, so re-import imports a new version and is never falsely
  reported as skipped).
- Bulk deactivate with mixed scopes: one call, one scope; mixing scopes in
  one bulk call is rejected with a teaching error.
- Directory listing of a symlink/junction cycle or reparse point: entry
  reported with kind, never followed recursively; listing is single-level
  only.
- Directory listing races: entries deleted between enumeration and stat are
  omitted or reported with partial metadata, never an error for the whole
  listing.
- Append concurrency: two appends racing on one file must serialize (both
  land, order unspecified) — no interleaved partial writes.
- Liveness delta across subscription seek/reopen: an explicit `sub_seek`
  or a re-opened subscription resets the delta baseline (full snapshot
  again) so no transition is unobservable.
- `event_context` by id alone when the owning bucket was evicted: the same
  honest not-found/evicted answer the two-field form gives today.
- WSL gating with exotic spellings: `wsl` vs `wsl.exe` vs absolute path to
  wsl.exe, distro flags before the payload, the `~` start-in-home
  shorthand, `--exec` long form — all classified identically; payloads not
  introduced by `-e`/`--exec` (bare, or after `--`) are shell-interpreted
  by WSL itself and classify as nested shell; unknown/novel WSL flags fail
  CLOSED (treated as potentially carrying a payload -> denied under
  `allow_shell=false`) with a teaching error, never open.
- Compact + severity filters compose: `compact` changes projection only,
  never which events match.

## Requirements *(mandatory)*

### Functional Requirements

Facade strictness (US1):

- **FR-001**: Every facade action MUST report ALL of its missing required
  fields in a single error response, naming the action validated against.
- **FR-002**: Every facade action MUST reject fields present in the call
  that the chosen action does not consume, naming the field; when a
  same-purpose counterpart exists the error MUST name it as the remedy.
- **FR-003**: Documented deprecated aliases (e.g. `rules_json`, `samples`
  alias) MUST remain accepted exactly where they are accepted today.

Registry lifecycle (US2):

- **FR-010**: `registry_import_pack` MUST compare each pack rule against
  the latest stored version of the same rule id and report content-identical
  rules in `skipped` without creating versions; changed or absent rules
  import as today.
- **FR-011**: The registry MUST support deactivating an entire pack by name
  and a list of rule ids in one call, with one explicit scope per call,
  reporting per-rule outcomes (deactivated / not-active / unknown) — partial
  success is explicit, never silent.

Files facade (US3, US6):

- **FR-020**: The files facade MUST offer a single-level directory listing
  returning name, kind (file/dir/symlink), size for files, and mtime,
  deterministically ordered, bounded by a server-side entry cap with a
  truthful truncation flag and total count.
- **FR-021**: Directory listing MUST be gated by the same read-path policy
  as `file_read` (deny paths, allow lists) and MUST be audited like other
  file operations.
- **FR-022**: `file_write` MUST support an append mode: same policy gate,
  same size bound per call, original content never modified, and racing
  appends never interleaved. All-or-nothing is not promised: on an I/O
  failure a partial append is possible and MUST surface as an error
  reporting the bytes actually written — honesty over an unkeepable
  guarantee.

Token economy (US4):

- **FR-030**: `wait` and `events` bucket reads MUST accept the established
  `compact` projection flag with the same field set `run_and_watch` emits;
  full records stay re-fetchable by cursor.
- **FR-031**: `sub_pull` MUST send a full liveness snapshot on a
  subscription's first pull and after any seek/reopen, and thereafter only
  entries whose liveness changed since the last pull; no transition may be
  skippable (change -> guaranteed present in exactly the next pull). This
  contract binds the MCP `sub_pull` action; the daemon wire flag is
  opt-in for compatibility with existing wire clients.

Round-trips (US5):

- **FR-040**: `event_context` MUST resolve a context window from
  `event_id` alone; a supplied `bucket_id` is validated against the event's
  actual bucket and a mismatch is an error.
- **FR-041**: `pty_stdin` MUST accept an optional bounded `wait_ms` that
  returns the combed signals produced after the write (cursor in, signals +
  next_cursor out, daemon-clamped like existing settle windows); omitted =
  today's immediate return, byte-identical.

Suggestions (US7):

- **FR-050**: `suggest_from_samples` MUST propose draft rules for
  `npm ERR!`-prefixed lines and `error TS\d+:` diagnostics when such lines
  appear in samples; proposals MUST validate against the samples via the
  dry-run test path and MUST NOT set a stream filter without stream
  evidence. The never-activate/never-persist contract is unchanged.

WSL boundary (US8):

- **FR-060**: The argv-lane interpreter gate MUST classify a `wsl`/`wsl.exe`
  invocation whose command payload is a recognized shell interpreter as a
  shell request, denied under `allow_shell=false` with a teaching error and
  permitted (and audit-tagged) under `allow_shell=true`. Non-shell payloads
  under `-e`/`--exec` and WSL management flags are unaffected; a payload
  not introduced by `-e`/`--exec` is shell-interpreted by WSL itself and
  MUST gate identically to a shell request. Unknown payload-position
  constructions fail closed under `allow_shell=false`.
- **FR-061**: The WSL boundary stance MUST be documented with the policy
  documentation (what is inspected, the fail-closed rule, and the rationale
  that the shell capability follows the shell across the WSL boundary).

Optional hardening (US9):

- **FR-070** *(optional, last)*: The Windows local endpoint MAY maintain a
  small fixed pool (>1) of pending pipe instances; if implemented, per-
  connection semantics (peer identity, policy, audit) are unchanged and
  clean shutdown closes every instance.

### Key Entities

- **Rule pack**: named, versioned set of rule definitions; identity of a
  member = (rule id, definition content) against latest stored version.
- **Activation scope**: global / bucket / job / probe binding of an active
  rule; bulk operations carry exactly one scope.
- **Directory entry**: name, kind, size (files), mtime — the discovery unit
  of the files facade.
- **Liveness delta**: the subset of per-source liveness entries that changed
  between two pulls of one subscription, plus the baseline-snapshot rule.
- **Nested shell classification**: the parsed judgment that an argv-lane
  WSL invocation carries a shell interpreter as its payload.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An agent discovering any facade call shape by trial needs at
  most ONE failed call before a correct call (was: three for deactivate).
- **SC-002**: Re-importing an already-imported pack creates zero new
  registry rows (was: one per rule) and pack deactivation is one call
  (was: one per rule).
- **SC-003**: An agent on a default-deny profile can enumerate a project
  directory through TC alone in one call (was: impossible on Windows).
- **SC-004**: Following a quiet long build via compact `wait` plus delta
  `sub_pull` costs at least 60% fewer response tokens than today's full
  records + full liveness for the same event stream (measured on the
  findings-doc repro transcript shapes).
- **SC-005**: One REPL interaction (send line, read result) is ONE tool
  call (was: two).
- **SC-006**: With `allow_shell=false`, zero shell interpreters are
  reachable through the argv lane — including via WSL carriers — while all
  non-shell WSL usage from the dogfood transcript still works unchanged.
- **SC-007**: The six-line JS/TS sample from the findings doc yields
  proposals covering `npm ERR!` and `error TS\d+` shapes (was: zero of
  two).
- **SC-008**: All existing tests remain green on Windows and Linux gates;
  every FR lands with at least one test that fails on the pre-change
  behavior (proven by the implementing agent, red -> green).

## Assumptions

- Additive evolution only: every new field/action is optional or new;
  existing well-formed calls and wire shapes keep byte-identical behavior
  (the strictness FRs affect only calls that were already erroneous or
  silently misread).
- The strictness contract (FR-001/002) applies at the MCP facade layer where
  flat action schemas live; the daemon wire protocol keeps its current
  serde behavior.
- The compact projection field set is exactly the one `run_and_watch`
  already established; no new projection design.
- Directory listing is read-only discovery: single level, no recursion, no
  globbing — deeper discovery composes by repeated calls.
- The WSL nested-shell gate reuses the existing `SHELL_INTERPRETERS_DENY`
  interpreter list for payload classification; it inspects argv only (never
  file contents), and fails closed on ambiguity under `allow_shell=false`.
- US9 (pipe instance pool) is explicitly optional: it ships only if the
  implementing agent can demonstrate the acceptance scenarios without
  destabilizing the accept path; skipping it with a written rationale is a
  compliant outcome.
- Out of scope, explicitly: the P1.0a-P1.0e trust-defect campaign items
  (tracked in `.planning/tc-bugfix-campaign/`), the unmerged omni review
  branches, and any wire-protocol breaking change. An implementing agent
  that finds itself editing those areas has left this feature's scope.
- Constitution v1.0.0 principles (two-process boundary, policy-before-
  spawn, bounded output, local-only endpoint) bind every item; no FR here
  may weaken them.
