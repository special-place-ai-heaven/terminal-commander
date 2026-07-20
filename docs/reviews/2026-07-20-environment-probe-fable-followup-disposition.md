# Disposition: 2026-07-20 Environment-Probe Fable Follow-up

Date: 2026-07-20

Reviewed input:
`docs/reviews/2026-07-20-environment-probe-fable-followup-review.md` at its
recorded stable fingerprint. The review itself remains immutable. This file
records independent reproduction, correction, and re-verification status.

## Verdict Before Re-review

All four reported HIGH items were independently adjudicated. Findings 1, 2,
and 4 reproduced as formal contradictions. Finding 3 did not reproduce as a
formal active/neutral contradiction because the selected property row already
activates the endpoint axis over its full domain, but its domain wording was
ambiguous and was clarified in the review's safer direction. Confirmed
MEDIUM/LOW defects were also corrected. A fresh independent re-review remains
the final planning gate.

## Finding Dispositions

1. **Terminal evidence projection: ACCEPTED AND CORRECTED.** The closed
   projection now enumerates every accepted prior state for `ready`, `denied`,
   and `excluded`; terminal priors `denied|exhausted|truncated` are admitted for
   a later hard exclusion, atomically superseded, and retained only as
   provenance. The bound projection admits both `timed_out` and `truncated`
   observations. See `scenario-matrix.md:1286-1320`.

2. **Preliminary native-name `unproved`: ACCEPTED AND CORRECTED.** `unproved`
   is now exclusively an evidence/observation state. EvidenceBoundary continues
   to normalize unproved classification evidence to verdict `unknown`; Security
   no longer declares or consumes an impossible preliminary `unproved` verdict.
   See `scenario-matrix.md:1918-1919` and the closed
   `surface_name_application` tuple.

3. **Private endpoint activity: FORMAL CONTRADICTION REFUTED; AMBIGUITY
   CORRECTED.** The controlling `overlay_transport` property row already names
   channel/confidentiality/endpoint axes active over their full domains, so the
   generic neutral-axis rule did not force the authenticated-local value to
   `not_applicable`. The endpoint domain now says explicitly that
   authenticated-local and cross-boundary non-campaign overlays require the
   axis, while same-process/no-overlay rows require `not_applicable`. See
   `scenario-matrix.md:1961`.

4. **Same-incident bound-effect bundle: ACCEPTED AND CORRECTED.** The ordered
   channel combination function now has an accepting row for
   `many + exact_same_incident_bound_effect_bundle`, complete member-key
   coverage, one incident/scope/bundle identity, compatible no-receipt
   negatives, and one atomic `bound_effect_bundle_commit`. Every other
   multi-source shape remains rejected. See `scenario-matrix.md:2287`.

5. **Committed-key retry conflict: REFUTED; CLARIFIED.** OperationModel is an
   ordered classifier: clause 2 derives the precommit `present_new` retry state,
   while clause 4 explicitly overrides named fields after an exact commit. The
   existing final result was therefore uniquely `retry=resolved`, not
   contradictory. The commit clause now repeats that override beside
   `bound_atomically` and the bounded horizon so a reader need not infer it
   across clauses.

6. **Decoder replay receipt effect: ACCEPTED AND CORRECTED.** A replayed private
   input counter remains an in-domain fail-closed decoder rejection but now
   derives `no_receipt` and causal `zero + not_applicable`, matching the
   authoritative replay partition. Structural invalid projections alone retain
   `receipt_effect = rejected`. See `scenario-matrix.md:2641-2645`.

7. **Surface-name tuple totality: ACCEPTED AND CORRECTED WITH FINDING 2.** Any
   non-inapplicable preliminary verdict paired with an unproved private
   observation now derives `unknown` with no receipt. The closed residual row
   still rejects extraneous or incompatible shapes. Canonical/rejected inputs do
   not authorize when observation proof is absent.

8. **Non-audit mismatch disposition: ACCEPTED AND CORRECTED.** The source-total
   table now covers every applicable non-audit immutable-receipt mismatch,
   preserves its exact subclass/scope in a non-authorizing receipt, fails the
   owning property/action closed, and never fabricates connector hard exclusion.
   A first fresh semantic gate then exposed a secondary omission in the
   single-source ordered-combination effect cell. That cell now explicitly
   derives the campaign-local property rejection with no hard-exclusion
   Transition, and the mandatory live families require mismatch and replay
   fixtures at every owning property. See `scenario-matrix.md:2217`, `:2288`,
   and `:3674`.

9. **Mandatory live-family omissions: ACCEPTED AND CORRECTED.** The generated
   minimum now covers every frozen bound-registry dimension, admission and queue
   effects, targeted/full census, operator-local composite plans, timer/capacity
   retention expiry and purge, final partition fold, saturated no-victim wake,
   and retired-proof dispatch denial. See `scenario-matrix.md:3678-3710` and
   `:3758`.

