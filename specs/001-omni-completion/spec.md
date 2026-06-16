# Feature Specification: Omni Completion Program

**Feature Branch**: `001-omni-completion` (spec dir; no git branch created)

**Created**: 2026-06-16

**Status**: Draft

**Input**: User description: take Terminal Commander from v0.1.49 (39 MCP tools;
shell_exec, trust campaign, and supervisor self-heal already landed) to a 100%
self-reliant omni terminal tool for LLM agents, covering ALL remaining items in
`docs/plans/LLM-HANDOFF-tc-omni-program.md` and `BACKLOG.md`, plus the still-open
field-ledger findings in `docs/field-ledger-2026-06-11-aap-campaign.md`.

## Overview

Terminal Commander (TC) lets an LLM agent run commands, drive interactive
programs, watch files, and receive only bounded STRUCTURED SIGNALS instead of raw
terminal output. Today an agent must still fall back to a separate raw shell tool
for: multi-step shell state, unknown-output parsing, non-Linux PTYs, privileged
installs, and remote hosts. This program closes those gaps so an agent can rely
on TC alone, while preserving every safety guarantee in the project constitution.

The program is delivered as six independently-shippable priority slices (P1-P6),
each a viable increment, plus a set of cross-cutting trust/ergonomics fixes drawn
from live field usage. Each slice maps to an omni acceptance gate (O-01..O-14).

## Clarifications

### Session 2026-06-16

- Q: How far should this session drive the implement phase? -> A: Implement as
  many priority slices as possible autonomously, gating each completed slice to a
  per-slice review branch.
- Q: Where should implemented code land? -> A: Each slice commits to its own
  feature/review branch and pauses before any merge or push (no push without
  explicit human approval).
- Q: Is the P4 privileged helper included in this program run? -> A: No -- P4 is
  planned and specified but NO privileged code lands until a separate security
  threat review is completed first.
- Q: How are the open field-ledger fixes scheduled? -> A: Each fix folds into the
  earliest priority slice that touches its code path (most land in P1, which
  touches the command/signal path).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Persistent shell sessions and workspace state (Priority: P1)

An agent needs to run a sequence of shell steps that share working directory and
environment -- e.g. `cd build`, then `cmake ..`, then `make` -- without
re-passing cwd/env on every call, and optionally save and restore that workspace
state.

**Why this priority**: Sticky session state is the single largest human-parity
gap once one-shot shell exists. Without it the agent cannot model the most common
human terminal workflow. It unblocks the most downstream value per unit effort.

**Independent Test**: Start a session; send `cd /tmp`; send `pwd`; confirm the
returned signal reports `/tmp` without the agent re-passing cwd. Save a snapshot,
start a fresh session, apply the snapshot, confirm cwd/env restored.

**Acceptance Scenarios**:

1. **Given** the session capability is enabled by the operator, **When** the
   agent starts a session and sends two lines that depend on shared cwd, **Then**
   the second line executes in the cwd set by the first and returns combed
   signals only.
2. **Given** an active session, **When** the agent requests session status,
   **Then** it receives the current cwd and a bounded env snapshot.
3. **Given** the session capability is disabled (default), **When** the agent
   tries to start a session, **Then** the request is denied by policy and
   audited.
4. **Given** the configured session limit is reached, **When** the agent starts
   another session, **Then** the request is refused with a bounded, explanatory
   result (never a silent hang).
5. **Given** an idle session past its TTL, **When** the TTL elapses, **Then** the
   session is torn down and its resources reclaimed.

---

### User Story 2 - Parse unknown output without hand-writing rules first (Priority: P2)

An agent runs an unfamiliar tool, gets little or no signal, and needs help
turning raw output into reusable structured rules -- and wants common tools to
emit baseline signals out of the box.

**Why this priority**: Parsing self-reliance ("run anything, still get signal")
is the second-biggest gap and is independent of platform work, so it can proceed
in parallel. It directly closes the "parse = only rules you wrote" limitation.

**Independent Test**: Run a tool with no active rule pack; capture a bounded
tail; ask the system to suggest rules from samples; confirm proposals are
returned but NOT auto-activated; test and activate one; re-run and confirm
signals now appear.

