# Fable M-1 Amendment Gate Brief: Environment and Prerequisite Probe

## Role and Review Boundary

Act as the independent clean-room reviewer for the single post-GREEN M-1
amendment to the environment-probe governing specification. The complete feature
was reviewed GREEN at the exact bytes preserved by commit `77e2fd0`
(`docs(spec): define goal-directed environment probe`). Do not inherit that
verdict blindly, but do not repeat the full-feature campaign: verify whether the
current two-line amendment exactly closes M-1 and leaves the committed
whole-feature contract unchanged everywhere else.

Current disk bytes, Git history, and the declared model are authority. Correctness,
totality, deterministic conformance, and machine readability are the gate.

## Mutation Boundary

This is a read-only review. Write exactly one new file:

`docs/reviews/2026-07-20-environment-probe-fable-m1-amendment-review.md`

Do not edit, format, stage, or generate any other file. Do not mutate source,
specification, build output, indexes, stores, or daemon state. Never print, copy,
hash, or persist environment values or secret material; names and locations alone
are sufficient evidence.

## Stable-State and Provenance Gate

Before substantive review, record repository root, branch, HEAD, exact
`git status --short`, and SHA-256 plus byte size for every reviewed file. Repeat
the same fingerprint immediately before the verdict. This brief and the one
permitted output file are the only expected new files. If any other path or
reviewed byte changes while reviewing, return `BLOCKED` for unstable evidence.

Establish these facts independently:

- base governing commit: `77e2fd0`;
- base `scenario-matrix.md` SHA-256:
  `633be34a21aa4b37f6a8f28de065486710b600aee894f10db2fad7e12ad8fbea`;
- current amended `scenario-matrix.md` SHA-256:
  `c319b92f2ed8f9193326032e88f4d7520905039b656872eabd83ceaac32ef56b`;
- the tracked diff from `77e2fd0` is exactly two additions and two deletions in
  `specs/003-environment-probe/scenario-matrix.md`, with no other tracked change.

If any fact differs, stop and report `BLOCKED` rather than reviewing a moving or
unexpected state.

## Required Reading

Read every byte of these files:

- `docs/reviews/2026-07-20-environment-probe-fable-full-feature-final-review.md`;
- this brief; and
- `specs/003-environment-probe/scenario-matrix.md`.

Read the exact `77e2fd0` version of `scenario-matrix.md` as needed for the diff.
At minimum, personally inspect and cite the current locations for:

- the M-1 finding and correction at full-feature review lines 485-531;
- the terminal-branch state vocabulary near matrix line 210;
- the campaign descendant partitions near lines 1059-1070;
- `campaign_bound_stop` and `campaign_completion` near lines 1088-1089;
- the Route/Goal effect and temporal-composition rules governing those rows;
- the last-open-branch/deadline/completion fixture family near lines 3810-3823;
- every declaration or reference containing `all-terminal`,
  `Route-bearing zero-open`, or the exact owned-state set changed by the
  amendment.

## Mandatory Reproduction

1. Compare `77e2fd0..WORKTREE` byte-for-byte. Prove that the only semantic change
   is the M-1 correction and the only second edit is its fixture-label rename.
2. Reproduce the original reachable counterexample: a `running`, zero-open
   campaign with a current terminal Route and ownership containing `ready` plus
   `cancelled`, also crossing an already-`retracted` member. Prove every owned
   member has exactly one legal disposition and the stop remains mandatory.
3. Prove the campaign-stop arms are total and pairwise disjoint:
   - at least one open branch;
   - zero open, at least one current terminal Route;
   - zero open, zero Routes, including accepted-empty and running ownership;
   - the distinct scheduler-completion path.
4. Re-run all six deadline/completion orderings from the full-feature gate:
   completion before the sampled cut; same-cut deadline; deadline after last
   terminalization but before completion commit; stale post-completion receipt;
   duplicate/replay; and crash/restart. Confirm no changed Route, Goal, receipt,
   snapshot, cleanup, retention, or lifecycle effect outside the intended branch
   enumeration.
5. Re-run the escaped-pipe-aware GFM table-shape check, declared-domain/reference
   checks, local link checks, and negative-token checks. In particular, verify the
   amendment leaves no normative `all-terminal` token for this arm and that
   `Route-bearing zero-open` is used consistently.
6. Attempt to refute the change with mixed branch sets, zero ownership, a Route
   on an open branch, multiple terminal Routes, cancelled-only zero-Route state,
   already-retracted descendants, accepted state with work, and conflicting
   deadline/completion receipts. A missing arm, overlapping arm, omitted member,
   synthetic Route, altered outcome, or ambiguous state vocabulary is blocking.
7. Confirm the previously GREEN complete-feature bytes remain exactly preserved
   by commit `77e2fd0` and the amendment introduces no claim that implementation,
   checker, corpus, measured token baseline, platform live gate, or AAP consumer
   work is complete.

## Output Contract

The one output file must contain:

- pre/post fingerprints and exact diff evidence;
- review and structural-check methods;
- exact verdict `GREEN`, `CONTESTED`, or `BLOCKED`;
- a numbered disposition for all seven mandatory reproductions;
- every finding ordered critical, high, medium, then low, with exact file:line,
  counterexample, consequence, smallest correction, and verification boundary;
- explicit confirmation that absence of implementation artifacts remains absence;
  and
- the exact conclusion whether this amendment is safe to commit directly to
  `main` on top of `77e2fd0` and whether the complete governing milestone is then
  planning-safe.

`GREEN` requires stable evidence, exact closure of M-1, no reopened full-feature
defect, no reachable unclassified or multiply classified assignment, and no
machine-readable ambiguity that can alter a conforming implementation. Editorial
observations are nonblocking only when you prove they cannot change conformance.
