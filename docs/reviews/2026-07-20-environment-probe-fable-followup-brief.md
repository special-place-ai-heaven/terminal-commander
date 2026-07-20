# Fable Clean-Room Follow-up Review Brief: Environment Probe

## Assignment

Perform a new, independent, code-backed adversarial review of Terminal
Commander's environment and prerequisite probe specification. This is a
specification-readiness review, not an implementation task. Treat current code,
configuration, public contracts, and tests as gospel; treat prose claims as
hypotheses to verify.

Do not trust any claim that the specification is internally green. Do not use a
patch narrative, prior finding list, or prior disposition to seed findings.
Derive the review from the stable checkout and the complete current
specification first.

## Output and Mutation Boundary

Write only this new file:

`docs/reviews/2026-07-20-environment-probe-fable-followup-review.md`

Modify no other tracked or untracked repository file. Do not edit the
specification or code, run mutating formatters or generators, or overwrite the
2026-07-17 review. Use read-only inspection. If a useful command would write
inside the repository, redirect its artifacts outside the checkout or record it
as missing verification.

## Stable-Checkout Gate

Before reading substantive content, record:

1. repository root, branch, and `git rev-parse HEAD`;
2. the complete `git status --porcelain=v1 -uall` path/status listing, without
   printing file contents;
3. every regular file recursively enumerated under the literal directory
   `E:\project\terminal-commander\specs\003-environment-probe`, sorted by
   repository-relative path;
4. the SHA-256 digest of each enumerated specification file.

Repeat all four observations immediately before writing the verdict. If HEAD,
status, the enumerated path set, or any digest changed, discard the mixed-state
analysis and restart against one stable state. Put the matching pre/post
fingerprint and the complete enumerated specification-file list in the review.
If another checkout is used as evidence, including AAP, fingerprint it the same
way and identify the exact revision.

## Clean-Room Evidence Order

Use this order and make it auditable:

1. Recursively enumerate and read **every byte of every regular file** under
   `E:\project\terminal-commander\specs\003-environment-probe`, including every
   subdirectory. Do not assume the known three files are the complete set.
2. Trace each normative claim into the current Terminal Commander
   implementation, configuration, crate graph, public wire/embed surfaces, and
   tests. Follow callers and consumers across boundaries; do not stop at a
   similarly named type or helper.
3. Read governing current contracts needed to judge the claims, including the
   constitution, privilege/policy/security contracts, embedding contract, Cargo
   manifests, adapter schemas, and relevant conformance tests.
4. Freeze the independently derived findings before consulting historical
   review material.
5. Only then, optionally read
   `docs/reviews/2026-07-17-environment-probe-fable-review.md` and
   `docs/reviews/2026-07-17-environment-probe-review-disposition.md` as history.
   State whether they were consulted and do not inherit their conclusions.

List every supporting code, config, test, and contract file actually used as
evidence. If a claim depends on unavailable external code or an unpinned
consumer, mark it unverified rather than inferring compatibility.

## Review Intent

Determine whether the specification is complete, internally executable, and
faithful to Terminal Commander's current working model while adding a superior,
low-cost LLM onboarding primitive: one goal-directed trigger, bounded layered
observations, progressive route discovery, target-native proof, safe
convergence, and one compact trustworthy answer. Required capability must not be
rejected merely for having scope; machinery that does not buy correctness,
robustness, trust, or material LLM utility should be challenged.

## Required Formal Audit

Audit all six main normative models and **every subordinate model, domain,
table, projection, outcome function, and conformance family** beneath them:

1. `RouteModel`
2. `GoalModel`
3. `TransitionModel`
4. `OperationModel`
5. `SecurityPropertyModel`
6. `EvidenceBoundaryModel`

For every model and cross-model composition, prove or refute:

- finite-domain totality, mutual exclusivity, deterministic classification, and
  reachability of every accepted state;
- no contradictory rows, shadowed cases, undefined combinations, illicit
  `N/A`, circular proof, or accepted state with no valid predecessor;
- exact ownership of authority, identity, policy, audit, time, lifecycle,
  evidence, persistence, and cleanup;
- consistent terminology and equivalent meaning across specification,
  scenario matrix, checklist, implementation, wire schema, and tests;
- a mandatory conformance fixture for every normative boundary, negative
  partition, failure/recovery path, and cross-surface invariant.

