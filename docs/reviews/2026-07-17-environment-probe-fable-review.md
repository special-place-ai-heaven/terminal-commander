# Fable Adversarial Review: Goal-Directed Environment Probe

Reviewer: Claude (Fable 5), independent adversarial pass.
Date: 2026-07-17.
Scope: `specs/003-environment-probe/spec.md` (spec-only review, pre-planning).
Method: all seven required sources read completely; live flow traced through
`crates/daemon/src/environment/`, `state.rs`, `router.rs`, `ipc/`, `policy.rs`,
`shell_session.rs`, `crates/probes/`, `crates/store/`, `crates/ipc/`,
`crates/mcp/`. Code is gospel; every cited line was read in the current
working tree. This is a static review; no probe, test, or daemon was executed.

## Intent Restatement

Terminal Commander wants one goal-directed `environment_probe` trigger that an
LLM in an unknown harness can call with nothing but a goal. Internally the
engine runs a bounded, staged sensor campaign - fan out cheap disposable
scouts along plausible routes, converge on evidence-supported branches, deepen
only survivors, retract the rest - and returns one combed signal map: terminal
readiness, a verified execution beachhead, exact blockers, bounded
alternatives, provenance, freshness, completeness, and blind spots. The same
semantics must hold from Codex/Claude Code/Cursor/generic MCP and from the
in-process embed (including AAP Firecracker guests), across Windows, WSL,
Linux, macOS, and multi-hop topologies, with policy evaluated before
observation, no environment-variable values or raw helper output on any
outward surface, and fewer LLM calls, tokens, and wrong-environment failures
than manual onboarding. Capability may be large; it must earn correctness and
trust.

## Verdict: CONTESTED

The specification is a serious, mostly constitution-aligned product spec with
strong secrecy, bounds, and lifecycle requirements, and its premises about the
existing engine check out against code in almost every place I tested them.
It is not ready to plan against: it contains one internal contradiction
(FR-053 vs FR-077 vs live surfaces) that SC-015 itself classifies as a gate
failure, and six high-severity underspecifications where an implementer must
guess about exactly the things the feature exists to make trustworthy
(what "ready" entails, what proves a route, whose policy and audit govern a
multi-hop sensor, which profiles may sense at all, and what the scenario
matrix must minimally cover).

## Findings