**Acceptance Scenarios**:

1. **Given** sample output lines, **When** the agent requests rule suggestions,
   **Then** it receives candidate rules with a confidence label and explicit
   next steps, and NO rule is activated automatically.
2. **Given** universal extractors are enabled, **When** any command emits stderr
   error/warning lines or a non-zero exit, **Then** the agent receives baseline
   low-severity signals even with no tool-specific pack active.
3. **Given** a known tool is started without its pack active, **When** the
   command starts, **Then** the response includes a hint naming the available
   pack and how to import it.
4. **Given** the built-in pack set, **When** packs are listed, **Then** at least
   25 packs are available, including docker, kubectl, and git.

---

### User Story 3 - Same capabilities on every platform (Priority: P3)

An agent (or the operator behind it) works on native Windows or macOS, not just
Linux/WSL, and expects interactive PTY programs, responsive file watching, and
clean process cancellation to behave identically.

**Why this priority**: Platform parity widens reach but depends on the runtime
seams being stable; it is valuable but not a prerequisite for the parsing or
session value, so it sequences after P1 and can overlap P2.

**Independent Test**: On native Windows, drive an interactive REPL through the
PTY tools and confirm prompt detection + bounded output. On macOS, run the
daemon+MCP smoke. On a native filesystem, confirm file-change signals arrive
without the multi-hundred-millisecond polling delay.

**Acceptance Scenarios**:

1. **Given** native Windows, **When** the agent opens an interactive REPL via the
   PTY tools, **Then** discovery reports PTY available and the REPL is driveable
   with bounded, combed output.
2. **Given** macOS, **When** the daemon and MCP adapter run the standard smoke,
   **Then** the full command -> wait -> status flow passes.
3. **Given** a native (non-9P) filesystem, **When** a watched file changes,
   **Then** the change signal arrives promptly via an event-driven backend, with
   the polling backend retained as fallback for WSL `/mnt/c`.
4. **Given** a running command, PTY, or session, **When** the agent stops it,
   **Then** a graceful terminate is attempted before a forced kill, and the
   terminal state is reported.

---

### User Story 4 - Operator-gated privileged operations (Priority: P4)

When the operator opts in, an agent can perform a closed list of privileged
operations (e.g. install a named package, restart a named service) through a
separate, audited helper -- never via generic sudo or a shell line.

**Why this priority**: High value but highest risk; it requires a threat review
and the policy/audit framework from earlier slices to be solid first.

**Independent Test**: With the helper disabled (default), confirm a privileged op
is denied. With it enabled and human-approval required, confirm the op enters a
pending-approval state, the operator approves out-of-band, and only then does the
op execute and return combed signals.

**Acceptance Scenarios**:

1. **Given** the privileged helper is disabled (default), **When** the agent
   requests a privileged op, **Then** it is denied and audited.
2. **Given** the helper is enabled with human-approval required, **When** the
   agent requests an allowed op, **Then** it receives a pending-approval handle
   and the op does not run until an operator approves it out-of-band.
3. **Given** an op not on the closed allow-list, **When** the agent requests it,
   **Then** it is refused regardless of approval state.
4. **Given** any privileged op, **When** it is accepted, **Then** an audit record
   with a redacted subject is written before execution.

---

### User Story 5 - Drive remote hosts through the same surface (Priority: P5)

An agent needs to run combed commands on a remote host or container using the
exact same tool surface, with the remote daemon reached only through a secure
tunnel to its local socket -- never a public network listener.

**Why this priority**: Federation is the last human-parity frontier and depends
on a stable local surface; it is valuable but the least foundational.

**Independent Test**: Register a remote target; probe its health; run a command
with the target selected; confirm combed signals come back and that no public TCP
port was opened on either host.

**Acceptance Scenarios**:

1. **Given** a registered remote target, **When** the agent lists targets,
   **Then** each target's reachability is reported.
2. **Given** a reachable remote target, **When** the agent runs a daemon-backed
   tool with that target selected, **Then** it receives combed signals from the
   remote host identical in shape to local signals.
3. **Given** any target, **When** connectivity is established, **Then** it is via
   a tunnel to the remote daemon's local socket, with no public TCP listener
   opened.
