# Fable Full-Feature Final Gate Brief: Environment and Prerequisite Probe

## Role and Standard

Act as the independent adversarial specification, security, liveness, platform,
and implementation-consistency reviewer for the complete environment-probe
feature. Current disk bytes, current source code, and current Git history are the
authorities. Prior reviews, dispositions, and internal GREEN reports are inputs
to challenge, not proof to inherit.

This is the commit gate for the governing specification milestone. Review the
whole feature, not only the latest N1, deadline, or legacy-target corrections.
Correctness, robustness, deterministic conformance, and genuine LLM utility are
required together.

## Mutation Boundary

This is a read-only review. Write exactly one new file:

`docs/reviews/2026-07-20-environment-probe-fable-full-feature-final-review.md`

Do not edit, format, stage, or generate any other file. Do not mutate source,
lockfiles, indexes, stores, daemon state, build outputs, or the reviewed
specification. Never print, copy, hash, or persist environment values or secret
material. Names and locations alone are sufficient evidence.

## Stable-State Gate

Before substantive review, record repository root, branch, HEAD, exact
`git status --short`, and SHA-256 for every reviewed file. Build and record exact
recursive manifests for:

- every regular file under `specs/003-environment-probe`;
- `.specify/feature.json` and `.specify/memory/constitution.md`; and
- every regular file matching `docs/reviews/*environment-probe*.md`, including
  this brief and every prior brief, review, and disposition.

Repeat status, manifests, sizes, and hashes immediately before the verdict. The
one permitted output file is the only allowed pre/post difference. If any other
path or reviewed byte changes, stop and return `BLOCKED` for unstable evidence.

## Required Reading

Read every byte of every file in those manifests. Read the complete specification
and all 3,899 current lines of the scenario matrix; do not sample it. Inspect
current source, Cargo configuration, migrations, tests, and Git history wherever
needed to verify every current-code or compatibility premise. Documentation is
testimony; code is authority. Delegation is permitted for byte-complete ranges,
but personally re-open and adjudicate every candidate finding at its exact lines.

## Complete Feature Reproduction

Independently derive whether the feature is implementable as one coherent finite
contract. At minimum:

1. Reconstruct RouteModel, GoalModel, TransitionModel, OperationModel,
   SecurityPropertyModel, and EvidenceBoundaryModel from their declared domains.
   Prove every reachable admitted assignment is classified exactly once, every
   outcome/effect has one owner, and no policy, evidence, audit, or lifecycle
   authority is circular or caller-asserted.
2. Verify FR-001 through FR-103 and SC-001 through SC-021 are contiguous,
   internally consistent, linked to the scenario model and checklist, and strong
   enough that the required checker/generator cannot report false GREEN by
   omitting a declared family, row, state, surface, platform, or boundary.
3. Verify the one-trigger adaptive probe campaign actually saves LLM steps,
   tokens, and uncertainty: bounded scouts fan out, evidence-supported routes
   deepen, dead branches retract, facts retain provenance/freshness/conflict, and
   fan-in returns a compact goal/prerequisite/beachhead answer without hiding
   decisive uncertainty or inventing readiness.
4. Reproduce every harness/host/target/workspace/policy/transport and
   Windows/WSL/Linux/macOS/container/sandbox/VM/CI/AAP-Firecracker topology
   obligation. Confirm discovery distinguishes observation, reachability,
   identity, authorization, and an action-valid verified beachhead. A template,
   Health reply, representative cwd, or policy-neutral probe must not overclaim
   the substituted real request.
5. Reproduce the names-only public environment boundary and opaque private-value
   path. No observed value, value pair, literal redaction marker, value-derived
   hash, secret-shaped data, or inherited ambient daemon environment may cross a
   public/audit boundary or reach an unauthorized sensor. Check migration,
   persistence, restore, allowlist, purge/quarantine, retry, and operator-repair
   paths as well as new runs.
6. Verify policy and durable pre-action audit occur before every observation,
   process spawn, dial, forward, private resolution, restore, or remote hop.
   Reproduce per-node authority, multi-hop end-to-end and hop-local security,
   replay/freshness/identity binding, fail-closed audit failure, and the exact
   distinction among denial, unavailable transport, unobserved state, and
   authoritative unreachability.