10. **Uncommitted constitutional authority: ACCEPTED AS A MILESTONE GATE; TEXT
    CORRECTED.** This specification milestone intentionally remains uncommitted
    until independent GREEN, so the procedural part closes only when the final
    verified commit lands before planning. The constitution now records the
    actual `1.0.0 -> 2.1.0` history, explains why no intermediate 2.0.0 was
    committed, and names rusqlite WAL plus the repository-owned manual migration
    runner instead of claiming active refinery integration.

11. **Editorial/machine-readability bundle: ACCEPTED AND CORRECTED.** The
    persistence-policy table has three columns in every row; the bound-bundle
    sentence has its subject; surface/private counter tokens are distinct;
    `unproved` follows exact owning-property rules; proof-validation causes are
    present; `EvidenceBoundaryModel`, catalogue spelling, primary-effect ids,
    purge predicate, total-time pre-route evidence, reached-set axis names, Goal
    aggregation, and census set/marker mappings are normalized and closed. A
    subsequent structural gate found 240 unescaped payload pipes across 106 GFM
    rows plus two rows whose semantic cells did not match their three-column
    headers. Literal union pipes are now escaped only inside inline code, both
    rows have explicit source/input/effect cells, and two independent parsers
    report all 59 tables and 860 rows structurally valid. A final six-model
    semantic rerun confirms the normalization did not change model values.

## Missing-Verification Adjudication

- **Suspend-inclusive clocks:** locally resolved at specification level. The
  platform support registry now requires implementation-tested mappings for
  Windows `QueryPerformanceCounter`, Linux/WSL `CLOCK_BOOTTIME`, and macOS
  `mach_continuous_time`, with target-native proof and fail-closed unavailable
  handling. The mappings cite the official platform contracts. Runtime OS gates
  remain implementation obligations.
- **Fresh alias on a terminal campaign:** not a totality defect. It reaches the
  complete structural rejected default; no key or campaign mutation occurs.
  No new public reason semantic was invented.
- **Queue-gate scope:** not a semantic contradiction. The disputed paragraph is
  scoped to a successful new/independent admission, while an existing-running
  queue gate is separately admitted. The sentence now states that scope
  explicitly.
- **AAP Firecracker/vsock consumer, comparator reference corpus, FR-095/FR-100
  measured artifacts, and shipped registries:** remain explicit pre-planning or
  implementation gates. Absence is not recorded as success, and no external AAP
  compatibility claim is made here.

## Final Gate

Planning and implementation remain blocked until:

1. fresh semantic and structural gates pass on one stable final fingerprint;
2. a narrow independent Fable re-review verifies the corrected findings and
   reports no new blocking contradiction; and
3. the entire governing specification/constitution milestone is committed
   before `/speckit-plan` begins.

## 2026-07-20 Fable Re-review Addendum

The narrow re-review in
`docs/reviews/2026-07-20-environment-probe-fable-rereview.md` returned
`CONTESTED`: reproduction items 2-9 were GREEN, while one new HIGH winner/event
composition defect and M1/L1-L3 remained. The review file is immutable; this
addendum records the independently reproduced dispositions and corrections.

1. **H1: ACCEPTED AND CORRECTED WITHOUT EVIDENCE DEMOTION; THE SET-LEVEL
   CORRECTION IS SUPERSEDED BY THE N1 ADDENDUM BELOW.** A later accepted
   hard exclusion from prior terminal `denied` or `exhausted` could not coexist
   with an unchanged higher-priority winner. The tempting fix of invalidating
   still-true evidence was rejected. The closed temporal composition now keeps
   an exact persisting prior `unsupported`, `denied`, or `unreachable` winner and
   Route outcome while the replacement Route carries lifecycle state `excluded`
   and the new exact hard-exclusion cause. With no exact persisting higher
   winner, the ordinary projection derives `blocked`. New/mismatched evidence
   cannot use the exception, no safe outcome is possible, and mandatory fixtures
   cover persistence, authorized invalidation/change, Goal replacement, and
   illicit-demotion rejection.
2. **M1: ACCEPTED AND CORRECTED.** The single-source combination row now forbids
   only a hard-exclusion Transition. Its conditionally required terminal sensor
   result still commits through the one ordinary sensor-result Transition.
3. **L1: ACCEPTED AS A STALE SECOND SOURCE.** The frozen bound registry remains
   the exhaustive generated universe. The adjacent prose list is explicitly
   representative, so omitted examples cannot narrow conformance.
4. **L2: ACCEPTED AND CORRECTED.** An authorized attachment to an existing
   `accepted` campaign may project the exact pending campaign-start wait. It
   reuses the wait/wake, changes only `bound_effect`, preserves the primary
   `campaign_state_after` value, and authorizes no new registration, reservation,
   Transition, spawn, campaign mutation, or key mutation.
5. **L3: ACCEPTED WITH A STRONGER CAUSAL DISCRIMINATOR.** `bound_reached` now
   requires the exact branch- or campaign-decisive bound receipt selected by the
   closed composition and bound to the creating Transition. Bad receipts cannot
   select it, and the dead `timed_out` alternative was removed only from the
   ordinary `evidence_unavailable` row.

