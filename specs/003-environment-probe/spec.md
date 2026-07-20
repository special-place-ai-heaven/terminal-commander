# Feature Specification: Goal-Directed Environment Probe

**Feature Branch**: `003-environment-probe` (spec directory; no git branch created)

**Created**: 2026-07-17

**Status**: Draft

**Input**: User description: Give an LLM one low-cost environment probe that can
start from any harness or isolation boundary, progressively discover what it is
connected to, fan out goal-relevant sensors, find the strongest authorized
execution path, and return a concise trustworthy readiness map without exposing
environment values or raw probe noise.

## Overview

An LLM entering an unfamiliar project currently spends several calls guessing
where it is running, locating the workspace, testing shells and runtimes,
reading manifests, checking dependency versions, and discovering whether the
useful environment is native, inside WSL, in a container or VM, or behind a
remote connection. Those calls cost time and context, frequently exercise the
wrong environment, and can still produce a false readiness conclusion.

Terminal Commander will expose a goal-directed `environment_probe` that turns
one compact request into a staged sensor campaign. It begins with the evidence
available from the harness, learns the isolation and execution topology,
establishes trustworthy connections, discovers the workspace and goal-specific
requirements, then unlocks progressively deeper sensors. It returns one bounded
signal map when the campaign is terminal, or one bounded `in_progress` receipt
that resumes the same campaign without repeating discovery. A terminal map
contains readiness, selected beachhead, blockers, observed versions,
alternatives, provenance, completeness, and unresolved blind spots.

The operating model follows *Physarum polycephalum*: send inexpensive scouts
along plausible paths, follow improving evidence gradients, reinforce verified
routes, retract stale or failed paths, and never assume terrain that has not
been sensed. This is a deterministic evidence model, not a biological claim or
a probabilistic substitute for verification.

The campaign advances in bounded waves. Each wave fans out disposable,
single-purpose sensors, collapses branches that reach terminal negative states,
and deepens only the surviving convergence points. When enough evidence exists
to answer the goal, the planner retracts the surviving evidence into one combed
result and disposes every sensor instance. Sensors observe; they do not plan,
delegate, retain authority, or become miniature orchestration engines.

## Clarifications

### Session 2026-07-17

- Q: Is implementation scope a design constraint? -> A: No. Choose the fully
  correct and superior product; scope may grow wherever LLM usefulness, trust,
  and robustness require it.
- Q: What is the primary LLM interaction? -> A: One goal-directed trigger that
  fans out many internal actions and returns one combed response.
- Q: Must the LLM provide a hardcoded probe directory? -> A: No. Workspace scope
  is inferred from evidence; an explicit path is only an override.
- Q: How does the probe learn an unknown environment? -> A: In ordered layers.
  Each sensor declares prerequisite evidence, and deeper sensors remain locked
  until earlier stages prove the facts they need.
- Q: Which origins and targets are in scope? -> A: All supported harness, host,
  target, isolation, and transport combinations, including Codex, Claude Code,
  Cursor, generic MCP clients, AAP embedding, Windows, WSL, Linux, macOS,
  containers, sandboxes, VMs, remote daemons, and Firecracker guests.
- Q: Can reusable probe plans grow over time? -> A: Yes. Plans are strict,
  versioned, declarative compositions of trusted sensor capabilities and are
  governed separately from output-sifting rules.
- Q: How does a broad first ping avoid becoming a fixed expensive checklist? ->
  A: It runs bounded waves: fan out cheap scouts, converge on evidence-supported
  branches, deepen only those branches, then retract and synthesize the result.
- Q: Where does probe intelligence live? -> A: In the central planner. Individual
  sensor invocations are disposable, narrow, deterministically bounded
  point-in-time observations.
- Q: Does one trigger require every topology to finish in one response? -> A:
  No. It forbids preparatory discovery and duplicate campaigns. The initial call
  returns a terminal map or a resumable `in_progress` receipt for the same run.
- Q: May normalized facts be derived from environment-variable values? -> A:
  Only inside the single FR-053 private-resolver carve-out. An approved,
  product-defined target-local resolver may consume an observed value as opaque
  tainted input and emit only its closed non-reconstructive typed result. No raw
  or reconstructive value is ever a public fact, receipt, diagnostic, cache,
  audit, or transport payload. Presence of approved names remains ordinary
  evidence; every other value-derived fact remains forbidden.
- Q: Does the persistent goal-plan registry remain in scope despite the older
  CAP01 deferral? -> A: Yes. The current product goal deliberately supersedes
  that scope deferral because reusable operator plans now have a concrete second
  use case. Their types, storage, activation, and authority remain strictly
  separate from sifter rules.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - One ping from uncertainty to a beachhead (Priority: P1)

An LLM begins work in an unfamiliar repository and asks Terminal Commander to
prepare for a goal such as build, test, run, develop, or diagnose. The LLM does
not first locate the workspace, identify the operating system, enumerate tools,
or experiment with shells. One probe returns whether the goal is ready, the
best verified execution route, exact blockers, and bounded alternatives.

**Why this priority**: The feature exists to remove the repeated calls, tokens,
time, and mistakes consumed by manual environment onboarding.

**Independent Test**: Start from an unambiguous project root with no explicit
workspace or target argument. A single initial probe call starts exactly one
campaign and returns either a terminal readiness state plus a runnable beachhead
or exact blockers, or an `in_progress` receipt that resumes that same campaign.

**Acceptance Scenarios**:

1. **Given** a harness with one discoverable project root and a goal of `test`,
   **When** the LLM sends one environment probe, **Then** the response identifies
   the workspace evidence, relevant stack, best authorized target, prerequisite
   verdicts, and exact beachhead without a preparatory tool call.
2. **Given** the native target lacks a required runtime but a verified WSL or
   remote target satisfies it, **When** the probe completes, **Then** the ready
   target is selected and the native target is retained as a bounded rejected
   alternative with its blocker.
3. **Given** no authorized target satisfies the goal, **When** every safe route
   reaches a terminal state, **Then** the result is `blocked` or `unknown`, lists
   the unresolved layer, and never invents a usable path.
4. **Given** the bounded initial wait expires while viable branches remain,
   **When** the response is returned, **Then** it is `in_progress` and contains
   the stable probe identifier, current stage, cursor, and recovery guidance
   without launching or requiring a second campaign.

---

### User Story 2 - Learn by staged sensing, never by assumption (Priority: P1)

A probe may begin inside a sandbox, VM, container, WSL distro, remote IDE, or
unknown MCP harness. It first uses low-risk evidence from the harness and local
operating system, then attests transports and target identity, then brings
target-native and goal-specific sensors online. A Python dependency sensor, for
example, cannot run until a Python interpreter in that exact target has been
resolved and its use authorized.

**Why this priority**: A sensor that assumes the target environment can return
confident facts about the wrong machine. Progressive evidence is the core trust
mechanism.

**Independent Test**: Provide a topology where the harness host, engine host,
and final execution guest are different platforms. Verify that every fact is
attributed to the correct node and that no deeper sensor starts before its
declared evidence gates are satisfied.

**Acceptance Scenarios**:

1. **Given** only harness identity and protocol capabilities, **When** a probe
   begins, **Then** those facts guide the first sensors but are labelled as
   harness evidence rather than target truth.
2. **Given** a candidate remote transport that connects but has not proven
   target identity, **When** the route is evaluated, **Then** target-native
   prerequisite facts remain locked or explicitly identity-unverified.
3. **Given** a sensor returns incomplete evidence, **When** a safer follow-up
   sensor exists, **Then** the probe graph unlocks that sensor; otherwise the
   final result remains incomplete and names the blind spot.
4. **Given** ten plausible initial routes of which two survive transport,
   identity, policy, and relevance gates, **When** the next wave begins, **Then**
   only those two convergence points receive deeper sensors and the other eight
   are retracted into bounded terminal reasons.

---

### User Story 3 - Trust every returned fact (Priority: P1)

An operator and LLM can rely on every probe fact because it is policy-truthful,
target-specific, freshness-labelled, bounded, and accompanied by provenance.
Environment-variable values and raw helper output never reach the caller or any
persistent diagnostic surface.

**Why this priority**: Saving calls is harmful if the replacement can leak a
secret, execute an unapproved helper, misidentify a remote target, or describe
stale evidence as current.

**Independent Test**: Place recognizable canary values in harness, engine,
session, WSL, remote, and guest environments. Exercise success, failure,
timeout, truncation, cache, audit, snapshot, and recovery paths. Verify that no
canary value appears and every returned fact has complete provenance.

**Acceptance Scenarios**:

1. **Given** sensitive and benign environment values, **When** targeted presence
   and an explicit names-only census are requested, **Then** only names and
   presence states can leave the observation source.
2. **Given** a fixed helper exits with output containing unexpected text, **When**
   the parser fails, **Then** the caller receives a typed bounded failure without
   captured stdout or stderr.
3. **Given** a configured remote label points to an unpinned or mismatched
   daemon identity, **When** the probe connects, **Then** it reports
   `identity_unverified` or `identity_mismatch` and cannot claim target facts as
   verified.

---

### User Story 4 - Work from any harness and through any supported topology (Priority: P1)

The same LLM-facing call works when Terminal Commander is installed under
Codex, Claude Code, Cursor, another MCP client, or an application embedding the
engine. It works across Windows, WSL, Linux, and macOS and across same-host,
sandbox, container, VM, forwarded remote, and multi-hop guest execution.

**Why this priority**: Harness and operating-system assumptions are the exact
uncertainty the feature is meant to remove. A solution tied to one starting
point is not a trustworthy onboarding primitive.

**Independent Test**: Execute the declared harness/host/target/workspace/policy/
transport matrix. Every row returns a classified result through the same
canonical evidence contract, including deliberately unsupported combinations.

**Acceptance Scenarios**:

1. **Given** Codex on Windows with a policy-usable WSL target, **When** the goal
   is better satisfied inside WSL, **Then** Windows and WSL quirks are observed
   by their own sensors and the returned path includes proven path translation.
2. **Given** a macOS harness whose interactive shell environment differs from
   its launch-service environment, **When** both matter to the goal, **Then** the
   result distinguishes the two sources rather than merging them.
3. **Given** AAP embeds Terminal Commander and executes inside a Firecracker
   guest, **When** a probe runs, **Then** the topology records the embedded
   engine, guest connector, vsock boundary, guest identity, and guest-native
   evidence without inventing an MCP adapter hop.

---

### User Story 5 - Reuse and improve goal knowledge (Priority: P2)

Common goals such as Python testing, Rust building, Node development, Go
testing, .NET building, Java tooling, native compilation, container work, and
Git/GitHub operations are represented by reusable versioned probe plans. The
LLM can normally provide only a goal; workspace evidence selects and specializes
the relevant plans. Operators can add declarative local plans without adding
new executable behavior.

**Why this priority**: Reusing proven goal knowledge eliminates repeated request
shapes and lets Terminal Commander improve onboarding without teaching every
LLM a growing sensor catalogue.

**Independent Test**: Add and activate a local declarative plan that composes
existing trusted sensors. Verify that one goal call selects it, records its
version and digest, and cannot use the plan to run caller-defined commands or
gain authority.

**Acceptance Scenarios**:

