# Fable Final Gate Brief: Environment-Probe Re-review Corrections

## Role

Act as an adversarial specification, security, and implementation-consistency
reviewer. Current disk bytes and code are authority. Do not infer correctness
from prior reviews, dispositions, or internal GREEN claims.

## Mutation Boundary

This is a read-only review. Write exactly one new file:

`docs/reviews/2026-07-20-environment-probe-fable-final-review.md`

Do not edit, format, stage, or generate any other file. Do not run commands that
mutate source, lockfiles, build outputs, indexes, stores, daemon state, or the
reviewed specification. Never print or copy environment values or secret
material.

## Stable-State Gate

Before substantive review, record repository root, branch, HEAD, exact
`git status --short`, the recursive regular-file manifest under
`specs/003-environment-probe`, and SHA-256 for every file in that manifest plus:

- `.specify/memory/constitution.md`
- `.specify/feature.json`
- `docs/reviews/2026-07-20-environment-probe-fable-followup-review.md`
- `docs/reviews/2026-07-20-environment-probe-fable-followup-disposition.md`
- `docs/reviews/2026-07-20-environment-probe-fable-rereview.md`

Repeat status, manifest, and digests immediately before the verdict. The one
permitted output file is the only allowed pre/post status difference. If any
reviewed byte or other worktree path changes, stop and report `BLOCKED` for an
unstable state.

## Required Reading

Read every byte of every regular file recursively under
`specs/003-environment-probe`, including subdirectories. Also read every byte of
the five files listed above. Inspect current code/config/Git history only where
needed to verify a premise; do not trust documentation over code.

## Required Reproduction

Independently re-derive the corrected H1 path, not merely the presence of new
text:

1. Confirm the ordinary initial Route winner order and winner-to-event closure
   remain total and unchanged.
2. Enumerate every accepted later hard exclusion from prior
   `denied|exhausted|truncated` across the prior Route causes and every accepted
   hard-exclusion cause.
3. Where the exact decisive set bound to the superseded terminal Route remains
   current with the same identity and evidence digest, verify persisting
   `unsupported`, `denied`, or `unreachable` remains the replacement Route
   outcome while lifecycle state becomes `excluded` and the new exact exclusion
   cause remains on the replacement Route.
4. Where no exact higher winner persists, verify the ordinary projection alone
   derives `blocked`.
5. Verify a new, changed, missing, mismatched, stale, or independently derived
   higher-priority record cannot impersonate temporal preservation; an
   authorized context/fact change follows its own invalidation/re-gating path.
6. Verify the composition is acyclic: prior Route/evidence plus the independent
   exclusion receipt determine the batch; the batch owns state/cause; the current
   winner owns only outcome. It must preserve true source/topology evidence,
   produce exactly one current replacement Route for Goal, retain audit/provenance,
   and never yield a safe outcome or work authority.
7. Verify the projection rows are disjoint and total for the accepted temporal
   domain, including prior unsupported, denied, unreachable, unknown, and
   truncated outcomes. Attempted evidence demotion by a non-change replay/audit
   cause must reject.

Then independently verify the four accompanying corrections:

1. M1: the single-source combination row forbids only a hard-exclusion
   Transition and remains compatible with the conditionally required ordinary
   terminal sensor-result Transition.
2. L1: the frozen scope-aware registry, not the representative prose list,
   generates the exhaustive finite bound universe and mandatory keys.
3. L2: an authorized existing-`accepted` attachment reuses the exact pending
   campaign-start wait/wake, overrides only `bound_effect`, preserves the primary
   `campaign_state_after`, and authorizes no registration, reservation,
   Transition, spawn, campaign mutation, or key mutation.
4. L3: only an exact branch- or campaign-decisive bound receipt selected by the
   closed composition and bound to the creating Transition selects
   `bound_reached`; bad receipts cannot select it, and ordinary
   `evidence_unavailable` has no dead `timed_out` alternative.

Re-run the escaped-pipe-aware GFM table-shape gate and a fresh cross-model pass
over RouteModel, GoalModel, TransitionModel, OperationModel,
SecurityPropertyModel, and EvidenceBoundaryModel. Seek overlaps, uncovered
reachable assignments, circular authority, unsafe fail-open behavior, impossible
mandatory fixtures, or a conformance set that can report GREEN while omitting a
required lifecycle/security path. Items 2-9 from the prior re-review may be
reported as still closed after regression checking; do not repeat their full
historical analysis unless a regression appears.

## Output Contract

The one output file must contain:

- complete pre/post fingerprints and input manifest;
- reading and verification method, including any delegated byte ranges;
- verdict `GREEN`, `CONTESTED`, or `BLOCKED`;
- a numbered disposition for H1 and M1/L1-L3 above;
- any new finding ordered critical, high, medium, low, with exact current
  file:line evidence, counterexample, consequence, smallest root-cause
  correction, and verification boundary;
- a structural/cross-model regression result;
- missing verification stated as absence, never success; and
- the exact gate conclusion: whether the corrected governing milestone is safe
  to commit. `/speckit-plan` remains blocked until that commit exists.

`GREEN` requires stable reviewed bytes, no unresolved critical/high
contradiction, no unclassified or multiply classified reachable assignment, no
secret/policy/audit regression, and no correction that falsifies still-current
evidence. A GREEN result authorizes committing this governing milestone; it does
not claim the still-uncommitted milestone already satisfies the later planning
gate.