1. **[critical] FR-053's absolute no-value rule contradicts FR-077 and three live public surfaces**
   - Lens: Skeptic (secret leaks, false compliance) + Architect (compatibility).
   - Evidence: `specs/003-environment-probe/spec.md:445-447` (FR-053: no
     environment-variable value may cross any public response), `spec.md:509-510`
     (FR-077: existing route discovery MUST remain available and
     backward-compatible); versus live code:
     `crates/daemon/src/environment/probe.rs:543-587` (`terminal_probe` /
     `bounded_env_marker` put the *values* of `TERM_PROGRAM`, `TERM`, and
     `TERM_PROGRAM_VERSION` into `TerminalProbe.name`/`version`),
     `crates/ipc/src/protocol.rs:666-677` and `:726-739` (those fields ride
     `DiscoverResponse.environment` on every `system_discover`),
     `crates/ipc/src/protocol.rs:2298-2312` and `crates/mcp/src/tools.rs:2530-2538`
     (`shell_session_status` serializes `env_snapshot: Vec<(String, String)>`
     name/value pairs over IPC and MCP), `crates/store/src/workspace.rs:29-39`
     (snapshot rows persist `(key, value)` pairs).
   - Concrete failure scenario: the SC-008 canary suite (spec.md:590-591)
     exports a canary value in `TERM_PROGRAM`, starts the daemon, and calls
     `status`/`system_discover` - which FR-077 forbids changing. The canary
     value arrives in `environment.terminal.name`. SC-008 fails while FR-077
     forbids the fix. The implementer either breaks FR-077's wire
     compatibility, silently carves an exemption FR-053 does not grant, or
     ships a probe whose flagship secrecy criterion is red from day one.
   - Why the current specification is insufficient: FR-053 is written as an
     absolute over "a public response ... snapshot response, or evidence
     cache" with no definition of "environment-variable value" and no
     treatment of the three existing surfaces that already violate it. FR-057
     migrates session/workspace status to names, but nothing addresses
     `TerminalProbe`, and FR-077 simultaneously freezes the surface that
     carries it. The companion design
     (`docs/superpowers/specs/2026-07-15-environment-trust-probes-design.md:191-193`)
     had the missing carve-out ("Derived terminal/CI facts may remain separate
     normalized fields"); the spec dropped it. SC-015 (spec.md:609-611) makes
     an unresolved contradiction of this severity a self-declared gate
     failure.
   - Exact recommended specification change: (a) define
     "environment-variable value" normatively; (b) add an explicit, closed,
     normative allow-list of derived/normalized environment-derived facts
     (terminal identity markers, CI flag) with per-field byte bounds, OR
     mandate migrating `TerminalProbe` to enumerated identities and declare
     that wire change - like the FR-057 session change - an authorized 0.x
     security correction, amending FR-077 to say "behaviorally compatible
     except the enumerated no-values corrections"; (c) enumerate the known
     noncompliant surfaces (system_discover terminal fields,
     `shell_session_status.env_snapshot`, workspace snapshot rows/apply) as
     in-scope migrations of this feature.
   - Implementation boundary affected: `crates/ipc/src/protocol.rs`
     (`TerminalProbe`, `ShellSessionStatusResponse`), `crates/daemon/src/environment/probe.rs`,
     `crates/mcp/src/tools.rs`, `crates/store/src/workspace.rs`.

2. **[high] The terminal-state entailment for `ready` is never defined, leaving false-ready implementable in good faith**
   - Lens: Skeptic (false-ready paths; brief question 7).
   - Evidence: `spec.md:503-505` (FR-076 names the six states but not their
     entailment), `spec.md:439-441` (FR-052 gives eight prerequisite
     verdicts), `spec.md:435-437` (FR-050 makes `unknown` a common verdict),
     `spec.md:341-345` (FR-015/016 give hard exclusions and a gradient for
     *selection*, not for *state*), `spec.md:560-561` (Beachhead = "verified
     route"), `spec.md:369-371` (FR-025 workspace reachability).
   - Concrete failure scenario: goal=`test`, Python plan. `python_runtime` is
     `satisfied`; `python_distribution` version comparison is unprovable so it
     is `unknown` (FR-050). Implementation A reads FR-076 as permitting
     `ready_with_warnings`; the LLM runs the suite against an interpreter
     missing the pinned pytest plugin - the exact wrong-environment failure
     the feature promises to remove, now wearing a "ready" label.
     Implementation B returns `unknown` overall; the same matrix row now has
     two defensible expected outcomes, and FR-083's "expected classified
     outcome" is unadjudicable.
   - Why the current specification is insufficient: `ready`,
     `ready_with_warnings`, `blocked`, and `unknown` are never given
     necessary-and-sufficient conditions over (prerequisite verdict multiset x
     transport state x identity state x workspace reachability x completeness).
     Every other trust promise (SC-007, SC-010, FR-083) quantifies over these
     states, so their meaning cannot be left to the planner author.
   - Exact recommended specification change: add a normative state-entailment
     table: for each route-level and overall state, the exact condition over
     hard-requirement verdicts (e.g. `ready` requires every hard requirement
     `satisfied` with target-native or fixed-helper grade, transport proven,
     identity verified or same-host structural, workspace reachability
     verified; any hard-requirement `unknown`/`truncated` forbids `ready` and
     selects `unknown` or `ready_with_warnings` per a stated rule; define
     which requirement classes may downgrade to warnings). State that the
     table is exhaustive and that planners cannot widen it.
   - Implementation boundary affected: planner/result-synthesis engine (new,
     daemon-side), matrix fixtures, both MCP surfaces.

3. **[high] "Verified beachhead" is not tied to validation of the real request that will use it (brief question 6)**
   - Lens: Skeptic + Architect (route validation vs policy validator).
   - Evidence: current code proves the divergence class is real:
     `crates/daemon/src/environment/mod.rs:33-59` (route retention evaluates
     policy against the *template* - `shell_line: "{command}"`, argv
     containing `"{args...}"` placeholders), `crates/daemon/src/policy.rs:708-713`
     (`environment_discovery_cwd` evaluates discovery at a representative
     anchor `Path::new(".")` and the doc comment concedes "Concrete command
     calls are always re-evaluated with their real cwd"),
     `crates/daemon/src/environment/probe.rs:292-319` (`wsl_argv` route with a
     `{program}` placeholder), `POLICY.md:388-435` (US8: `wsl.exe -e bash`
     is denied as a nested shell when `allow_shell=false`); spec side:
     `spec.md:306-308` (FR-004 "route validation"), `spec.md:341-345`
     (FR-015/016), `spec.md:560-561` (Beachhead entity).
   - Concrete failure scenario: probe returns `ready` with beachhead
     `wsl:default:argv`, template `wsl.exe -e {program} {args...}`
     (probe.rs:304-318). The LLM's goal-relevant substitution is
     `{program}=bash` for a build script. The US8 classifier denies it
     (`shell_interpreter_denied`, allow_shell off). The first real use of the
     "verified" beachhead is rejected by the policy validator - precisely the
     failure SC-010 and the product promise exclude - yet no FR was violated,
     because the spec never says what a route verification must prove about
     the requests that will flow through it.
   - Why the current specification is insufficient: FR-015 hard-excludes
     policy-denied routes, but "policy-denied" is undefined for a route (a
     route is a family of future requests, not one request). Nothing requires
     the readiness claim to name the validated envelope (which cwd, which
     argv shapes, which interpreter substitutions are inside the proof) or to
     validate the exact execution template against the same validator the
     real request will hit (including the nested-shell classifier and
     allow-root/containment rules that bind at instantiation time).
   - Exact recommended specification change: add an FR: a route may be
     reported `ready` only after the exact beachhead execution template,
     instantiated per its declared substitution classes, passes the same
     policy validator and request-schema validation the corresponding real
     request would pass, with the validated envelope (cwd anchor, argv class,
     interpreter constraints) recorded in the route's provenance; template
     substitutions outside the validated envelope MUST be declared as such in
     the beachhead ("valid for non-interpreter programs" etc.). Add a matrix
     axis or fixtures for template-instantiation divergence (US8 wsl carrier,
     repo_only cwd, non-empty allow_roots).
   - Implementation boundary affected: `crates/daemon/src/environment/mod.rs`
     (`apply_execution_policy` semantics), `crates/daemon/src/policy.rs`
     (evaluate + WSL classifier reuse), planner route-validation stage,
     response contract.

4. **[high] Sensor policy taxonomy and profile/cap mapping are unspecified; `read_only_observer` behavior is undefined**
   - Lens: Architect (policy-before-observation, responsibility placement).
   - Evidence: `crates/daemon/src/policy.rs:79-140` (the closed `PolicyAction`
     set has no environment-inspection action), `POLICY.md:110-126`
     (`read_only_observer`: "The agent can WATCH but not RUN"; denies
     command starts and process probes), `spec.md:459-460` (FR-059 names only
     two classes: passive census vs executable-backed inspection),
     `spec.md:461-463` (FR-060 re-evaluation), design intent at
     `docs/superpowers/specs/2026-07-15-environment-trust-probes-design.md:234-243`
     (a dedicated `PolicyAction::EnvironmentInspect`, available to a
     read-only observer).
   - Concrete failure scenario: an operator runs `read_only_observer`
     specifically because the agent must not execute anything. The LLM sends
     `environment_probe` with goal=`build`. The Rust plan's sensors spawn
     `cargo --version` and `rustc --version` helpers. If the implementation
     grants this under a new inspection action, the operator's documented
     "cannot run any new commands" contract is broken by a feature the
     profile predates. If it denies, every goal returns `denied` on that
     profile and the spec gives no expected outcome for the matrix row
     (FR-082 includes "policy state" as an axis but the mapping is
     underivable). Either way the implementer guessed.
   - Why the current specification is insufficient: FR-059's two classes are
     not a taxonomy: route-verification sentinel executions, transport
     dialing, WSL distro enumeration (`wsl --list`), remote health/identity
     challenges, and fixed version helpers all need a named policy class, and
     each existing profile and cap (`allow_shell`, `allow_session`,
     `allow_remote`) needs a stated relationship to each class. The spec
     never says which profiles admit which sensor classes, or whether new
     `[policy.caps]`-style opt-ins are required.
   - Exact recommended specification change: add a normative sensor-class /
     policy table: for each sensor class (passive local read, names census at
     a boundary, fixed-helper execution, route sentinel execution, transport
     dial, identity challenge, remote/target-native forms of each), the
     policy action that gates it, its default decision per existing profile,
     and which caps modify it. State explicitly the `read_only_observer`
     outcome for executable-backed sensing (recommended: passive and census
     classes allowed, all helper-executing classes denied with verdict
     `denied`, and the matrix carries those expected rows).
   - Implementation boundary affected: `crates/daemon/src/policy.rs`
     (new `PolicyAction` variants + profile arms), POLICY.md, audit labels,
     matrix fixtures.

5. **[high] Policy-before-observation (FR-010) collides with the pre-policy discovery the spec adopts as its foundation**
   - Lens: Skeptic + Architect (dual truth; parallel replacement by accident).
   - Evidence: `crates/daemon/src/environment/probe.rs:41-93` (discovery runs
     unconditionally in the engine), `:386-427` (`probe_shell` *executes*
     every resolvable shell with a sentinel line regardless of policy),
     `:617-624` (`wsl_probe` executes `wsl -e sh -lc "printf ..."`),
     `crates/daemon/src/state.rs:366-370` (policy filtering is applied only
     *after* observation), `crates/daemon/src/ipc/server.rs:881-898`
     (`system_discover` re-runs this on every call); spec side:
     `spec.md:326-329` (FR-010: no sensor runs until all hard prerequisites,
     including policy, are positively satisfied), `spec.md:509-510` (FR-077
     keeps the old surface), `spec.md:631-633` (Assumption: current route
     discovery "remain[s] the foundation rather than parallel replacements").
   - Concrete failure scenario: host with `allow_shell=false`. Today
     `system_discover` still executes `bash -lc` and `wsl -e sh -lc`
     sentinels and therefore holds execution-confirmed evidence about shells
     the policy forbids as a lane. The new probe, obeying FR-010 plus the
     finding-4 taxonomy, cannot run those sentinels. Now two engine surfaces
     describe the same host with different evidence grades for the same
     routes (one execution-confirmed, one policy-locked), or the probe
     silently reuses the ungated discovery result and FR-010 is violated on
     the very first wave. Either outcome is a standing contradiction between
     the probe and the surface FR-077 preserves - the "parallel replacement"
     the assumptions forbid, created by omission.
   - Why the current specification is insufficient: the spec never classifies
     the existing fixed, engine-authored sentinel executions. They are either
     (a) an engine-internal observation class exempt from sensor policy
     gating - which must then be said out loud, bounded, and applied
     identically to both surfaces - or (b) policy-gated sensors, in which
     case FR-077's surface inherits a behavior change the spec must own,
     exactly as FR-057 owns the session-status change.
   - Exact recommended specification change: add an FR classifying
     engine-origin fixed sentinel probes: either declare them a distinct,
     bounded, enumerated observation class exempt from per-sensor policy
     gates (with the closed sentinel list, byte/time bounds, and the note
     that they never execute caller-influenced text), or subject them to the
     finding-4 taxonomy and amend FR-077 with the resulting evidence-grade
     change to `system_discover`. Require that probe and legacy discovery
     share one sensing implementation and one evidence store so the two
     surfaces cannot diverge on the same host.
   - Implementation boundary affected:
     `crates/daemon/src/environment/probe.rs`, `state.rs:366-370`,
     `ipc/server.rs` system_discover path, new planner.

6. **[high] Multi-hop policy authority and audit ownership are unassigned (whose policy? whose audit row?)**
   - Lens: Architect (policy-before-observation across nodes, audit
     ownership; brief question 4/5 adjacency).
   - Evidence: the live model is per-node self-policing:
     `crates/daemon/src/ipc/server.rs:588-609` and `:713-734` (a non-local
     `EnvironmentSpec` forwards the *entire request* to the runner daemon;
     the parent's policy engine never evaluates the forwarded action),
     `crates/daemon/src/environment/router.rs:46-70` and
     `crates/daemon/src/environment/wsl.rs:20-44` (forwarding relay; the
     Windows arm is a stub returning Unavailable),
     `crates/mcp/src/tools.rs:1226-1244` (remote path: local `allow_remote`
     cap gate, then the remote daemon self-polices), `POLICY.md` section 3
     (policy is per-daemon TOML, restart-scoped); spec side: `spec.md:461-463`
     (FR-060 "reevaluated against current policy" - singular),
     `spec.md:474-476` (FR-065 audit content, no owner), `spec.md:517-518`
     (FR-081 "central policy" - singular).
   - Concrete failure scenario: Windows parent (`allow_shell=false`) probes a
     WSL runner whose own TOML has `allow_shell=true`. A goal plan wants a
     shell-class sensor inside the distro. Reading FR-060 as "the
     originating engine's current policy," the sensor is denied and the
     route is reported blocked even though the governing node authorizes it.
     Reading it as "each node's own policy," it runs - and the parent's audit
     log has no row for an execution its probe run caused, while FR-065
     requires the probe's audit to record sensor identities and policy
     decisions it never saw. Two implementers produce opposite trust
     behavior and neither violates the text.
   - Why the current specification is insufficient: with two-plus policy
     engines in a topology (parent, WSL runner, remote daemon, embedded
     engine), "current policy" and "central policy" do not identify a node.
     Audit ownership per hop, and how per-node decisions appear in returned
     provenance and in FR-065 audit rows, are unstated.
   - Exact recommended specification change: make the layered-authority model
     normative: (a) each boundary crossing requires the originating node's
     crossing capability (e.g. `allow_remote`, WSL routing) under the
     originating node's policy; (b) target-native sensor actions are
     authorized by the *target node's* policy engine; (c) every node audits
     the actions it executes, and the probe-owning engine additionally audits
     the campaign (goal, plan, per-node decision summaries as reported
     verdicts, not re-asserted authority); (d) provenance labels each fact
     with the authorizing node. State that a hop whose policy verdicts cannot
     be obtained yields `denied`/`unknown` facts, never inherited authority.
   - Implementation boundary affected: `crates/daemon/src/environment/router.rs`,
     `wsl.rs`, `crates/mcp/src/target_router.rs`, audit schema
     (`crates/store/src/audit.rs`), planner provenance model.

7. **[high] The scenario matrix is self-declared with no minimum coverage rule, so omitted combinations can silently disappear (brief question 8)**
   - Lens: Skeptic (coverage as proof) + Product critic (SC gameability).
   - Evidence: `spec.md:519-524` (FR-082 "full declared scenario matrix",
     FR-083 quantifies only over declared rows), `spec.md:587-589` (SC-007:
     "100% of declared ... rows"), `spec.md:525-527` (FR-084 exhaustive
     coverage only for "finite protocol and state combinations").
   - Concrete failure scenario: an implementation declares 40 comfortable
     rows (developer_local, unambiguous workspace, native targets). SC-007
     reads 100%. Codex-on-Windows + `repo_only` + stopped-WSL-distro +
     goal=test was never declared, ships unclassified, and returns an
     implicitly-successful state in production - the outcome FR-083 calls a
     failure, but only for rows that exist. "Full" does no normative work
     because the matrix's extent is chosen by the thing being graded.
   - Why the current specification is insufficient: taken literally, the
     cross-product of the eight axes is astronomically large, so everyone
     will sample - but the spec provides no sampling rule, so the sample is
     arbitrary and unauditable.
   - Exact recommended specification change: add normative minimum coverage:
     (a) every declared value of every axis appears in at least one row;
     (b) all pairs over a named critical axis subset (topology x policy
     state, target OS x goal family, harness x workspace source, connection
     state x topology) are covered (pairwise rule); (c) every FR-030
     isolation state and every FR-032 connection failure class appears in at
     least one expected-outcome row; (d) unsupported combinations must be
     declared as explicit `unsupported` rows, not omitted. Tie SC-007 to
     these minimums, not to bare declaration.
   - Implementation boundary affected: matrix fixture design, CI gates.

8. **[medium] The target-identity model stops one step short of implementable: no pin store, no boot-identity-change semantics, no embed/guest identity rule**
   - Lens: Architect + Skeptic.
   - Evidence: zero existing substrate: `crates/daemon/src/config.rs:604-625`
     (`RemoteTarget` carries only documentation fields and a local socket
     path), `crates/mcp/src/tools.rs:976-985` (`probe_target` = `Health` +
     version - exactly the evidence FR-062 says is insufficient); spec side:
     `spec.md:466-470` (FR-062/063), `spec.md:562-563` (Target Identity
     entity), `spec.md:379-381` (FR-029 guest identity), no FR locates the
     operator pin, defines identity lifetime across daemon replacement, or
     states how an in-process embed or vsock guest satisfies "persistent
     verifiable identity".
   - Concrete failure scenario: implementer A stores pins in `targets.toml`
     and treats first-connect as trust-on-first-use; implementer B requires
     pre-shared pins and refuses TOFU; implementer C decides the AAP
     Firecracker guest is "identity-verified by construction" while D forces
     a challenge protocol AAP's connector never implemented, so every guest
     probe reports `identity_unverified` forever. All four satisfy the text;
     they produce incompatible trust results for the same topology.
   - Why the current specification is insufficient: FR-062 mandates a
     mechanism family ("persistent verifiable identity and fresh challenge
     response") without the entities that make it decidable: pin storage and
     lifecycle, replacement/rotation semantics (`target replacement` appears
     only as a connection-failure label in FR-032), and the identity rule for
     non-remote nodes (same-host, embedded, guest-over-vsock).
   - Exact recommended specification change: extend the Target Identity
     entity with: where operator pins live and how they are added/revoked
     (operator-only, config-or-store, audited); the required outcome when
     persistent identity matches but boot identity changed (rebooted vs
     replaced); and a per-connector identity basis table (remote daemon =
     challenge against pinned identity; same-host = peer identity + boot id;
     embed = host-asserted, labelled as such; vsock guest = connector
     contract attestation, or explicitly `identity_unverified` in v1).
   - Implementation boundary affected: `crates/daemon/src/config.rs`,
     `crates/mcp/src/target_router.rs`, IPC protocol (challenge method),
     store (pin persistence), AAP connector contract.

9. **[medium] SC-004's 2,000-token budget is arithmetically incompatible with FR-064's ten provenance attributes on every returned fact**
   - Lens: Product critic (response bounds, evidence compression).
   - Evidence: `spec.md:471-473` (FR-064: every returned fact carries target
     node, route, source class, boot identity, catalogue revisions, time,
     duration, completeness, freshness, grade), `spec.md:578-580` (SC-004:
     default response within 2,000 tokens across the whole matrix),
     `spec.md:309-314` (FR-005/006 required contents).
   - Concrete failure scenario: a modest two-target, eight-requirement row:
     2 routes + 8 verdicts + blockers + alternatives + receipt is ~15-25
     facts; ten inline provenance attributes at ~8-15 tokens each is
     1,200-3,750 tokens of provenance alone. The implementer invents an
     unspecified compression (dropping or abbreviating attributes), and
     SC-006's "100% of facts identify ..." is then satisfied only by an
     encoding the spec never sanctioned - or SC-004 fails.
   - Why the current specification is insufficient: two MUSTs collide without
     a stated encoding rule; whichever one bends will bend silently.
   - Exact recommended specification change: state that provenance MAY be
     normalized by reference in responses (a bounded source/route table per
     response; per-fact short references), that SC-006 counts
     reference-resolvable attribution as identification, and that full
     inline provenance is available via the FR-074 continuation.
   - Implementation boundary affected: response contract on both surfaces,
     token-budget tests (`sc004`-style, cf. `crates/daemon/tests/sc004_token_lean_bytes.rs`).

10. **[medium] Evidence conflict resolution and mid-run invalidation are named as edge cases but have no governing rule**
    - Lens: Skeptic (stale evidence, ambiguous states).
    - Evidence: `spec.md:289-290` (edge case: evidence becomes stale or
      conflicts with a stronger target-native observation), `spec.md:281-282`
      (routes disagree on versions), `spec.md:332-334` (FR-012 ladder
      distinguishes classes but no precedence rule), `spec.md:488-489`
      (FR-070 labels staleness, does not resolve it), `spec.md:346-347`
      (FR-017 invalidates only *historical* route success on topology change).
    - Concrete failure scenario: wave 2's target-native sensor contradicts
      wave 1's harness-asserted workspace root while wave 3 sensors that
      depended on wave 1's fact are already running. Nothing states whether
      dependent facts are invalidated transitively, whether the run re-plans,
      or which fact the final map reports. An implementation that keeps both
      facts produces a signal map asserting two different workspaces with
      equal standing.
    - Why the current specification is insufficient: FR-013 requires derived
      facts to name dependencies, but no FR uses that graph: there is no rule
      that a superseded source fact invalidates its dependents, and no
      precedence rule ("higher evidence grade wins; ties resolved by
      freshness; conflicts above grade X force re-observation or `unknown`").
    - Exact recommended specification change: add an FR: evidence conflicts
      are resolved by ladder grade, then freshness; a superseded fact
      transitively invalidates dependent derived facts and unlocks re-probe
      or downgrade to `unknown`; the final map never carries two live
      contradictory facts about one attribute of one node - the losing fact
      is retracted into the bounded reason trail.
    - Implementation boundary affected: planner evidence store, FR-013
      dependency graph, retraction/reason model.

11. **[medium] FR-043/FR-045's full goal-plan lifecycle reverses the recorded evidence gate without recording why, and imports a second registry lifecycle**
    - Lens: Product critic + Architect (registry boundaries).
    - Evidence: `spec.md:421-426` (FR-043 "stored, searched, versioned,
      validated, tested, activated, and audited"; FR-045 operator local
      plans), `spec.md:606-608` (SC-014); versus the standing decision at
      `docs/superpowers/specs/2026-07-15-environment-trust-probes-design.md:113-119,305-315`
      ("Do not add storage or a public profile API in the first
      implementation ... Reconsider profiles only after real usage shows
      repeated requirement lists"); code: the store is rule-specific with
      migrations V0001-V0007 only (`crates/store/src/` tree,
      `crates/store/src/registry.rs`), matching the constitution's
      registry-boundary doctrine.
    - Concrete failure scenario: v1 ships a plan store with search,
      versioning, activation, and audit - a parallel of the sifter-rule
      registry lifecycle - before any usage evidence exists; the two
      registries then evolve divergent validation/activation semantics and
      the next feature must reconcile them (the CAP01 distortion the design
      doc rejected, arriving through the side door).
    - Why the current specification is insufficient: built-in plans (FR-044)
      need no store at all (code-owned, release-versioned). The only thing
      forcing a searchable persistent plan registry is FR-043's verb list,
      and the spec records no rationale for overturning the deferral it
      cites as required reading. "Do reject machinery that does not earn
      material LLM utility" (the brief's own standard) applies.
    - Exact recommended specification change: either (a) scope v1 operator
      plans to declarative files under operator config, validated at load,
      versioned by digest, listed and audited - dropping "searched" and
      store-backed "activated" from FR-043 - with a recorded trigger for
      promoting to a typed store later; or (b) keep FR-043 as-is and add the
      explicit justification plus the boundary statement that the plan store
      is a separate typed store contract, never the rule registry tables
      (the design doc's own condition).
    - Implementation boundary affected: `crates/store/` (new migration or
      none), config loading, plan validation/audit surface.

12. **[medium] FR-081 demands a "stable contract" on an embed surface the project explicitly declares unstable**
    - Lens: Architect (embed parity, contract ownership).
    - Evidence: `spec.md:517-518` (FR-081), `docs/EMBEDDING.md:70-73`
      ("Until a semver-stable embed facade is declared, treat public Rust
      types as a revision-pinned contract"), `spec.md:379-381` (FR-029
      requires representing an AAP Firecracker connector TC has no code
      for - no vsock substrate exists anywhere in the workspace),
      `spec.md:603-605` (SC-013 requires a live AAP Firecracker proof).
    - Concrete failure scenario: the connector trait ships as ordinary
      public Rust API; AAP pins a revision; the next TC refactor changes the
      trait; AAP's guest connector silently stops compiling against `main`
      and SC-013's live proof cannot be re-run - or, worse, the trait is
      frozen ad hoc without anyone deciding TC's first stability commitment.
      Separately, SC-013's AAP row is only provable in AAP's CI, not TC's,
      and the spec does not say whose gate it is.
    - Why the current specification is insufficient: "stable contract" is a
      real architectural commitment (TC's first) made in a subordinate
      clause; and the verification ownership for the embed/guest scenario is
      cross-repo but unassigned.
    - Exact recommended specification change: state that the embedded
      connector contract is a versioned trait with a declared compatibility
      policy (or explicitly revision-pinned like the rest of the embed
      surface, amending the word "stable" to "versioned and declared"); add
      to SC-013 that the AAP Firecracker row is proven by a named
      integration suite in the embedding repo against a pinned TC revision,
      with TC providing a connector conformance test kit.
    - Implementation boundary affected: `crates/daemon` public API surface,
      `docs/EMBEDDING.md`, AAP-side connector.

13. **[low] Non-UTF8 environment-name transport is required to be lossless but the wire is JSON**
    - Lens: Skeptic.
    - Evidence: `spec.md:276-277` (edge case: Unix names contain non-UTF8
      bytes), `spec.md:407-409` (FR-039 "without lossy collisions");
      the IPC wire is serde JSON (`crates/ipc/src/protocol.rs`, e.g.
      `decode_payload` and `MalformedJson` at `:793-794`), and existing code
      normalizes via `to_string_lossy`
      (`crates/daemon/src/environment/probe.rs:378`).
    - Concrete failure scenario: two Linux env names differing only in
      non-UTF8 bytes both lossy-decode to the same replacement-character
      string: a collision FR-039 forbids, invisible to every fixture that
      uses UTF-8 names.
    - Why the current specification is insufficient: no encoding rule for
      non-UTF8 names on a JSON wire is stated.
    - Exact recommended specification change: define the encoding (e.g.
      bytes-preserving escape for non-UTF8 names plus an `encoding` marker,
      or explicit `unrepresentable_name` counting with `complete=false`) and
      add it to the FR-039 test obligations.
    - Implementation boundary affected: IPC/MCP response types, census
      sensor.

14. **[low] Planner placement is derivable but never stated: it must be engine-owned**
    - Lens: Architect (Principle I).
    - Evidence: `.specify/memory/constitution.md:39-56` (one engine
      boundary; adapter must stay thin), `docs/security/PRIVILEGE_MODEL.md:54-74`
      (adapter holds no state across sessions), `spec.md:482-483` (FR-067
      runs survive transport interruptions - entails daemon ownership),
      `spec.md:515-516` (FR-080).
    - Concrete failure scenario: an implementer puts wave orchestration in
      the MCP adapter "because it already fans out target dials"
      (`crates/mcp/src/tools.rs` target routing precedent); runs then die
      with the stdio session, FR-067 fails, and the adapter grows exactly
      the authority Principle I forbids.
    - Why the current specification is insufficient: everything implies
      engine ownership; one sentence would foreclose the wrong reading.
    - Exact recommended specification change: state that the probe planner,
      scheduler, evidence store, and run registry are engine (`terminal-commanderd`)
      components; adapters and embed hosts only submit requests, poll or
      resume runs, and render results.
    - Implementation boundary affected: crate placement of the planner.

15. **[low] SC-003's savings baseline is self-defined and unpinned**
    - Lens: Product critic (measurability).
    - Evidence: `spec.md:575-577` (SC-003: "the equivalent documented manual
      discovery sequence").
    - Concrete failure scenario: the baseline document is written after the
      implementation, generously (extra manual calls), and the 70% claim is
      satisfied by construction.
    - Why the current specification is insufficient: a movable baseline
      cannot ground a superiority claim.
    - Exact recommended specification change: require the manual baseline
      sequences to be committed as versioned fixtures per goal family
      *before* implementation, derived from real harness transcripts, and
      referenced by SC-003.
    - Implementation boundary affected: benchmark fixtures only.

## Missing Verification

- Static review only: no daemon, probe, test suite, or canary was executed.
  Leak and behavior claims are code-read-backed, not run-backed.
- `PolicyEngine::evaluate` (`crates/daemon/src/policy.rs:749-1029`) was
  reviewed via its outline, its test names (`:1372-2680`), and POLICY.md -
  not line-by-line. Findings do not depend on its internals beyond the
  documented deny-first shape.
- The Windows-parent -> WSL runner relay is a stub returning `Unavailable`
  (`crates/daemon/src/environment/wsl.rs:35-43`), so no WSL-runner
  end-to-end path could be verified even in principle on this machine; all
  multi-hop reasoning is from code structure.
- No AAP repository was inspected; every statement about the Firecracker
  connector is from TC-side absence of substrate (no vsock code in this
  workspace) plus `docs/plans/2026-06-26-tc-embed-engine-design.md:57`.
- MCP client `roots` support was checked only by text search over
  `crates/mcp/src` (no matches); rmcp's own capability surface was not
  audited.
- Line numbers are from the working tree at review time (SymForge index
  generation 63); uncommitted local changes to `.specify/` do not affect
  cited code.

## Claims Confirmed Against Code

1. FR-079 has real substrate: `system_discover` advertises the callable
   method list derived from the dispatcher's own authority
   (`crates/daemon/src/ipc/server.rs:881-898`, parity-pinned by tests at
   `:1437-1481`), and `UnknownMethod` is a typed IPC error
   (`crates/ipc/src/protocol.rs:797-798`). Older-engine detection before
   dispatch is implementable as specified.
2. The spec's premise that route discovery is already policy-filtered is
   true as of the current tree: `apply_execution_policy`
   (`crates/daemon/src/environment/mod.rs:17-62`) removes routes the active
   policy cannot honor, and embed and IPC delivery share it via
   `DaemonState::discover_environment` (`crates/daemon/src/state.rs:366-370`)
   - satisfying the constitution's one-engine rule for this surface.
3. The design-doc gap behind FR-057 is still live: `shell_session_status`
   serializes `env_snapshot: Vec<(String, String)>` values across IPC and
   MCP (`crates/ipc/src/protocol.rs:2298-2312`,
   `crates/mcp/src/tools.rs:2530-2538`, `crates/daemon/src/shell_session.rs:213-220`).
   FR-057 targets real, current behavior.
4. FR-058's prohibition targets real behavior: snapshot rows persist
   redaction markers as values (`crates/store/src/workspace.rs:29-39`) and
   `apply_workspace` re-exports them verbatim into a live session
   (`export K='<redacted>'`; `crates/daemon/src/shell_session.rs:506-518`,
   dispatched from `crates/daemon/src/ipc/handlers/session.rs:338-342`).
5. FR-062's premise is confirmed: today's remote "probe" is a `Health` call
   returning a version (`crates/mcp/src/tools.rs:976-985`), and
   `RemoteTarget` carries no identity material - only an operator-forwarded
   local socket path (`crates/daemon/src/config.rs:604-625`). Configured
   label + health response is currently the entire trust basis.
6. The existing topology substrate is exactly two environment kinds
   (`EnvironmentSpec::Local | WslDistro`) attached to two request types
   (`crates/ipc/src/protocol.rs:1218-1221`, `:2068`;
   `crates/daemon/src/ipc/server.rs:588-609`, `:713-734`), plus MCP-level
   `target_id` federation. The spec's node/edge topology model is a large
   superset of anything present; it extends rather than contradicts, but
   nothing about multi-hop exists to reuse beyond these seams.
7. Overlay environment semantics for children are real and tested
   (`crates/probes/src/process.rs` tests `:1354-1417`), matching the
   `child_effective` hop concept.
8. FR-069's dedup direction has substrate: in-flight duplicate `start_nonce`
   collapsing already exists on the command path
   (`crates/ipc/src/protocol.rs:1255-1260`).
9. Policy is per-daemon, TOML-only, restart-scoped (POLICY.md sections 3,
   4.1; `crates/daemon/src/policy.rs` caps plumbing) - so within one node
   and one run, "current policy" cannot drift; re-evaluation complexity is
   entirely a cross-node and cross-run concern (finding 6).
10. Bounded-helper hygiene exists in discovery (`bounded_text` caps and
    redacts helper output, `crates/daemon/src/environment/probe.rs:667-681`)
    but `run_bounded` kills only the direct child on timeout (`:478-483`);
    the tree-kill machinery the spec's cleanup rules will need exists in
    `crates/probes/src/process.rs:464-583` and is not yet used by discovery.
11. The compact surface is five action-dispatched facades with
    `system_discover`/`target_probe` as `status` actions
    (`crates/mcp/src/surface_list.rs:59-68`), so FR-001/FR-078's
    two-surface, one-semantics requirement matches the existing delivery
    architecture.
12. The store is rule/audit/workspace/receipt-specific (migrations
    V0001-V0007, `crates/store/`), confirming the design doc's claim that no
    neutral registry abstraction exists for plans (finding 11).

## Lead Judgment

1. FR-053 vs FR-077 contradiction - **accept**. Both sides verified in spec
   text and live wire types; SC-015 makes it a gate item by definition.
2. Terminal-state entailment undefined - **accept**. False-ready is the
   feature's core failure mode; two conforming implementations already
   diverge on a trivial example.
3. Route readiness vs real request - **accept**. The divergence class is
   demonstrated by current code (template-anchor evaluation, US8 classifier).
4. Sensor policy taxonomy / profile mapping - **accept**. The
   read_only_observer question has no answer in the text and must have one.
5. Pre-policy discovery vs FR-010 - **accept**. Verified that sentinels
   execute pre-policy today; the spec adopts that code as foundation without
   classifying it.
6. Multi-hop policy/audit authority - **accept**. Per-node engines are the
   live model; the singular "current/central policy" wording is genuinely
   ambiguous over it.
7. Matrix minimum coverage - **accept**. Purely textual defect; direct
   answer to brief question 8.
8. Identity model completeness - **accept**. Kept medium because the
   entity framework is present and sound; only its decidability is short.
9. SC-004 vs FR-064 arithmetic - **accept**. Simple to fix; silently
   corrosive if left.
10. Conflict resolution / invalidation - **accept**. The dependency graph
    (FR-013) exists precisely to power this rule; the rule is missing.
11. Goal-plan lifecycle scope - **accept** as a required decision, not a
    mandated cut: the spec may keep the store, but must record the reversal
    of the documented evidence gate and the registry-boundary condition.
12. Embed contract stability - **accept**. One-sentence commitments that
    bind other repos need to be deliberate.
13. Non-UTF8 name encoding - **defer**. Real but small; acceptable to
    resolve in planning if the spec names the obligation.
14. Planner placement sentence - **defer**. Derivable from FR-067/FR-080 +
    constitution; add opportunistically.
15. SC-003 baseline pinning - **accept**. Cheap, and the superiority claim
    is the product's public promise.

## Final Planning Gate

**NOT READY FOR PLANNING.**

Blocking specification changes (findings 1-7; finding 11's decision and
finding 15's baseline rule should land in the same edit, and findings 8-10
and 12 should be resolved or explicitly deferred with owners):

1. Resolve the FR-053 / FR-077 / live-surface contradiction: define
   "environment-variable value", add the normative derived-fact allow-list
   or mandate the `TerminalProbe` migration as an authorized 0.x correction,
   and enumerate the existing noncompliant surfaces as in-scope migrations
   (finding 1).
2. Add the normative terminal-state entailment table for
   `ready`/`ready_with_warnings`/`blocked`/`unknown`/`denied`/`unreachable`
   over prerequisite verdicts, transport, identity, workspace, and
   completeness (finding 2).
3. Add the route-readiness proof requirement: beachhead templates must be
   validated against the real request schema and policy validator per
   declared substitution class, with the validated envelope in provenance
   (finding 3).
4. Add the sensor-class policy taxonomy and the profile/cap decision table,
   including the explicit `read_only_observer` outcome (finding 4).
5. Classify engine-origin fixed sentinel probes (exempt-and-bounded or
   policy-gated-with-FR-077-amendment) and require one shared sensing
   implementation for probe and legacy discovery (finding 5).
6. Make multi-hop policy and audit ownership normative: originating-node
   crossing caps, target-node action authority, per-node audit, per-fact
   authorizing-node provenance (finding 6).
7. Add minimum matrix coverage rules (axis coverage, critical-pair coverage,
   explicit `unsupported` rows) and bind SC-007 to them (finding 7).

With those changes, the remaining mediums are refinements inside a sound
frame, and the feature earns its scope: the staged-evidence model, secrecy
boundaries, bounds discipline, and lifecycle ownership are consistent with
the constitution and with the engine that exists.