1. **Given** a workspace with multiple recognized manifests, **When** the goal
   applies to more than one stack, **Then** the probe composes the relevant plans
   and explains the resulting requirement set.
2. **Given** a plan references an unavailable sensor or invalid version rule,
   **When** it is validated or activated, **Then** it is rejected before a probe
   can use it.
3. **Given** a previously successful route, **When** a later probe starts, **Then**
   decaying route history may change scout order but current policy, identity,
   and readiness must be proved again.

---

### User Story 6 - Survive interruptions and leave no debris (Priority: P2)

A long or multi-hop probe continues under Terminal Commander ownership when the
MCP transport disconnects. The LLM can recover the same run, receive its final
signal map, and trust that duplicate helpers were not launched. Completed and
abandoned runs clean themselves up.

**Why this priority**: A one-trigger experience is not superior if transient
adapter failures force the LLM to repeat expensive discovery or leave orphaned
processes and accumulated state.

**Independent Test**: Interrupt the client during a multi-target probe, reconnect,
and recover by probe identifier. Verify one logical run, one set of helper
actions, the same final evidence, and zero retained runtime artifacts after the
retention window.

**Acceptance Scenarios**:

1. **Given** a client disconnect after probe fan-out, **When** the client resumes,
   **Then** the same run continues and known evidence is preserved.
2. **Given** two identical concurrent requests, **When** both are accepted,
   **Then** the external sensor work is shared while each caller receives a
   truthful receipt.
3. **Given** a completed, failed, timed-out, or abandoned run, **When** its
   retention window expires, **Then** it leaves no live helpers, handles,
   buckets, temporary files, or unbounded historical state.

### Edge Cases

- The harness supplies no roots, several roots, a stale root, or a root that is
  valid on the harness but not addressable from the selected target.
- Harness identity is unknown, its version is newer than the compatibility
  catalogue, or it declares a capability but rejects the corresponding request.
- The engine starts inside a sandbox while the intended workspace and runtime
  exist only outside it.
- WSL, virtualization, containerization, sandbox restrictions, and CI context
  coexist in one nested stack rather than forming mutually exclusive states.
- A same-sandbox daemon is healthy but is not equivalent to the requested host
  or guest execution environment.
- A route is reachable but denied by policy, protocol-skewed, identity-unpinned,
  identity-mismatched, or replaced during the run.
- A WSL distro exists but is stopped, lacks the expected interop, maps the
  workspace differently, or provides different case and permission semantics.
- Windows environment names differ only by case; Unix names contain non-UTF8
  bytes; returned names approach item, per-name, or total byte caps.
- macOS launch-service, login-shell, and interactive-shell environments differ.
- A container or VM masks host tools, mounts, network routes, CPU architecture,
  or filesystem semantics.
- A translated path is reachable on the target but resolves to another checkout
  or another nested project with a different workspace identity.
- A configured WSL distro, container, VM, or guest is stopped and observation
  would wake or start it.
- Multiple candidate routes are ready but disagree on runtime or dependency
  versions.
- A manifest is malformed, mutually inconsistent with a lock or toolchain file,
  or belongs to several nested projects.
- A version is valid in its ecosystem but not representable by another
  ecosystem's comparison rules.
- A helper hangs, forks descendants, writes unexpected output, changes target
  state during inspection, or disappears between resolution and execution.
- Evidence becomes stale while the run is active or conflicts with a stronger
  target-native observation.
- A probe result exceeds response bounds after many requirements or targets.
- No authorized connector can cross the requested isolation boundary.

## Requirements *(mandatory)*

### Functional Requirements

#### One-trigger LLM experience

- **FR-001**: The compact surface MUST add a sixth `environment` facade whose
  primary action is `probe`, and the full surface MUST add
  `environment_probe`; both MUST translate to the same engine operation. The
  compact count source, count anchors, admission tests, and discovery fixtures
  MUST change together.
- **FR-002**: A valid normal request MUST require only a goal; workspace, target,
  plan, requirement, detail, and freshness inputs MUST be optional refinements.
- **FR-003**: The probe MUST support at least build, test, run, develop,
  diagnose, and automatic goal modes plus a structured custom requirement mode.
- **FR-004**: One engine-owned request MUST orchestrate workspace discovery,
  topology discovery, route validation, relevant prerequisite checks, route
  selection, and result synthesis without preparatory calls. MCP adapters and
  embedded hosts MAY enrich delivery context but MUST NOT implement campaign
  planning, fan-out, remote selection, polling loops, or result synthesis.
- **FR-005**: The initial response MUST prioritize either a terminal readiness
  map or `in_progress` with the stable probe identifier, current stage,
  continuation cursor, bounded retry hint, known evidence, and degraded recovery
  fields. A terminal map MUST prioritize the best verified beachhead, blockers,
  selected facts, bounded alternatives, provenance, completeness, and a receipt.
- **FR-006**: The default response MUST suppress repetitive satisfied evidence
  while reporting the suppressed count and preserving bounded access to detail.
- **FR-007**: Every invalid goal, plan, field, or action request MUST be rejected
  before campaign admission and return a bounded current-surface choice set for
  that invalid namespace plus one schema-validated corrective call that the
  current surface accepts. Recovery MUST allocate no campaign, job, runtime
  resource, persistent continuation state, external observation, mutation, or
  authority.
- **FR-008**: The product MUST NOT install, upgrade, repair, or silently modify
  the inspected environment as part of a probe.

#### Staged evidence and adaptive pathfinding

- **FR-009**: Every sensor invocation MUST be a single-purpose, deterministically
  bounded point-in-time observation that declares the evidence, policy,
  connection, and capability prerequisites that unlock it and returns only typed
  evidence, a terminal state, provenance, and bounded parser diagnostics. It
  MUST NOT select routes, launch other sensors, persist authority, or outlive
  planner ownership.
- **FR-010**: The central planner and scheduler MUST be the sole campaign
  authority that unlocks, launches, converges, retracts, and disposes sensor
  invocations. Before every external observation, helper, file read, or connector
  action, the engine governing that action MUST positively authorize both its
  sensor class and every underlying command, file, probe, or connector action.
  No legacy discovery sentinel is exempt.
- **FR-011**: Unknown or incomplete evidence MUST trigger the next safe discovery
  step when one exists and MUST otherwise remain explicit in the final result.
- **FR-012**: The evidence ladder MUST distinguish harness assertion, local OS
  observation, route hypothesis, transport proof, target identity proof,
  target-native observation, fixed-helper observation, and derived conclusion.
- **FR-013**: A derived fact MUST identify every source fact it depends on and
  MUST never be presented as directly observed.
- **FR-014**: Candidate exploration MUST advance over the currently unlocked
  frontier in bounded waves: fan out disposable sensors along every plausible
  authorized connector, collapse and cancel terminally excluded descendants,
  converge only branches whose target and workspace identities agree, deepen
  only evidence-supported convergence points, and retract discarded runtime
  resources while retaining bounded terminal reasons and counts.
- **FR-015**: Policy denial, target identity failure, and proven incompatibility
  MUST hard-exclude a route from ready selection.
- **FR-016**: Soft gradients MAY order scouts, but terminal route selection after
  hard exclusions MUST be deterministic and lexicographic: goal satisfaction,
  evidence grade, completeness, freshness, operator preference among equally
  qualified routes, then stable route identity. Identical evidence MUST select
  the same beachhead.
- **FR-017**: Historical route success MAY reorder scouts but MUST decay, MUST be
  invalidated by relevant topology changes, and MUST NOT count as current proof.
- **FR-018**: Probe progression MUST be driven by positive and negative terminal
  sensor states; elapsed time MAY only bound silence and cannot imply readiness.
- **FR-019**: Every wave and the total run MUST terminate within declared bounds
  even when no sensor produces a positive signal, and bounded termination MUST
  yield explicit partial evidence rather than an infinite wait.

#### Harness, workspace, and topology discovery

- **FR-020**: The MCP adapter MUST capture initialize/session evidence and pass
  one immutable, peer-bound typed Harness Context to the engine; embedded hosts
  MUST supply the same contract. It MUST normalize harness name/version,
  protocol version, declared capabilities, roots support, explicitly referenced
  TC session context, transport, and available environment-name evidence while
  distinguishing adapter-observed facts from caller assertions. Ambient daemon
  cwd MUST NOT masquerade as harness context.
- **FR-021**: Known harness and operating-system combinations MAY provide
  versioned predictive ordering and recovery hints but MUST be verified at use.
- **FR-022**: Unknown or changed harness versions MUST fall back to a generic
  protocol evidence ladder rather than failing or inheriting another harness's
  assumptions.
- **FR-023**: An explicit workspace override MUST rank first only after it is
  validated. TC session cwd, client roots, configured policy root, and discovered
  project markers MUST remain candidates until each is validated for authority,
  freshness, containment, target reachability, and workspace identity. A
  heuristically tracked cwd MUST NOT silently outrank a verified client root.
- **FR-024**: The result MUST identify the winning workspace source and MUST
  return bounded candidates instead of guessing when evidence is ambiguous.
- **FR-025**: Workspace reachability and identity MUST be verified independently
  for every selected target. Path existence or translation alone is
  insufficient; bounded VCS/worktree identity, canonical target root, and
  relative manifest, lock, and toolchain evidence MUST prove equivalence.
  Mismatch MUST yield `workspace_mismatch` and prohibit convergence or a
  beachhead.
- **FR-026**: The probe MUST model harness, adapter, engine, runner, guest, and
  final execution environment as nodes in an observed execution topology.
- **FR-027**: Every topology edge MUST identify its transport, direction,
  policy state, reachability state, identity state, and observation time.
- **FR-028**: Supported connectors MUST cover same-host native execution, WSL,
  forwarded remote daemons, containers or sandboxes where enabled, VM/guest
  transport, and in-process embedding. Configured multi-hop traversal MUST
  authorize and attest every hop independently, enforce a declared hop bound,
  track visited persistent and boot identities to prevent cycles, and downgrade
  every descendant when an ancestor hop is unverified or changes.
- **FR-029**: AAP embedding MUST represent its embedded engine, Firecracker
  connector, vsock boundary, guest identity, and guest-native execution facts
  without adding a synthetic MCP adapter.
- **FR-030**: Isolation discovery MUST model an ordered, composable stack plus
  independent context facets: operating system, virtualization, WSL, container,
  sandbox/restriction, automation/CI, and privilege context. `no_boundary_observed`
  MUST replace any unprovable `bare_host` claim, and every layer MUST carry its
  own evidence, capability restrictions, and unknown state.
- **FR-031**: Outward reachability checks MUST target only configured and
  policy-approved destinations and MUST never perform arbitrary network scans.
- **FR-032**: Connection failures MUST distinguish policy denial, absent
  transport, target stopped, name-resolution failure, refusal, timeout,
  protocol skew, identity-unverified, identity mismatch, and target replacement.
  Normal probes MUST NOT start or wake a stopped WSL distro, container, VM, or
  guest; that requires explicit per-run authorization and an audit receipt.

#### Platform-native sensing

- **FR-033**: One canonical evidence contract MUST be populated by distinct
  platform-native sensor implementations rather than a generic host assumption.