4. **Given** no target is selected, **When** any tool runs, **Then** it executes
   locally (backward compatible).

---

### User Story 6 - Prove and certify the omni promise (Priority: P6)

The operator (and a prospective adopter) needs a single, automated way to confirm
that "an LLM never needs a separate terminal tool" actually holds on each
supported platform and across the supported agent harnesses, and a clear
capability map of what is available where.

**Why this priority**: Certification is the closing gate; it is continuous but
can only be declared complete once the prior slices land.

**Independent Test**: Run the per-platform omni smoke; confirm it exercises the
O-01..O-14 sequence and exits non-zero on any failure. Query discovery and
confirm it reports a capability matrix.

**Acceptance Scenarios**:

1. **Given** a supported platform, **When** the omni smoke runs, **Then** it
   executes the full O-01..O-14 sequence and fails loudly on any gap.
2. **Given** a running daemon, **When** the agent queries discovery, **Then** it
   receives an omni capability matrix (which capabilities are available, and why
   any are not).
3. **Given** the three primary agent harnesses, **When** their trust smokes run,
   **Then** each completes a real command -> wait -> status flow.
4. **Given** all gates pass, **When** the release is cut, **Then** documentation
   presents TC's primary identity as an omni LLM terminal tool.

---

### Cross-cutting trust and ergonomics fixes (folded into earliest touching slice)

Drawn from live field usage (`docs/field-ledger-2026-06-11-aap-campaign.md`).
These are not a separate user journey; each attaches to the slice that first
touches the relevant code path. TC-B2 (daemon self-heal) and TC-E3 (deny-doc
clarity) are already closed and out of scope.

- **TC-B1 (ANSI stripping)**: colored output must not silently defeat rule
  matching or pollute summaries with escape bytes on the non-PTY command path.
- **TC-B3 (receipt persistence)**: after a daemon restart, a status poll for a
  prior job must return a known terminal/restart-marked result, not a bare error.
- **TC-E1 (compact response mode)**: an opt-in response shape that returns only
  the load-bearing fields per signal, cutting token cost for the dominant agent
  use case.
- **TC-E2 (honest waiting)**: a way to wait until a job exits (with a server-side
  hard cap) and/or a poll-interval hint in running responses, so agents do not
  guess poll timing.
- **TC-E4 (capture de-duplication)**: a signal must not echo the same captured
  bytes under multiple redundant fields.

### Edge Cases

- A session whose underlying shell dies unexpectedly: status must report the dead
  state and further sends must fail loudly, not hang.
- Rule suggestion on output that yields no confident candidates: return an empty
  proposal set with an explanation, never a low-quality auto-activated rule.
- A privileged approval token that is replayed or expired: the second use must be
  refused.
- A remote target that becomes unreachable mid-command: the result must degrade
  honestly (known handle + recover hint), consistent with TC-E2/TC-B3.
- Adding a new tool without updating every tool-count anchor: CI count assertions
  must fail the change.
- ANSI stripping must not corrupt multibyte UTF-8 or discard legitimate payload
  bytes; raw bytes remain available in the frame store.

## Requirements *(mandatory)*

### Functional Requirements

Sessions and workspace (P1)

- **FR-001**: System MUST provide persistent shell sessions that preserve working
  directory and environment across successive command sends within a session.
- **FR-002**: System MUST expose session lifecycle operations to start, send a
  line to, query status of, stop, and list sessions.
- **FR-003**: Session capability MUST be denied by default and enabled only via an
  explicit operator capability flag; every start MUST be policy-checked and
  audited.
- **FR-004**: System MUST enforce a configurable maximum number of concurrent
  sessions and an idle time-to-live, reclaiming resources on TTL expiry.
- **FR-005**: System MUST allow saving a workspace snapshot (cwd + bounded env)
  and restoring it into a session.
- **FR-006**: Session output MUST be combed (bounded structured signals), never a
  raw stream.

Parse omni (P2)

- **FR-007**: System MUST suggest candidate parsing rules from supplied output
  samples, returning a confidence label and explicit next steps.
- **FR-008**: System MUST NOT auto-activate any suggested rule; activation MUST
  require an explicit test-then-activate sequence.