At minimum, adversarially exercise these subordinate areas:

- topology, route discovery, real-request beachhead validation, target/workspace
  identity, nested and multi-hop authority, and route invalidation;
- goal-plan administration, registry behavior, plan selection and composition,
  prerequisite ecosystems, comparator provenance, unknown/ready entailment, and
  terminal aggregation;
- wave/campaign/branch fan-out, convergence, deepening, retraction, cancellation,
  resume, disconnect, idempotence, conflict, retry, and bounded termination;
- invalid-request pre-admission recovery and corrective schemas with zero
  unauthorized side effects;
- operation lifecycle, liveness, capacity, resource ownership, cleanup,
  retention, expiry, suspension, reboot, clock discontinuity, and replay;
- sensor policy taxonomy, policy-before-observation, per-hop policy and audit
  authority, least privilege, and denial/unknown behavior;
- no-secrets invariants across environment discovery, names-only census,
  overlays, private execution, helpers/decoders, diagnostics, output rings,
  buckets, tails, logs, audit, snapshots, persistence, recovery, and every
  future registered sink;
- evidence strength, provenance, source completeness, freshness, causal and
  receipt correlation, conflict handling, sanitizer/count bounds, and refusal to
  launder caller assertions or raw output into proof;
- legacy overlay migration, authoritative private-store inventory, failure,
  quarantine, retry, restart idempotence, and value-free audit;
- protocol and version skew, wire encoding/decoding, adapter behavior, compact
  and full MCP parity, daemon and in-process embed parity, stable embed contract,
  and AAP/Firecracker/vsock consumer compatibility;
- Windows, PowerShell Desktop/Core, `cmd.exe`, WSL, Linux, macOS, container,
  CI, sandbox, VM, remote, translated-path, and cross-environment combinations.

Check not only whether a requirement exists, but whether the present formal
domains force the correct result for a concrete adversarial sequence.

## Finding Standard

Do not report style preferences or ungrounded possibilities as defects. Every
finding must include:

- severity: `critical`, `high`, `medium`, or `low`;
- exact `file:line` evidence from the stable reviewed state (multiple citations
  when the contradiction crosses boundaries);
- the affected model/domain/requirement;
- one concrete input, state sequence, or deployment scenario that reaches the
  defect;
- the incorrect or undefined result and its product/security consequence;
- the exact required specification correction, stated precisely enough to
  implement and test;
- the code/wire/embed/test boundary against which the correction must be
  verified.

`critical` means unsafe disclosure, authority bypass, destructive ambiguity, or
a foundational contradiction that invalidates the model. `high` means a
reachable false-ready/false-proof/deadlock/leak/lifecycle failure or an
implementation-blocking ambiguity. Critical and high findings block readiness.
Order findings by severity and deduplicate shared root causes. Record missing
verification separately; absence of evidence is not confirmation.

## Required Review Structure

Use this structure:

```markdown
# Fable Clean-Room Follow-up Review: Environment Probe

## Reviewed-State Fingerprint
- Repository root:
- Branch:
- Commit:
- Pre/post worktree status match:
- Pre/post specification manifest match:
- External checkout fingerprints, if any:

## Complete Specification Input Manifest
| Relative path | SHA-256 |

## Supporting Evidence Files Consulted

## Intent Restatement

## Verdict: GREEN | CONTESTED

## Findings
1. **[critical|high|medium|low] Title**
   - Model/domain:
   - Evidence: `path:line`
   - Concrete counterexample or sequence:
   - Incorrect/undefined result and consequence:
   - Exact required specification correction:
   - Verification boundary:

## Model-by-Model Coverage Record
- RouteModel:
- GoalModel:
- TransitionModel:
- OperationModel:
- SecurityPropertyModel:
- EvidenceBoundaryModel:
- Cross-model compositions:
- Mandatory conformance coverage:

## Missing Verification

## Claims Independently Confirmed Against Code

## Historical Material Consultation

## Final Planning Gate: READY | NOT READY
- Blocking corrections, if any:
```

Use `GREEN` only when no critical or high finding remains and the stable-state,
complete-input, code-verification, and mandatory-conformance gates all pass.
Otherwise use `CONTESTED`. Use `READY` only with `GREEN`; otherwise use
`NOT READY` and list every blocking correction. Do not soften an unverified
claim into readiness.