- **FR-034**: Windows sensing MUST account for case-insensitive environment-name
  equality, native string encoding, process and job boundaries, executable
  resolution, `PATHEXT`, aliases and shims, Windows path semantics, `cmd.exe`
  built-ins and quoting, and PowerShell Desktop/Core version, architecture, and
  execution policy. Direct argv, shell bridge, and session lanes MUST remain
  distinct evidence.
- **FR-035**: WSL sensing MUST account for WSL1/WSL2, distro and default-user
  identity, lifecycle, both interop directions, mount and path translation,
  cwd semantics, Linux name semantics, direct `wsl.exe -e` execution, and
  nested-shell policy. Host-to-distro and distro-to-paired-host routes MUST bind
  the attested Windows host, distro, user, and both boot identities; reverse
  interop MUST NOT infer an arbitrary Windows host from WSL presence. A
  discovered lane MUST be accepted by the same request schema and policy
  validator that will execute it.
- **FR-036**: Linux sensing MUST account for distro and runtime identity,
  namespaces or containers, process visibility, mount boundaries, case-sensitive
  byte-oriented names, and direct-exec semantics.
- **FR-037**: macOS sensing MUST account for launch-service versus shell
  environments, architecture-specific package paths, native toolchain and SDK
  selection, platform restrictions, and Darwin/BSD tool behavior.
- **FR-038**: Unsupported platform facts MUST remain explicitly unsupported and
  MUST NOT be inferred from a related platform family.
- **FR-039**: Environment-name comparison, sorting, deduplication, encoding, and
  truncation MUST follow the observed target's semantics without lossy
  collisions. The wire form MUST tag valid UTF-8 text, raw Unix bytes encoded as
  base64, or Windows UTF-16 code units encoded as base64; display text MUST never
  serve as the equality or deduplication key.

#### Goal plans and prerequisite truth

- **FR-040**: Executable sensor behavior MUST be limited to trusted,
  product-defined, versioned sensor adapters. Every adapter MUST declare a
  side-effect class and use direct execution, a least-privilege environment, and
  controls that disable downloads, updates, hooks, plugins, startup files,
  module imports, build scripts, lifecycle scripts, and project-code execution.
  Executable resolution MUST exclude cwd/workspace search, canonicalize and bind
  stable file identity, classify origin and writability, and bind both interpreter
  and script identities for shims. Workspace-writable, user-writable, or
  unprovably writable candidates require explicit
  `allow_environment_untrusted_exec`; otherwise they are not executed.
  Canonical identity without an operator pin or catalogue identity is sufficient
  only for a system-installed candidate proved not writable by the subject, with
  complete form-specific binding and unchanged dispatch identity; every other
  canonical-identity-only candidate is `unknown`. If passive behavior or
  executable identity cannot be proved, the action returns `denied` or `unknown`.
- **FR-041**: Reusable goal plans MUST be strict declarative compositions of
  approved sensors, requirements, evidence gates, fallback rules, and output
  selection.
- **FR-042**: Goal plans MUST NOT represent caller-defined commands, scripts,
  shell text, executable paths, credentials, environment values, permissions,
  authority grants, side effects, or runtime behavior. A bounded versioned-field
  validator MUST derive a complete disjoint typed forbidden-content cause set
  before persistence. Rejection MUST persist no plan content, grant no authority,
  and expose only typed causes; diagnostics and audit MUST NOT contain the rejected
  command, script, shell, path, credential, environment-value, permission, or
  authority material.
- **FR-043**: Goal plans MUST use separate dependency-light core types, storage
  tables and migrations, validation, activation authority, audit actions, and
  LLM operations from output-sifting rules. They MUST be stored, searched,
  versioned, validated, tested, activated, and audited without reusing the rule
  registry or rule-pack lifecycle. This deliberately supersedes the older CAP01
  deferral because repeated built-in and operator-local goal compositions are
  now an approved product requirement.
- **FR-044**: The product MUST ship built-in plans for mainstream Python, Rust,
  Node, Go, .NET, Java, C/C++ build, container, Git, and GitHub goals.
- **FR-045**: Operators MUST be able to add local declarative plans that compose
  existing approved sensors without adding executable behavior.
- **FR-046**: Workspace manifest and toolchain evidence MUST automatically
  select and specialize applicable goal plans.
- **FR-047**: Multi-stack workspaces MUST support composition of several plans
  with conflicts and duplicated requirements resolved explicitly. Admission MUST
  prove exact set equality between the bounded manifest-family candidate set, the
  typed selected plan identity/version/digest/family set, the composition map, and
  the frozen snapshot using canonical count/roots; missing, extra, duplicated, or
  mismatched members MUST NOT derive a valid composed plan.
- **FR-048**: Callers MUST be able to add typed environment-name and prerequisite
  requirements without supplying executable instructions.
- **FR-049**: Known tool checks MUST reuse stable catalogue identities and MUST
  record the catalogue revision used.
- **FR-050**: Runtime, toolchain, and package compatibility MUST use the target
  ecosystem's comparison semantics; an unprovable comparison MUST be `unknown`.
  A comparator catalogue revision MUST become trusted only after a complete
  versioned mandatory conformance corpus passes against independent official
  reference fixtures. The corpus MUST cover ecosystem-specific canonical and
  normalized releases, prerelease/development/postrelease ordering,
  epoch-or-equivalent ordering, local/build metadata semantics, malformed input,
  and ambiguous or unsupported input, with exact required/observed parse classes
  and relation oracles. Missing keys, non-independent fixtures, or any observed/
  oracle mismatch MUST fail the trusted-revision gate.
- **FR-051**: Python package checks MUST use distribution metadata and MUST NOT
  import or execute package code.
- **FR-052**: Every prerequisite MUST expose two orthogonal axes. `verdict` MUST
  be exactly one of `satisfied`, `missing`, `incompatible`, `unknown`, `denied`,
  `unreachable`, or `not_applicable`; `observation_status` MUST be exactly one of `complete`,
  `in_progress`, `probe_failed`, `timed_out`, `truncated`, or `unsupported`.
  `missing` requires a complete authoritative search scope; incomplete,
  inaccessible, or denied observation MUST NOT be reported as missing.
  `not_applicable` is valid only for a frozen conditional or informational
  requirement proved irrelevant on the target. A live `in_progress` record is
  never a terminal RouteModel input; terminal route projection accepts only the
  other five observation statuses.

#### Privacy, identity, policy, and audit

- **FR-053**: An Observed Environment Value is any value read from an inherited,
  process, shell, session, target, guest, or harness environment during sensing.
  No such value may directly become a fact or cross a public, transport, embed,
  audit, log, error, output, context, snapshot-response, or cache surface.
  A trusted product-defined target-native resolver MAY consume a goal-relevant
  value locally as opaque tainted input only when platform resolution requires
  it (for example PATH/PATHEXT lookup). Before reading, the goal plan and sensor
  admission MUST freeze exactly one requested context: target process, target
  shell/session, or harness process. The private resolver receipt MUST prove that
  the observed source kind and persistent-instance/boot/workspace/action context
  exactly match that request. Any cross-context substitution, ambient harness
  fallback, or missing context MUST reject. Harness observation is valid only for
  an explicit harness-context plan row authorized as `EnvironmentPrivateResolve`;
  a generic harness sensor receipt cannot substitute. The raw or reconstructive value MUST stay
  inside the private resolver, MUST NOT be retained or transported, and MUST be
  counted under frozen independent opaque-input-byte and resolver-candidate caps
  while reading and before unbounded allocation or dispatch. Opaque input uses
  POSIX raw bytes or the checked product of two times the Windows UTF-16
  code-unit count; transcoding MUST NOT precede this gate. Reaching either cap
  MUST stop resolution, destroy partial private input, emit only a typed
  non-content bound result, and admit no fact; exceeding a cap is an invariant
  failure. The value MUST be
  discarded before only a non-reconstructive typed presence, canonical identity,
  or version fact leaves with `platform_resolver` provenance, an exact approved
  resolver revision and closed typed-conversion receipt. Every platform-native,
  fixed-helper, or future producer that can touch private tainted input MUST be in
  one authoritative versioned producer registry crossed with the authoritative
  forbidden-sink inventory. Producer publication or execution MUST require an
  exact-current build/runtime attestation for the complete producer x sink x
  success/failure/bound-stop canary key set. Registering a producer or sink MUST
  invalidate the old attestation; missing, stale, mismatched, or unproved coverage
  MUST start no producer and publish no evidence.
  Canonical executable identity or version additionally requires exact
  executable binding; a presence-only result MUST explicitly assert that no
  executable identity was emitted. Generic structural sanitization cannot turn
  an arbitrary value into evidence. Direct value facts and caller-defined
  resolvers remain forbidden. Presence of an approved name is valid evidence. A
  Caller-Supplied Overlay Value intentionally provided for command, PTY, or
  session execution is not probe evidence and MAY travel only through its
  existing private execution and restoration path; it MUST never be returned by
  the probe, copied from inherited environment state, admitted as evidence, or
  presented to an evidence decoder. Its opaque transport path MUST be structurally
  separate from probe facts. Every direct command/PTY/session or snapshot-store
  overlay transport MUST first consume both an immutable caller-ingress provenance
  receipt and the exact canonical
  surface-name application receipt, bound to the same operation, name identity,
  opaque material handle, origin action, target/boot/workspace, and policy
  revision. Inherited/legacy, mismatched, or replayed provenance MUST reject;
  caller assertion or unproved provenance MUST authorize nothing. Every transport
  of restored material MUST instead consume an exact persisted chain binding the
  original ingress receipt, successful store receipt, snapshot and retained-handle
  identity, and current restore authorization; a fresh ingress or store receipt
  MUST NOT substitute for that chain. Every transport or restore MUST also consume an exact bound authorization receipt for the
  existing command, PTY, session, or snapshot path. Crossing a connector additionally requires authenticated
  origin-to-final end-to-end confidentiality, exact original-sender/final-target
  route binding, and a fresh opaque material/message binding; forwarders may see
  ciphertext and routing metadata only. Individually confidential hop-by-hop
  links MUST NOT be treated as end-to-end confidentiality. Raw values and stable
  public value digests MUST NOT appear in receipts.
- **FR-054**: The normal probe MUST check only goal-relevant or caller-requested
  environment names; a full names-only census MUST require an explicit request
  bound to that campaign. A standing capability or goal plan MUST NOT imply it.
- **FR-055**: Names-only census results MUST have independent item, per-name,
  and total-byte bounds and MUST mark incomplete output truthfully. Per-name
  native bytes are POSIX raw bytes or the checked product of two times the
  Windows UTF-16 code-unit count; total bytes are measured after wire tagging and
  base64 expansion. Code-point counts or unscaled UTF-16-unit counts are invalid.
- **FR-056**: Target boundaries MUST reduce environment observations to names or
  presence states at the source; raw `NAME=VALUE` observations MUST NOT cross a
  process or transport boundary for later stripping. This does not authorize a
  value-reading fallback when a target lacks a names-only sensor.
- **FR-057**: Existing public session and workspace status MUST add a versioned
  names-only contract. Legacy value-pair fields MUST be deprecated and empty,
  with migration guidance and contract fixtures. Existing terminal identity
  fields MUST stop reading environment values and instead use adapter context or
  platform-native non-secret evidence. These are enumerated 0.x security
  corrections permitted by FR-077, not silent wire-shape changes.
