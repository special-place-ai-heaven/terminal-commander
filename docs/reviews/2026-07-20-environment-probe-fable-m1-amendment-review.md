# Fable M-1 Amendment Gate Review: Environment and Prerequisite Probe

Independent clean-room review of the single post-GREEN M-1 amendment to the
environment-probe governing specification, executed per
`docs/reviews/2026-07-20-environment-probe-fable-m1-amendment-brief.md`.
Current disk bytes, Git history, and the declared model were the authorities.
The full-feature GREEN verdict at commit `77e2fd0` was an input to challenge,
not an inherited conclusion; this review verified the amendment narrowly and
did not repeat the full-feature campaign. This is a read-only review; this
file is the only mutation performed.

## Verdict: GREEN

The two-line amendment exactly closes finding M-1 of the full-feature final
review, leaves every other committed byte of the whole-feature contract
unchanged (hash-proved), reopens no full-feature defect, leaves no reachable
unclassified or multiply classified assignment in the amended arm, and
introduces no machine-readable ambiguity that can alter a conforming
implementation. No critical, high, or medium finding. Three low editorial
observations are each proved unable to change conformance. All seven
mandatory reproductions verified. Reviewed bytes were stable across the
entire review.

## Stable-State and Provenance Fingerprint (pre == post)

Recorded before substantive review and re-verified immediately before this
verdict; all values identical at both samplings.

- Repository root: `E:/project/terminal-commander`
- Branch: `main`
- HEAD: `77e2fd0bbfbe453c89ca6c48c091bb0493efa7d1`
  (`docs(spec): define goal-directed environment probe`)
- `git status --short`, byte-identical pre and post (2 entries):
  ` M specs/003-environment-probe/scenario-matrix.md` and
  `?? docs/reviews/2026-07-20-environment-probe-fable-m1-amendment-brief.md`.
  This review file is the single permitted post-review difference.

| SHA-256 | Bytes | Lines | Path |
|---|---|---|---|
| `c319b92f2ed8f9193326032e88f4d7520905039b656872eabd83ceaac32ef56b` | 397343 | 3899 | `specs/003-environment-probe/scenario-matrix.md` (worktree, amended) |
| `633be34a21aa4b37f6a8f28de065486710b600aee894f10db2fad7e12ad8fbea` | 397269 | 3899 | `specs/003-environment-probe/scenario-matrix.md` (blob at `77e2fd0`, base) |
| `b1d0390178482993f0a86360347c27a9571a2fa142e41dd0e0518b5a07d505ef` | 6223 | 122 | `docs/reviews/2026-07-20-environment-probe-fable-m1-amendment-brief.md` |
| `630b5dcfbafb90cea230140f81e4fcd0d6b5232d15b0651c94abdb81377a25e8` | 43905 | 670 | `docs/reviews/2026-07-20-environment-probe-fable-full-feature-final-review.md` |

Independently established brief facts, all confirmed:

- base governing commit `77e2fd0` is HEAD of `main`;
- base `scenario-matrix.md` SHA-256 equals the brief's declared
  `633be34a...` (recomputed from `git show 77e2fd0:...`);
- current amended `scenario-matrix.md` SHA-256 equals the brief's declared
  `c319b92f...`;
- `git diff --numstat 77e2fd0` reports exactly `2 2` for
  `specs/003-environment-probe/scenario-matrix.md` and no other tracked
  change, pre and post.

## Exact Diff Evidence

The tracked diff from `77e2fd0` is two additions and two deletions: line 1088
(the `campaign_bound_stop` / `bound_reached` table row) and line 3813 (the
deadline/completion race fixture family). Word-level token enumeration
(`git diff --word-diff`), complete:

1. Line 1088, Route-bearing arm guard: `a nonempty all-terminal ownership
   set,` became `a nonempty ownership set containing no open branch,`.