7. Verify all resource and liveness machinery: finite admission, evidence,
   output, detail, queue, campaign-time, retention, alias, and persistence bounds;
   signal-driven wait/wake; deduplication; atomic cleanup; restart/clock
   continuity; tombstone/purge safety; and no polling or unbounded silence.
   Attempt same-incident multi-bound, receipt replay/mismatch, callback races,
   crash/restart, protected-retention, and no-eligible-victim counterexamples.
8. Verify compatibility and ownership for all six Product surfaces:
   `compact_environment`, `full_environment_probe`, `legacy_system_discover`,
   `legacy_target_list`, `legacy_target_probe`, and `embedded_facade`.
   There must be one engine-owned operation across MCP and embed delivery, no
   parallel adapter policy/router, no implicit dial during registry enumeration,
   and no legacy or embedded bypass of sensor policy, audit, identity, bounds,
   or evidence semantics.
9. Compare every premise about the live repository against code. In particular,
   check current discovery, policy, audit, IPC/MCP target routing, sessions,
   environment snapshots, persistence/migrations, daemon-library embedding,
   platform routing, compact/granular surface counts, Rust 1.97.1, MSRV 1.92,
   and rmcp 1.8. Classify missing implementation as an explicit migration or
   absent future artifact; never report it as implemented success.

## Mandatory Regression Reproduction

Do not merely search for corrected wording. Re-derive these paths:

1. N1: for every accepted later hard exclusion from prior
   `denied|exhausted|truncated`, apply exact member effects, derive the complete
   current post-batch closure, and run the ordinary winner algebra. Cover every
   persisting subset, partial invalidation, full invalidation, class shift,
   conflict recomputation, stale/mismatched negative twin, exactly one replacement
   Route, lifecycle `excluded`, exact new cause/event, no safe output, and no work.
2. Campaign deadline/completion: cover open branches, Route-bearing all-terminal,
   accepted-empty, cancelled-only zero-Route, and cancelled-root plus
   already-retracted-descendant zero-Route states. Prove arm guards and branch,
   Route, Goal, receipt, snapshot, cleanup, retention, and campaign effects are
   total and disjoint for completion-before-cut, same-cut reached, reached after
   terminalization but before completion, stale post-completion, duplicate, and
   restart orderings.
3. Legacy target list/probe: cover local and forwarded targets, enumeration
   without dial, denied-before-contact, audit denial/failure, authorized contact,
   transport unavailable, successful liveness, and the rule that Health cannot
   establish identity or a verified beachhead.
4. Recheck M1, L1, L2, L3 and every prior clean-room item 2-9. Re-run the
   escaped-pipe-aware GFM table-shape gate, domain/reference/link checks, negative
   token canaries, and machine-readability constraints from fresh bytes.

## Output Contract

The one output file must contain:

- complete pre/post fingerprints and manifests;
- byte-reading, code-verification, and structural-check methods;
- exact verdict `GREEN`, `CONTESTED`, or `BLOCKED`;
- a numbered disposition for every complete-feature area and mandatory
  regression above;
- every new finding ordered critical, high, medium, then low, with exact current
  file:line and code evidence, a reproducible counterexample, consequence,
  smallest root-cause correction, and verification boundary;
- missing implementation, external consumer, platform, oracle, corpus, checker,
  or measurement evidence stated as absence, never success; and
- the exact conclusion whether the complete governing milestone is safe to
  commit directly to `main`. `/speckit-plan` and implementation remain blocked
  until that commit exists.

`GREEN` requires stable reviewed bytes; no unresolved correctness, security,
privacy, totality, liveness, compatibility, machine-checkability, or evidence-
authority defect that could change a conforming implementation; no unclassified
or multiply classified reachable assignment; and no false claim that a future
artifact already exists. Pure editorial observations may be nonblocking only when
you demonstrate they cannot alter conformance. GREEN authorizes this specification
commit, not implementation completion.