- **FR-009**: System MUST, when enabled, emit baseline low-severity signals
  (error/warning/exit/progress) for any command even with no tool-specific pack
  active.
- **FR-010**: System MUST ship at least 25 built-in rule packs, including docker,
  kubectl, and git.
- **FR-011**: System MUST include a hint identifying an available pack when a
  recognized tool is started without that pack active.

Platform parity (P3)

- **FR-012**: System MUST make interactive PTY operations available on native
  Windows, with discovery accurately reporting availability.
- **FR-013**: System MUST pass a daemon + MCP smoke on macOS.
- **FR-014**: System MUST deliver file-change signals via an event-driven backend
  on native filesystems while retaining a polling fallback for WSL 9P mounts.
- **FR-015**: System MUST attempt a graceful terminate before a forced kill when
  stopping a command, PTY, or session, and MUST report the terminal state.

Privileged helper (P4)

- **FR-016**: System MUST execute privileged operations only through a separate
  helper limited to a closed allow-list of named operations; there MUST be no
  generic privilege escalation and no shell line on the privileged path.
- **FR-017**: System MUST support an operator-configured human-approval flow that
  holds a privileged op in a pending state until approved out-of-band.
- **FR-018**: System MUST deny any privileged operation not on the allow-list
  regardless of approval state, and MUST audit every accepted privileged op with
  a redacted subject before execution.

Remote federation (P5)

- **FR-019**: System MUST allow targeting a registered remote host on any
  daemon-backed operation, defaulting to local when no target is selected.
- **FR-020**: System MUST reach remote daemons only through a secure tunnel to
  their local socket and MUST NOT open a public network listener.
- **FR-021**: System MUST expose operations to list registered targets with
  reachability and to probe a single target's health.

Certification and release (P6)

- **FR-022**: System MUST provide per-platform automated smokes that run the full
  O-01..O-14 acceptance sequence and exit non-zero on any failure.
- **FR-023**: System MUST report an omni capability matrix via discovery,
  including the reason any capability is unavailable.
- **FR-024**: System MUST provide trust smokes for the three primary agent
  harnesses and an agent decision-tree playbook document.
- **FR-025**: Release documentation MUST present TC's primary identity as an omni
  LLM terminal tool and reflect the final capability set.

Cross-cutting trust/ergonomics

- **FR-026**: System MUST strip ANSI/CSI escapes before rule matching and in
  emitted summaries on the non-PTY command path, while preserving raw bytes in
  the frame store; stripping MUST be the default and MUST be UTF-8-safe.
- **FR-027**: System MUST persist job/bucket receipts so a status poll after a
  daemon restart returns a known terminal or restart-marked result rather than a
  bare error.
- **FR-028**: System MUST offer an opt-in compact response mode returning only
  load-bearing per-signal fields.
- **FR-029**: System MUST offer an honest wait-until-exit option bounded by a
  server-side cap and/or a poll-interval hint in running responses, never
  exceeding an advertised cap.
- **FR-030**: System MUST collapse redundant duplicate capture echoes into a
  single canonical field plus named captures.

Global invariants (apply to every requirement above)

- **FR-031**: The MCP adapter MUST NOT spawn commands or perform side effects;
  all execution MUST flow through the daemon over local IPC.
- **FR-032**: Every new tool addition MUST update all tool-count anchors and the
  discovery fixture in the same change, and CI count assertions MUST pass.
- **FR-033**: No production code path may depend on test-only/mock logic; every
  behavior-bearing change MUST carry a source-status label and pass the
  fmt + clippy(-D warnings) + nextest gate.

### Key Entities

- **Shell Session**: A long-lived interactive shell owning sticky cwd/env, a
  bounded env snapshot, a signal bucket, a lifecycle state, and idle/limit
  bookkeeping.
- **Workspace Snapshot**: A saved, restorable cwd + bounded env captured from a
  session.
- **Rule Suggestion**: A candidate parsing rule derived from samples, carrying a
  confidence label and next-step guidance; never self-activating.
- **Rule Pack**: A named bundle of parsing rules for a tool family, importable on
  demand.
