# Fable Final Gate Review: Environment-Probe Re-review Corrections

Reviewer: Claude (Fable 5), 2026-07-20. Final-gate review per
`docs/reviews/2026-07-20-environment-probe-fable-final-brief.md`. Current disk
bytes and code were treated as authority; no correctness was inferred from the
prior reviews, dispositions, or internal GREEN claims.

## Reviewed-State Fingerprint

- Repository root: `E:/project/terminal-commander`
- Branch: `main`
- HEAD commit: `40eb16f19cb9b15d36bbf3ddaaa4beaacb86225e`
- Pre/post `git status --short` match: YES. Both observations returned exactly:
  - ` M .specify/feature.json`
  - ` M .specify/memory/constitution.md`
  - `?? docs/reviews/2026-07-17-environment-probe-fable-brief.md`
  - `?? docs/reviews/2026-07-17-environment-probe-fable-review.md`
  - `?? docs/reviews/2026-07-17-environment-probe-review-disposition.md`
  - `?? docs/reviews/2026-07-20-environment-probe-fable-final-brief.md`
  - `?? docs/reviews/2026-07-20-environment-probe-fable-followup-brief.md`
  - `?? docs/reviews/2026-07-20-environment-probe-fable-followup-disposition.md`
  - `?? docs/reviews/2026-07-20-environment-probe-fable-followup-review.md`
  - `?? docs/reviews/2026-07-20-environment-probe-fable-rereview-brief.md`
  - `?? docs/reviews/2026-07-20-environment-probe-fable-rereview.md`
  - `?? specs/003-environment-probe/`
- Pre/post manifest and digest match: YES. The recursive regular-file manifest
  under `specs/003-environment-probe` and every SHA-256 digest below were
  identical before analysis and again immediately before this verdict. No
  reviewed byte changed during the review. The only file written is this one,
  which is the sole permitted pre/post status difference.

### Complete Input Manifest (SHA-256, identical pre and post)

| Relative path | SHA-256 |
|---|---|
| `specs/003-environment-probe/checklists/requirements.md` | `7a9c19deea43aab636227ebe03e4bcfbd9b7163abdaa35aa3059663740f60d2e` |
| `specs/003-environment-probe/scenario-matrix.md` | `514580e18f91ad4098af602c0fb9db8c2e469a5e0d5444a3038c67ca8f70fbd7` |
| `specs/003-environment-probe/spec.md` | `df6f6170e82f16f0b501aecd7ae08a7391af94a2dcdf4f86d47f8bcdd0679d53` |
| `.specify/memory/constitution.md` | `594e4e9126681d5fd10d982d91e382224a33499e22a09bb9e0d4781bcb8227b2` |
| `.specify/feature.json` | `c6e6d35dc14946e4156765647ac64b02bd318405c7c5a410e1f4927d033d52d0` |
| `docs/reviews/2026-07-20-environment-probe-fable-followup-review.md` | `06bb151bbe9d4391fa767e512339c6bea7aec20c7e7c42b175cb68e73976260a` |
| `docs/reviews/2026-07-20-environment-probe-fable-followup-disposition.md` | `071e49761ee4975da2be30a1ad630b7296bc41f3cdfdebc7f15d00454b4b230c` |
| `docs/reviews/2026-07-20-environment-probe-fable-rereview.md` | `04a7bc95da504fe183cb2ba8656dd039eb3e601f8c8ba178a740715bc2b9634b` |

The three files under `specs/003-environment-probe` are the complete recursive
regular-file set there, including subdirectories. Relative to the 2026-07-20
re-review fingerprint, exactly one specification file changed:
`scenario-matrix.md` (`5f5da594...` -> `514580e1...`, 3764 -> 3822 lines);
`spec.md`, `checklists/requirements.md`, the constitution, and `feature.json`
are byte-identical to the state the re-review verified. The followup
disposition changed only by its appended re-review addendum.

## Reading and Verification Method