Focused independent gates are GREEN for all five corrections. Full stable-state
semantic, content, structural, and final Fable gates remain required before the
milestone commit.

## 2026-07-20 Fable Final-Gate N1 Addendum

The final-gate review in
`docs/reviews/2026-07-20-environment-probe-fable-final-review.md` returned
`CONTESTED`. It verified M1/L1-L3, both original H1 endpoints, prior items 2-9,
and the structural gate, but reproduced one remaining HIGH gap in the set-level
H1 correction: partial persistence and winner-class shifts had no legal temporal
projection. The immutable review remains the finding authority; this addendum
records the correction.

1. **N1: ACCEPTED AND CORRECTED WITH POST-BATCH WINNER DERIVATION.** The strict
   historical-set fix proposed by the review was narrowed because it would reject
   a legitimate derived record recomputed after an authorized context change.
   The closed composition now applies the cause's exact typed per-record effects
   in one atomic `fresh_unique` Transition, derives the complete current
   post-batch source/topology/derived closure, and then runs the unchanged
   ordinary winner algebra. Current `unsupported`, else `denied`, else
   `unreachable`, else the exact hard-exclusion record selects `blocked`.
   Lifecycle state `excluded`, event `hard_excluded`, and the exact new cause are
   independent invariants; the superseded Route is provenance-only.
2. **TYPE AND AUTHORITY BOUNDARIES: CLOSED.** Authoritative-context,
   source-fact, and derived-fact members use only their declared effect domains.
   No-change causes consume the unchanged current store. Change causes require
   exact retain/invalidate/remove/recompute coverage and exact dependency roots;
   topology is derived, not an invented effect member. Unauthorized source
   admission, bad effects, stale/cross-snapshot inputs, caller-asserted outcomes,
   and stale Route provenance reject.
3. **TOTALITY AND FIXTURES: CLOSED.** The temporal rows are keyed by the current
   post-batch winner rather than the prior winner. Mandatory generated fixtures
   cover every same-class subset, full persistence/invalidation, all downward
   class shifts, conflict recomputation, no-change cases, prior unknown/truncated
   cases, one replacement Route, and the complete negative twin set.

Three focused independent byte-current gates now report GREEN for N1 semantics,
totality/disjointness, and GFM/regression integrity. This is not the milestone
gate: every full-feature semantic, structural, content, security, and independent
Fable check remains required on one stable fingerprint before commit.

## 2026-07-20 Complete Internal-Gate Addendum

The first complete internal pass did not treat the focused N1 result as feature
GREEN. It reread the full governing corpus and current implementation premises,
then found two independent HIGH gaps and one semantic residue in the first
correction. All three are accepted and corrected here before the final external
gate:

1. **CAMPAIGN DEADLINE/COMPLETION RACE: CORRECTED.** A total-time receipt can
   become decisive after every branch terminalizes but before the scheduler
   commits completion. `campaign_bound_stop` now has disjoint open,
   Route-bearing all-terminal, accepted-empty, and zero-Route terminal arms.
   Same-cut `reached` wins; completion requires a same-revision `within` cut;
   post-completion receipts reject as stale. Route-bearing terminalization
   retains the exact Route/Goal snapshot. Every zero-Route/zero-open case instead
   derives `bound_before_route` and Goal `unknown` without fabricating a Route.
2. **ZERO-ROUTE TRANSITION TOTALITY: CORRECTED.** The exclusive running
   zero-Route arm now maps every owned terminal/cancelled branch to `retracted`,
   preserves already-`retracted` branches unchanged, and assigns exact Route
   effect `none`; the accepted-empty case has no branch member. Cancelled-only
   and cancelled-root/already-retracted-descendant races are mandatory fixtures.
3. **LEGACY TARGET OBSERVATION: CORRECTED.** `legacy_target_list` and
   `legacy_target_probe` are explicit Product surfaces. Registry enumeration
   cannot dial. Every liveness or reachability observation requires the common
   connector sensor decision, capability and remote authority, durable
   pre-action audit, and denied-before-contact behavior. Transport failure is
   not policy denial, and a Health response cannot establish target identity or
   a verified beachhead. Both legacy surfaces remain MCP delivery projections of
   the one engine-owned operation, never a parallel adapter engine.

Fresh complete semantic, structural/GFM/domain/link, and security/code-premise
reviews are GREEN on specification hashes
`d081e0b96fddb4bb165f648c9d00a2bcec8548553d315f124b1e86cddc34082c`
and
`633be34a21aa4b37f6a8f28de065486710b600aee894f10db2fad7e12ad8fbea`.
They also rechecked N1, M1/L1-L3, prior items 2-9, compatibility, persistence,
platform, embed, secret, policy, audit, and remote premises. This internal GREEN
does not authorize commit by itself: the independent full-feature Fable review
must still return GREEN on the frozen final review corpus.