- **Privileged Operation**: A named entry on a closed allow-list with parameters,
  an approval state, and an audit subject.
- **Remote Target**: A registered host reachable via a secure tunnel to its local
  daemon socket, with an identity and reachability state.
- **Capability Matrix**: A discovery-time map of which omni capabilities are
  available on this host and why any are not.
- **Job/Bucket Receipt**: A persisted record of a job's terminal state surviving
  daemon restart.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An agent can complete a multi-step workflow that depends on shared
  working directory across at least three sends without ever re-passing the
  directory (gate O-02).
- **SC-002**: An agent can take an unknown tool from "no signal" to "structured
  signals only" using suggestion -> test -> activate, with zero rules activated
  without an explicit activation step (gate O-05).
- **SC-003**: At least 25 rule packs are available out of the box (up from 8),
  including docker, kubectl, and git.
- **SC-004**: Interactive REPL parity holds on native Windows and macOS in
  addition to Linux/WSL (gates O-03, O-07, O-08).
- **SC-005**: A privileged install completes only after explicit operator
  approval, and is impossible without the operator opting in (gate O-06).
- **SC-006**: An agent runs a combed command on a remote host with no public
  network port opened on either host (gates O-09, O-10).
- **SC-007**: A single automated smoke per platform runs O-01..O-14 and fails
  loudly on any unmet gate; all 14 gates pass on Linux and WSL at release (gate
  O-14).
- **SC-008**: The compact response mode reduces per-signal response size by a
  substantial, measured margin (target ~5x fewer non-payload bytes) versus the
  default shape on a representative signal set.
- **SC-009**: After a daemon restart, a status poll for a previously started job
  returns a known result (terminal state or restart marker) in 100% of cases,
  never a bare error.
- **SC-010**: Colored command output that previously defeated anchored rule
  matching now matches reliably, and summaries contain no escape bytes, with no
  loss of legitimate payload.
- **SC-011**: Every advertised wait/byte cap is honored to the wire (measured
  wall time never exceeds the advertised wait cap).

## Assumptions

- The two-process architecture, local-only IPC, policy-before-spawn, and
  combed-output invariants are fixed; all new capability is added via
  policy-gated seams, consistent with `.specify/memory/constitution.md`.
- Persistent shell sessions are implemented over the existing PTY runtime
  (long-lived login shell), reusing PTY infrastructure rather than inventing a
  new process model.
- Rule suggestion v1 is pure-Rust heuristics (error/warning/FAILED/path shapes),
  not a learned model; higher-fidelity suggestion is out of scope for this
  program.
- Remote federation ships SSH local-forward to a remote socket first; container
  targeting is acceptable as a follow-on within P5.
- Windows interactive PTY uses the platform's native pseudo-console facility;
  macOS reuses the existing POSIX PTY path.
- The privileged helper (P4) requires an explicit threat review before any code
  lands; for THIS program run P4 is plan/spec only -- no privileged code is
  written until that review completes. Default configuration ships it disabled.
- Execution mode for this program run: implement as many priority slices as
  possible autonomously, in priority order, committing each completed slice to
  its own review branch and pausing before any merge/push.
- "Tool" here means an MCP tool surface; "agent" means the LLM harness consuming
  it; "operator" means the human who owns the host and configures capabilities.
- Cross-cutting fixes (TC-B1/B3/E1/E2/E4) attach to the earliest slice that
  touches their code path rather than forming a separate release.
- Each priority slice is independently shippable and independently testable; the
  program may stop after any slice with a coherent product.
- Final release version target is 1.0.0 once all O-* gates are green; an
  intermediate 0.2.0 is acceptable if slices ship incrementally.

## Out of Scope

- Removing or weakening any existing security guard to add a capability.
- A public TCP daemon listener under any configuration.
- Generic sudo / arbitrary privileged shell execution.
- Auto-activation of suggested rules.
- Streaming unbounded raw stdout/stderr on a normal tool response.
- A learned/ML rule-suggestion model (heuristics only in this program).
- Already-closed ledger items TC-B2 (self-heal) and TC-E3 (deny-doc).
- Landing any privileged-helper code (P4) before its dedicated threat review;
  this program run produces only the P4 plan/spec, not its implementation.