- Every byte of every file in the manifest was read directly by this reviewer:
  `scenario-matrix.md` (all 3822 lines, in ten contiguous windows: 1-700,
  701-1100, 1101-1440, 1441-1780, 1781-2110, 2111-2440, 2441-2770, 2771-3100,
  3101-3430, 3431-3822), `spec.md` (all 1475 lines, four windows),
  `checklists/requirements.md` (49 lines), `.specify/memory/constitution.md`
  (234 lines), `.specify/feature.json` (3 lines), the follow-up review (690
  lines), the follow-up disposition (180 lines), and the re-review (468 lines).
  No delegate readers were used; there are no delegated byte ranges.
- Code and Git verification used read-only `grep`, `git show`, `git log`, and
  `git log -S` only. SymForge retrieval was deliberately not used: its indexer
  maintains a store, and the brief forbids commands that can mutate stores.
- The Markdown-table structural gate was reproduced independently with a
  character-scan cell counter (escaped `\|` excluded, no regex) fed to the
  interpreter over stdin; no script or temporary file was written.
- The 2026-07-17 historical review and disposition were not consulted.

## Verdict: CONTESTED

The H1 correction closes the two temporal-composition cases it names (full
persistence of the decisive winner set, and full authorized invalidation), and
all four accompanying corrections (M1, L1, L2, L3) verify exactly as
dispositioned. However, independent re-derivation of the corrected H1 path
finds one new HIGH finding: the closed temporal composition and its projection
rows are not total when the prior decisive winner record set persists only
partially, or when a different persisting higher-priority record re-derives a
winner class that no preservation row keys (Finding N1). It is the narrower
residue of the same must-accept/must-reject class as H1. Items 2-9 of the
prior re-review show no regression. No critical finding: no unsafe disclosure,
authority bypass, fail-open path, or secret/policy/audit regression was found,
and every code-facing premise re-checked was true.

## Numbered Dispositions: H1 and M1/L1-L3