- **FR-058**: Overlay-value persistence MUST default off, with exact source/default
  proof for explicit operator on/off and for absent or unset configuration before
  any overlay name, value, or path consultation. A private workspace
  snapshot MAY retain a value only when operator policy explicitly allowlists
  that canonical overlay name as restorable and the immutable provenance proves
  it was caller-supplied. Caller assertion and secret-shape heuristics MUST NOT
  authorize persistence. Policy-off, not-allowlisted, merely caller-asserted,
  unclassified or unresolved, and whole-name-bound inputs MUST be omitted before
  value lookup with a names-only/non-reconstructive report. Inherited values and
  legacy/redaction-marker provenance, plus provenance mismatch or replay, MUST be
  rejected rather than normalized into omission; they MUST never be stored,
  exposed, or restored. Every intentional value-bearing private store
  MUST be inventoried and covered by canary tests. Store and restore are distinct
  operations: each MUST carry its own exact snapshot-operation authorization and
  receipt, and neither receipt may substitute for the other even when snapshot,
  target, name, and opaque material identity otherwise match.
  Upgrade/startup MUST run a versioned engine/store-owned migration over the
  authoritative inventory of every intentional value-bearing private store before
  any snapshot restore/list/status or environment surface becomes available.
  Before classification it MUST make the whole store inaccessible. Using only
  the frozen current schema and operator persistence/allowlist policy plus
  canonical-name and immutable provenance metadata, it MUST prove a complete,
  disjoint record-and-index partition. A current private-value record MAY be
  preserved byte-untouched only when its canonical name is currently allowlisted,
  its schema is current, its immutable provenance is exactly caller-supplied, and
  its complete current index set is proved. Every legacy raw-value,
  literal-redaction-marker, stale-policy, malformed, or unproved-provenance record
  and only its indexes MUST be atomically deleted. A mixed store MUST preserve the
  verified opaque current partition and delete the legacy/unproved partition in
  the same commit, without opening, exporting, comparing, hashing, decoding, or
  logging value material. An already migrated store MUST verify idempotently.
  Preserved records remain inaccessible until exact all-inventory success releases
  the migration gate, and ordinary operation-matched restore authorization still
  applies afterward. Commit failure or crash MUST atomically preserve the exact
  verified-current opaque record-identity and index sets. Commit failure,
  crash/restart, unavailable/unknown store state, or incomplete coverage MUST keep
  the store inaccessible and quarantine the whole store under engine/store
  authority. Automatic recovery MUST persist a frozen finite attempt budget,
  strictly decrease it once per failed authorized attempt, use one crash-safe
  deduplicated signal-driven monotonic not-before/deadline wake, and preserve the
  attempt identity and remaining budget across restart. It MUST NOT poll, reset
  the budget, or schedule an unbounded chain. Exact same-clock continuity MAY
  resume the persisted remaining window; boot/clock change without exact
  continuity MUST terminalize for operator repair rather than re-anchor a fresh
  window. Exhausted budget or total recovery
  deadline MUST enter terminal `quarantined_requires_operator_repair`, clear every
  automatic wake, and require a distinct `OverlayMigrationRepair` storage-
  maintenance action with exact pre-existing `operator_store_admin` authority and
  a committed pre-action audit to start a new migration generation. LLMs, callers,
  goal plans, sensor authority, and the migration itself MUST NOT grant or invoke
  that authority. No quarantine state may re-enable restore.
  Only exact
  all-inventory success may mark the migration current. Audit/receipts may carry
  store/schema/policy identifiers, typed result classes, and independently bounded counts
  only--never environment names, values, literal secret-shaped data, or
  value-derived hashes.
- **FR-059**: Sensor actions MUST use the normative policy taxonomy in this spec.
  Passive observation, targeted names, full census, fixed-helper execution,
  route sentinel execution, connector use, identity challenge, and target wake
  or start MUST receive distinct decisions; remote or target-native forms also
  require their target-node decisions.
- **FR-060**: A goal plan or connector MUST NOT grant or widen authority. Every
  boundary crossing requires the originating node's connector decision; every
  forwarded hop requires its forwarding node's decision; every target-native
  sensor action requires the target node's sensor-class plus underlying-action
  decisions, while a non-sensor operation requires its exact target-node
  underlying-action decision with sensor class `not_applicable`. Authority is never inherited transitively. The engine MUST retain a
  complete bounded per-node authority record set and require exact action,
  sensor-class, policy-revision, ordered authority-chain/connector-route digest,
  target/boot/workspace, and exactly one applicable correlation scope before any
  downstream receipt is trusted. Probe work binds campaign/branch and applicable
  sensor; private resolution additionally binds its invocation; pre-admission
  responses bind request peer/action/request-key attempt; command/job, PTY,
  session, snapshot-operation, retention-partition, and retention-store work bind
  their own exact scopes and mark campaign/branch/sensor fields `not_applicable`.
  No path may fabricate a campaign to satisfy shape. The digest MUST cover every forwarder and edge in canonical
  hop order; an omitted, swapped, or changed hop is a mismatch.
- **FR-061**: Remote use MUST require local authorization and independent target
  authorization.
- **FR-062**: Every cross-boundary connector MUST bind a fresh nonce and the exact
  applicable run or operation scope to persistent instance identity, current engine and target boot identity,
  protocol version, endpoint, route, and connector context using an
  operator-pinned trust root or authenticated transport identity. Replay,
  unapproved rotation, cloning, mismatch, and mid-run replacement MUST be
  distinct hard exclusions. The resulting channel MUST provide authenticated
  integrity, or every message MUST carry equivalent authentication bound to
  the exact applicable scope: probe campaign/branch/sensor; private resolver
  invocation; pre-admission request; command/job; PTY; session; snapshot operation;
  or retention partition/store. Every form also binds exact action digest where
  applicable, workspace/boot identity, policy revision and verdict, sequence,
  expiry, and local audit receipt, while inapplicable scope fields remain explicit
  `not_applicable`. Freshness MUST be independently proved at every authenticated
  edge/verifier from that verifier's own engine boot and approved suspend-inclusive
  monotonic clock, binding the ordered edge identity, verifier-issued nonce/channel
  epoch, message sequence, local challenge/epoch issue and receive anchors, and
  frozen maximum age. A complete bounded record set with exact ordered-route
  count/root equality is required; missing, extra, duplicate, swapped, or
  mismatched edge records MUST NOT authorize. Sender or remote wall time is
  provenance only. Only exact within-age proof at every edge may authorize; expiry requires fresh
  attestation, while clock loss, rollback, discontinuity, restart without proved
  continuity, mismatch, or unproved freshness MUST fail closed and establish a new
  verifier-local epoch before retry. The channel identity/binding proof MUST be a closed exact,
  missing, typed-mismatch, replay/expiry, or unproved result covering nonce,
  persistent instance and boot, protocol, endpoint/route/connector context, and
  pin/authenticated-identity basis; only exact may authorize. Channel-establishment
  and per-message proof sources MUST remain distinct. Replay and expiry MUST also
  remain distinct across channel establishment, per-message binding, receipt
  correlation, channel security, and endpoint binding. Every receipt-emitting
  causal negative/effect result on those axes MUST bind its exact causal source.
  Unknown/no-receipt results and structural/compatibility rejections use
  `zero + not_applicable` unless another covered source exists. Multiple negative
  projections may coexist only under an exact same-incident/source-correlation
  proof with the same mapped disposition. Every receipt-emitting private-input stop effect request
  MUST participate in the same causal accounting. The normative checker MUST
  represent this as finite pre-decision effect-source cardinality and multi-source
  correlation inputs backed by immutable source receipts and a shared non-secret
  incident digest. The full channel tuple MUST use one ordered total combination
  function: reject structural shape contradictions first; then preserve the one
  exact causal disposition (or reject conflicting/independent causal sources);
  then combine non-receipt negatives with `rejected` stricter than `unknown`; and
  only then accept an all-positive tuple. A non-receipt unknown/rejection MUST
  never erase, downgrade, or remap an exact replay, expiry, audit-integrity,
  identity, or engine-loss receipt and its Transition. Multiple sources are valid only as either exact projections of
  one primary causal source or distinct complete members of the exact same-incident
  `bound_reached` Transition bundle. When no covered source exists,
  `zero + not_applicable` is mandatory; a covered receipt-emitting causal
  negative/effect paired with zero, independent multiple sources, incomplete
  bundles, missing/mismatched correlation, or conflicting mappings MUST reject.
  Connector-bound replay remains a hard
  exclusion; same-process non-audit receipt replay rejects only its receipt/fact and
  MUST NOT fabricate a connector exclusion. Non-replay expiry rejects the current
  proof/message and requires bounded fresh re-attestation; missing required
  per-message fields and sequence replay map to typed audit mismatch and replay,
  respectively. A Health
  response or point challenge alone is reachability/identity evidence, not
  subsequent action integrity. An explicitly approved pre-execution connector or
  target restart invalidates affected evidence and returns an affected probe branch
  to policy gates for re-attestation; a non-campaign operation aborts in its own
  scope and may be retried only after fresh authorization. Replay, unapproved rotation, cloning, binding
  mismatch, mid-run replacement, and graph-cycle detection instead terminate the
  affected probe branch as distinct hard exclusions; for a non-campaign operation,
  the same conditions abort or reject that exact operation scope without inventing
  a branch. None may be normalized into a recoverable restart.
- **FR-063**: Operator pins MUST live in an operator-only config or trust store;
  add, revoke, and rotate operations MUST be explicit and audited, and LLM calls
  MUST NOT perform trust-on-first-use. Every executable or channel pin use MUST
  carry finite origin/management proof that the pin is a current pre-existing
  operator pin or the result of an exact audited operator add/rotate. Revoked,
  LLM/caller-supplied, trust-on-first-use, and mismatched pins MUST deny;
  unproved origin or management state MUST remain unknown and authorize nothing.
  Same-host, embedded, remote-daemon, VM,
  and Firecracker/vsock connectors MUST declare their identity basis. AAP guest
  identity MUST bind room/VM identity, guest boot epoch, guest-agent identity and
  version, and vsock CID; CID alone is insufficient.
- **FR-064**: The response MUST carry a run-level provenance envelope containing
  plan and sensor revisions, topology identities, route metadata, policy
  authorities, and run timing. Each fact MUST carry source class, observation
  time, completeness, freshness, evidence grade, and bounded references or
  deltas sufficient to reconstruct full provenance. Inapplicable fields MUST be
  `not_applicable`, never invented or repeated inline merely to satisfy shape.
- **FR-065**: Every node MUST audit the sensor and connector actions it authorizes
  and executes. Each authorizing or executing node MUST durably append the exact
  action-digest, policy-revision, identity-bound audit admission before the action
  begins; failure, unavailability, mismatch, or replay MUST fail closed. A sensor
  admission MUST NOT substitute for a later spawn admission. Audit mismatch and
  replay MUST emit distinct typed, non-authorizing integrity-failure receipts and
  follow the hard-exclusion path; they MUST NOT be reported or evidenced as
  policy denials. The campaign-owning
  engine MUST additionally append a terminal campaign summary covering goal,
  frozen plan identities, topology, reported per-node policy summaries, counts,
  durations, and verdicts without reasserting another node's authority and
  without environment names, values, raw output, or secret-shaped request data.
  Every forwarded summary MUST be authenticated and reference the executing
  node's bound local audit receipt; unauthenticated summaries are `unknown`.
