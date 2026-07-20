# Fable Re-review: Environment-Probe Corrected State

Reviewer: Claude (Fable 5), 2026-07-20. Narrow independent re-review per
`docs/reviews/2026-07-20-environment-probe-fable-rereview-brief.md`. Code and
current disk bytes were treated as authority; no correctness was inferred from
the follow-up disposition, prior GREEN claims, or author intent.

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
  - `?? docs/reviews/2026-07-20-environment-probe-fable-followup-brief.md`
  - `?? docs/reviews/2026-07-20-environment-probe-fable-followup-disposition.md`
  - `?? docs/reviews/2026-07-20-environment-probe-fable-followup-review.md`
  - `?? docs/reviews/2026-07-20-environment-probe-fable-rereview-brief.md`
  - `?? specs/003-environment-probe/`
- Pre/post manifest and digest match: YES. The recursive regular-file manifest
  under `specs/003-environment-probe` and all SHA-256 digests below were
  identical before analysis and again immediately before this verdict. No
  reviewed byte changed during the review. The only file written is this one.

### Complete Input Manifest (SHA-256, identical pre and post)

| Relative path | SHA-256 |
|---|---|
| `specs/003-environment-probe/checklists/requirements.md` | `7a9c19deea43aab636227ebe03e4bcfbd9b7163abdaa35aa3059663740f60d2e` |
| `specs/003-environment-probe/scenario-matrix.md` | `5f5da594759726f0e5d654d1c548edccf33893d80ba7cdca9af118cc5c527ea9` |
| `specs/003-environment-probe/spec.md` | `df6f6170e82f16f0b501aecd7ae08a7391af94a2dcdf4f86d47f8bcdd0679d53` |
| `.specify/memory/constitution.md` | `594e4e9126681d5fd10d982d91e382224a33499e22a09bb9e0d4781bcb8227b2` |
| `.specify/feature.json` | `c6e6d35dc14946e4156765647ac64b02bd318405c7c5a410e1f4927d033d52d0` |
| `docs/reviews/2026-07-20-environment-probe-fable-followup-review.md` | `06bb151bbe9d4391fa767e512339c6bea7aec20c7e7c42b175cb68e73976260a` |
| `docs/reviews/2026-07-20-environment-probe-fable-followup-disposition.md` | `338e6f4254a57c02296e8296015326defa33f69476ae84abf73a1745ef969cdc` |

The three files above are the complete recursive regular-file set under
`specs/003-environment-probe`, including subdirectories.

## Reading and Verification Method

- Every byte of every file in the manifest was read directly by this reviewer:
  `spec.md` (all 1476 lines), `scenario-matrix.md` (all 3764 lines, in eight
  contiguous windows: 1-600, 601-1050, 1051-1490, 1491-1930, 1931-2360,
  2361-2790, 2791-3220, 3221-3764), `checklists/requirements.md` (49 lines),
  `.specify/memory/constitution.md` (234 lines), `.specify/feature.json`
  (3 lines), the follow-up review (690 lines), and the follow-up disposition
  (140 lines). No delegate readers were used; there are no delegated byte
  ranges.
- Code and Git verification used raw read-only reads, `grep`, `git show`,
  `git log`, and `git log -S` only. SymForge retrieval was deliberately not
  used for this review: its indexer maintains a store, and the brief forbids
  commands that can mutate stores; raw reads were the conservative substitute.
- The Markdown-table structural gate was reproduced independently with a
  character-scan cell counter (escaped `\|` excluded, no regex) over every
  table row in both specification files; no file was written by that check.
- The 2026-07-17 historical review and disposition were not consulted.

## Verdict: CONTESTED

One new HIGH finding remains: the corrected terminal evidence-projection still
cannot represent a mandated hard exclusion from a terminal prior whose
`denied` or `unreachable` winner evidence persists (Finding H1). It is the
residual half of the previously corrected Finding 1: the state bridge and batch
rows were fixed, but the evidence-winner algebra was not extended to match.
Reproduction-matrix items 2 through 9 all verified as closed. No critical
finding: no unsafe disclosure, authority bypass, fail-open path, or
foundational contradiction was found; the security posture is coherent and
every code-facing premise checked was true.

