# Fable Adversarial Review Brief: Goal-Directed Environment Probe

## Assignment

Perform an independent, code-backed adversarial review of Terminal Commander's
proposed goal-directed environment and prerequisite probe. This is a
specification review before implementation planning. Do not implement the
feature and do not edit any file except the requested review output.

Write your completed review to:

`docs/reviews/2026-07-17-environment-probe-fable-review.md`

## Intent

Terminal Commander should give an LLM a trustworthy onboarding primitive for an
unknown execution environment:

- one goal-directed trigger;
- many bounded internal observations;
- progressive discovery from harness evidence to target-native proof;
- disposable, narrow sensors controlled by one central planner;
- fan-out along plausible routes, convergence on viable routes, deeper sensing
  only at those convergence points, and retraction into one combed answer;
- a verified execution beachhead, exact blockers, bounded alternatives,
  provenance, freshness, completeness, and blind spots;
- fewer LLM calls, tokens, elapsed time, and wrong-environment failures;
- no environment-variable values or raw probe noise on any outward surface;
- identical trust and policy semantics through MCP delivery and in-process
  embedding, including AAP and Firecracker/vsock guests.

Correctness and product superiority govern the design. Do not reject necessary
capability merely because it is large. Do reject machinery that does not earn
correctness, trust, robustness, or material LLM utility.

## Required Sources

Read these completely:

1. `specs/003-environment-probe/spec.md`
2. `.specify/memory/constitution.md`
3. `docs/security/PRIVILEGE_MODEL.md`
4. `POLICY.md`
5. `SECURITY.md`
6. `docs/EMBEDDING.md`
7. `docs/superpowers/specs/2026-07-15-environment-trust-probes-design.md`

Then inspect the current code. Code is gospel; older plans and docs are
testimony. At minimum, trace the live flow through:

- `crates/daemon/src/environment/`
- `crates/daemon/src/state.rs`
- `crates/daemon/src/router.rs`
- `crates/daemon/src/ipc/`
- `crates/daemon/src/policy/`
- `crates/daemon/src/session/`
- `crates/probes/`
- `crates/store/`
- `crates/ipc/`
- `crates/mcp/`

Follow actual callers and public types wherever the proposed feature touches an
existing boundary. Do not accept a documentation claim that current code
contradicts.

## Adversarial Lenses

Apply all three lenses independently before synthesizing them.

### Skeptic: correctness and failure states

Find unproved assumptions, false-ready paths, races, stale evidence, ambiguous
states, secret leaks, orphaned work, incorrect negative conclusions, timeout
mistakes, and sequences that can make the planner report the wrong environment.

### Architect: boundaries and integration

Challenge responsibility placement, engine/adapter separation, MCP/embed parity,
policy-before-observation, audit ownership, registry boundaries, target identity,
multi-hop routing, AAP integration, lifecycle ownership, and compatibility with
existing route discovery rather than a parallel replacement.

### Product critic: LLM usefulness and signal quality

Determine whether one natural call really saves calls, tokens, time, and
uncertainty. Challenge request ergonomics, response bounds, continuation,
corrective guidance, goal-plan selection, evidence compression, and whether the
result tells an LLM exactly what it can safely do next.

## Required Challenges

Explicitly test the specification against:

- Codex, Claude Code, Cursor, generic MCP clients, and AAP embedding;
- Windows, PowerShell Desktop/Core, `cmd.exe`, WSL, Linux, and macOS;
- nested sandbox, container, CI, WSL, VM, remote, and multi-hop topologies;
- direct argv versus policy-gated shell routes;
- absent, stale, ambiguous, translated, and mismatched workspaces;
- reachable but unverified, mismatched, replaced, or protocol-skewed targets;
- multiple plausible routes that disagree on identity, workspace, versions, or
  readiness;
- a first wave with many scouts, only a few viable convergence points, deeper
  follow-up sensing, branch invalidation, retraction, and bounded termination;
- disconnect, resume, identical concurrent calls, cancellation, timeout, and
  cleanup;
- targeted environment-name checks and explicit names-only census across every
  topology node, with secret canaries in every value-bearing source;
- ecosystem-correct runtime and package version checks without importing or
  executing dependency code;
- finite exhaustive state coverage plus authoritative live operating-system and
  connector gates.

## Questions the Review Must Answer

1. Does the spec preserve Terminal Commander's existing working model and add
   capability through the current engine rather than replacing or bypassing it?
2. Are sensors truly simple, disposable observers while planning authority stays
   centralized?
3. Is the fan-out, convergence, deepening, invalidation, retraction, and terminal
   state model complete enough to implement without guesswork?
4. Can every fact be attributed to the exact environment and route that produced
   it, including nested and multi-hop cases?
5. Can any environment value, credential, raw helper output, or stronger caller's
   evidence escape through response, transport, audit, logs, cache, snapshots,
   recovery, or shared work?
6. Can a route be advertised that the real request schema or policy validator
   will reject?
7. Can `missing`, `ready`, or `verified` be claimed without authoritative proof?
8. Is the harness/host/target/workspace/policy/transport/goal matrix normative,
   finite, and strong enough to prevent omitted combinations from disappearing?
9. Are the success criteria measurable and sufficient to prove the promised LLM
   savings and trust?
10. What must change in the specification before implementation planning?

## Output Format

Use this exact structure:

```markdown
# Fable Adversarial Review: Goal-Directed Environment Probe

## Intent Restatement

## Verdict: PASS | CONTESTED | REJECT

## Findings

1. **[critical|high|medium|low] Short title**
   - Lens:
   - Evidence: `path:line`
   - Concrete failure scenario:
   - Why the current specification is insufficient:
   - Exact recommended specification change:
   - Implementation boundary affected:

## Missing Verification

## Claims Confirmed Against Code

## Lead Judgment

For every finding, state `accept`, `reject`, or `defer`, with one concise reason.

## Final Planning Gate

State either `READY FOR PLANNING` or `NOT READY FOR PLANNING`, and list the exact
blocking specification changes.
```

Order findings by severity. Cite exact current file and line evidence. Describe
a concrete failure scenario for every critical or high finding. Deduplicate
overlap. Do not praise the design in place of attacking it, but record claims
that you independently confirmed so the review is auditable.

Do not modify the specification. Do not write implementation code. Write only
the requested review file.