- **FR-066**: Fixed-helper output MUST remain in the constitution-authorized
  private bounded typed decoder and MUST never enter normal command output,
  rings, buckets, tails, context, logs, audit, snapshots, or persistence. The
  forbidden-sink set MUST come from the authoritative, versioned shared inventory
  covering every registered output, buffering, diagnostic, context, audit, and
  persistence sink. Registering a new sink MUST automatically add it to the
  mandatory private-tainted producer success/failure/bound-stop canary coverage;
  a missing, stale, mismatched, or unproved build/runtime coverage attestation
  MUST fail closed before helper execution and MUST NOT permit decoder evidence
  to publish. Only values accepted by a closed sensor-specific grammar
  and non-content diagnostic
  codes/counts may leave it. Unparseable or free-form stdout/stderr MUST NOT be
  re-labelled as typed evidence, and raw helper, connector, OS, library, or
  remote strings MUST remain private even when a caller labels them sanitized.
  Raw decoder input MUST be counted as transport bytes monotonically under its
  frozen byte cap while read and before decoding, transcoding, unbounded buffering,
  or parsing. Reaching the cap MUST terminate the owned helper, discard all raw
  input, and return only a typed non-content failure; exceeding it is an invariant
  failure. The cap stop MUST emit an exact non-authorizing effect receipt over the
  owned resource set, followed by an independently exact cleanup receipt; the
  effect receipt MUST NOT self-assert cleanup. Typed conversion and structural sanitization MUST carry an
  engine-derived exact correlation receipt.

#### Lifecycle, freshness, and bounded signals

- **FR-067**: The daemon-library engine MUST own the planner, scheduler, evidence
  graph, run registry, lifecycle, and cleanup. A run MUST have a stable
  identifier, count as daemon live work, survive client transport interruption,
  participate in idle-reaper/shutdown handling, and report engine-boot loss
  truthfully. Every causal branch, proof, Route, Goal, campaign, cleanup, and
  operation-trace mutation MUST commit as one atomic transition batch. Partial
  members, incomplete descendant coverage, or an uncorrelated lifecycle receipt
  MUST be rejected. Admission of an ordinary trusted sensor fact MUST atomically
  commit its exact fact-admission receipt, source fact, transitive derived-fact
  recomputation, Goal/frontier recomputation, and any resulting branch-state
  advance; no public fact may exist between those members. Every ordinary sensor
  invocation MUST also atomically commit exactly one typed terminal result,
  producer cleanup or a proved-empty owned-resource set, declared fallback/frontier
  effects, and Goal recomputation, whether or not it produced a trusted fact.
  Soft/informational bound observations MUST use the acyclic order private bound
  observation -> conditionally authorizing Security receipt -> private/inert
  Transition staging and cleanup -> final public boundary receipt -> atomic
  Transition publication. Pending facts, derived facts, Goal/frontier, and branch
  effects MUST be invisible to schedulers, Route/Goal classification, Operation,
  and public readers until publication; crash recovery MUST finalize or retire
  them without re-admitting the source. No intermediate is standalone public fact
  authority.
- **FR-068**: Start MAY include a stable caller request key. A successfully bound
  key MUST carry a persisted minimum resolution lease, and every authorized
  matching-key response MUST return the same run plus the exact remaining
  idempotency horizon without silently extending it. The horizon requires the
  persisted owner boot/clock or an exact same-revision durable re-anchor under the
  current owner clock; wall time and unproved cross-boot continuity MUST NOT be
  used. Each alias has its own lease;
  purge eligibility MUST prove complete alias coverage and the elapsed maximum of
  all alias leases and the tombstone minimum. Count/byte pressure MUST NOT evict a
  protected binding; when protected state consumes capacity, new admission or
  alias binding MUST return precommit `rejected_bound`. Every start without a key
  MUST be labelled unsafe for blind retry without inferring whether a prior
  response was lost. New-key binding, its lease, all capacity reservations, and
  campaign admission MUST be one durable commit; commit failure leaves all
  unchanged and starts nothing. Campaign-id lookup, key forward/reverse lookup,
  tombstone origin/horizon, capacity checks, admission, and purge MUST linearize
  on one serializable store revision. Before purge, an expired campaign with a
  matching key remains the same run; after purge an explicit old id reports
  `not_found_or_retention_elapsed` and an id-free old key is new. Hybrid
  cross-revision projections MUST be rejected. A completed campaign whose retained
  result expired or is unavailable MUST report `result_unavailable`, not engine
  loss. Resume and detail reads MUST be idempotent. Resume, detail, and cancel MUST
  reauthorize the current peer, identity, and effective policy before attaching or
  mutating the campaign; denied or changed authority leaves campaign state
  unchanged. Cancellation MUST be one atomic Transition. Proved non-commit leaves
  state unchanged; if cancellation committed but its response/trace projection is
  missing or corrupt, Operation MUST report a postcommit integrity error while
  preserving the commit-proved cancelled state, never falsely claim non-mutation.
  A start that resolves to an already completed, cancelled, failed, or
  expired campaign MUST perform no wait; contradictory wait or lifecycle evidence
  MUST be rejected rather than ignored.
- **FR-069**: External sensor work MAY be shared only when peer/tenant scope,
  normalized goal, canonical workspace identity, target and boot identities,
  topology and policy revisions, plan and sensor revisions, requested detail,
  freshness, and bounds are identical. Admission and attachment MUST be
  serialized, every reused fact MUST satisfy the joining caller's freshness and
  authority, and evidence MUST NOT cross peer or policy boundaries.
- **FR-070**: Each fact MUST expose its age, freshness rule, cache state, and
  invalidation evidence. Freshness and ranking MUST use a campaign-owner,
  boot-bound monotonic observation interval from dispatch through receive/commit;
  remote, guest, or target wall timestamps are provenance only. At evaluation cut
  `T`, an observation spanning dispatch lower bound through receive upper bound has
  age interval `[T - receive_upper, T - dispatch_lower]`: it is fresh only when the
  worst-case age is within the frozen maximum, stale only when the best-case age is
  past it, and a straddling interval requires fresh evidence rather than a guessed
  verdict. A route's required-fact interval is the associative conservative-oldest
  envelope `[min_i(lower_i), min_i(upper_i)]`; internal overlap does not make
  evidence coverage partial. Same-boot cache reuse MUST prove clock continuity.
  Cross-boot freshness requires independently authenticated bounded continuity
  between the old and new clock domains; lease re-anchor or operator approval alone
  does not prove observation freshness. Unproved continuity MUST become stale or unknown.
  Overlapping incomparable intervals MUST NOT be fabricated as newer, equal, or
  older. Stale evidence MUST never be silently labelled fresh.
- **FR-071**: Callers MUST be able to require fresh evidence when correctness
  outweighs reuse.
- **FR-072**: Independent scope-bound limits MUST cover topology nodes, edges,
  sensors, requirements, raw and unique names, per-name native bytes, serialized
  census bytes, opaque resolver input bytes, resolver candidates, decoder input
  bytes, helper processes, per-sensor time, total campaign time, concurrency,
  result detail, response bytes/tokens, queued and active campaigns, request-key
  aliases, run-registry and key-index records/serialized bytes, retained terminal
  artifacts, tombstones, retention partitions, retained records, and daemon-wide
  retained bytes. Request, campaign, branch, private-resolver invocation,
  immutable admission-retention-partition, and daemon retention-store scopes MUST
  remain distinct and MUST NOT borrow counters or authority. New campaign
  admission MUST reserve queued--not active--capacity, a possible new partition,
  and the maximum serialized footprint of every later lifecycle state; keyed
  admission additionally reserves alias/index capacity. A later alias MUST reserve
  its alias/index deltas plus the exact maximum current-to-terminal/tombstone
  run-registry, retained-byte, and key-proof delta; it MUST NOT borrow unused prior
  headroom. `campaign_start` MUST
  separately CAS-gate active capacity. Terminalization MUST be non-increasing
  inside the original reservation and MUST NOT fail for storage capacity. Purge
  MUST release no partition slot unless an exact same-revision final-record/refcount
  witness proves the partition becomes empty; that final purge releases exactly one.
- **FR-073**: Every reached bound MUST use its closed role-typed outcome and MUST
  never be silently omitted or exceeded. Sensing/evidence bounds produce explicit
  incomplete or truncated state; admission bounds produce precommit
  `rejected_bound` with no mutation; queue bounds produce durable signal-driven
  in-progress state; output/detail bounds produce bounded continuation; snapshot
  name bounds omit the whole name before value/allowlist lookup; command, PTY, or
  session name bounds reject before execution; retention bounds produce exact
  expiry, purge, or no-mutation saturated maintenance receipts. A per-sensor timeout that affects
  only soft or informational evidence MUST stop that sensor and preserve the
  hard-safe route/Goal path; it MUST NOT masquerade as a hard branch exclusion. A
  campaign-time bound reached before any route exists MUST produce exact
  `bound_before_route` evidence and Goal `unknown`, never synthetic Route
  `unsupported`. Sensor and campaign deadlines MUST use the campaign owner's
  boot-bound, suspend-inclusive monotonic clock with exact anchor, sampled cut,
  and competing lifecycle commit order. One hierarchical campaign scheduler cut
  MUST resolve simultaneous scopes in fixed order: campaign total-time, branch
  decisive, private/decoder stop, then soft/informational effects; losing receipts
  MUST be subsumed atomically with exactly one sensor result and cleanup ownership.
  Every queue outcome MUST first commit one scheduler-owned, startup-recoverable
  wait record keyed to the reached capacity revision and campaign deadline. Capacity
  release/store revision or deadline wakes it exactly once; terminalization cancels
  the complete owned wait set, and expiry deletes retained wait/wake indexes.
- **FR-074**: The first response MUST contain only selected signals, blockers,
  bounded alternatives, and a receipt. Additional evidence MAY be exposed through
  bounded continuation only for a successfully admitted campaign or a currently
  authorized attachment to one. Pre-admission conflict, denial, unavailability,
  bound rejection, and commit failure MUST use complete fixed-shape typed receipts whose mandatory
  fields fit the frozen response cap; they MUST NOT issue a continuation cursor.
- **FR-075**: Completed, failed, timed-out, and abandoned runs MUST clean all owned
  processes, handles, temporary files, output stores, and live registrations.
  Completion MUST atomically retire every current beachhead proof; retained results
  are historical and MUST NOT authorize later dispatch. Retention MUST be a
  bounded two-stage lifecycle: expiry deletes every heavy artifact into a fixed
  tombstone while retaining the complete bounded key/lease proof set; purge may
  delete that tombstone, every forward/reverse key index, and the partition slot
  only after exact max-lease/tombstone eligibility. Every transition into terminal
  retained state MUST atomically construct a boot-bound monotonic minimum-retention
  eligibility lease; expiry requires it proved elapsed. Unproved boot/clock continuity
  for any lease-bearing component--including keyless terminal state--MUST select a
  separate no-lifecycle-change, no-deletion lease-reanchor transition under the current
  campaign-owner monotonic clock before matching-key response, expiry, or purge.
  Final-partition purge MUST fold its terminal
  digest into one bounded daemon-global audit accumulator and delete all
  per-partition audit/metadata. When every remaining record is protected,
  maintenance MUST emit a durable no-mutation saturated scheduler receipt, map
  every reached retention dimension to it, schedule one bounded wake
  on the earliest eligibility or store-revision signal, reject new reservations,
  and stop rather than poll forever.