1. **H1 (excluded-from-terminal-prior winner algebra) - CORRECTION VERIFIED AS
   DESIGNED; NEW RESIDUAL HIGH (Finding N1).** Walking the brief's seven-step
   reproduction:
   1. *Initial winner order and closure unchanged and total - VERIFIED.* The
      joint winner order (`scenario-matrix.md:330-339`) is unchanged: (1)
      unsupported, (2) denied, (3) unreachable, (4) missing|incompatible, (5)
      residual hard unknown/observation/freshness, (6) hard-safe soft-warning,
      (7) fully safe. The derived initial terminal transition (`:355-365`)
      still maps every winner class to exactly one event/cause, now explicitly
      qualified "initial", with the sole later-terminal exception named at
      `:366-369`. Total over classes 1-7.
   2. *Accepted later hard exclusions enumerated - VERIFIED.* Prior terminal
      states/causes: `denied/policy_denial` (outcome `denied`),
      `exhausted/no_representable_candidate` (`unsupported`),
      `exhausted/authoritative_unreachable` (`unreachable`),
      `exhausted/evidence_unavailable` (`unknown`), `truncated/bound_reached`
      (`unknown`). The batch row (`:1062`) accepts all three prior terminal
      states; the cause partition (`:1105`) mandates
      `terminal_branch_commit/hard_excluded` for all thirteen hard-exclusion
      causes from those priors, never recoverable; the bridge (`:1308`)
      accepts them.
   3. *Persisting winner preservation - VERIFIED for full persistence.* Rows
      `:1320` (prior `denied` -> outcome `denied`), `:1321` (prior
      `exhausted/no_representable_candidate` -> `unsupported`), and `:1322`
      (prior `exhausted/authoritative_unreachable` -> `unreachable`) each
      require the exact decisive record set with the superseded Route's
      identity and evidence digest, put lifecycle state `excluded` and the new
      exact cause on the replacement Route, and preserve the persisting winner
      as the outcome. Consistent with the composition (`:341-353`), the batch
      row (`:1062`), and - decisively - with spec.md's entailment table:
      Route `denied` (`spec.md:1280`) remains the necessary-and-sufficient
      description of a branch whose governing denial evidence persists, and
      Route `blocked` (`spec.md:1282`, "supported, authorized, and reachable")
      would be falsified there, so preservation is the only spec-consistent
      outcome.
   4. *No persisting higher winner -> ordinary `blocked` - VERIFIED for full
      invalidation.* Row `:1323` (prior set includes `denied`, `exhausted`,
      `truncated`; "no unsupported/denied/unreachable winner; hard aggregate
      `missing|incompatible`") plus `:351-353`. A prior
      `evidence_unavailable`/`bound_reached` terminal (winner class 5) and a
      fully invalidated higher winner both land here; the new exclusion
      record's class-4 evidence wins over residual unknown and derives
      `blocked`.
   5. *Impersonation excluded - VERIFIED.* `:350-351` ("A new, changed,
      missing, or mismatched higher-priority record cannot use the
      exception"), the per-row "no new or mismatched higher-priority record"
      constraints (`:1320-1322`), and the digest-match rule (`:1354-1356`)
      reject fabricated preservation. A record whose evidence has gone stale
      can no longer project a `denied|unreachable` verdict (freshness `fresh`
      is required at `:236-239`), so it yields no persisting winner and routes
      to `blocked`. Authorized context/fact changes follow their own paths:
      recoverable revisions re-gate even terminal `denied|exhausted|truncated`
      branches (`:1065`, `:1102`; `spec.md:1134-1137` FR-099), and non-change
      replay/audit causes can invalidate nothing (`:751`-equivalent at
      `:1151-1156`).
   6. *Acyclic composition - VERIFIED.* Prior Route/evidence plus the
      independent exclusion receipt determine the batch; the batch owns
      state/cause; the current winner owns only the outcome (`:341-353`,
      `:366-369`). No demotion/invalidation/deletion by the composition
      itself (`:349-350`); exactly one current replacement Route with the old
      record retained as provenance outside the Goal presence vector
      (`:1345-1352`); no safe outcome or work authority is derivable
      (`:353`, and every preservation outcome is non-safe).
   7. *Disjointness and totality - PARTIALLY VERIFIED; totality FAILS in two
      reachable corners (Finding N1).* The rows `:1320-1323` are pairwise
      disjoint (each preservation row requires the winner that `:1323`
      forbids, and rows are keyed by distinct prior/state/cause). Totality
      holds for full persistence and full invalidation, and attempted evidence
      demotion by a non-change replay/audit cause rejects (`:1151-1156`;
      fixture `:3725-3726`). It does not hold for partial persistence of a
      multi-record decisive set or for a winner-class shift after partial
      invalidation. See Finding N1.

2. **M1 (single-source combination row) - VERIFIED CORRECTED.** Row `:2332`
   now reads "...campaign-local property rejection (including the exact
   terminal sensor `rejected` result when applicable, bounded fallback only,
   and no hard-exclusion Transition)...". This forbids only a hard-exclusion
   Transition and is compatible with the conditionally required ordinary
   terminal sensor-result Transition: a terminal sensor `rejected` result
   commits through `sensor_observation_commit/sensor_result_recorded`
   (`:916-939`), which is not a hard-exclusion Transition, and mirrors the
   authoritative source-total rows "no connector hard exclusion is fabricated"
   (`:2261`) and "no hard-exclusion Transition is fabricated" (`:2263`). The
   live families require the mismatch/replay fixtures to prove "the exact
   property-local rejection, exact non-authorizing receipt, and zero
   hard-exclusion Transition" (`:3727-3733`). No residual contradiction.

3. **L1 (bound-registry enumeration) - VERIFIED CORRECTED AS A STALE SECOND
   SOURCE.** The family now opens "every finite dimension generated from the
   frozen scope-aware bound registry, not a hand-selected subset.
   Representative families include ..." (`:3734-3736`). The prose list is
   explicitly representative and cannot narrow conformance; the generated
   required-key universe remains governed by the registry and the CI
   missing-key report (`:3620-3633`). The previously omitted `result_detail`
   (`:3126`) and retention dimensions (`:3129-3131`) are members of the frozen
   dimension table that generates the universe, so their absence from the
   representative prose is now harmless.

4. **L2 (existing-`accepted` attachment queue gate) - VERIFIED CORRECTED.**
   The queue-gate validity clause now names three forms (`:1516-1529`),
   including "(b) an authorized attachment to an existing `accepted` campaign
   carrying the exact still-pending `campaign_start` wait at the same
   registry/capacity revision", and states "An existing-accepted attachment
   reuses the exact durable wait and wake registration; it creates no
   registration, reservation, Transition, spawn, campaign mutation, or key
   mutation." Clause 7 (`:1909-1919`) overrides only `bound_effect` to
   `queued_in_progress` and explicitly preserves the primary mapping's
   `campaign_state_after` (`accepted` for `active_accepted`, `unchanged` for
   existing-campaign no-wait start/resume/detail). The validity clause and
   clause 7 now state the same scope; the live families require matching-key
   start/resume/detail attachments to "reuse the exact pending
   `campaign_start` wait/wake and prove zero new registration, reservation,
   Transition, spawn, campaign mutation, or key mutation" (`:3745-3748`).
   Consistent with the compatibility table (`:1556-1560`), which neutralizes
   only admission axes, not the Operation-bound input, for resume/detail.

5. **L3 (`bound_reached` discriminator and dead `timed_out` disjunct) -
   VERIFIED CORRECTED.** The event mapping now requires "an exact branch- or
   campaign-decisive bound receipt selected by the closed bound-effect
   composition and bound to the creating Transition" for
   `bound_reached/bound_reached` (`:360-364`), and "Missing, mismatched,
   replayed, stale, or cross-Transition bound receipts cannot select
   `bound_reached`; they retain their exact ordinary unknown/rejection class"
   (`:370-371`). The undefined phrase "terminal unknown with truncation" is
   gone. Row `:1327` (`exhausted/evidence_unavailable`) now accepts only hard
   observation `probe_failed` - the dead `timed_out` disjunct is removed,
   which is correct because hard `timed_out|truncated` observations exist only
   inside the bound compositions (`:952-955`) that terminalize through
   `bound_reached`. Row `:1328` retains `timed_out|truncated` with the bound
   receipt. Disjoint at event selection and at the bridge.

## New Findings

1. **[high] N1: The temporal composition is not total under partial
   persistence of the decisive winner set, or under a winner-class shift from
   a persisting non-decisive higher-priority record**
   - Model/domain: RouteModel winner algebra x TransitionModel temporal
     composition; the direct residue of corrected Finding H1, in a narrower
     region.
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:344-349` (arm
     A: preservation requires "the exact decisive record set bound to the
     superseded terminal Route remains current with the same identity and
     evidence digest"); `:351-353` (arm B: "When no exact prior higher winner
     persists, the ordinary hard-exclusion evidence owns the winner and Route
     outcome `blocked`"); `:1320-1322` (each preservation row requires the
     exact decisive set digest match AND is keyed to one exact prior
     state/cause pair); `:1323` + `:1343-1344` (the only `blocked` row
     requires "no unsupported/denied/unreachable winner"; every other tuple is
     a rejected checker constraint); `:1105` (the thirteen hard-exclusion
     causes from prior `denied|exhausted|truncated` MANDATE
     `terminal_branch_commit/hard_excluded`, "never recoverable"); `:1062`
     (the batch must "install or replace the exact terminal Route from the
     closed projection table"); `:1146` (a change-carrying batch "invalidate[s]
     exactly the sources whose receipts depend on the changed context;
     retain[s] proved-independent sources" - invalidation is per-record, not
     all-or-nothing); `:330-339` ("Lower-priority hard and all soft records
     remain in provenance"); `:257`, `:287` (any denied edge decision derives
     a hard `denied` record, so several simultaneous decisive denial records
     are reachable; `:259` likewise for unreachable transport records);
     `:1354-1356` ("Their identity/evidence digest must match the superseded
     Route for the later-terminal preservation rows; otherwise those rows
     reject"); `:1950` (authority-chain records bind node/target/boot/workspace
     identity, so policy receipts are context-bound and partially
     invalidatable); `:3718-3726` (the mandatory fixture family crosses only
     "an exact unchanged prior decisive record set and ... its authorized
     invalidation/change" - the partial case is not in the cross).
   - Concrete counterexample (class shift, the cleanest): branch B terminates
     `denied` - its decisive set is the fresh hard-denial record D on edge E2,
     while a lower-priority fresh hard `unreachable` record U (edge E1
     transport `refused`, winner class 3) remains in provenance per
     `:337-338`. Later, an unapproved connector rotation is detected on E2.
     `:1105` mandates `terminal_branch_commit/hard_excluded` with cause
     `connector_unapproved_rotation`; the batch carries the exact
     `connector_instance` authoritative-context member (`:773`) and, per
     `:1146`, invalidates D (dependent on E2's connector instance) while
     retaining U (proved independent of E2). The replacement snapshot
     re-derives winner `unreachable` from U. Row `:1320` rejects (the decisive
     denial set is gone); row `:1322` does not apply (it is keyed to prior
     `exhausted/authoritative_unreachable`, but the prior state is `denied`);
     row `:1323` rejects (an `unreachable` winner is present). Arm A fails;
     arm B's mandate of `blocked` is unsatisfiable against `:1323`'s "no ...
     unreachable winner" constraint. The mandated transition has no legal
     Route projection. Variant (partial persistence, same class): two denied
     edges E1+E2 give decisive set {D1, D2}; an unapproved workspace or
     connector change invalidates only D1 (D2's receipts bind an unaffected
     node/context and are proved independent). The re-derived winner is still
     `denied` via D2, but {D2} cannot match the superseded Route's set-level
     identity/evidence digest, so `:1320` rejects, and `:1323` rejects the
     persisting denied winner. Under either reading of arm B ("the prior
     winner does not persist exactly" -> blocked-but-must-reject; or "no
     higher winner persists at all" -> no arm applies) the assignment is
     must-accept and must-reject, or unclassified.
   - Incorrect/undefined result and consequence: a mandated, never-recoverable
     connector-security transition (`:1105`) has no satisfiable evidence
     projection for reachable snapshots; SC-007/FR-096 totality cannot be
     proved and the checker fails loudly rather than silently mis-modeling.
     The mandatory live family (`:3718-3726`) does not witness the partial
     case, so only the symbolic gate trips. Additionally, "the exact decisive
     record set" is nowhere defined for multi-record winners (one digest over
     the winning-class records is implied by `:1354-1356` but never stated),
     which is itself the kind of underdefinition FR-096 generation cannot
     absorb. Severity is high, not medium, because no single adjacent
     authoritative text forces one resolution: `:1062` speaks winner-level
     ("preserve an exact persisting prior ... winner"), `:1320-1322` speak
     set-digest-level, and `:344-353`'s two arms leave the middle undefined -
     three texts, three readings.
   - Smallest root-cause correction (one file, same region, preserving the
     no-demotion design): make the exception member-wise and winner-derived
     instead of set-wise and prior-keyed. (i) In `:344-353`, replace "the
     exact decisive record set ... remains current with the same identity and
     evidence digest" with: every higher-priority record current in the
     replacement snapshot must be identity- and evidence-digest-matched to a
     record that was current at the superseded terminalization (no new,
     changed, missing, mismatched, or independently derived member may
     participate); the re-derived current winner then owns the replacement
     Route outcome - which is already the composition's stated principle -
     and `blocked` applies exactly when no such higher-priority record
     persists. (ii) Re-key the three preservation rows (`:1320-1322`) by the
     persisting winner class (`unsupported`, `denied`, `unreachable`) with
     prior state in `{denied, exhausted, truncated}`, retaining the
     per-record match constraint and the "no new or mismatched
     higher-priority record" clause. (iii) Extend the `:3718-3726` family to
     cross partial invalidation: subset persistence with the same winner class
     and a class-shift row, each proving the preserved outcome, `excluded`
     state, exact new cause, and no safe selection. (iv) Define "decisive
     record set" (the records of the winning class bound at terminalization)
     in one sentence. No safe outcome becomes derivable; anti-impersonation
     (step 5) is preserved per-record.
   - Verification boundary: FR-096 checker totality over the temporal
     domain; SC-007; SC-017 branch-transition fixtures; the extended
     `:3718-3726` live family.

No other new finding. In particular, no new critical, no new medium, and no
new low finding survived re-derivation of the corrected regions and their
neighbors.

## Structural and Cross-Model Regression Result

- **Table-shape gate: PASS.** Independent escaped-pipe-aware character-scan
  cell counting over every table row: `scenario-matrix.md` 54 tables,
  `spec.md` 5 tables, zero cell-count mismatches. Under the prior reviews'
  row convention (header + delimiter + data) the total is 863 rows vs the
  re-review's 860 - the delta is exactly the three new preservation rows at
  `:1320-1322`, confirming the edit is confined to the H1/M1/L1-L3 regions.
- **Machine-readability token gates: PASS.** Zero occurrences of
  `preterminal`, `lease-eligible`, `EvidenceModel` (other than
  `EvidenceBoundaryModel`), the `catalog` misspelling, or the legacy effect
  ids (`queue_or_gate`, `snapshot_name_omitted`, `execution_name_rejected`).
  The surface and private counter tokens remain distinct
  (`exact_surface_name_scope_limit_and_counter` x3,
  `exact_scope_limit_and_counter` x5).
- **Items 2-9 of the prior re-review: still closed after regression check.**
  Spot re-verification on current bytes: preliminary/final native-name domains
  identical seven-value sets with `unproved` evidence-only (`:1962`,
  `:2956-2958`, tuple `:2101-2109`); endpoint-binding activity (`:2005`,
  `:2186-2188`); the same-incident bundle combination row (`:2331`) and
  pre-decision pairing rule with the disposition-conflict rejection
  (`:2038-2055`); decoder replay no-receipt derivation (`:2313`,
  `:2685-2687`); non-audit mismatch source-total row and receipt effect
  (`:2261`, `:2539-2541`); the five live families (retention purge/saturation
  `:3760-3765`, `retired` dispatch denial `:3819`, bound registry
  `:3734-3748`, census `:3755-3759`, `operator_local_composite`
  `:3651-3654`); proof-invalidation function completeness (`:1221-1253`);
  effect ids matching role names (`:3187-3192` vs `:3110-3132`);
  `bound_before_route` in the `total_time` row (`:3119`); census marker
  closure (`:3053-3056`) and the distinct `Census reached-dimension set`
  (`:3030`) vs independent `Reached-dimension set` (`:3085`). No regression.
- **Cross-model pass over the six models:** the corrected regions and their
  consumers were re-walked. The preservation outcomes flow correctly into
  Goal aggregation (an excluded branch's preserved `denied`/`unsupported`/
  `unreachable` Route outcome enters the presence vector and aggregates under
  clauses 6-9 at `:3600-3608`, matching `spec.md:1286-1293`, including
  `blocked + denied -> blocked` and `unknown + denied -> unknown`); Route
  effects remain total (`replace_terminal` when a current row exists,
  `:1269-1277`); supersession keeps at most one current Route per branch with
  provenance-only history (`:1345-1356`); the composition introduces no
  receipt cycle (receipt -> batch -> Route; records -> winner -> outcome) and
  no safe outcome or work authority. Beyond Finding N1, no new overlap,
  uncovered reachable assignment, circular authority, unsafe fail-open
  behavior, impossible mandatory fixture, or GREEN-capable coverage omission
  was found.

## Claims Independently Re-confirmed Against Code and Git

1. HEAD constitution is v1.0.0 in a single commit (`958c502`); `git log --all
   -S "2.0.0"` over the file returns nothing. The worktree 2.1.0 Sync Impact
   Report's "no intermediate 2.0.0 constitution was committed" is accurate.
2. Manual SQL migration runner with refinery deliberately unlinked:
   `crates/store/src/lib.rs:56` ("Manual runner because refinery 0.9" pins
   rusqlite), `Cargo.toml:52` and `crates/store/Cargo.toml:18` (commented
   out), zero `refinery` matches in `Cargo.lock`.
3. rusqlite WAL mode live (`crates/store/src/lib.rs:646`) with the WSL2
   9P/drvfs guard (`:155`, `:697`).
4. MSRV `rust-version = "1.92"` (`Cargo.toml:20`); rmcp `=1.8.0` with
   `transport-io` (`Cargo.toml:49`). All match the constitution's Additional
   Constraints.
5. `spec.md` is byte-identical to the re-reviewed state (digest match), and
   its entailment table (`spec.md:1260-1293`) is consistent with - indeed
   requires - the preservation direction the H1 correction chose.

## Missing Verification (recorded as absence, never success)

- The three cited platform clock contracts (Windows QueryPerformanceCounter
  standby/hibernate/connected-standby inclusion, Linux `CLOCK_BOOTTIME`,
  macOS `mach_continuous_time`) were not fetched or re-verified against live
  vendor documentation in this review; they remain the implementation-test
  obligations the registry itself imposes (`scenario-matrix.md:3095-3104`),
  with fail-closed `clock_continuity_unproved` handling bounding the risk at
  specification level.
- AAP Firecracker/vsock consumer compatibility, the FR-095 manual onboarding
  baselines, the FR-100 frozen measurement artifacts, the FR-050 independent
  official comparator reference corpus, and the shipped registries that
  generate the live universe still do not exist; they remain explicit
  pre-planning or implementation gates, unchanged from the prior reviews.
- No checker, parser, or fixture generator was executed beyond the read-only
  table-shape scan; FR-096 buildability - including the Finding N1 totality
  failure - is assessed analytically only.
- `.specify/memory/constitution.md`, `.specify/feature.json`, and the three
  specification files remain uncommitted worktree state; the reviewed state is
  a worktree state, not a committed tree.

## Gate Conclusion: NOT SAFE TO COMMIT - /speckit-plan remains blocked

`GREEN` requires no unresolved high contradiction and no unclassified or
multiply classified reachable assignment. Finding N1 is exactly such an
assignment class on a mandated connector-security path, so this review cannot
authorize committing the corrected governing milestone yet. Precisely:

- Blocking correction: Finding N1 (`scenario-matrix.md:344-353` and
  `:1320-1323`, plus the `:3718-3726` fixture cross) - one narrow, mechanical
  correction in the same file, in the member-wise/winner-derived direction
  stated above, which is the composition's own declared principle ("the
  current winner owns only outcome") applied consistently.
- All five reviewed corrections (H1 as designed for its two named cases, and
  M1/L1-L3 in full) are verified and need no further work; items 2-9 remain
  closed; the structural and token gates are clean; the constitution's
  code-facing claims hold.
- After the N1 correction, a fresh narrow verification of that region on a
  re-fingerprinted stable state, and the milestone commit, `/speckit-plan`
  may begin. Until that commit exists, planning remains blocked per the
  standing milestone gate.
