# Fable Re-review Brief: Environment-Probe Corrected State

## Role

Act as an adversarial specification, security, and implementation-consistency
reviewer. Code and current disk bytes are authority. Do not infer correctness
from the disposition, prior GREEN gates, or author intent.

## Mutation Boundary

Read-only review. Write exactly one new file:

`docs/reviews/2026-07-20-environment-probe-fable-rereview.md`

Do not edit, format, stage, or generate any other file. Do not run a command that
can mutate source, lockfiles, build outputs, stores, daemon state, or the reviewed
specification. Never print or copy environment values or secret material.

## Stable-State Gate

Before substantive review, record:

1. repository root, branch, and HEAD commit;
2. exact `git status --short`;
3. the recursive regular-file manifest under `specs/003-environment-probe`;
4. SHA-256 for every file in that manifest plus
   `.specify/memory/constitution.md`, `.specify/feature.json`,
   `docs/reviews/2026-07-20-environment-probe-fable-followup-review.md`, and
   `docs/reviews/2026-07-20-environment-probe-fable-followup-disposition.md`.

Repeat status, manifest, and digests immediately before the verdict. If any
reviewed byte or worktree path changes except your one allowed output file, stop
and report the state as unstable rather than reviewing mixed revisions.

## Required Reading

Read every byte of every regular file recursively under
`specs/003-environment-probe`, including subdirectories. Also read completely:

- `.specify/memory/constitution.md`
- `.specify/feature.json`
- `docs/reviews/2026-07-20-environment-probe-fable-followup-review.md`
- `docs/reviews/2026-07-20-environment-probe-fable-followup-disposition.md`

You may inspect any current code/config/test needed to verify a premise. Do not
trust stale documentation over code. The 2026-07-17 historical review and
disposition are outside this narrow re-review unless a current file explicitly
depends on a claim that can be resolved only there.

## Required Reproduction Matrix

Independently verify, rather than merely checking that text was added:

1. terminal Route evidence projection accepts every bridge/batch prior state,
   including later hard exclusion from `denied|exhausted|truncated`, and accepts
   `timed_out + fresh` for a bound terminal state without overlapping another
   outcome;
2. preliminary native-name, EvidenceBoundary final admission, and
   `surface_name_application` domains compose with no dead or unrepresentable
   `unproved` value and classify every observation tuple exactly once;
3. endpoint-binding activity is unambiguous and consistent for same-process,
   authenticated-local, and cross-boundary overlay transport/persistence;
4. the ordered channel function accepts exactly the valid complete
   same-incident bound-effect bundle, preserves every member disposition, and
   still rejects independent/conflicting/incomplete bundles;
5. private-input counter replay has one receipt effect across the replay
   partition and fixed-helper decode;
6. every applicable non-audit receipt mismatch has one property disposition,
   causal classification, and receipt effect without fabricating connector hard
   exclusion;
7. mandatory live families cover all normative minimums identified in the
   follow-up review: retention purge/saturation, retired proofs, every finite
   bound dimension including admission/queue/branch effects, names census, and
   operator-local composite plans;
8. constitution lineage and current persistence-stack claims match Git history
   and code; and
9. every Finding 11 machine-readability item is actually closed, including
   Markdown table shape, exact enums/tokens, proof mapping, effect ids, census
   mapping, and Goal aggregation.

Re-adjudicate the prior MEDIUM retry claim and the two OperationModel candidates
from first principles. If the ordered override/default rules make a candidate
total, say so with exact evidence; do not preserve a finding merely because it
appeared in the earlier review.

## Fresh Adversarial Pass

After the reproduction matrix, perform a clean cross-model pass over all six
normative models and their staged compositions. Seek new counterexamples,
overlap, holes, circular receipts, unsafe fail-open states, impossible mandatory
paths, or conformance families that can report GREEN while omitting a required
security/lifecycle behavior. Check that the new platform-clock table does not
claim unsupported inheritance across Windows, WSL, guests, or remote verifiers.

## Output Contract

The one review file must contain:

- reviewed-state pre/post fingerprints and complete input manifest;
- reading/verification method and any delegated byte ranges;
- verdict `GREEN`, `CONTESTED`, or `BLOCKED`;
- a numbered disposition for each item in the Required Reproduction Matrix;
- any new findings ordered `critical`, `high`, `medium`, `low`, each with exact
  current file:line evidence, a concrete counterexample, consequence, smallest
  root-cause correction, and verification boundary;
- claims independently confirmed against code;
- missing verification stated as absence, never success; and
- a final planning gate that says exactly whether `/speckit-plan` may begin.

`GREEN` requires no unresolved critical/high contradiction, no unclassified or
multiply classified reachable normative assignment, no secret/policy/audit
regression, and no unstable reviewed bytes. MEDIUM/LOW findings may coexist with
GREEN only if they cannot invalidate machine generation, security/lifecycle
behavior, or the mandatory planning gate; explain that judgment explicitly.