- **FR-076**: Branch and campaign views MUST distinguish nonterminal
  `in_progress` from terminal evidence. Terminal `RouteModel` outcomes are
  exactly `ready`, `ready_with_warnings`, `blocked`, `unknown`, `denied`,
  `unreachable`, and `unsupported`; `in_progress` is not a fabricated terminal
  route verdict. The overall Goal result MUST additionally distinguish
  `in_progress` according to the exhaustive entailment table.

#### Compatibility and integration

- **FR-077**: Existing route-discovery actions and response shape MUST remain
  available for callers that do not use goal-directed probes, but their sensing
  MUST converge on the same policy-before-observation engine. Evidence grades
  MAY truthfully downgrade when policy denies a former pre-policy sentinel, and
  the enumerated names-only/value-removal migrations in FR-057/FR-058 are
  authorized 0.x security corrections with versioned guidance and fixtures.
  Legacy target listing MUST separate registry enumeration from liveness sensing:
  listing alone performs no implicit dial, and each requested reachability or
  health observation in `target_list` or `target_probe` requires the connector
  sensor decision, connector capability, underlying remote authority, and durable
  pre-action audit before any forwarded-socket contact. Denied or unobserved
  reachability MUST remain typed as such rather than `unreachable`; a health reply
  MUST NOT be treated as authenticated target identity or beachhead proof.
- **FR-078**: Compact and full LLM surfaces MUST execute the same probe semantics
  and return equivalent evidence.
- **FR-079**: Older engines that do not support environment probes MUST be
  detected before dispatch and MUST return actionable compatibility guidance.
- **FR-080**: In-process consumers MUST call the same engine-owned policy,
  planning, sensing, audit, freshness, lifecycle, and result operation as MCP
  consumers; delivery adapters only translate and enrich typed context.
- **FR-081**: The feature MUST deliberately introduce and document a narrow,
  semver-stable environment-probe and connector facade without declaring all
  public daemon internals stable. Embedded connectors MUST declare capabilities,
  identity basis, and evidence sources through that facade and MUST NOT bypass
  the policy or audit engine governing their actions. TC MUST provide a connector
  conformance kit; AAP MUST run the Firecracker row against a pinned compatible
  TC release in its own integration gate.
- **FR-082**: The feature MUST maintain the closed finite symbolic state domain
  in [scenario-matrix.md](scenario-matrix.md). Every possible assignment across
  that domain MUST be classified by exhaustive symbolic/partitioned evaluation;
  structural impossibilities MUST remain explicit `unsupported` constraint
  classes rather than being omitted.
- **FR-083**: Totality MUST be proved at the correct model boundary. Every
  `RouteModel` row MUST have exactly one terminal route outcome; every
  `GoalModel` row MUST have exactly one goal outcome; every `TransitionModel`
  row MUST have an accepted or rejected transition; every `OperationModel` row
  MUST have one admission/attachment/retry/lifecycle result and an optional goal
  result only when the campaign completed; and every `SecurityPropertyModel` row
  MUST derive exactly one applicable sensor, private-environment-resolution,
  spawn, decoder-admission, fact-admission, non-campaign surface-name application,
  private-overlay-transport, overlay-storage, or diagnostic-egress decision plus
  its receipt effect while all non-selected security outputs remain
  `not_applicable`; and every `EvidenceBoundaryModel`
  row MUST derive exactly one native-name admission and one bounded-evidence
  disposition while preserving platform equality without lossy normalization.
  Unclassified
  fallthrough or borrowing a goal outcome to classify an operation is a failure.
- **FR-084**: Finite protocol and state combinations MUST receive exhaustive
  model/fixture coverage. Every supported harness delivery, operating-system and
  connector boundary pair, and declared critical nested topology MUST receive
  authoritative live conformance evidence; live sampling MUST NOT replace the
  exhaustive logical state gate. The minimum live universe MUST be generated
  from shipped support/capability registries and the mandatory families in
  [scenario-matrix.md](scenario-matrix.md), with stable required-row keys and a
  failing missing-key report; a test-local list MUST NOT redefine or shrink it.
- **FR-085**: Representative Codex, Claude Code, Cursor, generic MCP, and AAP
  interactions MUST prove that a valid first probe call does not require prior
  knowledge of internal sensor or plan identities.

#### Normative campaign, route, and evidence contracts

- **FR-086**: Every branch MUST follow the branch-state table below. Newly proved
  connectors MAY create children only in the next wave. Equivalent facts MAY
  converge only after target and workspace identity agree, and conflicting facts
  MUST remain explicit until the evidence-resolution rule resolves them. Every
  ancestor invalidation or cancellation MUST atomically cover the complete
  finite discovered-descendant set; no independently visible branch/proof/Route
  half is valid.
- **FR-087**: A campaign is terminal only when every in-scope frontier is
  terminal or proof establishes that no open branch can change safety,
  executability, or final selection. A still-open rank-changing branch therefore
  remains `in_progress`. When such work reaches a declared bound it becomes a
  terminal truncated branch; the campaign MAY then return
  `ready_with_warnings` with explicitly incomplete alternative optimality. Soft
  gradients MUST NOT silently exclude a branch, and bounded termination MUST
  never claim exhaustive discovery or global optimality. Campaign completion
  MUST carry an exhaustive terminal Goal witness, exact owned-branch coverage,
  complete cleanup, durable result/audit commit, and a headless scheduler receipt;
  it MUST NOT depend on a client remaining connected.
- **FR-088**: Evidence conflicts MUST resolve by source grade and then freshness.
  A superseded source fact MUST transitively invalidate derived facts, cancel or
  re-gate descendants, and unlock safe re-observation. Equal-grade fresh
  conflicts MUST yield `unknown`; the final map MUST NOT retain contradictory
  live facts for one attribute of one identity.
- **FR-089**: Immediately before dispatch, every sensor MUST revalidate its hard
  prerequisites, workspace identity, policy decisions, connector chain, and
  target boot identity. A change MUST invalidate affected facts and descendants,
  force re-attestation, and record the typed replacement or invalidation reason.
- **FR-090**: A route MAY be `ready` only when one concrete next-action request
  from the selected goal plan, including action schema, exact action digest,
  execution lane, target, canonical cwd, workspace identity digest, fixed argv
  elements, and constrained substitutions, passes the same request-schema and
  policy validators used at real dispatch. The beachhead MUST carry a bounded
  proof envelope binding the engine boot identity, policy revision and digest,
  connector and target boot identities, workspace identity digest, schema,
  catalogue trust/security revision, frozen admitted plan snapshot and digest,
  exact action and permitted substitutions, issuance time, and expiry. The
  envelope is evidence, never authority.
- **FR-091**: Every reached topology node MUST own an Environment Context that
  distinguishes harness, adapter, engine, session, runner, shell, and guest
  roles and records source, names or targeted presence only, target comparison
  semantics, observation time, completeness, and freshness. Contexts MUST NOT be
  merged across node, process, workspace, or boot identity.
- **FR-092**: A run MUST freeze immutable goal-plan versions and digests at
  admission. Activation MUST be atomic, operator-local precedence over built-ins
  MUST require an explicit plan identity rather than name shadowing, and changes
  to the registry activation head MUST affect only future runs. An admitted run
  MUST continue from its frozen snapshot even after the activation head changes;
  it MUST never rebind to a newer plan. Loss of the frozen snapshot or a digest
  mismatch is a terminal campaign-integrity failure, not recoverable cache
  invalidation.
- **FR-093**: Sensor children MUST NOT inherit the daemon's full environment.
  Each adapter MUST construct a documented least-privilege environment and must
  not transport raw environment material across WSL, remote, container, or guest
  boundaries. If target-native names-only reduction is unavailable, evidence is
  `unknown` or `unsupported`.
- **FR-094**: Peers lacking the required attestation protocol MUST return
  `identity_unverified` with `peer_engine_too_old`, required-version evidence,
  and corrective guidance; compatibility fallback MUST NOT upgrade reachability
  into identity proof.
- **FR-095**: Manual onboarding baselines for every goal family and harness class
  MUST be committed from real transcripts before implementation begins, with
  fixed call-count and tokenizer measurement rules. They MUST NOT be rewritten
  merely to make the finished implementation pass SC-003.
- **FR-096**: Before implementation, the project MUST commit a machine-readable
  form of the normative matrix domains, derived functions, constraints, and
  transitions. The six independently total `RouteModel`, `GoalModel`,
  `TransitionModel`, `OperationModel`, `SecurityPropertyModel`, and
  `EvidenceBoundaryModel` layers in the
  matrix MUST be covered. An exhaustive symbolic checker MUST prove total
  classification for all assignments, and concrete fixtures MUST witness every
  domain value, decision partition, transition, failure class, supported
  boundary, and explicit unsupported constraint. Coverage reports MUST expose
  missing proof or witnesses rather than reporting only a percentage.
- **FR-097**: The engine-level connector contract MUST declare `discover`,
  `attest`, `inspect`, `execute`, `resume`, and `start_or_wake` capabilities
  independently; `reachable` alone is insufficient. Built-in bootstrap without
  external connectors MUST remain valid, while approved embed hosts MAY register
  connectors through the stable facade without duplicating policy or audit.
- **FR-098**: An environment campaign MUST use its own run identifier, registry,
  status, detail, and continuation contract rather than masquerading as a fourth
  ordinary command/file-watch/PTY probe. Internal sensor actions MAY have private
  IDs but MUST remain children of one campaign lifecycle.
- **FR-099**: Client root changes, TC session cwd changes, workspace marker or
  identity changes, policy or catalogue-trust/security revision changes,
  adapter/engine restart, explicitly approved pre-execution connector
  replacement, and expected target boot changes
  MUST invalidate every affected cached candidate, route prior, proof envelope,
  and derived fact. FR-062 security exclusions terminate the affected probe branch,
  or abort/reject an affected non-campaign operation in its own scope, instead of
  using this recoverable invalidation path. Current denied, exhausted,
  and truncated Route records MUST also be invalidated when a governing
  recoverable input changes; a hard-excluded branch remains excluded for that
  campaign and requires a new branch/campaign identity rather than rehabilitation.
  A goal-plan registry-head change is future-admission state and does not
  invalidate an admitted run; frozen-snapshot loss or digest mismatch follows the
  terminal FR-092 integrity path.
- **FR-100**: Before implementation begins, the project MUST freeze the default
  response byte cap, reference tokenizer suite and versions, fixture corpus, and
  measurement command used by SC-004. Later changes require an explicit spec
  amendment rather than benchmark-only adjustment.