2. Line 1088, member disposition: `each terminal branch becomes retracted`
   became `every owned branch in {ready, denied, excluded, exhausted,
   truncated, cancelled} becomes retracted`.
3. Line 1088, adjacent phrasing alignment: `already retracted branches
   remain unchanged` became `every already-retracted branch remains
   unchanged` (now token-identical to the zero-Route arm's existing clause
   later in the same row; same meaning, universal quantification made
   explicit).
4. Line 1088, effect-column arm label: `the Route-bearing all-terminal arm`
   became `the Route-bearing zero-open arm`.
5. Line 3813, fixture-label rename: `the Route-bearing all-terminal
   campaign_bound_stop arm` became `the Route-bearing zero-open
   campaign_bound_stop arm`.

Edits 1-4 are one line; edit 5 is the second line. Edits 1-3 are exactly the
"smallest root-cause correction" prescribed by full-feature finding M-1
(review lines 523-528, mirroring row 1089's enumeration); edits 4-5 are the
necessary label rename of the same arm, whose old name asserted the guard
the correction removed. No Route, Goal, receipt, snapshot, cleanup,
retention, or lifecycle token elsewhere in either line changed; no other
line changed.

## Method

- Personally read byte-complete: the amendment brief (122 lines), the
  full-feature final review (all 670 lines, including finding M-1 at
  485-531), and `scenario-matrix.md` regions 183-252 (RouteModel domains
  including the Terminal-branch-state vocabulary at 210-211), 317-391
  (aggregate ladder, winner order, temporal composition, winner-to-event
  closure), 655-750 (GoalModel inputs including `bound_before_route` at 662
  and 736-738), 761-768 (TransitionModel domains), 1055-1129 (descendant
  partitions at 1059-1064 including `A_terminal` at 1063, rows 1074-1093
  including the amended 1088 and completion 1089, queue-wait witness
  1095-1105, cause partition 1110-1126), 1178-1202 and 1225-1243
  (root-scoped descendant disposition; completion/bound-stop witness),
  1290-1339 (Route effects 1293-1324 and the terminal bridge 1330-1336),
  3110-3169 (independent bound domain, `exact_hierarchical_campaign_cut` at
  3118, `total_time` role at 3162), 3240-3264 (multi-reach composition and
  linearization), 3630-3652 (Goal aggregation clauses), 3790-3834 (fixture
  families including the race family 3810-3823).
- Delegated byte-complete ranges: three read-only delegate readers covered
  `scenario-matrix.md` lines 1-1300, 1301-2600, and 2601-3899 (jointly the
  entire file), hunting for any text conflicting with the amended arm.
  Delegates mutated nothing (the post-fingerprint proves it). Both
  delegate-flagged tension candidates were personally re-read and
  adjudicated at their exact lines (dispositions under Findings and
  Reproduction 6); no delegate conclusion was accepted unverified.
- Structural checks executed as read-only stdin scripts over fresh bytes; no
  temporary file written inside the checkout; results under Reproduction 5.
- The one permitted output file was written through the shell because the
  user-level new-document gate blocks the Write tool for new markdown; the
  owner's task instruction and the brief pre-authorize exactly this path.
- Read-only `git rev-parse`, `git status`, `git diff`, `git show`,
  `git ls-files --eol`, and `sha256sum` only. No build, test, daemon,
  checker, generator, index, or store was executed or mutated. No
  environment value or secret material was read, printed, hashed, or
  persisted; all evidence is names and locations.

## Numbered Dispositions: Mandatory Reproductions

1. **Byte-for-byte compare `77e2fd0..WORKTREE`: VERIFIED.** The complete
   token-level enumeration above is exhaustive: five token edits across
   exactly two lines of one file (+74 bytes, line count unchanged at 3899),
   `git diff --numstat` confirms `2 2` with no other tracked change, and
   `git status` shows no other tracked modification. The only semantic
   change is the M-1 correction (edits 1-3); the only second edit is the
   fixture-label rename (edit 5, with edit 4 the same rename at the arm's
   defining row). Edit 3 is proved semantically neutral: it changes
   `already retracted branches remain unchanged` to the universally
   quantified form already used verbatim by the zero-Route arm in the same
   row; both forms dispose every already-`retracted` member identically as
   covered-and-unchanged, and no checker token keys on the old phrasing
   (zero other occurrences of it file-wide).

2. **Original reachable counterexample: VERIFIED CLOSED.** State: campaign
   `running`, zero open branches, one `ready` branch holding the current
   terminal Route (a `ready` Route is a current terminal-route record per
   matrix:658 with terminal state domain matrix:210), one `cancelled`
   branch, one already-`retracted` member. The stop is mandatory: the
   total-time receipt selected by `exact_hierarchical_campaign_cut` wins at
   the sampled cut (matrix:3254-3264 "at the same cut the deadline wins")
   and completion is barred (matrix:1089 requires `total_time=within`;
   a reached receipt at that cut selects `campaign_bound_stop`). Under the
   amended row 1088 the Route-bearing zero-open arm's guard now holds
   (`running`, zero open, nonempty ownership containing no open branch, at
   least one current terminal Route), and every owned member has exactly
   one legal disposition: `ready -> retracted` (enumerated),
   `cancelled -> retracted` (enumerated), already-`retracted` remains
   unchanged but covered (explicit clause); the descendant column's "exact
   complete ownership set; no branch omitted" is satisfiable; the proof
   member retires the `ready` branch's current proof (`current ->
   retired`); the Route set and exhaustive terminal Goal are retained
   unchanged (row 1088 effect column, consistent with Route effect
   `retain_final_snapshot` for "every existing ready/terminal Route" at
   matrix:1312-1315 and the completion/bound-stop witness at
   matrix:1235-1243). Under the base bytes the same state stranded the
   `ready` member (matrix:1063 vocabulary) or the `cancelled` member
   (matrix:210 vocabulary); the amendment eliminates both stranded readings
   by enumerating the exact owned-state set, independent of either
   "terminal" vocabulary.

3. **Campaign-stop arm totality and pairwise disjointness: VERIFIED.** The
   four arms of row 1088 partition the reachable guard space:
   (O) `running` with at least one owned open branch; (R) `running`, zero
   open, nonempty ownership, at least one current terminal Route;
   (Z) `accepted` or `running`, zero open, zero Routes (its `running` case
   enumerates the same owned-state set; its `accepted` case is the
   accepted-empty facet); (AE) `accepted` only in the exact
   zero-branch/zero-Route/zero-live-resource queue-gated case, which is
   Z's accepted facet, with any accepted work member rejecting the purpose.
   Disjointness: O vs R/Z by open-branch count; R vs Z by Route count
   (at-least-one vs zero, over the same current-terminal-route-record
   notion of matrix:658); Z's exclusivity clause bars cancelled-only or
   already-retracted-bearing zero-Route ownership from entering R; R from
   `accepted` is impossible because a Route is a work member and an
   accepted row with any work member rejects. Totality: "open branch" is
   pinned to the six nonterminal frontier states (matrix:391, matrix:660;
   `ready` cannot be open because `ready -> truncated` is not an accepted
   transition, matrix:1078 and bridge matrix:1336, where `discovered` is
   accepted only inside `campaign_bound_stop`); a current Route with empty
   ownership is unrepresentable (Routes are keyed by owned branch identity,
   matrix:658, and retraction is historical, not deletion), so every
   reachable zero-open state falls in exactly one of R (Route present) or
   Z (zero Routes, including empty, cancelled-only, and
   retracted-containing ownership). The scheduler-completion path
   (matrix:1089) is a distinct purpose disjoint by receipt: completion
   requires `exact_scheduler_completion` plus a same-revision cut with
   `total_time=within`, while any reached receipt at that cut or an
   earlier still-current revision selects `campaign_bound_stop` instead.

4. **Six deadline/completion orderings: VERIFIED, no out-of-scope effect
   change.** (i) Completion committed before the sampled cut wins
   (matrix:1089, matrix:3263). (ii) Same-cut deadline wins and selects
   `campaign_bound_stop` (matrix:1089, matrix:3263). (iii) Deadline after
   the last terminalization but before completion commit uses the
   Route-bearing zero-open arm, consumes the bound receipt, retains exact
   terminal Routes and Goal unchanged, snapshots, cleans up, and commits
   exactly one `running -> completed` (matrix:3810-3815, now naming the
   amended arm label at 3813). (iv) A receipt after completion is stale
   and rejects (matrix:1089, matrix:3815-3816, matrix:3263-3264).
   (v) Duplicate/replay fails the single campaign-decisive receipt
   correlation and duplicate-finalization rejection (matrix:1121) and the
   root lifecycle member. (vi) Crash/restart resolves through
   `engine_loss` (matrix:1086) and startup-recoverable wait records
   (matrix:1095-1105). The word-diff proves no Route, Goal, receipt,
   snapshot, cleanup, retention, or lifecycle effect token changed
   anywhere: every changed token is inside the ownership guard, the member
   enumeration, the already-retracted phrasing, or the arm label. Rows
   1089-1093, the Route-effects paragraph, the linearization rules, and
   the race family (other than the label at 3813) are byte-identical to
   the GREEN-reviewed base.

5. **Structural and token gates: VERIFIED, all green.** Escaped-pipe-aware
   GFM table-shape gate over fresh amended bytes: 54 tables, 694 body
   rows, 0 defects (identical to the full-feature gate; the amended row's
   cell structure is unchanged and the enumerated set contains no
   unescaped pipe). Declared-domain/reference gates: FR-001..FR-103 and
   SC-001..SC-021 remain contiguous and unique in `spec.md` (unchanged
   bytes, hash-proved) and the matrix contains zero dangling FR/SC
   references. Local link check: zero broken relative links. Content
   gates: 100 percent ASCII, LF-only, zero code fences, file ends in a
   complete sentence at line 3899. Negative-token canaries all zero:
   `preterminal`, `lease-eligible`, bare `EvidenceModel`, `catalog`
   misspelling, `queue_or_gate`, `snapshot_name_omitted`,
   `execution_name_rejected`, and every placeholder canary
   (TODO/TBD/FIXME/XXX/NEEDS CLARIFICATION/WIP/HACK/???). Counter tokens
   remain distinct and at prior counts
   (`exact_surface_name_scope_limit_and_counter` x4,
   `exact_scope_limit_and_counter` x5). Amendment-specific checks: the
   token `all-terminal` (and the phrase "all terminal") now has ZERO
   occurrences file-wide, so no normative `all-terminal` token remains
   for this arm; `Route-bearing` and `zero-open` each occur exactly twice,
   at 1088 and 3813, in the identical phrase `Route-bearing zero-open` --
   fully consistent; the remaining "zero open branches" prose (662, 1088
   twice) is guard prose, not the arm label, and is used consistently.

6. **Refutation attempts: ALL REFUTED.** (a) Mixed branch sets (open plus
   terminal plus cancelled plus retracted): arm O applies; each open
   branch commits `truncated/bound_reached` and "every owned branch ends
   `retracted`" disposes every non-open member, trivially including
   already-`retracted`; Route effects are total per branch
   (matrix:1312-1315: install-and-retain for newly truncated, retain for
   existing ready/terminal Routes, none for already-historical).
   (b) Zero ownership: zero branches implies zero Routes (matrix:658), so
   arm Z applies with a vacuous enumeration and evidence
   `bound_before_route`; whole-campaign coverage over an empty set is
   exact-complete (matrix:1064-1067). (c) A Route on an open branch is
   structurally unrepresentable (Route records carry exact terminal state,
   matrix:658, produced only through the terminal bridge matrix:1330-1336);
   any open branch routes the state to arm O regardless. (d) Multiple
   terminal Routes: R retains "the exact nonempty existing terminal Route
   set" as a set; every member enters the final snapshot; no per-Route
   choice exists. (e) Cancelled-only zero-Route state: Z is exclusive
   regardless of cancelled ownership, derives Goal `unknown` from
   `bound_before_route`, and cannot enter R (row 1088; fixtures
   3816-3820). (f) Already-retracted descendants: covered-and-unchanged in
   R and Z by explicit clause, trivially "end retracted" in O; the
   descendant column forbids omission. (g) Accepted state with work: the
   row rejects the purpose ("an accepted row with any work member rejects
   this purpose"), matching the accepted-arm guard at matrix:856-857.
   (h) Conflicting deadline/completion receipts: resolved by the closed
   ordering at matrix:1089 and matrix:3254-3264 (completion-before-cut
   wins; same-cut deadline wins; stale-after-competing-revision rejects;
   duplicates fail the one-campaign-decisive-receipt correlation,
   matrix:1121). No missing arm, overlapping arm, omitted member,
   synthetic Route (Z forbids creation; matrix:662 "never manufactures a
   Route"), altered outcome, or ambiguous state vocabulary survives: the
   amended arm enumerates its exact owned-state set and no longer keys any
   disposition on the conflicting "terminal" vocabularies.

7. **GREEN bytes preserved; no completion claim introduced: VERIFIED.**
   The three normative milestone files are blob-identical at `77e2fd0` to
   the full-feature GREEN manifest: `spec.md` `d081e0b9...`,
   `scenario-matrix.md` `633be34a...`, `checklists/requirements.md`
   `7a9c19de...`. The two `.specify` files' worktree bytes hash exactly to
   the manifest (`feature.json` `c6e6d35d...`, `constitution.md`
   `594e4e91...`) and `git status` reports them clean; their committed
   blobs differ from those digests only by `text=auto` LF normalization
   (`git ls-files --eol`: `i/lf w/mixed`), an end-of-line storage artifact
   with zero content difference (low observation 3 below). The
   amendment's five token edits contain no claim -- and create no
   implication -- that implementation, checker, generator, fixture corpus,
   FR-095 baselines, FR-100 measured token baseline, FR-050 comparator
   corpus, platform live clock gates, or AAP consumer work is complete.
   Every absence recorded by the full-feature review remains an absence
   (see Absence Confirmation).

## Findings

No critical findings. No high findings. No medium findings.

Three low editorial observations, each proved unable to change conformance
and therefore nonblocking:

1. **[low] The dual "terminal" vocabularies persist file-wide
   (pre-existing; the amendment removes this arm's dependence on them, not
   the vocabularies themselves).**
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:210`
     (Terminal branch state includes `ready`, excludes `cancelled`) versus
     `:1063` (`A_terminal` includes `cancelled`, excludes `ready`); residual
     phrases "current terminal Route" (`:1088`) and "ready/terminal Route"
     (`:1314`).
   - Counterexample: none reachable. Every member disposition in the
     amended arm is enumerated by exact state set, so both vocabularies
     now produce identical dispositions for every owned member; the
     residual "terminal Route" phrases resolve uniquely through the
     RouteModel record definition (`:658` with domain `:210`), under which
     a `ready` Route is a current terminal-route record -- the reading the
     Route-effects paragraph (`:1312-1315`) states explicitly.
   - Consequence: none for conformance; at worst a future editor must
     consult `:658`/`:210` to parse "terminal Route".
   - Smallest correction (optional, out of this amendment's scope): rename
     the `:210` axis label (for example "Terminal-evidence branch state")
     in a future editorial amendment; no normative row requires it.
   - Verification boundary: FR-096 checker totality over row 1088's arms;
     the race fixture family (`:3810-3823`).

2. **[low] Branch-retraction versus Route-retention two-level semantics is
   stated across two locations rather than one.**
   - Evidence: amended `:1088` retracts every owned branch in the
     enumerated set while retaining the Route set and Goal unchanged;
     `:1312-1315` assigns `retain_final_snapshot` to "every existing
     ready/terminal Route"; `:1241-1242` states the snapshot is historical
     only and "`retired` proof and `retracted` branches can never
     authorize a later dispatch".
   - Counterexample: none. The three texts compose to exactly one reading:
     the backing branch record is retracted, its Route record is retained
     historically in the final snapshot, and nothing retained can
     authorize dispatch. A `cancelled` branch owns no current Route (its
     current Route was invalidated at cancellation, `:1085`,
     `:1308-1311`), so its absence from the Route-retention phrase is
     correct, not an exclusion from retraction.
   - Consequence: none; a conforming implementation and checker derive the
     same member effects from any of the three texts.
   - Smallest correction (optional): none required; a cross-reference from
     `:1088` to the Route-effects paragraph would be cosmetic.
   - Verification boundary: SC-017 transition fixtures; the completion/
     bound-stop witness (`:1235-1243`).

3. **[low] `.specify` committed blobs are LF-normalized relative to the
   reviewed worktree bytes.**
   - Evidence: `git ls-files --eol` reports `i/lf w/mixed attr/text=auto`
     for `.specify/feature.json` and `.specify/memory/constitution.md`;
     worktree hashes equal the GREEN manifest digests exactly while blob
     hashes differ (`778dcdb0...`, `1f9c19f8...`).
   - Counterexample: none. `git status` proves content equality under
     normalization; every normative character is identical; the three
     milestone spec files are byte-identical blob-and-worktree.
   - Consequence: none for conformance; a future byte-exact provenance
     check against blobs must normalize line endings for these two files
     or hash the worktree, as this review did.
   - Smallest correction (optional): none required for this gate.
   - Verification boundary: any future fingerprint gate over `.specify/`.

## Absence Confirmation

Explicitly confirmed: absence of implementation artifacts remains absence.
The amendment touches only two specification lines. There is still no
campaign/planner/sensor/goal-plan engine, no `allow_environment_*` cap, no
`Environment*` policy action, no FR-096 checker/generator/fixture corpus, no
FR-095 onboarding baselines, no FR-100 frozen tokenizer/measurement
artifacts, no FR-050 comparator corpus, no shipped generator registries, and
no AAP Firecracker/vsock consumer evidence in this repository. Nothing
absent is claimed implemented, measured, or verified by the amended bytes,
and this review executed no build, test, daemon, or checker.

## Gate Conclusion

**GREEN -- this amendment (`specs/003-environment-probe/scenario-matrix.md`
at `c319b92f2ed8f9193326032e88f4d7520905039b656872eabd83ceaac32ef56b`,
397,343 bytes, 3,899 lines) is SAFE TO COMMIT directly to `main` on top of
`77e2fd0`, and with it committed the complete governing specification
milestone is planning-safe.**

Basis: stable evidence (pre/post fingerprints identical); the tracked diff
is exactly the M-1 smallest root-cause correction plus its arm-label rename,
proved token-complete; M-1 is exactly closed (the previously stranded
reachable member classes now each have exactly one legal disposition and
the stop remains mandatory); the campaign-stop arms are total and pairwise
disjoint; all six deadline/completion orderings re-verify with no effect
change outside the intended branch enumeration; every structural, domain,
link, and negative-token gate is green on fresh bytes, including zero
`all-terminal` tokens file-wide and consistent `Route-bearing zero-open`
usage; every refutation attempt failed; no full-feature defect is reopened;
and no absent artifact is claimed present. Per the full-feature gate's
condition 3, once this amendment commit exists on `main`, `/speckit-plan`
and implementation planning may begin against the amended matrix
fingerprint `c319b92f...` together with the unchanged `spec.md`
`d081e0b9...` and `requirements.md` `7a9c19de...`.