## Required Reproduction Matrix Dispositions

1. **Terminal Route evidence projection - PARTIALLY VERIFIED; NEW HIGH
   (Finding H1).** The bridge (`scenario-matrix.md:1275-1281`) and batch row
   (`:1040`) now accept every declared prior state, including
   `denied|exhausted|truncated` for a later hard exclusion, and the bound row
   (`:1296`) accepts `timed_out + fresh` ("hard observation
   `timed_out|truncated` or hard freshness non-fresh") without overlapping any
   other outcome, because projection rows are keyed by disjoint
   state/cause pairs and the supersession rule (`:1313-1320`) removes the
   prior Route record from the Goal presence vector. However, independent
   re-derivation of the winner algebra shows the excluded-state projection
   (`:1291-1292`) is still unsatisfiable for two mandated prior-state classes;
   see Finding H1. The item therefore does NOT fully verify.
2. **Preliminary/final native-name and surface-name domains - VERIFIED.**
   The preliminary verdict domain (`scenario-matrix.md:1918`) and the final
   `native_name_admission` (`:2912-2914`) are now the identical seven-value
   closed set; `unproved` is exclusively an evidence/observation state
   ("unproved classification evidence derives `unknown` and is never a verdict
   value", `:1918`; mirrored at `:2940-2942`). The closed
   `surface_name_application` tuple (`:2057-2065`) was exhaustively enumerated
   against preliminary {3 canonical, omitted, rejected, unknown, NA} x
   observation {exact, not_reached, missing, mismatch, replayed, unproved,
   NA}: every pair matches exactly one row (rows 1-3 receipt-emitting; row 4
   unknown/no-receipt; row `:2063` unproved -> unknown/no-receipt; row `:2064`
   missing/mismatch/replayed -> rejected; residual `:2065` structural). No
   dead or unrepresentable `unproved` value remains.
3. **Endpoint-binding activity - VERIFIED.** The domain (`:1961`) now states
   the axis is "active for authenticated-local or cross-boundary non-campaign
   overlay transport/persistence... same-process and no-overlay rows require
   `not_applicable`". This is consistent everywhere it is consumed:
   same-process overlay tuple requires `not_applicable` (`:2142`),
   authenticated-local and connector/guest tuples require
   `exact_origin_and_final_target_route` (`:2143-2144`), persistence subrows
   match per crossing (`:2390-2392`), and the source-total rows (`:2224-2226`)
   correctly mark the probe-campaign column structurally unreachable.
4. **Ordered channel combination function - VERIFIED.** Row
   `scenario-matrix.md:2287` accepts exactly
   `many + exact_same_incident_bound_effect_bundle` with complete distinct
   member-key coverage and one shared incident/scope/bundle identity,
   preserves every member's exact `bound_reached` disposition, and consumes
   the complete member receipt set into one atomic
   `bound_effect_bundle_commit`. Independent, mismatched, conflicting, or
   incomplete multi-source shapes remain rejected by row `:2286` and the
   pre-decision pairing rule (`:1996-2011`), which also rejects a covered
   source paired with `zero` and names the private-input-stop-vs-replay
   disposition conflict explicitly.
5. **Private-input counter replay - VERIFIED.** The replay-causality
   partition (`:2269`) assigns counter-observation `replayed` the deliberate
   no-receipt rejection, and the fixed-helper-decode derivation now matches:
   "A replayed private-input counter observation derives
   `decoder_admission = rejected`, `receipt_effect = no_receipt`, and causal
   `zero + not_applicable`" (`:2640-2643`), while only structural/exceeded/
   missing/mismatch shapes retain `receipt_effect = rejected` (`:2643-2645`).
   The resolver derivation (`:2566-2568`) does not contradict this (it assigns
   the property decision only; the partition owns the receipt effect).
6. **Non-audit receipt mismatch - VERIFIED.** The source-total table now has
   the applicable non-audit mismatch row (`:2217`) covering every subclass
   including `private_payload_binding_mismatch`, preserving the subclass in a
   non-authorizing receipt, committing the exact terminal sensor `rejected`
   result when applicable, and fabricating no connector hard exclusion; the
   inactive-field/audit-axis occurrences are correctly split out as structural
   no-receipt rejections (`:2228-2233`). Causal counting covers it as a
   "reachable receipt-emitting source-total row" (`:1962`), the receipt effect
   is `issued_exact_non_authorizing_effect` (`:2495-2498`), and the single-
   source combination row (`:2288`) carries the campaign-local rejection cell
   added by the disposition. The live families now require mismatch and replay
   fixtures at every owning property (`:3676-3680`). One residual wording
   contradiction inside `:2288` is Finding M1.
7. **Mandatory live families - VERIFIED.** All five follow-up omissions are
   now present: (a) retention purge/saturation family with timer and capacity
   paths, deterministic victim selection, protected/continuity-unproved
   no-deletion, final-partition fold, and the saturated no-victim wake
   (`:3702-3707`); (b) `retired` beachhead-proof dispatch denial
   (`:3757-3761`); (c) the bound-registry family crossed with
   below/reached/exceeded/unproved/missing/mismatch states plus explicit
   admission `rejected_bound`, queue wait/wake, and branch/campaign effects
   (`:3681-3693`); (d) targeted and full names census with counters, markers,
   consent exact/absent/mismatch/replay, and substitution rejection
   (`:3697-3701`); (e) `operator_local_composite` selection/composition
   (`:3607-3608`). One LOW enumeration gap remains (Finding L1).
8. **Constitution lineage and persistence-stack claims - VERIFIED against Git
   history and code.** `git show HEAD:.specify/memory/constitution.md` carries
   `**Version**: 1.0.0` (single commit `958c502`); `git log --all -S "2.0.0"`
   over the file returns nothing, matching the worktree Sync Impact Report's
   "1.0.0 -> 2.1.0... no intermediate 2.0.0 constitution was committed". The
   persistence claim now matches code exactly: manual migration runner because
   "refinery 0.9 transitively pins rusqlite <= 0.38"
   (`crates/store/src/lib.rs:56-58`), refinery commented out in `Cargo.toml:52`
   and `crates/store/Cargo.toml:18` with zero matches in `Cargo.lock`; rusqlite
   WAL mode is real (`crates/store/src/lib.rs:646`, WSL 9P guard `:155`); MSRV
   `rust-version = "1.92"` (`Cargo.toml:20`); rmcp 1.8.0 stdio
   (`Cargo.toml:49`). The amendment itself remains uncommitted worktree state,
   which is the follow-up disposition's still-open milestone gate, not a new
   finding.
9. **Finding 11 machine-readability items - VERIFIED closed.** Independent
   structural scan: 54 tables / 798 rows in `scenario-matrix.md` plus 5 tables
   / 62 rows in `spec.md` (59 tables, 860 rows total, matching the disposition
   exactly) with zero cell-count mismatches under escaped-pipe-aware counting.
   Token/naming greps: no `preterminal`, no `lease-eligible`, no
   `EvidenceModel`, no `catalog` spelling, no legacy effect ids
   (`queue_or_gate`, `snapshot_name_omitted`, `execution_name_rejected`); the
   surface and private counter tokens are now distinct
   (`exact_surface_name_scope_limit_and_counter` at `:1919` vs
   `exact_scope_limit_and_counter` at `:1925`); proof-validation causes are in
   the proof-invalidation function (`:1213-1216`); effect ids match role names
   (`:3072`, `:3076`, `:3146-3148`); `bound_before_route` is used correctly in
   the `total_time` row (`:3075`); census marker mapping is closed
   (`:3009-3012`); the census axis is renamed `Census reached-dimension set`
   (`:2986`) distinct from the independent-bound axis (`:3041`); Goal
   aggregation clause 1 has no dead disjunct (`:3548-3549`); the
   persistence-policy provenance table has three cells in every row
   (`:2370-2380`); the bound-bundle sentence has its subject (`:1019`).

## Re-adjudications From First Principles

- **Prior MEDIUM retry claim: RESOLVED, no finding.** OperationModel is an
  ordered classifier whose clauses override named fields
  (`scenario-matrix.md:1680-1682`). Clause 2 (`:1733-1738`) derives the
  precommit key classification with "No key mutation has occurred yet";
  clause 4 now states the postcommit override inline: "A successfully
  committed new key changes state to `present_matching`, effect
  `bound_atomically`, and `retry=resolved` with the exact bounded idempotency
  horizon" (`:1765-1767`), and the horizon projection (`:1686-1697`) is a
  single total override. The final wire value for a committed keyed admission
  is uniquely `retry=resolved`. The earlier contradiction claim does not
  reproduce on the current bytes.
- **OperationModel candidate: fresh alias on a terminal campaign - NOT a
  totality defect.** A `start` with lookup `found`, prior state
  `completed|cancelled|failed|expired`, key `present_new`, and relation
  `identical_shareable` matches no start-admission partition row
  (`:1546-1555`: the alias rows require prior `accepted|running`, and the
  conflict row's `present_new` arm requires a non-identical relation), so it
  retains the complete rejected default tuple (`:1505-1507`, `:1673-1684`)
  with no key or campaign mutation, exactly as row `:1549` demands
  ("terminal/history campaigns never accept a fresh alias"). Total and
  deterministic; the only cost is an untyped `rejected` for a plausible caller
  mistake, which is an ergonomics choice, not a defect.
- **OperationModel candidate: queue-gate scope - NOT a contradiction.** The
  validity clause now states its scope explicitly: "(a) a successful
  new/independent start whose campaign is durably accepted... or (b) an
  authorized existing running campaign whose exact `sensor_admission`
  capacity check reached before spawn" (`:1482-1489`), and clause 4 rejects
  terminal or `active_running` wait outcomes with a queue gate on the
  new/independent row while naming the existing-running row as separately
  valid (`:1817-1820`). One residual wording gap for an authorized
  existing-`accepted` attachment is Finding L2.

## New Findings

1. **[high] H1: Excluded-from-terminal-prior Route records are unrepresentable
   whenever the prior `denied` or `unreachable` winner evidence persists**
   - Model/domain: RouteModel winner algebra x TransitionModel bridge; the
     residual half of previously corrected Finding 1.
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:317-318`
     (aggregate priority: `denied`, then `unreachable`, then `missing`, then
     `incompatible`); `:330-338` (joint winner order: (2) denied and (3)
     unreachable precede (4) missing|incompatible; "Lower-priority hard...
     records remain in provenance but cannot override the winner");
     `:340-349` (closed winner-to-event mapping: hard denial ->
     `policy_denied/policy_decision`, hard unreachable ->
     `sensor_exhausted/authoritative_unreachable`; "No Common/Node/Edge
     assignment can be paired with a different terminal event or bridge
     projection"); `:236-237` (a structural `denied`/`unreachable` record
     requires freshness `fresh` and its exact witness); `:1040` + `:1083`
     (the hard-exclusion batch and cause partition MANDATE
     `terminal_branch_commit/hard_excluded` from prior
     `denied|exhausted|truncated`, installing/replacing "exact terminal Route
     `blocked`"); `:1291-1292` (the only excluded projection rows require "no
     unsupported/denied/unreachable winner; hard aggregate
     `missing|incompatible`"); `:1311-1313` ("Every other terminal
     state/cause/evidence tuple is a rejected checker constraint");
     `:1318-1320` (supersession covers the prior ROUTE record only);
     `:215-216` (mandatory topology records are re-projected from every
     Common/Node/Edge assignment, so a still-`denied` edge decision re-derives
     a fresh hard `denied` record); `:751` + `:1124` (evidence invalidation is
     mandated only for authoritative-context CHANGES - replay and
     audit-integrity causes are not context changes and invalidate nothing);
     `:2174` (nonce/sequence replay MUST be consumed as hard exclusion
     `connector_replay_detected`); `spec.md:1199-1201` (branch table legalizes
     `denied`/`exhausted` -> `excluded` on a later hard exclusion);
     `spec.md:776-780` (FR-062: replay et al. "terminate the affected probe
     branch as distinct hard exclusions").
   - Concrete counterexample: branch B reaches terminal `denied` (edge
     governing decision denied; fresh `complete_policy_receipt` record per
     `:236`). Within the freshness window a sequence replay is detected on
     B's authenticated channel. `:2174` and FR-062 mandate hard exclusion
     `connector_replay_detected`; `:1040`/`:1083` mandate
     `terminal_branch_commit/hard_excluded` from prior `denied`, installing
     Route `blocked`. Replay is not an authoritative-context change, so
     nothing invalidates the denial record; the replacement snapshot
     re-derives hard `denied`, the aggregate (`:317-318`) and joint winner
     (`:332`) select `denied` over the new `incompatible` exclusion record,
     the winner mapping (`:341-342`) derives `policy_denied/policy_decision` -
     which the bridge (`:1278`) rejects from prior `denied` - and the mandated
     Route record (excluded/`connector_replay_detected`) fails row `:1291`
     ("no... denied... winner; hard aggregate `missing|incompatible`") and is
     rejected by `:1311-1313`. The same shape reproduces for prior
     `exhausted/authoritative_unreachable` (fresh
     `complete_bounded_attempt_receipt`, `:237`) with any later hard exclusion
     that does not invalidate it: the persisting `unreachable` winner (order
     (3)) blocks row `:1291` identically. Only prior `truncated` (winner
     class (5)) and change-carrying exclusions that happen to invalidate the
     old winner via `:1124` escape.
   - Incorrect/undefined result and consequence: the same transition is
     simultaneously must-accept (`:1040`, `:1083`, `:2174`, spec branch table)
     and must-reject (`:1291` + `:1311-1313`), and the closed winner-to-event
     mapping derives a different event than the accepted batch. The FR-096
     exhaustive checker cannot satisfy both; SC-007 totality and SC-017
     branch-transition fixtures are blocked on a mandated connector-security
     path. This is exactly the destructive ambiguity class the matrix exists
     to prevent.
   - Smallest root-cause correction (pick one, state which):
     (i) mandate in the hard-exclusion supersession batch that the prior
     terminal winner records are demoted to provenance (an explicit
     `source_fact` supersede/invalidate member for the superseded branch's
     decisive records, added to the fact-effect table `:1114-1127` for the
     non-change security causes too), so the replacement snapshot derives
     winner `missing|incompatible` and rows `:1291-1292` become total - this
     preserves the current excluded -> `blocked` outcome; or
     (ii) add excluded projection rows accepting a persisting
     `denied`/`unreachable` winner for terminal priors with a
     connector-security/audit-integrity cause, define their derived Route
     outcome (spec.md:1260-1262 entailment order argues `denied` before
     `blocked`), and add a supersession carve-out to the winner-to-event
     closure at `:348-349`.
   - Verification boundary: FR-096 checker totality; SC-017 exhaustive
     branch-transition fixtures; the excluded-from-terminal-prior rows of the
     mandatory live families.

2. **[medium] M1: The single-source combination row forbids the Transition its
   own cell requires**
   - Model/domain: SecurityPropertyModel ordered channel combination function.
   - Evidence: `scenario-matrix.md:2288` effect cell: "...campaign-local
     property rejection (including the exact terminal sensor `rejected` result
     when applicable, bounded fallback only, and no Transition)...". A
     terminal sensor `rejected` result commits exclusively through the
     `sensor_observation_commit/sensor_result_recorded` TransitionBatch
     (`:894-917`), and every admitted sensor MUST receive exactly one durable
     terminal disposition through a Transition (`:935-939`). The authoritative
     source-total rows say it precisely: "no connector hard exclusion is
     fabricated" (`:2217`) and "no hard-exclusion Transition is fabricated"
     (`:2219`).
   - Concrete counterexample: a spawned sensor's producer receipt correlation
     is `sensor_class_mismatch` (single covered source). Row `:2217` and
     `:2288` require the terminal sensor `rejected` result; a generator that
     takes `:2288`'s "no Transition" literally must not commit the
     `sensor_observation_commit` batch, violating `:935-939`; one that commits
     it violates `:2288`.
   - Consequence: a mechanical FR-096 generator hits a direct row-vs-row
     conflict. Severity is medium, not high, because the model's own
     harder constraint (`:935-939`) plus `:2217`/`:2219` force exactly one
     resolution, so the defect cannot silently produce a wrong model - it
     fails loudly or is resolved correctly.
   - Smallest correction: in `:2288`, replace "and no Transition" with "and no
     hard-exclusion Transition" (mirroring `:2219`).
   - Verification boundary: FR-096 model generation from the combination
     function table.

3. **[low] L1: The bound-registry live family's enumeration omits dimensions
   its own header claims to cover**
   - Evidence: `scenario-matrix.md:3681-3687` opens "every finite dimension
     generated from the frozen scope-aware bound registry, not a hand-selected
     subset:" but the colon list omits `result_detail` (FR-072, spec.md:920;
     dimension row `:3082`) and `retained_records` (`:3087`); neither token
     appears anywhere in `:3573-3764` (grep-verified). Retention dimensions
     have their own family (`:3702-3707`), and the generator clause governs
     the universe, so no coverage hole is forced - but the incomplete
     enumeration is the exact hand-maintained-table fragility Finding 11
     targeted.
   - Smallest correction: add the missing members to the list or mark the list
     explicitly non-exhaustive ("including").
   - Verification boundary: FR-084 generated required-key set.

4. **[low] L2: Queue-gate validity does not name the existing-accepted
   attachment row that clause 7 accepts**
   - Evidence: `scenario-matrix.md:1482-1489` limits the queue-gate input to
     (a) a successful new/independent start (accepted) and (b) an authorized
     existing RUNNING campaign, while clause 7 (`:1868-1871`) says "For an
     accepted campaign it preserves state `accepted`" for a gate arriving
     "after a primary mapping for a successfully admitted campaign or an
     authorized existing-campaign attachment". A matching-key start, resume,
     or detail attaching to an existing still-`accepted` campaign with a
     durable pending campaign-start wait is in neither (a) nor (b), so whether
     that response may carry `queued_in_progress` is stated in two different
     ways. Harmless either way (no spawn, no mutation; only the annotation
     differs), but fixtures and clients can disagree.
   - Smallest correction: extend `:1482-1489` with the authorized
     existing-accepted attachment form (or state that such reads return the
     plain in-progress receipt with `bound_effect=none`).
   - Verification boundary: OperationModel queue-gate fixtures (SC-021).

5. **[low] L3: "terminal unknown with truncation" is the only discriminator
   between two terminal events, and row `:1295` retains a dead disjunct**
   - Evidence: the winner-to-event mapping distinguishes
     `bound_reached/bound_reached` from
     `sensor_exhausted/evidence_unavailable` only by the phrase "terminal
     unknown with truncation" (`scenario-matrix.md:345-347`). Because hard
     `timed_out|truncated` observations can only be created by the
     branch-decisive bound compositions (`:930-933`), which terminalize the
     branch immediately, the `timed_out` half of row `:1295`'s "hard
     observation `probe_failed|timed_out`" disjunct is unreachable in a live
     exhaustion row. Acceptance-set slack, not a contradiction (the bridge
     rejects unproduced tuples), but the undefined phrase is load-bearing for
     a generator.
   - Smallest correction: define "with truncation" as "hard observation
     `timed_out|truncated`" (or as the presence of the bound receipt) at
     `:345-347`, and drop `timed_out` from `:1295`.
   - Verification boundary: FR-096 winner-mapping generation.

## Fresh Adversarial Pass Record

- All six models were re-walked on the current bytes with the corrected
  regions and their neighbors re-derived from first principles. Beyond H1/M1/
  L1-L3, no new counterexample, overlap, hole, circular receipt, unsafe
  fail-open state, impossible mandatory path, or GREEN-capable coverage
  omission was found. Spot re-derivations that PASSED include: the
  surface-name tuple totality enumeration (item 2); the two staged
  cross-model compositions remain acyclic as declared (`:36-44`, `:941-1001`,
  `:3196-3210` - the postcommit Operation receipt is never an input to its
  own Transition); the causal cardinality/correlation pairing including the
  disposition-conflict rule (`:1996-2011`); goal aggregation vs the spec
  entailment table for {denied,blocked}, {unknown,denied}, zero-route
  `bound_before_route`, and the ready/ready_with_warnings/in_progress
  boundary (`:3546-3564` vs `spec.md:1286-1293`); the sensor-class cap and
  connector-cap functions against the spec taxonomy (`:2813-2847` vs
  `spec.md:1212-1252`); FR-052's five terminal observation statuses vs the
  RouteModel record domain (`:190`); the OperationModel twelve-output tuple
  (`:1647-1684`); and the overlay-migration retry/quarantine algebra vs
  FR-058 (`:753`, `:774-786`, `:1054`).
- Platform-clock table (`:3055-3060`): it claims NO unsupported inheritance.
  A WSL verifier must bind the guest kernel's `CLOCK_BOOTTIME` identity
  "rather than the Windows host clock"; other guests/remote verifiers require
  a target-native registered source with "parent, sender, and wall clocks...
  forbidden substitutes"; every row fails closed to
  `clock_continuity_unproved` on unavailability or contract mismatch, and the
  registry demands implementation-tested mappings bound to the exact OS/API
  revision. The strongest external claim - that the QueryPerformanceCounter
  contract includes standby, hibernate, and connected-standby time - was not
  re-verified against the cited vendor documentation in this review (recorded
  under Missing Verification); the implementation-tested requirement plus the
  fail-closed mismatch path bound the risk at specification level.

## Claims Independently Confirmed Against Code and Git

1. HEAD constitution is v1.0.0, committed once (`958c502`); no 2.0.0 version
   ever existed in history (`git log --all -S "2.0.0"` on the file is empty).
   The worktree 2.1.0 lineage statement is therefore accurate.
2. Manual SQL migration runner with refinery deliberately unlinked:
   `crates/store/src/lib.rs:12-13` and `:56-58` (comment states refinery 0.9
   pins rusqlite <= 0.38), `Cargo.toml:52` and `crates/store/Cargo.toml:18`
   (commented out), zero `refinery` matches in `Cargo.lock`.
3. rusqlite WAL mode live (`crates/store/src/lib.rs:646`,
   `PRAGMA journal_mode=WAL`), with a WSL2 9P/drvfs safety guard (`:155`).
4. MSRV `rust-version = "1.92"` (`Cargo.toml:20`), matching the constitution's
   "1.92.0 at ratification".
5. MCP layer rmcp `=1.8.0` with `transport-io` (stdio) (`Cargo.toml:49`),
   matching the constitution and the recent rmcp-1.8 upgrade commit.

## Missing Verification (recorded as absence, never success)

- The three cited platform clock contracts (Windows QPC standby/hibernate
  inclusion, Linux `CLOCK_BOOTTIME`, macOS `mach_continuous_time`) were not
  fetched or re-verified against live vendor documentation; they remain
  implementation-test obligations that the table itself imposes.
- AAP Firecracker/vsock consumer compatibility, the FR-095 manual onboarding
  baselines, the FR-100 frozen measurement artifacts, the FR-050 independent
  official comparator reference corpus, and the shipped registries that
  generate the live universe still do not exist; they remain explicit
  pre-planning or implementation gates, unchanged from the follow-up review.
- No checker, parser, or fixture generator was executed beyond the read-only
  table-shape scan; FR-096 buildability is assessed analytically only.
- `.specify/memory/constitution.md`, `.specify/feature.json`, and the three
  specification files remain uncommitted worktree state. The follow-up
  disposition's final gate 3 (commit the whole governing milestone before
  planning) is therefore still open; the reviewed state is a worktree state,
  not a committed tree.

## Final Planning Gate: NOT READY - /speckit-plan may NOT begin

- Blocking correction: Finding H1 (`scenario-matrix.md:1291-1292` evidence
  domain vs `:317-349`, `:1040`, `:1083`, `:2174` - one narrow, mechanical
  correction in the same file, in either direction (i) or (ii) above).
- Also required before planning, per the standing milestone gate: commit the
  governing specification/constitution milestone on a re-fingerprinted stable
  state and pass a fresh narrow verification of the H1 correction.
- M1 and L1-L3 should ride along with the H1 correction; none of them alone
  would block GREEN, because each has exactly one resolution forced by
  adjacent authoritative text and none can silently invalidate machine
  generation, security/lifecycle behavior, or this gate - the required
  explanation for why they could otherwise coexist with GREEN.
- Items 2-9 of the reproduction matrix are closed and need no further work.
  After the H1 correction (with M1/L1-L3 folded in) and the milestone commit,
  this specification is one narrow re-verification away from GREEN.