- **FR-101**: Run lifecycle MUST be orthogonal to readiness. `accepted`,
  `running`, `completed`, `cancelled`, `failed`, and `expired` describe campaign
  ownership; only a completed campaign carries a terminal goal state. `accepted`
  permits no branch, Route, sensor/helper, proof, or live work until the atomic
  transition to `running`; an accepted campaign may total-time-stop only with an
  exactly empty work set. Cancelled or failed runs atomically retire any current
  Goal projection into bounded status history and return known evidence and cause,
  never a fabricated readiness conclusion. Expiry MUST delete a completed result
  into an unavailable tombstone even when it had zero Routes, and reduce
  cancelled/failed status to bounded tombstone-only history. A lease re-anchor
  MUST retain lifecycle state `expired`; only exact retention purge may transition
  `expired` to absent. After purge, explicit lookup MUST report typed not-found
  rather than fabricate `expired`, engine loss, a prior Goal, or a new campaign.
- **FR-102**: A beachhead proof is time- and revision-bound evidence, not an
  authorization token. A dispatch claiming the envelope MUST atomically and
  immediately before spawn re-run every hard validator against current action
  schema, policy revision/digest, engine boot, cwd/workspace identity, connector
  instance, target persistent and boot identities, current catalogue
  trust/security revision, integrity of the admitted frozen plan snapshot/digest,
  exact action digest, and permitted substitutions. A different registry
  activation head alone is not a mismatch for the admitted run. Expiry, mismatch,
  unavailable proof, or an
  out-of-envelope substitution MUST return typed `proof_stale`, perform no
  spawn, and enter normal re-attestation; the envelope never bypasses policy.
  Selection does not retract or immortalize a current proof: it remains
  invalidatable until terminal campaign commit, which changes every current
  proof to historical `retired`. A retired proof always requires a new campaign
  and re-attestation before later execution.
- **FR-103**: Every public, recovery, log, and audit diagnostic in a campaign
  MUST use a closed typed code plus safe-enum or independently frozen-bounded
  numeric fields. Each numeric field MUST carry exact field/cap/within proof;
  exceeded values MUST be omitted and unproved or mismatched bounds MUST emit
  nothing. Its typed-conversion proof MUST also bind an approved source-specific
  diagnostic schema/revision and enumerated non-content operational derivation.
  Source payload length, byte/character value, hash bucket, or caller-selected
  arithmetic MUST NOT be laundered into a count. Raw OS,
  library, subprocess, connector, remote-policy, parser, or rejected-request
  strings MUST remain transient private input and may produce only closed typed
  codes/enums/bounded counts or omission. Only contextual paths, endpoints, and
  rejected request fields MAY pass one shared structural sanitizer; every other
  source MUST NOT become structural text. Contextual fields MUST be omitted
  whenever safe representation cannot be proved.

#### Branch-state table

| State | Meaning | Legal next states |
|---|---|---|
| `discovered` | Candidate exists; no gate is yet proved. | `gated`, `excluded`, `cancelled`, or `truncated` only as the private terminal snapshot inside an indivisible campaign-wide bound-stop/cleanup batch |
| `gated` | Required policy, connector, identity, and evidence gates are being resolved. | `gated` on invalidation, `scouting`, `denied`, `excluded`, `exhausted`, `truncated`, `cancelled` |
| `scouting` | Cheap observations are active. | `gated` on invalidation, `attesting`, `viable`, `denied`, `excluded`, `exhausted`, `truncated`, `cancelled` |
| `attesting` | Transport, target, and workspace identity are being proved. | `gated` on invalidation, `viable`, `denied`, `excluded`, `exhausted`, `truncated`, `cancelled` |
| `viable` | Safe evidence permits deeper goal sensing. | `gated` on invalidation, `deepening`, `ready`, `denied`, `excluded`, `exhausted`, `truncated`, `cancelled` |
| `deepening` | Goal-specific sensors are active. | `gated` on invalidation, `viable`, `ready`, `denied`, `excluded`, `exhausted`, `truncated`, `cancelled` |
| `ready` | Route satisfies the route entailment table; selection is derived and does not change this state. | `gated` on recoverable invalidation, `excluded` on a hard-exclusion cause, `cancelled`, or `retracted` by an exhaustive terminal subtree/campaign cleanup batch |
| `denied` | A required policy authority denied the branch. | `gated` when that governing policy/topology input is recoverably revised, `excluded` on a later hard exclusion, or `retracted` by an exhaustive terminal subtree/campaign cleanup batch |
| `excluded` | Identity failure, workspace mismatch, cycle, or proven incompatibility hard-pruned the branch. | `retracted` |
| `exhausted` | No remaining sensor can improve the branch under the frozen evidence/plan. | `gated` on a governing recoverable revision, `excluded` on a later hard exclusion, or `retracted` by an exhaustive terminal subtree/campaign cleanup batch |
| `truncated` | A declared bound stopped an otherwise open branch. | `gated` on a governing recoverable revision, `excluded` on a later hard exclusion, or `retracted` by an exhaustive terminal subtree/campaign cleanup batch |
| `cancelled` | Caller, shutdown, ancestor invalidation, or scheduler decision cancelled owned work. Scheduler pruning requires an exhaustive proof that the branch and descendants cannot change safety, executability, or final rank. | `retracted` |
| `retracted` | Runtime resources are gone; bounded terminal evidence remains. | none |

#### Sensor policy taxonomy

All new capability flags in this table default to `false`. Enabling a class does
not bypass its existing underlying action gate. `read_only_observer` may admit
only non-executing classes; its command denial always wins.

| Sensor class | Typed policy action | Required new capability | Additional required authority | `read_only_observer` after opt-in |
|---|---|---|---|---|
| Harness-context ingest | `HarnessContextIngest` | none; authenticated delivery only | schema and peer validation | allow |
| Passive local metadata | `EnvironmentObserve` | `allow_environment_probe` | applicable `FileRead` or OS metadata decision | allow |
| Targeted environment-name presence | `EnvironmentNamesObserve` | `allow_environment_probe` | target-node passive-observe decision | allow |
| Full names-only census | `EnvironmentCensus` | `allow_environment_probe` + `allow_environment_census` | target-node census decision + explicit request bound to this campaign | allow only for a non-executing implementation with both caps and explicit request |
| Private platform resolver | `EnvironmentPrivateResolve` | `allow_environment_probe`; spawned helper also requires the universal execution layer | approved product resolver revision + same-target private confinement + zero raw retention/egress | allow only for a non-executing platform-native resolver |
| Fixed read-only helper | `EnvironmentHelperStart` | `allow_environment_probe` + `allow_environment_exec`; additionally `allow_environment_untrusted_exec` for any workspace/user-writable or unprovably writable origin | target-node sensor decision + canonical executable identity + underlying `CommandStart` | deny |
| Route sentinel | `EnvironmentSentinelStart` | `allow_environment_probe` + `allow_environment_exec` | connector decision + target `CommandStart` | deny |
| Connector discover/dial/attest | `EnvironmentConnectorUse` and `EnvironmentIdentityChallenge` | exact connector-cap mapping below | every originating/forwarding node; existing `allow_remote` where remote | deny unless the profile and all connector caps allow non-executing use |
| Target start or wake | `EnvironmentTargetStart` | `allow_environment_target_start` plus connector cap | explicit per-run operator authorization at every affected node | deny |

Any spawned implementation, including a targeted-name or census adapter, also
requires `allow_environment_exec`, underlying `CommandStart`, exact executable
identity/binding, and the full spawn receipt pipeline at every executing node.
The semantic class never waives this universal execution layer.

Connector capability resolution is closed and exact:

| Connector | Required environment capability |
|---|---|
| `local_direct` | `allow_environment_probe`; executable work separately requires `allow_environment_exec` and its underlying action decision |
| `embedded` | `allow_environment_probe` plus approved embed-host connector registration and host policy; any external connector invoked by the host is checked under its own row |
| `wsl` | `allow_environment_probe` + `allow_environment_connector_wsl` |
| `forwarded_remote` | `allow_environment_probe` + `allow_environment_connector_remote` + existing remote authority |
| `container` | `allow_environment_probe` + `allow_environment_connector_container` |
| `sandbox` | `allow_environment_probe` + `allow_environment_connector_sandbox` |
| `vm_guest` | `allow_environment_probe` + `allow_environment_connector_vm` |
| `firecracker_vsock` | `allow_environment_probe` + `allow_environment_connector_firecracker` |
| `not_applicable` | no connector-specific cap; apply the base sensor, profile, and underlying-authority rules |
| `unknown` | SecurityPropertyModel `sensor_decision = denied` and no connector action; RouteModel emits `sensor_exhausted/no_representable_candidate` and classifies the route `unsupported`, never policy-denied |

Profile resolution is also normative:

| Existing profile | Environment-probe behavior |
|---|---|
| `developer_local` | All environment classes start denied by new caps; enabled classes still obey their underlying command/file/connector gates. |
| `repo_only` | Same cap posture, with passive reads, helpers, cwd, and workspace evidence additionally confined to the verified policy root; shell/session remain denied. |
| `read_only_observer` | After probe opt-in, passive metadata and targeted names may run under existing read allow-sets; helper, sentinel, target-start, shell, and session actions remain denied. |
| `admin_debug` | Same environment cap posture as `developer_local`; existing admin authority does not bypass sensor or connector gates. |
| `full_access` | Environment caps absent from config remain false after upgrade. Only explicitly configured environment caps apply; `full_access` may broaden the underlying action verdict only after the new cap is present and enabled. An all-on convenience requires a new schema-versioned explicit operator setting. |

#### Route and goal-state entailment

`hard` requirements affect safety and executability. `soft` requirements affect
quality only. A goal plan MUST declare the class; callers cannot downgrade a
built-in hard requirement.

Terminal route outcomes are mutually exclusive and evaluated in this order:
`unsupported`, `denied`, `unreachable`, `blocked`, `unknown`,
`ready_with_warnings`, `ready`. Goal aggregation operates on the complete
route records plus the frontier effect derived from exhaustive open-branch
records. It first
selects a safe ready route, then distinguishes any still-open safety,
executability, or rank-changing frontier from terminal bounded alternative
uncertainty before evaluating `unknown`, `blocked`, `denied`, `unreachable`, and
finally `unsupported`. Bounded reason lists retain every lower-priority
contributing condition.

`READY_SAFE(route)` means the route is supported, authorized, reachable, and not
unknown; transport and required identity are verified; workspace identity is
verified; the FR-090 envelope passes the real schema and policy validators; and
every hard requirement is `satisfied` with `complete`, fresh, adequate-grade
evidence.

| State | Necessary and sufficient condition |
|---|---|
| Route `unsupported` | No approved sensor, connector, protocol, platform, or plan can represent the route or goal. |
| Route `denied` | The route is supported, but at least one required governing policy decision denies observation, crossing, or execution. |
| Route `unreachable` | The route is supported and not denied, but its required target or connector is unavailable, stopped without authorization, or unreachable after bounded authoritative attempts. |
| Route `blocked` | The route is supported, authorized, and reachable, and complete authoritative evidence proves a hard requirement missing/incompatible or proves an identity, workspace, architecture, or other hard mismatch. |
| Route `unknown` | The route is supported, not denied, not unreachable, and not definitively blocked, but a hard condition remains unproved because of ambiguity, conflict, stale evidence, failed/timed-out/truncated observation, identity uncertainty, or incomplete scope. |
| Route `ready_with_warnings` | `READY_SAFE(route)` holds and at least one soft requirement is non-satisfied or remains unknown; soft uncertainty cannot become a hard Route `unknown`. |
| Route `ready` | `READY_SAFE(route)` holds and every applicable soft requirement is satisfied or not applicable. |
| Goal `ready` | At least one Route `ready` exists, the selected route has no soft warning, no rank-changing frontier remains, and alternative-rank proof is `complete` or `proved_cannot_outrank`; merely terminal `unknown` or truncated alternatives are insufficient. |
| Goal `ready_with_warnings` | A safe Route `ready` or Route `ready_with_warnings` exists, no open branch can affect safety, executability, or selection, and the selected route has soft warnings or derived alternative-rank proof is `bounded_incomplete` because terminal unknown/truncated work leaves alternative optimality incomplete. |
| Goal `in_progress` | An open branch can still affect safety, executability, or selection even though a safe route exists, or no safe route exists and at least one in-scope branch is non-terminal. |
| Goal `unknown` | No safe ready route or open frontier remains, and at least one route is `unknown`, or exact terminal campaign evidence is `bound_before_route` with zero Routes and zero open branches. |
| Goal `blocked` | No ready, in-progress, or unknown outcome applies, and at least one reachable authorized route is `blocked`. |
| Goal `denied` | No higher-priority goal outcome applies, at least one supported route is `denied`, and every other route is denied, unreachable, or unsupported. |
| Goal `unreachable` | No higher-priority goal outcome applies, at least one supported route is `unreachable`, and every other route is unreachable or unsupported. |
| Goal `unsupported` | Terminal campaign evidence is `none`, and either no route exists after a complete representability search or every in-scope route/plan combination is explicitly `unsupported`; `bound_before_route` never satisfies this row. |

### Key Entities

- **Harness Context**: Source-labelled evidence supplied or observed at the LLM
  harness boundary, including client identity, capabilities, roots, active
  session context, transport, and isolation clues.
- **Execution Topology**: The observed graph of harness, adapter, engine, runner,
  guest, and execution nodes plus the transports and trust states connecting
  them.
- **Connector**: A policy-governed means of discovering and attesting one
  topology edge and accessing target-native sensors.
- **Environment Probe Run**: One goal-directed sensor campaign with identity,
  caller request key, peer/tenant scope, frozen policy/plan/catalogue revisions,
  lifecycle, bounds, progress states, evidence, and cleanup ownership.
- **Goal**: The caller's intended activity, such as build, test, run, develop,
  diagnose, automatic discovery, or a structured custom requirement set.
- **Goal Plan**: A versioned declarative mapping from a goal and discovered
  workspace evidence to approved sensors, gates, fallbacks, and output signals.
- **Sensor Adapter**: A trusted typed definition for one narrow class of
  environment, platform, runtime, manifest, package, identity, or connection
  fact. Each invocation is disposable and planner-owned.
- **Evidence Fact**: An observed or derived claim with value, target, source,
  dependencies, grade, completeness, freshness, and bounds metadata.
- **Environment Context**: Names-only or targeted-presence evidence for one
  exact topology node, process role, workspace, and boot identity.
- **Workspace Identity**: Canonical target root plus bounded VCS/worktree,
  manifest, lock, and toolchain evidence used to prove two path views refer to
  the same workspace.
- **Policy Authority**: The exact engine node whose current decision authorizes
  one crossing, forwarding, observation, helper, or execution action.
- **Route Gradient**: The current trust, reachability, readiness, completeness,
  freshness, and preference signals used to order candidate paths after hard
  exclusions.
- **Requirement**: A typed environment-presence, tool, runtime, toolchain,
  package, platform, or capability condition relevant to a goal.
- **Requirement Verdict**: The semantic verdict, independent observation status,
  hard/soft class, and bounded evidence for one requirement.
- **Beachhead**: The highest-ranked verified route and exact execution template
  the LLM can follow for its goal.
- **Target Identity**: Persistent identity, current boot identity, attestation
  state, connector binding, pin lifecycle, and operator trust decision for a
  same-host, remote, embedded, VM, or guest target.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In 100% of supported unambiguous workspace rows in the normative
  matrix, one initial goal-only call starts one campaign and returns either the
  expected terminal decision and beachhead/blockers or a valid `in_progress`
  receipt that reaches that decision by bounded continuation.
- **SC-002**: Static request-schema fixtures prove Codex, Claude Code, Cursor,
  generic MCP, and AAP can issue a goal-only first call without internal sensor
  identifiers; real transcript evidence is retained as non-gating usability
  evidence.
- **SC-003**: Against the pre-implementation frozen baselines required by FR-095,
  the median supported onboarding scenario uses at least 70% fewer LLM tool
  calls and 70% fewer returned tokens.
- **SC-004**: Every default response remains within 2,000 tokens under the frozen
  reference tokenizer suite and its serialized byte cap. Provenance references
  count as attribution under SC-006. Optional overflow evidence for an admitted
  and currently authorized campaign is available only by bounded continuation;
  every pre-admission terminal response is complete within the cap and has no
  continuation.
- **SC-005**: At least 95% of same-host rows whose expected result is definitive
  reach that result within 5 seconds, and at least 95% of live WSL or
  already-connected remote definitive rows reach it within 15 seconds,
  excluding declared longer sensor requirements. A fast unexpected `unknown`,
  `unreachable`, or `in_progress` does not satisfy this measure.
- **SC-006**: 100% of returned facts resolve through their local fields and
  run-level provenance references to topology node, source, evidence grade,
  observation time, completeness, freshness, and applicable identity.
- **SC-007**: The symbolic matrix checker proves total, deterministic outcome
  classification over 100% of the closed domain, while concrete coverage proves
  every required witness class. There is no absent domain value, omitted
  unsupported constraint, unhandled transition, or implicit success.
- **SC-008**: Canary verification finds zero Observed Environment Values or raw
  canary material from caller overlays, helper/connector stderr, OS/library or
  remote errors, rejected request fields, malformed plans, paths, or endpoints
  in every public, transport, embed, audit, log, error, output, context,
  snapshot-response, recovery, and cache surface. Private restoration stores
  contain only operator-allowlisted caller overlays with immutable provenance,
  never inherited or unclassified values, and never restore redaction markers.
- **SC-009**: 100% of version fixtures use the declared ecosystem semantics;
  malformed and ambiguous versions return `unknown` rather than a guessed
  compatibility verdict.
- **SC-010**: Route selection never chooses a policy-denied, identity-failed,
  incompatible, or incomplete candidate over a verified ready candidate in the
  exhaustive route-state model.
- **SC-011**: Transport interruption and reconnect tests recover the same logical
  run with zero duplicated external sensor actions and no lost completed facts.
- **SC-012**: Cleanup verification finds zero live processes, handles, temporary
  files, output stores, or runtime registrations after every run retention
  window across success and failure states.
- **SC-013**: Live Windows, Linux, macOS, WSL, remote-daemon, container/sandbox,
  and AAP Firecracker scenarios each prove target-native evidence through the
  canonical contract. TC owns the connector conformance kit; AAP owns the
  pinned-revision Firecracker integration gate.
- **SC-014**: Operators can add, validate, activate, select, and audit a local
  declarative goal plan without adding executable behavior or granting a new
  capability.
- **SC-015**: In a fixture with ten plausible first-wave branches and two viable
  convergence points, only the two survivors receive deeper target sensors, all
  ten branches retain a terminal evidence trail, every sensor instance is
  disposed, and the default response remains within its signal budget.
- **SC-016**: Exact-validator fixtures prove no beachhead is advertised when its
  real action shape, cwd, interpreter substitution, or connector policy would be
  rejected at dispatch.
- **SC-017**: Exhaustive branch-transition tests reject every undeclared state
  transition and prove recursive invalidation, cancellation, retraction, and
  cycle prevention.
- **SC-018**: Instrumented policy tests observe zero external sensor, sentinel,
  file, helper, connector, or target-start actions before all required policy
  decisions are recorded.
- **SC-019**: Exhaustive security-property fixtures prove that every absent
  environment capability denies its sensor class under every profile, including
  `full_access`, and that no user/workspace-writable, changed, or unprovable
  executable or shim is spawned without the explicit untrusted-exec capability
  and unchanged bound interpreter/script identity.
- **SC-020**: Cross-boundary fixtures accept evidence only with authenticated
  action/audit binding, reject point-challenge-only, replayed, expired, or
  partially bound messages, and require authenticated end-to-end confidentiality
  whenever a caller overlay crosses the boundary.
- **SC-021**: Operation-model fixtures prove that disconnect, retry, attachment
  denial, independent concurrent admission, cancellation, expiry, and engine
  loss preserve or terminate the correct campaign lifecycle without changing or
  fabricating its goal verdict.

## Development Review Gate

The feature MUST NOT enter implementation planning until independent adversarial
review finds no unresolved critical or high-severity contradiction among this
specification, the project constitution, current code and trust model, and the
supported embed architecture. Review findings and root-agent dispositions MUST
remain committed evidence rather than being silently overwritten.

## Dependencies

- the existing daemon-library engine, policy evaluator, audit sink, lifecycle
  supervisor, route model, IPC protocol, MCP delivery surfaces, and store;
- platform-native process, filesystem, identity, and environment-name APIs;
- operator-configured target and connector trust material;
- a narrow stable embed/connector facade and conformance kit supplied by TC; and
- AAP-owned Firecracker/vsock connector implementation and integration gate.

## Assumptions

- Harness-provided context is useful routing evidence but is not target truth
  until independently confirmed.
- Missing or ambiguous workspace context is a valid result state; correctness
  takes precedence over guessing merely to preserve a one-call response.
- External targets and transports are operator-configured or host-provided. The
  feature discovers and validates them but does not create arbitrary network
  tunnels or public listeners.
- Complete discovery means complete within the requested, reachable, and
  authorized scope. Denied or physically unreachable evidence remains explicit.
- Environment-variable names are permitted outward metadata when requested;
  observed values are never permitted. Intentional caller-supplied execution
  overlays remain private execution data, not probe evidence.
- Goal-plan extensibility composes approved sensors only. New executable sensor
  behavior requires normal product implementation and verification.
- Current route types, policy, audit, output combing, and in-process engine
  boundary remain the foundation rather than parallel replacements. Existing
  pre-policy discovery execution and value-bearing public fields are explicitly
  migrated to the stronger shared sensing contract.

## Out of Scope

- installing, upgrading, repairing, or otherwise mutating prerequisites;
- executing caller-provided commands, scripts, shell text, module imports, or
  arbitrary executable paths as environment sensors;
- retrieving, classifying, or returning environment-variable values or
  credentials, except for the target-local opaque consumption and closed typed
  reduction performed by the single FR-053 private-resolver carve-out;
- arbitrary network discovery, port scanning, tunnel creation, or sandbox
  escape;
- claiming total visibility beyond authorized and reachable boundaries;
- replacing package managers, build systems, policy, audit, route discovery, or
  normal command probes;
- treating historical route success, configured target labels, platform names,
  or harness claims as current target proof; or
- allowing goal plans or connectors to grant permissions.
